CC = gcc
CFLAGS = -g -O0 -Wall -Wextra -std=c11
TARGET = ipc_matrix_demo
SRC = ipc_matrix_demo.c

.PHONY: all clean

all: $(TARGET)

$(TARGET): $(SRC)
	$(CC) $(CFLAGS) -o $@ $(SRC)

clean:
	rm -f $(TARGET)
