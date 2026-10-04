# Bounded multimodal reference composition

Plan 90 C1 is an application written in Align using existing HTTP, process,
filesystem, JSON and structured-task facilities. H1/H2/H3 are prerequisites.
This record fixes the reference's concrete proof boundary before implementation.
It adds no language, compiler scheduler, GPU manager or canonical pkg.jobs API.
Real engine wrappers and GPU/model qualification remain consumer work.

## Public-contract ledger

All records below are application records, not new builtin types. Integers are
signed i64 unless stated otherwise. Retained text is owned UTF-8 string. Internal
job/state/phase discriminators are fixed i64 tags in scalar SoA columns; wire records use the
fixed lowercase strings below because owned JSON schemas do not admit enums.

| Surface | Exact record, inputs/defaults and semantics |
| --- | --- |
| Executable | `examples/multimodal/main.align`, with `job_model.align`, `controller.align`, `execution.align`, `observer.align`, `transport.align`, `worker.align`. `serve ROOT CONTROL_PORT OBSERVER_PORT_0 OBSERVER_PORT_1 FFMPEG` supplies an absolute existing private no-follow directory, three distinct ports in 1..65535 and an absolute FFmpeg executable. Bind loopback only. Register exactly three deferred closures in one task_group (controller and two observers), then call wait to dispatch/join all three. Require process.cpu_count()+1 >=3 before effects; each task returns i64 code0 for success or code1 for an independently reported failure. No owned task result or shared mutable capture. The outer owner reads all three results after wait before cleanup. `selftest` runs deterministic model/timing checks without processes or sockets. Internal `worker KIND ...` accepts only parent-built arguments. No ambient model, timeout, path or worker setting. |
| Admission identity | Before any managed launch, exclusively create `controller.lock` under ROOT. Existing fence or unknown preexisting entries refuse startup. The fence survives every unclean exit; remove it only after successful scope release, artifact cleanup, observer task join and reconciliation of all now-closed managed leases. A new OS-CSPRNG 128-bit epoch is 32 lowercase hex bytes. Receipts are `(epoch: string, id: i64)`; ids start at 1 and checked increment refuses exhaustion. Only one cooperating controller uses this root/device admission domain. No global exclusion of external GPU programs. |
| Submit | `POST /v1/jobs/{type}` on control port; types `text`, `audio`, `batch`, `pipeline`. Body `Submit { input: string, delay_ns: i64 }`; both fields required, duplicate declared keys rejected, unknown keys skipped by existing typed JSON. UTF-8 input <=512 bytes, no embedded NUL; delay 0..1_000_000_000 ns. Copy/write retained input explicitly before releasing ctx; no request view is queued. Return 202 `Receipt { epoch: string, id: i64, status: string }`, status `queued`. Reject malformed inputs before admission/files/launch; reject queue/table/output capacity before retaining input. |
| State and bounds | States `queued`, `running`, `cancelling`, `succeeded`, `failed`, `cancelled`. Fixed 8 scalar SoA job slots, <=4 queued, <=1 active managed scope, <=3 publicly retained terminal jobs, retiring the oldest id after completion/cancellation and before the next accept. A pinned retired slot remains charged and unavailable publicly until its last exact-id release. No silent eviction of active/pinned ownership; return 429 on exhausted admission. FIFO smallest admitted id. Reserve 1 MiB per job, with <=8 MiB managed artifact reservations; input <=512, 16 KiB metadata allowance and 4 KiB diagnostic allowance are conservatively reserved explicitly; stderr is drained and capped per phase, never retained as an unbounded string. No sidecar file is written. Scratch is fixed 4096 bytes per pipe; at most one read from each pipe and one bounded write batch per controller tick. |
| Status | `GET /v1/jobs/{epoch}/{id}` on control port. Exact wire order `epoch:string, id:i64, state:string, phase:string, completed:i64, total:Option<i64>, progress_percent:Option<i64>, sequence:i64, oldest_sequence:i64, gap:bool, events:array<EventRow>, artifacts:array<ArtifactRow>, alignment:Option<AudioAlignment>, error:Option<string>`. Required fields are always present, arrays empty when unavailable; existing owned JSON omits every None object field. Some fields remain in declaration order; no JSON null is emitted. Encoder cap 32768; diagnostics <=1024 UTF-8 bytes. |
| Progress/history | `EventRow { sequence:i64, state:string, phase:string, completed:i64, total:Option<i64>, progress_percent:Option<i64> }`, in that declaration order. Per-job checked sequence starts at 1; a fixed 16-event ring retains ascending sequence order. Phase `""`, `text`, `audio`, `batch`, `mux`; counters are nonnegative output bytes within the current phase, reset only on phase change. Total is None unless the producer's exact byte total is known. Percent is floor(completed*100/total), checked and bounded 0..100, None for absent/zero total. No elapsed-time or overall-pipeline percentage. Refuse counter exhaustion, never wrap. |
| Cancel | `POST /v1/jobs/{epoch}/{id}/cancel` on control port, empty body. Queued removal releases its input/reservation and records Cancelled. Running enters Cancelling, requests cooperative TERM, then after explicit 250 ms uses authenticated KILL/reap cleanup. Admission remains owned until scope.release returns true. Cancellation serialized before completion prevents success publication; terminal state is immutable and repeated cancel returns that state. A failed release keeps the controller Faulted and excludes all next managed launches. |
| Health/shutdown | Control `GET /health` returns `Health { epoch:string, accepting:bool, device_state:string, queued:i64, running:Option<i64>, reserved_artifact_bytes:i64, active_leases:i64 }`, in order. Device states idle/running/cancelling/faulted are distinct from job state; a failed release leaves the job nonterminal Cancelling and device Faulted. `POST /shutdown` with empty body returns202 Health and stops admission/new leases. Keep serving finite cleanup cycles through successful scope release. The controller task may return code0 only with no owned unreleased scope and after writing its bounded cleanup record. Each idle observer probes authenticated health between finite accepts with at most3 attempts of100 ms; accepting=false or three failed probes ends the loop after local reader/stream closure. wait joins all three tasks; the outer owner then reconciles now-closed leases and removes only recorded owned files/fence. A failed scope release keeps the controller Faulted and does not permit this transition. |
| Private protocol | Authenticate every `/internal/` route with `X-Reference-Control` token, before id/body work; absent/mismatched token is403. `GET /internal/jobs/{epoch}/{id}/events?after=N` returns the exact status/history record with gap semantics. `POST /internal/jobs/{epoch}/{id}/lease/{phase}` identifies observer slot0/1 via explicit `X-Observer-Slot`; repeated grant for that slot/artifact returns the same LeaseGrant even after logical retirement or stop, so lost acknowledgement cannot create another lease. Refuse only genuinely new retired/stopping grants. A malformed successful grant or a non-success after an ambiguous transport attempt ends that observer with its charge retained; an acknowledged ordinary4xx without ambiguity is a client rejection. A different active artifact in that slot is409. `POST /internal/slots/{slot}/release/{lease_id}` is exact-id idempotent, empty body, returning `ReleaseAck { released:bool }`. The last released id per slot is retained: repetition of that id returns true, never mutates a newer active grant; an unknown/stale mismatched id is409 and leaves the current grant untouched. Lease ids never wrap or repeat in this epoch. A retry uses exactly the acquired id; request timeout is not proof that a request was not applied. Slots never transfer between observers. Each observer closes read/stream owners before release; retries the same grant/release at most3 times with100 ms request budgets. Failure ends that observer and retains its controller charge. Only actual task join at shutdown can reconcile a lost acknowledgement. |
| Events | `GET /v1/jobs/{epoch}/{id}/events` on either explicitly selected observer port. Last-Event-ID is canonical decimal sequence or absent=0. Query immutable snapshots through the authenticated loopback control boundary with100 ms request budgets and10 ms loop pauses; do not share mutable tables. Old cursor before retained history emits `event: gap` with current snapshot, then ordered retained changes. Future cursor is Invalid. SSE id is decimal sequence; data is bounded UTF-8 JSON. Terminal snapshot ends that request; an expected rejection before commitment emits its HTTP status, while later retirement closes only that stream and leaves the observer listener healthy; disconnect leaves the batch job admitted. Exactly two observer workers each own one distinct listener/port. The caller explicitly selects a free observer port; each port serves one ongoing stream. Two connections to the same port may queue. No implicit load balancing, shared-listener fairness or SO_REUSEPORT routing guarantee is promised. The independent control port remains available with one held stream on each observer port. |
| Artifact | `ArtifactRow { artifact_id:string, media_type:string, byte_length:i64, integrity:Option<string> }`, in that order. Opaque id contains epoch/job/phase, never a host path. Successful terminal jobs alone expose completed producer-qualified artifacts; integrity is None unless explicitly computed. `GET /v1/jobs/{epoch}/{id}/artifacts/{phase}` on either explicitly selected observer port uses a controller-granted managed read lease, a generated single-component basename and retained/no-follow single-link regular-file reads. Positive write budget and fixed binary reads; no whole-artifact allocation or base64 conversion. |
| Leases/retirement | At most two live managed leases, one per observer. Private `LeaseGrant { lease_id:i64, artifact_id:string, relative_name:string, media_type:string, byte_length:i64 }`; checked lease ids, no remote-selected path. Reader/stream owners close before releasing that lease through control. A lost release acknowledgement retains the charge; no expiry guesses that a reader closed. Retired-but-open files retain reservation until the last managed lease closes; no new download is granted for retired output. Retirement/deletion changes only generated entries owned by this job. |
| Live transport controls | Either observer `/v1/live/text` writes fixed mock token events directly; `/v1/live/pcm` sends 8000 mono sample frames of s16le silence at rate8000 as application/octet-stream with explicit format/rate/channels/frame count. Borrow fixed host byte chunks; chunk boundaries carry no sample meaning. Do not label PCM as WAV. These CPU mock routes neither take device admission nor proxy the existing LLM token path through jobs. |
| Audio timing | `TextSpan { start:i64, end:i64 }`; `AlignmentEntry { ordinal:i64, track:i64, kind:string, label:string, start_frame:i64, end_frame:i64, text_span:Option<TextSpan> }`; `AudioAlignment { artifact_id:string, transcript:string, sample_rate:i64, channels:i64, sample_frames:i64, producer:string, entries:Option<array<AlignmentEntry>> }`, fields in this order. Rate/channels positive, frames nonnegative. <=16 entries, labels<=64 UTF-8 bytes; categories word/phoneme/breath/silence and tracks0..3. Ordinals equal array indices, same-track starts nondecreasing; half-open 0<=start<=end<=frames. Text spans name this exact transcript and valid UTF-8 byte boundaries. None means unsupported, never guessed timing. Mock audio provides only its known whole-track silence interval; no word/phoneme/whisper claim. |
| Output time conversion | `frames_ms(frame:i64, rate:i64) -> Result<i64,Error>` uses checked quotient/remainder arithmetic and floor rounding; source sample positions stay authoritative. `VideoTime { numerator:i64, denominator:i64, pts:i64 }`, positive time-base terms and nonnegative PTS; checked rational conversion with explicit floor. Invalid domain/overflow returns Invalid. Resampling/concatenation transformations are explicit application arithmetic, not accumulated rounded milliseconds. |

