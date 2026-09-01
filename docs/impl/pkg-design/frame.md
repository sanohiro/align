# pkg — frame

> English is authoritative. A synchronized Japanese mirror lives at `ja/frame.md`.
>
> **Status:** design candidate; no public contract is accepted until independent review closes.

## Authoritative public-contract ledger

This table is the authority for the first `pkg.frame` capability. Later prose and implementation
may make a field more explicit but must not widen it. V1 is one bounded inner equi-join whose output
is ordinary pipeline data; it is not a query language or a second column/schema system.

| Public surface | Exact inputs, defaults, validation, and evaluation | Exact result, errors, order, and effects | Ownership, lifetime, allocation, and cleanup | Compiler/runtime/package owner, artifact, and cache identity | Prerequisite and acceptance owner |
|---|---|---|---|---|---|
| `pub RowPair { left: i64, right: i64 }` | One flat public record. `left` and `right` are zero-based source row ordinals; negative values are never produced. There is no hidden batch, key, hash, null, or provenance field. | Copy, Pure, equality follows ordinary record-field use rather than a new whole-record equality rule. Field order and physical/source order are exactly `left`, then `right`. | Two i64 fields, no borrow, allocation, Drop, or retained input. An `array<RowPair>` is the ordinary owned dynamic AoS array and supports the existing array/slice pipeline. | `pkg.frame` owns the nominal public definition. Whole-program and per-unit interfaces serialize its name and two ordered fields; the complete definition participates in interface/dependency/cache identity. | Existing dynamic flat-record arrays; source construction/field/index/pipeline/whole-per-unit owners and exact layout checks. |
| `pub JoinError { InvalidLimit, LimitExceeded }` | One closed public tag-only sum. Source ordinals are exactly `InvalidLimit=0`, `LimitExceeded=1`; there is no payload, native code, message, alias, or ambient registry. | Copy and Pure. `InvalidLimit` means `max_pairs < 0`. `LimitExceeded` means the right-build index cannot be represented for the target, or the exact result would exceed `max_pairs`, i64 result length, or the target-representable output byte range. OOM remains a hard abort and is never converted to either variant. | The ordinary one-field `{ i32 tag }` enum aggregate; no borrow, allocation, or Drop. | `pkg.frame` owns the nominal definition and ordinals. Interfaces, checked HIR, MIR, and cache identity use the ordinary closed-enum machinery. Native status values are compiler-private and map bijectively to these two variants. | Existing tag-only sums and `Result`; exact ordinal/mapping, exhaustive match, malformed tag, whole/per-unit owners. |
| `pkg.frame.inner_join_i64(left: codec.i64_column, right: codec.i64_column, max_pairs: i64) -> Result<array<RowPair>, JoinError>` | Arguments evaluate once, left-to-right. Both columns must be source-valid live `core.codec` i64 views; their row counts may differ, either may be empty, they may share one batch/storage generation, and no equal-length rule applies. `max_pairs` has no default and admits exactly `0..=i64::MAX`. The operation compares decoded signed i64 values with settled scalar `==`; it does not compare encoded byte order. | Success returns every matching `(left row, right row)` pair in left-row-major order, with matching right ordinals ascending. Duplicate keys therefore emit the stable Cartesian product; unmatched rows are absent. Empty input or no match returns an empty array. A negative limit returns `Err(InvalidLimit)` before reading either column or allocating. An unrepresentable right-build index or the first would-be pair beyond the bound, i64 length, or target output range returns `Err(LimitExceeded)` with no output published. Pure; no I/O, global state, randomness, or source mutation. | Inputs are borrowed only for the call and never retained. Nonempty success owns one ordinary `array<RowPair>` allocation; empty success uses the canonical null/zero array and allocates no output. Transient right-side hash/index scratch is operation-owned and freed before return. Every error frees all scratch and owns no output. OOM aborts after ordinary allocator cleanup semantics. | `pkg.frame` owns the exact public wrapper and signature. Every direct, imported, local/function-field, or joined indirect call executes that ordinary wrapper body; its single call to the exact private root-module bridge forms the dedicated checked operation. HIR/MIR/LLVM preserve evaluation, status, and cleanup. Runtime owns one inactive-until-implementation keyed A121 candidate row `i32 @SYM(ptr, i64, ptr, i64, i64, ptr)` over unaligned little-endian codec bytes plus one aligned output `{ptr,len}` slot. Package source, public interface hash, HIR semantic identity, runtime registry/fingerprint, compiler build id, dependency implementation hashes, and object cache keys change at implementation; no ambient schema/artifact input exists. | Implemented `core.codec`, existing array pipeline, and existing i64/str hash substrate. Owners: exact/empty/unequal/duplicate/self-storage/order/limit matrix; direct/imported/local/function-field/joined-indirect parity; failure allocation parity; source-null and malformed-HIR; whole/per-unit/cache; runtime ABI/export and optimized/unoptimized lowering. |
| `pkg.frame.inner_join_str(left: codec.str_column, right: codec.str_column, max_pairs: i64) -> Result<array<RowPair>, JoinError>` | Evaluation, lifetime, row-count, sharing, and limit rules are identical to `inner_join_i64`. Both inputs must be source-valid live `codec.str_column` views. Equality is settled byte-exact `str == str`; UTF-8 was validated by `codec.open`, embedded NUL/LF are ordinary bytes, and there is no normalization, locale, case fold, collation, or allocation for a key copy. | The same exact stable inner-join result and error rules. Hash equality always confirms byte length and bytes, so collisions cannot create a pair. Pure and deterministic. | Inputs and all string bytes remain borrowed for the call only. The result contains ordinals, not strings, so it is self-contained and retains neither batch. Output/scratch/error/OOM rules are identical to `inner_join_i64`. | The same package/compiler identity boundary. Runtime owns one inactive-until-implementation keyed A122 candidate row `i32 @SYM(ptr, ptr, i64, ptr, ptr, i64, i64, ptr)` over validated offset/data pairs plus the output slot. It shares the i64 row's status protocol, output allocator provenance, hash-table engine, and registry activation boundary. | Same prerequisites. Owners add empty/NUL/LF/multibyte/common-prefix/collision keys, distinct-but-equal bytes across batches, and no-retained-input Drop/mutation checks. |

