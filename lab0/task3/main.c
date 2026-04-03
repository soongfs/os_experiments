#include "concurrent.h"
#include "persistence.h"
#include "shared_state.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static int ensure_directory(const char *path) {
    if (mkdir(path, 0755) == 0 || errno == EEXIST) {
        return 0;
    }
    return -1;
}

static void print_sequence(const struct shared_state *state) {
    int i;

    printf("claim sequence: ");
    for (i = 0; i < state->total_claims; ++i) {
        printf("T%d%s", state->claim_sequence[i], (i + 1 == state->total_claims) ? "\n" : " -> ");
    }
}

int main(int argc, char *argv[]) {
    const char *label = (argc > 1) ? argv[1] : "default";
    const char *artifact_dir = "artifacts";
    char log_path[256];
    char state_path[256];
    struct shared_state state;
    int log_fd;

    if (ensure_directory(artifact_dir) != 0) {
        perror("mkdir");
        return 1;
    }

    snprintf(log_path, sizeof(log_path), "%s/%s_event_log.txt", artifact_dir, label);
    snprintf(state_path, sizeof(state_path), "%s/%s_final_state.txt", artifact_dir, label);

    log_fd = open(log_path, O_WRONLY | O_CREAT | O_TRUNC | O_APPEND, 0644);
    if (log_fd < 0) {
        perror("open log file");
        return 1;
    }

    if (init_shared_state(&state, log_fd) != 0) {
        perror("init_shared_state");
        close(log_fd);
        return 1;
    }

    if (run_concurrency_demo(&state) != 0) {
        perror("run_concurrency_demo");
        destroy_shared_state(&state);
        close(log_fd);
        return 1;
    }

    if (fsync(log_fd) < 0) {
        perror("fsync log file");
        destroy_shared_state(&state);
        close(log_fd);
        return 1;
    }

    if (persist_final_state(&state, label, state_path, log_path) != 0) {
        perror("persist_final_state");
        destroy_shared_state(&state);
        close(log_fd);
        return 1;
    }

    printf("label: %s\n", label);
    printf("log file: %s\n", log_path);
    printf("state file: %s\n", state_path);
    printf("total claims: %d\n", state.total_claims);
    print_sequence(&state);

    destroy_shared_state(&state);

    if (close(log_fd) < 0) {
        perror("close");
        return 1;
    }

    return 0;
}
