# LAB7 用户态 Task2：基于 signal 的异步通知

## 1. 原始任务说明

### 任务目标

理解信号的异步性与竞态风险，掌握可控的信号处理方式。

### 任务要求

1. 编写两进程程序：一方发送信号，一方处理信号；
2. 输出可观察证据：收到信号的次数与处理结果；
3. 在实验记录中说明：异步处理带来的竞态与可重入风险。

### 验收检查

1. 成功捕获 `SIGUSR1` 等自定义或标准信号并执行自定义 Handler；
2. 报告举例说明函数“不可重入”导致问题的典型场景。

## 2. Acceptance -> Evidence 清单

- 必须实现两个进程，一方发送信号，一方处理信号。
  证据：实验程序 [signal_async_demo.c](/root/os_experiments/lab7/task2/signal_async_demo.c) 中，父进程作为 receiver 安装 handler，子进程作为 sender 用 `kill()` 发送 `SIGUSR1` 与 `SIGUSR2`。
- 必须成功捕获 `SIGUSR1` 并执行自定义 handler。
  证据：两次运行日志中的 `[receiver] event=SIGUSR1 ...` 行和 `[acceptance] custom SIGUSR1 handler executed ... PASS` 行。
- 必须输出“收到信号的次数与处理结果”。
  证据：日志中逐次打印 `handled_usr1`、`handler_visible_count`、`shared_total`，最终 `[summary]` 行汇总 handler 视角和主循环视角的计数。
- 报告必须说明异步带来的竞态与可重入风险，并举出不可重入函数导致问题的典型场景。
  证据：本 README 第 5 节和第 8 节专门讨论“信号合并竞态”和“在 handler 中调用 `printf`/`malloc`/`strtok` 一类不可重入函数”的风险。

## 3. 实验环境与实现思路

本实验运行在宿主机 Linux 原生用户态环境，不是 QEMU guest。原因是题目要求验证 POSIX/Linux 信号语义，而 `sigaction`、`kill`、`sig_atomic_t` 和异步 signal handler 都是宿主机内核与 C 运行时直接提供的机制。

实现采用一个统一实验程序 [signal_async_demo.c](/root/os_experiments/lab7/task2/signal_async_demo.c)：

- 父进程是接收者，安装 `SIGUSR1` 和 `SIGUSR2` 的自定义 handler；
- 子进程是发送者，循环发送 `8` 次 `SIGUSR1`，最后发送 `1` 次 `SIGUSR2` 作为结束通知；
- handler 内只做两件事：
  - 更新 `volatile sig_atomic_t` 计数器；
  - 把信号编号写入 self-pipe；
- 父进程主循环从 self-pipe 读取事件，打印可观察结果，并通过 ack pipe 告诉子进程“本轮已经处理完成，可以发下一次”。

这里刻意把“异步接收”和“同步整理输出”分离开：

1. handler 保持最小化，避免在异步上下文中调用不可重入函数；
2. self-pipe 让异步信号变成主循环可控消费的事件流；
3. ack pipe 刻意限制发送节奏，避免标准信号因为快速连发而被合并，保证实验日志稳定可复现。

## 4. 文件列表与代码说明

- [signal_async_demo.c](/root/os_experiments/lab7/task2/signal_async_demo.c)：两进程 signal 实验程序，包含 handler、self-pipe 事件转发、ack 节流和验收输出。
- [Makefile](/root/os_experiments/lab7/task2/Makefile)：构建入口，使用 `gcc -g -O0 -Wall -Wextra -std=c11`。
- [artifacts/build_output.txt](/root/os_experiments/lab7/task2/artifacts/build_output.txt)：构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab7/task2/artifacts/run_output.txt)：第一次运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab7/task2/artifacts/run_output_repeat.txt)：第二次运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab7/task2/artifacts/tool_versions.txt)：工具链与内核版本记录。

## 5. 机制说明

### 5.1 为什么 signal 是异步的

信号不是像普通函数调用那样按程序员显式顺序执行。接收进程可能在任意指令点被内核打断，转去执行 handler，然后再回到原执行流。因此：

- handler 与主流程共享地址空间；
- 主流程以为“不会变”的状态，可能在信号到来后立刻变化；
- 如果 handler 做了复杂逻辑，就容易和主流程形成竞态。

### 5.2 本实验如何把异步处理变得可控

本实验采用典型的“最小 handler + 主循环收敛”模式：

- 在 handler 中，只修改 `sig_atomic_t` 计数器并调用 `write()` 把事件写到管道；
- 真正的打印、统计整合和退出决策，都在主循环里完成；
- 发送方只有在收到 ACK 后才发送下一次 `SIGUSR1`，确保这次实验看到的是“8 次都被稳定处理”，而不是被标准信号合并后的不确定结果。

### 5.3 为什么要显式讨论竞态

