CC ?= gcc
CFLAGS ?= -g -O0 -Wall -Wextra -std=c11
BIN := fork_logic_counter

.PHONY: all clean

all: $(BIN)

$(BIN): fork_logic_counter.c
	$(CC) $(CFLAGS) -o $@ $<

clean:
	rm -f $(BIN)
