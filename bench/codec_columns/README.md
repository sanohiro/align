# Checked codec column projections

This independent code-size fixture projects each of the four column kinds and
consumes only the resulting row count. A failed ordinal or kind returns -1.
Use identical explicit roots, profile, CPU target and source for both compilers:

```text
ALIGNC_CACHE=off COMPILER emit-obj bench/codec_columns/kernel.align OUTPUT.o --target-cpu baseline --profile release --export integers --export floats --export flags --export texts
llvm-size -A OUTPUT.o
```

Output paths must be distinct. Compare the code section on the same target and
LLVM version; exported wrappers are included. Backend owners pin both guarded
load stages and optimized joins, while the bounded core_codec native matrix
crosses all accessors, valid/mismatched columns, invalid indices and empty inputs.
This fixture makes no throughput or cross-platform size promise.
