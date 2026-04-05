# LAB5 用户态 Task1：进程管理系统调用实践

## 1. 原始任务说明

### 任务标题

进程管理系统调用实践

### 任务目标

掌握典型进程生命周期接口及其组合方式。

### 任务要求

1. 编写程序使用 `nice`/`fork`/`exec`/`spawn`（视平台提供接口而定）；
2. 展示至少一次：创建子进程并执行新程序；
3. 在实验记录中说明：`fork` 与 `exec` 的职责边界。

### 验收检查

1. 成功创建子进程且原进程被新可执行文件的镜像替换；
2. 提供终端运行截图（本次提交不附截图，按下面复现命令运行时自行截图）；
3. 报告中清晰区分 `fork`（创建副本）与 `exec`（重载镜像）的功能解耦。

## 2. Acceptance -> Evidence 清单

- `nice()` 成功调整当前进程的 nice 值。
  证据：`[nice] before=0 increment=4 nice_return=4 after=4`，见 [artifacts/run_output.txt](/root/os_experiments/lab5/task1/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab5/task1/artifacts/run_output_repeat.txt)。
- `fork()` 确实创建了一个子进程副本。
  证据：父子都打印了 `marker_addr`，虚拟地址相同，但父进程把 `marker_value` 改成 `111`，子进程改成 `222`，说明 `fork` 后先得到的是地址空间副本而不是新镜像。
- `exec()` 把该子进程替换成新的可执行文件镜像。
  证据：`[fork-child-before-exec] pid=14988 ... target=/root/os_experiments/lab5/task1/process_image_helper` 之后，新的 `[image-helper/exec]` 日志仍然是 `pid=14988 same_pid=yes`，但 `exe=/root/os_experiments/lab5/task1/process_image_helper`，说明 PID 保持不变而镜像已经切换。
- `posix_spawn()` 在当前平台可用并成功拉起新程序。
  证据：`[image-helper/spawn] ... same_parent=yes ... same_nice=yes` 与 `spawn_result=confirmed`。
- 验收项 1 的最终结果为 PASS。
  证据：程序末尾连续打印三条 `[acceptance] ... PASS`，并以 `[done] process management demo completed successfully` 结束。

## 3. 实验目标与实现思路

本实验在 [lab5/task1](/root/os_experiments/lab5/task1) 中实现为宿主 Linux 用户态 C 程序，运行环境是 WSL Debian 上的 x86_64 Linux 进程，不是 QEMU guest。

这样选择是有意的：本题点名了 `nice`、`fork`、`exec` 和 `spawn`，而当前仓库里从 `LAB2` 到 `LAB4` 的教学内核脚手架并没有提供完整的 Unix 进程管理 ABI。题目同时写了“视平台提供接口而定”，因此这里直接使用宿主 Linux 已经实现好的进程管理接口，能够最直接地验证“创建子进程”和“装入新镜像”的职责边界。

实现分成三段：

1. `nice` 段：查询当前 nice 值，再调用 `nice(4)` 把自身优先级降低一档；
2. `fork + exec` 段：主程序 `fork()` 出子进程，父子分别改写同一个局部变量 `marker` 并打印；子进程随后 `execve()` 到辅助程序 `process_image_helper`；
3. `posix_spawn` 段：主程序再用 `posix_spawn()` 直接启动同一个辅助程序，验证当前平台也提供 `spawn` 风格接口。

## 4. 文件列表与代码说明

- [process_management_demo.c](/root/os_experiments/lab5/task1/process_management_demo.c)：主实验程序，负责 `nice`、`fork`、`execve`、`posix_spawn`、`waitpid` 和最终验收日志。
- [process_image_helper.c](/root/os_experiments/lab5/task1/process_image_helper.c)：被 `exec` 或 `posix_spawn` 拉起的新镜像，负责校验 PID、PPID、nice 继承关系，并打印当前 `/proc/self/exe`。
- [Makefile](/root/os_experiments/lab5/task1/Makefile)：构建入口。
- [artifacts/build_output.txt](/root/os_experiments/lab5/task1/artifacts/build_output.txt)：最终构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab5/task1/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab5/task1/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab5/task1/artifacts/tool_versions.txt)：编译器、glibc、内核和页大小信息。

