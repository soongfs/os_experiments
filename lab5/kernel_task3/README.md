# LAB5 内核态 task3: 多核处理器支持

## 原始任务

> 完成LAB5 内核态task3：多核处理器支持
>
> 目标：理解 SMP 基本模型与并发内核的同步需求。
>
> 要求：
> 1. 支持多核启动与基本调度；
> 2. 引入必要同步原语（自旋锁等），避免共享结构数据竞争；
> 3. 给出验证：多核下并发运行任务且系统稳定。
>
> 验收检查：
> 1. 所有的 HART（Core）被成功唤醒并在内核中完成独立初始化；
> 2. 进程队列、内存分配器等全局数据结构加锁保护（Spinlock 机制生效）；
> 3. QEMU 开启多核（如 -smp 4）后应用正常执行不引发死锁或竞争崩溃。

## 实验目标与环境

- 执行环境：`LAB5` 内核态实验，运行在 `QEMU riscv64 virt` 客户机中的教学内核，不是宿主 Linux 内核。
- 运行模式：M-mode 内核，自举后直接运行内核工作队列，不引入 U-mode 进程。
- 多核配置：`qemu-system-riscv64 -machine virt -bios none -nographic -smp 4`
- SMP 目标：
  - 4 个 HART 全部启动并完成独立初始化
  - 多个 HART 从同一个全局运行队列中取任务并并发执行
  - 共享运行队列和共享 bump allocator 都由自旋锁保护

本实验用“内核工作队列”作为基本调度对象，重点验证 SMP 启动、共享数据保护与并发稳定性，而不是实现完整用户态进程系统。

## 文件列表

- `src/boot.S`
  多 HART 启动入口、secondary hart 释放屏障、每个 hart 的独立 boot stack 选择。
- `src/main.rs`
  主内核逻辑、SMP 启动流程、全局运行队列、共享分配器、工作负载执行、验收统计。
- `src/spinlock.rs`
  通用自旋锁实现，带 `acquisitions` 和 `contention_spins` 计数。
- `src/console.rs`
  串口输出与控制台锁，避免多核打印相互打散。
- `linker.ld`
  裸机镜像布局与 boot stack 区域。
- `artifacts/build_output.txt`
  最终成功构建输出。
- `artifacts/run_output.txt`
  第一轮 `-smp 4` 运行日志。
- `artifacts/run_output_repeat.txt`
  第二轮重复运行日志。
- `artifacts/smp_objdump.txt`
  内核镜像反汇编。
- `artifacts/tool_versions.txt`
  Rust/QEMU 版本信息。

## 实现思路

### 1. 多 HART 启动

- `hart0` 作为 primary hart，从 `_start` 进入 `start_primary()`。
- 其他 hart 先在汇编里轮询 `__boot_release_flag`，直到 primary 完成 `.bss` 清零和全局初始化后再进入 `start_secondary()`。
- 每个 hart 都会：
  - 读取自己的 `mhartid`
  - 拿到各自独立的 boot stack
  - 执行 `configure_pmp()`
  - 更新 `BOOTED_MASK` 与 `READY_HARTS`
  - 在 barrier 释放后进入调度循环

这样可以避免 secondary hart 在 primary 清理 `.bss` 期间提前触碰共享状态。

### 2. 基本调度

- 调度模型采用一个全局 FIFO `run_queue`。
- 队列中预置 8 个 kernel job，包含交互型短任务与计算型长任务两类。
- 每个 hart 在 `scheduler_loop()` 中：
  - 获取 `run_queue` 锁
  - 取出一个 job
  - 释放队列锁
  - 执行 job
  - 继续尝试领取下一个 job

这不是时间片抢占式调度，而是最小可验证的 SMP 基本调度：多个核从共享任务池并发消费工作。

### 3. 共享数据结构与自旋锁

本实验显式用 `SpinLock<T>` 保护两类全局共享结构：

- `RUN_QUEUE: SpinLock<RunQueue>`
  保护全局任务队列，防止多个 hart 同时修改队列游标。
