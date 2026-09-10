# Standard-library and package responsibility review

Status: **PLANNED — future assessment; implementation is not scheduled.**
Owner direction: 2026-09-11. Record the plan now; do not start implementation.
This document selects an assessment sequence, not public APIs or new language
semantics. No new exec, tree or CLI package is selected.

## Direction

Assess general process execution, filesystem traversal and command-line handling
through the existing `std.process`, `std.fs` and `std.cli` facilities first.
A convenience operation or an implementation written in Align does not, by
itself, belong in pkg. Standard-library operations may compose existing native
primitives in ordinary Align; they do not each require a new compiler builtin.

Keep the existing [package philosophy](../design-notes.md#the-package-philosophy)
and [library boundary](17-library-boundary-prerequisites.md): core owns language
intrinsics, std owns general system facilities, and pkg owns independently useful
framework/protocol/domain composition. Applications select product policy.
First-party packages should demonstrate practical ordinary Align, with explicit
ownership, errors, allocation and effects. General-purpose assembly should have
one appropriate library owner instead of being rewritten by every application.

## Baseline overlap assessment

Planning-time inspection covered the public surfaces and imports of all nine
shipped packages (29 Align files under `apps/*/pkg/`), their authoritative
[package designs](pkg-design/), the std.cli implementation, process capture/launch
entry points, retained-tree contracts and the current library roadmap. This is a
responsibility/overlap assessment, not an internal correctness audit or a claim
that every helper implementation is deduplicated.

| Existing package source | Responsibility already supplied | Disposition for this plan |
| --- | --- | --- |
| [web](../../apps/web/pkg/web.align), [ws](../../apps/ws/pkg/ws.align) | HTTP routing/serving and WebSocket handling through the same server. | Preserve the shared server; no new event-loop framework. |
| [db](../../apps/db/pkg/db.align), including driver/pool submodules | Queries, transactions, rows, metadata and explicit pooling. | Database execution remains a domain API; OS-command work adds no second resource/pool system. |
| [auth](../../apps/auth/pkg/auth.align), [s3](../../apps/s3/pkg/s3.align) | Authentication formats and request signing over crypto/encoding/HTTP. | Preserve shared primitives and explicit caller policy; no duplicate signing/transport layer. |
| [kv](../../apps/kv/pkg/kv.align) | Bounded RESP2 operations and connection lifecycle. | Protocol synchronization/retirement is distinct from process cleanup. |
| [csv](../../apps/csv/pkg/csv.align), [frame](../../apps/frame/pkg/frame.align) | Typed CSV decoding and bounded column joins. | Preserve existing data contracts; filesystem observations add no new table/codec layer. |
| [template](../../apps/template/pkg/template.align) | Escaped HTML construction. | No new text/HTML builder for CLI output or execution reports. |

| Suggested work | Existing owner and evidence | Decision |
| --- | --- | --- |
| CLI flags, bool/str/i64 values, defaults and usage | [std.cli](std-design/cli.md); `align_rt_cli_*` in `crates/align_runtime/src/lib.rs`. | Already supplied. Withdraw the proposed `pkg.cli`; assess a demonstrated subcommand gap through std.cli first. |
| Command launch, text/byte capture, output caps and timeout | [std.process](std-design/process.md); `align_rt_command_run` and `align_rt_command_run_bytes` share native capture. | Already supplied. Do not duplicate them as `pkg.exec`. |
| Live output, readiness, signals, typed status and descendant authority | [Plans 49](49-native-process-contract.md) and [50](50-r65-process-capability-handoff.md); native process modules. | Reuse existing owners and syscalls; assess only missing common composition or usability in std.process. |
| Test execution, cancellation and completion reporting | `crates/align_driver/src/test_runner.rs` and [core.test](core-design/test.md). | Preserve compiler test admission/acknowledgement guarantees; do not copy or automatically replace the runner. |
| Directory admission, enumeration, relative operations, metadata and streaming hash | [Plan 45](45-retained-byte-tree-plan.md), [plan 41](41-incremental-sha256-plan.md). | Already supplied. Assess general traversal through std.fs; retain hashing in std.crypto. No duplicate filesystem/digest implementation. |

The absence of a package with a suggested name is not evidence of a missing
capability. Use current source and owner evidence, not historical request labels.

## Future work order

1. **Reuse and responsibility first.** Start from the overlap table. For a
   concrete concern, identify the current source owner, the real consumer call
   site, and whether the disposition is use-as-is, improve documentation/example,
   simplify an implementation, or design an existing-module extension. Do not
   restart a broad review of unchanged packages.
2. **std.process composition.** Inspect one real live-execution workflow against
   the existing synchronous and live APIs. Assess common caller-controlled
   progress, cancellation and execution/cleanup results only where existing calls
   or a small example do not suffice. Keep one child ownership implementation.
3. **std.fs traversal.** Inspect one real retained-directory walk for repeated
   general traversal/order/filter assembly. Keep product manifests, snapshot
   formats, publication and workspace-deletion policy application-owned.
4. **std.cli only for a concrete remaining gap.** Existing flags, typed values,
   defaults and usage are not new work. Compare any subcommand need with actual
   use of the existing parser before proposing changes.

Existing correctness repairs and accepted delivery commitments take precedence.
Within this future library track, reuse/duplication review precedes new feature
breadth, followed by process then filesystem composition. The shipped package
sequence in [the roadmap](07-roadmap.md) is existing functionality, not an
unstarted queue. This plan does not automatically preempt unrelated transport
work or schedule a rewrite of shipped packages.

## Design constraints for selected work

- Prefer ordinary Align composition over existing general facilities. Retain
  justified native code and static compiler support. For db/csv/frame/template,
  distinguish necessary type/ownership/code-generation facts from ordinary
  library logic before proposing a relocation. Do not remove support merely
  because it recognizes a package name.
- Each behavior has one owner. An intentional layer move replaces the old path
  under the pre-release rules; a parallel parser, launcher, filesystem backend,
  ownership model or error model is not the solution.
- Process work preserves native Linux/macOS common behavior and explicit
  Linux-only guarantees. Examine dual-stream progress, EOF before termination,
  cancellation, partial acquisition, execution plus cleanup failures and scope
  overlap. No silent weakening on macOS or new background supervisor. R65 stays
  complete independently of this future usability assessment.
- Filesystem work considers byte names, symlinks, concurrent mutation, traversal
  order, storage bounds and retained-handle lifetime together. A walk does not
  certify a stable snapshot. Recursive deletion is not an automatic addition.
- Allocation, copies, limits and effects remain visible. Benchmark only a stated
  performance/resource claim; a move into Align source must preserve any existing
  claim rather than assume equivalent cost.

## Completion and handoff

The future assessment is complete when each examined concern has a disposition
and the selected next capability has a bounded design brief. Choosing the
existing facility or adding an example is a valid completed outcome. No package
count or requirement to implement every suggestion is imposed.

A selected public change gets its exact contract ledger, author consistency pass
and the normal independent design review before implementation. Ownership/FFI/IR
changes follow the existing implementation closure gate. A language restriction
gets a real reproducer and the applicable friction-ledger assessment, not a
privileged package exception. No signatures, ABI, performance promises or release
dates are approved here.

External consumer examples may be read; align-llm code, tests, pins and adoption
state remain external. Use Align-owned examples for any later experiments.
