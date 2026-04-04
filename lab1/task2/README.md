# LAB1 Task2: sleep 系统调用与可观察行为

## 1. 原始任务说明

### 任务标题

sleep 系统调用与可观察行为

### 任务目标

理解进程主动让出 CPU 的行为，掌握时间相关系统调用的使用与验证方法。

### 任务要求

1. 实现 Linux 应用程序，调用 `sleep`（或 `nanosleep`）睡眠 5 秒；
2. 睡眠前后分别打印时间戳或提示语，以便观察睡眠区间；
3. 在实验记录中说明：`sleep` 对进程状态的影响（如阻塞/可运行）。

### 验收检查

1. 终端打印显示前后的时间戳差值应约等于 5 秒；
2. 准确说明 `sleep` 期间进程处于挂起/阻塞（Blocked/Sleeping）状态，不占用 CPU 时间片。

## 2. 实验目标与实现思路

本实验使用 C 语言编写一个 Linux 用户态程序，直接调用 `nanosleep()` 睡眠 5 秒。程序在睡眠前后分别使用 `clock_gettime()` 记录时间戳，并使用 `CLOCK_MONOTONIC` 计算实际经过时间，避免系统时钟被手动调整时带来误差。

为验证进程在睡眠期间的状态，本实验在程序后台运行时使用 `ps` 观察其进程状态字段 `STAT`。Linux 中 `S` 表示可中断睡眠（sleeping）。

## 3. 文件列表

- [sleep_observer.c](/root/os_experiments/lab1/task2/sleep_observer.c)：实验程序源码。
- [README.md](/root/os_experiments/lab1/task2/README.md)：实验说明与验收记录。

## 4. 源代码与说明

文件：[sleep_observer.c](/root/os_experiments/lab1/task2/sleep_observer.c)

```c
#include <errno.h>
#include <stdio.h>
#include <time.h>

static void print_realtime_stamp(const char *label, const struct timespec *ts) {
    struct tm local_tm;
    char buffer[64];

    localtime_r(&ts->tv_sec, &local_tm);
    strftime(buffer, sizeof(buffer), "%Y-%m-%d %H:%M:%S", &local_tm);
    printf("%s: %s.%09ld\n", label, buffer, ts->tv_nsec);
}

static double timespec_diff_seconds(const struct timespec *start, const struct timespec *end) {
    time_t sec = end->tv_sec - start->tv_sec;
    long nsec = end->tv_nsec - start->tv_nsec;

    if (nsec < 0) {
        sec -= 1;
        nsec += 1000000000L;
    }

    return (double)sec + (double)nsec / 1000000000.0;
}

int main(void) {
    struct timespec start_real;
    struct timespec end_real;
    struct timespec start_mono;
    struct timespec end_mono;
    struct timespec request = {.tv_sec = 5, .tv_nsec = 0};
    struct timespec remain = {0};
    int ret;

    if (clock_gettime(CLOCK_REALTIME, &start_real) != 0) {
        perror("clock_gettime CLOCK_REALTIME");
        return 1;
    }

    if (clock_gettime(CLOCK_MONOTONIC, &start_mono) != 0) {
        perror("clock_gettime CLOCK_MONOTONIC");
        return 1;
    }

    print_realtime_stamp("Before nanosleep", &start_real);
    printf("Sleeping for %ld seconds...\n", request.tv_sec);
    fflush(stdout);

    do {
        ret = nanosleep(&request, &remain);
        if (ret != 0 && errno == EINTR) {
            request = remain;
        }
    } while (ret != 0 && errno == EINTR);

    if (ret != 0) {
        perror("nanosleep");
        return 1;
    }

    if (clock_gettime(CLOCK_REALTIME, &end_real) != 0) {
        perror("clock_gettime CLOCK_REALTIME");
        return 1;
    }

    if (clock_gettime(CLOCK_MONOTONIC, &end_mono) != 0) {
        perror("clock_gettime CLOCK_MONOTONIC");
        return 1;
    }

    print_realtime_stamp("After nanosleep ", &end_real);
    printf("Elapsed seconds (monotonic): %.6f\n", timespec_diff_seconds(&start_mono, &end_mono));

    return 0;
}
```

