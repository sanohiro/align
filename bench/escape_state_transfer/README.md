# Escape-flow state transfer measurement

This local benchmark owns plan 21 item 11's compiler-checking measurement. It
compares release compiler binaries on the same generated straight arithmetic,
Result `?`, and Result `match` chains inside a loop. Each case uses 16, 128 or
512 values. Whole-program and per-unit checks run in fresh bounded processes.

```text
python3 bench/escape_state_transfer/measure.py \
  --baseline /absolute/path/to/baseline/alignc \
  --candidate /absolute/path/to/candidate/alignc > results.jsonl
```

One warmup precedes five alternating repetitions. JSON records actual binary,
source and MIR hashes, source sizes, samples and medians. Accepted MIR and two
rejected-source diagnostic controls must agree before the run completes. These
comparisons supplement deterministic semantic owners; elapsed time is not a
correctness gate. Each compiler invocation has a 20-second work deadline and a
five-second cleanup reserve. Retirement kills the owned process group even if
its direct leader has already exited, then reaps that leader. Run the actual
exited-leader controls with `python3 bench/escape_state_transfer/test_measure.py`.

The synthetic corpus diagnoses a reported request-37 cost; it is not the
external consumer's recombined program. Block/join snapshots remain, so this
change does not promise linear complexity or a whole-client time budget.

## Local result (2026-10-07)

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
