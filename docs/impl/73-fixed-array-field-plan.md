# Fixed-array types and inline record fields

Status: plan of record for
[issue 1065](https://github.com/sanohiro/align/issues/1065). The language
contract is complete; implementation starts only after one fresh independent
adversarial review of this ledger. Evidence baseline: Align `e4395337`, LLVM
22.1.8. The requesting consumer baseline is align-llm Request 94.

This capability makes the already implemented fixed array a nameable type and
therefore permits it as an inline record field. Syntax, formation, stable-place
projection, checked HIR, MIR, LLVM layout, interface transport and ownership
must land together: publishing only the type spelling would create values that
later stages cannot safely access, while a field-only compiler special case
would add a second fixed-array model. The implementation is one capability PR.
It may exceed roughly 1,000 changed hand-written lines because the one new AST
type variant must be exhaustive in formatting, dependency discovery, generic
substitution and replay, and because field construction/projection must close
over every existing fixed-array owner. Splitting that chain would duplicate the
same type and place proof and leave no independently useful intermediate state.

## 1. Public-contract ledger

```text
Surface           [T; N]

Syntax            A fixed-array type starts with `[`, contains one element type,
                  the explicit-semicolon token `;`, one unsuffixed decimal
                  integer literal, and `]`, all on one logical line. Horizontal
                  whitespace is insignificant. The lexer separates a written
                  semicolon as `Semicolon` while newline remains `End`; ordinary
                  statement termination accepts either token, but this type
                  separator accepts only `Semicolon`. Thus `[T\nN]` is invalid
                  and existing `statement;` source retains its meaning. The type
                  is accepted anywhere an ordinary type
                  annotation is accepted, including record fields, locals,
                  parameters, results, generic arguments and function-value
                  signatures. This makes the type syntactically available; the
                  existing placement gates still reject it where aggregate
                  values are unsupported, including native extern signatures.
                  N is not an expression, named constant, signed literal,
                  inferred value or generic const parameter.

Exact type        N is in 0..=u32::MAX and is part of the structural type.
                  T uses the existing fixed-array element domain: a supported
                  scalar/view/function value, or a declared record accepted by
                  fixed-array literal formation after generic substitution.
                  Nested fixed arrays and independently owned scalar elements
                  such as string, dynamic array, resource or native handle stay
                  rejected by that one existing element classifier. A Move
                  record element remains admitted only through the existing
                  in-place construction and recursive element-Drop rules.

Construction      An array literal checked against [T; N] must contain exactly
                  N elements. Each element is checked against T in source order.
                  `[]` constructs every [T; 0]. An unannotated literal retains
                  its current inferred fixed-array type. Record fields are
                  initialized in declaration/source order under the existing
                  fresh-aggregate destination and partial-cleanup contract.

Layout            [T; N] is N consecutive inline T slots with the existing
                  target ABI size, alignment and stride for T. A record field
                  contributes that inline array directly to the record layout;
                  it is not a pointer, slice header, builder or dynamic array.
                  N=0 occupies the target's ordinary zero-length-array size and
                  retains T's semantic type without allocating storage.

Access            `.len()` is the compile-time N. `place[i]`, `place[a..b]`,
                  array-to-slice coercion, pipelines and `place[i] = value` for
                  a mutable root use the existing fixed-array operations and
                  bounds behavior.
                  A nameable fixed array may be rooted in a named local,
                  parameter, or recursively selected record/tuple field place;
                  receiver and index/bounds evaluate once in source order. An
                  arbitrary temporary fixed-array expression remains rejected:
                  bind it first, so a slice or indexed place always has stable
                  storage. A field-rooted slice borrows the containing place and
                  may not outlive or cross invalidation of that owner.

Ownership         [T; N] is Copy exactly when T is Copy. Copying or passing one
                  by value copies the complete inline value; no copy is hidden
                  behind a header. If T is an admitted Move record, the array is
                  Move, each initialized element is dropped exactly once in
                  ascending index order, and moves null the complete source
                  state. A Move array field cannot be extracted by value from a
                  larger record under the existing nested-Move-field rule;
                  moving the containing record remains supported. Replacement
                  and early exit use the existing drop-old, source-nulling and
                  partial-initialization ordering.

Allocation        Type formation, construction, copy, indexing, slicing,
                  parameter/result transport and Drop add no heap or arena
                  allocation and call no array-builder/runtime allocation ABI.
                  A later explicit materializer such as `.to_array()` retains
                  its existing allocation contract.

Errors            Malformed syntax is a parser error. An invalid or overflowing
                  N is a type-formation error. Unsupported T is diagnosed by the
                  existing fixed-array element classifier. Literal cardinality
                  mismatch is reported before checking any out-of-range surplus
                  element; for equal cardinality, element errors are reported in
                  source order. Recursive/over-aligned record-layout rejection,
                  borrow escape, mutation authority, move, unsupported-placement
                  and ABI-size errors retain their existing owners and
                  precedence. Native extern and raw ABI boundaries gain no
                  fixed-array parameter/result form.

Effects           Formation and fixed-array operations are Pure except for the
                  effects of element expressions or an invoked pipeline body.
                  There is no ambient configuration, reflection or runtime
                  length observation.

Owner             align_lexer/parser/ast own `[T; N]`; align_sema owns element,
                  cardinality, ownership, stable-place and lifetime validation;
                  checked-HIR replay proves the same type/place equations;
                  align_mir owns construction, projection, copy/move and cleanup;
                  align_codegen_llvm performs target layout and pure lowering.

Artifact/cache    The AST and checked type carry T and u32 N. Public signatures,
                  generic source bodies, nominal definitions and dependency
                  fingerprints include the complete reachable element graph and
                  N. Interface format 14 adds `IType::FixedArray { element,
                  length:u32 }` and replaces format 13 outright; there is no
                  compatibility reader. Its canonical type-record tag is u8
                  `3`, followed by the recursively encoded element `IType`, then
                  the length as little-endian u32. The reader bounds the complete
                  IType graph to 128 nested records, rejects an unknown tag,
                  truncation, trailing bytes or a 129th record before publishing
                  a summary, and semantic import validation rejects an element
                  outside the fixed-array domain before HIR/codegen. The exact
                  standalone record for `[i64; 32]` is:
                    03 00 03 00 00 00 69 36 34 00 00 00 00 20 00 00 00
                  (`FixedArray`, `Named`, byte string `i64`, zero type arguments,
                  length 32). Independent semantic-to-byte and byte-to-semantic
                  goldens own this vector; tag/truncation/trailing/depth and
                  invalid-element mutations own rejection. Resolved HIR
                  continues to use the existing Ty::Array/StructArray records.
                  Object/cache identity already includes source, compiler,
                  interface, target and profile identity.

Prerequisite      Existing fixed-array literals, Ty::Array/StructArray, recursive
                  Move-record array Drop, aggregate destination construction,
                  target layout validation and fixed-array-to-slice lowering.
                  No runtime/package/milestone prerequisite is added.

Acceptance        Section 3. Provider tests own syntax, type identity, inline
                  layout, zero allocation, stable field access, ownership and
                  whole/per-unit parity. align-llm adoption and its decode-step
                  measurement remain consumer-owned and do not block Align.

Benchmark         Compile and run a record containing thirteen [i64; 32] fields;
                  inspect optimized LLVM/native calls and allocation counters to
                  prove zero builder pushes and zero heap allocations. Record
                  frame size and construction time against the existing dynamic
                  builder form. This is evidence for the issue, not a general
                  latency or stack-size promise.

Mirrors           draft.md, docs/language-spec.md, docs/design-notes.md,
                  docs/history.md, docs/open-questions.md, docs/impl/03-types.md,
                  docs/impl/05-backend-llvm.md, docs/impl/07-roadmap.md,
                  docs/impl/19-hir-validation-ledger.md, core array design and
                  HANDOFF.md.
```

The spelling is `[T; N]`, not `array<T>[N]`: the latter makes the owned dynamic
container look like the base type and a static length like a postfix modifier.
The bracket spelling matches the literal's inline nature and keeps `array<T>`
singularly heap-owned. The semicolon separates a type from a cardinality and is
not a statement terminator.

## 2. Implementation closure matrix

The implementation must update this matrix before code review. One
parameterized owner may close several cells when it would fail for each defect.

| Cell | Required behavior | Owner evidence |
|---|---|---|
| Lex/parse/format | split written `Semicolon` from newline `End`; statement termination accepts both while `[T; N]` requires `Semicolon`; nested type parsing and round-trip formatting preserve T and N; newline separator and missing/negative/non-decimal/overflowing lengths reject deterministically | lexer/parser/formatter statement-compatibility, positive and recovery owners |
| Type formation | every annotation position resolves to the existing exact Array/StructArray representation; unsupported/nested/owning elements and bad generic substitutions reject through one classifier | sema table over primitive, str, function, Copy/Move record, generic and excluded T |
| Construction | exact, short, long and zero literal cardinalities; declaration-order record construction; reached partial initialization cleans only live Move elements | driver runtime owners plus malformed/reached-exit owners |
| Move in/out | Copy whole values copy all slots; Move-record arrays move/null/drop once; nested Move-array field extraction stays rejected; whole containing-record move works | move-check and recursive-Drop counters |
| Field projection | read/index/range/slice/len/pipeline and nested field roots use the inline slot, evaluate once and preserve bounds/UTF-8-independent semantics | sema/HIR/MIR/driver field-place matrix |
| Mutation/replacement | mutable field elements use existing authority and escape checks; whole field/record replacement preserves RHS, bounds, drop-old and source-nulling order | indexed-store, replacement and alias owners |
| Control flow | fixed-array fields survive if/match/else/?/map_err joins, loop-carried values, break, early return and divergence with correct partial cleanup | parameterized control-path owner |
| Lifetime | str/view-bearing admitted arrays and field-rooted slices retain the exact containing storage roots across copies, joins, calls and invalidation | EscapeCheck generation/root owners and negative escapes |
| Generic/interface | generic record/function monomorphization substitutes T once; public whole/per-unit signatures and nominal definitions preserve T/N and dependency identity; tag-3 element-then-u32-LE encoding, depth bound and malformed rejection are exact | independent encode/decode goldens, corruption/depth mutations, interface round trip, cache-difference and whole/per-unit twins |
| ABI/layout | semantic and LLVM size/alignment/offset/stride agree for N=0/1/32/37, mixed surrounding fields, nested records and supported over-aligned array elements; malformed checked types reject before LLVM | layout validators, raw/optimized LLVM assertions and platform CI |
| Allocation | construction/access/return/Drop of the 13x32 acceptance record has no builder/runtime allocator call; explicit materialization remains the only allocation | IR symbol scan plus runtime allocation counter |
| Malformed input | forged type/field/cardinality/place/layout equations fail in checked-HIR or MIR validation, never by panic or backend guess | one mutation owner per producer-owned discriminator/equation |

## 3. Acceptance corpus

The provider corpus must include:

1. A `NodeTable` with thirteen `[i64; 32]` fields, exact literal construction,
   parameter/result transport, runtime-index reads, mutation through a mutable
   field place, slicing, `.len()` and a pipeline reduction. Values and bounds
   behavior must match standalone fixed arrays.
2. Zero- and 37-element fields, nested ordinary records, a generic
   `Holder<T>` instantiated with valid and invalid T, Copy and Move-record
   element arrays, and whole/per-unit compilation twins.
3. Parser/formatter and type errors for every syntax/length/cardinality and
   excluded-element class in the ledger, plus stable-place and lifetime
   negatives.
4. Semantic/LLVM layout twins and a runtime offset/stride probe on every
   supported CI target. N and the complete element graph must change interface
   and object identity.
5. Optimized-IR and runtime allocation evidence that constructing and reading
   the thirteen-field table calls neither `align_rt_array_builder_push` nor any
   heap allocation/free path.

The external acceptance step rewrites `mf_decode_layer_node_table` and
`mm_table`, runs `scripts/run-decode-step` and
`scripts/run-gpu-session-reuse-smoke`, and confirms zero dynamic table arrays.
That work belongs to align-llm and remains explicitly pending after Align
merges.

## 4. Deliberate exclusions

This capability adds no inferred cardinality variable, const generic, length
expression, array repetition syntax, nested fixed-array element, dynamic-length
field, hidden boxing, C flexible-array member, native extern/raw array ABI,
equality/hash/print behavior, reflection, data serialization widening or new
runtime ABI. An arbitrary temporary is still not stable fixed-array storage;
bind it before indexing or slicing.

## 5. Design-review finding closure

The fresh full-diff review of candidate `b8e23187` found three P2 contract gaps.
They are closed together before implementation; no public surface or strategy
changed.

| Finding | Root cause | Closure |
|---|---|---|
| Explicit `;` and newline shared `TokKind::End` | the ledger named a glyph but not its lexer provenance or existing statement role | specify `Semicolon` versus newline `End`, dual statement termination, fixed-type-only separator use, newline negative and compatibility owners |
| Format 14 named a Rust record but not canonical bytes | artifact identity lacked a complete producer/consumer byte contract | fix tag 3, element/length order, u32 little endian, 128-record depth, malformed rules and independent bidirectional `[i64; 32]` golden |
| Japanese receiver prose contradicted the English contract | only the newly inserted mirror paragraph was compared | update the later receiver paragraph to stable local/parameter/field places plus arbitrary-temporary exclusion and re-scan both mirrors for the old restriction |
