# Session handoff

## Current direction

K1's selected compiler repair is complete under
[Plan240](docs/impl/240-call-boundary-view-authority.md). All three consumers
record adoption of that compiler on 2026-10-25, within their named native
acceptance boundaries. Do not restart the relational-solver investigation or
interpret its older handoff as unfinished production work.

The owner resumed the selected batch on 2026-10-25 and authorized merging
completed capability PRs. AM-R24 is delivered by #1410; consumer adoption remains
pending. R132 and R142 stay excluded. Finish each independently useful capability
through its normal review, local verification and merge boundary, then run the
batch release build without creating a versioned release. When external evidence
or a machine dependency is unavailable, request the specific missing input and
continue other selected work.

[Plan241](docs/impl/241-bounded-http-responses.md) implements AM-R22's positive
ordinary response budget with HEAD framing, bounded native writes and checked
keep-alive restoration. Plan210 implements R28 text-builder bytes with read-only
access and mutation/owner-generation invalidation. Plan242 implements retained read-only
indexed-field ranges for AM-R16/R137. The remaining selected queue below is separate work.
Missing evidence or a passing older provider PR is not a completed fix; the
wider long-term backlog is unselected.

Preserve the existing language specification where practical. The owner permits
a small specification revision when it substantially reduces compiler complexity
while retaining safety, practical consumer use and Align's language policies.
Difficulty alone is not a reason to change the language. Report the implementation
alternative, proposed restriction and consumer impact; follow the normal contract
ledger, mirror and independent strategy-review requirements before implementation.

Before selecting work, compare this checkout with main and read all three
consumer request registers. A preserved investigation branch can contain an
older HANDOFF; its local status does not override delivered compiler capabilities
or current consumer receipts. Inspect Git/GitHub for live branch and PR state.

## Completed K1 boundary

[Plan240](docs/impl/240-call-boundary-view-authority.md), delivered in #1409,
uses the existing explicit parameter modes. Ordinary value parameters, captures
and returned non-owning view leaves are read-only. Explicit mutable parameters
retain proved writable backing; owned storage remains writable. Publication
checks temporal validity at returns and mutable outputs. No hidden copy,
allocation or lifetime annotation was added.

LLM R160's independent owned-buffer views survive descriptor-preserving copy
helpers and loop joins. Overlapping backing, actual replacement and expired
observations retain their refusals. Whole/per-unit, independent checked-HIR
replay, cache and native byte/allocation owners qualify the compiler boundary.
The plan owns the exact proof and test inventory.

Consumer receipts, read on 2026-10-25:

- [align-llm](../align-llm/docs/align-requests.md): R160 is CLOSED. Current
  product checks, cursor/state-copy and C7 ownership owners, plus old/candidate
  native Responses comparisons on both named model artifacts passed. This is
  scoped Linux/RTX 4070 Ti evidence; Mac and other GPUs are not qualified.
- [align-media](../align-media/docs/align-requests.md): #1409 adopted after
  Linux/WSL2 native build and runtime qualification without a K1 source rewrite.
  AM-R07 and the new AM-R22/23/24 requests remain separate.
- [align-voice](../align-voice/docs/align-requests.md): #1409 adopted and V006
  receiving work completed, including CPU admission, shape lifetime, generic
  CUDA and the named full-model/lifecycle owners. Historical candidate and
  pending-adoption paragraphs do not override the current receipt.

The original Plan239 relational model and its checkpoints remain preserved on
`investigate/k1-feasibility`; this is research evidence, not a selected solver
port. The older Plan61 implementation and general aggregate-provenance experiment
also remain parked. Broader writable returns are not a queued requirement.

## Selected work and remaining prerequisites

The remaining batch follows the scope above; R132/R142 below are explicitly
excluded. AM-R24 is complete at the provider boundary; Plan241 owns AM-R22.
[Plan230](docs/impl/230-request-follow-up-plan.md) retains the historical request
reconciliation; Plan240 supersedes its pre-K1 call-authority blocker.

