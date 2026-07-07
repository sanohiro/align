This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.crypto — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/crypto.md)

## Overview

crypto.random, sha256/sha512, blake3, hmac, hkdf, argon2id, aes_gcm, chacha20_poly1305,
constant_time_equal (draft §18.2). **The hard requirement: every secret-dependent path MUST be
constant-time** (open-questions std.crypto — no secret-dependent branch or memory index;
CMOV/bitwise only). This is the one domain where Align's branchless machinery is a CORRECTNESS
requirement, not a perf choice.

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
HKDF and Argon2id via `EVP_KDF` (**OpenSSL ≥ 3.2**, the documented floor) — in one trust surface
with no engine mixing and no self-hosted-HKDF seam; it is a universal system lib in the same
always-link class as libz/libzstd (`-lcrypto` joins the driver's base link set, the compress
precedent); and its AES-GCM is constant-time on supported targets (AES-NI/PCLMULQDQ hardware path,
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
`argon2_params { m_cost: i64, t_cost: i64, parallelism: i64, len: i64 }` is a **builtin struct**
(reserved name, injected like the builtin `Error`; ordinary struct-literal construction and
typechecking) — m_cost in KiB, t_cost iterations, parallelism lanes, len output bytes; validated
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

FFI link to **OpenSSL libcrypto** (`-lcrypto` always-linked, floor ≥ 3.2 — see Overview); ~6
runtime wrappers over EVP: a shared one-shot digest (`EVP_Q_digest`, param-swapped by
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
- (FFI slices assume libcrypto present — the compress always-link precedent; no feature-gating.)
