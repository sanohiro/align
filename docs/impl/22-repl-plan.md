# `align-repl` plan of record

`align-repl` is an AOT REPL for Align: an editor for one growing Align program,
where every entry recompiles the whole session with the real compiler and runs
the real binary. This document is the plan of record and the public-contract
ledger for it. It adds no language surface, so `draft.md`,
`docs/language-spec.md`, and `docs/open-questions.md` are unaffected.

## Settled direction

| # | Settled | Where it is honored |
|---|---------|---------------------|
| S1 | No interpreter, no JIT. Every entry AOT-compiles the whole session and runs the real binary. | §3, §4 |
| S2 | Built on the existing crates. | §9, §11 |
| S3 | Nothing that exists gets slower. | §10 |
| S4 | A learning tool, but behavior byte-identical to a production compile. | §4.1, §7 |
| S5 | Benchmarking (`:time`) is a wanted use. | §8 |
| S6 | Not inside `alignc`. New crate `crates/align_repl`, binary `align-repl`. | §9, §10 |

## 1. Model

`align-repl` maintains a **living Align program**: an ordered list of *entries*.
Every entry is classified, spliced in at a deterministic position, compiled
through **the exact code path `alignc build` uses**, and executed as a real
native subprocess. There is no residual runtime state between entries — earlier
effects are reproduced by **re-executing them**, the only model that keeps S4
true. `:save` writes exactly the program that was compiled.

**The REPL emits no synthetic code that the user did not write, with one named
exception (§3.4 case D).**

## 2. Non-goals

Permanent (they would break S1 or S4):

- **N1** JIT, bytecode, or any interpreter.
- **N2** Incremental *runtime* state: keeping the previous entry's values in
  memory, hot-patching, or resuming a process.
- **N3** Suppressing or caching the *side effects* of earlier entries.
  Re-execution is real execution. §6 governs display, never whether it happens.
- **N4** A REPL-only dialect: no auto-`mut`, no implicit `?`, no implicit deep
  `print`, no relaxation of shadowing or of `unhandled Result`.

v1 only:

- **N5** Cursor-based multi-line editing, history search, completion, colouring.
- **N6** `pkg.*` and user-module resolution — the session is a one-unit package
  with no project root (V2c `--project DIR`).
- **N7** Profile / target / LTO switching inside the session (V2a).
- **N8** `main(args)`, exit codes, stdin forwarding to the program.
- **N9** Replay UX beyond "pipe a file into stdin", which already works.
- **N10** Signal handling. Ctrl-C during execution kills `align-repl` too (§8.10).
- **N11 Region-scoped values cannot span entries.** `arena { … }` and `heap.new`
  are block-scoped by the language, and every entry is a statement in one `main`,
  so a heap box or arena-owned value allocated in entry *n* is dropped at the end
  of entry *n*'s own `arena` block and cannot be referenced by entry *n+1*. No
  REPL mechanism can change this without an implicit session-wide arena, which
  would be hidden allocation and is what N4 forbids. **Consequence:** heap
  allocation cannot be explored one line at a time. `:help` says so and gives the
  working form — type the whole block as one entry (§9.1):

  ```align
  arena {
    p: box<i32> := heap.new(42)
    print(p.get())
  }
  ```

  Lifting this needs its own design (V2g); it is recorded here so the limitation
  is not mistaken for an oversight.

## 3. Session model

### 3.0 Pipeline order

Every entry passes through these stages, in this order, once. No stage runs
twice and no later stage feeds back into an earlier one.

```text
0  lex-only triage      no significant tokens            -> NO-OP, prompt again
1  classify             §3.3  -> region + entry kind
2  region-conflict check §3.5 -> reject before any compilation
3  replacement resolve  §3.5  -> which existing ordinals this entry displaces
4  echo form            §3.4  -> the exact text spliced into the program
5  build                §4.1  -> the alignc build path
6  run                  §4.2 / §6
```

Stages 3 and 4 both need a checked candidate and use **the same one**: stage 4's
accepted candidate *is* the program stage 3 resolved. Concretely, stages 3–4 are
one loop of at most three candidate checks (§3.4), and the replacement decision
(§3.5.1) is taken from the first candidate whose failure is duplicate-only. Any
failure at any stage rolls the session back completely (§3.7.2); nothing is
executed.

### 3.1 The invariant §3 exists to protect

> **An echo must not consume the user's value, and must not require the REPL to
> reason about ownership at all.**

The design satisfies this by **not generating bindings**. Verified against the
compiler:

```text
print(p.get()) twice on a `box` local        -> clean          (print borrows)
`p` alone as a statement, then `p.get()`     -> clean          (a bare place
                                                                expression does
                                                                NOT move)
`_1 := p`, then `p.get()`                    -> use of moved value 'p'
```

The third shape is the only one that moves, and the REPL emits it in exactly one
narrowly defined case (§3.4 case D), where the move is stated rather than
analysed away.

### 3.2 Regions and emission shape

The generated file has four **regions**, in the only order the grammar allows
(an `import` after any item is
`error: expected `fn`, a type declaration, or a constant (`NAME := …`) at top level`):

```align
// generated by align-repl; every line below is real Align
import <…>                                        region 1  IMPORT
<:const entries>                                  region 2  CONST
<fn / struct / sum / resource / extern entries>   region 3  DECL
fn main() -> Result<(), Error> {
  <binding / statement / value entries>           region 4  MAIN
  return Ok(())
}
```

An entry's region is fixed at classification and **never changes**. Regions 2
and 3 are order-independent (`fn`, type, and constant forward references all
check), so insertion order there is presentational; region 4's order is
semantic.

`main` is always `-> Result<(), Error>`: it compiles with zero fallible
operations present, exits 0, and is the only signature that lets `?` appear
later without rewriting `main`. Exit codes are N8. The signature is visible in
`:list` and `:save`.

### 3.3 Classification

```text
0. The entry has no significant token (blank, or comments only)
        -> NO-OP. The session is not rebuilt and the program is not re-run, so
           holding Enter does not replay every side effect in the session.
1. First significant token is `import`, `fn`, `extern`, or `resource`
        -> DECL (region 1 for `import`, region 3 otherwise). No ambiguity is
           possible: none of these begins a statement. Diagnostics for a broken
           DECL report at FILE position (§5).
2. Else if the entry does not parse as a file-scope item list  -> MAIN (region 4).
3. Else if the entry does not parse as a statement             -> DECL (region 3).
           e.g. `P { a: slice<u8> }`, whose field position is not an expression.
4. Else both parse — the `Ident { … }` shape. A **duplicate/shadow-class error is
   not a rejection here**: it is precisely the signal that §3.5.1 owns this
   entry, so it counts as accepted for routing and the region decision is made
   on it.
        check the MAIN candidate:
            clean, OR every error is duplicate/shadow-class   -> MAIN, then §3.5.1
        else check the DECL candidate:
            clean, OR every error is duplicate/shadow-class   -> DECL, then §3.5.1
        else (both contain a non-duplicate error)             -> MAIN; report the
            MAIN candidate's diagnostics and roll back.
```

A lone `Item::Const` never reaches step 4 as a declaration: `NAME := expr` parses
as a statement, so it never survives step 2 or 3, and step 4's MAIN candidate
checks clean for any well-formed initializer. It lands in region 4 by the
ordinary path — `x := load()` is a `main` local. The genuine compile-time
constant is `:const` (§8), which is also the only form a `fn` entry can
reference, because a `fn` cannot see `main`'s locals. `:help` says so.

**Why step 4 needs a check at all.** Neither parsing nor token shape decides it:

```text
`Point{x: a, y: b}` at file scope -> PARSES as a struct declaration; fails in
                                     SEMA (`duplicate type declaration: 'Point'`)
`P { a: i64 }`      as a statement -> PARSES as a struct literal; fails in SEMA
                                     (`unknown type: 'P'`)
`P { a: x }`, x a live local, as a statement -> parses AND checks
```