Encoding uses existing bounded owned JSON, stable declaration order recursively,
i64 decimal scalars and lowercase discriminator strings. No canonical binary,
cache key, interface format or restart journal is introduced. Job/history
snapshots are ephemeral. Every None-valued object field is omitted, including nested EventRow.total/progress_percent, ArtifactRow.integrity, AlignmentEntry.text_span and AudioAlignment.entries; Some values are present at their declaration positions. No nullable wrapper or alternate codec is added. Wire scalar/Option/array presence is exhaustive below:
artifacts is always an array, empty until successful atomic publication; alignment is omitted until succeeded audio/pipeline. error is present only for Failed, otherwise omitted. Health.running is present only when a managed active id exists; reserved counts include retired pinned files. Event/status total is present only when the producer supplied a known nonnegative total for that phase; progress_percent is present only for positive known total, including completed zero. Unsupported totals and zero totals omit percent. Retained event counters describe their own phase.
A cancellation/failure retains last observed counters; queued has empty phase,
zero completed, absent total/percent, empty artifacts and absent alignment/error.
Succeeded audio/pipeline has Some alignment; other kinds have None. Other states
have None alignment. Ordered artifact phases follow production order. New epochs
reject old receipts with 410; unknown current ids/routes use 404.

Validation order is private authentication, route grammar/canonical id and cursor
syntax, route-specific method, epoch domain/current epoch and job lookup, then
body UTF-8/typed JSON, input length/NUL, delay domain, capacity and retaining
input/native launch. Health/submit/release have no epoch lookup; release validates
its empty body and authenticated slot before exact lease id. All text uses
existing strict UTF-8 JSON: submit prohibits embedded NUL; metadata strings may
contain NUL as escaped JSON since no native C-string boundary consumes them.
Invalid input is 400, unsupported method405, full capacity429, internal failure500.
Native H3 body-cap refusal or total acquisition timeout may close the input
without a response, including a connection selected near budget exhaustion.
Read requests may be retried; an unacknowledged submit remains ambiguous and
must not be automatically resubmitted. This ephemeral reference adds no
idempotency-key protocol or crash/restart recovery.
Every response, including health/error/rejection, has a positive H2 budget;
no controller ctx.respond path is allowed. Control accept budget20 ms, input body
cap1024; stream-write budget50 ms. Internal clients use explicit100 ms request
budgets and 32768-byte response caps. Native scheduling/file I/O is not hard real time.

