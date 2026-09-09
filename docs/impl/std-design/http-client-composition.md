# HTTP client ownership across package boundaries

> English is authoritative. Japanese mirror: `ja/http-client-composition.md`.
> Status: proposed, 2026-09-09. This is the next S3/SigV4 prerequisite after
> the implemented named-time and path-encoding family.

## Evidence and capability boundary

The current compiler accepts `http.client()` and `http.request(...)` only as
inferred local owners. It cannot name either in a helper signature, cannot
return a request through `Result`, and cannot name a parsed response. The
S3 composition probe `prepare() -> Result<http_request, Error>` followed by
`send(borrow mut client: http_client, req: http_request) -> Result<http_response, Error>`
produces unknown-type diagnostics for all three names and rejects the request
payload. This is an unimplemented library composition capability, not a
proposal to change the language's ownership or parallel model.

One implementation adds the complete client/request/response function boundary.
A signer can return a fallible request, a caller can reuse its explicit client,
and a response inspector can borrow the completed response. This is independently
useful for ordinary HTTP packages before S3 exists. Splitting type spellings,
payload formation, native consumption, and imported helpers would expose dormant
or unsound intermediates. The implementation may exceed 1,000 handwritten lines:
one shared ownership/serialization proof across the three types has less
integration risk than repeating the same compiler gates for each type.

## Authoritative public-contract ledger

The records below are exact. No alias, new constructor, implicit client, credential,
clock, retry policy, HTTP protocol change, or S3 API is introduced.

| Surface | Inputs, defaults, results and errors | Ownership, lifetime and allocation | Owner, identity, prerequisite and acceptance |
|---|---|---|---|
| `http_client` | Global arity-zero builtin type spelling for the existing `http.client()` result. No import is needed to name the type; construction still requires the existing import. Wrong arity is a compile error. | Nominal opaque Move owner of the existing pool. Moving transfers it without allocation; Drop calls the existing client free exactly once. No Copy, equality, printing, raw conversion or implicit clone. | Sema spelling and `Scalar::HttpClient`, existing `Ty::HttpClient`; MIR/interface/codegen ownership traversal. H1, O1, I1. |
| `http_request` | Global arity-zero builtin type spelling for the existing `http.request(method: str, url: str)` result. Construction remains total; existing serialization/transport validation and setter aborts remain unchanged. Wrong arity is a compile error. | Nominal opaque Move owner of method, URL, headers and copied body. Helpers may construct and return it. Move is allocation-free; unfinished Drop uses the existing request free exactly once. Constructor/setters retain their existing explicit copies; no caller view is retained. | Sema spelling and `Scalar::HttpRequest`, existing `Ty::HttpRequest`; same ownership and interface owners. H1, O1, W1, I1. |
| `http_response` | Global arity-zero builtin type spelling for the existing successful `http.parse` and whole-body HTTP results. `http.response(status)` continues to construct the distinct existing `response_builder`. Wrong arity is a compile error. | Existing nominal opaque Move response; no new scalar representation. Header/body views borrow its exact storage root. Moving transfers its retained data without copying. Drop uses the existing parsed-response free. | Sema spelling and existing `Scalar::HttpResponse`/`Ty::HttpResponse`. H1, O1, V1, I1. |
| Owning carriers for all three | Bare locals, by-value parameters and returns; recursively owning records, user sums, tuples, `Option` and `Result`; source-formed fixed arrays of recursively owning Move records use the existing move-path carrier rules. All generic instances obey the same rules after substitution. | Each reachable live owner is moved, source-nulled and dropped exactly once, including partial moves, replacement and early exits. Constructors are the only safe source of a fresh handle. Borrowing a carrier does not mint ownership. | Existing recursive owner machinery gains the two missing scalar leaves. Direct handle collection elements, dynamic collections of owning records, boxes of owned values, constants/globals, escaping captures, tasks/parallel captures and public extern signatures stay rejected under their existing policies. The already shipped `array<http_response>` batch representation remains supported; this change does not add a client/request array representation. H1, O1, M1. |
| Borrowed helper parameters | `borrow` and `borrow mut` use the existing parameter syntax and call-bounded lifetime inference. A borrowed owner cannot be consumed, returned as owned, dropped or replaced by the callee. A returned response view retains the input response root; existing dependent raw/SSE stream carrier and origin rules still apply. | No allocation, retain count or new native shell. The caller remains the owner. Mutable loans exclude overlapping shared/mutable access and move/Drop/replacement for the loan's lifetime, including retained streams. | Sema borrow/move/region checks and imported ownership summaries. V1, M1, I1. |
| Client methods | Existing `get`, `post`, `request`, `request_stream`, `get_many`, `timeout`, and `max_response_body_bytes` require exclusive authority when the bound receiver is a borrowed parameter: `borrow mut`, never shared `borrow`. Owned local and by-value parameter receivers keep the existing implicit handle-mutation convention; binding `mut` is not newly required. All methods still require a bound local receiver. | A complete response owns its data independently of the client; a streaming response retains its existing client loan. Both request-taking methods consume the request exactly once even on transport failure. No access to the client may invalidate an outstanding dependent stream. | Existing method HIR and native operations; exclusive receiver checks and region transport must include imported/generic helpers. No effect change: network methods remain Impure, setters Pure. M1, V1, W1. |
| Request methods | Existing `header`, `body`, `timeout`, and `max_response_body_bytes` require `borrow mut` on a borrowed bound-local receiver. `serialize() -> Result<buffer, Error>` is read-only and accepts shared `borrow`; it returns independent owned bytes and does not consume the request. Owned/by-value receiver convention and bound-local rule remain unchanged. | Setter inputs are copied during the call and retain no input loan. Serialization owns its output. Consuming a shared or mutable borrowed request is forbidden; only an owning source may be passed to the native consuming request operation. | Existing operations and allocation/error contracts. M1, W1, V1. |
| Response methods | Existing `status() -> i64`, `header(name: str) -> Option<str>`, `body() -> slice<u8>` accept a shared borrowed bound local. Existing receiver rules otherwise remain unchanged. Header lookup retains no name argument. | Status is independent Copy data; header/body views retain the response root through helper return, fields, Option, joins and imports. Replacing/moving/dropping that root while a view is live is rejected. The completed response does not retain a client/request/input-buffer loan. | Existing response region producer and imported retained-root summaries. V1, I1. |

