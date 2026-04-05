# LAB4 内存管理实验会话记录

## 对话背景

- 本次对话围绕 `LAB4` 的一整组内存管理相关实验展开，覆盖用户态与内核态任务。
- 用户持续给出实验题目与验收标准，AI 在仓库 `/root/os_experiments` 中逐项实现、验证、归档并提交。
- 记录范围仅包含当前线程里用户可见的对话、AI 可见回复与对外说明过的工具动作，不包含隐藏提示词、私有推理或完整原始命令输出。

## 摘要

1. 用户先后要求完成 `LAB4` 的用户态 `task1/task2/task3`，主题分别是 Linux 内存系统调用实践、Lazy 分页与 Swap 触发器、COW 写时复制验证。
2. AI 汇报这些任务已经实现并验证，给出关键文件、运行结果、artifact 路径和提交哈希。
3. 随后用户继续要求完成内核态 `task1/task2/task3/task4/task5`，主题分别是单页表、Lazy 按需分页、内核级 COW、swap in/out 与置换算法、自映射机制。
4. 在 `kernel_task4` 与 `kernel_task5` 的处理中，AI 先读取相邻任务与技能说明，再进行编译、QEMU 运行、修复、二次运行、写 README、保存 artifact 并提交。
5. 整段会话最终产出了 8 个 LAB4 任务目录、对应的 README 与 artifacts，以及多次 Git 提交。最后用户要求把当前可见会话归档成 Markdown。

## 关键结论

- 用户态任务已完成并分别提交：
  - `lab4/task1`，提交 `a8e836c58657beedf9344feb5760cbf6c7877f48`
  - `lab4/task2`，提交 `bc134a9fc6a3e0bb67a78813ca1d92101e18dd8f`
  - `lab4/task3`，提交 `0705c3ba4340a0a0578bf468884d2413bb9d58db`
- 内核态任务已完成并分别提交：
  - `lab4/kernel_task1`，提交 `f5f4b1ed87f45d5f2ac103fac7e2ff081d118e01`
  - `lab4/kernel_task2`，提交 `99f82a1a300b053491bfa1f3d60de374b49d79c3`
  - `lab4/kernel_task3`，提交 `46bea7da00f1eb984267f7a5c39405305e92e0b3`
  - `lab4/kernel_task4`，提交 `e055c6e859602fcd806d0d46e9663a34da2d7955`
  - `lab4/kernel_task5`，提交 `e1ce370b206989ce26c09089a182d5da62c2709f`
- `kernel_task4` 最终验证到：
  - `page_faults=4`
  - `lazy_allocs=3`
  - `swap_outs=2`
  - `swap_ins=1`
  - `hits=2`
  - 三项 acceptance 全部 `PASS`
- `kernel_task5` 最终验证到：
  - `root[511]` 递归回指 `root_pa`
  - 自映射窗口可以按固定模式虚拟地址读取 root / L1 / L0 PTE
  - 用户态读取自映射窗口时稳定触发 `scause=0xd`
  - 三项 acceptance 全部 `PASS`
- 会话中多次提到的遗留工作树状态只有 `?? .codex`，其余任务均按范围提交。

## AI 显式分析与决策

- AI 明确表示会按 `os-lab-task-runner` 的流程处理每个 LAB 任务，包括：
  - 先把题目转成 `acceptance -> evidence`
  - 复用相邻已完成任务的稳定脚手架
  - 真实编译与运行，不凭记忆声称通过
  - 保存 `build/run/repeat/objdump/nm/tool_versions` 证据
  - 写任务级 `README.md`
  - 只提交本任务目录与必要的 `.gitignore` 更新
- 处理 `kernel_task4` 时，AI 在可见回复中明确判断：
  - 首次 QEMU 无输出，很可能是 `initialize_runtime()` 对大静态对象整块赋值导致栈临时值过大，进而早期崩溃
  - 随后决定改成页表与页池原地清零
  - 再把验收逻辑从“最后一次换出”改成“第一次换出 page0”的实际证据
