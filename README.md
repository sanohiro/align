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
The install script configures the LLVM 22 and Align apt repositories, then `apt install alignc` pulls the compiler and its runtime.

### Build from source

Building the compiler needs **Rust 1.96+** and **LLVM 22** (with a matching **clang** as the C compiler/linker).

#### Linux (Ubuntu 24.04)

Install the LLVM toolchain from the official `apt.llvm.org` repository; `llvm-config-22` must be on your `PATH`:
```sh
sudo apt install llvm-22 llvm-22-dev clang-22
```

#### macOS (Apple Silicon)

Install the dependencies with Homebrew:
```sh
brew install llvm openssl@3 zstd
```
The `llvm` formula currently provides LLVM 22; if Homebrew has since moved it past 22, install the versioned `llvm@22` formula instead. Homebrew's LLVM is keg-only (its `llvm-config` is not on your `PATH`), so point the build at it and add the linker search paths for the runtime's native libraries (`zstd`, `openssl@3`). Add these to your shell profile, or prefix each `cargo` / `alignc` command with them (the same `LIBRARY_PATH` is needed when running an `alignc`-built program that links those libraries):
```sh
export LLVM_SYS_221_PREFIX="$(brew --prefix llvm)"
export LIBRARY_PATH="$(brew --prefix)/lib:$(brew --prefix openssl@3)/lib"
```

#### Build

```sh
cargo build --release
# The compiler binary will be at target/release/alignc
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
