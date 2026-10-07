# Compiler check-time scaling measurement

This local benchmark owns plan 21 items 11–21 checking measurements. It
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


## Item 17 result (2026-10-07)

The same host/profile compares merged
`fa2d83b8f70a63f869574b498b9c6e954bb0f58d` with exact identity guards for
header-reference sets, borrow-root sets, ended-root maps and byte-validation
generation sets. The selected-key rebuilds and collision rules remain unchanged;
fallback and lifetime roots are checked independently. Both binaries use
`DYLD_SHARED_REGION=private`; no builds/tests overlap measurement.
[Full samples and hashes](results-leaf-identity-macos.json) retain five alternating
samples after warmup, all 30 command/shape/size cells and checked-work evidence.

| 512 fixed-array declarations | Baseline median | Candidate median | Ratio |
| --- | ---: | ---: | ---: |
| Straight-line check | 0.362 s | 0.230 s | 1.57x |
| Straight-line per-unit check | 0.725 s | 0.459 s | 1.58x |
| Loop check | 3.890 s | 1.279 s | 3.04x |
| Loop per-unit check | 7.654 s | 2.558 s | 2.99x |

All 15 source MIR/raw LLVM pairs, actual checked-work outputs and four rejected
controls agree across both corpora. Numeric control median changes stay below
23 ms; all 16-value controls stay within 5 ms. A deterministic backing-identity
owner detects restoring each of the four unnecessary rebuilds. Other state/fact
copies and repeated traversals remain superlinear; these local results establish
neither near-linear scaling nor Request37's whole-client acceptance.


## Item 18 result (2026-10-07)

The same host/profile compares merged
`7d7fda59a941d060f91b1c4cc6725a15b98f7646` with empty generation-ending
propagation skipped after every directory ending is recorded. Both binaries run
with `DYLD_SHARED_REGION=private`; no build/test jobs overlap measurement.
Run both numeric and storage corpora with `--llvm-parity`.
[Full samples and hashes](results-empty-retirement-macos.json) retain all 30
cells, five alternating samples after warmup and actual CLI checked-work output.

| 512 fixed-array declarations | Baseline median | Candidate median | Ratio |
| --- | ---: | ---: | ---: |
| Straight-line check | 0.231 s | 0.163 s | 1.42x |
| Straight-line per-unit check | 0.459 s | 0.324 s | 1.42x |
| Loop check | 1.278 s | 0.756 s | 1.69x |
| Loop per-unit check | 2.554 s | 1.508 s | 1.69x |

All 15 source MIR/LLVM pairs, checked-work outputs and rejected diagnostics
match. Numeric control median differences stay below 6 ms; all 16-value controls
stay within 2 ms. The exact-state and actual staging-caller owner detects 420
lost owned-path identities when the empty propagation is restored.

Directory endings, opaque observations and every nonempty propagation retain
their prior behavior. The guard avoids whole-state reconstruction when the
already-collected historical roots are empty. Other state/fact copies and
repeated traversals remain superlinear; these measurements do not establish a
whole-client budget or change runtime allocation policy.

## Item 19 result (2026-10-07)

The same host/profile compares merged
`03922e10e7161ac4bc058f4b1523e31165312fc9` with terminal escape-flow probes
skipped while retaining every input and the complete source-order replay.
No build/test jobs overlap measurement; `DYLD_SHARED_REGION` is unset for both
binaries. Numeric and storage corpora run with `--llvm-parity`. [Full samples and hashes](results-terminal-probes-macos.json)
retain 30 cells with five alternating samples after warmup.

| 512-value shape | Command | Baseline median | Candidate median | Ratio |
| --- | --- | ---: | ---: | ---: |
| Result match | check | 1.025 s | 0.943 s | 1.09x |
| Result match | per-unit check | 1.936 s | 1.766 s | 1.10x |
| Straight fixed arrays | check | 0.150 s | 0.128 s | 1.17x |
| Straight fixed arrays | per-unit check | 0.308 s | 0.277 s | 1.11x |

