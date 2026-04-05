#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <inttypes.h>
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

enum {
    DEFAULT_MAX_WORKERS = 4,
    MAX_SUPPORTED_WORKERS = 16,
    DEFAULT_WORDS_PER_WORKER = 1 << 20, /* 8 MiB per worker */
    DEFAULT_ITERATIONS_PER_WORKER = 60000000ULL,
};

struct benchmark_config {
    int workers;
    size_t words_per_worker;
    uint64_t iterations_per_worker;
};

struct usage_snapshot {
    struct timeval user_time;
    struct timeval system_time;
    long max_rss_kb;
};

struct benchmark_result {
    const char *mode_name;
    int workers;
    int online_cpus;
    size_t bytes_per_worker;
    uint64_t iterations_per_worker;
    double wall_seconds;
    double user_seconds;
    double system_seconds;
    double cpu_util_percent;
    long rss_snapshot_kb;
    long peak_component_rss_kb;
    uint64_t checksum;
};

struct thread_worker {
    int worker_id;
    size_t words_per_worker;
    uint64_t iterations_per_worker;
    uint64_t *buffer;
    pthread_barrier_t *ready_barrier;
    pthread_barrier_t *start_barrier;
    uint64_t checksum;
};

struct process_slot {
    atomic_int ready;
    long rss_snapshot_kb;
    uint64_t checksum;
};

struct process_shared_state {
    atomic_int start_flag;
    struct process_slot slots[MAX_SUPPORTED_WORKERS];
};

static void die_message(const char *message) {
    fprintf(stderr, "%s\n", message);
    exit(EXIT_FAILURE);
}

static void die_errno(const char *context) {
    fprintf(stderr, "%s: %s\n", context, strerror(errno));
    exit(EXIT_FAILURE);
}

static void die_pthread(const char *context, int error_code) {
    fprintf(stderr, "%s: %s\n", context, strerror(error_code));
    exit(EXIT_FAILURE);
}

static uint64_t parse_u64(const char *text, const char *label) {
    char *end = NULL;
    unsigned long long value;

    errno = 0;
    value = strtoull(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0') {
        fprintf(stderr, "failed to parse %s from '%s'\n", label, text);
        exit(EXIT_FAILURE);
    }

    return (uint64_t)value;
}

static int parse_positive_int(const char *text, const char *label) {
    uint64_t value = parse_u64(text, label);

    if (value == 0U || value > INT32_MAX) {
        fprintf(stderr, "%s must be in [1, %d]\n", label, INT32_MAX);
        exit(EXIT_FAILURE);
    }

    return (int)value;
}

static int online_cpu_count(void) {
    long count = sysconf(_SC_NPROCESSORS_ONLN);

    if (count <= 0) {
        die_message("failed to query online CPU count");
    }

    if (count > MAX_SUPPORTED_WORKERS) {
        count = MAX_SUPPORTED_WORKERS;
    }

    return (int)count;
}

static void read_usage_snapshot(int who, struct usage_snapshot *snapshot) {
    struct rusage usage;

    if (getrusage(who, &usage) != 0) {
        die_errno("getrusage");
    }

    snapshot->user_time = usage.ru_utime;
    snapshot->system_time = usage.ru_stime;
    snapshot->max_rss_kb = usage.ru_maxrss;
}

static double timeval_diff_seconds(const struct timeval *before, const struct timeval *after) {
    double before_seconds = (double)before->tv_sec + (double)before->tv_usec / 1000000.0;
    double after_seconds = (double)after->tv_sec + (double)after->tv_usec / 1000000.0;

    return after_seconds - before_seconds;
}

static double timespec_diff_seconds(const struct timespec *before, const struct timespec *after) {
    double before_seconds = (double)before->tv_sec + (double)before->tv_nsec / 1000000000.0;
    double after_seconds = (double)after->tv_sec + (double)after->tv_nsec / 1000000000.0;

    return after_seconds - before_seconds;
}

static long read_self_vmrss_kb(void) {
    FILE *file = fopen("/proc/self/status", "r");
    char line[256];

    if (file == NULL) {
        die_errno("fopen(/proc/self/status)");
    }

    while (fgets(line, sizeof(line), file) != NULL) {
        long value;

        if (sscanf(line, "VmRSS: %ld kB", &value) == 1) {
            fclose(file);
            return value;
        }
    }

    fclose(file);
    die_message("VmRSS not found in /proc/self/status");
    return 0L;
}

static uint64_t rotl64(uint64_t value, unsigned shift) {
    return (value << shift) | (value >> (64U - shift));
}

