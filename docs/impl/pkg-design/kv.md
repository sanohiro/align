# pkg — kv

> English is authoritative. A synchronized Japanese mirror lives at `ja/kv.md`.
>
> **Status:** design candidate; no public contract is accepted until independent review closes.

## Authoritative public-contract ledger

This table is the authority for the first `pkg.kv` capability. Later prose and implementation may
make a field more explicit but must not widen it. V1 is one synchronous RESP2 text-value client over
plaintext TCP. It introduces no generic Redis command surface, protocol negotiation, compiler
operation, ambient endpoint, or hidden retry. One package-internal, source-reachable runtime row
closes checked timeout installation. Before that row activates, the shared connect/timeout substrate
gets checked fd-mode transitions, start-plus-budget deadline arithmetic, and exact positive-timeout
quantization for its `std.net`, `std.http`, and `process.command` consumers. The existing TCP-derived
writer path is hardened in place so all `std.net` consumers, not only this package, receive
SIGPIPE-safe writes.

| Public surface | Exact inputs, defaults, validation, and evaluation | Exact result, errors, order, and effects | Ownership, lifetime, allocation, and cleanup | Compiler/runtime/package owner, artifact, and cache identity | Prerequisite and acceptance owner |
|---|---|---|---|---|---|
| `pub resource client = pkg.kv.internal.resource.drop_client` | One opaque non-null resource constructed only by successful `connect`. It is nominal, Move, non-Copy, non-comparable, non-printable, and has no public raw conversion or constructor. | A live value admits one synchronous mutable operation. A failed transport, oversized response, or malformed/unexpected/truncated RESP reply closes it before returning the selected terminal error; every later operation returns `Error.Closed` without I/O. A complete framed non-UTF-8 GET/error payload is instead reusable `Decode`. `Drop` is the sole public close operation. | The resource owns one package state allocation and, while live, one runtime TCP connection plus one non-owning reader shell and one non-owning unbuffered writer shell. Moving a live value transfers all four allocations; `borrow mut` is call-bounded and excludes overlapping requests. Drop frees writer then reader, closes the socket at most once, and frees the state exactly once. | Unit `pkg.kv` owns interface resource record `{ name: "client", type_params: [], generic_arity: 0, representation_version: 1, drop_thunk: "__align_resource_drop$pkg.kv$client", drop_abi_fingerprint: b"align-res-drop-1" }`; `pkg.kv.internal.resource` owns the private hook/state. Every field is serialized and participates in interface identity. | Shipped opaque resources and TCP; formation/visibility, Move/Drop, control-flow, malformed-state, close-once, later-Closed, whole/per-unit, and independent six-field interface-mutation owners. |
| `pub ClientOptions { connect_timeout_ns: i64, io_timeout_ns: i64, max_response_bytes: i64 }` | Exact field/source order as shown. No defaults. Both timeouts must be `1..=86400000000000` ns. `max_response_bytes` must be `0..=536870912` and is an inclusive cap for a GET bulk payload or owned RESP error payload. | Copy and Pure. Invalid fields return `Error.Invalid` during `connect`, in field order, before DNS, allocation, or socket work. A positive connect remainder is converted to the next representable millisecond for `poll`; a positive I/O timeout is converted to the next representable microsecond for `timeval`. Neither conversion expires early. | Three i64 fields; no borrow, allocation, Drop, or retained ambient state. On success the socket retains the installed I/O timeout settings and the package state retains the response cap; the connect timeout is consumed by construction. | `pkg.kv` owns the nominal definition. Whole-program/per-unit interfaces serialize its name and ordered fields; the complete definition enters interface, dependency, and cache identity. | Shipped i64-ns duration/TCP machinery plus the timeout-substrate prerequisite; field/order/exact/next-bound, no-default, ns/ms/us boundary, whole/per-unit, and cache owners. |
| `pub SetCondition { Always, IfAbsent, IfPresent }` | Closed source and discriminator order is exactly `Always = 0`, `IfAbsent = 1`, `IfPresent = 2`. | Copy and Pure. It maps respectively to no condition token, `NX`, or `XX`. No integer/string selector or unknown fallback exists. | One Copy tag; no borrow, allocation, Drop, or retained state. | `pkg.kv` owns the nominal sum and interface discriminator order. | Exact tag/order, construction/match, interface, and malformed checked-HIR owners. |
| `pub SetOptions { condition: SetCondition, expires_in_ns: Option<i64> }` | Exact field/source order as shown. No defaults. `None` requests a persistent value. `Some(ns)` requires `1..=i64::MAX` and maps to Redis `PX` milliseconds using checked `ceil(ns / 1000000)`. | Copy and Pure. Invalid expiry is `Error.Invalid` before request construction or I/O. `None` deliberately uses plain `SET`, which removes an existing key TTL under Redis SET semantics. | One Copy tag plus i64 and one Copy condition; no borrow, allocation, Drop, clock read, or retained state. | `pkg.kv` owns the nominal definition; its complete reachable definition graph enters ordinary interface and dependency identity. | Exact condition/expiry product, ns-to-ms boundary/overflow, persistence/TTL interop, interface, and cache owners. |
| `pub Error { Invalid, Io(core.Error), Server(string), Decode, ResponseTooLarge, Protocol, Closed }` | Closed source/discriminator order is exactly the shown `0..=6`. `Invalid` is caller input/options. `Io` carries the unchanged builtin transport category/code. `Server` carries one complete UTF-8 RESP error payload. `Decode` is a fully consumed bulk or error string that is not UTF-8. `ResponseTooLarge` is a GET/error payload beyond the caller cap or an otherwise admitted control line beyond 64 bytes. `Protocol` is malformed, unexpected, partially truncated, or trailing framing/control data. `Closed` is EOF before any reply byte or later use of a retired client. | Move because `Server` contains an owned string. There is no message synthesis, logging, retry, reconnect, redirect handling, or second cleanup error. Once a package operation selects a terminal error, later cleanup cannot replace it; resolved-address iteration has the separate last-failed-candidate rule below. A complete bounded `Server` response and a complete `Decode` leave the synchronized client reusable; `Invalid` occurs before I/O. `Io`, `ResponseTooLarge`, `Protocol`, and first-observation `Closed` retire it. | A nonempty `Server` owns one string allocation; empty `Server("")` uses the canonical `{null, 0}` owned string and allocates no result buffer. Moving transfers the representation and Drop frees a nonempty buffer normally. No reply view or scratch buffer escapes. Other variants allocate nothing. | Ordinary package sum identity; `Io` reuses the always-available `core.Error` identity without changing its tags. | Variant/payload/order/interface owners; every producer x reuse/close state; empty/nonempty owned-error allocation, escape, and Drop; whole/per-unit and malformed-HIR owners. |
| `pkg.kv.connect(host: str, port: i64, options: ClientOptions) -> Result<client, Error>` | Arguments evaluate once, left-to-right. Validate nonempty host with no U+0000, then `port` in `1..=65535`, then option fields in source order, all before side effects. Host is otherwise passed byte-exact UTF-8 to the system resolver; there is no URL, default host/port, database number, credential, environment, or config file. | The resolver's usable addresses are attempted in returned order; unsupported families, null addresses, and zero address lengths are skipped, and the first successful connection wins. If no usable entry exists, return `Io(core.Error.Invalid)`; if all attempted entries fail, return the last socket/connect/mode-transition failure. Success publishes only a socket whose checked nonblocking connect completed and whose blocking mode was checked-restored, then strictly installs both retained I/O timeouts, constructs reader then writer, and sends no Redis bytes. Timeout-configuration failure closes that already selected connection without trying another resolved address. DNS and the aggregate address list have no end-to-end deadline. Impure. | Host is borrowed only through resolution and copied transiently once into the runtime's NUL-terminated resolver input. Success retains exactly four allocations: connection, reader shell, writer shell, and package state. Every failed candidate socket and resolver allocation is cleaned by the runtime; timeout-configuration failure closes the new socket before wrapper/state construction and publishes no client. Wrapper/state OOM follows the hard-abort policy. | Ordinary package source uses exact compatible externs for shipped `align_rt_tcp_connect`/free and the planned unkeyed `align_rt_tcp_conn_set_io_timeout`. The compiler registry recognizes the physical timeout symbol for fixed-ABI compatibility, collision, and reachability; there is no new language builtin, checked-HIR/MIR operation, ABI shape, or call-spelling selector. | Shipped TCP/resource plus timeout-substrate hardening and the planned checked-timeout row; validation/no-side-effect, resolver ordering/skips/empty/mixed failure, checked mode transitions, timeout quantization/precedence, IPv4/IPv6 loopback, native status, strict timeout installation, construction/cleanup/effect, and whole/per-unit owners. |
| `pkg.kv.get(borrow mut owner: client, key: str) -> Result<Option<string>, Error>` | Receiver then key evaluate once. Validate live state, then key length `0..=536870912`, then checked canonical RESP request length before allocation/I/O. Empty UTF-8 keys and embedded NUL/CR/LF are valid because the request uses a bulk string. | Sends exact uppercase two-element RESP2 `GET`. A bulk reply returns one owned `Some(string)`; null bulk `$-1` returns `None`; zero-length bulk returns `Some("")`. A complete non-UTF-8 bulk returns `Decode` after consuming it and keeps the client live. Any complete bounded `-` reply is classified after framing as `Server` when UTF-8 or reusable `Decode` otherwise. Other type/length/framing, partial EOF, or bytes trailing the completed reply in the current read are `Protocol`; EOF before a byte is `Closed`; an over-cap declared bulk is `ResponseTooLarge` without drain. Impure. | Key is borrowed only through synchronous writes and never retained. A nonempty successful GET publishes one ordinary owned string allocation; empty `Some("")` uses canonical `{null, 0}` and `None` has no result allocation. Retained reader/writer shells survive synchronized success, while receive chunks, line state, conversion storage, and unpublished output are operation-owned and dropped on every exit. No returned value borrows client/key/scratch. | Package source owns RESP assembly/parser state over the existing TCP-derived writer, reader, buffer, and UTF-8 rows plus exact compatible externs. The generic writer prerequisite suppresses SIGPIPE for every connection-derived writer; there is no package-specific write row or runtime parser. | Official RESP2/GET semantics; independent wire vectors, fragmentation/coalescing, null/empty/nonempty allocation, exact/next bound, UTF-8/NUL/CRLF, ownership/Drop, safe-write, error/reuse, and loopback owners. |
| `pkg.kv.set(borrow mut owner: client, key: str, value: str, options: SetOptions) -> Result<bool, Error>` | Receiver, key, value, options evaluate once, left-to-right. Validate live state; key then value lengths independently in `0..=536870912`; condition; expiry; then every request-length/decimal calculation before allocation/I/O. Empty and embedded-NUL/CR/LF key/value bytes are valid. | Emits one canonical RESP2 `SET`: `SET key value`, optional `NX`/`XX`, then optional `PX <ceil-ms>`, in that order. Exact `+OK` returns `true`. Null bulk `$-1` returns `false` only for `IfAbsent`/`IfPresent`; it is `Protocol` for `Always`. A complete bounded `-` frame is `Server` when UTF-8 or reusable `Decode` otherwise. Every other success spelling/type/integer/bulk/framing or current-read trailing byte is `Protocol`. Impure. | Inputs are borrowed only during the call. Request framing uses bounded operation-owned decimal/header storage and writes key/value directly without retaining or cloning them. The bool result allocates nothing. The retained writer/reader shells survive synchronized success; all operation scratch drops before return. | Ordinary package source uses the hardened existing connection-derived writer plus existing read/buffer rows. Redis owns atomic SET condition/expiry behavior and its server clock; the package reads no clock. | Official SET semantics; three conditions x two expiry states, exact ns/ms edges, persistence/expiry behavior, collision/non-resurrection use, byte goldens, partial-write/response failure, and ownership/effect owners. |
| `pkg.kv.delete(borrow mut owner: client, key: str) -> Result<bool, Error>` | Receiver then key evaluate once. Validate live state, key length `0..=536870912`, and canonical request arithmetic before I/O. Key byte admission matches `get`. | Sends exact uppercase one-key RESP2 `DEL`. Any valid RESP signed-i64 integer spelling whose value is zero (`0`, optional sign, or leading zeros) returns `false`; any whose value is one returns `true`. Every other value/overflow or reply type is `Protocol`. A complete bounded `-` frame is `Server` when UTF-8 or reusable `Decode` otherwise. Impure. | Key is call-bounded and retained nowhere. The bool result and normal request framing require no value-sized allocation. Retained writer/reader shells survive synchronized success; all operation scratch drops before return. | Same hardened existing writer/read boundary; no package-specific write row or multi-key overload. | Official DEL semantics; zero/one optional-sign/leading-zero spellings, negative/two/overflow/type mutations, error, fragmentation, ownership, effect, and reuse owners. |

