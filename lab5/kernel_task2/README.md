# LAB5 内核态 task2: 实现一种非 RR 调度算法

## 原始任务

> 完成LAB5 内核态task2：实现一种非 RR 调度算法
>
> 目标：理解调度策略设计的目标函数与工程代价。
>
> 要求：
> 1. 实现一种非 RR 调度算法（如 Stride/MLFQ/优先级等其一）；
> 2. 给出对照实验：至少两类任务（交互型/计算型）在不同调度下的表现差异；
> 3. 说明策略可能带来的饥饿/公平性问题及缓解手段（定性即可）。
>
> 验收检查：
> 1. 新调度算法生效，且调度逻辑准确（如高优先级任务分配了更多时间）；
> 2. 对照实验中能观察到 RR 算法与新算法在任务完成顺序/时机上的显著区别；
> 3. 报告中讨论了优先级翻转或饥饿陷阱。

## 实验目标与环境

- 执行环境：`LAB5` 内核态实验，运行在 `QEMU riscv64 virt` 客户机中的教学内核，不是宿主 Linux 内核。
- 对照策略：
  - `phase0`: `round_robin`
  - `phase1`: `static_priority`
- 非 RR 算法选择：静态优先级调度。数值越小优先级越高。
- 时间来源：CLINT `mtime/mtimecmp`，时间片长度固定为 `18000` tick。

本实现把两个策略放进同一个内核镜像中，按相同任务集顺序运行两轮，从而避免“不同构建、不同启动环境”带来的干扰。

## 文件列表

- `src/main.rs`
  内核主逻辑、任务定义、RR/静态优先级调度器、切换日志、阶段汇总与验收判断。
- `src/trap.rs`
  TrapFrame 定义与 trap 向量初始化。
- `src/syscall.rs`
  用户任务使用的 `yield`/`finish` 系统调用封装。
- `src/boot.S`
  启动入口与从内核态进入用户任务的汇编桥接。
- `linker.ld`
  内核与 3 个用户任务的栈布局。
- `artifacts/build_output.txt`
  最终成功构建输出。
- `artifacts/run_output.txt`
  第一轮完整运行日志。
- `artifacts/run_output_repeat.txt`
  第二轮重复运行日志。
- `artifacts/scheduler_objdump.txt`
  内核镜像反汇编。
- `artifacts/tool_versions.txt`
  Rust/QEMU 版本信息。

## 实现思路

### 1. 任务模型

实验固定 3 个用户任务：

- `interactive_proc`
  交互型任务。每轮做一小段计算后主动 `yield`，共 10 轮，优先级 `0`。
- `compute_short`
  计算型短任务。持续 CPU 密集计算，不主动让出，优先级 `2`。
- `compute_long`
  计算型长任务。持续 CPU 密集计算，不主动让出，优先级 `2`。

这样可以同时观察：

- 主动让出触发的切换：`explicit_yield`
- 时间片耗尽触发的切换：`time_slice`
- 高优先级交互任务是否更快完成
- 低优先级计算任务是否仍能完成

### 2. 调度器设计

- RR：`next_runnable_rr()` 从当前任务的后一个位置循环找下一个 `Runnable` 任务。
- 静态优先级：`next_runnable_priority()` 先找当前可运行任务中的最高优先级，再在同优先级内部做小范围轮转。
- 同优先级轮转依赖 `PRIORITY_LAST_PICK`，避免同一级内部总是固定命中第一个任务。
- 对于主动 `yield` 与任务退出路径，优先级调度会先排除当前任务，再选其他可运行任务；对时间片中断则允许当前高优先级任务在重新仲裁后继续占优。

### 3. 计数与证据

每个阶段都会记录并打印：

- `finish_tick`
- `runtime_ticks`
- `switch_ins`
- `explicit_yields`
- `time_slice_preemptions`
- `total_switches`
- `timer_interrupts`

阶段结束后，内核自动比较 RR 与静态优先级下交互任务的完成时间与“完成前 CPU 服务份额”：

