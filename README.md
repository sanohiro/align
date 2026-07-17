# Align

> 🌐 **English** · [Japanese](./README.ja.md)

Align is an AOT-compiled, data-oriented programming language. It aligns four readers at once — the **human** who writes the code, the **AI** that generates it, the **compiler** that optimizes it, and the **hardware** that runs it. By combining a strict "nothing hidden" policy, a single unified model for errors and ownership, and a core built around data-oriented arrays and slices, Align guarantees predictable performance and cache-friendly, fused loops out of ordinary code.

## Platforms

Currently supported platforms:
- **Linux x86-64**
- **macOS Apple Silicon (aarch64)**
- *Windows is not supported.*

## Installation

Align is currently distributed as source only. Building the compiler needs **Rust 1.96+** and **LLVM 22** (with a matching **clang** as the C compiler/linker).

### Linux (Ubuntu 24.04)

Install the LLVM toolchain from the official `apt.llvm.org` repository; `llvm-config-22` must be on your `PATH`:
```sh
sudo apt install llvm-22 llvm-22-dev clang-22
```

### macOS (Apple Silicon)

Install the dependencies with Homebrew:
```sh
brew install llvm openssl@3 zstd
```
The `llvm` formula currently provides LLVM 22; if Homebrew has since moved it past 22, install the versioned `llvm@22` formula instead. Homebrew's LLVM is keg-only (its `llvm-config` is not on your `PATH`), so point the build at it and add the linker search paths for the runtime's native libraries (`zstd`, `openssl@3`). Add these to your shell profile, or prefix each `cargo` / `alignc` command with them (the same `LIBRARY_PATH` is needed when running an `alignc`-built program that links those libraries):
```sh
export LLVM_SYS_221_PREFIX="$(brew --prefix llvm)"
export LIBRARY_PATH="$(brew --prefix)/lib:$(brew --prefix openssl@3)/lib"
```

### Build

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
./target/release/alignc run hello.align
```

## Learn Align

Start with the guide — a hands-on introduction to thinking and writing in Align:

**[Tutorial (English)](docs/guide/README.md)** · **[Tutorial (Japanese)](docs/guide/ja/README.md)**

Prefer drills? **[The Little Aligner](docs/little-aligner/README.md)** ([Japanese](docs/little-aligner/ja/README.md)) teaches the same idioms as a Q&A workbook, in the style of *The Little Schemer*.

## Layout

- `draft.md` — authoritative language specification
- `docs/guide/` — hands-on tutorial, 19 chapters (`00`–`18`, English + Japanese)
- `docs/little-aligner/` — Q&A drill workbook in the style of *The Little Schemer* (English + Japanese)
- `docs/` — design rationale, history, non-goals, open questions
- `docs/impl/` — compiler implementation plan + std module design specs
- `editors/` — Vim / Emacs / VS Code support (syntax, snippets)
- `crates/` — the `alignc` compiler workspace

## License

Dual-licensed under either of:
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
