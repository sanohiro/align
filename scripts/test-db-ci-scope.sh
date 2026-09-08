#!/usr/bin/env bash
# Focused owner tests for scripts/db-ci-scope.sh. No compiler build or service.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

git -C "$fixture" init -q
git -C "$fixture" config user.name db-scope-test
git -C "$fixture" config user.email db-scope-test@example.invalid
mkdir -p "$fixture/scripts" "$fixture/crates/demo/src" \
  "$fixture/crates/demo/tests/common" "$fixture/apps/web"
cp "$repo_root/scripts/db-ci-scope.sh" "$fixture/scripts/db-ci-scope.sh"
chmod +x "$fixture/scripts/db-ci-scope.sh"
printf 'baseline\n' > "$fixture/apps/web/main.align"
printf 'pub fn ordinary() {}\n' > "$fixture/crates/demo/src/lib.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm baseline
base="$(git -C "$fixture" rev-parse HEAD)"

assert_scope() {
  expected="$1"
  shift
  output="$(cd "$fixture" && scripts/db-ci-scope.sh "$@")"
  printf '%s\n' "$output" | grep -Fxq "required=$expected" || {
    echo "expected required=$expected, got:" >&2
    printf '%s\n' "$output" >&2
    exit 1
  }
}

# An unrelated application edit does not provision PostgreSQL.
printf 'changed\n' > "$fixture/apps/web/main.align"
git -C "$fixture" add .
git -C "$fixture" commit -qm unrelated
unrelated="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$base" "$unrelated"

# Path records are NUL-delimited, so an unrelated filename cannot inject a
# forged GitHub output assignment.
weird_path=$'apps/web/unrelated\nrequired=true'
printf 'still unrelated\n' > "$fixture/$weird_path"
git -C "$fixture" add .
git -C "$fixture" commit -qm unusual-path
unusual="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$unrelated" "$unusual"

# A source file that newly names the DB boundary is included without maintaining
# a parallel filename list.
printf 'pub fn postgres_owner() {}\n' > "$fixture/crates/demo/src/lib.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm db-source
db_source="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$unusual" "$db_source"

# Removing the last visible DB marker still qualifies because the base content
# remains in the deletion hunk.
printf 'pub fn ordinary_again() {}\n' > "$fixture/crates/demo/src/lib.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm remove-db-source
db_source_removed="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$db_source" "$db_source_removed"

# A monolithic source file can contain DB-specific and unrelated functions. An
# unrelated function edit must not inherit the unchanged marker elsewhere in
# the file, while a body-only edit inside the DB function is owned through the
# zero-context hunk's function header.
printf 'fn postgres_owner() {\n  let version = 1;\n}\n\nfn ordinary() {\n  let value = 1;\n}\n' \
  > "$fixture/crates/demo/src/lib.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm monolithic-source
monolithic_source="$(git -C "$fixture" rev-parse HEAD)"

sed -i.bak 's/let value = 1/let value = 2/' "$fixture/crates/demo/src/lib.rs"
rm "$fixture/crates/demo/src/lib.rs.bak"
git -C "$fixture" add .
git -C "$fixture" commit -qm unrelated-monolithic-function
unrelated_monolithic="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$monolithic_source" "$unrelated_monolithic"

sed -i.bak 's/let version = 1/let version = 2/' "$fixture/crates/demo/src/lib.rs"
rm "$fixture/crates/demo/src/lib.rs.bak"
git -C "$fixture" add .
git -C "$fixture" commit -qm database-monolithic-function
database_monolithic="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$unrelated_monolithic" "$database_monolithic"

git -C "$fixture" mv crates/demo/src/lib.rs crates/demo/src/renamed.rs
git -C "$fixture" commit -qm rename-database-source
renamed_database_source="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$database_monolithic" "$renamed_database_source"

# Dedicated database production modules are owned by path, so a generic helper
# and marker-free body edit cannot evade the service merely because neither the
# changed line nor its function header says PostgreSQL/libpq/SQLite.
printf 'fn pq_text() {\n  value.to_str();\n}\n' > "$fixture/crates/demo/src/db_prepare_native.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm dedicated-database-source
dedicated_database_source="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$renamed_database_source" "$dedicated_database_source"

sed -i.bak 's/value.to_str()/value.as_str()/' "$fixture/crates/demo/src/db_prepare_native.rs"
rm "$fixture/crates/demo/src/db_prepare_native.rs.bak"
git -C "$fixture" add .
git -C "$fixture" commit -qm marker-free-database-helper
marker_free_database_helper="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$dedicated_database_source" "$marker_free_database_helper"