## Decision and scope

The first capability is deliberately one request/one reply over one opaque mutable owner:

```text
system resolver + plaintext TCP + RESP2  ->  pkg.kv.client
GET                                       ->  Option<owned string>
SET + explicit condition + explicit TTL   ->  bool applied
single-key DEL                            ->  bool removed
```

This is enough for the first demonstrated consumer without becoming a second Redis protocol API.
`pkg.auth.session_token()` supplies a high-entropy key but deliberately promises no uniqueness;
`IfAbsent` gives the caller an atomic collision check. `IfPresent` refreshes or replaces an
existing session without resurrecting one that expired or was revoked. An optional duration gives
session storage an explicit server-side expiry, and one-key DEL supplies logout/revocation.

GET, SET, and DEL are typed operations, not strings passed to a generic command function. Their
closed reply shapes let the client prove synchronization before reuse. There is no public RESP
value sum and no escape hatch that could leave unread nested data behind the typed state machine.

## Public use

Declarations and calls are shown separately because Align calls are positional:

```align
import pkg.kv

fn open() -> Result<pkg.kv.client, pkg.kv.Error> {
  options := pkg.kv.ClientOptions {
    connect_timeout_ns: 1000000000,
    io_timeout_ns: 1000000000,
    max_response_bytes: 1048576,
  }
  return pkg.kv.connect("127.0.0.1", 6379, options)
}
```

