# Align v0.3.0 Release Notes

The headline of v0.3.0 is **`std.regex` — a complete, linear-time regular-expression module**, from a compiled handle to captures, replacement and split. 5 merged changes since v0.2.0.

## `std.regex` — complete

Regular expressions live at the application-library boundary: no new syntax, no compile-time validation, no hidden cache. A pattern is compiled once into an owned `regex` Move handle, and every operation borrows it. The engine is the Rust `regex` crate (automata-based, **linear-time worst case** — no backreferences or look-around, no catastrophic backtracking), compiled into the runtime archive; two independent limits (64 KiB pattern source, 10 MiB compiled program) reject a hostile pattern as `Error.Invalid`.

```align
import std.regex

re := regex.compile("(?P<y>[0-9]{4})-(?P<m>[0-9]{2})")?
if re.is_match(input) { ... }
```

The full surface, all shipped and tested this release:

- **Search** — `re.is_match(text) -> bool`, `re.find(text)` / `re.find_at(text, start) -> Option<regex_match>`. `regex_match { start, end }` is a builtin Copy struct of half-open UTF-8 byte offsets, so `text[m.start..m.end]` is a valid slice. `find_at` anchors `^` / `\A` / `\b` against the true start of the input, not the offset.
- **Iteration & split** — `re.find_all(text)` and `re.split(text)` both return an owned `array<regex_match>`. The whole search API speaks one representation: byte spans, which view nothing and so escape freely (region-tied to neither the regex nor the input). `split` keeps empty leading/interior/trailing fields; an empty-match pattern advances by a codepoint and cannot loop.
- **Replacement** — `re.replace(text, repl)` / `re.replace_all(text, repl) -> string`, a fresh owned string that never aliases the input. `repl` expands the Rust contract: `$1` / `${name}` insert a capture group, `$0` the whole match, `$$` a literal `$`.
- **Captures** — `re.captures(text) -> Option<captures>` yields a `captures` Move handle holding the match's group spans; `caps.group(i) -> Option<regex_match>` reads group 0 (the whole match), a numbered group, or `None` for a group that did not participate; an out-of-range index aborts, matching the checked slice-boundary model. Named groups reduce to numbered ones — `re.group_index(name) -> Option<i64>` on the pattern, then `caps.group(i)` — one resolution path, not two. `re.group_count() -> i64` counts groups including group 0.

Every allocation and every owned handle is visible in source: `find_all` / `split` / `replace` results and the `captures` handle are Move values dropped at scope end, swept through the compiler's move / drop / region / escape passes so the ownership is exact — no leak, no double-free — across `block` / `if` / `match` / `?` / loop. Each slice was verified by an independent adversarial soundness review and shipped with runtime unit tests plus 21 end-to-end driver cases (Unicode offsets, empty-match termination, split empty fields, `$`-expansion, named and non-participating groups, out-of-range abort, drop soundness).

Still deferred, each until a consumer fixes its shape: a captures-iterator over all matches, a closure-callback replacement form, and `rx"..."` literal syntax.

## Documentation

The learning material tracked the language: the tutorial guide and the *Little Aligner* drill book were expanded (a new packages chapter, four-at-a-time SIMD reading drills), and the Vim / Emacs / VS Code editor support was refreshed. The `std.regex` design spec (`docs/impl/std-design/regex.md`) is complete, with its Japanese mirror in step.

## Backward Compatibility Warning

**Align makes zero backward compatibility guarantees during the 0.x series.** As we iterate towards a stable 1.0, the language syntax, standard library APIs, and ABI may break without warning or legacy fallbacks. v0.3.0 is purely additive over v0.2.0 — `std.regex` is new surface, so no existing program changes behavior.

## Known Intentional Limitations

Carried over from v0.2.0 (unchanged): `extern "C"` export-of-body; Windows (Align targets Linux x86-64/aarch64 and macOS Apple Silicon); capturing escaping closures; no database drivers; no application state in handlers; JWT is HS256 only; multipart is not wired into the core web surface by design.

New in v0.3.0, specific to `std.regex`:

- **No look-around or backreferences.** This is the automata engine's contract — the price of the linear-time guarantee — not an omission.
- **No `rx"..."` literal syntax or compile-time pattern validation.** A pattern is an ordinary `str` compiled at runtime; an invalid pattern is a runtime `Error.Invalid`, not a compile error.
- **No implicit cache.** The compiled `regex` handle *is* the cache, and its lifetime is visible; there is no hidden global/thread-local pattern cache.