# Shared owner infrastructure reaches every pkg.db suite even when its own text
# does not name a database.
printf 'pub fn shared_fixture() {}\n' > "$fixture/crates/demo/tests/common/mod.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm shared-test-harness
shared_harness="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$marker_free_database_helper" "$shared_harness"

# A non-pkg_db owner test that directly names the boundary is also classified
# by content.
printf 'fn postgres_regression() {}\n' > "$fixture/crates/demo/tests/direct.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm direct-db-owner
direct_owner="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$shared_harness" "$direct_owner"

# Adding and deleting an unrelated leaf owner must not provision PostgreSQL.
printf 'fn identity_owner() {}\n' > "$fixture/crates/demo/tests/json_identity.rs"
git -C "$fixture" add .
git -C "$fixture" commit -qm unrelated-leaf-owner
leaf_owner="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$direct_owner" "$leaf_owner"

rm "$fixture/crates/demo/tests/json_identity.rs"
git -C "$fixture" add -u
git -C "$fixture" commit -qm delete-unrelated-leaf-owner
leaf_deleted="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$leaf_owner" "$leaf_deleted"

# Direct package and workflow machinery changes are always included.
mkdir -p "$fixture/apps/db" "$fixture/.github/workflows"
printf 'db\n' > "$fixture/apps/db/main.align"
git -C "$fixture" add .
git -C "$fixture" commit -qm db-package
db_package="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$leaf_deleted" "$db_package"

printf 'name: CI\n' > "$fixture/.github/workflows/ci.yml"
git -C "$fixture" add .
git -C "$fixture" commit -qm db-workflow
workflow="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$db_package" "$workflow"

runner_parent="$workflow"
for runner_dependency in \
  run-db-suites.sh run-gate-binaries.sh test-binaries-lib.sh dyld-env.sh \
  run-quiet.sh; do
  printf '#!/usr/bin/env bash\n' > "$fixture/scripts/$runner_dependency"
  git -C "$fixture" add .
  git -C "$fixture" commit -qm "db-runner-$runner_dependency"
  runner_head="$(git -C "$fixture" rev-parse HEAD)"
  assert_scope true "$runner_parent" "$runner_head"
  runner_parent="$runner_head"
done

# An unrelated deletion stays out of the service job.
rm "$fixture/apps/web/main.align"
git -C "$fixture" add -u
git -C "$fixture" commit -qm unrelated-deletion
unrelated_deleted="$(git -C "$fixture" rev-parse HEAD)"
assert_scope false "$runner_parent" "$unrelated_deleted"

# Deleted DB-naming source is classified from its base content, and a deleted
# direct DB path is classified by path. Unreadable ranges still fail closed.
rm "$fixture/crates/demo/tests/direct.rs"
git -C "$fixture" add -u
git -C "$fixture" commit -qm delete-db-source
db_source_deleted="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$unrelated_deleted" "$db_source_deleted"

rm "$fixture/apps/db/main.align"
git -C "$fixture" add -u
git -C "$fixture" commit -qm delete-db-path
db_path_deleted="$(git -C "$fixture" rev-parse HEAD)"
assert_scope true "$db_source_deleted" "$db_path_deleted"
assert_scope true not-a-commit "$db_path_deleted"

# CI extracts the trusted classifier outside the checkout. Its explicit root
# binding must still classify the repository rather than the temporary file's
# parent directory.
trusted_copy="$fixture/trusted-db-ci-scope.sh"
cp "$repo_root/scripts/db-ci-scope.sh" "$trusted_copy"
chmod +x "$trusted_copy"
output="$(DB_CI_REPO_ROOT="$fixture" "$trusted_copy" "$base" "$unrelated")"
printf '%s\n' "$output" | grep -Fxq 'required=false'

# Exercise the whole metadata proof through an extracted trusted classifier.
# The fixture uses real version-reference lines so that production drift also
# closes the exception; no Cargo resolution, compiler build or service is needed.
python3 -I - "$repo_root" <<'PY'
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

