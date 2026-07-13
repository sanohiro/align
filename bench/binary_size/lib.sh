# Portable helpers shared by bench/binary_size/run.sh and bench/binary_size/profiles.sh.
#
# The two scripts previously assumed a GNU/Linux/ELF host: GNU `stat -c%s` (BSD/macOS `stat` has no
# `-c`, only `-f`), bash 4's `mapfile` (absent in bash 3.2, e.g. macOS's system `/bin/bash`), and GNU
# `readelf` output parsed directly. Ported (External binary-optimization audit, Codex 2026-07-12,
# item 2 deferred sub-item (a)) so the scripts run on any host with an LLVM toolchain and a
# POSIX-ish bash, matching #426's compiler-side move to `ObjectFormat`-aware linking and
# `llvm-readobj`/`llvm-nm` in `alignc size` (crates/align_driver/src/size.rs).
#
# Object-format note: this repo's compiler links ELF (Linux) and Mach-O (macOS) executables
# (`align_driver::ObjectFormat`). `llvm-readobj --needed-libs` / `llvm-nm` read both dialects
# uniformly, which is why they replace `readelf`/GNU `nm` here (same rationale as the compiler side,
# see size.rs's doc comment). This development box is Linux, so only the ELF path below is verified
# end-to-end; the Mach-O path is structurally identical -- llvm-readobj emits `LC_LOAD_DYLIB` install
# names instead of `DT_NEEDED` sonames inside the same `NeededLibraries [ ... ]` block, and
# llvm-nm's stripped-vs-symbols behavior (empty stdout + a stderr note) does not vary by format --
# but it has not been run on a Mach-O host.

# filesize FILE -> byte size on stdout.
# GNU coreutils `stat -c%s`; BSD/macOS `stat -f%z`; else POSIX `wc -c` (always present) as the final
# fallback so this never depends on which `stat` dialect the host ships.
filesize() {
  stat -c%s "$1" 2>/dev/null || stat -f%z "$1" 2>/dev/null || wc -c <"$1"
}

# llvm_tool NAME -> path/name of the LLVM tool on stdout, nonzero exit if not found.
# Mirrors the PATH half of align_driver::llvm_tool's search order (crates/align_driver/src/lib.rs):
# the build-time `LLVM_SYS_221_PREFIX` lookup it tries first is compiled-in Rust state, not
# something a shell script can read, so only the PATH-based half is replicated:
#   1. <name>-22 on PATH (apt.llvm.org naming; matches align_codegen_llvm::LLVM_TOOL_VERSION).
#   2. plain <name> on PATH.
llvm_tool() {
  local name="$1" cand
  for cand in "${name}-22" "$name"; do
    if command -v "$cand" >/dev/null 2>&1; then
      printf '%s\n' "$cand"
      return 0
    fi
  done
  return 1
}

# gated READOBJ BINARY -> the gated libs (z/zstd/crypto/ssl) among BINARY's dynamic dependencies,
# space-separated and sorted, "-" if none. READOBJ is an llvm-readobj path/name from llvm_tool (pass
# "" if unavailable -> prints "?"). Reads `--needed-libs`: DT_NEEDED sonames on ELF, LC_LOAD_DYLIB
# install-name paths on Mach-O -- both listed one per line inside a `NeededLibraries [ ... ]` block,
# so the line is basename'd before matching (an ELF soname is already bare; a Mach-O install name is
# a full path).
gated() {
  local readobj="$1" bin="$2" g rc
  [ -n "$readobj" ] || { echo "?"; return; }
  # Capture the pipeline's own exit status (via the `if`, so `set -e` never trips here regardless
  # of outcome — `set -o pipefail` in the caller makes that status reflect `--needed-libs` itself,
  # not just the downstream awk/sort/tr, which will happily emit empty output for empty/garbage
  # input). A nonzero status (bad file, unsupported format, tool crash) reports "?" — empty output
  # from a *successful* run (no gated deps at all) must not be conflated with a failed run.
  if g=$("$readobj" --needed-libs "$bin" 2>/dev/null | awk '
    /^NeededLibraries \[/ { inside=1; next }
    inside && /^\]/ { inside=0; next }
    inside {
      line = $0
      sub(/^[ \t]+/, "", line)
      n = split(line, parts, "/")
      base = parts[n]
      if (base ~ /^lib(z|zstd|crypto|ssl)([._-]|$)/) print base
    }
  ' | sort -u | tr '\n' ' '); then
    rc=0
  else
    rc=$?
  fi
  [ "$rc" -eq 0 ] || { echo "?"; return; }
  # Trailing space kept (not trimmed): matches the old `readelf | grep -oE ... | tr '\n' ' '`
  # byte-for-byte, which the printf column-width formatting in run.sh/profiles.sh was tuned against.
  [ -n "$g" ] && echo "$g" || echo "-"
}

# stripped NM BINARY -> "stripped" if BINARY carries no symbol table, else "symbols". NM is an
# llvm-nm path/name from llvm_tool (pass "" if unavailable -> prints "?"). llvm-nm prints nothing to
# stdout and a "no symbols" note to stderr for a stripped file, on both ELF and Mach-O, so
# stdout-emptiness is the format-independent stripped signal -- no need to grep for `.symtab`, which
# is ELF-only vocabulary (Mach-O's symbol table is a load command, not a section, so `readelf
# -SW | grep '\.symtab'` never even applied there). Emptiness is only a valid "stripped" signal on a
# *successful* run, though: llvm-nm also prints nothing to stdout (with an error to stderr) when it
# fails outright (bad file, unsupported format), so the exit status is checked first — that failure
# must report "?", not be silently read as "stripped".
stripped() {
  local nm="$1" bin="$2" out rc
  [ -n "$nm" ] || { echo "?"; return; }
  if out=$("$nm" "$bin" 2>/dev/null); then
    rc=0
  else
    rc=$?
  fi
  [ "$rc" -eq 0 ] || { echo "?"; return; }
  if [ -n "$out" ]; then echo symbols; else echo stripped; fi
}
