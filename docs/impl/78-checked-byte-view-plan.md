# Checked zero-copy byte views

Status: implemented for issue 1064. This plan implements G7 of the
[vectorization contract](68-vectorization-contract.md). It replaces the issue's
per-type `as_f32_slice` family and its disproved automatic-vectorization
criterion with one checked, order-explicit view operation and its inverse.

## 1. Public-contract ledger

```text
Surface           Declaration:
                    slice<u8>.view_le<T>() -> Option<slice<T>>
                    slice<T>.as_bytes() -> slice<u8>

                  Calls carry no written type argument:
                    values: slice<f32> := raw.view_le() else { ... }
                    raw: slice<u8> := values.as_bytes()

                  T is exactly u16, u32, u64, i16, i32, i64, f32, or f64.
                  It is inferred from view_le's expected Option/else-unwrapped
                  result type. With no complete expected type, view_le rejects
                  and asks for a binding or return annotation. as_bytes infers
                  T from its receiver. There are no per-type aliases.

Inputs/defaults   view_le takes one slice<u8> receiver and no arguments.
                  as_bytes takes one slice<T> receiver and no arguments.
                  There is no ambient endian, alignment, copy, provider, or
                  allocation option.

Byte order        The descriptor reinterpretation preserves bytes, so it is
                  valid only when the named little-endian order is the target's
                  native order. Every currently supported x86-64 and aarch64
                  target is little-endian. A future supported big-endian target
                  rejects view_le during compilation and tells the user to
                  decode with the existing alignment-1 scalar accessors.
                  as_bytes merely exposes native representation and therefore
                  has no order suffix; interpreting those bytes still requires
                  an order-named consumer.

Runtime result    view_le returns Some when byte_len is nonnegative, byte_len
                  is a whole multiple of sizeof(T), and the receiver pointer is
                  aligned to alignof(T). Otherwise it returns None. The pointer
                  is checked even for an empty non-null sub-slice; a canonical
                  null empty slice is aligned and succeeds. On success the
                  pointer is unchanged and result_len = byte_len / sizeof(T).
                  The checks read no payload byte and have no side effect.

Inverse           as_bytes preserves the pointer and returns
                  byte_len = element_len * sizeof(T). All safe typed-slice
                  producers already guarantee that product is representable as
                  i64; checked lowering treats violation as malformed internal
                  state before publishing a descriptor. It never returns None.

Ownership         Both results are Copy views of the receiver's exact backing
                  storage and generation. Neither operation consumes or clones
                  the source. Return provenance follows the receiver through
                  Option unwrap, assignments, fields, branches, loops, direct
                  and imported calls. A view cannot outlive its source.

Authority         The result carries exactly the receiver's access authority.
                  A writable byte slice may yield a writable typed view; a
                  shared or immutable byte slice yields a read-only typed view.
                  Declaring the copied result header mut does not manufacture
                  writable backing. as_bytes preserves the same rule. Foreign
                  memory continues to enter through resource.view_from_raw;
                  these methods grant no raw-memory authority.

Allocation        None. Both operations are descriptor-only and copy no payload
                  bytes. view_le emits comparisons plus descriptor formation;
                  as_bytes emits checked length scaling plus descriptor
                  formation. No runtime ABI symbol or library call is added.

Errors            Runtime alignment or length failure is None, never an abort.
                  Wrong receiver, arguments, unsupported T, absent expected T,
                  and non-native named order are compile-time diagnostics.
                  Receiver/type checking precedes arity/domain diagnostics.
                  Existing indexing and vector load/store retain their bounds
                  traps after a successful view.

Effects           Pure. No I/O, allocation, global state, locale, errno,
                  floating-point status, ownership transfer, or Drop action.

Owner             align_sema owns method dispatch, expected-type inference,
                  authority and provenance. Checked HIR authenticates receiver,
                  element and result equations. align_mir preserves the exact
                  view operation, source dependency and required little-endian
                  order. align_driver validates that requirement against the
                  resolved target after MIR validation and before cache access,
                  LLVM construction or output. align_codegen_llvm only lowers
                  the already-admitted descriptor and runtime checks.

Artifact/cache    New HIR/MIR operation identities and concrete public-body
                  serialization enter compiler_build_id. Interface format 16
                  replaces format 15 because an exported inline body may contain either
                  operation. Target identity already separates endian policy;
                  no new artifact, manifest, ABI row or cache component exists.

Prerequisite      None. G1-G4 and G8 improve loops that consume the view but are
                  not prerequisites for zero-copy construction. The original
                  claim that a typed view alone vectorizes argmax is withdrawn.

Acceptance        Section 2 owns the exact type/domain, runtime condition,
                  descriptor shape, provenance/authority, whole/per-unit,
                  interface, malformed-HIR/MIR and vector reachability matrix.
                  No latency, vector width, automatic loop vectorization or
                  final machine-SIMD promise belongs to issue 1064.

Mirrors           draft.md binary decode/view section,
                  docs/language-spec.md, docs/design-notes.md,
                  docs/open-questions.md, plans 62 and 68,
                  docs/impl/03-types.md, 04-mir.md, 05-backend-llvm.md,
                  07-roadmap.md, 19-hir-validation-ledger.md and HANDOFF.md.
```

