# Guarded codec Option joins

This independent code-size fixture consumes each of the six guarded codec
Option accessors as an ordinary scalar. It calls no encoder or validator;
the input batch/column descriptors follow the existing core.codec contract.

Use the same source with two compiler artifacts and distinct output paths:

```text
ALIGNC_CACHE=off COMPILER emit-obj bench/codec_joins/kernel.align OUTPUT.o --target-cpu baseline --profile release --export integer --export real --export flag --export text_length --export name_length --export kind_tag
llvm-size -A OUTPUT.o
```

Compare the code section on the same target and LLVM version. The explicit
exports are part of the measurement: their wrappers also occupy code bytes.
The backend owner pins guarded address/load formation and scalar promotion;
`core_codec` and `pkg_frame` own execution, interfaces and input lifetimes.
No throughput or cross-platform code-size guarantee is implied.