```align
import pkg.kv

fn create_session(
  borrow mut store: pkg.kv.client,
  key: str,
  payload: str,
) -> Result<bool, pkg.kv.Error> {
  options := pkg.kv.SetOptions {
    condition: pkg.kv.SetCondition.IfAbsent,
    expires_in_ns: Some(900000000000),
  }
  return pkg.kv.set(store, key, payload, options)
}
```

```align
import pkg.kv

fn revoke(
  borrow mut store: pkg.kv.client,
  key: str,
) -> Result<bool, pkg.kv.Error> = pkg.kv.delete(store, key)
```

No example relies on named arguments, optional parameters, implicit endpoint/configuration, method
dispatch, a client clock, or unimplemented syntax.

## Input bounds, options, and validation precedence

V1 fixes `536870912` bytes (512 MiB) as the inclusive maximum for each request key/value and for a
configured response cap. This matches the ordinary RESP bulk-string ceiling while remaining a
client contract even when a server is configured differently. Key/value inputs are already
caller-owned and are written as length-delimited bytes rather than copied into a value-sized
request. `max_response_bytes` makes the only value-sized receive allocation explicit. Exact limit
succeeds; the next byte does not.

All public strings are valid UTF-8 by type. RESP bulk framing therefore admits empty text and
embedded U+0000, CR, or LF without escaping. `connect` alone rejects U+0000 because the shipped
system resolver requires one transient C string. No normalization, prefix, namespace, hash tag,
case fold, or Unicode reinterpretation occurs.

`connect` validates host, port, `connect_timeout_ns`, `io_timeout_ns`, then
`max_response_bytes`. A command first validates the complete resource state, so `Closed` wins over
an invalid key/options product and performs no I/O. It then validates key, value where present,
condition, expiry, and checked wire arithmetic in that order. `set` converts positive nanoseconds
without addition overflow as `ns / 1000000 + (if ns % 1000000 == 0 { 0 } else { 1 })`; the result
is always a positive i64 millisecond count. No builder, native call, or socket write precedes the
complete public validation pass.

The two timeout fields have exact substrate meanings rather than one hidden wall-clock promise:

- `connect_timeout_ns` records a fresh monotonic start and positive `Duration` budget for each usable
  socket address immediately before its first `F_GETFL`, after synchronous DNS resolution. It never
  forms an absolute `start + budget`, so the shared substrate's complete positive-i64 input range
  cannot overflow into an unbounded wait. Before `connect`, both `F_GETFL` and `F_SETFL(O_NONBLOCK)` are
  checked. Failure records that mapped status, closes the candidate, and advances to the next
  address without calling `connect`. After checked installation, one immediate `connect` is issued:
  zero succeeds, `EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` enter the wait, and every other errno is mapped
  immediately. Either immediate terminal result wins even if the budget is simultaneously
  exhausted. The in-progress path waits with `poll` for the positive remaining duration rounded
  **up** to the next millisecond and saturated at `i32::MAX`; an already-reached deadline is
  `AL_TIMEOUT`, `EINTR` recomputes the
  remainder, and zero from `poll` causes a monotonic-budget recheck and re-poll if time remains.
  Once the budget is exhausted, `AL_TIMEOUT` returns before any additional `poll` call. Any other
  poll error is mapped immediately. A positive readiness/error event from an already-running poll
  wins over a simultaneously exhausted budget and is resolved through `SO_ERROR`. Every successful
  immediate or polled connect then checks both
  `F_GETFL` and `F_SETFL(flags & !O_NONBLOCK)`; restoration failure closes that candidate and
  records the mapped failure. No socket is published unless blocking mode was checked-restored.
  Scheduler/kernel delay may return after the requested instant, so this is a logical wait deadline,
  not an impossible end-to-end wall-clock guarantee. It does not bound DNS or the sum across
  multiple resolved addresses.
- `io_timeout_ns` is installed through a checked package-internal TCP row onto both blocking socket
  receive/send timeouts. Every positive nanosecond value is rounded **up** to the next microsecond
  before splitting into normalized `timeval { tv_sec, tv_usec: 0..999999 }`; exact microseconds stay
  exact and `0` remains the existing clear/no-timeout value outside this package's admitted range.
  The kernel may schedule return later than the option value. The option bounds one blocking
  read/write wait for progress, not the total duration of a multi-read command. A timeout returns
  `Error.Io(core.Error.Timeout)` and closes the client because a request may be partially sent or a
  response partially consumed.

The resolver's order is observable. Unsupported families, null addresses, and zero address lengths
are skipped without changing the last failure. The first successful usable address wins. With no
usable address the substrate returns `AL_INVALID`; when every attempted candidate fails it returns
the last socket, nonblocking-install, connect/poll/`SO_ERROR`, or blocking-restoration status.
Package-level I/O-timeout installation happens only after this selection: its failure closes the
selected unpublished connection and is returned without reopening resolution or trying another
address. Later cleanup failures never replace that selected error.

