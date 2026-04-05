#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

enum operator_kind {
    OP_AND = 1,
    OP_OR = 2,
};

enum node_kind {
    NODE_FORK = 1,
    NODE_AND = 2,
    NODE_OR = 3,
};

enum {
    VERIFY_MAX_PROCESSES = 64,
};

struct expr_node {
    enum node_kind kind;
    size_t leaf_index;
    struct expr_node *left;
    struct expr_node *right;
    uint64_t true_count;
    uint64_t false_count;
};

struct parser {
    const enum operator_kind *ops;
    size_t op_count;
    size_t index;
    size_t next_leaf_index;
};

struct theory_summary {
    struct expr_node *root;
    enum operator_kind *ops;
    size_t operator_count;
    size_t operand_count;
    uint64_t *fork_eval_counts;
    char *expression_text;
    uint64_t total_processes;
    uint64_t child_processes;
    uint64_t printf_outputs;
    uint64_t true_results;
    uint64_t false_results;
};

struct leaf_record {
    int expr_result;
};

struct verify_context {
    int report_read_fd;
    int report_write_fd;
    pid_t direct_children[64];
    size_t direct_child_count;
    bool is_root;
    uint32_t forks_executed;
};

struct verify_summary {
    uint64_t total_records;
    uint64_t true_records;
    uint64_t false_records;
};

static void die_message(const char *message) {
    fprintf(stderr, "%s\n", message);
    exit(EXIT_FAILURE);
}

static void die_errno(const char *context) {
    fprintf(stderr, "%s: %s\n", context, strerror(errno));
    exit(EXIT_FAILURE);
}

static void die_parse(const char *context, const char *text) {
    fprintf(stderr, "%s: %s\n", context, text);
    exit(EXIT_FAILURE);
}

static uint64_t checked_add_u64(uint64_t left, uint64_t right) {
    uint64_t result;

    if (__builtin_add_overflow(left, right, &result)) {
        die_message("u64 addition overflow");
    }

    return result;
}

static uint64_t checked_mul_u64(uint64_t left, uint64_t right) {
    uint64_t result;

    if (__builtin_mul_overflow(left, right, &result)) {
        die_message("u64 multiplication overflow");
    }

    return result;
}

static bool is_space_char(char ch) {
    return ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r';
}

static struct expr_node *new_node(enum node_kind kind,
                                  size_t leaf_index,
                                  struct expr_node *left,
                                  struct expr_node *right) {
    struct expr_node *node = calloc(1, sizeof(*node));

    if (node == NULL) {
        die_errno("calloc(expr_node)");
    }

    node->kind = kind;
    node->leaf_index = leaf_index;
    node->left = left;
    node->right = right;
    return node;
}

static struct expr_node *parse_leaf(struct parser *parser) {
    return new_node(NODE_FORK, parser->next_leaf_index++, NULL, NULL);
}

static struct expr_node *parse_and_expression(struct parser *parser) {
    struct expr_node *node = parse_leaf(parser);

    while (parser->index < parser->op_count && parser->ops[parser->index] == OP_AND) {
        struct expr_node *right;

        parser->index += 1U;
        right = parse_leaf(parser);
        node = new_node(NODE_AND, 0U, node, right);
    }

    return node;
}

static struct expr_node *parse_or_expression(struct parser *parser) {
    struct expr_node *node = parse_and_expression(parser);

    while (parser->index < parser->op_count && parser->ops[parser->index] == OP_OR) {
        struct expr_node *right;

        parser->index += 1U;
        right = parse_and_expression(parser);
        node = new_node(NODE_OR, 0U, node, right);
    }

    return node;
}

static void free_tree(struct expr_node *node) {
    if (node == NULL) {
        return;
    }

    free_tree(node->left);
    free_tree(node->right);
    free(node);
}

static enum operator_kind *parse_operator_sequence(const char *text, size_t *count_out) {
    const size_t length = strlen(text);
    enum operator_kind *ops = calloc(length == 0U ? 1U : length, sizeof(*ops));
    size_t count = 0U;
    size_t index = 0U;

    if (ops == NULL) {
        die_errno("calloc(ops)");
    }

    while (index < length) {
        if (is_space_char(text[index])) {
            index += 1U;
            continue;
        }

        if (index + 1U >= length) {
            free(ops);
            die_parse("invalid operator sequence", text);
        }

        if (text[index] == '&' && text[index + 1U] == '&') {
            ops[count++] = OP_AND;
            index += 2U;
            continue;
        }

        if (text[index] == '|' && text[index + 1U] == '|') {
            ops[count++] = OP_OR;
            index += 2U;
            continue;
        }

        free(ops);
        die_parse("invalid operator sequence", text);
    }

