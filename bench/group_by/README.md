# `group_by` — grouped-sum duel (Align vs Rust `HashMap`)

Measures Align's column-oriented `s.group_by(.k).sum(.v)` against idiomatic Rust grouped sum with
both `std::collections::HashMap` (the **default**, SipHash) and a fast `ahash` map (the honest fast
baseline). 1M rows, varying the number of distinct groups. The Align aggregate picks one of two
strategies from an O(n) key-range pre-scan: a **dense-id direct-index** path (`acc[key - min]`, no
hashing) when the keys span a tight integer range (`max - min < n`), else a primitive-key
open-addressing **hash** table fed by sequential soa columns. The bench's keys are `LCG % groups`
(range `[0, groups)`), so every config here exercises the dense path — the dense-id analytics shape
this targets.

```sh
bench/group_by/run.sh [baseline|v3|native]   # default native
```

Same plumbing as `bench/json_soa/`: the kernel is built with `alignc emit-obj` and the runtime is
linked as a **cdylib** (dynamic, over the C-ABI). `ahash` is an ordinary cargo dep; standalone
cargo project (own `[workspace]`).

## Result (2026-06-29, native, 1M rows — dense-id path)

```
   groups   distinct    align ms      std ms    ahash ms      vs std    vs ahash
      100        100       1.504        6.690       2.660       4.45x       1.77x
    10000      10000       1.666        8.152       3.364       4.89x       2.02x
  1000000     632390      10.129       47.911      24.863       4.73x       2.45x
```

- **Beats the default `std::HashMap` (SipHash) everywhere — 4.5–4.9×.** The idiomatic-Rust comparison:
  a direct-index columnar aggregate vs a generic SipHash map.
- **Now beats even `ahash` everywhere — 1.77× / 2.02× / 2.45×.** The dense-id path skips hashing
  entirely (`acc[key - min]`), so it wins across the cardinality range — including the high-cardinality
  regime where the older hash path *lost* to `ahash` (see history below).
- **~5× faster than the previous hash path at high cardinality** (≈52 → 10 ms at 632k groups): direct
  indexing replaces the probe chains + rehashes a wide key set used to pay.

### History — the hash path (2026-06-27, superseded for dense keys)
Before the dense-id path, the same kernel used only the open-addressing hash table:

```
   groups   distinct    align ms      std ms    ahash ms      vs std    vs ahash
      100        100       3.81        13.77       4.97        3.62x       1.31x
    10000      10000      12.27        16.20       6.33        1.32x       0.52x
  1000000     632390      54.44        65.24       39.41       1.20x       0.72x
```

It beat `std` everywhere but **lost to `ahash` at high cardinality (0.52–0.72×)**. An earlier mechanism
bug (sizing the table to `2·n` regardless of group count, thrashing cache) was first fixed by growing
the table to track the live group count (10k groups: 0.11× → 0.52× vs `ahash`). The dense-id path then
removed the loss outright for tight-range keys. The hash table still backs **sparse / wide-range** keys
(where a direct-index array would be mostly empty); beating `ahash` *there* still wants a SwissTable
layout (interleaved key+value, SIMD control-byte probing) + a stronger hash — recorded, not yet done.
