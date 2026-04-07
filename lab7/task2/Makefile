CC = gcc
CFLAGS = -g -O0 -Wall -Wextra -std=c11
TARGET = signal_async_demo
SRC = signal_async_demo.c

.PHONY: all clean

all: $(TARGET)

$(TARGET): $(SRC)
	$(CC) $(CFLAGS) -o $@ $(SRC)

clean:
	rm -f $(TARGET)
