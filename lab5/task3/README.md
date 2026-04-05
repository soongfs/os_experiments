# LAB5 用户态 Task3：多线程/多进程性能对比

## 1. 原始任务说明

### 任务标题

多线程/多进程性能对比

### 任务目标

形成对多核与调度策略影响的直观认识。

### 任务要求

1. 实现计算密集任务，分别使用多线程与多进程版本；
2. 记录运行时间与 CPU 利用率；
3. 在实验记录中讨论：线程与进程在调度与地址空间上的差异。

### 验收检查

1. 提供两种版本的对比数据表（执行耗时、内存占用等）；
2. 报告结合地址空间隔离、TLB 刷新等角度探讨进程切换和线程切换开销差异。

## 2. Acceptance -> Evidence 清单

- 已实现同一份计算密集工作负载的单进程串行版、多线程版和多进程版。
  证据：源码见 [parallel_benchmark.c](/root/os_experiments/lab5/task3/parallel_benchmark.c)，支持 `single`、`threads`、`processes` 和 `benchmark` 模式。
- 线程版与进程版记录了运行时间、CPU 利用率和内存占用。
  证据：`[result]` 行直接输出 `wall_s`、`user_s`、`sys_s`、`cpu_util_percent`、`rss_snapshot_kb`，见 [artifacts/run_output.txt](/root/os_experiments/lab5/task3/artifacts/run_output.txt) 与 [artifacts/run_output_repeat.txt](/root/os_experiments/lab5/task3/artifacts/run_output_repeat.txt)。
- 两种版本的对比数据表已给出。
  证据：本 README 第 6.3 节给出线程版与进程版的对比表，并附上单进程串行版作为基线。
- 报告中已从调度实体、地址空间隔离、TLB/页表上下文切换角度讨论差异。
  证据：本 README 第 7 节。

## 3. 实验目标与实现思路

本实验在 [lab5/task3](/root/os_experiments/lab5/task3) 中实现为宿主 Linux 用户态 C 程序，运行环境是 WSL Debian 上的 x86_64 Linux 进程，不是 QEMU guest。

这里继续选择宿主 Linux，有两个直接原因：

1. 题目需要真实 `pthread` 和真实 `fork()` / `wait4()` 的调度与地址空间行为；
2. 当前仓库里的 LAB2+ 教学内核并没有提供可复用的完整线程库和多进程 ABI。

因此本题最合理的做法，是直接利用宿主 Linux 的线程与进程机制做“同负载、不同并发模型”的对照实验。

工作负载设计为：

- 每个 worker 拥有 `8 MiB` 私有 `uint64_t` 缓冲区；
- 先完整初始化缓冲区，确保页面都已触达；
- 再执行 `2e7` 次整数混合与随机索引访存的计算内核；
- 最终输出 checksum，防止编译器把计算消掉。

这样能同时保留：

- 明确的 CPU 密集计算；
- 一定的内存访问压力；
- 可比较的 RSS 快照；
- 线程与进程在“共享地址空间 vs 独立地址空间”上的差异。

## 4. 文件列表与代码说明

- [parallel_benchmark.c](/root/os_experiments/lab5/task3/parallel_benchmark.c)：主实验程序。包含串行版、`pthread` 线程版、多进程版、`getrusage` 计时、`VmRSS` 快照和 checksum 汇总。
- [Makefile](/root/os_experiments/lab5/task3/Makefile)：构建入口，使用 `-O2 -pthread`。
- [artifacts/build_output.txt](/root/os_experiments/lab5/task3/artifacts/build_output.txt)：最终构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab5/task3/artifacts/run_output.txt)：第一次完整对比运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab5/task3/artifacts/run_output_repeat.txt)：第二次完整对比运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab5/task3/artifacts/tool_versions.txt)：编译器、glibc、内核和 CPU 信息。

## 5. 测量方法、边界与复现步骤

### 5.1 测量方法

本实验没有使用外部 GNU `time -v`，而是在程序内部直接记录：

- `wall_s`
  - 用 `clock_gettime(CLOCK_MONOTONIC)` 计算墙钟时间；
