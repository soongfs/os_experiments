# LAB4 用户态 Task2：Lazy 分页与 Swap 触发器

## 1. 原始任务说明

### 任务标题

Lazy 分页与 Swap 触发器

### 任务目标

构造可重复的内存压力场景，触发按需分页与换入换出机制。

### 任务要求

1. 编写内存消耗型程序，逐步触发缺页并扩大工作集；
2. 若实现了 swap，需触发 swap in/out 并输出可观察证据（计数器/日志）；
3. 给出对照：不同内存压力下的行为差异（例如缺页次数变化、换出次数变化）。

### 验收检查

1. 程序能够平稳申请超出物理内存上限的虚拟内存总量；
2. 运行日志中可明显观察到大量 Page Fault 和换出到磁盘的动作发生。

## 2. Acceptance -> Evidence 清单

- 逐步扩大工作集并持续触发缺页。
  证据：程序在 `grow#1 .. grow#N` 阶段每 16 MiB 打印一次快照，记录 `minflt_total`、`majflt_total`、`resident_pages`、`VmRSS`、`VmSwap`。
- 进程能申请并触达明显超过“实验物理内存上限”的虚拟内存。
  证据：`high` 档在 `memory.max=192MiB` 的 cgroup 中成功 `mmap + touch` 到 `working_set=640MiB`，见 [artifacts/run_output.txt](/root/os_experiments/lab4/task2/artifacts/run_output.txt)。
- 在有 swap 的环境中触发 swap out / swap in。
  证据：`medium`/`high` 档的 `VmSwap` 从 `0` 增长到约 `198MiB` / `462MiB`；系统 `vmstat` 中 `pswpout`、`pswpin` 的 delta 明显大于 0。
- 不同压力下行为差异清晰可见。
  证据：`low` 档 `pswpout=0`、`majflt=0`；`medium`/`high` 档分别出现数十万级 `pgfault` 和数十万页级 `pswpout`。

## 3. 实验目标与实现思路

本实验在 [lab4/task2](/root/os_experiments/lab4/task2) 中实现，运行环境是宿主 Linux 用户态，不是 QEMU guest。

这里没有继续沿用 LAB3 的 bare-metal guest 骨架，而是直接利用宿主 Linux 已经存在的 lazy anonymous paging 与 swap 机制做观测。原因是本题的目标是“构造触发器并拿到直接证据”，而当前仓库尚未包含一个现成的 LAB4 guest 分页/换页内核；在宿主 Linux 上做用户态实验，可以用真实页表、真实缺页、真实 swap 设备拿到稳定数据。

为了避免把整台 15 GiB 宿主机推到接近 OOM，本实验用 `cgroup v2` 给被测进程施加一个可复现的内存上限：

- `memory.max = 192MiB`
- `memory.swap.max = 768MiB`

可以把这 `192MiB` 视为“实验中的可用物理页上限”。程序再去依次触达：

- `low = 128MiB`
- `medium = 384MiB`
- `high = 640MiB`

这样就能在同一台宿主机上稳定比较三档压力：

1. `low`：工作集低于 192 MiB，上升到全驻留，不应发生 swap；
2. `medium`：工作集约为物理上限的 2 倍，应出现明显换出；
3. `high`：工作集约为物理上限的 3.3 倍，回扫时应出现更强的 swap in/out 和更多 major fault。

## 4. 文件列表与代码说明

- [lazy_swap_trigger.c](/root/os_experiments/lab4/task2/lazy_swap_trigger.c)：主实验程序。用匿名 `mmap` 保留大块虚拟地址区间，按 16 MiB 分块逐页触达，并在每个快照点输出 `minflt`、`majflt`、`VmRSS`、`VmSwap`、`resident_pages`。
- [run_cgroup_experiment.sh](/root/os_experiments/lab4/task2/run_cgroup_experiment.sh)：实验驱动脚本。创建独立 cgroup、设置 `memory.max` / `memory.swap.max`，分别运行 `low` / `medium` / `high` 三档，并在前后记录 `vmstat` 与 cgroup `memory.stat`。
- [Makefile](/root/os_experiments/lab4/task2/Makefile)：构建入口。
- [artifacts/build_output.txt](/root/os_experiments/lab4/task2/artifacts/build_output.txt)：最终构建日志。
- [artifacts/run_output.txt](/root/os_experiments/lab4/task2/artifacts/run_output.txt)：第一次完整实验日志。
- [artifacts/run_output_repeat.txt](/root/os_experiments/lab4/task2/artifacts/run_output_repeat.txt)：第二次完整实验日志。
- [artifacts/tool_versions.txt](/root/os_experiments/lab4/task2/artifacts/tool_versions.txt)：工具链、宿主内存、swap、内核与 cgroup 信息。

## 5. 构建、运行与复现步骤

进入任务目录：

```bash
cd /root/os_experiments/lab4/task2
```

构建：

```bash
make
```

运行一次完整实验并保存日志：

