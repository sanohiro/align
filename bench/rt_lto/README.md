# `--rt-lto` benchmark (M14 Slice 2)

Measures the runtime-bitcode LTO win for the fast-path string primitives
(`docs/impl/07-roadmap.md`, "M14 Slice 2"). The SAME Align kernel object is built twice through the
real driver — once without `--rt-lto`, once with — linked into a Rust timing harness, and compared.

- `eq_count = s.where(x == "hello").count()` — the constant-length equality filter. Under `--rt-lto`
  `align_rt_str_eq`'s body is linked in and inlined, so the literal's length (5) folds into an
  `icmp len, 5` fast path and the majority (length ≠ 5) elements are rejected with no call/`bcmp`.
- `sum_sq_pos = s.where(x > 0).map(x*x).sum()` — the numeric non-regression control. Already
  vectorized with zero in-loop runtime calls, so `--rt-lto` must leave it unchanged.

## Run

```
bench/rt_lto/run.sh [baseline|v3|native]   # default: native
```

It reports the OFF/ON ratio (>1 = `--rt-lto` faster) for both kernels and the compile-time delta of
the `emit-obj` step.

## Reference numbers (WSL2 / AMD Zen 3, LLVM 22.1.8, native, N=1M, 300 rounds)

```
eq_count     off=4398165 ns  on=1510694 ns  ratio=2.911
sum_sq_pos   off=112524 ns   on=112374 ns   ratio=1.001
compile-time (emit-obj, best of 5): off=24ms on=26ms delta=2ms
```

`eq_count` clears the 1.15× gate robustly (2.9×, even above the Slice-1 probe's 2.1× — the probe
double-optimized already-optimized IR, whereas Slice 2 links into the raw module and optimizes once);
the numeric control is flat, confirming the merge is non-regressing on the already-saturated core.
