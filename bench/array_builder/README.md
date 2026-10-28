# Scalar array construction comparison

```sh
python3 bench/array_builder/run.py BASELINE_ALIGNC CANDIDATE_ALIGNC PRODUCTION_RUNTIME_A > measurements.csv
```

Supply two existing compiler binaries and one compatible production release
`libalign_runtime.a`. The runner copies the archive once for both arms and
rejects its `alloc-count` exports. It prints compiler, runtime and fixture
SHA-256 identities on stderr. Use matching compiler/runtime ABIs; the runner
does not infer compatibility from filenames. Python 3.11 or newer and `nm` are
required.

Both compilers emit the same exported i64 construction function with
release/baseline codegen and `--no-rt-lto`. Kind 0 grows from zero capacity;
kind 1 reserves the exact final length. The function freezes the builder,
checks its length and every element, computes a sum, and destroys the array.
The C harness checks eight lengths (including 0 and growth boundaries) and
three seeds against an independent closed-form sum before timing 300 arrays
of 100,000 elements. Timed calls retain all element checks and verify their
aggregate checksum. Thus timing includes construction, validation and cleanup.

Seven alternating baseline/candidate pairs emit LF CSV. Compare medians by
kind; timings are measurements, never correctness gates. Allocation probes
and instruction profiling do not run inside these measurements. `CC` selects
the native C compiler; macOS and Linux use their respective dead-strip flags.
The shared `bench/native_probe.py` supervisor owns build/link/probe deadlines,
signals, diagnostics and child retirement before exclusive scratch cleanup.

Plan256 records a same-source generic-path control experiment and its separate
private tokenizer qualification. This harness can also compare actual compiler
revisions; its control experiment does not recover the lost historical Mac
baseline or claim whole-application throughput.

The checked-in Linux receipt uses production base `cf8d5495` and the adjacent
`generic-control.patch` applied only in a disposable checkout of that base.
It forces the existing scalar fast-path condition false; no other compiler or
runtime source differs. Copy the two compiler artifacts to separate paths
before timing and use the same uninstrumented runtime archive. Do not install
the control compiler or treat its build cache as production verification.