## Canonical RESP2 bytes

Commands are arrays of bulk strings, with uppercase ASCII command names, canonical unsigned decimal
lengths, and exact CRLF. The package emits only these four shapes:

```text
GET(k)                  = *2\r\n$3\r\nGET\r\n$K\r\n<k>\r\n
SET(k,v,Always,None)    = *3\r\n$3\r\nSET\r\n$K\r\n<k>\r\n$V\r\n<v>\r\n
SET(k,v,c,Some(ns))     = *5/*6, SET k v [NX|XX] PX ceil-ms
SET(k,v,c,None)         = *3/*4, SET k v [NX|XX]
DEL(k)                  = *2\r\n$3\r\nDEL\r\n$K\r\n<k>\r\n
```

Here `K` and `V` are UTF-8 byte lengths; the notation on the two SET rows selects array length five
versus six and three versus four according to whether a condition token is present. The exact
semantic-to-wire goldens include:

```text
GET "k"                                  *2\r\n$3\r\nGET\r\n$1\r\nk\r\n
SET "k" "v" Always persistent           *3\r\n$3\r\nSET\r\n$1\r\nk\r\n$1\r\nv\r\n
SET "k" "v" IfAbsent 1500001 ns         *6\r\n$3\r\nSET\r\n$1\r\nk\r\n$1\r\nv\r\n$2\r\nNX\r\n$2\r\nPX\r\n$1\r\n2\r\n
DEL "k"                                  *2\r\n$3\r\nDEL\r\n$1\r\nk\r\n
```

Independent byte-to-semantic response goldens are `$3\r\none\r\n` → `Some("one")`,
`$-1\r\n` → `None`, `$0\r\n\r\n` → `Some("")`, `+OK\r\n` → SET `true`, conditional
`$-1\r\n` → SET `false`, `:0\r\n` / `:1\r\n` → DEL false/true, and
`-ERR denied\r\n` → `Server("ERR denied")`. Test or implementation code must reproduce these bytes
independently rather than deriving both sides from the package encoder/parser.

## Reply grammar, synchronization, and error precedence

The parser starts in the expected command shape but recognizes a RESP error first for every
operation. V1 accepts only the server forms needed by the closed surface:

- GET: `$-1\r\n`, or `$<one-or-more-digit nonnegative decimal>\r\n<payload>\r\n`;
- SET: exact `+OK\r\n`, plus `$-1\r\n` only for a conditional SET;
- DEL: a signed-i64 integer frame whose parsed value is exactly zero or one; and
- every command: `-<0..=max_response_bytes arbitrary payload bytes>\r\n`.

Request length/count text is canonical unsigned ASCII. Response bulk length accepts one or more
unsigned decimal digits, including leading zeros, plus exact `-1` for null. Its digit grammar is
checked before magnitude; a valid magnitude above the configured cap is `ResponseTooLarge` even
when it would not fit i64, while every admitted magnitude necessarily fits. RESP integer text
accepts an optional `+`/`-`, one or more digits including leading zeros, and must fit signed i64.
Every otherwise admitted non-error control line is capped at 64 bytes excluding its marker and
CRLF; exact cap succeeds. A recognizable invalid byte or i64 overflow is `Protocol` immediately;
requesting the 65th still-unresolved control byte is `ResponseTooLarge`. CRLF is exact. Arrays,
RESP3 types, nested replies, null arrays,
alternate simple strings, semantically wrong integers, lone CR/LF, and bytes after the completed
reply in the current native read are protocol failures. Input may arrive one byte at a time or
several response parts in one read; framing is independent of TCP chunk boundaries.

The response decision order is fixed:

1. A negative native read status becomes `Io(core.Error)`. EOF before any response byte is
   `Closed`; EOF after a prefix is `Protocol`. Each retires the client.
2. A recognized `-` frame is bounded and framed over arbitrary bytes before text classification.
   Crossing the inclusive cap returns `ResponseTooLarge` and closes without drain. After terminal
   CRLF and same-read trailing-byte validation, a complete non-UTF-8 payload returns `Decode`;
   otherwise clone it into `Server`. Both complete synchronized outcomes leave the connection live.
3. Reject a reply marker not admitted for the current command, then validate its canonical line
   grammar and semantic value. Any failure is `Protocol` and closes.
4. For GET, validate every observed length byte before comparing the valid digit magnitude; reject
   a magnitude over the cap as `ResponseTooLarge` and close without drain. Read exactly the payload
   and terminal CRLF. A complete non-UTF-8 payload returns `Decode` while
   keeping the client live; otherwise publish its owned clone.
5. Before any success, `Server`, or `Decode` publication, reject bytes trailing the complete frame
   in the same native read as `Protocol` and close. UTF-8 classification and final cloning happen
   only after this check. Later unsolicited bytes cannot be distinguished
   from a future server reply; V1 relies on Redis's one-reply-per-command contract and never
   pipelines.

Every fragment uses the existing connection-derived `writer`, hardened for every `std.net`
consumer rather than bypassed by a package-only row. Its private sink kind selects Linux
`send(MSG_NOSIGNAL)` or checked macOS/BSD `SO_NOSIGPIPE` plus `send`; file and standard-stream
writers keep the existing generic fd path. The retained macOS/BSD writer shell caches only a
successful option installation; a failed first attempt sends no bytes and a later call retries it.
The socket option is monotone and idempotent. If separate shells overlap, each shell requires its
own successful installation result before sending; one shell's failure sends no bytes even if
another shell succeeds. No shell clears the option, and closing the owning connection discards it.
Partial writes and EINTR retain the one shared writer
loop, socket timeout retains the shared read/write status mapping, positive-length zero progress is
deterministic `core.Error.Code(0)`, and an option-install failure sends no bytes. Any writer error
occurs after zero or more request bytes may have reached the peer, so it becomes `Io(core.Error)`
and closes. There is no automatic replay even for GET/DEL. Cleanup cannot replace an earlier error
and Drop cannot report a later close failure.

The writer prerequisite directly owns macOS/BSD failed-install/no-send followed by retry and
success, two overlapping shells with success/failure in both orders, shell Drop without option
clear, and connection close discarding the setting. Linux/macOS subprocess owners cover direct
writer, logger, and `io.copy` routes to a closed peer and require a returned `Error` rather than
signal termination. File/standard-stream parity and partial/EINTR/timeout/zero-progress owners
remain independent of those state-transition tests.

