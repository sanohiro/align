# pkg — kv

> English is authoritative. A synchronized Japanese mirror lives at `ja/kv.md`.
>
> **Status:** design candidate; no public contract is accepted until independent review closes.

## Authoritative public-contract ledger

This table is the authority for the first `pkg.kv` capability. Later prose and implementation may
make a field more explicit but must not widen it. V1 is one synchronous RESP2 text-value client over
plaintext TCP. It introduces no generic Redis command surface, protocol negotiation, compiler
operation, ambient endpoint, or hidden retry. One package-internal, source-reachable runtime row
closes checked timeout installation; the existing TCP-derived writer path is hardened in place so
all `std.net` consumers, not only this package, receive SIGPIPE-safe writes.

| Public surface | Exact inputs, defaults, validation, and evaluation | Exact result, errors, order, and effects | Ownership, lifetime, allocation, and cleanup | Compiler/runtime/package owner, artifact, and cache identity | Prerequisite and acceptance owner |
|---|---|---|---|---|---|
| `pub resource client = pkg.kv.internal.resource.drop_client` | One opaque non-null resource constructed only by successful `connect`. It is nominal, Move, non-Copy, non-comparable, non-printable, and has no public raw conversion or constructor. | A live value admits one synchronous mutable operation. A failed transport, oversized response, or malformed/unexpected/truncated RESP reply closes it before returning the first error; every later operation returns `Error.Closed` without I/O. A complete framed non-UTF-8 GET/error payload is instead reusable `Decode`. `Drop` is the sole public close operation. | The resource owns one package state allocation and, while live, one runtime TCP connection plus one non-owning reader shell and one non-owning unbuffered writer shell. Moving a live value transfers all four allocations; `borrow mut` is call-bounded and excludes overlapping requests. Drop frees writer then reader, closes the socket at most once, and frees the state exactly once. | `pkg.kv` owns the nominal identity and synthesized Drop thunk; `pkg.kv.internal.resource` owns the private state and hook. Existing resource interface identity includes the nominal name, representation version, and Drop-thunk fingerprint. | Shipped opaque resources and TCP; formation/visibility, Move/Drop, control-flow, malformed-state, close-once, later-Closed, whole/per-unit, and interface-identity owners. |
| `pub ClientOptions { connect_timeout_ns: i64, io_timeout_ns: i64, max_response_bytes: i64 }` | Exact field/source order as shown. No defaults. Both timeouts must be `1..=86400000000000` ns. `max_response_bytes` must be `0..=536870912` and is an inclusive cap for a GET bulk payload or owned RESP error payload. | Copy and Pure. Invalid fields return `Error.Invalid` during `connect`, in field order, before DNS, allocation, or socket work. A positive sub-microsecond I/O timeout inherits the shipped TCP clamp to one microsecond. | Three i64 fields; no borrow, allocation, Drop, or retained ambient state. On success the socket retains the installed I/O timeout settings and the package state retains the response cap; the connect timeout is consumed by construction. | `pkg.kv` owns the nominal definition. Whole-program/per-unit interfaces serialize its name and ordered fields; the complete definition enters interface, dependency, and cache identity. | Shipped i64-ns duration and TCP timeout machinery; field/order/exact/next-bound, no-default, sub-microsecond, whole/per-unit, and cache owners. |
| `pub SetCondition { Always, IfAbsent, IfPresent }` | Closed source and discriminator order is exactly `Always = 0`, `IfAbsent = 1`, `IfPresent = 2`. | Copy and Pure. It maps respectively to no condition token, `NX`, or `XX`. No integer/string selector or unknown fallback exists. | One Copy tag; no borrow, allocation, Drop, or retained state. | `pkg.kv` owns the nominal sum and interface discriminator order. | Exact tag/order, construction/match, interface, and malformed checked-HIR owners. |
| `pub SetOptions { condition: SetCondition, expires_in_ns: Option<i64> }` | Exact field/source order as shown. No defaults. `None` requests a persistent value. `Some(ns)` requires `1..=i64::MAX` and maps to Redis `PX` milliseconds using checked `ceil(ns / 1000000)`. | Copy and Pure. Invalid expiry is `Error.Invalid` before request construction or I/O. `None` deliberately uses plain `SET`, which removes an existing key TTL under Redis SET semantics. | One Copy tag plus i64 and one Copy condition; no borrow, allocation, Drop, clock read, or retained state. | `pkg.kv` owns the nominal definition; its complete reachable definition graph enters ordinary interface and dependency identity. | Exact condition/expiry product, ns-to-ms boundary/overflow, persistence/TTL interop, interface, and cache owners. |
| `pub Error { Invalid, Io(core.Error), Server(string), Decode, ResponseTooLarge, Protocol, Closed }` | Closed source/discriminator order is exactly the shown `0..=6`. `Invalid` is caller input/options. `Io` carries the unchanged builtin transport category/code. `Server` carries one complete UTF-8 RESP error payload. `Decode` is a fully consumed bulk or error string that is not UTF-8. `ResponseTooLarge` is a GET/error payload beyond the caller cap or an otherwise admitted control line beyond 64 bytes. `Protocol` is malformed, unexpected, partially truncated, or trailing framing/control data. `Closed` is EOF before any reply byte or later use of a retired client. | Move because `Server` owns a string. There is no message synthesis, logging, retry, reconnect, redirect handling, or second cleanup error. The operation's first error wins. A complete bounded `Server` response and a complete `Decode` leave the synchronized client reusable; `Invalid` occurs before I/O. `Io`, `ResponseTooLarge`, `Protocol`, and first-observation `Closed` retire it. | `Server` alone owns an allocation; moving the error transfers it and Drop frees it normally. No reply view or scratch buffer escapes. Other variants allocate nothing. | Ordinary package sum identity; `Io` reuses the always-available `core.Error` identity without changing its tags. | Variant/payload/order/interface owners; every producer x reuse/close state; owned-error escape/Drop; whole/per-unit and malformed-HIR owners. |
| `pkg.kv.connect(host: str, port: i64, options: ClientOptions) -> Result<client, Error>` | Arguments evaluate once, left-to-right. Validate nonempty host with no U+0000, then `port` in `1..=65535`, then option fields in source order, all before side effects. Host is otherwise passed byte-exact UTF-8 to the system resolver; there is no URL, default host/port, database number, credential, environment, or config file. | Success establishes one TCP connection, strictly installs both retained socket I/O timeouts, constructs its reader then writer shell, and returns a live client without sending PING, AUTH, SELECT, HELLO, or any Redis bytes. Native connect/configuration failure is `Error.Io(core.Error)`. `connect_timeout_ns` bounds each resolved address's socket-connect attempt; DNS and the aggregate address list are not an end-to-end deadline. Impure. | Host is borrowed only through resolution and copied transiently once into the runtime's NUL-terminated resolver input. Success retains exactly four allocations: connection, reader shell, writer shell, and package state. Every failed candidate socket and resolver allocation is cleaned by the runtime; timeout-configuration failure closes the new socket before wrapper/state construction and publishes no client. Wrapper/state OOM follows the hard-abort policy. | Ordinary package source uses exact compatible externs for shipped `align_rt_tcp_connect`/free and the planned unkeyed `align_rt_tcp_conn_set_io_timeout`. No new ABI shape, checked-HIR operation, or compiler recognition. | Shipped TCP/resource plus the planned checked timeout row; validation/no-side-effect, resolver/address ordering, per-attempt timeout, IPv4/IPv6 loopback, native status, strict timeout installation, construction order, cleanup, effect, and whole/per-unit owners. |
| `pkg.kv.get(borrow mut owner: client, key: str) -> Result<Option<string>, Error>` | Receiver then key evaluate once. Validate live state, then key length `0..=536870912`, then checked canonical RESP request length before allocation/I/O. Empty UTF-8 keys and embedded NUL/CR/LF are valid because the request uses a bulk string. | Sends exact uppercase two-element RESP2 `GET`. A bulk reply returns one owned `Some(string)`; null bulk `$-1` returns `None`; zero-length bulk returns `Some("")`. A complete non-UTF-8 bulk returns `Decode` after consuming it and keeps the client live. Any valid bounded `-` reply returns `Server`; a complete non-UTF-8 error likewise returns reusable `Decode`. Other type/length/framing, partial EOF, or bytes trailing the completed reply in the current read are `Protocol`; EOF before a byte is `Closed`; an over-cap declared bulk is `ResponseTooLarge` without drain. Impure. | Key is borrowed only through synchronous writes and never retained. Success publishes one ordinary owned result string, with no result allocation for `None`; the retained reader/writer shells survive synchronized success, while receive chunks, line state, conversion storage, and unpublished output are operation-owned and dropped on every exit. No returned value borrows client/key/scratch. | Package source owns RESP assembly/parser state over the existing TCP-derived writer, reader, buffer, and UTF-8 rows plus exact compatible externs. The generic writer prerequisite suppresses SIGPIPE for every connection-derived writer; there is no package-specific write row or runtime parser. | Official RESP2/GET semantics; independent wire vectors, fragmentation/coalescing, null/empty/exact/next bound, UTF-8/NUL/CRLF, ownership/Drop, safe-write, error/reuse, and loopback owners. |
| `pkg.kv.set(borrow mut owner: client, key: str, value: str, options: SetOptions) -> Result<bool, Error>` | Receiver, key, value, options evaluate once, left-to-right. Validate live state; key then value lengths independently in `0..=536870912`; condition; expiry; then every request-length/decimal calculation before allocation/I/O. Empty and embedded-NUL/CR/LF key/value bytes are valid. | Emits one canonical RESP2 `SET`: `SET key value`, optional `NX`/`XX`, then optional `PX <ceil-ms>`, in that order. Exact `+OK` returns `true`. Null bulk `$-1` returns `false` only for `IfAbsent`/`IfPresent`; it is `Protocol` for `Always`. A valid bounded UTF-8 `-` reply returns `Server`; a complete non-UTF-8 error is reusable `Decode`. Every other success spelling/type/integer/bulk/framing or current-read trailing byte is `Protocol`. Impure. | Inputs are borrowed only during the call. Request framing uses bounded operation-owned decimal/header storage and writes key/value directly without retaining or cloning them. The bool result allocates nothing. The retained writer/reader shells survive synchronized success; all operation scratch drops before return. | Ordinary package source uses the hardened existing connection-derived writer plus existing read/buffer rows. Redis owns atomic SET condition/expiry behavior and its server clock; the package reads no clock. | Official SET semantics; three conditions x two expiry states, exact ns/ms edges, persistence/expiry behavior, collision/non-resurrection use, byte goldens, partial-write/response failure, and ownership/effect owners. |
| `pkg.kv.delete(borrow mut owner: client, key: str) -> Result<bool, Error>` | Receiver then key evaluate once. Validate live state, key length `0..=536870912`, and canonical request arithmetic before I/O. Key byte admission matches `get`. | Sends exact uppercase one-key RESP2 `DEL`. Any valid RESP signed-i64 integer spelling whose value is zero (`0`, optional sign, or leading zeros) returns `false`; any whose value is one returns `true`. Every other value/overflow or reply type is `Protocol`. A valid bounded UTF-8 `-` reply returns `Server`; a complete non-UTF-8 error is reusable `Decode`. Impure. | Key is call-bounded and retained nowhere. The bool result and normal request framing require no value-sized allocation. Retained writer/reader shells survive synchronized success; all operation scratch drops before return. | Same hardened existing writer/read boundary; no package-specific write row or multi-key overload. | Official DEL semantics; zero/one optional-sign/leading-zero spellings, negative/two/overflow/type mutations, error, fragmentation, ownership, effect, and reuse owners. |

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

