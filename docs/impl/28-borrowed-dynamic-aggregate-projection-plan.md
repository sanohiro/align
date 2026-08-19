# Borrow-safe dynamic aggregate projection

Status: **DESIGN SETTLED; implementation pending; required by align-llm Request 17.**

This document extends the borrowed sum-payload projection shipped by
`26-borrowed-sum-projection-plan.md`. The shipped capability intentionally rejects every dynamic
array reachable from a projected payload and accepts shared-borrow call arguments only when they
are named locals or struct fields. The decoded C6 verifier needs both missing pieces: optional
records contain dynamic arrays, and dynamic arrays contain Move records whose nested options must
be inspected without extracting an element owner.

This is one compiler capability, not an application exception. It preserves the existing
`borrow` model, tagged layouts, dynamic-array layouts, Drop plans, and function ABI. The complete
producer-to-consumer path spans semantic classification, move and escape analysis, checked HIR,
MIR, LLVM lowering, generic rechecking, interfaces, and owner tests. Splitting the implementation
between array-bearing sum payloads and indexed shared borrows would leave the first half without a
useful stable consumer and duplicate the same alias, lifetime, and no-cleanup proof. The resulting
implementation may exceed roughly 1,000 changed hand-written lines; the atomic boundary has lower
integration risk because neither dormant half is independently useful or safe to expose.

## 1. Public-contract ledger

| Surface | Exact input and defaults | Result, errors, and validation order | Ownership, lifetime, allocation, and cleanup | Compiler/interface owner | Identity, prerequisite, and acceptance |
| --- | --- | --- | --- | --- | --- |
| Dynamic-array-bearing borrowed sum payload | Existing exhaustive `match place { Variant(binding) => arm, ... }`. `place` satisfies the exact stable borrowed-place rule in plan 26. The payload satisfies `BorrowedDynamicPayload` below. No new syntax, option, type argument, environment input, or default. | The existing tag is read in place. Sema first resolves the complete sum and pattern, then verifies the exact borrowed place, then classifies every bound Move payload in variant/payload order. The first unsupported reachable type keeps the existing borrowed-place diagnostic. Exhaustiveness and ordinary pattern diagnostics retain their current precedence. | The active payload and every reachable dynamic-array header and element remain owned by the original sum. The binding has no independent storage, cleanup bit, source nulling, allocation, mutation, or `Drop`. Read-only tag, length, Copy/view element, string-as-`str`, record-field, and shared-borrow operations are permitted. A whole array, owning element, or enclosing Move payload cannot be moved, replaced, stored, returned, captured, sent, or retained. Derived views carry the original owner generation and underlying region roots. | `align_sema` extends the one cycle-safe payload classifier and records the existing projection metadata. Checked-HIR replay independently recomputes the expanded grammar and cleanup exclusions. MIR retains a pointer/path operand; LLVM reads the array header and element storage through that pointer. No runtime helper, native ABI field, or interface-body field is added. | The extension consumes plan 26 and the existing dynamic-array/AoS representation. Structural reachable types remain part of MIR/cache identity; generic bodies are rechecked from transported source, while non-generic interfaces retain signatures only. Direct and nested `Option`/`Result`/user-sum owners, malformed metadata owners, and the real align-llm result/evidence graph are acceptance evidence. |
| Indexed shared borrow of an owning dynamic-array element | Existing call syntax `inspect(values[index])`, where the selected parameter mode is exactly `borrow`. `values` is a stable named local, borrowed parameter, projected borrowed binding, or struct-field path of one of those, and has an admitted ordinary dynamic scalar-array or AoS record-array type. `index` has the existing array-index type and semantics. No implicit `borrow`, temporary array materialization, or `borrow mut` element form is added. | Static checking resolves the call target and mode, checks the base place and concrete element type, then checks the index expression in existing source order. A non-array, unsupported array class, non-stable base, type mismatch, by-value target, or `borrow mut` target is rejected before MIR. At runtime the base and index are each evaluated once, the existing bounds check occurs before pointer formation and before the call, and an invalid index keeps the existing hard bounds error. | The callee receives a non-null pointer to the existing element for the dynamic extent of the call. The caller and array remain owners; there is no element load, transfer, nulling, cleanup bit, temporary owner, allocation, or independent `Drop`. A view returned through the callee's existing return-borrow summary is rooted in the source array generation and any contained region roots; moving, dropping, or validly replacing the array invalidates that view under existing rules. The element cannot be mutably borrowed, replaced, or retained by this surface. | `align_sema` admits and records the indexed shared-borrow argument and derives exact storage roots. Checked HIR recomputes base stability, mode, type, and index. MIR carries one checked dynamic element place with the base borrowed place, evaluated index operand, and exact element type. LLVM performs bounds checking, computes the element pointer, and passes it through the existing borrowed parameter ABI. | The call changes only implementation/body identity. Function signatures, interface format, runtime ABI, and persisted formats are unchanged. Direct/local/field/projected bases, scalar-string and Move-record arrays, returned views, source invalidation, malformed MIR, generic/imported whole/per-unit parity, and cache edit/revert owners are required. |