    *count_out = count;
    return ops;
}

static char *build_expression_text(const enum operator_kind *ops, size_t op_count) {
    size_t size = 16U + op_count * 13U;
    char *buffer = calloc(size, sizeof(*buffer));
    size_t used = 0U;
    size_t index;

    if (buffer == NULL) {
        die_errno("calloc(expression_text)");
    }

    used = (size_t)snprintf(buffer, size, "fork()");
    if (used >= size) {
        die_message("expression buffer is too small");
    }

    for (index = 0U; index < op_count; ++index) {
        const char *symbol = ops[index] == OP_AND ? " && fork()" : " || fork()";
        int written = snprintf(buffer + used, size - used, "%s", symbol);

        if (written < 0 || (size_t)written >= size - used) {
            die_message("expression buffer is too small");
        }
        used += (size_t)written;
    }

    return buffer;
}

static void compute_counts(struct expr_node *node) {
    if (node->kind == NODE_FORK) {
        node->true_count = 1U;
        node->false_count = 1U;
        return;
    }

    compute_counts(node->left);
    compute_counts(node->right);

    if (node->kind == NODE_AND) {
        node->true_count = checked_mul_u64(node->left->true_count, node->right->true_count);
        node->false_count = checked_add_u64(
            node->left->false_count,
            checked_mul_u64(node->left->true_count, node->right->false_count));
        return;
    }

    node->true_count = checked_add_u64(
        node->left->true_count,
        checked_mul_u64(node->left->false_count, node->right->true_count));
    node->false_count = checked_mul_u64(node->left->false_count, node->right->false_count);
}

static void collect_eval_counts(const struct expr_node *node,
                                uint64_t incoming_processes,
                                uint64_t *fork_eval_counts) {
    if (node->kind == NODE_FORK) {
        fork_eval_counts[node->leaf_index] =
            checked_add_u64(fork_eval_counts[node->leaf_index], incoming_processes);
        return;
    }

    collect_eval_counts(node->left, incoming_processes, fork_eval_counts);

    if (node->kind == NODE_AND) {
        collect_eval_counts(node->right,
                            checked_mul_u64(incoming_processes, node->left->true_count),
                            fork_eval_counts);
        return;
    }

    collect_eval_counts(node->right,
                        checked_mul_u64(incoming_processes, node->left->false_count),
                        fork_eval_counts);
}

static struct theory_summary build_theory_summary(const char *operator_text) {
    struct theory_summary summary;
    struct parser parser;

    memset(&summary, 0, sizeof(summary));
    summary.ops = parse_operator_sequence(operator_text, &summary.operator_count);
    summary.expression_text = build_expression_text(summary.ops, summary.operator_count);

    parser.ops = summary.ops;
    parser.op_count = summary.operator_count;
    parser.index = 0U;
    parser.next_leaf_index = 0U;

    summary.root = parse_or_expression(&parser);
    if (parser.index != parser.op_count) {
        free_tree(summary.root);
        free(summary.ops);
        free(summary.expression_text);
        die_message("parser did not consume the full operator sequence");
    }

    summary.operand_count = parser.next_leaf_index;
    summary.fork_eval_counts = calloc(summary.operand_count, sizeof(*summary.fork_eval_counts));
    if (summary.fork_eval_counts == NULL) {
        free_tree(summary.root);
        free(summary.ops);
        free(summary.expression_text);
        die_errno("calloc(fork_eval_counts)");
    }

    compute_counts(summary.root);
    collect_eval_counts(summary.root, 1U, summary.fork_eval_counts);

    summary.true_results = summary.root->true_count;
    summary.false_results = summary.root->false_count;
    summary.total_processes = checked_add_u64(summary.true_results, summary.false_results);
    summary.child_processes = summary.total_processes - 1U;
    summary.printf_outputs = summary.total_processes;

    return summary;
}

static void free_theory_summary(struct theory_summary *summary) {
    if (summary == NULL) {
        return;
    }

    free_tree(summary->root);
    free(summary->ops);
    free(summary->fork_eval_counts);
    free(summary->expression_text);
    memset(summary, 0, sizeof(*summary));
}

