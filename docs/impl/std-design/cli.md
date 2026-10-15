This directory records the detailed `std` module designs that supplement the roadmap.
These specifications define the contract for each module's implementation.

# std.cli — implementation design (M10 Slice 3)

> 🌐 **English** · [Japanese](./ja/cli.md)

> **Status:** shipped in M10. This file is the implemented contract plus its historical slice plan.

## Overview

A parser over `main(args: array<str>)`'s `array<str>` (§17) — the one argv source (no
`env.args`). Pure in-language, no syscalls. v1 is an explicit flag-registration builder;
struct-decode (the `json.decode`-shaped ideal) waits for derive. This is the last M10 slice.

## Signatures

From `draft.md` §18.2, authoritative:

```text
c := cli.command(name: str)                    // builder; returns a cli command (Move)
c.flag_bool(name: str)                          // register a bool flag (default false)
c.flag_str(name: str, default: str)             // str flag with a default
c.flag_i64(name: str, default: i64)             // i64 flag with a default
c.flag_f64(name: str, default: f64)             // register an f64 flag with a default
c.parse(args: array<str>) -> Result<parsed, Error>
p.get_bool(name: str) -> bool                   // total after a successful parse
p.get_str(name: str) -> str
p.get_i64(name: str) -> i64
p.get_f64(name: str) -> f64
p.occurrences(name: str) -> i64                 // explicit uses; zero for an omitted flag
c.usage() -> string
```

## Type & ownership classification

The load-bearing decision:

- `command` and `parsed` are **Move types** — new `Ty::CliCommand` / `Ty::CliParsed`. They own
  internal heap buffers (the registered-flag table; the parsed name→value map with owned `string`
  values), so they follow the reader/writer/buffer Move path, NOT the Copy rng path. Drop frees
  the internal buffers (flag table entries incl. owned default/parsed strings). Deep-drop like
  `read_dir`'s `array<string>` (#339): each owned `string` inside is freed. (Terminology: `str` is
  a borrowed read-only view; `string` is the heap-owned Move type — the values these tables own are
  `string`s.)
- **`parse` borrows `c`, it does NOT consume it.** `c.parse(args)` takes `c` by shared borrow (it
  reads the flag table; it does not move the command). So `c.usage()` stays callable *after* parse
  — including on the `Err` path, which is exactly when you want to print help. (If parse consumed
  `c`, a parse failure would leave you unable to render usage — the reason this is called out.)
- They are rejected as array/slice/vec/box element types and as Option/Result payloads at the
  single `scalar_arg` choke point — same as reader/writer (Slice 1 precedent). (`parse` returns
  `Result<parsed, Error>`, so `parsed` DOES appear as a Result Ok payload — allow `parsed` in the
  Ok position exactly like reader/writer were allowed in Option/Result payload positions;
  buffer's `Scalar::Buffer` precedent from #346 is the template.)
- `p.get_*` return values: `bool`/`i64`/`f64` and occurrence counts are Copy; `get_str` returns a `str` **view into the
  parsed structure** — region-bound to `p` (the parsed value), like `json.decode` field views are
  bound to the decoded arena/value. So `region_of(CliGetStr) = region_of(p)`; escaping the str
  past `p`'s drop is rejected; `.clone()` copies out. (Do NOT return Static — that is the #297
  wildcard trap. Add the explicit `region_of` arm.)

## Effect classification

All cli ops are **Pure** (no syscalls — argv already captured by `main(args)`). But
`command`/`parsed` are Move, so they never ride a `par_map` closure regardless. Still mark Pure
for correctness of effect inference.

## Error policy