static uint64_t splitmix64(uint64_t value) {
    value += UINT64_C(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30U)) * UINT64_C(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27U)) * UINT64_C(0x94d049bb133111eb);
    return value ^ (value >> 31U);
}

static void initialize_buffer(uint64_t *buffer, size_t words, uint64_t seed) {
    size_t index;

    for (index = 0U; index < words; ++index) {
        buffer[index] = splitmix64(seed + (uint64_t)index);
    }
}

static uint64_t compute_kernel(uint64_t *buffer,
                               size_t words,
                               uint64_t iterations,
                               uint64_t seed) {
    const size_t mask = words - 1U;
    uint64_t state = splitmix64(seed);
    uint64_t accumulator = seed ^ UINT64_C(0xd1b54a32d192ed03);
    uint64_t iteration;

    for (iteration = 0U; iteration < iterations; ++iteration) {
        size_t index_a;
        size_t index_b;

        state ^= state >> 12U;
        state ^= state << 25U;
        state ^= state >> 27U;
        state *= UINT64_C(2685821657736338717);

        index_a = (size_t)(state & (uint64_t)mask);
        index_b = (size_t)((state >> 17U) & (uint64_t)mask);

        buffer[index_a] ^= accumulator + iteration;
        accumulator ^= rotl64(buffer[index_b] + state + iteration, 17U);
        accumulator += buffer[index_a] ^ UINT64_C(0x9e3779b97f4a7c15);
    }

    return accumulator ^ state;
}

static uint64_t combine_checksum(uint64_t accumulator, uint64_t value) {
    return rotl64(accumulator ^ splitmix64(value + UINT64_C(0x123456789abcdef0)), 9U);
}

static uint64_t worker_seed(int worker_id) {
    return UINT64_C(0x6a09e667f3bcc909) ^ ((uint64_t)worker_id * UINT64_C(0x9e3779b97f4a7c15));
}

static void require_power_of_two(size_t words) {
    if (words == 0U || (words & (words - 1U)) != 0U) {
        die_message("words_per_worker must be a non-zero power of two");
    }
}

static void barrier_wait_checked(pthread_barrier_t *barrier, const char *label) {
    int error = pthread_barrier_wait(barrier);

    if (error != 0 && error != PTHREAD_BARRIER_SERIAL_THREAD) {
        die_pthread(label, error);
    }
}

static void *thread_worker_main(void *opaque) {
    struct thread_worker *worker = opaque;
    uint64_t seed = worker_seed(worker->worker_id);

    initialize_buffer(worker->buffer, worker->words_per_worker, seed);
    barrier_wait_checked(worker->ready_barrier, "pthread_barrier_wait(ready)");
    barrier_wait_checked(worker->start_barrier, "pthread_barrier_wait(start)");

    worker->checksum = compute_kernel(worker->buffer,
                                      worker->words_per_worker,
                                      worker->iterations_per_worker,
                                      seed ^ UINT64_C(0xfeedface));
    return NULL;
}

static void fill_common_result_fields(struct benchmark_result *result,
                                      const struct benchmark_config *config) {
    memset(result, 0, sizeof(*result));
    result->workers = config->workers;
    result->online_cpus = online_cpu_count();
    result->bytes_per_worker = config->words_per_worker * sizeof(uint64_t);
    result->iterations_per_worker = config->iterations_per_worker;
}

static void run_single_mode(const struct benchmark_config *config, struct benchmark_result *result) {
    uint64_t *buffer;
    struct usage_snapshot before_usage;
    struct usage_snapshot after_usage;
    struct timespec start_time;
    struct timespec end_time;
    uint64_t checksum = 0U;
    int worker;

    fill_common_result_fields(result, config);
    result->mode_name = "single";

    buffer = calloc((size_t)config->workers * config->words_per_worker, sizeof(*buffer));
    if (buffer == NULL) {
        die_errno("calloc(single buffer)");
    }

    for (worker = 0; worker < config->workers; ++worker) {
        uint64_t seed = worker_seed(worker);

        initialize_buffer(buffer + (size_t)worker * config->words_per_worker,
                          config->words_per_worker,
                          seed);
    }

    result->rss_snapshot_kb = read_self_vmrss_kb();
    read_usage_snapshot(RUSAGE_SELF, &before_usage);
    if (clock_gettime(CLOCK_MONOTONIC, &start_time) != 0) {
        die_errno("clock_gettime(start)");
    }

    for (worker = 0; worker < config->workers; ++worker) {
        uint64_t seed = worker_seed(worker);
        uint64_t worker_checksum = compute_kernel(buffer + (size_t)worker * config->words_per_worker,
                                                  config->words_per_worker,
                                                  config->iterations_per_worker,
                                                  seed ^ UINT64_C(0xfeedface));
        checksum = combine_checksum(checksum, worker_checksum);
    }

    if (clock_gettime(CLOCK_MONOTONIC, &end_time) != 0) {
        die_errno("clock_gettime(end)");
    }
    read_usage_snapshot(RUSAGE_SELF, &after_usage);

    result->wall_seconds = timespec_diff_seconds(&start_time, &end_time);
    result->user_seconds = timeval_diff_seconds(&before_usage.user_time, &after_usage.user_time);
    result->system_seconds = timeval_diff_seconds(&before_usage.system_time, &after_usage.system_time);
    result->cpu_util_percent =
        100.0 * (result->user_seconds + result->system_seconds) / result->wall_seconds;
    result->peak_component_rss_kb = after_usage.max_rss_kb;
    result->checksum = checksum;

    free(buffer);
}

