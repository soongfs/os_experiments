# LAB4 用户态 Task1：Linux 内存系统调用实践

## 1. 原始任务说明

### 任务标题

Linux 内存系统调用实践

### 任务目标

理解虚拟内存接口在用户态的典型使用方式，为后续内核实现建立直观对照。

### 任务要求

1. 编写 Linux 程序，综合使用 `sbrk`/`mmap`/`munmap`/`mprotect`；
2. 展示至少两种不同权限（读/写/执行）映射的效果；
3. 在实验记录中解释：页对齐、权限与缺页异常的关系（定性说明即可）。

### 验收检查

1. 代码成功通过 `mmap` 分配内存并读写；
2. 尝试写入 `PROT_READ` 映射区触发 Segfault 并截图验证；
3. 报告解释内存保护机制。

## 2. Acceptance -> Evidence 清单

- `sbrk` 被调用并成功扩展/回收堆空间。
  证据：`[sbrk]` 日志行出现在 [artifacts/run_output.txt](/root/os_experiments/lab4/task1/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab4/task1/artifacts/run_output_repeat.txt)。
- `mmap(PROT_READ|PROT_WRITE)` 成功分配匿名页并完成读写。
  证据：`[mmap-rw] payload="Linux mmap read/write demo"` 和固定校验和 `0xf91b8ebbc5221810`。
- `mprotect` 成功把页权限切成 `PROT_READ|PROT_EXEC` 与 `PROT_READ`。
  证据：`/proc/self/maps` 中分别出现 `r-xp` 与 `r--p`。
- `munmap` 成功回收映射。
  证据：日志中的三个 `[munmap]` 成功提示。
- 向 `PROT_READ` 页写入时触发 Segfault。
  证据：`[probe] child write-to-PROT_READ terminated by signal=11 (SIGSEGV)`、[artifacts/segfault_shell_output.txt](/root/os_experiments/lab4/task1/artifacts/segfault_shell_output.txt) 中的 shell 报错，以及 [artifacts/segfault_terminal_screenshot.svg](/root/os_experiments/lab4/task1/artifacts/segfault_terminal_screenshot.svg)。
- 页对齐要求得到直接观测。
  证据：`mprotect(addr+1, ...)` 在日志中以 `EINVAL` 失败，说明页保护修改必须从页边界开始。

## 3. 实验目标与实现思路

本实验在 [lab4/task1](/root/os_experiments/lab4/task1) 中实现了一个宿主 Linux 用户态 C 程序，运行环境是 WSL Debian 上的 x86_64 Linux 进程，不是 QEMU guest。

实现分成四个可观察阶段：

1. `sbrk` 阶段：打印程序 break，向上扩展一页，写入并回收；
2. `mmap` 读写阶段：映射两页 `PROT_READ|PROT_WRITE` 匿名页，写入字符串与标记字节，计算校验和后 `munmap`；
3. `mprotect` 权限切换阶段：
   - 把一页从 `RW` 改成 `RX`，执行一段放进映射页里的极小函数，验证执行权限效果；
   - 把另一页从 `RW` 改成 `R`，验证读取仍可用，并额外演示未对齐 `mprotect` 被 `EINVAL` 拒绝；
4. Segfault 阶段：
   - 默认模式里通过 `fork()` 在子进程中故意写只读页，让父进程记录 `SIGSEGV`；
   - 单独的 `segfault` 子命令直接崩溃，专门用于采集 shell 日志和截图证据。

这样既能保留“完整 demo 成功结束”的主日志，也能单独留下“真实只读写入导致段错误”的验收材料。

## 4. 文件列表与代码说明

- [memory_syscall_demo.c](/root/os_experiments/lab4/task1/memory_syscall_demo.c)：主实验程序，包含 `sbrk`、`mmap`、`mprotect`、`munmap` 和故意触发 `SIGSEGV` 的子命令。
- [Makefile](/root/os_experiments/lab4/task1/Makefile)：构建入口。
- [render_terminal_svg.py](/root/os_experiments/lab4/task1/render_terminal_svg.py)：把真实终端文本日志渲染成 SVG 截图，适配当前无 GUI 的 headless 取证环境。
- [artifacts/build_output.txt](/root/os_experiments/lab4/task1/artifacts/build_output.txt)：最终构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab4/task1/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab4/task1/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/segfault_shell_output.txt](/root/os_experiments/lab4/task1/artifacts/segfault_shell_output.txt)：第一次直接触发段错误的 shell 日志。
- [artifacts/segfault_shell_output_repeat.txt](/root/os_experiments/lab4/task1/artifacts/segfault_shell_output_repeat.txt)：第二次段错误复验日志。
- [artifacts/segfault_terminal_screenshot.svg](/root/os_experiments/lab4/task1/artifacts/segfault_terminal_screenshot.svg)：由真实 shell 输出渲染得到的终端截图。
- [artifacts/tool_versions.txt](/root/os_experiments/lab4/task1/artifacts/tool_versions.txt)：编译器、glibc、内核和页大小信息。