- `c.parse(args)` input errors — unknown flag, missing value for a str/i64/f64 flag, malformed i64/f64
  literal, wrong kind — return `Error.Invalid` (the fixed mapping, no errno syscall path; build
  the Error directly like encoding's decode).
- `p.get_bool/get_str/get_i64/get_f64` for a name that was never registered, or against the wrong flag
  type — **runtime abort** (the settled decision from #345 review: Align has no comptime, so a
  `get_*` call cannot be statically checked against the flag set a builder registered at
  runtime; abort like OOB-index/div-by-zero, "programmer error aborts, never silently
  misbehaves"). This is NOT a compile-time error and NOT a Result.

**Finite floats and occurrences.** `flag_f64` takes an exact `f64` default.
Before reading argv or allocating parsed entries, `parse` rejects any registered
nonfinite default with `Error.Invalid`, including a duplicate registration or an
overridden default. Float argv values must be complete ASCII JSON numbers:
`-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`. Whitespace, leading plus/zeros,
hex, NaN, infinity, embedded NUL and trailing bytes reject. Conversion rounds to
binary64 with nearest/ties-to-even independently of host rounding mode; overflow
rejects, while subnormals, underflow to signed zero and negative zero are valid.
Application-specific ranges remain explicit caller checks.

`occurrences` accepts every registered flag kind. It counts explicit options,
including repeated bools and empty string values, across both value spellings;
defaults count zero. Separate value tokens are consumed verbatim, even if they
look like flags. Values still use the last occurrence; repeated registrations
still select the first registered kind/default for that name. Each parse starts
new counts. Checked count overflow returns `Error.Invalid`. Unknown names abort
like an unknown getter; `get_f64` also aborts on the wrong kind. Both operations
borrow their bound parsed local and name only during the call, allocate nothing,
and return independent Copy scalars. They do not extend string-view lifetimes.
Parsing still owns its copied strings, releases partial entries on failure and
keeps the command reusable. Usage adds registration-order f64 defaults, including
nonfinite defaults before parse rejects them, using existing display rendering.

The exact compiler/runtime closure and tests are in
[plan181](../181-cli-float-occurrences.md).

## New machinery required

Two new Move `Ty` (CliCommand, CliParsed) + their runtime structs + Drop; the builder methods
(flag_bool/str/i64/f64), parse, and the four total getters and occurrence observation + usage as sema builtins behind
`import std.cli` with the `name_in_scope` shadowing guard (the #340 helper); a new `region_of`
arm for `CliGetStr` (view into parsed). No new effect, no new syscall.

## Slice breakdown

Single slice, but ordered:

1. The two Move types + runtime structs + Drop (deep-free owned strings) + Gate-1 sweep across
   all passes (the reader/writer + `read_dir` `array<string>` template).
2. Builder: `command` + `flag_bool`/`str`/`i64`/`f64` (store name, kind, default into the command's
   table).
3. `parse`: tokenize argv (`--name`, `--name=value`, `--name value`, `-` conventions per draft —
   keep v1 minimal: `--name` for bool, `--name value` and `--name=value` for str/i64/f64), validate
   against the table → `Error.Invalid` on any input error, else build parsed.
4. Getters (total, abort on unregistered/wrong-type) + `usage` (render from the table).

## Pitfalls (implement carefully)

- **P1 (Move sweep)**: two new Move Ty must be swept through EVERY pass exactly like
  reader/writer — `ty_is_move`, `tracks_region`, `null_moved_source`, drop insertion,
  `MoveCheck`, `EscapeCheck`, `region_of`, finalize, MIR lower, codegen, print. A miss = double-free
  or use-after-move. Highest risk.
- **P2 (bound-receiver, #337/#338 lesson)**: `command`/`parsed` are owned Move; an unbound
  temporary cannot be a method receiver in v1 (bind first). So `cli.command("x").flag_bool("v")`
  chaining and `c.parse(args)?.get_bool("v")` remain rejected by the v1 receiver surface even
  though general Move-temporary cleanup landed 2026-07-15 — require
  `c := cli.command("x"); c.flag_bool("v"); ...` and `p := c.parse(args)?;
  p.get_bool("v")`. Design the bound-receiver gate into `check_cli_*` from the start (the
  `check_reader_method`/`check_writer_method` precedent).
- **P3 (get_str view region, #297 trap)**: `get_str` returns a str view into parsed; its region
  MUST be `region_of(p)`, not Static. Explicit `region_of` arm + an escape-rejection test.
- **P4 (get_* runtime table lookup)**: the getter cannot resolve the name statically; codegen
  emits a runtime lookup into the parsed table that aborts on miss/type-mismatch. Confirm the
  abort path uses the existing abort mechanism (OOB/div-by-zero style), not a silent default.
- **P5 (deep-drop)**: command's flag table and parsed's value map hold owned strings (defaults,
  parsed values) — Drop must free each, like `read_dir`'s `array<string>` deep-free (#339). A
  shallow free leaks; a double-free crashes.

## Test checklist

- bool present/absent (default false)
- str/i64/f64 with default and overridden
- `--name=value` and `--name value` forms
- unknown flag → `Error.Invalid`
- missing value → `Error.Invalid`
- malformed i64/f64 or nonfinite float → `Error.Invalid`
- `get_*` unregistered name → abort
- `get_*` wrong type → abort
- `get_str` view escaping `p` → compile error
- `get_str` `.clone()` escapes OK
- `usage()` renders all flags
- command/parsed as array/box element → rejected
- unbound-temporary receiver → rejected (P2)
- deep-drop no leak/double-free (valgrind-style or the existing RSS/drop test pattern)
- import-required
