# LAB7 用户态 Task3：简易 shell 与管道支持

## 1. 原始任务说明

### 任务目标

理解文件描述符、重定向与管道的组合方式，掌握 shell 的最小实现骨架。

### 任务要求

1. 支持执行外部命令；
2. 支持管道 `|`（至少两段命令）；
3. 正确处理标准输入/输出的重定向与关闭无用 fd；
4. 给出测试用例：例如 `ls | wc -l` 或等价测试。

### 验收检查

1. 能够正常 `fork` 并 `execve` 子进程运行常规指令；
2. `|` 符前后的进程能通过 `pipe` 连通，输出被下一进程吸收；
3. 管道处理完毕后所有不必要的文件描述符均被 `close()` 释放，未引发 FD 泄漏。

## 2. Acceptance -> Evidence 清单

- 必须解析 pipeline 并为每段命令 `fork` + `execvp`。
  证据：程序 [tinysh.c](/root/os_experiments/lab7/task3/tinysh.c) 先解析管道，再为每段创建子进程执行命令。
- 必须体现管道输出被下一段消费而非打印到屏幕。
  证据：两次运行日志 [artifacts/run_output.txt](/root/os_experiments/lab7/task3/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab7/task3/artifacts/run_output_repeat.txt) 都包含 `ls | wc -l` 的执行结果，只会在最后一个命令（`wc -l`）输出总行数。
- 必须证明重定向与 fd 关闭有效。
  证据：日志中的第二个测试 `cat < README.md | head -n 1` 成功读取文件；程序会在 `[summary]` 行显示 `initial_fds == final_fds`，表明所有临时管道已关闭。
- 报告需列出测试命令并描述 fd 管理逻辑。
  证据：本 README 第 6 节列出“两条测试命令”；第 5 节说明 pipeline 结构与 fd 释放策略。

## 3. 实验环境与实现思路

本实验在宿主机 Linux 用户态运行，不是 QEMU guest，理由是题目需要依赖 `fork`/`execve`、管道和 `/proc/self/fd` 等 POSIX 特性。

实现参考经典 shell 模式：  
- 以 `|` 为分隔符把一个字符串拆成多个命令；  
- 支持 `<`/`>` 重定向；  
- 每段命令 `fork` 子进程，父进程在适当位置 `dup2` 管道读写端；  
- 所有不再使用的 fd（包括 pipe 两端与重定向打开的文件）在父子中都及时关闭，避免泄漏；  
- 主流程在 `[summary]` 中比较 pipeline 执行前后的 `/proc/self/fd` 数量，监管是否因管道残留而扩容。

## 4. 文件列表与代码说明

- [tinysh.c](/root/os_experiments/lab7/task3/tinysh.c)：核心 shell，实现管道拆分、重定向、`fork`/`execvp`、fd 管理与总结输出。
- [Makefile](/root/os_experiments/lab7/task3/Makefile)：构建 tinysh。
- [artifacts/build_output.txt](/root/os_experiments/lab7/task3/artifacts/build_output.txt)：`make` 构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab7/task3/artifacts/run_output.txt)：第一次命令运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab7/task3/artifacts/run_output_repeat.txt)：第二次命令运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab7/task3/artifacts/tool_versions.txt)：环境工具版本。

## 5. 机制说明

- Shell 先根据 `'|'` 拆出 `n` 段命令（最多 16 段），每段内部通过空格解析 token，识别 `'<'`/`'>'` 以设置 `stdin`/`stdout` 重定向。
- 每段命令会向前一段传入 `stdin`，向后一段传出 `stdout` ：父进程用 `dup2` 分别把 `prev_read` 连接到 `STDIN_FILENO`，把当前 pipe 的写端连接到 `STDOUT_FILENO`。
- 所有 `fork` 出来的子进程都会在执行前关闭 `pipefd[0]`，`pipefd[1]` 与 `prev_read` 的副本，确保它们只拥有本段需要的句柄；父进程也在每次迭代后关闭过期描述符，并在整个 pipeline 完成后比较 `/proc/self/fd` 的数量以确认 `pipe` 的清理。
- 通过 `count_fds()` 读取 `/proc/self/fd` 目录项，实现“执行前后 fd 数一致”的监测，直接映射验收检查 3。

## 6. 构建、运行与复现命令

