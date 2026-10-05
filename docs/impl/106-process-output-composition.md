# Incremental child-output composition

## Capability boundary

Provide `examples/process_output.align` and synchronized English/Japanese guide
instructions through the existing plan 49 process contract. This is one usable
application composition, not a new API, runtime mechanism or compiler strategy.
The live reads, writable-array admission, native child ownership and typed wait
result remain unchanged. No language specification or library-design change is
needed.

## Application ledger

| Surface | Exact policy |
| --- | --- |
| Entry | `pub fn main(args: array<str>) -> Result<(), Error>` requires at least the executable name after its own `argv[0]`; absence returns `Error.Invalid` before allocation or launch. |
| Launch | `process.command(args[1], args[1..]).start()` passes the executable as the child's `argv[0]`. Existing native text/NUL validation, path lookup and launch errors apply. The default stdin is `/dev/null`; stdout and stderr are separate capture pipes. No shell is inserted. |
| Storage | One explicitly reserved, initialized `array<u8>` of 65,536 bytes is constructed before launch. The builder transfers its storage to the array. Each read exclusively borrows the same writable window; only the returned prefix is synchronously written before its next reuse. No full-output collector or UTF-8 conversion is used. This is not an exact whole-process RSS or latency promise. |
| Read | Existing `Result<Option<i64>, Error>`: positive `Some(n)` publishes `bytes[0..n]`; `Some(0)` marks that pipe's EOF; `None` publishes nothing and permits polling. Stdout is visited before stderr in each iteration; no cross-stream ordering is promised. |
| Wait | Stop reading a pipe after its EOF, poll only remaining pipes for at most 1,000,000,000 ns per call, and leave the loop once both reach EOF. Then consume the existing typed `wait_result` through `child.wait()`. |
| Result/errors | `Exited(0)` returns `Ok(())`; any other exit or signal returns `Error.Invalid`. This is application policy, not child-status forwarding. Native read/write/poll/wait errors propagate with `?`; a late error can leave already-written output. |
| Lifetime/limits | Child and array are Move owners. Direct-child cleanup follows plan 49, including blocking Drop. Poll duration does not bound the entire command; output writes and final wait may block. No cancellation, output cap, stdin forwarding, descendant containment or engine/GPU policy is promised. |
| Build/identity | Ordinary `alignc` source checks and executable build; no application cache format or native ABI is introduced. The actual example file, not a copied lookalike, is the owner-test input. |
| Prose | `docs/guide/18-std-services.md` and its `ja/` mirror describe the same invocation, read states, termination policy and blocking limits. |

## Implementation closure matrix

This composition follows the reviewed plan 49 contract and existing private
ArtifactStage/process-group fixture strategy. It changes no safety strategy.
The author-side matrix pass precedes one fresh full-diff preflight review.

| Invariant | Implementation and focused owner |
| --- | --- |
| Writable initialization, builder move-out and bounded prefixes | Example constructor and both read/write arms; `incremental_process_output_example` checks the actual file in whole-program/per-unit modes and executes multi-window arbitrary bytes. Existing `live_out_buffers_and_borrow_modes` retains writable-origin refusal. |
| Pending/EOF joins and both EOF orders | Independent stdout/stderr flags and interest mask; parameterized controlled producer closes either pipe first, releases the other later and proves exact output before child exit. |
| No collect-before-exit or UTF-8 conversion | One reused array and synchronous prefix writes; producer remains blocked on fixture-owned release files while exact NUL/non-UTF-8 output is observed. |
| Typed success, nonzero exit and signal | Final termination match; application owner checks success, exit seven and SIGTERM. Existing `typed_wait_and_cached_result` owns native cached/whole-program status representation. |
| Early refusal and fallible writes | Argument guard and `?`; application owner checks missing executable argument and a read-only stdout sink, including exact normal error exit/report. Partial-prefix errors remain explicitly permitted. |
| Test ownership and bounded failure | Exclusive `ArtifactStage` contains source, executable, worker, payload, release markers and file-backed captures. Immediate child guard owns one fresh process group, with one deadline shared by setup/probes/execution/EINTR/kill/reap and five seconds reserved for cleanup. The worker creates no detached sessions. |
| Platform and compilation parity | Same owner on macOS and Linux; whole/per-unit source admission plus actual default per-unit executable build. Existing R65 owners supply native and whole-program execution coverage. |
| Non-applicable compiler cells | No new type, IR variant, generic rule, interface serialization, replacement lowering, runtime allocation or ABI. Existing owners retain those contracts; no synthetic new tests are required. |

Focused command: `scripts/cargo.sh test -p align_driver --test m11_process_live`.
The common owner is correctness verification, not a benchmark. No new
performance/resource guarantee requires a separate benchmark.
