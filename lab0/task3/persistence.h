#ifndef LAB0_TASK3_PERSISTENCE_H
#define LAB0_TASK3_PERSISTENCE_H

#include "shared_state.h"

int persist_final_state(
    const struct shared_state *state,
    const char *label,
    const char *state_path,
    const char *log_path
);

#endif