## Ownership, allocation, state, and cleanup

The public resource is one native word pointing to a package-owned 40-byte, 8-aligned v1 state:

```text
offset 0   u32 version = 1
offset 4   u8  state: 0 live, 1 closed
offset 5   u8  zero
offset 6   u16 zero
offset 8   raw runtime TCP connection: non-null iff live
offset 16  raw non-owning runtime reader shell: non-null iff live
offset 24  raw non-owning unbuffered runtime writer shell: non-null iff live
offset 32  i64 max_response_bytes: 0..=536870912
```

Every operation validates the complete record before dereferencing or calling through a retained
pointer. Closing copies
the three pointers to locals, stores state 1 and null at offsets 8/16/24, then frees writer, reader,
and TCP owner in that order. The unbuffered non-owning writer has no pending flush and neither shell
closes the fd. Drop repeats the same live-to-closed transition when needed, then frees the package
state; a previously closed value frees only the state. Any other version, tag, reserved byte,
pointer/state product, or retained bound is an internal malformed-state failure: the public
operation returns `Closed` without native I/O, and Drop hard-aborts rather than call an untrusted
pointer. Safe consumer code cannot construct or mutate this record.

Commands borrow the resource mutably, so no second operation, task capture, replacement, move, or
Drop can overlap the current request/reply. Network effects make every operation Impure and therefore
ineligible for parallel closures independently of the resource rule. No lock, shared client, global
registry, background reader, callback, or reversible post-publication connection-global mode
transition exists. On macOS/BSD, the retained writer's monotone SIGPIPE-ready transition follows
the failure/retry/close rule above and cannot overlap another package operation.

The reader and writer shells are constructed once by `connect` and reused without per-command shell
allocation. Request headers and decimal text are bounded operation storage. Key/value bytes are
written from their call-bounded `str` views and retained nowhere. Receive chunks and framing state
are bounded by the response cap plus fixed protocol overhead. A nonempty successful GET or `Server` error allocates
one ordinary owned string result after the complete frame is synchronized; an empty `Some("")` or
`Server("")` publishes the canonical `{null, 0}` owned string with no final buffer allocation. Peak
storage for a nonempty result may contain both the N-byte receive buffer and its N-byte final owned
copy because V1 adds no consuming buffer-to-string freeze. Every native receive buffer compares its
actual `buffer.capacity()` with the requested positive capacity before the first read; a mismatch
hard-aborts under the OOM policy
instead of masquerading as EOF.
Intermediate raw/source buffers are unpublished and dropped first on every error. OOM follows the
language's existing hard-abort contract. Beyond the exact four allocations retained by a live
client, no per-command scratch/result allocation-count, zero-copy receive, zeroization, throughput,
or latency promise is made.

## Package, runtime, artifact, and cache boundary

The vendorable subtree owns root `pkg.kv` plus `pkg.kv.internal.resource`. Its internal modules
import `std.process`; every impossible native state calls `process.abort()`, selecting the shipped
keyed `ProcessAbort` row rather than inventing an extern or returning a recoverable package error.
Their source uses exact type-compatible extern declarations for the already keyed TCP
connect/free/reader/writer, I/O read/write/free, buffer new/bytes/capacity/free rows, and one planned
source-reachable unkeyed row:

```align
extern "C" {
  fn align_rt_tcp_connect(host: str, host_len: i64, port: i64, timeout_ns: i64, out: raw) -> i32
  fn align_rt_tcp_conn_free(connection: raw)
  fn align_rt_tcp_conn_reader(connection: raw) -> raw
  fn align_rt_tcp_conn_writer(connection: raw) -> raw
  fn align_rt_tcp_conn_set_io_timeout(connection: raw, timeout_ns: i64) -> i32
  fn align_rt_io_reader_read(reader: raw, output: raw) -> i64
  fn align_rt_io_reader_free(reader: raw)
  fn align_rt_io_writer_write(writer: raw, bytes: slice<u8>, length: i64) -> i32
  fn align_rt_io_writer_free(writer: raw)
  fn align_rt_buffer_new(capacity: i64) -> raw
  fn align_rt_buffer_bytes(buffer: raw, out: raw)
  fn align_rt_buffer_capacity(buffer: raw) -> i64
  fn align_rt_buffer_free(buffer: raw)
}
```

An FFI `str`/`slice<u8>` contributes its data pointer only; the adjacent explicit length is
therefore required for exact compatibility and is always the same source view's `.len()`. The one
new row is:

```text
TcpConnSetIoTimeout  align_rt_tcp_conn_set_io_timeout  i32(ptr, i64)  // ABI A04
```

`TcpConnSetIoTimeout` first rejects a null connection with `AL_INVALID`, then rejects a timeout
outside `1..=86400000000000` with `AL_INVALID`; neither rejection reads the fd or calls
`setsockopt`. A non-null dangling pointer is outside the unsafe native ABI provenance contract and
is not detectable. For admitted inputs it constructs the normalized positive-timeout `timeval`
above, installs `SO_RCVTIMEO`, and returns its fixed errno-mapped status without attempting
`SO_SNDTIMEO` if that call fails. Otherwise it installs `SO_SNDTIMEO` and returns that status, or
zero only after both succeed. A second-option failure leaves the first installed, but the package
immediately closes the unpublished connection without retrying another resolved address.
Configuration cannot overlap another operation, and no rollback is required. A parameterized
direct-runtime owner fixes the exact `timeval`, option order, option-call counts, returned status,
and `{receive failure, send failure, both succeed}` state product, including package close/no-publish
behavior after the second case. The row allocates, retains, and closes nothing. It is a mandatory base
export, source-reachable compatible extern, and collision-reserved unkeyed identity. It reuses an
existing ABI shape, so activation changes the exact base/maximum counts from 347/355 to 348/356
while the keyed count remains 330. The unkeyed count becomes eighteen, thirteen of which are
source-reachable, and A123 remains the next unreserved shape. Its LLVM and Rust definitions use the
existing A04/default-C-calling-convention contract with no curated function, return, or parameter
attributes.

