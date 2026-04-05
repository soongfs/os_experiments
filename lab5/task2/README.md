# LAB5 用户态 Task2：逻辑短路与进程树计数

## 1. 原始任务说明

### 任务标题

逻辑短路与进程树计数

### 任务目标

理解 `&&` / `||` 的短路语义与 `fork()` 组合产生的进程树效应。

### 任务要求

1. 分析指定表达式输出次数；
2. 设计通用程序：输入一段 `&&` / `||` 序列，输出产生的进程数量或输出次数；
3. 给出验证：与实际运行结果一致。

### 验收检查

1. 运行结果产生的子进程数量和 `printf` 输出次数严格符合逻辑短路理论预期；
2. 附上源码与对应截图（本次提交不附截图，按下文复现命令自行截图）。

## 2. Acceptance -> Evidence 清单

- 经典混合表达式的理论输出次数被明确分析。
  证据：本 README 第 7.3 节分析了 `fork() && fork() || fork(); printf("X\n");`，结论是最终共有 `5` 个进程执行 `printf`，因此输出 `5` 次。
- 通用程序能够输入一段 `&& / ||` 序列并输出理论计数。
  证据：`./fork_logic_counter analyze '&& || &&'` 这种调用方式在 [fork_logic_counter.c](/root/os_experiments/lab5/task2/fork_logic_counter.c) 中实现；`[theory]` 日志会输出总进程数、子进程数、`printf` 输出次数以及每个 `fork#N` 的实际求值次数。
- 理论值和真实运行值一致。
  证据：在 [artifacts/run_output.txt](/root/os_experiments/lab5/task2/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab5/task2/artifacts/run_output_repeat.txt) 中，5 组表达式都出现 `[acceptance] theory matches runtime process/printf counts: PASS`。
- 子进程数量和 `printf` 输出次数都被真实验证。
  证据：每个 case 的 `[verify] actual_total_processes=... actual_child_processes=... actual_printf_outputs=...` 与预测值完全一致。

## 3. 实验目标与实现思路

本实验在 [lab5/task2](/root/os_experiments/lab5/task2) 中实现为宿主 Linux 用户态 C 程序，运行环境是 WSL Debian 上的 x86_64 Linux 进程，不是 QEMU guest。

这里继续选择宿主 Linux，是因为本题的核心机制就是 C 语言短路求值配合真实 `fork()` 的返回值分裂，而当前仓库里的 LAB2+ 教学内核并没有提供一套完整可复用的 Unix `fork` ABI。题目要求又是“输入一段 `&& / ||` 序列并验证实际运行结果”，因此直接用宿主 Linux 的真实 `fork()` 更贴近题意，也能得到稳定、可审查的运行日志。

程序的输入约定是：

- 输入一串带引号的操作符序列，例如 `'&& || &&'`
- 程序自动把它展开成 `fork() && fork() || fork() && fork()`
- 默认研究的输出模型是：

```c
fork() && fork() || fork() && fork();
printf("X\n");
```

也就是“表达式后面紧跟一个 `printf`”。  
因此：

- 最终存活的进程数 = 表达式求值结束后的叶子数
- 子进程总数 = 最终存活进程数 - 1
- `printf` 输出次数 = 最终存活进程数

## 4. 文件列表与代码说明

- [fork_logic_counter.c](/root/os_experiments/lab5/task2/fork_logic_counter.c)：主实验程序，包含运算符序列解析、理论计数、真实 `fork()` 验证、进程回收和内置 demo 套件。
- [Makefile](/root/os_experiments/lab5/task2/Makefile)：构建入口。
- [artifacts/build_output.txt](/root/os_experiments/lab5/task2/artifacts/build_output.txt)：最终构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab5/task2/artifacts/run_output.txt)：第一次完整 demo 运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab5/task2/artifacts/run_output_repeat.txt)：第二次完整 demo 运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab5/task2/artifacts/tool_versions.txt)：编译器、glibc、内核和页大小信息。

## 5. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab5/task2
```

构建：

```bash
make
```

运行内置 demo 套件并保存日志：

```bash
./fork_logic_counter demo > artifacts/run_output.txt 2>&1
./fork_logic_counter demo > artifacts/run_output_repeat.txt 2>&1
```

对任意输入序列只做理论分析：

```bash
./fork_logic_counter analyze '&& || &&'
```

对任意输入序列做“理论 + 真实运行”验证：

```bash
./fork_logic_counter verify '&& ||'
```

注意：命令里的操作符序列必须带引号，否则 shell 会把 `&&` / `||` 当成自己的控制运算符。

如果你要补“终端运行截图”，建议前台运行：

```bash
./fork_logic_counter verify '&& ||'
```

截图时至少保留这些关键行：

- `[theory] expression=fork() && fork() || fork()`
- `[verify] predicted_total_processes=5 ...`
- `[verify] actual_total_processes=5 ...`
- `[acceptance] theory matches runtime process/printf counts: PASS`

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/tool_versions.txt
```

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab5/task2/artifacts/build_output.txt) 的实际内容：

