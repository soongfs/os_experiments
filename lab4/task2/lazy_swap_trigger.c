#define _DEFAULT_SOURCE

#include <errno.h>
#include <inttypes.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>

#define KIB 1024UL
#define MIB (1024UL * 1024UL)

struct proc_status {
    long vm_size_kb;
    long vm_rss_kb;
    long vm_swap_kb;
};

struct usage_snapshot {
    long minflt;
    long majflt;
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

static unsigned long parse_positive_mib(const char *text, const char *flag_name) {
    char *end = NULL;
    unsigned long value = strtoul(text, &end, 10);

    if (end == text || *end != '\0' || value == 0UL) {
        fprintf(stderr, "invalid %s: %s\n", flag_name, text);
        exit(EXIT_FAILURE);
    }

    return value;
}

static void read_proc_status(struct proc_status *status) {
    FILE *file = fopen("/proc/self/status", "r");
    char line[256];

    if (file == NULL) {
        die_errno("fopen(/proc/self/status)");
    }

    status->vm_size_kb = -1;
    status->vm_rss_kb = -1;
    status->vm_swap_kb = -1;

    while (fgets(line, sizeof(line), file) != NULL) {
        long value = 0;

        if (sscanf(line, "VmSize: %ld kB", &value) == 1) {
            status->vm_size_kb = value;
        } else if (sscanf(line, "VmRSS: %ld kB", &value) == 1) {
            status->vm_rss_kb = value;
        } else if (sscanf(line, "VmSwap: %ld kB", &value) == 1) {
            status->vm_swap_kb = value;
        }
    }

    fclose(file);
}

static void read_usage_snapshot(struct usage_snapshot *snapshot) {
    struct rusage usage;

    if (getrusage(RUSAGE_SELF, &usage) != 0) {
        die_errno("getrusage");
    }

    snapshot->minflt = usage.ru_minflt;
    snapshot->majflt = usage.ru_majflt;
}

static uint64_t monotonic_ns(void) {
    struct timespec ts;

    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        die_errno("clock_gettime(CLOCK_MONOTONIC)");
    }

    return (uint64_t)ts.tv_sec * UINT64_C(1000000000) + (uint64_t)ts.tv_nsec;
}

static size_t count_resident_pages(void *mapping, size_t length, size_t page_count) {
    unsigned char *vec = malloc(page_count);
    size_t resident = 0;
    size_t index;

    if (vec == NULL) {
        die_errno("malloc(mincore vector)");
    }

    if (mincore(mapping, length, vec) != 0) {
        free(vec);
        die_errno("mincore");
    }

    for (index = 0; index < page_count; ++index) {
        if ((vec[index] & 1U) != 0U) {
            resident++;
        }
    }

    free(vec);
    return resident;
}

static uint64_t touch_range(unsigned char *mapping,
                            size_t start_page,
                            size_t end_page,
                            size_t page_size,
                            unsigned int seed,
                            uint64_t checksum) {
    size_t page_index;

    for (page_index = start_page; page_index < end_page; ++page_index) {
        size_t offset = page_index * page_size;
        unsigned char value = mapping[offset];

        value = (unsigned char)(value + (unsigned char)(seed + (page_index * 17U)));
        mapping[offset] = value;
        checksum = (checksum * UINT64_C(11400714819323198485)) ^ (uint64_t)value;
    }

    return checksum;
}

static void print_snapshot(const char *label,
                           const char *stage,
                           unsigned long touched_mib,
                           size_t total_pages,
                           size_t resident_pages,
                           uint64_t checksum,
                           uint64_t elapsed_ns,
                           const struct usage_snapshot *baseline_usage) {
    struct usage_snapshot current_usage;
    struct proc_status status;
    long minflt_delta;
    long majflt_delta;
    double resident_percent = 0.0;

    read_usage_snapshot(&current_usage);
    read_proc_status(&status);

    minflt_delta = current_usage.minflt - baseline_usage->minflt;
    majflt_delta = current_usage.majflt - baseline_usage->majflt;
    if (total_pages != 0U) {
        resident_percent = (100.0 * (double)resident_pages) / (double)total_pages;
    }

    printf("[snapshot] label=%s stage=%s touched=%luMiB elapsed_ms=%.2f "
           "minflt_total=%ld majflt_total=%ld resident_pages=%zu/%zu (%.2f%%) "
           "VmSize=%ldkB VmRSS=%ldkB VmSwap=%ldkB checksum=0x%016" PRIx64 "\n",
           label,
           stage,
           touched_mib,
           (double)elapsed_ns / 1000000.0,
           minflt_delta,
           majflt_delta,
           resident_pages,
           total_pages,
           resident_percent,
           status.vm_size_kb,
           status.vm_rss_kb,
           status.vm_swap_kb,
           checksum);
}

static void print_usage(const char *program_name) {
    printf("Usage: %s --label <name> --working-set-mib <MiB> --step-mib <MiB> --revisit-passes <count>\n",
           program_name);
}