- `service_share = runtime_ticks / finish_tick`

这个指标比单纯看 `switch_ins` 更合理，因为交互任务的主动 `yield` 次数是固定的，`switch_ins` 不足以表达“它在完成前拿到了多少 CPU 时间”。

## 关键代码位置

- 策略与任务定义：`src/main.rs:16-41`、`src/main.rs:200-233`
- Trap 分发与系统调用/时钟中断处理：`src/main.rs:298-408`
- 切换日志与阶段汇总：`src/main.rs:410-695`
- RR 与静态优先级选路：`src/main.rs:731-813`

## 构建与运行命令

在仓库根目录执行：

```bash
cd /root/os_experiments/lab5/kernel_task2
cargo build 2>&1 | tee artifacts/build_output.txt
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab5_kernel_task2 \
  > artifacts/run_output.txt 2>&1
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab5_kernel_task2 \
  > artifacts/run_output_repeat.txt 2>&1
cargo objdump --bin lab5_kernel_task2 -- --demangle -d \
  > artifacts/scheduler_objdump.txt
rustc --version > artifacts/tool_versions.txt
cargo --version >> artifacts/tool_versions.txt
rustup target list | grep riscv64gc >> artifacts/tool_versions.txt
qemu-system-riscv64 --version >> artifacts/tool_versions.txt
```

## 实际观测结果

### 1. 两轮对照数据表

| Run | Policy | interactive finish_tick | interactive runtime_ticks | interactive service_share | compute_short finish_tick | compute_long finish_tick | total_switches |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| run1 | RR | 404943 | 84126 | 20.7% | 708087 | 858525 | 48 |
| run1 | Static Priority | 192054 | 94313 | 49.1% | 616754 | 844776 | 44 |
| run2 | RR | 400591 | 78395 | 19.5% | 683119 | 870021 | 46 |
| run2 | Static Priority | 192695 | 96515 | 50.0% | 613851 | 875965 | 44 |

结论：

- 高优先级交互任务在静态优先级下完成时间大约缩短到 RR 的一半。
- 交互任务完成前拿到的 CPU 服务份额从约 `20%` 提升到约 `49%~50%`。
- 两个低优先级计算任务仍然都能完成，说明当前工作负载下没有出现“永久饿死”。
- 本组工作负载下完成顺序仍为 `interactive -> compute_short -> compute_long`，显著差异主要体现在完成时机，而不是顺序翻转。

### 2. 日志摘录

`artifacts/run_output.txt` 中的关键片段：

```text
[sched][round_robin] switch#01 reason=explicit_yield from pid=1(interactive_proc) runtime=15849 -> to pid=2(compute_short) runtime=0 priority=0->2
[sched][round_robin] switch#02 reason=time_slice from pid=2(compute_short) runtime=25691 -> to pid=3(compute_long) runtime=0 priority=2->2
...
[sched][static_priority] switch#01 reason=explicit_yield from pid=1(interactive_proc) runtime=10182 -> to pid=2(compute_short) runtime=0 priority=0->2
[sched][static_priority] switch#02 reason=time_slice from pid=2(compute_short) runtime=8670 -> to pid=1(interactive_proc) runtime=10182 priority=2->0
...
[kernel] comparison: interactive_finish_tick rr=404943 priority=192054 interactive_service_share=20%.7 rr_runtime=84126 priority_runtime=94313
[kernel] comparison: interactive_service_share_priority=49%.1 priority_switch_ins=11 rr_switch_ins=11
[kernel] acceptance high-priority interactive task favored under static_priority: PASS
[kernel] acceptance rr vs static_priority show clearly different completion timing: PASS
[kernel] acceptance low-priority compute tasks still completed in finite run: PASS
```

可以直接看出：

- RR 中，交互任务 `yield` 后，CPU 轮到两个计算任务循环执行。
- 静态优先级中，低优先级计算任务一旦时间片耗尽，会立即被高优先级交互任务重新抢回。
- `reason=explicit_yield` 和 `reason=time_slice` 两类触发原因在日志中被明确区分。

