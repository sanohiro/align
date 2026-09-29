# Proactive compiler audit — 2026-09-26

Status: reproduced A1–A4 cases corrected and locally verified; D1 wording
corrected. K1 remains the explicitly unshipped plan 61 capability.

## Corrective validation

- Driver owners: `array_truncate`, `constants`, `m1`, `borrow_liveness`,
  `borrowed_params`, `disjoint_field_borrows`, and `imported_mutable_retention`
  passed (152 tests across the seven targets). Ownership negatives are
  compile-only and compare whole/per-unit checking.
- `scripts/test-pr.sh`: all 16 bounded-gate binaries passed. The first candidate
  exposed the runtime-key ordering finding below; the corrected gate passed.
- `CARGO_TARGET_DIR=target/clippy scripts/cargo.sh clippy --workspace --lib --bins`
  passed, as did `scripts/lint-ratchet.sh` and `git diff --check`.
- `scripts/test-runtime-abi-exports.sh`: native signatures and all five feature
  export sets passed (466 base, 473 allocation-probe, 470 parallel-probe,
  466 task-probe, 477 combined).
- `scripts/cargo.sh build --release --workspace` passed. A smoke test using that
  compiler at dev and release printed imported `f32` cancellation as `0.0`,
  the maximum u64 value as `18446744073709551615`, and the retained truncated prefix correctly.
- The new bilingual performance guide and this report passed a focused local-link
  and fence check; the specification and digest contain no stale one-instruction
  guarantee.

## Implementation closure matrix

This repair implements the existing plan 66 exclusive-action contract and the
existing scalar arithmetic/display contracts. It introduces no source syntax,
ownership model, IR variant, or allocation policy. Plan 61's general interprocedural
read-only authority remains a separate unshipped capability.

| Boundary | Implementation obligation | Regression owner |
| --- | --- | --- |
| Truncate formation and receiver completion | Preserve dynamic-array/mutable-place/i64 validation; reserve the exact completed receiver before the count; diagnose moves, replacement and overlapping writes during later operands. | `array_truncate`: existing formation tests and receiver-reservation cases. |
| Truncate successful action | End prior overlapping observations, including retained-prefix and same-length views, borrowed strings and records, and eager operand snapshots; permit last-use-before-action and independent sibling storage. Keep the array usable with its new length. | `array_truncate`: compile-only old-view negatives and positive sibling/reborrow controls. |
| Control flow and calls | Apply the action only on fallthrough; preserve facts through branch/loop joins and early exits; transport the same effect through direct/imported and generic helpers in whole/per-unit checking. | `array_truncate`: parameterized local/imported cases and control-flow cases. |
| Caller record fields and addressed backing | A symbolic `CallerStorage` field path identifies a descriptor place, not the allocation addressed by a view in that field. Discard a shared caller root for disjoint fields only when both reached generations are authenticated owned-storage headers with distinct identities; a view header, missing directory entry, joined/unknown source or copied descriptor retains the possible alias. Apply this rule to locals and eager argument/result snapshots as well as direct uses. | `array_truncate`: alias stored beside its owner through `borrow mut Aliased` rejects for saved, projected and eager uses in both checking modes; two owned sibling arrays and their derived views stay usable. |
| Header release versus contained borrows | Resolve an owned header's release root separately from recursively reachable element/content dependencies. A borrowed element, view descriptor, unknown nested header or missing directory/content entry prevents dropping the common caller root merely because it also names the header release. Never use a flattened root set to subtract both meanings at once. Independent owned numeric siblings still pass; an owned array of views into a truncated sibling's owned elements rejects. | `array_truncate`: `borrow mut` record with `array<string>` owner and direct `array<str>` or nested record view elements rejects in whole/per-unit checking; existing independent owned-array sibling stays accepted. |
| Ownership and cleanup | Receiver remains owned; no move-out or source nulling at the action; existing suffix Drop order, outer storage, allocation mode and later replacement/return behavior stay unchanged. | Existing `array_truncate` prefix, repeated, field-path, suffix-string/record and borrow-mut owners. |
| Constant arithmetic | Round f32 literals and every f32 operation at its own width before memoization, comparison or aggregate construction; preserve f64 and IEEE non-finite behavior. No runtime allocation or ABI change. | `constants`: scalar, comparison, aggregate, overflow, underflow, signed-zero and f64 controls; imported constants and dev/release lowering. |
| Exported constant source | Capture the complete initializer token span, including leading grouping delimiters, before serializing its existing source field. Re-fold the same expression in importers. No interface record shape changes. | `constants::f32_constants_round_at_each_literal_and_operation`, whole/per-unit and dev/release. |
| Unsigned display ABI | Select unsigned decimal formatting for unsigned values; add one `u64` scalar runtime entry with `void(i64)` ABI, HostState effects, no retained inputs, releases or source-visible allocation. Update the finite registry, golden declarations and ABI ledger together. | Integer display owner at signed/unsigned boundaries; MIR/runtime ABI registry and export checks. |
| SIMD wording | Describe lane-wise semantics without an instruction-count guarantee; preserve explicit vector semantics and target-dependent lowering. | One focused documentation consistency check. |

