# LAB5 内核态 task4: 内核态中断响应与处理

## 原始任务

> 完成LAB5 内核态task4：内核态中断响应与处理
>
> 目标：在多核背景下正确处理中断，保证一致性与可重入边界。
>
> 要求：
> 1. 支持内核态中断处理
> 2. 明确中断上下文与普通上下文的差异
> 3. 给出验证：中断频繁触发时系统仍可稳定运行
>
> 验收检查：
> 1. 各核心的时钟中断和跨核中断（IPI）被正确投递和处理；
> 2. 中断处理程序执行过程中不会发生栈溢出或破坏正在持有的锁状态。

## 实验目标与环境

- 执行环境：`LAB5` 内核态实验，运行在 `QEMU riscv64 virt` 客户机中的教学内核，不是宿主 Linux 内核。
- 运行命令：`qemu-system-riscv64 -machine virt -bios none -nographic -smp 4`
- 中断类型：
  - 时钟中断：CLINT `mtime/mtimecmp` 产生的 `MTIP`
  - 跨核中断：CLINT `msip` 产生的 `MSIP`
- 设计目标：
  - 4 个 hart 在普通内核上下文中并发运行
  - 每个 hart 都独立接收并处理中断
  - 中断处理程序运行在专用中断栈上
  - 中断处理程序不获取共享自旋锁，只做最小原子更新和中断应答

## 文件列表

- `src/boot.S`
  多 hart 启动入口、secondary hart 释放屏障、M-mode trap 入口、寄存器现场保存与恢复。
- `src/main.rs`
  多核 bring-up、CLINT 定时器/IPI 配置、普通上下文工作负载、trap 分发、验收统计。
- `src/spinlock.rs`
  自旋锁实现，带 `acquisitions` 与 `contention_spins` 计数。
- `src/console.rs`
  串口输出，内部带 console lock，防止普通上下文并发打印互相打散。
- `linker.ld`
  镜像布局、每 hart boot stack 和每 hart interrupt stack 区域。
- `artifacts/build_output.txt`
  最终成功构建输出。
- `artifacts/run_output.txt`
  第一轮多核中断实验日志。
- `artifacts/run_output_repeat.txt`
  第二轮重复实验日志。
- `artifacts/interrupt_objdump.txt`
  中断相关镜像反汇编。
- `artifacts/tool_versions.txt`
  Rust/QEMU 版本信息。

## 实现思路

### 1. 普通上下文与中断上下文的边界

本实验显式区分两类上下文：

- 普通上下文：
  - 每个 hart 在自己的 boot stack 上运行 `worker_loop()`
  - 会获取 `SHARED_STATE` 自旋锁
  - 会进入临界区并故意停留一段时间，制造“持锁时被中断”的场景
- 中断上下文：
  - trap 入口先把 `sp` 切换到该 hart 的专用 interrupt stack
  - 只处理 `MTIP` 与 `MSIP`
  - 只做：
    - 重新设置 `mtimecmp`
    - 发送/清除 `msip`
    - 更新原子计数器
  - 不获取 `SHARED_STATE` 锁，不做会阻塞的操作

这是本题的关键工程边界：  
普通上下文可以持锁和执行较长逻辑；中断上下文必须短、小、可重入边界清晰。

### 2. 多核中断投递路径

- 每个 hart 在 barrier 之后分别：
  - 安装自己的 `mtvec`
  - 设置自己的 `mscratch` 为 interrupt stack 顶部
  - 设置本 hart 的 `mtimecmp`
  - 打开 `mie.MTIE`、`mie.MSIE` 和 `mstatus.MIE`
- 定时器中断处理程序：
  - 增加本 hart `timer_irqs`
  - 重新 arm 下一次 `mtimecmp`
  - 每隔固定次数向下一个 hart 发送一次 `MSIP`
- IPI 处理程序：
  - 清除本 hart 的 `msip`
  - 增加本 hart `ipi_irqs`

这样每个 hart 都既能收到自己的 timer interrupt，也能收到来自其他 hart 的软件 IPI。

### 3. 锁状态一致性验证

普通上下文使用 `SHARED_STATE: SpinLock<SharedKernelState>` 保护共享结构。  
额外维护两个原子状态用于自检：

