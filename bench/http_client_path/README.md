# `http_client_path` — pricing `http.get`'s own client path

What one `http.get` costs **inside the client**: allocations and the calling thread's CPU time,
against an in-process floor that moves the same bytes through the same syscalls.

The client-side twin of [`bench/http_path`](../http_path/README.md). Read that README first — the
statistics, the counting allocator, the `CLOCK_THREAD_CPUTIME_ID` argument and the counterbalancing
rule are all established there and are not re-derived here.

```sh
bench/http_client_path/run.sh              # 100000 requests per arm in 6 interleaved blocks
bench/http_client_path/run.sh 200000 10    # requests-per-arm, blocks (blocks must be even)
```

```
6 interleaved blocks x 16666 requests per arm (after 2000 warm-up)
  arm            allocs/req   fresh B/req   growth B/req   CPU ns/req   block spread
  floor                0.00           0.0            0.0        31624           310
  align                2.00         213.0            0.0        34742           799

  http.get's CPU work above the floor:  3118 ns/req, 2.00 allocations
```

## Why this exists — `bench/http_client` cannot price this

`bench/http_client` is the **R6 throughput gate**, and it is right for that: it answers "does the
keepalive pool beat reconnecting" (2.86×, floor 1.48×) and "is Align competitive with hand-rolled
Rust" (1.02×). But it reports **~65 µs/req end to end over loopback**, and the client path's
remaining items are ~0.5–1 µs each — under 2% of that, derived as a difference of two large numbers.
That is precisely the mistake `bench/http_path/README.md` records for `web_e2e`, and the roadmap
repeated it here: "`bench/http_client` exists to price it" was written before anyone checked whether
it could.

It cannot. This harness can: **σ ≈ 48 ns (1.1%)** on the reported difference across three consecutive
runs (4412 / 4404 / 4509), and an allocation count that is exactly integral in every run.

## One floor, not two

`http_path` needs two floors because Align's **server** does one syscall more than a plain read/write
loop — the keep-alive `poll({parked, listener})`, ~0.9 µs, which is not CPU work and would otherwise
be charged to Align.

The **client** has no such asymmetry: `http_socket_exchange` is `write_all` then a `read` loop, which
is exactly what the floor arm does. So one floor is the honest zero, and the whole reported
difference is CPU work — the request build, the response head parse, the owned `http_response`, and
the pool lookup.

## What it measures, and the choices that matter

- **Allocations per request — exact, integral, zero noise.** A counting `#[global_allocator]`, armed
  only inside a measured block, with `align_runtime` as a **Rust lib** dependency so its internal
  `Vec`/`String` traffic is visible. `fresh` and `growth` bytes are reported separately, because
  pre-reserving a buffer moves bytes *between* them and one summed figure would show an improvement
  as a regression.
- **The peer server must not allocate.** The counting allocator is global and the peer runs in this
  same process, so a per-request allocation there would be charged to the client path. The peer uses
  a fixed read buffer and a `const` response — and **the floor arm reading exactly `0.00`
  allocations/req is the assertion that proves it**, the mirror of the zero-check `http_path` applies
  to its client half. If that assertion ever fires, the numbers below it are meaningless.
- **Both arms send byte-identical requests**, asserted by the peer on each arm's first request. The
  `Host` header carries the ephemeral port, so the request cannot be a constant — and the *floor*
  arm deliberately sends the bytes built for the *Align* port. Its peer never parses the request, and
  byte-identical arms matter more than a `Host` naming the socket it arrived on.
  - This assertion earned its place immediately: the first version guessed
    `Host: 127.0.0.1\r\nConnection: keep-alive`, and the peer printed exactly what Align really
    sends (`Host: 127.0.0.1:{port}`, and **no** `Connection` header — Align relies on 1.1's
    persistent default, which is the leanest bytes).
- **The response is `bench/http_path`'s response**, byte for byte, so the two harnesses price the
  same message from the two ends.
- **The client is reused across every request**, so the pool is warm and the measured path is the
  keep-alive one. A fresh client per request would price `connect` — that is `bench/http_client`'s
  `align-nopool` arm, and a different question.
- **The default release profile, deliberately** (see `Cargo.toml`), and a watchdog on a global
  progress counter, since a dead peer leaves the measured thread blocked in `read` forever.

## What it found

The original instrument found **14 allocations and ~4.4 µs of CPU per `http.get`** — a bigger budget
than the server path's (~2.5 µs after #602–#604). The first allocation slice cut that to **7
allocations and ~3.4 µs**:

- `get` / `post` use static method strings and borrow the ABI URL/body through a request view instead
  of first building owned `String` / `Vec` fields. A full request builder exposes the same view, so
  serialization and exchange still have one path.
