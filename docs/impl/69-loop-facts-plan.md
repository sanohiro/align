# Loop facts: view headers, bounds checks, and counted-loop shape

Status: plan of record for issues
[1079](https://github.com/sanohiro/align/issues/1079),
[1080](https://github.com/sanohiro/align/issues/1080),
[1081](https://github.com/sanohiro/align/issues/1081) and
[1084](https://github.com/sanohiro/align/issues/1084). Nothing here is
implemented.

[Plan 68](68-vectorization-contract.md) is the public-contract ledger and owns
guarantees G1–G10. This plan implements G1, G2 and G3 and does not restate,
widen or reinterpret them; where a promise is quoted it is quoted to bind an
owner test to it. No public language surface, no unit-interface field and no
cache-key component is added, so the large design authoring gate does not apply
a second time. What this document therefore carries is what the cross-cutting
implementation gate requires and plan 68 deliberately left to its implementing
plan: the per-layer owners, the named IR facts, and one implementation closure
matrix per PR.

Evidence baseline is plan 68's: Align `8c8bfbc7a3169e84ecc8415f5149ab8c61afe863`
(alignc 0.7.5, LLVM 22.1.8), Apple M1, `--profile release`, default
`--target-cpu baseline`. Code locations below were re-verified against
`cfe0f3be`.

## 1. Scope

### 1.1 What this plan owns

```text
PR 1   1079 + 1080   materialize a borrowed view header once; state its alias
                     facts; attach !range to every length materialization
PR 2   1081          fuse the bounds check to one unsigned compare; move the
                     monotone-induction check out of the loop body
PR 3   1084          lower a recognized counted loop with its trip-count exit
                     at the latch; supply the data buffer's extent
```

The three land in that order. PR 1 is a precondition for PR 2's measurement
(plan 68 §2 records the conjunction: each half alone produces zero vector
bodies), and PR 1's preheader header materialization is what makes PR 3's
extent fact cheap, because there is then one `%ptr` SSA value to attribute.

### 1.2 Non-goals

```text
no new syntax        one loop expression with value-carrying break is locked;
                     no range loop, no for/while, no loop annotation
no check removal     every bounds and range check is preserved; PR 2 moves a
                     check, it never deletes one, and there is no unchecked
                     mode and no profile-dependent checking
no vectorization
  promise            plan 68's promise/try split governs. This plan promises
                     emitted IR shape; whether LLVM then widens a given loop
                     on a given target is a try
no float change      no reassociation, no contraction, no fast-math flag. G4
                     is not touched
no assume policy     PR 3's one carve-out (§4.2) is the only admitted use, and
                     it is narrower than the attribute it falls back from
no source facts      every fact stated to LLVM is a property of Align's own
                     lowering or representation, never of source spelling
no interface change  no unit interface field, no cache-key component, no
                     artifact identity change; the object content changes and
                     the interface hash does not (plan 68 §3.1)
```

### 1.3 Relation to plans 63 and 64

`63-codegen-performance-audit.md:229-234` observed from the caller side that
asserting `src.len() >= 0` enables a vector main loop for the byte-sum witness,
and directed that the invariant belong to the compiler rather than to callers.
PR 1's `!range` is that move. `63-...:232` states that the shipped byte-range
proof "is not complete bounds-check elimination"; PR 2 is the general
induction-variable rule that sentence leaves open, and it does not generalize
the byte-range proof itself.

`64-composed-byte-optimization-plan.md` owns composed byte loops, descriptor
stability and local read-leaf exposure. Its rules bind PR 2 unchanged: no fact
minted from source spelling (`:186`), proofs recomputed for the actual body
rather than deserialized as trusted metadata, and the original failure block
retained for every edge not independently proved. `byte_ranges::simplify`
remains the owner of the byte-range proof; PR 2 adds a second, independent
transform beside it and must compose with it in one direction only (§3.4).

Neither plan owns aliasing metadata or loop rotation, and neither is modified
by this plan.

### 1.4 Which guarantee each PR closes

| PR | Issues | Guarantee | Closes |
| --- | --- | --- | --- |
| 1 | 1079, 1080 | G1 in full; G2's proof surface | one header materialization per function or loop preheader; the TBAA header/element split; `noalias`/`dereferenceable`/`align` on borrow headers; `!range` on every length |
| 2 | 1081 | G2 | one unsigned compare; the monotone-induction check out of the body with trap behavior unchanged |
| 3 | 1084 | G3 | the trip-count exit at the latch, recognition total for the canonical shape, one named MIR fact, and the buffer extent |

Plan 68 assigns issue 1080 to G2 rather than to G1, and calls it G2's proof
surface rather than a separate guarantee. It ships in PR 1 anyway, because
`!range` attaches at exactly the length materialization site that PR 1 creates;
splitting it into PR 2 would mean writing that site twice. PR 2 therefore
inherits a closed prerequisite rather than opening one.

## 2. PR 1 — header materialization and data facts (1079 + 1080)

### 2.1 Layers and exact owners

**MIR (`crates/align_mir/src/lib.rs`).** The re-materialization is structural,
not a codegen artifact: `hir::Stmt::AssignIndex` lowering pushes a fresh
`Rvalue::Load(base)` and a fresh `Rvalue::SliceLen` for every element store, and
`lower_index` does the same on the read side before calling
`emit_bounds_check`. Both take the length operand from one per-(place, loop)
materialization instead of re-emitting it. No new `Rvalue` variant, no new
validator: `validate_slice_index_rvalues` still sees an `i64` index operand and
`validate_mir_producers` still sees the same producer set.

**LLVM lowering (`crates/align_codegen_llvm/src/lib.rs`).**

```text
borrowed_view_part            gains a cache keyed on (place, field), materialized
                              at the dominating point: the entry block for a
                              parameter-derived header, the loop preheader
                              otherwise. borrowed_place_ptr is unchanged
mark_borrow_param_contracts   /_at stops `continue`ing on every mode that is not
                              ParamMode::Borrow, so a BorrowMut header pointer
                              gets its attributes too
abi_param_type                unchanged. Borrow and BorrowMut still pass a bare
                              `ptr` to the {ptr,i64} header (slice_struct_type);
                              ByValue/Out still pass the header by value
add_enum_attr /
  add_valued_enum_attr        emit the new parameter attributes; enum_kind_id
                              resolves their kind ids
FnGen::alias_scope_lists      gains a sibling holding the TBAA nodes, cached per
                              function exactly as the alias scopes are
```

The existing scoped-`noalias` metadata on `map_into` load/store pairs
(`Rvalue::SliceIndexNoalias`) is untouched and keeps its own domain. TBAA and
alias scopes are independent metadata; neither weakens the other.

### 2.2 The IR facts, named

```text
!tbaa                     one align TBAA root with two children,
                          align.view.header and align.elem.<T>. Attached to
                          header field loads and to element loads/stores whose
                          base is an Align-lowered view
noalias                   on a Borrow or BorrowMut header pointer parameter,
                          gated by §2.3 I3
dereferenceable(16)       on a Borrow or BorrowMut header pointer parameter —
                          the {ptr,i64} header, never the buffer
align 8                   on the same parameter
nonnull                   already emitted; unchanged
readonly                  already emitted for Borrow; unchanged, and never
                          emitted for BorrowMut
captures(none)            already emitted for Borrow when the borrow is not
                          returned; unchanged
!range !{i64 0, i64 -9223372036854775808}
                          on every length materialization: the borrowed header
                          field load, and the `extractvalue` path for a by-value
                          {ptr,i64} header
```

The `!range` pair is the half-open wrapped range `[0, 2^63)`, which is exactly
"this `i64` is non-negative" and nothing more. A tighter per-element bound
(`isize::MAX / sizeof(T)`, the runtime's own `safe_len` limit) is available but
deliberately not claimed: 1080's experiment measured that non-negativity alone
removes every `llvm.smax`/`llvm.umin` guard and widens the two-length kernels,
and a tighter bound would require auditing every length producer for a fact no
measurement needs.

Not emitted by this PR, and each for a stated reason: `dereferenceable` on the
data buffer (PR 3 owns it; `dereferenceable(16)` on the header provably does not
reach the buffer), `llvm.assume` in any form, `inbounds`/`nuw`/`nsw` beyond what
codegen already emits, and any loop metadata.

### 2.3 Invariants

```text
I1  a materialized header dominates every use it replaces, and is materialized
    at a point where the place is initialized. A conditionally initialized or
    conditionally created view is not cached
I2  the cache is killed by any write to the place, by any call that could write
    it, and at a loop back-edge when the place is assignable in the loop.
    The fail-closed default is no caching
I3  noalias is emitted on header parameters of a function only when that
    function writes no view header through any parameter. A borrowed
    replacement (`cur = next` on a `borrow mut` binding) and an owned struct
    field replacement (`r.xs = other`) are both header writes and both disable
    the attribute for the whole function
I4  align.elem.<T> is stated disjoint from align.view.header only when T
    contains no view header transitively. Otherwise the access carries the TBAA
    root and claims nothing
I5  no TBAA node is attached to an access whose pointer did not come from an
    Align-lowered view: a raw FFI pointer, a `resource` handle payload, and
    anything reached through `unsafe raw` are untagged
I6  two element accesses never claim no-alias against each other. The split is
    header-versus-element only; element-versus-element aliasing is unchanged
I7  !range asserts non-negativity, which is true of every length Align produces
    at every profile, so the attachment is unconditional and profile-independent
```

I3 and I4 are the two that carry the soundness of the whole PR, and both are
reachable in code that compiles today:

```align
Row { xs: slice<i64>, n: i64 }

fn replace_view(borrow mut cur: slice<i64>, next: slice<i64>) {
  cur = next
}

fn set_field(src: slice<i64>, other: slice<i64>) -> i64 {
  mut r := Row { xs: src, n: 1 }
  r.xs = other
  return r.n
}
```

`replace_view` writes a header slot through a parameter, which is what I3
excludes. `Row` is a record whose element storage contains a header, which is
what I4 excludes. Whole-element assignment into a `slice<Row>` is rejected by
the checker today ("element assignment of struct is not supported yet"), so the
currently reachable header writes are exactly borrowed replacement, owned struct
field replacement, and a materializing pipeline terminal writing struct
elements. I4 is written against the type, not against that list, so lifting the
element-assignment restriction later cannot silently invalidate it.

### 2.4 Aliasing closure matrix (PR 1)

| Cell | Required behavior | Owner |
| --- | --- | --- |
| Header load versus element store that can alias | `align.elem.<T>` disjoint from `align.view.header` only for a `T` with no reachable header (I4); otherwise root-tagged | new `loop_facts` owner asserting the tagged and untagged forms for `slice<i64>` and `slice<Row>`; `struct_slice_fields` |
| Mutable view | `BorrowMut` header gets `nonnull dereferenceable(16) align 8`, never `readonly`; element stores tagged `align.elem.<T>` | new `loop_facts` owner; `out_params`, `borrowed_params` |
| Header written through a parameter | no `noalias` on any header parameter of that function (I3) | `borrowed_replacement`, `owned_field_replacement`, plus a new negative owner pinning the attribute's absence |
| Two views of the same buffer | unchanged: the existing sema gate rejects the aliasing `out` shapes, and `noalias` is never the thing that makes them sound | `out_params::out_arg_aliasing_another_arg_rejected`, `out_arg_two_slices_of_same_array_rejected`, `map_into::map_into_dst_aliasing_source_rejected` |
| Interprocedural laundering | a view whose pointer came from FFI, `unsafe raw` or a `resource` payload carries no TBAA and no `noalias` (I5) | `ffi_views`, `unsafe_raw`, new negative owner |
| Cache kill on write, call, back-edge | a re-read after any of the three (I2) | new `loop_facts` owner with a store, an opaque call, and a loop-carried reassignment |
| Conditionally initialized view | no preheader materialization (I1) | new `loop_facts` owner |
| `map_into` scoped metadata | unchanged domain, unchanged nodes, now coexisting with TBAA | `map_into::map_into_emits_scoped_noalias_metadata`, `map_into_fixed_source_omits_load_metadata` |
| Length fact universality | `!range` on the borrowed field load, the by-value `extractvalue`, and a fixed-array constant length (a no-op there) | new `loop_facts` owner; `fixed_array_len` |
| Trap parity | `align_rt_bounds_fail` and `align_rt_range_fail` text byte-identical | `runway_a2_binary_codec::read_past_end_aborts`, `negative_offset_aborts`, `out_params::out_write_out_of_bounds_aborts` |

### 2.5 Acceptance corpus and the architecture gate

Plan 68 §7 records that every shape test in
`crates/align_driver/tests/vectorize_shapes.rs` is gated on `x86_backend()`
(`cfg!(target_arch = "x86_64") && backend_available()`) and pins `x86-64-v3` or
`x86-64-v2`, so on aarch64 — the host that produced every measurement behind
these four issues — none of the fifteen tests executes anything, and that the
gate must be addressed in the same change that adds the first guarantee owner.
PR 1 is that change, and it does two separate things rather than one:

```text
new owner, arch-neutral     crates/align_driver/tests/loop_facts.rs asserts the
                            emitted MIR and raw LLVM IR — metadata names,
                            attribute text, one header load, one SliceLen. It
                            uses common::emit_llvm (BuildTarget::Baseline, raw)
                            and align_mir::print::function_to_string, so it runs
                            identically on x86-64 and aarch64 and pins no width
extended owner, per arch    vectorize_shapes.rs gains an aarch64 arm. x86 keeps
                            x86-64-v3 and x86-64-v2; aarch64 uses the portable
                            per-arch baseline (armv8-a), which is what the
                            default build emits and what every measurement in
                            1079/1080/1081/1084 used
```

Why both: an IR fact is a promise and is arch-independent, so it belongs in an
owner that actually runs everywhere; a vector width is a target property, so it
belongs in an owner that names its target. Asserting the promise only through
the x86-gated suite would leave G1's promise unproved on the host it was
measured on.

What each arch asserts:

```text
arch-neutral (both)   the TBAA node names, the parameter attribute text, the
                      !range pair, exactly one header load per loop, exactly one
                      SliceLen per indexed element store
x86-64-v3             a vector body at 256-bit lane counts
x86-64-v2             a vector body at 128-bit lane counts
aarch64 baseline      a vector body at 128-bit lane counts
```

Width is pinned per target tier for test stability, exactly as the existing
suite pins `<4 x i64>` at v3 and `<2 x i64>` at v2. The *promise* remains the
presence of a vector body; the width in a test is a stability device, not a
guarantee, and plan 68's rule that no guarantee is stated as a target-specific
width is unchanged.

New owner names, all planned: `g1_view_header_hoisted` (plan 68's name),
`g1_borrow_header_attributes`, `g1_tbaa_header_element_split`,
`g1_tbaa_absent_for_header_bearing_elements`, `g1_noalias_absent_when_header_written`,
`g1_no_tbaa_on_foreign_pointers`, `g1_len_range_on_every_length_source`.

Existing suites that must pass unchanged, and would fail for a defect in this
PR: `map_into`, `out_params`, `borrowed_params`, `borrowed_replacement`,
`owned_field_replacement`, `ffi_views`, `unsafe_raw`, `struct_slice_fields`,
`runway_a2_binary_codec`, `loop_expr`, `bytes_ops`, `vec_simd`,
`emit_llvm_stage`, `explain_opt`, `vectorize_shapes`.

Benchmarks: 1079's `scale_only` and AXPY timings and 1080's `dot` guard count
are local measurements for the issues' own performance claims. They are not
correctness gates.

## 3. PR 2 — one bounds check, moved and never deleted (1081)

### 3.1 The two changes

**(a) Fusion.** A length is non-negative by construction, so
`(idx < 0) || (idx >= len)` is `(u64)idx >= (u64)len`. `emit_bounds_check`
replaces three `Rvalue::Bin` nodes with one `Rvalue::Cast` per operand and one
`BinOp::Ge` on `u64`, which codegen already lowers to an unsigned `icmp`
because the predicate follows the operand type. `emit_range_bounds_check` fuses
its five operations the same way; `emit_vec_bounds_check` delegates to it and
inherits the fusion. No new `BinOp`, no new `Rvalue`, no validator change: the
index operand of `Rvalue::SliceIndex` stays exactly `i64`, which is what
`validate_slice_index_rvalues` checks.

Fusion is unconditionally correct, independent of everything else in this plan,
and changes no call-site count: the check is fused, not removed.

**(b) Movement.** A new MIR module, `loop_facts`, derives the induction
structure of a loop and rewrites its guards. It is registered beside
`byte_ranges::simplify` inside `lower_program_checked_with_catalog`, after it,
and is fail-closed and roll-back in exactly the same way: derive, rewrite,
re-derive from the rewritten function, and discard the rewrite if the re-derived
facts disagree. `loop_facts` is the module PR 3 extends; there is never a second
induction derivation.

### 3.2 The exact shape of the movement

The guard is moved to the preheader by **versioning**, not by relocation:

```text
preheader   one admission test proving every guarded access in the loop body is
            in range for every iteration the loop will run
fast loop   the identical body with the proved guards removed
slow loop   the original body, guards intact, entered when the admission test
            fails
```

Relocating the trap itself is not admissible, and the reason is observable
rather than theoretical:

```align
fn scan_report(borrow xs: slice<i64>, off: i64) -> i64 {
  mut total := 0
  mut i := 0
  loop {
    if i >= xs.len() { break }
    print(i)
    total = total + xs[i + off]
    i = i + 1
  }
  return total
}
```

With `off` negative, the original program prints `0` and then traps. A guard
relocated to the preheader traps before printing anything. Versioning keeps both
the printed prefix and the trap, because the failing run takes the unmodified
loop. That is what makes plan 68's "the guard is *moved* to the preheader, not
deleted; trap text and first-failing-access iteration unchanged" true for an
effectful body and not only for a pure one.

Admission requires all of:

```text
one monotone index      a slot assigned i + step in the latch, with step a
                        positive integer constant, assigned on every path
                        through the body
loop-invariant bound    the trip-count comparison's right operand is invariant
                        across the loop
affine access           each guarded index is a*i + b with a and b
                        loop-invariant and a > 0
invariant length        each guarded length is invariant across the loop
no wrap                 bound + step does not overflow i64. With PR 1's !range
                        this is provable whenever the bound is a length; for a
                        general i64 bound it is not, and the loop is not admitted
every guard proved      if any guarded access in the body is not proved, the
                        loop is not versioned at all
```

A loop that fails any condition keeps its fused in-loop guards and is otherwise
untouched. Fail-closed is the default in every direction.

Code growth is real and bounded: versioning is applied only to a body under a
named node budget, recorded as a constant in `loop_facts` and pinned by an owner
test. `alignc size` and `build_profiles` measure the effect; the transform is
profile-independent, because `align_mir` has no profile plumbing and acquires
none here.

### 3.3 Trap diagnostics contract

```text
message text        align_rt_bounds_fail(index, len) keeps
                    `align: panic: index out of bounds: the len is {len} but the
                    index is {index}`; align_rt_range_fail(start, end, len) keeps
                    `align: panic: slice range out of bounds: {start}..{end} is
                    not within length {len}`. Byte-identical, both arguments
                    unchanged
failing iteration   a failing run executes the slow loop, so it traps at the
                    same iteration with the same argument values as today
effect prefix       every observable effect of the iterations before the failure
                    happens, in the same order
call-site count     unchanged by fusion; the slow loop retains every trap block,
                    so MIR runtime keys BoundsFail and RangeFail and ABI shapes
                    A60/A61 are unchanged
multiple guards     when a body has several guarded accesses, the slow loop
                    decides which traps first, so precedence is unchanged by
                    construction
```

### 3.4 Implementation closure matrix (PR 2)

Every cell names the owner that would fail for a defect in it. A reused owner is
named where it already discriminates the defect; a new owner is named `planned`.

| Axis | Cell | Required behavior | Owner |
| --- | --- | --- | --- |
| Control paths | `if` in the body | admission unaffected; a guarded access under a branch is proved for the iterations that reach it or the loop is not versioned | `loop_expr`, planned `loop_facts` owner |
| | `match` in the body | as `if`; a discriminator-unreachable arm contributes no access | `enum_match`, planned |
| | `else` unwrap | the unwrap's failure edge is an exit; the fast loop keeps it | `else_result`, planned |
| | `?` propagation | an early function return from the body is an exit, and is preserved in both loop versions | `loop_expr::a_question_mark_in_a_loop_exits_the_function`, `structured_error` |
| | `map_err` | no effect on admission; the call is opaque and kills nothing it does not write | `structured_error` |
| | branch joins | facts join by intersection; an index proved on one arm only is not proved at the join | planned |
| | loop joins | the back-edge join must re-derive; a fact killed on any path is killed at the header | planned |
| | early `break` with a value | both versions carry the same break value and the same break type; the loop's value is unchanged | `loop_expr::loop_yields_its_break_value`, `a_break_moves_an_owned_value_out_once` |
| | nested loops | an inner loop's index is not the outer loop's; versioning an inner loop inside an unversioned outer one is allowed | `loop_expr::nested_loops_break_the_innermost`, planned |
| | malformed input | a loop whose MIR fails the existing HIR/MIR validation never reaches `loop_facts`; a malformed index type is already rejected by `validate_slice_index_rvalues` | `analysis_coverage`, `runway_a2_binary_codec::byte_range_malformed_and_invalidated_proofs_fail_closed` |
| Index form | non-monotone index | not admitted: an index assigned on some paths only, reassigned in the body, or stepped by a non-constant | planned negative owner (`skip_zeros`, §4.3) |
| | wrapping index | not admitted unless `bound + step` provably does not overflow `i64`; integer overflow is defined wrap, so a wrapping loop must keep its guards | planned negative owner |
| | `i` | the base case | planned |
| | `i + 1` | `a = 1`, `b = 1`; admission uses the maximum reached index, not `i` | planned |
| | `i * 2` | `a = 2`, `b = 0`; the admission bound is `a*(N-1) + b` | planned (`stride_sum`) |
| | `i + off` with `off` a parameter | `b` loop-invariant but unknown and possibly negative; both ends of the range must be proved | planned (`scan_report`) |
| | `a*i + b` with `a` loop-invariant non-constant | admitted only with `a > 0` proved; otherwise not admitted | planned |
| Access kind | single index | `emit_bounds_check`'s guard | planned |
| | range `xs[i..i+k]` | `emit_range_bounds_check`'s guard; `start > end` and `end > len` both proved, including the loop-invariant overflow test 1081 §3 measures as un-hoisted | planned (`window_first`) |
| | `vecN` load/store | `emit_vec_bounds_check` delegates, so it inherits both fusion and movement | `vec_simd`, planned |
| | byte accessors | `bytes_read_le`/`set` guards are `byte_ranges`' territory; `loop_facts` does not re-prove them | `runway_a2_binary_codec`, `bytes_ops` |
| View shape | zero-length view | admission fails for any access, so the loop is versioned into a slow loop that traps identically, or runs zero times and traps not at all | `runway_a2_binary_codec::read_past_end_aborts`, planned |
| | negative index | impossible after fusion only in the sense that it still fails the unsigned compare; the trap text still reports the original signed index | `runway_a2_binary_codec::negative_offset_aborts` |
| | length-1 view | one-trip admission; the fast loop's single iteration is identical | planned |
| | length changed in the loop | not invariant, so not admitted | planned negative owner |
| Compilation model | whole-program | `loop_facts` runs inside `lower_program_checked_with_catalog`, so one `Program` is rewritten once | `emit_llvm_stage`, planned |
| | per-unit | the transform is a pure function of the validated body already fingerprinted by `CodegenKey`'s `impl_hash`, and `compiler_build_id` covers the new code, so no key component is added and no stale object survives | `per_unit`, `cache_codegen`, `unit_cache` |
| | ThinLTO | a partition body is rewritten by the same pass and re-validated by `validate_thin_partition_program`; import decisions are unchanged because no linkage or signature changes | `thin_lto`, `function_thin_lto`, `partition` |
| | generic monomorphization | each instantiation is rewritten independently after monomorphization; an instantiation whose element type changes the admission is not forced to agree with its siblings | `generics`, planned |
| | interface serialization | nothing serialized changes; the interface hash of a unit whose bodies are rewritten is byte-identical | `per_unit_surface`, `emit_interface` behavior pinned by `cache_codegen` |
| Composition | `byte_ranges::simplify` | runs first and unchanged; `loop_facts` derives from the simplified function and never consumes `byte_ranges`' facts, so a rolled-back byte-range proof cannot leave `loop_facts` holding a stale one | `runway_a2_binary_codec::byte_range_recurrence_preserves_tails_and_eliminates_only_proved_guards`, `paired_byte_ranges_keep_unproved_guards`, `composed_byte_effects_and_reaching_definitions_fail_closed` |
| | `byte_prepare::prepare` | the second `byte_ranges` site is guarded by `structurally_valid`; `loop_facts` respects the same gate and is skipped when it fails | `runway_a2_binary_codec::composed_reader_per_unit_and_thin_cache_bind_private_body_edits` |
| Ownership | construction, move-in, move-out, source nulling, replacement, return, Drop | versioning duplicates a body, so every owned value in it is constructed and dropped exactly once on each path, and a value moved out by `break` is moved out once across both versions | `loop_expr::a_per_iteration_owned_string_is_freed_each_pass`, `a_break_moves_an_owned_value_out_once`, `move_return_cleanup`, `reassign_drop`, `borrow_liveness` |
| | allocation parity | no allocation is added, removed or moved by either half | `lint_unnecessary_heap`, `runway_a2_binary_codec` |
| | runtime ownership provenance | unchanged: no new runtime call, no new handle | `resource_ownership` |
| Diagnostics | trap text and arguments | §3.3, byte-identical | `runway_a2_binary_codec::read_past_end_aborts`, `negative_offset_aborts`, `out_params::out_write_out_of_bounds_aborts`, planned zero-length/length-1/first-out-of-range owners |
| | other traps in the body | integer division and checked arithmetic keep their own guards and their own order | `div_guard`, `checked_arith` |
| Resource | code growth | the versioning budget is enforced and pinned; per-profile text size recorded | planned budget owner, `build_profiles` |
| | compile time | the pass is linear in blocks per function and is measured against the existing compiler gate budget | `docs/impl/16-test-policy.md` selection; local measurement |
| Shape | fusion is universal | every one of the guards emitted by the three emitters is fused, in every profile, whether or not the loop is versioned | planned MIR-text owner over `emit_bounds_check`, `emit_range_bounds_check`, `emit_vec_bounds_check` |
| | conjunction with PR 1 | `bytes_to_f32_out` reaches a vector body only with both PRs present; a regression in either half fails the same test | plan 68's conjunction pin in `vectorize_shapes` (planned in PR 1, asserted in PR 2) |

Cross-cutting note for the whole matrix: `loop_facts` changes a safety strategy
(which bounds checks execute where), so per the cross-cutting implementation
gate this matrix and the proposed capability boundary get one fresh independent
adversarial review **before** PR 2 is coded, not at review time. That review is
of this section; the implementation PR then carries the author-side
matrix-to-diff pass and the one ordinary review.

### 3.5 Acceptance corpus

New owners in `crates/align_driver/tests/loop_facts.rs` (arch-neutral, MIR text
and raw IR): the fusion shape, each admitted index form, each negative index
form, the versioning shape, the budget, and the join rules. New owner in
`vectorize_shapes.rs`: `g2_monotone_check_hoisted` (plan 68's name) plus the
matvec kernel `w[r*d+c] * x[c]` from 1081 §2, per arch tier.

Trap parity owners are executable, not IR-shape: zero-length, length-1, and
first-out-of-range accesses each build and run, and assert both the exit status
and the exact stderr text.

## 4. PR 3 — the trip-count exit at the latch (1084)

### 4.1 Recognition, and the named fact

Recognition lives in `loop_facts`, the module PR 2 introduces, and extends it
with the rotation record rather than adding a second derivation. The canonical
shape is exactly one spelling:

```text
header      a conditional break on `i REL bound`, first in the loop body
step        `i = i + step` with step a positive integer constant, on every path
bound       loop-invariant
```

and it lowers to exactly one shape:

```text
preheader   the zero-trip test, peeled
header      only the body's data-dependent exits
latch       `i + step REL bound`
```

The fact `loop_facts` records — trip count, step, monotone index, bound — is the
single source PR 2's admission and PR 3's rotation both read. Recognition is
total for the canonical shape: one spelling in, one shape out, with no
profitability heuristic, because "did you write the loop the fast way?" must not
be a question a programmer can ask about the only loop the language has.

Lowering only. No syntax, no annotation, no second loop form. The value of `i`
after the loop, the value carried by every `break`, and the iteration at which
every exit is taken are all unchanged.

### 4.2 The `dereferenceable` carve-out

`docs/open-questions.md`'s `REJECTED (do not re-litigate; reasons)` record
(`:4198-4204`, in the **Open** section; plan 68 §5 cites the same record at its
earlier line numbers) rejects "`llvm.assume` / early intrinsic emission /
loop-metadata overrides **as a general policy**", with the guidance "attributes
and flags first". Plan 68 §5 requires PR 3 to write an explicit carve-out
against that exact record. The carve-out is written in the PR that needs it,
amending that Open record in place; it promotes nothing to Settled.

```text
preferred, always tried first
  dereferenceable(<n>) as a parameter attribute or !dereferenceable as load
  metadata, where <n> is a compile-time constant byte count. This covers a
  fixed-length array and a loop whose bound is a constant, and is taken whenever
  it applies

admitted fallback, and only this
  exactly one `call void @llvm.assume(i1 true)
  ["dereferenceable"(ptr %p, i64 %n)]` in the preheader of a recognized counted
  loop, where %p is the data pointer PR 1 materialized for an Align-owned
  slice or array view and %n is that view's own `len * sizeof(T)`

conditions, all required
  the loop is recognized by loop_facts as canonical
  the view is Align-owned: not FFI, not `unsafe raw`, not a resource payload
  the bundle is emitted after the zero-trip peel, so it never executes for a
  zero-trip loop and never asserts anything about an empty view's pointer
  exactly one bundle per view per loop; never in the body, never on a back edge

not admitted, anywhere
  llvm.assume for alignment, non-nullness, ranges, aliasing, trip counts,
  branch probabilities, or any other fact
  loop metadata overrides of any kind
  any assume outside a recognized counted loop
  any assume whose extent is not `len * sizeof(T)` of that exact view
```

The general prohibition still stands, and the carve-out says why it does: what
makes this admissible is not that `assume` is convenient but that LLVM's
constant-only `dereferenceable` cannot express a runtime extent, that the extent
is Align's own representational invariant, and that the emission shape is
closed. If the carve-out is refused at review, G3's extent half ships for
statically known extents only and the rotation half ships regardless; plan 68 §5
already records that outcome.

### 4.3 Shapes that must not be recognized

| Shape | Why | Owner |
| --- | --- | --- |
| Step is not a positive integer constant | `skip_zeros` below steps by 1 or 2 depending on data | planned negative owner |
| Index assigned on some paths only | no monotone recurrence | planned |
| Index reassigned in the body after the guard | the recurrence is not the latch's | planned |
| Bound written in the body | not loop-invariant | planned |
| Bound near `i64::MAX` | `i + step` wraps, and wrap is defined behavior that rotation must not change | planned |
| Trip-count test not first in the body | the header's first exit is data-dependent; there is nothing to rotate | planned |
| Two trip-count-shaped exits | ambiguous canonical form; not admitted | planned |
| Index escapes to a lambda or a call | the recurrence is not local | `lambda`, planned |
| Index used after the loop | admitted, but the post-loop value must be identical | `loop_expr`, planned |
| Loop with no exit at all | a diverging loop is unchanged | `loop_expr::a_diverging_loop_body_is_a_function_result` |
| Zero-trip, one-trip, first-element exit, last-element exit, no early exit | all five behave exactly as today | planned executable owners (1084's own list) |

```align
fn skip_zeros(borrow xs: slice<u8>) -> i64 {
  mut i := 0
  loop {
    if i >= xs.len() { break }
    if xs[i] == 0 { i = i + 2 } else { i = i + 1 }
  }
  return i
}
```

### 4.4 Acceptance corpus

New in `vectorize_shapes.rs`: `g3_counted_latch_exit` plus the zero-trip,
one-trip, first-element-exit, last-element-exit and no-exit owners (plan 68's
names), per arch tier; the `all`-style `slice<u8>` scan asserting a 128-bit
integer vector body on aarch64 baseline and x86-64-v2.

New in `loop_facts.rs` (arch-neutral): the rotated MIR shape, the named fact's
contents, the `dereferenceable` attribute form, the assume bundle's presence for
a recognized loop and its absence for a raw FFI pointer, a zero-length view and
an unrecognized loop.

Reused: `loop_expr` in full — it is the semantics owner for `loop` and would
fail for any rotation defect — plus `bytes_ops`, `text_boundary`, `explain_opt`
(the `Cannot vectorize early exit loop` remark count), and `runway_a2_binary_codec`.

## 5. Documents this plan edits

```text
07-roadmap.md      the Slice 5 deferral of type-derived per-program-fn param
                   attributes is retracted for the borrowed view header only,
                   with a pointer here. The rest of that deferral stands
HANDOFF.md         one sentence in the existing open-issue-batch paragraph,
                   recording plan 69 as planned
23-friction-ledger.md
                   a refused row for issue 1047 Finding 3. Unrelated to the
                   three PRs; recorded here because the same pass reads it
68-vectorization-contract.md
                   unchanged. This plan implements G1–G3 and changes no promise
04-mir.md, 05-backend-llvm.md, draft.md, docs/language-spec.md
                   unchanged. No language surface, no new MIR node, no new
                   vectorizable property, and MIR stays width-agnostic
```

## 6. Author-side consistency pass

- Every promise this plan makes is a promise plan 68 already states; each is
  bound here to a named owner test, and no new promise is introduced.
- Every named function, module, type and test in §2–§4 exists in the tree at
  `cfe0f3be` or is marked planned.
- Every IR fact is named exactly once, with its attachment site and its
  emission condition, and each PR states what it deliberately does not emit.
- Every matrix cell has an owner; reused owners are named only where they would
  fail for the defect in that cell, per the gate's reuse rule.
- The two soundness gates of PR 1 (I3, I4) and the one of PR 2 (versioning
  rather than relocation) are each stated with a shape that compiles today.
- Every normative `align` example is syntax-checked against `alignc check`, uses
  one `loop` with newline-terminated statements, and has a repository precedent
  for each construct it uses.
- No PR consumes a capability scheduled for a later PR: PR 1 needs neither
  induction facts nor rotation, PR 2 needs PR 1's `!range` which PR 1 ships, and
  PR 3 extends PR 2's module rather than duplicating it.
- Cache and artifact identity are stated once (§3.4) and match plan 68 §3.1: the
  object content changes, the interface hash does not, and no key component is
  added.
- The one carve-out against a standing rejection is written out in full (§4.2),
  including what it does not admit, and its failure mode is stated.
