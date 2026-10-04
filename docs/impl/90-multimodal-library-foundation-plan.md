# Multimodal workloads through ordinary Align libraries

Status: H1/H2 implemented; H3 and the reference composition remain pending.
H1 adds no source API. Concrete engine packages still require consumer evidence.
Baseline: `5b76f1b3ecf1ec006d4df86fb99ac4f9df74a942` (2026-10-04).

## Authoritative public-contract ledger

This ledger owns the proposed provider changes. The composition records later in
this plan are application design guidance, not new compiler builtins, a frozen
model protocol, or a package API promised before a concrete consumer exists.
Existing language and library contracts remain authoritative until a capability
is implemented. `bytes` below denotes the existing byte-input admission, not a
new source type: `slice<u8>`, `str`, and owned text admitted by the current checker.

| ID | Surface and exact inputs/defaults | Ownership, allocation, effects and errors | Owner, identity and prerequisite | Acceptance and required measurement | Sources to synchronize when implemented |
| --- | --- | --- | --- | --- | --- |
| H1 | Existing `s.send(chunk: bytes) -> Result<(), Error>` and `s.send_event(data: bytes) -> Result<(), Error>` | Call-scoped readonly input; exclusive bound stream receiver; Impure. Remove payload-sized framing allocation/copy. Existing empty-send, lazy-head, rejection, poison, finish and Drop behavior remains. No retained input, encoding conversion or implicit queue. | Runtime stream writer; existing HIR/MIR, symbols and public type identity. Runtime artifact identity changes normally; no interface schema change. Existing HTTP stream is the only prerequisite. | Arbitrary binary octets, exact SSE bytes, partial vectored writes, EINTR, disconnect and ownership owners; allocation/copy counters and paired token/bulk benchmarks. | `std-design/http.md` + `ja/http.md`, `pkg-design/web.md` + `ja/web.md`, plan 15's performance record; wording in `draft.md` / `language-spec.md` only where it describes writer behavior. |
| H2 | `s.write_timeout_ns(timeout_ns: i64) -> Result<(), Error>` on `http_stream`; default `0`, meaning no bound; nonnegative values accepted | Exclusive nonconsuming receiver; Impure. Stores one scalar, no allocation/I/O. Each later send/send_event/finish/reject that writes bytes snapshots one monotonic total write budget; partial progress and EINTR do not reset it. Empty send and close-only raw finish remain clock-free. Invalid argument/state is Invalid. A timed-out write poisons the stream and returns Timeout; consuming operations still close exactly once. | Sema, HIR/MIR validation/lowering, rechecked interface source bodies, runtime key/ABI, runtime. No new type or interface-format change (format 17). H1 is a shared writer prerequisite. | Deadline/partial-write/state Cartesian owner, both native platforms, whole/per-unit and forged-HIR owners. No new speed claim; H1 regression measurement includes default and configured modes. | `draft.md`, `language-spec.md`, `design-notes.md`, `open-questions.md`, std HTTP English/Japanese, runtime ABI ledger and relevant implementation plan. |
| H3 | Proposed `srv.accept_timeout_ns(timeout_ns: i64) -> Result<(), Error>` on `http_server`; default `0`, meaning no bound; nonnegative values accepted | Exclusive nonconsuming receiver; Impure. Stores one scalar, no allocation/I/O. `accept()` snapshots one monotonic budget covering selection and complete request acquisition. Timeout closes a selected incomplete request, preserves the listener and other parked connections, and returns Timeout. No incomplete request escapes. | Same compiler/runtime owners as H2; existing request cap and keep-alive owners. New operation identity, no new type or request-body model. Independent of GPU libraries and K1. | Idle/partial/malformed/fresh/parked request and timeout-order owners; resource bounds, repeated timeout then success, whole/per-unit and forged-HIR owners. No performance promise. | Same specification/HTTP/ABI set as H2; pkg.web docs only if a package-level configuration surface is separately adopted. |
| R1 | Existing opaque `resource`, explicit `borrow` / `borrow mut`, `resource_ref`, and native FFI | Package-owned model/session handles; existing Move and exactly-once Drop rules. Model allocations and copies remain explicit operations. All native errors use Result; absent observations use Option. No Backend trait, hidden GPU allocation or compiler-known model kind. | Existing library-boundary plan 17; concrete engine package owns its native dependencies and interface identity. | A real wrapper must demonstrate constructor failure, partial load rollback, borrowed-output lifetime, explicit release failure, and Drop before claiming readiness. | Existing sources remain unchanged unless a reproduced language/library gap is found. |
| P1 | Existing `process.command`, `start` / `start_scope`, `poll`, reads, status, signalling, reap and release | Explicit child owner; caller-provided byte scratch; bounded waits and cleanup steps. Linux child_scope can certify managed descendant absence; a root exit or group scan cannot. No hidden supervisor/thread. | Plans 49/50. Linux/WSL2 production; native macOS supports the common child subset, not Linux-only scope acquisition. | Mock worker completion/cancellation/crash/descendant controls; real GPU release is a separate engine acceptance. | Existing process contracts reused, not redefined. |
| C1 | Ordinary single-owner job state, artifact records and sample/frame time records | Arrays/records/sums and existing Result/Option. Application supplies bounds, persistence and engine policy. Every controller reply uses H2; a parent-owned capped pipe-to-file sink enforces reference artifact output, accounting for staging, retained and retired-but-open files. HTTP connections borrow/read snapshots; job lifetime is owned independently. No new async language model. | Reference composition using H2/H3 and existing HTTP/process/fs/JSON. No new canonical persisted format, scheduler ABI or model cache identity is introduced by this plan. | Complete mock text/audio/batch pipeline, bounded queue/event/output storage, cancellation, disconnect and failed-release controls; stalled control reader, oversized output and retirement during live download; metadata arithmetic/round-trip controls. | One reference guide/example after the necessary provider capabilities; extract a pkg API only from demonstrated common consumers. |

## Purpose and layer ownership

Support text generation, speech, image/video generation, upscaling and final
media assembly on one Linux/WSL2 machine with a 12 GB NVIDIA GPU and 128 GB RAM.
The GPU workloads run serially when their working sets cannot coexist. Native
macOS retains general I/O/process support; GPU model qualification on the
user's hardware is a Linux/WSL2 acceptance, not a macOS milestone.

