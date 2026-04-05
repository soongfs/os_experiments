#define _DEFAULT_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#define PAGE_COUNT 2U
#define OWNER_LEN 16U
#define STAGE_INITIAL 1U
#define STAGE_CHILD_AFTER_WRITE 2U
#define STAGE_CHILD_FINAL 3U
#define CMD_CHILD_WRITE_PAGE0 1
#define CMD_CHILD_REPORT_FINAL 2

struct cow_slot {
    uint64_t value;
    char owner[OWNER_LEN];
};

struct usage_snapshot {
    long minflt;
    long majflt;
};

struct page_info {
    uint64_t pfn;
    uint64_t kpagecount;
    bool present;
};

struct child_report {
    uint32_t stage;
    uint64_t page0_value;
    uint64_t page1_value;
    char page0_owner[OWNER_LEN];
    char page1_owner[OWNER_LEN];
    long minflt_delta;
    long majflt_delta;
};

static void die_errno(const char *context) {
    fprintf(stderr, "%s: %s\n", context, strerror(errno));
    exit(EXIT_FAILURE);
}

static void die_message(const char *message) {
    fprintf(stderr, "%s\n", message);
    exit(EXIT_FAILURE);
}

static long checked_page_size(void) {
    long page_size = sysconf(_SC_PAGESIZE);

    if (page_size <= 0) {
        die_message("failed to query page size");
    }

    return page_size;
}

static void read_usage_snapshot(struct usage_snapshot *snapshot) {
    struct rusage usage;

    if (getrusage(RUSAGE_SELF, &usage) != 0) {
        die_errno("getrusage");
    }

    snapshot->minflt = usage.ru_minflt;
    snapshot->majflt = usage.ru_majflt;
}

static void warm_up_process(void) {
    volatile unsigned char scratch[16384];
    size_t index;
    struct usage_snapshot snapshot;

    for (index = 0; index < sizeof(scratch); ++index) {
        scratch[index] = (unsigned char)(index & 0xffU);
    }

    read_usage_snapshot(&snapshot);
    (void)snapshot;
}

static void write_full(int fd, const void *buffer, size_t length) {
    const unsigned char *cursor = buffer;

    while (length > 0U) {
        ssize_t written = write(fd, cursor, length);

        if (written < 0) {
            if (errno == EINTR) {
                continue;
            }
            die_errno("write");
        }

        if (written == 0) {
            die_message("short write with zero progress");
        }

        cursor += (size_t)written;
        length -= (size_t)written;
    }
}

static void read_full(int fd, void *buffer, size_t length) {
    unsigned char *cursor = buffer;

    while (length > 0U) {
        ssize_t read_bytes = read(fd, cursor, length);

        if (read_bytes < 0) {
            if (errno == EINTR) {
                continue;
            }
            die_errno("read");
        }

        if (read_bytes == 0) {
            die_message("unexpected EOF on pipe");
        }

        cursor += (size_t)read_bytes;
        length -= (size_t)read_bytes;
    }
}

static void read_page_info_for_pid(pid_t pid,
                                   const void *address,
                                   long page_size,
                                   struct page_info *info) {
    char pagemap_path[64];
    int pagemap_fd;
    int kpagecount_fd;
    uint64_t entry;
    off_t pagemap_offset;
    off_t count_offset;
    ssize_t got;

    snprintf(pagemap_path, sizeof(pagemap_path), "/proc/%ld/pagemap", (long)pid);
    pagemap_fd = open(pagemap_path, O_RDONLY);
    if (pagemap_fd < 0) {
        die_errno("open(pagemap)");
    }

    pagemap_offset = (off_t)(((uintptr_t)address / (uintptr_t)page_size) * sizeof(uint64_t));
    got = pread(pagemap_fd, &entry, sizeof(entry), pagemap_offset);
    close(pagemap_fd);
    if (got != (ssize_t)sizeof(entry)) {
        die_errno("pread(pagemap)");
    }

    info->present = ((entry >> 63U) & 1U) != 0U;
    info->pfn = entry & ((UINT64_C(1) << 55U) - 1U);
    info->kpagecount = 0U;

    if (!info->present || info->pfn == 0U) {
        return;
    }

    kpagecount_fd = open("/proc/kpagecount", O_RDONLY);
    if (kpagecount_fd < 0) {
        die_errno("open(/proc/kpagecount)");
    }

    count_offset = (off_t)(info->pfn * sizeof(uint64_t));
    got = pread(kpagecount_fd, &info->kpagecount, sizeof(info->kpagecount), count_offset);
    close(kpagecount_fd);
    if (got != (ssize_t)sizeof(info->kpagecount)) {
        die_errno("pread(/proc/kpagecount)");
    }
}