- `ALLOCATOR: SpinLock<BumpAllocator>`
  保护共享 bump allocator，防止多个 hart 同时分配内存导致重叠区间。

锁实现见 `src/spinlock.rs`，核心机制是：

- `AtomicBool` 做互斥位
- `compare_exchange_weak()` 做抢锁
- 失败后 `spin_loop()` 自旋等待
- 记录 `acquisitions` 与 `contention_spins` 作为运行时证据

### 4. 共享分配器

- 共享堆大小固定为 `64 KiB`
- 分配器只做 bump 分配，不回收
- 每个 job 在运行前从共享堆里申请一段不重叠内存
- job 会把模式数据写入自己那段堆内存，再回读一部分字节生成 checksum

这使得“共享分配器被正确串行化”不只是代码声明，而是实际参与了多核并发执行路径。

## 关键代码位置

- 多 HART 入口与 secondary 释放：`src/boot.S:1-31`
- 任务队列、共享分配器、全局状态定义：`src/main.rs:17-227`
- primary/secondary 初始化与 barrier：`src/main.rs:235-321`
- 调度循环、取任务、执行 job：`src/main.rs:324-512`
- 汇总与验收判断：`src/main.rs:514-604`
- 自旋锁实现：`src/spinlock.rs:6-80`

## 构建与运行命令

在仓库根目录执行：

```bash
cd /root/os_experiments/lab5/kernel_task3
cargo build 2>&1 | tee artifacts/build_output.txt
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -smp 4 \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab5_kernel_task3 \
  > artifacts/run_output.txt 2>&1
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -smp 4 \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab5_kernel_task3 \
  > artifacts/run_output_repeat.txt 2>&1
cargo objdump --bin lab5_kernel_task3 -- --demangle -d \
  > artifacts/smp_objdump.txt
rustc --version > artifacts/tool_versions.txt
cargo --version >> artifacts/tool_versions.txt
rustup target list | grep riscv64gc >> artifacts/tool_versions.txt
qemu-system-riscv64 --version >> artifacts/tool_versions.txt
```

## 实际观测结果

### 1. 两轮摘要数据

| Run | ready_harts | booted_mask | completed_jobs | max_parallel_jobs | run_queue acquisitions | run_queue contention_spins | allocator allocations | allocator high_water |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| run1 | 4 | `0xf` | 8/8 | 4 | 2550 | 190237 | 8 | 17664 |
| run2 | 4 | `0xf` | 8/8 | 4 | 2515 | 111303 | 8 | 17664 |

关键结论：

- 两轮都成功唤醒 `4` 个 hart，`booted_mask=0xf` 说明 `hart0~hart3` 全部完成了初始化登记。
- 两轮都观测到 `max_parallel_jobs=4`，说明四个核都曾同时处于执行 job 的状态。
- 两把锁都被实际使用：`run_queue_lock` 与 `allocator_lock` 的获取次数均明显大于 0，且都有非零 `contention_spins`。
- 两轮都完整完成 `8/8` 个 job，没有 panic、死锁或竞争崩溃。

### 2. 关键日志摘录

`artifacts/run_output.txt` 中的关键片段：

```text
[hart2] init complete: role=secondary sp=0x8002ed30 ready_harts=4/4
[hart1] init complete: role=secondary sp=0x80032d30 ready_harts=3/4
[hart3] init complete: role=secondary sp=0x8002ad30 ready_harts=1/4
[hart0] init complete: role=primary sp=0x80036b50 ready_harts=2/4
[hart0] start barrier released: ready_harts=4 booted_mask=0xf

[hart1] schedule pick: job=4 name=batch_crc_b class=compute queue_remaining=4
[hart0] schedule pick: job=2 name=batch_crc_a class=compute queue_remaining=6
[hart3] schedule pick: job=3 name=ui_refresh class=interactive queue_remaining=5
[hart2] schedule pick: job=1 name=tty_echo class=interactive queue_remaining=7

[kernel] summary: ready_harts=4 booted_mask=0xf finished_harts=4 completed_jobs=8/8 max_parallel_jobs=4
[kernel] run_queue_lock: acquisitions=2550 contention_spins=190237 pop_count=8 remaining=0
[kernel] allocator_lock: acquisitions=10 contention_spins=740 allocations=8 high_water=17664 bytes
[kernel] acceptance all configured harts completed independent initialization: PASS
[kernel] acceptance global run queue and allocator were protected by spinlocks: PASS
[kernel] acceptance -smp 4 run completed without deadlock or crash: PASS
```