- `ACTIVE_LOCK_HOLDER`
  记录当前持锁 hart
- `HART_IN_CRITICAL[hart]`
  记录某 hart 是否正处在临界区

中断处理程序若发现“当前 hart 在持锁临界区内被中断”，会增加：

- `interrupts_while_lock_held`

同时检查：

- `ACTIVE_LOCK_HOLDER == 当前 hart`

若不一致，就记为 `lock_state_violations`。  
最终两轮实验都得到：

- `interrupts_while_lock_held > 0`
- `lock_state_violations = 0`

这说明测试真正覆盖到了“持锁时被中断”的危险边界，但中断处理没有破坏锁状态。

### 4. 栈安全验证

每个 hart 单独分配 `8 KiB` interrupt stack。  
trap 入口把 `sp` 从普通内核栈切换到 interrupt stack，再保存完整寄存器现场。  
运行时记录：

- `last_sp`
- `min_sp`
- 每个 hart 的 `interrupt_stack` 区间

最终检查 `min_sp` 始终落在对应 hart 的 interrupt stack 范围内，用来证明没有发生 trap 栈越界。

## 关键代码位置

- 启动入口与 trap 汇编保存恢复：`src/boot.S`
- 多 hart 初始化、barrier、worker loop：`src/main.rs:141-285`
- trap 分发与 timer/IPI 处理：`src/main.rs:422-478`
- 锁状态自检与 summary：`src/main.rs:287-396`
- 自旋锁实现：`src/spinlock.rs:6-80`

## 构建与运行命令

在仓库根目录执行：

```bash
cd /root/os_experiments/lab5/kernel_task4
cargo build 2>&1 | tee artifacts/build_output.txt
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -smp 4 \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab5_kernel_task4 \
  > artifacts/run_output.txt 2>&1
timeout 30s qemu-system-riscv64 -machine virt -bios none -nographic -smp 4 \
  -kernel target/riscv64gc-unknown-none-elf/debug/lab5_kernel_task4 \
  > artifacts/run_output_repeat.txt 2>&1
cargo objdump --bin lab5_kernel_task4 -- --demangle -d \
  > artifacts/interrupt_objdump.txt
rustc --version > artifacts/tool_versions.txt
cargo --version >> artifacts/tool_versions.txt
rustup target list | grep riscv64gc >> artifacts/tool_versions.txt
qemu-system-riscv64 --version >> artifacts/tool_versions.txt
```

## 实际观测结果

### 1. 两轮摘要数据

| Run | ready_harts | booted_mask | shared_critical_sections | shared_lock acquisitions | shared_lock contention_spins | hart0 timer/IPI | hart1 timer/IPI | hart2 timer/IPI | hart3 timer/IPI |
| --- | ---: | --- | ---: | ---: | ---: | --- | --- | --- | --- |
| run1 | 4 | `0xf` | 48 | 50 | 414951 | 68 / 6 | 77 / 6 | 67 / 6 | 71 / 6 |
| run2 | 4 | `0xf` | 48 | 50 | 424720 | 62 / 6 | 72 / 6 | 81 / 6 | 65 / 6 |

结论：

- 两轮都成功唤醒 `4` 个 hart，`booted_mask=0xf` 说明 `hart0~hart3` 全部完成初始化。
- 每个 hart 都处理了远超阈值的 timer interrupt 和 6 次 IPI。
- 两轮都有大量 `interrupts_while_lock_held`，说明中断确实在临界区中打进来。
- `lock_state_violations=0`，说明中断处理没有破坏正在持有的锁状态。

### 2. 关键日志摘录

`artifacts/run_output.txt` 中的关键片段：

