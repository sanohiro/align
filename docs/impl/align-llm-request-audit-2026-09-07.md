# align-llm request audit — 2026-09-07

This is a dated status audit, not a new language contract or an instruction to
implement every proposal. It covers all 60 numbered entries in the external
[request register](../../../align-llm/docs/align-requests.md), against Align
`b947b5d92e17242e5511a80de8d5a446c7ed8b83` after PR #975. The consumer checkout
was `1a37b5b`; its code, pin, tests, branches, and publication state were not changed.

## Result and evidence limits

The register declares **22 CLOSED, 1 ALIGN_LLM_VERIFIED, 7 ALIGN_MERGED,
and 30 PROPOSED**. It declares no ACCEPTED or IMPLEMENTING entries. Therefore
neither “all requests complete” nor “30 wholly unstarted implementations” is
accurate. Some proposals have shipped subsets or adequate existing alternatives.
The table separates those cases and gives an action recommendation, not a
retroactive consumer acceptance claim.

Closed entries were audited against their recorded compiler delivery and named
consumer acceptance evidence. This audit does not rerun all historical consumer
suites. Open language/API claims were compared with current source/contracts and
small compiler probes; diagnostic and ownership failures below were reproduced
locally. Resource observations use the local Linux release compiler/runtime and
are not cross-platform benchmark promises. Probe sources, outputs, and metadata
are preserved in `/tmp/align-request-audit-20260907/` for this session; the table
records the conclusions independently of those temporary files.

PR #975 passed independent review, local owner/bounded/Clippy gates, all required
CI platforms, and the release workspace build before merging. No versioned
release was requested or published for this batch.

## All requests