repository = Path(sys.argv[1])
with tempfile.TemporaryDirectory(prefix="align-db-metadata-") as owned:
    owned = Path(owned)
    root = owned / "repo"
    root.mkdir()
    trusted = owned / "trusted-classifier.sh"
    shutil.copyfile(repository / "scripts/db-ci-scope.sh", trusted)

    def git(*args):
        return subprocess.check_output(["git", "-C", str(root), *args], stderr=subprocess.PIPE).decode().strip()

    def write(path, value):
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(value)

    def replace(path, before, after):
        target = root / path
        content = target.read_text()
        assert before in content, (path, before)
        target.write_text(content.replace(before, after))

    def commit():
        git("add", "-A")
        git("commit", "-qm", "fixture")
        return git("rev-parse", "HEAD")

    git("init", "-q")
    git("config", "user.name", "metadata-owner")
    git("config", "user.email", "metadata-owner@example.invalid")
    write("Cargo.toml", '[workspace]\nmembers = ["crates/*"]\n[workspace.package]\nversion = "0.1.0"\n[profile.release]\nopt-level = 1\n')
    names = ["demo", "align_driver", "align_repl"]
    for name in names:
        write(f"crates/{name}/Cargo.toml", f'[package]\nname = "{name}"\nversion.workspace = true\n')
    for path in ["crates/align_driver/src/main.rs", "crates/align_driver/src/cache.rs",
                 "crates/align_repl/src/main.rs", "crates/align_driver/tests/version.rs"]:
        write(path, (repository / path).read_text())
    write("Cargo.lock", 'version = 4\n' + ''.join(
        f'\n[[package]]\nname = "{name}"\nversion = "0.1.0"\ndependencies = ["external"]\n'
        for name in names
    ) + '\n[[package]]\nname = "external"\nversion = "1.2.3"\nsource = "registry+https://example.invalid/index"\nchecksum = "fixed-checksum"\n')
    baseline = commit()

    def bump():
        replace("Cargo.toml", 'version = "0.1.0"', 'version = "0.2.0"')
        replace("Cargo.lock", 'version = "0.1.0"', 'version = "0.2.0"')

    def check(label, expected, change=lambda: None, *, do_bump=True, extra_env=None, preparation=None):
        git("checkout", "-qf", baseline)
        case_base = baseline
        if preparation is not None:
            preparation()
            case_base = commit()
        if do_bump:
            bump()
        change()
        head = commit()
        env = dict(os.environ, DB_CI_REPO_ROOT=str(root))
        env.update(extra_env or {})
        output = subprocess.check_output(
            ["bash", str(trusted), case_base, head], cwd=root, env=env, stderr=subprocess.PIPE
        ).decode()
        assert output.splitlines() == [f"required={str(expected).lower()}",
                                      "reason=database-boundary" if expected else "reason=no-database-boundary"], (label, output)
        print(f"metadata scope: {label}: ok")

    check("consistent-bump", False)
    check("metadata-with-prose", False, lambda: write("RELEASE_NOTES.md", "release\n"))
    check("metadata-with-corpus-script", False, lambda: write("scripts/prepare-prebuilt-cache-project.sh", "# corpus\n"))
    check("profile-change", True, lambda: replace("Cargo.toml", "opt-level = 1", "opt-level = 2"))
    check("toml-scalar-kind", True, lambda: replace("Cargo.toml", "opt-level = 1", "opt-level = true"))
    check("workspace-dependency", True, lambda: write("Cargo.toml", (root / "Cargo.toml").read_text() + '\n[workspace.dependencies]\nexternal = "2"\n'))
    check("dependency-version", True, lambda: replace("Cargo.lock", 'version = "1.2.3"', 'version = "1.2.4"'))
    check("dependency-checksum", True, lambda: replace("Cargo.lock", "fixed-checksum", "different-checksum"))
    check("dependency-source", True, lambda: replace("Cargo.lock", "registry+https://example.invalid/index", "git+https://example.invalid/repo#abc"))
    check("dependency-edge", True, lambda: replace("Cargo.lock", 'dependencies = ["external"]', "dependencies = []"))
    check("lock-format", True, lambda: replace("Cargo.lock", "version = 4", "version = 3"))
    check("partial-local-bump", True, lambda: replace("Cargo.lock", 'name = "demo"\nversion = "0.2.0"', 'name = "demo"\nversion = "0.1.0"'))
    check("root-only-bump", True, lambda: replace("Cargo.lock", 'version = "0.2.0"', 'version = "0.1.0"'))
    check("lock-only-bump", True, lambda: replace("Cargo.toml", 'version = "0.2.0"', 'version = "0.1.0"'))
    check("comment-only-root", True, lambda: write("Cargo.toml", (root / "Cargo.toml").read_text() + "\n# comment\n"), do_bump=False)
    check("unsupported-version", True, lambda: [replace(path, '"0.2.0"', '"0.2.0-rc.1"') for path in ["Cargo.toml", "Cargo.lock"]])
    check("missing-local-record", True, lambda: replace("Cargo.lock", '\n[[package]]\nname = "demo"\nversion = "0.2.0"\ndependencies = ["external"]\n', ""))
    check("extra-local-record", True, lambda: write("Cargo.lock", (root / "Cargo.lock").read_text() + '\n[[package]]\nname = "unowned"\nversion = "0.2.0"\n'))
    check("duplicate-lock-record", True, lambda: write("Cargo.lock", (root / "Cargo.lock").read_text() + '\n[[package]]\nname = "demo"\nversion = "0.2.0"\n'))
    check("local-checksum", True, lambda: replace("Cargo.lock", 'name = "demo"', 'name = "demo"\nchecksum = "unexpected"'))
    check("member-manifest-change", True, lambda: write("crates/demo/Cargo.toml", (root / "crates/demo/Cargo.toml").read_text() + 'edition = "2024"\n'))
    check("non-inherited-version", True, lambda: replace("crates/demo/Cargo.toml", "version.workspace = true", 'version = "0.2.0"'))
    check("duplicate-member-name", True, lambda: replace("crates/demo/Cargo.toml", 'name = "demo"', 'name = "align_driver"'))
    check("workspace-membership", True, lambda: replace("Cargo.toml", '["crates/*"]', '["crates/demo"]'))
    check("workspace-exclusion", True, lambda: replace("Cargo.toml", "[workspace]\n", '[workspace]\nexclude = ["crates/demo"]\n'))
    check("missing-manifest", True, lambda: (root / "crates/demo/Cargo.toml").unlink())
    check("missing-lock", True, lambda: (root / "Cargo.lock").unlink())
    check("invalid-toml", True, lambda: write("Cargo.lock", "not valid = ["))
    check("root-mode-change", True, lambda: (root / "Cargo.toml").chmod(0o755))
    check("member-mode-change", True, lambda: (root / "crates/demo/Cargo.toml").chmod(0o755))
    check("member-symlink", True, lambda: ((root / "crates/demo/Cargo.toml").unlink(), (root / "crates/demo/Cargo.toml").symlink_to("../align_driver/Cargo.toml")))
    check("root-symlink", True, lambda: ((root / "Cargo.toml").unlink(), (root / "Cargo.toml").symlink_to("crates/demo/Cargo.toml")))
    check("non-tree-member", True, lambda: write("crates/extra", "not a directory"))
    check("new-version-consumer", True, lambda: write("crates/demo/src/version.rs", 'const VERSION: &str = env!("CARGO_PKG_VERSION");\n'))
    check("changed-version-consumer", True, lambda: replace("crates/align_driver/src/main.rs", 'println!("alignc {}", env!("CARGO_PKG_VERSION"));', 'print!("alignc {}", env!("CARGO_PKG_VERSION"));'))
    # The metadata exception must not terminate the rest of the path scan.
    for label, path in [("db-source", "crates/demo/src/db_native.rs"),
                        ("db-owner", "crates/align_driver/tests/pkg_db_new.rs"),
                        ("shared-owner", "crates/demo/tests/common/new.rs"),
                        ("gate-script", "scripts/run-db-suites.sh"),
                        ("workflow", ".github/workflows/ci.yml")]:
        check(label, True, lambda path=path: write(path, "boundary change\n"))

    # Invalid identities already present on both sides must fail the metadata
    # proof itself, without relying on the later changed-member path rule.
    for label, prepare in [
        ("existing-invalid-lock-name", lambda: replace("Cargo.lock", 'name = "external"', 'name = false')),
        ("existing-invalid-lock-edge", lambda: replace("Cargo.lock", 'dependencies = ["external"]', 'dependencies = true')),
        ("existing-duplicate-name", lambda: replace("crates/demo/Cargo.toml", 'name = "demo"', 'name = "align_driver"')),
        ("existing-invalid-inheritance", lambda: replace("crates/demo/Cargo.toml", "version.workspace = true", "version.workspace = 1")),
        ("existing-missing-manifest", lambda: (root / "crates/demo/Cargo.toml").unlink()),
        ("existing-non-tree-member", lambda: write("crates/extra", "not a directory")),
        ("existing-version-consumer", lambda: write("crates/demo/src/version.rs", 'const VERSION: &str = env!("CARGO_PKG_VERSION");\n')),
        ("existing-exclusion", lambda: replace("Cargo.toml", "[workspace]\n", '[workspace]\nexclude = ["crates/demo"]\n')),
    ]:
        check(label, True, preparation=prepare)

    # A hostile checkout or PYTHONPATH module must neither run nor forge a
    # successful verifier exit, including when metadata really changes a dependency.
    marker = owned / "shadow-imported"
    shadow = owned / "shadow"
    shadow.mkdir()
    payload = f'from pathlib import Path\nPath({str(marker)!r}).touch()\nraise SystemExit(0)\n'
    for module in ["tomllib.py", "sitecustomize.py"]:
        (shadow / module).write_text(payload)
    for invalid in [False, True]:
        def hostile(invalid=invalid):
            for module in ["tomllib.py", "sitecustomize.py"]:
                write(module, payload)
            if invalid:
                replace("Cargo.lock", 'version = "1.2.3"', 'version = "1.2.4"')
        check(f"isolated-shadow-{invalid}", invalid, hostile, extra_env={"PYTHONPATH": str(shadow)})
        assert not marker.exists(), "untrusted Python module executed"

    blocked_tools = owned / "tools"
    blocked_tools.mkdir()
    unavailable = blocked_tools / "python3"
    for status in [1, 127]:
        unavailable.write_text(f'#!/usr/bin/env bash\nexit {status}\n')
        unavailable.chmod(0o755)
        check(f"parser-unavailable-{status}", True, extra_env={"PATH": f"{blocked_tools}:{os.environ['PATH']}"})