The language's own rule is that keyword-less type declarations are disambiguated
by their **contents**, and only sema knows whether `i64` in field position is a
type or a value.

**Why duplicate-tolerance is required.** Rewriting a type is a central gesture in
a learning REPL. Redeclaring an existing `T` with `T { a: i64 }` produces a
non-duplicate error on the MAIN reading (`undefined name: 'i64'`) and a
duplicate-only error on the DECL reading. If the DECL arm demanded a *clean*
candidate, the entry would fall through to the both-error arm, the user would see
an unrelated diagnostic, and §3.5.1 would never be reached.

**Determinism.** The MAIN candidate is evaluated first and its verdict is final
when it is clean or duplicate-only, so the procedure is total and
order-deterministic by construction — no tie can arise. A structural fact
corroborates it: MAIN-clean requires `T` to be a declared struct while DECL-clean
requires `T` to be undeclared, so **both-clean is impossible**. Two overlaps are
resolved by the same first-wins ordering rather than by a preference:

```text
MAIN clean  ∧ DECL duplicate-only   reachable: `P { a: x }` where P is a declared
                                    struct with a field `a` of type `x`, and `x`
                                    is also a type name -> MAIN
MAIN dup-only ∧ DECL clean          UNREACHABLE: MAIN being well-formed-but-
                                    duplicate requires `T` declared, which makes
                                    DECL a `duplicate type declaration`
```