## 机制说明

### RR 与静态优先级的职责差异

- RR 的目标是“尽量平均地轮流给每个可运行任务一个时间片”，不关心任务类型。
- 静态优先级的目标是“优先缩短高优先级任务的等待时间与响应时间”，允许低优先级任务让路。

### 本实验中为什么交互任务明显更快

交互任务每次只做一小段计算，然后主动 `yield`。在 RR 下，它每次让出之后，两个计算任务都会依次占用时间片，所以它虽然总计算量不大，但完成得并不快。  
在静态优先级下，交互任务优先级最高，计算任务每次被时钟中断抢占后，调度器重新选择时会再次把 CPU 给交互任务，因此交互任务能更快完成全部 10 轮短突发。

### 为什么低优先级任务没有在本实验中被饿死

本实验中的高优先级交互任务是“短 burst + 主动 `yield` + 总轮数有限”，因此它很快结束。它结束后，剩余低优先级计算任务仍会继续获得 CPU 并完成。  
如果把高优先级任务改成无限循环且持续可运行，静态优先级会出现明显饥饿风险。

## 饥饿、公平性与优先级翻转讨论

### 1. 饥饿风险

静态优先级的直接问题是饥饿：

- 若高优先级任务长期保持可运行，低优先级任务可能长时间得不到运行机会。
- 这类策略牺牲的是全局公平性，换取的是高优先级任务的响应性。

### 2. 可能的缓解手段

- Aging：等待时间越长，逐步提升低优先级任务的动态优先级。
- Periodic boost：周期性把所有任务提升回高队列，防止长期饿死。
- Budget / quota：限制高优先级任务在一个窗口内可连续占用的 CPU 预算。
- Hybrid policy：高层按优先级选队列，队列内仍用 RR 保持局部公平。

### 3. 优先级翻转

即使调度器支持优先级，仍可能出现优先级翻转：

- 低优先级任务持有锁；
- 高优先级任务等待该锁；
- 中优先级任务不断运行，导致低优先级持锁者迟迟不能释放锁；
- 结果是高优先级任务反而被“间接阻塞”。

本实验没有实现锁竞争，但真实内核里应考虑：

- Priority Inheritance：临时提升持锁低优先级任务的优先级。
- Priority Ceiling：对关键资源设置优先级上限。

## 验收检查映射

### 1. 新调度算法生效，且调度逻辑准确

- 证据：
  - `artifacts/run_output.txt:66-69`
  - `artifacts/run_output_repeat.txt:66-69`
- 观测：
  - run1 中交互任务服务份额从 `20.7%` 提升到 `49.1%`
  - run2 中交互任务服务份额从 `19.5%` 提升到 `50.0%`
  - 两轮均输出 `acceptance high-priority interactive task favored under static_priority: PASS`

### 2. RR 与新算法在完成顺序/时机上有显著区别

- 证据：
  - `artifacts/run_output.txt:34-36` 与 `artifacts/run_output.txt:63-65`
  - `artifacts/run_output_repeat.txt:34-36` 与 `artifacts/run_output_repeat.txt:63-65`
- 观测：
  - 交互任务完成时间从 `404943/400591` tick 缩短到 `192054/192695` tick
  - 完成时机差异显著，超过 20%

### 3. 报告讨论了优先级翻转或饥饿陷阱

- 证据：本 README 的“饥饿、公平性与优先级翻转讨论”章节。

## 环境说明与复现限制

- 工具链版本见 `artifacts/tool_versions.txt`。
- 本实验测量的是同一次 QEMU 启动内、相同任务集下 RR 与静态优先级的相对差异。
- `mtime` tick 属于客户机时间基准，但具体数值仍会受宿主机调度、QEMU 翻译与缓存状态影响，因此更适合比较相对趋势，而不是把 tick 数值当成宿主物理时间。
- 为降低日志对系统的干扰，调度切换日志设置了上限 `18` 条；超过后只打印一次抑制提示。