## Process, output and restart boundary

Startup probes existing Linux child_scope support before the server begins job
admission; macOS refuses that stronger reference boundary before a worker starts.
Common model/timing/HTTP owners still compile/run on macOS. WSL2 must pass the same
kernel feature probe; do not degrade to ordinary child or a group scan.

Each phase launches one root owned by process.child_scope. Text/audio/batch mocks
write only to captured stdout, with diagnostic stderr separate; receive no direct
output file path/fd. Parent creates an exclusive staging writer, drains fixed
scratch, and refuses an extra byte beyond its job's remaining reserved allowance without publishing a
truncated success. Oversized output initiates owned cancellation. Disk/write or
framing-validation failure retains cleanup ownership. Every tick bounds pipe and HTTP
work, then observes/reaps/releases without a blocking inference handler.

Text bytes are fixed mock text; audio is complete mono 8000 Hz s16le WAV silence;
batch is a complete 16x16 binary PPM. Known byte totals are respectively29,16044,781; mux total is unavailable. Parent publication checks exact mock byte lengths and WAV/PPM/MP4 framing signatures, not arbitrary codec validity. Independent ffprobe qualification checks the real FFmpeg result. Pipeline runs text, audio, batch, then FFmpeg
in that order. FFmpeg receives admitted input artifacts and writes completed
fragmented MP4 to its stdout pipe, under the same parent byte cap. Never launch
FFmpeg or a next engine until the previous scope releases successfully. The
native process absence witness, not EOF/root exit/RSS, permits the next phase.
Per-phase generation budget is explicit 5 seconds, independent of socket budgets.
Mock workers observe TERM cooperatively; authenticated descendant KILL and bounded
reap repeat through release. Failure to release retains Faulted ownership.

