#!/usr/bin/env bash
# Run Cargo with the repository's LLVM 22 environment on macOS or Debian-family Linux.
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "usage: scripts/cargo.sh CARGO_ARGUMENT..." >&2
  exit 2
fi
command -v cargo >/dev/null 2>&1 || { echo "cargo is required" >&2; exit 2; }

# shellcheck source=scripts/dyld-env.sh
. "$(dirname "$0")/dyld-env.sh"

llvm_config="${LLVM_CONFIG:-}"
if [[ -n "$llvm_config" ]]; then
  resolved="$(command -v "$llvm_config" 2>/dev/null || true)"
  [[ -n "$resolved" ]] || { echo "LLVM_CONFIG is not executable: $llvm_config" >&2; exit 2; }
  llvm_config="$resolved"
else
  candidates=(
    llvm-config-22
    /usr/lib/llvm-22/bin/llvm-config
    /opt/homebrew/opt/llvm@22/bin/llvm-config
    /opt/homebrew/opt/llvm/bin/llvm-config
    /usr/local/opt/llvm@22/bin/llvm-config
    /usr/local/opt/llvm/bin/llvm-config
    llvm-config
  )
  for candidate in "${candidates[@]}"; do
    resolved="$(command -v "$candidate" 2>/dev/null || true)"
    [[ -n "$resolved" ]] || continue
    version="$($resolved --version 2>/dev/null || true)"
    [[ "${version%%.*}" == 22 ]] || continue
    llvm_config="$resolved"
    break
  done
fi
[[ -n "$llvm_config" ]] || {
  echo "LLVM 22 was not found. Install llvm-22-dev on Debian/Ubuntu, or brew install llvm@22 on macOS." >&2
  echo "You may also set LLVM_CONFIG to an LLVM 22 llvm-config executable." >&2
  exit 2
}
version="$($llvm_config --version)"
[[ "${version%%.*}" == 22 ]] || {
  echo "Align requires LLVM 22; $llvm_config reports $version" >&2
  exit 2
}

llvm_prefix="$($llvm_config --prefix)"
export LLVM_CONFIG="$llvm_config"
export LLVM_SYS_221_PREFIX="${LLVM_SYS_221_PREFIX:-$llvm_prefix}"

prepend_library_path() {
  local directory="$1"
  [[ -d "$directory" ]] || return 0
  case ":${LIBRARY_PATH:-}:" in
    *":$directory:"*) ;;
    *) LIBRARY_PATH="$directory${LIBRARY_PATH:+:$LIBRARY_PATH}" ;;
  esac
}

# Homebrew keeps LLVM, OpenSSL, and libpq keg-only (keg-only kegs are not
# symlinked into /opt/homebrew/lib). Debian/Ubuntu packages install their link
# libraries in compiler-default paths, so Linux needs no mutation.
if [[ "$(uname -s)" == Darwin ]]; then
  prepend_library_path "$llvm_prefix/lib"
  prepend_library_path /opt/homebrew/lib
  prepend_library_path /opt/homebrew/opt/openssl@3/lib
  prepend_library_path /opt/homebrew/opt/libpq/lib
  prepend_library_path /usr/local/lib
  prepend_library_path /usr/local/opt/openssl@3/lib
  prepend_library_path /usr/local/opt/libpq/lib
  export LIBRARY_PATH
  align_use_private_dyld_region
fi

exec cargo "$@"