- `connect_timeout_ns` bounds each socket connect attempt after synchronous DNS resolution. It does
  not bound DNS or sum across multiple resolved addresses.
- `io_timeout_ns` is installed through a checked package-internal TCP row onto both blocking socket
  receive/send timeouts. It bounds one blocking
  read/write wait for progress, not the total duration of a multi-read command. A timeout returns
  `Error.Io(core.Error.Timeout)` and closes the client because a request may be partially sent or a
  response partially consumed.

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
- every command: `-<0..=max_response_bytes payload bytes>\r\n` with a UTF-8 payload.

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
2. A recognized `-` frame is bounded while reading. Crossing the inclusive cap returns
   `ResponseTooLarge` and closes without drain. A complete non-UTF-8 error payload returns `Decode`;
   otherwise clone it into `Server`. Both complete synchronized outcomes leave the connection live.
3. Reject a reply marker not admitted for the current command, then validate its canonical line
   grammar and semantic value. Any failure is `Protocol` and closes.
4. For GET, validate every observed length byte before comparing the valid digit magnitude; reject
   a magnitude over the cap as `ResponseTooLarge` and close without drain. Read exactly the payload
   and terminal CRLF. A complete non-UTF-8 payload returns `Decode` while
   keeping the client live; otherwise publish its owned clone.
