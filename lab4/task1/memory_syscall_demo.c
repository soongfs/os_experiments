#define _DEFAULT_SOURCE

#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

static void die_errno(const char *context) {
    fprintf(stderr, "%s: %s\n", context, strerror(errno));
    exit(EXIT_FAILURE);
}

static long checked_page_size(void) {
    long value = sysconf(_SC_PAGESIZE);
    if (value <= 0) {
        fprintf(stderr, "sysconf(_SC_PAGESIZE) failed\n");
        exit(EXIT_FAILURE);
    }
    return value;
}

static void disable_core_dumps(void) {
    struct rlimit limit = {0, 0};

    if (setrlimit(RLIMIT_CORE, &limit) != 0) {
        die_errno("setrlimit(RLIMIT_CORE)");
    }
}

static void print_mapping_line(const void *address) {
    FILE *maps = fopen("/proc/self/maps", "r");
    char line[512];
    uintptr_t target = (uintptr_t)address;

    if (maps == NULL) {
        die_errno("fopen(/proc/self/maps)");
    }

    while (fgets(line, sizeof(line), maps) != NULL) {
        unsigned long start;
        unsigned long end;
        char perms[5];

        if (sscanf(line, "%lx-%lx %4s", &start, &end, perms) != 3) {
            continue;
        }

        if (target >= (uintptr_t)start && target < (uintptr_t)end) {
            size_t length = strlen(line);

            if (length > 0 && line[length - 1] == '\n') {
                line[length - 1] = '\0';
            }

            printf("    /proc/self/maps: %s\n", line);
            fclose(maps);
            return;
        }
    }

    fclose(maps);
    printf("    /proc/self/maps: no entry found for %p\n", address);
}

static uint64_t fnv1a64(const unsigned char *data, size_t length) {
    uint64_t hash = UINT64_C(14695981039346656037);
    size_t i;

    for (i = 0; i < length; ++i) {
        hash ^= data[i];
        hash *= UINT64_C(1099511628211);
    }

    return hash;
}

static void checked_munmap(void *mapping, size_t length) {
    if (munmap(mapping, length) != 0) {
        die_errno("munmap");
    }
}

static void demo_sbrk(long page_size) {
    void *break_before = sbrk(0);
    void *heap_chunk;
    void *break_after_grow;
    void *break_after_restore;
    char *bytes;

    if (break_before == (void *)-1) {
        die_errno("sbrk(0)");
    }

    printf("[sbrk] initial_break=%p offset_in_page=%zu\n",
           break_before,
           (size_t)((uintptr_t)break_before % (uintptr_t)page_size));

    heap_chunk = sbrk((intptr_t)page_size);
    if (heap_chunk == (void *)-1) {
        die_errno("sbrk(+page)");
    }

    bytes = (char *)heap_chunk;
    memset(bytes, 0, (size_t)page_size);
    bytes[0] = 'L';
    bytes[page_size / 2] = 'A';
    bytes[page_size - 1] = 'B';

    break_after_grow = sbrk(0);
    if (break_after_grow == (void *)-1) {
        die_errno("sbrk(0) after grow");
    }

    printf("[sbrk] grew_by=%ld bytes old_break=%p new_break=%p sample=%c%c%c\n",
           page_size,
           heap_chunk,
           break_after_grow,
           bytes[0],
           bytes[page_size / 2],
           bytes[page_size - 1]);

    if (sbrk(-(intptr_t)page_size) == (void *)-1) {
        die_errno("sbrk(-page)");
    }

    break_after_restore = sbrk(0);
    if (break_after_restore == (void *)-1) {
        die_errno("sbrk(0) after restore");
    }

    printf("[sbrk] restored_break=%p\n", break_after_restore);
}

static void demo_rw_mapping(long page_size) {
    size_t length = (size_t)page_size * 2U;
    unsigned char *mapping = mmap(NULL,
                                  length,
                                  PROT_READ | PROT_WRITE,
                                  MAP_PRIVATE | MAP_ANONYMOUS,
                                  -1,
                                  0);
    const char *message = "Linux mmap read/write demo";
    uint64_t checksum;

    if (mapping == MAP_FAILED) {
        die_errno("mmap(PROT_READ|PROT_WRITE)");
    }

    memset(mapping, 0x5a, length);
    memcpy(mapping, message, strlen(message) + 1U);
    mapping[page_size] = 0xa5;
    mapping[length - 1U] = 0x3c;
    checksum = fnv1a64(mapping, length);

    printf("[mmap-rw] addr=%p length=%zu aligned=%s\n",
           mapping,
           length,
           (((uintptr_t)mapping % (uintptr_t)page_size) == 0U) ? "yes" : "no");
    print_mapping_line(mapping);
    printf("[mmap-rw] payload=\"%s\" checksum=0x%016" PRIx64
           " marker_bytes=[0x%02x,0x%02x]\n",
           (char *)mapping,
           checksum,
           mapping[page_size],
           mapping[length - 1U]);

    checked_munmap(mapping, length);
    printf("[munmap] released rw mapping\n");
}

