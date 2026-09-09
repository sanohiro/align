# pkg.s3 — explicit-expiry presigning

> English is authoritative. Japanese mirror: `ja/s3-presign.md`.
> Status: design candidate, 2026-09-09; implementation follows independent review.

## Capability and public-contract ledger

This capability extends the implemented [S3 package](s3.md) with query
authentication. It returns a transferable URL together with its method and
required user headers. It performs no HTTP operation. Credentials, endpoint,
clock input and lifetime remain explicit; the existing header-authenticated
`request` contract is unchanged. The same vendorable `pkg.s3` module owns both
operations and shares private canonicalization and HMAC machinery.

The following declarations and P1–P8 are the authoritative public record. There
are no defaults, overloads or implicit arguments. Argument evaluation is once in
source order, before validation.

| Exact declaration | Input and result semantics | Ownership, allocation, effects | Identity, prerequisite and acceptance |
|---|---|---|---|
| `pub SignedHeader { name: string, value: string }` | Fields in this order. One lowercase header name and normalized value, P4. These are the headers the recipient must send, excluding transport-owned Host. | Ordinary Move record owning two strings. Construction, field moves, replacement and recursive Drop follow existing rules. No borrowed input survives in either string. | Nominal package record; complete ordered field definitions enter the ordinary interface. P-G, P-O. |
| `pub PresignedRequest { method: string, url: string, headers: array<SignedHeader> }` | Fields in this order. Exact method, complete ASCII URL, and ascending-name user-header rows, P4–P7. Empty input headers produce an empty array. No body, expiry timestamp, credential owner or hidden request handle is stored. | Ordinary Move record, owning both strings and every array element. It may outlive every input root and may be borrowed repeatedly by callers. No package-specific Drop or cloning rule. | Ordinary interface and implementation hashes; no new cache schema or native ABI. P-O, P-I. |
| `pub fn presign(credentials: Credentials, endpoint: Endpoint, method: str, path: str, query: slice<Field>, headers: slice<Field>, now_ns: i64, expires_seconds: i64) -> Result<PresignedRequest, Error>` | Existing three borrowed input records are unchanged. Returns one complete P7 result or `Error.Invalid` in P1 order. Method is signed; body bytes are deliberately not inputs. Time/lifetime rules P2; query P3; headers P4; canonical/signature bytes P5–P7. | Inputs borrow only for the call. Impure through existing crypto. All validation precedes package allocation and crypto; success allocates owned output strings/array and temporary builders/encoded strings/digests, released on normal exits. Existing allocation/crypto hard-failure behavior remains. | Implemented HTTP owners, source record arrays, time formatting, percent encoding, SHA-256 and binary-key HMAC are sufficient. One ordinary source capability; no compiler/runtime prerequisite. P-G, P-V, P-O, P-I. |

There is no persisted or exchanged binary record format, reflection, runtime table,
process-global mutation, credential cache or key cache. The URL's text is the wire
artifact specified below. The module retains ordinary module-wide import
capabilities, including its existing HTTP/time/encoding/crypto dependencies.
Secret temporaries use ordinary Drop with no zeroization guarantee. Output
intentionally contains the access identifier, signature and optional session token;
the secret key is never included. The package emits no diagnostic or log containing
those values. Possession of the URL and required headers permits its signed
operation subject to server authorization; it is not a one-use token.

## Numbered rules

**P1 — deterministic validation.** All failures are `Error.Invalid`. Before any
package allocation, formatting, encoding or hashing, check: (1) query count at
most 128, then header count at most 120; (2) credentials using S3 V2;
(3) endpoint using V3; (4) method then path using V4; (5) query entries using V5;
(6) header entries using V6; (7) `now_ns >= 0`; (8) `expires_seconds` in
1–604800 inclusive. V2–V6 retain their exact byte limits, UTF-8/ASCII/NUL rules,
source-order checks, duplicate and reserved-name rejection. There is no body
length check because there is no body argument. Bounds describe caller entries;
the generated authentication query entries are additional, never counted against
128. No input rejection calls HTTP or reaches its header-abort boundary.