## Decision and scope

`pkg.frame` v1 is the smallest dynamic relational operation that the implemented codec makes useful
without creating a second language inside Align:

```text
validated typed codec columns
  -> bounded stable inner hash join
  -> ordinary array<RowPair>
  -> existing array/slice pipeline and explicit typed codec access
```

The package does not wrap a batch in another `Frame` object. `codec.batch` already owns validated
dynamic column metadata, and its typed projections already make kind checks explicit. The package
also does not select a column by string, infer a schema, materialize joined columns, or introduce a
query-plan value. Callers perform the existing explicit `find` plus typed projection, then pass the
two columns whose equality they want. This keeps missing-column/wrong-kind policy in the caller and
makes the join itself total except for its visible resource bound.

V1 supports i64 and str because they are the two equality/hash key families shared with the shipped
SoA `group_by` substrate. Bool has only two buckets and no demonstrated frame consumer; f64 would
need an explicit hash canonicalization for `-0.0` and NaN consistent with IEEE equality; neither is
admitted speculatively. There is one join direction and one output shape.

## Public use

Public calls remain fully qualified, like the other first-party packages:

```align
import core.codec
import pkg.frame

fn join_ids(
  left: codec.i64_column,
  right: codec.i64_column,
) -> Result<array<pkg.frame.RowPair>, pkg.frame.JoinError> =
  pkg.frame.inner_join_i64(left, right, 1000000)
```

The returned array is normal pipeline input; the record keeps the two row ordinals together because
that is the unit consumed by a gather:

```align
fn matched_left_rows(
  pairs: slice<pkg.frame.RowPair>,
) -> array<i64> = pairs.map(fn pair { pair.left }).to_array()
```

Declarations are shown separately from calls. No example relies on named arguments, method
overloading, implicit imports, reflection, or syntax not already accepted by the compiler.

