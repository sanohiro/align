# Release and distribution

This document records the release shape implemented under `.github/`. It is intentionally narrower than “portable static binary”: `alignc` is an LLVM-based native compiler and its dependencies remain explicit.

## Supported release targets

| Artifact | Runner | Package path |
|---|---|---|
| `alignc-macos-aarch64.tar.gz` | `macos-15` (Apple Silicon) | Homebrew tap |
| `alignc-linux-x86_64.tar.gz` | `ubuntu-24.04` | tarball + amd64 `.deb` |
| `alignc-linux-aarch64.tar.gz` | `ubuntu-24.04-arm` | tarball + arm64 `.deb` |

These are native builds, not cross-compiles. That matters because the compiler links dynamically to LLVM 22, and because the emitted `alignc` rpath must describe the package layout on the target operating system. Every archive contains `alignc`, `align-repl`, and their matching `libalign_runtime.a`; both binaries need that archive beside their real executable when linking user programs, because each resolves it relative to its own path.

## CI and release contracts

`.github/workflows/ci.yml` builds the workspace on all three supported targets with Rust 1.96 and LLVM 22. Linux CI uses a checksum-pinned OpenSSL 3.5 LTS build because `crypto.argon2id` requires OpenSSL 3.2 or newer, while Ubuntu 24.04 provides OpenSSL 3.0. Linux x86_64 runs bounded library/binary Clippy and `scripts/test-pr.sh`, which covers the deterministic compiler libraries, the cross-crate interface and formatter targets, and the M0 compile/link/run smoke. All three targets gate the workspace build, a release build, and the packaged-command smoke test. Deep feature, resource, fuzz, stress, and measurement suites are selected when their owner or a consumed shared invariant changes; they are not accumulated into a full-workspace PR or release gate. A repository-wide rustfmt baseline is intentionally separate from this release-automation change.

`.github/workflows/nightly.yml` adds two scheduled jobs that are deliberately not PR gates: the out-of-gate full workspace suite (`docs/impl/16-test-policy.md`) and `release-smoke`, which builds the release-profile compiler on x86_64 and runs the packaged-command smoke test. Every Linux job in `ci.yml`, `nightly.yml`, and `release.yml` sets `ALIGNC_LINKER=lld` and the macOS legs set `system`, so a missing `ld.lld` is a red build rather than a silent fallback (`docs/impl/21-build-perf-plan.md` item 2).

`.github/workflows/release.yml` runs for `v*` tags (or an explicitly selected tag), rejects a tag whose version differs from `[workspace.package].version`, and then:

1. builds and smoke-tests the three native compilers with `--profile dist` under two-phase PGO over the examples and `pkg.db` corpus;
2. creates tarballs, two Debian packages, checksums, and a generated Homebrew formula;
3. attaches the files to the release;
4. updates `sanohiro/homebrew-align` when `HOMEBREW_TAP_TOKEN` exists;
5. builds and deploys a signed apt repository when `APT_GPG_PRIVATE_KEY` exists.

Only a versioned release pays for that tuning: ordinary `--release` builds stay on the untuned default so routine batch builds never carry the LTO/PGO cost. The compiler uses thin LTO, one codegen unit, and compiler-only PGO. Its packaged runtime uses the same dist optimization profile but stays outside compiler-training instrumentation because `alignc` links that archive into the programs compiled during training. `align-repl` is built in the same profile-use invocation as the compiler — it is a thin shell over the profiled driver and carries no training corpus of its own — but because it is the one workspace crate that depends on `align_runtime`, that invocation would otherwise rebuild the archive under the compiler's profile; the phase stashes the uninstrumented archive and restores it, and `scripts/test-pr-workflow.sh` fails if the restore is ever dropped. The `alignc` binary links mimalloc in every profile.

The optional secrets make the external publishing steps fail closed: no secret means no tap or apt-repository mutation. Release assets are still produced.

## Prebuilt cache distribution

Build-performance item 4 shipped in PR #893. Every native archive contains one
target-native, compiler-exact tree:

```text
share/align/cache/1/
  cas/
  actions/
    codegen/
    unit/
  index/
    codegen/
    unit/
```

Debian and Homebrew retain this path relative to the real `alignc` executable.
The tree is warmed only after the final PGO-use compiler is built, and the same
binary that warmed it is copied into the package. At run time it is an
immutable fallback behind the ordinary XDG cache, never a publication or
`cache clear` target. Missing or unusable packaged bytes are a cache miss, not
an installation failure. The native release gate installs the generated
Homebrew formula through Homebrew's real cleanup/relocation path, compares the
installed real compiler with the warmed binary byte-for-byte, and requires
packaged hits; a formula that rewrites or strips the compiler after warming
cannot ship.

The corpus is the byte-exact first-party `pkg.db`, `pkg.web`, `pkg.frame`, and `pkg.auth`
source at the release tag. Those source trees remain separately vendored by
users and are not added to compiler import search or to the release archive.
Compiler-provided `core` and `std` imports have no file-backed compilation unit,
so they have no distributable unit-cache entry. Only the default
release/baseline/runtime-LTO-on/non-ThinLTO codegen tuple is warmed. Frontend
entries remain applicable to every ordinary non-ThinLTO backend configuration;
ThinLTO deliberately keeps its existing all-MIR path and consults none of the
packaged entries.

The shipped implementation also closes the dynamic-LLVM identity gap before
distributing objects. Codegen, prelink, and backend keys use v4 and carry the
loaded ELF GNU build id or Mach-O UUID in addition to the reported LLVM
version. If that native identifier cannot be obtained and validated, frontend
reuse remains available but all codegen cache reads and publications are
disabled; a same-version generator from another package build can never
consume the release objects.

## Runtime dependencies are part of the product contract

The distributed compiler is not self-contained:

- LLVM 22 is dynamically linked into `alignc`.
- `cc` links every executable produced by `alignc`. On ELF `alignc` additionally asks that driver to run `ld.lld` from the matched LLVM install when one is present; `ALIGNC_LINKER=lld|system` forces the choice and Mach-O never selects lld.
- `clang-22` and compiler-rt are used by instrumented PGO.
- zlib, zstd, and OpenSSL are linked only when the source program uses the corresponding standard-library capability. `crypto.argon2id` specifically requires OpenSSL 3.2 or newer.

The Debian package therefore depends on LLVM/Clang 22 and the development packages that provide the capability-library linker names. It installs the real compiler, the REPL, and the runtime archive together under `/usr/lib/align`, with `/usr/bin/alignc` and `/usr/bin/align-repl` as small launchers. The apt installer configures apt.llvm.org before the Align repository. The Homebrew formula likewise installs all three artifacts under `libexec`, and its launchers expose the keg-only OpenSSL and zstd library directories through `LIBRARY_PATH` for the system linker — a bare symlink would not, and `align-repl` links a program on every entry.

## Repository setup required before the first published release

- Create the `sanohiro/homebrew-align` tap repository with a `Formula/` directory.
- Add a token that may update that repository as `HOMEBREW_TAP_TOKEN`.
- Add an ASCII-armored private signing key as `APT_GPG_PRIVATE_KEY`.
- Enable GitHub Pages for the `gh-pages` branch of `sanohiro/align`.

The installer URL and package documentation become live only after the first signed apt deployment. Key rotation requires updating the deployed `align.gpg`; existing clients must then refresh `/etc/apt/keyrings/align.gpg`.
