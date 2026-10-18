# Owned task output bands

Provider witness for align-llm Request152 / Plan228. Eight explicit tasks read
shared byte input and return contiguous owned `array<u8>` bands. The caller
collects handles in input order and explicitly appends their bytes to one buffer.
There is no index array, packed scalar output, unsafe pointer or mutable capture.
The same fused sequential map loop runs inside every band and in the sequential
control. This CPU pixel transform performs 48 dependent floating-point steps per
input byte; it is a workload witness, not a resize/product benchmark.

Build with `scripts/cargo.sh build --bin alignc` and
`scripts/cargo.sh build -p align_runtime` (without allocation probes), then run:

```text
python3 bench/task_owned_results/run.py target/debug/alignc
```

The compiler's host build profile does not change the generated program's
explicit release/native target. Each fresh process checks every byte against the
sequential output, warms both arms, and times one complete arm. Timing includes
output allocation, computation, join and ordered assembly; input creation,
parity checks, checksum and printing are outside the interval. Samples alternate
arm order. The script reports every sample, output length/checksum, host, CPU
count and exact compiler hash. The shared `native_probe` supervisor retires each complete process group on
success, failure, timeout and HUP/INT/TERM, then removes temporary native artifacts.
A failed child retirement preserves scratch and reports its path; diagnostics
inherit stderr. Each subprocess has a 60-second budget.
`python3 bench/task_owned_results/test_run.py` checks both entrypoint phases with
actual signal-resistant descendants and already-exited leaders. No timing threshold runs in CI;
`task_owned_bands_match_every_sequential_byte` is the whole/per-unit correctness
owner. Consumer adoption and `test-vision-prepare` remain separate.

On 2026-10-18, the 8-core Apple M1 measured a **4.03x median speedup** for
4,194,307 bytes: 197.22 ms sequential versus 48.97 ms parallel, with seven
samples per arm. [The measurement record](measurement-20261018.json) retains
every sample, host/compiler identity and workload source hashes. This is the
provider kernel above; no consumer resize implementation was changed.

The recorded `run.py` hash identifies the original successful measurement. The
subsequent review fix replaced only the Python child-process supervisor; kernel,
generated-program source and measured scope are unchanged.