## Exact join semantics

For left length `L` and right length `R`, the semantic result is the following source-order product:

```text
for left_ordinal in 0 .. L
  for right_ordinal in 0 .. R
    if left_key[left_ordinal] == right_key[right_ordinal]
      emit RowPair { left: left_ordinal, right: right_ordinal }
```

The implementation is a hash join, not that nested loop, but must produce identical records and
order. The right input is always the build side. It is not chosen from row counts or ambient
profiling because switching sides would change duplicate order and make resource cost
input-dependent in a hidden way. Each right ordinal joins one collision-safe chain in ascending
source order. The left input is probed twice: once to count and enforce the complete bound, then
again to fill the exact output allocation. Hash-table iteration order is never observable.

I64 values are loaded from the opaque codec column with alignment 1 and explicit little-endian
decoding, then compared as signed i64. String offsets are loaded the same way; a key is the exact
validated byte range between adjacent offsets. The existing runtime byte hash is shared, with one
fixed implementation seed and mandatory equality confirmation. Hash collisions, table capacity,
host endianness, pointer values, allocator addresses, thread count, and process configuration cannot
change result membership or order.

The exact pair bound is inclusive. An answer of exactly `max_pairs` succeeds. Detection of the next
pair returns `LimitExceeded` without allocating or publishing an output. Counting stops at that
first rejected pair; it does not scan the remaining Cartesian product merely to report a larger
number. If the exact output count fits the caller bound but `count * 16` cannot be represented as an
i64 and target allocation size, the same `LimitExceeded` is returned before output allocation.

The right index has one deterministic logical layout. For positive right length `R`, let
`Q = R + ceil(R / 3)` and let `C` be the smallest power of two at least `max(8, Q)`. The index has
i64 head and tail tables of `C` entries each plus an i64 next-link table of `R` entries; right rows
append to their bucket chain in ascending ordinal order. `Q`, `C`, and the logical scratch byte size
`16*C + 8*R` must each fit i64 and the target allocation-size domain. Any failure is
`LimitExceeded`; `R == 0` needs no index. This formula fixes the observable representability
boundary without promising whether the three logical tables share one allocation.

## Validation and error precedence

Source evaluation follows ordinary left-to-right call rules. After checked HIR has formed a call,
the runtime boundary uses one fixed pre-side-effect sequence:

1. Require a nonnull, correctly aligned writable output header and set it to `{ null, 0 }`.
2. Reject `max_pairs < 0` with the private invalid-limit status before inspecting either input.
3. Validate the left private view, then the right private view: nonnegative and target-representable
   row length; positive length implies the required nonnull data/offset range. For str, the validated
   codec producer guarantees monotonic offsets and UTF-8, but the ABI defensively requires nonnull
   offset storage for the `(rows + 1) * 4` range and a nonnull data pointer when the final offset is
   positive. No slice or typed reference is formed before its protecting arithmetic/pointer check.
4. Validate the exact `Q`, `C`, and `16*C + 8*R` right-index arithmetic above before allocating
   scratch. A source-valid right column whose index cannot be represented for the target returns
   `LimitExceeded` before allocation, even when the eventual semantic result would be empty.
5. Build the right index in ordinal order, then probe left in ordinal order and count stable matches.
   The caller limit precedes i64/output-byte representability because it is checked at each
   would-be pair; all three map to `LimitExceeded` and publish nothing.
6. Allocate the exact nonempty output once, probe again, fill records in canonical order, then
   publish `{ ptr, len }`. Empty success keeps `{ null, 0 }` and allocates no output.
7. Free all scratch before every return. No error path retains either input or a partial output.

The two package-visible errors are deliberately disjoint. A malformed compiler-private ABI returns
the existing positive `AL_INVALID`; producer-valid lowering cannot create it, so LLVM treats it as a
compiler/runtime contract violation and hard-aborts rather than misreporting a package error.
Runtime-private status `-1` maps only to `JoinError.InvalidLimit`, `-2` only to
`JoinError.LimitExceeded`, and zero publishes success. Multi-invalid direct-ABI tests pin every
boundary above, including invalid limit plus invalid inputs, left plus right invalidity, and output
range plus a later match.