代码要点：

- `clock_gettime(CLOCK_REALTIME, ...)`：打印可读时间戳，便于直接观察睡眠前后时刻。
- `clock_gettime(CLOCK_MONOTONIC, ...)`：计算稳定的时间差值。
- `nanosleep()`：让当前进程主动进入睡眠状态 5 秒。
- 若 `nanosleep()` 被信号中断，则利用 `remain` 继续睡眠剩余时间。

## 5. 编译、运行与复现方法

进入任务目录：

```bash
cd /root/os_experiments/lab1/task2
```

编译：

```bash
gcc -g -O0 -Wall -Wextra -o sleep_observer sleep_observer.c
```

直接运行：

```bash
./sleep_observer
```

观察睡眠中的进程状态：

```bash
./sleep_observer > program_output.txt &
pid=$!
sleep 1
ps -o pid,stat,time,etime,cmd -p "$pid"
wait "$pid"
cat program_output.txt
```

## 6. 本次实际运行结果

程序直接运行时的实际输出：

```text
Before nanosleep: 2026-04-04 20:09:54.981558564
Sleeping for 5 seconds...
After nanosleep : 2026-04-04 20:09:59.981742696
Elapsed seconds (monotonic): 5.000184
```

从最后一行可以看到，单调时钟测得的经过时间约为 `5.000184` 秒，满足“约等于 5 秒”的验收要求。

程序后台运行 1 秒后，`ps` 的实际输出：

```text
    PID STAT     TIME     ELAPSED CMD
   1599 S    00:00:00       00:01 ./sleep_observer
```

这里的 `STAT` 为 `S`，表示进程正处于 sleeping 状态；`TIME` 仍为 `00:00:00`，说明该阶段几乎没有消耗 CPU 运行时间。

程序结束后保存的标准输出内容为：

```text
Before nanosleep: 2026-04-04 20:10:07.463276841
Sleeping for 5 seconds...
After nanosleep : 2026-04-04 20:10:12.463526061
Elapsed seconds (monotonic): 5.000249
```

## 7. `sleep/nanosleep` 对进程状态的影响

当进程调用 `nanosleep()` 时，它不会继续处于“可立即运行”的状态，而是主动告诉内核：在指定时间到达之前，不需要继续占用 CPU。

从调度角度看，可以这样理解：

1. 进程执行 `nanosleep()` 后进入睡眠等待；
2. 内核为该进程设置一个定时器，并把它从运行队列中移走；
3. 在睡眠期间，进程处于阻塞/睡眠状态，不能被正常调度执行；
4. 定时器到期后，内核再把该进程标记为可运行，等待调度器重新分配 CPU。

因此，`sleep`/`nanosleep` 的核心作用不是“空转等待”，而是让进程主动阻塞，让出 CPU 时间片给其他可运行进程。

## 8. 机制解释

1. 用户态程序调用 `nanosleep()` 时，会通过系统调用陷入内核，请求内核挂起当前线程直到指定时间到达。
2. 内核不会让进程一直忙等 5 秒，而是把当前任务放入睡眠队列，同时设置对应的高精度定时器或时钟事件。
3. 在这段时间里，调度器不会把 CPU 时间片分配给该睡眠中的进程，所以它不消耗正常的用户态计算时间。
4. 当定时器超时，内核唤醒该进程，把它重新放回可运行队列。
5. 进程被再次调度后，从 `nanosleep()` 返回，继续执行后续打印语句。

这就是为什么实验中既能观察到约 5 秒的时间间隔，又能看到进程在睡眠期间处于 `S` 状态且几乎不消耗 CPU 时间。

## 9. 环境说明与限制

- 本次实验在当前 Linux 环境中完成并验证；
- 本回合未在第二台原生 Linux 云服务器上再次复现；
- `ps` 观察到的状态字母依赖 Linux 进程状态表示，本次实验记录中 `S` 表示 sleeping（可中断睡眠）。
