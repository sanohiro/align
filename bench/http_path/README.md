# `http_path` — pricing the server's own request path

What one request costs **inside** the server: allocations and the server thread's CPU time, against
two in-process floors.

```sh
bench/http_path/run.sh              # 100000 requests per arm in 5 interleaved blocks
bench/http_path/run.sh 200000 10    # requests-per-arm, blocks
```

```
5 interleaved blocks x 20000 requests per arm (after 2000 warm-up)
  arm            allocs/req   fresh B/req   growth B/req   CPU ns/req   block spread
  floor (plain)        0.00           0.0            0.0        30959           444
  floor (+poll)        0.00           0.0            0.0        32053           523
  align               14.00         585.0          135.0        35675           662

  the keep-alive `poll` costs:                    1094 ns/req
  Align above the plain floor (web_e2e's figure):  4716 ns/req
  Align's CPU work above the poll floor:          3621 ns/req, 14.00 allocations
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
not stable to 1%, so the roadmap's "price each allocation with `bench/web_e2e` at `CONNS=1`" does not
work. That is what this harness is for.

## Two floors, because Align does one syscall more

Align's path is `poll({parked, listener})` → `read` → `send`. A plain read/write floor does one fewer
syscall, and on this box **that `poll` costs ~0.8–1.1 µs — about 21% of the naive difference.**
Charging it to "Align's CPU work" would overstate what removing allocations can ever reach, so both
floors run:

- **plain floor** — read, write, keep the connection. Its difference is the number comparable with
  `web_e2e`'s.
- **poll floor** — the same, plus the identical `poll({conn, listener})` before the read. Its
  difference is the honest budget for CPU-side work: **~3.6 µs, of which 14 allocations.**

Both floors write the response Align produces, byte-for-byte, and the client asserts that on its
first response **in every arm** — which is what keeps the arms comparable rather than merely similar.

## What it measures, and why each choice

- **Allocations per request — exact, integral, zero noise.** A counting `#[global_allocator]`, armed
  only inside a measured block. `align_runtime` is a dependency as a **Rust lib**, not through its C
  ABI, precisely so its internal `Vec`/`String` traffic is visible. A `Vec` growing counts as its own
  event — that is exactly the cost a right-sized or reused buffer removes.
  - **Scope, and a trap:** this sees the runtime's **Rust** allocations only. Align-language
    allocation (`align_rt_alloc`) calls libc `malloc` directly, bypassing Rust's global allocator, so
    it is invisible here. Fine while the measured handler is this harness's pure-Rust `serve_one`;
    anyone extending it to a real Align handler or to `pkg.web` will get a silently low count.
  - **Bytes are reported as two columns, not one.** `fresh` is what fresh allocations requested;
    `growth` is what `realloc` added. Pre-reserving a buffer moves bytes *between* them (a measured
    14 → 10 allocation win read 770 → 882 on a single summed figure), so a single number would show
    an improvement as a regression.
- **Server CPU ns per request, from `CLOCK_THREAD_CPUTIME_ID`.** Wall time is the wrong clock:
  `accept` blocks in `poll` until the client's next request, so a wall-clock loop measures the ~65 µs
  loopback round-trip and buries the server's ~4 µs inside it. Thread CPU time accrues only while the
  thread is on-CPU. Verified independently: the wall-clock per-request delta reproduces the same
  figure (67.79 − 63.52 = 4.27 µs wall vs 4.27 µs CPU), so the clock is not inventing the difference
  — it is about 2× less noisy.
- **The floors' absolute ~31 µs is not work.** It is syscall plus WSL2 accounting overhead, present
  in every arm, meaningful only in the difference.
- **`bench/web_e2e`'s exact request bytes** (`Host: h`, one header). A 3-header variant measured
  ~340 ns higher — with the allocation count unchanged at 14 — so the request has to match for the
  two harnesses' numbers to be comparable at all.
- **The default release profile, deliberately.** The compiler links the shipped `libalign_runtime.a`
  built with the workspace's default release settings and calls it across a real C-ABI boundary.
  `lto = true` here let the harness inline `align_rt_http_rb_header` into its caller and const-fold
  the header validation, reading ~2.8% faster than what ships.
- **No channel between the client and server halves.** The socket already sequences them, and a
  channel would allocate on the client thread — which the counting allocator would then charge to the
  request path. Removing it is what makes allocations/request exactly integral.
- **A watchdog.** If a client dies, its server arm blocks in `poll`/`accept` forever and the client's
  own assertions are never observed. 20 s without progress aborts the process instead of hanging.

## Precision — what to trust

**Interleaved blocks and a median, not one long pass.** Within a run the harness is tight (an A/A
floor-vs-floor check reads ±25 ns), but the box drifts *between* runs by more than the harness's own
σ — an unchanged binary read mean 4201 in one batch and 4682 ten minutes later. **More iterations do
not fix that**; alternating the arms inside one process does, because the drift then hits all of them
alike. This is `bench/README.md`'s balanced-order rule applied.

Observed across five consecutive runs of the current harness:

| metric | values (ns/req) | σ |
|---|---|---|
| Align above the **poll** floor | 3621 3630 3712 3565 3499 | ~78 ns (2.2%) |
| Align above the plain floor | 4716 4416 4475 4737 4561 | ~130 ns (2.9%) |
| the `poll` itself | 1094 786 763 1172 1063 | ~170 ns (large — a syscall's cost is the noisiest thing here) |

So: **quote the poll-floor difference**, expect ~2% run to run, and take any A/B **adjacently**. A
single earlier 3-sample reading suggested ±1.3%; with 11 samples the true figure was 3.5%, which is
why the claim now comes with its sample size.

The allocation count carries none of this: it is 14.00 in every run, every variant, invariant to
request shape and profile.

## Does it converge?

Yes, and this was checked before the harness was used for anything. Pre-reserving the response
serialize buffer (`Vec::new()` → `with_capacity`) removes exactly the 4 `realloc` events the counter
attributes to it: the count reads 14 → 10 and the time **−971 / −690 / −1089 / −541 ns across four
adjacent A/B pairs, 4/4 the same sign** — ≈206 ns per realloc event.

## Caveats

- Linux-specific (`CLOCK_THREAD_CPUTIME_ID`, raw `clock_gettime`/`poll`). WSL2 here.
- One connection, one request in flight, keep-alive. It prices the *path*, not concurrency; use
  `web_e2e` at 32 connections for throughput questions.
- The floors are not competitors — they are this harness's zeros.
- The response built is `bench/web_e2e`'s `/plaintext` route: status 200, one `Content-Type` header,
  a 13-byte body. A route with more headers allocates more; this is the cheap end.
