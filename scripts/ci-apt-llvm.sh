#!/usr/bin/env bash
# Install the CI LLVM 22 toolchain and native libraries from a cached .deb set.
#
# The caller wraps this script in one actions/cache entry:
#
#   ALIGN_APT_PACKAGES="llvm-22-dev clang-22 ..." scripts/ci-apt-llvm.sh key
#   <actions/cache/restore with the printed path and key>
#   ALIGN_APT_PACKAGES="llvm-22-dev clang-22 ..." scripts/ci-apt-llvm.sh install
#   <actions/cache/save, main only>
#
# `install` unpacks the restored archives with dpkg, which reads no package
# lists at all, and otherwise performs the identical full apt install and
# leaves the resolved archives behind for the cache to save. Measured on the
# ubuntu-24.04 runner the full path costs about 23 s, of which only about 3 s
# is the 160 MB download: `apt-get update` plus two package-list resolutions
# dominate, and the cached path skips all of them.
#
# What the key does and does not identify. It identifies the *request* — the
# runner image, the LLVM major version, and the sorted package list — plus a
# manual generation counter. It does not identify the resolved bytes:
# apt.llvm.org serves a moving nightly snapshot, so the archive contents are a
# function of when the miss happened, not of anything in the key. A hit
# therefore pins one snapshot for the life of the runner image (roughly a
# week), which is more stable than the uncached path's "whatever the mirror
# serves this hour" but makes the pinned version worth logging: the Verify
# toolchain and Print database evidence steps print the exact llvm-22-dev
# version so an x86_64/ARM64 difference is never misattributed to a code
# change. Bump ALIGN_APT_CACHE_GENERATION to escape a bad entry without
# waiting for the runner image to roll.
#
# nightly.yml and release.yml deliberately keep the uncached install so a fresh
# snapshot is still exercised every night and release artifacts never link one
# that only a cache still has. The two ci.yml jobs keep separate package lists,
# and so separate entries, because giving the lint job libpq-dev and
# libsqlite3-dev to share one entry would change what its build detects.
#
# Trust boundary. The manifest check below detects truncation and corruption,
# not a hostile writer: installing archives with dpkg bypasses apt's repository
# signature verification, so integrity rests on GitHub Actions cache scope
# isolation — a run reads only its own branch, its base, and the default
# branch. Re-evaluate that assumption before introducing pull_request_target,
# reusable workflows called from less trusted contexts, or any other path that
# lets a fork populate an entry this workflow restores.
#
# On a hit the apt.llvm.org repository definition is never added, so any step
# that needs to apt-install from it must run the full path itself.
set -euo pipefail

readonly LLVM_VERSION=22
readonly APT_CONF=/etc/apt/apt.conf.d/99-align-archives
# Manual escape hatch: bump to invalidate every entry (see the header).
readonly CACHE_GENERATION=g1

usage() {
  echo "usage: scripts/ci-apt-llvm.sh {key|install}" >&2
  echo "  key      print the cache path and key for actions/cache" >&2
  echo "  install  install the cached archive set, or resolve it through apt" >&2
  echo "  ALIGN_APT_PACKAGES must list the packages to install." >&2
}

apt_conf_written=0
workdir=""

cleanup() {
  [[ "$apt_conf_written" -eq 1 ]] && sudo rm -f "$APT_CONF"
  [[ -n "$workdir" ]] && rm -rf "$workdir"
  return 0
}

# Sorted so that reordering the caller's list does not churn the cache entry.
cache_key() {
  local normalized digest
  set -f
  # shellcheck disable=SC2086 # the package list is deliberately word-split.
  normalized="$(printf '%s\n' $packages | LC_ALL=C sort | tr '\n' ' ')"
  set +f
  digest="$(printf '%s' "$normalized" | sha256sum | cut -c1-16)"
  # ImageVersion is the dependency baseline the archive set was resolved
  # against. Fail closed rather than key every image to one bucket.
  printf 'apt-llvm%s-%s-%s-%s-image%s-%s\n' \
    "$LLVM_VERSION" "${RUNNER_OS:-Linux}" "${RUNNER_ARCH:-X64}" "$CACHE_GENERATION" \
    "${ImageVersion:?ImageVersion must be set; it pins the dependency baseline}" \
    "$digest"
}

# Every requested package configured, and the toolchain the build actually
# resolves through, present. A restored set that fails this is discarded.
toolchain_complete() {
  local package status
  set -f
  # shellcheck disable=SC2086 # the package list is deliberately word-split.
  set -- $packages
  set +f
  for package in "$@"; do
    status="$(dpkg-query --show --showformat='${db:Status-Status}' "$package" 2>/dev/null || true)"
    [[ "$status" == "installed" ]] || return 1
  done
  [[ -x "/usr/lib/llvm-${LLVM_VERSION}/bin/llvm-config" ]]
}

