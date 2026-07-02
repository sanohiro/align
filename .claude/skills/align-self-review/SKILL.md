---
name: align-self-review
description: Pre-PR self-review checklist for the Align compiler. Run before opening any PR that changes Rust code under crates/, to catch the recurring bug classes the gemini-code-assist reviewer keeps flagging on this repo — FFI/allocation memory-safety, soundness holes when a new IR variant (or a sibling type-class) skips an analysis pass, type-checker inference/check-ordering bugs, LLVM-IR correctness, compiler panics on malformed input, and cross-stage ABI mismatches. Invoke when about to commit/PR compiler changes, or when asked to self-review Align code.
---

# Align compiler self-review

Derived from 388 `gemini-code-assist` findings on past PRs, refreshed against the **#234–#298** record (~88 more inline findings, ~27% high-severity — the M6 `vec`/`soa`/`mask` arc, M8 `unsafe`/FFI, fuzz suite). Every high/critical finding to date lives in Gates 1–5 below. **Run these gates against your working diff before opening a PR.** Scope by what you touched — skip a gate whose files you didn't change. Gate 6 is a light sweep; don't let it crowd out 1–5.

**`gemini-code-assist` stops reviewing on 2026-07-17.** After that this checklist + `/code-review` are the *only* pre-merge defense — treat these gates as the reviewer, not a warm-up for it.

Start by listing the diff so you review the actual change, not the whole tree:
`git diff --stat origin/main...HEAD` and `git diff origin/main...HEAD`.

---

## Gate 1 — "Added a variant → update every pass" (the #1 soundness killer)

The most dangerous and most frequent critical pattern: you wire a new `ExprKind` / `Ty` / `Scalar` / `StageKind` variant into the *lowering* happy-path and call it done — but several orthogonal passes each `match` over that enum, and a `_ => Region::Static` / `_ => false` / `_ => 0` wildcard silently swallows your new variant. No compile error; a latent use-after-free or double-free.