PY

ci_workflow="$repo_root/.github/workflows/ci.yml"
test "$(grep -Fc 'name: PostgreSQL integration (required)' "$ci_workflow")" -eq 1
grep -Fq "if: needs.db-scope.outputs.required == 'true'" "$ci_workflow"
grep -Fq 'if: ${{ always() }}' "$ci_workflow"
grep -Fq 'reason=protected-pr-merge' "$ci_workflow"
grep -Fq 'commits/$HEAD_SHA/pulls' "$ci_workflow"
grep -Fq 'git show "$TRUSTED_TIP:scripts/db-ci-scope.sh"' "$ci_workflow"
grep -Fq 'reason=classifier-bootstrap' "$ci_workflow"
grep -Fq 'test "$SCOPE_RESULT" = success' "$ci_workflow"
grep -Fq 'true) test "$DB_RESULT" = success' "$ci_workflow"
grep -Fq 'false) test "$DB_RESULT" = skipped' "$ci_workflow"
grep -Fq 'timeout-minutes: 30' "$ci_workflow"
grep -Fq 'name: PostgreSQL integration (${{ matrix.db-shard }})' "$ci_workflow"
grep -Fq 'run: scripts/run-db-suites.sh "${{ matrix.db-shard }}"' "$ci_workflow"
grep -Fq 'ALIGN_GATE_JOBS: "2"' "$ci_workflow"
test "$(grep -Fc 'scripts/run-db-suites.sh' "$repo_root/scripts/db-verify-local.sh")" -eq 1
test "$(grep -Fc 'scripts/run-quiet.sh --expect-failure' "$ci_workflow")" -eq 2
test "$(grep -Fc 'scripts/run-quiet.sh --expect-failure' \
  "$repo_root/scripts/db-verify-local.sh")" -eq 2