There is no native or wire text boundary, persisted scalar/tag encoding, runtime inspection table,
connection-global state, process-global state, or benchmark promise in this capability. Those
ledger dimensions are N/A. All source text and diagnostics retain Align's existing UTF-8 source
rules; no value text crosses a new FFI boundary.

## 2. Admitted grammar

`BorrowedDynamicPayload` is plan 26's `BorrowedSumPayload` with exactly these additions:

```text
BorrowedDynamicPayload :=
  CopyScalarOrView
  | string
  | Struct(BorrowedDynamicPayload fields)
  | Option(BorrowedDynamicPayload)
  | Result(BorrowedDynamicPayload, BorrowedDynamicPayload)
  | UserSum(BorrowedDynamicPayload payloads)
  | DynamicScalarArray(BorrowedDynamicPayload scalar element)
  | DynamicRecordArray(AdmittedRecord element, AoS)
```

The traversal is finite, cycle-safe, concrete after monomorphization, and fail-closed on missing
type-table entries. `DynamicScalarArray` is the ordinary `array<T>` representation carried by
`Ty::DynArray`; its element must already be a representable scalar and satisfy the same payload
grammar. `DynamicRecordArray` is the ordinary AoS `array<Record>` representation carried by
`Ty::DynStructArray(_, Layout::Aos)`; the element record and every reachable field satisfy the
same grammar. This admits the C6 graph's arrays of owned strings, arrays of records, and records
that themselves contain optional records and further ordinary dynamic arrays.

Fixed arrays, dynamic arrays of slices, opaque response arrays, dynamic vector/mask/fixed-aggregate
arrays, SoA, buffers, builders, boxes, resources, resource references, raw owning handles, and every
other specialized collection or opaque Move shape remain excluded. `array<Option<T>>` and
`array<array<T>>` remain outside the language's current dynamic-array element representation rather
than becoming representable through this projection. A future new dynamic-array representation is
not admitted automatically; the closed classifier and its variant sweep must be extended together.

The indexed shared-borrow surface uses the same two ordinary dynamic array classes and concrete
element grammar. Copy element indexing keeps its existing by-value behavior. A Move element becomes
addressable only as the argument to an explicit shared-`borrow` parameter. Ordinary `values[index]`
in a value position still rejects rather than copying or moving the element, and `borrow mut` of an
element remains deferred because it would require element-local replacement and cleanup state.

## 3. Semantic and lifetime contract

### 3.1 Array-bearing match projections

The borrowed match mode, exact-root predicate, binding metadata, nested borrowed-match exclusion,
and owning-mode parity are unchanged from plan 26. Admitting an array in the reachable graph does
not make an array header an owner of its own. Reading `.len()` projects and loads only the length.
Reading a Copy/view element uses the existing checked index path. Reading a string element uses the
existing non-consuming string-to-`str` behavior. A Move record element is inspected through the
indexed shared-borrow call below; it is never loaded as a value merely to reach one field.

An array-bearing projection may be matched repeatedly, passed to a shared-borrow helper, and read
on branch, loop, and early-return paths. It cannot be the source of `else`, `?`, by-value call,
assignment, return, value-carrying join, task capture, or array pipeline operation that consumes a
Move element. The existing owning forms retain their current extraction, nulling, element Drop,
and partial-cleanup behavior.

### 3.2 Indexed shared-borrow place

The declaration side of the indexed-borrow example is:

```align
Record { value: i64 }
Container { values: array<Record> }
fn inspect(borrow value: Record) -> i64 = value.value
```

Positional call expressions then use the existing call syntax:

```align
fn inspect_two(
  borrow values: array<Record>,
  borrow container: Container,
  i: i64,
) -> i64 {
  first := inspect(values[i])
  second := inspect(container.values[i])
  return first + second
}
```

