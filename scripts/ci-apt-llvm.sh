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
# A caller with no cache around it uses `install --uncached` instead. That mode
# never restores, never leaves archives behind, and never applies the
# cache-specific empty-resolve guard: "apt downloaded nothing" is only a defect
# when an entry is about to be saved from it.
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
# repository uses lldb or clangd (the driver shells out to `clang-22` only for
# the PGO profile runtime and the IR comparison test), and lld-N joined the
# list only once it had a real consumer: `alignc` now drives `cc` with
# `-fuse-ld=lld` on ELF when it can resolve an `ld.lld`, so without lld-22 the
# Linux jobs would silently link at the old speed (build-perf track item 2,
# docs/impl/21-build-perf-plan.md). Installing lldb-22, by contrast, dragged in
# python3-lldb-22, which Conflicts with
# the unversioned `python3-lldb-x.y` that the runner image's preinstalled
# python3-lldb-18 provides, so apt satisfied the request by *removing* lldb-18
# and python3-lldb-18. `dpkg --install` can never perform a removal, so the
# archive set apt left behind was not replayable by the very command the cache
# path uses: every hit failed in python3-lldb-22, left lldb-22 unpacked but
# unconfigured, and then poisoned the apt state that the fallback needed. A
# request whose closure conflicts with nothing preinstalled is dpkg-replayable
# by construction, which is what makes the cache sound at all.
#
# That property is enforced, not documented: the authoritative install runs with
# `--no-remove`, so the first request that needs a removal fails the job that
# would have saved the entry instead of saving one no dpkg run can replay.
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
# nightly.yml and release.yml call this same script as `install --uncached`, so
# a fresh snapshot is still exercised every night and release artifacts never
# link one that only a cache still has. Three workflows and four install call
# sites (two ci.yml jobs, nightly, release) therefore resolve one package set
# from one repository definition. The two ci.yml jobs keep separate package
# lists, and so separate entries, because the database job additionally requests the PostgreSQL
# client and SQLite development package used by its provisioned integration suites.
#
# Trust boundary. The manifest check below detects truncation and corruption,
# not a hostile writer: installing archives with dpkg bypasses apt's repository
# signature verification, so integrity rests on GitHub Actions cache scope
# isolation — a run reads only its own branch, its base, and the default
# branch. Re-evaluate that assumption before introducing pull_request_target,
# reusable workflows called from less trusted contexts, or any other path that
# lets a fork populate an entry this workflow restores.
#
# On a clean hit the apt.llvm.org repository definition is never added, so any
# step that needs to apt-install from it must run the full path itself. Every
# other path — a miss, `--uncached`, and the recovery after a restored set
# fails — does add it, and leaves it in place for the rest of the job.
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
  echo "usage: scripts/ci-apt-llvm.sh {key | install [--uncached]}" >&2
  echo "  key                 print the cache path and key for actions/cache" >&2
  echo "  install             install the restored archive set, or resolve it" >&2
  echo "                      through apt and leave the archives for the cache" >&2
  echo "  install --uncached  resolve through apt for a caller with no cache" >&2
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

# Every requested package configured, and the two things the build actually
# resolves through — llvm-config and the C driver alignc links with — present.
# A restored set that fails this is discarded. `cc` is checked because no
# requested package names it: it comes from the runner image, and a repair that
# was allowed to remove packages could take it away without any dpkg-query in
# the loop above noticing.
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
  [[ -x "/usr/lib/llvm-${LLVM_VERSION}/bin/llvm-config" ]] || return 1
  command -v cc >/dev/null 2>&1
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

# Print the llvm-N-dev candidate version, but only when apt would fetch that
# exact version from apt.llvm.org. A bare `Candidate:` is not evidence the
# repository works: an llvm-22-dev already unpacked by a restored set answers it
# out of /var/lib/dpkg/status, which would let a failed repository add look like
# a success and skip the suite fallback. Walks the version table, finds the row
# whose version is the candidate, and requires one of that row's origin lines to
# name apt.llvm.org. Prints nothing when it does not.
llvm_org_candidate() {
  apt-cache policy "llvm-${LLVM_VERSION}-dev" 2>/dev/null | awk '
    /^ *Candidate: / { candidate = $2; next }
    /^ *Version table:/ { in_table = 1; next }
    !in_table { next }
    {
      # A version row is "<version> <priority>", or "*** <version> <priority>"
      # for the installed one. An origin row is "<priority> <uri-or-path> ...",
      # whose second field is never a bare number.
      version = ""
      if ($1 == "***") { version = $2 }
      else if ($2 ~ /^[0-9]+$/) { version = $1 }
      if (version != "") { current = (version == candidate); next }
      if (current && index($0, "apt.llvm.org") > 0) { print candidate; exit }
    }'
}

