# pkg — auth

> English is authoritative. A synchronized Japanese mirror lives at `ja/auth.md`.
>
> **Status:** design accepted (2026-09-01); implementation pending.

## Authoritative public-contract ledger

This table is the authority for the first `pkg.auth` capability. Later prose and implementation
may make a field more explicit but must not widen it. V1 composes the shipped JSON, encoding,
crypto, and time representations; it introduces no cryptographic primitive, native ABI, key owner,
clock read, or ambient authentication state.

| Public surface | Exact inputs, defaults, validation, and evaluation | Exact result, errors, order, and effects | Ownership, lifetime, allocation, and cleanup | Compiler/runtime/package owner, artifact, and cache identity | Prerequisite and acceptance owner |
|---|---|---|---|---|---|
| `pub Argon2Policy { m_cost: i64, t_cost: i64, parallelism: i64 }` | One flat public record using the same units as `crypto.argon2_params`: KiB, iterations, and lanes. Field/source order is exactly `m_cost`, `t_cost`, `parallelism`. There is no default policy. In `password_hash`, the fields are the exact derivation parameters. In `password_verify`, they are independent inclusive maxima for an admitted stored PHC record. | Copy and Pure. Hash policy admits `parallelism 1..=16777215`, `t_cost 1..=4294967295`, `m_cost 8*parallelism..=4194304`. Verify maxima each admit `1..=` the corresponding engine ceiling; the stored tuple must independently satisfy the hash relation and not exceed any maximum. | Three i64 fields; no borrow, allocation, Drop, secret, or retained state. | `pkg.auth` owns the nominal definition. Ordinary whole-program/per-unit interfaces serialize its name and ordered fields; the complete definition enters interface, dependency, and cache identity. | Shipped flat Copy records and ordinary interface/cache owners; exact field/order/bound tests. |
| `pkg.auth.encode_hs256(claims_json: str, key: slice<u8>) -> Result<string, Error>` | Arguments evaluate once, left-to-right. `key.len() >= 32`; there is no upper key bound or default. `claims_json` is at most 8192 bytes and must be one strict RFC 8259 UTF-8 JSON object with no duplicate semantic member names. A package-owned allocation-free lexical precheck rejects the shipped parser's two known leniencies—unescaped C0 bytes in strings and a leading-zero integer—before `json.doc` parses shape and values. Present `exp` and `nbf` claims must each be integer-form i64 NumericDate seconds. The package header is the exact UTF-8 bytes `{"alg":"HS256","typ":"JWT"}`. | Success is the RFC 7515 compact JWS `base64url(header) + "." + base64url(claims_json) + "." + base64url(HMAC-SHA256(key, signing_input))`, with the URL alphabet and no padding. The exact maximum result is 11004 bytes. Invalid key/claims shape, lexical form, duplicate, time-claim type, or length arithmetic is `Error.Invalid`; package allocation or HMAC failure hard-aborts under shipped contracts. Impure because HMAC is FFI-backed. | Both inputs are borrowed for the call and never retained or mutated. Success owns one `string`; intermediate encoded strings, builder storage, JSON tape, and the 32-byte tag are operation-owned and dropped before return. An error publishes no token. | Ordinary `pkg.auth` source owns the wrapper. There is no checked operation or new ABI row. Whole-program and per-unit capability collection are module-wide: importing any `pkg.auth` function retains the module's existing JSON/base64/HMAC/Argon2/random capabilities and therefore libcrypto, including a session-token-only consumer. Package source/interface and dependency implementation hashes determine cache identity, with no source artifact or ambient input. | Shipped `core.json`, `std.encoding`, and `std.crypto`; canonical RFC vector, strict lexical/JSON/duplicate/bound/key, allocation/Drop, effect, whole/per-unit, and module-capability owners. |
| `pkg.auth.verify_hs256(token: str, key: slice<u8>, now_ns: i64) -> Result<string, Error>` | Arguments evaluate once, left-to-right. Key admission is the same; `now_ns` is a required nonnegative Unix-wall-clock nanosecond value and is never read internally. `token.len()` must be `1..=16384`. The token must have exactly three nonempty unpadded canonical base64url segments; the decoded signature is exactly 32 bytes. | The package first authenticates the exact ASCII `header.payload` bytes with HMAC-SHA256 and a constant-time equal-length comparison. A mismatch is `Error.Denied` before JSON parsing. After authentication, malformed/non-strict header JSON, non-object shape, or duplicate semantic names are `Error.Invalid`; a valid unique object without exactly one string `alg` equal to `HS256`, with `typ` present other than string `JWT`, or with any `crit` is `Error.Denied`. The payload must be a strict JSON object of at most 8192 decoded bytes with unique semantic names. Present `exp`/`nbf` must be integer-form i64 seconds; malformed authenticated payload JSON or claim types are `Invalid`. With `now_s = now_ns / 1000000000`, validity is `now_s < exp` and `now_s >= nbf`; a failed time predicate is `Denied`, checked `exp` then `nbf`. Success returns an owned byte-exact clone of the payload JSON. Base64/structure/bound/key/time-argument failure is `Invalid`. Impure because HMAC is FFI-backed. | Token/key are borrowed only for the call. Success owns one `string`; decoded buffers, JSON tapes, recomputed tag, and builders are operation-owned and dropped on every exit. No unauthenticated header or payload JSON is parsed, returned, logged, or retained. | Same module-wide ordinary package and identity boundary. There is no clock/runtime auth row or ambient provider/configuration input. | Exact interop/round-trip; wrong key/tag/alg/typ/crit; dot/segment/alphabet/padding/signature-length; authenticated strict JSON/duplicate/claim-type/time-edge; no-pre-auth-JSON instrumentation; ownership/effect/whole-per-unit/cache owners. |
| `pkg.auth.password_hash(password: slice<u8>, policy: Argon2Policy) -> Result<string, Error>` | Arguments evaluate once, left-to-right. Empty and arbitrary-byte passwords, including NUL, are valid. Policy uses the exact hash bounds above. The operation draws exactly 16 salt bytes from `crypto.random`, derives exactly 32 bytes with Argon2id version 19 and the supplied policy, and uses no pepper, clock, environment, or process-global policy. | Success is the one canonical ASCII PHC string `$argon2id$v=19$m=<m_cost>,t=<t_cost>,p=<parallelism>$<salt>$<tag>`. Decimal fields have no sign or leading zero. Salt/tag use the standard RFC 4648 alphabet without padding and are exactly 22/43 characters. Invalid policy is `Error.Invalid` before randomness. Argon2 provider/context/output-reserve/derive engine failure is exactly `Error.Code(0)`; package builder/string allocation and random failure abort. Impure because randomness and Argon2 are native operations. | Password is borrowed and never retained or copied by package code. Success owns one `string`. Salt buffer, 32-byte derived buffer, encoded strings, and builder storage are operation-owned and dropped before return. Existing buffer/string Drop does not promise zeroization; V1 adds no second secret-container model. | Ordinary package source owns construction and parsing helpers. Existing random, Argon2id, and base64 owners remain the only runtime rows, retained module-wide as above. Source/interface/dependency hashes own artifact/cache invalidation. | Shipped random/Argon2id/base64; independent PHC vector/parser, policy boundaries, salt-shape/randomness, NUL/empty password, exact `Code(0)`, cleanup/effect/capability owners. |
| `pkg.auth.password_verify(password: slice<u8>, phc: str, maximum: Argon2Policy) -> Result<bool, Error>` | Arguments evaluate once, left-to-right. Password admission matches `password_hash`. Verify maxima are validated before the PHC input. The parser accepts only the exact canonical string emitted above: exact identifier/version/parameter names and order, canonical positive decimal, exactly five `$` separators, no trailing bytes, 16 decoded salt bytes, and 32 decoded tag bytes. Stored parameters must satisfy the hash-policy bounds and be no greater than the three caller maxima before Argon2 runs. | Success is `Ok(true)` only when the recomputed 32-byte tag matches through `crypto.constant_time_equal`; a wrong password is `Ok(false)`. Invalid maxima, malformed/noncanonical/unsupported PHC, or a stored cost over the caller limit is `Error.Invalid`, all before derivation. Argon2 provider/context/output-reserve/derive engine failure is exactly `Error.Code(0)`. Impure because Argon2 is FFI-backed. | Inputs are borrowed and never retained. Decoded salt/tag, derived tag, and parser-owned buffers are dropped on every result. Comparison covers all 32 bytes. The same existing no-zeroize caveat applies; no partial password, PHC, or tag is returned. | Same module-wide ordinary package/cache boundary; only existing base64, Argon2id, and constant-time-equality machinery is used. No algorithm registry or native PHC parser is introduced. | Canonical accept/reject mutation matrix, multi-invalid precedence, exact/over-limit costs with a no-KDF probe, true/false comparison, exact `Code(0)`, Drop/effect/whole-per-unit/cache owners. |
| `pkg.auth.session_token() -> string` | No arguments, defaults, clock, seed, namespace, prefix, or ambient store. It draws exactly 32 bytes from `crypto.random`. | Returns their canonical unpadded base64url encoding: exactly 43 ASCII characters in `[A-Za-z0-9_-]`, carrying 256 random bits. No uniqueness guarantee is made; collision probability follows the OS CSPRNG. Random/allocation failure hard-aborts under shipped contracts. Impure. | The return is one ordinary owned `string`. The transient 32-byte buffer is dropped after encoding. The returned bearer secret uses ordinary string Drop and is not zeroized. | Ordinary package source over the existing random and base64url rows. No session registry, cookie, expiry, clock, storage, rotation, or revocation owner exists. | Shipped random/base64url; exact length/alphabet/decode, distinct-sample, capability/effect, and Drop owners. |