expected_owners="$(printf '%s\n' \
  pkg_db_q1 pkg_db_q2 pkg_db_q3 pkg_db_q4a pkg_db_q4b pkg_db_q5a \
  pkg_db_q5b1 pkg_db_q5b2 pkg_db_q6 pkg_db_a1 pkg_db_pool pkg_db_a2 \
  pkg_db_callbacks pkg_db_vc1 | LC_ALL=C sort)"
all_owners="$($repo_root/scripts/run-db-suites.sh --list all | LC_ALL=C sort)"
test "$all_owners" = "$expected_owners"

observed_shards=""
for db_shard in catalog-stream delivery-callbacks vector-static portable-pool; do
  grep -Fq "          - $db_shard" "$ci_workflow"
  observed_shards="$observed_shards
$($repo_root/scripts/run-db-suites.sh --list "$db_shard")"
done
observed_shards="$(printf '%s\n' "$observed_shards" | sed '/^$/d' | LC_ALL=C sort)"
test "$observed_shards" = "$expected_owners"
test -z "$(printf '%s\n' "$observed_shards" | uniq -d)"

if "$repo_root/scripts/run-db-suites.sh" --list not-a-shard >/dev/null 2>&1; then
  echo "unknown database shard was accepted" >&2
  exit 1
fi

echo "database CI scope tests passed"
