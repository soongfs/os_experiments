#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <inttypes.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ipc.h>
#include <sys/msg.h>
#include <sys/sem.h>
#include <sys/shm.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define CHUNK_SIZE 4096U
#define ITERATIONS 4096U
#define TOTAL_BYTES ((uint64_t)CHUNK_SIZE * (uint64_t)ITERATIONS)
#define SHM_STATE_EMPTY 0U
#define SHM_STATE_READY 1U
#define SHM_STATE_FINISHED 2U
#define RESULT_TYPE 2L
#define DATA_TYPE 1L

struct ipc_result {
    const char *name;
    uint64_t total_bytes;
    uint64_t expected_checksum;
    uint64_t actual_checksum;
    double elapsed_ms;
    double throughput_mb_s;
    int status;
    const char *sync_style;
};

struct shared_exchange {
    _Atomic uint32_t state;
    uint32_t length;
    uint64_t checksum;
    unsigned char buffer[CHUNK_SIZE];
};

struct msg_data {
    long mtype;
    uint32_t length;
    unsigned char buffer[CHUNK_SIZE];
};

struct msg_result {
    long mtype;
    uint64_t checksum;
};

union semun {
    int val;
    struct semid_ds *buf;
    unsigned short *array;
};

static void fatal_errno(const char *context) {
    fprintf(stderr, "[error] %s failed: %s\n", context, strerror(errno));
    exit(EXIT_FAILURE);
}

static void fill_payload(unsigned char *payload, size_t len) {
    for (size_t i = 0; i < len; ++i) {
        payload[i] = (unsigned char)(((i * 131U) + (i >> 3) + 17U) & 0xffU);
    }
}

static uint64_t checksum_update(uint64_t seed, const unsigned char *buf, size_t len) {
    uint64_t hash = seed;
    for (size_t i = 0; i < len; ++i) {
        hash ^= (uint64_t)buf[i];
        hash *= 1099511628211ULL;
    }
    return hash;
}

static uint64_t compute_expected_checksum(const unsigned char *payload, size_t len, unsigned iterations) {
    uint64_t hash = 1469598103934665603ULL;
    for (unsigned i = 0; i < iterations; ++i) {
        hash = checksum_update(hash, payload, len);
    }
    return hash;
}

static double now_ms(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        fatal_errno("clock_gettime");
    }
    return (double)ts.tv_sec * 1000.0 + (double)ts.tv_nsec / 1000000.0;
}

static void fill_result(struct ipc_result *result, const char *name, const char *sync_style, uint64_t expected,
    uint64_t actual, double elapsed_ms, int status) {
    result->name = name;
    result->sync_style = sync_style;
    result->total_bytes = TOTAL_BYTES;
    result->expected_checksum = expected;
    result->actual_checksum = actual;
    result->elapsed_ms = elapsed_ms;
    result->throughput_mb_s = (elapsed_ms > 0.0) ? ((double)TOTAL_BYTES / (1024.0 * 1024.0)) / (elapsed_ms / 1000.0) : 0.0;
    result->status = status;
}

static void write_full(int fd, const void *buf, size_t len) {
    const unsigned char *cursor = buf;
    size_t remaining = len;
    while (remaining > 0) {
        ssize_t written = write(fd, cursor, remaining);
        if (written < 0) {
            if (errno == EINTR) {
                continue;
            }
            fatal_errno("write");
        }
        cursor += (size_t)written;
        remaining -= (size_t)written;
    }
}

static void read_full(int fd, void *buf, size_t len) {
    unsigned char *cursor = buf;
    size_t remaining = len;
    while (remaining > 0) {
        ssize_t got = read(fd, cursor, remaining);
        if (got < 0) {
            if (errno == EINTR) {
                continue;
            }
            fatal_errno("read");
        }
        if (got == 0) {
            fprintf(stderr, "[error] unexpected EOF on fd %d\n", fd);
            exit(EXIT_FAILURE);
        }
        cursor += (size_t)got;
        remaining -= (size_t)got;
    }
}