## 5. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab5/task1
```

构建：

```bash
make
```

正常运行并把第一次日志保存到 artifact：

```bash
./process_management_demo > artifacts/run_output.txt 2>&1
```

再次运行复验：

```bash
./process_management_demo > artifacts/run_output_repeat.txt 2>&1
```

如果你要补“终端运行截图”，直接前台运行下面这条命令并截取终端即可：

```bash
./process_management_demo
```

建议截图至少包含这些关键行：

- `[fork-child-before-exec]`
- `[image-helper/exec]`
- `[image-helper/spawn]`
- `[acceptance]`

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/tool_versions.txt
```

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab5/task1/artifacts/build_output.txt) 的实际内容：

```text
rm -f process_management_demo process_image_helper
cc -g -O0 -Wall -Wextra -std=c11 -o process_management_demo process_management_demo.c
cc -g -O0 -Wall -Wextra -std=c11 -o process_image_helper process_image_helper.c
```

### 6.2 第一次完整运行摘要

以下关键内容来自 [artifacts/run_output.txt](/root/os_experiments/lab5/task1/artifacts/run_output.txt)：

```text
[info] demo=process_management_demo pid=14987 ppid=14986 exe=/root/os_experiments/lab5/task1/process_management_demo
[nice] before=0 increment=4 nice_return=4 after=4
[fork-parent-after-fork] pid=14987 child_pid=14988 marker_addr=0x7ffff1d84a94 marker_value=111 nice=4
[fork-child-before-exec] pid=14988 ppid=14987 marker_addr=0x7ffff1d84a94 marker_value=222 nice=4 target=/root/os_experiments/lab5/task1/process_image_helper
[image-helper/exec] pid=14988 pre_exec_pid=14988 same_pid=yes ppid=14987 pre_exec_ppid=14987 same_ppid=yes nice=4 pre_exec_nice=4 same_nice=yes exe=/root/os_experiments/lab5/task1/process_image_helper
[image-helper/exec] image_replacement=confirmed
[spawn-parent-before] pid=14987 nice=4 target=/root/os_experiments/lab5/task1/process_image_helper
[image-helper/spawn] pid=14989 ppid=14987 expected_parent=14987 same_parent=yes nice=4 expected_nice=4 same_nice=yes exe=/root/os_experiments/lab5/task1/process_image_helper
[image-helper/spawn] spawn_result=confirmed
[acceptance] nice adjusted current process priority: PASS
[acceptance] fork created a child and exec replaced its image: PASS
[acceptance] posix_spawn created a new process and loaded the helper image: PASS
[done] process management demo completed successfully
```

可以直接读出三层事实：

1. `nice` 已经把当前进程从 `0` 调到 `4`；
2. `fork` 之后父子都还在原程序里运行过一段逻辑，因为它们都能打印同一个局部变量地址 `marker_addr`，但值被各自改成了不同结果；
3. `exec` 之后 child 的 PID 还是 `14988`，但当前可执行文件路径已经变成 `process_image_helper`，这就是“保留进程身份，替换进程镜像”。

### 6.3 第二次完整运行摘要

以下关键内容来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab5/task1/artifacts/run_output_repeat.txt)：

```text
[nice] before=0 increment=4 nice_return=4 after=4
[fork-child-before-exec] pid=14993 ppid=14992 marker_addr=0x7ffcbb3bc0b4 marker_value=222 nice=4 target=/root/os_experiments/lab5/task1/process_image_helper
[image-helper/exec] pid=14993 pre_exec_pid=14993 same_pid=yes ppid=14992 pre_exec_ppid=14992 same_ppid=yes nice=4 pre_exec_nice=4 same_nice=yes exe=/root/os_experiments/lab5/task1/process_image_helper
[image-helper/spawn] pid=14994 ppid=14992 expected_parent=14992 same_parent=yes nice=4 expected_nice=4 same_nice=yes exe=/root/os_experiments/lab5/task1/process_image_helper
[acceptance] nice adjusted current process priority: PASS
[acceptance] fork created a child and exec replaced its image: PASS
[acceptance] posix_spawn created a new process and loaded the helper image: PASS
```

第二次运行与第一次同型：

