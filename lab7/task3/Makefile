CC = gcc
CFLAGS = -g -O0 -Wall -Wextra -std=c11
TARGET = tinysh
SRC = tinysh.c

.PHONY: all clean

all: $(TARGET)

$(TARGET): $(SRC)
	$(CC) $(CFLAGS) -o $@ $(SRC)

clean:
	rm -f $(TARGET)
