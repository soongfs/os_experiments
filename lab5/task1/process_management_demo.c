#define _POSIX_C_SOURCE 200809L
#define _DEFAULT_SOURCE

#include <errno.h>
#include <limits.h>
#include <spawn.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

#ifndef PATH_MAX
#define PATH_MAX 4096
#endif

enum {
    NICE_INCREMENT = 4,
    FORK_MARKER_INITIAL = 17,
    FORK_MARKER_PARENT = 111,
    FORK_MARKER_CHILD = 222,
};

struct child_result {
    bool exited;
    int code;
    int signal_number;
};

static void die_errno(const char *context) {
    fprintf(stderr, "%s: %s\n", context, strerror(errno));
    exit(EXIT_FAILURE);
}

static void die_posix_error(const char *context, int error_code) {
    fprintf(stderr, "%s: %s\n", context, strerror(error_code));
    exit(EXIT_FAILURE);
}

static void die_message(const char *message) {
    fprintf(stderr, "%s\n", message);
    exit(EXIT_FAILURE);
}

static void read_current_exe_path(char *buffer, size_t buffer_size) {
    ssize_t length;

    length = readlink("/proc/self/exe", buffer, buffer_size - 1U);
    if (length < 0) {
        die_errno("readlink(/proc/self/exe)");
    }
    if ((size_t)length >= buffer_size - 1U) {
        die_message("executable path is too long");
    }

    buffer[length] = '\0';
}

static void extract_directory(const char *path, char *buffer, size_t buffer_size) {
    const char *slash = strrchr(path, '/');
    size_t length;

    if (slash == NULL) {
        length = 1U;
        if (buffer_size < 2U) {
            die_message("directory buffer is too small");
        }
        memcpy(buffer, ".", length);
        buffer[length] = '\0';
        return;
    }

    length = (size_t)(slash - path);
    if (length == 0U) {
        length = 1U;
    }
    if (length + 1U > buffer_size) {
        die_message("directory buffer is too small");
    }

    memcpy(buffer, path, length);
    buffer[length] = '\0';
}

static void join_path(char *buffer,
                      size_t buffer_size,
                      const char *directory,
                      const char *name) {
    int written;

    written = snprintf(buffer, buffer_size, "%s/%s", directory, name);
    if (written < 0 || (size_t)written >= buffer_size) {
        die_message("path buffer is too small");
    }
}

static int query_nice_value(void) {
    int value;

    errno = 0;
    value = getpriority(PRIO_PROCESS, 0);
    if (value == -1 && errno != 0) {
        die_errno("getpriority");
    }

    return value;
}

static int apply_nice_increment(int increment) {
    int result;

    errno = 0;
    result = nice(increment);
    if (result == -1 && errno != 0) {
        die_errno("nice");
    }

    return result;
}

static struct child_result wait_for_child(pid_t pid) {
    struct child_result result;
    int status;
    pid_t waited;

    memset(&result, 0, sizeof(result));

    do {
        waited = waitpid(pid, &status, 0);
    } while (waited < 0 && errno == EINTR);

    if (waited < 0) {
        die_errno("waitpid");
    }

    if (WIFEXITED(status)) {
        result.exited = true;
        result.code = WEXITSTATUS(status);
    } else if (WIFSIGNALED(status)) {
        result.exited = false;
        result.signal_number = WTERMSIG(status);
    } else {
        die_message("child ended in unexpected wait state");
    }

    return result;
}

static void verify_helper_exists(const char *path) {
    if (access(path, X_OK) != 0) {
        die_errno("access(helper)");
    }
}

