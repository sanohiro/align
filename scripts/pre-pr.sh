#!/usr/bin/env bash
# Produce a HEAD-bound preflight stamp. A draft PR cannot be opened through the
# repository wrapper without this stamp.
set -euo pipefail

usage() {
  echo "usage: scripts/pre-pr.sh --reviewer ID [--base REF] [--owner-test LABEL] [-- COMMAND ...]" >&2
  exit 2
}

base="origin/main"
reviewer=""
owner_test="none"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --base)
      [[ $# -ge 2 ]] || usage
      base="$2"
      shift 2
      ;;
    --reviewer)
      [[ $# -ge 2 ]] || usage
      reviewer="$2"
      shift 2
      ;;
    --owner-test)
      [[ $# -ge 2 ]] || usage
      owner_test="$2"
      shift 2
      ;;
    --)
      shift
      break
      ;;
    *)
      usage
      ;;
  esac
done
[[ -n "$reviewer" ]] || {
  echo "--reviewer must identify the fresh adversarial preflight reviewer" >&2
  exit 2
}
[[ "$reviewer" =~ ^[A-Za-z0-9._-]+$ ]] || {
  echo "--reviewer must contain only letters, digits, dot, underscore, or hyphen" >&2
  exit 2
}

git rev-parse --is-inside-work-tree >/dev/null
base_sha="$(git rev-parse --verify "${base}^{commit}")"
head_sha="$(git rev-parse HEAD)"
branch="$(git branch --show-current)"
[[ -n "$branch" ]] || {
  echo "preflight requires a named branch" >&2
  exit 1
}
if [[ "$branch" == "main" ]]; then
  echo "preflight must not run on main" >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "preflight requires a clean worktree; commit the coherent implementation first" >&2
  exit 1
fi
if git diff --quiet "$base_sha"...HEAD; then
  echo "preflight found no change against $base" >&2
  exit 1
fi

git diff --check "$base_sha"...HEAD

rust_changed=false
shell_changed=false
while IFS= read -r path; do
  case "$path" in
    crates/*|Cargo.toml|Cargo.lock|rust-toolchain*|.cargo/*)
      rust_changed=true
      ;;
  esac
  case "$path" in
    *.sh)
      if [[ -f "$path" ]]; then
        bash -n "$path"
      fi
      shell_changed=true
      ;;
  esac
done < <(git diff --name-only "$base_sha"...HEAD)

if [[ "$rust_changed" == true ]]; then
  scripts/test-pr.sh
  cargo clippy --workspace --all-targets --locked -- -D warnings
fi
if [[ $# -gt 0 ]]; then
  "$@"
elif [[ "$shell_changed" == true ]]; then
  echo "shell changes require an owner test command after --" >&2
  exit 1
fi

if [[ "$(git rev-parse HEAD)" != "$head_sha" ]] || [[ -n "$(git status --porcelain)" ]]; then
  echo "HEAD or the worktree changed during preflight; rerun it on the final commit" >&2
  exit 1
fi

stamp_dir="$(git rev-parse --git-path align-preflight)"
mkdir -p "$stamp_dir"
stamp="$stamp_dir/$head_sha"
{
  printf 'version=1\n'
  printf 'head=%s\n' "$head_sha"
  printf 'base_ref=%s\n' "$base"
  printf 'base_sha=%s\n' "$base_sha"
  printf 'reviewer=%s\n' "$reviewer"
  printf 'owner_test=%s\n' "$owner_test"
  printf 'created_at=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
} >"$stamp"

echo "preflight recorded for $head_sha"
echo "reviewer: $reviewer"
echo "owner test: $owner_test"