- Media AM-R22 requests bounded, HEAD-correct HTTP response writes. AM-R23
  requests non-consuming connection observation before response. Plan241 implements AM-R22; AM-R23 remains a
  non-blocking draft with its current workaround. The register's older missing-URL
  note is supplemented by the private committed-source location below; consumer acceptance of timed
  HEAD or connection observation remains external. AM-R23 still needs its exact
  half-close, connection reuse and cleanup contract. Connection observation does not itself
  deliver model cancellation or GPU cleanup.
- Plan210 implements R28 `builder.bytes()` as an allocation-free read-only view
  of initialized bytes. Every write, finish, move, replacement and Drop ends
  earlier views; shared readers may return caller-borrowed views. Plan240 supplies
  ordinary-call authority. Consumer adoption remains external.
- [Plan242](docs/impl/242-indexed-field-views.md) implements AM-R16/R137 explicit
  read-only ranges into independent fixed/dynamic indexed array fields. It retains
  parent and selected backing without a field snapshot; Plan240 owns call authority.
  Independent Copy reads and lengths keep their existing semantics. Consumer
  adoption remains external. Whole fixed record-array call producer limits and
  unresolved returned byte-backing precision remain explicitly scoped in the plan.
- R132 exclusive owned-subrecord borrowing remains unshipped and is reobserved
  by current MTP and structured-decoding consumers. Projection/replacement must
  preserve sibling cleanup and heap-versus-arena allocation authority. This is
  a distinct large ownership capability, not removal of a diagnostic guard.
- R142 exclusive matched owned payloads remain unshipped. Active-tag authority,
  conflicting replacement, aliases and exactly-once cleanup need a separate
  complete design; nested owning fields may also encounter R132. The retired
  Qwen2 dispatcher is not current adoption evidence.
- AM-R07 external writable read destinations need an explicit native-view
  producer, initialization/bounds/overlap/resource-lifetime proof and matching
  positional-read semantics. K1 and fallible raw allocation do not supply these
  permissions or establish a pinned-memory speedup.
- Plan210 also repairs R49's headerless-owner loop precision: mutations of an
  independent builder preserve validated text. The authenticated original helper
  and immutable control pass both check modes; actual-source mutation negatives
  remain rejected. Consumer verification is pending.
- LLM R37/R80 remain investigations under Plan230. Their historical source/log
  packet was found and authenticated on 2026-10-25 (below). R37 reports roughly
  79.5 GiB peak memory at its recorded revision, so reproduction needs explicit
  resource limits; do not infer closure from K1.

### Recommended order and rough difficulty

These are pre-design scope estimates, not delivery-time commitments. K1's
completed call-boundary authority is a prerequisite to reuse, not a solver port
to repeat. AM-R22 now has its implementation and public contract in Plan241; later rows
remain separate capabilities.

| Order | Capability | Rough scale / difficulty | First closure decision |
| --- | --- | --- | --- |
| 1 | AM-R22 bounded HEAD-correct responses | Medium / medium | Plan241 implements `respond_timeout_ns` with one finite budget, partial-write cleanup and eligible keep-alive reuse. |
| 2 | R28 text-builder byte views | Medium to large / high | Plan210 implements the read-only prefix and exact invalidation; consumer adoption and view-bearing typed-builder elements are separate. |
| 3 | AM-R16/R137 retained indexed fields | Large / high | Plan242 implements explicit read-only ranges with parent/selected-storage lifetime and exact outer producer proof. |
| 4 | AM-R23 connection observation | Medium / medium to high | Define exactly what a non-consuming bounded observation can establish, including half-close, buffered/pipelined data, errors and races; cancellation policy stays in the consumer. |
| 5 | AM-R07 external writable positional reads | Large / high | Close exclusive native-view creation, initialization, bounds/overlap, external resource lifetime and synchronous short-read/error semantics together. No pinned-memory speed claim. |
| Evidence lane | R37/R49/R80 | Unknown until reproduction / potentially high | Recheck the exact packets early under resource bounds. Separate checker precision from MIR/emission cost; do not assume that K1 fixed them. |

Retained indexed views and external writable reads need preparation comparable
to ownership work; they should not be treated as diagnostic-guard removals. If a
small specification restriction would materially simplify one, compare it with
preserving the existing behavior, report consumer impact, and follow the language
policy and review gates above. Difficulty alone does not select a restriction.

### Evidence transfer to the receiving machine

