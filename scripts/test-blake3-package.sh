#!/usr/bin/env bash
# Link from a relocated toolchain and inspect the *application's* dependency table.
set -euo pipefail
if [[ $# != 2 ]]; then
  echo "usage: scripts/test-blake3-package.sh TOOLCHAIN_DIR PROVIDER_ARCHIVE" >&2
  exit 2
fi
toolchain=$(cd "$1" && pwd)
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
mkdir "$scratch/bin"
cp "$toolchain/alignc" "$toolchain/libalign_runtime.a" "$scratch/bin/"
cp "$2" "$scratch/bin/libblake3.a"
cd "$scratch"
cat > main.align <<'ALIGN'
import std.crypto
import std.encoding
fn main() {
  digest := crypto.blake3("")
  print(encoding.hex_encode(digest[0..digest.len()]))
}
ALIGN
verify_image() {
  local libraries
  if [[ "$(uname -s)" == Darwin ]]; then
    libraries="$(otool -L ./main)"
  else
    libraries="$(readelf -d ./main)"
  fi
  if [[ "$libraries" == *libblake3* ]]; then
    echo "unexpected dynamic BLAKE3 dependency: $libraries" >&2
    exit 1
  fi
}
XDG_CACHE_HOME="$scratch/cache" bin/alignc build main.align
verify_image
[[ "$(./main)" == af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262 ]]
# A warm frontend/object hit must still open the selected provider at link time.
mv bin/libblake3.a bin/verified.a
printf '%s\n' 'invalid archive' > bin/libblake3.a
if XDG_CACHE_HOME="$scratch/cache" bin/alignc build main.align >link.out 2>&1; then
  echo "a cached build ignored a replaced BLAKE3 archive" >&2
  exit 1
fi
mv bin/verified.a bin/libblake3.a
XDG_CACHE_HOME="$scratch/cache" bin/alignc build main.align
verify_image
[[ "$(./main)" == af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262 ]]
# A non-BLAKE3 binary must link even with a corrupt companion present.
printf '%s\n' 'invalid archive' > bin/libblake3.a
cat > main.align <<'ALIGN'
import std.crypto
fn main() { digest := crypto.sha256(""); print(digest.len()) }
ALIGN
XDG_CACHE_HOME="$scratch/cache" bin/alignc build main.align
verify_image
[[ "$(./main)" == 32 ]]
echo "packaged BLAKE3 and independent SHA-256 links passed"
