This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — Option / Result / Error

> 🌐 **English** · [Japanese](./ja/option-result.md)

## Overview

The one optional model and the one error model (draft §5). `Option<T>` = maybe absent (a normal
answer); `Result<T, E>` = failed with a reason. No null anywhere in the language; no exceptions;
`?` is the only propagation. The surface is deliberately *narrow*: deconstruction is `match`,
plus exactly three conveniences (`else`, `?`, `map_err`).

## Signatures (verified)

```text
Some(x) / None                     // Option constructors — bare, not qualified
Ok(x)   / Err(e)                   // Result constructors — bare
v else fallback     -> T           // unwrap-with-fallback on Option (Some/None) OR Result (Ok/Err);
                                   //   on Result it yields Ok's value, discarding the (Copy) error
expr?                              // Try: unwrap Ok/Some, else early-return Err/None to the caller
r.map_err(f)        -> Result<T,F> // Result-only; f: fn(E) -> F; Ok passes through untouched
match v { Some(x) => …, None => … }        // exhaustive, payload binds positionally
match r { Ok(v) => …, Err(e) => … }

Error { NotFound, Invalid, Denied, Code(i64) }   // builtin; user redeclaration of `Error` rejected
error(c)                            // sugar: constructs the Code-carrying Error for Err(error(c))
```

## Type & ownership classification

`Option<T>`/`Result<T,E>` are ordinary generic sum types (monomorphized). The current implementation
still rejects several owned Move payload shapes at `scalar_arg`, with compiler-known std-handle
exceptions. The replacement is settled and mandatory before `pkg.db`: one recursive tagged
`DropPlan` admits any finite non-recursive Move payload, makes the tagged container Move, drops only
the active payload, and moves/nulls it through construction/`match`/`else`/`?`. L1a/L1b in
[`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md) owns that
implementation. Until those slices land, the existing diagnostics remain the honest compiler
boundary; new library handles must not add another exception.

The PR boundary is exact: L1a admits only `Option<string>` as an owned struct-field leaf and leaves
`Option<MoveStruct>` rejected; L1b admits Move structs/sums as Option/Result/user-sum payloads and
completes their tagged control flow.

## Effects

Pure machinery. `?` is control flow, not an effect; a function is impure only through what it
*calls*.

## Errors & aborts

- **Unhandled `Result` is a hard compile error** (the lint-suite's correctness slice, #138): a
  discarded `Result` statement must be `?`-propagated, matched, or bound.
- `?` performs **no implicit error-type conversion** — `Result<T, MyErr>` does not flow through a
  `Result<T, Error>` context; convert visibly with `.map_err(to_error)`.
- `main() -> Result<(), Error>`: an escaping `Err` maps to the process exit code — categorical
  variants exit `tag + 1` (`NotFound`→1, `Invalid`→2, `Denied`→3), `Code(c)` exits `c` (#308
  restricted `main`'s error type to the builtin `Error`; a user `E` in `main` is rejected).

## Regions

None of their own; a payload view (`str` in an `Ok`) keeps its own region.

## Spec'd but not implemented

- **No combinator methods**: `.map`, `.and_then`, `.unwrap_or`, `.ok()`, `.is_some/.is_none/
  .is_ok/.is_err` do not exist — the method table stops at `map_err`. This is currently a
  *stance*, not a gap-by-accident: `match` + `else` + `?` cover the uses without growing a
  second, combinator-flavored control-flow dialect. Adding any of them is a design decision
  (One-way review) — record in `open-questions.md` before implementing.
- **Remaining recursive tagged Move slices:** L1b-a implements direct existing-`Scalar` Move
  payloads per tagged arm, including recursive `Drop`, `match`, `else`, `?`, and `map_err`.
  L1b-b additionally implements multiple Move payload partial construction and requires one
  uniform allocation mode across those payloads. L1b-c still owns nested tagged payload
  representation such as `Result<Option<T>, E>`. These are settled implementation slices, not
  remaining API decisions. Arrays of arbitrary new Move-element layouts and recursive types remain
  separate.

## Pitfalls

- P1 — constructors are **bare** (`Some`/`Ok`), unlike user sum types (`Type.Variant`). Docs and
  diagnostics must not suggest `Option.Some`.
- P2 — a payload-less generic variant alone (`Opt.None`-style in user generics) can't pin `T`;
  the builtin `None` relies on context. Tests that construct bare `None` need an annotation or a
  flow context.
- P3 — `error(c)` is the only sugar; do not add per-variant constructors or auto-conversions —
  the visibility of `map_err` at the boundary is the point.
- P4 — exit-code mapping is part of the language contract (guide ch04 teaches it); changing the
  `tag + 1` scheme is a breaking spec change, not an implementation detail.

## Test anchors

`crates/align_driver/tests/enum_match.rs` (Error variants, `error(c)` → exit code, `map_err`
conversion, no-implicit-`?`-coercion, exhaustiveness); `m1.rs`/`m2.rs` Option/Result basics +
`?`; `generics.rs:229` (`o else d` in a generic fn); `else_result.rs` (`else` on `Result` — Ok
passthrough / Err fallback / nested chains / Move-Ok no double-free / recursive Move-Err Drop);
`owned_tagged_payloads.rs` (Move struct/sum/string/handle, all control edges, per-unit parity);
`lint_unhandled_result.rs`; #308 main-error
restriction tests; examples `option.align`, `result.align`, `match_option_result.align`,
`error_enum.align`.