The design is for Align and its libraries. `align_codegen_llvm` is a compiler
backend, not an inference backend. It must never learn Qwen, VITS, FLUX, Wan,
model IDs, job routes, waveform codecs or a universal tensor graph.

| Layer | Responsibility |
| --- | --- |
| Language / core | Existing Move/Copy, borrow, Result/Option, contiguous arrays/slices, explicit allocation, SIMD, structured task_group and FFI. No new syntax or parallel/error/ownership model. |
| std | Generic bounded HTTP byte I/O, existing process observation/control, filesystem owners, clocks and native error mapping. |
| pkg | Concrete native-engine wrappers and reusable codecs/frameworks when actual consumers establish their contracts. Opaque resources hide native representation, not allocation or side effects. |
| Applications | Model selection, device admission, scheduling, job persistence, OpenAI-compatible routes, pipeline stages and deployment configuration. |

The proposed four requests therefore have these dispositions:

1. Reuse `http_stream`; repair its payload copy and bound slow writes.
2. Use concrete package resources and explicit native/process lifetimes. A
   universal Backend trait or compiler-provided Engine is not required.
3. Compose a job owner independently of HTTP requests; add bounded server
   acquisition so a serial controller can keep supervising live children.
   A long job is asynchronous relative to its submitting request, without
   introducing language-level async/await or implicit background work.
4. Carry binary media and ordinary typed metadata separately. Preserve exact
   sample/frame positions; JSON and container encoding remain library work.

K1 remains deferred. These changes read call-scoped bytes and mutate existing
opaque I/O owners; they neither depend on nor claim to repair cross-call view
authority. A newly reproduced compiler defect gets its own bounded request.

## Audit at the plan baseline

| Evidence | Consequence |
| --- | --- |
| `check_http_stream_method` in `crates/align_sema/src/lib.rs` accepts the existing byte-input family for `send`; `apps/web/pkg/web.align::stream` accepts a content type. | Arbitrary binary data already has a source and routing path. A second binary stream type would duplicate ownership and framing. |
| `http_stream_send_parts` in `crates/align_runtime/src/lib.rs` allocates a Vec and copies head, framing, prefix, payload and suffix for every send. | Binary support is present; payload-copy avoidance is not. Preserve the existing text path while improving this shared writer. |
| `HttpStream` owns an fd, lazy head and poison bit. `finish` consumes; Drop only closes. | Keep one lifetime and error model for text, PCM and encoded media. |
| `align_rt_http_accept`, `http_read_request` and `http_send_all` can block; `pkg.web.serve` has a finite explicit worker count. | A long stream occupies a worker. Slow peers and partial requests need bounded I/O; an unbounded number of progress subscribers cannot be advertised. |
| `std.http` already has client raw/SSE streaming into caller-owned buffers, inbound body caps and protocol-neutral Upgrade. | Reuse them; a server copy fix does not imply a zero-copy client/proxy or a streaming request-body API. |
| Plans 17, 49 and 50 provide opaque resources, native FFI, live byte capture, finite process polls and Linux exclusive child scopes. | Engine lifetime and cancellation have usable owners today. No model-specific runtime class is needed. |
| `std.fs` has exclusive creation/publication, no-follow rooted reads, bounded positional reads and explicit file/writer sync. | Completed artifacts can stay file-backed; a video need not become one giant owned byte array or base64 JSON string. File sync is not parent-directory durability. |

## H1: one borrowed-byte stream writer

Use one scatter/gather writer for both existing send operations. Its ordered
segments are pending response head, chunk-size line, event prefix, caller data,
event suffix, and chunk terminator CRLF: at most six segments. Omit the chunk
segments in HTTP/1.0 mode and the event segments for plain send. Encode the
hex length into fixed stack scratch. The already-owned response head remains
alive until its write attempt ends; caller bytes remain borrowed until return.

Check pointer/length admission and checked payload/wire-length arithmetic
before taking the pending head or forming native I/O vectors. Preserve the
existing native empty sentinel (`null` or nonpositive length denotes empty);
reject an unrepresentable nonempty range or wire length as Invalid before I/O.
The validity of a nonempty foreign allocation remains the caller's unsafe
obligation; the runtime cannot validate arbitrary addresses. Input is bytes:
no UTF-8 scan, NUL-terminated conversion,
escaping, payload retention or payload-sized temporary. HTTP header validation
is still performed at response construction. No user-controlled text is added
to a native C-string API by this change.

Advance the vector cursor by the exact number of bytes a native write accepted,
including a short write in the middle of any segment. EINTR retries the same
remaining bytes. A zero-progress nonempty write is an error, never a busy loop.
Retain Linux MSG_NOSIGNAL and checked macOS SO_NOSIGPIPE behavior. One native
vectored write is attempted for a normally writable stream; short writes may
require more. Success means all bytes were accepted by the local transport,
not that the client has received, decoded or displayed them.

| Operation / state | Required result |
| --- | --- |
| `send` with empty valid bytes, including poisoned stream | Existing unconditional Ok; no head commit, clock read, write or allocation. |
| Nonempty `send`, or any `send_event`, on poisoned stream | Existing Invalid; no I/O. |
| First valid nonempty send / first event | Take the pending head once; the write attempt commits it even if that attempt fails. |
| Successful send/event | Return only after all borrowed bytes are consumed; caller may then reuse/drop the source buffer. |
| Failed send/event | Latch poison; no retry from the beginning and no later clean terminator. |
| `finish` / `reject` / Drop | Existing consume/null/close behavior. `reject` retains its pre-first-send window; Drop never writes. |

The precise resource promise is zero payload copies and zero allocation calls
inside the send framing/writer after stream construction, for ordinary
plaintext HTTP sends. Head construction, application JSON encoding, file
reads, OS socket copies, IPC, GPU-to-host transfer and TLS are outside that
promise. There is no claim of end-to-end kernel, TLS or GPU zero-copy. Future
server TLS must disclose its own buffering rather than inheriting this claim.

HTTP chunks are transfer framing, not audio/frame message boundaries. Receivers
must reconstruct the payload stream even when intermediaries rechunk it.
SSE remains UTF-8 text; arbitrary audio/video uses the binary body path.

## H2/H3: explicit bounded waits

Both proposed setters take a bound local receiver and one `i64`; they return
`Result<(), Error>`, borrow the owner exclusively, and are Impure. Evaluate
arguments once in source order. Negative budget is Invalid before receiver
state inspection; then reject null/spent/poisoned native state, before mutation.
A setter failure preserves the previous value. Zero disables the bound;
`1..=i64::MAX` nanoseconds is valid. No ambient timeout, environment variable,
timer thread, allocation, native socket-option change or I/O occurs in a setter.

