# Release and distribution

This document records the release shape implemented under `.github/`. It is intentionally narrower than “portable static binary”: `alignc` is an LLVM-based native compiler and its dependencies remain explicit.

## Supported release targets

| Artifact | Runner | Package path |
|---|---|---|
| `alignc-macos-aarch64.tar.gz` | `macos-15` (Apple Silicon) | Homebrew tap |
| `alignc-linux-x86_64.tar.gz` | `ubuntu-24.04` | tarball + amd64 `.deb` |
| `alignc-linux-aarch64.tar.gz` | `ubuntu-24.04-arm` | tarball + arm64 `.deb` |

These are native builds, not cross-compiles. That matters because the compiler links dynamically to LLVM 22, and because the emitted `alignc` rpath must describe the package layout on the target operating system. Every archive contains both `alignc` and its matching `libalign_runtime.a`; the compiler needs that archive beside its real executable when linking user programs.

## CI and release contracts

`.github/workflows/ci.yml` builds and tests the workspace on all three supported targets with Rust 1.96 and LLVM 22. Linux CI uses a checksum-pinned OpenSSL 3.5 LTS build because `crypto.argon2id` requires OpenSSL 3.2 or newer, while Ubuntu 24.04 provides OpenSSL 3.0. Clippy runs once on Linux x86_64; aarch64 maps C `char` to `u8`, which makes several otherwise-portable FFI casts appear redundant only on that target. The macOS job skips two cache tests whose assertions require a distinct baseline/native CPU identity and byte-identical linked executables; neither condition is portable to Apple Silicon and the Mach-O linker. All targets still gate the workspace build, the remaining workspace tests, a release build, and the packaged-command smoke test. A repository-wide rustfmt baseline is intentionally separate from this release-automation change.

`.github/workflows/release.yml` runs for `v*` tags (or an explicitly selected tag), rejects a tag whose version differs from `[workspace.package].version`, and then:

1. builds and smoke-tests the three native compilers;
2. creates tarballs, two Debian packages, checksums, and a generated Homebrew formula;
3. attaches the files to the release;
4. updates `sanohiro/homebrew-align` when `HOMEBREW_TAP_TOKEN` exists;
5. builds and deploys a signed apt repository when `APT_GPG_PRIVATE_KEY` exists.

The optional secrets make the external publishing steps fail closed: no secret means no tap or apt-repository mutation. Release assets are still produced.

## Runtime dependencies are part of the product contract

The distributed compiler is not self-contained:

- LLVM 22 is dynamically linked into `alignc`.
- `cc` links every executable produced by `alignc`.
- `clang-22` and compiler-rt are used by instrumented PGO.
- zlib, zstd, and OpenSSL are linked only when the source program uses the corresponding standard-library capability. `crypto.argon2id` specifically requires OpenSSL 3.2 or newer.

The Debian package therefore depends on LLVM/Clang 22 and the development packages that provide the capability-library linker names. It installs the real compiler and runtime archive together under `/usr/lib/align`, with `/usr/bin/alignc` as a small launcher. The apt installer configures apt.llvm.org before the Align repository. The Homebrew formula likewise installs both artifacts under `libexec`, and its launcher exposes the keg-only OpenSSL and zstd library directories through `LIBRARY_PATH` for the system linker.

## Repository setup required before the first published release

- Create the `sanohiro/homebrew-align` tap repository with a `Formula/` directory.
- Add a token that may update that repository as `HOMEBREW_TAP_TOKEN`.
- Add an ASCII-armored private signing key as `APT_GPG_PRIVATE_KEY`.
- Enable GitHub Pages for the `gh-pages` branch of `sanohiro/align`.

The installer URL and package documentation become live only after the first signed apt deployment. Key rotation requires updating the deployed `align.gpg`; existing clients must then refresh `/etc/apt/keyrings/align.gpg`.
