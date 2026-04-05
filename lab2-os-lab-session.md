# LAB2 实验会话记录与仓库规范调整

## 对话背景

- 本次会话围绕操作系统课程实验仓库 `/root/os_experiments` 展开。
- 用户先后推进了多项 `LAB2` 任务，包括用户态与内核态实验，以及后续的仓库规范整理。
- 会话后半段的实际实现工作主要集中在：
  - `LAB2 内核态task2：系统调用编号与次数统计`
  - `LAB2 内核态task3：完成时间统计`
  - `LAB2 内核态task4：异常信息统计与现场输出`
  - `AGENTS.md` 规范补充与执行环境澄清
- 本记录只基于当前线程中用户可见的对话内容，不包含隐藏提示、私有推理或工具内部细节。

## 摘要

1. 用户在会话前半段连续给出多个 `LAB2` 任务。AI 在可见回复中先后报告了 `LAB2 用户态task1`、`LAB2 用户态task2`、`LAB2 用户态task3` 和 `LAB2 内核态task1` 的完成情况，附带路径、验证命令和提交信息。
2. 随后用户要求继续完成 `LAB2 内核态task2`。AI 采用独立目录 `lab2/kernel_task2`，实现了基于 TCB 的 syscall 直方图统计，复用用户态测试 workload，补充 README、artifact 和 `.gitignore`，并完成构建、QEMU 复验与提交。
3. 用户继续要求完成 `LAB2 内核态task3`。AI 采用独立目录 `lab2/kernel_task3`，改为基于 `CLINT mtime` 的完成时间统计，通过 QEMU `virt` 设备树确认 `timebase-frequency = 10_000_000 Hz`，使用 `1 次 warm-up + 3 次正式测量` 的策略稳定时间结果，完成 README、artifact、复验与提交。
4. 用户继续要求完成 `LAB2 内核态task4`。AI 采用独立目录 `lab2/kernel_task4`，将内核运行模式切换为 `S-mode`，通过 M-mode trap delegation 和最小 Sv39 页表实现真实的 `scause/sepc/stval` 诊断输出，并稳定触发 `Illegal Instruction` 与 `Store/AMO Page Fault`，同时保证 fault 后继续运行后续任务，最终完成 README、artifact、复验与提交。
5. 用户随后询问 `AGENTS.md` 是否需要调整。AI 先给出建议，再根据用户确认，将仓库级规范文件纳入版本控制并补充目录命名、RISC-V/QEMU 验证命令、README 最低要求、artifact 规则和执行环境区分等内容。
6. 用户进一步追问“用户态任务是否都在沙盒中运行”以及 “LAB0/LAB1 和 LAB2 的执行环境区别”。AI 明确解释：`LAB0/LAB1` 直接运行在宿主机 Linux 用户态；`LAB2` 开始，用户态与内核态都在 QEMU 的 RISC-V guest 中运行；随后把这一点正式补进 `AGENTS.md`。
7. 最后用户调用 `conversation-markdown-recorder`，要求将当前可见会话保存为 Markdown 文件。AI 依据技能要求，使用英文文件名并以中文写入本记录。

## 关键结论

- 可见会话中，AI 报告已完成的 `LAB2` 任务包括：
  - `lab2/task1`
  - `lab2/task2`
  - `lab2/task3`
  - `lab2/kernel_task1`
  - `lab2/kernel_task2`
  - `lab2/kernel_task3`
  - `lab2/kernel_task4`
- 本轮实际新增并完成提交的关键目录与提交为：
  - `lab2/kernel_task2`，提交 `79dde8c Add LAB2 kernel task2 syscall histogram`
  - `lab2/kernel_task3`，提交 `6aa6a76 Add LAB2 kernel task3 completion timing`
  - `lab2/kernel_task4`，提交 `874c853 Add LAB2 kernel task4 exception diagnostics`
  - `AGENTS.md` 初始纳入仓库，提交 `fa02259 Add repository AGENTS guidelines`
  - `AGENTS.md` 执行环境补充，提交 `bd42a77 Clarify lab execution environments`