**P2 — explicit time.** Format `now_ns` with the existing `time.basic_iso`,
discarding positive subsecond parts. Every nonnegative i64 time is accepted,
including its maximum. Use the first eight ASCII timestamp bytes as the date.
`expires_seconds` is a duration from that whole-second timestamp, serialized in
base ten without a sign or leading zeroes. Do not add or multiply the duration
into an i64 nanosecond deadline; no representable absolute deadline is promised.
There is no local current-time or credential-expiry check. The server may reject
a request earlier because credentials expire, are revoked, or policy denies it;
this duration is not a guarantee of access until an inferred timestamp.

**P3 — query.** Preserve every admitted user pair, including empty values and
duplicates. Append these generated pairs before encoding and sorting:

| Name | Unencoded value | Presence |
|---|---|---|
| `X-Amz-Algorithm` | `AWS4-HMAC-SHA256` | Always |
| `X-Amz-Credential` | access key + `/` + date + `/` + region + `/s3/aws4_request` | Always |
| `X-Amz-Date` | P2 timestamp | Always |
| `X-Amz-Expires` | P2 duration | Always |
| `X-Amz-SignedHeaders` | P4 names joined with `;` | Always |
| `X-Amz-Security-Token` | exact credential token | Exactly `Some`; `None` omits the pair |

All names and values are encoded and ordered together using S3 W1's separate
encoded-name then encoded-value ordering. The existing case-insensitive `x-amz-`
query exclusion prevents caller collisions. `X-Amz-Signature` is absent from this
canonical query and added only at P7. Empty user input still produces the five
mandatory pairs. Token bytes such as `+`, `/` and `=` are percent-encoded as data.

**P4 — headers.** Apply S3 W3's lowercase/whitespace normalization to every user
header and sort by lowercase name. Include exact origin authority as the one
generated `host` row in canonical headers and SignedHeaders. Canonical rows end
in LF, including the last row; names join with `;`. Do not generate date, payload
hash, token or Authorization headers. P1 retains all V6 reserved names, including
those four authentication names. The returned header array contains only the
normalized user rows in the same order, excluding Host; every returned row is
signed. It is empty exactly when user headers are empty, while SignedHeaders is
then `host`. Caller field spelling, whitespace, order and backing storage are not
retained. Other `x-amz-*` headers admitted by V6 remain signed user rows.

**P5 — canonical bytes.** The URI uses unchanged S3 W1 path encoding, including
preserved slashes and dot segments. Form the same six-field S3 W4 canonical
record, substituting P3 query, P4 header rows/names and literal `UNSIGNED-PAYLOAD`
for the payload field. Retain the exact LF and final-no-LF rules. This operation
does not hash or bind any body. A caller may separately bind a checksum by
supplying a supported checksum header; its interpretation remains server-owned.

**P6 — signature.** Hash P5 bytes and apply the existing S3 W4 StringToSign,
scope and raw-key HMAC derivation without modification. The result is exactly
64 lowercase hex characters. Header-authenticated `request` continues hashing
its supplied body and emitting its existing Authorization format.

**P7 — output.** URL is exact origin + encoded URI + `?` + canonical P3 query
+ `&X-Amz-Signature=` + P6 signature. Signature is always the final pair, rather
than inserted into the already signed ordering. There is no fragment, extra
separator or newline. Copy the exact input method and own all P4 user rows.
The URL is ASCII with no embedded NUL; input path/query UTF-8 and NUL become
percent escapes. No request construction, header setter or body copy occurs.
All output fields are present; no ordinal, unavailable value or optional output
field is introduced. No additional output-size policy is applied after P1;
all capacity arithmetic must use the admitted bounded lengths without wrap.

**P8 — consumption.** The recipient sends the returned method, URL and every
returned header using its chosen HTTP implementation. Host must preserve the
URL's exact authority, including explicit ports; path/query must not be
normalized or re-encoded. A browser-style URL-only consumer is appropriate only
when no required user headers are returned and it sends the signed method.
A PUT consumer supplies its own body and HTTP framing, including explicit empty
body presence under the existing rule. The same result can be reused for
multiple requests without consuming credentials. Changing the signed method,
target or headers can invalidate authentication. Transport, status, response
ownership, streaming, timeouts and errors remain those of the chosen HTTP API.

## Public use and closure matrix

