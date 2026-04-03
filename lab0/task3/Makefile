CC = gcc
CFLAGS = -g -O0 -Wall -Wextra -pthread
TARGET = task3_demo
OBJS = main.o shared_state.o concurrent.o persistence.o

all: $(TARGET)

$(TARGET): $(OBJS)
	$(CC) $(CFLAGS) -o $@ $(OBJS)

main.o: main.c concurrent.h persistence.h shared_state.h
shared_state.o: shared_state.c shared_state.h
concurrent.o: concurrent.c concurrent.h shared_state.h
persistence.o: persistence.c persistence.h shared_state.h

clean:
	rm -f $(TARGET) $(OBJS)

.PHONY: all clean
