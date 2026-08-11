#!/usr/bin/env bash
# Shared macOS dynamic-loader setting for anything that runs a freshly linked
# binary from this repository. Sourced, never executed.
#
# Some macOS hosts intermittently park freshly linked binaries at 0% CPU in
# _dyld_start against the shared dyld cache (recurring HANDOFF incidents,
# historically dodged per-run with dedicated target dirs). A private region
# avoids the stall. Respect an explicit override, and leave CI untouched.
#
# scripts/cargo.sh applies this to every Cargo invocation, and
# scripts/run-gate-binaries.sh applies it to the gate's test binaries, which it
# executes itself rather than through `cargo test`. One definition, so the two
# cannot drift into treating the same host differently.
align_use_private_dyld_region() {
  [ "$(uname -s)" = Darwin ] || return 0
  [ -z "${CI:-}" ] || return 0
  export DYLD_SHARED_REGION="${DYLD_SHARED_REGION:-private}"
}