## Decision and scope

`pkg.auth` v1 is the smallest security package that removes repeated protocol-sensitive assembly
without creating another cryptographic substrate:

```text
shipped JSON + base64url + HMAC + explicit now_ns  -> authenticated HS256 claims JSON
shipped CSPRNG + Argon2id + standard base64       -> canonical bounded PHC password record
shipped CSPRNG + base64url                         -> opaque 256-bit session token
```

Claims remain JSON text. The package does not own an application schema, issuer, audience, subject,
role, cookie, session store, or revocation rule. Password policy is a caller value, and verification
also receives the maximum native work it may accept. The wall clock is similarly a caller value:
`verify_hs256` never calls `time.now()`, which keeps replay tests deterministic and prevents a
hidden time read.

HS256 is the only JWT algorithm in v1. The asymmetric primitives now shipped in `std.crypto` are
lower-level prerequisites for later issuer/JWKS work, not permission to add an algorithm selector,
network fetch, key cache, or provider policy here. The existing `pkg.jwt` prototype is replaced
outright when this capability implements; no deprecated alias or parallel old/new package remains.

## Public use

Declarations and positional calls are shown separately:

```align
import pkg.auth

fn issue(claims: str, key: slice<u8>) -> Result<string, Error> =
  pkg.auth.encode_hs256(claims, key)

fn accept(token: str, key: slice<u8>, now_ns: i64) -> Result<string, Error> =
  pkg.auth.verify_hs256(token, key, now_ns)
```

