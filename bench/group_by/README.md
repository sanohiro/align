# `group_by` — grouped-sum duel (Align vs Rust `HashMap`)

Measures Align's column-oriented `s.group_by(.k).sum(.v)` (a primitive-key open-addressing
hash-aggregate fed by sequential soa columns) against idiomatic Rust grouped sum with both
`std::collections::HashMap` (the **default**, SipHash) and a fast `ahash` map (the honest fast
baseline). 1M rows, varying the number of distinct groups.

```sh
bench/group_by/run.sh [baseline|v3|native]   # default native
```

Same plumbing as `bench/json_soa/`: the kernel is built with `alignc emit-obj` and the runtime is
linked as a **cdylib** (dynamic, over the C-ABI). `ahash` is an ordinary cargo dep; standalone
cargo project (own `[workspace]`).

## Result (2026-06-27, native, 1M rows)

```
   groups   distinct    align ms      std ms    ahash ms      vs std    vs ahash
      100        100       3.81        13.77       4.97        3.62x       1.31x
    10000      10000      12.27        16.20       6.33        1.32x       0.52x
  1000000     632390      54.44        65.24       39.41       1.20x       0.72x
```

- **Beats the default `std::HashMap` (SipHash) everywhere — 1.2–3.6×.** This is the idiomatic-Rust
  comparison: a primitive-key columnar aggregate vs a generic SipHash map.
- **Beats even `ahash` for low-cardinality grouping (1.31× at 100 groups)** — the common
  analytics shape (few distinct keys, many rows).
- **Loses to `ahash` at high cardinality (0.52–0.72×).** `ahash` pairs an AES-NI hash with
  hashbrown's SwissTable (SIMD control-byte probing, interleaved storage); Align's table uses a
  multiply-hash and three parallel arrays (key / acc / occupancy), so a probe touches more cache
  lines and the hash is weaker.

### The benchmark caught a mechanism bug
The first cut sized the table to `2·n` (the row count). That allocated/zeroed a ~34 MB table for 1M
rows **regardless of group count** and thrashed cache when groups ≪ rows — it lost to `ahash` ~9× at
10k groups (0.11×). Fix: **grow the table to track the live group count** (start at 16, double + rehash
past a 0.75 load). That took 10k groups from 0.11× → 0.52× vs `ahash` and is why Align now beats `std`
across the board. (Exactly the "benchmark before claiming, reconsider the mechanism" mandate paying off.)

### To beat `ahash` at high cardinality (recorded, not yet done)
A SwissTable-style layout (interleaved key+value, SIMD control-byte probing) and a stronger/faster
hash. Secondary — Align already beats the *default* Rust map everywhere; this closes the gap to the
fastest specialized one.
