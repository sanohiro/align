# pkg — template

> English is authoritative. A synchronized Japanese mirror lives at `ja/template.md`.
>
> **Status:** PROPOSED. This document is the authority for the first `pkg.template` capability.
> Design acceptance activates no package source, compiler operation, runtime row, or ABI shape.

## Authoritative public-contract ledger

This ledger is authoritative. Later prose and implementation may make a cell more explicit but
must not widen it. V1 is one HTML text builder whose ordinary write path escapes by default. It is
not a template parser, DOM, component model, file loader, or contextual JavaScript/CSS/URL encoder.

| Public surface | Exact inputs, defaults, validation, and evaluation | Exact result, effects, and errors | Ownership, lifetime, allocation, and cleanup | Compiler/runtime/package owner, artifact, and cache identity | Prerequisite and acceptance owner |
|---|---|---|---|---|---|
| qualified path segment `template` | In `module`, `import`, type, and value paths, the token `template` is admitted only as a noninitial segment immediately after `.`. Thus `pkg.template`, `pkg.template.internal.resource`, and `pkg.template.html()` parse. At expression head, exact `template` followed by a string token remains the shipped template expression. A bare declaration/reference named `template`, `template.html()`, another keyword segment, or any other keyword-as-identifier use remains rejected. | Compile-time only; no runtime value, effect, allocation, or error. The formatter emits the canonical spelling unchanged and round-trips both the qualified segment and template expression without ambiguity. | No ownership or lifetime effect. | Lexer token identity remains unchanged. Parser path-segment admission and formatter traversal own the rule. Canonical module names, imports, interfaces, dependency keys, and diagnostics retain the exact `pkg.template` bytes. | Shipped dotted module paths and contextual template expression. Parser/formatter positive/negative matrix, span diagnostics, source round trip, whole/per-unit resolution, and no-other-keyword widening owner. |
| `pub resource html_builder = pkg.template.internal.resource.drop_html_builder` | Nominal arity-zero Move resource. It is constructed only by canonical `pkg.template.html()`. It is non-Copy, non-comparable, non-printable, and has no public raw/view conversion. It uses the complete shipped resource carrier grammar: direct locals and by-value parameters/returns plus recursively owning records, user sums, tuples, `Option`, and `Result`; `write`/`raw` take only a mutable borrow. Constants/globals, fixed or dynamic collection elements, boxes of owned values, function values/captures, tasks, parallel work, and extern ABI remain rejected by existing rules. | Pure owner. Its observable state is the ordered byte prefix appended so far. No error value, length/capacity query, reset, clone, or partial view exists. Detectable malformed private state aborts before dereference or mutation. | Owns one runtime builder shell and its allocator-compatible grow buffer. Move nulls the complete source or selected aggregate path. Unfinished recursive Drop frees shell and payload exactly once; null Drop is a no-op. No borrow survives a call. | `pkg.template` owns nominal identity; internal resource/descriptor modules own construction and Drop. The complete resource spelling, hook, wrappers, checked operations, and runtime keys enter whole-program/per-unit interface, dependency, object, and link identity. | Shipped package resources, recursive move/borrow/Drop, canonical-package admission, and builder allocation. Exact direct/record/sum/tuple/Option/Result graph, move/replacement/return/control-path/Drop, forbidden collection/capture/extern carriers, malformed state, and whole/per-unit owners. |
| `pub fn html() -> html_builder` | No arguments, overload, default, ambient arena, locale, document mode, or allocator option. The canonical root wrapper evaluates once and invokes only the canonical internal descriptor operation. | Pure. Returns an empty live builder. OOM aborts. An impossible null runtime result aborts before resource publication. | Allocates one fixed-size shell. Initial payload is canonical null/zero with no payload allocation. The returned owner may move normally. | Canonical root `pkg.template`, empty `pkg.template.internal.descriptor`, and private `pkg.template.internal.resource`. Checked `TemplateHtmlNew` HIR/MIR lowers to `TemplateHtmlNew`, runtime key `TemplateHtmlNew`, existing pointer-return ABI shape. | Empty construction/Drop, allocation/free counters, null-result abort, direct/imported/function-return, whole/per-unit, interface edit/revert, and optimized/unoptimized lowering. |
| `pub fn write(borrow mut output: html_builder, value: str)` | `output`, then `value`, evaluate exactly once left-to-right. An owned `string` auto-borrows by the ordinary `str` rule. The complete input is valid UTF-8 by type. Every input byte is processed in order. Exact substitutions are `& -> &amp;`, `< -> &lt;`, `> -> &gt;`, `\" -> &quot;`, and `' -> &#39;`; every other byte, including NUL, CR/LF, non-ASCII UTF-8, and an existing entity spelling, is copied unchanged. Existing entity text is therefore escaped again. | Pure exclusive mutation; returns Unit. It is safe as HTML element text and as the complete contents of a single- or double-quoted attribute. It does not validate surrounding markup or make unquoted attributes, URL schemes, event handlers, CSS, JavaScript, comments, tag/attribute names, or foreign-content grammars safe. No recoverable error exists. Detectable malformed owner/view shape or output-length overflow aborts before mutation; OOM aborts. | Borrows both arguments only for the call and retains neither. It performs no temporary owned-string allocation and no full-output copy. It first computes the checked escaped byte count without mutation, reserves once if growth is needed, then appends the exact bytes. Empty input is a no-op. | Canonical wrapper plus checked `TemplateHtmlWrite` HIR/MIR. Runtime key `TemplateHtmlWrite` reuses the exact five-entity table owned by `encoding.html_escape`; it must not carry a second table. Existing void `(ptr, ptr, i64)` ABI shape and empty curated LLVM attributes. | Five substitutions individually/together/every ordering, existing entities, empty/NUL/controls/UTF-8, quoted-attribute and text parses, unsafe-context negative fixtures, output exact/overflow, no-temporary-allocation, alias/state validation, whole/per-unit, and differential parity with `encoding.html_escape`. |
| `pub fn raw(borrow mut output: html_builder, value: str)` | `output`, then `value`, once left-to-right. `string` auto-borrows. Bytes append exactly as supplied, with no entity recognition, escaping, normalization, parsing, validation of HTML syntax, or trust marker retained in the result. | Pure exclusive mutation; returns Unit. This is the sole public unescaped append operation on `html_builder`. Calling it is the explicit trust boundary; it may emit malformed or unsafe HTML. Detectable malformed owner/view shape or length overflow aborts before mutation; OOM aborts. | Call-only borrows, no retention or temporary owned allocation. Checked length/reserve precedes the first mutation. Empty input is a no-op. | Checked `TemplateHtmlRaw` HIR/MIR and runtime key `TemplateHtmlRaw`; existing void `(ptr, ptr, i64)` ABI shape and empty attributes. | Byte-exact empty/ASCII/NUL/UTF-8/markup, mutation/overflow ordering, no-copy/no-retain, sole-bypass inventory, whole/per-unit, and optimized/unoptimized owners. |
| `pub fn to_string(output: html_builder) -> string` | Evaluates and consumes `output` once. It is the only public finisher; there is no `finish`, `build`, `as_str`, view, writer conversion, or implicit conversion. The source must be an initialized owner expression accepted by ordinary path-selected Move-call rules. | Pure. Returns the complete appended byte sequence as an owned valid-UTF-8 `string`. Detectable malformed state aborts before publication. | Transfers the allocator-compatible payload without allocation or copying and frees only the shell. It nulls the selected source path before any later cleanup edge. Empty output returns the canonical null/zero owned string. After transfer, resource Drop is a no-op and ordinary string Drop frees the payload exactly once. | Checked `TemplateHtmlToString` HIR/MIR and runtime key `TemplateHtmlToString`; existing owned-string `(ptr, i64) @SYM(ptr)` ABI shape. Interface, object, and link identity retain consuming ownership and result type. | Empty/nonempty zero-copy pointer identity, path-selected source nulling, string Drop, all control exits and by-value helper returns, malformed state, double-use rejection, whole/per-unit, and cache identity owners. |

