CC ?= gcc
CFLAGS ?= -g -O0 -Wall -Wextra -std=c11
MAIN_BIN := process_management_demo
HELPER_BIN := process_image_helper

.PHONY: all clean

all: $(MAIN_BIN) $(HELPER_BIN)

$(MAIN_BIN): process_management_demo.c
	$(CC) $(CFLAGS) -o $@ $<

$(HELPER_BIN): process_image_helper.c
	$(CC) $(CFLAGS) -o $@ $<

clean:
	rm -f $(MAIN_BIN) $(HELPER_BIN)
