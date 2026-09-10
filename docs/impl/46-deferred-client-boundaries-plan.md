# R65 and R63: decisions before implementation

Status: **R63 implementation and acceptance follow [plan 47](47-json-numeric-contract.md);
R65 exact contract remains incomplete and its implementation has not started**.
Assessed provider main `d469adcb931848ba03d18dcff022d32d95c2fda9` and the
uncommitted align-llm request register. This document advances the two unresolved
rows of [plan 40](40-align-llm-product-boundary-plan.md). It is a decision and
qualification plan, not an accepted replacement for the process or JSON specs.
This earlier decision record does not qualify any production isolation deployment.
The implementation-model handoff and remaining design work are in §8. Plan 47
supersedes this document's unclosed JSON design cells and policy alternatives;
its review status is authoritative for R63.

## 1. Decision ledger

Keep this table authoritative while the next exact contracts are authored.
An unclosed cell is a prerequisite, not implementer discretion. Existing
[process](std-design/process.md), [JSON](core-design/json.md) and
[recursive-owned JSON](25-recursive-owned-json-plan.md) contracts remain in force.

| Boundary | Recommended decision | Exact contract still to close | Ownership, allocation and errors | Owner, identity and acceptance |
| --- | --- | --- | --- | --- |
| P1 immutable inputs and executable identity | Explicit streaming construction followed by irreversible sealing; hash the final sealed bytes and execute that same image. Native executable admission and data admission are distinct. | Constructor/finisher types; executable versus non-executable memfd flags and modes; ELF admission; inherited reader offsets; current-image observer result; argv/env/cwd grammar and error order. | Move owners; no implicit file-sized snapshot during command launch. Writes retain no input. Finalized bytes cannot be modified through another fd. Constructor failure publishes no owner. Existing fatal allocation policy remains unless the exact ledger explicitly specifies native allocation errors. | `std.process` over filesystem readers and existing digest; compiler transport plus runtime syscalls. Runtime closure and source provenance are not an executable digest. Owner: proposed `m11_process_verified`. |
| P2 launch and live I/O | Extend the existing command/child implementation with finite readiness, caller-buffer reads and cached nonblocking direct-child status. | Exact launch spelling; `Data(n)`/`Pending`/`Eof` read result; output-buffer rules; readiness fields; incompatible builder settings; launch error handshake. | Child owns pipe ends and reap authority; input borrows end at launch or are explicitly retained in the builder contract. No implicit unbounded capture. Existing synchronous timeout/capture settings cannot silently be ignored by live launch. | `std.process`; reuse launch and pipe code. Proposed `m11_process_live`, existing `m11_process_command`. No general event framework or interactive protocol prerequisite. |
| P3 isolated lifetime | Prefer one owned PID namespace/init per isolated launch, created under the admitted user namespace. Keep subreaping out of the caller's global process state. | Actual deployment admission, exact init bootstrap/control protocol, mapping to existing child state, member authentication, parent-death coupling and release/Drop behavior. These must pass Q0 before the public API is frozen. | Explicit isolated launch creates the init and workload; their extra processes are documented. No policy/report supervisor. Failed explicit release retains authority. No absence result from a procfs snapshot or pipe EOF. | `std.process`, Linux x86_64/ARM64 first; strict isolation unavailable on macOS. Proposed native qualification and `m11_process_containment`; all persisted/helper identities must be specified if Q0 selects any helper artifact. |
| J0 shared JSON numeric validity | Owner-approved: finite typed JSON numbers and `json.encode -> Result<string, Error>` across existing routes, with owned float leaves. | Exact numeric vectors, error precedence, cleanup and compiler/native transport; see §4. The public direction is approved; its complete ledger still needs review. | Keep IEEE arithmetic unchanged. Invalid JSON conversion is recoverable; no nonfinite-to-null substitution, implicit string conversion or hard abort. | Existing `core.json`; every producer/decoder, checked IR, interface/cache and callers. Proposed numeric policy owner plus existing `m5`, `m5_json_bounded` and `m5_owned_json`. |
| J1 owned float leaves | The owner has admitted reconsideration; add `f32` and `f64` structurally at the existing scalar positions. | Exact graph/envelope version and tags; target layouts; complete numeric vectors; final J0 decision. | Float leaves are Copy. Existing owned text/array/record cleanup remains recursive and independent of source/arena. No per-record opt-in or second codec. | Sema, interface, HIR validation, MIR, LLVM and runtime together. Complete reachable graph identity; integer-to-float changes must invalidate caches. |

