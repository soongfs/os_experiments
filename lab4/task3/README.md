# LAB4 用户态 Task3：COW（写时复制）验证程序

## 1. 原始任务说明

### 任务标题

COW（写时复制）验证程序

### 任务目标

验证 `fork` 后父子进程共享只读页，写入时触发缺页并复制的正确语义。

### 任务要求

1. 父子进程共享同一段内存，分别写入并验证互不影响；
2. 输出验证结果（例如不同进程读取到的值不同）；
3. 若可用，输出内核页分配/复制计数器证明“写入才复制”。

### 验收检查

1. `fork` 调用之后，父子进程读取变量的值初始相同；
2. 父或子任一方修改变量后，另一方读取不受影响（隔离成功）；
3. 日志证明首次写入时引发了特定的 Page Fault。

## 2. Acceptance -> Evidence 清单

- `fork` 后父子进程初始值相同，且确实映射到同一个物理页。
  证据：`[post-fork/page0]`、`[post-fork/page1]` 中父子 PFN 相同且 `kpagecount=2`。
- 子进程先写第 0 页，父进程视角不受影响。
  证据：`[child-write/page0/parent-view]` 仍然是 `seed_page0`，而 `[child-write/page0/child-view]` 已变成 `child_page0`。
- 父进程后写第 1 页，子进程视角不受影响。
  证据：`[parent-write/page1/parent-view]` 已变成 `parent_page1`，而 `[parent-write/page1/child-view]` 仍然是 `seed_page1`。
- “写入才复制”有内核级证据。
  证据：`pre-fork` 时 `kpagecount=1`，`post-fork` 变成 `2`；首次写入后写入方 PFN 与另一方 PFN 分裂，且两边 `kpagecount` 都回到 `1`。
- 首次写入时触发了特定 page fault。
  证据：子进程第一次写页 0 时 `child_minflt_delta=2`、`child_majflt_delta=0`；父进程第一次写页 1 时 `parent_minflt_delta=1`、`parent_majflt_delta=0`。结合 PFN 分裂可知这是用户态写保护触发的 COW minor fault，而不是磁盘 I/O 型 major fault。

## 3. 实验目标与实现思路

本实验在 [lab4/task3](/root/os_experiments/lab4/task3) 中实现，运行环境是宿主 Linux 用户态，不是 QEMU guest。

实现的关键不是只比较“值有没有变化”，而是同时保留三层证据：

1. 用户态可见值：
   - `fork` 后初始读取相同；
   - 写后另一方读到的值不变。
2. 页框级共享/分裂证据：
   - 用 `/proc/<pid>/pagemap` 读取父进程和子进程同一虚拟地址对应的 PFN；
   - 用 `/proc/kpagecount` 读取该 PFN 的内核引用计数。
3. 缺页计数证据：
   - 用 `getrusage(RUSAGE_SELF)` 读取 `ru_minflt` / `ru_majflt`；
   - 只围绕“首次写本页”做前后快照。

为了让父子双方都发生一次“对仍处于共享状态页的首次写入”，程序没有只用一页，而是用了同一映射段中的两页：

- 第 0 页由子进程先写；
- 第 1 页由父进程后写。

这样两次写入都发生在“页仍由父子共享”的时刻，因此两次都能看到真正的 COW 分裂，而不是后一方只是在独占页上解除写保护。

## 4. 文件列表与代码说明

- [cow_fork_demo.c](/root/os_experiments/lab4/task3/cow_fork_demo.c)：主实验程序。建立两页匿名私有映射，`fork` 后用 pipe 同步父子进程，采集值、PFN、`kpagecount` 和 `minflt/majflt`。
- [Makefile](/root/os_experiments/lab4/task3/Makefile)：构建入口。
- [artifacts/build_output.txt](/root/os_experiments/lab4/task3/artifacts/build_output.txt)：最终构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab4/task3/artifacts/run_output.txt)：第一次完整运行日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab4/task3/artifacts/run_output_repeat.txt)：第二次完整运行日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab4/task3/artifacts/tool_versions.txt)：运行身份、工具链、页大小，以及 `/proc/vmstat` / `pagemap` / `kpagecount` 可用性信息。

