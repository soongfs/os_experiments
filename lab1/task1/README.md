# LAB1 Task1: 目录遍历与文件名展示

## 1. 原始任务说明

### 任务标题

目录遍历与文件名展示

### 任务目标

熟悉 Linux 用户态基本 I/O 接口与目录项遍历机制，理解“文件名展示”背后的系统调用与库封装关系。

### 任务要求

1. 实现 Linux 应用程序，输出当前工作目录下所有文件/目录名称；
2. 允许使用标准库接口（如 `opendir/readdir` 或等价方案），但需在实验记录中说明其对应的系统调用含义；
3. 输出格式需清晰（每行一个名称或使用序号）。

### 验收检查

1. 程序能正确打印当前目录下所有文件/目录名称，无遗漏或乱码；
2. 正确指出 `opendir/readdir` 底层调用的 Linux syscall（如 `getdents64`）。

## 2. 实验目标与实现思路

本实验使用 C 语言和 POSIX 目录遍历接口 `opendir()`、`readdir()`、`closedir()` 实现。程序先打开当前目录 `"."`，逐个读取目录项名称，跳过特殊项 `.` 和 `..`，将其保存到内存中，按字典序排序后，以“序号 + 文件名”的方式逐行输出。

这样既满足“输出清晰”的要求，也便于和 `ls -1A` 的结果逐项比对。

## 3. 文件列表

- [list_dir_entries.c](/root/os_experiments/lab1/task1/list_dir_entries.c)：目录遍历程序源码。
- [README.md](/root/os_experiments/lab1/task1/README.md)：实验说明与验收记录。

## 4. 源代码与说明

文件：[list_dir_entries.c](/root/os_experiments/lab1/task1/list_dir_entries.c)

```c
#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int compare_names(const void *left, const void *right) {
    const char *const *lhs = (const char *const *)left;
    const char *const *rhs = (const char *const *)right;
    return strcmp(*lhs, *rhs);
}

int main(void) {
    DIR *dir;
    struct dirent *entry;
    char cwd[4096];
    char **names = NULL;
    size_t count = 0;
    size_t capacity = 0;
    size_t i;

    if (getcwd(cwd, sizeof(cwd)) == NULL) {
        perror("getcwd");
        return 1;
    }

    dir = opendir(".");
    if (dir == NULL) {
        perror("opendir");
        return 1;
    }

    while ((entry = readdir(dir)) != NULL) {
        char *copy;

        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }

        if (count == capacity) {
            size_t new_capacity = (capacity == 0) ? 8 : capacity * 2;
            char **new_names = realloc(names, new_capacity * sizeof(char *));
            if (new_names == NULL) {
                perror("realloc");
                closedir(dir);
                free(names);
                return 1;
            }
            names = new_names;
            capacity = new_capacity;
        }

        copy = strdup(entry->d_name);
        if (copy == NULL) {
            perror("strdup");
            closedir(dir);
            for (i = 0; i < count; ++i) {
                free(names[i]);
            }
            free(names);
            return 1;
        }

        names[count++] = copy;
    }

    if (closedir(dir) != 0) {
        perror("closedir");
        for (i = 0; i < count; ++i) {
            free(names[i]);
        }
        free(names);
        return 1;
    }

    qsort(names, count, sizeof(char *), compare_names);

    printf("Current working directory: %s\n", cwd);
    printf("Directory entries:\n");
    for (i = 0; i < count; ++i) {
        printf("%zu. %s\n", i + 1, names[i]);
        free(names[i]);
    }

    free(names);
    return 0;
}
```

代码要点：

- `opendir(".")` 打开当前工作目录；
- `readdir()` 逐个读取目录项名称；
- `strcmp()` 过滤特殊项 `.` 和 `..`；
- `qsort()` 按名称排序，保证输出稳定、便于验收；
- `printf()` 按序号逐行打印结果。

## 5. 编译、运行与复现方法

进入任务目录：

```bash
cd /root/os_experiments/lab1/task1
```

编译：

```bash
gcc -g -O0 -Wall -Wextra -o list_dir_entries list_dir_entries.c
```

运行：

```bash
./list_dir_entries
```

使用 `ls -1A` 对照验证：

```bash
ls -1A
```

## 6. 本次实际运行结果

本次在 `lab1/task1` 目录中的程序输出：

```text
Current working directory: /root/os_experiments/lab1/task1
Directory entries:
1. README.md
2. list_dir_entries
3. list_dir_entries.c
```

同一目录下，`ls -1A` 的实际输出为：

```text
README.md
list_dir_entries
list_dir_entries.c
```

两者包含的目录项名称一致，说明程序能够正确输出当前工作目录下的文件名称，无遗漏、无乱码。

## 7. `opendir/readdir` 与 Linux syscall 的对应关系

本实验使用的是标准库接口，但目录项最终不是由标准库“凭空生成”的，而是由 Linux 内核提供。

在 Linux/glibc 环境下，可以这样理解：

- `opendir()`：负责打开目录并建立 `DIR *` 目录流。底层通常会调用 `open()` 或 `openat()` 之类的系统调用来拿到目录文件描述符。
- `readdir()`：从目录流中取出一个目录项。真正向内核批量读取目录项记录时，底层关键 syscall 是 `getdents64`。
- `closedir()`：关闭目录流，底层对应 `close()`。

因此，本题要求指出的核心目录遍历 syscall 是：

```text
getdents64
```

## 8. 机制解释

1. Linux 中，目录本身也是一种特殊文件，目录中的“文件名 -> inode”映射以目录项记录的形式保存在内核管理的文件系统中。
2. 用户态程序调用 `opendir(".")` 时，glibc 会先打开当前目录，获得一个目录文件描述符，并为它分配一个 `DIR` 结构作为用户态缓冲与迭代状态。
3. 当程序调用 `readdir()` 时，glibc 会先查看 `DIR` 的内部缓冲区里是否还有未消费的目录项；如果没有，就通过 `getdents64` 向内核请求一批新的目录项记录。
4. 内核从文件系统中读取当前目录的目录项，把多个 `linux_dirent64` 记录拷贝回用户态缓冲区；glibc 再把这些记录逐个解析成 `struct dirent`，每次返回一个给程序。
5. 程序拿到 `d_name` 后，就可以把文件名打印出来，或者像本实验一样先保存、排序，再按清晰格式展示。

所以，“文件名展示”表面上看是 `printf()` 输出字符串，实质上依赖的是：

- libc 用 `opendir/readdir` 提供易用封装；
- 内核用 `open/openat`、`getdents64`、`close` 提供底层目录访问能力。

## 9. 环境说明与限制

- 本次实验在当前 Linux 环境中完成并验证；
- 本回合未在第二台原生 Linux 云服务器上再次复现；
- 程序有意跳过 `.` 和 `..`，因为它们是特殊目录项，不属于通常所说的“当前目录下文件/目录名称”列表。
