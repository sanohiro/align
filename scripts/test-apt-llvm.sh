#!/usr/bin/env bash
# Focused tests for scripts/ci-apt-llvm.sh. No root, no apt, no network.
#
# This script stopped CI twice in one day — once by caching a package set that
# `dpkg --install` can never replay, and once because the recovery that was
# supposed to catch that ran before the repository it needed existed. Neither
# defect is visible by reading the file; both are visible the moment the branch
# is executed. So the branches are executed here.
#
# The script under test is copied and rewritten so that /etc and the LLVM
# prefix point inside a temporary directory, and apt-get, apt-cache, dpkg,
# dpkg-query, gpg, wget, sudo and install are replaced by stubs on PATH. The
# stubs record every apt-get invocation together with whether the apt.llvm.org
# sources.list entry existed at the time, which is what makes the recovery
# ordering assertable at all.
#
# Runs on the macOS-provided bash 3.2 and on current Debian/Ubuntu bash.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
under_test="$repo_root/scripts/ci-apt-llvm.sh"
tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

bin="$tmp_dir/bin"
fakeroot="$tmp_dir/root"
prefix="$tmp_dir/llvm-prefix"
script="$tmp_dir/ci-apt-llvm-under-test.sh"
apt_log="$tmp_dir/apt.log"
sources="$fakeroot/etc/apt/sources.list.d/align-apt-llvm-org.list"
keyring="$fakeroot/etc/apt/keyrings/apt-llvm-org.asc"
# Must match scripts/ci-apt-llvm.sh's pin; a drift here is a test bug, and the
# fingerprint case below proves the script actually compares against it.
key_fingerprint=6084F3CF814B57C1CF12EFD515CF4D18AF4F7421

mkdir -p "$bin" "$prefix/bin" \
  "$fakeroot/etc/apt/apt.conf.d" "$fakeroot/etc/apt/sources.list.d"
printf 'ID=ubuntu\nVERSION_CODENAME=noble\n' > "$fakeroot/etc/os-release"
printf '#!/bin/sh\necho 22.1.8\n' > "$prefix/bin/llvm-config"
chmod +x "$prefix/bin/llvm-config"

cat > "$bin/sudo" <<'STUB'
#!/bin/bash
while [[ "$1" == *=* ]]; do shift; done
exec "$@"
STUB

cat > "$bin/install" <<'STUB'
#!/bin/bash
directory=0
args=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -d) directory=1 ;;
    -m | -o | -g) shift ;;
    *) args+=("$1") ;;
  esac
  shift
done
if [[ "$directory" -eq 1 ]]; then
  mkdir -p "${args[@]}"
else
  mkdir -p "$(dirname "${args[1]}")"
  cp "${args[0]}" "${args[1]}"
fi
STUB

cat > "$bin/dpkg" <<'STUB'
#!/bin/bash
case "$1" in
  --print-architecture) echo amd64; exit 0 ;;
  --configure) exit 0 ;;
esac
if [[ "${STUB_DPKG_FAIL:-0}" == 1 && "$1" == "--install" ]]; then
  echo "stub dpkg: conflicting packages - not installing python3-lldb-22" >&2
  exit 1
fi
exit 0
STUB

cat > "$bin/dpkg-query" <<'STUB'
#!/bin/bash
[[ "${STUB_PKG_MISSING:-0}" == 1 ]] && exit 1
printf installed
STUB

cat > "$bin/cc" <<'STUB'
#!/bin/bash
exit 0
STUB

cat > "$bin/gpg" <<'STUB'
#!/bin/bash
# One unrelated key first, so a test that passes only because the script reads
# the first fpr record would fail here.
printf 'pub:-:4096:1:0000000000000000:::::::\n'
printf 'fpr:::::::::%s:\n' "0000000000000000000000000000000000000000"
printf 'pub:-:4096:1:15CF4D18AF4F7421:::::::\n'
printf 'fpr:::::::::%s:\n' "${STUB_GPG_FPR:?}"
STUB

cat > "$bin/wget" <<'STUB'
#!/bin/bash
[[ "${STUB_WGET_FAIL:-0}" == 1 ]] && exit 1
out=""
previous=""
for argument in "$@"; do
  [[ "$previous" == "-O" ]] && out="$argument"
  previous="$argument"