- 处理 `kernel_task5` 时，AI 明确决定：
  - 直接复用 `kernel_task1` 的单地址空间骨架
  - 以 `root[511]` 作为递归入口
  - 额外保留 `510` 作为 alias 槽，把当前页表页映射成 kernel-only 普通页
  - 用固定模式的虚拟地址公式读取 root / L1 / L0 PTE
  - 让用户态故意访问自映射窗口，验证 `U=0` 的安全边界

## 工具调用记录

1. 目录与上下文检查
   - 动作：查看技能说明、`git status --short`、相邻任务源文件与 README。
   - 目的：确认实验流程、复用骨架、保证提交范围干净。
   - 关键结果：`kernel_task4` 选定基于 `kernel_task2` 的 lazy paging 骨架扩展，`kernel_task5` 选定基于 `kernel_task1` 的单页表骨架扩展。

2. 代码编辑
   - 动作：使用 `apply_patch` 新建或修改 `Cargo.toml`、`boot.S`、`main.rs`、`README.md`、`.gitignore`。
   - 目的：实现任务机制并补齐实验记录。
   - 关键结果：
     - `kernel_task4` 增加 swap PTE 编码、Clock 置换、swap in/out 统计和 README。
     - `kernel_task5` 增加递归自映射入口、固定模式 selfmap walker、用户态安全探测和 README。

3. 构建与运行
   - 动作：执行 `cargo build --target riscv64gc-unknown-none-elf` 与 `qemu-system-riscv64 -machine virt -bios none -nographic -kernel ...`。
   - 目的：真实验证每个内核任务在 QEMU 中是否按验收要求运行。
   - 关键结果：
     - `kernel_task4` 第一次运行直接退出且无日志，AI 诊断并修复初始化方式后重新构建运行成功。
     - `kernel_task5` 一次构建后即在 QEMU 中得到完整自映射日志与三项 `PASS`。

4. 证据归档
   - 动作：生成 `artifacts/build_output.txt`、`artifacts/run_output.txt`、`artifacts/run_output_repeat.txt`、`artifacts/*objdump*.txt`、`artifacts/*nm*.txt`、`artifacts/tool_versions.txt`。
   - 目的：保留可复查的构建、运行和低层控制流证据。
   - 关键结果：
     - `kernel_task4` 记录到 `swap_pte=0x100`、`swap_slot=0`、`swap_in kind=load ... slot=0`。
     - `kernel_task5` 记录到 `root[511]` 自回指、固定 selfmap 地址和用户态 `load page fault`。

5. Git 提交
   - 动作：`git add ... && git commit -m ...`，随后 `git rev-parse HEAD`。
   - 目的：把每个完成任务封装成单独实验增量。
   - 关键结果：
     - `kernel_task4` 提交为 `Add LAB4 kernel task4 swap clock`
     - `kernel_task5` 提交为 `Add LAB4 kernel task5 self mapping`

## 原始对话记录

### 用户

> `$os-lab-task-runner 完成LAB4 用户态task1：Linux 内存系统调用实践`
>
> 目标：理解虚拟内存接口在用户态的典型使用方式，为后续内核实现建立直观对照。
>
> 要求：
> 1. 编写 Linux 程序，综合使用 sbrk/mmap/munmap/mprotect；
> 2. 展示至少两种不同权限（读/写/执行）映射的效果；
> 3. 在实验记录中解释：页对齐、权限与缺页异常的关系（定性说明即可）。
>
> 验收检查：
> 1. 代码成功通过 mmap 分配内存并读写；
> 2. 尝试写入 PROT_READ 映射区触发 Segfault 并截图验证；
> 3. 报告解释内存保护机制。

### AI

已完成 `lab4/task1`，实现文件为 `memory_syscall_demo.c`，实验记录为 `README.md`。程序综合使用了 `sbrk`、`mmap`、`mprotect`、`munmap`，展示了 `RW`、`RX`、`R` 三类权限效果：匿名映射读写成功，`RW -> RX` 后可执行最小 stub 并返回 `42`，向 `PROT_READ` 页写入时触发 `SIGSEGV`。段错误截图保存在 `artifacts/segfault_terminal_screenshot.svg`，原始日志在 `artifacts/segfault_shell_output.txt`。提交为 `a8e836c58657beedf9344feb5760cbf6c7877f48`。