Use the existing `MonotonicTimeoutBudget` and positive ceil/saturating poll
conversion. Keep start and duration separate; a huge positive value must not
overflow an absolute clock and become unbounded. Snapshot once per operation,
not once per syscall. Zero mode performs no deadline clock reads. A syscall
or scheduling delay can finish after the logical deadline; this is a bounded
wait policy, not a hard real-time guarantee.

### Stream writes

For positive mode, validate inputs, snapshot the budget, then commit the lazy
head at the same valid first-write attempt as H1. Linux uses per-call nonblocking
`sendmsg` flags. macOS uses a checked, once-latched O_NONBLOCK setup on the
stream's exclusively owned socket, with no descriptor alias published by Align.
Both paths use readiness waits so no individual write can bypass the budget.
Retain the platform SIGPIPE policy. A never-configured stream keeps H1's ordinary
blocking operation. A latched macOS stream later reset to zero uses indefinite
readiness waits without deadline clocks; Drop closes it without restoring a
socket that cannot return to another owner. Check the
remaining budget before every I/O/wait and after its return before accepting
the native result; expiration wins at that observation point, including over
simultaneous native success/error. EINTR, EAGAIN and short writes use the same
budget. Native poll wakeups recheck the clock; no final zero-timeout probe is
issued after expiration. Native hard errors otherwise use the existing mapping.

Timeout can occur after a prefix reached the peer. `send`/`send_event` therefore
return Timeout and poison, even if no payload byte was accepted. A later send
returns Invalid, as existing poison does; finish closes and returns Invalid.
Timeout during finish/reject returns Timeout and consumes/closes the stream
and, for reject, the response builder. Validation errors in reject retain its
existing consuming behavior. Drop remains close-only and never waits to flush.

The budget covers each complete send, finish or reject write, including head
and framing. It does not cap time spent generating the next token or audio
block. Generation deadlines and cancellation are caller policy.

An unpoisoned HTTP/1.0 stream whose head has already been sent has no bytes
remaining at finish. Preserve its close-only success without starting or checking
a write budget, even in positive mode. Empty send has the same clock-free rule;
empty send_event still writes an event and uses the budget.

### Request acquisition

At `srv.accept()` entry, snapshot the body cap and timeout. The positive budget
covers listener/parked-peer selection, connection admission, complete header
and body reads, retryable accept errors, malformed-peer retries and descriptor
pressure backoff. Poll readiness must not be followed by an unbounded blocking
accept/read. On the first positive-budget accept, use checked F_GETFL/F_SETFL
to put this server's listener in nonblocking mode, retaining all other flags.
Keep that mode for its remaining lifetime. Record successful setup before
checking expiration; a later zero budget uses indefinite readiness waits,
without deadline clock reads. Never-configured servers retain the original
blocking-listener path. A setup error returns the mapped native error (or
Timeout if the budget has expired) without publishing a context. A failed
F_SETFL does not update the recorded mode. The listener is not closed on Timeout.

This is owner-local state: `serve_shared` creates a separate SO_REUSEPORT socket
per server, not duplicated descriptors for one open file description. The
exclusive receiver prevents overlapping accept/setup on the same server; no
connection-global or process-global mode is changed by another owner. For
selected fresh or parked peers use per-call nonblocking recv, preserving their
descriptor flags. Fresh descriptors must undergo checked blocking-mode setup
before publication (some platforms inherit listener flags); failure closes that
descriptor once. Thus existing response/Upgrade operations see their required
blocking descriptor. No fallible listener-mode restoration is deferred to Drop.

Check before each blocking opportunity and after each native result as for H2;
before publishing a complete context, check once more. A parsed explicit body
cap refusal recognized while budget remains is Invalid. Existing malformed
input is closed/skipped only while budget remains. On expiration, close and
discard any selected but unpublished context, return Timeout, and leave the
listener plus unselected parked peers usable. Do not discard unrelated parked
peers merely for a timeout; existing explicit fd-pressure reclamation can have
already evicted idle peers. Output slots are null on every failure.

Timed-out partial input is not resumable by the next accept call. This keeps
the existing complete-request API and its bounded body ownership. The caller
handles Timeout and polls other owned work before the next accept. This is not
a general event reactor or streaming-upload API. A control server can explicitly
select a small body cap and short request budget; ordinary `pkg.web.serve`
retains its existing defaults until a separate actual package configuration
requirement exists. Each shared-listener worker owns its own configured server.

## Model resources and the single-GPU sequence

Concrete engines belong in packages. A package may expose a named opaque Move
resource whose functions use explicit borrowed receivers, concrete config
records and Result. Static function calls or an ordinary application sum select
an engine; no trait, dynamic compiler registration or universal tensor type is
necessary. A native wrapper's validated host output view must remain tied to
its resource owner; it is not a view of a device-only pointer.

The lifecycle requirement is stronger than a method named `unload`:

| State / operation | Required composition rule |
| --- | --- |
| Idle → Loading | One controller owns device admission. Validate configuration and capacity policy before native side effects. No second managed model load while admission is owned. |
| Loading → Ready | Publish readiness only after model/session/workspace initialization and any required native synchronization succeed. |
| Ready ↔ Running | Generation borrows the engine owner; concurrent unload/replacement is excluded. Output views expire before their backing is reused or freed. |
| Ready → Offloaded | Only when a concrete engine supports explicit CPU retention. Report retained RAM and any device allocations honestly; offload is not unload or device release. |
| Ready/Offloaded → Releasing → Idle | End work, synchronize device operations, release model/KV/workspaces/caches and prove the managed release boundary. Only then transfer admission to the next engine. |
| Failed load, cancellation or release | Roll back acquired state or retain an owned Faulted state. Never mark Idle just because an error was returned, a root exited, or a metric read as zero. |

For engines whose in-process teardown cannot establish that boundary, use an
explicit worker process. `process.child_scope` is the preferred Linux managed
descendant owner when native engines may create children. Observe/terminate,
drain bounded stdout/stderr, reap and obtain successful scope release before
starting the next managed GPU worker. One scope excludes other child launches
in its supervising process; release it before launching the next GPU engine
or FFmpeg. Common direct-child control alone cannot certify escaped descendants.
WSL2 must pass the existing scope feature probe; unsupported acquisition is
an error, not permission to weaken the release proof.

