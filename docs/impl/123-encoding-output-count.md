# Avoid duplicate input counts in escaped-output writers

Percent/component, percent/path, HTML and form encoders already compute a
checked exact output length before allocation. Their three private writers
repeat the complete count in an initial assertion, then scan input again to
write. Validate the actual number of initialized bytes at the end of writing
instead. Keep the caller's checked sizing pass and safe indexed writes.
Base64/hex length assertions are constant-time and remain unchanged.

This refines the existing `owned_str_exact` strategy used by plan117: a
successful callback must have initialized every output byte before publishing
the owned string. The shared HTML builder uses the same private writer and
publishes its new length only after return. There is no public API, UTF-8,
allocation, ownership, ABI, effect, serialization, compiler or safety-strategy
change. Plan122's HTML admission remains independent and precedes counting.

| Closure obligation | Implementation and owner |
| --- | --- |
| Exact initialized output | Each of the three writers maintains one monotonically increasing offset. Every increment matches its immediately preceding safe indexed writes. At return assert offset == out.len(); a short output fails a bounds check and a long output fails the final equality. A parameterized native owner compares exact output with independent entity/percent/form oracles and catches short/long destination panics without reading uninitialized memory. |
| Admission and allocation | Existing checked output-size pass still runs before allocation/reserve; all pointer, shape, UTF-8 and alias checks remain in the callers. No extra allocation or buffer copy. Existing html_text native failpoint/counter owner and template native owners cover admission-before-allocation, final payload ownership and zero-copy transfer. |
| Byte and text identity | Percent/component and form accept arbitrary bytes; path preserves slash. HTML accepts UTF-8 and shares the exact five-entity table with template.write. Empty, all256 binary bytes for byte encoders, allASCII plus Unicode/entity/control text for HTML, and boundary/corpus combinations share the same parameterized destination owner. |
| Failure and publication | A private scratch mismatch can partially initialize output before panic but never returns; public callers pass their admitted exact extent. No owned descriptor or builder length is published before callback completion. Malformed input, output-size overflow and OOM retain existing pre-allocation/terminal behavior. No new recovery/cleanup path. |
| Language and consumers | No IR or source changes. Existing m10_encoding, html_text whole/per-unit/LTO owners and pkg.template owners close returned/replaced/moved strings, input borrowing, normal/error exits and shared table identity. |
| Measurement | A checked C harness calls the four real exported entries from an ordinary release runtime, including output allocation/free. Two plain/mixed UTF-8 corpora at about256,4KiB,64KiB; nine samples/case; independent output oracle plus producer-observed calls/byte counts. Retain paired baseline/candidate runs on macOS ARM64 and Linux ARM64 Docker with identical compiler/flags and no competing local builds/tests. Local measurements qualify this path only; no CI threshold, universal throughput or allocation-count improvement claim. |

The destination proof and single final allocation strategy are already
established; the author matrix pass and one fresh independent implementation
review close this representation-preserving refinement. No separate public
design review or normative mirror update is needed. K1 stays deferred.

Acceptance: new native destination owner, existing html_text and template native
owners, m10_encoding/html_text and package template driver owners, local release
measurement, bounded gate and Clippy. Record compact measurement results here
after the implementation; never turn timing into a correctness assertion.

Local release measurement (2026-10-06, Rust 1.96.1): macOS ARM64 and Linux
ARM64 Docker on the same host. The baseline is plan122's merged runtime; both
arms link the identical C harness with `-O3` against ordinary release archives,
without allocation-probe features. Baseline/candidate/candidate/baseline process
order gives eighteen samples per arm/case. All cases return identical exact
bytes and observed call/output-byte counts. No local build or test ran during
timing. The following table gives median microseconds per complete call for
65,550-byte inputs, including final allocation and free.

| Input | Encoder | macOS before | macOS after | Linux before | Linux after |
| --- | --- | ---: | ---: | ---: | ---: |
| plain | percent/component | 156.14 | 87.17 | 166.16 | 95.13 |
| plain | percent/path | 181.64 | 112.98 | 183.90 | 112.93 |
| plain | HTML | 166.95 | 115.59 | 186.14 | 124.57 |
| plain | form | 199.76 | 134.24 | 208.55 | 136.74 |
| mixed | percent/component | 202.23 | 122.48 | 219.99 | 132.47 |
| mixed | percent/path | 239.00 | 149.77 | 238.90 | 151.68 |
| mixed | HTML | 195.35 | 141.24 | 203.41 | 142.50 |
| mixed | form | 234.67 | 163.46 | 246.24 | 164.66 |

All 24 cases improve on each host. Across the measured sizes and inputs, the
baseline/candidate median ratio is 1.35–1.79 on macOS and
1.42–1.76 on Linux. This is an encoder-entry measurement, not a
network/template/application speed promise. The shared template writer retains
its correctness owners but has no separate throughput claim. Reproduce with
`bash bench/encoding_writes/run.sh`; raw CSV includes every sample and observed
work count. Keep timing separate from correctness gates.
