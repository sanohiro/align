# Test execution policy

The test suite has two distinct jobs:

1. prove on every change that the compiler still builds and its vertical path
   still works;
2. protect individual implementations with deep, expensive regression checks.

Those jobs must not share one mandatory command.

## Audit baseline

The July 2026 audit found:

- 159 `align_driver` integration-test binaries containing 2,159 tests;
- 10 differential tests that each compile and run 150–200 generated programs;
- frontend and formatter fuzz loops with 10,000–12,000 seeds;
- 14 driver test binaries using real sockets and 32 using the filesystem;
- runtime tests that include TLS, timeouts, process control, fd-leak cycles,
  cryptographic cost cases, and performance probes;
- a router benchmark with eight paired trials in the ordinary PR workflow.

Most of these tests have a valid regression role. Their accumulation into one
default gate did not.

## Ordinary PR gate

Run:

```text
scripts/test-pr.sh
```

This gate is deliberately fixed and bounded:

1. build the workspace, including the runtime archive;
2. run the explicit deterministic library-test list for the compiler crates and
   the small interface/formatter integration targets;
3. run `align_driver/tests/m0.rs`, which covers source checking, HIR-to-MIR
   lowering, native object emission, linking, execution, and a rejected
   program.

CI additionally builds release compiler/runtime artifacts and compiles and runs
`examples/hello.align` on Linux x86-64, Linux ARM64, and Apple Silicon. This is
the cross-platform packaged-command smoke path.

The ordinary gate does not run:

- the full driver regression corpus;
- differential fuzzers;
- real network, TLS, filesystem, timeout, process, or fd-leak suites;
- performance or scaling measurements;
- repeated concurrency and artifact-staging stress tests.

The compiler library list is intentionally explicit in `scripts/test-pr.sh`.
Adding a new workspace crate does not silently add its tests to every PR; the
new crate must first be given an intentional owner and gate classification.

## Change-specific verification

The author must run the narrow regression targets that own the changed
behavior. Examples:

```text
cargo test -p align_driver --test par_map
cargo test -p align_driver --test fuzz_differential
cargo test -p align_driver --test m11_http_server
cargo test -p align_runtime --lib http_client
cargo test -p align_runtime --lib par_map
```

Optimization work runs its named benchmark or measurement probe. Network,
filesystem, timeout, process, and fd work runs the corresponding real-resource
target in an unrestricted environment. A test should be added to an existing
owner target when possible; do not create another cross-cutting integration
matrix for a unit-level rule.

## Selection procedure

Choose the smallest test set that covers the changed boundary, then expand only
when the change crosses another boundary:

1. Identify the owner: documentation, one private implementation helper, a
   public FFI/ABI surface, compiler lowering, or a resource boundary.
2. Run the focused owner target first. For a private runtime helper, use a
   filtered library test such as `cargo test -p align_runtime --lib par_map`
   rather than the entire runtime library test binary.
3. Run the ordinary PR gate required for the change. Rust code changes use
   `scripts/test-pr.sh`; documentation-only changes need only their relevant
   consistency or render check.
4. Add a broader target only when the changed behavior is not exercised by the
   owner target, crosses crate/ABI/linker boundaries, changes scheduling or
   resource semantics, or is unusually broad.
5. Use `scripts/test-full.sh` only for an unusually broad change, a release
   candidate, or an explicit full-regression request.

Do not run a whole crate or the full workspace by reflex after a narrow change.
Do not repeat a target already covered by `scripts/test-pr.sh` unless the
focused invocation selects an additional behavior. Record the reason for every
expanded target and distinguish a product failure from a host permission,
network, toolchain, or dependency-linking limitation.

## Full regression

Run the retained full corpus explicitly when the change is unusually broad or
before a versioned release:

```text
scripts/test-full.sh
```

The full corpus is not a mandatory ordinary-PR or push gate. A focused
regression test remains required for a bug or optimization that needs it, but
that requirement does not promote the entire historical corpus into every
change's critical path.

## Growth rule

Every new integration test must name the boundary or regression it protects.
Prefer, in order:

1. a unit test beside the implementation;
2. one focused regression in the existing owner target;
3. an end-to-end test only when the failure crosses crate, ABI, linker,
   runtime, process, or protocol boundaries.

Load, throughput, scaling, repeated-race, differential-fuzz, and resource-leak
checks are explicit change-specific tests, never ordinary smoke tests.