如果去掉 ACK 节流，子进程把很多个 `SIGUSR1` 极快地连续打给父进程，Linux 标准信号可能被合并，接收方未必能观察到与发送次数完全相同的 handler 触发次数。这正是信号语义里很典型的竞态来源：发送次数、待处理信号集合、调度时机三者并不总能一一对应。

## 6. 构建、运行与复现命令

进入目录：

```bash
cd /root/os_experiments/lab7/task2
```

构建：

```bash
make
```

前台运行，便于终端截图：

```bash
./signal_async_demo
```

保存第一次日志：

```bash
./signal_async_demo > artifacts/run_output.txt 2>&1
```

保存第二次日志：

```bash
./signal_async_demo > artifacts/run_output_repeat.txt 2>&1
```

记录工具版本：

```bash
{
  gcc --version | head -n 1
  uname -srmo
  getconf SIGQUEUE_MAX
} > artifacts/tool_versions.txt
```

## 7. 本次实际运行结果

### 7.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab7/task2/artifacts/build_output.txt) 的实际内容：

```text
gcc -g -O0 -Wall -Wextra -std=c11 -o signal_async_demo signal_async_demo.c
```

### 7.2 第一次运行结果

[artifacts/run_output.txt](/root/os_experiments/lab7/task2/artifacts/run_output.txt) 的关键输出：

```text
[config] environment=linux-native sender_pid=4 receiver_pid=3 sigusr1_rounds=8
[config] control=self-pipe for async-safe handoff, ack-pipe to avoid coalescing races
[receiver] event=SIGUSR1 handled_usr1=1 handler_visible_count=1 shared_total=1 status=processed
[receiver] event=SIGUSR1 handled_usr1=2 handler_visible_count=2 shared_total=2 status=processed
[receiver] event=SIGUSR1 handled_usr1=3 handler_visible_count=3 shared_total=3 status=processed
[receiver] event=SIGUSR1 handled_usr1=4 handler_visible_count=4 shared_total=4 status=processed
[receiver] event=SIGUSR1 handled_usr1=5 handler_visible_count=5 shared_total=5 status=processed
[receiver] event=SIGUSR1 handled_usr1=6 handler_visible_count=6 shared_total=6 status=processed
[receiver] event=SIGUSR1 handled_usr1=7 handler_visible_count=7 shared_total=7 status=processed
[receiver] event=SIGUSR1 handled_usr1=8 handler_visible_count=8 shared_total=8 status=processed
[receiver] event=SIGUSR2 handled_usr2=1 handler_visible_count=1 action=shutdown
[summary] handler_sigusr1=8 mainloop_sigusr1=8 handler_sigusr2=1 mainloop_sigusr2=1 shared_total=8 handler_failures=0
[acceptance] custom SIGUSR1 handler executed and receiver observed 8 deliveries: PASS
[acceptance] shutdown signal SIGUSR2 was captured by custom handler: PASS
[acceptance] handler used only async-signal-safe write plus sig_atomic_t counters: PASS
```

### 7.3 第二次运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab7/task2/artifacts/run_output_repeat.txt) 的关键输出与第一次一致：

```text
[receiver] event=SIGUSR1 handled_usr1=1 handler_visible_count=1 shared_total=1 status=processed
[receiver] event=SIGUSR1 handled_usr1=2 handler_visible_count=2 shared_total=2 status=processed
[receiver] event=SIGUSR1 handled_usr1=3 handler_visible_count=3 shared_total=3 status=processed
[receiver] event=SIGUSR1 handled_usr1=4 handler_visible_count=4 shared_total=4 status=processed
[receiver] event=SIGUSR1 handled_usr1=5 handler_visible_count=5 shared_total=5 status=processed
[receiver] event=SIGUSR1 handled_usr1=6 handler_visible_count=6 shared_total=6 status=processed
[receiver] event=SIGUSR1 handled_usr1=7 handler_visible_count=7 shared_total=7 status=processed
[receiver] event=SIGUSR1 handled_usr1=8 handler_visible_count=8 shared_total=8 status=processed
[receiver] event=SIGUSR2 handled_usr2=1 handler_visible_count=1 action=shutdown
[summary] handler_sigusr1=8 mainloop_sigusr1=8 handler_sigusr2=1 mainloop_sigusr2=1 shared_total=8 handler_failures=0
[acceptance] custom SIGUSR1 handler executed and receiver observed 8 deliveries: PASS
[acceptance] shutdown signal SIGUSR2 was captured by custom handler: PASS
[acceptance] handler used only async-signal-safe write plus sig_atomic_t counters: PASS
```

### 7.4 可观察证据汇总

从两次运行都能直接确认：

1. 发送进程一共发出 `8` 次 `SIGUSR1` 和 `1` 次 `SIGUSR2`；
2. receiver 主循环逐次观察到 `8` 次 `SIGUSR1` 处理结果，`shared_total` 最终为 `8`；
3. handler 视角计数与主循环视角计数一致：
   - `handler_sigusr1 = 8`
   - `mainloop_sigusr1 = 8`
   - `handler_sigusr2 = 1`
   - `mainloop_sigusr2 = 1`
