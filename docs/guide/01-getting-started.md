# Getting started

> 🌐 **English** · [Japanese](./ja/01-getting-started.md)

Align is pre-release: there are no binary downloads yet, so you build the compiler from source once, and use it from the build directory.

## Building the compiler

You need **Rust 1.96+** and **LLVM 22**. On Debian/Ubuntu (via apt.llvm.org):

```text
apt install llvm-22 llvm-22-dev
git clone https://github.com/sanohiro/align
cd align
cargo build
```

The compiler is now at `./target/debug/alignc`. It is not on `PATH`; either call it by path or add an alias. (A `--release` build produces `./target/release/alignc` — the compiler itself runs faster, the generated code is the same.)

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
alignc check     file.align          type-check only, print diagnostics
alignc build     file.align          produce a native executable (./file)
alignc run       file.align [args…]  build + run; trailing args go to main(args)
alignc fmt       file.align [--write] format (prints to stdout; --write rewrites in place)
alignc emit-mir  file.align          dump the mid-level IR (for the curious)
alignc emit-llvm file.align          dump LLVM IR (see exactly what your code became)
alignc emit-obj  file.align [out.o]  object file only, no link
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
