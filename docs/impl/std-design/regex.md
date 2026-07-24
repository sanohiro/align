# std.regex design

Status: first slice SHIPPED 2026-07-23 — built, tested, and IR-verified. `regex` 1.13.1 in the
runtime archive (`Cargo.lock` updated); workspace suite green (2686 passed / 0 failed), 8 runtime
FFI unit tests (`regex_tests`) + 13 driver E2E tests (`std_regex.rs`), `clippy -D warnings` clean.

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

## Deferred surface

Captures, named groups, find iteration, replacement, and split are deferred until an application
consumer fixes their allocation and ownership shapes. A future capture result should prefer Copy
byte spans (possibly an owned array of `Option<regex_match>`) over borrowed substring storage.
Language literal syntax such as `rx"..."`, compile-time validation, implicit caching, and a
backtracking compatibility engine are not part of this design.

## Validation handoff

This machine did not build or test. The capable-machine gate must update `Cargo.lock`, format, build,
run sema diagnostics and end-to-end cases (Unicode boundaries, empty matches, invalid syntax,
limits, Move/drop/error paths), run runtime unit tests and the workspace suite/clippy, inspect
generated IR for ABI correctness, and complete the Codex review before marking the slice shipped.