| Request | Register state | Audit and next action |
|---|---|---|
| 1 — `std.process`: child output capture (+ working directory, environment, timeout) | CLOSED | Closed capture/cwd/env/timeout adoption; #630–632. No new work. |
| 2 — `std.http` / `std.net`: I/O timeouts | CLOSED | Closed deadline adoption; #633–634 and named HTTP timeout consumer owner. No new work. |
| 3 — `core.json`: decode/encode scalar-array struct fields (`array<str>`, `array<i64>`, …) | CLOSED | Closed scalar-array JSON adoption; #635. No new work. |
| 4 — `std.http`: client-side chunked response de-framing for provider SSE | CLOSED | Closed chunked response adoption; #800. No new work. |
| 5 — `std.http`: bounded client response bodies | CLOSED | Closed bounded HTTP response adoption; #812. No new work. |
| 6 — `core.json`: require recursively Copy `json.scan` rows | CLOSED | Closed recursively Copy scan-row adoption; #704 and consumer #84. No new work. |
| 7 — `core.json`: escaped strings in declared-record decoding | CLOSED | Closed escaped-string adoption; #850 and consumer #94. No new work. |
| 8 — `core.array_builder`: runtime construction of declared-record arrays | CLOSED | Closed declared-record array-builder adoption; #801 and consumer #94. No new work. |
| 9 — `core.json`: owned text fields and runtime-sized text arrays | CLOSED | Closed owned JSON text adoption; #852 and C7 source-expiry/qualification evidence. No new work. |
| 10 — `core.array_builder`: recursive evaluator record fields | CLOSED | Closed recursive evaluator array adoption; #804 and consumer #94. No new work. |
| 11 — `std.process`: bounded child output capture | CLOSED | Closed bounded process capture adoption; #808 and consumer #100. No new work. |
| 12 — `core.json`: bounded canonical encoding | CLOSED | Closed bounded canonical JSON adoption; #807 and consumer #94. No new work. |
| 13 — `core.json`: recursive owned C6 artifact graphs | CLOSED | Closed recursive owned graph adoption; #854 and consumer #94. No new work. |
| 14 — `std.fs`: exclusive creation and no-replace publication | CLOSED | Closed exclusive creation/no-replace publication; #861 and consumer #100. No new work. |
| 15 — `core.json`: complete decoded-owner transitions | CLOSED | Closed decoded-owner transition adoption; #849 and consumer #94. R45 is a distinct unclosed nested consuming context. |
| 16 — language: borrow-safe inspection of owned sum payloads | CLOSED | Closed borrowed sum inspection; #857 and consumer #98. No new work. |
| 17 — language: borrow-safe dynamic aggregate projection | CLOSED | Closed dynamic aggregate projection; #865 and consumer #98. No new work. |
| 18 — `std.fs`: retained-root regular-file access | CLOSED | Closed retained-root access; #867 and consumer #99. No new work. |
| 19 — compiler: code-generation cost on a graph of large by-value structs | CLOSED | Closed shared recursive Drop code-generation cost; #891 and consumer #108. Does not establish R37 check-time scaling. |
| 20 — CI: run the owned-JSON boundary regressions on the macOS matrix leg | CLOSED | Closed macOS owned-JSON coverage; #887 and consumer #107. No new work. |
| 21 — `std.fs`: read-only random-access file open (`fs.open_ro`) | PROPOSED | Missing read-only random-access constructor. Existing reader/mmap paths do not supply the requested file pread/len surface. Consider with the filesystem capability needs; not implemented by #892. |
| 22 — Indexing arrays of Move element types | CLOSED | Closed array<string> indexing adoption; #920. Do not widen this into arbitrary Move-element extraction. |
| 23 — Huge-struct-copy warning fires on borrow parameters | PROPOSED | Current 136-byte record warns for both borrow modes as well as by-value. Worth fixing the false diagnostic; by-value control must keep warning. |
| 24 — `builder` as a `borrow mut` parameter type | PROPOSED | Named builder parameter type is rejected. array_builder<T> parameters already exist but do not replace a text builder. Defer until the shared encoder use justifies a public type. |
| 25 — Streaming child stdout (redirect or incremental read) for `std.process` | PROPOSED | spawn_piped is absent; command capture exists but collects a complete bounded result. Streaming remains a separate capability; defer without a current streaming consumer. |
| 26 — `str`-to-integer parsing in the standard library | PROPOSED | No standard integer parsing surface found; parse_i64 method probe rejected. Useful library addition, but requires an exact error/range contract before implementation. |
| 27 — String ordering and sorting | PROPOSED | Partially available: str/string ordering is settled and implemented; sort rejects str and sort_by is absent. Avoid reimplementing ordering; assess only the sorting gap. |
| 28 — A readable append-only accumulator | PROPOSED | buffer already supports append, len, and byte views. Builders are still write-only. Prefer the existing buffer for byte accumulation; only add builder inspection if that does not meet the actual representation need. |
| 29 — Incremental digest (`sha256` init/update/final) | PROPOSED | One-shot digest exists; incremental digest state does not. Defer while the existing bounded hashing path suffices. |
| 30 — `fs.create_rw_exclusive` | PROPOSED | Exclusive sequential writer exists, but no random-access exclusive constructor. Consider together with R21 and the actual publication flow. |
| 31 — File durability (`fsync`/`fdatasync`) | PROPOSED | No file sync surface. Current reproducible artifact publication does not itself promise crash durability; defer until that contract is needed. |
| 32 — FFI v1 by-value struct ABI (AAPCS64 and SysV MEMORY class) and `bool` FFI type | PROPOSED | Partial FFI support is not the requested complete ABI: raw layout(C) field and bool extern probes reject. C shims remain an explicit working boundary; do not widen ABI support without a real required call shape. |
| 33 — Aligned heap allocation (`buffer` / `raw.alloc` with explicit alignment) | PROPOSED | Explicit alignment constructor arguments reject. Existing overallocation/C shim workarounds remain; implement only against the measured native allocation requirement. |
| 34 — `Result` ok payloads beyond scalars (`raw`, `buffer`, records) | PROPOSED | Partially implemented elsewhere: Result<buffer, Error> checks, builds, and runs successfully. Result<raw, Error> and raw record fields still reject. Do not reopen the shipped buffer carrier; assess raw ownership separately. |
| 35 — Observable `buffer` capacity and allocation failure | PROPOSED | Buffer capacity/cap/try_new language methods reject. A runtime capacity helper is not a public observation API. Worth designing if allocation admission remains a real client requirement. |
| 36 — In-place replacement of owned array record fields and moving out of nested fields | PROPOSED | Whole owned array-field replacement and nested Move-field extraction still reject. This is an ownership capability extension; defer until its complete transfer/Drop matrix is designed. |
| 37 — Compiler check-time scaling for long function bodies and `match` on `Result` inside loops | PROPOSED | Not closed by current evidence. Historical client measurements concern body-length and loop-match checking; R19 and R58 address different stages. Re-measure the representative workload before selecting another performance fix. |
| 38 — Positional write, reset, and bounded read length for `buffer` | PROPOSED | Three-argument pread, buffer clear, and put_at reject. Existing refill and append are narrower. Assess a coherent reusable-window API with R35/R39 rather than adding disconnected methods. |
| 39 — Release of rebound `buffer` allocations before frame exit | PROPOSED | Current rebind experiment retains memory proportional to calls: one 2 MiB fill versus 32 gives 11,876 versus 66,908 KiB peak RSS. Worth fixing replacement cleanup; inspect all affected Move handles before implementation. |
| 40 — `array_builder<T>` as a struct field type | PROPOSED | array_builder<T> record field rejects. Local/parameter builders do not provide the requested stored accumulator. Defer until its construction/move/Drop contract is needed. |
| 41 — Non-`Copy` capture in `spawn` closures (`task_group`) | PROPOSED | Move/exclusive spawn captures and actual concurrent I/O scheduling remain separate missing prerequisites. Current synchronous GPU/G5 work does not require this extension; defer rather than treating capture alone as completion. |
| 42 — `alignc check` as a superset of `alignc build` (region checking parity) | PROPOSED | Current two-module reproduction: check succeeds, check-per-unit/build reject. Worth fixing check/build parity; retain diagnostics for genuinely invalid programs. |
| 43 — Cross-module `borrow mut` record out-parameters | PROPOSED | Same two-module reproduction rejects an independent output record after the foreign call. Worth investigating foreign mutable-argument summaries together with R49; not fixed by unrelated producer repairs. |
| 44 — compiler: array-to-slice view retype through a borrowed sum projection | ALIGN_LLM_VERIFIED | Compiler #892 and real-client verification are recorded complete; register stops at ALIGN_LLM_VERIFIED. No additional compiler work. Consumer publication/status closure remains consumer-owned. |
| 45 — Compiler soundness: moving a field out of a decoded record double-frees at run time | PROPOSED | Current registered example checks and builds but fails at execution due to incorrect nested owned-field transfer. Highest repair priority. Existing named nested-move rejection is an acceptable request outcome. |
| 46 — `borrow mut` array locals inside loops, and no element assignment through an array field | PROPOSED | Both original examples still reject: repeated mutable array local in a loop, and element assignment through an array field. Separate the loop analysis defect from the field-place capability before implementation. |
| 47 — A `Borrow` argument may be a temporary value | PROPOSED | Temporary slice, builder.build(), and if-result Borrow arguments reject. R22 temporary indexed-view handling does not close general temporary arguments. Requires a complete argument-lifetime design. |
| 48 — Same-call argument aliasing between a `borrow mut` owner and its own scalar field | PROPOSED | Current same-call mutable owner plus its Copy scalar field rejects. Worth reviewing evaluation-time snapshots; do not relax actual overlapping borrows. |
| 49 — A cross-module call with a `borrow mut` argument refuses every shorter-lived operand | PROPOSED | A literal-only control succeeds now, but an owned local string passed beside the caller mutable parameter still fails only in per-unit/build. Multi-output issue overlaps R43. Worth fixing precise foreign summaries; a no-view return alone does not prove no retention. |
| 50 — `std.os`: how much physical and available memory the host has | PROPOSED | No std.os memory-query module/surface. Existing host preflight already handles the cited admission need, and the later dense client explicitly does not need this. Defer. |
| 51 — A reserved word used as an identifier should say so | PROPOSED | Original reserved-identifier example still yields parser cascades rather than a named reserved-word explanation. Useful diagnostic improvement after correctness repairs. |
| 52 — `match` on an owned record's `Option` field silently moves the payload out, and a later encode drops it | ALIGN_MERGED | Align #970 merged. Its answer rejects encoding moved owners; it does not make Move payload matching implicitly Copy. Consumer must verify against that shipped answer. |
| 53 — `std.fs`: directory creation, directory listing, and a file-type predicate | PROPOSED | Partially implemented elsewhere: fs.read_dir is accepted. General create_dir/is_dir remain absent; R56 private staging is narrower. Defer the remaining general-directory surface until the deferred store feature needs it. |
| 54 — ELF support libraries precede user static archives and cannot resolve their symbols | ALIGN_MERGED | Align #971 merged; support-library ordering repaired. Consumer archive/link smoke remains pending. |
| 55 — `std.fs`: retained-root single-link regular-file open | ALIGN_MERGED | Align #952 shipped in v0.7.3. Compiler delivery complete; consumer single-link admission adoption remains pending. |
| 56 — `std.fs`: private temporary-directory lifecycle | ALIGN_MERGED | Align #962 shipped in v0.7.5. Compiler delivery complete; consumer private-staging adoption remains pending. |
| 57 — MIR producer certification for owned `Result` record fields | CLOSED | Closed producer loop-join adoption; #950. No new work. |
| 58 — MIR resource validation must terminate for a bounded borrowed-reader pipeline | ALIGN_MERGED | Align #960 shipped in v0.7.4. Termination repair complete; exact consumer load smoke remains pending. Does not close all frontend performance requests. |
| 59 — Owned builder record return producer certification | ALIGN_MERGED | Align #972 plus #975 (b947b5d9) merged. Both unchanged verification_loop and alignpack per-unit checks pass. Consumer build/smoke adoption remains pending. |
| 60 — Explicit absolute C link driver | ALIGN_MERGED | Align #973 merged. Explicit absolute C driver is available; consumer closed-environment preparation remains pending. |