```align
import pkg.auth

fn store(password: slice<u8>) -> Result<string, Error> {
  policy := pkg.auth.Argon2Policy{m_cost: 65536, t_cost: 3, parallelism: 1}
  return pkg.auth.password_hash(password, policy)
}

fn check(password: slice<u8>, stored: str) -> Result<bool, Error> {
  maximum := pkg.auth.Argon2Policy{m_cost: 262144, t_cost: 6, parallelism: 4}
  return pkg.auth.password_verify(password, stored, maximum)
}
```

```align
import pkg.auth

fn new_session() -> string = pkg.auth.session_token()
```

No example relies on named arguments, overloads, implicit imports, a clock read, or unimplemented
syntax. String passwords and keys cross the binary boundary explicitly through `.bytes()`.

## JWT grammar, validation, and precedence

`encode_hs256` validates the key before claims bytes, applies the strict lexical precheck below,
parses the complete claims document in one scoped arena, proves object shape and semantic-key
uniqueness, then checks `exp` followed by `nbf` when present. The fixed 36-byte encoded header, two dots, 43-byte signature, and at most 10923-byte
encoded payload produce at most 11004 output bytes. It computes all output-length arithmetic before
HMAC or allocation. Exact-limit output succeeds. The package then forms header and payload encodings, the exact signing input, HMAC tag,
and final token. A failure publishes no partial string.

The allocation-free strictness precheck is deliberately narrower than a second JSON parser. It
walks the UTF-8 bytes with `outside`, `string`, and `escaped-byte` states. Within a string it rejects
every unescaped byte `0x00..=0x1f`; a backslash protects exactly the next byte for this scan, while
`json.doc` still validates the actual escape and Unicode grammar. Outside strings, only at a JSON
value boundary (start of input, or after JSON whitespace, `[`, `{`, `,`, or `:`), it rejects a
number beginning `0` followed by another decimal digit, or `-0` followed by another decimal digit.
`json.doc` then owns every other token, nesting, UTF-8, escape, number, separator, trailing-input,
and object-shape rule. This closes its two documented systemic leniencies without changing the
shared parser or accepting a second semantic JSON model.