## Boundary and exact source surface

The capability is deliberately one append-only state machine:

```text
html() -> live html_builder
  write(text)  -> append the five-entity escaped text
  raw(markup)  -> append byte-exact trusted markup
  to_string()  -> consume and transfer the complete owned string
```

The vendorable source topology and declarations are exact:

```align
module pkg.template

import pkg.template.internal.descriptor
import pkg.template.internal.resource

pub resource html_builder = pkg.template.internal.resource.drop_html_builder

pub fn html() -> html_builder = pkg.template.internal.descriptor.html()

pub fn write(borrow mut output: html_builder, value: str) =
  pkg.template.internal.descriptor.write(output, value)

pub fn raw(borrow mut output: html_builder, value: str) =
  pkg.template.internal.descriptor.raw(output, value)

pub fn to_string(output: html_builder) -> string =
  pkg.template.internal.descriptor.to_string(output)
```

`pkg.template.internal.descriptor` contains no source declarations. Its four spellings are
compiler-private operations admitted only from the exact canonical wrappers above. The application
cannot import either internal module. A same-named application module/function/extern, modified
wrapper, additional internal declaration, or noncanonical package path cannot select an operation.
`pkg.template.internal.resource` contains only the exact public raw Drop hook needed by the nominal
resource; the hook is inaccessible outside its package subtree and delegates to the one runtime
free row.

```align
module pkg.template.internal.resource

extern "C" {
  fn align_rt_template_html_free_v1(state: raw)
}

pub fn drop_html_builder(state: raw) {
  unsafe { align_rt_template_html_free_v1(state) }
}
```

