# Local native runtime probe runner

The Base64, buffer, CSV field-discovery/normalization, percent/form escape,
escaped-output, UTF-8 replacement, timestamp, Hex, regex and random-sample probes share
`bench/native_probe.py`. Their existing entry points remain:

```sh
bash bench/base64_decode/run.sh
bash bench/buffer_zero/run.sh
bash bench/buffer_pages/run.sh
bash bench/csv_quoted_scan/run.sh
bash bench/csv_normalization/run.sh
bash bench/escaped_decode/run.sh
bash bench/encoding_writes/run.sh
bash bench/utf8_lossy/run.sh
bash bench/time_parse/run.sh 0 1
bash bench/time_fields/run.sh 3
bash bench/hex_decode/run.sh 96
bash bench/regex_spans/run.sh 49
bash bench/rand_sample/run.sh
```

The runner accepts only those thirteen probe names, builds the ordinary release
runtime through `scripts/cargo.sh`, links the selected `main.c` with `-O3` and
the existing macOS/Linux library flags, and passes its CSV output through.
The timestamp, Hex, regex and random-sample fixtures retain their measured
`-O2 -Wall -Wextra -Werror` flags. The timestamp parser requires exactly two
arguments: kind 0–4 and group 0–3. The formatter takes an optional single-digit
kind 0–4; Hex and regex take optional case indices 0–107 and 0–49, retaining
the C fixtures' base-10 syntax (leading ASCII whitespace, sign and zeroes).
Omitting an optional selector runs the complete corpus. Every other probe takes
no arguments. Malformed arguments reject before scratch acquisition or source
mtime changes.
`CC` selects one compiler executable (default `cc`); `CARGO_TARGET_DIR` selects
the Cargo target directory (default `target`, relative to the repository).
Before building, the runner refreshes the runtime source mtime without changing
its bytes. This requires a writable checkout and forces the static-archive
producer to run even with a shared target's Fresh fingerprint; a different
producer's cached archive must not become a measured result.

Empty values use the same defaults. `TMPDIR` follows Python's temporary-directory
selection. Fixture contents, timing loops, output validation and historical
measurement data belong to the individual probes.

Each build, link and probe command owns a new process group. Fixed limits are
900, 60 and 60 seconds respectively, each reserving its last five seconds for
cleanup. HUP, INT and TERM are deferred through spawn and cleanup; failure,
timeout, interruption and successful leader exit all retire the entire group.
The leader remains unreaped until group signalling, then is reaped before the
private scratch directory is removed. Failed child cleanup preserves scratch
and reports its path. This runner requires Python's `waitid`/`WNOWAIT` support.

`python3 -B bench/test_native_probe.py` exercises every shell entry point with
a stalled build/link/probe, a descendant surviving its leader and HUP/INT/TERM
in each phase. The fixture's descendant owns a socket: EOF proves actual exit.
The owner also checks source/archive selection, empty environment defaults and
scratch removal. Test-only interpreter shims shorten phase budgets without
adding runtime configuration to the production runner.

The producer owner additionally compiles a tiny real Rust static library,
replaces its published and fingerprinted archive with another native producer,
and runs the real build/link/probe sequence again. It verifies the original
source's result and unchanged source bytes. Removing the mtime refresh makes
this owner fail at the probe, even though Cargo reports the prior build Fresh.

## Remaining direct native wrapper closure

The direct C fixture wrappers for `time_fields`, `hex_decode`, `regex_spans`
and `rand_sample` duplicated unbounded build/link/run orchestration.
These four entry points now use the existing supervisor without changing its
process ownership strategy or any C fixture. This is one lifecycle boundary;
compiler-driven and resource-measurement harnesses have different owners.

| Obligation | Implementation and owner |
| --- | --- |
| Entry and input admission | Preserve no-argument corpus runs, the optional time kind 0–4 and the optional Hex/regex case index 0–107 / 0–49. Reject malformed selectors before scratch acquisition, source touching or subprocess creation. Admission owners cover bounds, arity and the existing native selector spellings. |
| Producer and link identity | Keep each C fixture and `-O2 -Wall -Wextra -Werror`, the shared runtime producer refresh and platform libraries. Actual-process owners assert source, archive, arguments and flags; the existing stale-archive owner retains producer identity. |
| Construction, return and cleanup | Delegate every build/link/probe to the existing deadline, deferred-signal and pinned-leader process-group owner. The parameterized owner requires descendant socket EOF, direct-child reap and empty scratch after timeout, successful leader exit, HUP, INT and TERM in every phase. Original-wrapper stalled-build controls must fail. |
| Coverage closure | Every direct `bench/<name>/main.c` fixture with a sibling shell runner must appear in the shared registry. A discovery owner makes a new unregistered direct wrapper fail the focused test instead of silently escaping the lifecycle matrix. |
| Fixture behavior | Run the actual release-linked C fixtures separately from lifecycle tests; preserve status/value/byte/count assertions and historical measurements. These runs qualify orchestration, not a new performance claim. |

The matrix precedes implementation. This reuses the reviewed supervisor and
changes neither runtime/library contracts nor its cleanup strategy. One fresh
preflight review covers the complete wrapper boundary; no separate strategy
review or new compiler/ABI/ownership matrix is required.