Move-in/out, Drop, return cleanup, monomorphization and interface record shapes
are unchanged by the numeric fixes. Truncate consumes the existing storage and
call-effect analysis; it must not add a parallel provenance scheme or claim to
ship plan 61. No benchmark is required: the repair makes no performance promise.

The corrective capability keeps the four finite audit findings and their exact
owner tests together. Its handwritten diff exceeds roughly 1,000 lines because
the same settled contracts are reconciled in the specification, learner guides,
English/Japanese mirrors and this evidence record. Splitting those documents
from the fixes would publish examples and summaries against the known broken
compiler, while splitting the corrections would repeat the shared ownership and
ABI checks at each publication boundary. The code changes and their owner tests
remain substantially smaller than the documentation and evidence updates.

The author-side matrix pass maps receiver reservations to
`prepare_mutable_call_snapshots` and the truncate action's pre-mutation
`validate_value_snapshot`; existing eager worklist completion prevents an action
after a terminating count. `invalidate_roots_except_local` resolves header-backed
observations as well as legacy roots and preserves the exact destination subtree;
an alias in a sibling record field still ends. The native truncate lowering and
Drop plans are unchanged. Existing imported exclusive-call transitions use the
same observation invalidation, preserving their retention summaries rather than
pretending truncation replaces the retained element contents.

The local owner covers direct/same-length/string/record/Option views, eager
operands, receiver move/replacement/nested mutation, branches and loops. Imported
helper negatives have a matching fresh-view positive in both checking modes.
Existing `borrow_liveness`, `borrowed_params`, `disjoint_field_borrows`, and
`imported_mutable_retention` owners qualify the shared analysis. General
source-qualified access, unknown observers, native access-certificate publication
and the complete call/control product remain plan 61's explicitly unshipped
boundary; this repair does not attest completion of that plan.

### Independent review closure

One inspection-only review found four valid issues in the corrective candidate.
The author verified and addressed the complete set together:

| Finding | Correction | Owner |
| --- | --- | --- |
| A mutable slice argument incorrectly invalidated its separate backing owner. | Distinguish an owned header's retained allocation from its contained borrowed dependencies. Completed eager operands still observe the allocation. | `truncate_preserves_fresh_views_siblings_and_terminated_operands`: owner and passed view stay usable; `truncate_excludes_old_observations_in_both_checking_modes`: another old view is rejected. |
| Different fields of one borrowed record flattened to the same parameter root. | Match authenticated caller storage generations before flattening; end the observing header leaf. Header-free and unknown observers remain conservative. | The same positive owner checks saved and fresh sibling views through `borrow mut Pair`; the negative owner checks an alias stored beside its owner in one record. |
| The new runtime key violated logical-name ordering. | Put `PrintU64` after `PrintStr` and synchronize declaration order. | `runtime_keys_are_complete_unique_and_alphabetical` and the runtime declaration golden. |
| Two feature-specific ABI counts remained stale. | Record 473 allocation-probe and 470 parallel-probe exports alongside 466 base and 477 maximum exports. | `scripts/test-runtime-abi-exports.sh`. |

