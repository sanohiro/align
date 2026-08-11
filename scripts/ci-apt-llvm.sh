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
# Only ALIGN_APT_PACKAGES and their dependency closure are installed. This is
# load bearing, not tidiness. The script used to run apt.llvm.org's llvm.sh,
# whose built-in list is `clang-N lldb-N lld-N clangd-N`; nothing in this
# repository uses lldb, lld, or clangd (the driver links with `cc` and shells
# out to `clang-22` only for the PGO profile runtime and the IR comparison
# test). Installing lldb-22 dragged in python3-lldb-22, which Conflicts with
# the unversioned `python3-lldb-x.y` that the runner image's preinstalled
# python3-lldb-18 provides, so apt satisfied the request by *removing* lldb-18
# and python3-lldb-18. `dpkg --install` can never perform a removal, so the
# archive set apt left behind was not replayable by the very command the cache
# path uses: every hit failed in python3-lldb-22, left lldb-22 unpacked but
# unconfigured, and then poisoned the apt state that the fallback needed. A
# request whose closure conflicts with nothing preinstalled is dpkg-replayable
# by construction, which is what makes the cache sound at all.
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
# change. Bump CACHE_GENERATION below to escape a bad entry without waiting for
# the runner image to roll.
#
# nightly.yml and release.yml call this same script with no cache around it, so
# a fresh snapshot is still exercised every night and release artifacts never
# link one that only a cache still has, while all four workflows resolve one
# package set from one repository definition. The two ci.yml jobs keep separate
# package lists, and so separate entries, because giving the lint job libpq-dev
# and libsqlite3-dev to share one entry would change what its build detects.
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
readonly LLVM_KEYRING=/etc/apt/keyrings/apt-llvm-org.asc
readonly LLVM_SOURCES=/etc/apt/sources.list.d/align-apt-llvm-org.list
readonly LLVM_KEY_URL=https://apt.llvm.org/llvm-snapshot.gpg.key
# "Sylvestre Ledru - Debian LLVM packages", the apt.llvm.org archive signing
# key. Pinned because this script adds the repository itself: fetching a key
# over TLS and trusting whatever arrives is the weakest link in the chain.
readonly LLVM_KEY_FINGERPRINT=6084F3CF814B57C1CF12EFD515CF4D18AF4F7421
# Manual escape hatch: bump to invalidate every entry (see the header).
# g1 entries hold the llvm.sh package set, which no dpkg run can replay.
readonly CACHE_GENERATION=g2

usage() {
  echo "usage: scripts/ci-apt-llvm.sh {key|install}" >&2
  echo "  key      print the cache path and key for actions/cache" >&2
  echo "  install  install the cached archive set, or resolve it through apt" >&2
  echo "  ALIGN_APT_PACKAGES must list the packages to install." >&2
}

apt_conf_written=0
workdir=""
llvm_repository_ready=0

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