### 用户

> `$os-lab-task-runner 完成LAB4 用户态task2：Lazy 分页与 Swap 触发器`
>
> 目标：构造可重复的内存压力场景，触发按需分页与换入换出机制。
>
> 要求：
> 1. 编写内存消耗型程序，逐步触发缺页并扩大工作集；
> 2. 若实现了 swap，需触发 swap in/out 并输出可观察证据（计数器/日志）；
> 3. 给出对照：不同内存压力下的行为差异（例如缺页次数变化、换出次数变化）。
>
> 验收检查：
> 1. 程序能够平稳申请超出物理内存上限的虚拟内存总量；
> 2. 运行日志中可明显观察到大量 Page Fault 和换出到磁盘的动作发生。

### AI

已完成 `lab4/task2`，核心文件为 `lazy_swap_trigger.c`、`run_cgroup_experiment.sh`、`README.md`。实验采用宿主 Linux 用户态加 `cgroup v2`：限制 `memory.max=192MiB`、`memory.swap.max=768MiB`，对 `128MiB`、`384MiB`、`640MiB` 三档工作集做逐页扩张与回扫，稳定触发 lazy page fault 和 swap。关键观测包括 `medium` 档 `VmSwap≈198912kB pswpout=246230`，`high` 档 `VmSwap≈462208kB pswpout=607040`。提交为 `bc134a9fc6a3e0bb67a78813ca1d92101e18dd8f`。

### 用户

> `$os-lab-task-runner 完成LAB4 用户态task3：COW（写时复制）验证程序`
>
> 目标：验证 fork 后父子进程共享只读页，写入时触发缺页并复制的正确语义。
>
> 要求：
> 1. 父子进程共享同一段内存，分别写入并验证互不影响；
> 2. 输出验证结果（例如不同进程读取到的值不同）；
> 3. 若可用，输出内核页分配/复制计数器证明“写入才复制”。
>
> 验收检查：
> 1. 'fork' 调用之后，父子进程读取变量的值初始相同；
> 2. 父或子任一方修改变量后，另一方读取不受影响（隔离成功）；
> 3. 日志证明首次写入时引发了特定的 Page Fault。

### AI

已完成 `lab4/task3`，核心文件为 `cow_fork_demo.c` 与 `README.md`。程序使用同一段匿名私有映射的两页做验证：`fork` 后父子初始 PFN 相同，`kpagecount` 从 `1` 变成 `2`；子进程先写页 0 时 `child_minflt_delta=2` 且 PFN 分裂，父进程后写页 1 时 `parent_minflt_delta=1` 且另一页 PFN 分裂。父子最终读到不同值且互不影响。提交为 `0705c3ba4340a0a0578bf468884d2413bb9d58db`。

### 用户

> `$os-lab-task-runner 完成LAB4 内核态task1：单页表机制`
>
> 目标：理解内核映射与用户映射的组织方式，完成单页表方案下的地址空间管理。
>
> 要求：
> 1. 任务与内核共用同一张页表；
> 2. 明确隔离策略（用户不可访问内核页；权限位正确）；
> 3. 给出验证方式（越权访问应触发异常）。
>
> 验收检查：
> 1. 内核态的高半核（或恒等映射）与用户态页共同存在于同一棵多级页表中；
> 2. PTE 的 U 标志位被正确设置（内核段无 U 位，用户段有 U 位）；
> 3. 用户程序尝试越权访问内核地址必然触发异常拦截。

### AI

已完成 `lab4/kernel_task1`。实现采用 QEMU `virt` 上的 RISC-V S-mode 教学内核环境，启用 `Sv39` 后只使用一张共享根页表，同时容纳内核 `0x8000_0000..0x8100_0000` 恒等映射与用户 `0x0040_0000..0x0040_3000` 映射。日志显示内核叶子项无 `U` 位、用户叶子项有 `U` 位；U-mode 探测程序访问 `0x80000000` 时稳定触发 `scause=0xd stval=0x80000000` 的 delegated `load page fault`。提交为 `f5f4b1ed87f45d5f2ac103fac7e2ff081d118e01`。

