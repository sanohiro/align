# Reuse HTTP client storage during destruction

Client destruction currently collects every idle endpoint into a temporary Vec
before closing its descriptor/SSL pairs. A native 96-connection fixture measures
five Rust allocation events during destruction. The exclusively owned client
already contains all required storage.

Consume its idle mutex, recover a poisoned mutex's value through the existing
PoisonError::into_inner rule, and iterate the owned host maps and buckets
directly. Each pair still reaches the shared close_tls exactly once; no mutex
is held across TLS shutdown, SSL free or descriptor close. The remaining client
fields are destroyed normally. This uses the existing exclusive native free
precondition and changes no FFI, ABI or ownership-safety rule. Map and bucket
storage now remains live until its traversal finishes, instead of being freed
before the first close; no reduction in retained bytes or blocking teardown time
is claimed.

| Closure axis | Implementation and acceptance |
| --- | --- |
| Complete owner traversal | Native client destruction visits every host, scheme, port and bucket entry. The owner populates three hosts, two schemes, two ports and eight entries each, observes all peers open before Drop and EOF afterward. Actual SSL/BIO pairs cover empty, single and full buckets through the shared TLS close counter plus peer EOF. Existing handshaken HTTPS owners retain wire behavior. |
| Exceptional and empty state | Null free remains a no-op. Empty clients and empty retained buckets remain valid. The same native owner poisons only its private mutex, then requires all connections to close; existing client/retained-bucket tests cover the empty and null cases. Every fixture socket/SSL/context is immediately guarded before fallible setup or assertions. |
| Allocation discrimination | Existing thread-local actual Rust allocation counting surrounds production free after setup. An independent live allocation witness verifies the counter. The baseline's 96-connection destruction performs five allocation events; the candidate must perform zero, including poisoned recovery. Native TLS allocator activity is outside this Rust-only claim. |
| Existing consumers | Native pool/client/HTTPS owners and the driver m11_http, http_read_stream and m11_http_get_many targets run on Linux/macOS. Existing shared-borrow and joined-worker lifetime rules remain the free precondition; pool take/put, network I/O, timeout, retry and request-buffer reuse are unchanged. |
| Compiler and public surface | No type/IR variant, generic rule, interface/cache encoding, generated Drop, native symbol or public contract changes. No language/library prose or mirror update is needed. |

The allocation reduction is the only resource claim; no latency or RSS gain is
promised. An initial fixed-scratch take/put candidate removed expiry allocations
but shifted warm-request CPU measurements upward by roughly 2% above the socket
floor in two paired comparisons. That candidate and its tests are retained only
as external investigation evidence. The selected boundary changes exclusively
client destruction and preserves the hot pool operations byte-for-byte in source.

Author matrix-to-diff checking and one fresh independent preflight inspection
close the existing ownership strategy. This is one complete native destruction
capability, not a producer waiting for later adoption.

Qualification: Linux ARM64 passes all 48 selected native and 51 driver tests.
macOS ARM64 passes both new destruction owners and all 51 driver tests. The
broader macOS native survey has ten failures before client destruction; the
unchanged base reproduces the exact same failure set. Those existing interim-
response/TLS fixture failures are retained as separate evidence, not counted
as passing qualification or repaired by this change. The native destruction
owner records zero Rust allocation events for both normal and poisoned clients.
