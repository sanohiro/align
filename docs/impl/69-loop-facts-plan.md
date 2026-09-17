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
                     facts; attach !range to every length load
PR 2   1081          fuse each emitted guard to its minimal unsigned form; move
                     the monotone-induction check out of the loop body
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
| 1 | 1079, 1080 | G1 in full; G2's proof surface | one header materialization per function or loop preheader; the TBAA header/element split; `noalias` on read-only `borrow` headers and `dereferenceable`/`align` on borrowed headers; `!range` on every length load |
| 2 | 1081 | G2 | one unsigned compare for an element index and two for a range (§3.1); the monotone-induction check out of the body with trap behavior unchanged |
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
`emit_bounds_check`. `lower_borrowed_place` and `lower_index_field` re-emit the
same pair, and there are fourteen `emit_bounds_check` call sites in total, so
the reuse is a property of the length operand rather than of two named sites:
every site takes its length from one per-(place, loop) materialization instead
of re-emitting it. No new `Rvalue` variant, no new validator:
`validate_slice_index_rvalues` still sees an `i64` index operand and
`validate_mir_producers` still sees the same producer set.

**LLVM lowering (`crates/align_codegen_llvm/src/lib.rs`).**

```text
borrowed_view_part            gains a cache keyed on (place, field), materialized
                              at the dominating point: the entry block for a
                              parameter-derived header, the loop preheader
                              otherwise. borrowed_place_ptr is unchanged
mark_borrow_param_contracts   /_at stops `continue`ing on every mode that is not
                              ParamMode::Borrow, so a BorrowMut header pointer
                              gets nonnull/dereferenceable/align — but never
                              noalias and never readonly. The shared helper also
                              serves declare_imported_fn, so the noalias arm is
                              taken only where the body is available
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
noalias                   on a Borrow header pointer parameter only, gated by
                          §2.3 I3, and only at a definition site
dereferenceable(16)       on a Borrow or BorrowMut header pointer parameter —
                          the {ptr,i64} header, never the buffer
align 8                   on the same parameter
nonnull                   already emitted for Borrow; now also for BorrowMut
readonly                  already emitted for Borrow; unchanged, and never
                          emitted for BorrowMut
captures(none)            already emitted for Borrow when the borrow is not
                          returned; unchanged
!range !{i64 0, i64 -9223372036854775808}
                          on the load that materializes a length from a borrowed
                          header, and on a fixed-array constant length where it
                          is a no-op
```

`noalias` stays on `Borrow` exactly as plan 68 G1 states it, and this plan does
not widen it to `BorrowMut`. The reason is that `Borrow` also carries `readonly`,
so a `Borrow` header is never written through; a writable header has no such
companion claim, and `noalias` on it would turn an aliasing pair of arguments
into undefined behavior rather than into a diagnostic. `BorrowMut` therefore
gains only the non-aliasing facts (`nonnull dereferenceable(16) align 8`), which
is what 1079 §1 observes it lacks entirely today.

`!range` is attached to loads, and to nothing else: LLVM accepts `!range` on a
`load`, `call`, `invoke` and `callbr`, and an `extractvalue` cannot carry it. A
slice passed by value arrives as a `{ptr,i64}` aggregate parameter, so its length
is produced by `extractvalue` and the integer `range(...)` parameter attribute
does not apply to the aggregate either. That path therefore carries no range
fact under this plan, and supplying one would require exactly the `llvm.assume`
mechanism §4.2 admits only for a different, narrower purpose. This costs nothing
measured: every `llvm.smax` clamp 1080 counted came from the borrowed-header
path, which is covered.

The `!range` pair is the half-open wrapped range `[0, 2^63)`, which is exactly
"this `i64` is non-negative" and nothing more. A tighter per-element bound is
expressible in principle — no `slice<T>` can exceed `isize::MAX` bytes — but is
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
I3  noalias is emitted on a Borrow header parameter of a function only when
    that function's body is available and writes no view header at all —
    neither directly nor through a call it hands a header pointer to, with an
    opaque call failing closed. A borrowed replacement (`cur = next` on a
    `borrow mut` binding) and an owned struct field replacement
    (`r.xs = other`) are both header writes and both disable the attribute for
    the whole function. An imported declaration has no body, so it never
    carries the attribute. `noalias` cannot be violated by a caller when no
    access through any pointer in the callee writes header memory, which is
    what makes a body-derived predicate the right gate for it
I4  align.elem.<T> is stated disjoint from align.view.header only when T
    contains no view header transitively. Otherwise the access carries the TBAA
    root and claims nothing
I5  no TBAA node is attached to an access whose pointer did not come from an
    Align-lowered view: a raw FFI pointer, a `resource` handle payload, and
    anything reached through `unsafe raw` are untagged
I6  two element accesses never claim no-alias against each other. The split is
    header-versus-element only; element-versus-element aliasing is unchanged