- The request serializer computes the exact wire size and reserves once; decimal `Content-Length`
  rendering no longer allocates a temporary `String`. Growth went from 56 to **0 B/request**.
- The socket exchange retains the already-parsed head, then moves its header spans and receive
  buffer directly into the response. It no longer parses the same head twice or copies the complete
  response bytes into a second `Vec`.

Three consecutive 100k-request runs measured **3362 / 3429 / 3489 ns** above the floor, versus the
instrument's original **4404 / 4412 / 4509 ns**: about **−1.0 µs (−22%)**, with the allocation count
exactly 7 in every block. The harness pins 7 as a ceiling; allocation regressions fail instead of
quietly becoming a new printed baseline. Further cuts now require representation or reuse work
(header spans, response handle and pool bookkeeping), not another obvious redundant ownership hop.

One pool-bookkeeping ownership hop was still redundant: after a completed exchange the request's
owned `(scheme, host, port)` key is dead, but returning the conn cloned its host `String` into the
idle map. Moving that key instead makes the common path **7 → 6 allocations** (fresh bytes 489 →
480); three 100k-request runs measured **3219 / 3418 / 3575 ns** above the floor, CPU-neutral at this
noise level. The harness now pins 6. The same cut makes the 200 KiB probe 10 → 9 allocations.

The pool also discarded its idle bucket on every take of the last conn, then rebuilt the same
`Vec<IdleConn>` on the corresponding put. Keeping that empty bucket while the request is in flight
reuses its 128-byte allocation. Every terminal path that does not put removes the bucket if it is
still empty; if another request refilled it concurrently, the conditional cleanup leaves that conn
untouched. The common path is now **6 → 5 allocations** and **480 → 352 fresh B/request**. Three
100k-request runs measured **3267 / 3181 / 3451 ns** above the floor, again CPU-neutral at this noise
level. A temporary exact-size histogram made the cut explicit: the prior six allocations were 9 B
(host key), 50 B (request bytes at the sampled port), 72 B (response handle), 93 B (response bytes),
and two 128 B blocks (header spans and the idle bucket); this removes the bucket block. The harness
pins 5; the 200 KiB probe is 9 → 8 allocations.

The remaining 128-byte block was the header-span `Vec`. The dominant response has exactly two
headers, so those spans now live inside the opaque response handle; a third header spills to a heap
Vec with insertion order, lookup, and the 128-header cap unchanged. The handle grows 72 → 120 B,
but replaces the old 72 B handle + 128 B span allocation: the common path is **5 → 4 allocations**
and **352 → 272 fresh B/request**. Three 100k-request runs measured **3113 / 3363 / 3146 ns** above
the floor, CPU-neutral at this noise level. The harness pins 4; the 200 KiB probe is 8 → 7.

The parsed host was the 9-byte block: authority splitting returned an owned `String` because the old
pool key was a `(scheme, String, port)` tuple. Splitting now returns a slice into the URL, and the pool
uses host as its outer `HashMap<String, ...>` key so lookup accepts that `&str`; `(scheme, port)` is
the inner key and keeps TLS/plaintext and distinct ports isolated. The host becomes owned only when a
new host bucket is first created, outside the warmed per-request path. The common path is now **4 →
3 allocations** and **272 → 263 fresh B/request**. Three 100k-request runs measured **3054 / 3468 /
3229 ns** above the floor, CPU-neutral at this noise level. The harness pins 3; 200 KiB is 7 → 6.

The request's 50-byte wire buffer was the next reusable allocation. Each client now lends a scratch
`Vec` to an in-flight serializer and takes it back through an RAII lease on every terminal path,
including validation, connect, TLS, socket, and retry failures. Concurrent `get_many` workers lease
distinct buffers; retention is bounded to eight buffers of at most 64 KiB, while larger requests are
still sent normally and simply drop their storage afterward. The common path is now **3 → 2
allocations** and **263 → 213 fresh B/request**. Three 100k-request runs measured **2995 / 3482 /
3118 ns** above the floor (median 3118 ns, CPU-neutral); the harness pins 2, and a disabled-reuse
mutation restores 3 and trips the gate. The 200 KiB response probe is 6 → 5 allocations.

The remaining two common-path allocations are the returned response itself: its 120-byte opaque
handle (including two inline header spans) and its 93-byte response buffer in this sample. Combining
them needs a custom single-allocation representation and a correspondingly careful ownership/free
boundary, rather than another local reuse cut.

### Negative result: unconditional read-into-the-response-buffer does NOT transfer from the server

