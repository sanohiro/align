# Nameable HTTP listener ownership

Status: implemented for align-llm R161. The existing listener already has a nominal
type, Move classification, scalar carrier, interface encoding, native owner and
Drop. Source type resolution omits its spelling. This capability exposes that
identity through the existing ownership paths; it adds no listener wrapper.

## Public contract

| Surface | Exact contract |
| --- | --- |
| Type | `http_server` is the existing nullary, globally nameable Move type produced by `http.serve(host: str, port: i64) -> Result<http_server, Error>` and `http.serve_shared(host: str, port: i64) -> Result<http_server, Error>`. Type arguments are rejected. Existing builtin/local-type precedence applies. |
| Ownership | Owning parameters, returns, ordinary record fields and existing Option/Result carriers transfer the listener exactly once. Moving a direct field clears only that field; replacing or dropping an owner releases its remaining listeners. Moves introduce no allocation or native operation. The listener retains no input-text borrow. |
| Borrowed use | `fn receive(borrow server: http_server) -> Result<http_request_ctx, Error>` may call `server.accept()` repeatedly. A borrowed helper may receive a stable record field. Existing configuration setters require owned local or exclusive borrowed authority. Shared borrowing never permits consuming or replacing the listener. |
| Receiver boundary | Existing methods require a bound local receiver, including a borrowed parameter. This nameability change does not widen direct field/temporary method receivers or partial exclusive field arguments. A record field can be inspected through the shared helper above or transferred into an owned local. |
| Context lifetime | Accepted contexts own their connection and retain the existing parking-owner lifetime independently of the source listener's lexical binding. Naming the listener adds no context-to-local loan. Existing request views remain tied to their context. |
| Exclusions | Existing direct array/slice/box, capture, task/parallel, extern and unsupported carrier restrictions remain. A listener cannot be copied, consumed through a borrow, or reused after a move. No new resource reference, capture mode or type syntax is introduced. |
| Errors and inputs | The spelling has no runtime inputs/defaults. Formation and ownership errors are diagnosed before execution. Host/port validation, encoding/NUL handling, bind/accept/configuration errors and their precedence remain the HTTP contract. |
| Owners and identity | Sema resolves the spelling; existing checked HIR, MIR, LLVM and runtime paths own transport/cleanup. The existing nominal `HttpServer` tag, pointer ABI and free symbol remain unchanged. Existing compiler-byte/source/interface identities invalidate changed compilation; no format/version, cache key, artifact or runtime table is added. |
| Prerequisites and performance | All prerequisites are shipped. No throughput or resource improvement is promised, so no benchmark is required. Existing native allocation and cleanup behavior is preserved. |

## Implementation closure matrix

One capability contains name resolution, its ownership qualification and public
documentation. No producer-only seam is useful. The primary new owner is
`align_driver --test http_server_nameable`.

| Axis | Implementation and exact owner |
| --- | --- |
| Formation, arity, nominal identity, record/Option/Result transport and forbidden carriers | `BUILTIN_SPELLING_TYS`, `resolve_type` and independent `validate_hir::resolve_type_ok` declaration admission; other carrier predicates remain unchanged. `formation_and_authority` parameterizes whole/per-unit admission and rejection. `align_mir --lib http_server_header_placement` covers independently built local/import/function-value headers; the existing body-only negative retains CliParsed. |
| Construction, move-in/out, field source nulling, Drop, replacement and return | Existing scalar/record lifecycle; `ownership_and_cleanup` runs repeated listener acquisition/release across owning/imported helpers and record extraction/replacement, and proves the port can be rebound before process exit. |
| `if`, `match`, `else`, `?`, `map_err`, branch/loop joins and early exits | Existing Move and Result paths; `ownership_and_cleanup` exercises selected moves, fallible returns, repeated loops and early errors, with negative twins in `formation_and_authority`. |
| Shared versus exclusive authority; move/replace during live borrowed use | Existing parameter modes and receiver gates; `formation_and_authority` covers borrowed accept, shared setter/consumption rejection, exclusive whole-owner replacement and unchanged field/temporary restrictions. |
| Real accept, malformed request, bind failure, response after helper return and listener retirement | Existing HTTP runtime; `imported_accept_and_context_lifetime` executes real loopback requests through imported generic helpers in whole/per-unit modes, then confirms listener/context cleanup and later bind. Existing runtime HTTP owners retain framing/failure-path coverage. |
| Generic monomorphization, function-value signatures, interfaces and cache replay | Add the spelling to the interface's nullary opaque builtin inventory; existing nominal scalar serialization and callable ABI remain unchanged. Positive/negative generic helpers and signatures in `formation_and_authority`; `interfaces_and_cache` covers cold/hit/edit/restore, and the interface builtin parity owner pins source/import agreement. |
| Malformed HIR/MIR and native provenance | No IR or access-policy change. The independent header gate must admit the now-nameable HttpServer. Existing native-core malformed-HIR owners must accept owning/exclusive typed server parameters while still rejecting shared setters; exact HTTP operand/result and producer validation remains authoritative. The source owner reaches these paths in both compilation modes. Any discovered missing invariant must be repaired across the existing class before admission. |
| Allocation parity | No runtime/representation change; moves retain existing pointer/cleanup-bit operations. Native port reuse and process success catch leaked/double-closed listener ownership; existing runtime allocation/fd owners retain native allocation coverage. |

Author ledger-to-prose pass: the table fixes every changed public surface and
its lifetime, ownership, errors, allocation, identity and acceptance. There is
no new wire/text boundary, persisted format, global-state operation or milestone.
Normative examples are compiled in the owner. Synchronize HTTP EN/JA ownership
sections and the existing HTTP summaries in `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md` and Settled in `docs/open-questions.md`; record delivered
status once in HANDOFF and the affected external request register. Unchanged
runtime/HIR ledgers require no inventory update.

The independent strategy review found one missing formation consumer: the
checked-HIR declaration gate explicitly classified HttpServer as body-only.
The matrix now includes that admission and typed authority owners. Author
verification also identified the interface builtin-name inventory; it is
updated with the same nullary opaque identity and covered by the existing
cross-inventory spelling assertion. No ownership strategy changed.