done
[[ -n "$out" ]] && printf -- '-----BEGIN PGP PUBLIC KEY BLOCK-----\nstub\n' > "$out"
exit 0
STUB

# A realistic `apt-cache policy` version table. STUB_GOOD_SUITE names the one
# suite apt.llvm.org actually serves; every other suite yields a table whose
# only entry is the locally installed version, which is exactly the shape that
# must NOT be accepted as a working repository.
cat > "$bin/apt-cache" <<'STUB'
#!/bin/bash
suite=""
[[ -f "$STUB_SOURCES" ]] && suite="$(awk '{ print $(NF-1) }' "$STUB_SOURCES")"
printf '%s:\n' "$2"
if [[ "$suite" == "${STUB_GOOD_SUITE:-}" ]]; then
  printf '  Installed: (none)\n  Candidate: 1:22.1.8-1\n  Version table:\n'
  printf '     1:22.1.8-1 500\n'
  printf '        500 https://apt.llvm.org/noble %s/main amd64 Packages\n' "$suite"
else
  # Installed locally (a restored set unpacked it), served by nobody.
  printf '  Installed: 1:22.1.8-1\n  Candidate: 1:22.1.8-1\n  Version table:\n'
  printf ' *** 1:22.1.8-1 100\n'
  printf '        100 /var/lib/dpkg/status\n'
fi
STUB

cat > "$bin/apt-get" <<'STUB'
#!/bin/bash
# Record the call and whether the apt.llvm.org list already existed, so the
# recovery ordering is assertable rather than assumed.
{
  printf 'repo=%s call=' "$([[ -f "$STUB_SOURCES" ]] && echo yes || echo no)"
  printf '%s ' "$@"
  printf '\n'
} >> "$STUB_LOG"
[[ "$1" == "update" ]] && exit "${STUB_APT_UPDATE_RC:-0}"
for argument in "$@"; do
  if [[ "$argument" == "--fix-broken" ]]; then
    for flag in "$@"; do
      [[ "$flag" == "--no-remove" ]] && exit "${STUB_APT_FIX_NOREMOVE_RC:-0}"
    done
    exit "${STUB_APT_FIX_RC:-0}"
  fi
done
[[ "${STUB_APT_INSTALL_RC:-0}" != 0 ]] && exit "${STUB_APT_INSTALL_RC}"
if [[ "${STUB_APT_RESOLVES:-1}" == 1 ]]; then
  mkdir -p "$STUB_ARCHIVES"
  printf 'resolved-a' > "$STUB_ARCHIVES/llvm-22-dev_1.deb"
  printf 'resolved-b' > "$STUB_ARCHIVES/clang-22_1.deb"
fi
exit 0
STUB

# The script under test uses sha256sum; a stock macOS has only shasum.
if ! command -v sha256sum >/dev/null 2>&1; then
  cat > "$bin/sha256sum" <<'STUB'
#!/bin/bash
exec shasum -a 256 "$@"
STUB
fi

