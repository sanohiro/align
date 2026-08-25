#!/usr/bin/env bash
# Replay the exact Request 6 Copy-row codegen comparison between its fixed baseline and reviewed
# implementation. The Rust owner and fixture come from the implementation commit so later compiler
# and interface evolution cannot turn this historical evidence into a current-tree failure.
set -euo pipefail

BASELINE_SHA="576e57307fe4ef34e74566f5e389a2f0e2a04acd"
IMPLEMENTATION_SHA="aa5bb7d66d0436c2d9ebf89f252b0ba5d528c2a8"
TOOLCHAIN="1.96.1"
if [[ $# -ne 0 ]]; then
    echo "usage: $0" >&2
    exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
COMMON_DIR="$(git -C "$REPO_ROOT" rev-parse --git-common-dir)"
case "$COMMON_DIR" in
    /*) ;;
    *) COMMON_DIR="$REPO_ROOT/$COMMON_DIR" ;;
esac
COMMON_DIR="$(cd "$COMMON_DIR" && pwd)"

if [[ "$(rustc +"$TOOLCHAIN" --version)" != *"rustc 1.96.1 "* ]]; then
    echo "json-scan identity requires rustc 1.96.1; found: $(rustc +"$TOOLCHAIN" --version)" >&2
    exit 3
fi
if [[ "$(llvm-config-22 --version)" != "22.1.8" ]]; then
    echo "json-scan identity requires llvm-config-22 22.1.8; found: $(llvm-config-22 --version)" >&2
    exit 3
fi
command -v cc >/dev/null
if [[ -n "${RUSTFLAGS-}" ]]; then
    echo "json-scan identity requires RUSTFLAGS to be unset" >&2
    exit 3
fi

TMP_ROOT="$(mktemp -d)"
BASELINE_DIR="$TMP_ROOT/baseline"
IMPLEMENTATION_DIR="$TMP_ROOT/implementation"
BASELINE_OUT="$TMP_ROOT/out-baseline"
IMPLEMENTATION_OUT="$TMP_ROOT/out-implementation"

cleanup() {
    git --git-dir="$COMMON_DIR" worktree remove --force "$BASELINE_DIR" >/dev/null 2>&1 || true
    git --git-dir="$COMMON_DIR" worktree remove --force "$IMPLEMENTATION_DIR" >/dev/null 2>&1 || true
    rm -rf "$TMP_ROOT"
}
trap cleanup EXIT

git --git-dir="$COMMON_DIR" worktree add --detach "$BASELINE_DIR" "$BASELINE_SHA" >/dev/null
git --git-dir="$COMMON_DIR" worktree add --detach "$IMPLEMENTATION_DIR" "$IMPLEMENTATION_SHA" >/dev/null

run_probe() {
    local worktree="$1"
    local output="$2"
    mkdir -p "$output" "$worktree/crates/align_driver/tests/fixtures"
    git --git-dir="$COMMON_DIR" show \
        "${IMPLEMENTATION_SHA}:crates/align_driver/tests/json_scan_identity.rs" \
        >"$worktree/crates/align_driver/tests/json_scan_identity.rs"
    git --git-dir="$COMMON_DIR" show \
        "${IMPLEMENTATION_SHA}:crates/align_driver/tests/fixtures/json_scan_copy_identity.align" \
        >"$worktree/crates/align_driver/tests/fixtures/json_scan_copy_identity.align"
    (
        cd "$worktree"
        env -u RUSTFLAGS LC_ALL=C ALIGNC_CACHE=off ALIGN_JSON_SCAN_IDENTITY_OUT="$output" \
            cargo +"$TOOLCHAIN" test --release --locked --target x86_64-unknown-linux-gnu \
            -p align_driver --test json_scan_identity -- --exact json_scan_cross_compiler_identity
    )
}

run_probe "$BASELINE_DIR" "$BASELINE_OUT"
run_probe "$IMPLEMENTATION_DIR" "$IMPLEMENTATION_OUT"

compare_exact() {
    local relative="$1"
    if ! cmp -s "$BASELINE_OUT/$relative" "$IMPLEMENTATION_OUT/$relative"; then
        echo "json-scan identity mismatch: $relative" >&2
        exit 1
    fi
}

compare_exact interface.bin
compare_exact interface-hash
compare_exact mir.txt
compare_exact llvm.ll
compare_exact object.o

KEY_FIELDS=(
    cache_format_version
    frontend_schema
    located
    impl_hash
    dep_interface_hashes
    exports
    target_triple
    object_format
    resolved_cpu
    resolved_features
    profile_name
    pipeline
    codegen_opt
    reloc_model
    code_model
    llvm_version
    rt_lto
    rt_lto_digest
    pgo_mode
    unit
)
for field in "${KEY_FIELDS[@]}"; do
    compare_exact "key-fields/$field"
done

if cmp -s "$BASELINE_OUT/key-fields/compiler_build_id" "$IMPLEMENTATION_OUT/key-fields/compiler_build_id"; then
    echo "json-scan identity expected compiler_build_id to differ" >&2
    exit 1
fi
if cmp -s "$BASELINE_OUT/key-full-digest" "$IMPLEMENTATION_OUT/key-full-digest"; then
    echo "json-scan identity expected full CodegenKey digest to differ" >&2
    exit 1
fi
if cmp -s "$BASELINE_OUT/key-slot-digest" "$IMPLEMENTATION_OUT/key-slot-digest"; then
    echo "json-scan identity expected compiler-isolated slot digest to differ" >&2
    exit 1
fi

# Only compiler_build_id differs in the ordered CodegenKey comparison. The object is identical, but
# the distinct full/slot digests prove that no cache object can be shared across the builds. The
# implementation-side cache_codegen owner separately exercises the production classifier and its
# compiler-independent full-key digest.
echo "json-scan identity: interface/MIR/LLVM/object and all non-build-id CodegenKey fields match"
echo "json-scan identity: compiler build id, full digest, and slot digest are isolated"