```text
rm -f fork_logic_counter
cc -g -O0 -Wall -Wextra -std=c11 -o fork_logic_counter fork_logic_counter.c
```

### 6.2 第一次完整 demo 结果

[artifacts/run_output.txt](/root/os_experiments/lab5/task2/artifacts/run_output.txt) 中验证了 5 组表达式。每组都先输出 `[theory]`，再输出 `[verify]` 和真实 `[leaf]` 进程日志，最后给出 PASS。

关键汇总如下：

- `fork() && fork()`
  - 预测：`total_processes=3 child_processes=2 printf_outputs=3`
  - 实测：`actual_total_processes=3 actual_child_processes=2 actual_printf_outputs=3`
- `fork() || fork()`
  - 预测：`total_processes=3 child_processes=2 printf_outputs=3`
  - 实测：`actual_total_processes=3 actual_child_processes=2 actual_printf_outputs=3`
- `fork() && fork() || fork()`
  - 预测：`total_processes=5 child_processes=4 printf_outputs=5`
  - 实测：`actual_total_processes=5 actual_child_processes=4 actual_printf_outputs=5`
- `fork() || fork() && fork()`
  - 预测：`total_processes=4 child_processes=3 printf_outputs=4`
  - 实测：`actual_total_processes=4 actual_child_processes=3 actual_printf_outputs=4`
- `fork() && fork() || fork() && fork()`
  - 预测：`total_processes=7 child_processes=6 printf_outputs=7`
  - 实测：`actual_total_processes=7 actual_child_processes=6 actual_printf_outputs=7`

其中，题目要求的“指定表达式输出次数分析”我选用了最典型的混合表达式：

```text
fork() && fork() || fork()
```

它在第一次运行中的关键片段是：

```text
[theory] expression=fork() && fork() || fork()
[theory] operands=3 operators=2 total_processes=5 child_processes=4 printf_outputs=5 true_results=3 false_results=2
[theory] fork#1 evaluated_by=1 process(es)
[theory] fork#2 evaluated_by=1 process(es)
[theory] fork#3 evaluated_by=2 process(es)
[verify] predicted_total_processes=5 predicted_child_processes=4 predicted_printf_outputs=5 predicted_true_results=3 predicted_false_results=2
[verify] actual_total_processes=5 actual_child_processes=4 actual_printf_outputs=5 actual_true_results=3 actual_false_results=2
[acceptance] theory matches runtime process/printf counts: PASS
```

这说明：

1. 表达式结束后共有 `5` 个叶子进程；
2. 一共产生 `4` 个子进程；
3. 如果表达式后面接一个 `printf`，它会被执行 `5` 次；
4. 理论计数和真实 `fork()` 运行结果完全一致。

### 6.3 第二次完整 demo 结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab5/task2/artifacts/run_output_repeat.txt) 再次验证同一组 5 个表达式。由于调度顺序不同，`[leaf]` 行里的 PID 和先后顺序会变化，但所有 summary 行保持一致。

第二次运行中，关键计数仍然是：

- `fork() && fork()` -> `3` 个最终进程，`3` 次 `printf`
- `fork() || fork()` -> `3` 个最终进程，`3` 次 `printf`
- `fork() && fork() || fork()` -> `5` 个最终进程，`5` 次 `printf`
- `fork() || fork() && fork()` -> `4` 个最终进程，`4` 次 `printf`
- `fork() && fork() || fork() && fork()` -> `7` 个最终进程，`7` 次 `printf`

并且每个 case 末尾都再次得到：

```text
[acceptance] theory matches runtime process/printf counts: PASS
```

这说明计数逻辑是稳定的，非确定性的只是进程调度顺序，不是最终数量。

### 6.4 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab5/task2/artifacts/tool_versions.txt) 中记录了本次环境：

```text
gcc (Debian 14.2.0-19) 14.2.0
Linux Laptop-SoongFS 6.6.87.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Thu Jun  5 18:30:46 UTC 2025 x86_64 GNU/Linux
glibc 2.41
4096
```

## 7. 机制解释

