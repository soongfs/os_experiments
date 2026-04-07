#define _POSIX_C_SOURCE 200809L

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#define MAX_COMMANDS 16
#define MAX_TOKENS 64

struct command {
    char **argv;
    size_t argc;
    char *infile;
    char *outfile;
};

static void fatal_errno(const char *context) {
    fprintf(stderr, "[error] %s: %s\n", context, strerror(errno));
    exit(EXIT_FAILURE);
}

static void trim_whitespace(char *s) {
    char *start = s;
    while (*start && (*start == ' ' || *start == '\t')) {
        ++start;
    }
    memmove(s, start, strlen(start) + 1);
    size_t len = strlen(s);
    while (len > 0 && (s[len - 1] == ' ' || s[len - 1] == '\t')) {
        s[--len] = '\0';
    }
}

static char *copy_string(const char *src) {
    char *dst = strdup(src);
    if (!dst) {
        fatal_errno("strdup");
    }
    return dst;
}

static void free_command(struct command *cmd) {
    for (size_t i = 0; i < cmd->argc; ++i) {
        free(cmd->argv[i]);
    }
    free(cmd->argv);
    free(cmd->infile);
    free(cmd->outfile);
    memset(cmd, 0, sizeof(*cmd));
}

static size_t count_fds(void) {
    DIR *dir = opendir("/proc/self/fd");
    if (!dir) {
        return 0;
    }
    size_t count = 0;
    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        ++count;
    }
    closedir(dir);
    return count;
}

static int parse_segment(char *segment, struct command *cmd) {
    trim_whitespace(segment);
    if (*segment == '\0') {
        return -1;
    }

    char *save = segment;
    char *token;
    while ((token = strtok_r(save, " \t", &save)) != NULL) {
        if (strcmp(token, "<") == 0) {
            token = strtok_r(save, " \t", &save);
            if (!token) {
                fprintf(stderr, "[parse] stray '<'\n");
                return -1;
            }
            cmd->infile = copy_string(token);
            continue;
        }
        if (strcmp(token, ">") == 0) {
            token = strtok_r(save, " \t", &save);
            if (!token) {
                fprintf(stderr, "[parse] stray '>'\n");
                return -1;
            }
            cmd->outfile = copy_string(token);
            continue;
        }
        if (cmd->argc >= MAX_TOKENS - 1) {
            fprintf(stderr, "[parse] too many tokens\n");
            return -1;
        }
        cmd->argv = realloc(cmd->argv, sizeof(char *) * (cmd->argc + 1));
        if (!cmd->argv) {
            fatal_errno("realloc");
        }
        cmd->argv[cmd->argc++] = copy_string(token);
    }
    if (cmd->argc == 0) {
        return -1;
    }
    cmd->argv = realloc(cmd->argv, sizeof(char *) * (cmd->argc + 1));
    if (!cmd->argv) {
        fatal_errno("realloc");
    }
    cmd->argv[cmd->argc] = NULL;
    return 0;
}

static int parse_pipeline(const char *line, struct command **out_cmds, size_t *out_count) {
    char *copy = strdup(line);
    if (!copy) {
        fatal_errno("strdup");
    }
    struct command *cmds = calloc(MAX_COMMANDS, sizeof(struct command));
    if (!cmds) {
        fatal_errno("calloc");
    }

    size_t count = 0;
    char *save;
    char *segment = strtok_r(copy, "|", &save);
    while (segment) {
        if (count >= MAX_COMMANDS) {
            fprintf(stderr, "[parse] too many pipeline commands (max %d)\n", MAX_COMMANDS);
            break;
        }
        if (parse_segment(segment, &cmds[count]) == 0) {
            ++count;
        } else {
            for (size_t i = 0; i < count; ++i) {
                free_command(&cmds[i]);
            }
            free(cmds);
            free(copy);
            return -1;
        }
        segment = strtok_r(NULL, "|", &save);
    }

    free(copy);
    if (count == 0) {
        free(cmds);
        return -1;
    }
    *out_cmds = cmds;
    *out_count = count;
    return 0;
}

