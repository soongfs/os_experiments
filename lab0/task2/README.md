# LAB0 Task2: 基础系统调用、文件写入与进程控制

## 1. 实验目标

编写一个程序，在休眠约 5 秒后输出指定字符串，并将同样的字符串写入文件，理解进程睡眠、标准输出、文件写入与“数据落盘”之间的关系。

## 2. 源代码

文件：[delayed_write.c](/root/os_experiments/lab0/task2/delayed_write.c)

本实验使用 **覆盖写入** 策略：打开文件时使用 `O_TRUNC`，每次运行都会清空旧内容并写入新的字符串，便于重复实验和结果核对。

```c
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
```

## 3. 使用到的系统调用

- `sleep(5)`：让当前进程阻塞约 5 秒，体现进程控制中的“主动让出 CPU”。
- `write()`：分别向标准输出和普通文件写入字符串。
- `open()`：以 `O_WRONLY | O_CREAT | O_TRUNC` 方式打开目标文件。
- `fsync()`：将文件对应的内核缓冲内容尽量刷新到存储设备，增强“数据落盘”的确定性。
- `close()`：关闭文件描述符，释放内核资源。

说明：

- `sleep()` 是标准库接口，但底层对应内核的进程休眠/定时器机制。
- 本程序的输出和文件写入都使用了 `write()`，属于直接面向文件描述符的系统调用风格。

## 4. 编译与运行

编译命令：

```bash
gcc -g -O0 -Wall -Wextra -o delayed_write delayed_write.c
```

为证明程序确实停顿了约 5 秒，本次使用 shell 内建 `time` 计时运行：

```bash
time ./delayed_write
```

本次实际输出：

```text
LAB0 task2: data written after a 5-second sleep.

real    0m5.023s
user    0m0.003s
sys     0m0.001s
```

从 `real 0m5.023s` 可以看出，程序在输出字符串前确实经历了约 5 秒的暂停。

## 5. 文件内容验证

目标文件：[output.txt](/root/os_experiments/lab0/task2/output.txt)

使用 `cat` 验证：

```bash
cat output.txt
```

本次输出：

```text
LAB0 task2: data written after a 5-second sleep.
```

使用 `hexdump -C` 验证：

```bash
hexdump -C output.txt
```

本次输出：

```text
00000000  4c 41 42 30 20 74 61 73  6b 32 3a 20 64 61 74 61  |LAB0 task2: data|
00000010  20 77 72 69 74 74 65 6e  20 61 66 74 65 72 20 61  | written after a|
00000020  20 35 2d 73 65 63 6f 6e  64 20 73 6c 65 65 70 2e  | 5-second sleep.|
00000030  0a                                                |.|
00000031
```

从十六进制结果可见，文件末尾的 `0a` 对应换行符。

文件属性：

```bash
ls -l output.txt
```

本次输出：

```text
-rw-r--r-- 1 root root 49 Apr  3 14:19 output.txt
```

## 6. “数据落盘”与“进程控制”的关系

1. `sleep(5)` 让进程进入休眠态，这说明进程控制决定了程序何时继续执行写操作。
2. 当进程被重新调度运行后，程序调用 `write()` 把字符串分别写到终端和文件描述符。
3. 对普通文件来说，`write()` 成功通常表示数据已经进入内核缓冲区，但不一定已经真正写到物理存储介质。
4. 为了更明确地体现“落盘”，本实验额外调用了 `fsync()`，要求内核把相关脏页和元数据同步到设备，再继续执行。
5. 因此，进程控制负责“什么时候执行写入”，而数据落盘机制负责“写入的数据什么时候真正持久化到存储设备”。

## 7. 结论

本实验展示了一个完整的最小流程：

- 进程先休眠约 5 秒；
- 恢复运行后，通过 `write()` 向标准输出和文件写入同一字符串；
- 通过 `fsync()` 提高数据持久化的确定性；
- 最后用 `cat` 和 `hexdump` 验证文件内容正确，并确认目标文件已生成。
