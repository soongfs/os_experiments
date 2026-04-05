CC ?= gcc
CFLAGS ?= -g -O0 -Wall -Wextra -std=c11
BIN := lazy_swap_trigger

.PHONY: all clean

all: $(BIN)

$(BIN): lazy_swap_trigger.c
	$(CC) $(CFLAGS) -o $@ $<

clean:
	rm -f $(BIN)