This admission covers cooperating managed engines, not every external program
on the GPU. Supervisor crash does not prove its children died. Recovery starts
Faulted until remaining owners are reconciled; unknown device usage is not free
capacity. Do not introduce a hidden keeper, global device reset, implicit
offload, automatic model fallback or automatic retry of a possibly running job.

Memory observations are ordinary package records: sample timestamp plus optional
nonnegative byte counts for host resident usage, device live allocations,
allocator-reserved storage and device-wide free/total memory. Each value names
its scope and producer. `None` means unavailable, never zero. Process maximum
RSS is a peak and cannot stand in for current RAM. WSL's NVML limitations make
per-process GPU observations optional. Cached telemetry is diagnostic, not
the device-admission lock or a release certificate.

An admin unload request is a control-plane operation through that owner. During
generation it either reports busy or explicitly requests cancel-and-release,
according to the application's documented policy. It never frees memory behind
a live inference call. Long load/unload operations can themselves have a job
receipt. Their endpoint names and authentication belong to the serving app.

## Long jobs without a second concurrency model

The first useful composition is one explicit state owner plus a bounded queue,
not a generic framework installed in the compiler. Ordinary records hold IDs,
state, input/artifact references and progress. Owned arrays/records carry queue
storage; no job retains a request-arena view after the HTTP handler returns.
Input retention requires an explicit owned copy or an owned artifact reference.
No borrowed buffer is silently queued for use after a send returns.

| Event | State and ownership rule |
| --- | --- |
| Submit | Validate complete input and all configured bounds before admission; reserve a queue slot and input owner, then return a queued receipt. Full capacity rejects before launching work. |
| Start | FIFO selection among admitted runnable jobs in the initial serial policy; one running GPU job. CPU-only work is parallel only under an explicit task_group and capacity choice. |
| Progress | Monotonic completed work within a named phase. Total may be None. A phase change can reset its counter; do not invent an overall percentage from elapsed time. |
| Cancel queued | Remove from runnable queue, release input ownership, publish Cancelled. |
| Cancel running | Enter Cancelling and request cooperative stop; on its explicit grace deadline, signal the owned process. Retain admission until cleanup/release succeeds. |
| Complete / fail | Publish terminal state once output validation/publication and required cleanup finish. A failed release keeps the device Faulted, even when an artifact was produced. |
| Cancel/complete race | The single owner serializes events. Cancellation accepted first prevents later success publication; a terminal result accepted first is immutable. |
| HTTP disconnect | Ends that observer, not an admitted batch job. A live speech session may choose cancel-on-disconnect explicitly; batch cancellation is a separate operation. |
| Restart | Initial reference queue is process-local, with no restart durability or automatic retry. A new controller epoch invalidates old receipts; interrupted work is reconciled before new device admission. |

Queue slots, retained terminal records, request bytes, event history, diagnostic
bytes, artifact bytes and subscriber count all have explicit finite bounds.
Event loss from a bounded history is reported as a gap with a fresh snapshot;
an old resume cursor must not silently look complete. Use a controller epoch
and monotonic sequence, refusing counter exhaustion rather than wrapping IDs.
Completed results persist only under an explicit artifact-retention policy.

The controller alternates finite `accept` budgets with nonblocking process
observation and bounded pipe draining. Slow progress subscribers are served by
separate, explicitly sized request workers; they query immutable snapshots
through a documented control boundary rather than sharing mutable queue state.
Reserve control capacity so open streams cannot prevent cancellation or health
requests. A controller never spends minutes inside an HTTP handler doing inference.

The reference uses directly configured `std.http` servers. Every controller
success, error and rejection reply uses a stream with a positive H2 budget,
including finish/reject; on failed configuration or write, close that output
owner. H3 does not bound ordinary `ctx.respond()`, so that unbounded operation
is not used in the supervision loop. Limit reply construction and bytes as well
as socket wait. A stalled control reader must time out while the controller
continues process observation, pipe draining and cancellation.

The sample serving shape may use submit/status/cancel/events/artifact routes
(including the suggested `/v1/jobs/...` names), with a 202 queued receipt, SSE
progress and binary artifact retrieval. These are application routes, not
`std.http` builtins or an OpenAI compatibility claim. SSE IDs and metadata are
encoded as text; binary payloads use the separate stream. Final wire schemas,
authorization and persistence must be specified by the adopting application.

For the reference consumer, a fixed serial pipeline and one owner are sufficient.
Extract a `pkg.jobs` API only after independent text and media consumers show the
same queue, cancellation, persistence and failure contract. Do not publish
dormant queue metadata or force the existing text SSE path through job polling,
database access, a new proxy hop or per-token engine dispatch.

## Artifacts, audio and timing

Large outputs live in explicitly owned files. Publish only after the writer is
finished, the media container is finalized and validation succeeds. Use exclusive
temporary creation and no-replace publication; cleanup removes only artifacts
owned by that job. Serve through opaque application IDs resolved under a retained
or no-follow artifact root. Never return arbitrary server filesystem paths to a
remote client or reopen a client-selected path as an artifact.

A descriptor carries media type, exact byte length when complete, integrity
digest if computed, and application identity. Its byte owner is separate from
JSON metadata. A file descriptor/read lease keeps a download usable while its
directory entry is retired; unlink/retention policy must not invalidate a live
borrowed view. File and database durability are distinct choices. Existing
file sync plus rename does not establish durable directory publication after
power loss; no such promise is introduced here.

The reference artifact sink is parent-owned: a mock worker emits artifact bytes
through its captured stdout pipe, and the controller reads into fixed scratch
and writes only within the job's reserved byte allowance. Diagnostic stderr has
a separate cap. An extra output byte fails the job and starts owned cancellation;
never publish truncated output as success. Neither `stdout_to` nor a live
command's capture settings supply this bound. The reference worker receives no
direct artifact file descriptor/path. Bound each drain/write batch so output
cannot monopolize the controller; regular-file I/O still has no hard real-time
latency guarantee.

Reserve capacity before admitting production, and charge staging, published
and retired-but-open artifact bytes against it. Retirement cannot release that
reservation until the last managed read lease closes. Disk-full/write errors
fail the job and clean up its staging owner. A concrete engine that writes
files directly must provide an enforced bounded writer or a separately configured
OS storage limit before claiming the same hard byte bound; periodic size checks
or publication-time rejection alone do not enforce it. No new filesystem-quota
builtin is implied by this reference composition.

