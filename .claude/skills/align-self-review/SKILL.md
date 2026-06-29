---
name: align-self-review
description: Pre-PR self-review checklist for the Align compiler. Run before opening any PR that changes Rust code under crates/, to catch the recurring bug classes the gemini-code-assist reviewer keeps flagging on this repo — FFI/allocation memory-safety, soundness holes when a new IR variant skips an analysis pass, compiler panics on malformed input, and cross-stage ABI mismatches. Invoke when about to commit/PR compiler changes, or when asked to self-review Align code.
---

# Align compiler self-review

Derived from 388 `gemini-code-assist` findings on past PRs (the large majority on AI-written code). Every security-critical / critical finding to date lives in Gates 1–4 below. **Run these gates against your working diff before opening a PR.** Scope by what you touched — skip a gate whose files you didn't change. Gate 5 is a light sweep; don't let it crowd out 1–4.

Start by listing the diff so you review the actual change, not the whole tree:
`git diff --stat origin/main...HEAD` and `git diff origin/main...HEAD`.

---

## Gate 1 — "Added a variant → update every pass" (the #1 soundness killer)

The most dangerous and most frequent critical pattern: you wire a new `ExprKind` / `Ty` / `Scalar` / `StageKind` variant into the *lowering* happy-path and call it done — but several orthogonal passes each `match` over that enum, and a `_ => Region::Static` / `_ => false` / `_ => 0` wildcard silently swallows your new variant. No compile error; a latent use-after-free or double-free.

**If your diff adds or changes a variant of `ExprKind`, `Ty`, or `Scalar`:** confirm each of these handles it *explicitly* (not via a catch-all). Grep them:

```
rg -n "fn region_of|fn tracks_region|fn null_moved_source|fn is_move|ret_is_move|impl.*MoveCheck|impl.*EscapeCheck|fn abi_type|fn scalar_bytes|fn int_type|fn int_bits|fn ty_to_scalar|fn scalar_to_ty" crates/align_sema crates/align_codegen_llvm crates/align_mir
```

Non-negotiable checklist for a new variant:
- [ ] **`region_of`** — does it return the correct region, or fall through to `Static`/`0`? (PRs #28, #40, #49, #70, #125 were all this.)
- [ ] **`tracks_region`** — if the variant can carry a `str`/`soa`/region-tied payload, it must report it. (#48, #110, #136)
- [ ] **`MoveCheck::expr`** + **`null_moved_source`** — if the payload is owned (`string`/`array<T>`/`box`/`Task`), is it nulled at every consuming site, with the right `consuming`/`tail_consuming` flag? (#42, #110, #125)
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

---

## Gate 3 — The compiler must diagnose, never panic

Two distinct mistakes that combine into a crash on bad input. Sema *logs-and-continues*, so `Ty::Error` and malformed nodes reach later stages where you assumed "this can't happen."

**Sema side — guard against propagating bad nodes:**
- [ ] When a sub-expression is `Ty::Error` or a check fails, **return early** with a sentinel (`Ty::Error` node / `None`) — do **not** build a typed node on top of an invalid operand. (#9, #14, #62, #74, #85, #90, #95, #178, #210)
- [ ] `let Ty::Int(it) = ty else { unreachable!() }` and friends: an operand can be `Ty::Error` — handle it, don't `unreachable!`. (#145)

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

---

## Gate 5 — Light sweep (don't over-index; one pass each)

- **Diagnostics**: reusing an error helper in a new context — re-read its hardcoded message and generalize it (#26/#31 "Option payload" reused for arrays); point the span at the offending sub-node; emit one error per root cause, then return a sentinel (no cascade). (#74, #105, #117, #189, #206)
- **Perf** (mostly medium — one glance, don't chase every nit): don't clone a `Copy` type (`Ty`, small structs); `&str` over `String`; `with_capacity` for known sizes; hoist invariant work out of hot loops (#94 re-lowered captures in an inner sort loop). The bot posts one comment per line — treat fifteen near-identical clone nits as *one* lesson.
- **Concurrency** (`task_group` / `par_map`): on a worker panic, the shared counter must still be decremented and the condvar notified (guard / `catch_unwind`); never `unwrap_or(0)` a `join()` (swallows the failure); justify any `unsafe impl Send/Sync`. (#114, #117, #179)
- **Parser/lexer**: on invalid UTF-8 advance `pos` by exactly the bytes consumed, not `'\u{FFFD}'.len_utf8()` (#22, #231); lookahead must survive a newline token (#21); `//` is a comment, not a `/` line-continuation (#18).
- **Tests/bench portability**: unique temp paths (PID) + RAII cleanup, no leaks/races (#20, #37, #132, #134); branch on `cfg!(target_os/arch)`, don't hardcode `.so` / drop `.exe` / x86-only `target-cpu` on ARM (#128, #152, #167).
- **Docs**: if the diff touches `draft.md` / `docs/`, verify §-cross-references resolve and no wording contradicts a settled decision (no turbofish, leading `.` on field predicates, columnar-not-map group_by). Spelling/backtick nits are editorial — fix in passing, don't gate on them.

---

## After the gates

Fix what the gates surface, then still open the PR and **wait for the gemini review** (the repo mandate) — this checklist reduces findings, it doesn't replace the review. When the review lands, scrutinize each finding against the code (some are false positives — verify, e.g. PR #232's "wyhash final-mix" claim was wrong) before applying or rejecting with a reason.