I7  !range asserts non-negativity of the length a borrowed header load
    produces, which is true at every profile, so that attachment is
    unconditional and profile-independent. It is not a claim about every
    operand any guard calls a "length" — §3.1 states the separate
    non-negativity precondition the fusion needs, per guard
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
what I4 excludes. Element assignment into a `slice<Row>` is rejected by the
checker today — `element assignment of struct#0 is not supported yet (Copy
scalar or borrowed `str` elements only for now)` — so the currently reachable
header writes are exactly borrowed replacement, owned struct field replacement,
and a materializing pipeline terminal writing struct elements. I4 is written
against the type, not against that list, so lifting the element-assignment
restriction later cannot silently invalidate it.

### 2.4 Aliasing closure matrix (PR 1)

| Cell | Required behavior | Owner |
| --- | --- | --- |
| Header load versus element store that can alias | `align.elem.<T>` disjoint from `align.view.header` only for a `T` with no reachable header (I4); otherwise root-tagged | new `loop_facts` owner asserting the tagged and untagged forms for `slice<i64>` and `slice<Row>`; `struct_slice_fields` |
| Mutable view | `BorrowMut` header gets `nonnull dereferenceable(16) align 8`, never `readonly` and never `noalias`; element stores tagged `align.elem.<T>` | new `loop_facts` owner asserting `noalias`'s absence on a `borrow mut` header; `out_params`, `borrowed_params` |
| Header written anywhere in the body | no `noalias` on any header parameter of that function (I3) | `borrowed_replacement`, `owned_field_replacement`, plus a new negative owner pinning the attribute's absence |
| Imported declaration | no body, so no `noalias`; the other header attributes are unaffected | new negative owner over `declare_imported_fn`'s output; `imports`, `interface_param_modes` |
| Two views of the same buffer | unchanged: the existing sema gate rejects the aliasing `out` shapes, and `noalias` is never the thing that makes them sound. Because `noalias` lands only on read-only headers of bodies that write no header, an aliasing pair of arguments cannot be made unsound by it | `out_params::out_arg_aliasing_another_arg_rejected`, `out_arg_two_slices_of_same_array_rejected`, `map_into::map_into_dst_aliasing_source_rejected`, plus a new owner passing one place as two `borrow` arguments and as a `borrow`/`borrow mut` pair |
| Interprocedural laundering | a view whose pointer came from FFI, `unsafe raw` or a `resource` payload carries no TBAA and no `noalias` (I5) | `ffi_views`, `unsafe_raw`, new negative owner |
| Cache kill on write, call, back-edge | a re-read after any of the three (I2) | new `loop_facts` owner with a store, an opaque call, and a loop-carried reassignment |
| Conditionally initialized view | no preheader materialization (I1) | new `loop_facts` owner |
| `map_into` scoped metadata | unchanged domain, unchanged nodes, now coexisting with TBAA | `map_into::map_into_emits_scoped_noalias_metadata`, `map_into_fixed_source_omits_load_metadata` |
| Length fact coverage | `!range` on the borrowed header field load and on a fixed-array constant length (a no-op there); asserted absent on the by-value `extractvalue` path, where no IR mechanism exists | new `loop_facts` owner, positive and negative; `fixed_array_len` |
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
suite pins `<4 x i64>` at v3 and `<2 x i64>` at v2. The promise is the emitted
IR shape, which the arch-neutral owner pins; the vector body a tier asserts is a
`try` in plan 68's sense, pinned for regression detection rather than promised,
and plan 68's rules that no guarantee is stated as a target-specific width and
that a `try` is never a guarantee are both unchanged.