Private control routes use a separate 128-bit CSPRNG token retained only by the
controller and observer tasks; it is absent from public receipts/status. Tasks
open their own retained roots and distinct listeners and read immutable control snapshots.
The directory fence prevents a new cooperating controller from treating an
unclean predecessor as free device capacity. It is not a crash cleanup guarantee:
after crash, manually reconcile remaining processes/device ownership before
removing that fence. Clean shutdown first stops admission/new downloads and cancels/releases active work. The controller writes a bounded Cleanup record under an exclusive generated entry before returning code0: epoch:string and names:array<string>, <=48 single-component generated names, <=8192 encoded bytes. It includes input/staging/published/retired files still owned by its table and validates each generated-name identity before encoding. It never stores host paths or a process absence guess. The outer owner opens and decodes this record only after wait has joined all tasks and controller code0 has explicitly certified successful scope releases. It validates every name, removes recorded files (already absent is harmless), then the cleanup record and fence last. Unknown entries, malformed/missing record, controller code1 or cleanup error preserve the fence. Idle observers perform up to three100 ms authenticated health probes between accepts; accepting=false or three failed probes exits after native local owners close. Each task catches its own Result error, prints its own diagnostic and returns code1; wait and three primitive results retain separate controller/observer failures. Observer code1 still proves its reader has closed after actual join and allows file cleanup when the controller release witness is code0; final program result remains failure. A controller that cannot prove scope.release continues its Faulted cleanup loop and cannot return code0. Preserve controller
and observer errors separately through this cleanup; an observer error cannot
turn an unproved process release into clean shutdown. Task join, not elapsed
time or a missing acknowledgement, closes a managed reader lifetime. No supervisor crash, root exit, scan or peak RSS certifies release.

