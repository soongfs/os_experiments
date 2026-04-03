#include <stdio.h>

int main(void) {
    puts("LAB0 task1: about to write through a null pointer.");
    fflush(stdout);

    volatile int *ptr = (int *)0;
    *ptr = 42;

    return 0;
}