| Output | Composition |
| --- | --- |
| Text | Existing token SSE route and framing. Keep the existing API and direct hot path. |
| Live PCM | Explicit format, sample rate, channel count, interleaving and endianness; borrowed host byte blocks and bounded buffering. HTTP chunk boundaries carry no sample/segment meaning. |
| WAV | Prefer a completed artifact when final sizes are needed. Do not label an arbitrary sequence of PCM chunks `audio/wav` without a valid container strategy. |
| Images / completed video | Binary artifact retrieval; bounded file reads, then borrowed sends. No whole-video allocation or base64 envelope. |
| Incrementally playable MP4 | A container/codec choice such as fragmented MP4, verified with the actual receiver. Merely setting `video/mp4` does not make unfinished output playable. |
| Alignment | Typed metadata plus an artifact/stream identity. Use a sidecar JSON result or progress channel; potentially large late metadata does not belong in response headers. Multipart is optional only after a concrete codec contract, not a core requirement. |

Use integer sample-frame positions as the audio authority: one sample frame
contains all channels. Intervals are half-open `[start, end)`, nonnegative and
bounded by the emitted sample-frame count. Rate is a positive integer. Labels
are ordinary owned UTF-8 strings when retained. Word/phoneme/breath/silence
categories are a package/application sum; unsupported alignment is None.
Do not fabricate timings from text length or from HTTP arrival time.

Each alignment set names its audio artifact, normalized transcript, sample
rate, channel layout and producer/method. If source-text offsets are supplied,
name the exact transcript and use validated UTF-8 byte boundaries. Different
tracks may overlap; entries within one ordered token track have nondecreasing
starts, valid ends and deterministic ordinal order. Resampling, inserted
silence and concatenation transform the positions explicitly before publishing
the final sidecar. Convert to milliseconds only at an output boundary, with
checked integer arithmetic and an explicit rounding rule; do not accumulate
rounded milliseconds across chunks. Video uses explicit rational time bases
and presentation timestamps, not an assumed integer frame rate.

The inspected Style-Bert-VITS2 API returns a completed sample-rate/audio pair
encoded as WAV. It is not evidence that streaming generation, whisper/breath
control, or phoneme timestamps are available from every chosen model. Those
are adapter capabilities to measure. Align should carry their results faithfully
without baking an unverified model promise into its types.

## Computation: add only a demonstrated missing primitive

The dominant matrix/attention/convolution/diffusion work initially belongs to
the selected mature native/GPU engine, through explicit FFI or a process.
Align owns orchestration and can own measured preprocessing/postprocessing.
There is no new GPU codegen, autodiff system, tensor DSL, f16/bf16 arithmetic
family or FFT builtin in this plan.

| Possible bottleneck | Existing first choice | Trigger for a further Align capability |
| --- | --- | --- |
| PCM scaling, mixing, channel interleave/deinterleave, simple image normalization | Contiguous primitive arrays/slices, existing loops/pipelines/SIMD; explicit checked sizes and layout conversion | A small ordinary source kernel remains scalar or repeatedly checks/copies despite an expressible proof, and costs a measured material share of the stage. Fix that compiler/library boundary. |
| FFT/STFT, high-quality resampling, complex image resize/color conversion and codecs | A concrete pkg wrapper around a mature native implementation or FFmpeg; reuse opaque resource owners | Two real callers need the same exact operation/format/ownership contract, or FFI marshaling is demonstrably the bottleneck. Then design that wrapper, not a language-wide operator family. |
| Host ↔ device movement | Engine-owned workspaces and explicit transfers; existing typed byte views where the backing is valid host memory | A profile attributes material time/copies to pageable staging and a concrete backend requires pinned/aligned host storage. Specify one explicit allocation owner, bounds, completion and Drop contract before adding it. WSL pinned-memory limits remain real. |
| Async device work | Keep buffers inside the concrete session owner; synchronize before publishing/reusing host output, or use a proven dependent operation owner | A required overlap cannot be expressed safely with existing resource dependencies. Record the exact lifetime counterexample; no guessed escape/alias exemption. |
| Strided tensor/image layouts | Ordinary shape/stride records plus flat storage; explicit views or materialization, checked dimension arithmetic | Demonstrated code duplication or extra materialization across real consumers, with an exact reusable primitive and no hidden copy. |

Benchmark complete stages as well as kernels: load/unload, transfer, preprocessing,
inference, encode/mux and HTTP output. Record model/weight/tokenizer/codec versions,
quantization/dtype, shapes, steps, seed, device/driver and explicit offload policy.
These are consumer provenance inputs, not a new compiler cache format. A seed
does not promise byte-identical output across engines/hardware.

Do not infer that a model fits from its family name or nominal parameter count.
The exact checkpoint, quantization, context/KV size, image/video shape and
workspace peak must fit the configured host/device budgets. Large system RAM
permits explicit offload; it does not remove transfer cost or make VRAM shared
memory. No model/download/driver change is part of this design task.

## Implementation closure matrix

