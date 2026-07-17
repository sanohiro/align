# Align

> 🌐 **English** · [Japanese](./README.ja.md)

Align is an AOT-compiled, data-oriented programming language. It aligns four readers at once — the **human** who writes the code, the **AI** that generates it, the **compiler** that optimizes it, and the **hardware** that runs it. By combining a strict "nothing hidden" policy, a single unified model for errors and ownership, and a core built around data-oriented arrays and slices, Align guarantees predictable performance and cache-friendly, fused loops out of ordinary code.

## Platforms

Currently supported platforms:
- **Linux x86-64**
- **macOS Apple Silicon (aarch64)**
- *Windows is not supported.*

## Installation

Align is currently distributed as source only. To build the compiler from source, you need:

- **Rust 1.96+**
- **LLVM 22** (Must be on your `PATH` as `llvm-config-22`)
- **clang-22** (Used as the C compiler/linker)

On Ubuntu 24.04, you can install the LLVM dependencies via the official repository (`apt.llvm.org`):
```sh
sudo apt install llvm-22 llvm-22-dev clang-22
```

Build the compiler:
```sh
cargo build --release
# The compiler binary will be at target/release/alignc
```

## Hello World

Create a file named `hello.align`:

```align
fn main() -> i32 {
    print("hello, align\n")
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
