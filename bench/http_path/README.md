# `http_path` — pricing the server's own request path

What one request costs **inside** the server: allocations, bytes, and the server thread's CPU time,
against an in-process floor that does `read` + `write` and nothing else.

```sh
bench/http_path/run.sh            # 200000 requests (the default; see "How many iterations")
bench/http_path/run.sh 20000      # quicker, noisier
```

```
requests measured per arm: 180000
  arm            allocs/req   bytes/req   CPU ns/req
  floor                0.00         0.0        31788
  align               14.00       770.0        36557

  Align's server-side cost above the floor: 4317 ns/req, 14.00 allocations, 770 bytes
```

## Why this exists — `web_e2e` cannot price an allocation

`bench/web_e2e` reports "Align's protocol path above the floor" as the difference of two ~70 µs
end-to-end measurements, so it inherits the noise of both. Three adjacent baseline runs on this box:

```
3.3 µs/req      3.9 µs/req      4.8 µs/req
```

**A 1.5 µs spread on a 4.0 µs signal** — larger than the entire allocation budget on that path. The
`CONNS=1` lesson recorded in `web_e2e/README.md` is still correct about what it measured (throughput
moves 18% run to run; req/s at one connection ~1%), but the *derived difference* it then quotes is
not stable to 1%, and the roadmap's "price each allocation with `bench/web_e2e` at `CONNS=1`" does
not work. That is what this harness is for.

Same quantity, measured here at 200k iterations: **4317 / 4240 / 4352 ns** — ±1.3%, about 14× the
sensitivity. And it agrees with `web_e2e`'s 4.0–4.1 µs, which is the cross-validation that makes both
believable.

## What it measures, and why each choice

- **Allocations and bytes per request — exact, zero noise.** A counting `#[global_allocator]`, armed
  only for the measured loop. `align_runtime` is a dependency as a **Rust lib**, not through its C
  ABI, precisely so its internal `Vec`/`String` traffic is visible; a cdylib boundary would hide it
  behind its own allocator. A `Vec` growing counts as its own event — that is exactly the cost a
  right-sized or reused buffer removes.
- **Server CPU ns per request, from `CLOCK_THREAD_CPUTIME_ID`.** Wall time is the wrong clock:
  `accept` blocks in `poll` until the client's next request, so a wall-clock loop measures the ~65 µs
  loopback round-trip and buries the server's few µs inside it (measured: 65 µs wall vs 4.3 µs of
  actual cost). Thread CPU time accrues only while the thread is on-CPU.
- **An in-process floor arm.** The absolute CPU number is ~32 µs/req for a server that only reads and
  writes — that is syscall plus WSL2 accounting overhead, not work. It is uninterpretable alone, and
  it is in *both* arms, so only the difference means anything. The floor writes the byte-for-byte
  response the Align path produces, so both arms move the same bytes through the same syscalls.
- **No channel between the client and server halves.** The socket already sequences them (the
  server's next accept blocks until the client sends), and a channel would allocate on the client
  thread — which the counting allocator would then charge to the request path. Removing it made
  allocations/request exactly integral.

## How many iterations

At 20k the delta spread is ~1 µs (4048–5080); at 200k it is ~110 ns. Use 200k for any comparison you
intend to act on, and take the arms **adjacently** — the box's own state drifts, which is how
`web_e2e`'s recorded speculative-read result nearly got mis-read.

## Caveats

- Linux-specific (`CLOCK_THREAD_CPUTIME_ID`, raw `clock_gettime`). WSL2 here.
- One connection, one request in flight, keep-alive. It prices the *path*, not concurrency; use
  `web_e2e` at 32 connections for throughput questions.
- The floor is not a competitor — it is this harness's zero.
- The response built is `bench/web_e2e`'s `/plaintext` route: status 200, one `Content-Type` header,
  a 13-byte body. A route with more headers allocates more; this is the cheap end.
