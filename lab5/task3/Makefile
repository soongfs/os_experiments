CC ?= gcc
CFLAGS ?= -O2 -g -Wall -Wextra -std=c11 -pthread
BIN := parallel_benchmark

.PHONY: all clean

all: $(BIN)

$(BIN): parallel_benchmark.c
	$(CC) $(CFLAGS) -o $@ $<

clean:
	rm -f $(BIN)
