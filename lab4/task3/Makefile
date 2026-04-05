CC ?= gcc
CFLAGS ?= -g -O0 -Wall -Wextra -std=c11
BIN := cow_fork_demo

.PHONY: all clean

all: $(BIN)

$(BIN): cow_fork_demo.c
	$(CC) $(CFLAGS) -o $@ $<

clean:
	rm -f $(BIN)
