# Compiler check-time scaling measurement

This local benchmark owns plan 21 items 11–16 checking measurements. It
compares release compiler binaries on the same generated straight arithmetic,
Result `?`, and Result `match` chains inside a loop. Each case uses 16, 128 or
512 values. `--corpus storage` selects fixed-array declarations in straight-line
and loop bodies at the same sizes. Whole-program and per-unit checks run in
fresh bounded processes.

```text
python3 bench/escape_state_transfer/measure.py \
  --baseline /absolute/path/to/baseline/alignc \
  --candidate /absolute/path/to/candidate/alignc > results.jsonl
```

Add `--llvm-parity` when qualifying MIR optimization changes: it also compares
raw LLVM byte-for-byte and records its hash, covering emitted cold attributes
and exceptional branch weights. Item 12 uses this option.

One warmup precedes five alternating repetitions. JSON records actual binary,
source and MIR hashes, source sizes, samples and medians. New runs also retain
the actual CLI checked-function/unit output and require it to agree between
revisions and repetitions. Accepted MIR and two
rejected-source diagnostic controls must agree before the run completes. These
comparisons supplement deterministic semantic owners; elapsed time is not a
correctness gate. Each compiler invocation has a 20-second work deadline and a
five-second cleanup reserve. Retirement kills the owned process group even if
its direct leader has already exited, then reaps that leader. Run the actual
exited-leader controls with `python3 bench/escape_state_transfer/test_measure.py`.

The synthetic corpus diagnoses a reported request-37 cost; it is not the
external consumer's recombined program. Block/join snapshots remain, so this
change does not promise linear complexity or a whole-client time budget.

## Item 11 result (2026-10-07)

Apple M1, macOS arm64, Rust 1.96.1, LLVM 22.1.8, release binaries. The baseline
is merged `e17648b39ff8bdbdf8492b756a459d339f22569d`; the candidate changes the
state-transfer wrapper while retaining its evaluator. No build/test jobs ran
during measurement. [Full samples and hashes](results-macos.json) retain all
18 shape/size/command cells and four diagnostic comparisons.

| 512-value shape | Command | Baseline median | Candidate median | Ratio |
| --- | --- | ---: | ---: | ---: |
| straight | `check` | 0.965 s | 0.049 s | 19.64x |
| straight | `check-per-unit` | 1.916 s | 0.082 s | 23.48x |
| try | `check` | 1.369 s | 0.061 s | 22.43x |
| try | `check-per-unit` | 2.736 s | 0.120 s | 22.86x |
| match | `check` | 5.964 s | 1.096 s | 5.44x |
| match | `check-per-unit` | 18.670 s | 8.939 s | 2.09x |

All 16-value controls improve or remain close: whole-program medians change from
24.3/25.5/32.9 ms to 22.9/23.6/27.3 ms for straight/try/match; per-unit medians
change from 28.2/30.2/45.4 ms to 25.4/26.5/34.1 ms. These small differences do
not establish a general startup-speed claim.

Residual scaling remains visible: match checks at 128/512 values take
0.112/1.096 s, and per-unit checks take 0.310/8.939 s. The wrapper removes one
measured cost; block snapshots, joins and other checking/lowering work remain.
Request 37's real consumer recombination and full-module budget are unverified.


## Item 12 result (2026-10-07)

The same host/profile and corpus compare merged
`8bb307b4f6ef608191e88e8728771a2bf1a509f9` with the empty exceptional-edge guard.
No builds/tests overlap. [Full samples and hashes](results-empty-region-macos.json)
retain all 18 cells, nine matching MIR and raw LLVM outputs, and four matching
rejected-source diagnostic comparisons. Run the command above with `--llvm-parity`.

At 128/512 values, Result-match `check-per-unit` medians improve from
0.311/8.895 s to 0.203/2.091 s (1.53x/4.25x). The 512-value whole-program
`check` remains 1.106/1.101 s, because it does not run the MIR cold pass. The
512-value `?` per-unit control remains 0.119/0.119 s; it has exceptional records
and continues through the existing algorithm. All six 16-value controls remain
within 1 ms of their baseline medians. These are local observations, not a
startup-time guarantee.

The empty-edge case no longer builds a dominator relation that cannot contribute
any exceptional block. Nonempty-edge dominance and remaining semantic analysis
still cost work; this does not establish near-linear checking or Request 37's
actual consumer acceptance. The earlier item11 measurements remain historical.

## Item 13 result (2026-10-07)

The same host/profile compares merged
`c44ebf5704b613bf898f013f64f170642e6c4e86` with final initial-edge state transfer
and in-place joins. Both binaries run with `DYLD_SHARED_REGION=private`; no
builds/tests overlap. [Full samples and hashes](results-edge-propagation-macos.json)
retain all 18 cells, nine matching MIR outputs and four matching rejected-source
diagnostic comparisons, with five alternating samples after warmup.