Ordinary package source decodes every native status explicitly; it cannot use `error(status)`, which
always constructs `core.Error.Code(status)`. For the i32-status rows, zero is success, `1`, `2`,
`3`, and `4` map to `NotFound`, `Invalid`, `Denied`, and `Timeout`, and every `5..=i32::MAX` maps to
`Code(status - 5)`. A negative i32 status hard-aborts as an impossible ABI result. A reader result is
positive bytes, zero EOF, or a negative encoded status. Source classifies it before parser state
changes: `i64::MIN` and every value below `-(i32::MAX as i64)` abort before buffer-view inspection;
an admitted negative is checked-negated, explicitly narrowed to i32, and requires an empty buffer
view before the same status table is applied; zero requires an empty view and means EOF; a positive
count first must be at most the requested buffer capacity, then requires a view of exactly that
length before parsing. An oversized positive count or any negative/zero/positive view-length
mismatch aborts. The runtime owns the view pointer provenance; a forged matching non-null pointer is
outside the unsafe native ABI contract and is not detectable. For `align_rt_tcp_connect`, zero
requires a non-null output connection and nonzero requires null; a contradictory status/pointer product
hard-aborts before ownership changes. Every category sentinel, `Code(0)`, representative positive
code, signed-width boundary, byte-count/view product, and malformed product has an independent
owner. Every hard-abort branch is ordinary package source using the explicit `std.process`
dependency and is exercised before later parsing, publication, or ownership change.

The existing `TcpConnWriter`/`IoWriterWrite`/`IoWriterWriteBuilder`/`IoWriterFree` identities,
declarations, attributes, and counts do not change. `IoWriterWriteBuilder` keeps delegating to
`IoWriterWrite`, so the source-visible builder overload reaches the same sink policy. The private
runtime `Writer` gains a socket sink kind and macOS/BSD ready
bit set only by `align_rt_tcp_conn_writer`. Nonempty writes from that kind use the SIGPIPE-safe send
policy above and cache only a successful `SO_NOSIGPIPE`; all other constructors retain their
byte-identical fd path. The option is a monotone per-socket setting: overlapping shells may each
attempt it, each sends only after its own success, a failed shell remains retryable, and no shell
Drop restores or clears it. Connection-derived writers remain unbuffered and non-owning, so their
free path performs no hidden write and never closes the socket; connection close discards the
option with the fd.

Source extern compatibility reuses every registry row's exact LLVM types, attributes, symbols, and
runtime definitions; it does not declare a second physical symbol or bypass collision checking. The
compiler therefore recognizes `align_rt_tcp_conn_set_io_timeout` as one fixed physical ABI symbol,
while the package adds no language builtin, HIR/MIR variant, call-spelling capability selector,
reflection table, static artifact, schema input, or environment option.
`docs/impl/19-hir-validation-ledger.md` remains unchanged; `docs/impl/20-runtime-abi-ledger.md`
reserves the exact one-row inactive delta and pins the existing writer hardening without changing
its ABI.

Whole-program compilation sees the ordinary package bodies. Per-unit compilation serializes the
resource, `ClientOptions`, `SetCondition`, `SetOptions`, `Error`, and four public signatures, while
the producer object retains the resource Drop thunk and existing native dependencies. Current
capability collection is module-wide, so use of any operation retains the complete root/internal
TCP, I/O, buffer, and keyed `ProcessAbort` set; no call-spelling selector changes that set. Under
ordinary per-unit cache identity, any edit to a unit's source bytes misses that unit's frontend key.
A semantic private-body edit also misses that unit's structural object and causes the final link to consume the changed
object, while unchanged consumer frontends and objects hit because they depend on the dependency
interface hash, not its private implementation hash. An exported-surface edit changes the interface
hash and misses each reverse dependency frontend/object whose transitive interface set contains it.
A source-only edit whose span-erased semantic MIR is unchanged may re-hit the structural object. An
exact revert may re-hit the prior keys; unrelated units hit throughout.
Whole-program mode retains its ordinary complete-source identity. No endpoint, resolver result,
response, clock, ambient/runtime-inspected source file, or runtime inspection enters artifact
identity; the vendored package source itself is an explicit input.

The current function-value subset is scalar-only. These resource/reference/string/owned-Result
signatures therefore cannot form local, aggregate-field, or control-joined function values; V1 adds
no exception. A project without `pkg.kv` retains no package-specific source or native reachability.

## Complexity and performance boundary

Encoding and parsing are linear in visible key/value/reply bytes. No response byte is parsed more
than a constant number of times; no quadratic delimiter scan is permitted. SET condition and TTL
work are constant besides decimal formatting. The implementation may use bounded chunked socket
reads and writes, but V1 promises no syscall count, chunk size, allocation count, latency,
throughput, server-version performance, or memory-ratio target. No benchmark is an acceptance gate.

## V1 non-goals and later boundaries

No generic command/reply API, binary key/value sibling, PING, EXISTS, MGET/MSET, INCR/rate-limit
primitive, TTL query/touch-only operation, compare-and-swap, transaction, Lua/script, pipeline,
batch, pub/sub, stream, list/set/hash/sorted-set operation, RESP3/HELLO, client tracking, cluster or
Sentinel discovery/redirection, replica/read preference, pool, shared/thread-safe client,
reconnect/retry/backoff, AUTH/ACL credential, SELECT/database number, URL parser, configuration
file/environment, TLS/rediss, Unix socket, proxy, metrics, tracing, or framework session abstraction
is included. TLS and credentials must be designed together so V1 never encourages a plaintext
secret downgrade. Each later typed command or transport policy requires its own consumer and exact
ledger; none is reserved behind a string or option tag here.

## Implementation closure matrix

The shared timeout substrate is the first distinct prerequisite capability: it makes positive
connect deadlines enforceable, prevents publication of a nonblocking connection, and fixes the
shared connect/I/O quantization for the already-shipped `std.http` and `std.net` consumers without
changing an ABI identity. The generic TCP-writer hardening is a second independently useful
prerequisite with a closed signal-safety failure domain and likewise no public signature or ABI
identity change. The remaining new timeout row plus client/resource/parser/three commands form one
strict producer-to-consumer chain. A dormant row, parser-only, or connection-only package PR would
leave no stable public consumer, while splitting commands would repeat the same synchronization,
poisoning, fake-server, capability, and Drop proof. The package capability may exceed roughly 1,000
changed hand-written lines once the adversarial owner matrix is included; one boundary has lower
integration risk because every reply kind closes against the same state machine before publication.