```text
[hart0] start barrier released: ready_harts=4 booted_mask=0xf
[hart0] progress: work_units=6 timer_irqs=28 ipi_irqs=6 interrupts_while_lock_held=11
[hart1] progress: work_units=6 timer_irqs=36 ipi_irqs=6 interrupts_while_lock_held=11
[hart3] progress: work_units=6 timer_irqs=38 ipi_irqs=6 interrupts_while_lock_held=9
[hart2] progress: work_units=6 timer_irqs=40 ipi_irqs=6 interrupts_while_lock_held=11
...
[kernel] summary: ready_harts=4 booted_mask=0xf finished_harts=4 shared_critical_sections=48 shared_checksum=0xb53508e195967978
[kernel] shared_lock: acquisitions=50 contention_spins=414951 lock_state_violations=0
[kernel] hart[0]: init_done=1 work_units=12 timer_irqs=68 ipi_sent=6 ipi_irqs=6 interrupts_while_lock_held=19 last_mepc=0x80002494
[kernel] hart[0]: interrupt_stack=[0x8001ccf0, 0x8001ecf0) min_sp=0x8001eae0 last_sp=0x8001eae0
...
[kernel] acceptance each hart handled timer interrupts and IPIs: PASS
[kernel] acceptance interrupt handlers preserved dedicated stacks and lock state: PASS
```

### 3. 普通上下文与中断上下文差异的直接证据

- 普通上下文：
  - 有 `work_units`
  - 有 `shared_critical_sections`
  - 会产生 `shared_lock` 的竞争统计
- 中断上下文：
  - 体现为 `timer_irqs`、`ipi_irqs`
  - 使用独立 `interrupt_stack`
  - 即使 `interrupts_while_lock_held` 非零，仍保持 `lock_state_violations=0`

这正是“中断上下文不应复用普通上下文的锁语义”的直接实验体现。

## 机制说明

### 1. 为什么选择 MSIP 作为 IPI

在 QEMU `virt` 平台的 CLINT 中：

- `mtimecmp[hart]` 负责每个 hart 的本地 timer interrupt
- `msip[hart]` 负责每个 hart 的 machine software interrupt

因此直接对目标 hart 的 `msip` 寄存器写 `1`，就是最直接的 IPI 模型。

### 2. 为什么中断处理程序不能拿普通自旋锁

若一个 hart 在普通上下文里拿着锁，又被中断打断；  
若中断处理程序再次试图拿同一把锁，那么：

- 该 hart 会在中断上下文里自旋等待自己释放锁
- 普通上下文又无法恢复执行
- 最终就是本地死锁

所以本实验的处理原则是：

- 普通上下文拿锁
- 中断上下文只做短路径、只更新原子变量
- 不在中断处理程序里获取共享自旋锁

### 3. 为什么专用 interrupt stack 很重要

若 trap 直接在普通内核栈上层层嵌套保存上下文，极端情况下更容易：

- 冲掉普通上下文的局部变量
- 在深调用路径下逼近栈边界

本实验把 trap 保存现场切换到每 hart 独立 interrupt stack，  
因此普通执行栈和中断保存栈被解耦，边界更清晰，也更容易做栈安全验证。

## 验收检查映射

### 1. 各核心的时钟中断和 IPI 被正确投递和处理

- 证据：
  - `artifacts/run_output.txt:6-12` 与 `artifacts/run_output.txt:26-35`
  - `artifacts/run_output_repeat.txt:6-11` 与 `artifacts/run_output_repeat.txt:26-35`
- 观测：
  - 两轮中每个 hart 的 `timer_irqs` 都远大于 `10`
  - 两轮中每个 hart 的 `ipi_irqs` 都为 `6`
  - `booted_mask=0xf`

### 2. 中断处理过程中不会栈溢出或破坏锁状态

- 证据：
  - `artifacts/run_output.txt:27-35`
  - `artifacts/run_output_repeat.txt:27-35`
- 观测：
  - `lock_state_violations=0`
  - 每个 hart 的 `interrupts_while_lock_held` 都非零，说明边界被实际覆盖
  - 每个 hart 的 `min_sp` 都落在对应 `interrupt_stack` 范围内

## 环境说明与复现限制

- 工具链版本见 `artifacts/tool_versions.txt`。
- 本实验采用的是 M-mode 裸机中断模型，没有引入 SBI、中断控制器抽象层或完整进程系统。
- `timer_irqs` 的绝对数值会受到宿主机调度和 QEMU 执行速度影响，因此更适合用来判断“是否稳定收到大量中断”，而不是做高精度性能测量。
- 当前 console lock 只用于普通上下文打印；中断处理程序刻意不打印，避免“打印锁在中断上下文中重入”这种反模式。
