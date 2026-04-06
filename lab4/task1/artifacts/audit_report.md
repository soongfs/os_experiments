# LAB4 用户态 Task1 审计报告

## 1. Task scope and reviewed inputs

- Target task: `lab4/task1`（LAB4 用户态 task1）
- Review mode: examiner-style audit against stated requirements and acceptance checks
- Reviewed inputs:
  - `lab4/task1/README.md`
  - `lab4/task1/memory_syscall_demo.c`
  - `lab4/task1/Makefile`
  - `lab4/task1/artifacts/build_output.txt`
  - `lab4/task1/artifacts/run_output.txt`
  - `lab4/task1/artifacts/run_output_repeat.txt`
  - `lab4/task1/artifacts/segfault_shell_output.txt`
  - `lab4/task1/artifacts/segfault_shell_output_repeat.txt`
  - `lab4/task1/artifacts/segfault_terminal_screenshot.svg`
  - `lab4/task1/artifacts/tool_versions.txt`
  - repository `.gitignore`

## 2. Acceptance-to-evidence matrix

| Requirement / Acceptance | Expected evidence | Observed evidence | Result |
|---|---|---|---|
| 综合使用 `sbrk`/`mmap`/`munmap`/`mprotect` | Source implementation + runtime logs | `memory_syscall_demo.c` 包含完整调用路径；`run_output*.txt` 含 `[sbrk]`、`[mmap-rw]`、`[mprotect-rx]`、`[mprotect-ro]`、`[munmap]` | pass |
| 至少两种不同权限映射效果（读/写/执行） | 日志出现不同权限映射与行为差异 | `run_output*.txt` 展示 `rw-p`、`r-xp`、`r--p`；`[mprotect-rx] executed stub result=42` 证明执行权限；`mmap-rw` 证明读写 | pass |
| 代码通过 `mmap` 分配并读写 | 可复验读写内容或校验 | `run_output*.txt` 中 payload 和固定 checksum `0xf91b8ebbc5221810` | pass |
| 写入 `PROT_READ` 区触发 Segfault 并截图验证 | 终端段错误日志 + 截图材料 | `run_output*.txt` 的子进程 `SIGSEGV`；`segfault_shell_output*.txt` 出现 `Segmentation fault` + `exit_status=139`；`segfault_terminal_screenshot.svg` 存在 | pass |
| 报告解释页对齐、权限与缺页异常关系 | README 机制解释段落 | `README.md` 第 7 节给出页对齐要求、权限位行为、`SIGSEGV` 因果链 | pass |
| 运行敏感项应有重复运行证据 | repeat 运行日志 | `run_output_repeat.txt` 与 `segfault_shell_output_repeat.txt` 均存在且关键观测一致 | pass |
| 任务目录结构与工件组织 | task-level README + artifacts + 无不当构建产物提交 | `lab4/task1/` 结构完整；二进制已在 `.gitignore` (`lab4/task1/memory_syscall_demo`) | pass |

## 3. Findings ordered by severity

### blocking

- none

### recommended

- none

### nice_to_have

- 可在后续补充一张“原始终端截图”（若具备 GUI 环境）与当前 SVG 转录截图并列，降低评审方对截图形式的歧义。

## 4. Open questions or assumptions

- 假设课程验收允许在无 GUI 环境下使用“终端 transcript 渲染图（SVG）”作为截图替代材料。
- 假设本任务允许在 WSL Debian 主机环境完成，不强制要求第二个独立 Linux 主机复验（README 已披露该限制）。

## 5. Readiness verdict and residual risks

- Verdict: `ready`
- Residual risks:
  - 若评审方对“截图”定义严格要求桌面截图而非终端转录图，可能要求补充证据形式；
  - 其余验收证据链（代码 -> 日志 -> 机制说明）完整，当前未见影响通过的阻断项。
