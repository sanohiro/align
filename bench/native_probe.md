# Local native runtime probe runner

The CSV field-discovery/normalization, percent/form escape and UTF-8 replacement probes share
`bench/native_probe.py`. Their existing entry points remain:

```sh
bash bench/csv_quoted_scan/run.sh
bash bench/csv_normalization/run.sh
bash bench/escaped_decode/run.sh
bash bench/utf8_lossy/run.sh
```

The runner accepts only those four probe names, builds the ordinary release
runtime through `scripts/cargo.sh`, links the selected `main.c` with `-O3` and
the existing macOS/Linux library flags, and passes its CSV output through.
`CC` selects one compiler executable (default `cc`); `CARGO_TARGET_DIR` selects
the Cargo target directory (default `target`, relative to the repository).
Empty values use the same defaults. `TMPDIR` follows Python's temporary-directory
selection. Fixture contents, timing loops, output validation and historical
measurement data belong to the individual probes and are unchanged.

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