The committed-candidate review found one further P1: an imported or local
`borrow mut` record can carry a view field that aliases an owned array field.
The first correction treated their distinct symbolic `CallerStorage` paths as
proof of disjoint backing. The witness is accepted in whole and per-unit
checking, including when the caller stores the alias beside its owner. This
reopens the caller-field/backing axis above. The revised boundary requires
producer-owned evidence that each compared generation denotes an owned backing;
the type of a borrowed view field cannot provide it. The distinct-owned-sibling
positive owner remains necessary, so a root-wide conservative rejection
is not a complete repair. An inline fixed-array sibling remains conservative:
admitting it at this sema seam currently reaches an owned-leaf provenance
rejection in the MIR producer, so it is outside this correction's proven
boundary. The review log is bound to `3a759e05`; the correction
and its owner test belong in one coherent fix commit.
The saved-view and eager-argument witnesses both pass on the reviewed compiler
before this fix, then fail with the intended invalidated-observation diagnostic.

The revised-diff review found a second P1 and reopens the header-content axis:
`array<str>` can own its outer array while its elements borrow strings from a
sibling `array<string>`. Both the release and contained dependency flatten to
the caller parameter root. Subtracting the release root from the flattened fact
also erased the dependency, so truncating the sibling left its old view usable.
The correction must inspect the generation contents before deciding whether an
owned header's caller root represents only its release. Missing or unresolved
content remains conservative; a plain owned numeric sibling still needs to pass.
Both direct and nested contained-view witnesses pass on the compiler before this
correction and reject with an invalidated-borrow diagnostic afterward.

### Nightly follow-up: mutable Copy-view call effects

The first nightly after the A1 repair found four `return_provenance` positives
that reject after a changed `borrow mut` slice header or element. A candidate
that simply exempted changed Copy views from exclusive root invalidation made
both of these source programs check, which is unsound:

```align
fn reset(borrow mut destination: slice<str>) {
  destination = []
}
fn main() -> i32 {
  mut owner := "old".clone()
  view: str := owner
  mut values := [view]
  mut destination: slice<str> := values
  alias := destination
  reset(destination)
  owner = "new".clone()
  _ := alias[0].len()
  return 0
}
```

The same false acceptance occurs when `reset` instead writes
`destination[0] = "static"`, `values` has a second element rooted in `owner`, and
the final read uses `alias[1]`. The first program rebinds only the descriptor;
the second leaves one old element. An empty mutable-retention root set cannot
distinguish either from a complete content overwrite. The reviewed candidate
is parked and must not be published.

