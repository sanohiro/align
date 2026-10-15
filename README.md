# Align

> 🌐 **English** · [Japanese](./README.ja.md)

Align is an AOT-compiled, data-oriented programming language designed for the **human** who writes the code, the **AI** that generates it, the **compiler** that optimizes it, and the **hardware** that runs it. Errors, ownership transfers, allocation, and parallel work are explicit in the source. Array and slice pipelines let you express data transformations that the compiler can fuse into loops, while columnar layouts keep frequently used fields together in memory.

## Platforms

Currently supported platforms:
- **Linux x86-64 and ARM64**
- **macOS Apple Silicon (aarch64)**
- *Windows is not supported.*

## Installation

### Homebrew (macOS Apple Silicon)

```sh
brew tap sanohiro/align
brew install align
```

### apt (Ubuntu 24.04)

```sh
curl -fsSL https://sanohiro.github.io/align/install.sh | sudo sh
sudo apt install alignc
```
The install script configures the package repositories, then `apt install alignc` pulls the compiler and its runtime dependencies.

### Build from source

Building the workspace needs **Rust 1.96+**, **LLVM 22** and a native C toolchain. Matching **clang 22** and its compiler runtime are also needed for instrumented PGO. The native libraries include OpenSSL, zstd, SQLite and **libpq 17+**; a PostgreSQL server is only needed for database applications and integration tests.

#### Linux (Ubuntu 24.04)

From the repository root, use the same signed LLVM and PostgreSQL package repositories as CI. Ubuntu 24.04's default libpq 16 is too old for the streamed database API:
```sh
sudo apt install build-essential curl wget gnupg pkg-config
scripts/ci-pgdg.sh
align_deps_tmp="$(mktemp -d)"
RUNNER_TEMP="$align_deps_tmp" \
ALIGN_APT_PACKAGES="llvm-22-dev clang-22 libclang-rt-22-dev lld-22 libpq-dev libsqlite3-dev libssl-dev zlib1g-dev libzstd-dev" \
  scripts/ci-apt-llvm.sh install --uncached
rmdir "$align_deps_tmp"
scripts/check-libpq-version.sh
```

#### macOS (Apple Silicon)

Install the dependencies with Homebrew:
```sh
brew install llvm@22 openssl@3 libpq zstd
```
`scripts/cargo.sh` locates LLVM 22 and supplies Homebrew library paths during the compiler build. For later `alignc` invocations, make the keg-only tools and native libraries available in your shell:
```sh
export PATH="$(brew --prefix llvm@22)/bin:$(brew --prefix libpq)/bin:$PATH"
export LIBRARY_PATH="$(brew --prefix libpq)/lib:$(brew --prefix openssl@3)/lib:$(brew --prefix zstd)/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
scripts/check-libpq-version.sh
```

#### Build

```sh
scripts/cargo.sh build --release --workspace
# Compiler: target/release/alignc; REPL: target/release/align-repl
```

## Hello World

Create a file named `hello.align`:

```align
fn main() -> i32 {
    print("hello, align")
    return 0
}
```

Run it with:
```sh
alignc run hello.align
```

If you built from source, use `./target/release/alignc` in place of `alignc`
and `./target/release/align-repl` in place of `align-repl`, from the repository root.

While editing, keep the compiler running and rebuild whenever one of the source files or other inputs
it used changes:

```sh
alignc build hello.align --watch
```

The process keeps the last successful executable in place. Toolchain and library replacements are
picked up by the next observed source/input change, or immediately after restarting the command.

You can also try expressions with `align-repl`. Each entry adds to the session's program, which
`alignc` compiles and runs as a native executable.

```sh
align-repl
```

```text
align> 1 + 2
3
```

## Learn Align

The guide explains syntax, tools, and libraries through worked examples:

**[Tutorial (English)](docs/guide/README.md)** · **[Tutorial (Japanese)](docs/guide/ja/README.md)**

**[The Little Aligner](docs/little-aligner/README.md)** ([Japanese](docs/little-aligner/ja/README.md)) teaches through short questions and answers, in the tradition of *The Little Schemer*. Start here if you want to work out each step yourself: predict a result, follow the data, and reason about ownership and cost. The two books can be read independently or alongside each other.

**[Writing fast Align](docs/guide/26-performance.md)** ([Japanese](docs/guide/ja/26-performance.md)) connects measurement, pipeline fusion, storage reuse, data layout, SIMD, and parallelism in one practical workflow.

## Layout

- `draft.md` — authoritative language specification
- `docs/guide/` — hands-on tutorial (English + Japanese)
- `docs/little-aligner/` — Q&A drill workbook in the style of *The Little Schemer* (English + Japanese)
- `docs/` — design rationale, history, non-goals, open questions
- `docs/impl/` — compiler implementation plan + std module design specs
- `apps/` — first-party package workspaces, including `pkg.web`, `pkg.auth`, and `pkg.kv`
- `editors/` — Vim / Emacs / VS Code support (syntax, snippets)
- `crates/` — the `alignc` compiler workspace

## License

Dual-licensed under either of:
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