## 5. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab4/task3
```

构建：

```bash
make
```

运行一次并保存日志：

```bash
./cow_fork_demo > artifacts/run_output.txt 2>&1
```

再次运行做复验：

```bash
./cow_fork_demo > artifacts/run_output_repeat.txt 2>&1
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/tool_versions.txt
```

说明：

- 本实验读取 `/proc/<pid>/pagemap` 和 `/proc/kpagecount`，当前环境以 `root` 运行；
- 若在普通用户环境没有足够权限，值隔离和 `minflt` 证据仍可做，但 PFN / `kpagecount` 这一层内核证据可能无法读取。

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab4/task3/artifacts/build_output.txt) 的实际内容：

```text
rm -f cow_fork_demo
cc -g -O0 -Wall -Wextra -std=c11 -o cow_fork_demo cow_fork_demo.c
```

### 6.2 接口可用性与环境摘要

[artifacts/tool_versions.txt](/root/os_experiments/lab4/task3/artifacts/tool_versions.txt) 显示：

```text
uid=0(root) gid=0(root) groups=0(root)
gcc (Debian 14.2.0-19) 14.2.0
Linux 6.6.87.2-microsoft-standard-WSL2 ...
4096
--- /proc/vmstat cow probe ---
cow_ksm 0
--- pagemap interfaces ---
/proc/kpagecount
/proc/kpageflags
/proc/self/clear_refs
/proc/self/pagemap
/proc/self/smaps
```

这里可以看出：

1. 当前进程具备读取 `pagemap`/`kpagecount` 的权限；
2. 该内核没有直接暴露通用匿名 COW 次数计数器，例如 `cow_anon` / `cow_fault` 之类的键；
3. 因此本实验把 `/proc/<pid>/pagemap` 的 PFN 和 `/proc/kpagecount` 的引用计数作为“写入才复制”的内核级证据。

### 6.3 第一次完整运行结果

以下关键内容来自 [artifacts/run_output.txt](/root/os_experiments/lab4/task3/artifacts/run_output.txt)：

```text
[pre-fork/page0] value=0x1111111111111111 owner=seed_page0 self_pfn=0x11f5c0 ... self_kpagecount=1
[pre-fork/page1] value=0x2222222222222222 owner=seed_page1 self_pfn=0x10ea8b ... self_kpagecount=1
[post-fork/page0] value=0x1111111111111111 owner=seed_page0 self_pfn=0x11f5c0 peer_pfn=0x11f5c0 self_kpagecount=2 peer_kpagecount=2
[post-fork/page1] value=0x2222222222222222 owner=seed_page1 self_pfn=0x10ea8b peer_pfn=0x10ea8b self_kpagecount=2 peer_kpagecount=2
[child-write/page0] child_minflt_delta=2 child_majflt_delta=0
[child-write/page0/parent-view] value=0x1111111111111111 owner=seed_page0 self_pfn=0x11f5c0 peer_pfn=0x1c9855 self_kpagecount=1 peer_kpagecount=1
[child-write/page0/child-view] value=0xc0ffee0000000001 owner=child_page0 child_pfn=0x1c9855 child_kpagecount=1
[child-write/page1/shared-still] value=0x2222222222222222 owner=seed_page1 self_pfn=0x10ea8b peer_pfn=0x10ea8b self_kpagecount=2 peer_kpagecount=2
[parent-write/page1] parent_minflt_delta=1 parent_majflt_delta=0
[parent-write/page1/parent-view] value=0xa11ce00000000002 owner=parent_page1 self_pfn=0x126271 peer_pfn=0x10ea8b self_kpagecount=1 peer_kpagecount=1
[parent-write/page1/child-view] value=0x2222222222222222 owner=seed_page1 child_pfn=0x10ea8b child_kpagecount=1
[final] parent_page0=0x1111111111111111 (seed_page0) parent_page1=0xa11ce00000000002 (parent_page1)
[final] child_page0=0xc0ffee0000000001 (child_page0) child_page1=0x2222222222222222 (seed_page1)
[acceptance] fork initial values same: PASS
[acceptance] child write isolates page0: PASS
[acceptance] parent write isolates page1: PASS
[acceptance] child first write triggered COW minor fault: PASS
[acceptance] parent first write triggered COW minor fault: PASS
```

从这份日志可以直接读出完整的 COW 链路：

1. `fork` 前每页 `kpagecount=1`，说明各自只有一个映射；
2. `fork` 后父子 PFN 相同，且 `kpagecount=2`，说明同一物理页被两边共享；
3. 子进程第一次写第 0 页后：
   - `child_minflt_delta=2`、`child_majflt_delta=0`；
   - 子进程页 0 PFN 从 `0x11f5c0` 变成 `0x1c9855`；
   - 父进程页 0 仍保留原值和原 PFN；
4. 父进程第一次写第 1 页后：
   - `parent_minflt_delta=1`、`parent_majflt_delta=0`；
   - 父进程页 1 PFN 从 `0x10ea8b` 变成 `0x126271`；
   - 子进程页 1 仍保留原值和原 PFN。

也就是说，这里不仅验证了“值隔离”，还验证了“写发生时页框才分裂”。

### 6.4 第二次完整运行结果

来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab4/task3/artifacts/run_output_repeat.txt) 的关键内容：

```text
[pre-fork/page0] ... self_kpagecount=1
[pre-fork/page1] ... self_kpagecount=1
[post-fork/page0] ... self_kpagecount=2 peer_kpagecount=2
[post-fork/page1] ... self_kpagecount=2 peer_kpagecount=2
[child-write/page0] child_minflt_delta=2 child_majflt_delta=0
[parent-write/page1] parent_minflt_delta=1 parent_majflt_delta=0
[acceptance] fork initial values same: PASS
[acceptance] child write isolates page0: PASS
[acceptance] parent write isolates page1: PASS
[acceptance] child first write triggered COW minor fault: PASS
[acceptance] parent first write triggered COW minor fault: PASS
```

第二次运行和第一次完全同型：

- `fork` 前 `kpagecount=1`；
- `fork` 后同页共享且 `kpagecount=2`；
- 子写页 0、父写页 1 时都只出现 minor fault，没有 major fault；
- 所有验收项继续为 `PASS`。

这说明实验结果稳定。

## 7. 机制解释

### 7.1 `fork` 后为什么初始值相同

`fork()` 会复制父进程的地址空间描述，但对私有匿名页通常不会立刻做“整页物理复制”。  
Linux 会让父子 PTE 暂时共同指向同一个物理页，并把双方的映射都设置成只读，以便后续识别“谁先写”。

因此在本实验中：

- `pre-fork` 时 `kpagecount=1`；
- `post-fork` 时父子 PFN 相同，`kpagecount=2`；
- 父子第一次读取到的值自然相同。

### 7.2 为什么写入后另一方不受影响

当某一方第一次写共享页时：

1. CPU 发现该页当前不可写；
2. 触发一次页故障并陷入内核；
3. 内核识别这是 COW 场景；
4. 为写入方分配新物理页，复制旧页内容；
5. 仅把写入方的 PTE 改到新页并恢复可写权限。

所以写入方看到的是“新私有页上的新值”，而另一方仍然留在旧页上，看到原值不变。

### 7.3 为什么这里把 `minflt` 解释为 COW 页故障

本实验不是磁盘换页实验，所以不期待 major fault。  
当共享匿名页第一次因为写保护而触发 COW 时，常见表现就是：

- `ru_minflt` 增加；
- `ru_majflt` 不增加；
- 同时 PFN 发生分裂。

本实验里：

- 子进程写页 0：`child_minflt_delta=2`，`child_majflt_delta=0`；
- 父进程写页 1：`parent_minflt_delta=1`，`parent_majflt_delta=0`。

这里的 `2` 并不表示“两次复制同一页”，而是说明围绕那次写入的极小临界区内至少发生了 minor fault；再结合“写后 PFN 分裂、另一方值不变”这两个更强证据，可以把它定性为目标页的 COW 写保护 fault。

### 7.4 为什么用了两页而不是一页

如果只用一页，并让子进程先写，那么这次写确实会触发复制；  
但等到父进程稍后再写同一页时，该页对父进程往往已经不再由双方共享，内核可能只需要把写权限恢复给父进程，而不是再次复制。

为了保证“父写”和“子写”都发生在仍共享的页上，本实验用同一段映射里的两页：

- 页 0 专门给子进程先写；
- 页 1 专门给父进程后写。

这样两次写入都能观测到真正的 COW 分裂。

## 8. 验收检查映射

- 验收 1：`fork` 调用之后，父子进程读取变量的值初始相同。
  证据：`[post-fork/page0]`、`[post-fork/page1]` 两页在 [artifacts/run_output.txt](/root/os_experiments/lab4/task3/artifacts/run_output.txt) 中都显示父子 PFN 相同、值相同、`kpagecount=2`。
- 验收 2：父或子任一方修改变量后，另一方读取不受影响。
  证据：子写页 0 后父仍读到 `seed_page0`，父写页 1 后子仍读到 `seed_page1`；见 [artifacts/run_output.txt](/root/os_experiments/lab4/task3/artifacts/run_output.txt) 中 `[child-write/page0/*]`、`[parent-write/page1/*]` 和 `[final]`。
- 验收 3：日志证明首次写入时引发了特定的 Page Fault。
  证据：`[child-write/page0] child_minflt_delta=2 child_majflt_delta=0` 与 `[parent-write/page1] parent_minflt_delta=1 parent_majflt_delta=0`，再结合写后 PFN 分裂，证明首次写入触发的是 COW minor fault。

## 9. 环境说明、复现实限与未解决问题

- 本实验是宿主 Linux 用户态实验，不是 QEMU guest。
- 本机内核未提供好用的匿名 COW 次数总计数器；`/proc/vmstat` 中只看到 `cow_ksm 0`，因此本实验改用 `pagemap + kpagecount` 给出页框级证据。
- 读取 `pagemap` / `kpagecount` 需要足够权限；如果在普通用户环境中无法读取，需要退化为“值隔离 + `minflt`”版本。
- 当前会话无法访问第二台原生 Linux 服务器，因此尚未做跨宿主环境复验。
