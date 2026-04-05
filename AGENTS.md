# Repository Guidelines

## Scope & Repository Layout

This repository covers the full OS course sequence from `lab0` through `lab7`, not only `lab0`. Keep each task self-contained inside its own lab directory, with source code, a task-level `README.md`, and any required evidence files stored together.

Use these directory conventions unless the user explicitly requests otherwise:

- User-space tasks: `labX/taskY/`
- Kernel-space tasks: `labX/kernel_taskY/`
- Task evidence: `labX/<task>/artifacts/`
- Reusable environment or helper scripts: `scripts/`

For simple tasks, a single `.c` or `.rs` file is acceptable. For larger tasks, follow the `lab0/task3` pattern and split entrypoint, shared state, trap or concurrency logic, and persistence or reporting logic into separate modules.

If a new task would conflict with an existing task directory that already serves a different lab requirement, prefer creating a new task directory instead of overloading the old one.

## Required Environment

All labs should be reproducible on WSL Debian and on a native Linux server. The baseline host toolchain should include:

- `build-essential`
- `curl`
- `git`
- `bsdextrautils` for `hexdump`

Rust should be installed through `rustup`. For RISC-V and bare-metal labs, the environment should also include:

- `riscv64gc-unknown-none-elf`
- `llvm-tools-preview`
- `cargo-binutils`
- `qemu-system-misc` with `qemu-system-riscv64`

Use `scripts/setup_host_env.sh` and `scripts/setup_riscv_env.sh` as the canonical setup entrypoints when present.

Verify the environment with:

- `rustc --version`
- `cargo --version`
- `rustup target list | grep riscv64gc`
- `qemu-system-riscv64 --version`

## Build, Test, and Verification Commands

Common Linux-side build commands:

- `gcc -g -O0 -Wall -Wextra -o <bin> <src>.c`
- `make`
- `make clean`

Common Rust and RISC-V commands:

- `cargo build`
- `cargo build --release`
- `cargo objdump --bin <bin> -- --demangle -d`
- `cargo nm --bin <bin> -- --demangle`
- `qemu-system-riscv64 -machine virt -bios none -nographic -kernel <elf>`

Common output verification commands:

- `cat <file>`
- `hexdump -C <file>`
- `sed -n '1,120p' <file>`

Always record the exact commands and observable results in the task README.

If a task involves concurrency, scheduling, timing, exceptions, system call statistics, trap handling, or any other non-deterministic or runtime-sensitive behavior, run it multiple times and preserve representative outputs.

## README Requirements

Each task-level `README.md` should be a reviewable experiment record, not a placeholder. Include at least:

- The original task statement copied from the user prompt
- The experiment goal and chosen implementation approach
- A file list with brief code explanations
- Build, run, and reproduction commands
- Actual observed outputs that satisfy the acceptance criteria
- A mechanism explanation for the relevant OS path, such as syscall flow, trap flow, timing source, fault handling, paging, scheduling, or QEMU boot flow
- An acceptance checklist mapping observed results back to the task requirements
- Environment notes, reproduction limits, and any unresolved issues

If a requested second environment, such as a native Linux server, is unavailable, state that explicitly in the README.

## Artifact & Ignore Rules

Keep generated review artifacts under a task-local `artifacts/` directory. Text logs, hexdumps, symbol dumps, and similar review evidence may be committed when they are required for verification.

Do not commit transient build outputs unless a task explicitly requires them as evidence. In particular:

- Add each new Rust task's `target/` directory to the repository `.gitignore`
- Do not commit Linux binaries, `.o` files, or scratch outputs
- Do not commit QEMU-generated temporary files outside the task directory

Stage only task-related files and any necessary `.gitignore` updates.

## Coding Style

Use 4-space indentation, same-line braces, and `snake_case` names for files, functions, and variables.

Keep new code warning-free:

- C code should compile cleanly under `-Wall -Wextra`
- Rust code should build without warnings unless a task explicitly justifies an exception

Prefer straightforward, reviewable code over cleverness. When working in bare-metal Rust, keep unsafe blocks narrow and tied to the hardware or trap mechanism being exercised.

## Commit Guidelines

Follow the existing Git history style: short, imperative commit subjects scoped to one lab increment or one environment change, for example:

- `Add LAB0 task2/task3 and hexdump setup`
- `Add LAB2 kernel task3 completion timing`

Keep each commit focused. In PRs or handoff notes, include the relevant build commands, verification commands, and artifact paths.