## Ownership, regions, and allocation

Both codec arguments are Copy views whose region and storage-generation facts remain rooted in
their input buffers. The call borrows them for its dynamic extent. It does not move, null, mutate,
store, return, or attach either fact to the result. Moving/replacing either source owner before
argument formation remains rejected by the existing codec rules; moving it after the call is safe
because only ordinals survive.

`array<RowPair>` is an ordinary Move dynamic AoS array with representation `{ ptr, len }`, element
size 16, the target ABI alignment of `{ i64, i64 }`, `{ null, 0 }` empty representation, and the
existing null-safe array Drop. A successful nonempty call publishes one exact-size output allocation. Hash slots, chain
links, and any key metadata are transient runtime scratch; their exact allocation count and peak
ratio are not public promises, but all are bounded by the validated right row count and released
before return. Limit and defensive errors publish no output. OOM follows the settled immediate-abort
policy and never becomes `JoinError`.

The ordinary Result/array owners cover move-in, move-out, source nulling, destructuring, `if`,
`match`, `else`, `?`, `map_err`, branch and loop joins, replacement, early return, unused-result
Drop, and returned-array Drop. `RowPair` contains no view, so structs, sums, tuples, generic
monomorphization, whole-program compilation, and per-unit compilation use existing recursive Move
classification without a new carrier exception.

## Package, compiler, runtime, and ABI boundary

The vendorable package subtree owns module `pkg.frame`, its two public definitions, and its two
public functions. No `pkg.frame.internal` module or native symbol is public. Each public function is
an ordinary Align wrapper whose complete body is one call to its corresponding private root-module
bridge. Direct, imported, local/function-field, and control-joined function-value calls therefore
all invoke the same compiled wrapper; no call-site spelling selects semantics. The compiler
recognizes only each exact private bridge declaration after ordinary module resolution proves the
canonical package source, wrapper signature, and public definition graph. A same-named declaration
in any other module, or a widened/modified vendored package definition, is an ordinary function and
never selects the bridge. This is a package-owned compiler bridge, not an ambient builtin available
without the vendored package.

The canonical root module imports `core.codec` and `std.process` and owns exactly these private
bridge declarations and wrapper bodies; the unreachable abort bodies are the same source-level
placeholder pattern used by package operations whose resolved call becomes checked HIR:

```align
fn inner_join_i64_bridge(
  left: codec.i64_column,
  right: codec.i64_column,
  max_pairs: i64,
) -> Result<array<RowPair>, JoinError> = process.abort()

fn inner_join_str_bridge(
  left: codec.str_column,
  right: codec.str_column,
  max_pairs: i64,
) -> Result<array<RowPair>, JoinError> = process.abort()

pub fn inner_join_i64(
  left: codec.i64_column,
  right: codec.i64_column,
  max_pairs: i64,
) -> Result<array<RowPair>, JoinError> =
  inner_join_i64_bridge(left, right, max_pairs)

pub fn inner_join_str(
  left: codec.str_column,
  right: codec.str_column,
  max_pairs: i64,
) -> Result<array<RowPair>, JoinError> =
  inner_join_str_bridge(left, right, max_pairs)
```

The compiler admits the bridge discriminator only at the corresponding wrapper's complete
single-expression body. Any other call position, helper body, wrapper shape, function value of the
private bridge, or altered declaration rejects package admission rather than executing the abort.
The public wrappers themselves remain ordinary callable values.

Sema owns exact private-bridge recognition, wrapper/argument/result types, evaluation order, purity,
input-region liveness, and the two checked expression discriminators. Checked-HIR validation independently
recomputes the canonical package definition identity, both input kinds, result record/error identity,
fallthrough, and region/no-retention facts. MIR owns status mapping, exact output type, ownership,
failure cleanup, and target-independent little-endian semantics. LLVM owns the typed native calls,
output slot, guarded success/error reconstruction, source nonnull/range attributes only where
proved, and output allocation provenance. Runtime owns the shared hash/index engine, count/fill
passes, scratch/output allocation, collision equality, limit enforcement, and cleanup.