## Implementation closure and acceptance

| Axis | Exact owner and failure boundary |
| --- | --- |
| Inputs and state | `controller.selftest` owns FIFO, capacity, terminal retirement, history gap and exact/extra output admission; the Linux HTTP owner owns queued/running repeated cancellation, full admission without new entries and wire availability. Checked id/sequence increments use existing scalar checked arithmetic. Wire semantic-to-byte and byte-to-semantic goldens use independent expected strings; duplicate/missing fields, invalid UTF-8 and simultaneous NUL/delay rejection are exercised before effects; unknown-key skipping reuses the existing owned JSON owner. |
| Resources | Parent-owned capped sink crosses exact cap/extra byte, stdout/stderr independent explicit caps and bounded drains; oversized mux, nonzero exit, invalid framing, orphan release and cancellation have native reference controls. Pipe partial/pending/EOF and filesystem write/no-follow failures reuse the existing process/fs owners; no new native semantics are introduced. One step retains its native owner through every Result/branch/loop exit; Drop is cleanup, never a success report. |
| Managed release | Linux integration mock root with delayed/orphan descendant release, cancellation, producer crash and oversize; prove next launch/FFmpeg is excluded until successful scope.release. Existing library native scope owners are reused. A real retained descendant crosses the generation budget and proves Faulted ownership excludes the queued next job until authenticated cleanup/release, without redefining process semantics. |
| HTTP/control | Partial/stalled input and stalled output; control remains available during two held event/download observers. The fixture expands the actual control reply after its H2 setter to fill native send buffers; a nonreading peer must yield Timeout and leave health responsive. Every production reply path uses configured H2 including invalid/rejection/finish; disconnect does not cancel a queued job. Loopback authenticated snapshots do not share mutable storage. Three deferred closures execute only after wait; two idle observers observe shutdown and join, with each error preserved as its own primitive result. |
| Artifacts | Exclusive/no-follow staging, no-replace publication, invalid opaque id/path, failed container validation, exact bound and retired live download. Delayed release -> exact-id retry ACK -> new grant -> delayed old release must preserve the newer grant and charge. Charge staging/published/retired output until the last closed read lease; no clean result on extra output or failed release. |
| Timing | Sample-frame/UTF-8-boundary/ordering errors, absent unsupported alignment, zero/max/overflow quotient/rational conversions, exact independently checked JSON vectors and round trips. Live PCM and completed WAV fixtures retain distinct media types. |
| Composition | Native Linux runnable submit/status/SSE/cancel/artifact routes, mocked serial text/audio/batch and FFmpeg audio/video assembly, ffprobe validation, bounded memory/disk/worker assertions and graceful shutdown/fence cleanup. macOS runs common source/model owners; no GPU performance/fit/model-quality claim. |
| Compiler and interface | No compiler/runtime edit. Imported module and whole/per-unit source owners reuse existing capability owners. New friction gets a separate finite recorded capability rather than reactivating K1 or hiding unsafe workarounds. |

This complete example is expected to exceed 1000 handwritten lines once wire,
process, output, download and owner controls are included. Splitting dormant DTOs,
sink, release and handlers would publish the resource promise without its useful
consumer and duplicate proof. The one boundary is runnable bounded composition;
real engine adapters remain separate capabilities. No new performance benchmark
is required: it makes correctness/resource bounds, not model speed promises.

Sources to keep aligned: this ledger, plan90 C1/file map, example modules, one
focused driver owner, English `docs/guide/27-multimodal-reference.md` and its ja
mirror, guide index and one capability status in HANDOFF. Language specifications
and core/std/pkg contracts remain unchanged unless a concrete defect is found.

Author-side ledger pass: all public fields, default modes, error precedence,
resource identity, exact owner/Drop/release, encoding/NUL boundaries, wire ordering,
Option presence, process-global overlap and restart refusal are recorded above.
Declarations are ledger signatures, not executable code examples. Source examples
and golden owners are syntax-checked before the implementation candidate. Obtain
one fresh independent adversarial review of this resource strategy before coding.