5. Before any success, `Server`, or `Decode` publication, reject bytes trailing the complete frame
   in the same native read as `Protocol` and close. Later unsolicited bytes cannot be distinguished
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
allocation. Request headers and decimal text are bounded operation storage. Key/value bytes are written from
their call-bounded `str` views and retained nowhere. Receive chunks and framing state are bounded by
the response cap plus fixed protocol overhead. A successful GET or Server error allocates one
ordinary owned string result after the complete frame is synchronized; peak storage may contain
both the N-byte receive buffer and its N-byte final owned copy because V1 adds no consuming
buffer-to-string freeze. Every native receive buffer compares its actual `buffer.capacity()` with
the requested positive capacity before the first read; a mismatch hard-aborts under the OOM policy
instead of masquerading as EOF.
Intermediate raw/source buffers are unpublished and dropped first on every error. OOM follows the
language's existing hard-abort contract. Beyond the exact four allocations retained by a live
client, no per-command scratch/result allocation-count, zero-copy receive, zeroization, throughput,
or latency promise is made.

## Package, runtime, artifact, and cache boundary

The vendorable subtree owns root `pkg.kv` plus `pkg.kv.internal.resource`. Its internal source uses
exact type-compatible extern declarations for the already keyed TCP connect/free/reader/writer,
I/O read/write/free, buffer new/bytes/capacity/free rows, and one planned source-reachable unkeyed
row:

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