## 5. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab4/task1
```

构建：

```bash
make
```

运行完整 demo 并保存日志：

```bash
./memory_syscall_demo > artifacts/run_output.txt 2>&1
./memory_syscall_demo > artifacts/run_output_repeat.txt 2>&1
```

单独触发只读页写入段错误并保存 shell 日志：

```bash
{ set +e; ./memory_syscall_demo segfault; status=$?; printf '[shell] exit_status=%d\n' "$status"; } > artifacts/segfault_shell_output.txt 2>&1
{ set +e; ./memory_syscall_demo segfault; status=$?; printf '[shell] exit_status=%d\n' "$status"; } > artifacts/segfault_shell_output_repeat.txt 2>&1
```

把 shell 日志渲染成截图：

```bash
python3 render_terminal_svg.py artifacts/segfault_shell_output.txt artifacts/segfault_terminal_screenshot.svg
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/segfault_shell_output.txt
cat artifacts/segfault_shell_output_repeat.txt
cat artifacts/tool_versions.txt
```

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab4/task1/artifacts/build_output.txt) 的实际内容：

```text
rm -f memory_syscall_demo
cc -g -O0 -Wall -Wextra -std=c11 -o memory_syscall_demo memory_syscall_demo.c
```

### 6.2 第一次完整运行摘要

以下关键内容来自 [artifacts/run_output.txt](/root/os_experiments/lab4/task1/artifacts/run_output.txt)：

```text
[info] host_page_size=4096 bytes
[sbrk] initial_break=0x5b2983739000 offset_in_page=0
[sbrk] grew_by=4096 bytes old_break=0x5b2983739000 new_break=0x5b298373a000 sample=LAB
[sbrk] restored_break=0x5b2983739000
[mmap-rw] addr=0x77e0eeb37000 length=8192 aligned=yes
    /proc/self/maps: 77e0eeb37000-77e0eeb3b000 rw-p 00000000 00:00 0
[mmap-rw] payload="Linux mmap read/write demo" checksum=0xf91b8ebbc5221810 marker_bytes=[0xa5,0x3c]
[mprotect-rx] changed page to PROT_READ|PROT_EXEC
    /proc/self/maps: 77e0eeb38000-77e0eeb39000 r-xp 00000000 00:00 0
[mprotect-rx] executed stub result=42
[mprotect-ro] unaligned mprotect(addr+1, ...) rejected with EINVAL as expected
[mprotect-ro] addr=0x77e0eeb38000 length=4096 aligned=yes read_back="read-only page after mprotect"
    /proc/self/maps: 77e0eeb38000-77e0eeb39000 r--p 00000000 00:00 0
[probe] child write-to-PROT_READ terminated by signal=11 (SIGSEGV)
[done] memory syscall demo completed successfully
```

可以直接看到：

1. `mmap` 返回了页对齐地址；
2. 读写映射中的字符串和校验和都成功读回；
3. `RW -> RX` 权限切换后，映射页中的机器码成功执行并返回 `42`；
4. `RW -> R` 权限切换后，读取成功，而子进程写入只读页时收到 `SIGSEGV`。

### 6.3 第二次完整运行摘要

以下关键内容来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab4/task1/artifacts/run_output_repeat.txt)：

```text
[mmap-rw] payload="Linux mmap read/write demo" checksum=0xf91b8ebbc5221810 marker_bytes=[0xa5,0x3c]
[mprotect-rx] executed stub result=42
[mprotect-ro] unaligned mprotect(addr+1, ...) rejected with EINVAL as expected
[probe] child write-to-PROT_READ terminated by signal=11 (SIGSEGV)
[done] memory syscall demo completed successfully
```

虽然地址因 ASLR 改变，但关键观测保持一致：

- 读写映射校验和不变；
- 执行页仍然返回 `42`；
- 只读页写入仍然触发 `SIGSEGV`。

### 6.4 段错误日志与截图证据

[artifacts/segfault_shell_output.txt](/root/os_experiments/lab4/task1/artifacts/segfault_shell_output.txt) 的实际关键输出：

```text
[segfault] read-only mapping prepared at 0x73d86ed6c000 length=4096
    /proc/self/maps: 73d86ed6c000-73d86ed6d000 r--p 00000000 00:00 0
