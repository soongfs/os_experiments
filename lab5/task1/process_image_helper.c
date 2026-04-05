#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <limits.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <unistd.h>

#ifndef PATH_MAX
#define PATH_MAX 4096
#endif

static void read_current_exe_path(char *buffer, size_t buffer_size) {
    ssize_t length;

    length = readlink("/proc/self/exe", buffer, buffer_size - 1U);
    if (length < 0) {
        fprintf(stderr, "readlink(/proc/self/exe): %s\n", strerror(errno));
        exit(EXIT_FAILURE);
    }
    if ((size_t)length >= buffer_size - 1U) {
        fprintf(stderr, "helper executable path is too long\n");
        exit(EXIT_FAILURE);
    }

    buffer[length] = '\0';
}

static long parse_long(const char *text, const char *label) {
    char *end = NULL;
    long value;

    errno = 0;
    value = strtol(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0') {
        fprintf(stderr, "failed to parse %s from '%s'\n", label, text);
        exit(EXIT_FAILURE);
    }

    return value;
}

static int query_nice_value(void) {
    int value;

    errno = 0;
    value = getpriority(PRIO_PROCESS, 0);
    if (value == -1 && errno != 0) {
        fprintf(stderr, "getpriority: %s\n", strerror(errno));
        exit(EXIT_FAILURE);
    }

    return value;
}

static int run_exec_mode(int argc, char **argv) {
    char exe_path[PATH_MAX];
    long pre_exec_pid;
    long pre_exec_ppid;
    long pre_exec_nice;
    long expected_nice;
    long current_pid = (long)getpid();
    long current_ppid = (long)getppid();
    int current_nice = query_nice_value();
    bool pid_match;
    bool ppid_match;
    bool nice_match;

    if (argc != 6) {
        fprintf(stderr, "usage: %s exec <pre_exec_pid> <pre_exec_ppid> <pre_exec_nice> <expected_nice>\n",
                argv[0]);
        return EXIT_FAILURE;
    }

    pre_exec_pid = parse_long(argv[2], "pre_exec_pid");
    pre_exec_ppid = parse_long(argv[3], "pre_exec_ppid");
    pre_exec_nice = parse_long(argv[4], "pre_exec_nice");
    expected_nice = parse_long(argv[5], "expected_nice");
    pid_match = current_pid == pre_exec_pid;
    ppid_match = current_ppid == pre_exec_ppid;
    nice_match = (long)current_nice == pre_exec_nice && (long)current_nice == expected_nice;

    read_current_exe_path(exe_path, sizeof(exe_path));
    printf("[image-helper/exec] pid=%ld pre_exec_pid=%ld same_pid=%s ppid=%ld pre_exec_ppid=%ld same_ppid=%s nice=%d pre_exec_nice=%ld same_nice=%s exe=%s\n",
           current_pid,
           pre_exec_pid,
           pid_match ? "yes" : "no",
           current_ppid,
           pre_exec_ppid,
           ppid_match ? "yes" : "no",
           current_nice,
           pre_exec_nice,
           nice_match ? "yes" : "no",
           exe_path);
    printf("[image-helper/exec] image_replacement=%s\n",
           (pid_match && ppid_match && nice_match) ? "confirmed" : "failed");

    return (pid_match && ppid_match && nice_match) ? EXIT_SUCCESS : EXIT_FAILURE;
}

static int run_spawn_mode(int argc, char **argv) {
    char exe_path[PATH_MAX];
    long expected_parent_pid;
    long expected_nice;
    long current_ppid = (long)getppid();
    int current_nice = query_nice_value();
    bool parent_match;
    bool nice_match;

    if (argc != 4) {
        fprintf(stderr, "usage: %s spawn <expected_parent_pid> <expected_nice>\n", argv[0]);
        return EXIT_FAILURE;
    }

    expected_parent_pid = parse_long(argv[2], "expected_parent_pid");
    expected_nice = parse_long(argv[3], "expected_nice");
    parent_match = current_ppid == expected_parent_pid;
    nice_match = (long)current_nice == expected_nice;

    read_current_exe_path(exe_path, sizeof(exe_path));
    printf("[image-helper/spawn] pid=%ld ppid=%ld expected_parent=%ld same_parent=%s nice=%d expected_nice=%ld same_nice=%s exe=%s\n",
           (long)getpid(),
           current_ppid,
           expected_parent_pid,
           parent_match ? "yes" : "no",
           current_nice,
           expected_nice,
           nice_match ? "yes" : "no",
           exe_path);
    printf("[image-helper/spawn] spawn_result=%s\n",
           (parent_match && nice_match) ? "confirmed" : "failed");

    return (parent_match && nice_match) ? EXIT_SUCCESS : EXIT_FAILURE;
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);

    if (argc < 2) {
        fprintf(stderr, "usage: %s <exec|spawn> ...\n", argv[0]);
        return EXIT_FAILURE;
    }

    if (strcmp(argv[1], "exec") == 0) {
        return run_exec_mode(argc, argv);
    }
    if (strcmp(argv[1], "spawn") == 0) {
        return run_spawn_mode(argc, argv);
    }

    fprintf(stderr, "unknown mode '%s'\n", argv[1]);
    return EXIT_FAILURE;
}
