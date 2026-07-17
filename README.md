# Align

> 🌐 **English** · [Japanese](./README.ja.md)

Align is an AOT-compiled, data-oriented programming language. It aligns four readers at once — the **human** who writes the code, the **AI** that generates it, the **compiler** that optimizes it, and the **hardware** that runs it.

> Less code. Predictable performance. Nothing hidden.

This is an early-stage project. The authoritative design lives in `draft.md` + `docs/`; the compiler (`alignc`) is being implemented in Rust under `crates/`.

## Why Align

- **Data-oriented core.** Arrays and slices are the center of the language. You write `prices.map(with_tax).where(in_stock).sum()` and the compiler fuses it into one loop with no intermediate arrays — cache- and SIMD-friendly, because ordinary code lowers well.
- **Nothing hidden.** Allocation, errors, effects, and parallelism are always visible in the source. No hidden copies, no exceptions, no threads spawned behind your back.
- **One way to do things.** One error model (`Result` + `?`), one optional model (`Option`, no null), one ownership model (value / `arena` / heap), one parallel model (`map` / `reduce` / `task_group`).
- **No manual memory, no GC.** Ownership is a property of the type; lifetimes are inferred as regions, never written. You choose *where* data lives — a value, an `arena`, or the heap — and the compiler inserts the cleanup.

## A taste

```align
Item { price: f64, active: bool }

fn with_tax(p: f64) -> f64 = p * 1.08

fn main() -> i32 {
    items := [
        Item { price: 100.0, active: true },
        Item { price: 50.0,  active: false },
        Item { price: 200.0, active: true },
    ]
    total := items.where(.active).price.map(with_tax).sum()  // one fused loop, no temporaries
    print(total)                                             // 324.0
    return 0
}
```

## Learn Align

New to the language? Start with the guide — a hands-on introduction to thinking and writing in Align:

**[Tutorial (English)](docs/guide/README.md)** · **[Tutorial (Japanese)](docs/guide/ja/README.md)**

Prefer drills? **[The Little Aligner](docs/little-aligner/README.md)** ([Japanese](docs/little-aligner/ja/README.md)) teaches the same idioms as a Q&A workbook, in the style of *The Little Schemer*.

## Build & run

```sh
cargo build
cargo test
cargo run --bin alignc -- run examples/arena.align     # arena + heap box; exits 42
cargo run --bin alignc -- run examples/pipeline.align  # fused map/where/sum; exits 24
```

The everyday commands are `check`, `fmt`, `build`, and `run`. Inspection and build-control commands are `check-per-unit`, `emit-interface`, `emit-mir`, `emit-llvm`, `emit-obj`, `explain-opt`, `size`, and `cache clear`; `alignc --version` reports the compiler version. Multi-file codegen is parallel and cached by default, with explicit `--rt-lto`, `--thin-lto`, and instrumented-PGO modes for production builds.

**Requirements for a source build:** Rust 1.96+, LLVM 22 (`llvm-config-22` on `PATH`), and a C compiler (`cc`) for linking. Programs that use compression or crypto/HTTP also need the zlib, zstd, and OpenSSL development libraries. Most crypto operations work with OpenSSL 3.0; `crypto.argon2id` requires OpenSSL 3.2+ and returns an engine error when that provider is unavailable.

## Install a release

The commands below become available after the first distribution release and repository setup. Until then, build from source.

macOS Apple Silicon (Homebrew):

```sh
brew tap sanohiro/align
brew install align
```

Ubuntu 24.04, x86_64 or ARM64 (signed apt repository):

```sh
curl -fsSL https://sanohiro.github.io/align/install.sh | sudo sh
sudo apt install alignc
```

Release archives and `.deb` files are also attached to each GitHub release. A raw archive must keep `alignc` and its matching `libalign_runtime.a` together. `alignc` dynamically uses LLVM 22 and invokes the system C linker, so these are native packages with declared toolchain dependencies rather than fully static binaries.

## Status

Early-stage, but the pipeline runs end to end (`lexer → parser → sema → MIR → LLVM → native`): functions and control flow, structs, the full primitive set, `Option`/`Result` with `?`, `arena`/`box` with move & escape checking, fused array pipelines, strings + `json`, SIMD (`vecN`/`soa`/`group_by`), `par_map`/`task_group` on real threads, `unsafe`/FFI, and a growing std library (`io`/`fs`/`path`/`env`/`time`/`encoding`/`rand`/`cli`/`net`/`process`/`compress`/`crypto`/`http`). See `docs/impl/07-roadmap.md` for the milestone detail.

## Performance & portability

The default build uses a **safe, portable per-architecture baseline** (`x86-64-v2` on amd64, `armv8-a`/NEON on arm64), so one binary runs across a mixed cloud fleet. Aggressive targets are **opt-in, never the default** — `--target-cpu native` for a host-specific build, or the portable AVX2/FMA tier `x86-64-v3`. Wide SIMD across a varied fleet is meant to come from runtime CPU-feature dispatch in the library, not a raised baseline. See `draft.md` §3.4 and `docs/open-questions.md` ("Build targets & portability").

## Layout

- `draft.md` — authoritative language specification
- `docs/guide/` — hands-on tutorial, 19 chapters (`00`–`18`, English + Japanese)
- `docs/little-aligner/` — Q&A drill workbook in the style of *The Little Schemer* (English + Japanese)
- `docs/` — design rationale, history, non-goals, open questions
- `docs/impl/` — compiler implementation plan + std module design specs
- `editors/` — Vim / Emacs / VS Code support (syntax, snippets)
- `crates/` — the `alignc` compiler workspace

## License

MIT