# The minimum llvm.sh did that this script actually needs: the signing key and
# one sources.list entry. Idempotent, because the recovery path adds the
# repository before the authoritative install also asks for it.
add_llvm_repository() {
  [[ "$llvm_repository_ready" -eq 1 ]] && return 0

  local codename architecture tool fingerprints suite candidate
  # Every capture below tolerates its own failure so the explicit check, not
  # `set -e`/`pipefail` on the assignment, reports what actually went wrong.
  codename="$(. /etc/os-release 2>/dev/null && printf '%s' "${VERSION_CODENAME:-}")" \
    || codename=""
  if [[ -z "$codename" ]]; then
    echo "/etc/os-release declares no VERSION_CODENAME; cannot pick an apt.llvm.org suite" >&2
    return 1
  fi
  architecture="$(dpkg --print-architecture)" || return 1
  for tool in gpg wget; do
    command -v "$tool" >/dev/null 2>&1 || {
      echo "$tool is required to add the apt.llvm.org repository" >&2
      return 1
    }
  done

  [[ -n "$workdir" ]] || workdir="$(mktemp -d)" || return 1
  wget -q "$LLVM_KEY_URL" -O "$workdir/llvm-snapshot.asc" || {
    echo "cannot download the apt.llvm.org signing key from $LLVM_KEY_URL" >&2
    return 1
  }
  # Any fpr record, not just the first: the file is a key block, and upstream
  # may publish a rotation alongside the current key or reorder the two. Still
  # fail closed — the pinned fingerprint must appear somewhere in it.
  fingerprints="$(gpg --show-keys --with-colons "$workdir/llvm-snapshot.asc" 2>/dev/null \
    | awk -F: '$1 == "fpr" { print $10 }')" || fingerprints=""
  if ! printf '%s\n' "$fingerprints" | grep -qxF "$LLVM_KEY_FINGERPRINT"; then
    echo "apt.llvm.org signing key does not carry $LLVM_KEY_FINGERPRINT" >&2
    echo "  fingerprints offered: ${fingerprints:-none}" >&2
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
    candidate="$(llvm_org_candidate)" || candidate=""
    if [[ -n "$candidate" ]]; then
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
  # removal: one way a restored set breaks is dpkg being unable to remove a
  # conflicting package that apt would have, and refusing the removal outright
  # leaves no repair at all. That second stage is a general safety net rather
  # than a live path — retiring the g1 entries removed the only known set that
  # needed it, and the `--no-remove` on the authoritative install keeps a new
  # one from being saved. The authoritative install reinstates every requested
  # package and toolchain_complete, which now also checks `cc`, fails closed if
  # a removal took something the build needs.
  sudo DEBIAN_FRONTEND=noninteractive apt-get --fix-broken --no-remove --yes install \
    || sudo DEBIAN_FRONTEND=noninteractive apt-get --fix-broken --yes install \
    || true
}

# $1 is why this run is resolving through apt:
#   cache-miss  an entry will be saved from the result, so an empty resolve is
#               a defect and the manifest has to be written
#   repair      a restored set failed; most packages are already unpacked, so
#               downloading nothing is correct and no entry is saved
#   uncached    the caller keeps no cache at all; same as repair, and the
#               archives are discarded by install_packages afterwards
install_from_apt() {
  local purpose="$1"
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
  # --no-remove turns "this request is dpkg-replayable" from a claim into a
  # gate. An apt transaction that removes a package cannot be replayed by
  # `dpkg --install` on a later hit, which is exactly how the g1 entries broke;
  # failing here means main never saves such an entry in the first place. It is
  # unconditional so nightly, the canary, hits it before any cached job does.
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-remove "$@"

  sudo rm -f "$APT_CONF"
  apt_conf_written=0
  sudo rm -rf "$archives/partial" "$archives/lock"
  sudo chown -R "$(id -u):$(id -g)" "$archives"

  local resolved=()
  shopt -s nullglob
  resolved=("$archives"/*.deb)
  shopt -u nullglob
  if [[ ${#resolved[@]} -eq 0 ]]; then
    if [[ "$purpose" == cache-miss ]]; then
      echo "apt resolved no archives into $archives, so the cache entry would be" >&2
      echo "empty and every later run would silently take the full install." >&2
      echo "If the runner image now ships the toolchain, drop the cache instead." >&2
      exit 1
    fi
    # No entry is saved from a repair or an uncached run, so downloading
    # nothing means the packages are already installed — the desired end state,
    # not a defect, and there is nothing to write a manifest over.
    echo "apt resolved no archives; every requested package was already installed"
    return 0
  fi
  if [[ "$purpose" == cache-miss ]]; then
    ( cd "$archives" \
      && sha256sum ./*.deb > SHA256SUMS.partial \
      && mv SHA256SUMS.partial SHA256SUMS )
    echo "resolved ${#resolved[@]} archives ($(du -sh "$archives" | cut -f1)) for the cache"
  else
    echo "resolved ${#resolved[@]} archives ($(du -sh "$archives" | cut -f1))"
  fi
}

install_packages() {
  local restored=()

  if [[ "$uncached" -eq 1 ]]; then
    install_from_apt uncached
    # Nothing will ever read these again, and release.yml already carries two
    # Cargo target directories on the same runner disk.
    sudo rm -rf "$archives"
    toolchain_complete || {
      echo "LLVM ${LLVM_VERSION} and $packages are not fully installed" >&2
      exit 1
    }
    return 0
  fi

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
    install_from_apt repair
  else
    install_from_apt cache-miss
  fi

  toolchain_complete || {
    echo "LLVM ${LLVM_VERSION} and $packages are not fully installed" >&2
    exit 1
  }
}

mode="${1:-}"
uncached=0
case "$mode" in
  key)
    [[ $# -eq 1 ]] || { usage; exit 2; }
    ;;
  install)
    case "${2:-}" in
      "") ;;
      --uncached) uncached=1 ;;
      *) usage; exit 2 ;;
    esac
    [[ $# -le 2 ]] || { usage; exit 2; }
    ;;
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
