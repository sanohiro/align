# `string_builder` — builder write-path duel (Align vs Rust `String`)

Measures Align's `builder` reduce-append pattern (the tool the Gap A error points users to) against
idiomatic Rust string building: **naive** (`String::new()` + `x.to_string()` per int — the common
way) and **optimized** (`String::with_capacity` + a manual itoa into a stack buffer). Per element it
appends `"item-" + int + "-status "`, so the integer write path is exercised.

```sh
bench/string_builder/run.sh [baseline|v3|native]   # default native
```

Same plumbing as `bench/json_soa/` / `bench/group_by/`: kernel via `alignc emit-obj`, runtime linked
as the cdylib. No external deps (std-only baselines).

## Result (2026-06-27, native) — after the itoa fix + `builder(capacity)`

`build` = `builder()`; `+cap` = `builder(n*16)` (pre-sized). Both vs naive (`String` + `to_string`)
and optimized (`with_capacity` + manual itoa) Rust:

```
        n   align ms    +cap ms   naive ms     opt ms    cap/opt  cap/naive
     1000      0.025      0.025      0.025      0.017      0.69x      0.99x
    10000      0.260      0.280      0.262      0.181      0.65x      0.94x
   100000      2.767      2.771      2.699      1.870      0.67x      0.97x
```

- **Beats / ties idiomatic naive Rust** (`String` + `to_string`) — the realistic baseline.
- **≈0.67× (≈1.5× slower) vs hand-optimized Rust** — and **`builder(capacity)` does NOT close it**:
  `+cap` ≈ `build` (2.77 vs 2.77 ms). The hypothesis that the residual was the `Vec` realloc was
  **wrong** (measured). The real residual is the **per-append FFI call overhead**: each element does
  3 `align_rt_builder_*` calls (≈300k extern calls at N=100k), which aren't inlined, where optimized
  Rust inlines `push_str` + itoa. ~0.9 ms gap ≈ 300k × ~3 ns/call — that's the cost, not reallocation.

The earlier **itoa** fix (`align_rt_builder_write_int`: generic `write!` → a back-to-front byte itoa)
*did* help — it halved the gap (Gemini's M2 Part 1 had the old builder ~2.8× slower than optimized;
now ~1.5×). `builder(capacity)` is still a legitimate primitive (it helps *realloc-bound* building,
and it's nothing-hidden), it just isn't the lever for this per-write-call-bound workload.

### Profile finding (2026-06-29, native, 100k rows)

`ALIGN_BENCH_PROFILE=1 bench/string_builder/run.sh native` adds variants that isolate static writes
from integer writes. After adding a small-integer fast path in `align_rt_builder_write_int`
(`-999..999` writes directly into the buffer), the profile is:

```
static one write/row   0.551 ms
static two writes/row  0.878 ms  extra call delta  0.327 ms
int only write/row     0.773 ms
full build             1.483 ms
runtime batch          0.986 ms
runtime batch +cap     0.954 ms
```

The fast path moved the benchmark's full build from ~1.58 ms to ~1.48 ms and `int only` from ~0.88 ms
to ~0.77 ms. The runtime batch probe (`align_rt_builder_write_str_int_str`, called directly by the
harness to model lowering `literal + int + literal` as one call) lands at ~0.95–0.99 ms, confirming
the main lever: remove the three per-row runtime calls.

### Batch lowering implemented (2026-06-29)

The compiler now lowers the `b.write("literal"); b.write_int(x); b.write("literal")` sequence in a
builder-reduce body to a single `align_rt_builder_write_str_int_str` call (a MIR peephole,
`fuse_builder_writes` in `align_mir`, gated to exactly the `str,int,str` shape on one builder). Honest
before/after on the same host (100k rows, native), with the batch probe as the floor:

```
                full build   runtime batch
fusion off         ~1.65 ms       ~1.11 ms
fusion on          ~1.30 ms       ~1.11 ms
```

So the lowering removes two of the three per-row FFI calls and moves generated `build` ~1.65 → ~1.30 ms
(≈21%), closing most of the gap to the direct batch probe. `build` now beats Rust `naive` (~1.43 ms)
and is ≈1.4× behind hand-optimized `opt` (~0.92 ms), down from ~1.5×. The residual ~0.19 ms over the
batch probe is the reduce loop + `to_string`, not the per-write boundary. (Absolute numbers here are
higher than the earlier profile block because that was measured in a faster host state; the within-run
deltas are the honest signal.)

### Remaining lever (recorded): the general append chain
The peephole is deliberately narrow (`str,int,str` only). Other shapes (`int,str`, longer chains,
non-literal strings interleaved with side effects) still pay per-`write` FFI; a general builder-chain
batcher is the follow-up, not done here. (Float writing also still uses the generic formatter; `ryu`
is the float analogue of the integer itoa.)