static void demo_read_only_mapping(long page_size) {
    size_t length = (size_t)page_size;
    char *mapping = mmap(NULL,
                         length,
                         PROT_READ | PROT_WRITE,
                         MAP_PRIVATE | MAP_ANONYMOUS,
                         -1,
                         0);

    if (mapping == MAP_FAILED) {
        die_errno("mmap(read-only staging page)");
    }

    snprintf(mapping, length, "read-only page after mprotect");

    errno = 0;
    if (mprotect(mapping + 1, length, PROT_READ) != -1 || errno != EINVAL) {
        fprintf(stderr,
                "expected unaligned mprotect to fail with EINVAL, got errno=%d\n",
                errno);
        checked_munmap(mapping, length);
        exit(EXIT_FAILURE);
    }

    printf("[mprotect-ro] unaligned mprotect(addr+1, ...) rejected with EINVAL as expected\n");

    if (mprotect(mapping, length, PROT_READ) != 0) {
        die_errno("mprotect(PROT_READ)");
    }

    printf("[mprotect-ro] addr=%p length=%zu aligned=%s read_back=\"%s\"\n",
           mapping,
           length,
           (((uintptr_t)mapping % (uintptr_t)page_size) == 0U) ? "yes" : "no",
           mapping);
    print_mapping_line(mapping);

    checked_munmap(mapping, length);
    printf("[munmap] released read-only mapping\n");
}

static void demo_exec_mapping(long page_size) {
#if defined(__x86_64__)
    static const unsigned char executable_stub[] = {
        0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3
    };
    const char *arch_name = "x86_64";
#elif defined(__aarch64__)
    static const unsigned char executable_stub[] = {
        0x40, 0x05, 0x80, 0x52, 0xc0, 0x03, 0x5f, 0xd6
    };
    const char *arch_name = "aarch64";
#else
    printf("[mprotect-rx] executable mapping demo skipped on this architecture\n");
    (void)page_size;
    return;
#endif
    size_t length = (size_t)page_size;
    unsigned char *mapping = mmap(NULL,
                                  length,
                                  PROT_READ | PROT_WRITE,
                                  MAP_PRIVATE | MAP_ANONYMOUS,
                                  -1,
                                  0);
    int (*fn)(void);
    int result;

    if (mapping == MAP_FAILED) {
        die_errno("mmap(executable staging page)");
    }

    memcpy(mapping, executable_stub, sizeof(executable_stub));

    printf("[mprotect-rx] staged %s stub at %p before mprotect\n", arch_name, mapping);
    print_mapping_line(mapping);

    if (mprotect(mapping, length, PROT_READ | PROT_EXEC) != 0) {
        die_errno("mprotect(PROT_READ|PROT_EXEC)");
    }

    printf("[mprotect-rx] changed page to PROT_READ|PROT_EXEC\n");
    print_mapping_line(mapping);

    fn = (int (*)(void))mapping;
    result = fn();
    printf("[mprotect-rx] executed stub result=%d\n", result);

    checked_munmap(mapping, length);
    printf("[munmap] released rx mapping\n");
}

static void run_direct_sigsegv_mode(long page_size) {
    size_t length = (size_t)page_size;
    volatile unsigned char *mapping = mmap(NULL,
                                           length,
                                           PROT_READ | PROT_WRITE,
                                           MAP_PRIVATE | MAP_ANONYMOUS,
                                           -1,
                                           0);

    if ((void *)mapping == MAP_FAILED) {
        die_errno("mmap(segfault page)");
    }

    ((unsigned char *)mapping)[0] = 'O';
    ((unsigned char *)mapping)[1] = 'K';

    if (mprotect((void *)mapping, length, PROT_READ) != 0) {
        die_errno("mprotect(PROT_READ) in segfault mode");
    }

    printf("[segfault] read-only mapping prepared at %p length=%zu\n", (const void *)mapping, length);
    print_mapping_line((const void *)mapping);
    printf("[segfault] about to write into a PROT_READ page; Linux should raise SIGSEGV\n");
    fflush(stdout);

    mapping[0] = 'X';
    fprintf(stderr, "unexpectedly wrote to read-only mapping\n");
    exit(EXIT_FAILURE);
}

static void run_sigsegv_probe(void) {
    pid_t child = fork();
    int status = 0;

    if (child < 0) {
        die_errno("fork");
    }

    if (child == 0) {
        run_direct_sigsegv_mode(checked_page_size());
        _exit(EXIT_FAILURE);
    }

    if (waitpid(child, &status, 0) < 0) {
        die_errno("waitpid");
    }

    if (WIFSIGNALED(status)) {
        printf("[probe] child write-to-PROT_READ terminated by signal=%d (%s)\n",
               WTERMSIG(status),
               (WTERMSIG(status) == SIGSEGV) ? "SIGSEGV" : "unexpected");
        return;
    }

    if (WIFEXITED(status)) {
        printf("[probe] child exited unexpectedly with status=%d\n", WEXITSTATUS(status));
        exit(EXIT_FAILURE);
    }

    fprintf(stderr, "[probe] child ended in an unexpected state\n");
    exit(EXIT_FAILURE);
}

static void print_usage(const char *program_name) {
    printf("Usage:\n");
    printf("  %s               # run the full demo\n", program_name);
    printf("  %s segfault      # intentionally write to a PROT_READ page and crash\n", program_name);
}

int main(int argc, char **argv) {
    long page_size;

    setvbuf(stdout, NULL, _IONBF, 0);
    disable_core_dumps();
    page_size = checked_page_size();

    if (argc > 2) {
        print_usage(argv[0]);
        return EXIT_FAILURE;
    }

    if (argc == 2) {
        if (strcmp(argv[1], "segfault") == 0) {
            run_direct_sigsegv_mode(page_size);
            return EXIT_FAILURE;
        }

        if (strcmp(argv[1], "--help") == 0 || strcmp(argv[1], "-h") == 0) {
            print_usage(argv[0]);
            return EXIT_SUCCESS;
        }

        print_usage(argv[0]);
        return EXIT_FAILURE;
    }

    printf("[info] host_page_size=%ld bytes\n", page_size);
    demo_sbrk(page_size);
    demo_rw_mapping(page_size);
    demo_exec_mapping(page_size);
    demo_read_only_mapping(page_size);
    run_sigsegv_probe();
    printf("[done] memory syscall demo completed successfully\n");

    return EXIT_SUCCESS;
}