The planned rows remain inactive during design:

| Candidate row | Exact symbol | Candidate ABI shape | Exact Rust ABI |
|---|---|---|---|
| A121 | `align_rt_frame_inner_join_i64_v1` | `i32 @SYM(ptr, i64, ptr, i64, i64, ptr)` | `unsafe extern "C" fn(*const u8, i64, *const u8, i64, i64, *mut AlignStr) -> i32` |
| A122 | `align_rt_frame_inner_join_str_v1` | `i32 @SYM(ptr, ptr, i64, ptr, ptr, i64, i64, ptr)` | `unsafe extern "C" fn(*const u8, *const u8, i64, *const u8, *const u8, i64, i64, *mut AlignStr) -> i32` |

Both are C calling convention and `nounwind`, with no curated memory, return, or parameter
attribute until implementation proves one against the final bodies. The final output pointer is an
aligned writable `AlignStr` header, not an Align source `str`; its pointer names an allocation of
16-byte `RowPair` elements and its length is an element count. The two symbols, keyed rows,
checked-HIR discriminators, package interface, and owners activate atomically. Until then they do not
enter `RuntimeKey::ALL`, collision reservation, declaration/export totals, ABI fingerprints, or
compiler capability identity. With current codec totals, activation would move keyed/base/
maximum-optional-probe inventories from 328/345/353 to 330/347/355 and make A123 the next
unreserved shape; implementation must recompute rather than trust these planned totals.

## Complexity and performance boundary

The implementation is a right-build hash equi-join with work proportional to right build rows,
left probes, confirmed collision bytes, and emitted pairs under ordinary hash distribution. It is
not a nested-loop implementation and never sorts output. Adversarial hash collisions may increase
equality work; correctness and the explicit pair bound remain intact. No throughput, latency,
allocation-count, peak-byte ratio, SIMD width, or parallel scaling number is a public promise.

`bench/frame_join` records non-gating local evidence for one-to-one i64, duplicate-fanout i64,
equal-byte str, and collision-heavy str corpora, including build/probe/output row counts and peak
scratch/output bytes. The benchmark exists to catch an accidental nested-loop or copied-string
implementation before review; acceptance is semantic and does not use a timing threshold.

## V1 non-goals and later boundaries

V1 has no `Frame` wrapper; batch construction; schema reflection; name-based column selection;
projection/materialization; filter; sort; aggregate; group-by wrapper; query DSL; expression tree;
lazy execution; optimizer; planner; SQL; file/mmap/stream input; output codec encoder; mutable batch;
nullable key; composite key; bool/f64 key; outer/left/right/full/semi/anti/cross/as-of join; join
predicate closure; parallel join; spill; distributed execution; or automatic build-side choice.

Those are not hidden implementation hooks. A later capability must name a real consumer and add one
new complete ledger. Bool/f64/composite/null key support must first fix equality/hash/null semantics.
Materialized joined columns must choose explicit ownership and allocation rather than smuggling
copies behind `RowPair`. A future parallel or spill implementation must preserve the exact stable
result order or deliberately replace this pre-release contract in one pass.

## Implementation closure matrix

Implementation may begin only after independent review accepts this ledger. The capability crosses
package formation, sema, checked HIR, MIR, LLVM, runtime ABI, and owned-array construction, so the
author-side matrix is mandatory before coding.