Calling a helper with a `borrow mut` parameter still requires mutable caller
storage (`mut client := http.client()`). This is distinct from invoking an
existing handle method directly, which keeps its owned-local convention.

Every call evaluates its receiver/arguments exactly once in source order. Type,
borrow, malformed-HIR and ownership errors are compile-time diagnostics before
native emission. This change adds no runtime error variant or precedence: URL,
header, timeout, cap, transport, TLS, framing and allocation failures retain the
existing HTTP ledger's exact behavior. OOM remains a hard failure.

## Representation and persistence

These are three distinct nominal builtin identities, never structurally equal
merely because each native representation is one pointer. The existing native
constructors, methods, consuming calls and free rows are reused without changing
signatures, physical symbols, runtime fingerprints or capability requirements.
In particular parsed-response Drop is `align_rt_http_resp_free`; the unrelated
`align_rt_http_response_free` belongs to `response_builder`.

Adding client/request scalar leaves must update every exhaustive type/classifier,
layout, ownership, native envelope, producer-certification and interface path.
No new expression/rvalue opcode or native symbol is necessary. Checked HIR/MIR
must reject a client presented as a request or response, a forged borrowed
consumption, and invalid native operand/output storage before code generation;
a pointer-sized layout is not proof of semantic identity.

Canonical scalar serialization adds one-byte leaf tags `49` (`http_client`) and
`50` (`http_request`), with no payload bytes; `http_response` remains tag `27`.
Tags `51..=255` remain unknown. Independent semantic-to-byte and byte-to-semantic
goldens pin `31`, `32`, and `1b` respectively (hex). Truncated enclosing records
and unknown tags are rejected. Interface `FORMAT_VERSION` advances from 10 to 11
under the existing any-encoding-change rule; no sibling identity is renumbered. Builtin nominal identities, ordered
aggregate definitions, parameter modes, generic bodies and retained-root summaries
enter the existing interface/dependency/object cache identity. Cold/warm and
edit/restore compilation must agree with whole-program compilation; old incompatible
interfaces must miss or fail validation, never deserialize as a sibling handle.
No source or artifact I/O is introduced at runtime.