**Fix the rule, not the instance — then sweep every parallel case.** When you close one hole, the *same-shaped* hole almost always survives in a sibling you didn't look at. A patch that names a single case is a red flag. Before calling it done, enumerate the parallel axes and check each:
- **Type-classes**: fixed a scalar array? the `str`-array / `struct`-array-with-`str`-fields carry region-tracked *elements* and hit a different branch (#297 — the scalar fix left `DynSliceArray` out of `slice_is_local`; #282 — a `str` view out of an array *literal* folds to `Region::Static`). Nested/multi-dim arrays (`[[T;n];m]`) miss single-level matches (#279).
- **`Place` kinds**: a check on `Place::Local` is bypassed by `Place::Field` / `Place::ElemField` (#281 whole-array reassign to a struct field).
- **Binding kinds**: a rule on `let` locals must also cover `mut` fn params (#279 mutating a Move-struct array param leaks).
- **Recursion / nesting**: an FFI-safe or drop check must recurse into element/field types, not just the outer shape (#267 slice-of-non-FFI-safe; #266 `layout(C)` on generics; #279 `drop_struct_fields` skipping `Ty::StructArray`).

**If your diff adds or changes a variant of `ExprKind`, `Ty`, or `Scalar`:** confirm each of these handles it *explicitly* (not via a catch-all). Grep them:

```
rg -n "fn region_of|fn tracks_region|fn null_moved_source|fn is_move|ret_is_move|impl.*MoveCheck|impl.*EscapeCheck|fn abi_type|fn scalar_bytes|fn int_type|fn int_bits|fn ty_to_scalar|fn scalar_to_ty" crates/align_sema crates/align_codegen_llvm crates/align_mir
```

Non-negotiable checklist for a new variant:
- [ ] **`region_of`** — does it return the correct region, or fall through to `Static`/`0`? (PRs #28, #40, #49, #70, #125 were all this.)
- [ ] **`tracks_region`** — if the variant can carry a `str`/`soa`/region-tied payload, it must report it. (#48, #110, #136)
- [ ] **`MoveCheck::expr`** + **`null_moved_source`** — if the payload is owned (`string`/`array<T>`/`box`/`Task`), is it nulled at every consuming site, with the right `consuming`/`tail_consuming` flag? (#42, #110, #125) Also **null the source *after* evaluating/storing the value, never before** — nulling first makes the store read the already-zeroed slot (assigns `{null,0}` + leaks). (#283); and pass `null_moved_source` the lowered MIR operand, not the HIR `&Expr` (#281).
- [ ] **MIR lowering after a sub-expr**: if a lowered `rhs` can diverge (`return` / `?`), guard the follow-up `Store`/`Goto` with `if !b.is_terminated()` — appending to a terminated block breaks the CFG. (#274)
- [ ] **`is_move` / `ret_is_move`** — correct Move-vs-Copy classification.
- [ ] **codegen**: `scalar_bytes`, `int_type`, `int_bits`, `abi_type` all agree on size/width (see Gate 4).
- [ ] **Prefer making the wildcard exhaustive** so the compiler forces the next person to update it.
- [ ] If the variant carries an owned/region payload, **write a use-after-free / double-free test** (an `arena {}` that returns or stores the value) before moving on.

---

## Gate 2 — FFI & allocation boundary (every memory-safety critical in `align_runtime`)

You reason on a 64-bit box where `i64 as usize` is lossless and `from_raw_parts(null, 0)` "looks harmless." On 32-bit, or with multiplication overflow, null pointers, or `Vec<u8>` alignment, these are heap overflows / UB. **For every `extern "C"` fn or allocation-size computation in the diff:**

- [ ] **No `as usize` on an incoming `i64` length/count.** Use `usize::try_from(len)` and bail/empty on `Err`. (#50, #66, #67, #85, #178, #193, #220, #228, #231) — grep: `rg -n "as usize|as u32|as i32" crates/align_runtime`.
- [ ] **Every size math uses `checked_mul`/`checked_add`** — `n * size_of::<T>()`, `count * stride`, ceil-div `(len + n - 1) / n` all wrap silently. Bail on `None`. (#71, #85, #228)
- [ ] **Null-check every pointer before `from_raw_parts` / deref**, including when `len == 0` (`from_raw_parts(null, 0)` is UB). Return empty/error for null. (#1, #58, #59, #170, #208, #228)
- [ ] **Alignment**: never form a `&[T]` (T ≠ u8) from a pointer derived from a `Vec<u8>` / arena bump without proving alignment; compute alignment against the *absolute* address, not the chunk-relative offset; use `read_unaligned` if unsure. (#26 arena, #153, #155, #210, #223)
- [ ] **Negative len** cast to usize wraps huge — guard `len <= 0` first. (#208)
- [ ] **`try_from` is not enough for `align`.** An alignment must be a **non-zero power of two** *and* the handle non-null before you dereference/allocate — `usize::try_from(align).ok().filter(|a| a.is_power_of_two())` + `if arena.is_null() { return core::ptr::null_mut() }`. (#293)
- [ ] **Zero-width underflow**: `bits - 1` / `w - 1` / `len - 1` on a `u32`/`usize` panics or wraps when the operand is `0` — guard `w == 0` (etc.) up front, not just the overflow at the top. (#295)
- [ ] **Sign of the width extension**: extending a size/count operand with `build_int_s_extend` sign-extends an unsigned value (MSB set → huge `size_t`). Use `build_int_cast_sign_flag(..., is_signed=false)` for unsigned. (#262)
- [ ] **String → linker/driver**: a user library/symbol name that passes char validation can still start with `-` and inject a linker flag — reject a leading `-`. (#268)

---

## Gate 3 — The compiler must diagnose, never panic

Two distinct mistakes that combine into a crash on bad input. Sema *logs-and-continues*, so `Ty::Error` and malformed nodes reach later stages where you assumed "this can't happen."

**Sema side — guard against propagating bad nodes:**
- [ ] When a sub-expression is `Ty::Error` or a check fails, **return early** with a sentinel (`Ty::Error` node / `None`) — do **not** build a typed node on top of an invalid operand. (#9, #14, #62, #74, #85, #90, #95, #178, #210)
- [ ] `let Ty::Int(it) = ty else { unreachable!() }` and friends: an operand can be `Ty::Error` — handle it, don't `unreachable!`. (#145)
- [ ] **A new builtin / operator family must reject its whole *invalid domain* in sema**, or codegen's `match`/`unreachable!` fires on real input. Restrict the operator set *and* the element type explicitly — an `else if is_vec` chain silently accepts `& | ^ >>` (#235); vector arithmetic/reductions must check the element is numeric/float (#238, #249); don't leave the guard for codegen. Return the sentinel after erroring — don't build the typed node and "continue" (#253 returned `Place::ElemField` after an immutability error instead of `Place::Err`).

**MIR / codegen side — no direct indexing of anything derived from user input:**
- [ ] No `map[key]` / `slice[i]` / `vec[i]` indexing — `funcs[name]`, `self.blocks[t]`, `structs[id]`, `arms[i]`, `xs[1..]`, `len - 1`. Use `.get(...).ok_or_else(|| self.err(...))` and `checked_sub`. (#18, #19, #34, #106, #107, #119, #160)
- [ ] No `expect(...)` / `unwrap()` / `unreachable!()` on anything traceable to user source. (#33 `expect("map before projection")` panicked on real input.)
- [ ] grep: `rg -n "unwrap\(\)|expect\(|unreachable!|\[[a-z_]+\]|len\(\) - 1| - 1\]" crates/align_mir crates/align_codegen_llvm` and eyeball each on the diff.

---

## Gate 4 — Cross-stage representation / ABI agreement

The same concept is encoded independently in 2–4 places; you update one and a `_` fallback elsewhere returns a plausible-but-wrong default (often `i32`/`64`). **If your diff changes a type's size/representation, an LLVM fn signature, a builtin, or a payload rule:**

- [ ] **Scalar size/width agree across** `scalar_bytes` ↔ `int_type` / `int_bits` ↔ `abi_type` (a mismatch = heap overflow, e.g. #26 `Scalar::Unit` 1 byte vs i32; #20 `int_bits` 64 vs `int_type` i32).
- [ ] **A new/changed runtime builtin**: the codegen `add_function` / `fn_type` declaration matches **both** the `align_runtime` `extern "C"` signature **and** every call site (param count, types, return). (#3, #24, #113, #171, #208) — declare the by-value aggregate return the same way `str_clone` does.
- [ ] **Payload-acceptance rules** (`ty_to_scalar`, what an `Option`/`Result` may carry) don't drift between sema and codegen. (#15, #56, #106 `Ty::Fn` missing in `abi_type`, #122)

**LLVM-IR correctness idioms (emitting new IR):**
- [ ] **Byte-offset / raw GEP uses `build_gep`, not `build_in_bounds_gep`** — an out-of-object `inbounds` GEP is poison and lets LLVM delete later bounds checks. Force **alignment 1** on raw un-aligned load/store, and keep the doc comment truthful about it. (#263)
- [ ] **Over-aligned struct** (`align(N)`): claiming the alignment in `type_align` without also padding the struct *size* to it (and propagating through nesting) is array-element UB (`[N x %S]` puts elements at unpadded offsets). Ship both, or reject the feature. (#246)
- [ ] **Vector ops**: `build_select`'s condition must be an `IntValue` — a `Ty::Mask` `VectorValue` needs `transmute` to the vector-select form (#236); guard `n > 0` before `build_extract_element` on a vector (#239).

---

## Gate 5 — Type-checker inference discipline (sema)

The M6 builtin arc surfaced a whole class of sema bugs around type variables and check-ordering (several high). **If your diff adds/changes a `check_*` path (a new builtin, method, or operator):**

- [ ] **No speculative "check with a hint, then `diags.truncate` to roll back".** Unification bindings are **not** rolled back — the tyvar stays bound and the type is corrupted (later codegen panic). Check the RHS *without* a hint first; only re-check with the hint if it came back a scalar/literal. (#244)
- [ ] **Check and unify all operands *before* applying a domain restriction** (float-only / numeric-only). An unbound tyvar `is_float_like() == false`, so restricting on arg 0 rejects a program the other args would have constrained. (#251)
- [ ] **`self.resolve(ty)` once, up front** — then match on it *and* format diagnostics from it. Matching/printing an unresolved `ty` yields `?0` in messages or resolves twice. (#239, #245, #285)
- [ ] **A "receiver is type X" dispatch must accept any expr of type X** — a field access, parenthesized expr, call result, or literal — not just a bare `Local`, or it silently routes to the wrong path. (#240)
- [ ] **Type-check the receiver/operands before the mutability/writability gate**, and check each node **once** — an early `return` before `check_expr(recv)` masks errors inside it; a re-check emits duplicate diagnostics. (#241, #242)

---

## Gate 6 — Light sweep (don't over-index; one pass each)

- **Diagnostics**: reusing an error helper in a new context — re-read its hardcoded message and generalize it (#26/#31 "Option payload" reused for arrays); point the span at the offending sub-node; emit one error per root cause, then return a sentinel (no cascade). (#74, #105, #117, #189, #206)
- **Perf** (mostly medium — one glance, don't chase every nit): don't clone a `Copy` type (`Ty`, small structs); `&str` over `String`; `with_capacity` for known sizes; hoist invariant work out of hot loops (#94 re-lowered captures in an inner sort loop). The bot posts one comment per line — treat fifteen near-identical clone nits as *one* lesson.
- **Concurrency** (`task_group` / `par_map`): on a worker panic, the shared counter must still be decremented and the condvar notified (guard / `catch_unwind`); never `unwrap_or(0)` a `join()` (swallows the failure); justify any `unsafe impl Send/Sync`. (#114, #117, #179)
- **Parser/lexer**: on invalid UTF-8 advance `pos` by exactly the bytes consumed, not `'\u{FFFD}'.len_utf8()` (#22, #231); lookahead must survive a newline token (#21); `//` is a comment, not a `/` line-continuation (#18); a saved/restored flag (e.g. `no_struct_literal`) must be restored on *every* exit — a `?` early-return leaks the mutated state and corrupts the rest of the parse (#272). Don't `diags.error` for something a helper you call already reports (`parse_path`→`parse_ident`) — return `None`/sentinel (#272).
- **SIMD arch parity**: a hand-written SIMD routine must ship as a **set** — x86 (`#[cfg(target_arch = "x86_64")]` + `is_x86_feature_detected!`) **and** arm64 (`#[cfg(target_arch = "aarch64")]`, NEON is ARMv8-A baseline) **and** a scalar fallback — with a test asserting every available path matches the scalar oracle byte-for-byte. Never land an x86-only intrinsic on a live path (it silently makes ARM slower); the dispatcher's non-x86 arm must reach NEON, not scalar. (`json_decode_index`/`json_structural_index` carry both; the carry-less fold pairs `pclmulqdq` with NEON `PMULL`.) Auto-vectorized loops and the `vec`/`mask` surface go through LLVM per target arch, so they need no per-arch code — only explicit intrinsics do.
- **Tests actually exercise the case**: no silent bypass — `None => continue`, `unwrap_or_default()` on `read_dir`, or an empty input set makes a test pass without asserting (#286); an "out-of-range" test must use an out-of-range index, not `s[0]` on a len-1 array (#247); every new guard/rejection deserves a negative test (null/zero/non-pow-2 align #293, leading-`-` lib name #268, assign-from-local #283). Print the diagnostics on failure so the runner shows them (#286).
- **Tests/bench portability**: unique temp paths (PID) + RAII cleanup, no leaks/races (#20, #37, #132, #134); branch on `cfg!(target_os/arch)`, don't hardcode `.so` / drop `.exe` / x86-only `target-cpu` on ARM (#128, #152, #167); a process exit code is the **low byte** on Unix but full 32-bit on Windows — `cfg!(windows)` the expected value (#287).
- **Constant-operand fast path**: when an operand is a compile-time constant, skip the runtime guard — a known non-zero divisor needs no div-by-zero / `INT_MIN / -1` check (#294). Extract a bit flag with `(x & MASK) != 0`, not `(x >> n) & 1 == 1` (arithmetic shift on signed) (#295).
- **Docs**: if the diff touches `draft.md` / `docs/`, verify §-cross-references resolve and no wording contradicts a settled decision (no turbofish, leading `.` on field predicates, columnar-not-map group_by). Also — this recurs — **don't leave a note that this same PR obsoletes**: grep the diff for `pending`/`parallel`/`deferred`/`FIXED (this PR`/`well underway` and update it to the landed state (#271, #278, #292); **re-read comments adjacent to changed code** for staleness (a doc comment referencing removed code, or split by an inserted fn) (#257, #258); enumerations must be **1:1** (6 reducers vs 5 identities #292); keep cross-file status consistent (`CLAUDE.md` ↔ `README.md` ↔ `07-roadmap.md` #271). Spelling/backtick/proper-noun nits are editorial — fix in passing, don't gate.

---

## After the gates

Fix what the gates surface, then — **until 2026-07-17** — still open the PR and **wait for the gemini review** (the repo mandate); this checklist reduces findings, it doesn't replace the review. When the review lands, scrutinize each finding against the code (some are false positives — verify, e.g. PR #232's "wyhash final-mix" claim was wrong) before applying or rejecting with a reason. **After 2026-07-17** there is no bot: run `/code-review` on the branch and treat these gates as the last line of defense.