| Axis | Required implementation closure | Exact owner evidence |
|---|---|---|
| Public formation and identity | Canonical vendored `pkg.frame` only; exact two records/two public wrappers/two private root bridges; no ambient/same-name interception; ordinals and field order; whole/per-unit and generic signature reconstruction. Every direct, imported, local/function-field, and control-joined indirect target executes the wrapper and reaches exactly one corresponding bridge action. | Package import/name/signature positives and wrong-module/type negatives; exact public wrapper/private bridge body and interface bytes/hash; parameterized direct/imported/local/function-field/joined-indirect parity; same-name control; malformed public definition/bridge rejection. |
| Input region and evaluation | Left, right, limit evaluate once in order; every terminating child stops later formation/action; direct/field/Option/Result/control-selected codec views retain complete input generation through the call and no longer. | Parameterized direct/control/termination and source-invalidation owner across i64/str, whole/per-unit, and malformed HIR. |
| Join products | Zero/one/many rows; unequal lengths; no-match; one-to-one; left/right duplicates; stable Cartesian order; shared/same batch; every i64 edge and str byte class; collision confirmation. | Independent nested-loop oracle over fixed and generated bounded fixtures; mutation of pair membership/order; base-alignment 0..7 and endian-lowering twins. |
| Limit and validation precedence | Negative, zero, exact, rejected-next, right-index load-factor/capacity/byte overflow, i64/output-byte overflow; output slot then limit then left/right private views then scratch arithmetic then build/count/output; no partial publication. | Direct runtime multi-invalid matrix; exact/rejected-next source owners; target-representability twins including empty semantic output; failpoint after every scratch/output acquisition; output header remains null/zero on every error. |
| Output ownership and control | Exact `{ptr,len}` `array<RowPair>` reconstruction; empty/nonempty Drop; move-in/out/nulling; replacement; return; destructure; `if`/`match`/`else`/`?`/`map_err`; branch/loop joins; early exit; unused success/error. | Existing recursive-array Drop counters plus focused driver control matrix and MIR cleanup/source-null assertions. |
| Native ABI and allocation | Exact A121/A122 declarations/exports/statuses, nonnegative lengths, null/alignment, unaligned LE reads, output allocator/free provenance, no unwind, every acquisition cleanup, no retained input/global state. | Runtime ABI registry/export/attribute mutation owner; direct malformed ABI; cumulative allocation/free parity; optimized/unoptimized and rt-LTO link. |
| Hash engine | One shared engine policy; fixed seed; right ordinal chain stability; collision equality; no key byte copy; two probes produce identical counts/order; scratch bounded by right rows and released. | Shared i64/str engine units, forced-collision fixture, pass counters, no-string-copy allocation probe, count/fill parity assertion. |
| Compatibility and cache | Codec bytes/API unchanged; SoA/group_by/hash and array pipeline unchanged; package absence unchanged; implementation invalidates exact package/compiler/runtime identities once; no source/artifact/environment inputs. | Codec/group_by/pipeline focused controls; package add/remove and source edit/revert cache twins; runtime fingerprint and whole/per-unit object/link parity. |

## Sources of truth and author consistency pass

This English ledger, `docs/impl/pkg-design/ja/frame.md`, `draft.md`,
`docs/language-spec.md`, `docs/design-notes.md`, `docs/history.md`,
`docs/open-questions.md`, `docs/impl/07-roadmap.md`, `docs/impl/19-hir-validation-ledger.md`,
`docs/impl/20-runtime-abi-ledger.md`, and `HANDOFF.md` must agree before implementation.

Author-side pass for the design candidate:

- every public argument/result has one exact type, evaluation order, default, ownership, lifetime,
  allocation, cleanup, error, and effect rule;
- the i64/str × empty/nonempty × unique/duplicate × limit/error product has exact membership,
  row order, and unavailable-output rules;
- all text is already validated UTF-8, embedded NUL is data, equality is byte-exact, and no native
  boundary retains a borrowed range;
- every multi-invalid input has one validation order and package error precedence;
- there is no ambient configuration, schema reflection, artifact/source I/O, hash seed, adaptive
  build-side choice, or target-dependent result;
- both native signatures fix scalar widths, pointer roles, output initialization, status mapping,
  malformed-input behavior, allocator provenance, and activation identity;
- runtime inspection consumes only compiler-formed codec views and never reads source, interfaces,
  or artifacts;
- examples use accepted syntax and separate declarations from call expressions; and
- acceptance owners cover every ledger invariant, while the benchmark is local evidence for the
  named hash implementation class and not a correctness gate.
