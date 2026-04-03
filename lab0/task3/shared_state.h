#ifndef LAB0_TASK3_SHARED_STATE_H
#define LAB0_TASK3_SHARED_STATE_H

#include <pthread.h>

#define THREAD_COUNT 4
#define CLAIM_COUNT 16

struct shared_state {
    pthread_mutex_t lock;
    pthread_barrier_t start_barrier;
    int next_claim;
    int total_claims;
    int claims_by_thread[THREAD_COUNT];
    int claim_sequence[CLAIM_COUNT];
    int log_fd;
};

int init_shared_state(struct shared_state *state, int log_fd);
void destroy_shared_state(struct shared_state *state);

#endif
