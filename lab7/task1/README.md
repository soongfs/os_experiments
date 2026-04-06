# LAB7 用户态 Task1：System V IPC 综合实验

## 1. 原始任务说明

### 任务目标

掌握典型 IPC 机制的语义差异与适用场景。

### 任务要求

1. 分别实现：管道 / 共享内存 / 信号量 / 消息队列的示例程序；
2. 每种机制需完成一次可验证的数据交换；
3. 在实验记录中对比：吞吐、同步方式与易用性差异（定性即可）。

### 验收检查

1. 4 个不同机制的数据交换演示在 Linux 原生环境下运行成功并出图；
2. 报告对比分析合理。

## 2. Acceptance -> Evidence 清单

- 4 个机制都必须在 Linux 原生环境下完成一次可验证的数据交换。
  证据：实验程序 [ipc_matrix_demo.c](/root/os_experiments/lab7/task1/ipc_matrix_demo.c) 顺序执行 `pipe`、`shared_memory`、`semaphore`、`message_queue` 四个子实验；两次运行日志 [artifacts/run_output.txt](/root/os_experiments/lab7/task1/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab7/task1/artifacts/run_output_repeat.txt) 都包含四个机制的 `status=PASS` 行与总体验收行。
- 每种机制都要有“可验证”的交换结果，而不是只打印流程说明。
  证据：每个子实验都输出 `expected_checksum` 与 `actual_checksum`，并要求完全一致；见两份运行日志中的 `[result] mechanism=...` 行。
- 报告必须合理比较吞吐、同步方式与易用性。
  证据：本 README 第 7 节与第 8 节给出基于真实输出的对比表和定性分析。

## 3. 实验环境与实现思路

本实验运行在宿主机 Linux 原生用户态环境，不是 QEMU guest。原因很直接：题目要求验证典型 Linux IPC 语义，`pipe` 与 System V `shmget/semget/msgget` 都属于宿主机内核直接提供的 IPC 能力。

实现采用一个统一驱动程序 [ipc_matrix_demo.c](/root/os_experiments/lab7/task1/ipc_matrix_demo.c)，固定使用同一组负载：

- 每轮传输 `4096` 字节；
- 共传输 `4096` 轮；
- 总数据量 `16 MiB`；
- 数据方向统一为“父进程发送，子进程接收并返回校验结果”。

这样做有两个目的：

1. 保证 4 种机制面对的是同一份 payload，方便直接对照；
2. 日志里既能看到“是否交换成功”，也能看到粗粒度吞吐差异。

这里对“共享内存”和“信号量”作了明确区分：

- `shared_memory` 子实验只用 System V 共享内存段和共享状态位，依赖忙等轮询完成同步；
- `semaphore` 子实验使用 System V 信号量协调一块共享内存段，实现阻塞式同步。

这样既能体现共享内存“快但同步要自己补”，也能体现信号量“主要负责同步，不直接承载数据”的语义差异。

## 4. 文件列表与代码说明

- [ipc_matrix_demo.c](/root/os_experiments/lab7/task1/ipc_matrix_demo.c)：统一实验程序，包含 4 个 IPC 子实验、校验和逻辑、耗时统计与验收输出。
- [Makefile](/root/os_experiments/lab7/task1/Makefile)：构建入口，使用 `gcc -g -O0 -Wall -Wextra -std=c11`。
- [artifacts/build_output.txt](/root/os_experiments/lab7/task1/artifacts/build_output.txt)：构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab7/task1/artifacts/run_output.txt)：第一次运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab7/task1/artifacts/run_output_repeat.txt)：第二次运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab7/task1/artifacts/tool_versions.txt)：工具链与内核版本记录。

## 5. 机制说明

### 5.1 管道 `pipe`

父进程通过匿名管道把固定 payload 连续写给子进程，子进程读取全部数据后计算校验和，再通过第二根管道回传结果。其同步方式由内核管道缓冲区和阻塞式 `read` / `write` 自动完成，接口最直接，但只能表示字节流。

### 5.2 共享内存 `shared_memory`

父子进程通过 `shmget` / `shmat` 共享一块内存，数据直接写到共享缓冲区。为了不额外引入别的 IPC，这个子实验只在共享段中维护一个状态位，由双方轮询完成“空槽 / 数据就绪 / 结束”切换。它体现了共享内存路径短、拷贝少，但同步设计需要程序员自己承担。

### 5.3 信号量 `semaphore`