4. `handler_failures = 0`，说明 handler 内的异步安全事件转发没有失败。

### 7.5 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab7/task2/artifacts/tool_versions.txt) 的实际内容：

```text
gcc (Debian 14.2.0-19) 14.2.0
Linux 6.6.87.2-microsoft-standard-WSL2 x86_64 GNU/Linux
62353
```

## 8. 异步性、竞态与可重入风险分析

### 8.1 异步处理为什么天然有竞态风险

信号的到达时机由内核决定，不受用户态主流程直接控制。进程可能正在更新共享状态、写日志、维护链表或修改全局缓冲区时被打断，随后 handler 在同一地址空间里异步执行。这意味着：

- 主流程和 handler 可能同时依赖同一份全局状态；
- 主流程中的“中间态”可能被 handler 观察到；
- handler 执行完返回后，主流程继续运行时，前提条件可能已经被改写。

这就是 signal 实验必须明确讨论竞态的原因。

### 8.2 本实验里如何降低竞态

本实验没有在 handler 里直接做复杂处理，而是采用“最小 handler”策略：

- 只更新 `sig_atomic_t` 计数器；
- 只调用 async-signal-safe 的 `write()` 把事件投递到 self-pipe；
- 所有 `printf`、业务统计、ACK 回复和退出逻辑都放在普通主循环里做。

因此，异步部分只负责“告诉主循环有信号来了”，而不是在 handler 里直接完成业务逻辑。

### 8.3 标准信号的典型竞态：合并与丢失可见次数

本实验专门加入 ACK 节流，是因为标准信号如 `SIGUSR1` 默认不是实时队列信号。若发送方极快地连续执行多次 `kill(receiver, SIGUSR1)`，接收方还没来得及从 pending 集合中清空同类型信号时，多个 `SIGUSR1` 可能只表现为“一次待处理 SIGUSR1”。结果就是：

- 发送方逻辑上“发了很多次”；
- 接收方 handler 不一定能同样多次触发；
- 最终观察到的次数可能小于发送次数。

这就是异步处理中的典型竞态之一。当前实现通过“每处理完一次再发下一次”把这个变量收敛掉，目的是稳定展示 handler 确实执行了 8 次，而不是让实验结果被调度偶然性主导。

### 8.4 不可重入函数导致问题的典型场景

典型错误场景是：主线程正在执行 `printf()` 向 `stdout` 写一行日志，此时收到 `SIGUSR1`，handler 里也调用 `printf()`。由于 `printf()` 依赖内部缓冲区和锁状态，它不是 async-signal-safe 函数，可能出现：

- 输出内容交错、重复或截断；
- stdio 内部锁状态被破坏，引发死锁；
- 缓冲区元数据损坏，导致后续输出异常。

同类风险还包括：

- `malloc()` / `free()`
  如果主流程正持有堆分配器内部锁，handler 再调用分配器，可能直接死锁或破坏堆状态。
- `strtok()`
  它依赖内部静态状态；主流程和 handler 交错调用时，分词上下文会被覆盖。
- 大多数标准库 I/O 和格式化函数
  它们通常维护共享内部状态，不适合在异步 signal handler 中使用。

因此，signal handler 的推荐实践不是“在里面做更多事”，而是“只做异步安全的最小动作，把真正工作延后到普通执行流”。

## 9. 验收结论

### 9.1 验收项 1

已经成功捕获 `SIGUSR1` 和 `SIGUSR2`，并执行了自定义 handler。

- 证据：两次日志都包含连续 8 条 `[receiver] event=SIGUSR1 ... status=processed`，以及 1 条 `[receiver] event=SIGUSR2 ... action=shutdown`。
- 证据：两次日志都包含：
  - `[acceptance] custom SIGUSR1 handler executed ... PASS`
  - `[acceptance] shutdown signal SIGUSR2 was captured by custom handler: PASS`

### 9.2 验收项 2

报告已经举例说明不可重入函数导致问题的典型场景。

- 证据：本 README 第 8.4 节明确举出 `printf()`、`malloc()/free()`、`strtok()` 在 handler 中使用时的风险，并解释了输出错乱、死锁、内部状态损坏等问题。

### 9.3 结论

本实验完成了“两进程 signal 异步通知”的核心要求，并给出了可复现证据：receiver 稳定捕获了 `8` 次 `SIGUSR1` 和 `1` 次 `SIGUSR2`，handler 计数与主循环计数完全一致。结合 README 中的机制分析，可以清楚看到：signal 最大的难点不在“收不到信号”，而在于它会在任意时刻打断当前执行流，因此必须通过最小 handler、异步安全函数和主循环收敛来控制竞态与可重入风险。