static void print_theory_summary(const struct theory_summary *summary) {
    size_t index;

    printf("[theory] expression=%s\n", summary->expression_text);
    printf("[theory] operands=%zu operators=%zu total_processes=%" PRIu64
           " child_processes=%" PRIu64 " printf_outputs=%" PRIu64
           " true_results=%" PRIu64 " false_results=%" PRIu64 "\n",
           summary->operand_count,
           summary->operator_count,
           summary->total_processes,
           summary->child_processes,
           summary->printf_outputs,
           summary->true_results,
           summary->false_results);

    for (index = 0U; index < summary->operand_count; ++index) {
        printf("[theory] fork#%zu evaluated_by=%" PRIu64 " process(es)\n",
               index + 1U,
               summary->fork_eval_counts[index]);
    }
}

static void write_full(int fd, const void *buffer, size_t length) {
    const unsigned char *cursor = buffer;

    while (length > 0U) {
        ssize_t written = write(fd, cursor, length);

        if (written < 0) {
            if (errno == EINTR) {
                continue;
            }
            die_errno("write");
        }

        if (written == 0) {
            die_message("write returned zero");
        }

        cursor += (size_t)written;
        length -= (size_t)written;
    }
}

static bool read_record(int fd, struct leaf_record *record, bool *saw_eof) {
    unsigned char *cursor = (unsigned char *)record;
    size_t remaining = sizeof(*record);

    *saw_eof = false;

    while (remaining > 0U) {
        ssize_t got = read(fd, cursor, remaining);

        if (got < 0) {
            if (errno == EINTR) {
                continue;
            }
            die_errno("read");
        }

        if (got == 0) {
            if (remaining == sizeof(*record)) {
                *saw_eof = true;
                return false;
            }
            die_message("truncated leaf_record on pipe");
        }

        cursor += (size_t)got;
        remaining -= (size_t)got;
    }

    return true;
}

static bool wait_for_direct_children(struct verify_context *context) {
    size_t index;
    bool all_ok = true;

    for (index = 0U; index < context->direct_child_count; ++index) {
        int status;
        pid_t waited;

        do {
            waited = waitpid(context->direct_children[index], &status, 0);
        } while (waited < 0 && errno == EINTR);

        if (waited < 0) {
            die_errno("waitpid");
        }

        if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
            all_ok = false;
        }
    }

    return all_ok;
}

static bool execute_fork_leaf(struct verify_context *context) {
    pid_t pid;

    context->forks_executed += 1U;
    pid = fork();
    if (pid < 0) {
        die_errno("fork");
    }

    if (pid == 0) {
        context->is_root = false;
        context->direct_child_count = 0U;
        return false;
    }

    if (context->direct_child_count >= sizeof(context->direct_children) / sizeof(context->direct_children[0])) {
        die_message("too many direct children for verification context");
    }

    context->direct_children[context->direct_child_count++] = pid;
    return true;
}

static bool evaluate_expression_runtime(const struct expr_node *node, struct verify_context *context) {
    bool left_result;

    if (node->kind == NODE_FORK) {
        return execute_fork_leaf(context);
    }

    left_result = evaluate_expression_runtime(node->left, context);

    if (node->kind == NODE_AND) {
        if (!left_result) {
            return false;
        }
        return evaluate_expression_runtime(node->right, context);
    }

    if (left_result) {
        return true;
    }
    return evaluate_expression_runtime(node->right, context);
}

static struct verify_summary collect_verify_summary(int read_fd) {
    struct verify_summary summary;
    bool saw_eof = false;

    memset(&summary, 0, sizeof(summary));

    while (!saw_eof) {
        struct leaf_record record;
        bool have_record = read_record(read_fd, &record, &saw_eof);

        if (!have_record) {
            continue;
        }

        summary.total_records += 1U;
        if (record.expr_result != 0) {
            summary.true_records += 1U;
        } else {
            summary.false_records += 1U;
        }
    }

    return summary;
}