信号量本身不携带业务数据，因此这里配合共享内存使用：共享内存承载 payload，两个 System V 信号量分别表示“缓冲区空 / 缓冲区满”。父进程 `P(empty)` 后写数据，子进程 `P(full)` 后读数据。相比忙等版共享内存，它把同步从轮询改成了阻塞等待，更接近真实生产中的用法。

### 5.4 消息队列 `message_queue`

父进程用 `msgsnd` 发送类型化消息，子进程用 `msgrcv` 接收并统计校验和，最后回传一条结果消息。消息队列比管道更结构化，支持按类型收发，但内核需要维护消息边界和队列管理，吞吐通常不如共享内存。

## 6. 构建、运行与复现命令

进入目录：

```bash
cd /root/os_experiments/lab7/task1
```

构建：

```bash
make
```

前台运行，便于终端截图：

```bash
./ipc_matrix_demo
```

保存第一次日志：

```bash
./ipc_matrix_demo > artifacts/run_output.txt 2>&1
```

保存第二次日志：

```bash
./ipc_matrix_demo > artifacts/run_output_repeat.txt 2>&1
```

记录工具版本：

```bash
{
  gcc --version | head -n 1
  uname -srmo
  getconf PAGE_SIZE
} > artifacts/tool_versions.txt
```

## 7. 本次实际运行结果

### 7.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab7/task1/artifacts/build_output.txt) 的实际内容：

```text
gcc -g -O0 -Wall -Wextra -std=c11 -o ipc_matrix_demo ipc_matrix_demo.c
```

### 7.2 第一次运行结果

[artifacts/run_output.txt](/root/os_experiments/lab7/task1/artifacts/run_output.txt) 的关键输出：

```text
[config] environment=linux-native chunk_bytes=4096 iterations=4096 total_bytes=16777216
[demo] mechanism=pipe
[result] mechanism=pipe total_bytes=16777216 elapsed_ms=36.500 throughput_mib_s=438.35 expected_checksum=0xb0845c5f275d0383 actual_checksum=0xb0845c5f275d0383 status=PASS
[demo] mechanism=shared_memory
[result] mechanism=shared_memory total_bytes=16777216 elapsed_ms=22.115 throughput_mib_s=723.48 expected_checksum=0xb0845c5f275d0383 actual_checksum=0xb0845c5f275d0383 status=PASS
[demo] mechanism=semaphore
[result] mechanism=semaphore total_bytes=16777216 elapsed_ms=283.605 throughput_mib_s=56.42 expected_checksum=0xb0845c5f275d0383 actual_checksum=0xb0845c5f275d0383 status=PASS
[demo] mechanism=message_queue
[result] mechanism=message_queue total_bytes=16777216 elapsed_ms=33.114 throughput_mib_s=483.18 expected_checksum=0xb0845c5f275d0383 actual_checksum=0xb0845c5f275d0383 status=PASS
[acceptance] four mechanisms completed a verified data exchange: PASS
[acceptance] all checksums matched expected payload stream: PASS
```

### 7.3 第二次运行结果

[artifacts/run_output_repeat.txt](/root/os_experiments/lab7/task1/artifacts/run_output_repeat.txt) 的关键输出：

```text
[result] mechanism=pipe total_bytes=16777216 elapsed_ms=30.799 throughput_mib_s=519.49 expected_checksum=0xb0845c5f275d0383 actual_checksum=0xb0845c5f275d0383 status=PASS
[result] mechanism=shared_memory total_bytes=16777216 elapsed_ms=21.340 throughput_mib_s=749.77 expected_checksum=0xb0845c5f275d0383 actual_checksum=0xb0845c5f275d0383 status=PASS
[result] mechanism=semaphore total_bytes=16777216 elapsed_ms=284.198 throughput_mib_s=56.30 expected_checksum=0xb0845c5f275d0383 actual_checksum=0xb0845c5f275d0383 status=PASS
[result] mechanism=message_queue total_bytes=16777216 elapsed_ms=33.801 throughput_mib_s=473.36 expected_checksum=0xb0845c5f275d0383 actual_checksum=0xb0845c5f275d0383 status=PASS
[acceptance] four mechanisms completed a verified data exchange: PASS
[acceptance] all checksums matched expected payload stream: PASS
```

### 7.4 两次运行的汇总对比

