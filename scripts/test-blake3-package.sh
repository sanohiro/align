#!/usr/bin/env bash
# Inspect the application's dynamic dependencies after each native link.
set -euo pipefail
if [[ $# != 2 ]]; then
  echo "usage: scripts/test-blake3-package.sh TOOLCHAIN_DIR PROVIDER_ARCHIVE | --shared COMPILER | --installed COMPILER" >&2
  exit 2
fi
case "$1" in
  --shared|--installed)
    mode=$1
    compiler=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
    ;;
  *)
    mode=relocated
    toolchain=$(cd "$1" && pwd)
    archive=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
    ;;
esac
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
cd "$scratch"
if [[ "$mode" == relocated ]]; then
  mkdir bin
  cp "$toolchain/alignc" "$toolchain/libalign_runtime.a" bin/
  cp "$archive" bin/libblake3.a
  compiler=$scratch/bin/alignc
fi
export XDG_CACHE_HOME="$scratch/cache"
verify_program() {
  local expected=$1 dependency=$2 libraries
  "$compiler" build main.align
  [[ "$(./main)" == "$expected" ]]
  if [[ "$(uname -s)" == Darwin ]]; then
    libraries=$(otool -L ./main)
  else
    libraries=$(readelf -d ./main)
  fi
  case "$dependency:$libraries" in
    shared:*libblake3*) ;;
    absent:*libblake3*|shared:*)
      echo "expected BLAKE3 dependency $dependency, inspected: $libraries" >&2
      exit 1
      ;;
  esac
}
cat > main.align <<'ALIGN'
import std.crypto
import std.encoding
fn main() {
  digest := crypto.blake3("")
  print(encoding.hex_encode(digest[0..digest.len()]))
}
ALIGN
empty=af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262
if [[ "$mode" == --shared ]]; then
  verify_program "$empty" shared
else
  verify_program "$empty" absent
fi
if [[ "$mode" == relocated ]]; then
  # A warm frontend/object hit must still open the selected provider at link time.
  mv bin/libblake3.a bin/verified.a
  printf '%s\n' 'invalid archive' > bin/libblake3.a
  if "$compiler" build main.align >link.out 2>&1; then
    echo "a cached build ignored a replaced BLAKE3 archive" >&2
    exit 1
  fi
  mv bin/verified.a bin/libblake3.a
  verify_program "$empty" absent
  # Unrelated programs must also link with a corrupt companion present.
  printf '%s\n' 'invalid archive' > bin/libblake3.a
fi
cat > main.align <<'ALIGN'
import std.crypto
fn main() { digest := crypto.sha256(""); print(digest.len()) }
ALIGN
verify_program 32 absent
printf '%s\n' 'fn main() { print(42) }' > main.align
verify_program 42 absent
echo "verified $mode BLAKE3 dependency and independent SHA-256 and scalar links"