static int verify_expression(const struct theory_summary *theory) {
    int pipe_fds[2];
    struct verify_context context;
    bool expr_result;
    struct leaf_record record;
    bool child_tree_ok;

    if (theory->total_processes > VERIFY_MAX_PROCESSES) {
        fprintf(stderr,
                "verification refused: predicted total_processes=%" PRIu64 " exceeds limit=%d\n",
                theory->total_processes,
                VERIFY_MAX_PROCESSES);
        return EXIT_FAILURE;
    }

    if (pipe(pipe_fds) != 0) {
        die_errno("pipe");
    }

    memset(&context, 0, sizeof(context));
    context.report_read_fd = pipe_fds[0];
    context.report_write_fd = pipe_fds[1];
    context.is_root = true;
    context.forks_executed = 0U;

    printf("[verify] expression=%s\n", theory->expression_text);
    printf("[verify] predicted_total_processes=%" PRIu64
           " predicted_child_processes=%" PRIu64
           " predicted_printf_outputs=%" PRIu64
           " predicted_true_results=%" PRIu64
           " predicted_false_results=%" PRIu64 "\n",
           theory->total_processes,
           theory->child_processes,
           theory->printf_outputs,
           theory->true_results,
           theory->false_results);

    expr_result = evaluate_expression_runtime(theory->root, &context);

    if (!context.is_root) {
        close(context.report_read_fd);
    }

    printf("[leaf] expression=%s pid=%ld ppid=%ld result=%s forks_executed_on_path=%u\n",
           theory->expression_text,
           (long)getpid(),
           (long)getppid(),
           expr_result ? "true" : "false",
           context.forks_executed);

    record.expr_result = expr_result ? 1 : 0;
    write_full(context.report_write_fd, &record, sizeof(record));
    close(context.report_write_fd);

    child_tree_ok = wait_for_direct_children(&context);

    if (!context.is_root) {
        if (!child_tree_ok) {
            _exit(EXIT_FAILURE);
        }
        _exit(EXIT_SUCCESS);
    }

    {
        struct verify_summary actual = collect_verify_summary(context.report_read_fd);
        bool counts_match = child_tree_ok
            && actual.total_records == theory->total_processes
            && actual.total_records - 1U == theory->child_processes
            && actual.total_records == theory->printf_outputs
            && actual.true_records == theory->true_results
            && actual.false_records == theory->false_results;

        close(context.report_read_fd);

        printf("[verify] actual_total_processes=%" PRIu64
               " actual_child_processes=%" PRIu64
               " actual_printf_outputs=%" PRIu64
               " actual_true_results=%" PRIu64
               " actual_false_results=%" PRIu64 "\n",
               actual.total_records,
               actual.total_records - 1U,
               actual.total_records,
               actual.true_records,
               actual.false_records);
        printf("[acceptance] theory matches runtime process/printf counts: %s\n",
               counts_match ? "PASS" : "FAIL");

        return counts_match ? EXIT_SUCCESS : EXIT_FAILURE;
    }
}

static int run_case(const char *operator_text, bool do_verify) {
    struct theory_summary theory = build_theory_summary(operator_text);
    int status = EXIT_SUCCESS;

    print_theory_summary(&theory);
    if (do_verify) {
        status = verify_expression(&theory);
    }

    free_theory_summary(&theory);
    return status;
}

static int run_demo(void) {
    static const char *const cases[] = {
        "&&",
        "||",
        "&& ||",
        "|| &&",
        "&& || &&",
    };
    size_t index;

    for (index = 0U; index < sizeof(cases) / sizeof(cases[0]); ++index) {
        int status;

        printf("[demo] case=%zu ops=\"%s\"\n", index + 1U, cases[index]);
        status = run_case(cases[index], true);
        if (status != EXIT_SUCCESS) {
            return status;
        }
    }

    printf("[done] demo suite completed successfully\n");
    return EXIT_SUCCESS;
}

static void print_usage(const char *program_name) {
    fprintf(stderr,
            "usage:\n"
            "  %s analyze '<&&/|| sequence>'\n"
            "  %s verify  '<&&/|| sequence>'\n"
            "  %s demo\n",
            program_name,
            program_name,
            program_name);
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);

    if (argc < 2) {
        print_usage(argv[0]);
        return EXIT_FAILURE;
    }

    if (strcmp(argv[1], "analyze") == 0) {
        if (argc != 3) {
            print_usage(argv[0]);
            return EXIT_FAILURE;
        }
        return run_case(argv[2], false);
    }

    if (strcmp(argv[1], "verify") == 0) {
        if (argc != 3) {
            print_usage(argv[0]);
            return EXIT_FAILURE;
        }
        return run_case(argv[2], true);
    }

    if (strcmp(argv[1], "demo") == 0) {
        if (argc != 2) {
            print_usage(argv[0]);
            return EXIT_FAILURE;
        }
        return run_demo();
    }

    print_usage(argv[0]);
    return EXIT_FAILURE;
}