The existing `u32_le(offset)` and sibling scalar accessors remain distinct.
They accept alignment-1 wire data and may byte-swap; `view_le<T>` promises a
zero-copy typed slice and therefore requires native order and natural alignment.
Neither is specified as an implementation of the other.

## 2. Implementation closure matrix

| Cell | Required behavior | Owner evidence |
|---|---|---|
| expected type | each admitted T infers from an annotated `slice<T>` binding, Option binding, return, `else` unwrap and call argument; no context and partial/wrong context reject once | parameterized sema owner plus no-context/wrong-context negatives |
| receiver and arity | only `slice<u8>` receives `view_le`; only `slice<T>` in the closed set receives `as_bytes`; receiver is checked once before arity/domain; fields, subslices and call results dispatch by type | sema receiver/arity table with nested-invalid receiver controls |
| runtime validation | exact/misaligned pointer, exact/nonmultiple length, empty null, aligned empty non-null and misaligned empty sub-slice produce the ledger result without reading payload | execution table over every width and both integer/float classes |
| descriptor shape | success preserves the pointer and divides length exactly; inverse preserves pointer and multiplies length exactly; raw and optimized IR contain no allocation, memcpy or runtime call | LLVM structural owner and round-trip execution owner |
| byte order | MIR retains the little-endian requirement; little-endian supported targets admit it and a synthetic future big-endian target is rejected by driver target admission before cache access or LLVM construction; scalar `_le`/`_be` accessors remain unchanged | target-policy unit owner plus existing binary-codec suite |
| provenance | buffer, array, subslice, parameter, field, returned view and control-flow joins retain the exact source root/generation through view, Option unwrap and inverse | sema/MIR provenance owners across direct and per-unit calls |
| authority | writable source permits typed store and inverse-byte mutation; immutable/shared/string-derived source rejects mutation even when the copied header binding is mut; aliases conflict through either representation | writable/read-only/alias owner matrix |
| vector reachability | `buffer.bytes()` -> `view_le` -> `slice<T>.load` and writable `store` type-check and reach vector LLVM loads/stores without a conversion loop | explicit vec2/4/8/16 load/store owner for f32 plus representative i32 |
| checked HIR | receiver type, element, order and exact Option/result equations are re-derived; every field mutation rejects before MIR | one valid record plus field mutation table |
| MIR validation | operand/result type, closed element set and descriptor relationship are checked; malformed values fail without panic or LLVM publication | malformed-MIR mutation table |
| enum/pass closure | every HIR and MIR walker classifies both operations for purity, region, escape, move, effects, source dependency, replay and printing | variant tripwire plus relevant owner assertions |
| whole/per-unit | local and interface-carried bodies preserve operation identity, provenance, diagnostics and emitted shape; unused imports emit nothing | paired whole/per-unit owner and interface codec golden |
| existing behavior | byte reads/writes, bytes.as_str, buffer.bytes, slice range/index and resource.view_from_raw retain semantics and diagnostics | focused existing owners plus one composition fixture |
| failure hygiene | rejected source or malformed checked IR publishes no object/cache entry; inspection does not mutate output | existing publication gate with focused negative fixture |

One parameterized owner may close multiple type and producer rows. The matrix
requires discriminating failures, not one fixture per spelling.

Implementation evidence is concentrated in
`runway_a2_binary_codec::{checked_typed_byte_view_inference_and_domain_diagnostics,
checked_typed_byte_view_runtime_predicates_cover_the_closed_domain,
checked_typed_byte_views_are_descriptor_only_in_llvm,
checked_typed_byte_views_reach_vector_loads_and_stores,
checked_typed_byte_view_round_trip_and_failures}`. Checked-HIR field mutations
are owned by `checked_byte_views_hir_rejects_forged_type_equations_in_every_entrypoint`;
MIR mutations by `checked_byte_view_mir_relations_fail_closed_before_publication`;
target refusal by `checked_byte_view_target_admission_precedes_backend_lowering`.
The exhaustive HIR storage-generation and MIR fact inventories remain the
compile-time pass-closure owners.

## 3. Validation and diagnostic order

Source checking is deterministic:

1. check and resolve the receiver once;
2. reject a receiver outside the method's exact slice domain;
3. reject nonzero argument count;
4. for view_le, resolve the complete expected result and infer T;
5. reject T outside the closed set;
6. record the little-endian requirement on MIR; driver target admission checks
   it after MIR validation and before cache access or LLVM construction.

At runtime, view_le computes the nonnegative-length, whole-element and pointer-
alignment predicates without forming a typed pointer or reading memory. All
invalid combinations produce the same None, so their internal conjunction order
is unobservable. The Some descriptor is formed only from a valid conjunction.

## 4. PR boundary

The design and mirror updates form one documentation PR. Implementation is one
capability because method inference, provenance, checked HIR/MIR, interface
serialization and LLVM descriptor formation are one strict producer-to-consumer
chain. Splitting the operations would duplicate provenance and interface proof;
shipping only view_le would omit the inverse needed to pass typed storage back
to byte APIs. The implementation is expected to exceed 1,000 hand-written lines
because the new operation crosses every HIR/MIR analysis sweep and the matrix
requires both ownership directions. One capability avoids two dormant variants
and duplicated whole/per-unit proof.

Issue 1072 remains separate. Its scalar builder fast path is already shipped;
checked byte views neither depend on nor alter that runtime boundary.
