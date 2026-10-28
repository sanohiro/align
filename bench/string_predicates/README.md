# String predicate specialization probe

Compare two existing `alignc` binaries on the same independently authored
prefix/suffix classifiers and dynamic-needle control:

```sh
python3 bench/string_predicates/run.py /path/to/baseline/alignc /path/to/candidate/alignc > measurements.csv
```

The compiler artifacts must match the host architecture and use LLVM22. The
probe emits exported objects with release/baseline and default rt-LTO, then
links them to the same C harness. No full consumer build or runtime archive is
needed. Each case validates every result against an independent byte comparison
before timing 6,400,000 calls, and checks the final checksum. Kinds 0/1/2 are
constant prefix, constant suffix and dynamic prefix-plus-suffix respectively.
Seven repetitions alternate baseline/candidate order; compare per-kind medians.

The runner reuses `bench/native_probe.py`'s build/link/probe deadlines, deferred
signals and process-group retirement, owns an exclusively acquired scratch
directory, and streams LF CSV rows. Child diagnostics remain on stderr. Timing
contains no allocation/resource instrumentation. Plan251 records the local
measurement and its limits; this is not an application-throughput benchmark.