| Axis | Required closure | Owner evidence |
|---|---|---|
| Public formation and identity | Exact module/resource/record/sum definitions, field and discriminator order, four signatures, qualification, visibility, direct/imported calls, generic consumer-wrapper monomorphization, current function-value rejection, whole/per-unit interface parity. | Public-source extraction, positive consumer compile/run, near-spelling/type/arity negatives, monomorphic/generic-wrapper parity, interface round-trip and generic alias controls. |
| Shared timeout substrate | Complete positive-i64 monotonic start-plus-budget arithmetic without an absolute deadline; per-address ceil-ns-to-ms `poll` conversion, zero-result recheck, no poll after exhaustion, immediate/readiness precedence, checked nonblocking install and blocking restore, and candidate close/continuation; `process.command` uses the same start/budget and ceil conversion while retaining its timeout-wins checkpoint order; positive socket timeouts shared by `std.net`/`std.http` ceil ns to normalized `timeval` microseconds; every zero-timeout behavior remains unchanged. | Direct exact/next/maximum ns/us/ms/chunk/deadline owners; `F_GETFL`/`F_SETFL` install and restore failpoints on immediate and polled success; `EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` and other immediate errno; early zero-result versus exhausted/no-call poll, EINTR, readiness-at-deadline, mixed-address, no-publication/blocking-mode, command capture/reap, and HTTP plain/TLS/pool-rearm probes. |
| Connect and option admission | Host/U+0000, port, three ordered option bounds, exact/next boundaries, no side effect before complete validation, DNS plus ordered/skipped/empty/first-success/last-failure address semantics, timeout application without address retry, every native status and pointer product, reader-then-writer-then-state construction, IPv4/IPv6, no Redis bytes. | Instrumented connect/runtime counters and loopback listeners parameterize socket failure, nonblocking GET/SET failure, immediate errno, poll error/timeout, `getsockopt` failure/nonzero `SO_ERROR`, blocking-restore GET/SET failure, a skipped entry after each failure, and a later success; each vector fixes last attempted status and close count. Malformed status/pointer, retained-allocation, and allocation/cleanup failpoints close the remaining products. |
| Request bytes | Exact GET/SET-product/DEL arrays, uppercase tokens, canonical decimal, 512-MiB admission without value-sized request copy, embedded NUL/CR/LF, all arithmetic before first write. | Independent semantic-to-byte goldens, boundary/mutation table, fragmented writer and partial-write owner. |
| Reply framing | Every marker, official bulk/integer grammar including leading zeros and integer signs, 64-byte control cap, CRLF, fragmented/coalesced reads, null/empty/exact/next cap, error cap, trailing bytes, EOF at every ordinal, no quadratic scan. | Deterministic scripted TCP peer with one-byte/all-split/multi-part products, independent byte-to-semantic goldens, comparison counter. |
| GET and error semantics | Bulk/null/empty ownership, exact bytes, arbitrary bounded error bytes, UTF-8 classification only after framing/trailing validation, Decode reuse, wrong kinds and oversized declaration close. | Official vectors plus empty/nonempty valid/invalid UTF-8, empty `Server`/GET allocation and Drop, lifetime escape, subsequent-command, and no-drain probes. |
| SET semantics | Always/NX/XX x None/Some, condition-before-PX wire order, ceil-ns conversion, persistent TTL removal, +OK/null result matrix, server errors, unexpected status. | Scripted bytes plus a Redis-compatible state model for collision, expiry, refresh-without-resurrection, exact/next duration, and wrong-reply products. |
| DEL semantics | One-key request, every official signed/leading-zero spelling of values 0/1, server error, every other value/type. | False/true and sign/leading-zero/negative/two/overflow/type mutation matrix with reuse/close checks. |
| Error, native status, and poison state | Invalid before I/O; bounded UTF-8 Server and complete non-UTF-8 Decode reusable; exact `0/1/2/3/4/>=5` status decode; reader `{invalid negative, admitted negative, zero, admitted positive, oversized positive}` x exact/mismatched view products with checked i32 narrowing; Io/too-large/protocol/truncation/partial-write close; selected terminal error retained over cleanup; every later call Closed with zero I/O; every impossible product reaches `process.abort` before parser/publication/ownership change. | Error-producer x command x before/during/after-frame x reuse table; every category/representative code/width/count/view/malformed product; native call counters, explicit `ProcessAbort` IR/capability retention, no-import negative, and selected-error/cleanup-failure probes. |
| Ownership and cleanup | Resource formation, move-in/out/return/replacement, if/match/else/?/map_err/branch/loop/early return, source nulling, state/socket/wrapper/scratch/result Drop once, malformed state no untrusted call. | Resource/drop counters, allocation parity, parameterized control-flow owner, state semantic-to-byte and byte-to-semantic goldens, malformed field product. |
| ABI, effects, capabilities, and cache | Null-then-range/no-side-effect validation and atomic activation of fixed-symbol `TcpConnSetIoTimeout`; exact receive-then-send option state product; default-C A04/no-curated-attribute identity; existing connection-writer sink provenance and partial/EINTR/zero/EPIPE/timeout mapping through slice and builder overloads; Linux/macOS SIGPIPE state and transitive routes without writer ABI/count changes; no new ABI shape, language builtin, HIR/MIR row, or selector; Impure operations; module-wide TCP/I/O/buffer/`ProcessAbort` retention; package absence; exact own-source/public-interface/private-dependency cache outcomes. | Exact registry/golden/base-export/type/attribute/collision/source-reuse, null x range, exact-timeval, receive-fail/no-send, send-fail/receive-retained/package-close/no-address-retry, and both-success owners; failed-install/retry/overlap/Drop plus file/std/direct/slice/builder/logger/`io.copy` writer owners; package whole/per-unit IR/link runs, effect checks, exact `ProcessAbort` dependency, six-field resource mutations, no-package negative, private/public/add/remove/edit/revert cache twins. |

## Sources of truth and author consistency pass

This English ledger, `docs/impl/pkg-design/ja/kv.md`, `draft.md`,
`docs/language-spec.md`, `docs/design-notes.md`, `docs/history.md`,
`docs/open-questions.md`, `docs/impl/07-roadmap.md`, the English/Japanese `std-design` documents for
`net`, `http`, and `process`, `docs/impl/20-runtime-abi-ledger.md`, and `HANDOFF.md` must agree. The
HIR ledger remains unchanged; any ABI change beyond the exact one-row reservation or any public
writer-surface change reopens this design.