The roadmap's first target here was to apply #602's server-side fix — `http_socket_exchange` reads
into a `let mut chunk = [0u8; 32 * 1024]` and then `extend_from_slice`s into the buffer that keeps
the response, the identical *source shape* #602 removed, where it was worth ~−640 ns and was larger
than all fourteen heap allocations together. **It was implemented, measured, and discarded.** Two
independent reasons, both worth keeping:

**1. The memset it targets is not there.** `objdump` of the shipped `libalign_runtime.a`:
`http_socket_exchange` is inlined into `http_client_perform`, which contains **zero `memset` calls**
(one stack probe remains, for the 32 KiB frame). LLVM elides the zero-init here — the array's only
use before being read is the transport call that fills it — where on the server, in a different
inlining context, #602 measured the `memset` present in the object. *The premise was a source-level
resemblance that the compiler had already dissolved.* Check the object, not the source.

**2. Reading into the buffer forces a size decision before the framing is known**, and on the client
that decision has no good answer. Adjacent A/B, `align_runtime` stashed and rebuilt between arms:

| response body | before | after | |
|---|---|---|---|
| 13 B | 4646 | 4194 | −452 ns |
| **8 KiB** | 4699 / 4816 / 4887 | 5931 / 6809 / 5815 | **+1200 ns, 3/3** |
| 200 KiB | 11689 | 6738 | −4951 ns (−42%) |

The 8 KiB regression is the whole story. A 2 KiB starting buffer (what the server uses, sized for a
request head) caps the *first* read at 2 KiB, so every response past 2 KiB costs an extra `read`
syscall that the 32 KiB stack chunk absorbed in one. Starting bigger is not available: **this buffer
IS the returned response body** — `truncate` never gives capacity back — so a 16 KiB start would
retain 16 KiB for a 96-byte response. The server does not face this: request heads are small, and
bodied requests are the rare case.

The 200 KiB win is real (per-byte copy elimination, matching #602's −16% server-side on the same
size) and is the gateway's actual shape, so this is worth revisiting **as a body-size-aware read
strategy**, not as a transplant of the server's.

**3. And it broke the pooling safety property**, which is recorded as its own test
(`http_client_does_not_pool_leftover_arriving_after_the_framing`): a response carrying bytes past its
`Content-Length` is a dirty conn, detected by having *read* the overshoot. A read sized to the framed
remainder cannot overshoot, so `buf.len() == t` becomes trivially true and the dirty conn is pooled —
misframing the next response on it. The test asserts on the **idle pool**, not the accept count,
because the pooled-then-failed path retries on a fresh connect and accepts twice either way.

### Positive follow-up: direct-read only the large body's middle

The body-size-aware version keeps the 32 KiB first read, so framing is known without adding a
syscall to 8 KiB responses. Once Content-Length is known, only a large body's **middle** reads
directly into the response `Vec`; capacity grows geometrically as bytes arrive, rather than trusting
an advertised 1 GiB length up front. It deliberately leaves the last 32767 bytes for the original
unclamped 32 KiB read. That final one-byte margin is what still observes an immediately available
overshoot and makes the connection unpoolable.

Runtime-only stash/rebuild A/B, with the benchmark change retained in both arms:

| response body | before | body-aware | result |
|---|---:|---:|---:|
| 13 B | 3562 | 3525 | −37 ns (unchanged) |
| 8 KiB | 3677 | 3509 | −168 ns (unchanged) |
| 200 KiB, pair 1 | 6694 | 3976 | −2718 ns |
| 200 KiB, pair 2 | 6330 | 3693 | −2637 ns |
| 200 KiB, pair 3 | 5227 | 3390 | −1837 ns |

The 200 KiB medians are **6330 → 3693 ns/req above the floor (−42%)**. In that runtime-only #608 A/B,
allocation events stayed 10 because both arms predated the pool-key, idle-bucket, inline-header, and
borrowed-host cuts; the current total is 6.
Geometric growth capped at the framed total reduces retained growth from **229376 → 172116
bytes/request**. The benchmark's 3-allocation regression ceiling now applies only to its default
13-byte response; `BODY=...` is intentionally a buffer-growth probe and must print those counts
rather than aborting before it can report them.

## Caveats

- Linux-specific (`CLOCK_THREAD_CPUTIME_ID`). WSL2 here.
- One connection, one request in flight, keep-alive. It prices the *path*, not concurrency;
  `bench/http_client` at N clients is the throughput instrument.
- Plaintext only. The TLS path shares `http_socket_exchange` but adds `SSL_read`/`SSL_write`, which
  this does not measure.
- The floor is not a competitor — it is this harness's zero.
- The absolute ~33 µs of both arms is loopback syscall plus WSL2 accounting overhead, present in
  every arm, meaningful only in the difference.