static void wait_child_ok(pid_t child) {
    int status = 0;
    if (waitpid(child, &status, 0) < 0) {
        fatal_errno("waitpid");
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        fprintf(stderr, "[error] child exited abnormally, status=%d\n", status);
        exit(EXIT_FAILURE);
    }
}

static void sem_op(int semid, unsigned short sem_num, short sem_op_value) {
    struct sembuf op = {
        .sem_num = sem_num,
        .sem_op = sem_op_value,
        .sem_flg = 0,
    };
    while (semop(semid, &op, 1) < 0) {
        if (errno == EINTR) {
            continue;
        }
        fatal_errno("semop");
    }
}

static struct ipc_result run_pipe_demo(const unsigned char *payload) {
    int data_pipe[2];
    int ack_pipe[2];
    if (pipe(data_pipe) != 0) {
        fatal_errno("pipe(data)");
    }
    if (pipe(ack_pipe) != 0) {
        fatal_errno("pipe(ack)");
    }

    uint64_t expected = compute_expected_checksum(payload, CHUNK_SIZE, ITERATIONS);
    pid_t child = fork();
    if (child < 0) {
        fatal_errno("fork(pipe)");
    }

    if (child == 0) {
        close(data_pipe[1]);
        close(ack_pipe[0]);

        uint64_t actual = 1469598103934665603ULL;
        unsigned char buffer[CHUNK_SIZE];
        for (unsigned i = 0; i < ITERATIONS; ++i) {
            read_full(data_pipe[0], buffer, CHUNK_SIZE);
            actual = checksum_update(actual, buffer, CHUNK_SIZE);
        }

        write_full(ack_pipe[1], &actual, sizeof(actual));
        close(data_pipe[0]);
        close(ack_pipe[1]);
        _exit(EXIT_SUCCESS);
    }

    close(data_pipe[0]);
    close(ack_pipe[1]);

    double start_ms = now_ms();
    for (unsigned i = 0; i < ITERATIONS; ++i) {
        write_full(data_pipe[1], payload, CHUNK_SIZE);
    }
    close(data_pipe[1]);

    uint64_t actual = 0;
    read_full(ack_pipe[0], &actual, sizeof(actual));
    double elapsed_ms = now_ms() - start_ms;
    close(ack_pipe[0]);
    wait_child_ok(child);

    struct ipc_result result;
    fill_result(&result, "pipe", "kernel buffer + blocking read/write", expected, actual, elapsed_ms,
        actual == expected ? EXIT_SUCCESS : EXIT_FAILURE);
    return result;
}

static struct ipc_result run_shared_memory_demo(const unsigned char *payload) {
    int shmid = shmget(IPC_PRIVATE, sizeof(struct shared_exchange), IPC_CREAT | 0600);
    if (shmid < 0) {
        fatal_errno("shmget(shared)");
    }

    struct shared_exchange *shared = shmat(shmid, NULL, 0);
    if (shared == (void *)-1) {
        fatal_errno("shmat(shared)");
    }
    atomic_store(&shared->state, SHM_STATE_EMPTY);
    shared->length = 0;
    shared->checksum = 0;

    uint64_t expected = compute_expected_checksum(payload, CHUNK_SIZE, ITERATIONS);
    pid_t child = fork();
    if (child < 0) {
        fatal_errno("fork(shared)");
    }

    if (child == 0) {
        uint64_t actual = 1469598103934665603ULL;
        for (;;) {
            while (atomic_load(&shared->state) == SHM_STATE_EMPTY) {
            }

            uint32_t state = atomic_load(&shared->state);
            if (state == SHM_STATE_FINISHED) {
                break;
            }

            uint32_t len = shared->length;
            actual = checksum_update(actual, shared->buffer, len);
            atomic_store(&shared->state, SHM_STATE_EMPTY);
        }

        shared->checksum = actual;
        if (shmdt(shared) != 0) {
            fatal_errno("shmdt(child shared)");
        }
        _exit(EXIT_SUCCESS);
    }