### 7.1 `fork()` 在逻辑表达式里的真假值

在 C 语言里，把 `fork()` 直接放进逻辑表达式时：

- 父进程拿到的是子进程 PID，非零，因此逻辑值为 `true`
- 子进程拿到的是 `0`，因此逻辑值为 `false`

所以一次 `fork()` 不只是“多出一个进程”，还会让这两个进程在同一表达式上走向不同的真假分支。

### 7.2 短路计数公式

设某个子表达式从“一个进入它的进程”开始求值后：

- 得到 `T` 个结果为 `true` 的叶子进程
- 得到 `F` 个结果为 `false` 的叶子进程

那么：

- 对单个 `fork()`：`T=1, F=1`
- 对 `A && B`：
  - 只有 `A` 为 `true` 的那些进程才会继续执行 `B`
  - 因此：
    - `T(A && B) = T(A) * T(B)`
    - `F(A && B) = F(A) + T(A) * F(B)`
- 对 `A || B`：
  - 只有 `A` 为 `false` 的那些进程才会继续执行 `B`
  - 因此：
    - `T(A || B) = T(A) + F(A) * T(B)`
    - `F(A || B) = F(A) * F(B)`

最终：

- 总进程数 = `T + F`
- 子进程数 = `T + F - 1`
- 若表达式后仅有一个 `printf`，输出次数 = `T + F`

### 7.3 指定表达式 `fork() && fork() || fork()` 的理论分析

题目没有单独给出具体表达式，因此这里选用最经典的混合表达式：

```c
fork() && fork() || fork();
printf("X\n");
```

按照 C 的优先级，它等价于：

```c
(fork() && fork()) || fork()
```

先看左半边 `fork() && fork()`：

- 第一个 `fork()` 后有两个进程：
  - 父分支为 `true`
  - 子分支为 `false`
- 只有父分支会继续执行第二个 `fork()`
- 因此左半边最终得到：
  - `1` 个 `true`
  - `2` 个 `false`

再看整体 `(A) || fork()`：

- `A` 为 `true` 的那 `1` 个进程直接短路，不再执行最后一个 `fork()`
- `A` 为 `false` 的那 `2` 个进程才会执行最后一个 `fork()`
- 每个执行最后一个 `fork()` 的进程又会裂变成：
  - `1` 个 `true`
  - `1` 个 `false`

所以最终：

- `true` 结果叶子进程：`1 + 2 * 1 = 3`
- `false` 结果叶子进程：`2 * 1 = 2`
- 总进程数：`3 + 2 = 5`
- 子进程数：`5 - 1 = 4`
- 表达式后面那个 `printf("X\n")` 会执行 `5` 次

这和实际运行日志完全一致。

### 7.4 为什么 `fork() || fork() && fork()` 只有 4 个最终进程

这个例子专门用来提醒优先级：

```c
fork() || fork() && fork()
```

在 C 里不是 `(fork() || fork()) && fork()`，而是：

```c
fork() || (fork() && fork())
```

因此：

- 第一个 `fork()` 的父进程结果已经是 `true`，直接短路，不再执行右边
- 只有第一个 `fork()` 的子进程才去执行右边的 `fork() && fork()`

所以最终只有 `4` 个叶子进程，而不是别的数量。  
这也正是 [artifacts/run_output.txt](/root/os_experiments/lab5/task2/artifacts/run_output.txt) case 4 中看到的 `actual_total_processes=4`。

## 8. 验收检查映射

- [x] 运行结果产生的子进程数量和 `printf` 输出次数严格符合逻辑短路理论预期。
  证据：5 组表达式全部出现 `[acceptance] theory matches runtime process/printf counts: PASS`；其中经典表达式 `fork() && fork() || fork()` 的预测值和实测值都是 `total_processes=5 child_processes=4 printf_outputs=5`。
- [x] 附上源码与对应截图。
  证据：源码见 [fork_logic_counter.c](/root/os_experiments/lab5/task2/fork_logic_counter.c)；截图本次未直接附文件，复现时前台运行 `./fork_logic_counter verify '&& ||'` 并截取包含 `[theory]`、`[verify]`、`[acceptance]` 的终端即可。

## 9. 环境说明、复现限制与未决事项

- 本实验只在当前 WSL Debian 环境验证；没有额外在原生 Linux 服务器上复验。
- 这里使用的是宿主 Linux 的真实 `fork()` 和 C 语言短路语义，不是 QEMU guest 中的教学内核 ABI。
- `[leaf]` 行的 PID 和先后顺序会因调度不同而变化，但 summary 行里的最终数量应保持稳定。