static void run_threads_mode(const struct benchmark_config *config, struct benchmark_result *result) {
    uint64_t *buffer;
    pthread_t threads[MAX_SUPPORTED_WORKERS];
    struct thread_worker workers[MAX_SUPPORTED_WORKERS];
    pthread_barrier_t ready_barrier;
    pthread_barrier_t start_barrier;
    struct usage_snapshot before_usage;
    struct usage_snapshot after_usage;
    struct timespec start_time;
    struct timespec end_time;
    uint64_t checksum = 0U;
    int worker;
    int error;

    fill_common_result_fields(result, config);
    result->mode_name = "threads";

    buffer = calloc((size_t)config->workers * config->words_per_worker, sizeof(*buffer));
    if (buffer == NULL) {
        die_errno("calloc(thread buffer)");
    }

    error = pthread_barrier_init(&ready_barrier, NULL, (unsigned)config->workers + 1U);
    if (error != 0) {
        die_pthread("pthread_barrier_init(ready)", error);
    }
    error = pthread_barrier_init(&start_barrier, NULL, (unsigned)config->workers + 1U);
    if (error != 0) {
        die_pthread("pthread_barrier_init(start)", error);
    }

    for (worker = 0; worker < config->workers; ++worker) {
        memset(&workers[worker], 0, sizeof(workers[worker]));
        workers[worker].worker_id = worker;
        workers[worker].words_per_worker = config->words_per_worker;
        workers[worker].iterations_per_worker = config->iterations_per_worker;
        workers[worker].buffer = buffer + (size_t)worker * config->words_per_worker;
        workers[worker].ready_barrier = &ready_barrier;
        workers[worker].start_barrier = &start_barrier;

        error = pthread_create(&threads[worker], NULL, thread_worker_main, &workers[worker]);
        if (error != 0) {
            die_pthread("pthread_create", error);
        }
    }

    barrier_wait_checked(&ready_barrier, "pthread_barrier_wait(main ready)");
    result->rss_snapshot_kb = read_self_vmrss_kb();
    read_usage_snapshot(RUSAGE_SELF, &before_usage);
    if (clock_gettime(CLOCK_MONOTONIC, &start_time) != 0) {
        die_errno("clock_gettime(start)");
    }
    barrier_wait_checked(&start_barrier, "pthread_barrier_wait(main start)");

    for (worker = 0; worker < config->workers; ++worker) {
        error = pthread_join(threads[worker], NULL);
        if (error != 0) {
            die_pthread("pthread_join", error);
        }
        checksum = combine_checksum(checksum, workers[worker].checksum);
    }

    if (clock_gettime(CLOCK_MONOTONIC, &end_time) != 0) {
        die_errno("clock_gettime(end)");
    }
    read_usage_snapshot(RUSAGE_SELF, &after_usage);

    pthread_barrier_destroy(&ready_barrier);
    pthread_barrier_destroy(&start_barrier);

    result->wall_seconds = timespec_diff_seconds(&start_time, &end_time);
    result->user_seconds = timeval_diff_seconds(&before_usage.user_time, &after_usage.user_time);
    result->system_seconds = timeval_diff_seconds(&before_usage.system_time, &after_usage.system_time);
    result->cpu_util_percent =
        100.0 * (result->user_seconds + result->system_seconds) / result->wall_seconds;
    result->peak_component_rss_kb = after_usage.max_rss_kb;
    result->checksum = checksum;

    free(buffer);
}