static void fill_report(struct child_report *report,
                        uint32_t stage,
                        const struct cow_slot *page0,
                        const struct cow_slot *page1,
                        long minflt_delta,
                        long majflt_delta) {
    memset(report, 0, sizeof(*report));
    report->stage = stage;
    report->page0_value = page0->value;
    report->page1_value = page1->value;
    memcpy(report->page0_owner, page0->owner, OWNER_LEN);
    memcpy(report->page1_owner, page1->owner, OWNER_LEN);
    report->minflt_delta = minflt_delta;
    report->majflt_delta = majflt_delta;
}

static void print_page_line(const char *label,
                            const struct cow_slot *slot,
                            const struct page_info *self_info,
                            const struct page_info *peer_info) {
    printf("%s value=0x%016" PRIx64 " owner=%s self_pfn=0x%llx peer_pfn=0x%llx "
           "self_kpagecount=%llu peer_kpagecount=%llu\n",
           label,
           slot->value,
           slot->owner,
           (unsigned long long)self_info->pfn,
           (unsigned long long)peer_info->pfn,
           (unsigned long long)self_info->kpagecount,
           (unsigned long long)peer_info->kpagecount);
}

int main(void) {
    const long page_size = checked_page_size();
    const size_t mapping_length = (size_t)page_size * PAGE_COUNT;
    unsigned char *mapping;
    struct cow_slot *page0;
    struct cow_slot *page1;
    int parent_to_child[2];
    int child_to_parent[2];
    pid_t child_pid;
    struct page_info parent_page0_initial;
    struct page_info parent_page1_initial;
    struct page_info child_page0_initial;
    struct page_info child_page1_initial;
    struct page_info pre_fork_page0;
    struct page_info pre_fork_page1;
    struct child_report child_initial_report;
    struct child_report child_after_write_report;
    struct child_report child_final_report;
    struct usage_snapshot before_write;
    struct usage_snapshot after_write;
    struct page_info parent_page0_after_child;
    struct page_info parent_page1_after_child;
    struct page_info child_page0_after_child;
    struct page_info child_page1_after_child;
    struct page_info parent_page1_after_parent;
    struct page_info child_page1_after_parent;
    long parent_page1_minflt_delta;
    long parent_page1_majflt_delta;
    int child_status = 0;
    unsigned char command;
    bool accept_initial_same;
    bool accept_child_isolation;
    bool accept_parent_isolation;
    bool accept_child_cow_fault;
    bool accept_parent_cow_fault;

    setvbuf(stdout, NULL, _IONBF, 0);

    mapping = mmap(NULL,
                   mapping_length,
                   PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANONYMOUS,
                   -1,
                   0);
    if (mapping == MAP_FAILED) {
        die_errno("mmap");
    }

    if (madvise(mapping, mapping_length, MADV_NOHUGEPAGE) != 0) {
        die_errno("madvise(MADV_NOHUGEPAGE)");
    }

    page0 = (struct cow_slot *)(void *)(mapping + (size_t)page_size * 0U);
    page1 = (struct cow_slot *)(void *)(mapping + (size_t)page_size * 1U);

    memset(page0, 0, sizeof(*page0));
    memset(page1, 0, sizeof(*page1));
    page0->value = UINT64_C(0x1111111111111111);
    page1->value = UINT64_C(0x2222222222222222);
    memcpy(page0->owner, "seed_page0", 11U);
    memcpy(page1->owner, "seed_page1", 11U);

    read_page_info_for_pid(getpid(), page0, page_size, &pre_fork_page0);
    read_page_info_for_pid(getpid(), page1, page_size, &pre_fork_page1);

    printf("[info] page_size=%ld bytes mapping=%p\n", page_size, (void *)mapping);
    print_page_line("[pre-fork/page0]", page0, &pre_fork_page0, &pre_fork_page0);
    print_page_line("[pre-fork/page1]", page1, &pre_fork_page1, &pre_fork_page1);

    if (pipe(parent_to_child) != 0) {
        die_errno("pipe(parent_to_child)");
    }

    if (pipe(child_to_parent) != 0) {
        die_errno("pipe(child_to_parent)");
    }

    child_pid = fork();
    if (child_pid < 0) {
        die_errno("fork");
    }

    if (child_pid == 0) {
        struct child_report report;
        struct usage_snapshot child_before_write;
        struct usage_snapshot child_after_write;

        close(parent_to_child[1]);
        close(child_to_parent[0]);

        warm_up_process();

        fill_report(&report, STAGE_INITIAL, page0, page1, 0L, 0L);
        write_full(child_to_parent[1], &report, sizeof(report));

        read_full(parent_to_child[0], &command, sizeof(command));
        if (command != CMD_CHILD_WRITE_PAGE0) {
            _exit(EXIT_FAILURE);
        }

        read_usage_snapshot(&child_before_write);
        page0->value = UINT64_C(0xc0ffee0000000001);
        memcpy(page0->owner, "child_page0", 12U);
        read_usage_snapshot(&child_after_write);

        fill_report(&report,
                    STAGE_CHILD_AFTER_WRITE,
                    page0,
                    page1,
                    child_after_write.minflt - child_before_write.minflt,
                    child_after_write.majflt - child_before_write.majflt);
        write_full(child_to_parent[1], &report, sizeof(report));

        read_full(parent_to_child[0], &command, sizeof(command));
        if (command != CMD_CHILD_REPORT_FINAL) {
            _exit(EXIT_FAILURE);
        }

        fill_report(&report, STAGE_CHILD_FINAL, page0, page1, 0L, 0L);
        write_full(child_to_parent[1], &report, sizeof(report));

        close(parent_to_child[0]);
        close(child_to_parent[1]);
        _exit(EXIT_SUCCESS);
    }

    close(parent_to_child[0]);
    close(child_to_parent[1]);

    warm_up_process();

    read_full(child_to_parent[0], &child_initial_report, sizeof(child_initial_report));
    if (child_initial_report.stage != STAGE_INITIAL) {
        die_message("unexpected child stage for initial report");
    }

    read_page_info_for_pid(getpid(), page0, page_size, &parent_page0_initial);
    read_page_info_for_pid(getpid(), page1, page_size, &parent_page1_initial);
    read_page_info_for_pid(child_pid, page0, page_size, &child_page0_initial);
    read_page_info_for_pid(child_pid, page1, page_size, &child_page1_initial);

    accept_initial_same =
        (child_initial_report.page0_value == page0->value) &&
        (child_initial_report.page1_value == page1->value) &&
        (strcmp(child_initial_report.page0_owner, page0->owner) == 0) &&
        (strcmp(child_initial_report.page1_owner, page1->owner) == 0) &&
        (parent_page0_initial.pfn == child_page0_initial.pfn) &&
        (parent_page1_initial.pfn == child_page1_initial.pfn) &&
        (parent_page0_initial.kpagecount >= 2U) &&
        (parent_page1_initial.kpagecount >= 2U);

    printf("[info] child_pid=%ld\n", (long)child_pid);
    printf("[post-fork] parent and child initial views should match before any write\n");
    print_page_line("[post-fork/page0]", page0, &parent_page0_initial, &child_page0_initial);
    print_page_line("[post-fork/page1]", page1, &parent_page1_initial, &child_page1_initial);

    command = CMD_CHILD_WRITE_PAGE0;
    write_full(parent_to_child[1], &command, sizeof(command));

    read_full(child_to_parent[0], &child_after_write_report, sizeof(child_after_write_report));
    if (child_after_write_report.stage != STAGE_CHILD_AFTER_WRITE) {
        die_message("unexpected child stage after first write");
    }

    read_page_info_for_pid(getpid(), page0, page_size, &parent_page0_after_child);
    read_page_info_for_pid(getpid(), page1, page_size, &parent_page1_after_child);
    read_page_info_for_pid(child_pid, page0, page_size, &child_page0_after_child);
    read_page_info_for_pid(child_pid, page1, page_size, &child_page1_after_child);

    printf("[child-write/page0] child_minflt_delta=%ld child_majflt_delta=%ld\n",
           child_after_write_report.minflt_delta,
           child_after_write_report.majflt_delta);
    print_page_line("[child-write/page0/parent-view]", page0, &parent_page0_after_child, &child_page0_after_child);
    printf("[child-write/page0/child-view] value=0x%016" PRIx64 " owner=%s child_pfn=0x%llx child_kpagecount=%llu\n",
           child_after_write_report.page0_value,
           child_after_write_report.page0_owner,
           (unsigned long long)child_page0_after_child.pfn,
           (unsigned long long)child_page0_after_child.kpagecount);
    print_page_line("[child-write/page1/shared-still]", page1, &parent_page1_after_child, &child_page1_after_child);

    accept_child_isolation =
        (page0->value == UINT64_C(0x1111111111111111)) &&
        (strcmp(page0->owner, "seed_page0") == 0) &&
        (child_after_write_report.page0_value == UINT64_C(0xc0ffee0000000001)) &&
        (strcmp(child_after_write_report.page0_owner, "child_page0") == 0) &&
        (parent_page0_after_child.pfn != child_page0_after_child.pfn) &&
        (parent_page1_after_child.pfn == child_page1_after_child.pfn);

    accept_child_cow_fault =
        (child_after_write_report.minflt_delta > 0L) &&
        (child_after_write_report.majflt_delta == 0L) &&
        (parent_page0_initial.pfn == child_page0_initial.pfn) &&
        (child_page0_after_child.pfn != child_page0_initial.pfn) &&
        (parent_page0_after_child.kpagecount >= 1U) &&
        (child_page0_after_child.kpagecount >= 1U);

    read_usage_snapshot(&before_write);
    page1->value = UINT64_C(0xa11ce00000000002);
    memcpy(page1->owner, "parent_page1", 13U);
    read_usage_snapshot(&after_write);

    parent_page1_minflt_delta = after_write.minflt - before_write.minflt;
    parent_page1_majflt_delta = after_write.majflt - before_write.majflt;

    read_page_info_for_pid(getpid(), page1, page_size, &parent_page1_after_parent);
    read_page_info_for_pid(child_pid, page1, page_size, &child_page1_after_parent);

    printf("[parent-write/page1] parent_minflt_delta=%ld parent_majflt_delta=%ld\n",
           parent_page1_minflt_delta,
           parent_page1_majflt_delta);
    print_page_line("[parent-write/page1/parent-view]", page1, &parent_page1_after_parent, &child_page1_after_parent);

    command = CMD_CHILD_REPORT_FINAL;
    write_full(parent_to_child[1], &command, sizeof(command));

    read_full(child_to_parent[0], &child_final_report, sizeof(child_final_report));
    if (child_final_report.stage != STAGE_CHILD_FINAL) {
        die_message("unexpected child stage for final report");
    }

    printf("[parent-write/page1/child-view] value=0x%016" PRIx64 " owner=%s child_pfn=0x%llx child_kpagecount=%llu\n",
           child_final_report.page1_value,
           child_final_report.page1_owner,
           (unsigned long long)child_page1_after_parent.pfn,
           (unsigned long long)child_page1_after_parent.kpagecount);
    printf("[final] parent_page0=0x%016" PRIx64 " (%s) parent_page1=0x%016" PRIx64 " (%s)\n",
           page0->value,
           page0->owner,
           page1->value,
           page1->owner);
    printf("[final] child_page0=0x%016" PRIx64 " (%s) child_page1=0x%016" PRIx64 " (%s)\n",
           child_final_report.page0_value,
           child_final_report.page0_owner,
           child_final_report.page1_value,
           child_final_report.page1_owner);

    accept_parent_isolation =
        (page1->value == UINT64_C(0xa11ce00000000002)) &&
        (strcmp(page1->owner, "parent_page1") == 0) &&
        (child_final_report.page1_value == UINT64_C(0x2222222222222222)) &&
        (strcmp(child_final_report.page1_owner, "seed_page1") == 0) &&
        (parent_page1_after_parent.pfn != child_page1_after_parent.pfn);

    accept_parent_cow_fault =
        (parent_page1_minflt_delta > 0L) &&
        (parent_page1_majflt_delta == 0L) &&
        (parent_page1_initial.pfn == child_page1_initial.pfn) &&
        (parent_page1_after_parent.pfn != child_page1_after_parent.pfn) &&
        (parent_page1_after_parent.pfn != parent_page1_initial.pfn);

    if (waitpid(child_pid, &child_status, 0) < 0) {
        die_errno("waitpid");
    }

    printf("[acceptance] fork initial values same: %s\n", accept_initial_same ? "PASS" : "FAIL");
    printf("[acceptance] child write isolates page0: %s\n", accept_child_isolation ? "PASS" : "FAIL");
    printf("[acceptance] parent write isolates page1: %s\n", accept_parent_isolation ? "PASS" : "FAIL");
    printf("[acceptance] child first write triggered COW minor fault: %s\n", accept_child_cow_fault ? "PASS" : "FAIL");
    printf("[acceptance] parent first write triggered COW minor fault: %s\n", accept_parent_cow_fault ? "PASS" : "FAIL");

    if (!WIFEXITED(child_status) || WEXITSTATUS(child_status) != 0 ||
        !accept_initial_same || !accept_child_isolation ||
        !accept_parent_isolation || !accept_child_cow_fault ||
        !accept_parent_cow_fault) {
        return EXIT_FAILURE;
    }

    return EXIT_SUCCESS;
}
