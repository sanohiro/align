# Getting started

> 🌐 **English** · [Japanese](./ja/01-getting-started.md)

Align is an early 0.x project and does not yet promise backward compatibility. Tagged releases are packaged for macOS Apple Silicon and Ubuntu 24.04 (x86_64 and ARM64); the commands below install the latest published release. Build from source when you need the current repository state.

## Installing a packaged build

On macOS Apple Silicon:

```text
brew tap sanohiro/align
brew install align
```

On Ubuntu 24.04:

```text
curl -fsSL https://sanohiro.github.io/align/install.sh | sudo sh
sudo apt install alignc
```

The setup script adds both the signed Align repository and the official LLVM 22 repository. It does not install `alignc` until you run the second command. Release archives and `.deb` files are also available directly from the corresponding GitHub release.

## Building the compiler

You need **Rust 1.96+** and **LLVM 22**. On Debian/Ubuntu (via apt.llvm.org):

```text
apt install llvm-22 llvm-22-dev clang-22 libclang-rt-22-dev libssl-dev zlib1g-dev libzstd-dev
git clone https://github.com/sanohiro/align
cd align
cargo build
```

The compiler is now at `./target/debug/alignc`. It is not on `PATH`; either call it by path or add an alias. (A `--release` build produces `./target/release/alignc` — the compiler itself runs faster, the generated code is the same.) `alignc` dynamically uses LLVM 22 and invokes `cc` to link the programs it builds, so installing the executable alone does not remove those native toolchain requirements.

Ubuntu 24.04's default OpenSSL 3.0 is sufficient for TLS, hashes, HMAC, HKDF, and AEAD. The `crypto.argon2id` provider was added in OpenSSL 3.2; install a newer OpenSSL when you need that operation, otherwise it reports an engine error at runtime.

## Hello, Align

Save this as `hello.align`:

```align
fn main() -> i32 {
    print("hello, align")
    return 0
}
```

Run it:

```text
$ alignc run hello.align
hello, align
```

`alignc run` compiles to a native executable and runs it in one step. `main` returning `i32` makes the return value the process exit code. `print` writes any primitive value — integers, floats, `bool`, `char`, strings — followed by a newline.

A `main` that can fail returns `Result` instead; that form (and what happens to the exit code) is chapter [04](04-errors.md).

## The subcommands

```text
alignc check          file.align          type-check and lint
alignc check-per-unit file.align          check each imported unit through its interface
alignc emit-interface file.align          print public interfaces and their hashes
alignc build          file.align          produce a native executable (./file)
alignc run            file.align [args…]  build + run; trailing args go to main(args)
alignc fmt            file.align [--write] format (prints; --write rewrites in place)
alignc emit-mir       file.align          dump the mid-level IR
alignc emit-llvm      file.align          dump raw or optimized LLVM IR
alignc emit-obj       file.align [out.o]  object file only, no link
alignc explain-opt    file.align          explain optimizer decisions at source lines
alignc size           file.align          build and report the executable's size
alignc cache clear                        clear the resolved codegen cache
alignc --version                          print the compiler version
```

The everyday loop is `check` while editing, `run` to try it. `emit-llvm` is worth knowing early: Align's design promises that ordinary code lowers to tight machine code, and `emit-llvm` is how you check that promise yourself.

## Reading a compile error

Align's compiler is strict — no null, exhaustive `match`, unhandled `Result` is an error, moved values can't be reused. The diagnostics tell you which rule fired and where. A first program trips over two of them most often:

```align
fn main() -> i32 {
    x := 1
    x = 2          // error: x is not `mut`
    return 0
}
```

Mutation must be declared (`mut x := 1`). And:

```align
import std.fs

fn main() -> i32 {
    fs.write_file("out.txt", "hi")   // error: unhandled Result
    return 0
}
```

Anything that can fail returns a `Result`, and silently discarding one is a compile error, not a lint you can ignore. Chapter [04](04-errors.md) shows the three ways to handle it.

## Where to go next

Chapter [02](02-language-basics.md) covers the expression-oriented core in one sitting. If you prefer drills over prose, [The Little Aligner](../little-aligner/README.md) starts from zero as well.