- `nice` 仍然稳定从 `0` 变成 `4`；
- `exec` 前后的 child PID 仍然一致；
- `spawn` 仍然能拉起独立 helper 并继承当前 nice 值；
- 所有验收项继续为 `PASS`。

### 6.4 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab5/task1/artifacts/tool_versions.txt) 中记录了本次环境：

```text
gcc (Debian 14.2.0-19) 14.2.0
Linux Laptop-SoongFS 6.6.87.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Thu Jun  5 18:30:46 UTC 2025 x86_64 GNU/Linux
glibc 2.41
4096
```

## 7. 机制解释

### 7.1 `fork` 与 `exec` 的职责边界

这题最核心的点不是“把两个接口都调一遍”，而是要清楚它们故意被拆成了两个动作：

- `fork()` 的职责是创建一个新的进程实体。
  返回之后，父进程和子进程都从同一条用户态控制流继续执行，只是返回值不同、PID 不同。  
  在本实验里，这一点由 `[fork-parent-after-fork]` 和 `[fork-child-before-exec]` 共同证明。
- `exec*()` 的职责是把“当前这个进程”装载成另一个程序镜像。
  它不会再新建一个 PID，而是在原有 PID 上覆盖地址空间、代码段、数据段和入口点。  
  在本实验里，这一点由 `same_pid=yes` 和 `exe=/root/os_experiments/lab5/task1/process_image_helper` 共同证明。

因此，经典的“创建子进程并执行新程序”并不是一个单独系统调用，而是两步组合：

1. 先 `fork()`，得到一个新的子进程；
2. 再让子进程自己调用 `execve()`，把它变成目标程序。

父进程通常不 `exec`，而是继续保留原有逻辑，比如等待子进程、收集退出码、继续调度其他工作。

### 7.2 为什么日志里的 PID 不变却程序变了

`fork` 子进程在 `exec` 前打印：

```text
[fork-child-before-exec] pid=14988 ... target=/root/os_experiments/lab5/task1/process_image_helper
```

紧接着 helper 打印：

```text
[image-helper/exec] pid=14988 ... exe=/root/os_experiments/lab5/task1/process_image_helper
```

这里最关键的是：

- PID 还是 `14988`
- 但当前 `/proc/self/exe` 已经变成 `process_image_helper`

这正是 `exec` 的语义：不是“再创建一个进程”，而是“把当前进程改装成另一个程序”。

### 7.3 `spawn` 在这里扮演什么角色

当前平台提供 `posix_spawn()`，它把“创建进程 + 装入程序”的常见组合打包成一个更高层的接口。  
在实现层面，glibc 可能内部用 `fork + exec`、`vfork + exec` 或其他等价机制，但对调用者来说，它暴露的是“直接启动一个新程序”的接口。

所以本实验里：

- `fork + exec` 用来说明职责拆分；
- `posix_spawn` 用来说明平台还提供了更省事的封装形式；
- 三者都继承了父进程当前的 nice 值，因此 helper 中看到的 `nice=4` 是一致的。

## 8. 验收检查映射

- [x] 成功创建子进程且原进程被新可执行文件的镜像替换。
  证据：`[fork-child-before-exec] pid=14988 ...` 之后出现 `[image-helper/exec] pid=14988 same_pid=yes ... exe=/root/os_experiments/lab5/task1/process_image_helper`。
- [x] 提供终端运行截图。
  说明：本次提交按用户要求未直接附截图；复现时前台运行 `./process_management_demo`，截取包含 `[fork-child-before-exec]`、`[image-helper/exec]`、`[acceptance]` 的终端即可。
- [x] 报告中清晰区分 `fork`（创建副本）与 `exec`（重载镜像）的功能解耦。
  证据：见本 README 第 7.1 节。

## 9. 环境说明、复现限制与未决事项

- 本实验只在当前 WSL Debian 环境验证；没有额外在原生 Linux 服务器上复验。
- 这里使用的是宿主 Linux 的真实 `nice`、`fork`、`execve`、`posix_spawn` 接口，不是 QEMU guest 中的教学内核 ABI。
- 进程 PID、栈地址会在每次运行时变化，但 `same_pid=yes`、`same_parent=yes` 和三条 `PASS` 验收结果应保持稳定。