| Boundary | Closure obligation | Owner evidence |
| --- | --- | --- |
| Formation, validation, move and Drop | Keep the existing `borrow mut` place, mode, alias and storage checks. A Copy-view descriptor never owns or frees its addressed allocation; an owned array or resource still ends its displaced generation and runs its existing Drop/nulling plan. Reject malformed effect metadata before a call transition. | Existing `borrowed_params`, `borrow_liveness`, `array_truncate`, `resource_ownership` and malformed-HIR owners; new direct-versus-owned twins. |
| Descriptor versus content | Distinguish no-op, descriptor-only rebind, original-backing write, temporary-rebind-target write, and their combination on every returning path. Rebinding preserves old backing content for pre-call aliases while the destination selects its post-call backing. Every source written through a transient view remains in all compatible completed backing candidates even if a later rebind removes that view from the exit value. An unknown effect conservatively retains old content. | `return_provenance` no-op, copy-view-rebind, static-reset, sourced-write and rebind-write-rebind twins; imported and function-value parity. |
| Exact overwrite versus partial write | Remove an old element owner only when every successfully returning path proves its entire observed backing content was overwritten. A write to one element of a two-element backing leaves the other owner's dependency live; an offset/range or unknown backing cannot claim an exact index. A proved single-cell backing may use a guaranteed successful whole-element store as a complete overwrite; a one-row SoA field store cannot erase sibling fields. | Parameterized one/two-element, full/ranged slice, constant/dynamic index, projected-field, no-write and abort/return cases; old-owner replacement plus alias read. |
| Control and eager action | Apply effects only after every argument falls through. Join `if`, `match`, `else`, `?`, `map_err`, loop, and early-return paths with may-effects by union and must-overwrite by intersection. Preserve source-order snapshots, simultaneous mutable destinations, nested direct calls and action-time overlap exclusion. | Existing argument-completion and multiple-destination owners; new branch/loop/termination and mixed-effect owners. |
| Generic, imported and indirect calls | Infer direct/concrete generic effects from checked bodies; transport the exact public effect through producer-certified interfaces and validate it on import/replay. Missing, external or unresolved targets use a conservative effect, never known-empty or guaranteed overwrite; malformed records reject before analysis. Whole-program and per-unit checking must agree. | `return_provenance` and `imported_mutable_retention` whole/per-unit matrix; interface semantic/byte and malformed-record owners; generic forwarding and unresolved-target negatives. |
| Allocation, ABI and cache | Preserve runtime ownership and allocation, MIR/LLVM call ABI and source syntax. If the interface record changes, version and hash its canonical bytes before implementation; no implementation-only summary may be trusted as an imported fact. | Existing allocation/ABI owners, interface hash/edit-revert owner, bounded gate; no benchmark without a new resource promise. |

Before implementation, the owning contract ledger must define the exact effect
record, its validation and encoding, and the source-level Copy-view alias rule.
The revised matrix and capability boundary require one fresh adversarial plan
review. The two false-acceptance cases above remain rejection controls while
the previous candidate's four nightly positives remain acceptance controls.

The 2026-09-29 independent plan review found two soundness gaps and three
contract gaps. A temporary rebind target can retain a written source after the
destination is rebound again, so plan 80 now records writes throughout the
destination's header flow and joins their sources into all compatible completed
backings. A one-row SoA field write cannot clear sibling fields, so its must-write
bit requires a whole-element store. Malformed `Some` records now reject instead
of using unavailable fallback; resolved nominal eligibility is checked after
the full interface and hash; and a rebind to a fresh `clone_in(out)` view retains
an unknown header with the selected region root. The corresponding matrix rows
above and plan-80 validation, transfer, and owner records close these findings
before Rust implementation.

## Scope and evidence

Audited compiler commit `c2f32a2d213fcba6678f7c4dfeca5dac11e308a9` on Darwin arm64.
Rebuilt the compiler with `scripts/cargo.sh build --release -p align_driver`;
the executable reports `alignc 0.8.1`. Existing uncommitted documentation changes
were preserved. No compiler, runtime, consumer-repository or test source was
modified by this investigation.

The audit used the recent request classes to select adjacent cases, rather than
only rerunning each reported client witness: source-expression composition,
whole/per-unit checking, read-only and mutable storage, numeric constant/runtime
parity, loop code generation, and sequential/parallel results. Existing plans,
owner tests and the request register distinguish new findings from known gaps.
This is a finite local audit, not certification of the entire compiler or all
supported platforms. No throughput improvement or regression is claimed.

## Findings and priority

| ID | Priority | Classification | Finding |
| --- | --- | --- | --- |
| A1 | P1 | Newly reproduced contract failure | `array.truncate` accepts surviving views and receiver consumption/replacement during its length argument. |
| A2 | P1 | Newly reproduced wrong result | `f32` constant evaluation uses `f64` arithmetic, including intermediate results and comparisons. |
| A3 | P2 | Newly reproduced output error | `print(u64)` formats values at or above 2^63 as signed integers. |
| A4 | P1 | Found by repair regression | Exported parenthesized constant initializers can lose their opening delimiter, changing the value re-folded by a per-unit consumer. |
| K1 | P1 | Known, still unresolved | Ordinary slice argument/result flow can lose read-only permission; typed byte views compose with the same gap. Plan 61 explicitly remains unshipped. |
| D1 | P3 | Documentation overclaim | The specification promises one hardware instruction for each vector arithmetic operation; integer vector division disproves it. |

