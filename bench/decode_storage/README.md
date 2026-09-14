# Decode storage costs

Primary consumer: Apple Silicon and Metal. These CPU kernels isolate compiler
representation costs; they do not measure Metal synchronization or inference.
Plan [62](../../docs/impl/62-decode-optimization-plan.md) owns the measured scope.

Compile `kernels.align` with both compiler revisions, using identical target,
profile and runtime-LTO flags. Export `bits_roundtrip`, `byte_max`, `typed_max`,
`heap_control`, `fresh_sum`, `call_roundtrip`, `byte_loop`, `byte_words_sum` and
`typed_words_sum`. Link both against the **same production** runtime archive. Example Linux commands (substitute compiler and runtime paths):

```bash
alignc emit-obj bench/decode_storage/kernels.align kernels.o --no-rt-lto \
  --export bits_roundtrip --export byte_max --export typed_max \
  --export heap_control --export fresh_sum --export call_roundtrip --export byte_loop \
  --export byte_words_sum --export typed_words_sum
cc -O2 bench/decode_storage/main.c kernels.o target/release/libalign_runtime.a \
  -Wl,--gc-sections -lpthread -ldl -lm -lssl -lcrypto -lz -lzstd -o decode-time
./decode-time 0 0 2000000
./decode-time 1 50000 1000
./decode-time 2 50000 1000
./decode-time 3 0 2000000
./decode-time 4 0 1000000
./decode-time 5 0 2000000
./decode-time 6 50000 1000
./decode-time 7 50000 1000
./decode-time 8 50000 1000
```

Modes: 0 = 4-byte conversion, 1 = byte-chunk max, 2 = typed max control,
3 = forced heap conversion (65-byte capacity), 4 = fresh owned source/chunk sum,
5 = conversion through a local borrowed-byte reader, 6 = the explicit byte max
loop, 7 = byte-word wrapping sum, 8 = typed-word sum control. Mode 5 owns plan 63's
local-call allocation claim; compare against revision `0e236129`.
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

Plan 63 section 11 records the issue-1043 comparison, including the adverse
typed-control result and the follow-up with byte-identical control objects
linked first. Keep whole-module and layout-isolated evidence separate; a change
in code placement can move a control even when its instruction sequence is
unchanged. Native CPU selection is tuning, not a universal fastest-mode promise.
