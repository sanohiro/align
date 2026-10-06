# Bounded standard-library follow-up assessment

The owner requested this inventory on 2026-10-06 after the hexadecimal decoder
capability, then one bounded implementation batch and a stop. K1 and plan 61
remain deferred. The external align-llm request register is evidence only;
consumer source, tests, pins and adoption remain external.

## Evidence and priority

This assessment compares current implementation and owner tests with shipped
examples, the module design records and external requests. A historical request
label alone does not establish a current gap. P1 denotes the next contract/design
priority for the demonstrated use; P2 denotes a bounded measured refinement;
P3 requires a concrete consumer before becoming implementation work.

### 1. Missing functionality demonstrated by use

| Candidate | Use and evidence | Priority and completion condition |
| --- | --- | --- |
| Explicit alignment of owned byte-buffer payloads | Large read/upload windows and native-library interop. External requests 33 and 127 record the missing guarantee and a measured WSL read-address penalty; current `BufferStorage` owns an ordinary `Vec<u8>`, `align_rt_buffer_new`/`filled` select no payload alignment, and `aligned_binding.rs` covers fixed numeric arrays. `buffer_pread_into.rs` proves address preservation, not alignment selection. This is a core/compiler prerequisite used by std.io, not a missing std module. | P1, separate contract work. Close creation, filled construction, growth, reads, move/replacement and matching-layout Drop on Linux/macOS and whole/per-unit paths; reject invalid alignment and stale views. Establish exact supported bounds and the existing `align(N)` relationship before implementation. Consumer upload hashes/startup qualification remains external; alignment alone promises no portable speedup. |
| Recoverable buffer allocation failure | Resident-weight and bounded file-window admission. Request 35's capacity accessor is shipped by plan 87; `m9_io` owns that accessor. Current construction still degrades failed payload reserve to a zero window, while filled construction/growth/header OOM remain terminal. `examples/file_sha256.align` and `http_fetch.align` explicitly check capacity but cannot make later allocation failure recoverable. | P1, separate allocation-policy design. Specify one coherent failure/publication strategy for the promised construction/initialization/growth scope and header allocation, then close deterministic failpoint, cleanup and Result/control-flow owners. Another capacity alias or memory precheck does not close it. No new signature is approved here. |

### 2. Correctness and safety

| Candidate | Use and evidence | Priority and completion condition |
| --- | --- | --- |
| HTTPS certificate revocation policy | Clients whose security policy must refuse revoked certificates. `http_tls_connect` enforces chain and DNS/IP identity; native `https_self_signed_is_denied`, wrong-host and successful round-trip owners cover that boundary. `std-design/http.md` explicitly records absent CRL/OCSP/stapling verification. This is an existing security limitation, not a newly reproduced contract regression. | P1 when that policy is required. First specify required evidence, freshness, unavailable-evidence behavior, deadlines, caching and errors; then local certificate/revocation fixtures must prove refusal and cleanup. Do not silently add network work or a second trust policy. Not selected in this batch. |

During the selected consumer verification, three existing S3 wire owners failed
on macOS at the first socket read. Their common accept helper left the accepted
socket nonblocking; Linux did not inherit that mode. The selected batch also
restores blocking mode before installing the existing read/write timeouts. This
is a P2 test-fixture defect, closed by all three wire owners and the complete
`pkg_s3` target on both hosts, not a UTC or HTTP product defect.

No other unfixed std runtime defect is established by this bounded assessment.
The known-failure manifest currently has no active entries; that is not a new
whole-runtime safety certification. Concrete recent defects have already been
closed: complete gzip/zstd consumption (plan 124), environment/argv/HTML text
admission (plans 120–122), and opened-file fallback ownership (plan 118).
Their regression owners remain the evidence. K1 and requests 124–126/128 concern
compiler provenance/ownership composition and are explicitly outside this batch.

### 3. Performance with an explicit measurement owner

