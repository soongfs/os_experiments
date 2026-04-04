# LAB1 Task3: 调用栈链信息打印

## 1. 原始任务说明

### 任务标题

调用栈链信息打印

### 任务目标

理解函数调用栈、栈帧布局与调试信息的关系，掌握一种可复现的调用栈获取方式。

### 任务要求

1. 实现 Linux 应用程序，在多层函数调用下打印调用栈；
2. 可选实现路径：
   - 使用 `backtrace()`（glibc）或类似库机制；
   - 或使用编译器/调试工具链（如 `addr2line`）将地址解析为符号信息；
3. 要求在实验记录中说明：调用栈信息依赖哪些编译选项（如 `-g`、`-fno-omit-frame-pointer` 等）。

### 验收检查

1. 程序成功输出至少 3 层以上的函数调用回溯（包含函数名或地址）；
2. 报告中详细解释 `-fno-omit-frame-pointer` 或 DWARF 调试信息在栈回溯中的作用。

## 2. 实验目标与实现思路

本实验使用 glibc 提供的 `backtrace()` 和 `backtrace_symbols()` 获取调用栈。程序构造了 `main -> level_one -> level_two -> level_three -> print_stack_trace` 的多层函数调用链，在最深层收集并打印回溯信息。

为了让回溯结果更适合复现与分析，本实验编译时显式使用：

- `-g`：生成 DWARF 调试信息；
- `-O0`：避免优化带来的内联和调用链折叠；
- `-fno-omit-frame-pointer`：保留栈帧指针链；
- `-rdynamic`：导出主程序符号，便于 `backtrace_symbols()` 打印函数名；
- `-no-pie`：关闭 PIE，便于把运行时地址直接交给 `addr2line` 解析。

## 3. 文件列表

- [stack_trace_demo.c](/root/os_experiments/lab1/task3/stack_trace_demo.c)：调用栈回溯实验程序。
- [README.md](/root/os_experiments/lab1/task3/README.md)：实验说明与验收记录。

## 4. 源代码与说明

文件：[stack_trace_demo.c](/root/os_experiments/lab1/task3/stack_trace_demo.c)

```c
#define _GNU_SOURCE

#include <dlfcn.h>
#include <execinfo.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

#define MAX_FRAMES 32

__attribute__((noinline)) void print_stack_trace(void) {
    void *frames[MAX_FRAMES];
    char **symbols;
    int count;
    int i;

    count = backtrace(frames, MAX_FRAMES);
    symbols = backtrace_symbols(frames, count);
    if (symbols == NULL) {
        perror("backtrace_symbols");
        exit(1);
    }

    printf("Captured %d stack frames:\n", count);
    for (i = 0; i < count; ++i) {
        Dl_info info;

        if (dladdr(frames[i], &info) != 0 && info.dli_sname != NULL) {
            ptrdiff_t offset = (const char *)frames[i] - (const char *)info.dli_saddr;
            printf("#%02d %p %s+0x%tx | %s\n", i, frames[i], info.dli_sname, offset, symbols[i]);
        } else {
            printf("#%02d %p %s\n", i, frames[i], symbols[i]);
        }
    }

    free(symbols);
}

__attribute__((noinline)) void level_three(int value) {
    volatile int guard = value + 3;
    print_stack_trace();
    if (guard == -1) {
        puts("unreachable");
    }
}

__attribute__((noinline)) void level_two(int value) {
    volatile int guard = value + 2;
    level_three(guard);
    if (guard == -1) {
        puts("unreachable");
    }
}

__attribute__((noinline)) void level_one(int value) {
    volatile int guard = value + 1;
    level_two(guard);
    if (guard == -1) {
        puts("unreachable");
    }
}

int main(void) {
    puts("Starting stack trace demo...");
    level_one(0);
    return 0;
}
```

代码要点：

- `backtrace()`：收集当前线程的返回地址链；
- `backtrace_symbols()`：把地址转换成可读字符串；
- `dladdr()`：进一步把地址解析为当前可执行文件中的符号名；
- `__attribute__((noinline))`：避免关键函数被内联，保持调用链清晰；
- `volatile guard`：减少编译器把函数尾调用优化掉的机会。

## 5. 编译、运行与复现方法

进入任务目录：

```bash
cd /root/os_experiments/lab1/task3
```

编译：

```bash
gcc -g -O0 -fno-omit-frame-pointer -rdynamic -no-pie -Wall -Wextra -o stack_trace_demo stack_trace_demo.c -ldl
```

运行：

```bash
./stack_trace_demo
```

使用 `addr2line` 解析回溯地址：

```bash
addr2line -f -C -e ./stack_trace_demo 0x4011b5 0x401324 0x40135c 0x401394 0x4013cb
```

## 6. 本次实际运行结果

程序运行的实际输出：