```bash
RUN_ID=run1 ./run_cgroup_experiment.sh > artifacts/run_output.txt 2>&1
```

再次运行做复验：

```bash
RUN_ID=run2 ./run_cgroup_experiment.sh > artifacts/run_output_repeat.txt 2>&1
```

查看证据：

```bash
cat artifacts/build_output.txt
cat artifacts/run_output.txt
cat artifacts/run_output_repeat.txt
cat artifacts/tool_versions.txt
```

说明：

- 运行脚本需要能写 `cgroup v2`，通常需要 `root` 或等价权限；
- 当前实验依赖宿主机已经启用 swap。

## 6. 本次实际运行结果

### 6.1 构建结果

[artifacts/build_output.txt](/root/os_experiments/lab4/task2/artifacts/build_output.txt) 的实际内容：

```text
rm -f lazy_swap_trigger
cc -g -O0 -Wall -Wextra -std=c11 -o lazy_swap_trigger lazy_swap_trigger.c
```

### 6.2 环境摘要

[artifacts/tool_versions.txt](/root/os_experiments/lab4/task2/artifacts/tool_versions.txt) 显示：

```text
Mem: 15Gi total, 13Gi available
Swap: 4.0Gi total
vm.swappiness = 60
cgroup2fs
cpu memory pids
```

因此宿主 Linux 确实启用了 swap，并且 `cgroup v2 memory controller` 可用。

### 6.3 第一次完整运行结果

以下关键内容来自 [artifacts/run_output.txt](/root/os_experiments/lab4/task2/artifacts/run_output.txt)。

#### low：128 MiB，低于 192 MiB 物理上限

```text
[snapshot] label=low stage=revisit#1 touched=128MiB ... minflt_total=65551 majflt_total=0 ... VmRSS=132864kB VmSwap=0kB
[vmstat-delta/low] pgfault=68225 pgmajfault=0 pswpin=0 pswpout=0
```

结论：

- 只出现大量 lazy allocation 带来的 page fault；
- 没有 major fault；
- 没有任何 swap in/out。

#### medium：384 MiB，约为物理上限 2 倍

在工作集刚碰到 `192MiB` 附近时：

```text
[snapshot] label=medium stage=grow#12 touched=192MiB ... resident_pages=48937/98304 (49.78%) VmRSS=193664kB VmSwap=4736kB
```

继续扩到 `384MiB` 后：

```text
[snapshot] label=medium stage=grow#24 touched=384MiB ... resident_pages=48877/98304 (49.72%) VmRSS=196864kB VmSwap=198144kB
```

两轮回扫后：

```text
[snapshot] label=medium stage=revisit#2 touched=384MiB ... minflt_total=589515 majflt_total=341 ... VmSwap=198912kB
[vmstat-delta/medium] pgfault=604441 pgmajfault=24676 pswpin=196648 pswpout=246230
```

结论：

- 驻留页数被压在约 `48,9xx` 页，约合 `191 MiB`，与 `memory.max=192MiB` 一致；
- `VmSwap` 增长到约 `199 MiB`，说明匿名页已被换出；
- `pswpout` 和 `pswpin` 都达到数十万页，说明发生了真实 swap out / swap in。

#### high：640 MiB，约为物理上限 3.3 倍

扩到 `640MiB` 末尾时：

```text
[snapshot] label=high stage=grow#40 touched=640MiB ... resident_pages=48738/163840 (29.75%) VmRSS=195300kB VmSwap=461568kB
```

三轮回扫后：

```text
[snapshot] label=high stage=revisit#3 touched=640MiB ... minflt_total=1309881 majflt_total=942 ... VmSwap=462208kB
[vmstat-delta/high] pgfault=1334988 pgmajfault=61870 pswpin=492003 pswpout=607040
```

结论：

- 在 192 MiB 物理上限下，程序仍然平稳触达了 640 MiB 虚拟匿名页；
- 常驻页比例只剩约 `29.7%`，约合 `190 MiB`，其余大部分只能留在 swap；
- `VmSwap` 稳定在约 `462 MiB`；
- 系统级 `pswpout` 超过 `607k` 页，`pswpin` 超过 `492k` 页，换入换出都非常明显。

### 6.4 第二次完整运行结果

来自 [artifacts/run_output_repeat.txt](/root/os_experiments/lab4/task2/artifacts/run_output_repeat.txt) 的关键行：

```text
[vmstat-delta/low] pgfault=68406 pgmajfault=0 pswpin=0 pswpout=0
[vmstat-delta/medium] pgfault=602419 pgmajfault=24720 pswpin=196676 pswpout=246504
[vmstat-delta/high] pgfault=1329859 pgmajfault=61953 pswpin=492165 pswpout=607174
```

以及高压档末尾：

```text
[snapshot] label=high stage=revisit#3 touched=640MiB ... minflt_total=1309884 majflt_total=967 ... VmSwap=462720kB
```

第二次运行与第一次高度一致：