| Axis | Implementation boundary and exact owner closure |
| --- | --- |
| Formation, validation, malformed input | H2/H3 method arity/type, bound local and exclusive receiver in sema; checked-HIR twins reject forged receiver/value/result/effect records. H1 native input/overflow negatives run before head transfer. Owners: existing `hir_body_validator_native`, new cases in `m12_http_stream` and `m11_http_server`, runtime `http_stream_vectored_*` / `http_server_accept_budget_*`. |
| Construction, move-in/out, nulling, Drop, replacement, return | No new handle/carrier. Setter never moves; send borrows; respond_stream/finish/reject keep existing nulling. Existing `http_stream_nameable` / `m12_http_stream` owners plus injected first-write/finish/reject failures prove exactly-once close and no retained source view. |
| if/match/else/?/map_err, joins, loops, early exits | Existing handle rules remain; new setter Result paths and timeout-then-next-accept loops get whole/per-unit source cases. Diverging eager operands do not call setters/native I/O. Preserve original Error and cleanup on early exit. |
| H1 segment geometry | One parameterized native owner crosses HTTP 1.0/1.1, head pending/committed, plain/event, empty/nonempty and partial transfer at every segment boundary; verify actual payload pointers and allocator counters, not merely concatenated output. |
| H2 budgets and poison | Controlled clock/syscall owner crosses zero/positive/max budget, before/after partial progress, EINTR/EAGAIN/hard error, first/later send, empty send/event and finish/reject. Check Timeout precedence, poison, no final zero probe and clock-free close-only raw finish. Real stalled-reader and disconnected-peer controls on Linux/macOS. |
| H3 budgets and request ownership | Fresh/parked/no peer, header/body split, exact/over body cap, malformed floods, transient accept errors, readiness races and fd-pressure backoff. Timeout before/after selection and before publication preserves unselected fds and releases all unpublished storage. Real partial client then healthy client proves continued use. |
| Generic/interface/whole and per-unit | New operations participate in type/effect validation, all IR walks, cloning, producer checks and codec rejection. Existing generic/imported helper patterns cover both setters and borrowed stream payload. Interface edit/revert and native declarations agree; no new type tag. |
| Runtime provenance / allocation | Borrow caller bytes only for the send; no opaque-owner mutability exemption and no K1 dependency. H1 allocator/pointer assertions and ordinary/alloc-count runtime ABI/export parity; H2/H3 scalar fields introduce no additional owner. |
| Native ABI and backend lowering | Two proposed keys `HttpStreamWriteTimeoutNs` / `HttpServerAcceptTimeoutNs`, symbols `align_rt_http_stream_write_timeout_ns` / `align_rt_http_server_accept_timeout_ns`, both A04 `i32(ptr, i64)`. Conservative HostState effect records; no purity/capture attribute inferred from the name. Exact declarations and export owner. |
| OS state and overlap | Deadline state is per owner, not global. Checked latched listener-mode setup, timeout immediately after successful setup, later zero mode, accepted-fd blocking setup and failure cleanup are native owners. Linux per-call send and timed recv flags never mutate shared state; macOS stream setup mutates only its sole owned connection. SO_REUSEPORT servers have distinct listeners. Existing child_scope exclusivity and signal/reap semantics are reused by the reference, not modified. |
| Application composition | Mock text/audio/batch workers, one device-admission witness, delayed/failed release, cancel-before/after completion, full queue, subscriber gaps/disconnect, stalled control reader, oversized pipe output, retired-but-open download accounting, invalid artifact path and restart epoch. Model-quality/real GPU tests remain external adapter acceptance. |
| Resource/performance promise | Only H1 promises reduced copy/allocation work. The benchmark below and structural owners close it; other rows have correctness/finite-bound owners, not speculative speed claims. |

H2/H3 are new operations across compiler layers. Follow the existing exhaustive
variant machinery rather than adding catch-all arms. Inspect the actual interface
representation before assigning tags or changing its version. These operations
are reconstructed from rechecked source bodies under format 17, so their runtime
keys add no encoded HIR tags or new canonical layout. Existing malformed-codec
owners and imported whole/per-unit source owners cover that unchanged boundary.

### H1 implementation closure

`http_stream_bytes` validates the native extent before slice formation and head
transfer. `http_stream_send_parts_with` coalesces only fixed framing bytes on
stack and retains the borrowed caller payload in at most four vectors;
`http_stream_write_parts` advances exact accepted prefixes. Linux uses sendmsg
with MSG_NOSIGNAL; macOS blocking output uses writev on the accepted socket with
checked SO_NOSIGPIPE. There is one geometry and poison policy for text and binary.

`crates/align_runtime/src/http_stream_tests.rs` owns every partial prefix,
source-pointer and allocation parity, zero/error/impossible-count refusal,
pre-commit extent rejection, arbitrary native octets and caller buffer reuse.
Existing runtime lazy-head/finish/reject tests and the driver `m12_http_stream`
and `apps_web_stream` targets own the unchanged language/framework path.
No IR shape, ownership summary, native symbol or interface format changes.
The H3/C1 rows are explicitly deferred to their complete capabilities.
The writer, discriminating native owners and standalone performance/resource
harness are one proof boundary: splitting the harness would publish the copy
claim before its token regression and allocation controls exist.

### H2 implementation boundary

H2 adds `ExprKind::HttpStreamWriteTimeoutNs { stream, timeout_ns }` and the
matching MIR rvalue, runtime key and A04 status call. The receiver remains a
bound exclusive local; argument evaluation completes once before mutation.
Every depth/clone/effect/escape/move/provenance walk must follow both operands,
and checked HIR must independently check the receiver, signed i64 argument,
unit/Error result and Impure effect. Setter success/failure never nulls a source.
Existing finish/reject cleanup still consumes once on every Result exit.

The interface carries rechecked source bodies and existing signature/effect
records, not encoded HIR operation tags. H2 therefore allocates no type tag,
changes no canonical byte layout, and keeps interface format 17. Imported and
generic whole/per-unit owners exercise source-body reconstruction and malformed
interface rejection under that unchanged codec. The runtime ABI ledger and
native declaration/export goldens gain exactly one A04 operation.

The runtime stores `write_timeout_ns: i64` (initially zero). One statically
dispatched writer seam supplies send, poll and remaining-budget observations.
Linux timed sends use per-call MSG_DONTWAIT. macOS first latches O_NONBLOCK
with checked F_GETFL/F_SETFL on its sole owned connection; subsequent writes use
writev and readiness waits. Setup is part of the same budget: check before/after,
record successful mutation before selecting Timeout, and poison on setup failure.
A failed F_SETFL is not assumed atomic: no later I/O occurs and the sole owned
fd closes through Drop/consuming cleanup; no fallible restoration is attempted.
No fd alias is published by the stream, and it never returns to the context/pool.
Zero on an unlatched stream retains H1 blocking output with no clock observation;
zero after macOS latching retries EAGAIN via poll(-1), including EINTR, with no
budget/clock and no flag restoration. Close-only finish and empty send do not
latch or poll. The production test uses a TCP peer, with an additional Unix-socket
native control for the same platform mode transition.
Finish/reject pass their complete serialized bytes through the same bounded
writer. Native parameterized owners close the budget/result/partial/wait cross
product, setter preservation and consuming cleanup; source owners close argument
order, Result control exits, borrowed receiver rejection and whole/per-unit parity.
Owner commands: `scripts/cargo.sh test -p align_runtime --features alloc-count
--lib http_stream_`; `scripts/cargo.sh test -p align_driver --test
http_stream_write_budget --test m12_http_stream`; `scripts/cargo.sh test -p
align_mir --lib hir_body_validator_native`; `scripts/cargo.sh test -p
align_codegen_llvm --lib runtime_abi`; `scripts/test-runtime-abi-exports.sh`.
The native mode/result cross product supplies negative and multi-invalid
precedence evidence, including F_SETFL mutation followed by failure.

