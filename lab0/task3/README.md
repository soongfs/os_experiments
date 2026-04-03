# LAB0 Task3: 用工程化结构体现操作系统四大特征

## 1. 实验目标

在同一程序中，以清晰的工程结构体现操作系统的四大特征：

- 并发：多个执行流同时工作；
- 共享：多个执行流访问同一份资源；
- 异步：调度顺序不可预先完全确定；
- 持久：最终状态被写入磁盘并可验证。

## 2. 工程结构

- [main.c](/root/os_experiments/lab0/task3/main.c)：程序入口，负责创建输出目录、打开共享日志文件描述符、调用各模块并打印摘要。
- [shared_state.c](/root/os_experiments/lab0/task3/shared_state.c)：初始化和销毁共享状态，体现“共享内存”模块。
- [concurrent.c](/root/os_experiments/lab0/task3/concurrent.c)：创建多个线程并发执行，体现“并发 + 异步”模块。
- [persistence.c](/root/os_experiments/lab0/task3/persistence.c)：将最终状态写入磁盘，体现“持久化”模块。
- [shared_state.h](/root/os_experiments/lab0/task3/shared_state.h)：共享数据结构定义。
- [Makefile](/root/os_experiments/lab0/task3/Makefile)：构建脚本。

## 3. 四大特征在代码中的对应关系

### 3.1 并发

`concurrent.c` 中创建了 `THREAD_COUNT = 4` 个工作线程，它们同时竞争 `CLAIM_COUNT = 16` 个共享任务槽位。

### 3.2 共享

本实验同时展示两类共享资源：

- 共享内存：所有线程共享同一个 `struct shared_state`，其中保存 `next_claim`、`claims_by_thread[]` 和 `claim_sequence[]`。
- 共享文件描述符：主线程只打开一次日志文件，得到 `log_fd`，然后把它传给所有线程；各线程通过同一个文件描述符写入事件日志。

### 3.3 异步

线程虽然同时启动，但谁先获得 CPU、谁先进入临界区、谁先写日志，都由内核调度决定。程序额外引入了基于运行时钟值的微小延迟与 `sched_yield()`，使得每次运行时线程抢占顺序更容易发生变化，因此 `claim_sequence` 和日志顺序通常不同。

### 3.4 持久

程序结束前会把最终统计结果写入 `artifacts/<label>_final_state.txt`，并对事件日志和状态文件调用 `fsync()`，再通过 `cat` 验证磁盘文件内容。

## 4. 编译与运行

编译命令：

```bash
make
```

连续运行三次：

```bash
./task3_demo run1
./task3_demo run2
./task3_demo run3
```

程序会生成：

- `artifacts/run1_event_log.txt`
- `artifacts/run1_final_state.txt`
- `artifacts/run2_event_log.txt`
- `artifacts/run2_final_state.txt`
- `artifacts/run3_event_log.txt`
- `artifacts/run3_final_state.txt`

## 5. 使用到的关键接口

- `pthread_create()` / `pthread_join()`：创建并等待多个线程，体现并发执行。
- `pthread_mutex_lock()` / `pthread_mutex_unlock()`：保护共享内存中的关键状态。
- `pthread_barrier_wait()`：让多个线程几乎同时开始竞争。
- `write()`：多个线程通过共享文件描述符写事件日志。
- `open()` / `fsync()` / `close()`：把事件日志和最终状态持久化到磁盘。
- `nanosleep()` / `sched_yield()`：放大调度差异，便于观察异步现象。

## 6. 本次实验记录

编译输出：

```text
rm -f task3_demo main.o shared_state.o concurrent.o persistence.o
gcc -g -O0 -Wall -Wextra -pthread   -c -o main.o main.c
gcc -g -O0 -Wall -Wextra -pthread   -c -o shared_state.o shared_state.c
gcc -g -O0 -Wall -Wextra -pthread   -c -o concurrent.o concurrent.c
gcc -g -O0 -Wall -Wextra -pthread   -c -o persistence.o persistence.c
gcc -g -O0 -Wall -Wextra -pthread -o task3_demo main.o shared_state.o concurrent.o persistence.o
```

