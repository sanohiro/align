#!/usr/bin/env bash
# Decide whether a committed diff needs the provisioned PostgreSQL integration job.
#
# The caller supplies exact base and head commits. This script prints GitHub
# Actions output assignments, but they are also convenient to assert locally:
#
#   required=true|false
#   reason=<single-line explanation>
#
# Classification fails closed for an unreadable range. Dependency or DB-gate
# machinery and pkg.db surfaces/owners require the provisioned job by path.
# Other compiler sources qualify only when a changed zero-context hunk (including
# its function header) names the database boundary. That keeps an unrelated edit
# in a monolithic source file from inheriting an unchanged DB marker elsewhere in
# the file. Deletions use the same diff, so removed markers remain visible.
set -euo pipefail

cd "${DB_CI_REPO_ROOT:-$(dirname "$0")/..}"

base_sha="${1:-}"
head_sha="${2:-}"

emit() {
  printf 'required=%s\nreason=%s\n' "$1" "$2"
}

if [ -z "$base_sha" ] || [ -z "$head_sha" ] ||
   ! git cat-file -e "$base_sha^{commit}" 2>/dev/null ||
   ! git cat-file -e "$head_sha^{commit}" 2>/dev/null ||
   ! git diff --no-renames --name-only "$base_sha..$head_sha" >/dev/null 2>&1; then
  emit true uncomputable-diff
  exit 0
fi

source_change_names_database_boundary() {
  local path="$1" changed
  if ! changed="$(git diff --no-ext-diff --no-textconv --no-renames --unified=0 \
    "$base_sha..$head_sha" -- "$path" 2>/dev/null)"; then
    return 0
  fi
  printf '%s\n' "$changed" |
    grep -Ei 'pkg[._]db|postgres|libpq|sqlite|align_pkg_db' >/dev/null
}

# A release version is not a dependency update. Prove the entire committed
# metadata transition; unknown layouts or unavailable tooling retain the gate.
# Keep this verifier inline: CI extracts only this trusted-base script. Isolated
# Python must not import a checkout/PYTHONPATH-owned tomllib or site module.
release_metadata_only() {
  python3 -I - "$base_sha" "$head_sha" <<'PY' >/dev/null 2>&1
import copy
import re
import subprocess
import sys
import tomllib


def git(*args):
    return subprocess.check_output(["git", *args], stderr=subprocess.DEVNULL)


def typed(value):
    # TOML distinguishes booleans, integers and floats; Python equality does not.
    if isinstance(value, dict):
        return (dict, tuple(sorted((key, typed(item)) for key, item in value.items())))
    if isinstance(value, list):
        return (list, tuple(typed(item) for item in value))
    return (type(value), value)


def tree(ref, path):
    rows = git("ls-tree", "-z", f"{ref}:{path}").split(b"\0")
    result = {}
    for row in filter(None, rows):
        metadata, name = row.split(b"\t", 1)
        mode, kind, oid = metadata.split()
        name = name.decode("utf-8")
        if name in result:
            raise ValueError("duplicate tree entry")
        result[name] = (mode, kind, oid)
    return result


def blob(ref, path):
    parent, _, name = path.rpartition("/")
    entry = tree(ref, parent)[name]
    if entry[0] not in (b"100644", b"100755") or entry[1] != b"blob":
        raise ValueError("non-regular metadata")
    return entry[0], git("cat-file", "blob", entry[2].decode("ascii"))


def version_consumers(ref):
    expected = {
        b"crates/align_driver/src/main.rs": b'println!("alignc {}", env!("CARGO_PKG_VERSION"));',
        b"crates/align_repl/src/main.rs": b'println!("align-repl {}", env!("CARGO_PKG_VERSION"));',
        b"crates/align_driver/tests/version.rs": b'format!("alignc {}\\n", env!("CARGO_PKG_VERSION"))',
        b"crates/align_driver/src/cache.rs": b'format!("alignc-build-id-fallback-{}", env!("CARGO_PKG_VERSION")).as_bytes(),',
    }
    rows = git("grep", "-z", "-F", "CARGO_PKG_VERSION", ref, "--", "crates/").splitlines()
    observed = {}
    for row in rows:
        path, line = row.split(b"\0", 1)
        prefix = ref.encode() + b":"
        if not path.startswith(prefix):
            return False
        path = path[len(prefix):]
        if path in observed:
            return False
        observed[path] = line.strip()
    return observed == expected


def snapshot(ref):
    root_mode, root_bytes = blob(ref, "Cargo.toml")
    lock_mode, lock_bytes = blob(ref, "Cargo.lock")
    root = tomllib.loads(root_bytes.decode("utf-8"))
    lock = tomllib.loads(lock_bytes.decode("utf-8"))
    workspace = root["workspace"]
    if ("package" in root or workspace["members"] != ["crates/*"]
            or workspace.get("exclude", []) != [] or "default-members" in workspace):
        raise ValueError("unsupported workspace")
    version = workspace["package"]["version"]
    if not isinstance(version, str) or not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("unsupported version")
    del workspace["package"]["version"]
    members = {}
    manifests = {}
    for directory, entry in tree(ref, "crates").items():
        if entry[0] != b"040000" or entry[1] != b"tree":
            raise ValueError("unsupported member tree")
        path = f"crates/{directory}/Cargo.toml"
        manifest = blob(ref, path)
        package = tomllib.loads(manifest[1].decode("utf-8"))["package"]
        name = package["name"]
        if (not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", name)
                or typed(package["version"]) != typed({"workspace": True}) or name in members
                or "workspace" in package):
            raise ValueError("unsupported package identity")
        members[name] = path
        manifests[path] = manifest
    if not members or type(lock["version"]) is not int or lock["version"] != 4:
        raise ValueError("unsupported lock format")
    normalized = copy.deepcopy(lock)
    local = set()
    identities = set()
    if not isinstance(normalized["package"], list):
        raise ValueError("unsupported package sequence")
    for package in normalized["package"]:
        if (not isinstance(package, dict)
                or not isinstance(package.get("name"), str)
                or not isinstance(package.get("version"), str)
                or any(key in package and not isinstance(package[key], str)
                       for key in ("source", "checksum"))
                or not isinstance(package.get("dependencies", []), list)
                or not all(isinstance(edge, str) for edge in package.get("dependencies", []))):
            raise ValueError("unsupported package record")
        identity = (package["name"], package["version"], package.get("source"))
        if identity in identities:
            raise ValueError("duplicate lock identity")
        identities.add(identity)
        if "source" not in package:
            name = package["name"]
            if (name not in members or name in local or "checksum" in package
                    or package["version"] != version):
                raise ValueError("inconsistent local record")
            local.add(name)
            del package["version"]
    if local != set(members) or not version_consumers(ref):
        raise ValueError("incomplete identity or changed version consumer")
    return version, (root_mode, lock_mode, typed(root), manifests, typed(normalized))


try:
    before, after = (snapshot(ref) for ref in sys.argv[1:])
    sys.exit(0 if before[0] != after[0] and before[1] == after[1] else 1)
except Exception:
    sys.exit(1)
PY
}