- `user_s` / `sys_s`
  - 串行版与线程版：用 `getrusage(RUSAGE_SELF)` 取差值；
  - 进程版：对父进程取 `RUSAGE_SELF` 差值，再叠加每个子进程 `wait4()` 返回的 `rusage`；
- `cpu_util_percent`
  - 定义为 `(user_s + sys_s) / wall_s * 100`；
  - 因此 4 个 worker 并行时，数值可以接近 `400%`；
- `rss_snapshot_kb`
  - 定义为“所有 worker 都已分配并触达缓冲区，但尚未进入正式计算”的那个时刻的 RSS 快照；
  - 串行版/线程版：直接读取当前进程 `/proc/self/status` 中的 `VmRSS`；
  - 进程版：把父进程 `VmRSS` 与每个子进程在启动屏障处上报的 `VmRSS` 相加，得到更可比的聚合 RSS。

这个边界的意义是：

- 初始化和首次 page fault 不计入正式计算时间；
- 正式计时只覆盖“纯计算 + 并发执行 + 最终 join/wait”阶段；
- `rss_snapshot_kb` 的口径在三种模式之间尽量一致。

### 5.2 本次使用的固定参数

正式 artifact 使用同一组参数：

```text
workers = 4
words_per_worker = 1048576
bytes_per_worker = 8388608  (8 MiB)
iterations_per_worker = 20000000
```

也就是：

- 4 个 worker
- 每个 worker 8 MiB 工作缓冲区
- 每个 worker 2000 万次计算循环

### 5.3 构建与复现命令

进入任务目录：

```bash
cd /root/os_experiments/lab5/task3
```

构建：

```bash
make
```

分别运行三种模式：

```bash
./parallel_benchmark single 4 1048576 20000000
./parallel_benchmark threads 4 1048576 20000000
./parallel_benchmark processes 4 1048576 20000000
```

保存第一次完整日志：

```bash
{
  echo '$ ./parallel_benchmark single 4 1048576 20000000'
  ./parallel_benchmark single 4 1048576 20000000
  echo '$ ./parallel_benchmark threads 4 1048576 20000000'
  ./parallel_benchmark threads 4 1048576 20000000
  echo '$ ./parallel_benchmark processes 4 1048576 20000000'
  ./parallel_benchmark processes 4 1048576 20000000
} > artifacts/run_output.txt 2>&1
```

保存第二次完整日志：

```bash
{
  echo '$ ./parallel_benchmark single 4 1048576 20000000'
  ./parallel_benchmark single 4 1048576 20000000
  echo '$ ./parallel_benchmark threads 4 1048576 20000000'
  ./parallel_benchmark threads 4 1048576 20000000
  echo '$ ./parallel_benchmark processes 4 1048576 20000000'
  ./parallel_benchmark processes 4 1048576 20000000
} > artifacts/run_output_repeat.txt 2>&1
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/tool_versions.txt
```

如果你要补“终端截图”，建议直接前台运行下面两条命令并截图：

```bash
./parallel_benchmark threads 4 1048576 20000000
./parallel_benchmark processes 4 1048576 20000000
```

至少保留 `[result] mode=threads ...` 和 `[result] mode=processes ...` 两行。

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab5/task3/artifacts/build_output.txt) 的实际内容：

```text
rm -f parallel_benchmark
cc -O2 -g -Wall -Wextra -std=c11 -pthread -o parallel_benchmark parallel_benchmark.c
```

### 6.2 原始结果摘要

[artifacts/run_output.txt](/root/os_experiments/lab5/task3/artifacts/run_output.txt) 的关键结果：

```text
[result] mode=single ... rss_snapshot_kb=34304 ... wall_s=1.749040 ... cpu_util_percent=100.00 checksum=0x0058d92adecda4e4
[result] mode=threads ... rss_snapshot_kb=34304 ... wall_s=1.041699 ... cpu_util_percent=397.22 checksum=0x0058d92adecda4e4
[result] mode=processes ... rss_snapshot_kb=38912 ... wall_s=1.043205 ... cpu_util_percent=399.18 checksum=0x0058d92adecda4e4
```