- 关于执行环境，已在会话中明确并写入 `AGENTS.md`：
  - `LAB0/LAB1` 默认运行在宿主机 Linux 用户态
  - `LAB2+` 的用户态与内核态默认都运行在 QEMU 里的 RISC-V guest
- 本次记录保存为：
  - `lab2-os-lab-session.md`

## AI 显式分析与决策

- AI 明确决定为 `LAB2` 的内核态任务分别使用独立目录 `lab2/kernel_task2`、`lab2/kernel_task3`、`lab2/kernel_task4`，避免和已有用户态任务目录冲突。
- 在 `kernel_task2` 中，AI 明确选择复用 `lab2/task2` 已有 workload，而不是另写一套测试程序，以便让“内核侧统计趋势”和“用户态测试集趋势”可直接对照。
- 在 `kernel_task3` 中，AI 明确认为仅用 `rdcycle` 不足以满足题目对时间源说明的要求，因此改为基于 `CLINT mtime` 记账，并通过 QEMU `virt` 设备树确认 `timebase-frequency`。
- 在 `kernel_task3` 中，AI 观察到首轮测量存在明显冷启动效应，因此显式决定采用“每个应用 1 次 warm-up + 3 次正式测量，汇总排除 warm-up”的策略，以满足“多次运行合理波动”的验收要求。
- 在 `kernel_task4` 中，AI 明确判断如果继续沿用前面 M-mode 的最小框架，就很难自然满足 `scause/sepc/stval` 这一验收点，因此改为：
  - M-mode 只做启动与 delegation
  - 内核运行在 S-mode
  - 用户程序运行在 U-mode
  - 通过最小 Sv39 页表制造真正的 `Store Page Fault`
- 对于 `AGENTS.md`，AI 明确建议补充：
  - 用户态任务与内核态任务目录命名
  - RISC-V/QEMU 常用验证命令
  - README 最低要求
  - `target/`、`artifacts/` 提交规则
  - `LAB0/LAB1` 与 `LAB2+` 的执行环境区别

## 工具调用记录

1. `functions.exec_command`
   - 目的：检查仓库状态、查看现有目录、读取 README、查看技能说明和环境信息。
   - 关键动作：
     - `git status --short`
     - `sed -n ...`
     - `ls`
     - `find ... -name README.md`
     - `qemu-system-riscv64 --version`
   - 关键结果：确认已有任务布局、确认 `AGENTS.md` 当时尚未纳入版本控制、确认 QEMU 版本可用。

2. `functions.apply_patch`
   - 目的：创建和更新任务源码、README、`.gitignore`、`AGENTS.md` 和本次会话记录文件。
   - 关键结果：
     - 新增 `lab2/kernel_task2`
     - 新增 `lab2/kernel_task3`
     - 新增 `lab2/kernel_task4`
     - 新增并扩展 `AGENTS.md`
     - 新增本文件 `lab2-os-lab-session.md`

3. `functions.exec_command`
   - 目的：构建和运行 `LAB2` 裸机任务。
   - 关键命令：
     - `cargo build`
     - `qemu-system-riscv64 -machine virt -bios none -nographic -kernel ...`
   - 关键结果：
     - `kernel_task2` 稳定输出每个任务的 syscall histogram
     - `kernel_task3` 稳定输出 `mtime` 计时结果并给出波动范围
     - `kernel_task4` 稳定输出 `scause/sepc/stval`、故障指令与寄存器现场，并在故障后继续运行后续任务

4. `functions.exec_command`
   - 目的：为 `kernel_task3` 核实 QEMU 时间源。
   - 关键命令：
     - `qemu-system-riscv64 -machine virt,dumpdtb=...`
     - `strings -a ...`
     - `hexdump -C ...`
   - 关键结果：
     - 虽然未安装 `dtc/fdtget`，但通过 `dumpdtb + strings + hexdump` 成功确认 `clint@2000000` 和 `timebase-frequency = 0x00989680 = 10_000_000`

