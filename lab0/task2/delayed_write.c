#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void write_all(int fd, const char *buf, size_t len) {
    while (len > 0) {
        ssize_t written = write(fd, buf, len);
        if (written < 0) {
            perror("write");
            exit(1);
        }
        buf += (size_t)written;
        len -= (size_t)written;
    }
}

int main(void) {
    const char *message = "LAB0 task2: data written after a 5-second sleep.\n";
    const char *output_path = "output.txt";
    size_t message_len = strlen(message);

    sleep(5);

    write_all(STDOUT_FILENO, message, message_len);

    int fd = open(output_path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) {
        perror("open");
        return 1;
    }

    write_all(fd, message, message_len);

    if (fsync(fd) < 0) {
        perror("fsync");
        close(fd);
        return 1;
    }

    if (close(fd) < 0) {
        perror("close");
        return 1;
    }

    return 0;
}
