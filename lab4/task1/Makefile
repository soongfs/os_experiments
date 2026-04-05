CC ?= gcc
CFLAGS ?= -g -O0 -Wall -Wextra -std=c11
BIN := memory_syscall_demo

.PHONY: all clean

all: $(BIN)

$(BIN): memory_syscall_demo.c
	$(CC) $(CFLAGS) -o $@ $<

clean:
	rm -f $(BIN)