static bool run_fork_exec_demo(const char *helper_path, int expected_nice) {
    int marker = FORK_MARKER_INITIAL;
    pid_t child_pid;
    struct child_result child_result;

    child_pid = fork();
    if (child_pid < 0) {
        die_errno("fork");
    }

    if (child_pid == 0) {
        char pre_exec_pid[32];
        char pre_exec_ppid[32];
        char pre_exec_nice[32];
        char expected_nice_text[32];
        char *child_argv[7];
        int child_nice;

        marker = FORK_MARKER_CHILD;
        child_nice = query_nice_value();

        printf("[fork-child-before-exec] pid=%ld ppid=%ld marker_addr=%p marker_value=%d nice=%d target=%s\n",
               (long)getpid(),
               (long)getppid(),
               (void *)&marker,
               marker,
               child_nice,
               helper_path);

        snprintf(pre_exec_pid, sizeof(pre_exec_pid), "%ld", (long)getpid());
        snprintf(pre_exec_ppid, sizeof(pre_exec_ppid), "%ld", (long)getppid());
        snprintf(pre_exec_nice, sizeof(pre_exec_nice), "%d", child_nice);
        snprintf(expected_nice_text, sizeof(expected_nice_text), "%d", expected_nice);

        child_argv[0] = (char *)helper_path;
        child_argv[1] = "exec";
        child_argv[2] = pre_exec_pid;
        child_argv[3] = pre_exec_ppid;
        child_argv[4] = pre_exec_nice;
        child_argv[5] = expected_nice_text;
        child_argv[6] = NULL;

        execve(helper_path, child_argv, environ);
        die_errno("execve");
    }

    marker = FORK_MARKER_PARENT;
    printf("[fork-parent-after-fork] pid=%ld child_pid=%ld marker_addr=%p marker_value=%d nice=%d\n",
           (long)getpid(),
           (long)child_pid,
           (void *)&marker,
           marker,
           query_nice_value());

    child_result = wait_for_child(child_pid);
    if (child_result.exited) {
        printf("[fork-parent-after-wait] child_pid=%ld exit_code=%d\n",
               (long)child_pid,
               child_result.code);
        return child_result.code == 0;
    }

    printf("[fork-parent-after-wait] child_pid=%ld signal=%d\n",
           (long)child_pid,
           child_result.signal_number);
    return false;
}

static bool run_spawn_demo(const char *helper_path, int expected_nice) {
    char expected_parent[32];
    char expected_nice_text[32];
    char *spawn_argv[5];
    pid_t child_pid;
    struct child_result child_result;
    int spawn_error;

    snprintf(expected_parent, sizeof(expected_parent), "%ld", (long)getpid());
    snprintf(expected_nice_text, sizeof(expected_nice_text), "%d", expected_nice);

    spawn_argv[0] = (char *)helper_path;
    spawn_argv[1] = "spawn";
    spawn_argv[2] = expected_parent;
    spawn_argv[3] = expected_nice_text;
    spawn_argv[4] = NULL;

    printf("[spawn-parent-before] pid=%ld nice=%d target=%s\n",
           (long)getpid(),
           query_nice_value(),
           helper_path);

    spawn_error = posix_spawn(&child_pid, helper_path, NULL, NULL, spawn_argv, environ);
    if (spawn_error != 0) {
        die_posix_error("posix_spawn", spawn_error);
    }

    child_result = wait_for_child(child_pid);
    if (child_result.exited) {
        printf("[spawn-parent-after-wait] child_pid=%ld exit_code=%d\n",
               (long)child_pid,
               child_result.code);
        return child_result.code == 0;
    }

    printf("[spawn-parent-after-wait] child_pid=%ld signal=%d\n",
           (long)child_pid,
           child_result.signal_number);
    return false;
}

int main(void) {
    char self_path[PATH_MAX];
    char task_directory[PATH_MAX];
    char helper_path[PATH_MAX];
    int nice_before;
    int nice_return;
    int nice_after;
    bool nice_ok;
    bool fork_exec_ok;
    bool spawn_ok;

    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);

    read_current_exe_path(self_path, sizeof(self_path));
    extract_directory(self_path, task_directory, sizeof(task_directory));
    join_path(helper_path, sizeof(helper_path), task_directory, "process_image_helper");
    verify_helper_exists(helper_path);

    printf("[info] demo=process_management_demo pid=%ld ppid=%ld exe=%s\n",
           (long)getpid(),
           (long)getppid(),
           self_path);

    nice_before = query_nice_value();
    nice_return = apply_nice_increment(NICE_INCREMENT);
    nice_after = query_nice_value();
    nice_ok = (nice_after == nice_before + NICE_INCREMENT) && (nice_return == nice_after);

    printf("[nice] before=%d increment=%d nice_return=%d after=%d\n",
           nice_before,
           NICE_INCREMENT,
           nice_return,
           nice_after);

    fork_exec_ok = run_fork_exec_demo(helper_path, nice_after);
    spawn_ok = run_spawn_demo(helper_path, nice_after);

    printf("[acceptance] nice adjusted current process priority: %s\n",
           nice_ok ? "PASS" : "FAIL");
    printf("[acceptance] fork created a child and exec replaced its image: %s\n",
           fork_exec_ok ? "PASS" : "FAIL");
    printf("[acceptance] posix_spawn created a new process and loaded the helper image: %s\n",
           spawn_ok ? "PASS" : "FAIL");

    if (!(nice_ok && fork_exec_ok && spawn_ok)) {
        printf("[done] process management demo completed with failures\n");
        return EXIT_FAILURE;
    }

    printf("[done] process management demo completed successfully\n");
    return EXIT_SUCCESS;
}
