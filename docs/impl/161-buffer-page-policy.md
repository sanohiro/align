# Explicit owned-buffer page preference

Request141 supplies a Linux host-memory screening result, not a universal speed
claim. The owner selects an explicit per-buffer preference, with a portable API
and initially Linux-only optimization. Default construction remains unhinted.
Consumer managed pins, model/CUDA measurements and adoption remain external.

## Public contract ledger

Declarations (trailing defaults are existing builtin arities):

```text
buffer.page_policy { Default, PreferHuge }
buffer(capacity: i64, alignment: i64 = 1, pages: buffer.page_policy = Default) -> buffer
buffer.filled(length: i64, value: u8, alignment: i64 = 1, pages: buffer.page_policy = Default) -> buffer
buffer.try_new(capacity: i64, alignment: i64 = 1, pages: buffer.page_policy = Default) -> Result<buffer, Error>
buffer.try_filled(length: i64, value: u8, alignment: i64 = 1, pages: buffer.page_policy = Default) -> Result<buffer, Error>
```

Positional source example:

```align
fn cache(size: i64) -> Result<buffer, Error> =
    buffer.try_filled(size, 0, 64, buffer.page_policy.PreferHuge)
```

| Surface | Exact contract and owner |
| --- | --- |
| Policy type | Ordinary Copy, tag-only builtin enum `buffer.page_policy`, visible wherever the existing buffer constructors are visible; no import or type arguments. Declaration order is `Default = 0`, `PreferHuge = 1`. Qualified variants, ordinary parameters/results, match and imported interfaces use the same nominal enum. No bool/integer conversion or new ownership type. |
| Arguments | The policy is the final optional argument, after alignment; specifying it requires the alignment position. Evaluate every supplied argument once left-to-right. A terminating operand performs no constructor or later operand. Default omission produces the same checked enum value as explicit Default. |
| Validation and errors | Native order: for fallible calls reject null output, otherwise clear it; alignment, policy tag, then size/Layout admission; no allocation/advice precedes admission. Invalid policy terminates ordinary construction with `buffer page policy must be Default or PreferHuge`, or returns `Error.Invalid` for try constructors. All existing size, ordinary capacity degradation, terminal filled allocation and recoverable `Code(ENOMEM)` policies remain. Hint refusal is not allocation failure and adds no Result error. |
| Default | No additional page advice, no allocator change. OS/global-allocator decisions still apply: Default does not promise base pages or disable system THP. Vec transfers use Default. Empty buffers acquire no payload and preserve an aligned sentinel. |
| PreferHuge eligibility | On Linux, a fresh payload capacity of at least 2 MiB is eligible, for empty-window and zero/nonzero-filled constructors alike. This fixed internal threshold is an allocation-policy threshold, not an assumption about hardware page size. Base page size comes from the platform; unavailable/invalid page-size information disables this optimization. Other hosts and smaller capacities use the existing global allocator without advice. No environment variable, host-policy mutation or public size flag. |
| Allocation and initialization | An eligible payload is one private anonymous mapping, with checked span and alignment padding. Retain original mapping base/length separately from the published aligned payload; public capacity remains exactly the selected logical capacity. Fresh mapping bytes are initialized zero; nonzero fill initializes the requested prefix. Header remains one normal global allocation. A mapping refusal follows the existing allocation-failure policy, without a second acquisition/retry; advice refusal retains the same initialized owner. |
| Advice | Apply Linux MADV_HUGEPAGE once to complete base pages strictly inside the selected payload, excluding partial edges and alignment padding. Never force population, collapse, locking or global THP configuration. No huge-page, physical residency, latency, RSS, throughput or allocation-success promise. Byte correctness and ownership do not depend on advice success. |
| Growth and lifecycle | The stored preference and requested alignment follow the complete owner through move, return and replacement. Within-capacity operations do not reacquire or readvise. Growth across the threshold may move a global payload into one fresh mapping; mapped growth acquires one replacement mapping, copies only the initialized prefix, then releases the original. Acquisition failure retains the original bytes, pointer, capacity and provenance. Drop and failed header construction release the exact original mapping once. Unmapping retires its advice; no hinted pages return to the shared allocator. Future reuse/new Default buffers receive no inherited hint from this owner. |
| Release boundary | Exact valid mapping extents are released with munmap, without trimming partial extents during construction. An unexpected release failure is a terminal runtime invariant failure rather than a successful leaking cleanup; native owners discriminate this path in bounded children. No recoverable Drop error or implicit retry. |
| Representation | Buffer remains an opaque Move handle; runtime Buffer must still fit the existing 64-byte/16-alignment stack-header envelope. BufferStorage retains pointer/length/capacity, requested alignment plus preference, and mapping base/length. Global allocations retain their exact Layout and never use munmap; mappings never use realloc/dealloc. Existing views and generation invalidation are unchanged. |
| Compiler/native boundary | Add one required policy child to existing HIR BufferNew/BufferTryNew; all analysis/clone/replay paths visit it. Authenticate the exact builtin enum in checked HIR. MIR evaluates the enum and extracts its existing i32 tag before the constructor; native producer contracts require a founded i32 operand. LLVM only lowers those operands. Native new/filled/try_new/try_filled gain i32 pages immediately after alignment, before any output pointer. No extra symbol or compatibility ABI. Stack promotion requires proved Default in addition to its existing guards. |
| Persistence and prerequisites | Existing compiler/runtime content fingerprints invalidate affected artifacts. Generic interfaces preserve the ordinary builtin enum identity and source operands; register it in the builtin nominal inventory. No persisted format or milestone change; K1/plan61 remains deferred. |
| Documentation | draft.md, language digest, design notes, Settled, plans131/135, HIR ledger19, ABI ledger20, core string and array/slice English/ja, and the end-user buffer guide describe the same explicit policy and Linux-only initial optimization. Future hosts may implement the same optional hint after native qualification. |