The private LLM packet is now available on this machine under the common Git
directory as `handoff/2026-10-25-private-request-evidence.tar.gz` (832,473 bytes;
SHA-256 `38125adc4a41127f2e348d2688f3b850d84d7459da2511a16e69d26a073e3402`).
It is deliberately outside the public repository and is **not transferred by a
Git fetch**. It was copied privately to this checkout and its archive hash plus all 379
internal checksums were verified on 2026-10-25. It contains the authenticated 362-file consumer source snapshot,
three exact failing variants, passing borrow controls, compact original logs and
replay instructions. Current-compiler reproduction remains pending.

The source binding names private `align-llm` commit
`33e4d3ff40f45f48a4edf2a2f155a9e7966aeaed`; all 362 committed source hashes match.
R37's inline builder variant and R49's mutable-helper variant replace
`src/expert_trace.align`. R80's FILE_SET variant replaces
`src/prompt_source_trust.align`, with `src/prompt_source_trust_smoke.align` as the
check entry. Work in an Align-owned copy. R37 recorded 83,309,188 kB VmHWM:
check headroom, enforce a memory cap, and isolate check/MIR/emission before any
build. The packet also retains the newer R80 nested-loop MTP witness; its own
import revision still needs authentication before replay, and the old snapshot
must not be substituted for that evidence.

AM-R22/23's current source is committed in private `align-media` at
`f9948006c965ead29865d0008c86061919e21cf0`, verified present on GitHub with a clean
matching local tree. Read `src/image_http.align`, `src/image_server.align`,
`src/image-http-owner.align` and `validation/image-http-owners.py` from that
commit on the receiving machine. This supplies portable source evidence while
the register's older request draft still says its URLs are missing. The current
owner qualifies the HEAD-close workaround, not a timed HEAD response or model
cancellation. Use an Align-owned source copy for provider reproduction; follow
the request register for subsequent consumer acceptance. Do not upload private
consumer source into public Align docs or tests. Consumer pins, code, tests and
adoption remain external.

Unknown/multiple-target mutable outputs, fully escaping function values and
view-bearing builder elements retain their own unselected boundaries. R78 enum
fields, R128 optional-resource shared matching and std.id scalar admission keep
the friction-ledger prerequisites. R149 remains a non-blocking numerical-policy
discussion without a selected kernel rewrite.

PostgreSQL COPY, pkg.kv TLS/wider Redis operations, new cloud/transport surfaces
and the first pkg.web consumer retain their individual consumer/design gates.
The execution/storage/startup candidates in
[Plan31](docs/impl/31-execution-storage-startup-plan.md) require measurements and
named consumers; S2 and later are not scheduled. The
[build-performance plan](docs/impl/21-build-perf-plan.md) contains many completed
items and is not an unstarted queue.

## Baseline and source ownership

M0–M15, LLVM22, separate compilation, caches, parallel codegen, ThinLTO and PGO
are implemented. Existing package and optimization deliveries are recorded in
[the roadmap](docs/impl/07-roadmap.md) and their narrow ledgers. v0.11.0 collects
capabilities through Plan238; Plan240 is a later main delivery, not a new tagged
release. The former long completed-capability narrative is preserved in the
[2026-10-25 archive](docs/archive/HANDOFF-2026-10-25.md).

Read requests and relevant source in all three sibling consumers. Consumer
code, tests, compiler pins, branches and adoption remain consumer-owned. Align
may record provider answers in their request registers; leave those edits
uncommitted unless asked. A release build does not authorize a versioned release.

## Verification and continuation

Follow [CLAUDE.md](CLAUDE.md) for design, review, gates, publication and cleanup.
Use `scripts/cargo.sh`; keep Clippy in `target/clippy` and reuse compatible
build/test caches. Rebuild the workspace after runtime changes before native
driver owners. Select owner checks from [the test policy](docs/impl/16-test-policy.md).
DB-boundary changes require `scripts/db-verify-local.sh` before pushing.

Bind review and preflight to the final candidate and merge base, then publish
without changing that candidate. Non-normative Markdown-only handoff maintenance
needs `git diff --check` and at most one relevant consistency check; it does not
need compiler tests. At a completed request batch run exactly
`cargo build --release --workspace` with the required LLVM/native environment.
Preserve parked source and required evidence; remove obsolete task artifacts.
