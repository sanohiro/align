#!/usr/bin/env bash
# Install the official C provider into an explicit prefix. Source builds may
# instead use their distribution's libblake3 development package.
set -euo pipefail
if [[ $# -lt 2 || $# -gt 3 || "$1" != /* || ( $# == 3 && "$3" != --github-env ) ]]; then
  echo "usage: scripts/install-blake3.sh /absolute/prefix shared|static [--github-env]" >&2
  exit 2
fi
prefix=$1
case "$2" in
  shared) shared=ON ;;
  static) shared=OFF ;;
  *) echo "expected shared or static" >&2; exit 2 ;;
esac
version=1.8.7
checksum=c6782a28842b1c0478524ac06a4f2ede784038ee298d6e2162c0b089c4306a3c
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
cd "$scratch"
curl --fail --silent --show-error --location --retry 3 \
  "https://codeload.github.com/BLAKE3-team/BLAKE3/tar.gz/refs/tags/$version" -o provider.tar.gz
printf '%s  provider.tar.gz\n' "$checksum" | shasum -a 256 -c -
tar -xzf provider.tar.gz
src="BLAKE3-$version"
cmake -S "$src/c" -B build -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS="$shared" -DBLAKE3_USE_TBB=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
  -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib
cmake --build build --parallel 2
cmake --install build
install -d "$prefix/share/licenses/blake3"
install -m 644 "$src/LICENSE_A2" "$prefix/share/licenses/blake3/LICENSE_A2"
if [[ $# == 3 ]]; then
  {
    echo "BLAKE3_INCLUDE_DIR=$prefix/include"
    echo "LIBRARY_PATH=$prefix/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
    echo "LD_LIBRARY_PATH=$prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  } >> "${GITHUB_ENV:?expected a GitHub Actions environment file}"
fi