- `low` 仍然没有 swap；
- `medium` 仍然出现约 `246k` 页级别的 `pswpout`；
- `high` 仍然出现约 `607k` 页级别的 `pswpout` 和约 `462 MiB` 的 `VmSwap`。

这说明实验场景具备可重复性。

## 7. 机制解释

### 7.1 为什么匿名 `mmap` 可以触发 lazy paging

程序使用的是：

```text
mmap(PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE)
```

这一步主要只是在进程地址空间里保留一段虚拟地址范围，并不会立刻为每个页分配物理页框。  
真正触发分配的是后续“第一次访问某个虚拟页”的瞬间。

因此本程序按 16 MiB 一块、每页写 1 字节时，会出现：

1. 先有一个很大的 `VmSize`；
2. `VmRSS` 随第一次触页逐步增长；
3. `minflt_total` 在增长阶段持续上升。

这就是最典型的按需分页行为。

### 7.2 为什么会在 192 MiB 左右开始换出

实验脚本把目标进程放到一个单独 cgroup，并设置：

```text
memory.max = 192MiB
memory.swap.max = 768MiB
```

一旦匿名页总量逼近 `192MiB`，内核就不能再无限制地把新页留在 RAM 里。  
此时如果又继续触达新的匿名页，内核只能：

1. 回收旧页；
2. 对匿名脏页执行 swap out；
3. 当程序再次访问被换出的页时，再 swap in 回来。

所以从 `medium` 的 `grow#12` 开始，`VmSwap` 就开始出现；在 `high` 档继续扩张和回扫时，swap in/out 会被放大得非常明显。

### 7.3 `VmRSS`、`VmSwap`、`resident_pages`、`pgfault` 分别说明什么

- `VmRSS`：当前仍驻留在物理内存中的进程页数总量；
- `VmSwap`：该进程已经被换出到 swap 的匿名页总量；
- `resident_pages`：通过 `mincore()` 统计的映射区当前驻留页数；
- `minflt_total`：用户态可见的 minor fault 计数，主要对应首次触页时的 lazy allocation；
- `majflt_total`：用户态可见的 major fault 事件数，主要对应回扫时需要从 swap 重新取回页面的情况；
- `vmstat pswpin/pswpout`：系统级换入/换出页数，更直接反映“有多少页真的在和磁盘 swap 区交换”。

这些计数不是完全同一口径，所以数值不会逐项相等。  
例如本实验里：

- 进程级 `majflt_total` 是“fault 事件”视角；
- `vmstat pswpin/pswpout` 更接近“页迁移”视角。

二者一起看，更能说明“既发生了缺页，也发生了真实的换页 I/O”。

### 7.4 不同压力下的行为差异

- `low`：
  - 工作集能全部常驻；
  - `VmSwap=0`；
  - `pswpin=0`、`pswpout=0`。
- `medium`：
  - 工作集约为物理上限 2 倍；
  - 驻留页数被压在约 192 MiB；
  - `VmSwap≈199MiB`；
  - 已出现明显的 `pswpin/pswpout`。
- `high`：
  - 工作集约为物理上限 3.3 倍；
  - `VmSwap≈462MiB`；
  - `pgfault` 超过 `1.3M`，`pswpout` 超过 `607k` 页；
  - 回扫阶段产生更强的 thrashing。

因此，“工作集越大，缺页和换页越剧烈”的趋势非常明确。

## 8. 验收检查映射

- 验收 1：程序能够平稳申请超出物理内存上限的虚拟内存总量。
  证据：在 `memory.max=192MiB` 的实验上限下，`high` 档成功完成 `working_set=640MiB`，见 [artifacts/run_output.txt](/root/os_experiments/lab4/task2/artifacts/run_output.txt) 中 `=== case=high ... ===` 与 `[done] label=high completed working_set=640MiB`。
- 验收 2：运行日志中可明显观察到大量 Page Fault 和换出到磁盘的动作发生。
  证据：`high` 档第一次运行中 `minflt_total=1309881`、`VmSwap=462208kB`、`[vmstat-delta/high] ... pswpout=607040`；第二次运行中同样有 `pswpout=607174`。这些都记录在 [artifacts/run_output.txt](/root/os_experiments/lab4/task2/artifacts/run_output.txt) 和 [artifacts/run_output_repeat.txt](/root/os_experiments/lab4/task2/artifacts/run_output_repeat.txt)。

## 9. 环境说明、复现实限与未解决问题

- 本实验是宿主 Linux 用户态实验，不是 QEMU guest。
- 为了让“超出物理上限”可重复且不影响整机稳定性，实验使用了 cgroup 人为收窄被测进程的可用内存上限；这里的“物理内存上限”是实验上限 `memory.max=192MiB`，不是整台宿主机的 15 GiB 总内存。
- 若宿主机没有启用 swap，本程序仍能展示 lazy page fault 和驻留集受限，但不会产生 `pswpin/pswpout` 的证据。
- 当前会话无法访问第二台原生 Linux 服务器，因此还没有做跨宿主环境复验。
