# Fixed scalar-array fields in heap records

## Classification and useful boundary

Voice V006 retains a tensor's owned name and up to three inline dimensions in
one runtime-sized descriptor array. `src/assets.align` expands the dimensions
into `first`, `second`, and `third`, then reconstructs the inline array for
upload. The exact `docs/reproducers/owned-record-fixed-array.align` witness
rejects at heap-builder construction on main `285f402e`. The consumer remains
external; this capability supplies its requested representation, not adoption.

Plan17 §7.6 explicitly excluded fixed arrays in August's closed heap-record
grammar. That historical exclusion is real and is not an implemented promise.
Its rationale in design-notes is view-free relocatable bytes and complete
compile-time cleanup, with unsupported child representations left out. Plan73
subsequently made fixed arrays nameable inline fields with one existing
Copy/Move, layout and lifetime model. An inline fixed array of Copy primitive
scalars has no view, owner, self-relative address, additional allocation, or
cleanup obligation. It preserves the heap builder's original safety reason.

The proposed classification is therefore Category A composition work under
Plan23, as with the independently reviewed lifecycle completion in Plans164/165:
complete and qualify an existing representation through a previously closed
producer. No locked ownership/allocation/optional/error model changes. This
does not claim five friction occurrences or treat the current validator alone
as evidence of policy. The independent design review accepted this classification against the explicit
old exclusion; its zero-size storage finding is closed by the contract below.