The global allocator API proves ownership of bytes and Layout, not ownership of
the underlying VM mapping or retirement of its page flags. MADV_NORMAL does not
clear the huge-page flag. Private mappings are therefore selected only for the
explicit eligible preference, so cleanup has a complete page-policy owner.
This is a lifecycle choice, not a claim that mmap itself is faster. Measurements
must separate changed pointer placement from page policy.

The independent preimplementation review found no plan blocker. Its boundary
check confirms six machine words in Linux BufferStorage: pointer, initialized length,
capacity, alignment/preference word, mapping base and mapping length. PreferHuge
uses bit30 of the alignment word, disjoint from admitted alignments through
bit29; every Layout and alignment observation masks that bit. Buffer's outer
capacity/length words retain the 64-byte envelope with a compile-time assertion.
Literal tag-only policy values lower directly to i32 constants; other policy
expressions evaluate once and use the existing EnumTagEq plus i32 Select.
Qualification rejected a struct-field projection on an enum, so lowering uses
ordinary validated enum operations without widening any field-access contract.
Only proved constant Default permits stack promotion. Fallible native output
escape ordinals move with the added policy parameter (new:3, filled:4).

## Implementation closure matrix

| Axis | Implementation and discriminating owner |
| --- | --- |
| Type/argument formation | `buffer_page_policy` driver: all four constructors/default arities, qualified enum, variables/match/imported generic helpers, wrong primitive/foreign enum/arity/type arguments; every argument completion and ordered side effect. |
| HIR/MIR authentication | HIR mutation owner: wrong enum definition, policy type/child and default tag; MIR mutation owner: missing/ill-typed/unfounded policy, native signature and output equations in full/partition validation. Existing traversal tripwire and parameterized constructor owners close the child sweep. |
| Admission and failure | Native constructor probe crosses alignment/policy/count multi-invalid precedence, null output, payload/header refusal, denied advice and retained null output. Existing ordinary terminal/capacity-degrade and fallible owners remain authoritative. |
| Allocation provenance | BufferStorage native owner: Default/PreferHuge, small/threshold edges, zero/nonzero fills, explicit alignments up to admitted bound, nonempty payload alignment, exact map base/span and complete-page advice bounds, no extra acquisition, exact original release. Checked arithmetic near usize/isize limits is tested without huge speculative allocations. |
| Growth and reuse | Native owner: global-to-map threshold transition, mapped relocation, within-capacity address stability, failed growth, prefix bytes, zero append, clear/read-window reuse, Vec adoption; independent release/reallocation/advice event witnesses. Actual mapping retirement and newly mapped ordinary reuse have no old huge-page flag. Forced advice refusal leaves bytes/owner unchanged. |
| Ownership/control | Driver whole/per-unit execution: containing record fields, move/return/replacement, early Result exits, if/match/else/?/map_err/loops, preserved views and exact requested-live cleanup. Reuse `aligned_buffer`, `fallible_buffer`, `buffer_field_growth`, `buffer_self_append` for unchanged ownership cells. |
| ABI/optimization/cache | Native declarations/calls and exact runtime ABI inventories agree; default stack promotion remains and PreferHuge is not erased. Imported policy interface, cold/hit/edited-policy replay and generic templates agree. Every direct Rust/C caller is updated with explicit Default where intended. |
| Platform and performance | Linux native owners observe actual map retirement and optional advice/backing; other hosts prove portable accepted preference with no extra hint. A bounded local probe records bytes, acquisition/read-first/write-first/rewrite/release, wall/CPU/faults/RSS and actual AnonHugePages. Matched pointer-placement controls isolate advice, while original Default versus shipped PreferHuge measures the complete implementation. No requirement that advice succeeds on every host. Performance is local evidence, not a CI speed gate. |

