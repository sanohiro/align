#!/bin/sh
# Configure the LLVM 22 and Align apt repositories on Ubuntu 24.04.
# Usage: curl -fsSL https://sanohiro.github.io/align/install.sh | sudo sh
set -eu

if [ "$(id -u)" -ne 0 ]; then
  echo "install.sh must run as root (for example: curl ... | sudo sh)" >&2
  exit 1
fi

if [ ! -f /etc/os-release ]; then
  echo "alignc packages currently support Ubuntu 24.04 (noble) only" >&2
  exit 1
fi
. /etc/os-release
if [ "${ID:-}" != "ubuntu" ] || [ "${VERSION_CODENAME:-}" != "noble" ]; then
  echo "alignc packages currently support Ubuntu 24.04 (noble) only" >&2
  exit 1
fi

apt-get update
apt-get install -y ca-certificates curl gpg
install -d -m 0755 /etc/apt/keyrings

curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key \
  | gpg --dearmor --yes -o /etc/apt/keyrings/apt.llvm.org.gpg
echo "deb [signed-by=/etc/apt/keyrings/apt.llvm.org.gpg] https://apt.llvm.org/noble/ llvm-toolchain-noble-22 main" \
  > /etc/apt/sources.list.d/llvm-22.list

curl -fsSL https://sanohiro.github.io/align/align.gpg \
  | gpg --dearmor --yes -o /etc/apt/keyrings/align.gpg
echo "deb [signed-by=/etc/apt/keyrings/align.gpg] https://sanohiro.github.io/align stable main" \
  > /etc/apt/sources.list.d/align.list

apt-get update
echo "Align repository configured. Install the compiler with: apt install alignc"
