This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.crypto — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/crypto.md)

> **Status:** complete for the M11 symmetric/hash/KDF surface. The post-pkg.db asymmetric signature
> suite is designed below; implementation is pending. The documented BLAKE3 exception remains
> deferred.

## Overview

crypto.random, sha256/sha512, blake3, hmac, hkdf, argon2id, aes_gcm, chacha20_poly1305,
constant_time_equal (draft §18.2). **The hard requirement: every cryptographic primitive path over
already-admitted secret material MUST be constant-time with respect to secret contents**
(open-questions std.crypto — no secret-dependent wrapper branch or memory index; CMOV/bitwise
only). Public lengths, algorithm identifiers, formats, allocation outcomes, and setup-validation
results are outside that guarantee. In particular, key import/parser/provider-validation is a
trusted setup operation, not an attacker-queryable timing oracle. This is the one domain where
Align's branchless machinery is a CORRECTNESS requirement, not a perf choice.

**Strategy**: **borrow the vetted engine**. AEAD (aes_gcm, chacha20_poly1305), hashes (sha256/512),
KDF (hkdf, argon2id), hmac → FFI-wrap a constant-time-audited C library — inheriting its
constant-time guarantees is far safer than self-hosting crypto and re-proving CT.
`constant_time_equal` is the ONE trivial self-host (a branchless byte-diff-OR reduction — Align's
`where`/mask machinery makes it natural and it's simple enough to audit). `crypto.random` → the OS
CSPRNG (getrandom/getentropy — the rand.seed source, but here for key material, exposed as
crypto-grade).

**Engine: OpenSSL libcrypto (EVP), decided 2026-07-07** (recorded in `open-questions.md` Settled;
supersedes this doc's original "libsodium recommended"). Rationale, converged from independent
security and dependency reviews: libcrypto natively covers *every* required primitive — including
HKDF and Argon2id via `EVP_KDF` — in one trust surface with no engine mixing and no
self-hosted-HKDF seam. Most of this surface works with **OpenSSL ≥ 3.0**; Argon2id requires the
`ARGON2ID` provider added in **OpenSSL ≥ 3.2**, and its absence is reported as `Error.Code`.
The driver adds `-lcrypto` only when a used Crypto or TLS capability requires it; within this module,
`crypto.random` and `constant_time_equal` do not request it. AES-GCM is constant-time on supported targets
(AES-NI/PCLMULQDQ hardware path,
constant-time vpaes fallback — never T-table AES on x86-64/aarch64) and is not API-gated on
hardware, unlike libsodium's `crypto_aead_aes256gcm_*`. libsodium remains a fine engine in the
abstract but loses on whole-system seams (no HKDF in 1.0.18-class releases, hardware-gated
AES-GCM). **blake3 is deferred with record**: no system engine provides it (no Debian
`libblake3-dev`; OpenSSL has no BLAKE3), self-hosting violates P5, and aliasing BLAKE2b under the
`blake3` name is forbidden (misleading API) — it becomes a candidate again when a system lib
exists, or a `pkg`-layer citizen.

## Signatures

```text
crypto.random(out: mut buffer)                                  // fill with CSPRNG bytes
crypto.sha256(data: bytes) -> array<u8>    // 32-byte digest (fixed-size)
crypto.sha512(data: bytes) -> array<u8>
crypto.blake3(data: bytes) -> array<u8>    // DEFERRED v1 (no system engine provides BLAKE3 — see Overview)
crypto.hmac_sha256(key: bytes, data: bytes) -> array<u8>
crypto.hkdf_sha256(salt: bytes, ikm: bytes, info: bytes, len: i64) -> Result<buffer, Error>
crypto.argon2id(password: bytes, salt: bytes, params: argon2_params) -> Result<buffer, Error>
crypto.aes_gcm_seal(key: bytes, nonce: bytes, plaintext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.aes_gcm_open(key: bytes, nonce: bytes, ciphertext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.chacha20_poly1305_seal(...) / _open(...)    // same shape as aes_gcm
crypto.constant_time_equal(a: bytes, b: bytes) -> bool          // CT — self-hosted
```

**Shipped surface details (implementation record, 2026-07-07, PRs #384–#388):**
`argon2_params { m_cost: i64, t_cost: i64, parallelism: i64, len: i64 }` is a **builtin struct**.
Its bare alias is available unless a non-entry module declares a local `argon2_params`; the exact
builtin spelling there is `crypto.argon2_params`, which requires and counts as a use of
`import std.crypto`. Ordinary struct-literal construction and typechecking apply. m_cost is in KiB,
t_cost is iterations, parallelism is lanes, and len is output bytes; validated
before the engine (`parallelism 1..=2^24-1`, `t_cost 1..=u32max`, `m_cost 8*parallelism..=4 GiB-in-KiB` (= 4,194,304 KiB),
`len 4..=1 GiB` → `Error.Invalid`; engine `threads` pinned to 1, `OSSL_set_max_threads` deferred).
AEAD: both ciphers take 32-byte keys and 12-byte nonces (validated as public params →
`Error.Invalid`); seal output is the **combined** `ciphertext || 16-byte tag` in one buffer; open
requires `len >= 16`. hkdf `len` is bounded `1..=8160` (RFC 5869 L ≤ 255·HashLen). Digest/tag
returns are dynamic `array<u8>` with algorithm-fixed lengths (a fixed-size `array<u8; N>` is not
expressible in the runtime-return ABI today). All FFI ops are Impure; `constant_time_equal` is
Pure and its branchless property is **verified against the compiled machine code** (release +
debug disassembly — no content-dependent branch, no memcmp idiom).

## Type & ownership classification

byte→byte / byte→owned buffer or fixed `array<u8>`. No new Move type (reuses buffer/array).
Fixed-size digests as `array<u8>` (32/64).

## Effect classification

FFI-wrapped ops impure (extern call). `constant_time_equal` is pure but MUST stay branchless.

## Error policy

AEAD open auth failure → `Error.Invalid` (NEVER leak whether it was the tag or the length — a
single opaque failure). KDF/argon param errors → `Error.Invalid`. Engine errors → `Error.Code`.
**Critical**: `aes_gcm_open` MUST NOT return partial plaintext on auth failure — all-or-nothing,
opaque error.

## New machinery required

Capability-gated FFI link to **OpenSSL libcrypto** (this module requests `-lcrypto` only for used
EVP-backed operations; OpenSSL ≥ 3.0 generally, ≥ 3.2 for Argon2id — see Overview); ~6 runtime wrappers over EVP: a shared
one-shot digest (`EVP_Q_digest`, param-swapped by
`EVP_sha256/512`), HMAC (`EVP_MAC` "HMAC"), HKDF (`EVP_KDF_fetch("HKDF")` + `OSSL_PARAM`
salt/key/info), Argon2id (`EVP_KDF_fetch("ARGON2ID")` + `OSSL_KDF_PARAM_ARGON2_*`), and a shared
AEAD seal/open pair (`EVP_CIPHER`, param-swapped AES-256-GCM / ChaCha20-Poly1305) with the P2
all-or-nothing shape. Plus the `constant_time_equal` self-host (branchless, no early return) and
`crypto.random` over the OS CSPRNG. Argon2 params struct.

## Slice breakdown

1. `constant_time_equal` (self-host, branchless) + `crypto.random` (OS CSPRNG) — no external dep,
   validates the CT discipline.
2. hashes (sha256/512) via EVP; blake3 deferred (see Overview).
3. hmac + hkdf.
4. AEAD (aes_gcm, chacha20_poly1305) — the all-or-nothing auth.
5. argon2id (KDF, expensive-by-design).

## Pitfalls

- **P1 (constant-time is CORRECTNESS)**: `constant_time_equal` must have NO early return, NO
  secret-dependent branch — a byte-diff OR-reduction over the full length, then a single 0-check.
  A `for` with early break leaks length/content via timing. The self-review must verify no
  secret-dependent control flow. This is the module's defining constraint.
  - **Length handling**: the input *length* is treated as **public**, not secret (in the intended
    use — comparing MAC tags / digests — both sides are fixed, publicly-known lengths). Differing
    lengths therefore return `false` immediately; the constant-time guarantee is over the
    *content* of **equal-length** inputs. This matches the libsodium `sodium_memcmp` contract
    (equal length is effectively a precondition). Document this explicitly so callers never pass a
    secret-length input expecting the length itself to be hidden.
- **P2 (AEAD all-or-nothing)**: `open()` on auth failure returns `Error.Invalid` and ZERO
  plaintext bytes — never partial, never a distinguishable error. Releasing unverified plaintext
  is the classic AEAD misuse. **EVP-specific mandatory shape**: `EVP_DecryptUpdate` releases
  plaintext BEFORE the tag is checked by `EVP_DecryptFinal_ex`, so the wrapper must decrypt the
  whole ciphertext into an internal owned buffer (never streamed out), set the expected tag via
  `EVP_CIPHER_CTX_ctrl(EVP_CTRL_AEAD_SET_TAG)`, call `EVP_DecryptFinal_ex`, and hand the buffer
  to the caller ONLY on `Final == 1`; on failure, `OPENSSL_cleanse` the buffer, free it, and
  return the single opaque `Error.Invalid` (tag-mismatch vs length/param errors must be
  indistinguishable). Nonce/tag lengths are validated as *public* values (P1); the tag is fixed
  16 bytes.
- **P3 (nonce reuse)**: document that nonce reuse with the same key is catastrophic (esp.
  aes_gcm). v1 does NOT auto-generate nonces (caller supplies) — but the doc must warn, and a
  nonce-generating convenience is a candidate (record). Consider requiring nonce length
  validation.
- **P4 (FFI memory safety, Gate 2)**: same as compress — try_from/checked_mul/null-guard on every
  buffer crossing FFI.
- **P5 (don't self-host the primitives)**: resist reimplementing SHA/AES/argon in Align —
  re-proving constant-time + correctness is a huge audited surface. Borrow the engine. Only
  `constant_time_equal` (trivial) is self-hosted.
- **P6 (key material zeroization)**: buffers holding keys should be zeroed on Drop (a
  zeroize-on-drop buffer variant, or document that the caller must). Record as a v1 consideration
  — the buffer Drop currently just frees; crypto keys ideally zero first.

## Test checklist

- sha256/512 against known test vectors (NIST/RFC); blake3 deferred
- hmac against RFC 4231 vectors
- hkdf against RFC 5869
- aes_gcm/chacha20 against their test vectors
- `aes_gcm_open` with a flipped tag → `Error.Invalid` + zero plaintext (P2)
- `constant_time_equal` true/false + (by inspection/audit) no early return (P1)
- `crypto.random` fills distinct bytes
- argon2id known-answer
- import-required
- capability-linking tests prove that EVP-backed crypto retains libcrypto and programs with no
  Crypto/TLS capability do not link it

## Asymmetric signature suite (post-pkg.db)

This section is the authoritative public-contract ledger for RS256, ES256, and Ed25519 signing and
verification. It extends the shipped M11 engine rather than adding a second crypto provider. The
suite is deliberately algorithm-specific in both function and key type: no string/enum algorithm
selector and no generic key handle can make an RSA key reach an EC or Ed25519 operation.

Normative algorithms and wire signatures are fixed as follows.

| Name | Exact operation | Public signature bytes |
|---|---|---|
| RS256 | RSASSA-PKCS1-v1_5 with SHA-256, not RSA-PSS | exactly the RSA modulus width, 256..=1024 bytes for an admitted 2048..=8192-bit key |
| ES256 | ECDSA over named curve P-256 with SHA-256 | exactly 64 bytes: 32-byte big-endian `r` followed by 32-byte big-endian `s`, including leading zero bytes; DER is internal only |
| Ed25519 | pure Ed25519 over the complete message, with no caller digest, context, or prehash | exactly 64 bytes |

The RS256 and ES256 records follow RFC 7518 sections 3.3 and 3.4. Ed25519 follows RFC 8032 and its
PKCS#8/SPKI identifiers follow RFC 8410. The implementation passes the exact digest/padding/group
parameters to OpenSSL EVP; provider defaults may not choose them.

### Public surface ledger

The six compiler-provided nominal types are distinct **Move** owners:

```text
rs256_private_key       rs256_public_key
es256_private_key       es256_public_key
ed25519_private_key     ed25519_public_key
```

Each bare spelling is the existing no-import builtin fallback unless a same-module declaration wins
bare lookup. Its explicit builtin spelling prefixes `crypto.`, for example
`crypto.rs256_private_key`; only that qualified type spelling requires and counts as a use of
`import std.crypto`. The value operations below also require that import. The entry module cannot
declare a colliding bare name. Public interface production and import validation recognize both
spellings, and reconstruct `std.crypto` as a source import only for a qualified spelling.

These are the exact constructors and operations; there are no defaults or optional arguments.
`bytes` has the existing borrowed byte-view meaning. The explicit `borrow` mode requires a stable
bound key place and never consumes it.

```text
crypto.rs256_private_key_from_pem(pem: str) -> Result<rs256_private_key, Error>
crypto.es256_private_key_from_pem(pem: str) -> Result<es256_private_key, Error>
crypto.ed25519_private_key_from_pem(pem: str) -> Result<ed25519_private_key, Error>

crypto.rs256_public_key_from_pem(pem: str) -> Result<rs256_public_key, Error>
crypto.es256_public_key_from_pem(pem: str) -> Result<es256_public_key, Error>
crypto.ed25519_public_key_from_pem(pem: str) -> Result<ed25519_public_key, Error>

crypto.rs256_public_key_from_jwk(n: bytes, e: bytes) -> Result<rs256_public_key, Error>
crypto.es256_public_key_from_jwk(x: bytes, y: bytes) -> Result<es256_public_key, Error>
crypto.ed25519_public_key_from_jwk(x: bytes) -> Result<ed25519_public_key, Error>

crypto.rs256_sign(borrow key: rs256_private_key, message: bytes) -> Result<buffer, Error>
crypto.es256_sign(borrow key: es256_private_key, message: bytes) -> Result<buffer, Error>
crypto.ed25519_sign(borrow key: ed25519_private_key, message: bytes) -> Result<buffer, Error>

crypto.rs256_verify(borrow key: rs256_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
crypto.es256_verify(borrow key: es256_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
crypto.ed25519_verify(borrow key: ed25519_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
```

The `_from_jwk` functions consume already-base64url-decoded JWK fields, not JSON or encoded text.
Callers use the one existing `encoding.base64url_decode` path before construction. No private JWK,
key generation, key export, certificate parser, encrypted PEM, password callback, RSA-PSS,
Ed25519ctx/ph, generic `sign`/`verify`, or implicit algorithm selection is added.

### Key input and validation contract

PEM constructors accept one bounded RFC 7468 textual block. The input is UTF-8 by `str`; each
constructor first requires `1..=65,536` bytes and rejects an embedded NUL. The block starts at byte
zero with the exact applicable `-----BEGIN PRIVATE KEY-----` or `-----BEGIN PUBLIC KEY-----` line.
Its base64 body uses the standard alphabet, canonical final padding, and LF or CRLF line endings;
every non-final body line is exactly 64 base64 characters and the final body line is 4..=64
characters in a multiple of four. The exact matching END line follows immediately. At most one
final LF or CRLF is accepted; leading text, horizontal whitespace, a second block, or any other
trailing byte rejects. The decoded payload must be one complete BER/DER object with no trailing
octet: unencrypted PKCS#8 `PrivateKeyInfo` / `OneAsymmetricKey` for `PRIVATE KEY`, or
`SubjectPublicKeyInfo` for `PUBLIC KEY`. `ENCRYPTED PRIVATE KEY`, traditional `RSA PRIVATE KEY` /
`EC PRIVATE KEY`, certificates, and OpenSSH keys therefore reject before any password, terminal,
file, network, or environment lookup can occur.

After decoding, the constructor requires the exact advertised key class and algorithm. RSA keys
must have a 2048..=8192-bit odd modulus and an odd public exponent `>= 3` that fits in an unsigned
64-bit integer; private keys pass the provider's complete private/pairwise check and public keys
pass its public check. ES256 keys must name P-256 exactly and pass the full EC private/public check.
Ed25519 keys must use the id-Ed25519 algorithm with absent parameters. A key of another class,
curve, size, or algorithm is `Error.Invalid`; it is never converted.

Ed25519 admission does not treat `EVP_PKEY_public_check` as point validation. One wrapper helper
validates the exact 32-byte compressed public value from every SPKI/JWK and the value obtained with
`EVP_PKEY_get_raw_public_key` from every PKCS#8 seed-derived key. It interprets the low 255 bits as
little-endian `y` and the high bit as the `x` sign, requires `y < p = 2^255 - 19`, and performs RFC
8032 section 5.1.3 recovery with the fixed `d = -121665 / 121666 mod p`. It computes
`q = (y^2 - 1) / (d*y^2 + 1) mod p`, rejects a zero denominator, and obtains a square root with the
fixed `(p + 3) / 8` exponent and RFC `sqrt(-1)` correction; it does not call `BN_mod_sqrt`. A
non-square result and the forbidden `x = 0` with sign bit one reject. The recovered point must
satisfy the twisted-Edwards equation and serialize byte-for-byte to the input. Three complete
extended-coordinate Edwards doublings must not produce the projective identity `X = 0, Y = Z`,
which rejects the entire small-order subgroup without exceptional affine inversions; no stronger
prime-subgroup-membership promise is made. This public-data, variable-time check uses fallible
`BIGNUM`/`BN_CTX` temporaries and precedes handle publication. An invalid encoding/point is
`Error.Invalid`; a BN API allocation/arithmetic failure is `Error.Code(0)`. The provider
private/public check remains an additional algorithm check, not the owner of these point invariants.

JWK components are borrowed binary integers/points:

| Constructor | Exact decoded fields and validation |
|---|---|
| `rs256_public_key_from_jwk` | `n` and `e` are minimal unsigned big-endian values with no leading zero. `n` is odd and its actual bit width is 2048..=8192; `e` is 1..=8 bytes, fits `u64`, is odd, and is `>= 3`. The resulting RSA key passes the provider public check. |
| `es256_public_key_from_jwk` | `x` and `y` are each exactly 32-byte big-endian P-256 coordinates. The uncompressed point `0x04 || x || y` must be canonical, on-curve, non-infinite, and pass the provider public check. |
| `ed25519_public_key_from_jwk` | `x` is exactly the 32-byte RFC 8037 Ed25519 public value and passes the wrapper-owned canonical/on-curve/non-small-order validation above plus the provider algorithm check. |

All cheap structural checks precede provider key construction. No constructor publishes a handle
until decoding, algorithm matching, bounds, and complete key validation have succeeded.

### Ownership, allocation, and effects

Each successful constructor returns a fresh one-word opaque owner of one runtime shell. The shell
owns the repeated key kind, one private `OSSL_LIB_CTX`, its explicitly loaded built-in OpenSSL
`default` `OSSL_PROVIDER`, and one provider-managed `EVP_PKEY`. Move transfers copy the shell pointer
then null the complete source; replacement and every
active aggregate Drop call the one null-safe `align_rt_crypto_key_free` exactly once. The types use
the ordinary independent builtin-Move carrier rules: locals, by-value/shared-borrow parameters,
returns, and recursively admitted struct/sum/Option/Result fields are valid. A fixed or dynamic AoS
array of a Move struct that contains a key remains valid through that struct's recursive Drop plan;
whole-element reads retain the existing Move-element restriction. Direct key or tagged/sum-key
elements in fixed/dynamic scalar arrays, slices, vectors/masks, array builders, or pipelines reject.
So do tuple/box placement, closure or task/parallel capture, `out`/`borrow mut` parameters,
global/constant storage, user-native/`layout(C)` exposure, and print/equality/order/hash. One
structural carrier classifier owns this closed positive/negative inventory and fails closed for a
future carrier. Keys have no borrowed input lifetime and no process-global registration. A borrowed
signing or verification call keeps the owner usable afterward and cannot retain the message or
signature view.

Private scalar material stays inside the provider-owned key and is released by `EVP_PKEY_free`;
the runtime does not copy it into an Align `buffer`. The caller-owned PEM `str` remains the caller's
storage and is not zeroized. PEM scanning borrows the at-most-65,536-byte input and the memory BIO
uses its explicit length. JWK construction uses only bounded component views and a 65-byte stack EC
point. Ed25519 validation allocates a fixed-count set of public-data BN temporaries only during
construction. Provider, library-context, operation-context, key, BN, and shell allocations are
fallible and are released on every failure.

Signing and verification borrow the complete message without an Align-side message copy. RS256
and ES256 use incremental EVP digest-sign/verify; Ed25519 uses the required one-shot pure-EdDSA
call with a null digest name. A successful sign allocates and publishes one fresh `buffer`: exact
modulus width for RS256 and 64 bytes for ES256/Ed25519. ES256's EVP DER signature is decoded into
bounded internal `r`/`s` storage before the raw result is published. Failure publishes no buffer.
Verification allocates no public result; ES256 converts the fixed raw input to a bounded internal
DER value. An exact-length mathematically invalid or malformed signature returns `Ok(false)`;
RS256 wrong-modulus-width and ES256/Ed25519 non-64-byte signatures also return `Ok(false)` before
provider verification. `Ok(true)` means verification under the exact named algorithm only.

Every constructor, sign, and verify operation is **Impure** because it crosses EVP. There is no
shared key cache or mutable wrapper-global state, so calls over independent keys may overlap. Each
constructor creates an ordinary `OSSL_LIB_CTX`, explicitly loads the built-in provider named
`default`, and uses the exact property query `provider=default` for `_ex` decoding, `fromdata`, and
signature/digest fetches: `d2i_AutoPrivateKey_ex`, `d2i_PUBKEY_ex`,
`EVP_PKEY_CTX_new_from_name`/`EVP_PKEY_fromdata`, and `EVP_DigestSignInit_ex`/
`EVP_DigestVerifyInit_ex` are the admitted construction/operation families. It never uses a
null/global library context, loads an OpenSSL configuration,
changes a provider search path or default property, or loads another provider. The resulting
`EVP_PKEY_get0_provider` pointer must equal the shell's provider pointer before publication; every
sign/verify context's `EVP_PKEY_CTX_get0_provider` pointer must equal it before the engine action.
A mismatch/fetch failure is opaque `Error.Code(0)`. Thus process-global providers, default
properties, `OPENSSL_CONF`, and `OPENSSL_MODULES` cannot substitute an implementation. The only
ambient platform dependency is the already-linked libcrypto containing its built-in default
provider. The wrapper passes exact algorithm parameters and reads no configuration, path,
environment, terminal, network, or clock. ES256 signing may consume provider randomness; RS256 and
Ed25519 do not promise an observable determinism contract even where their standard construction is
deterministic.

The shell and its context stay on the owning runtime thread because key task/parallel capture is
forbidden. An operation frees its digest/PKEY context before returning. Final Drop frees the
`EVP_PKEY`, calls `OPENSSL_thread_stop_ex` for the private context on that thread, unloads the owned
provider, frees the library context, and then frees the shell. Partial construction unwinds the same
acquired-prefix order. No provider or context outlives the shell and no shell mutates global OpenSSL
state. Cleanup return statuses never replace the operation's winning result; library-context free is
the final release even if provider unload reports failure.

Constant-time scope is exact. Algorithm, key class, all lengths, formats, allocation outcomes, and
success/error class are public. A constructor handles secret private-key bytes, but PEM/DER parsing
and provider key validation make no timing promise and are restricted to trusted setup; callers must
not expose construction as a remote or repeated timing oracle. After successful construction, sign
is constant-time with respect to private-key and message contents for fixed public lengths: wrapper
code never extracts private components or branches/indexes on them, uses only the named high-level
EVP signature operation, and leaves RSA blinding enabled. The pinned built-in default provider's
constant-time primitive implementation is an explicit dependency; provider provenance is checked at
key and operation construction rather than assumed from ambient selection. Verification and the
wrapper-owned Ed25519 public-point check handle public material and have no constant-time promise.
Evidence audits wrapper source/LLVM, exact `_ex` context/property arguments, provider-pointer checks,
and linked EVP APIs/parameters; functional vectors or noisy wall-clock statistics are not
constant-time evidence.

### Errors and deterministic precedence

Constructor format/key rejection and any malformed internal ABI tag/view are `Error.Invalid`.
Provider fetch/context/allocation or a non-verification engine failure is the existing opaque
`Error.Code(0)`; no OpenSSL error-stack number or text is exposed. After the key and byte views are
valid, every signature length, encoding, or mathematical mismatch is data (`Ok(false)`), not an
error. Allocation failure, parse failure, and signing failure never publish a partial key or
signature.

For a multi-invalid runtime call, validation order is exact:

1. require a non-null, naturally aligned ABI output slot and zero it, then validate the closed
   algorithm tag; an invalid output slot returns `AL_INVALID` without a write;
2. for key-taking operations require a non-null, naturally aligned shell and validate its repeated
   key kind/class before inspecting a byte view;
3. validate every `(ptr, i64)` input pair left-to-right without forming a slice: reject a negative or
   non-`usize` length and a null pointer with positive length; length zero accepts null and uses an
   internal non-null empty sentinel without dereferencing the input pointer. The order is `pem`;
   `n` then `e`; `x` then `y`; `message`; or `message` then `signature`. Ed25519's synthetic absent
   second JWK pair must be exactly null/zero;
4. validate public structural lengths. PEM is `1..=65,536`; JWK uses the exact component bounds
   above; an empty message is valid. Once both verify views are valid, a wrong signature length
   publishes `false` before reading message contents;
5. validate the PEM envelope/base64 and obtain its bounded decoded bytes, or validate the cheap JWK
   numeric/component encoding;
6. create the private library context and explicit built-in default provider; decode exactly one
   complete object/import with `provider=default`, require the exact key algorithm/class/size/group,
   run the applicable provider key check, run the independent Ed25519 public-point check where
   applicable, and require the key's provider pointer to equal the owned provider;
7. create the operation context in that private context with `provider=default`, require its provider
   pointer to equal the owned provider, set the exact digest/padding/group parameters, and run the
   engine;
8. validate the produced length/ES256 conversion and only then publish the owner or result.

A verification signature-length failure returns `Ok(false)` after the typed key handle is checked
and before the message is processed. An invalid key cannot be masked by an invalid signature in
malformed HIR. Cleanup runs before the winning result is returned and never replaces it.

### Runtime ABI and compiler identity

One internal `SignatureAlgorithm` byte is closed as `0=RS256`, `1=ES256`, `2=Ed25519`. One
`SignatureKeyKind` byte is closed as `0=RS256-private`, `1=RS256-public`, `2=ES256-private`,
`3=ES256-public`, `4=Ed25519-private`, `5=Ed25519-public`; every other value rejects before an EVP
call. The runtime shell repeats that kind and every operation checks it, so malformed MIR cannot
turn static type confusion into an unsafe provider call. Its remaining private fields are the owned
library-context, provider, and PKEY pointers; none is exposed through the ABI.

The implementation adds these exact internal declarations (`algorithm` is `i32` at the C ABI;
the one-byte range is validated before narrowing):

```text
i32 @align_rt_crypto_private_key_from_pem(i32, ptr, i64, ptr)
i32 @align_rt_crypto_public_key_from_pem(i32, ptr, i64, ptr)
i32 @align_rt_crypto_public_key_from_jwk(i32, ptr, i64, ptr, i64, ptr)
i32 @align_rt_crypto_sign(i32, ptr, ptr, i64, ptr)
i32 @align_rt_crypto_verify(i32, ptr, ptr, i64, ptr, i64, ptr)
void @align_rt_crypto_key_free(ptr)
```

The JWK ABI passes Ed25519's absent second component as null/zero. Constructor/sign result slots are
pointer-sized handle slots initialized to null; verify's final slot is an `i32` initialized to zero.
Status `0` is success, `AL_INVALID` maps to `Error.Invalid`, and `AL_CODE` maps to `Error.Code(0)`.
The pointer/length and output-slot rules in the ordered validation contract apply to the five
fallible operation rows; `key_free` is separately null-safe. No Rust slice, shell dereference, BIO,
or EVP call precedes validation. Non-null input storage validity is an invariant of this
compiler-internal ABI after checked-HIR validation.

Checked HIR and MIR use one payloaded key type rather than six unrelated enum arms. Canonical type
record version 3 assigns `Scalar::SignatureKey(kind)` leaf tag 39 followed by the exact one-byte
kind, and `Ty::SignatureKey(kind)` leaf tag 63 followed by that byte. The next tags 40/64 remain
unknown because the kind is payload, not another leaf family. Interface format 8 remains unchanged:
these nominal paths use its existing length-prefixed UTF-8 type record. The producer and importer
recognize all twelve bare/qualified paths, add `std.crypto` to reconstructed source only for the six
qualified paths, and reconstruct Move/return-cleanup identity for both forms. Each key fingerprint
is nominal — the exact closed kind, not the `EVP_PKEY` layout or a structural definition graph.
There are no runtime inspection fields, descriptor thunks, or source/artifact reads. Operation and
helper discriminants enter the compiler
build fingerprint, in-process memo, frontend/object cache keys, and whole/per-unit parity once;
an exact source edit/revert restores the prior key.

OpenSSL libcrypto remains capability-linked only when one of these operations is reachable. No new
artifact, file format, CLI flag, environment variable, provider selector, or package dependency is
introduced.

### Implementation closure matrix

This is the author-side matrix required before implementation; no row may be closed only by a
reviewer's later discovery.

| Axis | Required closure | Owner evidence |
|---|---|---|
| Type formation and interface | six no-import bare fallbacks plus six import-required qualified names; local-shadow/entry-collision/import-use rules; Copy rejected; Move and return-cleanup reconstruction; canonical kind/tag round-trip and exact next-unknown rejection | `align_interface::summary` builtin/source-import sweep; `align_mir::canonical_graph` exact goldens; `crypto_asymmetric::type_identity_matrix` whole/per-unit |
| Carrier closure | Admit local, by-value/return/shared-borrow, struct/sum/Option/Result, and recursively dropped fixed/dynamic AoS Move-struct arrays. Reject direct or tagged/sum key elements in fixed/dynamic scalar arrays, slices, vectors/masks, builders, and pipelines; tuple/box; closure/task/parallel capture; `out`/`borrow mut`; global/constant; user-native/`layout(C)`; print/equality/order/hash. A future carrier fails closed. | one parameterized sema/checked-HIR `signature_key_carrier_matrix`, recursive DropPlan/codegen owner, malformed future-kind negative |
| Construction | three private PEM, three public PEM, and three decoded-JWK constructors; success initializes one complete shell owner, failure leaves null; wrong label/algorithm/class/curve/size/component and exact 65,536/65,537 PEM boundary | runtime RFC/PEM/JWK vectors plus `crypto_asymmetric::constructor_matrix` |
| Ed25519 point admission | Every SPKI/JWK public value and every PKCS#8-derived public value passes wrapper-owned RFC 8032 compressed recovery, canonical `y`, sign-bit, curve-equation, re-encoding, and `[8]A != identity` checks independently of provider `public_check`; BN failure and invalid point remain distinct errors | direct positive RFC 8032 vectors; `y >= p`, nonsquare, `x=0/sign=1`, re-encoding, identity and all seven other small-order negatives through PEM and JWK; injected BN/raw-public-extraction failures and private-constructor helper-call assertion; a provider-check-success case cannot override wrapper rejection |
| Move-in/out and cleanup | local bind, by-value parameter/return, shared borrow, struct/sum/Option/Result construction, `?`, `else`, `match`, `map_err`, branch/loop joins, replacement, early return, and ordinary/malformed Drop each preserve one kind and exactly-one free; source nulling precedes any later Drop | parameterized `crypto_asymmetric::ownership_matrix`; runtime free counter/failpoints; checked-HIR one-field negatives |
| Sign/verify semantics | empty/binary/large messages; RS256 padding+digest and modulus-width result; ES256 DER/raw conversion including leading zeros and invalid r/s; Ed25519 one-shot no-digest; valid/wrong-message/wrong-key/wrong-length signatures; key remains usable | RFC 7515/7518, RFC 8032/8410, and OpenSSL-cross-checked vectors in runtime and `crypto_asymmetric` |
| FFI/allocation/cleanup | every output slot is validated/aligned then zeroed; every input pair covers negative/non-`usize`, null/zero, non-null/zero, null/positive, and positive valid storage before slice/shell/EVP work; Ed25519 absent JWK is exact null/zero; libctx/provider/ctx/key/BIO/BIGNUM/signature/shell storage frees on every injected failure; final free order is PKEY, thread-local context cleanup, provider, libctx, shell; no partial publication; runtime kind recheck; libcrypto link retained only when reachable | runtime ABI view/slot and failpoint sweeps, ABI declaration golden, capability-linking twins |
| Provider provenance | Each shell owns a private ordinary libctx and its explicitly loaded built-in default provider; all decode/import/signature/digest fetches use exact `provider=default`; key and operation provider pointers equal the owned pointer; no global ctx/config/search path/default property/provider is consumed | child-process owner with hostile `OPENSSL_CONF`/`OPENSSL_MODULES`, global null provider, and incompatible global default properties; exact pointer assertions; independent-key overlap and teardown stress |
| Constant-time boundary | Constructor parsing/checking, including public BN validation, is explicitly trusted setup with no timing promise. For admitted private keys at fixed public lengths, sign wrapper code never extracts or branches/indexes on secret key/message contents, uses the exact high-level EVP operation, and leaves RSA blinding enabled; the pointer-verified built-in default provider primitive is the named dependency. Verification is public-data and outside the promise. | wrapper source/LLVM secret-flow audit, forbidden low-level/private-component API guard, exact `_ex` libctx/property/provider-pointer and EVP algorithm/parameter/blinding inspection; no timing benchmark as correctness evidence |
| Compilation paths | direct/imported calls, public key-bearing signatures, function values, generic monomorphization around a concrete key, whole-program/per-unit compilation, object/frontend cache edit/revert, optimized/unoptimized LLVM, and malformed HIR carry identical algorithm/kind/effect/cleanup facts | `crypto_asymmetric` driver owner, interface/cache owners, checked-HIR validator matrix |
| Resource claim | PEM exact limit, RSA size bound, one private libctx/provider/PKEY shell per live key, fixed-count Ed public BN temporaries only during construction, fixed ES/Ed operation temporaries, and no Align-side message copy hold for 1-byte and 8-MiB messages; benchmark is local evidence, not a correctness gate | `bench/crypto_asymmetric` live-key/peak-wrapper-allocation record plus deterministic limit tests |

The implementation is one capability PR even though it is expected to exceed roughly 1,000
hand-written changed lines. The six static types, their constructors, runtime kind checks, and the
first sign/verify consumers are one proof boundary: splitting them would land dormant key producers
or duplicate the type/Drop/interface/ABI proof once per algorithm. RS256, ES256, and Ed25519 remain
parameterized cells of that one boundary, not three independent ownership mechanisms.

Before review, the author must map every applicable matrix cell to implementation and a regression
or explicitly defer it here. A P1 or strategy-changing review finding reopens this matrix before
implementation continues.

### Acceptance and synchronized sources

Acceptance requires the complete matrix above, `scripts/cargo.sh test -p align_runtime`, the focused
`crypto_asymmetric` driver owner, interface/canonical/ABI goldens, capability-linking twins, the
constant-time boundary audit, bounded PR gate, and Clippy. The local benchmark runs only because
this ledger makes an explicit message-copy/resource claim; it is not a correctness or
constant-time gate and sets no latency target.

### Design-review finding-to-fix ledger

| Finding | Closure |
|---|---|
| P1 forbidden key carriers lacked negative ownership | Added the closed positive/negative carrier inventory, a fail-closed structural classifier, recursive AoS positive, and parameterized sema/checked-HIR/codegen owner. |
| P1 byte-view null/length behavior was unspecified | Fixed every input/output pointer, signed-length, zero-length, alignment, slice-formation, and multi-invalid rule plus the ABI sweep. |
| P1 private-key constant-time boundary had no closure | Distinguished trusted constructor setup from the fixed-public-length signing promise, named the provider assumption, and added wrapper/API audit evidence. |
| P2 malformed signature conflicted with format rejection | Restricted `Error.Invalid` to constructor/internal-ABI rejection and made every post-view signature mismatch `Ok(false)`. |
| P2 bare key alias import rule conflicted with the nominal model | Restored no-import bare fallback; only qualified type spellings and value operations require `std.crypto`. |
| P2 HTTP streaming implementation status drifted | Synchronized the roadmap, settled record, draft, and language digest to implemented 2026-08-30. |
| P1 provider provenance was only an ambient assumption | Reopened the provider axis: every key owns an isolated libctx and built-in default provider, every fetch uses `provider=default`, key/operation pointers are checked, teardown is ordered, and hostile-global child tests own substitution. |
| P1 Ed25519 provider check did not validate the encoded point | Reopened point admission: the wrapper independently performs canonical RFC 8032 recovery, curve/re-encoding checks, and complete small-order rejection for PEM, JWK, and private-derived public values. |
| P2 two HTTP tail sentences still claimed implementation was pending | Corrected the remaining `draft.md` and condensed-spec status sentences to the implemented state. |

This section is the source of truth. Its public types/signatures and algorithm/error/ownership
contract must agree with `draft.md` §18.2, `docs/language-spec.md`, `docs/open-questions.md`,
`docs/impl/07-roadmap.md`, `docs/impl/19-hir-validation-ledger.md`,
`docs/impl/20-runtime-abi-ledger.md`, and the Japanese mirror. Implementation status changes only at
the capability boundary; it does not rewrite this contract.