**Traced cells** (each diagnostic is the compiler's actual output):

| cell | MAIN candidate | DECL candidate | route | outcome |
|---|---|---|---|---|
| (a) `x := 2`, `x` an existing region-4 local | `` `x` is already bound in this scope chain … `` — dup-class only | also dup-only, never evaluated | **MAIN** (first wins) | §3.5.1 attempt 2 replaces the region-4 entry owning `x` in place; re-runs with the new value |
| (b) `T { a: i64 }`, `T` an existing region-3 type | `undefined name: 'i64'` — not dup-class | `duplicate type declaration: 'T' in module 'main'` — dup-only | **DECL** | §3.5.1 attempt 2 replaces the type entry in place |
| (b′) `S { A, B }`, `S` an existing sum type | as (b) | `duplicate type declaration: 'S' in module 'main'` | **DECL** | as (b) |
| (c) `Point{x: a, y: b}`, `Point` declared, `a`/`b` undefined | `undefined name: 'a'`, `undefined name: 'b'` — not dup-class | `duplicate type declaration: 'Point' in module 'main'` — dup-only | **DECL** | §3.5.1 attempt 2 fails with `unknown type: 'a'`; the entry **rolls back**, reporting those diagnostics plus the replaced-ordinal note |
| (c′) the same text once `a` and `b` exist as locals | clean | — | **MAIN** | ordinary append |

Cell (c) is a **misroute the design accepts and discloses**, not an oversight.
The text is genuinely a well-formed type redeclaration and a malformed struct
literal at once, and no discriminator short of guessing intent separates them.
Two properties keep it honest: the note names the ordinal being replaced, so the
user can see which reading was taken; and the routing follows what the program
*can* mean — (c′) is the same text with a different answer.

**Sum types are the same shape.** Align's sum type is keyword-less and
brace-delimited, `Shape { Circle(i32), Rect(i32, i32), Point }`
(`examples/enum_match.align`); there is no `Ident = … | …` form
(`Shape = Circle(i64) | Square(i64)` at top level is
`error: expected `fn`, a type declaration, or a constant (`NAME := …`) at top level`).
Sums are therefore already inside step 4's `Ident { … }` class and behave as
cell (b′).

**Namespaces.** `fn f` and a local `f` coexist; a type `x` and a local `x`
coexist. Functions, types, and value bindings are separate namespaces. A
top-level **constant** and a `main` local do *not* coexist (`K := 5` at file
scope plus `K := 2` in `main` gives
`` `K` is already bound in this scope chain ``). §3.5 is built on exactly these
facts.

### 3.4 Echo: candidates, not synthesis

For a region-4 entry, at most three candidate programs are formed. Each is the
session plus one line. **The accepted candidate is what gets built**, so the
check that accepted it and the build that follows compile byte-identical source
and the in-process whole-program memo hits.

```text
P = session + `print(<E>)`     formed only if it PARSES (a parse-only prefilter)
S = session + `<E>`
D = session + `_ := (<E>)`

1. If P was formed and check(P) is clean
       -> printable. EMIT `print(<E>)`. Echo: the printed value itself.
2. Else if check(S) is clean
       -> EMIT `<E>` verbatim.
          If E's trailing statement is an expression, echo its type only,
          rendered by `ty_display`. Nothing is bound and nothing is moved: a bare
          place expression of a Move type does not consume it, and a type echo
          touches no value.
3. Else if every error in check(S) is `unhandled Result` at E's own span
       -> D. check(D) clean -> EMIT `_ := (<E>)`. Echo the Result type plus:
          `note: a Result must be handled — `?`, `match`, or bind it with a name`
4. Else -> user error. Report check(S)'s diagnostics. Roll back.
```

**The one synthetic line, stated plainly.** Case 3 emits `_ := (E)`. When `E` is
a **place expression of Result type** (a bare `r` naming an existing
Result-typed local), this **moves** `r`, and a later entry naming `r` fails with
`use of moved value 'r'`. This is not analysed away and not hidden: the echo
prints the `note:` line, and `:undo` reverses it. It is kept because the
alternative — silently dropping a fallible expression the user typed — is worse,
and because Align's own rule is that a `Result` must be handled. `_ := (E)` is
legal for both a `Result` and a plain expression.

**`_` is not a name, so case D is repeatable.** Two `_ := (…)` statements in one
function check clean, and `(a, _) := t()` followed by `_ := (5)` checks clean.
`_` creates no local, contributes nothing to the §3.6 name set, and can never
collide with itself, with a user binding, or with another case-D entry. Without
this the design would cap a session at one bare fallible call.

**Cost.** The common interactive gesture — type a name or an expression to see a
value — is one check plus a memo-hit build. Aggregates cost two checks; a bare
fallible call costs three. §13 quantifies it.

**Errors always come from S**, so no synthetic text appears in a message the user
reads. The one exception is §3.3 step 1 (keyword-first DECL), whose diagnostics
report at file position because that is where the user's text is.

### 3.5 Rebinding = in-place replacement within one region

Align forbids shadowing across the scope chain, and duplicate items are hard
errors:

```text
`x` is already bound in this scope chain; a name binds once (no shadowing) — use
`mut` for a value that changes, or a new name
duplicate function 'f' in module 'main'
duplicate type declaration: 'P' in module 'main'
duplicate constant `x` in module `main`
```

> **An entry whose name set intersects an existing entry's, in the same region,
> replaces that entry at its original ordinal.** Every other entry keeps its
> ordinal.

- **In place, not appended** — load-bearing. Appending would put a new `x := 2`
  after a `print(x)` typed earlier, giving `undefined name: 'x'`. Replacing in
  place keeps every later entry resolvable, and re-execution honestly shows those
  entries running against the new value. That is the specified semantics: edit
  the program, then re-run it.
- **Region-scoped.** A match is a replacement only when both entries are in the
  same region. `fn f` never displaces a local `f`, and a type `P` never displaces
  a local `P` — they are separate namespaces and do not collide at all, so there
  is nothing to resolve.
- **A cross-region collision is refused, not silently resolved.** Exactly one
  pair collides across regions: a region-2 constant and a region-4 local of the
  same name. The REPL rejects the entry with one diagnostic and changes nothing:

  ```text
  align-repl: `K` is already a top-level constant (entry 3); a `main` binding
  cannot use that name — pick another name, or replace the constant with
  `:const K := …`
  ```

  Deleting a `:const` because a `main` binding reused its name would destroy an
  entry the user never referred to.
- A replacement whose new text depends on something defined *after* the replaced
  position fails to compile (`undefined name`), rolls back, and the REPL names
  the ordinal it was replacing. No fallback, no retry ladder.
- An entry displacing several entries takes the **lowest** displaced ordinal; the
  others are removed. `:list` shows the result before the next prompt.

#### 3.5.1 Resolving replacement before a candidate can compile

A duplicate name is a hard error, so the candidate that would tell us the name
set cannot compile. The resolution is one deterministic two-attempt sequence:

```text
attempt 1: candidate = session + E (appended in E's region).
   clean                                    -> no collision; append.
   errors, at least ONE of which is in the duplicate/shadowing class -> attempt 2
   errors, none in that class               -> user error; report and roll back.
attempt 2: N = (this entry's candidate name set, §3.6) ∩ (the union of existing
           `Entry.names` in the SAME region).
   N empty     -> the duplicate came from something the REPL does not own;
                  report attempt 1's diagnostics and roll back.
   N non-empty -> candidate = session with the entries owning N replaced in place.
       clean   -> accept.
       errors  -> report THESE, plus one line naming the replaced ordinals;
                  roll back.
```

This sequence owns region 4, whose binding names are available only from HIR.
Regions 2 and 3 have exact entry-AST names before checking, so they perform the
same same-region intersection and in-place replacement before their single
candidate check. This is required for `extern "C"`: Align currently accepts a
repeated extern declaration without a duplicate diagnostic, so waiting for an
error would silently accumulate two REPL-owned entries for one symbol.

`N` is computed from the **HIR delta intersected with existing `Entry.names`**
(§3.6), never parsed out of diagnostic text. Diagnostic strings are pinned by an
owner test only to prove the *class* test still recognises them, not to extract
names.

**The two duplicate-class tests are deliberately different and must stay so.**
§3.3 step 4 requires **every** error to be duplicate-class; attempt 1 requires
only **at least one**. Step 4 is choosing between two *readings of the text*, and
a single non-duplicate error proves that reading is wrong (cell (b): the MAIN
reading's `undefined name: 'i64'` is decisive). Attempt 1 has already fixed the
region and is only asking "is a rebinding what happened here?", where a duplicate
plus downstream `undefined name` noise is the normal shape. An owner test pins
both thresholds so a later simplification to one predicate fails.

Five compiler message classes are recognised: local shadowing, duplicate
function, duplicate type declaration, duplicate constant, and duplicate import.
The type class covers structs, sums, and resources. There is no sixth extern
class: the compiler intentionally accepts repeated compatible extern declarations,
so the AST-owned region-3 pre-resolution above owns extern replacement. An owner
test pins all five strings and separately proves that an extern entry replaces in
place.

### 3.6 Candidate name set: HIR delta ∩ entry-top-level

After a candidate checks (or fails only on duplicates), the entry's **candidate
name set** is computed mechanically. Two filters, both necessary:

```text
HIR delta — names present in the candidate's `align_sema::Program` and absent
            from the last accepted program's:
  region 4  the entry `main` Fn's added `Local`s, read through `Local::name`,
            whose type is `String` and is never optional — a local always has a
            name. One path covers `Stmt::Let`, `Stmt::LetTuple`, and `mut`
            bindings. An ignored tuple element is `None` in
            `LetTuple::locals: Vec<Option<LocalId>>` and therefore creates NO
            `Local` at all, so `(a, _) := t()` contributes exactly `a`; likewise
            a standalone `_ := (E)` (§3.4 case D) contributes nothing. `_` is
            never a member of any name set.
  region 3  `Program::fns[].name` EXCLUDING any name containing `$lambda` (a
            lifted lambda is named `{enclosing_fn}$lambda{n}`, compiler-generated
            and never a user name); `Program::structs[].source_name`,
            `Program::enums[].source_name`, `Program::resources[].source_name`;
            `Program::externs[].name` (the literal C symbol, never mangled)
  region 1  not from HIR: `Program::link_libs` is a link input, not a namespace;
            import identity is the rendered dotted path (§3.7.1)
  region 2  not from HIR at all — a top-level constant is folded to a literal at
            every use and has NO entry in `align_sema::Program`. `:const` names
            come from the entry's own AST `Item::Const`, which is exact because
            `:const` takes the declaration form verbatim.

∩ entry-top-level — the binding names appearing at the ENTRY's own top statement
            level in its AST. Required because HIR flattens every local of a
            function into one `Fn::locals`, so a nested block's bindings would
            otherwise look like the entry's own: an `arena { p := … }` entry
            followed by a later `p := 9` checks clean, because `p` is
            block-scoped and must NOT be treated as a name the arena entry owns.
            Lambda parameters are excluded by the same filter — they live in the
            lifted function, whose name is `$lambda`-excluded anyway.
```

`Entry.names` stores the result, so §3.5.1 attempt 2 is a set intersection
against data the REPL already owns.

### 3.7 Ordinals, imports, rollback

**Ordinals** are monotonically increasing, never reused `u32`s assigned at accept
time. Removal (replacement displacement, `:undo`, `:drop`) leaves a **gap**;
`:list` shows gaps rather than renumbering, so an ordinal quoted in a diagnostic
stays meaningful for the life of the session.

#### 3.7.1 Mixed import + item entries

A DECL entry containing both imports and items is **split**: one Import entry per
`File.imports` element and one Decl entry per `File.items` element, each with its
own ordinal in source order. An import already present is **deduplicated** by its
rendered dotted path; the existing entry keeps its ordinal. Rollback covers the
whole paste atomically, and `:undo` removes every entry it created as one step.

#### 3.7.2 Rollback

**Compilation is the transaction boundary.** Any failure at any stage of §3.0
leaves the session byte-identical: entry list, ordinal counter, rendered source,
retained `Checked`, and the §6 output snapshot. Nothing is executed.

**Execution is not.** If the program compiles and then aborts (bounds check,
invalid integer division) or exits non-zero, the entry **stays** — the program is
valid Align; it just aborts. The REPL prints the failure and the exit status or
signal, plus a hint pointing at `:undo` / `:drop`.

## 4. Compile and execute

### 4.1 The `alignc build` path, not a parallel one

v1 uses the same library calls `alignc build` uses, in the same order, with the
same inputs:

```text
align_driver::build_package(&mut source_map, name, src, &CacheContext::from_env(),
                            UnitReuse::Allowed)                     -> PackageBuild
align_driver::codegen_package_parallel(&mut build, &obj_paths, &cache, &target,
                                       profile, rt_lto, jobs, &PgoMode::Off)
                                                                    -> UnitCodegen
align_driver::link_objects(&obj_refs, &staged_exe, &link_libs, profile)
   then same-directory atomic rename
```

This is `build_package_to` / `finish_link` in `crates/align_driver/src/main.rs`
minus the CLI's PGO and `--cache-stats` branches. Consequences:

- **S4 is unconditional**: the object, the link line, the capability-library
  union, and the atomic publish are the production ones.
- The `PackageCodegenError::StaleCacheEntry` retry (one rebuild with
  `UnitReuse::Forbidden`) is inherited rather than reimplemented.
- `ALIGNC_CACHE` / `ALIGNC_JOBS` / `ALIGNC_LINKER` behave exactly as for
  `alignc`; a positive `ALIGNC_JOBS` overrides the REPL default and a malformed
  value fails startup.
- A v1 session is always a **one-unit package** (N6), so the capability-library
  union degenerates to that unit's `link_libs`; V2c promotes the union helper.

**Honest cost.** Each entry's source is new content, so the entry unit always
misses frontend and object lookup and publishes a new cache entry. Measured: 80
successive REPL-shaped builds grew the cache root by **808 KB** (~10 KB/entry); a
500-entry session costs ~5 MB in the shared root, which `alignc cache clear`
already owns. Forcing the cache off behind the user's back would be a hidden
ambient override and is rejected.

**Why multi-unit splitting is rejected for v1.** Splitting declarations into a
stable second unit only helps if that unit's content is unchanged, and the
natural REPL edit — redefining a function (§3.5) — changes it. It would buy
nothing on the entries that matter while forcing an `import`/`pub`/qualified-call
module the user never wrote into `:list` and `:save`, breaking the property the
whole design rests on. Revisit when V2c needs multi-unit staging anyway.

### 4.2 One long-lived process

`align-repl` links `align_driver` as a library and drives each compiler
transaction on a named 32 MB worker thread (sema, lowering, and codegen recurse
per expression-nesting level; `crates/align_driver/tests/common/mod.rs` does the
same). Thread-spawn failure is a compiler-operation failure, and a compiler panic
is resumed on the caller so normal panic cleanup still owns the session stage.

The **in-process memo** (`docs/impl/10-cache-first-optimization.md` §6.6) is
process-lifetime and content-keyed, and the REPL is the long-lived embedder it
was designed for. §3.4 is structured so the memo fires on the common path: the
accepted candidate *is* the built program, so every accepted entry's build
frontend is a whole-program memo hit. `memo::set_budget(256 MiB)` is set at
startup; the 768 MiB default has no eviction.

**Artifact staging.** `ArtifactStage` (pid + nanos + nonce directory,
`create_dir` as the atomic claim, `Drop` removes only what this invocation
created) lives in the `alignc` binary crate and is unreachable from a library
consumer. v1 promotes it to the driver library (L2); `main.rs` and its `size`
module consume the promoted type. Each session owns one stage holding
`session.align`, `unit0.o`, and `session`. Nothing is written to the user's cwd
except by `:save`.

## 5. Diagnostics presentation

`align_driver::format_diagnostics` rendered **against the generated program**,
with the generated program's own line numbers. `:list` prints that same program
with the same line numbers. No span remapping and no synthetic-line hiding — the
only synthetic lines are the `print(…)` and `_ := (…)` wrappers around the user's
own text, both of which the echo discloses.

Warnings are shown, including `unused import` — the program really does have one.

One exception by class: a **keyword-first DECL** (§3.3 step 1) that does not
compile reports the DECL candidate's file-position diagnostics, never a statement
candidate's.

v2 adds entry-relative span remapping.

## 6. Output policy

The program is re-executed in full every entry, so the raw output contains every
earlier entry's output. Showing all of it is unusable; hiding it silently would
hide re-executed side effects.

```text
new_output starts with prev_output (exact byte prefix)
    -> print only the suffix.
otherwise (divergence)
    -> one-line banner, then the FULL new output:
       `align-repl: re-execution differs from the previous run (a replaced
        binding, nondeterminism, or an external side effect) — full output
        follows`
first successful run, or after :undo / :drop / :clear / a truncated snapshot
    -> print the full output.
```

stdout and stderr are two independent streams under the same rule, printed in
that order. Interleaving is not preserved — the child is captured with two pipes.

**Retention bound.** Each snapshot is capped at **8 MiB**. A run exceeding it
prints in full, the snapshot is marked truncated with one notice
(`align-repl: output exceeded 8 MiB; suffix elision is disabled until the next
:clear`), and the prefix rule is disabled until reset.

**Nondeterministic programs** diverge on essentially every entry, so they print
in full and show the banner every entry. That is the correct report, and it is
the one case where the policy gives no ergonomic benefit; `:help` says so.

**External side effects are never elided.** A session containing `fs.write(...)`
writes the file again every entry. Stated in `:help` and in the `:save` header.

`:out` reprints the last run's full captured output when it fit the retention
bound. Immediately after an over-cap run, the bytes have already been printed
but are deliberately not retained; `:out` reports that bounded-retention case
instead of claiming that nothing has run.

## 7. Profile: no deviation from `alignc`

v1 uses `alignc`'s defaults — `Profile::Release`, `rt_lto` on
(`default_rt_lto(Profile::Release) == true`), `BuildTarget::Baseline`. Measured
(§13): the same build is **59.5 ms with rt-lto** and **60.0 ms at
`--profile dev`** — the reduced profile is marginally *slower*, because the link
step dominates. There is no interactivity/fidelity trade-off to make; taking the
default costs nothing measurable and makes S4 exactly true. Any future deviation
needs a measurement in this section.

## 8. v1 command surface

Commands begin with `:` in column 1. Anything else is an entry. Unknown commands
are errors that do not touch the program. **Eleven commands** (`:save!` and
`:time!` are flags, not separate commands):

| # | Command | Behavior | Errors |
|---|---|---|---|
| 1 | `:help` | The living-program model; in-place replacement and regions; `:const` and the fact that a `fn` entry cannot see `main` locals; re-execution and side effects; nondeterministic output; the N11 arena/heap limitation with the working `arena { … }` form; Ctrl-C; the command list. | — |
| 2 | `:quit` (also EOF) | Remove the stage, exit 0. | — |
| 3 | `:list` | The exact generated program with §5 line numbers, plus an ordinal gutter with gaps visible, region-annotated. | — |
| 4 | `:type <expr>` | Compile a throwaway candidate — never spliced, never executed — and print `ty_display`'s rendering. Session unchanged either way. | Check errors rendered per §5. |
| 5 | `:const NAME := expr` | Add or replace a region-2 top-level constant. The only form a `fn` entry can reference, and the canonical way to write a constant table. | The compiler's own diagnostic if the initializer is not compile-time-foldable. |
| 6 | `:save <path>` / `:save! <path>` | Write the generated program plus a 3-line header (generated-by; the fixed `main` signature; the side-effect note), then print the `alignc build <path>` line. Path resolved against the process cwd; no `~` or variable expansion (there is no shell); the parent must exist; `:save` refuses an existing file, `:save!` overwrites. | `SaveError::{Exists, ParentMissing, Io}` |
| 7 | `:undo` | Remove the most recent entry, restore the entry it replaced, or remove a whole split paste. Rebuild, re-run, reset the §6 baseline. | Empty session -> message. |
| 8 | `:time [N]` / `:time! [N]` | Run the already-built binary N times with output discarded; report min/median/max plus the startup spawn floor. Does not recompile. N clamped to `1..=1000`, and the clamp is reported when it fires; if last-run-time × N exceeds 10 s, print the projection and require `:time!`. | `TimeRefusal::{NoBinary, Projected}` |
| 9 | `:out` | Reprint the last successful run's full captured output when retained. | Nothing run yet, or the last run exceeded the retention bound -> distinct message. |
| 10 | `:clear` | Drop every entry; reset to the empty program and the empty output baseline. The ordinal counter keeps its current value (ordinals are never reused). | — |
| 11 | `:drop <ordinal>` | Remove one entry by ordinal. Rebuild and re-run; roll back if the result does not compile. | Unknown or removed ordinal -> message. |

### 8.9 `:time` honesty note

Printed on every invocation:

```text
:time measures the whole session program, not the last entry, and each sample
includes process spawn. Spawn floor on this host: <F> ms (measured at startup
from an empty program). Compilation is not included; the binary is already built.
```

`<F>` is measured once at startup from
`fn main() -> Result<(), Error> { return Ok(()) }`. Reference host: **2.5 ms**
for a real Align executable versus **1.9 ms** for `/usr/bin/true`.

### 8.10 Ctrl-C

With no signal dependency, std Rust cannot install a handler, and a terminal
SIGINT goes to the whole foreground process group. Stated plainly in `:help`:

- **At the prompt:** Ctrl-C terminates `align-repl`. V2d's `rustyline` returns
  `Interrupted` instead, so the session survives — a named reason V2d is worth
  doing, not a v1 promise.
- **While the program runs:** Ctrl-C terminates the program *and* `align-repl`.
  The REPL does not put the child in its own process group; that needs `libc`.
- **Leak note:** signal death means `Drop` does not run, so the staging directory
  survives. It is inert and uniquely named, and the system temp cleaner removes
  it. `align-repl` deliberately does **not** garbage-collect other directories —
  deleting paths it did not create is exactly what `ArtifactStage`'s rule exists
  to prevent.

## 9. Crate structure and dependencies

```text
crates/align_repl/
  Cargo.toml           [[bin]] name = "align-repl"; [lib] name = "align_repl"
  src/lib.rs           Session: entries, ordinals, regions, replacement, rendering
  src/entry.rs         classification (§3.3), continuation (§9.1), name delta (§3.6)
  src/echo.rs          the §3.4 three-candidate procedure
  src/build.rs         the §4.1 package path, ArtifactStage, subprocess, output diffing
  src/cmd.rs           `:` command dispatch
  src/main.rs          stdin loop, prompt, wiring
  tests/session.rs     leaf owner test (in-process)
  tests/e2e.rs         leaf owner test (drives the binary over a pipe)
  tests/fixtures/      transcripts + .align goldens, READ AT RUNTIME (§10)
```

Dependencies: `align_driver`, `align_runtime` (so a focused `cargo test -p
align_repl` materializes the static archive its generated programs link),
`align_sema`, `align_lexer`, `align_parser`, `align_ast`, `align_span`,
`align_diag` — **all path deps already in the workspace. Zero new external
crates.**

The workspace's complete external set is `inkwell` + `llvm-sys`
(`align_codegen_llvm`), `mimalloc` (`align_driver`), `memchr` + `regex` +
optional `libc` (`align_runtime`), and `cc` as a build dependency. Every one is
load-bearing. The governing property is that **no crate in this workspace has
ever taken a convenience or UX dependency** — which is precisely what a line
editor would be.

v1 reads `std::io::stdin().lock()` line by line in canonical mode: backspace
works, arrow-key history does not. Mitigations: `:list`, `:undo`, `:drop`,
`:save`, and piping a file of entries into stdin. **v2 adds `rustyline` behind a
non-default cargo feature `line-edit`**; an optional dependency enters
`Cargo.lock` but is not compiled by `--workspace --locked` builds that do not
enable it, so CI cost stays zero.

### 9.1 Multi-line continuation

Computed from a **throwaway `align_lexer::tokenize`** over the accumulated entry
with a discarded `Diagnostics`:

```text
continue reading while EITHER
  (a) bracket depth > 0, counting only LBrace/RBrace, LParen/RParen,
      LBracket/RBracket (`<`/`>` are NOT counted — ambiguous with comparison and
      generics), or
  (b) the throwaway diagnostics contain `unterminated string literal` or
      `unterminated character literal`
```

**Explicit limitation:** the lexer's leading-`.`/binary-operator continuation
(`next_significant_continues_line`) is private, so a method chain broken across
lines works **only inside brackets**. At top level, write it on one line or wrap
it in parentheses. `:help` says this in one line. V2e lifts it with a `pub`
predicate.

Prompt `...   `. A blank line while continuing abandons the partial entry with a
notice. A blank line at the *primary* prompt is §3.3 step 0's no-op, not an
abandon.

## 10. Non-impact proof (S3)

| Surface | Impact | Reason |
|---|---|---|
| `alignc` binary | **None functionally.** | `align_repl` depends on `align_driver`; nothing depends on `align_repl`. No subcommand added. `main.rs` changes only to consume the promoted `ArtifactStage` (L2). |
| `align_driver` / `align_sema` runtime behavior | **None.** | Two changes: L1 (`ty_display` extraction; the inherent method delegates, all 58 call sites unchanged) and L2 (`ArtifactStage` promotion; same code, `pub`, relocated). No new allocation on any existing path. |
| Compiled output of any existing program | **None.** | No IR, MIR, codegen, profile, or link-line change. |
| Bounded PR gate (`scripts/test-pr.sh`) | **Build only, ~1–2 s. No test time.** | The `cargo test --no-run` selection is an explicit `-p` list whose header says a new library must not silently enter it; `align_repl` is not added. `scripts/cargo.sh build --workspace --locked` compiles the new lib+bin. `scripts/test-pr-workflow.sh` is unaffected: no gate target added, no document embedded. |
| `scripts/pr-tier.sh` | **Correct without change.** | `tests/session.rs` and `tests/e2e.rs` are leaf owners not in `PR_TIER_GATE_TESTS` -> tooling tier. `tests/fixtures/*` is explicitly code tier, which the introducing PR already is. |
| Nightly (`scripts/run-suite-binaries.sh`) | **Automatic; must be green.** | Nightly builds every workspace test binary and diffs against `scripts/known-failures.txt` both ways. The two binaries join automatically; **no manifest entry is added.** Cost ~7 s against a 30-minute cap. |
| Clippy | **`--lib --bins` only.** | `scripts/pre-pr.sh` runs `clippy --workspace --lib --bins`, so the crate's **test** targets are not linted. Recorded so green Clippy is not mistaken for lint coverage of the tests. |
| `scripts/lint-ratchet.sh` | **Three counts must not move.** | Zero panic sources, zero lossy casts, and the `fixture-bake` kind is scanned across every crate's `tests/` — so fixtures are read at runtime (`std::fs::read_to_string` under `CARGO_MANIFEST_DIR`), never `include_str!`. |
| `db-verify-local.sh`, `preflight.yml`, `release.yml` | **None.** | No `apps/db` or `pkg_db_*` path touched. `release.yml` picks up one more small crate; `align-repl` is **not** in the released artifact set in v1. |

## 11. Public-contract ledger

### Changes to existing crates — the complete set is two

| id | Surface | Exact signature | Change | Ownership / alloc | Errors | Owner test |
|---|---|---|---|---|---|---|
| L1 | `align_sema::ty_display` | `pub fn ty_display(ty: Ty, structs: &[hir::StructDef], enums: &[hir::EnumDef], tagged_types: &[hir::TaggedType], tuples: &[hir::TupleDef], type_params: &[String]) -> String` | New public free fn. Body is `Checker::ty_display` verbatim with **five** `self.` field reads parameterised. `ty_name` stays private. The inherent method becomes a one-line delegate, so all **58** call sites are untouched. | Owned `String`; borrows five slices. No new allocation on any existing path. | none | free fn == method for struct, sum, nested `Option<Result<…>>`, recursive `Tagged`, `array<i64>[3]`, `slice<u8>`, tuple, `Param` |
| | `type_params` semantics | — | The REPL passes `&[]`, so `Ty::Param(i)` renders as the existing `<unknown type parameter>`. Correct for every REPL use: `:type` and §3.4's echo read types out of a *checked* `main`, never a generic template — monomorphization substitutes every `Param` before HIR is published. `&[]` states that no generic context is in scope, and the placeholder makes a violation visible instead of silent. | | | `session.rs::type_params_absent_renders_placeholder` |
| L2 | `align_driver::ArtifactStage` | `pub struct ArtifactStage`; `pub fn in_dir(parent: &Path, label: &str) -> io::Result<Self>`; `pub fn temp(label: &str) -> io::Result<Self>`; `pub fn path(&self) -> &Path`; `impl Drop` | Promotion from the `alignc` binary crate into the driver library. Code unchanged; `main.rs` and its `size` module consume it. A library consumer cannot reach it today, and the alternative is a third copy of the same directory-race protocol. | Owns one directory; `Drop` removes only what this invocation created. | `io::Error`; `AlreadyExists` after 1024 nonce attempts | existing `alignc` behavior + a unit test that two concurrent `temp` calls get distinct dirs and each `Drop` removes only its own |
| — | `align_sema::print_kind` | `pub fn print_kind(ty: Ty) -> Option<PrintKind>` | **Already public.** Used to render the `:type` classification line; printability itself is decided by `check(P)`. No second definition is written. | Copy | none | existing |
| — | `align_driver::{build_package, codegen_package_parallel, link_objects, format_diagnostics, backend_available, default_rt_lto, CacheContext, UnitReuse, PgoMode, Profile, BuildTarget}` | — | **Already public**; §4.1 uses them unchanged. | — | — | existing |

### `align_repl` library surface

| id | Surface | Exact signature | Inputs / defaults | Ownership / alloc | Errors | Owner test |
|---|---|---|---|---|---|---|
| R1 | `Session::new` | `fn new(cfg: Config) -> Result<Session, StartupError>` | `Config { profile: Profile::Release, rt_lto: true, target: BuildTarget::Baseline, jobs: 1 on aarch64 and available_parallelism() otherwise (a positive ALIGNC_JOBS overrides either), memo_budget_bytes: 256<<20, time_default_n: 10, output_cap_bytes: 8<<20 }` | Owns the `ArtifactStage`, the entry `Vec`, and the retained HIR. | `StartupError::{BackendUnavailable, InvalidJobs(String), RuntimeArchiveStale(String), Stage(io::Error), FloorBuild(String)}` — `RuntimeArchiveStale` is a real failure (`libalign_runtime.a is stale: … run cargo build`) a library consumer hits exactly as `alignc` does; reported once at startup with the driver's own message. | `session.rs::startup_creates_and_removes_stage` |
| R2 | `Session::feed` | `fn feed(&mut self, line: &str) -> Feed` | one raw input line | borrows | — | `session.rs::continuation_*` |
| R3 | `Feed` | `enum Feed { NeedMore, Ready(Outcome) }` | — | owned | — | — |
| R4 | `Session::submit` | `fn submit(&mut self, entry: &str) -> Outcome` | a complete entry | transactional (§3.7.2) | see `Outcome` | the whole `session.rs` matrix |
| R5 | `Outcome` | `enum Outcome { NoOp, Applied { ordinals: Vec<u32>, replaced: Vec<u32>, echo: Echo, out: RunOutput }, CompileFailed { rendered: String, replacing: Vec<u32> }, RegionConflict { name: String, ordinal: u32 }, RanAndFailed { status: ExitStatus, out: RunOutput }, Command(CmdResult) }` | — | owned | — | — |
| R6 | `Echo` | `enum Echo { None, Printed, TypeOnly { rendered: String }, ResultBound { rendered: String } }`; `fn render(&self) -> Option<String>` | `ResultBound` is §3.4 case 3 and drives the `note:` line | owned | — | `session.rs::echo_matrix` |
| R7 | `RunOutput` | `struct RunOutput { stdout_shown: Vec<u8>, stderr_shown: Vec<u8>, diverged: bool, truncated: bool }` | `*_shown` is the suffix or the full raw bytes per §6; no UTF-8 conversion | owned | — | `build.rs::output_matrix` plus `session.rs::child_output_remains_byte_exact_for_invalid_utf8` and replacement/removal owners |
| R8 | `Session::render` | `fn render(&self) -> String` | — | owned; the exact bytes compiled and `:save`d | — | `session.rs::render_is_what_alignc_builds` |
| R9 | `Session::entries` | `fn entries(&self) -> &[Entry]` | — | borrow | — | — |
| R10 | `Entry` | `struct Entry { ordinal: u32, region: Region, kind: EntryKind, text: String, emitted: String, names: Vec<String>, paste_group: Option<u32> }`; `enum Region { Import, Const, Decl, Main }`; `enum EntryKind { Import, Decl, Const, Statement, Printed, ResultBound }` | `text` is what the user typed; `emitted` is what §3.4 splices — identical except for a `print(…)` / `_ := (…)` wrapper | owned | — | `entry.rs` classification matrix |
| R11 | `Session::type_of` | `fn type_of(&self, expr: &str) -> Result<String, String>` | throwaway candidate; session untouched; never executed | owned | rendered diagnostics on `Err` | `session.rs::type_of_does_not_mutate_session` |
| R12 | `Session::time` | `fn time(&mut self, n: u32, force: bool) -> Result<Timing, TimeRefusal>` | `n` clamped to `1..=1000`; `force` bypasses the 10 s projection guard | owned | `TimeRefusal::{NoBinary, Projected { secs: f64 }}` | `session.rs::time_clamps_and_refuses` |
| R13 | `Timing` | `struct Timing { n: u32, clamped_from: Option<u32>, min_ms: f64, median_ms: f64, max_ms: f64, floor_ms: f64 }` | — | Copy | — | — |
| R14 | `Session::save` | `fn save(&self, path: &Path, force: bool) -> Result<(), SaveError>` | §8 row 6 path rules | — | `SaveError::{Exists, ParentMissing, Io}` | `session.rs::save_path_rules`, `e2e.rs::saved_file_builds_with_alignc` |
| R15 | `Session::undo` / `drop_entry` | `fn undo(&mut self) -> Outcome`; `fn drop_entry(&mut self, ordinal: u32) -> Outcome` | `undo` reverses a replacement or a whole paste group | transactional | — | `session.rs::undo_restores_a_replaced_entry`, `::drop_by_ordinal` |
| R16 | command-facing `Session` helpers | `fn continuing(&self) -> bool`; `fn add_const(&mut self, text: &str) -> Outcome`; `fn listing(&self) -> String`; `fn clear(&mut self) -> Outcome`; `fn last_output(&self) -> Option<(Vec<u8>, Vec<u8>)>`; `fn last_output_was_truncated(&self) -> bool` | Direct backing for the eleven binary commands; `last_output` preserves the two raw streams separately; no second state machine | borrows or returns owned reports | compiler diagnostics through `Outcome`; bounded-output status is distinct from never-run | command and session matrices |
| R18 | `Session::object_path` | `fn object_path(&self) -> PathBuf` | none | Returns an owned path into the session's staging directory; valid for the life of the `Session` and removed with it | none — the file always exists, because `Session::new` fails with `StartupError::FloorBuild` unless the startup probe build succeeded | `e2e.rs::saved_file_object_matches_the_real_alignc_binary` |
| | one-unit assumption | — | A v1 session is always a one-unit package (`pkg.*` and user modules are N6), so the unit index is fixed at zero. V2c's multi-unit staging must widen this to a set before it ships, or the accessor answers for a fraction of the build | | | the same owner asserts the stage holds exactly one `unit*.o` |
| R17 | `cmd` module | `enum Command`; `enum CmdResult { Message(String) }`; `fn parse(&str) -> Option<Command>`; `const HELP: &str` | Binary parser and help text for §8; public because the binary is a separate crate in the package | owned command arguments | malformed and unknown forms become `Command::Unknown` | `cmd.rs::command_table_covers_the_v1_surface_and_errors` |

### `align-repl` binary surface

| id | Contract |
|---|---|
| B1 | `align-repl`, no positional arguments, `--help` / `--version` only. **No environment variable of its own.** It honors the same `ALIGNC_CACHE` / `ALIGNC_JOBS` / `ALIGNC_LINKER` contracts as `alignc`; `ALIGNC_JOBS` overrides the architecture default before the shared driver build calls receive the resolved count. |
| B2 | `align> ` primary prompt, `...   ` continuation, both on **stdout**; diagnostics on **stderr**; program output forwarded to the matching stream. Non-tty stdin suppresses the prompt. |
| B3 | Exit 0 on `:quit`/EOF; 1 on a `StartupError`. Never propagates the program's exit code (N8). Ctrl-C: §8.10. |
| B4 | Exactly one `ArtifactStage`, removed on normal exit and on panic, **not** on signal death. Writes to the user's cwd only via `:save`. |

### Cache and artifact identity

No new cache namespace and no new key material. Object identity is whatever
`codegen_package_parallel` computes under (`Baseline`, `Release`, `exports=[]`,
`rt_lto=true`, `PgoMode::Off`) — the same key `alignc build` uses. The in-process
memo is process-local and content-keyed; the persistent unit cache is consulted
and always misses on the entry unit (~10 KB/entry of growth).

### Prerequisite milestones

None. Every entry point used already ships. The REPL is the first consumer of
`docs/impl/21-build-perf-plan.md` item 5's "keep the in-process memo alive across
builds": it realizes that lever without a daemon, because it *is* a long-lived
process.

### Documents that must agree

This document; one row in `docs/impl/21-build-perf-plan.md` item 5; one line in
`docs/impl/16-test-policy.md`. No `draft.md` / `docs/language-spec.md` /
`docs/open-questions.md` change — the REPL adds no language surface, syntax, or
semantics. `docs/guide/` gains a REPL page in v2 with its `ja/` mirror.

## 12. Implementation closure matrix

| Axis | Cells | Closed by |
|---|---|---|
| **Synthetic-code consumption** — every path on which the REPL emits text the user did not type | `print(E)` where E is: a Copy local; a **Move local** (owned `string`) — next entry must still use it; a `.field` of a Move struct; a call returning a temporary. (`box` cannot be a cross-entry local because N11's arena scope cannot span entries.) `_ := (E)` where E is: a fallible **call** (nothing named, no consumption); a **Result-typed place expression** — next entry naming it **must** fail with `use of moved value`, and the echo **must** have printed the `note:` line. Verbatim `E` where E is: a bare Move aggregate — next entry must still use it; a nested-scope binding. Plus the negative: **no other emission path exists** — asserted structurally by a test that walks every `Entry.emitted` in a scripted session and requires it to equal `text`, `print({text})`, or `_ := ({text})`. | `session.rs::synthetic_consumption_matrix`, `::emitted_forms_are_exhaustive` |
| Classification (§3.3) | **step 0:** blank line, whitespace-only, comment-only, comment-then-blank — each a NO-OP that does **not** rebuild or re-run. keyword-first: `import` / `fn` / `extern` / `resource`. step 2 statement-only: `print(1)`, `x := 1`, `mut x := 1`, `x = 2`, `xs[0] = 1`, `p.f = 1`, `return`, `break`, `if`/`match`/`loop`/`arena`. step 3 decl-only: `P { a: slice<u8> }`, `P { a: array<i64>[3] }`. step 4 routing: `P { a: i64 }` with `P` undeclared -> DECL; `P { a: x }` with `x` a live local -> MAIN; both-non-dup-error -> MAIN + MAIN's diagnostics. **step 4 duplicate-tolerance — the traced cells:** (a) `x := 2` rebinding an existing local -> **MAIN**, then §3.5.1 replaces in place and the value changes on re-run; (b) `T { a: i64 }` redeclaring an existing struct -> **DECL**, then §3.5.1 replaces the type in place; (b′) the same with a sum, `S { A, B }`; (c) `Point{x: a, y: b}` with `Point` declared and `a`/`b` undefined -> **DECL**, §3.5.1 attempt 2 fails with `unknown type: 'a'` and the entry **rolls back with the replaced-ordinal note**; (c′) the same text after `a` and `b` exist as locals -> **MAIN, clean**. **overlap determinism:** MAIN-clean ∧ DECL-dup-only (`P { a: x }` with `x` both a type and a local) -> MAIN by first-wins; MAIN-dup-only ∧ DECL-clean asserted **unreachable**; both-clean asserted **impossible**. **threshold pinning:** step 4 rejects on one non-dup error while §3.5.1 attempt 1 accepts on one dup error — both thresholds asserted so collapsing them to one predicate fails. unparseable text. | `entry.rs` parameterized table, `session.rs::step4_duplicate_routing`, `::step4_overlap_determinism`, `::duplicate_thresholds_differ` |
| Echo (§3.4) | P-clean printable (int, float, bool, char, `str`, owned `string`); P-fails/S-clean aggregate (struct, sum, tuple, array, slice, `Option`, `box`); S-clean statement-position non-expression (no echo); S-fails-with-`unhandled Result` -> D accepted, with the `note:` line; S-fails otherwise -> S's diagnostics, rollback; `Ty::Error` after a failed check | `session.rs::echo_matrix` |
| Mixed paste (§3.7.1) | import + fn; two fns; import already present (dedup, ordinal preserved); second item fails (whole paste rolls back); `:undo` after a paste | `session.rs::paste_matrix` |
| Name delta (§3.6) | `x := 1`; `mut x := 1`; `(a, b) := t`; `(a, _) := t` contributes exactly `a` (the `_` element is `None` in `LetTuple::locals` and creates no `Local`); two `_ := (…)` entries in one session coexist and contribute no names; `fn f`; struct; sum; resource; extern symbol; `:const`; multi-item paste; `$lambda` lifted fn excluded; `arena { p := … }` entry's `p` excluded by the entry-top-level filter, and a later `p := 9` must NOT displace it; `link_libs` never treated as a namespace | `session.rs::name_delta_matrix`, asserting the delta directly |
| Replacement (§3.5) | binding replaces binding; `mut` binding replaces binding; tuple binding replaces two; fn replaces fn; type replaces type; extern replaces extern through the AST-owned region-3 path; `:const` replaces `:const`; no collision -> append; `fn f` does NOT displace local `f`; type `P` does NOT displace local `P`; region-2 constant vs region-4 local -> `RegionConflict`, nothing removed; replacement whose RHS references a later ordinal (fails + rolls back); replacement of an entry a later entry reads (re-runs with the new value); §3.5.1 attempt-2 trigger with **one** duplicate error among several; attempt 2 with empty `N`; all five emitted duplicate message classes recognised | `session.rs::replacement_matrix`, `::duplicate_message_classes_are_complete` |
| Ordinals (§3.7) | monotonic; gap after replacement / `:undo` / `:drop`; `:list` shows gaps and regions; `:clear` does not reset the counter | `session.rs::ordinal_matrix` |
| Rollback (§3.7.2) | failure at §3.3 step 4, §3.4 cases 1/2/3/4, §3.5.1 attempts 1 and 2; lowering, codegen, and link failure; each leaves entries, ordinals, `render()`, retained HIR, and the output baseline byte-identical | `session.rs::rollback_is_total` |
| Execution failure | exit 0; non-zero exit; abort (bounds check); abort (invalid integer division); killed by signal | `session.rs::runtime_failure_keeps_the_entry` |
| Output (§6) | first run; pure-suffix growth; divergence via replaced binding; divergence via nondeterminism; empty; stderr-only; both streams; over-cap truncation + notice; after `:undo` / `:drop` / `:clear` | `build.rs::output_matrix`, session replacement/removal owners |
| Build path (§4.1) | the rendered source builds through `build_package` + `codegen_package_parallel` + `link_objects`; **the emitted object is byte-identical** to the object the library path produces for the same source, **and to the object the shipped `alignc` binary emits for the file `:save` actually wrote**; `ALIGNC_CACHE=off` works; a poisoned entry takes the `StaleCacheEntry` retry; an accepted entry's build frontend re-uses the candidate's | `build.rs::repl_object_matches_the_alignc_build_path` (library path, in-process), `e2e.rs::saved_file_object_matches_the_real_alignc_binary` (shipped binary, from the saved bytes), `e2e.rs::cache_off_still_builds_and_runs`, `::saved_file_builds_with_the_real_alignc_binary`, `session.rs::accepted_build_reuses_the_candidate_frontend` |
| main signature | zero fallible ops; `?` present; `?` introduced then undone; `Ok(())` tail always present | `session.rs::main_signature_is_constant` |
| Commands | each of the eleven plus each error branch plus unknown `:foo`; `:time` clamp and projection refusal; `:save` path rules; `:help` mentions N11 | `cmd.rs` table test |
| Stage lifetime | normal exit; `:quit`; EOF; panic mid-build; two concurrent sessions get distinct dirs; L2's "removes only its own" | `session.rs::stage_matrix`, `align_driver` L2 unit test |
| End-to-end | pipe a 25-entry script through the binary, diff against a runtime-loaded golden transcript; `:save` then real `alignc build` | `e2e.rs` |
| Non-impact | L1 free fn == method (8 shapes); L2 preserves `alignc` behavior | `align_sema` / `align_driver` unit tests |

**Build-parity granularity.** The parity owner compares the **emitted object**,
not the executable. Two links of identical objects differ on macOS: a Mach-O
image carries an `LC_UUID` and page hashes derived from link-time inputs. The
object is the artifact the codegen contract actually promises.

What the cross-process owner does and does not observe: `alignc build` stages its objects in an `ArtifactStage` that `Drop` removes, so `emit-obj` is the only externally observable object the shipped binary produces, and it is a different call site from the one §4.1 mirrors (`build_per_unit` + `emit_object_cached` rather than `build_package` + `codegen_package_parallel`). The two agree because both bottom out in the same `emit_object_file(mir, obj, target, profile, &[], rt_lto)`. The owner therefore pins the object; the link line, the capability-library union, and the atomic publish are covered by `saved_file_builds_with_the_real_alignc_binary` running the linked program, not by byte comparison.

No cell requires a benchmark: this plan makes no performance *promise*. §13 is
sizing evidence; `:time` measures the user's program.

## 13. Measurements

`alignc 0.5.0`, Apple Silicon, macOS 25.5, median of 7–9 runs.

**Stage breakdown, static sources, warm cache** (`big` = 60 `fn`s + 600
statements):

```text
            check     emit-obj    build
g_tiny       12.2       14.8       59.6
g_small      13.1       18.0       60.0
g_med        16.7       27.8       60.6
g_big        26.1       53.8       61.1
```

`build` is flat because the persistent cache serves an unchanged source;
`emit-obj` is the uncached codegen cost. Link floor from `g_tiny`:
**59.6 − 14.8 ≈ 45 ms**, size-independent.

**REPL-shaped growth** — each build is fresh content, so the entry unit misses
frontend *and* object every time:

```text
                                  check first/last     build first/last     build median
0 decls, growing to 80 stmts       28.7 / 13.4 ms       100.0 / 66.8 ms       64.6 ms
60 fns, growing to 80 stmts        14.5 / 16.3 ms        68.5 / 73.3 ms       72.6 ms
```

**Per-entry latency.** Measured CLI `check` + `build`, minus ~1.9 ms per avoided
`alignc` process spawn, plus the exe run:

```text
                                          small session      60-decl session
accepted on candidate P (printable)          ~67 ms              ~74 ms
accepted on candidate S (statement,
  aggregate, Unit, bare place expression)    ~67 ms              ~74 ms   [+13-16 ms
                                                                          for the failed
                                                                          P check when P
                                                                          was formed]
accepted on candidate D (bare fallible)      ~80 ms              ~90 ms
rejected (user error)                        ~27 ms              ~32 ms   (2 checks,
                                                                          no build)
```

**Profile sweep** (`g_med`): rt-lto on **59.5 ms**, `--profile dev` **60.0 ms**.

**Fixed costs:** trivial Align exe **2.5 ms**, `/usr/bin/true` **1.9 ms**,
80-print exe **2.5 ms**. Cache growth over 80 REPL-shaped builds: **808 KB**
(~10 KB/entry).

**Platform note.** macOS always uses the Apple linker by policy (`select_linker`:
Mach-O is never touched; `ALIGNC_LINKER=lld` there is a hard error). On ELF hosts
`lld` cuts the link stage ~5x, so Linux latency should be materially better; the
link floor dominates either way.

**Owner-test cost for nightly:** ~95 entries across the two binaries at ~70 ms ≈
**7 s**, against a 30-minute cap.

**Language facts this plan depends on**, all verified against the compiler:
top-level `fn`/type/constant forward references check; imports must precede all
items; unused bindings emit nothing; unused imports warn; `unhandled Result` as a
statement is a hard error; `fn main() -> Result<(), Error> { print(1); return Ok(()) }`
checks, builds, runs, exits 0; `mut` at top level is rejected; nested-block
shadowing is rejected; `print` of a struct errors; `_ := (E)` is legal for both a
`Result` and a plain expression; a bare place expression `p` of a `box` local
followed by `p.get()` checks clean (no move); `_1 := p` followed by `p.get()`
gives `use of moved value 'p'`; `print(p.get())` twice checks clean;
`Point{x: a, y: b}` at file scope parses and fails in sema with
`duplicate type declaration`; `P { a: x }` with `x` a live local checks as a
statement; a `fn f` and a local `f` coexist; a type `x` and a local `x` coexist; a
top-level constant `K` and a `main` local `K` **do** collide; a binding inside an
`arena { … }` block does not leak its name to a later statement; lifted lambdas
are named `{fn}$lambda{n}`; the lexer emits `unterminated string literal` /
`unterminated character literal`.

## 14. Milestones and sizing

**v1 — one PR.**

```text
crates/align_repl/src/*         ~820 lines
crates/align_repl/tests/*       ~820 lines
crates/align_sema  (L1)         ~20 lines changed + ~50 line unit test
crates/align_driver (L2)        ~40 lines moved + ~25 line unit test
docs/impl/22-repl-plan.md       this document
docs/impl/21-build-perf-plan.md + docs/impl/16-test-policy.md   two cross-references
-----------------------------------------------------------------------------
                                ~1,780 hand-written lines
```

Above the ~1,000-line threshold, so the capability-boundary proof is required:
**the capability does not decompose.** A session model without execution ships a
struct nobody can run, and §3.5.1 cannot even be *tested* without a real compiler
answering "is this a duplicate-name error?". Execution without the session model
is `alignc run` with extra steps. Splitting would duplicate the §12 matrix, the
review, and the gate across two PRs while leaving a dormant producer. The two
separable pieces are L1 (~20 lines) and L2 (~40 moved lines); both are cheaper
here than gated separately, and L2 is meaningless until a library consumer
exists.

**v2 — separately mergeable, in priority order:**

| slice | content | est. |
|---|---|---|
| V2a | `:profile` / `:target` / `:rt-lto` — the benchmarking surface (S5) | ~150 lines |
| V2b | entry-relative diagnostic spans (§5) | ~200 lines |
| V2c | `--project DIR`: multi-unit staging so `pkg.*` and user modules resolve (N6) | ~300 lines |
| V2d | `line-edit` feature: `rustyline`, history, prompt-SIGINT survival (§8.10) | ~120 lines |
| V2e | `:edit <ordinal>`; a `pub` lexer continuation predicate to lift §9.1's limit | ~150 lines |
| V2f | `docs/guide/` REPL page + `ja/` mirror | prose |
| V2g | Session-lifetime arena entry kind, to lift N11 — design-first; it is hidden allocation unless the arena is visible in `:list` / `:save` | design |

## 15. Settled decisions

| # | Decision |
|---|---|
| 1 | §6 output policy is suffix-with-divergence-banner, bounded by the 8 MiB cap with an explicit disabled-elision notice. It elides only bytes already read verbatim and never elides a replay that changed. |
| 2 | `x := 1` is a `main` local; `:const` is the compile-time form. This falls out of §3.3's ordinary path rather than needing a carve-out. |
| 3 | No line editing in v1; `rustyline` behind a non-default feature in V2d. This is partly a correctness question, not only ergonomics: without `rustyline`, Ctrl-C at the prompt kills the session (§8.10). |
| 4 | `main` is fixed at `fn main() -> Result<(), Error>`; no exit codes (N8). |
| 5 | A runtime abort keeps the entry (§3.7.2); `:undo` and `:drop` are the v1 escape hatches. |
| 6 | `align-repl` is not in `release.yml`'s artifact set in v1; add it after V2a/V2b. |
| 7 | §4.1 publishes ~10 KB per entry into the shared `ALIGNC_CACHE` root. Accepted: forcing the cache off behind the user's back is a hidden ambient override, and `alignc cache clear` already owns cleanup. |
| 8 | §3.4 case D (`_ := (E)`) moves a Result-typed place expression. Accepted: the alternative is silently discarding a fallible expression the user typed. The move is disclosed in the echo's `note:` line, pinned by §12's synthetic-consumption axis, and reversible with `:undo`. |
| 9 | Build parity is proven by **object** comparison, not executable comparison (§12). |
| 10 | N11 (heap/arena values cannot span entries) is accepted for v1; `:help` teaches the whole-block form. V2g is the only honest fix and needs its own design. |