static void wait_shortly(void) {
    struct timespec delay;

    delay.tv_sec = 0;
    delay.tv_nsec = 1000000L;
    nanosleep(&delay, NULL);
}

static void run_processes_mode(const struct benchmark_config *config, struct benchmark_result *result) {
    struct process_shared_state *shared;
    pid_t children[MAX_SUPPORTED_WORKERS];
    struct usage_snapshot parent_before;
    struct usage_snapshot parent_after;
    struct timespec start_time;
    struct timespec end_time;
    double child_user_seconds = 0.0;
    double child_system_seconds = 0.0;
    long child_peak_rss_kb = 0L;
    uint64_t checksum = 0U;
    int worker;
    long total_rss_snapshot_kb;

    fill_common_result_fields(result, config);
    result->mode_name = "processes";

    shared = mmap(NULL,
                  sizeof(*shared),
                  PROT_READ | PROT_WRITE,
                  MAP_SHARED | MAP_ANONYMOUS,
                  -1,
                  0);
    if (shared == MAP_FAILED) {
        die_errno("mmap(shared)");
    }
    memset(shared, 0, sizeof(*shared));

    for (worker = 0; worker < config->workers; ++worker) {
        pid_t pid = fork();

        if (pid < 0) {
            die_errno("fork");
        }

        if (pid == 0) {
            uint64_t *buffer = calloc(config->words_per_worker, sizeof(*buffer));
            uint64_t seed = worker_seed(worker);
            uint64_t worker_checksum;

            if (buffer == NULL) {
                _exit(EXIT_FAILURE);
            }

            initialize_buffer(buffer, config->words_per_worker, seed);
            shared->slots[worker].rss_snapshot_kb = read_self_vmrss_kb();
            atomic_store_explicit(&shared->slots[worker].ready, 1, memory_order_release);

            while (atomic_load_explicit(&shared->start_flag, memory_order_acquire) == 0) {
                wait_shortly();
            }

            worker_checksum = compute_kernel(buffer,
                                             config->words_per_worker,
                                             config->iterations_per_worker,
                                             seed ^ UINT64_C(0xfeedface));
            shared->slots[worker].checksum = worker_checksum;
            free(buffer);
            _exit(EXIT_SUCCESS);
        }

        children[worker] = pid;
    }

    for (;;) {
        bool all_ready = true;

        for (worker = 0; worker < config->workers; ++worker) {
            if (atomic_load_explicit(&shared->slots[worker].ready, memory_order_acquire) == 0) {
                all_ready = false;
                break;
            }
        }

        if (all_ready) {
            break;
        }
        wait_shortly();
    }

    total_rss_snapshot_kb = read_self_vmrss_kb();
    for (worker = 0; worker < config->workers; ++worker) {
        total_rss_snapshot_kb += shared->slots[worker].rss_snapshot_kb;
    }
    result->rss_snapshot_kb = total_rss_snapshot_kb;

    read_usage_snapshot(RUSAGE_SELF, &parent_before);
    if (clock_gettime(CLOCK_MONOTONIC, &start_time) != 0) {
        die_errno("clock_gettime(start)");
    }
    atomic_store_explicit(&shared->start_flag, 1, memory_order_release);

    for (worker = 0; worker < config->workers; ++worker) {
        int status;
        struct rusage usage;
        pid_t waited;

        do {
            waited = wait4(children[worker], &status, 0, &usage);
        } while (waited < 0 && errno == EINTR);

        if (waited < 0) {
            die_errno("wait4");
        }
        if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
            die_message("child process benchmark failed");
        }

        child_user_seconds += (double)usage.ru_utime.tv_sec + (double)usage.ru_utime.tv_usec / 1000000.0;
        child_system_seconds += (double)usage.ru_stime.tv_sec + (double)usage.ru_stime.tv_usec / 1000000.0;
        if (usage.ru_maxrss > child_peak_rss_kb) {
            child_peak_rss_kb = usage.ru_maxrss;
        }
    }

    if (clock_gettime(CLOCK_MONOTONIC, &end_time) != 0) {
        die_errno("clock_gettime(end)");
    }
    read_usage_snapshot(RUSAGE_SELF, &parent_after);

    for (worker = 0; worker < config->workers; ++worker) {
        checksum = combine_checksum(checksum, shared->slots[worker].checksum);
    }

    result->wall_seconds = timespec_diff_seconds(&start_time, &end_time);
    result->user_seconds =
        child_user_seconds + timeval_diff_seconds(&parent_before.user_time, &parent_after.user_time);
    result->system_seconds =
        child_system_seconds + timeval_diff_seconds(&parent_before.system_time, &parent_after.system_time);
    result->cpu_util_percent =
        100.0 * (result->user_seconds + result->system_seconds) / result->wall_seconds;
    result->peak_component_rss_kb = child_peak_rss_kb;
    result->checksum = checksum;

    if (munmap(shared, sizeof(*shared)) != 0) {
        die_errno("munmap(shared)");
    }
}

