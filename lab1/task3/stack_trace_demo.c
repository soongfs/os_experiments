#define _GNU_SOURCE

#include <dlfcn.h>
#include <execinfo.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

#define MAX_FRAMES 32

__attribute__((noinline)) void print_stack_trace(void) {
    void *frames[MAX_FRAMES];
    char **symbols;
    int count;
    int i;

    count = backtrace(frames, MAX_FRAMES);
    symbols = backtrace_symbols(frames, count);
    if (symbols == NULL) {
        perror("backtrace_symbols");
        exit(1);
    }

    printf("Captured %d stack frames:\n", count);
    for (i = 0; i < count; ++i) {
        Dl_info info;

        if (dladdr(frames[i], &info) != 0 && info.dli_sname != NULL) {
            ptrdiff_t offset = (const char *)frames[i] - (const char *)info.dli_saddr;
            printf("#%02d %p %s+0x%tx | %s\n", i, frames[i], info.dli_sname, offset, symbols[i]);
        } else {
            printf("#%02d %p %s\n", i, frames[i], symbols[i]);
        }
    }

    free(symbols);
}

__attribute__((noinline)) void level_three(int value) {
    volatile int guard = value + 3;
    print_stack_trace();
    if (guard == -1) {
        puts("unreachable");
    }
}

__attribute__((noinline)) void level_two(int value) {
    volatile int guard = value + 2;
    level_three(guard);
    if (guard == -1) {
        puts("unreachable");
    }
}

__attribute__((noinline)) void level_one(int value) {
    volatile int guard = value + 1;
    level_two(guard);
    if (guard == -1) {
        puts("unreachable");
    }
}

int main(void) {
    puts("Starting stack trace demo...");
    level_one(0);
    return 0;
}