三次实际运行输出：

```text
$ ./task3_demo run1
label: run1
log file: artifacts/run1_event_log.txt
state file: artifacts/run1_final_state.txt
total claims: 16
claim sequence: T3 -> T1 -> T0 -> T2 -> T0 -> T2 -> T3 -> T1 -> T0 -> T2 -> T1 -> T3 -> T0 -> T2 -> T1 -> T3

$ ./task3_demo run2
label: run2
log file: artifacts/run2_event_log.txt
state file: artifacts/run2_final_state.txt
total claims: 16
claim sequence: T0 -> T3 -> T1 -> T2 -> T1 -> T0 -> T3 -> T2 -> T1 -> T0 -> T3 -> T2 -> T0 -> T1 -> T3 -> T2

$ ./task3_demo run3
label: run3
log file: artifacts/run3_event_log.txt
state file: artifacts/run3_final_state.txt
total claims: 16
claim sequence: T3 -> T0 -> T1 -> T2 -> T3 -> T0 -> T1 -> T2 -> T3 -> T0 -> T1 -> T2 -> T2 -> T2 -> T3 -> T0
```

可以看到，三次运行的 `claim sequence` 明显不同，说明在相同程序逻辑下，线程实际获得 CPU 和进入临界区的顺序具有异步性。

使用 `cat` 验证第一次运行落盘的最终状态：

```bash
cat artifacts/run1_final_state.txt
```

本次输出：

```text
label=run1
log_path=artifacts/run1_event_log.txt
total_claims=16
claims_by_thread[0]=4
claims_by_thread[1]=4
claims_by_thread[2]=4
claims_by_thread[3]=4
claim_sequence=T3 -> T1 -> T0 -> T2 -> T0 -> T2 -> T3 -> T1 -> T0 -> T2 -> T1 -> T3 -> T0 -> T2 -> T1 -> T3
```

使用 `head` 查看共享日志文件的前 8 行：

```bash
head -n 8 artifacts/run1_event_log.txt
```

本次输出：

```text
thread-3 claimed slot-0
thread-1 claimed slot-1
thread-0 claimed slot-2
thread-2 claimed slot-3
thread-3 claimed slot-6
thread-0 claimed slot-4
thread-1 claimed slot-7
thread-2 claimed slot-5
```

这里可以看到，日志里 `slot-6` 出现在 `slot-4` 和 `slot-5` 之前，说明线程虽然按某个顺序拿到了共享任务编号，但写日志本身的先后仍受到调度影响，这正是异步执行的可核验现象。

还可以直接比较三次持久化文件中的 `claim_sequence`：

```bash
grep '^claim_sequence' artifacts/run1_final_state.txt artifacts/run2_final_state.txt artifacts/run3_final_state.txt
```

本次输出：

```text
artifacts/run1_final_state.txt:claim_sequence=T3 -> T1 -> T0 -> T2 -> T0 -> T2 -> T3 -> T1 -> T0 -> T2 -> T1 -> T3 -> T0 -> T2 -> T1 -> T3
artifacts/run2_final_state.txt:claim_sequence=T0 -> T3 -> T1 -> T2 -> T1 -> T0 -> T3 -> T2 -> T1 -> T0 -> T3 -> T2 -> T0 -> T1 -> T3 -> T2
artifacts/run3_final_state.txt:claim_sequence=T3 -> T0 -> T1 -> T2 -> T3 -> T0 -> T1 -> T2 -> T3 -> T0 -> T1 -> T2 -> T2 -> T2 -> T3 -> T0
```

## 7. 结果分析

1. 多线程同时运行，体现了操作系统的并发性。
2. 线程共享内存中的统计数组和共享日志文件描述符，体现了资源共享性。
3. 由于线程调度具有异步性，同样的程序多次运行时，`claim_sequence` 往往不同，这说明执行顺序并不是完全确定的。
4. 运行结束后，程序把最终状态写入磁盘文件，并能通过 `cat` 验证，这体现了持久化特征。