## Work selection

1. Repair R45 first. The registered program currently fails after a successful
   compilation; an application clone workaround does not close compiler correctness.
2. Investigate R39 replacement cleanup and the R42/R43/R49 checking/foreign-call
   boundary, with their owning closure matrices. R46 and R48 may share analysis
   mechanisms but must not be assumed to share one fix without evidence.
3. R23 and R51 are useful focused diagnostic improvements. R37 needs a fresh
   representative measurement before another performance change.
4. Do not redo R1–20, R22, R44, R57, or the Align side of the seven merged rows.
   Do not implement an entire proposal when the requested subset already works
   (notably R27, R28, R34, and R53). Remaining API extensions stay proposed until
   their concrete benefit and complete contract justify a capability boundary.
5. Consumer adoption is not Align implementation work. Do not edit the external
   consumer or advance its acceptance state merely to make every row CLOSED.

## Status reconciliation

The historical “Request 21” fixed by #892 is current **Request 44**; the register
explicitly reconciles a numbering collision. Current Request 21 means read-only
random-access file opening and was not implemented by that PR.

Historical register summaries and handoff paragraphs predate later merges.
In particular R52/R54 are merged, R59 includes #975 in addition to #972, and R56
is already published in v0.7.5. Those discrepancies explain part of the apparent
unfinished/finished conflict; they are status drift, not new code defects.

R52's consumer acceptance prose asks to retain data after a consuming match.
The shipped answer instead diagnoses the moved owner and preserves explicit
borrowed inspection. Consumer verification must respect the shipped ownership
contract; silently copying payloads is not an outstanding implementation task.

The register retains its lifecycle states. Align's audit answer is added there
without committing the external repository. Only actual implementation and
consumer evidence should move those states.
