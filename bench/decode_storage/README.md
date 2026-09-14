# Decode storage costs

Primary consumer: Apple Silicon and Metal. These CPU kernels isolate compiler
representation costs; they do not measure Metal synchronization or inference.
Plan [62](../../docs/impl/62-decode-optimization-plan.md) owns the measured scope.

Compile `kernels.align` with both compiler revisions, using identical target,
profile and runtime-LTO flags. Export `bits_roundtrip`, `byte_max`, `typed_max`,
`heap_control` and `fresh_sum`. Link both against the **same production** runtime
archive. Example Linux commands (substitute compiler and runtime paths):

```bash
alignc emit-obj bench/decode_storage/kernels.align kernels.o --no-rt-lto \
  --export bits_roundtrip --export byte_max --export typed_max \
  --export heap_control --export fresh_sum
cc -O2 bench/decode_storage/main.c kernels.o target/release/libalign_runtime.a \
  -Wl,--gc-sections -lpthread -ldl -lm -lssl -lcrypto -lz -lzstd -o decode-time
./decode-time 0 0 2000000
./decode-time 1 50000 1000
./decode-time 2 50000 1000
./decode-time 3 0 2000000
./decode-time 4 0 1000000
```

Modes: 0 = 4-byte conversion, 1 = byte-chunk max, 2 = typed max control,
3 = forced heap conversion (65-byte capacity), 4 = fresh owned source/chunk sum.
Max modes also accept sizes 0/1/39/40/41. Compare checksums before interpreting
times. Use five or more fresh processes per point and report medians; timing
has no universal pass threshold. Baseline revision is
`881076ce56be96a3ea4905fe4ad59e12a4c903c8`. The reference run used Linux x86-64,
Ryzen 9 5950X, LLVM 22.1.8, baseline CPU, O2, runtime LTO off. The shared release
runtime SHA-256 was
`efb7e51c78f5ef4d968e08667ecd39cc3002c9245e665c816ea03f4758746243`.

Build a **different** Linux diagnostic binary with the same command plus
`-DCOUNT_ALLOC -Wl,--wrap=malloc,--wrap=calloc,--wrap=realloc,--wrap=free`.
Run one repetition per mode. The diagnostic excludes input allocation and output
printing; calls into libc during the timed region are counted. Bytes are requested
allocation sizes, not RSS or peak-live storage. Mode 3 must report positive
allocation and free counts; without that control a zero count proves nothing.
Never use its timings. The ordinary timing binary has no wrappers or probe startup.

The primary Mac measurement must compile on the native Mac toolchain and link
with its ordinary platform libraries (the Linux linker-wrapper diagnostic is
not portable). Include Metal command completion/readback, unchanged sampling
and output publication in a separate end-to-end measurement. Keep model,
vocabulary, k, logits, seed, compiler revision and CPU target fixed. The Linux
reference speed ratios cannot predict that end-to-end result.