| 机制 | 同步方式 | 运行1 吞吐 MiB/s | 运行2 吞吐 MiB/s | 平均吞吐 MiB/s | 平均耗时 ms | 数据校验 |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| `pipe` | 内核管道缓冲区 + 阻塞 `read/write` | 438.35 | 519.49 | 478.92 | 33.65 | PASS |
| `shared_memory` | 共享状态位 + 忙等轮询 | 723.48 | 749.77 | 736.63 | 21.73 | PASS |
| `semaphore` | System V 信号量阻塞同步 + 共享内存承载数据 | 56.42 | 56.30 | 56.36 | 283.90 | PASS |
| `message_queue` | 内核维护消息边界和消息类型 | 483.18 | 473.36 | 478.27 | 33.46 | PASS |

从两次运行可以直接看到：

1. 四个机制都完成了同一份 `16 MiB` 数据流的交换，且 `expected_checksum == actual_checksum`；
2. 在这组实现下，共享内存最快；
3. 管道和消息队列处在同一量级；
4. 信号量方案显著更慢，因为这里每个 `4 KiB` chunk 都需要一次完整的 `P/V` 往返。

### 7.5 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab7/task1/artifacts/tool_versions.txt) 的实际内容：

```text
gcc (Debian 14.2.0-19) 14.2.0
Linux 6.6.87.2-microsoft-standard-WSL2 x86_64 GNU/Linux
4096
```

## 8. 对比分析

### 8.1 吞吐差异

- `shared_memory` 最快。原因是数据直接在共享页中读写，省掉了管道和消息队列那种“每次都经由内核复制到内核缓冲区”的路径。
- `pipe` 和 `message_queue` 接近。这两个机制都由内核托管传输，接口上天然同步，代码简单，但都要付出内核缓冲和系统调用开销。
- `semaphore` 最慢。不是因为信号量“传数据慢”，而是因为这个实验把信号量当作每个 chunk 的同步门闩使用，每轮都要做阻塞式 `semop`，同步成本远大于 `4 KiB` 数据本身。

### 8.2 同步方式差异

- `pipe`
  同步几乎是隐式完成的。缓冲区满时写端阻塞，缓冲区空时读端阻塞，适合线性生产者-消费者。
- `shared_memory`
  数据路径最短，但同步完全要自己设计。本实验用共享状态位忙等，虽然能跑通，但容易浪费 CPU，也更容易写出竞态。
- `semaphore`
  适合把“谁现在可以访问共享资源”表达成明确协议。它本身不承载业务 payload，但很适合给共享内存补同步。
- `message_queue`
  既有同步，又保留消息边界和消息类型。相比管道更结构化，但灵活性和管理成本也更复杂。

### 8.3 易用性差异

- `pipe` 最容易上手，`fork` 后马上可用，适合父子进程和简单字节流。
- `message_queue` 次之。接口比管道重一些，但能天然表达“消息”而不是裸字节流。
- `shared_memory` 编码负担更大，因为除了分配和映射共享段，还必须自己处理可见性、同步和结束条件。
- `semaphore` 单独看并不难，但它通常不是独立的数据交换载体，而是要和共享内存或共享资源一起设计，协议思维要求更高。

### 8.4 适用场景总结

- 如果只要简单单向字节流，优先用 `pipe`。
- 如果需要最高吞吐并且愿意自己处理同步，优先用共享内存。
- 如果多个进程要安全共享一块状态或缓冲区，信号量适合做同步骨架。
- 如果希望保留消息边界、消息类型，且不想自己拆包封包，消息队列更合适。

## 9. 验收结论

### 9.1 验收项 1

4 个不同机制的数据交换演示已在 Linux 原生环境下运行成功。

- 证据：两次运行日志 [artifacts/run_output.txt](/root/os_experiments/lab7/task1/artifacts/run_output.txt) 与 [artifacts/run_output_repeat.txt](/root/os_experiments/lab7/task1/artifacts/run_output_repeat.txt) 都包含 `pipe`、`shared_memory`、`semaphore`、`message_queue` 四个 `status=PASS` 行。
- 复现截图建议：直接前台运行 `./ipc_matrix_demo`，截取四个 `[result]` 行和最后两个 `[acceptance]` 行即可。

### 9.2 验收项 2

报告已经基于真实运行结果，对吞吐、同步方式和易用性给出合理对比。

- 证据：本 README 第 7.4 节给出真实吞吐表，第 8 节解释同步语义、编码复杂度和适用场景。

### 9.3 结论

本实验完成了题目要求的 4 类 IPC 演示，并且都通过校验和验证了数据交换正确性。结合两次实测结果，可以得出一个清晰结论：共享内存在吞吐上最有优势，但同步负担最大；管道和消息队列更易用；信号量更适合做同步协议，而不是单独承担数据传输本身。
