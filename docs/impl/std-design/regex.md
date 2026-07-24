# std.regex design

> 🌐 **English** · [Japanese](./ja/regex.md)

Status: **COMPLETE 2026-07-24** — the first slice (2026-07-23) plus the whole second surface
(find_all / split / replace / replace_all / captures / group_count / group_index / caps.group) are
shipped, built, tested, and IR-verified. `regex` 1.13.1 in the runtime archive; `regex_tests` +
`std_regex.rs` cover every operation (spans, Unicode offsets, empty-match termination, split empty
fields, `$`-expansion + named groups, non-participating groups, out-of-range abort, Move/Drop
soundness across `block`/`if`/`match`/loop). `clippy -D warnings` clean. Nothing regex-shaped remains
deferred except the adjacent extras noted below (captures-iterator, closure-callback replace,
`rx"..."` literals) — none has a consumer yet.

## Placement and goals

Regular expressions belong at the application-library boundary. They do not require new Align
syntax, typing rules, or compiler constant evaluation. The first slice therefore adds one explicit
import and one explicitly compiled handle:

```align
import std.regex

re := regex.compile("^[a-z]+$")?
if re.is_match(input) { ... }
```

The design goals are predictable worst-case matching, visible allocation/ownership, compile-once
reuse, no hidden cache, and match spans that do not allocate or borrow a new view.

## Surface

```text
regex.compile(pattern: str) -> Result<regex, Error>
re.is_match(text: str) -> bool
re.find(text: str) -> Option<regex_match>
re.find_at(text: str, start: i64) -> Option<regex_match>

regex_match { start: i64, end: i64 }
```

`regex` is a nameable opaque Move type. Compilation owns the engine program; Drop calls
`align_rt_regex_free`. All methods borrow the handle and their text. Because unnamed owning
receiver cleanup remains restricted at the surface, a caller binds the compiled handle before a
method call.

`regex_match` is an always-registered builtin Copy struct of two `i64` fields. The span is half-open
and measured in UTF-8 bytes. Both offsets are character boundaries, so `text[m.start..m.end]` is a
valid `str` slice. Returning offsets instead of a `str` keeps the result Copy/Static and avoids tying
its region to both the regex and input.

`find_at` starts searching at the supplied UTF-8 byte offset. Negative, past-end, or interior-of-code-
point offsets abort as programmer errors, matching Align's checked range-slice model. End-of-input is
valid and may find an empty match. A missing match is `None`, not an error.

**Anchoring is measured from the true start of the input, not from `start`.** `find_at(text, k)` is
*not* the same as `find(text[k..])` for an anchored pattern: `^`, `\A`, and the word boundary `\b`
resolve against the whole `text` (position 0, and the byte at `k-1` for `\b`), not against the
offset. So `regex.compile("^a")?.find_at("aXa", 1)` is `None` (the only `^` position is 0, which is
before `k = 1`), while `\bword` sees the byte before `k` as boundary context. Slice the input first
(`re.find(text[k..])`) when you want the offset itself to act as a fresh line/input start.

## Engine and resource contract

The runtime uses Rust `regex` 1.13.1 with Unicode enabled by default. This engine excludes
look-around and backreferences and guarantees automata-style worst-case search complexity rather
than backtracking blowups. Align exposes that restricted pattern language as the v1 contract.

Compilation has two independent limits:

- pattern UTF-8 source: at most 64 KiB;
- compiled engine size limit: 10 MiB.

Malformed syntax or either limit rejection returns `Error.Invalid`. There is no diagnostic string in
the v1 error value. Searching is total for valid UTF-8 `str` and a valid start boundary. The runtime
does not maintain an implicit global/thread-local cache; the owned handle is the cache and makes its
lifetime visible.

All four operations are semantically Pure: allocation and reading owned memory are not external
effects. This permits a captured compiled handle to participate in Pure sequential/parallel work
once the existing closure ownership rules can express that capture safely.

## Compiler/runtime shape

- sema: builtin `std.regex`, `Ty::Regex` + `Scalar::Regex`, builtin `regex_match`, bound receiver
  checking, Move/drop classifications;
- HIR: `RegexCompile`, `RegexIsMatch`, `RegexFind`;
- MIR: status/out-slot compile, i32 flag search, out-struct find; the generic owned-handle Result
  lowering is shared with HTTP handles;
- LLVM: opaque pointer ABI, `{ i64, i64 }` match out slot, destructor dispatch;
- runtime: `align_rt_regex_compile`, `align_rt_regex_is_match`, `align_rt_regex_find`,
  `align_rt_regex_free`.

No capability-specific native system library is linked; the Rust crate is compiled into the normal
runtime archive.

## Second surface — iteration, replacement, split, captures (design locked 2026-07-24)

The deferred features all ship in their ideal form on existing plumbing; **no new
language-capability gate is required**, and only `captures` adds one small opaque Move handle type
(mechanically identical to the `regex` handle itself).

