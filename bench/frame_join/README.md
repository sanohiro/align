# `pkg.frame` stable inner-join probe

This non-gating local benchmark exercises the runtime engine behind both `pkg.frame` v1 joins. It
records one-to-one i64, duplicate-fanout i64, equal-byte string, and deliberately bucket-colliding
string corpora. The output includes build/probe/output rows and the exact logical peak scratch and
output bytes from the accepted design formula. There is no timing acceptance threshold.

```sh
bench/frame_join/run.sh
```

The harness calls the checked runtime ABI directly with canonical codec column bytes. Preparation,
collision-key search, and correctness checks are outside the timed region. Every timed result is
freed through `align_rt_free` before the next sample.
