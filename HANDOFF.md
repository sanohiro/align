# Session handoff

## Current direction

K1's selected compiler repair is complete under
[Plan240](docs/impl/240-call-boundary-view-authority.md). All three consumers
record adoption of that compiler on 2026-10-25, within their named native
acceptance boundaries. Do not restart the relational-solver investigation or
interpret its older handoff as unfinished production work.

The owner's 2026-10-25 instruction selects the remaining assessed work except
R132 and R142: AM-R24, AM-R22, R28 text-builder views, AM-R16/R137 retained
indexed-field views, AM-R23 and AM-R07, plus evidence-dependent R37/R49/R80
investigations. Start with AM-R24, then bounded HTTP writes, read-only views,
connection observation and external writable reads; adapt order to proven
dependencies. Complete verification, review and merge for each capability before
starting the next. Missing evidence is not a completed fix. Consumer adoption
remains external, and this batch does not select the wider long-term backlog.

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

The selected batch follows the scope above; R132/R142 below are explicitly
excluded. [Plan230](docs/impl/230-request-follow-up-plan.md) retains the historical
request reconciliation; Plan240 supersedes its pre-K1 call-authority blocker.

- Media AM-R24 reports refusal to return a raw-loaded Copy C record containing
  fixed numeric arrays. Its register includes a reduced source and explicit
  reconstruction control. Recheck both on current main before classifying the
  repair; the recorded failure uses an older compiler. Preserve genuine borrowed
  escape refusals and qualify whole/per-unit/native return transport.
- Media AM-R22 requests bounded, HEAD-correct HTTP response writes. AM-R23
  requests non-consuming connection observation before response. Both are
  non-blocking drafts with current workarounds; the register still lacks shared
  reproducer URLs. Exact deadline, half-close, connection reuse and cleanup
  contracts precede implementation. Connection observation does not itself
  deliver model cancellation or GPU cleanup.
- R28 text-builder byte views and AM-R16/R137 retained indexed owning-field views
  still need dedicated producers and lifetime/invalidation proofs. Plan240
  supplies ordinary-call read-only authority; it does not deliver these views.
  Independent Copy builder reads, indexed Copy reads and scalar lengths already
  ship. Consult Plans209/210, 232 and 196/205 without reopening those deliveries.
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
- LLM R37/R49/R80 remain evidence-dependent investigations under Plan230. Obtain
  their exact source/log packets and recheck current main before choosing a fix;
  do not infer closure from K1. R37 reports roughly 79.5 GiB peak memory at its
  recorded revision, so reproduction needs explicit resource limits.

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
