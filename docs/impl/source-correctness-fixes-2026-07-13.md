# Source correctness fixes — 2026-07-13

Status: **implemented and regression-pinned in the working tree.** This record covers the focused
source audit and is self-contained: each corrected failure, root cause, and permanent gate is listed
below.

## Shipped fixes

| Area | Failure before the fix | Correction and regression gate |
|---|---|---|
| `str[a..b]` UTF-8 boundaries | A start or end inside a multibyte scalar produced an invalid `str` | After the ordinary range guard, MIR accepts `0`/`len` or checks `(byte & 0xc0) != 0x80`; split start/end abort through `align_rt_utf8_boundary_fail`. `m5.rs` covers 1/2/3/4-byte boundaries and both failing endpoints. |
| Unit function-value ABI | A `void` thunk was called through an `i32` indirect-call signature | Unit `CallIndirect` now uses LLVM `void(env,args...)` and yields no stored value. Raw IR rejects `call i32`; `fn_values.rs` executes both sides of a runtime-selected Unit target. |
| Buffered `io.copy` | A preceding `read_line` could leave unread lookahead that copy skipped by reading the fd directly | Copy uses the shared reader path, which drains lookahead before fresh fd bytes. The `AB\nCDEFG` gate copies exactly `CDEFG` and reports 5. |
| Closure-call result region | A zero-argument closure could return an arena-backed captured view as `Static`, then use it after `arena_end` | `region_of(CallFnValue)` now includes the callee/environment region before folding argument regions. `analysis_coverage.rs` rejects the zero-argument capture reproduction. |
| Spawn capture region | A task spawned inside an inner arena could retain a view until the enclosing task group's later `wait`, after the arena had freed its backing | `EscapeCheck` tracks active task-group regions and requires every region-bearing capture to outlive the innermost group, with a whole-expression fail-closed fallback for future HIR widening. `task_group.rs` rejects direct/parenthesized, struct, tuple, `Option`, `Result`, and nested-closure cases while accepting frame/static/outer-arena captures; local/block function expressions remain rejected at the literal-only surface. |
| Post-`where` callable execution | Reducing pipelines evaluated later stages and callable reducers for rejected elements, so a false filter could still divide by zero, fail bounds checks, allocate, loop, or perform I/O | MIR branches rejected elements around every general callable suffix and callable reducer. Safe field operations plus builtin `sum`/`count`/`min`/`max` retain mask/select. `branchless_where.rs` pins traps, `reduce`/`any`/`all`, later predicates, and Impure source order; `vectorize_shapes.rs` pins masked vector positives. |
| Parallel function-value effects | A lifted capturing closure omitted its call-graph edge, and a higher-order parameter had no effect in `FnTy`; either could let I/O enter an accepted Pure `par_map` | `EffectScan` adds the lifted edge and propagates unknown-indirect separately from I/O, rejecting unknown effects only at Pure/parallel boundaries. `analysis_coverage.rs` pins both negative paths and a legal sequential HOF control. |
| Borrowed call result as pipeline source | `whole(xs) -> slice<T>` was classified as an owned call temporary; `sum` and parallel `par_map` called `free` on the caller's stack/borrowed pointer | The shared source-drop predicate now classifies calls by return ownership: `array<T>` transfers ownership, `slice<T>` never does. MIR and runtime tests cover `sum` and `par_map`, while existing owned-array call tests retain their drops. **New finding.** |
| `buffer.append` self-alias | `b.append(b.bytes())` could reallocate `b.data` before copying from its old pointer, causing a UAF | The runtime detects overlap with the current allocation and snapshots the source before truncate/growth. A forced-growth doubling test pins byte identity. **New finding.** |
| Line-head `!=` | The lexer inserted `END` before a next-line `!=`, although line-head binary operators continue the expression | `!` suppresses `END` only when followed by `=`; a bare unary `!` still starts a new statement. Lexer tests pin both sides. **New finding.** |
| Duplicate struct fields | `P { x: i32, x: i32 }` entered HIR with ambiguous lookup and became impossible to construct coherently | Type collection diagnoses the second occurrence for concrete and generic structs before building their field tables. Sema tests require both diagnostics. **New finding.** |

## Ownership rule clarified by the audit

Syntax alone is not an ownership proof. In particular, `ExprKind::Call` says how a value was
produced, not whether its `{ptr,len}` owns the pointer. Pipeline cleanup must use the checked return
type and expression kind together:

```text
call -> array<T>       owned heap temporary; consuming pipeline drops it
call -> slice<T>       borrowed view; consuming pipeline never drops it
chunks(...)            owned heap header array; drop even inside arena
materializer in arena  arena-owned; arena end drops it in bulk
materializer outside   owned heap temporary; consuming pipeline drops it
```

The reducing, collecting, and parallel paths now share this classification so a future terminal
cannot silently reintroduce the `slice<T>` mis-free.

## Verification

The permanent gates live in:

- `crates/align_codegen_llvm/src/lib.rs` and `crates/align_driver/tests/fn_values.rs` (Unit ABI);
- `crates/align_driver/tests/m5.rs` (UTF-8 slicing);
- `crates/align_driver/tests/mmv2.rs` and `par_map.rs` (borrowed vs owned pipeline sources);
- `crates/align_driver/tests/m12_read_line.rs` (buffered copy);
- `crates/align_driver/tests/task_group.rs` (spawn-capture lifetime);
- `crates/align_driver/tests/branchless_where.rs` and `vectorize_shapes.rs` (guarded callable suffixes and safe masked vector shapes);
- `crates/align_driver/tests/analysis_coverage.rs` (lifted and higher-order parallel effects);
- `crates/align_driver/tests/runway_a2_binary_codec.rs` (self-append);
- `crates/align_lexer/src/lib.rs` and `crates/align_sema/src/lib.rs` (front-end diagnostics).

The audit intentionally did not mark unrelated broader findings complete. Scheduler forward
progress, allocation-size overflow,
arena-free owned-temporary leaks, and the remaining short-input work retain their recorded status.
