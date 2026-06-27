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

## Result (2026-06-27, native) — after the runtime itoa fix

```
        n    align ms    naive ms      opt ms   vs naive     vs opt
     1000       0.038       0.042       0.026      1.11x      0.67x
    10000       0.264       0.281       0.198      1.06x      0.75x
   100000       2.730       2.862       2.000      1.05x      0.73x
```

- **Beats idiomatic naive Rust (1.05–1.11×)** — the realistic baseline (`String` + `to_string`).
- **≈0.73× (≈1.35× slower) vs hand-optimized Rust** (exact `with_capacity` + manual itoa).

Hand-rolling the runtime integer write (`align_rt_builder_write_int`: a generic `write!(buf, "{v}")`
→ a back-to-front itoa straight into the buffer) **halved the gap to optimized Rust** — Gemini's M2
Part 1 measured the old builder at ~2.8× slower than optimized; it's now ~1.35×, and it overtook naive.

### Remaining gap → builder capacity (Gap C, recorded)
The last ~1.35× vs optimized Rust is the builder's `Vec<u8>` **reallocating** as it grows, where
optimized Rust pre-sizes with `with_capacity`. Closing it needs a `builder(capacity)` hint (surface
+ a runtime `builder_with_capacity`) — the next lever, now with a measured target (≈1.35× → parity).
Float writing still uses the generic formatter (`ryu` would be the float analogue of this itoa).