[artifacts/run_output_repeat.txt](/root/os_experiments/lab5/task3/artifacts/run_output_repeat.txt) 的关键结果：

```text
[result] mode=single ... rss_snapshot_kb=34176 ... wall_s=1.821869 ... cpu_util_percent=99.99 checksum=0x0058d92adecda4e4
[result] mode=threads ... rss_snapshot_kb=34304 ... wall_s=1.081261 ... cpu_util_percent=387.80 checksum=0x0058d92adecda4e4
[result] mode=processes ... rss_snapshot_kb=39040 ... wall_s=0.995696 ... cpu_util_percent=399.70 checksum=0x0058d92adecda4e4
```

这里有两个直接观察：

1. 三种模式 checksum 完全一致，说明做的是同一份计算工作；
2. 线程版和进程版都把 CPU 利用率抬到了约 `390% ~ 400%`，而串行版稳定在 `100%` 左右，说明确实用了 4 个并行 worker。

### 6.3 对比数据表

下表用两次运行的平均值汇总，便于审阅。原始值以两个 artifact 日志为准。

| 模式 | 运行1 wall_s | 运行2 wall_s | 平均 wall_s | 平均 CPU 利用率 | 平均 RSS 快照 KiB | 平均 sys_s | 相对串行加速 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `single` | 1.749040 | 1.821869 | 1.785455 | 100.00% | 34240 | 0.000552 | 1.00x |
| `threads` | 1.041699 | 1.081261 | 1.061480 | 392.51% | 34304 | 0.000428 | 1.68x |
| `processes` | 1.043205 | 0.995696 | 1.019451 | 399.44% | 38976 | 0.030050 | 1.75x |

从这张表能看到：

- 线程版和进程版都明显快于串行版；
- 线程版与进程版的 wall time 在这组粗粒度计算任务上非常接近；
- 进程版的聚合 `rss_snapshot_kb` 比线程版高约 `4672 KiB`，多了约 `13.6%`；
- 进程版的 `sys_s` 约为线程版的两个数量级以上，说明进入内核管理进程和地址空间的额外成本更高。

### 6.4 环境信息

[artifacts/tool_versions.txt](/root/os_experiments/lab5/task3/artifacts/tool_versions.txt) 中记录了本次环境：

```text
gcc (Debian 14.2.0-19) 14.2.0
Linux Laptop-SoongFS 6.6.87.2-microsoft-standard-WSL2 ...
glibc 2.41
4096
16
CPU(s): 16
Model name: AMD Ryzen 9 7940H w/ Radeon 780M Graphics
Thread(s) per core: 2
Core(s) per socket: 8
Socket(s): 1
```

因此本实验运行在：

- 16 个在线逻辑 CPU
- 8 个物理核心 / 16 线程
- WSL2 虚拟化环境

## 7. 机制解释与讨论

### 7.1 Linux 调度器眼里，线程和进程都要被调度

在 Linux 内核里，用户态线程和进程最终都会对应到可调度实体。  
从 CFS 的视角看，它们都要抢 CPU 时间片，因此“线程一定比进程更容易被调度”这种说法并不准确。

真正的差异不主要在“有没有调度”，而是在：

- 创建时要准备什么内核对象；
- 切换时要不要换地址空间；
- 运行中共享哪些资源。

### 7.2 地址空间差异：线程共享 `mm`，进程各有一套

线程版中，4 个 worker 都在同一个进程地址空间里：

- 共享同一套页表和同一个 `mm`；
- 共享堆、全局区、代码段；
- 只各自拥有独立栈和寄存器上下文。

进程版中，4 个 worker 是 4 个独立进程：

- 每个进程都有自己的地址空间和页表上下文；
- 彼此默认隔离，不能直接共享普通堆内存；
- 需要通过 `fork`、共享内存、pipe、socket 或其他 IPC 才能交换数据。

这正对应了实验里的 RSS 结果：

- 线程版平均 `rss_snapshot_kb ≈ 34304`
- 进程版平均 `rss_snapshot_kb ≈ 38976`

