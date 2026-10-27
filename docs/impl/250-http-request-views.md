# Borrowed HTTP request views

Status: implemented for AM-R25 on 2026-10-27 after independent strategy review.
Consumer adoption is separate.

## Problem and capability boundary

The media HTTP caller owns a request context throughout synchronous image
generation. AM-R25 proposes a checkpoint callback through the shared still and
denoising engine. Today that engine receives `cancel_path: str` and calls its
private-marker checkpoint directly; it does not yet accept callback behavior.
A closure cannot capture the owning HTTP context, including a shared parameter.

Expose an ordinary Copy view of the request instead. Its representation and
lifetime rules follow the existing `http_headers` view. The callback captures
that value using the existing by-value capture rule. No new capture mode,
generic callback-signature composition, closure ABI or scheduler is needed.

The earlier shared-parameter capture proposal was rejected during independent
strategy review before implementation: it changed the Settled by-value capture
rule, left worker proof representation undecided, and understated generated-ID
format changes. This replacement follows ordinary library view composition.
HTTP design item10 previously declined a full request view for lack of a
consumer; AM-R25 supplies that consumer without reopening a language rule.

All three request registers and relevant cancellation paths were inspected.
LLM and voice retain their existing explicit owner/parameter patterns; no other
pending request is claimed as delivered. R128, R131, R132/R142 and AM-R26 keep
their separate restriction/evidence boundaries.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Type | `http_request_view` is a global nullary Copy, non-owning, region/provenance-tracked type. It has no source constructor and no Drop. Like `http_headers`, it is a pointer-sized request view, not a `Scalar` payload/element category. Existing named helper parameters/returns, record fields, Copy captures and already-admitted aggregate carriers retain their normal rules; no direct Option/Result/array/slice/box or function-signature scalar admission is added. |
| Creation | `ctx.view() -> http_request_view` takes no arguments. The receiver is an existing live `http_request_ctx` local or field, including one reached through an admitted shared parameter. Temporary-owner receivers retain their existing rejection. Creation is Pure, cannot fail, and copies the context pointer without allocation or native work. |
| Shared reads | `view.method() -> str`, `view.path() -> str`, `view.body() -> slice<u8>`, `view.headers() -> http_headers`, and `view.upgrade_ready() -> bool` take no arguments and are Pure. Their bytes and readiness semantics are exactly the existing context operations. Method/path/body/header results inherit the view's lifetime/provenance, without a new frame cap at every helper call. A view receiver may be any well-typed expression. |
| Peer observation | `view.peer_eof() -> Result<bool, Error>` takes no arguments and is Impure. It uses Plan243's one nonblocking, nonconsuming peek, exact error mapping and native mode/lock behavior. False is not liveness; true includes write-half-close and can coexist with already co-read request bytes. It does not select cancellation policy. |
| Owner reads | Existing read operations on `http_request_ctx` remain its normal shared inspection surface. Owner and view receivers use one shared semantic/lowering implementation; this is not a legacy compatibility path. Only minting the detached view differs in lifetime formation. No direct header lookup alias is restored. `http_headers` remains the narrower header-table projection. |
| Authority | The view provides observation only. It cannot respond, start a response/upgrade stream, replace, close or otherwise consume the context, obtain a raw pointer, or acquire exclusive authority. Capturing it copies no owner and does not null the source. |
| Lifetime and cleanup | Every view and projected request slice/header retains the context's exact owner generation. Local context views cannot escape that owner through return, break, aggregate, callable, storage or loop turnover. A helper receiving a view/shared context may forward admitted borrowed results under existing summaries. Response, Drop, replacement or ownership transfer cannot leave a subsequently used stale view. Caller response remains legal after the last callback/view use. No view owns a descriptor or cleanup flag. |
| Calls and captures | Direct/imported/generic named helpers may take or return the bare view. Indirect transport uses an already-admitted record carrier or captures the view; bare `fn(http_request_view)` parameters and conversion of bare-view parameter/return helpers to function values remain rejected because those formation gates require a signature Scalar. Existing function-type annotations may name a non-Scalar return (`fn() -> http_request_view`); this does not widen function-value construction. Local callable copies/joins and sequential pipeline callbacks use existing Copy-value capture and view-provenance rules. The owned `http_request_ctx` remains uncapturable. No function-value escape rule changes. |
| Workers | The view follows the same structured shared-view policy as `http_headers` and request body slices. Copy is not a waiver of lifetime/authority checks: captured backing stays live and read-only through the covering group join. An Impure `spawn` observer is permitted within its existing `task_group`; normal/error join ends backing protection under the existing task rules. `par_map` requires Pure and therefore rejects `peer_eof`. There is no unstructured escape, new scheduler or permission for simultaneous response/exclusive access. Native methods operate on immutable request state or the already-shared Plan243 peek. `respond_stream` and `respond_upgrade` change the owner fd and therefore conflict with pending tasks retaining this context, even though they do not consume the context. Record this access through the existing task-overlap and exported mutable-access summary authority, including direct/indirect/imported helper forwarding; do not end the request-buffer generation. Sequential views remain valid after those transitions: byte reads still work and `peer_eof` observes the existing Invalid result once fd is spent. |
| Errors and validation order | Arity/type/place/authority errors are compile-time diagnostics before executable publication. The view has no validation defaults. Native read/EOF operations retain existing error precedence and encoding/NUL semantics; there is no new text, wire, foreign-input or process-global state boundary. Invalid HIR/MIR cannot forge a view from another pointer-shaped value. Protected Copy leaves include both HTTP views through aggregate returns and calls; every producer/result or `Use` source containing either view is authenticated even when unused. |
| Representation and ABI | Authenticate exactly `HttpRequestCtx -> HttpRequestView` for creation and `HttpRequestView -> HttpHeaders` for projection, retaining existing `HttpRequestCtx -> HttpHeaders`. Reject every reverse or unrelated pointer-shaped `Use` retype. Existing native read/EOF symbols and signatures are reused; no runtime symbol, allocation, ownership or wire format changes. LLVM uses one pointer for the view in locals, fields, arguments and closure environments. |
| Identity and persisted records | Canonical type-graph v3 gains nullary leaf tag 83 (`0x53`); all existing tags/records retain their bytes. Its complete zero-definition graph is `[3, 0, 0, 0, 0, 83]`: version byte 3, little-endian u32 definition count 0, leaf byte 83. The decoder rejects truncation, trailing bytes, malformed graphs and unknown tags; independently checked encode/decode goldens pin this exact vector and a record containing the view. Interface format21 is unchanged: its existing named-type record stores `http_request_view` and zero type arguments; builtin capability is BorrowLeaf. Append new serde enum variants rather than renumbering existing cache-internal variants. Generated closure-ID format does not change: its existing canonical capture-type sequence naturally contains the new canonical type. Existing compiler/source/interface/cache identities invalidate changed compilation; no ambient input. |
| Prerequisite and acceptance | Plan243's EOF observation and the existing Copy-view/closure/provenance substrate are shipped. Provider tests execute imported loopback checkpoints and preserve the subsequent response and cleanup. Actual media engine changes, pin adoption, still/denoising EOF policy and GPU qualification remain consumer-owned. |
| Resource promise | View creation and synchronous Copy capture allocate no heap memory and copy no request bytes. Structured `spawn` retains its existing group environment/result allocation, with no incremental allocation beyond an equivalent header-view capture. Structural LLVM checks and a local allocation-count owner compare repeated view/checkpoint use to the existing header-view baseline. No latency, throughput or cancellation-delay claim. |
| Sources of truth | Update this ledger, HTTP EN/JA design, `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, Settled HTTP view decision in `docs/open-questions.md`, and the changed checked-HIR/canonical-type records in plans17/19. The runtime ABI ledger records reuse only if its accepted semantic operand set changes. HANDOFF and AM-R25 receive one capability-level status; unchanged closure/generic contracts are not restated. |

```align
fn invoke(checkpoint: fn() -> bool) -> bool = checkpoint()
fn observe(borrow ctx: http_request_ctx) -> bool {
  request := ctx.view()
  return invoke(fn { request.peer_eof() else false })
}
```

## Implementation closure matrix

Source formation and its complete type/provenance/ABI sweep are one capability.
The new view type must be authenticated by independent frontend, HIR, interface,
MIR and LLVM consumers. Splitting those into a dormant producer/consumer chain
duplicates proof and risks admitting an untracked pointer; it creates no useful
intermediate consumer.

The new primary driver owner is `http_request_view`. Reuse existing HTTP
headers, EOF, callback-authority, task and malformed-callable owners where they
already fail for the changed class.

| Axis | Implementation obligation | Exact owner / coverage |
| --- | --- | --- |
| Formation, fields, arguments and returns | Add the spelling and independent builtin inventory; source/header/body types agree. Type arity, constructor absence and direct scalar-carrier exclusions remain fail-closed. | `http_request_view::formation_and_authority`: whole/per-unit positive helper/record/capture cases; nullary arity, forbidden carriers/extern declarations, bare-view function parameters/helper-to-function-value conversion, view mutation/response and owned-context capture negatives. |
| Construction, copies, move-in/out, Drop and replacement | Creation records the exact borrowed context place; copying a view never transfers or drops its owner. Original context ownership, field source nulling and response cleanup remain unchanged. | `http_request_view::lifetime_and_control_flow`: live-use and response-after-last-use twins; bare-view, record/callable/aggregate, replacement, ownership-transfer and early-exit negative controls; native context cleanup/rebind. |
| Branch, loop and error paths | Preserve view roots through `if`, `match`, `else`, `?`, `map_err`, joins, loops and callback invocation. Ending a generation invalidates only its dependent views. | `http_request_view::lifetime_and_control_flow`: direct/indirect/imported helpers, selected returns, loop turnover, early failure, same-owner expiry and unrelated-owner positive twins. |
| Sequential and worker callbacks | Existing Copy-capture transport stays byte-for-byte except for the new canonical type. Nested captures and callable fields retain the view's owner roots. Structured worker protection applies to shared view backing, while Impure operations retain existing callback effect refusal. | `http_request_view::callback_and_transition_overlap`: synchronous repeated imported checkpoint; pure inspection callbacks; nested/helper-forwarded worker owner-lifetime/exclusive negatives, joined native EOF task and unchanged Copy/header positives; Impure `par_map` EOF refusal. |
| Native transition overlap | Classify `HttpRespondStream` and `HttpRespondUpgrade` as access-only native mutations at their action boundary: record context storage roots through `record_storage_access`/`record_mutable_access_roots` and exported mutable-access summaries, without `invalidate_storage` or owner-generation expiry. Keep required-child evaluation and early divergence ordering. This closes direct, imported and indirect task overlap while retaining sequential request bytes and post-transition Invalid EOF. | `http_request_view::callback_and_transition_overlap`: same-root direct/helper/indirect and nested-group negatives; provably distinct owned-root and post-normal/error-join positives; unknown shared-parameter alias negatives; sequential reads and spent-fd EOF are executed by `http_checkpoint_and_allocation_parity`; the checked-HIR owner independently replays the shared-parameter helper summary. |
| Generic monomorphization and interfaces | Add canonical type admission, distinct encoding, source spelling, layouts and provenance descriptors. Generic bodies instantiate the ordinary view type without new generic-function-signature composition. | `http_request_view::interfaces_and_cache`: exported generic relay and named helpers; whole/per-unit/native, cold/hit/edit/restore and actual cache-hit evidence; interface spelling tripwire and canonical-type golden. |
| Independent HIR validation and replay | Validate view formation only from an admitted context place; shared read operations accept exactly owner or view input as specified. Recompute region/borrow/effect/cleanup facts and reject stale or forged facts before lowering. | `align_mir --lib http_request_view`: source/type/operand/place mutations and exported-summary replay, all public lowering entrypoints; driver owners cover capture lifetime and owner/view sibling cases. |
| MIR producer and LLVM | Authenticate both new pointer retypes (context to request view, request view to headers), retain context to headers and reject reverse/sibling pairs; validate every native input/result type and complete borrow provenance; layout/field/signature/closure agreement, no owner cleanup or new native symbol. | `align_codegen_llvm --lib http_request_view` and `sema_and_codegen_struct_layout_agree`: raw/optimized LLVM verification, pointer/record/closure layout; unchanged-type pointer-compatible invalid producers and forward/reverse retypes across bare/record returns, calls and unused carriers; native operand/result twins; exact canonical-type encoding golden. |
| Native HTTP behavior | Context views preserve method/path/body/header bytes and upgrade-readiness behavior. EOF peeks keep bytes, accepted context and response path alive, including write-half-close and next-request controls. | `http_request_view::http_checkpoint_and_allocation_parity`: imported whole/per-unit loopback chain, repeated callback and retained request views; bounded fixture-owned processes/sockets; explicit policy stop/continue and subsequent response. |
| Allocation parity | View construction lowers to pointer transport; synchronous closure environments remain stack-local; task environments retain existing group allocation with header-view parity. No native/allocation work is introduced by the view. | `http_request_view::http_checkpoint_and_allocation_parity`: repeated view/callback operations against header-view control, exact allocation delta and structural no-copy/no-allocation LLVM owner. |

## Author consistency and independent review

The ledger covers changed type/signature, authority, lifetime, allocation,
effects, inputs, errors, identity, prerequisites and acceptance. There is no new
wire format, text decoding, runtime table or global-state mutation. Canonical
type identity is the one persisted-format addition, fixed to v3 leaf tag 83
and the exact golden above before source changes. The matrix closes all view transport and lifetime paths rather
than admitting only the original HTTP example. Existing request callers and
consumers remain unchanged until they separately adopt the delivered compiler.

Independent strategy review accepted the ordinary Copy-view strategy. Its findings
were closed before source admission: access-only stream/upgrade overlap tracking,
bare-view indirect-signature exclusions, exact retype directions and synchronous
allocation scope. Concurrent observations retain their existing independent results.
