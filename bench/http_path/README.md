# `http_path` — pricing the server's own request path

What one request costs **inside** the server: allocations and the server thread's CPU time, against
two in-process floors.

```sh
bench/http_path/run.sh              # 100000 requests per arm in 6 interleaved blocks
bench/http_path/run.sh 200000 10    # requests-per-arm, blocks (blocks must be even)
```

```
6 interleaved blocks x 16666 requests per arm (after 2000 warm-up)
  arm            allocs/req   fresh B/req   growth B/req   CPU ns/req   block spread
  floor (plain)        0.00           0.0            0.0        31253           418
  floor (+poll)        0.00           0.0            0.0        32016           274
  align               14.00         585.0          135.0        35488           247

  the keep-alive `poll` costs:                    763 ns/req
  Align above the plain floor (web_e2e's figure):  4234 ns/req
  Align's CPU work above the poll floor:          3471 ns/req, 14.00 allocations
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
- **A watchdog, and deliberately no client read timeout.** If a client dies, its server arm blocks in
  `poll`/`accept` forever and the client's own assertions are never observed — so 20 s without
  progress aborts the process. The clients themselves must NOT have a read timeout: while one arm
  runs a block the other two clients sit in `read` for that whole block, which at large block sizes
  exceeds any timeout worth setting (a 20 s one aborted `run.sh 500000 4`). `SERVED` advances during
  *any* arm's block, so the watchdog fires only on a true global stall — the right condition.

## Precision — what to trust

**Interleaved blocks and a median, not one long pass.** The box drifts *between* runs by more than
the harness's own σ — an unchanged binary read mean 4201 ns in one batch and 4682 ten minutes later.
**More iterations do not fix that**; alternating the arms inside one process does, because the drift
then hits all of them alike.

**And the arm ORDER is counterbalanced, because the slot itself is biased.** Running three
*identical* plain floors in the three slots showed slot 2 reading ~115 ns above slot 1 and slot 3
~109 ns below slot 2 — systematically, not shrinking with more blocks. That is twice the headline's
own σ and half the size of the effects this harness exists to price; uncorrected it inflated the
`poll` line by 11% and deflated the headline by 3%. The order is therefore reversed on odd blocks
(hence `blocks` must be even), and the `poll` figure moved from ~1022 to ~858 ns — agreeing with an
independent two-arm measurement of 913 ns that never had the bias.

Observed across six consecutive runs of the current harness:

| metric | values (ns/req) | σ |
|---|---|---|
| Align above the **poll** floor | 3542 3664 3583 3533 3482 3639 | **68 ns (1.9%)** |
| Align above the plain floor | 4626 4335 4371 4378 4315 4568 | 126 ns (2.8%) |
| the `poll` itself | 1084 671 788 845 832 929 | 130 ns (a syscall's cost is the noisiest thing here) |

So: **quote the poll-floor difference**, expect ~2% run to run, and take any A/B **adjacently**. An
early 3-sample reading suggested ±1.3%; with 11 samples the true figure was 3.5%, which is why every
spread here comes with its sample size.

**Median, not min — deliberately differing from `bench/README.md`'s primary harness.** That rule
("keep the per-kernel minimum") is for a single kernel's own time, where interference is one-sided
noise on one quantity. Here the reported number is a *difference of two independently minimized
arms*, so taking each arm's min picks two uncorrelated low outliers and adds their noise — and throws
away exactly the correlated block-to-block drift the interleaving exists to cancel. Measured over the
same per-block data: median gives σ 88 / 173 / 99 ns on the three lines where min gives 146 / 214 /
127.

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