metadata_checked=false
metadata_only=false
while IFS= read -r -d '' path; do
  [ -n "$path" ] || continue
  case "$path" in
    Cargo.toml | Cargo.lock)
      if [ "$metadata_checked" = false ]; then
        metadata_checked=true
        release_metadata_only && metadata_only=true
      fi
      if [ "$metadata_only" = true ]; then
        continue
      fi
      emit true database-boundary
      exit 0
      ;;
    build.rs | rust-toolchain | rust-toolchain.toml | \
    .cargo/config | .cargo/config.toml | crates/*/Cargo.toml | \
    .github/workflows/ci.yml | \
    scripts/db-ci-scope.sh | scripts/test-db-ci-scope.sh | \
    scripts/db-verify-local.sh | scripts/run-db-suites.sh | \
    scripts/run-gate-binaries.sh | scripts/test-binaries-lib.sh | \
    scripts/dyld-env.sh | scripts/run-quiet.sh | \
    scripts/check-libpq-version.sh | \
    scripts/ci-pgdg.sh | scripts/ci-apt-llvm.sh | scripts/cargo.sh | \
    apps/db/* | \
    crates/*/src/db_*.rs | \
    crates/align_driver/src/query_meta_codegen.rs | \
    crates/align_driver/src/static_artifacts.rs | \
    crates/align_driver/src/static_inputs.rs | \
    crates/align_driver/src/static_runtime.rs | \
    crates/align_interface/src/static_artifact.rs | \
    crates/align_driver/tests/pkg_db_*.rs | \
    crates/*/tests/common* | crates/*/tests/helpers/* | \
    crates/align_driver/tests/db_harness/* | \
    crates/align_driver/tests/fixtures/pkg_db_* | \
    crates/align_driver/tests/golden/*postgres* | \
    crates/align_driver/tests/golden/*sqlite* | \
    crates/align_driver/tests/golden/migration_catalog_*)
      emit true database-boundary
      exit 0
      ;;
    crates/*/src/* | crates/*/build.rs | crates/*/tests/*.rs | crates/*/tests/*/*.rs)
      if source_change_names_database_boundary "$path"; then
        emit true database-source
        exit 0
      fi
      ;;
  esac
done < <(git diff --no-renames --name-only -z "$base_sha..$head_sha")

emit false no-database-boundary
