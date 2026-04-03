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

${SUDO} apt-get update
DEBIAN_FRONTEND=noninteractive ${SUDO} apt-get install -y build-essential bsdextrautils curl git

curl https://sh.rustup.rs -sSf | sh -s -- -y

# Load Rust tools into the current shell so version checks work immediately.
. "${HOME}/.cargo/env"

rustc --version
cargo --version