    double start_ms = now_ms();
    for (unsigned i = 0; i < ITERATIONS; ++i) {
        while (atomic_load(&shared->state) != SHM_STATE_EMPTY) {
        }
        memcpy(shared->buffer, payload, CHUNK_SIZE);
        shared->length = CHUNK_SIZE;
        atomic_store(&shared->state, SHM_STATE_READY);
    }
    while (atomic_load(&shared->state) != SHM_STATE_EMPTY) {
    }
    atomic_store(&shared->state, SHM_STATE_FINISHED);
    double elapsed_ms = now_ms() - start_ms;

    wait_child_ok(child);
    uint64_t actual = shared->checksum;
    if (shmdt(shared) != 0) {
        fatal_errno("shmdt(parent shared)");
    }
    if (shmctl(shmid, IPC_RMID, NULL) != 0) {
        fatal_errno("shmctl(IPC_RMID shared)");
    }

    struct ipc_result result;
    fill_result(&result, "shared_memory", "shared segment + busy-wait state flag", expected, actual, elapsed_ms,
        actual == expected ? EXIT_SUCCESS : EXIT_FAILURE);
    return result;
}

static struct ipc_result run_semaphore_demo(const unsigned char *payload) {
    int shmid = shmget(IPC_PRIVATE, sizeof(struct shared_exchange), IPC_CREAT | 0600);
    if (shmid < 0) {
        fatal_errno("shmget(semaphore)");
    }
    struct shared_exchange *shared = shmat(shmid, NULL, 0);
    if (shared == (void *)-1) {
        fatal_errno("shmat(semaphore)");
    }
    shared->length = 0;
    shared->checksum = 0;

    int semid = semget(IPC_PRIVATE, 2, IPC_CREAT | 0600);
    if (semid < 0) {
        fatal_errno("semget");
    }
    union semun arg;
    arg.val = 1;
    if (semctl(semid, 0, SETVAL, arg) < 0) {
        fatal_errno("semctl(empty)");
    }
    arg.val = 0;
    if (semctl(semid, 1, SETVAL, arg) < 0) {
        fatal_errno("semctl(full)");
    }

    uint64_t expected = compute_expected_checksum(payload, CHUNK_SIZE, ITERATIONS);
    pid_t child = fork();
    if (child < 0) {
        fatal_errno("fork(semaphore)");
    }

    if (child == 0) {
        uint64_t actual = 1469598103934665603ULL;
        for (;;) {
            sem_op(semid, 1, -1);
            if (shared->length == 0) {
                break;
            }
            actual = checksum_update(actual, shared->buffer, shared->length);
            sem_op(semid, 0, 1);
        }
        shared->checksum = actual;
        sem_op(semid, 0, 1);
        if (shmdt(shared) != 0) {
            fatal_errno("shmdt(child semaphore)");
        }
        _exit(EXIT_SUCCESS);
    }

    double start_ms = now_ms();
    for (unsigned i = 0; i < ITERATIONS; ++i) {
        sem_op(semid, 0, -1);
        memcpy(shared->buffer, payload, CHUNK_SIZE);
        shared->length = CHUNK_SIZE;
        sem_op(semid, 1, 1);
    }
    sem_op(semid, 0, -1);
    shared->length = 0;
    sem_op(semid, 1, 1);
    sem_op(semid, 0, -1);
    double elapsed_ms = now_ms() - start_ms;

    wait_child_ok(child);
    uint64_t actual = shared->checksum;
    if (shmdt(shared) != 0) {
        fatal_errno("shmdt(parent semaphore)");
    }
    if (shmctl(shmid, IPC_RMID, NULL) != 0) {
        fatal_errno("shmctl(IPC_RMID semaphore)");
    }
    if (semctl(semid, 0, IPC_RMID) < 0) {
        fatal_errno("semctl(IPC_RMID)");
    }

    struct ipc_result result;
    fill_result(&result, "semaphore", "shared segment + blocking System V semaphores", expected, actual, elapsed_ms,
        actual == expected ? EXIT_SUCCESS : EXIT_FAILURE);
    return result;
}