where `inspect`'s corresponding parameter is explicitly `borrow`. The base before `[i]` must resolve
to a stable local or struct-field path, including a field rooted in a plan-26/plan-28 projection.
Fresh array constructors, function/control results, pipeline results, nested indexed results, and
arbitrary temporaries are rejected. A field below the indexed element is not a separate borrow
argument in this capability; pass the complete element to a helper and inspect its fields through
that helper's ordinary borrowed parameter.

The index expression is evaluated exactly once after all earlier call arguments and before every
later argument, preserving existing left-to-right call evaluation. The existing signed bounds
check precedes pointer formation and the callee. A rejected static form creates no HIR/MIR element
place. A runtime bounds failure invokes no callee and creates no owner or cleanup action.

The element pointer is valid only for the call. Existing return-borrow summaries may return a view
of an element's string/slice field; caller-side root substitution maps the callee parameter to the
array place and its owner generation rather than to the numeric index or a temporary header. The
view cannot outlive an arena-backed array or its borrowed input, and moving/dropping/replacing the
array invalidates it. Different indices are not inferred disjoint: any simultaneous `borrow mut`
peer of the same array conflicts conservatively, and element `borrow mut` itself is rejected.

## 4. Checked representation and lowering

The existing `BorrowedProjection` record remains authoritative for a tagged payload. Its
`static_ty`, ordered path, exact owner fact, and projection-only cleanup exclusions now validate
against `BorrowedDynamicPayload`. No array index is stored in that record.

An indexed call argument adds one checked MIR-level place equivalent to:

```text
BorrowedElementPlace {
  base: BorrowedPlace,
  index: Operand,
  element_ty: Ty,
}
```

The exact Rust representation may differ, but it must preserve those three fields and may exist
only at an explicit shared-borrow call position. `base` retains the root slot and ordered static
struct/sum path; `index` is the once-evaluated checked integer operand; `element_ty` is recomputed
from the concrete dynamic-array class. MIR validation rejects a mismatched base type, unsupported
array class, wrong element type, missing slot/path, use outside a shared-borrow call, cleanup bit,
or malformed index operand before LLVM construction.

LLVM loads the base `{ptr,len}` parts through the borrowed place, checks the index with the existing
array bounds rule, computes `ptr + index * stride` using the canonical scalar or AoS record layout,
and passes that element pointer to the unchanged borrowed parameter ABI. It does not load the whole
array header as an owning value, extract a Move element, create an alloca for the element, copy
element bytes, emit a cleanup flag, or call a runtime helper. Scalar/record alignment and stride
come from the same producer-owned type/layout tables as ordinary indexing.

Generic source bodies are reparsed and rechecked after concrete substitution. Non-generic
interfaces transport only the unchanged signature and structural type graph; no indexed body place
is serialized. MIR fingerprints include the new place and exact structural array/element types.
Whole-program and per-unit compilation must reconstruct identical acceptance and executable
behavior. No interface-format or runtime-ABI version changes.

## 5. Implementation closure matrix