New owner names, all planned: `g1_view_header_hoisted` (plan 68's name),
`g1_borrow_header_attributes`, `g1_tbaa_header_element_split`,
`g1_tbaa_absent_for_header_bearing_elements`, `g1_noalias_absent_when_header_written`,
`g1_noalias_absent_on_mutable_and_imported_headers`,
`g1_no_tbaa_on_foreign_pointers`, `g1_len_range_on_borrowed_and_fixed_lengths`.

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

**(a) Fusion.** Each emitted guard collapses to its minimal unsigned form. The
two forms are different, and the plan states both rather than one:

```text
element index   (idx < 0) || (idx >= len)
                => (u64)idx >= (u64)len
                three Rvalue::Bin nodes become one BinOp::Ge on u64
range           (start < 0) || (start > end) || (end > len)
                => (u64)start > (u64)end || (u64)end > (u64)len
                five Rvalue::Bin nodes become two unsigned compares and one Or.
                There is no single unsigned predicate over three operands that
                reproduces the three-way test, and claiming one would be unsound
```

The range form is exact in all four sign quadrants given `len >= 0`: a negative
`start` with a non-negative `end` is caught by the first arm, and a negative
`end` by the second. `emit_vec_bounds_check` delegates to
`emit_range_bounds_check` and inherits that form, including the wrapping
`idx + n` case, which the unsigned `end > len` arm catches where the signed
`start > end` arm catches it today.

Both forms have one precondition: the guard's length operand must be
non-negative. It is for a `SliceLen`, for a fixed-array constant, and for a
computed count produced under a dominating non-negativity guard such as
`lower_chunks_count`'s. A guard whose length operand is not proved non-negative
keeps its signed form unchanged — the fusion is per guard, not a global rewrite,
and this is the precondition I7 deliberately does not assert globally.

Codegen lowers the fused predicate to an unsigned `icmp` because the predicate
follows the operand type: the two operands pass through the existing
`Rvalue::Cast` to `u64` and the compare is the existing `BinOp::Ge`. No new
`BinOp`, no new `Rvalue`. The index operand of `Rvalue::SliceIndex` stays
exactly `i64`, which is what `validate_slice_index_rvalues` checks.

One class of guard keeps its signed form. `lower_borrowed_place` emits a guard
for a borrowed element argument such as `inspect(rs[i])` and publishes it as a
`BorrowedElementGuard { reservation, len }`; codegen's
`checked_borrowed_element_guard` then re-derives that guard **literally** —
`Bin(Or, Bin(Lt, index, 0), Bin(Ge, index, len))`, `len` typed `i64` and
defined by `Rvalue::SliceLen`, exactly one `BoundsFail(index, len)` in an
`Unreachable` failure block whose success edge dominates the action — before it
forms the element pointer. That check is a second MIR-to-codegen safety
contract, and fusing its guard would fail it with `borrowed element guard
condition is not the canonical bounds predicate`. So `emit_bounds_check` gains
a signed-form entry used only by `lower_borrowed_place`; every other emitter
site fuses. Teaching the codegen validator the fused form is the extension in
§3.6, not part of PR 2.

Fusion changes no call-site count: the check is fused, not removed.

**(b) Movement.** A new MIR module, `loop_facts`, derives the induction
structure of a loop and rewrites its guards. It is registered beside
`byte_ranges::simplify` inside `lower_program_checked_with_catalog`, after it,
and is fail-closed and roll-back in exactly the same way: derive, rewrite,
re-derive from the rewritten function, and discard the rewrite if the re-derived
facts disagree. `loop_facts` is the module PR 3 extends; there is never a second
induction derivation.

Its position in the pipeline is an invariant, not an accident, because two
other passes read statement counts:

```text
1  annotate_par_map_work        lower_program_unchecked_with_plans; derives the
                                par_map work weight from block.stmts.len()
2  byte_ranges::simplify        lower_program_checked_with_catalog
3  loop_facts                   same loop, immediately after 2
4  byte_prepare::prepare        codegen lower_prepared_module; splices leaf
                                bodies into callers and re-runs
                                byte_ranges::simplify and snapshot_descriptors
                                on the spliced result
```

This adds one invariant to §2.3's list:

```text
I8  loop_facts runs after annotate_par_map_work and before codegen in every
    lowering entry point; a par_map work weight is derived from the unversioned
    body, so it is byte-identical with and without versioning
```

Consequences stated once: par_map work weights are computed on the unversioned
body and are byte-identical with and without versioning, so the `ParMapReduce`
partition and therefore float reduction order cannot change (I8, §3.4);
`byte_prepare` sees versioned bodies, so its statement-count admission counts
the body it actually splices (§3.2.2). `loop_facts` runs on every lowering entry
point, located or not, because all four funnel through the same hook.

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
IR identity             the body contains no Stmt::BorrowedElementReservation.
                        That marker is a function-unique token that codegen's
                        BorrowedElementValidationIndex::unique_reservation
                        requires to occur exactly once, and its guard is the
                        one checked_borrowed_element_guard re-derives literally
                        (§3.1). A cloned body would carry the token twice and
                        the fast copy would carry no guard at all. A loop that
                        borrows an element into a call keeps its checks in PR 2;
                        lifting that is §3.6
one monotone index      exactly one slot i is written in the body, by exactly
                        one statement i = i + step with step a positive integer
                        constant, and that statement post-dominates every
                        guarded access in the body: on every path from a
                        guarded access to the back-edge the step executes after
                        the access, so the value that reaches each guarded
                        access is the value i holds at the loop header. A step
                        that precedes an access on any path is disqualifying
                        (shifted_sum, §3.4), as is a second assignment anywhere
                        in the body, including from a nested inner loop. The
                        exit test i >= N reads the header value by construction
non-negative entry      the value of i on entry is a loop-invariant operand
                        proved >= 0 at the preheader: a non-negative constant, a
                        length, or an operand under a dominating
                        non-negativity guard. A negative or unproved entry fails
                        closed. §3.2.1 depends on it
loop-invariant bound    the loop exits on i >= N with N a loop-invariant i64
                        operand not killed in the body. Any other exit is an
                        ordinary exit and is preserved in both versions; only
                        this comparison forms the trip count
affine access           each guarded index is a*i + b with a and b
                        loop-invariant, not killed in the body, and a proved > 0
                        at the preheader
invariant view and      the root slot of each guarded view and the length
length (kill set)       operand of each guard are killed by no statement in the
                        body. Killing is decided by an exhaustive match over
                        every Stmt and Rvalue variant with no wildcard arm,
                        guarded by a compile-time variant tripwire in the manner
                        of align_sema::variant_sweep_tripwire, so a new variant
                        is a build error rather than a silent "does not
                        mutate". A variant kills the root when it names the
                        root slot or an operand deriving from it: Store,
                        StoreField, StoreIndex, StoreConstArray, StoreElemField,
                        StoreElemFieldPtr, ArrayTruncate, NullTupleField,
                        NullStructField, NullElemField, Drop, DropElem,
                        DropElemField, DropValue, DropFlagInit, PtrStore,
                        PtrStoreNoalias, RawStore, RawFree, ArenaEnd, TgEnd,
                        ColumnBatchFinish, ColumnBatchDrop, and every
                        store-like Rvalue. A call — DirectCall, CallIndirect,
                        RawCall, IndirectCallWithCleanup or a runtime call —
                        kills the root when any argument derives from it in any
                        mode other than read-only `borrow`, and kills every
                        root that is not itself a read-only `borrow` parameter
                        of the enclosing function when the call has no effect
                        summary. A borrowed replacement of the indexed binding
                        is a Store to the root and is covered by the same rule.
                        A length that "happens to be equal" is never a
                        substitute for the guard's own length operand
admission arithmetic    the preheader test is the exact i64 sequence in §3.2.1;
                        every operation in it is overflow-free by construction
                        or guarded by an explicit comparison, and any failed
                        guard selects the slow loop. No wider integer type
                        exists in MIR and none is introduced
both ends proved        b may be negative, so the test proves min >= 0 and
                        max < len for every guard, never only max
every guard proved      if any guarded access in the body is not proved, the
                        loop is not versioned at all
budget                  the body is within LOOP_FACTS_VERSION_BUDGET (§3.2.2)
```

A loop that fails any condition keeps its fused in-loop guards and is otherwise
untouched. Fail-closed is the default in every direction.

#### 3.2.1 The admission arithmetic

For entry `e` (proved `e >= 0`), constant step `s > 0`, bound `N`, and one
guard `(a, b, len)` with `a > 0` proved, the preheader computes, in `i64`:

```text
zero-trip   e >= N              the body never runs; select the fast loop, which
                                runs zero iterations and performs no access. No
                                further test is needed
induction   N <= i64::MAX - s   otherwise the last step i = imax + s wraps
                                negative under defined two's-complement wrap and
                                the fast loop, which has no guard, would index
                                with it; the original loop wraps identically but
                                keeps its guard, so the slow loop is selected
imax        e + s * ((N - 1 - e) / s)
                                overflow-free: e < N gives 0 <= N - 1 - e < N,
                                and e <= imax <= N - 1
amax        a * imax            overflows iff imax > i64::MAX / a; a > 0 is
                                proved, so the division is defined and is
                                computed once in the preheader
max         amax + b            b >= 0: overflows iff amax > i64::MAX - b
                                b <  0: cannot overflow (amax >= 0)
min         a * e + b           a * e <= amax, so it cannot overflow once amax
                                did not; b < 0 cannot underflow because
                                a * e >= 0; b >= 0 gives min <= max
admit       no overflow guard fired, and min >= 0, and max < len
```

The test is conjoined over every guard in the body and selects the fast loop
only when every conjunct holds. Because `a > 0`, `a*i + b` is monotone in `i`,
and every `i` the loop reaches lies in `[e, imax]`, so every intermediate index
lies in `[min, max]` and `a*i <= amax` for all of them: the review's
mid-iteration wrap is excluded without a wider type. The comparisons are the
exact `i64` compares MIR already has; the two divisions are by proved-positive
operands, so they cannot trip the hard error invalid integer division carries.

#### 3.2.2 Traversal order, re-entry and the budget

```text
order       loops are versioned innermost-first in post-order over the
            function's loop forest; a versioned inner loop contributes both of
            its copies to the enclosing body's statement count
re-entry    a block produced by cloning is marked and is never versioned again;
            each source loop is considered exactly once
budget      LOOP_FACTS_VERSION_BUDGET is a pub const in loop_facts, counted in
            body statements including terminators, pinned by an owner test.
            Over budget is not admitted and is reported (§3.2.3)
byte_prepare
            its statement-count admission (2048 statements in total, 32 sites,
            source order) counts the body it actually splices, which is the
            post-loop_facts body. A leaf near that budget can therefore stop
            being inlined once its loop is versioned. That is deterministic —
            the same input versions the same way in every unit — and is pinned
            by an owner whose leaf straddles the budget with and without
            versioning
determinism the transform is a pure function of the validated function; there
            is no profile, unit or entry-point dependence
```

#### 3.2.3 Observability

One source loop becoming two IR loops must be visible. `loop_facts` records one
decision per source loop in a per-function report, and `explain-opt` prints it
beside the LLVM remarks:

```text
loop versioned            budget used / LOOP_FACTS_VERSION_BUDGET
loop kept checks: <why>   borrowed-element | multiple-index-writes |
                          step-precedes-access | entry-unproved | bound-killed |
                          access-not-affine | root-killed:<Stmt kind> |
                          arithmetic-unproved | over-budget
```

The reason codes are the stable surface; the wording around them is not. A
budget refusal is therefore a regression an owner can assert on, and a user can
see why a loop was or was not versioned without reading MIR.

Plan 68 G2's exact statement — "at most one bounds check per loop, in the
preheader, for a monotone index" — reads on the admitted fast version. The slow
version exists precisely so that it can keep every check, and a loop that is not
admitted keeps its checks too. That is the same distinction G2 already draws
between moving a guard and deleting one; it is stated here because versioning
makes it visible as two loops.

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
| | `map_err` | an opaque call kills every place it may write, and a call reached by the indexed root kills that root (§3.2 kill set); a `map_err` whose arguments do not derive from the indexed view leaves admission unaffected | `structured_error`, planned kill-set owner |
| | branch joins | facts join by intersection; an index proved on one arm only is not proved at the join | planned |
| | loop joins | the back-edge join must re-derive; a fact killed on any path is killed at the header | planned |
| | early `break` with a value | both versions carry the same break value and the same break type; the loop's value is unchanged | `loop_expr::loop_yields_its_break_value`, `a_break_moves_an_owned_value_out_once` |
| | nested loops | an inner loop's index is not the outer loop's; versioning an inner loop inside an unversioned outer one is allowed | `loop_expr::nested_loops_break_the_innermost`, planned |
| | malformed input | a loop whose MIR fails the existing HIR/MIR validation never reaches `loop_facts`; a malformed index type is already rejected by `validate_slice_index_rvalues` | `analysis_coverage`, `runway_a2_binary_codec::byte_range_malformed_and_invalidated_proofs_fail_closed` |
| Loop producer | source `loop` expression | the primary case | planned `loop_facts` owner |
| | pipeline stage loops, including `scan` | generated by fusion; admitted only under the same conditions, and `scan`'s loop-carried shape stays a negative control | `vectorize_shapes::k3_scan_does_not_vectorize`, `deep_pipeline`, planned |
| | `map_into` / `to_array` materialize loops | already carry the scoped-`noalias` metadata; versioning them must leave those nodes and their domain intact | `map_into`, `array_materialize`, planned |
| | `chunks` and byte/str helper loops | `lower_chunks_count`'s consumer and the byte helpers use a computed count, which is admitted only under the fusion precondition in §3.1 | `chunks`, `bytes_ops`, planned |
| | `par_map` / `ParMapParallel` / `ParMapReduce` kernels | the kernel body is generated and lifted; versioning it must not change its purity, its work partition or its reduction order | `par_map`, `task_group`, planned |
| Index form | non-monotone index | not admitted: an index assigned on some paths only, reassigned in the body, or stepped by a non-constant | planned negative owner (`skip_zeros`, §4.3) |
| | step precedes a guarded access | not admitted: `i = i + 1` followed by `xs[i]` in the same body reads `1..n` while the header sees `0..n-1`; admission is stated on the value reaching each access (§3.2), so this shape keeps its checks and traps at `xs[n]` exactly as today | planned negative owner `shifted_sum` (`--emit mir` shows the guard retained; the executable trap-parity owner asserts the abort) |
| | wrapping induction | the preheader tests `N <= i64::MAX - s` (§3.2.1); integer overflow is defined wrap, so a loop whose last step would wrap keeps its guards by taking the slow version | planned negative owner |
| | wrapping access | admitted only when §3.2.1's `amax` and `max` guards hold; because `a > 0` and every reached `i` lies in `[e, imax]`, no intermediate `a*i + b` can wrap once `amax` did not, so the endpoint-only admission the review attacked cannot occur | planned negative owner, distinct from the wrapping-induction one |
| | `i` | the base case | planned |
| | `i + 1` | `a = 1`, `b = 1`; admission uses the maximum reached index, not `i` | planned |
| | `i * 2` | `a = 2`, `b = 0`; the admission bound is `a*imax + b` (§3.2.1) | planned (`stride_sum`) |
| | `i + off` with `off` a parameter | `b` loop-invariant but unknown and possibly negative; both ends of the range must be proved | planned (`scan_report`) |
| | `a*i + b` with `a` loop-invariant non-constant | admitted only with `a > 0` proved; otherwise not admitted | planned |
| Access kind | single index | `emit_bounds_check`'s guard | planned |
| | range `xs[i..i+k]` | `emit_range_bounds_check`'s guard; `start > end` and `end > len` both proved, including the loop-invariant overflow test 1081 §3 measures as un-hoisted | planned (`window_first`) |
| | `vecN` load/store | `emit_vec_bounds_check` delegates, so it inherits both fusion and movement | `vec_simd`, planned |
| | byte accessors | the guards `lower_bytes_read` emits for `b.u32_le(off)` and `b.set_u32_le(off, v)` are `byte_ranges`' territory; `loop_facts` does not re-prove them | `runway_a2_binary_codec`, `bytes_ops` |
| View shape | zero-length view | admission fails for any access, so the loop is versioned into a slow loop that traps identically, or runs zero times and traps not at all | `runway_a2_binary_codec::read_past_end_aborts`, planned |
| | negative index | impossible after fusion only in the sense that it still fails the unsigned compare; the trap text still reports the original signed index | `runway_a2_binary_codec::negative_offset_aborts` |
| | length-1 view | one-trip admission; the fast loop's single iteration is identical | planned |
| | length changed in the loop by `truncate` | `Stmt::ArrayTruncate { root, .. }` kills the root (§3.2 kill set); `drain` (four elements, `truncate(2)` in the body) keeps its checks and traps at `i = 2` as today, instead of reading past the published length and returning `6` | planned negative owner `drain`, executable |
| | length or contents changed by any other statement kind | each of `Store`, `StoreField`, `StoreIndex`, `StoreConstArray`, `StoreElemField`, `StoreElemFieldPtr`, `Null*Field`, `Drop*`, `DropValue`, `DropFlagInit`, `PtrStore`, `PtrStoreNoalias`, `RawStore`, `RawFree`, `ArenaEnd`, `TgEnd`, `ColumnBatch*` naming the root or a derived operand kills it; the match has no wildcard arm and a compile-time tripwire fails the build for a new variant | planned MIR-text negative owner per statement kind, plus the tripwire itself |
| | root passed to a call | a call receiving the root or a derived operand in any mode other than read-only `borrow` kills it; a call with no effect summary kills every root that is not a read-only `borrow` parameter | planned negative owners: `borrow mut` argument, owned array argument, opaque runtime call |
| | indexed view replaced in the loop | the view, not only its length, must be invariant; a borrowed replacement of the indexed binding disqualifies the loop even when the two lengths are equal | `borrowed_replacement`, planned negative owner |
| | guard length differs from the trip-count bound | admitted, and proved per guard against that guard's own length — 1081's matvec is exactly this shape | planned owner over `w[r*d+c]` and `x[c]` |
| Compilation model | whole-program | `loop_facts` runs inside `lower_program_checked_with_catalog`, so one `Program` is rewritten once | `emit_llvm_stage`, planned |
| | located MIR (`explain-opt`) | all four lowering entry points funnel through the same hook, so the located namespace (`CodegenKey.located`) is rewritten identically. Two consequences are stated rather than discovered: a duplicated body's source-line attribution is no better than any block-mutating pass leaves it, and one source loop now emits two IR loops, so per-loop remark counts change | `explain_opt`, planned located-shape owner |
| | `explain-opt` decision report | every source loop prints `loop versioned` with its budget use or `loop kept checks: <reason code>` (§3.2.3); the reason codes are stable and asserted | planned `explain_opt` owner over one admitted and one refused loop per reason code |
| | `--profile dev` versus `release` | `align_mir` carries no profile plumbing and acquires none, so the CFG is identical at every profile and only the optimizer's treatment of it differs | `build_profiles`, `emit_llvm_stage` |
| | instrument PGO | instrument and use builds see the same CFG for the same source, so counters attach consistently; a profile collected before this change mismatches and is dropped by LLVM with the existing warning, which the two-phase release flow regenerates | `pgo`, `pgo_cache`, `pgo_sv` |
| | per-unit | the transform is a pure function of the validated body already fingerprinted by `CodegenKey`'s `impl_hash`, and `compiler_build_id` covers the new code, so no key component is added and no stale object survives | `per_unit`, `cache_codegen`, `unit_cache` |
| | ThinLTO | a partition body is rewritten by the same pass and re-validated by `validate_thin_partition_program`; import decisions are unchanged because no linkage or signature changes | `thin_lto`, `function_thin_lto`, `partition` |
| | generic monomorphization | each instantiation is rewritten independently after monomorphization; an instantiation whose element type changes the admission is not forced to agree with its siblings | `generics`, planned |
| | interface serialization | nothing serialized changes; the interface hash of a unit whose bodies are rewritten is byte-identical | `per_unit_surface`, and `cache_codegen`'s `interface_hash` / `dep_interface_hashes` gates |
| Composition | `byte_ranges::simplify`, first site | runs before `loop_facts` in `lower_program_checked_with_catalog` and is unchanged; `loop_facts` derives from the simplified function and never consumes `byte_ranges`' facts, so a rolled-back byte-range proof cannot leave `loop_facts` holding a stale one | `runway_a2_binary_codec::byte_range_recurrence_preserves_tails_and_eliminates_only_proved_guards`, `paired_byte_ranges_keep_unproved_guards`, `composed_byte_effects_and_reaching_definitions_fail_closed` |
| | `byte_prepare::prepare`, the second site | runs in codegen **after** `loop_facts`, on versioned bodies: it splices leaf bodies into callers and re-runs `byte_ranges::simplify` and `snapshot_descriptors` on the result. `loop_facts` touches only element/range/vec guards and never a byte-accessor guard, so plan 64's rule that the original failure block is retained for every unproved byte edge is evaluated on guards `loop_facts` did not modify; a spliced fast copy keeps `structurally_valid` true, pinned by an owner | `runway_a2_binary_codec::composed_reader_per_unit_and_thin_cache_bind_private_body_edits`, planned owner splicing a leaf whose loop is versioned |
| | `byte_prepare`'s statement budget | its 2048/32 admission counts the post-`loop_facts` body; a leaf that straddles the budget is inlined without versioning and not inlined with it, deterministically (§3.2.2) | planned straddle owner; `composed_leaf_exposure_covers_generics_returns_and_has_bounded_growth` |
| | `annotate_par_map_work` (I8) | runs in `lower_program_unchecked_with_plans`, before `loop_facts`, so the work weight — and with it the `ParMapReduce` partition and float reduction order — is byte-identical with and without versioning; a kernel is versioned only under the same admission as any loop | planned owner asserting equal annotated weights; `par_map`, `vectorize_shapes::k6_float_sum_does_not_vectorize_without_fast_math` |
| IR identity | `Stmt::BorrowedElementReservation` in the body | not admitted: the token is function-unique and `unique_reservation` requires exactly one occurrence; the `total`/`inspect` witness (a `borrow` record element passed to a call) compiles, runs and keeps its guard | planned owner `total` (builds and runs); `cache_codegen::borrowed_element_graph_edit_invalidates_exact_dependents_and_reverts` |
| | guard published as a `BorrowedElementGuard` | keeps its signed form (§3.1) so `checked_borrowed_element_guard` still re-derives it literally; every other emitter site fuses | planned MIR-text owner asserting the signed form at the borrowed-element site and the fused form beside it |
| | other function-unique tokens or descriptors | the cloning step re-mints nothing in PR 2, so admission refuses any body containing a per-function unique marker; the refusal predicate is an exhaustive match over `Stmt` like the kill set, so a future marker variant is a build error | planned tripwire |
| Ownership | drop flags and cleanup bits across the duplicated body | the flag slots are per-binding and shared by both versions, so each version must initialize, set and clear them exactly as the original did, and the post-loop join must see one consistent flag state whichever version ran. A source-level behavior test cannot discriminate a flag-bookkeeping defect in the version that did not execute, so the discriminating owner is a MIR-text one | planned `loop_facts` owner asserting flag parity in both versions and at the join; `loop_expr::a_per_iteration_owned_string_is_freed_each_pass`, `move_return_cleanup`, `reassign_drop`, `borrow_liveness` as the executable backstop |
| | construction, move-in, move-out, source nulling, replacement, return | every owned value is constructed and dropped exactly once on the executed path, and a value moved out by `break` is moved out once across both versions | `loop_expr::a_break_moves_an_owned_value_out_once`, `owned_temporaries`, planned |
| | `arena` region inside the loop | region entry and release are lowered into the body before `loop_facts` runs, so both versions carry them and only one executes; allocation and free counts are unchanged | `fb_region`, `region_flow`, planned |
| | allocation parity | no allocation is added, removed or moved by either half | `lint_unnecessary_heap`, `runway_a2_binary_codec` |
| | runtime ownership provenance | unchanged: no new runtime call, no new handle | `resource_ownership` |
| Diagnostics | trap text and arguments | §3.3, byte-identical | `runway_a2_binary_codec::read_past_end_aborts`, `negative_offset_aborts`, `out_params::out_write_out_of_bounds_aborts`, planned zero-length/length-1/first-out-of-range owners |
| | other traps in the body | integer division and checked arithmetic keep their own guards and their own order | `div_guard`, `checked_arith` |
| Resource | code growth | the versioning budget is enforced and pinned; per-profile text size recorded | planned budget owner, `build_profiles` |
| | compile time | the pass is linear in blocks per function and is measured against the existing compiler gate budget | `docs/impl/16-test-policy.md` selection; local measurement |
| Shape | fusion reaches every emitter | every guard from the three emitters takes its §3.1 form — one unsigned compare for an element index, two plus an `Or` for a range — in every profile, whether or not the loop is versioned, and a guard whose length is not proved non-negative keeps its signed form | planned MIR-text owner over `emit_bounds_check`, `emit_range_bounds_check`, `emit_vec_bounds_check` |
| | conjunction with PR 1 | `bytes_to_f32_out` reaches a vector body only with both PRs present; a regression in either half fails the same test | plan 68's conjunction pin in `vectorize_shapes` (planned in PR 1, asserted in PR 2) |

Cross-cutting note for the whole matrix: `loop_facts` changes a safety strategy
(which bounds checks execute where), so per the cross-cutting implementation
gate this matrix and the proposed capability boundary got one fresh independent
adversarial review **before** PR 2 is coded.

**Independent matrix review, 2026-09-18, against `main` 956f3869: FINDINGS
(4 P1, 5 P2, 1 P3).** The P1s were two classes: the `BorrowedElementPlace`
channel is a second MIR-to-codegen safety contract this section did not know
existed (IR identity axis, §3.1 signed-form exemption), and the admission
predicate was stated informally where an exact analysis was required (the
per-access reaching-definition rule, the exhaustive kill set with
`ArrayTruncate`, and §3.2.1's arithmetic). The P2s fixed the `byte_prepare`
composition direction, the two interacting budgets, the par_map work-weight
ordering (I8), the missing arithmetic width, and observability; the P3 reworded
the `map_err` row. Every finding is folded into §3.1, §3.2 and this matrix in
one pass. The cells the review confirmed sound stand unchanged and are not to be
re-litigated: fusion exactness in all four sign quadrants, `emit_vec_bounds_check`'s
wrapping `idx + n` caught by the unsigned arm, trap arguments unchanged by
fusion, versioning over relocation, lambda capture never writing back the
induction slot, drop flags shared consistently by both versions, and `!range`
non-negativity as exactly the fusion precondition. PR 2's implementation
carries the author-side matrix-to-diff pass and the one ordinary preflight
review; no further plan review is commissioned unless the implementation changes
strategy.

### 3.5 Acceptance corpus

New owners in `crates/align_driver/tests/loop_facts.rs` (arch-neutral, MIR text
and raw IR): the fusion shape, each admitted index form, each negative index
form, the versioning shape, the budget, and the join rules. New owner in
`vectorize_shapes.rs`: `g2_monotone_check_hoisted` (plan 68's name) plus the
matvec kernel `w[r*d+c] * x[c]` from 1081 §2, per arch tier.

Trap parity owners are executable, not IR-shape: zero-length, length-1, and
first-out-of-range accesses each build and run, and assert both the exit status
and the exact stderr text.

The review witnesses are owners by name, each with its stated disposition:
`total`/`inspect` (borrowed element into a call: not versioned, signed guard,
builds and runs), `shifted_sum` (step before access: not versioned, traps at
`xs[n]`), `drain` (`truncate` in the body: not versioned, traps at `i = 2`),
one MIR-text negative per kill-set statement kind, the `byte_prepare` straddle
leaf, the par_map work-weight parity owner, and the `explain-opt` reason-code
owner.

### 3.6 Deferred extension: versioning a borrowed-element loop

Lifting the IR-identity refusal is a separate capability with its own cells,
because it changes codegen's safety channel and so crosses a third layer:

```text
re-mint        the cloning step assigns a fresh reservation token to each
               cloned Stmt::BorrowedElementReservation and rewrites the
               matching BorrowedElementGuard.reservation in the clone
validator      checked_borrowed_element_guard accepts the fused unsigned form,
               and accepts a guardless access in a fast copy only when the
               preheader admission fact for that access is published as a MIR
               record the validator can re-derive, so the pointer is still
               formed under a proved predicate rather than under trust
owners         the same trap-parity and MIR-text owners, plus the
               borrowed-element cache_codegen owner, run against both copies
```

Until that lands, G2's fast version excludes such loops and plan 68 records the
exclusion beside G2.

## 4. PR 3 — the trip-count exit at the latch (1084)

### 4.1 Recognition, and the named fact

Recognition lives in `loop_facts`, the module PR 2 introduces, and extends it
with the rotation record rather than adding a second derivation. The canonical
shape is exactly one spelling:

```text
header      a conditional break on `i >= bound` or `i > bound`, first in the
            loop body. Those two relations are the whole admitted set: `!=`
            and `==` do not bound the index monotonically, and `<`/`<=` are the
            admission's negation rather than its exit
step        `i = i + step` with step a positive integer constant, on every path
bound       loop-invariant
```

and it lowers to exactly one shape:

```text
preheader   the zero-trip test, peeled
header      only the body's data-dependent exits
latch       `i + step` against the same bound, with the same relation
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
(`:4198-4204`, in the **Open** section) rejects "`llvm.assume` / early intrinsic emission /
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
| Exit relation other than `>=` or `>` | `if i != n { … } else { break }` with a step that skips `n` runs to defined `i64` wrap today; a peeled and latched form need not reproduce that, so it is not recognized | planned negative owner per relation |
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
fail for any rotation defect — plus `bytes_ops`, `text_boundary`,
`runway_a2_binary_codec`, and `explain_opt`, whose existing assertions are on
the `loop(s) vectorized` / `not vectorized` report text and pin no remark count.
1084's own criterion is a drop in the `Cannot vectorize early exit loop` remark
count; that string appears nowhere in the tree today, so the remark-count owner
is planned, not reused.

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
                   G2 records the one guard excluded from both halves: the
                   borrowed-element guard keeps its signed form and its loop
                   keeps its checks until §3.6 lands. That narrows G2's fast
                   version by one named loop class and is the only promise this
                   plan changes in plan 68.
                   Earlier: two stale line citations corrected — the llvm.assume record
                   and the three section boundaries of docs/open-questions.md
                   have moved since it was written. No promise changes, and
                   this plan implements G1-G3 without widening any of them
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
- The two soundness gates of PR 1 (I3, I4) and the four of PR 2 (versioning
  rather than relocation; the IR-identity refusal; the per-access
  reaching-definition rule; the exhaustive kill set) are each stated with a
  shape that compiles today: `scan_report`, `total`/`inspect`, `shifted_sum`
  and `drain`.
- The admission arithmetic (§3.2.1) uses only `i64` operations MIR has, and
  each of its overflow cases is argued in place; the independent matrix review
  of 2026-09-18 is recorded in §3.4 and its confirmed-sound cells are marked
  not to be re-litigated.
- Every normative `align` example is syntax-checked against `alignc check`, uses
  one `loop` with newline-terminated statements, and has a repository precedent
  for each construct it uses.
- No PR consumes a capability scheduled for a later PR: PR 1 needs neither
  induction facts nor rotation, PR 2 needs PR 1's `!range` which PR 1 ships, and
  PR 3 extends PR 2's module rather than duplicating it. Plan 68 G3 names its
  fact "the single source G1's and G2's owners read"; G1's owners read no
  induction fact at all, so PR 1 shipping before `loop_facts` exists consumes
  nothing, and G2's owner is the module's first consumer.
- Neither `noalias` nor `!range` is stated anywhere this plan cannot emit it:
  `noalias` stays on read-only `borrow` headers as plan 68 G1 states, and
  `!range` stays on loads, which is the only place LLVM accepts it.
- Cache and artifact identity are stated once (§3.4) and match plan 68 §3.1: the
  object content changes, the interface hash does not, and no key component is
  added.
- The one carve-out against a standing rejection is written out in full (§4.2),
  including what it does not admit, and its failure mode is stated.