`TcpConnSetIoTimeout` requires a non-null live connection and timeout
`1..=86400000000000`. It installs `SO_RCVTIMEO` first and returns its fixed errno-mapped status
without attempting `SO_SNDTIMEO` if that call fails. Otherwise it installs `SO_SNDTIMEO` and returns
that status, or zero only after both succeed. A second-option failure leaves the first installed,
but the package immediately closes the unpublished connection. Configuration cannot overlap
another operation, and no rollback is required. The row allocates, retains, and closes nothing. It
is a mandatory base
export, source-reachable compatible extern, and collision-reserved unkeyed identity. It reuses an
existing ABI shape, so activation changes the exact base/maximum counts from 347/355 to 348/356
while the keyed count remains 330, and A123 remains the next unreserved shape.

The existing `TcpConnWriter`/`IoWriterWrite`/`IoWriterFree` identities, declarations, attributes,
and counts do not change. The private runtime `Writer` gains a socket sink kind and macOS/BSD ready
bit set only by `align_rt_tcp_conn_writer`. Nonempty writes from that kind use the SIGPIPE-safe send
policy above and cache only a successful `SO_NOSIGPIPE`; all other constructors retain their
byte-identical fd path. The option is a monotone per-socket setting: overlapping shells may each
attempt it, each sends only after its own success, a failed shell remains retryable, and no shell
Drop restores or clears it. Connection-derived writers remain unbuffered and non-owning, so their
free path performs no hidden write and never closes the socket; connection close discards the
option with the fd.