The complete useful boundary is fixed arrays of the same Copy integer, float,
bool and char leaves already accepted by the heap-record grammar. Fixed arrays
of records, owned elements, views, callables, handles and nested fixed arrays
retain their existing exclusions. Their different element/projection or owner
closure is not needed for V006. AM-R16's indexed-parent field access and AM-R17's
C-layout/raw storage remain separate consumers; neither follows from builder
admission. A built Move record is already observable through its complete Copy fixed-field value: `shape := values[i].shape`. This keeps the normal Copy semantics and stable local fixed-array operations; it needs neither a new borrowed payload nor AM-R16 direct indexed-field access. No new syntax, collection, intrinsic or runtime entrypoint is added.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Formation | Extend the recursive `HeapTreeRecord` field grammar with `[P; N]`, where P is `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `bool` or `char`, and N is the existing u32 literal length. Zero length retains P's semantic type and all exclusions. A field may occur inside an existing nested/optional/dynamic-array record; the surrounding record still satisfies every original layout, cycle, view and ownership restriction. Existing fixed-array formation and placement remain authoritative; this does not add a fixed-array Option payload or top-level heap fixed-array builder. |
| Existing operations | Under an annotated `array_builder<S>` binding, positional calls `array_builder()` and `array_builder(capacity)` retain the existing default and explicit `capacity: i64` behavior; `push(value: S)` takes the complete S; `build() -> array<S>` consumes the builder. A `borrow mut` helper may push but not consume the borrowed builder. By-value transfer/return remains supported. `append` stays Copy-scalar-only. |
| Layout and allocation | Each fixed field stores N inline P values at the existing element stride/alignment. The surrounding record uses its existing computed size, alignment and nominal identity. Push may grow only the existing outer byte buffer; fixed fields cause no child allocation. Relocation copies the complete inline bytes without retaining an address into the old buffer. Build transfers the buffer without allocating or copying its elements. No signature, header layout or runtime export changes; the zero-stride behavior below completes the existing byte-storage ABI. |
| Zero-sized storage | A record made entirely of zero-length primitive fields (including nested records) has exact stride zero. Heap constructors preserve zero, reject negative/unrepresentable strides by allocation-size abort before allocation, and validate explicit capacity normally. Stack headers allocate no heap storage; boxed headers retain their ordinary header allocation. No payload is allocated for any capacity or initialized count. Push and valid native append increment the initialized count without pointer arithmetic or copying source bytes; null-source no-op behavior is unchanged. Growth rejects a resulting count above `isize::MAX` before mutation in both heap and region modes. Heap build returns `{null, initialized_count}` and consumes only the header; abandonment and frozen-array Drop free no payload. Region zero-size build retains its existing arena sentinel. `len` and bounds checks use the initialized count, never capacity or byte size. |
| Zero-sized consumers | Existing builder Copy reads already bounds-check then return the unique zero-sized value. Frozen-array/slice element value reads and writes preserve evaluation and bounds while touching no nonexistent bytes. When an existing borrowed-element call needs an address, codegen supplies aligned zero-sized entry storage, with the original semantic borrow still tied to the original array; no source-visible identity or lifetime changes. Audit direct and pipeline reads/writes, nested fields, subslices, borrowed helpers, return and Drop. No zero-sized load/store may depend on dereferencing a null payload. |
| Ownership and inspection | A fixed P field is Copy, including N=0, and contributes no owned allocation. An owned sibling still makes the complete record Move. Existing push transfer, source nulling, replacement, return and initialized-prefix Drop remain singular. After build, ordinary stable-record/borrowed-record fixed-field operations and their bounds/lifetimes apply; no indexed-parent projection or whole Move-element extraction is added. Copy-only records also retain existing independent builder/array Copy reads. |
| Errors and order | Existing parser/import/type inference and fixed formation precede heap-record field admission in declaration order, builder receiver/move checks, exact push type, recursive individual-allocation proof and growth. View-bearing or Move fixed elements remain rejected even at N=0. An invalid source or incompatible heap/region mode cannot reach allocation. Capacity overflow/allocation failure retain the terminal-abort contract. |
| Interfaces and identity | Existing type records encode P and N inside the complete nominal definition graph. Concrete generic substitution precedes admission. Whole/per-unit checked HIR consume the same heap-record classifier; old codegen-cache entries are separated by compiler identity. Changes to P/N and exact revert follow the existing definition identity. No new persisted or exchanged record is introduced. |
| Other semantics | Constructor/push/build retain their existing Pure classification and exclusive mutable-builder rule. No encoding, process-global state, ambient option, text/native boundary or default changes. There is no latency/throughput promise; counted allocation parity checks the explicit no-child-allocation contract. |
| Owners and consistency | Sema owns the shared closed classifier; checked HIR consumes it. Existing MIR materialization/cleanup remains authoritative. LLVM owns zero-sized element access and runtime owns exact stride/count/storage. Update draft, language digest, design notes, the Settled heap-builder paragraph, Plan17 §7.6 and its scalar-field predecessor note, ledgers19/20, and `core-design/array-slice-pipeline.md` and its ja mirror where their grammar is stated. Plan230/HANDOFF and the voice register record delivery once. |

## Implementation closure matrix

| Axis | Implementation and acceptance owner |
| --- | --- |
| Formation and malformed metadata | Extend only the shared `heap_tree_record_error` field arm. Its parameterized predicate owner covers every primitive family, N=0/1/3/37, nested records/options/dynamic-record children, deterministic first-invalid paths, missing IDs and cycles. Retain fixed String/view/record/raw/callable negatives including zero length; global fixed formation still validates widths and cardinality. Checked-HIR constructor mutation owners test the admitted shape and excluded twins through checked lowering. |
| Construction, move-in/out and replacement | New `heap_fixed_scalar_fields::lifecycle_and_modes` covers literal/local/function/generic complete records, push/growth/build, by-value builder forwarding, borrowed push, complete record/array return and replacement. No field or indexed ownership extraction is introduced. Existing `m12_array_builder` transfer/nulling and `fixed_array_fields` replacement owners retain the generic source machinery; native values prove inline bytes survive relocation. |
| Source order and control | Exercise successful and terminating `if`, `match`, `else`, `?`, `map_err`, loop/break, early return and partial next-element construction with a new fixed field beside an allocated String. Earlier completed owners clean up once, uncommitted bound sources remain live, a failed operand never increments the initialized prefix, and later side effects do not run. Reuse the existing full control matrix where the same typed record fixture can carry the fixed field. |
| Drop and allocation | `heap_fixed_scalar_fields::allocation_and_partial_cleanup` closes normal and unfinished stack/boxed builders, reassignment, build transfer, nested owning-record arrays, empty fixed fields and post-return cleanup. Compare the fixed-field descriptor against an equivalent expanded-scalar descriptor with the same positive String/outer-buffer allocations; fixed fields add none and all owned allocations are freed. Existing runtime growth/layout tests remain applicable. |
| Zero-sized lifecycle | Extend `array_builder_read_tests::array_builder_reads_initialized_prefix` across heap boxed/stack and region, stride 0 and positive controls, capacities 0/1/64, growth, native append, bounds observations and freeze. Add allocation-free payload/abandonment assertions and bounded subprocess probes for count overflow and invalid constructor stride. Driver `heap_fixed_scalar_fields::lifecycle_and_modes` covers all-zero and nested-zero records, zero fields beside positive fields, Copy reads before/after freeze, array/slice/pipeline transport, borrowed helpers and replacement in both compilation modes. `zero_bounds_preserve_evaluation` checks zero-size before/after-freeze signed bounds and exactly-once index evaluation. `align_codegen_llvm::heap_fixed_zero_elements_use_addressable_storage` pins zero-stride reads/materialization writes without suppressing bounds or positive-stride loads, including outlined materialize, filter-count/scatter and reduction kernels. `zero_parallel_consumers` preserves callback values and cardinality for empty, short and pool-sized zero/positive input arrays in both compilation modes. The shared borrowed-pointer helper also preserves nonnull zero-sized storage, but the newly admitted all-zero heap records are Copy and therefore do not add indexed shared-call admission. |
| Borrowed access and mode | Whole/per-unit owners read the complete Copy fixed field from a stored Move record into an explicit local, then inspect/index/slice it. Stable standalone borrowed-record helpers remain supported; borrowed sum and whole indexed Move-record admission remain unchanged. Whole/per-unit owners inspect dimensions through the already-supported shared-record helper, verify bounds/slice behavior on stable fields, reject escaping local views and repeated whole moves, and preserve existing borrowed-payload and indexed-field restrictions. Inline field replacement keeps the local storage valid; it is not falsely classified as a dangling view. Copy fixed-field records may use heap or explicit-region builders; a borrowed builder helper must not assume heap mode. Owned String siblings keep the heap-only predicate and existing free-standing/mixed-mode checks. |
| Generic/interface/cache | New `heap_fixed_scalar_fields::imports_and_cache` builds imported generic/nongeneric records with fresh source maps, cold/hit replay, fixed-element definition change and exact revert; `lifecycle_and_modes` supplies whole/per-unit native parity. `primitive_layouts` checks every primitive leaf and length37 through heap relocation. Native output and hit/miss evidence must agree; generic invalid element substitution remains rejected. Existing interface graph/corruption owners cover unchanged encoding. |
| Neighbor restrictions | `heap_fixed_scalar_fields::rejections` pairs invalid source with a positive control: fixed view/Move/record fields, top-level heap fixed-array builder, append of records, explicit layout/alignment, builder capture/storage and new indexed-parent fixed-field spelling. Preserve established refusals rather than granting a writable/view authority with formation. Existing `builder_copy_reads` owns newly constructible Copy-record observations. |

The author-side pass must map every new grammar/owner promise to these tests and
the unchanged canonical paths before review. This is one useful formation and
lifecycle capability; a constructor-only PR would leave its byte relocation and
Drop proof unqualified. No benchmark or new runtime export is required.

## Author design pass

The new observable promise is only primitive fixed-field formation and its
composition through existing record transport. The shared source/checked-HIR
classifier is the sole admission owner. The existing fixed field layout and
Copy rules make the new field independent of owner/provenance tables; the
containing record's existing ownership remains authoritative. The matrix closes
the nontrivial consequences: outer relocation stride, partial construction,
heap/region borrowed helpers, imported identity, initialized-prefix cleanup and
fixed-field views after freeze. The baseline reproducer fails in both check modes.

Independent plan review found one P1: heap initialization coerced a zero ABI
stride to one, so aggregate push could read past a zero-sized source. The revised
ledger preserves stride zero, closes count and byte-operation behavior, and
qualifies the frozen-array consumers as well as the builder itself. The same
zero-width rule applies across the existing runtime entrypoints and LLVM access
paths; it is not a special case for the consumer or a particular record name.

## Boundary recheck after the first executable owner

The first fixture attempted `dimension(values[i])`, which the existing borrowed-payload
classifier excludes for fixed-field Move records. Widening that shared classifier would also
expose fixed fields under projection-only Option/Result/sum match bindings. Independent review
identified the missing tagged-storage path in `array_source_slot`; merely accepting the type
would address an uninitialized synthetic local. That broader strategy is withdrawn before any
shared-classifier change. The closure matrix was reopened around actual consumer observation.

The existing complete-field value operation `shape := values[i].shape` provides the required
stored dimensions using normal Copy semantics. Native whole/per-unit owners must prove this
read survives replacement and parent retirement. This is a useful stable V006 consumer, with
no dormant new producer waiting on a later PR. Shared indexed borrowing and shared-match
fixed-field projection retain their prior exclusions and are recorded with the subsequent
fixed-field access investigation; they require a complete tagged-place lowering design.

## Author matrix-to-diff closure

- Formation is the one primitive-fixed arm in `heap_tree_record_error`. The existing
  source-order/cycle owner enumerates scalar families and zero/nonzero excluded twins;
  `heap_record_array_builder_rows_match_the_producer` transports fixed fields through nested
  Options and owning child arrays and retains malformed constructor/push/build mutations.
- `lifecycle_and_modes`, `primitive_layouts` and `imports_and_cache` close source values,
  forwarding, generic/imported identity, relocation, heap/region borrowed helpers, and stable
  complete-field Copy observation after replacement and parent retirement.
- `allocation_and_partial_cleanup` compares positive allocation/free events against the
  scalar-expanded record and requires zero requested-live bytes after nested ownership,
  both header modes, abandonment, replacement and failed next-element construction. Existing
  `m12_array_builder` exit/source matrices now carry the fixed field through their control paths.
- Zero-width native storage uses exact stride, bounded count and no byte access. The extended
  runtime grid, failure probes, native bounds owner and LLVM address owner distinguish the
  zero-size path from ordinary sizes without broadening source stores or borrowed payloads.
- `rejections` and existing fixed-field/builder-Copy owners retain placement, view escape,
  Move transfer, append and indexed-field limits. The new public contract has no benchmark;
  allocation parity is a correctness owner, not a throughput claim.

## Review finding closure

The independent code review found that the outlined range kernel still loaded its
materialized zero-width input from the payload pointer. The shared input arm now
produces the unique zero-sized constant without an address or load, preserving
range iteration, callbacks, survivor counts and reduction. The IR owner covers
all four kernel modes with a positive-width control; native whole/per-unit owners
check empty, short and pool-sized arrays, accepted and rejected filters, materialized
values and fused reduction. This closes the existing zero-sized-consumer promise.

The same-class audit checked range output stores: `ParMapParallel` requires a
primitive scalar output and reduction requires an integer, so both have positive
size. Column-batch fields also exclude fixed arrays and records; their typed
loads cannot consume this new zero-width representation. Ordinary element and
pipeline accesses use the addressable-storage path qualified above.
