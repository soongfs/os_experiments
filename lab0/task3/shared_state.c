#include "shared_state.h"

#include <string.h>

int init_shared_state(struct shared_state *state, int log_fd) {
    memset(state, 0, sizeof(*state));
    state->log_fd = log_fd;

    if (pthread_mutex_init(&state->lock, NULL) != 0) {
        return -1;
    }

    if (pthread_barrier_init(&state->start_barrier, NULL, THREAD_COUNT) != 0) {
        pthread_mutex_destroy(&state->lock);
        return -1;
    }

    return 0;
}

void destroy_shared_state(struct shared_state *state) {
    pthread_barrier_destroy(&state->start_barrier);
    pthread_mutex_destroy(&state->lock);
}