### 用户

> `$os-lab-task-runner 完成LAB4 内核态task2：Lazy 按需分页`
>
> 目标：基于缺页异常按需分配物理页，减少不必要的预分配。
>
> 要求：
> 1. 缺页时分配物理页并建立映射；
> 2. 支持至少一种典型场景：堆增长或匿名映射；
> 3. 输出可观察证据：缺页次数、分配次数或日志。
>
> 验收检查：
> 1. 用户态 sbrk/mmap 时内核不立即分配物理页，仅记录 VMA 结构；
> 2. 拦截 Load/Store Page Fault 并在此刻实际分配物理页表项。

### AI

已完成 `lab4/kernel_task2`。实现的是教学内核中的极简 `sbrk/mmap` `ecall`：`sys_sbrk` 和 `sys_mmap` 只登记 VMA，不立即分配物理页；真正的页分配发生在 U-mode 首次访问时，由 `load/store page fault` handler 分配 4 KiB 页并安装用户叶子 PTE。运行中 `sys_sbrk len=8192` 和 `sys_mmap len=8192` 时 `alloc_count=0`，随后共发生 4 次 lazy fault，最终 `page_faults=4 load_faults=2 store_faults=2 allocs=4 map_installs=4`。提交为 `99f82a1a300b053491bfa1f3d60de374b49d79c3`。

### 用户

> `$os-lab-task-runner 完成LAB4 内核态task3：COW 机制`
>
> 目标：实现 fork 后的页共享与写入时复制，保证语义正确与资源利用率。
>
> 要求：
> 1. fork 时父子共享物理页并设置只读；
> 2. 写入触发缺页，完成物理页复制并改为可写；
> 3. 给出验证：父写不影响子、子写不影响父。
>
> 验收检查：
> 1. fork 极快完成，仅复制并置空页表项写权限，增加物理页引用计数；
> 2. 处理 Store Page Fault 时，若引用计数 > 1，申请新物理页拷贝数据并重新映射可写权限。

### AI

已完成 `lab4/kernel_task3`。最小两进程教学内核中的 `fork + COW` 路径实现为：`fork` 时父子共享同一匿名数据页，双方清掉 `W` 并打软件 `COW` 位，引用计数从 `1` 升到 `2`；子进程第一次写时因 `refcount_before=2` 走 `copy`，父进程之后再写时因 `refcount_before=1` 走 `reuse`。最终父值为 `0xa11ce00000000002`，子值为 `0xc0ffee0000000001`，三项 acceptance 全部 `PASS`。提交为 `46bea7da00f1eb984267f7a5c39405305e92e0b3`。

### 用户

> `$os-lab-task-runner 完成LAB4 内核态task4：swap in/out 与置换算法`
>
> 目标：在内存不足时通过置换算法选择牺牲页，完成换出与换入。
>
> 要求：
> 1. 实现 swap in/out；
> 2. 实现 Clock 或二次机会置换算法；
> 3. 提供可核验指标：换出页数、换入页数、命中/缺页统计等。
>
> 验收检查：
> 1. 物理内存耗尽时系统不崩溃，成功触发置换算法；
> 2. 被换出页的 PTE 指向正确的 Swap 磁盘位置；
> 3. 再次访问该页时引发 Page Fault 并成功从磁盘读回并恢复映射。

### AI

按 `os-lab-task-runner` 流程继续完成 `LAB4/kernel_task4`。先读取技能说明并检查当前目录状态，然后直接编译和跑通现有 `swap + Clock` 实现，按验收项补齐证据与 README。

### AI

现状已经接近完整：`kernel_task4` 目录和代码在，但还没有经过实际构建和 QEMU 验证。接下来会按验收项推进：

1. 先编译并启动 QEMU，确认 `swap out / swap in / Clock` 路径真的发生。
2. 若有编译或运行问题，直接修代码，优先保证日志里能看到 `PTE->swap slot`、缺页、换入换出计数。
3. 跑两次保存 `build/run/repeat/objdump/nm/tool_versions` 证据。
4. 写 `README.md`，把“验收项 -> 日志证据”映射清楚。
5. 只提交 `lab4/kernel_task4` 和必要的 `.gitignore` 更新。