static void print_result(const struct benchmark_result *result) {
    printf("[result] mode=%s workers=%d online_cpus=%d iterations_per_worker=%" PRIu64
           " bytes_per_worker=%zu rss_snapshot_kb=%ld peak_component_rss_kb=%ld "
           "wall_s=%.6f user_s=%.6f sys_s=%.6f cpu_util_percent=%.2f checksum=0x%016" PRIx64 "\n",
           result->mode_name,
           result->workers,
           result->online_cpus,
           result->iterations_per_worker,
           result->bytes_per_worker,
           result->rss_snapshot_kb,
           result->peak_component_rss_kb,
           result->wall_seconds,
           result->user_seconds,
           result->system_seconds,
           result->cpu_util_percent,
           result->checksum);
}

static void print_usage(const char *program_name) {
    fprintf(stderr,
            "usage:\n"
            "  %s single [workers] [words_per_worker] [iterations_per_worker]\n"
            "  %s threads [workers] [words_per_worker] [iterations_per_worker]\n"
            "  %s processes [workers] [words_per_worker] [iterations_per_worker]\n"
            "  %s benchmark [workers] [words_per_worker] [iterations_per_worker]\n",
            program_name,
            program_name,
            program_name,
            program_name);
}

static struct benchmark_config load_config(int argc, char **argv) {
    struct benchmark_config config;
    int default_workers = online_cpu_count();

    if (default_workers > DEFAULT_MAX_WORKERS) {
        default_workers = DEFAULT_MAX_WORKERS;
    }

    config.workers = default_workers;
    config.words_per_worker = DEFAULT_WORDS_PER_WORKER;
    config.iterations_per_worker = DEFAULT_ITERATIONS_PER_WORKER;

    if (argc >= 3) {
        config.workers = parse_positive_int(argv[2], "workers");
    }
    if (argc >= 4) {
        config.words_per_worker = (size_t)parse_u64(argv[3], "words_per_worker");
    }
    if (argc >= 5) {
        config.iterations_per_worker = parse_u64(argv[4], "iterations_per_worker");
    }
    if (argc > 5) {
        print_usage(argv[0]);
        exit(EXIT_FAILURE);
    }

    if (config.workers > MAX_SUPPORTED_WORKERS) {
        fprintf(stderr, "workers must be <= %d\n", MAX_SUPPORTED_WORKERS);
        exit(EXIT_FAILURE);
    }

    require_power_of_two(config.words_per_worker);
    return config;
}

int main(int argc, char **argv) {
    struct benchmark_config config;

    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);

    if (argc < 2) {
        print_usage(argv[0]);
        return EXIT_FAILURE;
    }

    config = load_config(argc, argv);

    printf("[config] workers=%d online_cpus=%d words_per_worker=%zu bytes_per_worker=%zu "
           "iterations_per_worker=%" PRIu64 "\n",
           config.workers,
           online_cpu_count(),
           config.words_per_worker,
           config.words_per_worker * sizeof(uint64_t),
           config.iterations_per_worker);

    if (strcmp(argv[1], "single") == 0) {
        struct benchmark_result result;

        run_single_mode(&config, &result);
        print_result(&result);
        return EXIT_SUCCESS;
    }

    if (strcmp(argv[1], "threads") == 0) {
        struct benchmark_result result;

        run_threads_mode(&config, &result);
        print_result(&result);
        return EXIT_SUCCESS;
    }

    if (strcmp(argv[1], "processes") == 0) {
        struct benchmark_result result;

        run_processes_mode(&config, &result);
        print_result(&result);
        return EXIT_SUCCESS;
    }

    if (strcmp(argv[1], "benchmark") == 0) {
        struct benchmark_result single_result;
        struct benchmark_result thread_result;
        struct benchmark_result process_result;

        run_single_mode(&config, &single_result);
        print_result(&single_result);
        run_threads_mode(&config, &thread_result);
        print_result(&thread_result);
        run_processes_mode(&config, &process_result);
        print_result(&process_result);
        printf("[done] benchmark suite completed successfully\n");
        return EXIT_SUCCESS;
    }

    print_usage(argv[0]);
    return EXIT_FAILURE;
}