This is one complete source-to-native capability, likely over 1,000 handwritten
lines once discriminating owners and synchronized public prose are included.
Splitting compiler producers, native consumers or the finite-write acceptance
would leave a dormant chain and duplicate its ownership/ABI proof. No new handle,
parallel model or default finite timeout is introduced.

H2 platform correction (2026-10-04): the first local macOS stalled-peer owner
blocked inside sendmsg despite MSG_DONTWAIT. A bounded Python syscall reproduction
confirmed both TCP and Unix-stream behavior, and the process sample is retained
outside the worktree. Apple's public kernel `sosendcheck` checks MSG_NBIO for a
full send buffer; that private flag is not adopted as a user-space contract.
The owner-local latch above replaces the invalid per-call assumption. This is a
platform implementation strategy change, requiring a fresh independent review of
this changed matrix before implementation, not a reopening of the source API.

| Changed H2 cell | Required owner |
| --- | --- |
| Sole fd authority | respond_stream transfers ctx.fd to -1; stream never parks, returns or publishes its fd. Existing ctx/view and consuming stream owners; explicit source ownership prose. |
| Setup before/after expiry | Inject F_GETFL/F_SETFL failure and success followed by expiry; success latches before Timeout, failure leaves native mode indeterminate and poisons, no send follows failure/expiry. |
| Subsequent positive/zero | One successful setup across repeated sends; zero reset uses indefinitely blocking poll, retries EINTR/EAGAIN, observes no clock, and keeps SIGPIPE policy. Never-configured zero remains H1 path. |
| Terminal paths | Timed send failure poisons; finish/reject consumes exactly once. Empty send and raw committed finish avoid setup, poll and clocks. Drop closes the sole fd without restoration. |
| Platform native acceptance | Real stalled TCP/Unix reader times out on macOS/Linux; macOS configured-zero sends complete with a draining peer; default token regression comparisons remain required. |


### H2/H3 eager receiver reservation repair

The H2 independent code review found that loading a non-borrow-bearing native
handle before its scalar argument does not by itself preserve that handle.
An argument can consume or replace the bound owner before the native action.
This is a finite, call-local repair for the two new setters, not an expansion
of the deferred K1 interprocedural view work.

| Changed cell | Exact strategy and owner |
| --- | --- |
| Receiver completion | Register only the receiver expression of `HttpStreamWriteTimeoutNs` and `HttpServerAcceptTimeoutNs` in `prepare_mutable_call_snapshots`, in both the argument and mutable-place sets. The existing completion path records its exact local place despite having no storage header or borrowed leaves. No type-wide snapshot rule is added. |
| Later eager invalidation | Existing move, Drop and assignment invalidation marks that reservation; validation at the enclosing setter rejects consumption and replacement before MIR/native execution. Source-order evaluation stays unchanged. Whole/per-unit owners cross direct move, direct replacement, nested expression, branch and loop joins. |
| Successful action and retirement | Reuse a dedicated exact receiver selector in preparation and `retire_builtin_action_input`. Validate children first, then retire only that setter receiver's snapshot after the successful action boundary. A different enclosing operation's reservation remains live. The setter result is unit/Error and retains no handle. |
| Noninvalidating work | Ordinary scalar work, mutation of the same owner's deadline field, independent-owner destruction and a setter followed by consumption remain valid. Whole/per-unit positive controls distinguish call-scoped protection from a permanent borrow. |
| Terminating argument | Return, propagated error or loop exit that prevents the setter action does not validate an action that never occurs. Cleanup may consume the owner on that terminal path. MIR owners verify that no setter call is emitted for unconditional termination; branch owners validate only paths that reach it. |
| Interfaces and runtime | Existing source reconstruction rechecks these reservations in whole/per-unit compilation. No new HIR variant, native ABI, runtime state, ownership summary, allocation or interface format is introduced by this repair. |

A fresh inspection-only independent review of this changed safety strategy
and its receiver lifetime boundaries completed clean before implementation. After the
P1 repair, review the revised complete H2 candidate once under the repository's
P1 redesign rule. H3 applies the same explicit receiver registration and owners
before its first complete candidate review.

## Performance acceptance

H1 must preserve the existing text/SSE route and use the same implementation
for binary sends. A candidate does not qualify merely because bulk media wins.
Use identical compiler/runtime profiles, linker, host and receiver setup; compare
baseline and candidate in ten order-balanced pairs after warmup. Cover small
token payloads (including empty and multibyte cases), 4 KiB and 256 KiB blocks,
first-send and steady-state, HTTP 1.0/1.1 and plain/event frames. Record bytes,
allocation calls, payload copies, syscalls/short writes, p50/p95 send latency,
time to first received payload and sustained throughput.

Zero payload copies and zero framing allocations are exact structural gates.
Text throughput/latency must not regress outside the observed baseline-to-baseline
repeatability envelope; repeatability is measured with the same paired procedure,
not a retrospectively chosen tolerance. If the comparison is too noisy to resolve,
improve the measurement instead of declaring a win. A reproducible text regression
blocks H1 adoption; do not hide it behind a size threshold with two semantically
different stream implementations. Device/kernel/IPC copies are reported separately.

The benchmark is a local performance acceptance, not a broad correctness gate.
Provider mock fixtures protect wire bytes and transport costs. Actual align-llm
token latency and voice first-audio latency need later consumer-side measurements;
this task neither runs nor modifies those repositories' code.

H1 measured on native Apple Silicon with Rust 1.96.1, the default release profile
and identical locked dependencies against the plan baseline. The first candidate
regressed some token rows; fixed framing coalescing and the macOS blocking writev
operation closed that result. The final idle-host run used ten balanced baseline
repeatability pairs and ten balanced baseline/candidate pairs. No token metric
regressed outside its premeasured envelope. Token p50 ratios ranged 0.945–1.037;
256 KiB p50 ratios ranged 0.692–0.816, with wire throughput ratios 1.012–1.049.
These are local transport measurements, not model inference or first-audio claims.
Separate counting binaries observed one allocation per nonempty baseline send
and zero candidate send allocations, including empty SSE events. Pointer owners
prove that caller payloads are not copied; scripted owners count actual native
attempts and every partial prefix. Real kernel syscall counts require OS tracing.
Raw CSVs and binary hashes are retained under the session's
`align-http-stream-timing-20261004-idle` evidence directory; the resource CSVs
remain in `align-http-stream-timing-20261004-v2`. `bench/http_stream/compare.py`
can summarize completed runs without repeating their measurements.