这组输出直接对应三项验收：

- 所有 hart 都打印了 `init complete`
- barrier 释放时 `booted_mask=0xf`
- 多个 hart 同时从共享队列取到不同 job
- 汇总阶段明确打印两把锁的统计和三条 `PASS`

## 机制说明

### 1. SMP 启动路径

- 汇编入口先按 `mhartid` 选择各自的栈顶。
- secondary hart 在汇编层等待 `__boot_release_flag`，不提前进入 Rust 共享状态。
- primary hart 完成 `.bss` 清零、队列初始化、分配器初始化后，才写 release flag 放行 secondary。
- 所有 hart 在 Rust 中再通过 `READY_HARTS + START_SCHEDULING` 完成第二层 barrier，同步进入调度循环。

这是最基本的 SMP bring-up 模式：先解决“谁先初始化全局状态”，再解决“什么时候所有核一起开始工作”。

### 2. 为什么要给运行队列和分配器加锁

若不加锁：

- 两个 hart 可能同时读取同一个 `run_queue.next`，导致同一个 job 被重复执行，或某个 job 被跳过。
- 两个 hart 可能同时从 bump allocator 读到同一个 `next_offset`，导致分配区间重叠，随后写坏彼此的数据。

因此 `run_queue` 和 `allocator` 都必须是临界区。  
本实验用同一种 `SpinLock<T>` 做保护，简化并发内核中共享结构的同步语义。

### 3. 为什么 `run_queue_lock` 获取次数远大于 `pop_count`

这是当前最小实现的工程代价：

- 队列被取空后，尚未结束的 hart 仍会轮询检查“还有没有新任务”；
- 每次检查都要获取 `run_queue_lock`；
- 因而 `acquisitions` 明显大于真正成功弹出的 `pop_count=8`。

这不是正确性问题，而是一个简化实现带来的性能代价。  
更进一步的改进方向包括：

- 引入 per-hart local queue
- 空队列时增加 backoff
- 使用更复杂的 work stealing 或等待唤醒机制

## 验收检查映射

### 1. 所有 HART 被成功唤醒并完成独立初始化

- 证据：
  - `artifacts/run_output.txt:13-20`
  - `artifacts/run_output_repeat.txt:13-19`
  - 两轮 summary 中 `ready_harts=4`、`booted_mask=0xf`

### 2. 进程队列 / 内存分配器等全局结构加锁保护

- 证据：
  - 代码：`src/main.rs:205-206`、`src/main.rs:361-451`
  - 锁实现：`src/spinlock.rs:6-80`
  - 运行日志：
    - `artifacts/run_output.txt:58-59`
    - `artifacts/run_output_repeat.txt:58-59`

### 3. `-smp 4` 下应用正常执行且不死锁不崩溃

- 证据：
  - 两轮都完成 `completed_jobs=8/8`
  - 两轮都输出三条 `PASS`
  - 两轮 QEMU 都正常退出

## 环境说明与复现限制

- 工具链版本见 `artifacts/tool_versions.txt`。
- 当前实现是“共享工作队列 + 自旋锁 + 轮询收敛”的基础 SMP 内核，不包含抢占式时钟调度、中断负载均衡或真正的进程地址空间。
- 本实验重点是 bring-up 和共享结构同步，因此选择了最小但可稳定验证的内核模型。
- 若把 job 数量、workload 长度或 `-smp` 参数改得很大，`run_queue_lock` 的轮询开销会继续上升，这是本简化实现的已知代价。