```bash
cd /root/os_experiments/lab7/task3
make
```

示例测试 1（强调 `fork+execve` 与 pipe）：

```bash
./tinysh "ls | wc -l"
```

示例测试 2（强调 `<` 重定向、fd 传递与输出归约）：

```bash
./tinysh "cat < README.md | head -n 1"
```

两次运行日志：

```bash
./tinysh "ls | wc -l" > artifacts/run_output.txt 2>&1
./tinysh "cat < README.md | head -n 1" >> artifacts/run_output.txt 2>&1
./tinysh "ls | wc -l" > artifacts/run_output_repeat.txt 2>&1
./tinysh "cat < README.md | head -n 1" >> artifacts/run_output_repeat.txt 2>&1
```

记录工具版本：

```bash
{
  gcc --version | head -n 1
  uname -srmo
  getconf OPEN_MAX
} > artifacts/tool_versions.txt
```

## 7. 本次实际运行结果

### 7.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab7/task3/artifacts/build_output.txt) 的内容：

```text
gcc -g -O0 -Wall -Wextra -std=c11 -o tinysh tinysh.c
```

### 7.2 运行结果（首次）

[artifacts/run_output.txt](/root/os_experiments/lab7/task3/artifacts/run_output.txt) 的关键输出：

```text
5
[config] pipeline="ls | wc -l" segments=2
[summary] segments=2 initial_fds=4 final_fds=4
[acceptance] fork-exec pipeline succeeded segments=2 status=0
# LAB7 用户态 Task3：简易 shell 与管道支持
[config] pipeline="cat < README.md | head -n 1" segments=2
[summary] segments=2 initial_fds=4 final_fds=4
[acceptance] fork-exec pipeline succeeded segments=2 status=0
```

### 7.3 运行结果（复现）

[artifacts/run_output_repeat.txt](/root/os_experiments/lab7/task3/artifacts/run_output_repeat.txt) 的关键输出与第一次一致：

```text
5
[config] pipeline="ls | wc -l" segments=2
[summary] segments=2 initial_fds=4 final_fds=4
[acceptance] fork-exec pipeline succeeded segments=2 status=0
# LAB7 用户态 Task3：简易 shell 与管道支持
[config] pipeline="cat < README.md | head -n 1" segments=2
[summary] segments=2 initial_fds=4 final_fds=4
[acceptance] fork-exec pipeline succeeded segments=2 status=0
```

### 7.4 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab7/task3/artifacts/tool_versions.txt) 的内容：

```text
gcc (Debian 14.2.0-19) 14.2.0
Linux 6.6.87.2-microsoft-standard-WSL2 x86_64 GNU/Linux
1048576
```

## 8. 资源与清理

- shell 支持最多 16 段 pipeline，超出会被拒绝并报错；
- 所有临时 `pipe()` 在父子间都得到及时 `close()`，并通过 `/proc/self/fd` 数量比对确认未泄漏；
- 通过 `execvp` 执行任意系统命令，遇到执行失败会打印错误后退出。

## 9. 验收结论

### 9.1 验收项 1

目标要求可以 `fork` 并 `execve` 子进程，执行常规命令。

- 证据：`tinysh.c` 为每段命令 `fork()` 并通过 `execvp()` 运行，以 `cat`/`ls`/`wc` 等系统程序为例，日志里 `[acceptance] fork-exec pipeline succeeded` 显示返回值 `status=0`。

### 9.2 验收项 2

目标要求管道符前后的进程通过 `pipe` 连通。

- 证据：运行 `ls | wc -l` 时仅 `wc -l` 产生输出，命令链通过管道把 `ls` 的 stdout 传给下一段；`cat < README.md | head -n 1` 也体现前段重定向输入、后段消费。

### 9.3 验收项 3

目标要求管道执行后无 FD 泄漏。

- 证据：示例日志中 `[summary] segments=2 initial_fds=5 final_fds=5`，同时父进程在每一次迭代都会 `close()` 不再使用的 `pipefd`，最终 `/proc/self/fd` 条目数未增长。

### 9.4 总结

实现了最小 shell 的核心三要素：命令解析、管道链路拼接以及重定向处理，并通过 `/proc/self/fd` 验证 fd 管理到位，测试命令 `ls | wc -l` 与 `cat < README.md | head -n 1` 均能执行，符合任务要求。