[segfault] about to write into a PROT_READ page; Linux should raise SIGSEGV
/bin/bash: line 1:  9839 Segmentation fault      ./memory_syscall_demo segfault
[shell] exit_status=139
```

[artifacts/segfault_shell_output_repeat.txt](/root/os_experiments/lab4/task1/artifacts/segfault_shell_output_repeat.txt) 再次得到同样的 `Segmentation fault` 和 `exit_status=139`。

截图文件见 [artifacts/segfault_terminal_screenshot.svg](/root/os_experiments/lab4/task1/artifacts/segfault_terminal_screenshot.svg)。  
由于当前取证环境是无 GUI 的 headless shell，这张“截图”不是桌面截图，而是把真实终端 transcript 原样渲染成 SVG 图像；图中保留了 shell 的 `Segmentation fault` 报错和退出码，便于审查。

### 6.5 工具与环境版本

[artifacts/tool_versions.txt](/root/os_experiments/lab4/task1/artifacts/tool_versions.txt) 中记录了本次环境：

```text
gcc (Debian 14.2.0-19) 14.2.0
rustc 1.94.1 (e408947bf 2026-03-25)
cargo 1.94.1 (29ea6fb6a 2026-03-24)
Linux 6.6.87.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Thu Jun  5 18:30:46 UTC 2025 x86_64 GNU/Linux
4096
ldd (Debian GLIBC 2.41-12+deb13u2) 2.41
```

## 7. 机制解释

### 7.1 页对齐为什么重要

`mmap`/`mprotect`/`munmap` 的管理单位都是页，而不是任意字节区间。  
因此：

- `mmap()` 返回的起始地址天然按页对齐；
- `mprotect()` 修改的是整页权限，传入地址必须从页边界开始；
- 本实验里 `mprotect(mapping + 1, ...)` 得到 `EINVAL`，就是因为起始地址故意错开了 1 字节。

`sbrk()` 暴露给用户的是“程序 break”这一段连续堆区的边界，看起来像字节级增长；但底层真正建立或回收页表映射时，内核仍按页粒度处理。

### 7.2 权限位如何控制读、写、执行

每个虚拟页在页表项里都带有权限位。用户态访问该页时，CPU 会先查页表，再检查这次访问是否合法：

- `PROT_READ | PROT_WRITE`：允许读写；
- `PROT_READ | PROT_EXEC`：允许取指执行，但不允许写；
- `PROT_READ`：允许读，不允许写，也通常不允许执行。

本实验的三个直接观测分别对应：

1. `RW` 页能写入字符串并读回固定校验和；
2. `RX` 页在 `mprotect()` 后可以执行一段最小机器码并返回 `42`；
3. `R` 页可以正常读取字符串，但写入时会被内核拦住。

### 7.3 写只读页为什么会变成 Segfault

当用户态代码对只读页执行写操作时，CPU 发现当前页表项没有写权限，就会触发页异常（page fault / protection fault）。  
Linux 内核检查到这是一个用户态非法访问后，不会帮程序“自动修复”，而是向当前进程发送 `SIGSEGV`。shell 再把这个结果显示成：

```text
Segmentation fault
```

所以这里的 `Segfault` 本质上是：

1. 用户态访问某虚拟地址；
2. 硬件查页表发现权限不满足；
3. 处理器陷入内核；
4. Linux 判断为非法内存访问并投递 `SIGSEGV`。

### 7.4 `munmap` 做了什么

`munmap()` 会撤销对应虚拟地址区间的映射关系，把相关 VMA/PTE 从进程地址空间里移除。  
本实验里没有在 `munmap` 后再次访问这些地址，因为那会引入另一次非法访问；但从机制上说，后续再访问已解除映射的地址，同样会触发页异常，只是原因从“权限不足”变成了“页不存在/未映射”。

## 8. 验收检查映射

- 验收 1：代码成功通过 `mmap` 分配内存并读写。
  证据：`[mmap-rw] payload="Linux mmap read/write demo" checksum=0xf91b8ebbc5221810`，见 [artifacts/run_output.txt](/root/os_experiments/lab4/task1/artifacts/run_output.txt)。
- 验收 2：尝试写入 `PROT_READ` 映射区触发 Segfault 并截图验证。
  证据：`[probe] child write-to-PROT_READ terminated by signal=11 (SIGSEGV)`、[artifacts/segfault_shell_output.txt](/root/os_experiments/lab4/task1/artifacts/segfault_shell_output.txt)、[artifacts/segfault_terminal_screenshot.svg](/root/os_experiments/lab4/task1/artifacts/segfault_terminal_screenshot.svg)。
- 验收 3：报告解释内存保护机制。
  证据：本 README 第 7 节已解释页对齐、权限位和缺页异常/保护异常到 `SIGSEGV` 的因果关系。

## 9. 环境说明、复现实限与未解决问题

- 本实验证据来自当前 WSL Debian x86_64 宿主 Linux 环境，不是 QEMU guest。
- 当前会话无法访问额外的原生 Linux 服务器，因此尚未在第二宿主环境做复验。
- `RX` 演示中的最小可执行代码字节序列针对当前 x86_64 环境编写；如果将任务迁移到其他 CPU 架构，需要替换对应架构的机器码，或者改用该架构的汇编/代码生成方式。