P1/P2/P3 define OS mechanisms. The Align application selects tools, builds sandbox
arguments, drains streams fairly, enforces deadlines and resource limits, records
its first execution error separately from cleanup failure, and decides whether
workspace deletion is authorized. No consumer code is changed by this work.

## 2. R65: isolate ownership at namespace construction

### 2.1 Recommended topology

```text
Align application: policy loop and one existing child owner
  +-- authenticated namespace init: minimal launch/reap mechanism
        +-- admitted executable / external sandbox
              +-- workload processes, nested sessions and namespaces
```

The outer lifetime namespace belongs to Align's launch. The existing external
sandbox still establishes mounts, networking, credentials and filesystem policy
from explicit application arguments. Its launcher PID, an info-fd message, or a
caller-supplied namespace path alone cannot manufacture an absence certificate.
Do not infer the owned init from an arbitrary raw PID. Admission binds the
namespace, its actual init and stable process handle before releasing the workload.

This refines plan 40's preferred PID-namespace path. A caller-wide subreaper lease
would need to exclude unrelated spawning, reaping, SIGCHLD changes and FFI across
its entire lifetime. A separate owned namespace removes that shared adoption
state from the normal application. The init performs only bootstrap, status
transport and reaping. It contains no deadline, capture limit, resource aggregation,
retry, workspace traversal or report logic.

This removes shared adoption state, not caller-side wait interference. The init
is still a child whose status can be auto-reaped or consumed by another waiter.
Q0 must preserve exclusive reap authority for the entire owner lifetime, including
signal handlers and foreign code. Checking SIGCHLD only at construction is not
sufficient. `ECHILD` alone is never an absence/release certificate.

