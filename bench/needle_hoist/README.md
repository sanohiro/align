# needle_hoist — repeated-needle plan hoisting leak gate

doc-13 §6.6 / §11 P3. The real leak / double-free assertion for the hoisted
`xs.where(fn s { s.contains(NEEDLE) }).…` plan (finding 5 of the branch review).

The default `cargo test` suite (`crates/align_driver/tests/needle_plan_hoist.rs`)
runs the hoisted pipeline and checks it does not crash and returns the right
count — enough to catch a **double free / use-after-free**, but a **pure leak**
neither crashes nor drifts the count. This harness closes the gap: it links the
compiled `kernel.align` against the runtime built with `--features alloc-count`
and reads `align_rt_str_finder_new_count` / `align_rt_str_finder_free_count`
around each call, asserting the plan is freed **exactly once** per invocation —
after a reps loop and after an early `?` error exit.

```
bench/needle_hoist/run.sh          # native
bench/needle_hoist/run.sh baseline
```

Exits non-zero (and prints `FAIL`) on any imbalance (`new != free`). This is a
MANUAL gate — it needs the alloc-count runtime, so it is not part of
`cargo test --workspace`.

Measurement of the adoption win (hoisted vs per-call through the real compiled
pipeline) lives separately in the `#[ignore]`d
`crates/align_driver/tests/needle_plan_hoist_probe.rs`, which compiles the same
source twice under the `ALIGN_NEEDLE_HOIST` toggle.
