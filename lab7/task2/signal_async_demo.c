#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#define SIGUSR1_ROUNDS 8

static int event_pipe[2] = {-1, -1};
static volatile sig_atomic_t sigusr1_count = 0;
static volatile sig_atomic_t sigusr2_count = 0;
static volatile sig_atomic_t handler_failures = 0;

static void fatal_errno(const char *context) {
    fprintf(stderr, "[error] %s failed: %s\n", context, strerror(errno));
    exit(EXIT_FAILURE);
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

static void signal_handler(int signo) {
    unsigned char event = (unsigned char)signo;

    if (signo == SIGUSR1) {
        ++sigusr1_count;
    } else if (signo == SIGUSR2) {
        ++sigusr2_count;
    }

    if (write(event_pipe[1], &event, sizeof(event)) < 0) {
        ++handler_failures;
    }
}

static void install_handler(int signo) {
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = signal_handler;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = SA_RESTART;

    if (sigaction(signo, &sa, NULL) != 0) {
        fatal_errno("sigaction");
    }
}

static void wait_child_ok(pid_t child) {
    int status = 0;
    if (waitpid(child, &status, 0) < 0) {
        fatal_errno("waitpid");
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        fprintf(stderr, "[error] sender exited abnormally, status=%d\n", status);
        exit(EXIT_FAILURE);
    }
}

int main(void) {
    int ack_pipe[2];
    if (pipe(event_pipe) != 0) {
        fatal_errno("pipe(event)");
    }
    if (pipe(ack_pipe) != 0) {
        fatal_errno("pipe(ack)");
    }

    install_handler(SIGUSR1);
    install_handler(SIGUSR2);

    pid_t sender = fork();
    if (sender < 0) {
        fatal_errno("fork");
    }

    if (sender == 0) {
        close(event_pipe[0]);
        close(event_pipe[1]);
        close(ack_pipe[1]);

        pid_t receiver = getppid();
        for (int i = 0; i < SIGUSR1_ROUNDS; ++i) {
            if (kill(receiver, SIGUSR1) != 0) {
                fatal_errno("kill(SIGUSR1)");
            }
            unsigned char ack = 0;
            read_full(ack_pipe[0], &ack, sizeof(ack));
        }

        if (kill(receiver, SIGUSR2) != 0) {
            fatal_errno("kill(SIGUSR2)");
        }
        unsigned char final_ack = 0;
        read_full(ack_pipe[0], &final_ack, sizeof(final_ack));

        close(ack_pipe[0]);
        _exit(EXIT_SUCCESS);
    }

    close(ack_pipe[0]);

    int handled_usr1 = 0;
    int handled_usr2 = 0;
    int shared_total = 0;
    bool stop = false;

    printf("[config] environment=linux-native sender_pid=%ld receiver_pid=%ld sigusr1_rounds=%d\n",
        (long)sender, (long)getpid(), SIGUSR1_ROUNDS);
    printf("[config] control=self-pipe for async-safe handoff, ack-pipe to avoid coalescing races\n");

    while (!stop) {
        unsigned char event = 0;
        read_full(event_pipe[0], &event, sizeof(event));

        if (event == (unsigned char)SIGUSR1) {
            ++handled_usr1;
            ++shared_total;
            printf("[receiver] event=SIGUSR1 handled_usr1=%d handler_visible_count=%d shared_total=%d status=processed\n",
                handled_usr1, (int)sigusr1_count, shared_total);
            unsigned char ack = '1';
            write_full(ack_pipe[1], &ack, sizeof(ack));
        } else if (event == (unsigned char)SIGUSR2) {
            ++handled_usr2;
            printf("[receiver] event=SIGUSR2 handled_usr2=%d handler_visible_count=%d action=shutdown\n",
                handled_usr2, (int)sigusr2_count);
            unsigned char ack = '2';
            write_full(ack_pipe[1], &ack, sizeof(ack));
            stop = true;
        } else {
            printf("[receiver] event=unknown signo=%u status=ignored\n", event);
        }
    }

    wait_child_ok(sender);
    close(event_pipe[0]);
    close(event_pipe[1]);
    close(ack_pipe[1]);

    printf("[summary] handler_sigusr1=%d mainloop_sigusr1=%d handler_sigusr2=%d mainloop_sigusr2=%d shared_total=%d handler_failures=%d\n",
        (int)sigusr1_count, handled_usr1, (int)sigusr2_count, handled_usr2, shared_total, (int)handler_failures);

    printf("[acceptance] custom SIGUSR1 handler executed and receiver observed %d deliveries: %s\n",
        handled_usr1, (handled_usr1 == SIGUSR1_ROUNDS && sigusr1_count == SIGUSR1_ROUNDS) ? "PASS" : "FAIL");
    printf("[acceptance] shutdown signal SIGUSR2 was captured by custom handler: %s\n",
        (handled_usr2 == 1 && sigusr2_count == 1) ? "PASS" : "FAIL");
    printf("[acceptance] handler used only async-signal-safe write plus sig_atomic_t counters: %s\n",
        handler_failures == 0 ? "PASS" : "FAIL");

    return (handled_usr1 == SIGUSR1_ROUNDS && handled_usr2 == 1 && handler_failures == 0) ? EXIT_SUCCESS : EXIT_FAILURE;
}
