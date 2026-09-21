# Immutable raw-null module constant

Status: design candidate for issue 1159 and align-llm Request 117.

This capability adds one compile-time sentinel, not general call evaluation in
constant initializers. The constant declaration, substitution, interface
transport and backend proof form one strict producer-to-consumer chain; there
is no useful intermediate surface to split into another PR.

## 1. Public-contract ledger

```text
Surface           NAME: raw := raw.null()
                  pub NAME: raw := raw.null()

                  The annotation may be omitted because the exact initializer
                  fixes the definition type to raw. A same-module constant may
                  alias the value under the existing constant-reference rule;
                  every such value remains the same null sentinel.

Initializer       The only new expression admitted by constant evaluation is
                  a Call whose callee is the exact unshadowable module path
                  raw.null, with zero arguments. Parentheses add no AST node.
                  raw.alloc, raw.offset, raw.load/store/free, an unsafe block,
                  a user function, a method, arguments, or any other call
                  remains rejected. Normal expression use of raw.null() still
                  requires an unsafe block; only this compile-time initializer
                  grammar is exempt because it cannot inspect or mutate memory.

Value             The result type is raw and its target value is exactly the
                  all-zero null pointer. It is Copy, immutable and may be read
                  outside unsafe. Pointer inspection and every other raw
                  operation retain their existing unsafe requirements. No
                  non-null raw value can be constructed by constant evaluation.

Ownership         The value owns no storage, resource or lifetime and has no
                  Drop action. Substitution copies the null pointer value.

Allocation        None. The declaration emits no runtime initializer, global
                  storage, constructor call, allocation or hidden cleanup. Each
                  use substitutes the existing HIR RawNull leaf, which lowers
                  to the existing MIR RawNull and LLVM null pointer constant.

Errors            A nonzero argument count or any near-miss call is rejected by
                  the existing restricted-initializer diagnostic; a non-raw
                  annotation reports the existing annotation/value mismatch.
                  Declaration type formation is checked before initializer
                  folding. A referenced failed constant remains the Error
                  sentinel and does not cascade or panic.

Effects           Pure and side-effect free. It does not make an enclosing
                  function unsafe or Impure because no operation executes at
                  the declaration or use site.

Owner             align_sema owns recognition, folding and literal
                  substitution. Existing checked HIR, MIR and LLVM RawNull
                  owners retain their exact type and target-null proofs.
                  align_interface transports a public declaration's existing
                  type plus exact initializer source and importers reparse and
                  re-fold it before any HIR or code generation.

Interface/wire    IConst already represents ty=Named("raw") and UTF-8
                  value_src="raw.null()". Both fields already participate in
                  interface_hash. No tag, field, ordering or format-version
                  change is required. Invalid UTF-8 is already rejected by the
                  interface codec; a decoded altered initializer is parsed and
                  checked by the same closed constant grammar before consumer
                  object/cache publication. Whitespace is ordinary source
                  spelling and therefore remains part of the existing hash.

Artifact/cache    Editing the exported type or initializer changes the existing
                  interface hash and dependent cache identity. Local source
                  identity already changes for a private constant edit. There
                  is no runtime ABI symbol, static artifact or new cache input.

Validation order  Resolve an optional annotation; recognize an exact
                  zero-argument raw.null call; check its raw type against the
                  annotation; memoize it. A malformed call never reaches normal
                  unsafe-call checking from the const evaluator. Importers run
                  the identical order on synthesized interface source.

Prerequisite      The shipped raw type and HIR/MIR/LLVM RawNull lowering, plus
                  the shipped source-carried public-constant interface.

Acceptance        Local private/public and imported uses produce exact null in
                  whole-program and per-unit builds. Interface summaries retain
                  raw and raw.null(). Raw and optimized LLVM contain a null
                  pointer and no constructor, allocator or module initializer.
                  Non-null operations and arbitrary calls remain rejected.

Mirrors           draft.md, docs/language-spec.md, docs/design-notes.md,
                  docs/open-questions.md and docs/impl/02-frontend.md.
```

The exemption belongs to constant evaluation, not to the normal unsafe checker.
That keeps ordinary native-boundary operations visible while allowing packages
to name a universal ABI sentinel once.

## 2. Implementation closure matrix

| Cell | Required behavior | Owner evidence |
|---|---|---|
| Formation | annotated and inferred direct null constants fold to one raw value; a raw-null alias remains null; wrong annotation rejects | constant sema and diagnostic cases |
| Substitution | bare and qualified references become the existing typed RawNull HIR leaf with no declaration artifact | HIR inspection plus local execution |
| Restricted grammar | alloc/offset/load/store/free, user calls, unsafe blocks, arguments and field/method near misses all reject; ordinary raw.null remains unsafe-only | parameterized negative source owner plus existing raw-unsafe owner |
| Interface | summary records raw plus exact initializer source; whole/per-unit accept and reject the same cases; edited source changes interface identity | interface summary/hash and per-unit twins |
| MIR/backend | the use lowers to existing RawNull and LLVM null; raw and optimized IR contain no allocator, constructor call, global initializer or hidden allocation | MIR assertion and LLVM structural scan |
| Control paths | null constants pass through binding, parameter, return, if/match/loop joins and repeated reads as Copy without cleanup | existing raw Copy owners plus focused branch/return execution |
| Malformed input | invalid interface initializer and forged non-raw RawNull type reject before object/cache publication without panic | interface reconstruction negative and existing checked-HIR/MIR validation |

The enum addition is limited to `ConstVal`; every exhaustive use is updated in
one pass. No AST, HIR, MIR, interface or runtime variant is added.

## 3. Deliberate exclusions

There is no general pure-function constant evaluation, unsafe constant block,
compile-time FFI, non-null raw constant, pointer arithmetic, address-to-integer
conversion, static mutable pointer, new optional-value model or runtime global.
`Option<T>` remains the sole ordinary absence model.
