#!/usr/bin/env bash
# Install the CI LLVM 22 toolchain and native libraries from a cached .deb set.
#
# The set of archives apt resolves is a pure function of the runner image, the
# LLVM major version, and the requested package list, so the caller wraps this
# script in one actions/cache entry keyed by exactly that triple:
#
#   ALIGN_APT_PACKAGES="llvm-22-dev clang-22 ..." scripts/ci-apt-llvm.sh key
#   <actions/cache with the printed path and key>
#   ALIGN_APT_PACKAGES="llvm-22-dev clang-22 ..." scripts/ci-apt-llvm.sh install
#
# `install` unpacks the restored archives with dpkg, which reads no package
# lists at all, and otherwise performs the identical full apt install and
# leaves the resolved archives behind for the cache to save. Measured on the
# ubuntu-24.04 runner the full path costs about 23 s, of which only about 3 s
# is the 160 MB download: `apt-get update` plus two package-list resolutions
# dominate, and the cached path skips all of them.
#
# A cache hit pins the apt.llvm.org snapshot for the life of the runner image
# (roughly a week). That is strictly more stable than the uncached path, which
# installs whatever nightly snapshot apt.llvm.org happens to serve that hour.
# nightly.yml and release.yml deliberately keep the uncached install so a fresh
# snapshot is still exercised every night and release artifacts never link one
# that only a cache still has.
set -euo pipefail

readonly LLVM_VERSION=22
readonly APT_CONF=/etc/apt/apt.conf.d/99-align-archives

usage() {
  echo "usage: scripts/ci-apt-llvm.sh {key|install}" >&2
  echo "  ALIGN_APT_PACKAGES must list the packages to install." >&2
}

command -v apt-get >/dev/null 2>&1 || {
  echo "scripts/ci-apt-llvm.sh targets the Debian-family CI runners" >&2
  exit 2
}

packages="${ALIGN_APT_PACKAGES:-}"
[[ -n "$packages" ]] || { usage; exit 2; }

archives="${RUNNER_TEMP:?RUNNER_TEMP must be set}/apt-archives-llvm-${LLVM_VERSION}"

# Sorted so that reordering the caller's list does not churn the cache entry.
cache_key() {
  local normalized digest
  # shellcheck disable=SC2086 # the package list is deliberately word-split.
  normalized="$(printf '%s\n' $packages | LC_ALL=C sort | tr '\n' ' ')"
  digest="$(printf '%s' "$normalized" | sha256sum | cut -c1-16)"
  # ImageVersion is the dependency baseline the archive set was resolved
  # against; GitHub-hosted runners always export it.
  printf 'apt-llvm%s-%s-%s-image%s-%s\n' \
    "$LLVM_VERSION" "${RUNNER_OS:-Linux}" "${RUNNER_ARCH:-X64}" \
    "${ImageVersion:-unknown}" "$digest"
}

# Every requested package configured, and the toolchain the build actually
# resolves through, present. A restored set that fails this is discarded.
toolchain_complete() {
  local package status
  # shellcheck disable=SC2086 # the package list is deliberately word-split.
  for package in $packages; do
    status="$(dpkg-query --show --showformat='${db:Status-Status}' "$package" 2>/dev/null || true)"
    [[ "$status" == "installed" ]] || return 1
  done
  [[ -x "/usr/lib/llvm-${LLVM_VERSION}/bin/llvm-config" ]]
}

install_from_apt() {
  sudo rm -rf "$archives"
  sudo install -d -m 0755 "$archives"
  # apt drops privileges to _apt while fetching, so hand it a writable
  # partial/ directory instead of letting it fall back to an unsandboxed root
  # download.
  sudo install -d -m 0700 -o _apt -g root "$archives/partial"
  printf 'Dir::Cache::archives "%s";\n' "$archives" | sudo tee "$APT_CONF" >/dev/null

  local workdir
  workdir="$(mktemp -d)"
  # llvm.sh adds the apt.llvm.org repository, refreshes the package lists, and
  # installs the base clang/lld/lldb set for the requested major version.
  wget -q https://apt.llvm.org/llvm.sh -O "$workdir/llvm.sh"
  chmod +x "$workdir/llvm.sh"
  sudo "$workdir/llvm.sh" "$LLVM_VERSION"
  # shellcheck disable=SC2086 # the package list is deliberately word-split.
  sudo apt-get install -y $packages
  rm -rf "$workdir"

  sudo rm -f "$APT_CONF"
  sudo rm -rf "$archives/partial" "$archives/lock"
  sudo chown -R "$(id -u):$(id -g)" "$archives"
}

install_packages() {
  local restored=()
  if [[ -d "$archives" ]]; then
    shopt -s nullglob
    restored=("$archives"/*.deb)
    shopt -u nullglob
  fi

  if [[ ${#restored[@]} -gt 0 ]] && sudo dpkg --install "${restored[@]}" && toolchain_complete; then
    echo "installed the cached LLVM ${LLVM_VERSION} package set (${#restored[@]} archives)"
  else
    if [[ ${#restored[@]} -gt 0 ]]; then
      echo "the cached package set is unusable; falling back to a full apt install" >&2
      # dpkg may have stopped part way through; leave apt consistent before it
      # resolves anything itself.
      sudo dpkg --configure --pending || true
      sudo apt-get update
      sudo apt-get --fix-broken --yes install
    fi
    install_from_apt
  fi

  toolchain_complete || {
    echo "LLVM ${LLVM_VERSION} and $packages are not fully installed" >&2
    exit 1
  }
}

case "${1:-}" in
  key)
    printf 'path=%s\n' "$archives"
    printf 'key=%s\n' "$(cache_key)"
    ;;
  install)
    install_packages
    ;;
  *)
    usage
    exit 2
    ;;
esac
