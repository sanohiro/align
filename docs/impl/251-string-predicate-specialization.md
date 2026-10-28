# Constant string predicate specialization

Status: implemented R97 follow-up, 2026-10-28. Plans20/70 continue to own the
runtime ABI and rt-LTO admission contract. This is a representation-preserving
implementation change; no language, API, attribute or admission rule changes.

## Evidence and boundary

The current LLM ARM64 acceptance retains prefix/suffix calls after the original
target-attribute defect was repaired. Its source-file classifier supplies a
live sequence of constant suffix tests. Media's model-name/codec admission and
voice's tokenizer/asset admission use the same predicates. All three request
registers and those uses were inspected; consumer source and pins stay external.

An independently authored six-way classifier at main `99f90eab` retains two
`align_rt_str_ends_with` calls on macOS arm64 at the default release/baseline
target. The shared Rust primitives independently normalize two nullable ranges
through `safe_slice`. LLVM retains conditional lengths and substitute pointers
inside the comparison, making specialization more expensive.

Resolve the null pair before constructing the two slices. For a positive
needle that fits the haystack, one null pointer compares unequal and two null
pointers compare equal, matching the existing empty-slice normalization wherever
the old operation is defined. Non-null inputs still use `safe_slice` and ordinary
slice byte equality. In `ends_with`, resolve nulls before forming the tail pointer.
Empty/nonpositive needles and overlong needles retain their earlier precedence.
The public unsafe entrypoints still require valid initialized nonempty ranges;
this does not admit arbitrary pointers or promise a new invalid-input API.

The two predicates are one capability because they share this comparison
structure and the artifact/staticlib source. Equality and ASCII-case equality
remain unchanged. There is no `alwaysinline`, `inlinehint`, custom pass order,
new runtime symbol, unchecked load, allocator or CPU-dependent dispatch.

## Implementation closure matrix

| Boundary | Implementation and invariant | Owner |
| --- | --- | --- |
| Length admission | Preserve nonpositive-needle true, then overlong-needle false, before pointer inspection. Positive admitted lengths fit `isize` on the supported 64-bit targets. | Runtime `str_predicate_range_matrix` crosses empty/negative/overlong lengths and null controls. |
| Byte comparison | Resolve the nullable pair, then retain `safe_slice` for each exact needle-sized range. Prefix reads only the prefix; suffix reads only the tail. | Runtime `str_predicate_range_matrix` compares against Rust slice predicates across lengths, offsets, equal/mismatching bytes, embedded NUL and non-ASCII bytes. Existing `str_predicates_match_byte_semantics` remains. |
| Pointer arithmetic | Do not form a suffix pointer before positive-length, fit and non-null admission. The offset stays `hlen - slen` within the caller's range. | The same matrix covers same-view, interior aligned/unaligned views, empty haystacks and null pairs; code inspection checks the admission order. |
| Ownership/cleanup/control | Borrowed input only; no construction, move, replacement, Drop, error carrier, allocation or retained reference changes. Branches return only 0/1. | Existing runtime predicate owner plus the range matrix; aggregate/control-flow ownership cells are unaffected. |
| Artifact and staticlib parity | One Rust source still builds both implementations; unchanged four-symbol guarded set, effect records, signatures and native memory contract. | Full driver `rt_lto` target, including artifact budget, closed symbol set, attribute and fallback owners. |
| Specialization | Default LLVM optimization can fold a chain of constant byte predicates after rt-LTO; flag-off keeps runtime calls. Do not assert universal inlining. | Driver `gate10_constant_predicate_chains_specialize` crosses prefix/suffix and the existing CPU matrix, with flag-off controls. |
| Cache/import/compilation | No interface/MIR shape or cache format changes. Existing runtime bitcode/source digest selects changed objects. | Existing `rt_lto` owners, native exported-object execution and imported per-unit/ThinLTO execution in the local probe. No new cross-unit semantic fact. |
| Performance | Measure the independently authored fixed classifier and dynamic-predicate controls with baseline/candidate code, identical inputs and alternating runs. Record code size and outcomes. This is local evidence, not a new cross-platform or whole-product bound. | `bench/string_predicates/run.py`, using the existing bounded native supervisor; original consumer release census remains consumer-owned. |

The author matrix pass finds no new ownership, FFI signature, public safety
strategy, IR variant or compilation stage. The independent strategy check was
clean: defined-input equivalence, null/length order and unchanged artifact/ABI
boundaries were confirmed before editing the shared unsafe implementation.
Final full-diff review remains separate.

## Qualification

The new driver owner fails on `99f90eab` with two retained suffix calls and
passes with the changed shared source. All 13 `rt_lto` owners pass, including
the baseline/native/cortex-a72 CPU matrix and flag-off controls. Both runtime
predicate owners pass, including the length/offset/byte/null matrix. Native
imported per-unit and ThinLTO programs return the expected predicate results.

On macOS arm64 / Apple M1 / LLVM22.1.8, seven alternating runs of
`bench/string_predicates` give these medians for 6,400,000 calls per sample:

| Case | Baseline ns | Candidate ns | Baseline / candidate |
| --- | ---: | ---: | ---: |
| Constant prefix chain | 20,598,000 | 20,087,000 | 1.025 |
| Constant suffix chain | 39,114,000 | 20,051,000 | 1.951 |
| Dynamic prefix + suffix | 39,956,000 | 40,860,000 | 0.978 |

Every sample passes the independent result oracle and checksum. The dynamic
control is 2.3% slower in this local run; no universal speedup is claimed.
The same three-function object's `__TEXT` decreases from 1,024 to 924 bytes.
The unchanged private LLM source-file classifier, SHA-256
`ffc9c500e88e22c420ca8553c5a18434159287a3bb8adefd26cf89dad468e133`,
changes from six retained suffix calls to zero; prefix calls are zero in both.
No private consumer source is copied into the public fixture. These are scoped
local measurements, not cross-platform or whole-application qualification.

R97's original whole-application zero-symbol acceptance remains consumer-owned.
R109's global sign-test census and R100's missing historical comparison are
independent; this change claims neither.