Source extern compatibility reuses every registry row's exact LLVM types, attributes, symbols, and
runtime definitions; it does not declare a second physical symbol or bypass collision checking. The
package adds no HIR/MIR variant, compiler-recognized function spelling, reflection table, static
artifact, schema input, or environment option. `docs/impl/19-hir-validation-ledger.md` remains
unchanged; `docs/impl/20-runtime-abi-ledger.md` reserves the exact one-row inactive delta and pins
the existing writer hardening without changing its ABI.

Whole-program compilation sees the ordinary package bodies. Per-unit compilation serializes the
resource, `ClientOptions`, `SetCondition`, `SetOptions`, `Error`, and four public signatures, while
the producer object retains the resource Drop thunk and existing native dependencies. Current
capability collection is module-wide, so use of any operation retains the complete root/internal
TCP, I/O, and buffer set; no call-spelling selector changes that set. Package source/interface and
dependency implementation hashes determine normal object/link/cache identity. No endpoint,
resolver result, response, clock, source file, or runtime inspection enters artifact identity.

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

The generic TCP-writer hardening is a distinct prerequisite capability with an already-shipped
`std.net` consumer and a closed signal-safety failure domain, so it lands as one independently
useful PR before the package. It changes no public signature or ABI identity. The remaining
client/resource/parser/three commands form one strict producer-to-consumer chain. A parser-only
or connection-only PR would leave no stable public consumer, while splitting commands would repeat
the same synchronization, poisoning, fake-server, capability, and Drop proof. The capability may
exceed roughly 1,000 changed hand-written lines once the adversarial owner matrix is included; one
boundary has lower integration risk because every reply kind closes against the same state machine
before publication.

| Axis | Required closure | Owner evidence |
|---|---|---|
| Public formation and identity | Exact module/resource/record/sum definitions, field and discriminator order, four signatures, qualification, visibility, direct/imported calls, generic consumer-wrapper monomorphization, current function-value rejection, whole/per-unit interface parity. | Public-source extraction, positive consumer compile/run, near-spelling/type/arity negatives, monomorphic/generic-wrapper parity, interface round-trip and generic alias controls. |
| Connect and option admission | Host/U+0000, port, three ordered option bounds, exact/next boundaries, no side effect before complete validation, DNS plus per-address timeout meaning, timeout application, every native status, reader-then-writer-then-state construction, IPv4/IPv6, no Redis bytes. | Instrumented connect/runtime counters and loopback listeners; resolver, refused, timeout, malformed, retained-allocation, and allocation/cleanup failpoints. |
| Request bytes | Exact GET/SET-product/DEL arrays, uppercase tokens, canonical decimal, 512-MiB admission without value-sized request copy, embedded NUL/CR/LF, all arithmetic before first write. | Independent semantic-to-byte goldens, boundary/mutation table, fragmented writer and partial-write owner. |
| Reply framing | Every marker, official bulk/integer grammar including leading zeros and integer signs, 64-byte control cap, CRLF, fragmented/coalesced reads, null/empty/exact/next cap, error cap, trailing bytes, EOF at every ordinal, no quadratic scan. | Deterministic scripted TCP peer with one-byte/all-split/multi-part products, independent byte-to-semantic goldens, comparison counter. |
| GET semantics | Bulk/null/empty ownership, exact bytes, UTF-8 decode after full consumption, Decode reuse, wrong kinds and oversized declaration close. | Official vectors plus valid/invalid UTF-8, lifetime escape, subsequent-command, allocation/Drop, and no-drain probes. |
| SET semantics | Always/NX/XX x None/Some, condition-before-PX wire order, ceil-ns conversion, persistent TTL removal, +OK/null result matrix, server errors, unexpected status. | Scripted bytes plus a Redis-compatible state model for collision, expiry, refresh-without-resurrection, exact/next duration, and wrong-reply products. |
| DEL semantics | One-key request, every official signed/leading-zero spelling of values 0/1, server error, every other value/type. | False/true and sign/leading-zero/negative/two/overflow/type mutation matrix with reuse/close checks. |
| Error and poison state | Invalid before I/O; bounded UTF-8 Server and full-bulk Decode reusable; Io/too-large/protocol/truncation/partial-write close; first error retained; every later call Closed with zero I/O. | Error-producer x command x before/during/after-frame x reuse table, native call counters, first-error/cleanup-failure probes. |
| Ownership and cleanup | Resource formation, move-in/out/return/replacement, if/match/else/?/map_err/branch/loop/early return, source nulling, state/socket/wrapper/scratch/result Drop once, malformed state no untrusted call. | Resource/drop counters, allocation parity, parameterized control-flow owner, state semantic-to-byte and byte-to-semantic goldens, malformed field product. |
| ABI, effects, capabilities, and cache | Exact compatible reuse of existing rows plus atomic activation of `TcpConnSetIoTimeout`; strict timeout status; existing connection-writer sink provenance and partial/EINTR/zero/EPIPE/timeout mapping; Linux/macOS SIGPIPE safety without changing writer ABI/counts; no new ABI shape or HIR row; Impure operations; module-wide native retention; package absence; source/interface/dependency edit invalidation. | Exact registry/golden/base-export/type/collision/source-reuse owners; file/std/socket writer-kind parity and subprocess closed-peer signal owner on Linux/macOS; package whole/per-unit IR/link runs, effect checks, no-package negative, add/remove/edit/revert cache twins. |