```text
Starting stack trace demo...
Captured 8 stack frames:
#00 0x4011b5 print_stack_trace+0x1f | ./stack_trace_demo(print_stack_trace+0x1f) [0x4011b5]
#01 0x401324 level_three+0x19 | ./stack_trace_demo(level_three+0x19) [0x401324]
#02 0x40135c level_two+0x1e | ./stack_trace_demo(level_two+0x1e) [0x40135c]
#03 0x401394 level_one+0x1e | ./stack_trace_demo(level_one+0x1e) [0x401394]
#04 0x4013cb main+0x1d | ./stack_trace_demo(main+0x1d) [0x4013cb]
#05 0x747884a0fca8 /lib/x86_64-linux-gnu/libc.so.6(+0x29ca8) [0x747884a0fca8]
#06 0x747884a0fd65 __libc_start_main+0x85 | /lib/x86_64-linux-gnu/libc.so.6(__libc_start_main+0x85) [0x747884a0fd65]
#07 0x4010d1 _start+0x21 | ./stack_trace_demo(_start+0x21) [0x4010d1]
```

可以看到，输出中包含 `print_stack_trace`、`level_three`、`level_two`、`level_one`、`main` 等多层回溯信息，满足“至少 3 层以上调用回溯”的验收要求。

使用 `addr2line` 解析部分地址：

```text
print_stack_trace
/root/os_experiments/lab1/task3/stack_trace_demo.c:17
level_three
/root/os_experiments/lab1/task3/stack_trace_demo.c:42
level_two
/root/os_experiments/lab1/task3/stack_trace_demo.c:50
level_one
/root/os_experiments/lab1/task3/stack_trace_demo.c:58
main
/root/os_experiments/lab1/task3/stack_trace_demo.c:66
```

这里既能看到程序自身打印出来的函数名，也能看到 `addr2line` 借助调试信息把地址解析回具体源码行号。

## 7. 编译选项对调用栈回溯的作用

### `-fno-omit-frame-pointer`

在很多体系结构和编译优化级别下，编译器会省略帧指针（frame pointer），把原本用来串联栈帧的寄存器释放出来做通用寄存器使用。这样能提高一点性能，但会让基于栈帧链的回溯变得更困难。

显式使用 `-fno-omit-frame-pointer` 后，编译器会保留每一层函数的帧指针，形成更清晰的“上一层栈帧 -> 下一层栈帧”的链式结构。这样做的好处是：

1. 回溯工具更容易沿着栈帧链向上遍历；
2. 调试器在优化较少时更容易定位局部变量和返回地址；
3. 对教学实验来说，栈帧布局更稳定、更容易解释。

### `-g` 与 DWARF 调试信息

`-g` 会在可执行文件中加入 DWARF 调试信息。DWARF 本身不是调用栈“地址链”的来源，但它提供了“地址 -> 源文件/函数名/行号”等映射关系，因此像 `addr2line`、`gdb` 这样的工具可以把回溯地址还原成更有意义的符号信息。

如果没有 `-g`：

- `backtrace()` 仍然可能拿到地址；
- `backtrace_symbols()` 可能仍然显示部分符号名；
- 但 `addr2line` 很难准确解析到源代码行号，报告的可读性会明显下降。

### `-rdynamic`

`-rdynamic` 会把主程序中的全局符号导出到动态符号表。这样 `backtrace_symbols()` 在处理主程序地址时，更容易直接打印出函数名，而不只是裸地址。

### `-O0`

`-O0` 能减少内联、尾调用优化和指令重排。对于“教学用回溯演示”，它不是必须条件，但非常有帮助，因为它能让实际看到的调用链更接近源代码里写下来的函数层次。

## 8. 机制解释

1. 每次函数调用都会在栈上建立一个新的栈帧，通常包含返回地址、保存的寄存器、局部变量等信息。
2. 当程序执行到最深层函数时，当前线程的用户态栈中已经按调用顺序堆叠了多层栈帧。
3. `backtrace()` 会按照当前平台和 glibc 的展开机制，从当前执行位置一路向上收集返回地址。
4. `backtrace_symbols()` 只是把这些地址转换成易读字符串；若想得到更准确的源码位置，则需要 `addr2line` 借助 DWARF 调试信息进一步解析。
5. 因此，“调用栈打印”本质上是两部分能力的结合：
   - 栈展开：找出一层层返回地址；
   - 符号解析：把地址映射为函数名、文件名、源代码行号。

## 9. 环境说明与限制

- 本次实验在当前 Linux 环境中完成并验证；
- 本回合未在第二台原生 Linux 云服务器上再次复现；
- `backtrace()` 的具体实现细节依赖 glibc、体系结构和编译选项，不同发行版下输出格式可能略有差异；
- 若开启更激进的优化，调用链可能因为内联或尾调用优化而变短。