## Implementation closure matrix and owner checklist

The primary new driver owner is `align_driver --test http_client_composition`.
The suffixes below are exact planned tests in that target unless qualified.
Existing owners may close a cell when they discriminate the new failure mode;
record the implementation and final owner mapping here before preflight.

| ID | Applicable invariant / paths | Exact acceptance owner |
|---|---|---|
| H1 | Three names/arity, direct and nested admitted carriers, generic substitution; reject forbidden direct collections, boxes, capture/parallel/extern carriers and wrong nominal type | `formation_and_carrier_matrix` |
| O1 | Construct, move-in/out, selected source nulling, recursive Drop, partial move/reinitialization, replacement, returned Result; `if`, `match`, `else`, `?`, `map_err`, branch/loop joins and early exits | `ownership_control_flow_whole_and_unit` |
| M1 | Mutable versus shared parameter authority for every client/request method; read-only serialization and response access; reject borrowed consumption and same-root overlap through helpers/aggregates | `borrow_authority_and_consumption_matrix` |
| V1 | Response body/header return lifetimes, independent status/serialized bytes/completed response, stream-derived client loan through helper/import/generic paths; move/replacement/Drop invalidation | `retained_views_and_stream_origins` |
| W1 | Returned request serialized and sent from caller-owned client; literal/owned/NUL/binary inputs preserve existing HTTP behavior; success and network-error consumption; pool reuse; whole/unit optimized/unoptimized native output | `package_request_and_pool_wire_round_trip` |
| I1 | Imported/generic ownership modes, complete nominal graph, serialized tags and malformed records, cold/warm/edit/restore cache, borrowed roots not lost in interface replay | `interfaces_and_cache_restore`; `align_mir --lib canonical_field_codec_covers_every_primitive_and_scalar_tag` and `canonical_type_codec` extended for both scalar tags |
| P1 | Checked HIR operand/result identity; exact native access/slot producer certification; sibling-type substitution, missing storage proof and forged borrowed consumption refuse before LLVM | `align_mir --lib hir_body_validator_native`; `align_codegen_llvm --lib xml_mir_gate_authenticates_types_on_every_producer_edge` |
| A1 | Exact pointer ABI and frees; allocation-free move, no double-free/leak on completed or failed consuming calls, partially moved aggregate cleanup | `ownership_control_flow_whole_and_unit` plus existing native HTTP lifecycle owner; use allocation probes where existing structural owners do not discriminate cleanup |

No speed or peak-memory improvement is promised, so no benchmark is a correctness
gate. There is no new runtime allocation path to justify a new global probe ABI.
A lifetime/cleanup test must fail for an actual lost root or duplicate ownership;
counting source spellings is not sufficient ownership evidence.

## Example and consistency obligations

This is an example of the proposed composition surface, not a shipped API claim:

```align
module main
import std.http

fn prepare(url: str) -> Result<http_request, Error> {
  req := http.request("GET", url)
  req.header("accept", "application/octet-stream")
  return Ok(req)
}

fn send(borrow mut client: http_client, req: http_request) -> Result<http_response, Error> {
  return client.request(req)
}

fn main() -> Result<(), Error> {
  mut client := http.client()
  req := prepare("https://example.com/object")?
  response := send(client, req)?
  print(response.status())
  return Ok(())
}
```

The baseline parser accepts this syntax and reports only the missing type/payload
capability. The implementation owner must compile/run the same composition with
an isolated loopback endpoint in whole-program and per-unit modes.

Required synchronized sources: this ledger and Japanese mirror; `http.md` and
its mirror; `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, Settled in
`docs/open-questions.md`, `docs/impl/07-roadmap.md`; the checked-HIR and runtime-ABI
ledgers only if their normative record changes. HANDOFF records the prerequisite
once at the accepted capability boundary. S3's endpoint, credential, canonical
request, signature, response/status and interoperability contract remains a
separate package design after this prerequisite.
