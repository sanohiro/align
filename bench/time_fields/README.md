# Named UTC field rendering

This C consumer measures the actual non-test release runtime's formatter entry,
including final owned-string allocation and free. It is a local measurement,
not a timing correctness gate.

```text
CARGO_TARGET_DIR=/path/to/shared-target bash bench/time_fields/run.sh
CARGO_TARGET_DIR=/path/to/shared-target bash bench/time_fields/run.sh 3
```

The script requires a writable checkout and forces the release runtime producer
before linking, avoiding a shared Cargo archive from a different checkout. The
optional format kind is 0 RFC3339, 1 RFC3339-ms, 2 RFC1123, 3 basic ISO, or
4 basic date.

Each format traverses the same 18 independent UTC golden records: both i64
endpoints, the day after the lower endpoint, negative/zero/modern/leap-day
instants, and every fractional width from one through nine. Four coarser kinds
include the unrepresentable lower-endpoint floor. The fixture fixes expected
ASCII bytes independently of either runtime; warmup compares every byte,
length and status for 200ms before timing. Each of seven samples traverses all
records 20,000 times: 360,000 actual native calls. Timed results check status,
length, pointer presence, observed output-byte and invalid-result counts; every
output is freed. The reported latency is per call in this explicit input mix,
not an individual-input worst case.

Build this identical source against baseline and candidate release producers,
record actual recompilation and distinct executable hashes, then run each of
five format cases in baseline/candidate/candidate/baseline order. Use all 14
samples per arm and kind. Run hosts sequentially, without concurrent agent
builds or tests; pin Linux VM processes to one guest vCPU. Host scheduling remains
uncontrolled. Correctness/allocation owners run separately. No application,
network, SIMD or RSS improvement is promised.

## Local comparison, 2026-10-06

Baseline: `47126ca29357bf5e6d5f57cf2c1256e8a7f63d2c`. Both producers use Rust
1.96.1; macOS ARM64 uses Apple clang 21.0.0 and Linux ARM64 uses GCC 12.2.0.
Linux is the local VM with `taskset -c 0`. Both hosts pass the independent C
goldens. The following are the medians of all 14 samples per arm and format,
with per-format ABBA and no concurrent agent builds/tests. Every sample from
these paired runs contributes to the medians. Times include native
allocation/free and the common C status/count checks.

Separate retained build-qualification runs produced seven samples per format
and producer on each host; later script-reproduction runs produced seven
candidate basic-ISO samples per host. Those runs checked the built consumer and
the checked-in runner. They did not follow the paired ABBA order and are not
included in the paired comparison above; no paired sample was discarded and no
paired case was repeated.

| Format | macOS baseline → candidate | Linux baseline → candidate |
| --- | ---: | ---: |
| RFC3339 | 172.62 → 56.85 ns | 190.65 → 67.07 ns |
| RFC3339-ms | 150.03 → 40.05 ns | 167.27 → 51.56 ns |
| RFC1123 | 137.89 → 52.59 ns | 148.11 → 60.98 ns |
| Basic ISO | 122.78 → 37.30 ns | 134.85 → 50.84 ns |
| Basic date | 66.98 → 34.72 ns | 82.00 → 48.01 ns |

All five corpus medians improve (about 1.9–3.7x on macOS and 1.7–3.2x on
Linux). These are not individual-input or application/network speed guarantees.
The additional leap-day fraction cases differ from plan 119's older nine-input
probe, so compare the paired producers here, not the two documents' absolute
numbers. The private native owner separately confirms no Rust heap request
while constructing Text; the final owned-string allocation is unchanged.

Fixture SHA256: `4aa7b9053d9bdfc13b2610a138dfcff13e5678f2f68357e037b24af809c8fac6`.
Measured executable identities (baseline / candidate):

```text
macOS d59e230d729106fce94edce22bea2549ae151894744df752095988acf7adfdf9
      965ad14bc3e2d411f7452971b6d99172449d400b85e65d9ab8583c2710259e45
Linux 2b5a4f49c2e739d5ebd29633a68fe0360a5e369eea12924dbdd1c1fe592cb1bd
      f8f176c438de97f142e3d656b8c60faf97644da9ebcde7b3f558e9fb2b55f312
```