5. `functions.exec_command`
   - 目的：Git 提交本轮改动。
   - 关键命令：
     - `git add ...`
     - `git commit -m "..."`
   - 关键结果：
     - 生成多条提交，包括 `79dde8c`、`6aa6a76`、`874c853`、`fa02259`、`bd42a77`

## 原始对话记录

### 第一阶段：前置 LAB2 任务结果汇报

#### 用户

依次发起多个 `LAB2` 任务，请求完成：

- `LAB2 用户态task1：访问 get_taskinfo 并打印结果`
- `LAB2 用户态task2：系统调用统计与完成时间统计测试集`
- `LAB2 用户态task3：裸机调用栈打印`
- `LAB2 内核态task1：实现 get_taskinfo 系统调用`

随后又要求：

- “继续 LAB2 用户态task3”
- “继续完成 LAB2 内核态task1”

#### AI

AI 在可见回复中先后报告这些任务已经完成，并给出：

- 对应目录路径
- 构建与 QEMU 验证命令
- artifact 路径
- 提交号

这些回复构成了后续 `LAB2` 工作的上下文基础。

### 第二阶段：完成 `LAB2 内核态task2`

#### 用户

要求继续完成：

`LAB2 内核态task2：系统调用编号与次数统计`

并给出目标、要求和验收检查，包括：

- 每个应用执行期间触发的 syscall 编号与次数
- 多应用依次执行时的统计隔离
- 任务退出时打印 syscall 直方图

#### AI

AI 明确表示将：

- 使用 `os-lab-task-runner`
- 先检查 `lab2/kernel_task2` 状态和 skill 约束
- 复用已有 `lab2/task2` workload
- 新建独立目录而不是修改已有用户态任务目录

随后 AI：

- 创建并完善 `lab2/kernel_task2`
- 在 TCB 中加入 `SyscallStats`
- 仅在 `handle_syscall()` 中记录 syscall 号，避免内核初始化和切换路径污染统计
- 输出 per-task syscall histogram
- 补充 README 与 `.gitignore`
- 运行 `cargo build`
- 两次运行 QEMU 复验趋势
- 创建提交 `79dde8c Add LAB2 kernel task2 syscall histogram`

AI 最终向用户报告：

- `io_burst` 的 `write` 桶占优
- `compute_spin` 只有 `exit`
- `info_flood` 的 `get_taskinfo` 桶占优
- `illegal_trap` 没有 syscall 污染
- 所有检查均通过

### 第三阶段：完成 `LAB2 内核态task3`

#### 用户

要求继续完成：

`LAB2 内核态task3：完成时间统计`

并要求：

- 统计每个应用完成时间
- 说明时间源和单位
- 同一应用多次运行在合理波动范围内
- 使用 `mtime` 等硬件寄存器进行精确计时

#### AI

AI 明确说明：

- 将使用 `os-lab-task-runner`
- 检查相邻任务与 QEMU 时间源信息
- 准备用独立目录 `lab2/kernel_task3`
- 不直接复用 `rdcycle` 方案

随后 AI 做出关键决策：

- 通过 `qemu-system-riscv64 -machine virt,dumpdtb=...` 导出 QEMU `virt` DTB
- 使用 `strings` 和 `hexdump` 确认 `clint@2000000` 和 `timebase-frequency = 10_000_000 Hz`
- 采用 `mtime @ 0x0200bff8`
- 采用 `1 次 warm-up + 3 次 measured runs`
- 汇总时排除 warm-up

实际实现后，AI：

- 两次运行 QEMU
- 生成 `build_output.txt`、`run_output.txt`、`run_output_repeat.txt`、`qemu_timebase_probe.txt`
- 编写 README 解释换算关系与误差来源
- 创建提交 `6aa6a76 Add LAB2 kernel task3 completion timing`

AI 最终向用户报告：