chmod +x "$bin"/*
export PATH="$bin:$PATH"

# /etc and /usr/lib are not writable here (and must not be touched even where
# they are), so the copy under test is redirected into the temporary tree. Both
# rewrites are asserted: a silent miss would run the real paths.
sed -e "s#/usr/lib/llvm-\${LLVM_VERSION}#$prefix#g" \
  -e "s#/etc/#$fakeroot/etc/#g" \
  "$under_test" > "$script"
grep -q "$prefix" "$script" || {
  echo "the LLVM prefix rewrite matched nothing; refusing to run" >&2
  exit 1
}
grep -q "$fakeroot/etc/apt/sources.list.d" "$script" || {
  echo "the /etc rewrite matched nothing; refusing to run" >&2
  exit 1
}
grep -Fq "$key_fingerprint" "$under_test" || {
  echo "scripts/ci-apt-llvm.sh no longer pins $key_fingerprint" >&2
  exit 1
}

export RUNNER_OS=Linux RUNNER_ARCH=X64 ImageVersion=20260803.1.0
export ALIGN_APT_PACKAGES="llvm-22-dev clang-22"
export STUB_SOURCES="$sources"
export STUB_LOG="$apt_log"
export STUB_GPG_FPR="$key_fingerprint"
export STUB_GOOD_SUITE=llvm-toolchain-noble-22

failures=0
output=""

fail() {
  echo "FAIL: $1" >&2
  [[ -n "$output" ]] && printf '%s\n' "$output" | sed 's/^/      /' >&2
  [[ -s "$apt_log" ]] && sed 's/^/      apt: /' "$apt_log" >&2
  failures=$((failures + 1))
}

seed_archives() { # $1=directory $2=manifest shape
  rm -rf "$1"
  mkdir -p "$1"
  printf 'aaa' > "$1/llvm-22-dev_1.deb"
  printf 'bbb' > "$1/clang-22_1.deb"
  case "$2" in
    good) (cd "$1" && sha256sum ./*.deb > SHA256SUMS) ;;
    none) ;;
    empty) rm -f "$1"/*.deb ;;
    truncated) (cd "$1" && sha256sum ./llvm-22-dev_1.deb > SHA256SUMS) ;;
    corrupt)
      (cd "$1" && sha256sum ./*.deb > SHA256SUMS)
      printf 'XXX' > "$1/clang-22_1.deb"
      ;;
  esac
}

# $1=label $2=manifest shape $3=expected exit, then any NAME=VALUE stub
# settings, then `--` followed by any extra arguments for the install mode.
run_install() {
  local label="$1" manifest="$2" want="$3"
  shift 3
  local environment=()
  while [[ $# -gt 0 && "$1" != "--" ]]; do
    environment+=("$1")
    shift
  done
  [[ "${1:-}" == "--" ]] && shift

  local run_dir="$tmp_dir/run"
  rm -rf "$run_dir"
  mkdir -p "$run_dir"
  rm -f "$sources" "$keyring"
  : > "$apt_log"
  seed_archives "$run_dir/apt-archives-llvm-22" "$manifest"

  # bash 3.2 treats an empty array as unbound under `set -u`.
  local status=0
  output="$(env RUNNER_TEMP="$run_dir" STUB_ARCHIVES="$run_dir/apt-archives-llvm-22" \
    ${environment[@]+"${environment[@]}"} \
    /bin/bash "$script" install "$@" 2>&1)" || status=$?
  [[ "$status" == "$want" ]] || fail "$label: exit $status, expected $want"
  archives_dir="$run_dir/apt-archives-llvm-22"
}

expect_output() {
  grep -Eq "$2" <<< "$output" || fail "$1"
}
refute_output() {
  grep -Eq "$2" <<< "$output" && fail "$1"
  return 0
}
expect_apt() {
  grep -Eq "$2" "$apt_log" || fail "$1"
}
refute_apt() {
  grep -Eq "$2" "$apt_log" && fail "$1"
  return 0
}

# --- the authoritative install must never be allowed to remove a package -----
# An apt transaction containing a removal cannot be replayed by `dpkg
# --install`, which is precisely how the first cached generation broke. This is
# the one assertion that keeps that class closed by machinery.
run_install "authoritative install" empty 0
expect_apt "the cache-miss authoritative install omits --no-remove" \
  'call=install -y --no-remove llvm-22-dev clang-22'
run_install "uncached install --no-remove" none 0 -- --uncached
expect_apt "the uncached authoritative install omits --no-remove" \
  'call=install -y --no-remove llvm-22-dev clang-22'
run_install "repair install --no-remove" good 0 STUB_DPKG_FAIL=1 --
expect_apt "the repair path's authoritative install omits --no-remove" \
  'call=install -y --no-remove '

# --- cache hit: no apt, no repository, no network ---------------------------
run_install "clean hit" good 0
expect_output "a clean hit is not reported" 'installed the cached LLVM 22 package set \(2 archives\)'
refute_apt "a clean hit still ran apt-get" '.'
[[ -f "$sources" ]] && fail "a clean hit added the apt.llvm.org repository"

# --- restored-set rejection ------------------------------------------------
run_install "missing manifest" none 0
expect_output "a missing manifest is not reported" 'no SHA256SUMS manifest'
run_install "truncated manifest" truncated 0
expect_output "a short manifest is not reported" 'holds 2 archives but lists 1'
run_install "corrupt archive" corrupt 0
expect_output "a corrupt archive does not fall back" 'falling back to a full apt install'

# --- recovery ordering: repository first, then the two repair stages --------
# The g1 failure was exactly this order being wrong: apt cannot resolve an
# apt.llvm.org dependency while it has no package list for apt.llvm.org.
run_install "recovery ordering" good 0 STUB_DPKG_FAIL=1 STUB_APT_FIX_NOREMOVE_RC=100 --
expect_output "the fallback is not announced" 'falling back to a full apt install'
expect_apt "the repository was not added before the first repair" \
  'repo=yes call=--fix-broken --no-remove --yes install'
expect_apt "the removal-allowing repair did not follow" \
  'repo=yes call=--fix-broken --yes install'
expect_apt "the authoritative install did not run after the repair" \
  'call=install -y --no-remove llvm-22-dev clang-22'
run_install "recovery survives failing repairs" good 0 \
  STUB_DPKG_FAIL=1 STUB_APT_UPDATE_RC=100 STUB_APT_FIX_NOREMOVE_RC=100 STUB_APT_FIX_RC=100 --
expect_apt "a failing repair blocked the authoritative install" \
  'call=install -y --no-remove llvm-22-dev clang-22'

# --- empty resolve: fatal only when an entry would be saved from it ---------
run_install "empty resolve on a miss" empty 1 STUB_APT_RESOLVES=0 --
expect_output "an empty resolve on a miss is not fatal" 'apt resolved no archives into'
run_install "empty resolve while repairing" good 0 STUB_DPKG_FAIL=1 STUB_APT_RESOLVES=0 --
expect_output "an empty repair resolve is treated as a defect" 'already installed'
run_install "empty resolve when uncached" none 0 STUB_APT_RESOLVES=0 -- --uncached
expect_output "an empty uncached resolve is treated as a defect" 'already installed'

# --- uncached mode ----------------------------------------------------------
run_install "uncached leaves no archives" none 0 -- --uncached
[[ -e "$archives_dir" ]] && fail "uncached mode left its archive directory behind"
refute_output "uncached mode claimed to fill a cache entry" 'for the cache'
run_install "cache miss keeps archives" empty 0
[[ -f "$archives_dir/SHA256SUMS" ]] || fail "a cache miss wrote no manifest"
expect_output "a cache miss does not report its archives" 'resolved 2 archives .* for the cache'
# A restored set with .debs but no manifest is a rejected hit, not a miss: it
# must repair rather than write a manifest over an unverified set.
run_install "unmanifested set is not a miss" none 0
refute_output "an unverified restored set was manifested as a cache miss" 'for the cache'

# --- post-condition ---------------------------------------------------------
run_install "unconfigured package" good 1 STUB_PKG_MISSING=1 --
expect_output "an unconfigured package does not fail closed" 'are not fully installed'

# --- repository provenance --------------------------------------------------
# An already-installed llvm-22-dev answers `Candidate:` out of dpkg's status
# file. Accepting that would skip the suite fallback and then fail in apt.
run_install "versioned suite unavailable" none 0 STUB_GOOD_SUITE=llvm-toolchain-noble --
expect_output "the unversioned suite was not tried" 'apt.llvm.org llvm-toolchain-noble offers'
run_install "no suite serves the package" none 1 STUB_GOOD_SUITE=none --
expect_output "a locally installed version was accepted as a candidate" \
  'no llvm-22-dev candidate on apt.llvm.org'
[[ -f "$sources" ]] && fail "a failed repository add left its sources list behind"

# --- signing key ------------------------------------------------------------
run_install "wrong fingerprint" none 1 STUB_GPG_FPR=DEADBEEF --
expect_output "an unpinned key was accepted" 'does not carry 6084F3CF'
run_install "key download failure" none 1 STUB_WGET_FAIL=1 --
expect_output "a failed key download was not reported" 'cannot download the apt.llvm.org signing key'

# --- miss then hit ----------------------------------------------------------
round_trip="$tmp_dir/round-trip"
rm -rf "$round_trip"
mkdir -p "$round_trip/apt-archives-llvm-22"
rm -f "$sources"
: > "$apt_log"
env RUNNER_TEMP="$round_trip" STUB_ARCHIVES="$round_trip/apt-archives-llvm-22" \
  /bin/bash "$script" install >/dev/null 2>&1 || fail "round trip: the miss failed"
: > "$apt_log"
output="$(env RUNNER_TEMP="$round_trip" STUB_ARCHIVES="$round_trip/apt-archives-llvm-22" \
  /bin/bash "$script" install 2>&1)" || fail "round trip: the hit failed"
expect_output "the manifest a miss wrote was not accepted by the next run" \
  'installed the cached LLVM 22 package set \(2 archives\)'
refute_apt "the second run still ran apt-get" '.'

# --- cache key --------------------------------------------------------------
output=""
key_of() {
  env RUNNER_TEMP="$tmp_dir/key" "$@" /bin/bash "$under_test" key
}
db_list="llvm-22-dev clang-22 libclang-rt-22-dev lld-22 libpq-dev libsqlite3-dev libssl-dev zlib1g-dev libzstd-dev make perl"
reversed_list="perl make libzstd-dev zlib1g-dev libssl-dev libsqlite3-dev libpq-dev lld-22 libclang-rt-22-dev clang-22 llvm-22-dev"
lint_list="llvm-22-dev clang-22 libclang-rt-22-dev lld-22 libssl-dev zlib1g-dev libzstd-dev make perl"
db_key="$(key_of ALIGN_APT_PACKAGES="$db_list")"
reversed_key="$(key_of ALIGN_APT_PACKAGES="$reversed_list")"
lint_key="$(key_of ALIGN_APT_PACKAGES="$lint_list")"
# Read the generation out of the script rather than pinning it here, so a
# deliberate bump keeps this assertion meaningful instead of silently failing.
generation="$(sed -n 's/^readonly CACHE_GENERATION=\([A-Za-z0-9]*\)$/\1/p' "$under_test")"
[[ -n "$generation" ]] || fail "scripts/ci-apt-llvm.sh declares no CACHE_GENERATION"
grep -q -- "-${generation:-g?}-" <<< "$db_key" \
  || fail "the cache key does not carry generation $generation: $db_key"
[[ "$db_key" == "$reversed_key" ]] || fail "reordering the package list churned the cache key"
[[ "$db_key" != "$lint_key" ]] || fail "the two job package lists share one cache entry"
# set -f keeps a glob-hostile entry literal instead of expanding it in cwd.
glob_key="$(key_of ALIGN_APT_PACKAGES="llvm-22-dev *")"
glob_digest="$(printf '%s' '* llvm-22-dev ' | sha256sum | cut -c1-16)"
grep -q "$glob_digest" <<< "$glob_key" || fail "a glob in the package list was expanded: $glob_key"

# --- argument handling ------------------------------------------------------
expect_exit() { # $1=label $2=expected exit; rest: arguments
  local label="$1" want="$2"
  shift 2
  local status=0
  env RUNNER_TEMP="$tmp_dir/key" ALIGN_APT_PACKAGES="llvm-22-dev" \
    /bin/bash "$under_test" "$@" >/dev/null 2>&1 || status=$?
  [[ "$status" == "$want" ]] || fail "$label: exit $status, expected $want"
}
expect_exit "no mode" 2
expect_exit "unknown mode" 2 bogus
expect_exit "unknown install flag" 2 install --cached
expect_exit "extra install argument" 2 install --uncached extra
expect_exit "argument after key" 2 key --uncached
env -u ImageVersion RUNNER_TEMP="$tmp_dir/key" ALIGN_APT_PACKAGES="llvm-22-dev" \
  /bin/bash "$under_test" key >/dev/null 2>&1 \
  && fail "a missing ImageVersion produced a cache key"
env RUNNER_TEMP="$tmp_dir/key" ALIGN_APT_PACKAGES="" \
  /bin/bash "$under_test" key >/dev/null 2>&1 \
  && fail "an empty package list produced a cache key"

if [[ "$failures" -ne 0 ]]; then
  echo "scripts/test-apt-llvm.sh: $failures assertion(s) failed" >&2
  exit 1
fi
echo "scripts/test-apt-llvm.sh: all cases passed"
