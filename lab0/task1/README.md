# LAB0 Task1: 用户态异常、Trap 与信号

## 1. 实验目标

编写一个在 Linux 用户态必然触发异常的程序，观察终端输出，并理解异常从 CPU 陷入内核，再由内核转换为信号并终止进程的处理过程。

## 2. 触发异常的源代码

文件：[null_deref.c](/root/os_experiments/lab0/task1/null_deref.c)

该程序在打印提示信息后，主动向空指针地址 `0x0` 写入数据，触发非法内存访问。

```c
#include <stdio.h>

int main(void) {
    puts("LAB0 task1: about to write through a null pointer.");
    fflush(stdout);

    volatile int *ptr = (int *)0;
    *ptr = 42;

    return 0;
}
```

## 3. 编译与运行

编译命令：

```bash
gcc -g -O0 -Wall -Wextra -o null_deref null_deref.c
```

运行命令：

```bash
./null_deref
```

本次在当前 Linux 环境中的实际终端输出：

```text
$ ./null_deref
LAB0 task1: about to write through a null pointer.

$ echo $?
139
```

说明：

- 退出码 `139 = 128 + 11`，其中 `11` 对应 `SIGSEGV`。
- 在普通交互式 shell 中，很多情况下还会看到类似 `Segmentation fault` 或 `Segmentation fault (core dumped)` 的提示；本次抓取到的 PTY 输出没有额外打印该提示，但退出码已经明确表明进程因 `SIGSEGV` 终止。

## 4. Trap 与信号处理机制说明

1. 用户态程序执行 `*ptr = 42` 时，CPU 发现访问地址 `0x0` 对当前进程来说不是一个合法可写页，于是产生同步异常。对这类非法访存，常见硬件表现为 page fault 等异常。
2. 异常发生后，CPU 会自动从用户态切换到内核态，保存当前关键寄存器状态，并按照异常向量跳转到内核的异常入口。这个由硬件触发、把控制流从用户态转交给内核的过程就是 trap。
3. Linux 内核拿到异常现场后，会检查异常来源是否来自用户态、异常地址是什么、访问类型是读还是写、该地址是否属于进程合法映射等。空指针写入通常会被判定为用户态非法内存访问。
4. 对这种用户态非法访存，内核不会让 CPU 直接继续执行原指令，而是把该硬件异常转换为对当前进程的 `SIGSEGV`。更具体地说，内核会为该线程/进程记录待处理信号及附加信息，例如出错地址。
5. 当内核准备从异常处理路径返回用户态时，会检查是否存在待处理且未屏蔽的信号。若程序注册了 `SIGSEGV` 处理函数，内核会按信号机制转入该处理函数；若没有注册，则执行默认动作。
6. `SIGSEGV` 的默认动作是终止进程，并且在系统配置允许时生成 core dump。因此 shell 最终观察到的是子进程被信号 11 杀死，常见地显示为 `Segmentation fault`，或者仅体现为退出码 `139`。

## 5. 结论

本实验说明了两层机制的分工：

- trap 是 CPU 发现异常后进入内核的硬件控制流转移；
- signal 是内核把异常结果抽象成进程语义后，交付给用户进程的软件机制。

因此，用户态程序的非法操作并不是“直接打印报错”，而是先由硬件触发异常、内核完成处理，再以信号形式终止对应进程。
