# LAB4 内核态 Task1 审计报告

## 1. Task scope and reviewed inputs

- Target task: `lab4/kernel_task1`（LAB4 内核态 task1）
- Review mode: examiner-style audit against stated requirements and acceptance checks
- Reviewed inputs:
  - `lab4/kernel_task1/README.md`
  - `lab4/kernel_task1/src/main.rs`
  - `lab4/kernel_task1/src/boot.S`
  - `lab4/kernel_task1/src/trap.rs`
  - `lab4/kernel_task1/src/console.rs`
  - `lab4/kernel_task1/linker.ld`
  - `lab4/kernel_task1/artifacts/build_output.txt`
  - `lab4/kernel_task1/artifacts/run_output.txt`
  - `lab4/kernel_task1/artifacts/run_output_repeat.txt`
  - `lab4/kernel_task1/artifacts/single_pagetable_objdump.txt`
  - `lab4/kernel_task1/artifacts/single_pagetable_nm.txt`
  - `lab4/kernel_task1/artifacts/tool_versions.txt`
  - repository `.gitignore`

## 2. Acceptance-to-evidence matrix

| Requirement / Acceptance | Expected evidence | Observed evidence | Result |
|---|---|---|---|
| 任务与内核共用同一张页表 | 同一 `satp(root)` 下同时可走到 kernel/user 映射 | `run_output*.txt` 显示同一 `satp(root)=0x8000000000080009`；`[pt] kernel_probe` 与 `[pt] user_*` 同时有效 | pass |
| 明确隔离策略且权限位正确 | 内核叶子无 `U`，用户叶子有 `U` | 日志中 `kernel_probe flags=VRWX--AD`，`user_text/data/stack` 分别为 `VR-XU-A-`/`VRW-U-AD` | pass |
| 越权访问应触发异常 | U 态访问内核 VA 触发受控 trap | `run_output*.txt` 中 `scause=0xd`、`stval=0x80000000`，并输出 acceptance PASS | pass |
| 验收1：同一多级页表中共存 kernel 与 user 映射 | 页表 walk 证据 + 同根页表 | `main.rs` 的 `build_single_page_table()` 在同一 root 下挂接 `kernel_l1` 与 `low_l1/user_l0`；日志 walk 与结论一致 | pass |
| 验收2：`U` 位配置正确 | PTE flags 与代码映射标志一致 | `main.rs` 里 kernel 映射未带 `PTE_U`，用户页显式带 `PTE_U`；运行日志 flags 对应正确 | pass |
| 验收3：用户越权访问内核地址必触发异常拦截 | 用户探测程序 + trap handler 判定 | `boot.S` 用户探测 `ld 0x80000000`；S 态 `handle_supervisor_trap` 按 `LOAD_PAGE_FAULT` 路径处理并验收 PASS | pass |
| runtime-sensitive 可复验 | repeat run 一致 | `run_output_repeat.txt` 与首轮关键字段一致 | pass |
| 低层控制流与符号证据 | objdump/nm 产物 | `single_pagetable_objdump.txt` 与 `single_pagetable_nm.txt` 已归档且 README 引用关键符号/指令 | pass |

## 3. Findings ordered by severity

### blocking

- none

### recommended

- none

### nice_to_have

- 可在 README 额外给出 `scause=0xd` 对应 RISC-V 异常编码（load page fault）的简短对照表，帮助非 RISC-V 背景评审快速核验。

## 4. Open questions or assumptions

- 假设课程验收允许“恒等映射内核窗口”作为“高半核（或恒等映射）”的等价实现路径。
- 假设评审环境与当前工具链一致支持 `qemu-system-riscv64` 与 `riscv64gc-unknown-none-elf` 目标。

## 5. Readiness verdict and residual risks

- Verdict: `ready`
- Residual risks:
  - 结果依赖当前最小教学内核模型与 QEMU `virt` 设备布局，迁移到不同地址布局需同步更新映射常量；
  - 在当前证据范围内，代码实现、运行日志、反汇编证据与 README 机制说明一致，未见影响通过的阻断项。
