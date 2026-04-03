#include "persistence.h"

#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int write_all(int fd, const char *buf, size_t len) {
    while (len > 0) {
        ssize_t written = write(fd, buf, len);
        if (written < 0) {
            return -1;
        }
        buf += (size_t)written;
        len -= (size_t)written;
    }
    return 0;
}

int persist_final_state(
    const struct shared_state *state,
    const char *label,
    const char *state_path,
    const char *log_path
) {
    char buffer[2048];
    int offset;
    int i;
    int fd = open(state_path, O_WRONLY | O_CREAT | O_TRUNC, 0644);

    if (fd < 0) {
        return -1;
    }

    offset = snprintf(
        buffer,
        sizeof(buffer),
        "label=%s\nlog_path=%s\ntotal_claims=%d\n",
        label,
        log_path,
        state->total_claims
    );

    for (i = 0; i < THREAD_COUNT; ++i) {
        offset += snprintf(
            buffer + offset,
            sizeof(buffer) - (size_t)offset,
            "claims_by_thread[%d]=%d\n",
            i,
            state->claims_by_thread[i]
        );
    }

    offset += snprintf(buffer + offset, sizeof(buffer) - (size_t)offset, "claim_sequence=");
    for (i = 0; i < state->total_claims; ++i) {
        offset += snprintf(
            buffer + offset,
            sizeof(buffer) - (size_t)offset,
            "T%d%s",
            state->claim_sequence[i],
            (i + 1 == state->total_claims) ? "\n" : " -> "
        );
    }

    if (write_all(fd, buffer, (size_t)offset) < 0) {
        close(fd);
        return -1;
    }

    if (fsync(fd) < 0) {
        close(fd);
        return -1;
    }

    if (close(fd) < 0) {
        return -1;
    }

    return 0;
}