The package intentionally does not expose the underlying ordinary `builder`. If it did, a caller
could invoke `builder.write` and bypass escaping without naming `raw`, contradicting the defining
contract. Likewise, `html_builder` exposes no borrowed view before consumption.

## Public use

Declarations and calls remain separate. Markup is visibly trusted; dynamic text uses the default
escaped path:

```align
import pkg.template

fn page(name: str, count: i64) -> string {
  mut out := pkg.template.html()
  pkg.template.raw(out, "<p class=\"summary\">")
  pkg.template.write(out, name)
  pkg.template.raw(out, " has ")
  pkg.template.write(out, template "{count}")
  pkg.template.raw(out, " items</p>")
  return pkg.template.to_string(out)
}
```

The nested plain template allocation is explicit. V1 does not add numeric overloads or another
formatting mechanism: callers use the shipped `template`/`builder` scalar formatting surface and
hand its `str` result to `write`, or spell trusted fixed digits through `raw` when that trust is
intentional.

## Escaping and context contract

The byte mapping is exactly the shipped `encoding.html_escape` mapping and uses the same runtime
helper. It is not HTML entity decoding or canonicalization. In particular:

- `&amp;` becomes `&amp;amp;`;
- `<script>` becomes `&lt;script&gt;`;
- both quote bytes are escaped, so one result works in either kind of quoted attribute; and
- valid non-ASCII UTF-8 remains byte-identical.

The safety promise requires the surrounding trusted markup to place one complete `write` result in
element text or inside already-opened matching quotes. Escaping cannot enforce semantic policy for
URLs, event handlers, inline styles/scripts, HTML comments, element/attribute names, or a fragment
split across calls. Those domains require a dedicated encoder or validation before `raw`. V1 has no
context tracker because pretending that five-entity escaping validates those languages would be a
false security boundary.

## Evaluation, state, and deterministic failure order

Public calls use ordinary eager left-to-right evaluation. The compiler first validates canonical
module/wrapper identity and exact types. At runtime each operation follows this order:

1. Validate the output pointer, alignment, version, live state, reserved bytes, and builder pointer
   before dereferencing the builder. A malformed state aborts.
2. For `write`/`raw`, validate signed length, target-address representability, and nullness before
   forming an input slice. A zero length may use null; a positive length may not. A dangling
   nonnull pointer remains outside the detectable ABI contract.
3. Compute the exact added byte count and checked final length before mutation. `write` scans the
   input once for this measurement using the shared entity table; `raw` uses the input length.
4. Reserve sufficient payload storage. Allocation failure hard-aborts without a recoverable
   partial result.
5. Append bytes in order and commit the new length last.

`to_string` repeats the complete state validation, then atomically marks the wrapper spent, removes
the payload owner, frees the shell, and publishes the owned string. Drop validates a nonnull state
before cleanup, removes payload ownership once, and is null-safe after a move or finish. There is
no fallible public operation and therefore no error-precedence sum or cleanup-error channel.

## Compiler, runtime, ABI, and cache closure

The implementation adds four checked HIR/MIR operations and four keyed runtime calls plus the
resource Drop call. All five use already-shipped ABI shapes; no shape is reserved and A124 remains
the next unused active shape.

| Operation | Runtime key and symbol | Exact existing ABI shape |
|---|---|---|
| `TemplateHtmlNew` | `TemplateHtmlNew`; `align_rt_template_html_new_v1` | `ptr @SYM()` |
| `TemplateHtmlWrite` | `TemplateHtmlWrite`; `align_rt_template_html_write_v1` | `void @SYM(ptr, ptr, i64)` |
| `TemplateHtmlRaw` | `TemplateHtmlRaw`; `align_rt_template_html_raw_v1` | `void @SYM(ptr, ptr, i64)` |
| `TemplateHtmlToString` | `TemplateHtmlToString`; `align_rt_template_html_into_string_v1` | `{ ptr, i64 } @SYM(ptr)` |
| Drop hook | `TemplateHtmlFree`; `align_rt_template_html_free_v1` | `void @SYM(ptr)` |

On supported 64-bit targets the runtime shell is one exact 32-byte, 8-aligned private record:

```text
offset 0   u32 version = 1
offset 4   u8  lifecycle = 0 (live)
offset 5   u8  reserved = 0
offset 6   u16 reserved = 0
offset 8   ptr payload; null iff capacity = 0
offset 16  u64 length
offset 24  u64 capacity
```