The kernel's namespace teardown is stronger than a descendant scan. Linux v6.18
`zap_pid_ns_processes` stops new PID admission, kills remaining tasks, waits for
children and other namespace tasks, then permits the namespace init to be reaped.
That supplies a candidate completion witness **when the actual owned init is
reaped**, subject to the admission and exclusion proof. It is not a time bound.
Sources: [kernel implementation](https://github.com/torvalds/linux/blob/v6.18/kernel/pid_namespace.c#L180),
[PID namespaces](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html).

Process pidfds must not request `PIDFD_THREAD`. A process pidfd becomes ready
only after its last thread exits; a zombie leader with a live worker is still
live for this contract. Stable identity is separate from authority to reap:
`waitid(P_PIDFD)` requires the correct parent/reaper relationship.
Source: [pidfd_open](https://man7.org/linux/man-pages/man2/pidfd_open.2.html).

Cgroup v2 is not an automatic fallback. It requires trusted delegation and
migration exclusion, and its live-process listing omits zombies. A cgroup kill
operation alone does not discharge reaping. Introducing that deployment dependency
would require a separate selected profile, not a runtime heuristic.
Source: [cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html).

### 2.2 Proposed lifecycle and failure policy

```text
Preparing -> Admitted -> Running -> Stopping -> Empty -> Released
                 \-> constructor failure cleanup (no successful child published)
```

- Admission precedes execution: typed namespace validation, explicit inherited-fd
  allowlist, safe complete fd remapping, signal disposition and exec handshake.
  Validate all text/slots before spawning; no fallback to PATH or a replaced path.
- The direct command's cached status and namespace completion are separate facts.
  EOF on both streams is a third fact. No one of these implies the other two.
- Stop signals the owned init through authenticated authority. The application
  can continue bounded drain/status/absence observations and retain its first error.
- Explicit release is exclusive and retryable. Pending work reports pending;
  failure leaves the owner able to stop or retry. It cannot consume the only
  authority on an error path. Release succeeds only after init completion/reap and
  closure of any owned bootstrap participants/control state.
- Recommend Drop = request termination of this owned namespace, reap the init,
  then close remaining resources. It can block; no bounded successful Drop is
  promised. This fits the existing blocking child Drop model without detachment.
  If ownership invariants prevent safe reap, fail-stop rather than claim success
  or free a reusable identity. The exact fatal path must be selected in Q0.
- Normal cancellation uses explicit fallible cleanup. Supervisor SIGKILL/OOM/
  abort cannot rely on language Drop. Qualify kernel parent-death coupling before
  advertising that stronger guarantee; installation race and the identity of the
  creating thread matter. Source: [PR_SET_PDEATHSIG](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html).
- Uninterruptible kernel I/O may prevent completion. The application keeps the
  workspace and reports failure; a deadline cannot turn unknown absence into success.

### 2.3 Q0: implementation-independent qualification

Run a disposable, bounded probe harness before fixing P3 signatures. Pin the
kernel/profile, architecture, installed sandbox identity and explicit namespace
input in its result. Local WSL evidence below is not production qualification.

| Proof | Required positive and adversarial observations |
| --- | --- |
| Namespace admission | Fresh/retained user namespace permissions and uid/gid mappings on the actual deployment; wrong ns type, stale handle, denied permission and unsupported OS reject before workload side effects. Verify namespace ownership and init identity while launch remains blocked. |
| Exclusive reap authority | Qualify the exact init/bootstrap parent topology against initial and later `SIGCHLD = SIG_IGN`, `SA_NOCLDWAIT`, signal-handler `waitpid(-1)` and concurrent foreign waiters. Select enforceable admission/lifetime exclusion or a different reaper arrangement. Automatic/stolen reap is an ownership failure, never successful release based solely on `ECHILD`; stable pidfd identity alone does not fix it. |
| Init implementation | Decide syscall-only post-fork bootstrap versus explicit helper executable. No allocator/lock use inherited from a multithreaded parent. A helper would need a producer-owned artifact identity and exact bootstrap byte protocol before implementation. |
| Executable/input binding | Seal write/grow/shrink and seal set; writable mappings; ELF versus scripts; exec/noexec memfd policy; independent read positions; replaced original executable; OS-derived running image; dynamic interpreter/library and origin-relative loading limits. |
| Descriptor transport | Exact 0/1/2 treatment, extra slot bounds/duplicates/cycles, parent flags/offset preservation, all unlisted descriptors closed, and typed namespace slots inaccessible to untrusted workload unless explicitly authorized. |
| Lifetime and identity | Nested setsid, double fork, nested namespace, zombie leader/live thread, init death during startup, root exit with descendants, repeated stop/reap and deliberately stale PID. Concurrent owners must not affect one another. |
| Parent failure | Kill the supervising process and creating thread at each handshake boundary; credential/exec transitions that clear parent-death state; external sandbox launcher death. Prove actual owned init termination and final absence, not just a requested signal. |
| Observation | Admit individual members using stable handles plus namespace identity checks. Define each resource field, width, units, unavailable value and producer-owned OS source. A changing scan is not an atomic total or a hard resource ceiling. |
| Release/Drop | Failure before/after every resource acquisition; pending release retains authority; explicit stop/drain/reap; cancellation and discarded results; no workspace mutation by runtime; unsupported profile rejects. |

The init/control transport, member admission and exact resource fields are
intentionally not invented in this plan. Q0 must answer them together. If the
selected deployment cannot support this topology, reopen this boundary with the
measured failure instead of implementing a weaker process-group approximation.

### 2.4 Evidence obtained on 2026-09-10

Provider `d469adcb`; local Linux `6.18.33.2-microsoft-standard-WSL2`, x86_64,
ordinary user with effective capabilities zero; installed bubblewrap reports
`0.12.0`. Temporary probes changed no compiler or consumer implementation.

| Probe | Observed result | Limit |
| --- | --- | --- |
| `unshare --user --map-root-user --pid --fork --mount-proc` | The isolated process observes PID 1. | Fresh local user namespace only; not the consumer's prepared namespace. |
| Non-executable sealed memfd | Write, grow and shrink each fail with EPERM; two separately reopened read-only fds both read the first bytes independently. | Does not qualify executable memfd, writable mappings or the complete fd allowlist. |
| Kill namespace init after double setsid/orphan creation | Retained descendant pidfd completes; descendant proc entry is absent after the launcher wait. | Cooperative probe processes; not the full adversarial sandbox or parent-death proof. |
| `pthread_exit` leaves leader zombie and worker live | Process pidfd remains pending; completes after worker exits. | Does not alone prove namespace completion or arbitrary member admission. |

Reproducibility checkpoints: `/tmp/align-r65-namespace-probe.py`,
`/tmp/align-r65-thread-probe.c` and its binary. These are local investigation
artifacts, not shipped test owners. The implementation must check in a hermetic
owner for every selected guarantee and run its matching local script before CI.

## 3. R63: owner-directed reconsideration

On 2026-09-10 the owner directed Align to support standards-conforming JSON,
retain performance, and leave application-specific treatment of values outside
JSON with the caller. Combined with the instruction to address R63, this admits
reconsideration of the owned-float exclusion for this request. Do not keep R63
waiting for the friction count. The [general protocol](23-friction-ledger.md)
remains unchanged; no additional occurrences or second client are claimed.

JSON does not specify Align ownership or require arbitrary-precision machine
numbers. R63's finite `temperature: f64` with owned text is a reasonable typed
mapping of ordinary JSON, and ownership supplies no reason to forbid it.
The owner subsequently approved the recommendation that both encoders return
`Result<string, Error>`, with `Error.Invalid` for nonfinite encode input and bounded
size failure, no partial output, and ownership transfer of the generated buffer.
This settles output ownership and fallibility. Exact graph/transport contracts,
numeric vectors and the complete error/cleanup ledger still need design closure
and review. No second nonstandard JSON codec is authorized.

## 4. JSON numeric validity: a prerequisite discovered during planning

### 4.1 Reproduced behavior

A borrowed record with `label: str` and `value: f64` on provider main:

```text
value = 1.0 / 0.0
json.encode(value-record)              -> {"label":"x","value":inf}
json.encode_bounded(value-record, 100) -> Ok("{\"label\":\"x\",\"value\":inf}")
json.decode(... "value":1e999 ...)    -> Ok(record with positive infinity)
```

Both encode outputs are invalid JSON: the number grammar excludes infinity and
NaN ([RFC 8259 §6](https://www.rfc-editor.org/rfc/rfc8259.html#section-6)).
The temporary executable
`/tmp/align-r63-float-policy.align` reproduced all three observations against the
optimized compiler. This is not evidence that the owned route should copy that
behavior. Existing scalar parsing uses `JsonParser::number`; several template
and descriptor encode paths call `push_float`, also used by ordinary printing.
Changing the general float formatter would accidentally change print semantics.

### 4.2 Recommended public decision

Admit `f32` and `f64` at every supported JSON scalar position with one rule:
conversion must produce a finite value in the destination type. JSON number
grammar, signed zero, subnormal/underflow rounding, shortest round-trip output and
exact bytes must be pinned with shared independent vectors. Arithmetic remains
IEEE 754. A grammar-valid number outside the destination's finite range is a
conversion error; raw document syntax need not reject a number merely because
no chosen machine float can hold it.

For encoding, the owner approved `Error.Invalid` on nonfinite input and an owned
Result for both operations. The selected public direction is:

| Operation | Approved result and ownership |
| --- | --- |
| `json.encode(value: T) -> Result<string, Error>` | Success owns free-standing output, independent of source and arena. Every accepted schema uses this signature. Nonfinite input returns Invalid; failure frees partial output. |
| `json.encode_bounded(value: T, max_bytes: i64) -> Result<string, Error>` | Same numeric policy and output ownership, with the existing inclusive byte limit. Negative/exceeded limits return Invalid. |

The source remains borrowed for the call. Ordinary `?` or `match` handles the
result. No implicit conversion to null/text and no infallible parallel encoder.
The earlier `Result<str, Error>` alternative is no longer the selected direction.

Source inspection on 2026-09-10 found that `BuilderBuf::into_raw_parts` and
`builder_into_string_value` already transfer the grow buffer into an owned string
without another allocation or full-result copy. The bounded encoder uses this
finisher today, and `stack_builder_header_finish_transfer_and_unfinished_drop`
checks pointer identity. Current arena finish instead copies the grow buffer into
the arena. Owned output therefore does not inherently require an extra copy;
individual Drop/storage lifetime changes still require the §4.4 benchmark.
This explains the selected owned-output direction. The complete transport and
cleanup contract is still pending; this is not a shipped behavior change.

R63's reported `temperature` cases are 0.0 and 0.3, both within f64's finite
range. The latter uses normal binary floating-point rounding. The register does
not require arbitrary-precision numbers for this request. This establishes range
suitability for R63, not consumer-wide runtime qualification.

`json.encode_bounded(value, max_bytes) -> Result<string, Error>` already has the
needed error surface. Recommended precedence: negative limit first, then one
source-declaration/array-index ordered traversal. The first nonfinite leaf or
first byte exceeding the cap wins; both return Invalid without publishing output.
No unbounded-first buffer or second whole-message scan. Decode preserves its
existing `Error.Code(1)` conversion route, input-order error precedence and partial
owner cleanup. Numeric access through `json.doc` reports an unavailable conversion
through its existing Option; structural document parsing remains separate.

This choice affects every encoding route, including borrowed records, owned
records, arrays, unions, descriptor encoders and template lowering. No source
shape may continue emitting invalid JSON by bypassing the common JSON numeric
writer. Ordinary `print` and template float formatting are unchanged. This wider
contract must be completed and reviewed before implementation. Owner approval
settles the return type; it does not supply missing ABI or numeric golden vectors.
Decode owners must also sweep bare scalars, scalar arrays, borrowed/owned records,
unions, AoS speculative and fallback paths, SoA fill, `json.scan`, and numeric
`json.doc` access. Check finiteness after narrowing to f32 as well as after f64
parsing. Reuse their shared numeric conversion rule without changing raw syntax
navigation or general-purpose float formatting.

### 4.3 Owned graph and compiler work

After the exact numeric policy and API decisions:

- Add a Float leaf (`f32`/`f64`) to the one recursive classifier at direct, Option
  payload and array element positions, including nested/generic records. Keep the
  transitive owned-string route selector and all unrelated exclusions unchanged.
- Float leaves are Copy, with target width/alignment; no Drop. A prior owned text,
  record, array prefix or Some payload must still clean up exactly once on error.
- Extend semantic graph production, independent interface graph decoding, HIR
  validation, MIR fingerprints, LLVM table emission and runtime parsing/writing
  atomically. Current V2 leaves use tags 0x01 (integer) and 0x03 (bool), but an
  unused number is not permission to extend the persisted format without a ledger.
- Specify the new descriptor/envelope/interface version policy, exact tags and
  widths, every nested layout and structural graph identity before coding. Reject
  stale or malformed metadata; no old/new compatibility decoder. Do not copy the
  historical plan 25 interface version number over the current format.
- Keep all three operations on one accepted graph. Add independent byte/semantic
  goldens, malformed width/tag/layout cases, and a source-level accepted/rejected
  matrix. Include ordinary no-owned-text routes so their policy cannot drift.

### 4.4 Standards boundary and performance requirement

Align-generated JSON must obey the standard grammar. When a source value cannot
be represented, report conversion failure; the caller chooses explicit omission,
substitution or another data model. `NaN`/infinity are not JSON number tokens.
A token such as `1e999` is valid JSON syntax but may exceed a chosen destination
type. RFC 8259 permits implementation range/precision limits; rejecting that typed
conversion does not require rejecting raw document syntax or adding arbitrary
precision. Source: [RFC 8259 §6](https://www.rfc-editor.org/rfc/rfc8259.html#section-6).

The owner's performance requirement makes local measurement mandatory for J0/J1.
Check numeric validity during existing conversion/emission, without a separate
whole-value validation pass, message-sized temporary copy or dynamic JSON tree.
Schemas without float leaves should carry no float-specific runtime checks; the
public Result signature remains uniform. These are design targets, not measured
speed claims.

Compare optimized builds on the same machine, inputs and settings. Measure
existing integer/text encode/decode, borrowed float records, arrays/SoA, bounded
encoding and the real mixed owned-text/float shape. Use the existing borrowed
route plus explicit text cloning as the owned-shape baseline because old owned
float decode rejects. Record throughput/latency, allocations and peak storage;
repeat enough to distinguish noise and investigate regressions before merge.
Do not invent a percentage allowance, promise zero cost before measurement, or
accept a slowdown solely because it is needed for correctness. Fix the execution
path while preserving conforming output. Benchmarks remain local measurements,
separate from the bounded correctness gate.

## 5. Capability boundaries and implementation closure

Preferred order: qualify the uncertain P3 deployment first; author the exact
process ledger and close the JSON decision in parallel; then implement independently
useful P1/P2 verified live launch, full P3 isolation, and the unified J0/J1 codec
capability. JSON need not wait for process implementation after its decisions close.
P1/P2 is a valid boundary only when a caller can seal/hash/launch/read/reap end to
end. Do not split dormant descriptor producers from their sole consumers.
P3 does not count as complete when only P1/P2 ships.

J0 and J1 should normally share one compiler capability PR: both change scalar
acceptance, encoder error transport and recursive graph proof. J0 is also independently useful if another concrete J1 prerequisite prevents
the combined boundary from closing. No implementation starts from an unresolved ledger.

| Closure axis | Applicable implementation and proposed owner |
| --- | --- |
| Formation, generic substitution, exact schemas, malformed input | Shared source/HIR graph classifiers; independent interface decoder; LLVM validators. Extend `m5_owned_json`, `m5_json_bounded` and proposed process owners with negative controls. |
| Construct/move-in/move-out/source nulling/replace/return/Drop | Existing recursive ownership machinery and explicit process owner state transitions. Cover omitted-Drop negatives and failures after every acquired fd/owner. |
| if/match/else/?/map_err/branch joins/loop joins/early exits | Parameterized whole/per-unit owners crossing returned owned decode, encoded owned Result values and live process owners; invalid-source diagnostics must precede lowering. |
| Bytes, outputs, native ABI | Exact layouts, widths, bounds, overlap, zeroing and side-effect order; malformed output does not advance or launch. New native keys go in the runtime ABI ledger with independent export/type checks. |
| Interfaces and cache | Imported borrowed helpers and generic roots; cold/hit/private edit/restore and reachable field type changes; actual compiler/runtime/helper artifact identities. |
| Runtime provenance and allocation | Decode output survives source/arena expiry; encoded string survives source/arena expiry and is freed exactly once; successful finalization transfers the grow buffer without a full-result copy; no retained scratch; allocation/free and fd parity including error prefixes. |
| Platform and global state | Q0 deployment matrix; unrelated child owners remain isolated; no caller-wide namespace/subreaper mutation; initial and lifetime SIGCHLD/foreign-waiter exclusion; unsupported profile refusal. |
| Performance/resource claims | JSON has the explicit owner performance requirement in §4.4 and needs before/after local measurements. Process work makes no bounded-cleanup claim; verify finite caller-buffer reads and no hidden preimage snapshot as invariants. |

Before implementation, each row must reference the exact selected API and native
record, implementation sites, and owner tests in its capability ledger. Above
roughly 1,000 handwritten lines, record why the complete capability avoids
repeated ownership/ABI proof; do not manufacture a smaller dormant split.

## 6. Documents, review and acceptance

At public-contract acceptance update `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md`, Settled entries, plan 25, the process/JSON English designs
and Japanese mirrors, relevant HIR/runtime/interface/cache ledgers, caller examples
and the request register in one consistency pass. Proposed names here are not
syntax-checked Align declarations; normative examples must be checked when the
exact declarations exist. This planning update does not rewrite shipped promises.

Perform one author-side ledger/matrix consistency pass, then one independent
adversarial review of the selected strategy and capability boundaries. Resolve
those findings before code. The implementation follows normal local owner,
review, bounded-gate and Clippy checks; qualified Linux execution needs a local
reproduction script alongside any new required CI job. macOS refusal tests do
not constitute Linux isolation qualification. Consumer adoption and final product
acceptance remain explicitly pending after provider merge.

## 7. Planning review checkpoint

The author-side pass checked the decision ledger against the proposed topology,
state machine, numeric-policy alternatives and closure matrix. Unclosed public
fields, transport formats and deployment proofs remain explicit gates; no shipped
specification or mirror was changed. Local links and fenced blocks were checked.

One independent inspection-only strategy review found one P2: per-namespace
ownership does not by itself exclude caller SIGCHLD/foreign-wait interference.
The complete cause class is now explicit in the topology rationale, Q0 proof
table, release failure rule and platform closure row. The review accepted the
kernel completion witness and found no other P1/P2 strategy issue. The later owned-output recommendation followed direct builder-source inspection.
Its exact R63 API/ABI/numeric contract has since completed author and independent
review in plan 47. This earlier strategy checkpoint is not a substitute for that
review, and R65 still needs its own exact contract.

## 8. Handoff to the implementation model

Read `HANDOFF.md`, this plan, plan 40 and the process design before R65 work.
This document is the process planning handoff; **plan 47 owns the exact R63
contract, implementation and acceptance**. R65 is not ready for code.
Do not reopen the approved JSON policy or silently choose values for unclosed
public/native fields.

| Boundary | Settled direction | First unfinished work before production code |
| --- | --- | --- |
| JSON / R63 | Standard JSON, finite f32/f64 conversions, structural owned float leaves, both encoders return owned `Result<string, Error>`, Invalid for nonfinite encode/limit failure, no partial result, performance preservation. No friction-count wait. | The exact contract, vectors, compiler/native transport, closure matrix, migration and benchmarks are now in [plan 47](47-json-numeric-contract.md). Its author pass and independent review are complete, with the one P2 resolved. Follow its implementation order and acceptance owners; do not repeat this earlier design exploration. |
| Process / R65 | Per-launch owned PID namespace/init preferred; typed executable/input/fd authority; minimal native launch/reap; application supervision; no weak fallback or guessed absence. | Run Q0 on the actual supported deployment, including prepared user-namespace credentials. Close init parent/reap exclusion, bootstrap/helper choice and artifact identity, authenticated status/member transport, complete signatures/native layouts/errors, parent-death and release/Drop failure semantics. Propagate and review the exact contract after qualification. |

The broad strategy review in §7 is useful evidence, not a review of an exact
contract. R63 now has that separate review in plan 47; R65 remains unwritten. Reuse its finding and local probes; do not rerun the
same strategy review as an implementation-discovery loop. Review the newly closed
contract once, then follow the repository's ordinary code review and local gates.
No consumer implementation or adoption work is authorized. Maintain the sibling
request register only and leave its edit uncommitted.