Prioritize the storage failures and numerical wrong results before extending
their surfaces. K1 belongs to the existing plan 61 implementation boundary;
rediscovering it does not create a second design or establish a regression in
`view_le` itself. A1 needs the missing plan 66 action invariants closed, including
their dependency on the existing access-analysis boundary.

## A1 — truncate does not enforce its exclusive action contract

[Plan 66](66-array-prefix-and-text-boundary-plan.md#t-exclusive-prefix-mutation-and-cleanup)
requires reserving the receiver before evaluating the length, then excluding any
overlapping view that survives the action. This explicitly includes a same-length
call and does not permit a retained-prefix exception.

This negative witness is accepted by both `check` and `check-per-unit`:

```align
fn main() -> i32 {
    mut a := [10, 20, 30].to_array()
    view := a[..]
    a.truncate(1)
    return view[2] as i32
}
```

Both dev and release builds succeed; the retained observation returned `30`.
The outer allocation remains live, so that observation alone does not prove a
freed-memory read. It does prove that the required old-view rejection is absent.
An analogous `array<string>` witness retains `text := a[1]` across removal of
that element and is also accepted. Its `text.len()` returns the old header length;
this does not dereference the released payload and must not be represented as a
runtime payload-read test. The source lifetime obligation is already violated.
A helper taking `borrow mut array<i64>` and performing the truncation also leaves
the caller's old view accepted.

The receiver reservation has a separate negative witness:

```align
fn consume(a: array<i64>) -> i64 = 0

fn main() -> i32 {
    mut a := [10, 20, 30].to_array()
    a.truncate(consume(a))
    return 0
}
```

Both checking modes and dev/release builds accept it. Replacing the receiver in
the length expression is accepted too. Native runs that return normally do not
make these programs valid: plan 66 requires their rejection before lowering.
Further negative qualification should be compile-only.

Implementation evidence: `check_array_truncate` in
[`align_sema/src/lib.rs`](../../crates/align_sema/src/lib.rs) checks type, local
mutability and the i64 count. The MoveCheck arm visits the borrowed receiver and
then the count. `source_visible_mutation_action` classifies truncation as an
ordinary `Collection` write; `invalidate_collection_mutation_target` handles
validated bytes and place observations, without closing the stronger suffix-Drop
and receiver-reservation obligations demonstrated above. Search anchors are
`check_array_truncate`, `ExprKind::ArrayTruncate`, and
`invalidate_collection_mutation_target` (audited lines 58657, 45708, 43428 and 40595).

The existing [`array_truncate.rs`](../../crates/align_driver/tests/array_truncate.rs)
owns prefix values, suffix cleanup, invalid counts and formation errors. Add
negative owners for surviving scalar/string/record views, no-op truncation,
direct/imported helpers, and terminating or receiver-changing length expressions;
pair them with views whose last use precedes the action and independent siblings.
Passing existing prefix tests is not evidence for these missing cells.

## A2 — f32 constants do not round at f32 operations

Save this complete positive program as `f32_constant.align`:

```align
C: f32 := (16777216.0 + 1.0) - 16777216.0

fn calculate(x: f32) -> f32 = (x + 1.0) - x

fn main() -> i32 {
    print(C)
    print(calculate(16777216.0))
    return 0
}
```

Both checking modes accept. Both dev and release print:

```text
1.0
0.0
```

The strict f32 expression must round the addition before the subtraction; neither
expression grants reassociation. This is not an allowed elementary-math ULP
difference. The equivalent f64 control agrees at `1.0`.

The same class was reproduced in f32 constant-array elements and constant
comparisons. With `A: f32 := 16777217.0`, a constant comparison to `16777216.0`
disagrees with the runtime comparison of `A`. The overflow witness
`C: f32 := (1.0e38 * 4.0) / 4.0` produces a finite constant while the corresponding
strict runtime expression produces infinity. Thus the defect can change value
classification, not only the last bit.

Cause: `ConstEval::expr` retains an f32-tagged literal in `ConstVal::Float(f64)`
without f32 normalization. `ConstEval::binary` matches `Ty::Float(_)` and performs
the host f64 operations without using the width. `compare` then observes those
unrounded values. The repair must normalize literals, intermediate arithmetic and
all consumers, rather than only casting the final constant. Source anchors:
[`align_sema/src/lib.rs`](../../crates/align_sema/src/lib.rs), audited lines
7408, 7655–7667 and 7675. Owners should cover scalar/aggregate/comparison forms,
overflow, and f64 controls in whole/per-unit and dev/release compilation.

## A3 — unsigned integer printing selects the signed runtime entry

```align
fn main() -> i32 {
    high: u64 := 9223372036854775808
    top: u64 := 18446744073709551615
    print(high)
    print(template "{high}")
    print(top)
    print(template "{top}")
    return 0
}
```

Both dev and release print:

```text
-9223372036854775808
9223372036854775808
-1
18446744073709551615
```

[The display contract](../../draft.md#print-and-value-display) requires the same decimal formatting
for `print` and interpolation. In `gen_print`, all integer widths route through
`RuntimeKey::Print`, which calls `align_rt_print_i64`; zero extension only helps
unsigned types narrower than 64 bits. Source anchors:
[`align_codegen_llvm/src/lib.rs`](../../crates/align_codegen_llvm/src/lib.rs),
audited lines 20709–20725, and
[`align_runtime/src/lib.rs`](../../crates/align_runtime/src/lib.rs), line 50.

The owner should compare direct printing and interpolation at 2^63−1, 2^63,
u64 maximum, and signed/narrow-unsigned controls. Existing float-bit tests that
explicitly cast to i64 correctly expect signed output; they are not evidence
that unsigned printing is intended to behave this way.

## A4 — exported initializer spans lose grouping delimiters

The new A2 owner exposed a separate whole/per-unit discrepancy. For
`pub C: f32 := (16777216.0 + 1.0) - 16777216.0`, interface extraction read
`16777216.0 + 1.0) - 16777216.0` from the initializer's expression span.
Parentheses have no AST node, so the compound span began at its first inner
operand. The consumer re-folded a different partial expression; fixing f32
arithmetic alone left `C` unequal to zero in the per-unit executable.

`Parser::parse_const` now captures the complete consumed initializer range before
statement termination. The existing interface source field retains its shape and
includes every grouping delimiter. The A2 imported scalar/array, overflow,
underflow and f64 controls now run through both whole-program and per-unit native
builds, with dev/release coverage on the latter.

## K1 — known interprocedural read-only gap remains present

[Plan 52](52-readonly-view-provenance-plan.md#capability-boundaries) separates local
origin propagation from ordinary function argument/result obligations.
[Plan 61](61-interprocedural-view-access-plan.md) explicitly records the latter as
an unshipped implementation. The current compiler still accepts a writable local
bound to the return of a plain slice identity function whose input is literal
text bytes. Returning `Option<slice<u8>>` has the same issue.

The typed-view variant also passes both checking modes:
`fn view(raw: slice<u8>) -> Option<slice<u32>> = raw.view_le()` followed by a
write through the unwrapped result of `view("abcd".bytes())`. The retained early
probe logs record SIGBUS in dev/release on this host. After classifying it under
plan 61, no further negative native runs were performed; the plan's negative
owners are compile-only.

The direct local `"abcd".bytes().view_le()` write is correctly rejected, and a
view derived from an owned mutable buffer is accepted. These controls locate the
gap at ordinary call flow rather than the local conversion's write check. The
typed-view contract must not be read as proof that plan 61 has shipped.

## D1 — vector-shaped operations do not imply one machine instruction

At the audited commit, the fixed-vector section of [draft.md](../../draft.md#fixed-vector-types) said
elementwise `+ - * / %` map to one lane-wise hardware instruction each.
The exported kernel below contradicts that unconditional statement:

```align
pub fn divide(a: vec4<i64>, b: vec4<i64>) -> vec4<i64> = a / b
```

`emit-obj --export divide --profile release --target-cpu baseline --no-rt-lto`
followed by `otool -tvV` shows four scalar `sdiv` instructions, guards and lane
transport on arm64. Results are correct. The existing SIMD guide already uses
the accurate target-dependent wording; the specification should describe
lane-wise semantics without promising one machine instruction. This is not a
benchmark or a claim that the generated code is unnecessarily slow.

## Checks that did not find a defect

- A 57-program source matrix compared `check` and `check-per-unit`: 33 admitted
  positives executed successfully at dev, 18 malformed inputs produced ordinary
  diagnostics, and six cases hit existing documented restrictions. Covered
  fixed-array call/control carriers, string/NUL and integer/character match,
  typed-view and math receiver expressions, structural masks and float scopes.
  No ICE or checking-mode divergence was observed in this matrix.
- Twelve contrasting numeric kernels matched expected outputs at dev/release/fast.
  Inspection included value/borrowed slices, offsets, stride and reentry, early
  exits, byte scans, strict/relaxed arithmetic and typed views. Imported small
  bodies inline and retain strict/scoped floating-point boundaries. Four trap
  and zero-trip controls preserve status, printed prefix and diagnostic.
- Six parallel shapes over seven lengths (`0`, `1`, `7`, `65535`, `65536`,
  `65537`, `131075`) matched an independent Python value/order digest across
  five configurations: dev, release, release without runtime LTO, fast, and fast
  without runtime LTO. Shapes include direct maps, multiple filters/maps,
  struct-field projection, chunks, filtered chunks and nested parallel maps.
  This is 210 shape/size/configuration comparisons, not a timing benchmark or
  proof that each runtime call used worker threads.
- Signed `INT_MIN / -1` constant/runtime controls retain the specified wrap.
  Conservative vectorization decisions were not counted as defects without a
  violated promise. Requests 121 and 122 remain existing known items, not new
  findings from this audit.

## Reproduction and retained evidence

For the positive numeric/display programs above, use the rebuilt compiler:

```text
target/release/alignc check /absolute/path/example.align
target/release/alignc check-per-unit /absolute/path/example.align
target/release/alignc run /absolute/path/example.align --profile dev --no-rt-lto
target/release/alignc run /absolute/path/example.align --profile release --no-rt-lto
```

Use only the first two commands for negative ownership/read-only witnesses.
The examples above preserve the relevant source independently of temporary logs.
Detailed command arrays, outputs and probes remain in these private local
directories under `/var/folders/_9/bntwz1dj7q3bfbssg25w0myw0000gn/T/`:

| Directory | Evidence |
| --- | --- |
| `align-ownership-audit-nn7fv5e_` | 14 ownership cases; exact-HEAD check and build records. One audit agent stopped before its final narrative; completed JSON records were retained and inspected. |
| `align-audit-confirm-1uggog3u` | Independent compile-only permission/truncation controls and numeric confirmations; `unsigned_print/confirmed.json` supersedes the initial literal-string comparison. |
| `align-constant-audit-vpbu5k9w` | f32 arithmetic/comparison/aggregate/overflow witnesses, f64 and integer controls. |
| `align-surface-audit-oeflyk2p` | 57-program source/diagnostic matrix. |
| `align-opt-audit-i1rwf5y_` | Numeric corpus, cross-unit controls, unsigned output, exported vector division and disassembly. |
| `align-parallel-audit-ysvj14kf` | Parallel corpus, independent expected digest and five build/run records. |

No full-workspace suite, service-dependent suite or cross-platform execution was
used. Fixes require their own implementation boundary, focused owner checks and
normal repository review flow; this report is evidence for that work, not a
preflight attestation.
