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
6 interleaved blocks x 6666 requests per arm (after 2000 warm-up)
  arm            allocs/req   fresh B/req   growth B/req   CPU ns/req   block spread
  floor                0.00           0.0            0.0        33048           950
  align               14.00         703.0           56.0        37459          1164

  http.get's CPU work above the floor:  4412 ns/req, 14.00 allocations
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

**14 allocations and ~4.4 µs of CPU per `http.get`** — a bigger budget than the server path's
(~2.5 µs after #602–#604). Known items in it, in the order they are understood:

- **`http_socket_exchange` reads into a `let mut chunk = [0u8; 32 * 1024]`** and then
  `extend_from_slice`s into the buffer that keeps the response — the identical shape #602 removed
  from the server, where it measured ~−640 ns and was *larger than all fourteen heap allocations
  together*. Its fix is also the same, including the trap #602's review caught: reserving a flat
  chunk before the final short read doubles the buffer, so the reserve must be bounded by what the
  framing still wants.
- **`Vec::new()` for the response buffer** — 56 bytes of `realloc` growth per request says it grows
  at least once even for a 96-byte response.
- The remaining allocations: the request `String`s (`method`, `url`), the parsed head's span `Vec`,
  the owned response.

## Caveats

- Linux-specific (`CLOCK_THREAD_CPUTIME_ID`). WSL2 here.
- One connection, one request in flight, keep-alive. It prices the *path*, not concurrency;
  `bench/http_client` at N clients is the throughput instrument.
- Plaintext only. The TLS path shares `http_socket_exchange` but adds `SSL_read`/`SSL_write`, which
  this does not measure.
- The floor is not a competitor — it is this harness's zero.
- The absolute ~33 µs of both arms is loopback syscall plus WSL2 accounting overhead, present in
  every arm, meaningful only in the difference.