`verify_hs256` has one fixed order:

1. Reject a short key, negative `now_ns`, empty/oversized token, or invalid length arithmetic.
2. Find exactly two dots and require three nonempty segments. Reject `=`, a non-URL alphabet byte,
   an impossible length, or noncanonical trailing bits. Decode the signature and require 32 bytes.
3. Recompute HMAC over the original first two segments and compare all 32 bytes. A mismatch returns
   `Denied`; no header or payload decode/JSON work has occurred.
4. Decode the authenticated header and cap its decoded range by the token bound. Apply the strict
   lexical precheck, parse one object, and reject duplicate semantic names; any failure here is
   `Invalid`. Then require `alg: "HS256"`, require an optional `typ` to be `"JWT"`, and reject any
   `crit` member because v1 implements no critical extension; these valid-document policy failures
   are `Denied`.
5. Decode the authenticated payload, require at most 8192 bytes, apply the strict lexical precheck,
   parse one object, and reject any duplicate semantic name. A lexical/malformed document or
   present non-i64 `exp`/`nbf` is `Invalid`.
6. Compute `now_s = now_ns / 1000000000`. Check `exp` first (`now_s < exp`), then `nbf`
   (`now_s >= nbf`). A failed predicate is `Denied`. Missing claims impose no constraint.
7. Clone the exact decoded payload bytes into the sole published result and drop every temporary.

Only integer-form JSON numbers are admitted for these two NumericDate claims. Fractions and
exponents are rejected even when numerically integral. Other registered or private claims remain
application-owned and are preserved byte-for-byte; `iat`, `iss`, `aud`, `sub`, and `jti` acquire no
implicit policy.

## PHC grammar, work bounds, and precedence

The canonical password record is:

```text
$argon2id$v=19$m=<positive decimal>,t=<positive decimal>,p=<positive decimal>$<22 base64 chars>$<43 base64 chars>
```

The alphabet is standard unpadded base64 (`A-Z a-z 0-9 + /`), not base64url. The fixed decoded
salt/tag sizes and canonical decimal rule leave one spelling per semantic record. `password_verify`
rejects padding, whitespace, alternate parameter order/names, absent/extra parameters, versions,
algorithms, separators, leading `+`, and leading zero. Base64 decode must consume the entire field
and prove canonical trailing bits.

Hash validates its exact policy before random work. Verify validates the three independent caller
maxima before reading PHC, then grammar, decimal overflow, engine bounds and `m_cost >=
8*parallelism`, caller maxima, base64 salt, and base64 tag, in that order. Only after all public
checks pass does it call Argon2 once. This makes an attacker-selected PHC unable to request work
beyond the visible call-site ceiling. All public invalidities return `Error.Invalid`; only the
underlying provider can return `Error.Code`. Tag mismatch is ordinary `Ok(false)`.

`password_hash` always uses a fresh 16-byte salt and 32-byte result. Argon2's existing native
provider/context/output-reserve/derive engine failures propagate as exactly `Error.Code(0)`; an
allocation in package-owned builder/string assembly follows ordinary hard-OOM behavior. There is no default work
factor, hidden pepper, automatic policy upgrade, password normalization, UTF-8 requirement, or
prehash. Callers that use a pepper must compose it explicitly before this boundary. A later
rehash-advice surface would need its own exact return and migration policy.

## Ownership, allocation, effects, and secrets

Every byte/text input is a call-bounded borrow. No public result contains a view, so strings can be
moved, returned, stored, or dropped through ordinary control flow without retaining the key,
password, token, PHC input, JSON input, or arena. Success returns one ordinary owned string except
password verification, which returns Copy `bool`. Every recoverable error publishes no owned
result, and normal `Result` cleanup covers `if`, `match`, `else`, `?`, `map_err`, branch/loop joins,
replacement, early return, and unused values.

All five functions are Impure. Four call an FFI-backed HMAC or Argon2 operation; `session_token`
calls the OS CSPRNG. JSON and encoding helpers remain Pure internally, but wrapper purity is the
transitive result. There is no concurrency, global mutable state, clock read, filesystem, network,
environment, terminal, source/artifact I/O, or provider selector.

