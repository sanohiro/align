# Startup observation harness

This directory implements the local S0A startup-evidence producer specified by
[`docs/impl/35-startup-observation-design.md`](../../docs/impl/35-startup-observation-design.md).
It is evidence tooling, not a CI timing gate.

Build the standalone observer explicitly:

```text
scripts/cargo.sh build --release --locked \
  --manifest-path bench/startup/Cargo.toml \
  --target-dir /absolute/private/target
```

Then invoke the sole supported entry point with a caller-created, empty,
exclusive filesystem mount of at most 4 GiB and at least 1 GiB available:

```text
bench/startup/run.sh \
  --observer /absolute/private/target/release/align-startup-observer \
  --observer-sha256 LOWERCASE_SHA256 \
  --work-dir /absolute/empty/mount \
  --provenance /absolute/provenance.tsv
```

The wrapper accepts no defaults or ambient configuration. Successful evidence
is written to `results.tsv` under the supplied work root; inputs, artifacts,
logs, and the copied provenance record remain beside it for audit.

Structural owner tests are run with:

```text
scripts/cargo.sh test --manifest-path bench/startup/Cargo.toml
```
