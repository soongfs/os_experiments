#!/usr/bin/env bash

set -euo pipefail

WORKDIR="${HOME}/os_experiments"

if [ "$(id -u)" -eq 0 ]; then
  SUDO=""
else
  SUDO="sudo"
fi

mkdir -p "${WORKDIR}"
cd "${WORKDIR}"

. "${HOME}/.cargo/env"

rustup target add riscv64gc-unknown-none-elf
rustup component add llvm-tools-preview
cargo install cargo-binutils

${SUDO} apt-get update
DEBIAN_FRONTEND=noninteractive ${SUDO} apt-get install -y qemu-system-misc

rustup target list | grep riscv64gc
qemu-system-riscv64 --version