### AI

开始做实构建和启动验证。先让编译器给出真实问题，再针对性修正 `swap fault -> evict -> swapin` 路径。

### AI

编译过了，但 QEMU 直接退出且没有串口输出，优先怀疑的是 `initialize_runtime()` 对几个 4 KiB/16 KiB 静态对象做整块赋值，会在栈上构造临时值，导致 16 KiB 内核栈被打爆。这和前一个 `task3` 遇到的症状一致，因此先对照 `task3` 改成原地清零。

### AI

行为已经正确，只是验收逻辑记成了“最后一次换出”而不是“第一次换出 page0”。随后收紧证据记录，让日志明确显示 `swap slot -> swap_pa`，并让 acceptance 对准真实的 `fault/swapin` 链路。

### AI

核心机制跑通后，AI 保存 artifacts、补写 README、检查工作树并提交。最终结论是：`lab4/kernel_task4` 中的 `main.rs`、`boot.S` 和 `README.md` 完成了 3 个工作集页配 2 个驻留帧的实验，统计稳定为 `page_faults=4 lazy_allocs=3 swap_outs=2 swap_ins=1 hits=2 clock_scans=4 second_chances=2`，三项 acceptance 全部 `PASS`。提交为 `e055c6e859602fcd806d0d46e9663a34da2d7955`。

### 用户

> `$os-lab-task-runner 完成LAB4 内核态task5：自映射机制`
>
> 目标：通过自映射让页表结构自身可被线性访问，便于调试与管理。
>
> 要求：
> 1. 实现页表自映射；
> 2. 提供示例：通过虚拟地址访问页表项并输出部分映射信息；
> 3. 说明安全边界：用户态不可利用自映射越权读写内核结构。
>
> 验收检查：
> 1. 顶层页表的某一特定表项指向顶层页表自身物理地址；
> 2. 内核可通过固定模式的虚拟地址高效遍历任意层级的 PTE。

### AI

按 `os-lab-task-runner` 继续做 `LAB4/kernel_task5`，先查看相邻内核任务与当前工作树，再把题目拆成 “acceptance -> evidence” 后直接落到新的 `kernel_task5` 目录。

### AI

骨架检查后，AI 明确决定复用 `kernel_task1` 的单地址空间版本：启动、trap 和用户态探测都沿用原结构，只替换成自映射相关逻辑。

### AI

开始落代码时，AI 新建了 `kernel_task5` 目录、复制 `.cargo/config.toml`、`linker.ld`、`trap.rs`、`console.rs`，随后编写新的 `Cargo.toml`、`boot.S` 与 `main.rs`，把递归地址公式、递归表项、用户态越权探测和验收日志一起做进去。

### AI

首次构建后，QEMU 运行已经输出完整日志：`root[511]` 回指 `root_pa`，`root_table_va=0xffffffffffffe000` 等固定地址可读出正确的 root / L1 / L0 PTE，`direct/selfmap equality` 五项全 `PASS`，用户态读取 `0xffffffffffffe000` 时触发 `scause=0xd`。随后 AI 去掉 warning、保存 artifacts、写 README 并提交。

### AI

最终结论是：`lab4/kernel_task5` 中的 `main.rs`、`boot.S` 和 `README.md` 实现了 `Sv39` 递归自映射与 alias 槽方案，`root[511]` 指向 `root_pa`，内核可通过固定模式虚拟地址遍历 root / L1 / L0 PTE，而用户态访问 selfmap 窗口因 `U=0` 被拦截。提交为 `e1ce370b206989ce26c09089a182d5da62c2709f`。

### 用户

> `$conversation-markdown-recorder`

### 用户

> 提供 `conversation-markdown-recorder` 技能说明，并要求按该技能把当前可见对话归档成 `.md` 文件。

### AI

表示将使用 `conversation-markdown-recorder`，把当前线程中的 LAB4 工作记录整理成一份中文 Markdown，保存在当前目录，避开隐藏提示词、私有推理和冗长原始命令输出。随后创建本文件。

## 后续动作

- 无。