# The minimum llvm.sh did that this script actually needs: the signing key and
# one sources.list entry. Idempotent, because the recovery path adds the
# repository before the authoritative install also asks for it.
add_llvm_repository() {
  [[ "$llvm_repository_ready" -eq 1 ]] && return 0

  local codename architecture fingerprint suite candidate
  # Every capture below tolerates its own failure so the explicit check, not
  # `set -e`/`pipefail` on the assignment, reports what actually went wrong.
  codename="$(. /etc/os-release 2>/dev/null && printf '%s' "${VERSION_CODENAME:-}")" \
    || codename=""
  if [[ -z "$codename" ]]; then
    echo "/etc/os-release declares no VERSION_CODENAME; cannot pick an apt.llvm.org suite" >&2
    return 1
  fi
  architecture="$(dpkg --print-architecture)" || return 1
  command -v gpg >/dev/null 2>&1 || {
    echo "gpg is required to verify the apt.llvm.org signing key" >&2
    return 1
  }

  [[ -n "$workdir" ]] || workdir="$(mktemp -d)" || return 1
  wget -q "$LLVM_KEY_URL" -O "$workdir/llvm-snapshot.asc" || {
    echo "cannot download the apt.llvm.org signing key from $LLVM_KEY_URL" >&2
    return 1
  }
  fingerprint="$(gpg --show-keys --with-colons "$workdir/llvm-snapshot.asc" 2>/dev/null \
    | awk -F: '$1 == "fpr" { print $10; exit }')" || fingerprint=""
  if [[ "$fingerprint" != "$LLVM_KEY_FINGERPRINT" ]]; then
    echo "apt.llvm.org signing key is ${fingerprint:-unreadable}, expected $LLVM_KEY_FINGERPRINT" >&2
    return 1
  fi
  sudo install -d -m 0755 /etc/apt/keyrings || return 1
  sudo install -m 0644 "$workdir/llvm-snapshot.asc" "$LLVM_KEYRING" || return 1

  # apt.llvm.org names a released branch <codename>-<major> and the
  # in-development branch plain <codename>. Try the versioned suite first and
  # fall back, so the major moving to trunk cannot silently leave no candidate.
  for suite in "llvm-toolchain-${codename}-${LLVM_VERSION}" "llvm-toolchain-${codename}"; do
    printf 'deb [arch=%s signed-by=%s] https://apt.llvm.org/%s/ %s main\n' \
      "$architecture" "$LLVM_KEYRING" "$codename" "$suite" \
      | sudo tee "$LLVM_SOURCES" >/dev/null
    # A suite that does not exist 404s and fails the whole update; the
    # candidate probe below, not the exit status, decides whether it worked.
    sudo DEBIAN_FRONTEND=noninteractive apt-get update || true
    candidate="$(apt-cache policy "llvm-${LLVM_VERSION}-dev" 2>/dev/null \
      | awk '$1 == "Candidate:" { print $2; exit }')" || candidate=""
    if [[ -n "$candidate" && "$candidate" != "(none)" ]]; then
      echo "apt.llvm.org $suite offers llvm-${LLVM_VERSION}-dev $candidate"
      llvm_repository_ready=1
      return 0
    fi
  done

  sudo rm -f "$LLVM_SOURCES"
  echo "no llvm-${LLVM_VERSION}-dev candidate on apt.llvm.org for $codename/$architecture" >&2
  return 1
}

# Best-effort repair after a restored set failed to install. install_from_apt
# is what has to succeed, so nothing here may take the job down before it runs.
repair_dpkg_state() {
  sudo dpkg --configure --pending || true
  # Ordering is the whole point: apt cannot resolve an apt.llvm.org dependency
  # while it has no package list for apt.llvm.org, so the repository goes in
  # (with its own apt-get update) before the repair, not after.
  add_llvm_repository || true
  # --no-remove first, so a repair cannot solve a conflict by deleting a
  # library the build then fails to link. If that cannot converge, allow the
  # removal: a restored set breaks precisely when dpkg could not remove a
  # conflicting package that apt would have, and refusing the removal outright
  # leaves no repair at all. The authoritative install below reinstates every
  # requested package and toolchain_complete fails closed if it does not.
  sudo DEBIAN_FRONTEND=noninteractive apt-get --fix-broken --no-remove --yes install \
    || sudo DEBIAN_FRONTEND=noninteractive apt-get --fix-broken --yes install \
    || true
}

# $1: 1 when an empty resolve must fail the job (this run started from a cache
# miss, so its result is what gets saved). After a failed restore most packages
# are already unpacked, so apt legitimately downloads nothing and the entry is
# not saved anyway.
install_from_apt() {
  local strict="$1"
  sudo rm -rf "$archives"
  sudo install -d -m 0755 "$archives"
  # apt drops privileges to _apt while fetching, so hand it a writable
  # partial/ directory instead of letting it fall back to an unsandboxed root
  # download.
  sudo install -d -m 0700 -o _apt -g root "$archives/partial"
  printf 'Dir::Cache::archives "%s";\n' "$archives" | sudo tee "$APT_CONF" >/dev/null
  apt_conf_written=1

  add_llvm_repository || {
    echo "cannot add the apt.llvm.org repository for LLVM $LLVM_VERSION" >&2
    exit 1
  }
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
    if [[ "$strict" -eq 1 ]]; then
      echo "apt resolved no archives into $archives, so the cache entry would be" >&2
      echo "empty and every later run would silently take the full install." >&2
      echo "If the runner image now ships the toolchain, drop the cache instead." >&2
      exit 1
    fi
    # Repair path: the restored set already put most packages on disk, so
    # downloading nothing is the correct outcome. The caller restored a hit, so
    # no entry is saved from this run and there is nothing to manifest.
    echo "apt resolved no archives; the repaired install needed no download"
    return 0
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
  elif [[ ${#restored[@]} -gt 0 ]]; then
    echo "the cached package set is unusable; falling back to a full apt install" >&2
    repair_dpkg_state
    install_from_apt 0
  else
    install_from_apt 1
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