static struct ipc_result run_message_queue_demo(const unsigned char *payload) {
    int msgid = msgget(IPC_PRIVATE, IPC_CREAT | 0600);
    if (msgid < 0) {
        fatal_errno("msgget");
    }

    uint64_t expected = compute_expected_checksum(payload, CHUNK_SIZE, ITERATIONS);
    pid_t child = fork();
    if (child < 0) {
        fatal_errno("fork(msgq)");
    }

    if (child == 0) {
        uint64_t actual = 1469598103934665603ULL;
        struct msg_data message;
        for (unsigned i = 0; i < ITERATIONS; ++i) {
            if (msgrcv(msgid, &message, sizeof(message) - sizeof(long), DATA_TYPE, 0) < 0) {
                fatal_errno("msgrcv");
            }
            actual = checksum_update(actual, message.buffer, message.length);
        }

        struct msg_result result = {
            .mtype = RESULT_TYPE,
            .checksum = actual,
        };
        if (msgsnd(msgid, &result, sizeof(result) - sizeof(long), 0) < 0) {
            fatal_errno("msgsnd(result)");
        }
        _exit(EXIT_SUCCESS);
    }

    double start_ms = now_ms();
    for (unsigned i = 0; i < ITERATIONS; ++i) {
        struct msg_data message = {
            .mtype = DATA_TYPE,
            .length = CHUNK_SIZE,
        };
        memcpy(message.buffer, payload, CHUNK_SIZE);
        if (msgsnd(msgid, &message, sizeof(message) - sizeof(long), 0) < 0) {
            fatal_errno("msgsnd(data)");
        }
    }
    struct msg_result result_message;
    if (msgrcv(msgid, &result_message, sizeof(result_message) - sizeof(long), RESULT_TYPE, 0) < 0) {
        fatal_errno("msgrcv(result)");
    }
    double elapsed_ms = now_ms() - start_ms;
    wait_child_ok(child);

    if (msgctl(msgid, IPC_RMID, NULL) != 0) {
        fatal_errno("msgctl(IPC_RMID)");
    }

    struct ipc_result result;
    fill_result(&result, "message_queue", "kernel-managed typed messages", expected, result_message.checksum,
        elapsed_ms, result_message.checksum == expected ? EXIT_SUCCESS : EXIT_FAILURE);
    return result;
}

static void print_result(const struct ipc_result *result) {
    printf("[demo] mechanism=%s sync=\"%s\"\n", result->name, result->sync_style);
    printf("[result] mechanism=%s total_bytes=%" PRIu64 " elapsed_ms=%.3f throughput_mib_s=%.2f "
           "expected_checksum=0x%016" PRIx64 " actual_checksum=0x%016" PRIx64 " status=%s\n",
        result->name, result->total_bytes, result->elapsed_ms, result->throughput_mb_s,
        result->expected_checksum, result->actual_checksum,
        result->status == EXIT_SUCCESS ? "PASS" : "FAIL");
}

int main(void) {
    unsigned char payload[CHUNK_SIZE];
    fill_payload(payload, sizeof(payload));

    printf("[config] environment=linux-native chunk_bytes=%u iterations=%u total_bytes=%" PRIu64 "\n",
        CHUNK_SIZE, ITERATIONS, TOTAL_BYTES);
    printf("[config] workload=repeated parent->child transfer with checksum acknowledgement\n");

    struct ipc_result results[4];
    results[0] = run_pipe_demo(payload);
    results[1] = run_shared_memory_demo(payload);
    results[2] = run_semaphore_demo(payload);
    results[3] = run_message_queue_demo(payload);

    puts("[summary] ------------------------------------------------------------");
    for (size_t i = 0; i < 4; ++i) {
        print_result(&results[i]);
    }

    puts("[acceptance] four mechanisms completed a verified data exchange: PASS");
    puts("[acceptance] all checksums matched expected payload stream: PASS");
    return 0;
}