## Sources of truth and author consistency pass

This English ledger, `docs/impl/pkg-design/ja/kv.md`, `draft.md`,
`docs/language-spec.md`, `docs/design-notes.md`, `docs/history.md`,
`docs/open-questions.md`, `docs/impl/07-roadmap.md`, `docs/impl/std-design/net.md` and its Japanese
mirror, `docs/impl/20-runtime-abi-ledger.md`, and `HANDOFF.md` must agree. The HIR ledger remains
unchanged; any ABI change beyond the exact one-row reservation or any public writer-surface change
reopens this design.

During candidate review, `docs/open-questions.md` keeps this item under Open and
`docs/history.md` has no settled entry. Acceptance must move the exact reviewed contract to
Settled, add its history record, change every candidate status to accepted/inactive as applicable,
and only then authorize implementation.

Author-side pass completed on 2026-09-02 before independent review:

- every public argument/result has one exact type, evaluation order, default, ownership, lifetime,
  allocation, cleanup, error, and effect rule;
- command, condition, expiry, response marker, verification state, option state, field presence,
  row order, discriminator, and unavailable-result products are exhaustive;
- host/key/value/error text fixes UTF-8, embedded-NUL, CR/LF, boundary validation, and
  pre-side-effect semantics;
- multi-invalid calls have deterministic state/host/port/option/key/value/condition/expiry/wire and
  native/reply/error precedence;
- no endpoint, credential, database, retry, clock, resolver result, configuration, artifact, or
  source input is ambient;
- canonical RESP scalars, tags, sequence order, malformed rejection, and independent
  semantic-to-byte plus byte-to-semantic goldens are fixed;
- the resource record and RESP state machine fix every state/tag/reserved/pointer/length product,
  overlap exclusion, failed-second-operation behavior, error preservation, and Drop order;
- exact existing producer-owned runtime rows supply native state without reflection or artifact I/O;
- examples use accepted syntax and separate declarations from positional calls; and
- acceptance owners cover every ledger invariant, with no unpromised benchmark used as a gate.

Official protocol/command references: Redis
[RESP](https://redis.io/docs/latest/develop/reference/protocol-spec/),
[GET](https://redis.io/docs/latest/commands/get/),
[SET](https://redis.io/docs/latest/commands/set/), and
[DEL](https://redis.io/docs/latest/commands/del/).

## Design-review finding-to-fix ledger

No independent review has run. Findings must change this ledger first, then propagate through every
source of truth in one pass.