H2 default-zero transport comparison against H1 used the same native release
harness and paired procedure. No token metric fell outside the independently
measured baseline envelope; token p50 ratios ranged 0.992–1.008. Separate
configured-budget allocation measurement observed zero send allocations in all
20 cases. Configured timing includes the first owner-local mode transition and
every deadline checkpoint; its raw results are retained separately, without
claiming that an explicitly bounded write has no clock/setup overhead. Evidence
directories are `align-http-write-budget-timing-20261004-default` and
`align-http-write-budget-timing-20261004-configured`; the separate allocation CSV
is `align-http-write-budget-allocations-configured.csv`.

## Delivery and file map

| Capability | Concrete files expected to change | Useful completion boundary |
| --- | --- | --- |
| 1. H1 borrowed vectored stream output | `crates/align_runtime/src/lib.rs`; `crates/align_driver/tests/m12_http_stream.rs`, `apps_web_stream.rs`; a focused benchmark under `bench/`; the H1 source documents in the ledger | Existing binary and SSE callers get the complete copy/allocation improvement together. No dormant producer or new public source operation. |
| 2. H2 bounded stream writes | `crates/align_sema/src/lib.rs`, `hir.rs`, `hir_depth.rs`, `replay_clone.rs`; `crates/align_mir/src/lib.rs`, `validate_hir.rs`, `producer.rs`, `runtime_key.rs`, `print.rs`; unchanged format-17 imported-source owner; `crates/align_codegen_llvm/src/lib.rs`, `runtime_abi.rs`, ABI golden; runtime and stream owners; ledger-listed normative documents/mirrors | A slow reader can no longer hold a caller indefinitely once that caller explicitly configures the budget. All source/interface/native consumers land together. |
| 3. H3 bounded complete-request acquisition | The same setter/codec/ABI owner files, runtime accept/read/poll owners, `m11_http_server.rs`, native timeout owners; ledger-listed normative documents/mirrors | A controller can perform finite accept/supervision cycles and remain responsive to failed or partial requests. Useful without an inference engine. |
| 4. C1 reference composition | Planned `examples/jobs/` or a focused `apps/` example with mock worker sources, one driver owner and English/Japanese usage guide | Runnable bounded submit/observe/cancel/artifact flow with explicit process ownership; separate text, live binary and batch fixtures. Final example paths are selected before that capability starts. |
| 5. Concrete engine / media packages | Package design and native wrapper files chosen from actual voice/media consumers; no speculative compiler edits | Each wrapper carries complete ownership, supported formats/cancellation/release and a real consumer. Add a shared jobs or media API only when this evidence justifies it. |

Capabilities 2 and 3 isolate different failure domains and each has a useful
consumer without the other. Each still ships its entire source-to-native chain.
If a capability is expected to exceed 1,000 handwritten lines, explain its proof
boundary before coding; do not split IR producers from consumers to meet a line
target. Run only the applicable owners and normal repository preflight. No DB
service gate or GPU benchmark is required for an unrelated HTTP change.

H1 preserves source behavior and updates the HTTP and pkg.web English/Japanese
writer resource records. The language specification and ABI inventory retain
their existing operations. H2 adds its explicit setter and runtime ABI row across
the ledger-listed specification set. H3 retains its proposed implementation-time
documentation set. The external align-llm register receives an answer,
left uncommitted; consumer code, fixtures, branches and adoption stay untouched.

## External evidence and limits

- [HTTP/1.1 chunked transfer coding, RFC 9112 §7.1](https://www.rfc-editor.org/rfc/rfc9112.html#name-chunked-transfer-coding): body transport framing is separate from media framing.
- [WHATWG server-sent events](https://html.spec.whatwg.org/multipage/server-sent-events.html): event streams use UTF-8 text and define resume/event fields; binary media requires another body representation.
- [PyTorch CUDA memory management](https://docs.pytorch.org/docs/main/notes/cuda.html#memory-management): live allocations and allocator reservation differ; emptying unused cache does not free live tensors. This supports the explicit release-proof requirement, not a claim about a particular model's teardown.
- [NVIDIA CUDA on WSL](https://docs.nvidia.com/cuda/wsl-user-guide/index.html#features-not-yet-supported): some NVML queries are unavailable. Missing telemetry cannot be encoded as zero or required as the only release proof.
- [Style-Bert-VITS2 API implementation](https://github.com/litagin02/Style-Bert-VITS2/blob/master/server_fastapi.py): inspected inference returns complete audio and constructs a WAV response. Adapter streaming/alignment claims need their own evidence.
- [FLUX inference repository](https://github.com/black-forest-labs/flux) and [Wan2.1 repository](https://github.com/Wan-Video/Wan2.1): concrete variants and offload settings matter. This plan makes no 12 GB fit or generation-speed promise for an unspecified checkpoint.
- [FFmpeg MOV/MP4 muxer](https://ffmpeg.org/ffmpeg-formats.html#mov_002c-mp4_002c-ismv): fragmentation and finalization are container decisions, not HTTP content-type behavior.

Sources were inspected on 2026-10-04. Model and driver capabilities are moving
inputs; concrete package implementation pins and rechecks them.

## Design closure

The author-side ledger pass checked the new method domains, default modes,
error precedence, byte/owner lifetimes, native mode changes, allocation claims,
compiler/interface closure and implementation-time documentation set. No new
canonical wire/cache format or executable Align example is introduced here;
table signatures describe proposed operations, not presently compilable calls.
The reference composition deliberately leaves application schemas and package
APIs to their concrete consumers. H1 implementation and local transport/resource
measurements are recorded above; the remaining capabilities and real GPU
qualification are pending.

One independent inspection-only design review completed on 2026-10-04. Its two
P2 findings and one P3 clarification are closed in this document: every control
reply has an H2 budget; artifact output has a capped producer-to-file path and
live-lease accounting; close-only raw finish retains its clock-free success.
The ledger, corresponding prose and acceptance rows carry each correction.
The review confirmed the language/package/application boundary and found no
core GPU-manager promise. These are design checks, not runtime test or benchmark
results.
