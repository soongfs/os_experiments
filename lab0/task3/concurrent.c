#include "concurrent.h"

#include <sched.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

struct worker_arg {
    struct shared_state *state;
    int thread_id;
};

static void sleep_for_ns(long nanoseconds) {
    struct timespec ts;

    ts.tv_sec = nanoseconds / 1000000000L;
    ts.tv_nsec = nanoseconds % 1000000000L;
    nanosleep(&ts, NULL);
}

static long asynchronous_delay_ns(int thread_id, int local_round) {
    struct timespec now;

    clock_gettime(CLOCK_MONOTONIC, &now);

    return 500000L + ((now.tv_nsec + thread_id * 977 + local_round * 313) % 3500000L);
}

static void log_claim(struct shared_state *state, int thread_id, int claim_id) {
    char buffer[128];
    int len = snprintf(
        buffer,
        sizeof(buffer),
        "thread-%d claimed slot-%d\n",
        thread_id,
        claim_id
    );

    if (len > 0) {
        write(state->log_fd, buffer, (size_t)len);
    }
}

static void *worker_main(void *opaque) {
    struct worker_arg *arg = (struct worker_arg *)opaque;
    struct shared_state *state = arg->state;
    int local_round = 0;

    pthread_barrier_wait(&state->start_barrier);

    for (;;) {
        int claim_id;

        sleep_for_ns(asynchronous_delay_ns(arg->thread_id, local_round));

        pthread_mutex_lock(&state->lock);
        if (state->next_claim >= CLAIM_COUNT) {
            pthread_mutex_unlock(&state->lock);
            break;
        }

        claim_id = state->next_claim++;
        state->claim_sequence[state->total_claims++] = arg->thread_id;
        state->claims_by_thread[arg->thread_id]++;
        pthread_mutex_unlock(&state->lock);

        log_claim(state, arg->thread_id, claim_id);

        if (((claim_id + arg->thread_id) & 1) == 0) {
            sched_yield();
        }

        local_round++;
    }

    return NULL;
}

int run_concurrency_demo(struct shared_state *state) {
    pthread_t threads[THREAD_COUNT];
    struct worker_arg args[THREAD_COUNT];
    int i;

    for (i = 0; i < THREAD_COUNT; ++i) {
        args[i].state = state;
        args[i].thread_id = i;
        if (pthread_create(&threads[i], NULL, worker_main, &args[i]) != 0) {
            return -1;
        }
    }

    for (i = 0; i < THREAD_COUNT; ++i) {
        pthread_join(threads[i], NULL);
    }

    return 0;
}
