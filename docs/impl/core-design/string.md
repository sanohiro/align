This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — str / string / builder / template

> 🌐 **English** · [Japanese](./ja/string.md)

## Overview

Text (draft §12–§13): a borrowed view type, an owned buffer type, a builder for assembly, and one
template form. Byte-oriented UTF-8 throughout; the searching methods ride the memchr-class SIMD
scan layer (#310). The load-bearing policy: **every string allocation has a visible home** — an
arena, an owner, or a builder; allocation inside pipeline lambdas is a compile error.

## Signatures and settled surface

```text
"lit"                      -> str        // single-line only; \n \t \" escapes; UTF-8
'A' / 'あ'                 -> char       // one Unicode scalar
s.is_char_boundary(index: i64) -> bool  // total byte-boundary query; no allocation
s.parse_i64()              -> Result<i64, Error>  // checked ASCII decimal; no allocation
s.len()                    -> i64        // BYTE length ("あ".len() == 3)
s.contains(n) / s.starts_with(n) / s.ends_with(n)      -> bool
s.eq_ignore_ascii_case(t)  -> bool       // ASCII fold only, not Unicode
s.find(n) / s.rfind(n)     -> Option<i64>   // byte index of first/last occurrence
s.trim() / s.trim_start() / s.trim_end()    -> str   // ASCII-whitespace; zero-copy sub-view
s[a..b]                    -> str        // range view; region-tied; NO s[i] byte indexing
s.bytes()                  -> slice<u8>  // zero-copy byte view; no UTF-8 obligation
s.clone()                  -> string     // deep copy; the arena-escape hatch
a + b                      -> compile error; builder is the one concatenation path

b := builder()  /  builder(cap)
b.write(s: str|string)  /  b.write_int(i: i64)
b.len()                    -> i64        // initialized UTF-8 byte count
b.to_string()              -> string     // the finisher (there is no finish()/build())

fn append(borrow mut output: builder, text: str)  // direct helper declaration

template "…{expr}…"        -> str        // holes: int, float, str, bool, char; full expressions
```

Receivers auto-borrow: every method above takes `str` or `string` (an owned `string` is viewed,
not consumed). `hash64`/`hash128` also accept these views ([hash.md](hash.md)).

`builder` is a nameable opaque Move type. Direct by-value parameters and results
transfer its owner; `borrow mut` permits all append methods and nested exclusive
forwarding. Shared `borrow` is read-only through scope tails and selected branches.
The caller supplies a mutable binding and may finish after the helper returns.
Borrowed builders cannot move, return, capture, or finish; exclusive replacement
uses ordinary Drop-before-store. Append copies bytes and retains no text view;
passing the handle allocates nothing. Aggregate placement and function-value
formation remain excluded. The existing function-type result grammar can spell
`fn() -> builder`, but forming such a function value remains rejected.
[Plan 82](../82-text-builder-parameter-plan.md) owns this boundary;
`text_builder_params.rs` owns source/native and whole-program/per-unit parity.

`b.len() -> i64` is a Pure, nonconsuming observation of initialized UTF-8 bytes, including embedded NUL.
It takes no arguments, allocates nothing, retains no view or owner dependency,
and may be called on immutable bound locals or admitted borrowed/owned helper
parameters. Bind temporary or control-result receivers first. Reserved capacity
is not length. An unrepresentable count raises the allocation-size hard error
without wrapping or saturation. Further growth and consuming finish remain
valid. [Plan 140](../140-builder-length-plan.md) owns the source/native contract.

## Type & ownership classification

- `str` — Copy view `{ptr, len}`, region = the pointed-at data (literals are region-0/static).
- `string` — owned Move heap buffer; drop frees; reassign-drops-old; auto-borrows to `str`.
- `builder` — an owned accumulator; `to_string` finishes it. Adjacent writes are fused by the
  MIR peephole (`fuse_builder_writes`, `"lit" + int + "lit"` → one runtime call) — new write
  shapes should extend the batcher, not bypass it.
- `template` results are arena-regioned `str` inside an arena. Outside one, dynamic results are
  frame-bounded views over hidden scoped `string` owners; static-only templates are pooled literals
  ([audit 13](../13-string-array-allocation-short-input-audit.md#33-fixed-2026-07-15--arena-free-template-and-jsonencode-have-scoped-owners)).

## Effects

Pure (no I/O). The *allocation-visibility* rules are enforced structurally, not via effects:
`str + str` is a settled hard error everywhere. An arena-free `template` may be consumed locally in
a pipeline lambda, but its frame-bounded view cannot be returned from that lambda (`lambda.rs`). The
checker enforces the uniform concatenation rule and the obsolete MIR path is removed (audit 13 §3.2,
fixed 2026-07-15).

## Errors & aborts

Checked integer conversion returns `Result<i64, Error>`; malformed or out-of-range text is `Error.Invalid`. `s[a..b]` out of bounds aborts. Non-UTF-8 *input* is a `std` boundary
concern (`fs.read_file` → `Error.Invalid`); core string ops assume the invariant and stay
byte-oriented. Range lowering now enforces the promised O(1) UTF-8-scalar-boundary abort at both
endpoints (audit 13 §3.1; fixed 2026-07-13).

`s.parse_i64() -> Result<i64, Error>` parses the entire input as ASCII
`[+-]?[0-9]+`. One optional leading sign, leading zeros and signed zero are
accepted. Whitespace is rejected; trimming is explicit. Empty/sign-only input,
non-ASCII digits, embedded NUL, separators, radix prefixes, fractions, exponents,
and values outside the inclusive i64 range return `Err(Error.Invalid)` without
wrapping or aborting. The receiver is evaluated once and borrowed, including an
owned `string`; the Copy result retains no view. The operation is Pure, performs
no heap allocation or input copy, and does not use locale state.

`s.is_char_boundary(index: i64) -> bool` queries a UTF-8 byte boundary without
allocation. It returns false for negative indices or indices greater than the byte
length, true at zero and the end, and otherwise tests that the byte is not a
continuation byte (`(byte & 0xc0) != 0x80`). Endpoints and invalid indices do not
load bytes. Owned `string` receivers are borrowed as usual. The receiver must
remain live and valid through index evaluation; a bool result retains no view.
The operation is Pure and has no fallible result or bounds trap. Ordinary
`s[a..b]` keeps its range and UTF-8 boundary traps.

`starts_with` and `ends_with` compare bounded bytes without forming an interior
string slice: an empty needle matches and a longer needle does not. They allocate
nothing and compare at most the needle's byte length; no particular libc call or
SIMD speedup is promised.

## Regions

`region_of(trim*/s[a..b]/s.bytes()) = region_of(s)` — sub-views inherit. `clone` → owned,
region-free. A
`string` struct field read borrows as a
Frame-regioned `str` (owned-structs work).

## Spec'd but not implemented

- **`split`** and **`find_any`** (§18.1 catalog) — no dispatch arms. `split` is the big one:
  its return shape (`array<str>` of views — a Move array of regioned views) needs the
  Move-element collection work; do not ship it as an owned-copies compromise ("ideal form, or
  defer"). Today: `find`/`rfind` + `s[a..b]` compose the manual split.
- No direct `s[i]` byte access — use the explicit byte view `s.bytes()[i]` so dropping the UTF-8
  obligation is visible at the call site.
- The §13/§18.1 language template variants (`html`, `raw`, json-template) — only plain
  `template "…"` exists. The designed `pkg.template` capability deliberately leaves that syntax
  unchanged and instead provides an opaque HTML builder with default-escaped `write` plus explicit
  `raw`; contextual language-template parsing remains deferred.

## Pitfalls

- P1 — every search/compare is **byte-oriented**; document char-vs-byte in anything user-facing
  (find returns a *byte* index — valid input to `s[a..b]`, not a char count).
- P2 — `str + str` is a settled hard error, not a pipeline-lambda-only rule. Use builder; do not
  weaken it to a lint or revive the stale arena-concat implementation.
- P3 — `builder.to_string` is the only finisher; adding `finish()` aliases violates One-way.
- P4 — `eq_ignore_ascii_case` is ASCII-only by name and by design; a Unicode case-fold is a
  different (locale-infested) feature — reject as out of scope, per non-goals.

## Test anchors

`m5.rs` (methods incl. find/rfind pairs, trim family, zero-copy bytes views, builder incl. fuse,
template, escapes, UTF-8 byte lengths, print type coverage); `lambda.rs:271/280/287/294`
(template allocation rejection + arena-in-lambda allowance); `hash.rs` (view acceptance);
`fuzz_fmt.rs` (formatter round-trips string-heavy sources); examples `strings.align`,
`template.align`. String-concat rejection is covered uniformly across reducer, named-function,
and lambda contexts. SIMD scan pin: #310 differential oracle.

## In-place byte writes and typed views

The following declarations operate on existing storage; they do not allocate or copy owners.
`S` is one of `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `f32`, `f64`; `E` is `le` or `be`.

```text
slice<u8>.set_u8(offset: i64, value: u8) -> ()
slice<u8>.set_i8(offset: i64, value: i8) -> ()
slice<u8>.set_S_E(offset: i64, value: S) -> ()
slice<u8>.fill(value: u8) -> ()
slice<u8>.fill_S_E(value: S) -> ()
slice<u8>.copy_from(source: slice<u8>) -> ()
slice<u8>.view_le<T>() -> Option<slice<T>>
slice<T>.as_bytes() -> slice<u8>
```

Writes require writable backing and preserve length/capacity. Stores validate the full width;
typed fill requires length divisible by width; copy requires equal lengths. Validation failure
aborts before writing. Copy borrows its source and requires proved independent backing; overlap
or unknown backing rejects. Empty fill/copy is valid; no `fill_u8` alias exists.
[Plan 65](../65-open-issue-batch-plan.md) owns exact access/effect and validation rules.

Typed views are Pure, descriptor-only and retain source lifetime and read/write authority.
`T` is the same eight-type set as `S`; calls write `.view_le()` with `T` inferred from the complete
expected result, never a written type argument. `view_le` returns `None` for invalid length or
alignment and rejects a non-little-endian target at compile time. `as_bytes` exposes native
representation. A `mut` header cannot grant write authority to shared/string-derived backing.
These views promise no automatic SIMD. [Plan 78](../78-checked-byte-view-plan.md) owns the exact
contract; `bytes_ops.rs`, `runway_a2_binary_codec.rs` and `consumer_borrow_boundaries.rs` own the
byte/view checks.

## Explicit constructor capacity

`buffer.filled(length: i64, value: u8) -> buffer` returns exactly initialized
bytes with allocated capacity at least length. It acquires one payload for
nonzero length, none for zero; the Move handle may allocate. Initialization is
O(length). Invalid/overflowing counts abort before allocation; OOM aborts.
The ordinary `buffer(capacity)` remains a best-effort empty read window.

Both constructors accept an optional trailing `alignment: i64`, default 1:
`buffer(capacity, alignment)` and `buffer.filled(length, value, alignment)`.
Arguments are evaluated once in source order. Alignment must be a power of two
from 1 through 536870912. Any other value aborts before payload or handle
allocation, including when capacity/length is zero or invalid; alignment
admission precedes size admission. Existing best-effort reservation and terminal
filled/growth failure policies remain. The first payload address is a multiple
of the requested alignment, including the non-dereferenceable empty sentinel.
Growth preserves alignment, while possibly moving the address; bounded reads
preserve both. Move/return transfers the guarantee with the owner; replacement
adopts the replacement's guarantee. Drop uses the matching allocation layout.
Borrowing, initialized length, usable capacity and view invalidation are
unchanged. This does not align interior offsets, promise physical residency or
speed, or strengthen LLVM view-load assumptions. `align(N)` remains the
struct/fixed-array storage attribute; constructor arguments select buffer payload
storage directly. See [plan 131](../131-aligned-buffer-payload.md).

`buffer.try_new(capacity: i64, alignment: i64 = 1) -> Result<buffer, Error>`
and `buffer.try_filled(length: i64, value: u8, alignment: i64 = 1) -> Result<buffer, Error>`
provide Pure, explicit fallible construction (plan 135). Size is required;
alignment is the optional final argument. Arguments evaluate once in source
order. Admission first checks power-of-two alignment 1..536870912, then a
nonnegative count representable by the target allocation layout, including
alignment rounding. Invalid admission returns `Error.Invalid` before allocation.
Allocator refusal for either payload or handle returns `Error.Code(ENOMEM)`
(12 on supported Linux/macOS); handle refusal releases any acquired payload.
Success owns an independent Move buffer: `try_new` has length zero and usable
capacity exactly capacity; `try_filled` has length/capacity exactly length and
all bytes initialized to value. Zero needs no payload but still needs a handle;
nonzero uses one payload and one handle acquisition. Filled initialization takes
O(length) work. Alignment, move, Drop and view rules are the existing buffer rules.
Existing constructors and later growth retain their failure policies. Success
does not promise physical residency, later growth, or recovery from OS process
termination/overcommit failure during initialization.

`b.capacity() -> i64` is a Pure, zero-argument, nonconsuming query of a
buffer's usable read-window capacity, independent of initialized `b.len()`.
The existing stable local/field and borrowed-payload receiver rules apply;
no allocation or retained view is added. Successful `buffer(n)` reservation
publishes `n`, while invalid/unreservable requests publish zero. Filled buffers
have capacity at least length. Puts, appends and successful `read_line` retain
old capacity or raise it to the new initialized/body length. Short reads, EOF
and failed line reads retain capacity; decoded/returned buffers publish their
initialized length as capacity. `read_line` may grow beyond the old window;
its inclusive 64 MiB stripped-body cap is checked before append, independently
of refill boundaries (plan 102). Existing larger reservations remain. Capacity
limits bounded fills. Hidden allocator spare bytes do not enlarge
that window. The result promises neither physical-memory residency nor future
growth success; existing best-effort construction and terminal growth/OOM
policies remain ([plan 87](../87-buffer-read-capacity-plan.md)).

`b.append_filled(length: i64, value: u8) -> ()` is the family's append member.
It extends a `mut buffer`'s published window by exactly `length` bytes of
`value` in one growth, never a growth sequence or a call per byte. Zero length
is a no-op; a negative or overflowing length aborts before any write, the same
terminal policy as the constructor. Because `buffer(capacity)` reserves without
publishing, this is how a window grows by repeated bytes. There are no typed
`append_filled_S_E` forms: `fill_S_E` overwrites an already-published window of
known length, while an append form would need its own element-count grammar
that no recorded program requires yet.

Expected-type `array_builder()` and `array_builder(out)` accept an optional last
i64 capacity argument: `array_builder(capacity)` and
`array_builder(out, capacity)`. Omitted capacity is zero. Initial length is zero;
at least capacity pushes fit without growth. Count × stride and target-size
checks precede allocation. Heap freeze transfers storage; region freeze retains
contiguous materialization. Element, lifetime, Drop and purity rules are unchanged.