At 128/512 values, Result-match whole-program medians change from 0.122/1.128 s
to 0.117/1.049 s; per-unit medians change from 0.213/2.102 s to 0.205/1.969 s.
The 512-value improvements are 1.08x and 1.07x. All six 16-value controls and
all straight/try controls remain within 1 ms of the baseline median. This is a
modest local improvement, not near-linear scaling or a consumer time guarantee.

A previously absent final successor receives the owned output without another
complete copy. Existing successor inputs retain their allocations while joining;
all facts, join rules, worklist order and diagnostics remain. Block evaluation
still copies its saved input, so substantial block/state cost remains. Request 37
consumer recombination and acceptance stay external.

## Item 14 result (2026-10-07)

The same host/profile compares merged
`c9c9edde401f209607810bd8b6a175e1eb21f2d8` with selected Current-to-Prior table
updates. Both binaries run with `DYLD_SHARED_REGION=private`; no build/test jobs
overlap measurement. Run the command above with `--corpus storage --llvm-parity`,
and again with `--corpus numeric --llvm-parity` for the previous numeric controls.
[Full samples and hashes](results-recency-map-macos.json) retain five alternating
samples after warmup, actual CLI checked-work output and output parity evidence.

| 512 fixed-array declarations | Baseline median | Candidate median | Ratio |
| --- | ---: | ---: | ---: |
| Straight-line check | 0.789 s | 0.511 s | 1.54x |
| Straight-line per-unit check | 1.586 s | 1.023 s | 1.55x |
| Loop check | 9.112 s | 5.052 s | 1.80x |
| Loop per-unit check | 18.346 s | 10.134 s | 1.81x |

All 15 numeric/storage source MIR and LLVM comparisons match. The four rejected
source/command controls pass in both corpus runs. Numeric control medians differ
by at most 6 ms; the 16-value numeric controls differ by less than 1 ms.

Only named Current entries move into Prior. Collision operand/callback order,
all facts and diagnostics remain; unrelated entries no longer undergo complete
map reconstruction. The no-op storage owner fails with the original algorithm.
Table/fact copies and other repeated analysis still produce superlinear growth;
these local measurements do not establish Request 37's real-client acceptance.

## Item 15 result (2026-10-07)

The same host/profile compares merged
`6ffc322cd6eb1eb1607606cda31cd5399683dbf3` with validated storage-formation
commit. Both binaries run with `DYLD_SHARED_REGION=private`; no build/test jobs
overlap measurement. Run both numeric and storage corpora with `--llvm-parity`.
[Full samples and hashes](results-formation-commit-macos.json) retain all 30
cells, five alternating samples after warmup and actual CLI checked-work output.

| 512 fixed-array declarations | Baseline median | Candidate median | Ratio |
| --- | ---: | ---: | ---: |
| Straight-line check | 0.512 s | 0.433 s | 1.18x |
| Straight-line per-unit check | 1.025 s | 0.867 s | 1.18x |
| Loop check | 5.066 s | 4.268 s | 1.19x |
| Loop per-unit check | 10.144 s | 8.595 s | 1.18x |

All 15 source MIR/LLVM pairs and checked-work outputs match. The four rejected
source/command controls pass in both corpus runs. Numeric control medians differ
by at most 4 ms; the 16-value numeric controls differ by less than 1 ms.

Formation validates the complete batch before updating the caller's staged
tables. Concrete prepared records remove one complete inner directory/content
clone while preserving validation errors, recency and collision order. Restoring
the original Clone-bound helper fails the non-Clone payload owner at compile
time. Outer staging copies, key-parity scans and repeated analysis remain;
these local measurements do not establish near-linear checking or Request 37's
consumer acceptance, and do not claim fewer runtime allocations.


## Item 16 result (2026-10-07)

The same host/profile compares merged
`811437746fb5576be1550ae0e9fb42edb3a71a3a` with direct validated formation in
both checkers. Both binaries run with `DYLD_SHARED_REGION=private`; no build/test
jobs overlap measurement. Run both corpora with `--llvm-parity`.
[Full samples and hashes](results-formation-transaction-macos.json) retain all 30
cells and five alternating samples after warmup.

| 512 fixed-array declarations | Baseline median | Candidate median | Ratio |
| --- | ---: | ---: | ---: |
| Straight-line check | 0.434 s | 0.361 s | 1.20x |
| Straight-line per-unit check | 0.867 s | 0.723 s | 1.20x |
| Loop check | 4.261 s | 3.892 s | 1.09x |
| Loop per-unit check | 8.596 s | 7.655 s | 1.12x |

All 15 numeric/storage source MIR and raw LLVM pairs, actual CLI checked-work
outputs and four rejected-source controls agree across both corpora. Numeric
median changes stay below 9 ms, and the 16-value controls stay below 1 ms.

Complete admission precedes existing-payload renaming and table key transitions,
so callers no longer need a separate full table copy for failure atomicity.
Fresh payloads and all external observations retain their existing semantics.
Actual EscapeCheck/MoveCheck caller owners detect restoration of the copies by
pinning unrelated release-set backing. Other fact/state copies and repeated
analysis remain superlinear; these measurements do not establish Request 37's
consumer budget or a runtime allocation improvement.
