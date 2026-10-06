# HTML escape text admission

`encoding.html_escape` currently accepts arbitrary bytes and copies non-entity
bytes into an owned `string`. `hex_decode("ff")?.bytes()` reaches stdout as
invalid UTF-8 with exit 0 on macOS and Linux. Restrict this text transformation
to the existing `str` domain and independently validate its native input.

## Public contract ledger

| Field | Contract |
| --- | --- |
| Surface | `encoding.html_escape(data: str) -> string`, Pure. Ordinary owned `string` arguments auto-borrow for the call. Raw byte views are rejected at compilation; the canonical explicit conversion is `data.as_str() -> Result<str, Error>`. No new overload, alias, implicit validation, lossy conversion or error model. |
| Text and bytes | Input is complete valid UTF-8. Escape exactly `& -> &amp;`, `< -> &lt;`, `> -> &gt;`, `" -> &quot;`, and `' -> &#39;`. Copy every other byte unchanged, including NUL, CR/LF, controls and Unicode. Existing entity spellings are escaped again. No normalization. |
| Context | Preserve the existing element-text and complete single/double-quoted attribute-content use. This operation does not validate surrounding markup or URL, script, CSS, comment, unquoted attribute or foreign-content grammars; no expanded security promise. |
| Errors and ordering | Safe typed calls are infallible except terminal allocation/size failure. The unchanged unsafe native row independently admits signed length, pointer/range shape, then complete UTF-8, then checked output size, then allocation and fill. Detectable malformed shape or invalid UTF-8 aborts before allocation/publication; one fixed admission diagnostic covers those failures, so multi-invalid precedence is deterministic. No recoverable partial result. |
| Native safety | Keep `{ ptr, i64 } @align_rt_html_escape(ptr, i64)`. Reject negative/unrepresentable length, null positive-length pointer and address-wrap extent before slice construction. Zero length permits null and returns canonical null/zero. Nonempty input must otherwise be one live readable initialized allocation for the call and remain stable; no runtime proof of provenance or arbitrary pointer validity is claimed. UTF-8 is checked at this boundary rather than left to the unsafe caller. |
| Ownership and allocation | Borrow input only for the call; retain nothing. Nonempty output owns exactly one final Align allocation, contains exact initialized escaped bytes and does not alias input; no intermediate owned buffer. Empty output allocates nothing. Existing string move/nulling/replacement/return/Drop rules release it exactly once. |
| Compiler and runtime owners | Sema admits text and auto-borrows string, returning an Error sentinel for invalid children. Existing EncodingEncode/Html HIR and MIR rows require a Str operand, produce String, and keep fresh-owned provenance. No new type/variant/serialization tag. LLVM remains ordinary ptr/len marshalling. Runtime owns the independent validation and existing shared entity writer. |
| ABI, identity and consumers | Keep runtime key HtmlEscape, ArgRead/call-only input, A84 physical shape and existing curated attributes. Compiler build identity and runtime-source fingerprint invalidate stale artifacts. Existing interfaces serialize the same types/operation; rebuilt consumers obey the narrowed input domain. Other byte encoders, Utf8Lossy, and pkg.template's already-validated text input retain their contract. |
| Prerequisites and sources | Existing text views, bytes.as_str, Move string and EncodingEncode suffice; K1 remains deferred. Synchronize draft, digest, design rationale, Settled note, roadmap encoding record, checked-HIR ledger, runtime ABI note, and English/Japanese encoding guide. This plan owns the exact admission record. |

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation and invalid source | check_encoding_op text-only Html branch, ordinary check_str_init and a resolved-Ty::Str guard (the helper may diagnose and return a non-Str child). New `html_text` driver owner accepts literal/borrowed/owned/temporary text and rejects bytes, wrong scalar, unresolved/already-invalid child and bad arity without compiler panic or artifact output. Existing m10_encoding owns binary codec admission. |
| Checked HIR and MIR provenance | validate_hir requires Str specifically for Html; producer requires the same operand shape and existing owned output. An unconditional statement guard checks the Html operand/result signature even when the result is discarded; the demand-driven ownership graph alone cannot close that case. Parameterized `html_text` compiler owners mutate an otherwise valid encode to a bytes or owned-string operand and reject it with both returned and discarded results; sibling binary transforms retain byte admission. No new analysis variant or access rule. |
| Native admission | New `html_text_tests` native owner covers negative/null-positive/address-wrap, all UTF-8 invalid families at leading/middle/trailing positions, late invalid bytes after escapable prefixes, zero-null and multi-invalid shape. Immediate bounded child guard owns abort tests; require the fixed admission diagnostic and SIGABRT before result publication. Force the next Align allocation to fail in each isolated invalid-input child; a valid nonempty OOM control has a distinct allocation diagnostic. This witnesses admission before allocation. Validate lengths/pointers before slice, UTF-8 before allocation. |
| Exact output and allocation | Native independent entity oracle over all ASCII bytes, Unicode boundary scalars, combined/order/existing-entity/empty cases; verify valid UTF-8, exact length/content, independent allocation, input preservation and one allocation/free (zero for empty) in an isolated allocation-counter child. Existing pkg.template native differential owner retains the shared table parity. |
| Move-in/out, nulling, Drop, replacement, return | Existing String ownership machinery unchanged. Driver uses imported/generic helpers, input dropped before output use, owned input reused after call, returned escaped strings and replacement. Native allocation owner proves direct output release once. Existing string/tagged ownership owners close general carrier cleanup. |
| If/match/else/?/map_err, joins and early exits | Driver composes explicit bytes.as_str with successful/failed Result propagation, mapping and else, branch/loop scalar joins and early returns. Invalid source byte admission never reaches codegen; invalid native text never publishes output. No changes to existing control-flow analysis. |
| Whole/per-unit, generic, interfaces and native linking | Run one imported consumer with whole/per-unit and runtime LTO off/on. Reuse ordinary imported generic string flow and current ABI declaration owners; no codec or artifact format change. Inspect generated HtmlEscape declaration and run linked output on macOS/Linux. |
| Test isolation | Driver ArtifactStage owns all generated sources, objects, executables and capture files; native captures use an exclusively created private directory with immediate RAII ownership; no TempProject or process-global environment mutations. Native exact-filter children and generated programs have immediate deadlines and unwind-safe kill/reap. No service dependencies. |

One capability joins source, checked IR and native enforcement: leaving any one
boundary unchanged preserves a route to invalid owned text. Expected change is
below 1,000 handwritten lines. No performance claim or benchmark gate; the
separately measured duplicate-scan optimization remains a later capability.

## Acceptance bindings

- `align_runtime --features alloc-count --lib html_text_tests`: native admission,
  exact independent output, one-allocation/free and empty parity.
- `align_runtime --lib template_html_write_matches_the_shared_escape_owner`:
  existing shared-entity differential.
- `align_driver --test html_text --test m10_encoding`: source and linked
  whole/per-unit/runtime-LTO behavior plus unchanged byte codec family.
- `align_mir --lib html_text`: forged HIR and MIR producer admission.
- `align_codegen_llvm --lib runtime_abi`: unchanged declaration/effect closure.
- Workspace build, bounded gate, Clippy, and final-SHA preflight before push.

Author ledger-to-prose and matrix-to-diff extraction precede the independent
strategy review and the later implementation review. Normative guide examples
are included in the compiled owner source.