Independent strategy review resolved before code: deferred-task execution is one three-closure group dispatched by wait; exact-id releases survive delayed/retried requests; idle observers have a bounded stop observation; existing JSON omission has exhaustive field presence. These four findings are closed in this ledger and the discriminating owners above. No task scheduler or JSON semantics change is required.

### Distinct observer admission boundary

The initial same-port `http.serve_shared` composition cannot prove two live
observer requests: Linux SO_REUSEPORT assigns flows to independent listener
queues, including a busy observer while another is idle. C1 instead accepts
three explicit distinct ports and uses ordinary `http.serve` per task. No new
listener type, shared mutable owner, runtime thread or server duplication API is
introduced. Routes and per-slot lease authority are unchanged. Native owner
requests one event stream on each observer port and keeps control accessible;
connections to one occupied observer port have no fairness promise. A scoped independent boundary review completed CLEAN before this source
correction. The final code candidate still requires its separate full-diff review.

## Final author-side closure

Internal state tags are queued0/running1/cancelling2/succeeded3/failed4/
cancelled5; kind tags text1/audio2/batch3/pipeline4; phase tags empty0/text1/
audio2/batch3/mux4. All stored tags are i64, not exchanged as numbers. Job
columns and the five fixed128-entry history columns have one controller owner.
Named mutable scratch views cross module calls; native scope/writer handles stay
in an inner lexical phase, with Copy metadata extracted before native moves.
These mechanical source forms use current checked-HIR/borrow contracts and do
not reopen K1 or add a compiler special case.

Owner `crates/align_driver/tests/multimodal_reference.rs` reads the actual seven
Align modules. The model/golden owner compiles and runs whole/per-unit source on
both native platforms; the Linux owner uses installed explicit FFmpeg/ffprobe
and exercises the full process/socket/filesystem composition. Its private-token,
positive-delay and large-reply seams are assertion-checked fixture rewrites only;
production has CSPRNG tokens and no test controls. No performance claim or
benchmark is attached to this application. HTTP H1's token/allocation evidence
remains the provider owner; LLM generation is not routed through this queue.

The reference intentionally ships as one runnable capability despite exceeding
1000 hand-written lines. Splitting its controller, lease observers and cleanup
manifest into dormant PRs would duplicate resource proof and expose no stable
consumer; the one bounded native owner closes their joint failure domain.
Final paths are `examples/multimodal/`, this ledger, the leaf owner, and English/
Japanese `docs/guide/27-multimodal-reference.md` plus their indexes. Plan90 and
HANDOFF record the capability once. Core language and library API promises,
cache identity, compiler passes and ABI inventory do not change.

### Review-fix closure

The one full code review of candidate a1bb862b found three P2 classes and one
P3 owner-cleanup class. Existing grant replay now precedes new-grant refusal
across retained/retired and accepting/stopping states; one parameterized model
owns all those cases. The native owner discards an applied acknowledgement,
retires its pinned job, replays the exact grant and rejects a new slot grant.
Malformed successful private grants terminate the observer, retain its charge
and preserve that task failure after join/cleanup. A controlled later snapshot404
after real SSE bytes closes that stream; another request on its port succeeds.
Authentication, grammar, listener-specific route admission, method and epoch/
lookup precedence have one dispatcher order with simultaneous-invalid owners.
These fixes fulfill the ledger; no public type or resource strategy changes.

The dedicated single-thread Python native owner sets child-subreaper authority
before spawning any server and restores it after cleanup. It uses only its
producer-owned direct-child table, keeps each child unreaped until opening its
pidfd, signals by that identity and reaps before refreshing the table. SIGCHLD
is default and no parallel/asynchronous reaper runs. Four-second bounded cleanup
handles adopted descendants across setsid after a server is forcibly killed;
a negative owner exercises that fallback and verifies the unclean fence remains
before external fixture reconciliation. Unexpected recovered children still
fail the ordinary native run. This test-only authority never substitutes for
Align child_scope.release or a GPU release witness. The isolated harness owns
the process-global operation, exclusion, errors, restoration and exhaustion.
Native authority follows [PR_SET_CHILD_SUBREAPER](https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html)
and [pidfd_open child identity conditions](https://man7.org/linux/man-pages/man2/pidfd_open.2.html).