| Axis | Required closure | Exact owner evidence |
| --- | --- | --- |
| Type formation and classification | Extend the one cycle-safe borrowed-payload classifier through only `DynArray` and AoS `DynStructArray`; recurse through concrete scalar/record elements; preserve all exclusions and malformed-table failure. | `align_sema` wildcard-free type/classifier matrix; direct/nested/imported/generic accepted graphs; each excluded dynamic/fixed/specialized collection twin. |
| Match construction and binding | Produce existing borrowed metadata for direct `Option<array<i64>>`, `Option<array<string>>`, `Option<array<MoveRecord>>`, `Result`, user sum, and an array-bearing record payload; bind no independent owner. | sema metadata owners and driver direct/field/nested-field/`borrow mut` positives; owning twins retain transfer. |
| Indexed call formation | Admit only explicit shared-borrow parameters and stable local/field/projected dynamic-array bases; evaluate the index once in argument order; reject by-value, `borrow mut`, temporary, nested-index, and unsupported element forms. | sema diagnostic/precedence table; side-effecting index and multi-argument order runtime owner; bounds-failure no-call owner. |
| Move-in, move-out, source nulling, replacement, and return | Projection and indexed borrow perform none. Whole projected payload/array/element consumption, storage, return, capture, and mutable element replacement reject. Owning matches and ordinary whole-array moves remain unchanged. | MoveCheck parameterized negative matrix; repeated source use; owning parity; source replacement invalidates returned views. |
| Borrow roots and escape | Indexed element calls substitute the complete array storage roots and generation; contained view roots remain exact through fields, returns, loops, and early exits. | returned string/slice view positive from caller-owned input; arena/local escape negatives; move/drop/borrow-mut peer invalidation; no index-as-root artifact. |
| Control flow | Match and indexed calls compose with `if`, nested ordinary blocks, loop joins, divergent arms, early return, and malformed input without manufacturing moves or fallthrough. `else`, `?`, and nested match over a projected binding retain their existing exclusions. | direct control-family owner matrix plus unchanged plan-26 negatives and owning control owners. |
| MIR and checked HIR | Recompute exact payload eligibility and cleanup exclusions; validate every borrowed element base/index/type/mode; include the place in exhaustive traversal, printing, fingerprinting, and variant tripwires. | forged payload/static type/path/cleanup records; malformed element slot/path/class/index/type/mode; print/fingerprint goldens and validator rejection. |
| LLVM and layout | Address tags, headers, elements, and record fields in place with canonical type tables; no owner aggregate load, element copy, source null, cleanup, allocation, or runtime call. | raw MIR/LLVM assertions for scalar/string/Move-record arrays, nested field base, alignment/stride, and malformed lowering errors rather than panic. |
| Generic, interface, and cache | Concrete substitution reruns both predicates; whole/per-unit results agree; reachable element-graph edits invalidate exact dependents while unrelated edits remain hits. | generic and imported driver owners; interface hash and isolated cold/hit/edit/revert cache owner. |
| Real consumer | The exact `PromptEvaluationResult`/`PromptEvaluationEvidence` graph matches array-bearing options, passes Move task/row/trace/aggregate/reason/expected-input elements to borrowed helpers, and leaves both decoded roots usable. | align-llm `c6-borrowed-array-adoption`, complete C6c2 owner matrix, whole/per-unit check, and final `make ci`. |

One parameterized owner may close multiple rows when it fails for every listed defect. No benchmark
is required: this capability makes no performance/resource promise. Pointer/no-copy and allocation
absence are correctness assertions.

## 6. PR boundaries and gates

The design lands first. Its independent adversarial review must resolve the public grammar,
indexed-place lifetime, validation order, checked representation, and capability boundary before
implementation begins. The implementation is one capability PR because payload admission without
element inspection leaves the C6 graph unusable, while indexed Move-element borrowing without the
array-bearing payload grammar cannot reach that consumer.

Before implementation review, the author performs one matrix-to-diff pass. Every applicable row
above points to production code and a regression owner or is explicitly shown N/A. Rust changes
under `crates/` use the repository's compiler self-review, one fresh full-diff review, the narrow
owner target, and the exact final-SHA preflight. The real-client adoption runs only after the Align
implementation merge is pinned.

## 7. Non-goals

- No new syntax, reference type, lifetime annotation, trait, macro, implicit clone, or implicit
  borrow.
- No by-value Move-element indexing, partial element move, element replacement, element
  `borrow mut`, disjoint-index proof, mutable iterator, or Move-element pipeline callback.
- No fixed-array, SoA, array-of-slice, response-array, vector/mask/fixed-aggregate-array, resource,
  box, builder, buffer, or opaque-handle projection.
- No nested borrowed `match` over an outer arm binding; pass a complete indexed element to an
  explicit borrowed helper when its own options need inspection.
- No runtime helper, allocation, JSON exception, application sentinel, alternate verifier
  signature, interface-format field, or native ABI change.

## 8. Author-side consistency pass

The author-side ledger-to-prose pass is complete:

- `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, `docs/open-questions.md`, plans 26
  and 28, MIR documentation, and the L2 prerequisite table use the same two ordinary dynamic-array
  classes, shared-only indexed element rule, lifetime roots, and closed exclusions;
- every public input, result, error/precedence rule, ownership/lifetime/allocation rule,
  compiler/interface/cache owner, prerequisite, and acceptance owner appears in the ledger and
  closure matrix;
- every static multi-invalid form fails in type/call/base/index order, while runtime index
  evaluation and bounds behavior are explicit and precede pointer formation or the callee;
- the three example call forms are declarations-independent positional calls and will be
  syntax-checked by the implementation owner;
- no native/wire text, persisted format, runtime inspection, global-state, benchmark, later mutable
  element, or new array representation promise is implied;
- Request 16 remains the shipped base contract; this extension changes its payload grammar only
  through the named plan-28 capability and does not rewrite its historical acceptance; and
- the prerequisite consumes no later evaluator/provider capability. The align-llm verifier remains
  paused until the merged implementation is pinned and its exact real-record adoption owner passes.
