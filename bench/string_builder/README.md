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

### Remaining lever (recorded): inline / batch the builder appends
Closing the last ~1.5× needs removing the per-`write` FFI boundary — inlining the common append, or a
batched write API — a codegen/runtime concern, not capacity. (Float writing also still uses the
generic formatter; `ryu` is the float analogue of the integer itoa.)