V1 inherits ordinary buffer/array/string Drop. It does not claim zeroization for the returned PHC,
JWT, or session string, nor for package-owned salt/tag temporaries. Key and password inputs are not
copied by package code. A zeroizing secret owner remains a separate core/std contract; duplicating
one locally would violate the one ownership model.

## Package, runtime, artifact, and cache boundary

The vendorable subtree owns one ordinary module, `pkg.auth`, one public Copy record, and five public
functions. It imports `core.json`, `std.crypto`, and `std.encoding`. There is no `internal` module,
native declaration, compiler-recognized source shape, checked-HIR discriminator, runtime ABI row,
reflection table, static artifact, schema input, or environment-derived option.

Direct, imported, local/function-field, and control-joined function-value calls use ordinary Align
function semantics. Whole-program compilation sees the bodies directly. Per-unit compilation
serializes the public signatures and `Argon2Policy` definition, while the package object's ordinary
implementation/dependency hashes retain the existing runtime capabilities. Capability collection
is intentionally module-wide rather than call-reachability-based: every `pkg.auth` import retains
the root module's JSON/base64/HMAC/Argon2/random set and therefore libcrypto, even when the consumer
calls only `session_token`. This is an existing compilation-unit property, not a new hidden selector.
Editing package source or the public definition invalidates the normal unit/object/link cache keys;
the same-named function in another module has no special meaning. A project without `pkg.auth`
retains no auth-specific code and receives the normal unresolved-import diagnostic if it imports it.

No canonical byte format is newly persisted by compiler or runtime. JWT and PHC wire bytes are
package outputs with independent external vectors. The implementation adds no row to
`19-hir-validation-ledger.md` or `20-runtime-abi-ledger.md`; those ledgers remain unchanged because
all trust boundaries are already shipped and reached through ordinary calls.

## Complexity and performance boundary

JWT work is linear in bounded token/claims bytes except semantic duplicate detection, which is a
bounded quadratic comparison over object member names. Password work is dominated by the explicit
Argon2 policy stored in or limiting the PHC. Session-token work is fixed at 32 input and 43 output
bytes. There is no throughput, latency, allocation-count, zeroization-cost, or memory-ratio promise,
so no benchmark is an acceptance gate.

## V1 non-goals and later boundaries

No HS384/HS512, RSA/ECDSA/Ed25519 JWT, generic algorithm selector, JWK/JWKS/OIDC/OAuth client,
issuer/audience/scope/role policy, key fetch/cache/rotation, refresh token, cookie, CSRF, session
store/index/revocation/expiry, password reset, MFA/WebAuthn/TOTP, pepper store, password
normalization/rules, policy auto-upgrade, rehash advice, PHC algorithm agility, scrypt/bcrypt/PBKDF2,
secret zeroizing string, user database, HTTP middleware, or clock read is included. Each requires a
consumer and a separate exact ledger; asymmetric JWT additionally needs a key-discovery/trust
contract rather than merely calling a shipped signature primitive.

## Implementation closure matrix

The implementation is one independently useful package capability and is expected to stay below
roughly 1,000 changed hand-written lines. Splitting its source from its owner tests would publish no
separately useful consumer boundary, while no compiler/runtime producer chain is involved.