- `io_burst`、`compute_spin`、`info_probe` 的平均耗时与波动范围
- `compute_spin` 始终最慢
- `mtime` 时间源与单位换算已在 artifact 中给出证据

### 第四阶段：完成 `LAB2 内核态task4`

#### 用户

要求继续完成：

`LAB2 内核态task4：异常信息统计与现场输出`

并要求：

- 触发异常时输出异常类型、出错地址、出错指令或 PC、必要寄存器信息
- 输出结构化日志，避免二次崩溃
- 解释异常现场如何从硬件 trap 上下文中获取
- 给出 `Store Page Fault / Illegal Instruction` 等异常现场

#### AI

AI 明确表示：

- 将继续使用 `os-lab-task-runner`
- 先检查 `kernel_task3` 的 trap 骨架
- 决定切到 `S-mode` 以满足 `scause/sepc/stval` 验收点
- 新建 `lab2/kernel_task4`

随后 AI 实现了：

- M-mode 启动与 trap delegation
- S-mode 内核
- U-mode 用户应用
- 最小 Sv39 页表
- `scause/sepc/stval` 异常采集
- 对 faulting instruction 的安全读取
- `Illegal Instruction` 和 `Store/AMO Page Fault` 两类 fault
- fault 后跳过故障任务并继续运行后续健康任务

AI 中途还修复了一次编译报错：

- `enter_supervisor` 与 `enter_user_mode` 需要显式 `unsafe`

之后 AI：

- 两次运行 QEMU 复验
- 生成 README 和 artifact
- 创建提交 `874c853 Add LAB2 kernel task4 exception diagnostics`

AI 最终向用户报告：

- `Illegal Instruction` 命中 `scause=0x2`
- `Store/AMO Page Fault` 命中 `scause=0xf`
- 两次 fault 后后续健康任务仍继续运行
- 6 项检查全部通过

### 第五阶段：讨论并修改 `AGENTS.md`

#### 用户

询问：

“`AGENTS.md` 这个文件你觉得有哪些需要调整的地方吗”

随后确认：

“同意修改”

#### AI

AI 先给出建议，重点包括：

- 目录命名约定
- RISC-V/QEMU 常用命令
- README 最低要求
- `.gitignore` 与 artifact 规则
- Rust 也应无 warning

在用户确认后，AI：

- 用 `apply_patch` 改写 [AGENTS.md](/root/os_experiments/AGENTS.md)
- 将其纳入版本控制
- 创建提交 `fa02259 Add repository AGENTS guidelines`

### 第六阶段：澄清 LAB0/LAB1 与 LAB2 的执行环境

#### 用户

先问：

“用户态任务是否都在沙盒中运行？”

后又追问：

“之前做的 LAB0 和 LAB1 没有区分用户态任务和内核态任务，而 LAB2 开始有了这种区分，我想问用户态和内核态是否都在 QEMU 中运行，还是直接在宿主机环境运行”

最后要求：

“补进 AGENTS.md 吧”

#### AI

AI 先解释：

- 早期 `LAB2` 任务只有特权级上的用户态，不具备完整内存沙盒
- `kernel_task4` 才引入了部分页表隔离

随后 AI 明确区分实验阶段：

- `LAB0/LAB1` 默认运行在宿主机 Linux 用户态
- `LAB2+` 的用户态和内核态默认都运行在 QEMU guest 中

之后 AI 将这一点补入 `AGENTS.md`，并创建提交：

- `bd42a77 Clarify lab execution environments`

### 第七阶段：保存当前会话记录

#### 用户

调用：

`$conversation-markdown-recorder`

并给出 `conversation-markdown-recorder` 技能内容。

#### AI

AI 确认将：

- 仅基于当前线程中的可见对话内容记录
- 不包含隐藏系统信息和私有推理
- 使用英文文件名
- 用中文写 Markdown

随后检查目标文件名是否冲突，确认 `lab2-os-lab-session.md` 可用，并将本记录写入当前工作目录。

## 后续动作

- 无