```text
re.find_all(text: str)               -> array<regex_match>     // owned Move; leftmost, non-overlapping
re.split(text: str)                  -> array<regex_match>     // owned Move; the between-match field spans
re.replace(text: str, repl: str)     -> string                // owned Move; first match only
re.replace_all(text: str, repl: str) -> string                // owned Move; every non-overlapping match
re.captures(text: str)               -> Option<captures>      // Move handle; None = no match at all
re.group_count()                     -> i64                   // total groups, incl. group 0
re.group_index(name: str)            -> Option<i64>           // name -> numbered index, on the pattern

caps.group(i: i64)                   -> Option<regex_match>   // absent group = None; out-of-range aborts
```

Locked design decisions:

- **Everything is a byte span.** `find_all` and `split` both return `array<regex_match>` — one
  representation for the whole API. Spans are Copy `i64` pairs that view nothing (they are pure
  offsets), so the array **freely escapes** and is region-tied to neither the regex nor the input,
  matching the shipped `find` contract. `split` returns the field spans (`text[p.start..p.end]`);
  it does not allocate substrings. An owned `array<str>` was rejected: it would either deep-copy N
  substrings or tie the pieces' region to `text` — both worse than spans.
- **`replace` / `replace_all` return an owned `string`** and expand the Rust `regex` replacement
  contract — `$1`, `${name}`, and `$$` for a literal `$`. This is the one genuinely useful form and
  is fully documented, so it is specified, not hidden. The result always owns a fresh buffer, even
  when there was no match (never a borrow into `text`).
- **`captures` returns an opaque Move `captures` handle**, not an array. `array<Option<regex_match>>`
  is not representable (there is no Option-of-struct array element), and a handle is the ideal form
  regardless: it owns a fixed buffer of optional Copy spans, borrows nothing, and mirrors
  `CliParsed` exactly (opaque Move handle, total-or-abort/`Option` getters, `Drop` via a free fn).
  `caps.group(i)` returns `Option<regex_match>` — group 0 is the whole match, a group that did not
  participate is `None`, and an out-of-range `i` aborts as a programmer error (the `find_at`
  boundary model).
- **Named groups reduce to numbered groups.** The name→index map is a property of the *pattern*, so
  it lives on the compiled `regex`: `re.group_index(name) -> Option<i64>` (unknown name = `None`),
  then the ordinary `caps.group(i)`. This keeps one resolution path (no duplicate `caps.name(...)`
  mechanism), per "one way".
- All second-surface operations stay **Pure** and keep the receiver-bound rule (bind the handle
  first). `caps` is likewise bound before `.group`.

**Still deferred (no consumer yet, not blockers):** a captures-iterator over all matches
(`array<captures>` = a Move-handle array, the `get_many` `DynResponseArray` pattern), a
closure-callback replacement form (needs escaping first-class closures), language literal syntax
(`rx"..."`), compile-time validation, implicit caching, and a backtracking compatibility engine.

### Slice plan

1. **R1 `find_all`** — establishes runtime-materialized `array<regex_match>` (the
   `lower_json_decode_struct_array` template, minus the `Result`: out slot receives `{ptr, len}`,
   `Load`, return). Runtime `align_rt_regex_find_all` collects `find_iter` into a fresh
   `align_rt_alloc` buffer; empty result is `{null, 0}` (`Drop` is null-safe).
2. **R2 `replace` / `replace_all`** — independent; owned `string` via `AlignStr` (the `str_clone` /
   `PathJoin` return-by-value shape). Always materializes an owned buffer (a no-match `Cow::Borrowed`
   is cloned out).
3. **R3 `split`** — same representation and plumbing as R1; the runtime walks matches and emits the
   between-match spans, including empty leading/trailing/interior fields and one empty field for empty
   input.
4. **R4 `captures` + `group_count` + `group_index` + `caps.group`** — adds `Ty::Captures` /
   `Scalar::Captures` (swept through every Move/drop `matches!` list, the codegen ptr-type +
   destructor arms, and every exhaustive HIR/MIR walk), `align_rt_regex_captures*` runtime.

Soundness invariants for every slice (per `/align-self-review`): validate the `{ptr,len}` text view
(`len < 0 || (len > 0 && ptr.is_null())`), `usize::try_from` every offset, `i64::try_from` back
(abort past `i64`, mirroring `find`), zero the out slot before work, hand back exactly the allocation
`align_rt_free` expects, and **advance empty matches by one codepoint** (never one byte) in
`find_all` / `split` / `replace_all` to avoid an infinite loop and to stay on a char boundary. Every
new HIR `ExprKind` / MIR `Rvalue` must be entered into the escape/region/effect/`MoveCheck`/drop
passes it would otherwise silently skip.

## Validation handoff

This machine did not build or test. The capable-machine gate must update `Cargo.lock`, format, build,
run sema diagnostics and end-to-end cases (Unicode boundaries, empty matches, invalid syntax,
limits, Move/drop/error paths), run runtime unit tests and the workspace suite/clippy, inspect
generated IR for ABI correctness, and complete the Codex review before marking the slice shipped.