| Axis | Required closure | Owner evidence |
|---|---|---|
| Public formation and identity | Exact module, record fields/order, five signatures, core `Error`, import/name/type/arity rules, ordinary direct/imported/local/function-field/joined-indirect calls, whole/per-unit definitions. | Package source compilation, negative import/signature fixtures, interface/hash and target-shape parity. |
| JWT encode bytes | Fixed header, canonical unpadded segments, exact signing input/tag, 8192-byte claims and 11004-byte result arithmetic, no partial result. | Independent RFC HS256 vector plus external decoder; empty/exact/next bounds; output segment decode oracle. |
| JWT authentication and policy | Key/time/token precedence; exact three segments; canonical signature; MAC before JSON; allocation-free C0/leading-zero precheck; unique header/payload keys; malformed-header `Invalid` versus alg/typ/crit `Denied`; exp then nbf boundaries; exact payload clone. | Parameterized mutation matrix, raw C0 and leading-zero encode/authenticated header/payload fixtures, duplicate escaped-key fixtures, HMAC/JSON call instrumentation, second/nanosecond edge and error-kind assertions. |
| PHC construction and parser | Exact version/parameter grammar/order/decimal/base64/salt/tag; one random salt; fixed output; every policy boundary. | Independent Argon2id/PHC vectors and parser oracle; one-byte grammar mutations; deterministic injected-random fixture; real-random shape test. |
| Password resource and comparison | Maximum validation before PHC; stored engine relation and three inclusive ceilings before one KDF; NUL/empty bytes; CT 32-byte true/false; exact `Error.Code(0)` preservation for native engine/output-reserve failure. | No-KDF over-limit probe, KDF call count, exact/next maxima product, wrong-password and provider/context/output-reserve/derive failure owners. |
| Ownership and effects | Every temporary dropped on success/Invalid/Denied/Code; no retained borrow; return/control/move paths; all functions Impure; no secret-dependent package comparison. | Allocation/Drop parity, MIR effect/call checks, control-flow matrix, constant-time owner reuse. |
| Capability, compatibility, and cache | Existing crypto/encoding/JSON/time semantics and ABI unchanged; every imported `pkg.auth` module retains its complete existing capability set and libcrypto, including session-only use; package absence unchanged; source/interface/dependency edits invalidate normal keys. | Module-wide capability-linking session/JWT/password positives and no-package negative, existing crypto/encoding/JSON controls, add/remove/edit/revert cache twins, optimized/unoptimized and whole/per-unit runs. |
| Session token | Exactly 32 CSPRNG bytes to 43 canonical base64url characters; no clock/prefix/store; ordinary owned-string Drop. | Decode-to-32 oracle, alphabet/length, multi-sample inequality sanity check, random capability/effect/Drop checks. |

## Sources of truth and author consistency pass

This English ledger, `docs/impl/pkg-design/ja/auth.md`, `draft.md`,
`docs/language-spec.md`, `docs/design-notes.md`, `docs/history.md`,
`docs/open-questions.md`, `docs/impl/07-roadmap.md`, and `HANDOFF.md` must agree before
implementation. The HIR/runtime ledgers must remain unchanged unless implementation discovers a
genuine new compiler/native boundary, which would reopen this design.

Author-side pass for the accepted design:

- every public argument/result has one exact type, evaluation order, default, ownership, lifetime,
  allocation, cleanup, error, and effect rule;
- JWT header/payload/signature, authentication state, optional claim state, and every malformed,
  denied, unavailable, and successful field-presence case have exact rules;
- PHC algorithm/version/parameter/limit/salt/tag products have one grammar and unavailable-result
  rule;
- all text/native inputs fix UTF-8 or arbitrary-byte treatment, embedded NUL behavior, validation
  order, and pre-side-effect semantics;
- multi-invalid calls have deterministic key/time/bound/shape/auth/JSON/claim and maximum/PHC/KDF
  precedence;
- no clock, policy, key source, pepper, provider, configuration, artifact, source, session store,
  or randomness source is ambient;
- all wire scalar text, sequence order, malformed rejection, and independent semantic-to-byte and
  byte-to-semantic vectors are fixed;
- runtime inspection uses only producer-owned existing primitive arguments and no reflection or
  artifact/source I/O;
- examples use accepted syntax and separate declarations from positional calls; and
- acceptance owners cover every ledger invariant, with no unpromised benchmark used as a gate.

## Design-review finding-to-fix ledger

The 2026-09-01 independent full-diff review produced one P1 and four P2 findings. The accepted
ledger resolves the complete set before implementation:

| Finding | Ledger-first resolution and propagation |
|---|---|
| P1: `json.doc` accepts raw C0 string bytes and leading-zero numbers | Added one allocation-free package lexical precheck, exact state/boundary rules, `Invalid` precedence, and encode plus authenticated header/payload owners. Existing shared JSON behavior remains unchanged. |
| P2: capability retention was promised per used body | Corrected the package/runtime and closure rows to the actual module-wide whole/per-unit collection: every `pkg.auth` import retains libcrypto and the complete module capability set, including session-only use. |
| P2: malformed authenticated-header error conflicted | Fixed lexical/parse/non-object/duplicate header failures as `Invalid`; only valid-document alg/typ/crit policy failures are `Denied`, in both ledgers and the owner matrix. |
| P2: Argon2 output allocation was described as aborting | Preserved the shipped native provider/context/output-reserve/derive mapping as exact `Error.Code(0)` and distinguished later package-owned hard OOM. |
| P2: candidate and Settled states conflicted | This fix closes the reviewed findings and marks the English ledger, Japanese mirror, roadmap, handoff, Settled record, and history uniformly accepted. |