During candidate review, `docs/open-questions.md` keeps this item under Open and
`docs/history.md` has no settled entry. Acceptance must move the exact reviewed contract to
Settled, add its history record, change every candidate status to accepted/inactive as applicable,
and only then authorize implementation.

After the finding ledger was repaired, the revised author-side ledger-to-prose and closure-matrix
consistency pass completed on 2026-09-02 before the fresh complete review:

- every public argument/result has one exact type, evaluation order, default, ownership, lifetime,
  allocation, cleanup, error, and effect rule;
- command, condition, expiry, response marker, verification state, option state, field presence,
  row order, discriminator, and unavailable-result products are exhaustive;
- host/key/value/error text fixes UTF-8, embedded-NUL, CR/LF, boundary validation, and
  pre-side-effect semantics;
- multi-invalid calls have deterministic state/host/port/option/key/value/condition/expiry/wire and
  native/reply/error precedence;
- shared connect, HTTP socket, and command-capture timeout consumers fix start/budget arithmetic,
  ceil conversion, zero-result/exhaustion behavior, and their distinct terminal-event precedence;
- native status, reader count x view, connect status x output, and receive/send option-call products
  are exhaustive; impossible products reach the explicit existing `std.process`/`ProcessAbort`
  dependency before parsing, publication, or later ownership change;
- no endpoint, credential, database, retry, clock, resolver result, configuration, artifact, or
  runtime-inspected source input is ambient; vendored package files remain explicit compiler input;
- canonical RESP scalars, tags, sequence order, malformed rejection, and independent
  semantic-to-byte plus byte-to-semantic goldens are fixed;
- the resource record and RESP state machine fix every state/tag/reserved/pointer/length product,
  overlap exclusion, failed-second-operation behavior, error preservation, and Drop order;
- exact existing producer-owned runtime rows supply native state without reflection or artifact I/O;
  slice and builder writer overloads converge on the same hardened sink;
- examples use accepted syntax and separate declarations from positional calls; and
- acceptance owners cover every ledger invariant, with no unpromised benchmark used as a gate.

Official protocol/command references: Redis
[RESP](https://redis.io/docs/latest/develop/reference/protocol-spec/),
[GET](https://redis.io/docs/latest/commands/get/),
[SET](https://redis.io/docs/latest/commands/set/), and
[DEL](https://redis.io/docs/latest/commands/del/).

## Design-review finding-to-fix ledger

The exact-base independent review of
`ad5d6969194c26b4cbd8c7521d15ed6ac05f49f7...45a5cea85579c2dd5170cd6e41958f114bcad3c3`
returned one P1, ten P2 findings, and one P3 finding. This ledger records the authoritative repair;
every row must propagate through the Japanese mirror and synchronized summaries before a fresh
complete review may accept the design.

| Finding | Authoritative correction and closure owner |
|---|---|
| P1 unchecked connect fd mode | Add the independently useful shared-timeout prerequisite: checked `F_GETFL`/`F_SETFL` install and restore, close-and-continue on failure, and no publication before checked blocking restoration. Direct immediate/polled failpoints own every transition. |
| P2 timeout quantization/precedence | Use monotonic start-plus-budget arithmetic for the complete positive-i64 range. Positive connect and `process.command` waits ceil ns to milliseconds and recheck an early zero; connect lets immediate/readiness events win while command keeps its existing timeout-wins checkpoints. Positive `std.net`/`std.http` I/O options ceil ns to normalized microseconds. Exact/next/maximum and readiness-at-deadline owners pin every shared consumer. |
| P2 multi-address selection | Attempt usable resolver entries in order and return first success. With none, the substrate returns `AL_INVALID` and package source maps it to `Io(core.Error.Invalid)`; with attempted failures, return the last socket/connect/mode failure. Post-selection timeout configuration never restarts resolution. Mixed-failure owners pin ordering and the native/package error layers. |
| P2 native status decode | Package source implements the fixed `0/1/2/3/4/>=5` table; exhausts invalid-negative/admitted-negative/zero/positive reader count x view-length products with checked i32 narrowing; checks connect-status/output products; and uses its explicit `std.process`/`ProcessAbort` dependency for every impossible ABI result. Category/code/width/product and whole/per-unit capability owners close it. |
| P2 new-row malformed input | `TcpConnSetIoTimeout` validates null first, then the inclusive timeout range, returning `AL_INVALID` before fd access; dangling non-null provenance stays an unsafe precondition. Direct runtime evidence covers the null x range product plus exact `timeval` and receive-fail/no-send, send-fail/receive-retained, and both-success option-call products; the package closes a second-option failure without publication or address retry. |
| P2 RESP error grammar | Frame arbitrary bounded error bytes first, validate same-read trailing bytes, then select UTF-8 `Server` or non-UTF-8 reusable `Decode`. Empty/invalid UTF-8 and trailing-byte vectors own the distinction. |
| P2 empty owned allocation | Empty GET/`Server` results use canonical `{null, 0}` with no final buffer; only nonempty results own one. Empty/nonempty allocation and Drop counters own the rule. |
| P2 SIGPIPE state evidence | Add failed-install/no-send→retry, overlapping-shell order, Drop/no-clear, connection-close, and direct slice/builder/logger/`io.copy` closed-peer owners on the applicable platforms; the existing `IoWriterWriteBuilder` identity remains unchanged and delegates into the hardened write row. |
| P2 physical-symbol recognition | Retain compiler registry recognition for exact ABI compatibility/collision/reachability while adding no language builtin, HIR/MIR operation, ABI shape, or call-spelling selector. Wrong-type/collision/source-reuse owners pin it. |
| P2 resource interface identity | Pin all six serialized fields for non-generic `pkg.kv.client`, including exact generated thunk and `b"align-res-drop-1"`; mutate each independently in the interface owner. |
| P2 cache identity | Any own-source byte edit misses its frontend; a semantic private-body edit misses its own object/final link but leaves consumer frontend/object hits; public interface edits miss transitive reverse dependencies; a source-only semantic no-op may object-hit. Exact edit/revert cache twins own each scope. |
| P3 package inventory | Until source ships, normative summaries list four implemented vendorable subtrees and `pkg.kv` separately as a design candidate. |

Because the P1 changes capability boundaries and timeout strategy, acceptance requires one fresh
full review of the complete revised diff. Candidate status remains Open until that review is clean.