| Candidate | Use and evidence | Priority and completion condition |
| --- | --- | --- |
| Fixed-field rendering of the five named UTC formats | `apps/s3/pkg/s3.align::request` and `presign` call `time.basic_iso`. The runtime's calendar conversion is already improved by plan 119; `time_formats.rs::format` still routes fixed 2/3/4/9-digit fields through generic formatting into a 32-byte stack buffer. Existing native wire/endpoint/grammar and driver `time_formats` owners constrain exact output. | P2; the only selected follow-up capability. Preserve all five grammars, floor/error behavior, independent owned output, native admission and parser. Compare actual release entrypoints including allocation/free on macOS/Linux before accepting a checked direct-decimal implementation. Reject an unhelpful result instead of adding another optimization candidate. No application/network throughput promise. |

Hex group decoding is complete in PR #1250 (plan 128), not a remaining item:
actual release-runtime measurements show about 3–3.8x for the reported large
valid inputs, with small short-case costs disclosed in `bench/hex_decode`.
Base64 groups, regex output storage, sparse sampling and Gregorian conversion
(plans 125–127 and 119) are likewise shipped. The earlier percent/form storage
experiment remains parked because its local Linux measurement regressed; it is
not a promised improvement or part of this batch.

### 4. Future capabilities awaiting a use case

These are existing recorded deferrals, not newly invented work.

| Recorded family | Possible use and current record | Priority and completion condition |
| --- | --- | --- |
| Regex captures iteration/callback replacement/literals | Repeated capture extraction or computed replacement; `std-design/regex.md` explicitly has no demonstrated consumer for these extras. Current `std_regex` owners cover the shipped span/capture/replacement surface. | P3. Produce a real caller and workaround, then choose one exact surface with lifetime/allocation/empty-match owners; callbacks also need their callable-lifetime prerequisites. |
| XML streaming and richer tree/query operations | Incremental large documents or richer selection. `std-design/xml.md` records these exclusions and says S3/Azure need only the shipped forward event stream. | P3. Demonstrate a consumer that the current bounded event API cannot serve, then establish input ownership, errors, storage bounds and owner tests before extending it. |
| Compression streaming/dictionaries/configurable limits | Incremental or dictionary-backed data. Plan 124 and `std-design/compress.md` retain complete-input APIs with an aggregate output cap. | P3. A concrete corpus and caller must establish the missing operation; close chunk boundaries, failed finalization, limits and native state lifetime in its own ledger. |
| Server TLS and additional TLS configuration | Direct encrypted server deployment or specialized client trust/authentication. The HTTP design ships client TLS and records the remaining surfaces separately. | P3 until a concrete deployment requires them; define one transport/trust/configuration model and native handshake/failure/cleanup owners. Revocation's security disposition is recorded separately above. |
| BLAKE3 | A consumer specifically requiring that digest. The crypto design records a deferred engine/library-layer decision; SHA-256/512 and incremental SHA-256 already have real callers and owners. | P3. Establish the actual digest requirement and an accepted engine/layer owner, then independent vectors and lifecycle/allocation tests. Do not rename another digest or assume external engine availability. |

## Reuse and selected boundary

Plan 51 and plans 92–95 already assess process execution, filesystem traversal
and CLI composition. Their current owners and `retained_tree_summary`,
`file_sha256`, `http_fetch`, `http_sse_watch` and multimodal examples demonstrate
those facilities; no duplicate exec/tree/CLI package is selected. Requests for
capacity observation, direct bounded pread, memory observations and durability
have provider implementations (plans 87, 89, 101 and 86). Consumer adoption is
not a missing Align implementation.

The selected batch contains this inventory plus one private UTC rendering
refinement under plan 130, if its measurement qualifies it. The P1 items change
public allocation or trust contracts and require separate design closure; they
are not silently folded into a timing change. Finish the selected capability's
owner checks, one independent review, final-SHA preflight, PR and full platform
CI/merge, then report outcomes and remaining items and stop. Do not automatically
add another candidate, widen K1 or modify align-llm implementation.

The selected rendering implementation and necessary S3 fixture repair are now
locally qualified on macOS/Linux under plan 130. The measured five-format corpus
improves on both hosts; `bench/time_fields/README.md` records the results and
limits. The other candidates above remain outside this batch. Its completion
boundary is this capability's merge and the final report, not another queue item.