# Truncation and corruption check over the restored set. See the header for
# what this does not defend against.
verify_archives() {
  local expected="$1" listed
  if [[ ! -f "$archives/SHA256SUMS" ]]; then
    echo "the restored package set has no SHA256SUMS manifest" >&2
    return 1
  fi
  listed="$(grep -c '\.deb$' "$archives/SHA256SUMS" || true)"
  if [[ "$listed" != "$expected" ]]; then
    echo "the restored package set holds $expected archives but lists $listed" >&2
    return 1
  fi
  ( cd "$archives" && LC_ALL=C sha256sum --check --quiet --strict SHA256SUMS )
}

install_from_apt() {
  sudo rm -rf "$archives"
  sudo install -d -m 0755 "$archives"
  # apt drops privileges to _apt while fetching, so hand it a writable
  # partial/ directory instead of letting it fall back to an unsandboxed root
  # download.
  sudo install -d -m 0700 -o _apt -g root "$archives/partial"
  printf 'Dir::Cache::archives "%s";\n' "$archives" | sudo tee "$APT_CONF" >/dev/null
  apt_conf_written=1

  workdir="$(mktemp -d)"
  # llvm.sh adds the apt.llvm.org repository, refreshes the package lists, and
  # installs the base clang/lld/lldb set for the requested major version.
  wget -q https://apt.llvm.org/llvm.sh -O "$workdir/llvm.sh"
  chmod +x "$workdir/llvm.sh"
  sudo DEBIAN_FRONTEND=noninteractive "$workdir/llvm.sh" "$LLVM_VERSION"
  set -f
  # shellcheck disable=SC2086 # the package list is deliberately word-split.
  set -- $packages
  set +f
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y "$@"

  sudo rm -f "$APT_CONF"
  apt_conf_written=0
  sudo rm -rf "$archives/partial" "$archives/lock"
  sudo chown -R "$(id -u):$(id -g)" "$archives"

  local resolved=()
  shopt -s nullglob
  resolved=("$archives"/*.deb)
  shopt -u nullglob
  if [[ ${#resolved[@]} -eq 0 ]]; then
    echo "apt resolved no archives into $archives, so the cache entry would be" >&2
    echo "empty and every later run would silently take the full install." >&2
    echo "If the runner image now ships the toolchain, drop the cache instead." >&2
    exit 1
  fi
  ( cd "$archives" \
    && sha256sum ./*.deb > SHA256SUMS.partial \
    && mv SHA256SUMS.partial SHA256SUMS )
  echo "resolved ${#resolved[@]} archives ($(du -sh "$archives" | cut -f1)) for the cache"
}

install_packages() {
  local restored=()
  if [[ -d "$archives" ]]; then
    shopt -s nullglob
    restored=("$archives"/*.deb)
    shopt -u nullglob
  fi

  if [[ ${#restored[@]} -gt 0 ]] \
    && verify_archives "${#restored[@]}" \
    && sudo DEBIAN_FRONTEND=noninteractive dpkg --install \
      --force-confold --force-confdef "${restored[@]}" \
    && toolchain_complete
  then
    echo "installed the cached LLVM ${LLVM_VERSION} package set (${#restored[@]} archives)"
  else
    if [[ ${#restored[@]} -gt 0 ]]; then
      echo "the cached package set is unusable; falling back to a full apt install" >&2
      # Best-effort repair only: dpkg may have stopped part way through, but
      # install_from_apt below is what has to succeed, so a failure here must
      # not take the job down before it runs. --no-remove keeps a repair from
      # solving a conflict by deleting a library the build then fails to link.
      sudo dpkg --configure --pending || true
      sudo DEBIAN_FRONTEND=noninteractive apt-get update || true
      sudo DEBIAN_FRONTEND=noninteractive apt-get --fix-broken --no-remove --yes install || true
    fi
    install_from_apt
  fi

  toolchain_complete || {
    echo "LLVM ${LLVM_VERSION} and $packages are not fully installed" >&2
    exit 1
  }
}

mode="${1:-}"
case "$mode" in
  key | install) ;;
  *)
    usage
    exit 2
    ;;
esac

packages="${ALIGN_APT_PACKAGES:-}"
if [[ -z "$packages" ]]; then
  echo "ALIGN_APT_PACKAGES is empty" >&2
  usage
  exit 2
fi
archives="${RUNNER_TEMP:?RUNNER_TEMP must be set}/apt-archives-llvm-${LLVM_VERSION}"

if [[ "$mode" == "key" ]]; then
  # Assign before printing: inside printf's argument a failed expansion would
  # be swallowed and emit an empty key, which caches the wrong thing forever.
  key="$(cache_key)"
  if [[ -z "$key" ]]; then
    echo "refusing to emit an empty cache key" >&2
    exit 1
  fi
  printf 'path=%s\n' "$archives"
  printf 'key=%s\n' "$key"
  exit 0
fi

command -v apt-get >/dev/null 2>&1 || {
  echo "scripts/ci-apt-llvm.sh install targets the Debian-family CI runners" >&2
  exit 2
}
trap cleanup EXIT
install_packages