int main(int argc, char **argv) {
    const char *label = "default";
    unsigned long working_set_mib = 0UL;
    unsigned long step_mib = 0UL;
    unsigned long revisit_passes = 0UL;
    long page_size = checked_page_size();
    size_t total_bytes;
    size_t total_pages;
    size_t step_pages;
    unsigned char *mapping;
    struct usage_snapshot baseline_usage;
    uint64_t checksum = UINT64_C(0xcbf29ce484222325);
    uint64_t experiment_start_ns;
    int argi;

    setvbuf(stdout, NULL, _IONBF, 0);

    for (argi = 1; argi < argc; ++argi) {
        if (strcmp(argv[argi], "--label") == 0 && argi + 1 < argc) {
            label = argv[++argi];
        } else if (strcmp(argv[argi], "--working-set-mib") == 0 && argi + 1 < argc) {
            working_set_mib = parse_positive_mib(argv[++argi], "--working-set-mib");
        } else if (strcmp(argv[argi], "--step-mib") == 0 && argi + 1 < argc) {
            step_mib = parse_positive_mib(argv[++argi], "--step-mib");
        } else if (strcmp(argv[argi], "--revisit-passes") == 0 && argi + 1 < argc) {
            revisit_passes = parse_positive_mib(argv[++argi], "--revisit-passes");
        } else if (strcmp(argv[argi], "--help") == 0 || strcmp(argv[argi], "-h") == 0) {
            print_usage(argv[0]);
            return EXIT_SUCCESS;
        } else {
            print_usage(argv[0]);
            return EXIT_FAILURE;
        }
    }

    if (working_set_mib == 0UL || step_mib == 0UL) {
        print_usage(argv[0]);
        return EXIT_FAILURE;
    }

    if (working_set_mib > (SIZE_MAX / MIB)) {
        die_message("working set is too large for this build");
    }

    total_bytes = (size_t)(working_set_mib * MIB);
    total_pages = total_bytes / (size_t)page_size;
    if (total_pages == 0U) {
        die_message("working set is smaller than one page");
    }

    step_pages = ((size_t)step_mib * MIB) / (size_t)page_size;
    if (step_pages == 0U) {
        die_message("step size must be at least one page");
    }

    mapping = mmap(NULL,
                   total_bytes,
                   PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE,
                   -1,
                   0);
    if (mapping == MAP_FAILED) {
        die_errno("mmap");
    }

    if (madvise(mapping, total_bytes, MADV_NOHUGEPAGE) != 0) {
        die_errno("madvise(MADV_NOHUGEPAGE)");
    }

    read_usage_snapshot(&baseline_usage);
    experiment_start_ns = monotonic_ns();

    printf("[info] label=%s page_size=%ldB mapping=%p working_set=%luMiB total_pages=%zu step=%luMiB revisit_passes=%lu\n",
           label,
           page_size,
           (void *)mapping,
           working_set_mib,
           total_pages,
           step_mib,
           revisit_passes);

    {
        size_t start_page = 0U;
        size_t chunk_index = 0U;

        while (start_page < total_pages) {
            size_t end_page = start_page + step_pages;
            size_t resident_pages;
            uint64_t elapsed_ns;
            char stage[64];
            unsigned long touched_mib;

            if (end_page > total_pages) {
                end_page = total_pages;
            }

            checksum = touch_range(mapping,
                                   start_page,
                                   end_page,
                                   (size_t)page_size,
                                   (unsigned int)(chunk_index + 1U),
                                   checksum);

            resident_pages = count_resident_pages(mapping, total_bytes, total_pages);
            elapsed_ns = monotonic_ns() - experiment_start_ns;
            touched_mib = (unsigned long)(((uint64_t)end_page * (uint64_t)page_size) / MIB);
            snprintf(stage, sizeof(stage), "grow#%zu", chunk_index + 1U);
            print_snapshot(label,
                           stage,
                           touched_mib,
                           total_pages,
                           resident_pages,
                           checksum,
                           elapsed_ns,
                           &baseline_usage);

            start_page = end_page;
            chunk_index++;
        }
    }

    {
        unsigned long pass_index;

        for (pass_index = 0UL; pass_index < revisit_passes; ++pass_index) {
            size_t resident_pages;
            uint64_t elapsed_ns;
            char stage[64];

            checksum = touch_range(mapping,
                                   0U,
                                   total_pages,
                                   (size_t)page_size,
                                   (unsigned int)(pass_index + 97U),
                                   checksum);
            resident_pages = count_resident_pages(mapping, total_bytes, total_pages);
            elapsed_ns = monotonic_ns() - experiment_start_ns;
            snprintf(stage, sizeof(stage), "revisit#%lu", pass_index + 1UL);
            print_snapshot(label,
                           stage,
                           working_set_mib,
                           total_pages,
                           resident_pages,
                           checksum,
                           elapsed_ns,
                           &baseline_usage);
        }
    }

    if (munmap(mapping, total_bytes) != 0) {
        die_errno("munmap");
    }

    printf("[done] label=%s completed working_set=%luMiB checksum=0x%016" PRIx64 "\n",
           label,
           working_set_mib,
           checksum);
    return EXIT_SUCCESS;
}
