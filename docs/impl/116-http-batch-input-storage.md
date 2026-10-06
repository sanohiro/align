# Borrow HTTP batch input storage

`cl.get_many` currently copies each URL into an owned `HttpRequest`, including
one owned `GET` string per request. Its scoped workers only read those requests
and all join before the synchronous native call returns. The existing
`bench/http_client` batch consumer and native/driver batch owners exercise this
path.

Prepare one vector of `Cow<str>` URL values. Valid UTF-8 borrows the caller's
immutable bytes for the existing call lifetime; malformed native UTF-8 retains
the current lossy-owned conversion. Each worker forms an ordinary
`HttpRequestView` with the static GET method and empty headers/body. The direct
GET entry and batch workers share that borrowed constructor. No input byte or
view escapes the joined scope or enters a response or idle pool.

This refines native input storage under the existing synchronous FFI lifetime
precondition. Public signatures, admission, errors, ordering, effects, worker
count, result ownership, and compiler/ABI/cache records stay unchanged. URL
parsing still occurs in each claimed exchange; preparation must not reject the
whole batch early or skip valid requests beside an invalid URL. No new worker
thread or `Send`/`Sync` assertion is introduced.

| Closure axis | Implementation and owner |
| --- | --- |
| Formation and admission | Preserve output clearing, concurrency validation, empty/null/count handling, native byte-view normalization and lossy UTF-8 conversion. A parameterized native input owner covers empty, valid ASCII/Unicode, NUL, invalid UTF-8 and existing null/negative-length empty normalization. The existing empty/null-output owners retain entry precedence. |
| Input lifetime and construction | One private URL preparation helper returns call-scoped Cow values; its unsafe contract requires every input byte range live and immutable for the returned borrow. The descriptor slice and byte owners outlive preparation and all scoped workers. Pointer-identity and repeated/overlapping-view owners prove valid URL bytes are borrowed, with no caller mutation. |
| Worker transfer and return | Workers borrow the prepared vector through scoped shared references, construct GET views at the call site, and join before URL scratch drops. Responses own their bytes independently. Existing input-order, concurrency-one pool reuse, mixed HTTP/HTTPS, and error cleanup owners exercise success/failure. A real nonempty imported helper owner runs whole/per-unit and uses response bytes after the caller's URL owner has gone. |
| Errors and completion | Keep lowest-input-index error selection and run-to-completion across malformed URL, native failure, body-cap failure and success. Existing malformed/body-cap ordering and response-free owners are reused; add a malformed-URL/live-peer matrix so preparation cannot become an early batch rejection. |
| Allocation discrimination | The actual Rust allocator counter measures preparation after fixture setup for 0, 1, 8, 64 and 1024 valid URLs, including long and repeated inputs. Empty preparation allocates zero; nonempty preparation allocates only its vector, while static GET view construction allocates zero. An independent live allocation witness checks the counter. A caller-thread full-entry probe on invalid URLs records before/after counts without charging worker/network allocations to input preparation. |
| Unchanged compiler paths | No type/IR variant, generic/interface record, generated cleanup or native symbol changes. Existing `m11_http_get_many` and HTTP client composition owners preserve normal moves, result Drop and imported signatures; no new malformed HIR matrix is needed. |

Only reduced native preparation allocations and copied bytes are claimed. Socket
latency, throughput, RSS and the existing overlap measurements are not new
performance promises. Replace the obsolete staged-versus-owned preparation
probe with the production storage owner instead of preserving a second runtime
implementation. No specification or language mirror update is needed.

An author matrix pass and one independent strategy inspection precede the native
borrow change. The committed implementation receives one fresh full-diff
inspection, owner checks, the bounded PR gate and Clippy before publication.

The independent strategy inspection found no issues. Native preparation and
pointer owners close the input-storage matrix; the malformed-URL/live-peer owner
crosses both input orders and one/two workers. The nonempty imported-helper owner
reads a returned response after both local URL and client owners have dropped,
under whole/per-unit compilation and dev/release profiles.

The macOS actual-allocation probe records 0/9/32/144/2064 caller-thread Rust
allocations before the change and 0/7/16/16/16 afterward, for 0/1/8/64/1024
invalid-syntax UTF-8 URLs at concurrency four. This removes exactly two
allocations per nonempty input URL. The probe deliberately avoids network work
and excludes worker-thread allocation; it is not a complete-request cost claim.
Production preparation separately measures zero allocations for empty input and
one vector allocation for nonempty valid URLs of 32 or 2048 bytes, including
repeated views. GET view construction measures zero allocations.