两者的“纯计算数据量”本来是一样的，但进程版仍然多出约 `4.6 MiB`，这部分主要来自：

- 多份用户栈；
- 多份 libc / runtime 私有状态；
- 独立地址空间带来的页表与管理开销；
- 父进程自身仍然驻留。

### 7.3 线程切换与进程切换：为什么会牵涉 TLB

线程切换若发生在同一进程内部，通常不需要切换到另一套用户地址空间。  
因此：

- 页表根和地址空间上下文通常不变；
- TLB 里和当前地址空间相关的条目更容易继续复用；
- 切换成本主要集中在寄存器、内核栈、调度实体状态和少量内核 bookkeeping。

进程切换则不同：

- 需要切换到另一套地址空间；
- 通常意味着切换页表根寄存器或等价的地址空间标识；
- 即使在现代 CPU 上可以用 PCID/ASID 一类机制减少“全量 TLB 清空”，不同进程之间仍然会带来更高的 TLB 压力和地址空间上下文管理成本。

所以，从机制上说：

- 线程切换通常比进程切换更轻；
- 进程切换更容易引入 TLB miss、页表上下文切换和更高的 system time；
- 这也是为什么题目特别要求从“地址空间隔离”和 “TLB 刷新”角度分析。

### 7.4 为什么这次实验里进程版 wall time 没有明显输给线程版

这次结果里：

- 线程版平均 `1.061 s`
- 进程版平均 `1.019 s`

进程版甚至略快一点，但这并不意味着“进程总比线程快”。  
更合理的解释是：

1. 本实验是粗粒度、低同步、几乎无 IPC 的计算任务；
2. 一旦 4 个 worker 都启动完成，它们就长时间在各自 core 上跑纯计算；
3. 在 16 个逻辑 CPU 的机器上，真正发生高频抢占切换的压力并不大；
4. 因此“进程切换比线程切换更重”的那部分差异，被长时间的纯计算阶段摊薄了。

但额外成本并没有消失，它体现在别的指标里：

- 进程版平均 `sys_s ≈ 0.030 s`
- 线程版平均 `sys_s ≈ 0.00043 s`

也就是说，进程版进入内核的管理开销仍然明显更高，只是没有在 wall time 上形成压倒性劣势。

### 7.5 本实验能得出的更稳妥结论

这次实验更适合得出下面这些结论，而不是简单喊“线程赢”或“进程赢”：

- 对粗粒度、长时间、低交互的计算任务，只要 worker 数不大于可用核心数，线程和进程都能把 CPU 利用率推到接近 `N * 100%`；
- 线程版通常更省内存，因为共享地址空间；
- 进程版通常 system time 更高，因为创建、等待和地址空间管理更重；
- 线程切换在机制上更轻，但如果任务足够粗粒度，这部分优势可能不会直接反映成明显更低的 wall time；
- 一旦任务粒度变细、同步更频繁、IPC 更重，进程版的劣势通常会更明显。

## 8. 验收检查映射

- [x] 提供两种版本的对比数据表（执行耗时、内存占用等）。
  证据：本 README 第 6.3 节的数据表，给出了线程版与进程版的 wall time、CPU 利用率、RSS 快照、sys time，并保留了串行基线。
- [x] 报告结合地址空间隔离、TLB 刷新等角度探讨进程切换和线程切换开销差异。
  证据：本 README 第 7.2 ~ 7.5 节。

## 9. 环境说明、复现限制与误差来源

- 本实验只在当前 WSL Debian 环境验证；没有额外在原生 Linux 服务器上复验。
- 本题使用的是宿主 Linux 的真实 `pthread` 与 `fork` 行为，不是 QEMU guest 教学内核 ABI。
- 由于这是性能实验，wall time 会受以下因素影响：
  - WSL2 虚拟化层调度；
  - CPU 睿频与温度；
  - L3 / 内存带宽竞争；
  - 同机其他负载；
  - 两次运行之间的缓存暖机差异。
- `rss_snapshot_kb` 是“预热完成、正式计算开始前”的快照，不是整个生命周期中的绝对峰值；选择这个口径是为了让线程版和进程版更可比。