The author first checks this ledger against existing alignment/fallible/stack
contracts, then obtains one independent adversarial plan review before changing
the ownership strategy. One capability PR keeps enum admission, complete operand
traversal, native ABI, allocation provenance and cleanup usable together. An
expected diff above 1,000 handwritten lines is justified by those inseparable
proof obligations and updating existing ABI call owners; dormant per-layer PRs
would multiply integration risk without a useful stable consumer.

Before final review, bind each row to concrete owners and inspect every must,
exact, every, before, reject and required obligation against the implementation.
One independent full-diff review follows the compiling owner-backed candidate;
local owner checks, bounded gate and Clippy precede publication. Update only
Align's answer in the external request register, leave it uncommitted, and run
`cargo build --release --workspace` at delivery. No versioned release is implied.

## Author-side closure

The ledger-to-diff pass binds type/default/evaluation rules to sema's constructor
checks, ordinary enum resolution, complete HIR child traversal and
buffer_page_policy_whole_per_unit_and_cache. That owner executes all four
constructors, every final-operand termination, ordered side effects, generic
records, containing-field growth, moves/replacement/early exits and exact final
requested-live cleanup. It checks whole/per-unit, serialized interfaces,
cold/hit caches and an edited policy. Source negatives include a consumed owner
inside the policy operand.

Checked-HIR buffer_page_policy_hir_authenticates_builtin_definition and the
existing constructor mutation owners reject altered nominal definitions,
children, variants and results. Native MIR constructor mutations reject wrong
policy widths and unfounded SSA; ordinary EnumTagEq/Select provide the dynamic
tag without a new IR operation. aligned_buffer's stack owner admits omitted
and explicit Default and refuses PreferHuge.

buffer_pages tests bind checked span/interior arithmetic, threshold edges,
all representative alignments through 2^29, zero/nonzero bytes, small/global
fallback, unsupported page-size lookup, refused advice, failed growth, prefix
copy and retained within-capacity pointers to independent mapping event
witnesses. buffer_constructor_probe crosses invalid tags/count/alignment,
null output and mapped acquisition/header refusal. The existing bounded
aligned_buffer admission child also verifies terminal release failure and
real same-address mapping reuse with no inherited hg flag. Successful munmap
uses the exact recorded base/span; no mapping reaches the global allocator.

The runtime ABI registry/golden, shifted output escape ordinals and Align's
apps/kv declarations/intercepted native owners cover all changed direct callers.
KV fingerprint changes cover only the extra Default ABI operand; parser work
and ownership expectations remain. The forged-put owner now asserts the
existing earlier native producer rejection rather than an obsolete LLVM-only
diagnostic. Existing aligned/fallible/field-growth/self-append/runway owners
close unchanged byte/window/authority behavior.

The bounded native probe and compact results are in
[bench/buffer_pages](../../bench/buffer_pages/README.md); its shared supervisor's
actual-process owner covers the added benchmark. Linux observes huge backing
and the complete first-touch cost; platform-independent source/native owners
exercise accepted fallback on the other supported CI hosts. Performance remains
local qualification, with no speed gate or consumer adoption claim.
