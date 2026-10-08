# Percent/form escape lookup measurement

Baseline: `89729abdf48ea8aea4c04534bdc823dc54c20b04`. The candidate reuses the existing hex decode table for both percent-escape decoders; [plan 146](../../docs/impl/146-percent-form-hex-lookup.md) owns its unchanged public contract and acceptance.

Measured on 2026-10-08: Apple M1 macOS and Linux ARM64 Docker on the same host, Rust 1.96.1 ordinary release builds. Each platform links the same C harness at `-O3` to its baseline/candidate runtime archives, without allocation-probe features. Platform runs are sequential and no builds/tests run during timing. These are local decoder-entry measurements, not network/application throughput, RSS or a portable speed promise.

The harness calls the real percent/form exports, observes the buffer view, and frees every returned owner. Before every trial it compares every output byte against an independent scalar oracle and checks the published read capacity. Timed loops verify actual statuses and lengths and accumulate completed call/output-byte counts. Input fixtures and expected output are prepared outside timing; returned payload and handle allocation/free are included.

Five input patterns cover plain text, sparse escapes, dense escapes including binary output, plus-only text and alternating short ordinary/escaped runs. Empty and small inputs are controls beside approximately 4 KiB and 64 KiB inputs. Each case warms up for 100 ms and records seven trials. Process order baseline/candidate/candidate/baseline provides fourteen samples per arm/case. The table reports their median microseconds per complete call for approximately 64 KiB inputs.

| Input | Decoder | macOS baseline → candidate µs | Linux baseline → candidate µs |
| --- | --- | ---: | ---: |
| plain | percent | 41.95 → 41.30 | 42.02 → 41.87 |
| plain | form | 44.62 → 44.55 | 43.88 → 44.48 |
| sparse | percent | 42.32 → 41.56 | 42.43 → 41.50 |
| sparse | form | 43.58 → 42.94 | 43.65 → 43.38 |
| dense | percent | 39.52 → 23.27 | 39.35 → 23.20 |
| dense | form | 45.38 → 23.59 | 44.66 → 23.53 |
| plus | percent | 41.93 → 42.09 | 41.98 → 41.97 |
| plus | form | 46.35 → 47.20 | 47.21 → 47.24 |
| mixed | percent | 49.31 → 37.11 | 49.41 → 37.74 |
| mixed | form | 46.23 → 33.55 | 46.12 → 33.62 |

Dense-escape medians improve 1.70–1.92× on macOS and 1.70–1.90× on Linux; mixed-input medians improve 1.33–1.38× and 1.31–1.37× respectively. Large plain/sparse/plus controls remain within about 2%; no broad claim of faster ordinary-byte copying follows. Small-case differences are retained in the raw data rather than used as a timing gate. Exact allocation owners separately preserve one input-sized payload reservation and the existing decoded read capacity.

All observations: [macOS samples](macos-samples.csv), [Linux samples](linux-samples.csv). Reproduce the current revision with:

```sh
bash bench/escaped_decode/run.sh
```

For comparison, build the baseline runtime at the commit above and link the same `main.c` against that archive, using the platform flags in `run.sh`. Alternate the two executables in the recorded process order, with no concurrent build/test load. Keep all samples and validate their producer work counts before comparing medians. Timing is not a correctness assertion.