Declarations and ordinary positional calls are separate. This helper consumes a
body only through HTTP's existing copy; it borrows the reusable signed artifact:

```align
import pkg.s3
import std.http

fn upload(borrow client: http_client, borrow signed: pkg.s3.PresignedRequest,
  body: slice<u8>) -> Result<http_response, Error> {
  outgoing := http.request(signed.method, signed.url)
  mut i := 0
  loop {
    if i >= signed.headers.len() { break }
    outgoing.header(signed.headers[i].name, signed.headers[i].value)
    i = i + 1
  }
  outgoing.body(body)
  return client.request(outgoing)
}
```

The existing `crates/align_driver/tests/pkg_s3.rs` owns the new cases; reuse its
isolated project, child deadline and local peer machinery. One coherent package
implementation shares canonicalization/signing helpers and closes the complete
URL-to-wire boundary. No separate helper-only producer capability is useful.
No performance or exact allocation-count promise is made; no benchmark is required.

| ID / exact owner | Required implementation and regression closure |
|---|---|
| P-G `presign_vectors` | Published AWS 2013-05-24 GET example: canonical hash `3bfa292879f6447bbcda7001decf97f4a54dc650c8942174ae0a9121cf58ad04`, signature `aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404`. Independently assemble canonical bytes and full URL. Exact URL, method and returned header rows cover None/Some token, zero/multiple user headers, encoded prefix sorting, duplicate/empty pairs, binary secret, normalized whitespace, authority/port and UTF-8/NUL/slash/dot paths. Reconstruct captured bytes back to canonical form and verify the signature, excluding only its final pair. |
| P-V `presign_validation` | Reuse V2–V6 parameterized owners where shared production validators are called; discriminate presign's aggregate and validation entry points. Counts 0/limit/next, expiry -1/0/1/604800/604801/i64 extrema, time -1/0/fraction/max, every reserved query/header category, token None/Some/empty, multi-invalid inputs. Private predicate probes may close impractical size limits, but public calls own admission routing. No rejected input triggers HTTP or sends bytes. |
| P-O `presign_round_trip` | Output escapes an input arena and temporary credentials/field buffers; original inputs then change or drop. Move result through helpers/Result/Option, replace it, return, destructure, borrow repeatedly and drop nested headers. Existing record/array owners cover if/match/else/?/loop joins, early exit, move-out source nulling and cleanup; add one result-replacement/reuse witness distinguishing this aggregate. Local peer verifies GET and PUT (binary and explicit empty payload), nonempty normalized required headers, token states, shared-client reuse and raw denial response. Arbitrary body variation does not change query authentication. |
| P-I `presign_imports_effects_cache` | Whole-program/per-unit × Dev/Release use the same URL/wire oracle. Cross-package nominal record/array interface and borrowed projections compile; Impure call is rejected in a parallel closure returning a valid primitive. Cold/warm and private signature-helper edit/restore invalidate artifacts correctly; existing request vectors stay unchanged. The unchanged single-module package inventory includes the new callable surface without native symbols or new source units. |

Malformed checked IR, generic specialization, record/array allocation provenance,
recursive Drop, source nulling and interface serialization remain existing compiler
owners. No ownership strategy, IR variant, FFI, runtime body framing, callback or
process-global state changes. If a probe exposes a missing invariant, reopen this
matrix before expanding scope. Credential refresh, presigned POST policies,
streaming/chunk signing, multipart orchestration, typed operation schemas, SigV4a
and provider-specific certification remain outside this capability.

## Sources and consistency

[S3 query authentication](https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sigv4-query-string-auth.html)
owns the protocol and published example;
[S3 presigned URL lifetimes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-presigned-url.html)
distinguishes requested duration from credential/policy validity. Align's types,
input bounds, validation ordering, ownership and output-row contract are package
design decisions. `s3.md` remains authoritative for shared V2–V6 and W1/W3/W4.

Synchronize `s3.md`, both Japanese mirrors, `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md`, `docs/open-questions.md` and `docs/impl/07-roadmap.md`.
`HANDOFF.md` records the capability boundary once. Before independent review,
compile the public type/example probe, check the ledger-to-prose Cartesian cases
and reproduce the published vector with an independent hash/HMAC implementation.
The type probe is not an implementation or signature witness.