All 15 source MIR/LLVM pairs, checked-work outputs and rejected diagnostics
match. All 16-value medians remain within 1 ms. Straight/try numeric and loop
array control differences remain below 8 ms. The differential semantic owner
compares complete published body facts, cleanup metadata and ordered diagnostics
against the original terminal-probing algorithm; a disabled-skip mutation must
fail its actual probe-count assertion. Nonterminal state copies and repeated
traversals remain superlinear; no whole-client time budget is established.

## Item 20 result (2026-10-07)

The same host/profile compares merged
`2518ca3d1c084538611024b4f87d035ecd032ab3` with union-size lower-bound
reservation and exact-equality construction guards in escape-state joins.
No build/test jobs overlap measurement; `DYLD_SHARED_REGION` is unset for both
binaries. Numeric and storage corpora run with `--llvm-parity`.
[Full samples and hashes](results-join-cost-macos.json) retain all 30 cells with
five alternating samples after warmup from the final implementation.

| 512-value shape | Command | Baseline median | Candidate median | Ratio |
| --- | --- | ---: | ---: | ---: |
| Result match | check | 0.976 s | 0.784 s | 1.25x |
| Result match | per-unit check | 1.802 s | 1.421 s | 1.27x |
| Straight fixed arrays | check | 0.129 s | 0.128 s | 1.01x |
| Straight fixed arrays | per-unit check | 0.280 s | 0.266 s | 1.05x |
| Loop fixed arrays | check | 0.743 s | 0.727 s | 1.02x |
| Loop fixed arrays | per-unit check | 1.496 s | 1.464 s | 1.02x |

All 15 source MIR/raw LLVM pairs, actual checked-work outputs and four rejected
controls agree. Numeric straight/try median differences remain below 2 ms;
all 16-value controls stay within 0.5 ms. Direct self-join owners independently
check idempotence before the equality guard can hide a defect. Capacity and
actual construction-call owners fail when either optimization is disabled.
The same fact lattice, missing-input rules and exact change detector remain.
State copies and repeated traversal still leave superlinear work; no whole-client
time budget or runtime allocation improvement is established.

## Item 21 result (2026-10-07)

The same host/profile compares merged
`146c9894137621ca6e8f303fffe0bae6c23d5390` with immutable value-fact payloads
shared across saved escape states. Only selected generation mutations detach;
owned projections and consuming reads preserve their existing copies.
No builds/tests overlap measurement; `DYLD_SHARED_REGION` is unset for both
binaries. Numeric and storage corpora run with `--llvm-parity`.
[Full samples and hashes](results-shared-values-macos.json) retain all 30 cells
with five alternating samples after warmup from the final implementation.

| 512-value shape | Command | Baseline median | Candidate median | Ratio |
| --- | --- | ---: | ---: | ---: |
| Result match | check | 0.767 s | 0.478 s | 1.60x |
| Result match | per-unit check | 1.403 s | 0.912 s | 1.54x |
| Straight fixed arrays | check | 0.128 s | 0.129 s | 0.99x |
| Straight fixed arrays | per-unit check | 0.266 s | 0.273 s | 0.97x |
| Loop fixed arrays | check | 0.724 s | 0.727 s | 1.00x |
| Loop fixed arrays | per-unit check | 1.458 s | 1.463 s | 1.00x |

All 15 source MIR/raw LLVM pairs, actual checked-work outputs and four rejected
controls agree. Numeric straight/try median differences remain below 1.1 ms;
all 16-value controls stay within 0.6 ms. The straight fixed-array per-unit
median increases by 7.1 ms (2.7%); storage cases do not show a speedup. The
tradeoff is retained for the measured Result-match improvement, without a
uniform-speedup claim. Sharing reduces saved-state payload copies, while fresh
fact insertion adds Arc allocations and other hash-table/traversal work remains.
Lifecycle, all-four-map mutation isolation and selected/no-op allocation owners
qualify the implementation. This does not establish near-linear scaling, a
whole-client time budget or a runtime allocation improvement.