The complete live product is version one, lifecycle zero, zero reserved fields,
`0 <= length <= capacity <= isize::MAX`, and payload null exactly when capacity is zero. No spent
shell is published: consuming finish removes and frees the shell after taking its payload, while the
compiler nulls the source resource slot. The embedded grow buffer reuses the ordinary builder's
allocation and five-entity helpers. Layout/size assertions and a malformed-product sweep fail closed
if the native record drifts. Checked HIR validates exact canonical operation kind, resource identity, input/result
types, effects, and borrow/consume mode. MIR repeats those facts and rejects forged operations
before LLVM. LLVM declaration/call preflight checks key/signature agreement and keeps the current
empty function-attribute set: allocation, mutation, abort, and ownership transfer forbid stronger
attributes.

The root/internal source bytes, resource hook identity, public signatures, checked operation kinds,
runtime keys, and shared escape-table semantics participate in interface/frontend/object/link cache
identity through the existing mechanisms. Input text and emitted bytes do not. Whole-program and
per-unit compilation must produce the same operation and runtime inventory. Design acceptance alone
changes no active inventory.

## Implementation closure matrix

This matrix is required before implementation and owns the cross-layer proof. One parameterized
owner may close multiple cells when it fails for every listed defect.

| Closure cell | Required implementation evidence | Exact owner |
|---|---|---|
| contextual path and canonical formation | Only noninitial qualified `template` segments parse; template expressions remain unchanged; exact root/internal topology and wrappers form the four operations; all bare/other-keyword, same-name, altered-signature/body, extra-item, and application-import twins reject before MIR. | parser/formatter + package source/admission owner |
| construction and move-in/out | New publishes one live owner; direct/imported return, by-value relay, reassignment, branch/match/loop joins, and caller return preserve one source nulling and one final Drop. | package lifecycle owner |
| mutable borrow | `write`/`raw` accept a bound initialized direct or path-selected aggregate owner through `borrow mut`, enforce all-peer exclusivity, and preserve the containing owner. Shared borrow, moved source, unbound temporary misuse, retention, collection access, capture, task, parallel, and extern paths reject. | compiler resource-carrier sweep |
| escaped append | Every entity and byte class, split sequence, repeated writes, existing entity, and empty input match the shared encoder byte-for-byte. Measurement/reserve precedes mutation. | runtime differential + package integration owner |
| raw append | Exact bytes append with no transform; it is the only reachable unescaped operation and cannot expose the underlying builder. | operation inventory + byte golden owner |
| finish and return | Empty/nonempty finish transfers pointer/length without copy, nulls the source, frees shell once, and leaves payload to one string Drop across direct/imported/helper/control returns. | allocation/pointer-identity lifecycle owner |
| early exit and replacement | Unfinished owners clean once on normal fallthrough, return, `?`, `map_err`, `else`, branch, match, loop break, replacement, malformed downstream input, and enclosing owner Drop. | parameterized cleanup owner |
| malformed HIR/MIR/native | Wrong type/resource/effect/borrow/consume/key/signature/state/version/reserved/pointer/length/alignment products fail before unsafe formation, mutation, allocation, or publication. | checked-HIR/MIR mutation sweep + runtime abort subprocess owner |
| allocation parity | Constructor shell, zero payload for empty, no temporary escaped string, capacity-sufficient no-growth, growth, overflow-before-mutation, OOM abort, zero-copy finish, and exact free counts agree. | allocation/failpoint/resource owner |
| interface, ABI, and cache | Semantic-to-byte and byte-to-semantic operation/resource records, runtime declarations/exports, whole/per-unit identity, edit/revert invalidation, and optimized/unoptimized lowering agree; A124 remains unused. | interface/ABI/cache golden owners |
| documentation and examples | English authority, Japanese mirror, roadmap/handoff/open-question record, syntax-checked declarations and call example agree. | documentation consistency and package-source syntax owner |

The implementation is one capability PR. Splitting producer operations from their only consumer
would leave dormant privileged operations with no independently useful stable boundary. If the
hand-written diff is expected to exceed roughly 1,000 lines, the PR records that this single
resource lifecycle and shared escape-table proof avoids duplicate carrier, ABI, and cleanup proofs.

## Deferred surface

V1 deliberately has no `html "..."` syntax, contextual template parser, interpolation AST,
component/slot/layout system, condition/loop/include DSL, reflection, map/dynamic value, file or
cache loader, hot reload, streaming writer, arena form, borrowed result, capacity option, reset,
clone, escaping disable flag, HTML unescape/parser/sanitizer, DOM, URL/CSS/JavaScript encoder, URL
scheme policy, CSP/nonce support, attribute-name/tag-name construction, or framework integration.
Each needs a real consumer and its own exact context and ownership contract.

Plain language `template "..."` remains the one scalar formatter and does not acquire escaping.
`encoding.html_escape` remains the allocate-and-return codec. `pkg.template.write` is the first
builder-sink consumer of its exact table, not a second table or an alias for the codec result.

## Design-review record

No independent review has yet accepted this ledger. Findings that change a public surface update
the ledger first and then all prose/mirrors in one pass.