static void close_if_open(int fd) {
    if (fd >= 0) {
        close(fd);
    }
}

static void run_pipeline(struct command *cmds, size_t cmd_count) {
    size_t initial_fds = count_fds();
    pid_t *children = calloc(cmd_count, sizeof(pid_t));
    if (!children) {
        fatal_errno("calloc");
    }

    int prev_read = -1;
    for (size_t idx = 0; idx < cmd_count; ++idx) {
        int pipefd[2] = {-1, -1};
        if (idx + 1 < cmd_count) {
            if (pipe(pipefd) != 0) {
                fatal_errno("pipe");
            }
        }

        pid_t pid = fork();
        if (pid < 0) {
            fatal_errno("fork");
        }

        if (pid == 0) {
            if (prev_read != -1) {
                dup2(prev_read, STDIN_FILENO);
            }
            if (idx + 1 < cmd_count) {
                dup2(pipefd[1], STDOUT_FILENO);
            }

            close_if_open(pipefd[0]);
            close_if_open(pipefd[1]);
            close_if_open(prev_read);

            if (cmds[idx].infile) {
                int fd = open(cmds[idx].infile, O_RDONLY);
                if (fd < 0) {
                    fprintf(stderr, "[exec] cannot open %s: %s\n", cmds[idx].infile, strerror(errno));
                    _exit(EXIT_FAILURE);
                }
                dup2(fd, STDIN_FILENO);
                close(fd);
            }
            if (cmds[idx].outfile) {
                int fd = open(cmds[idx].outfile, O_CREAT | O_TRUNC | O_WRONLY, 0644);
                if (fd < 0) {
                    fprintf(stderr, "[exec] cannot open %s: %s\n", cmds[idx].outfile, strerror(errno));
                    _exit(EXIT_FAILURE);
                }
                dup2(fd, STDOUT_FILENO);
                close(fd);
            }

            execvp(cmds[idx].argv[0], cmds[idx].argv);
            fprintf(stderr, "[exec] %s: %s\n", cmds[idx].argv[0], strerror(errno));
            _exit(EXIT_FAILURE);
        }

        children[idx] = pid;
        close_if_open(prev_read);
        close_if_open(pipefd[1]);
        prev_read = (idx + 1 < cmd_count) ? pipefd[0] : -1;
    }

    if (prev_read != -1) {
        close(prev_read);
    }

    int status = 0;
    for (size_t i = 0; i < cmd_count; ++i) {
        int child_status = 0;
        waitpid(children[i], &child_status, 0);
        if (WIFEXITED(child_status)) {
            status |= WEXITSTATUS(child_status);
        } else {
            status = -1;
        }
    }

    size_t final_fds = count_fds();
    printf("[summary] segments=%zu initial_fds=%zu final_fds=%zu\n", cmd_count, initial_fds, final_fds);
    if (final_fds != initial_fds) {
        printf("[warning] fd count mismatch, check that all pipes were closed\n");
    }
    printf("[acceptance] fork-exec pipeline succeeded segments=%zu status=%d\n", cmd_count, status);

    free(children);
}

int main(int argc, char *argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s \"cmd1 | cmd2 [| cmd3 ...]\"\n", argv[0]);
        return EXIT_FAILURE;
    }

    struct command *cmds = NULL;
    size_t cmd_count = 0;
    if (parse_pipeline(argv[1], &cmds, &cmd_count) != 0) {
        fprintf(stderr, "[main] failed to parse pipeline\n");
        return EXIT_FAILURE;
    }

    printf("[config] pipeline=\"%s\" segments=%zu\n", argv[1], cmd_count);
    run_pipeline(cmds, cmd_count);

    for (size_t i = 0; i < cmd_count; ++i) {
        free_command(&cmds[i]);
    }
    free(cmds);
    return EXIT_SUCCESS;
}
