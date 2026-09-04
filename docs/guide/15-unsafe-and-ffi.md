# The edges: unsafe and C FFI

> 🌐 **English** · [Japanese](./ja/15-unsafe-and-ffi.md)

The compiler checks view lifetimes, moves, and purity to prevent dangling views, double-free, and data races in `par_map`. It cannot inspect a C library or verify manual pointer operations. An `unsafe {}` block marks the code where you must uphold those guarantees yourself. A safe wrapper must do so for every input its callers can supply.

## `unsafe {}` and `raw.*`

`raw` is a bare pointer type; the six `raw.*` operations are the only way to touch one, and they are legal **only** inside `unsafe`:

```align
fn main() -> i32 {
    unsafe {
        p := raw.alloc(16)          // 16 raw bytes
        raw.store(p, 0, 42)         // write an i64 at byte offset 0
        raw.store(p, 8, 99)
        a: i64 := raw.load(p, 0)    // read back — type from the annotation
        b: i64 := raw.load(p, 8)
        raw.free(p)                 // yours to free — a raw is never dropped
        print(a + b)                // 141
        return 0
    }
}
```

`null` / `alloc` / `free` / `load` / `store` / `offset` — that is the entire unsafe vocabulary. No pointer arithmetic operators, no casts-through-pointers dialect: six named operations you can grep for. (A `resource` type's own internal code adds a few representation-privilege verbs — `resource.from_raw`, `.into_raw`, `.view_from_raw` — usable only inside that type's `unsafe` code, never a bypass in ordinary `unsafe`.) `raw.null()` is the explicit native-ABI null sentinel; it does not add null to ordinary Align values. *Holding* a `raw` is safe (it's a Copy value; pass it around freely); only operating on one needs the block.

`load` and `store` admit primitive scalars, `raw` pointers, and eligible `layout(C)` structs. A
native wrapper can therefore keep a C handle in a package-owned state block without casting the
address to an integer: `raw.store(state, 0, handle)` and `handle: raw := raw.load(state, 0)`. The
programmer remains responsible for the slot size, pointer validity, and effective type.

What `unsafe` does **not** do: it is a marker, not a mode switch. Arena escape checks, move checking, bounds checks on ordinary types — all still apply inside. And purity inference (chapter [10](10-closures-and-parallelism.md)) marks any function containing `unsafe` as impure, so raw-memory code can never ride into `par_map`.

## `extern "C"` — declaring the outside world

```align
extern "C" {
    fn abs(x: i32) -> i32
    fn labs(x: i64) -> i64
}

fn main() -> i32 {
    unsafe {
        print(abs(-7))      // 7 — a real libc call
        print(labs(-40))    // 40
        return 0
    }
}
```

Declare the C signature and call it inside `unsafe`. You must check that the signature is correct and the call satisfies the C API's requirements. libc and libm resolve automatically; name other libraries with `link`:

```align
extern "C" link("m") {
    fn sqrt(x: f64) -> f64
    fn cbrt(x: f64) -> f64
}
```

## Passing data across

Scalars map directly (`i32`↔`int32_t`, `f64`↔`double`). An Align view (`str`, `slice<T>`, `bytes`) lowers to its **data pointer** — pass the length yourself:

```align
extern "C" fn write(fd: i32, buf: str, count: i64) -> i64

fn main() -> i32 {
    msg := "written by libc\n"
    unsafe {
        n := write(1, msg, msg.len())   // fd 1 = stdout
        print(n)                        // 16
        return 0
    }
}
```

**Align strings are not NUL-terminated.** Use C APIs that accept an explicit length, such as `write`, `memcmp`, or `memcpy`, and pass the correct length. `strlen` and `printf("%s")` expect NUL termination and may read past the view. When C expects a struct, use `layout(C)` to preserve declaration order and C alignment rules; without it, Align may reorder fields for density:

```align
layout(C) Point { x: i32, y: i32 }      // matches `struct { int32_t x, y; }`
```

`layout(C)` structs can cross through a `raw` pointer. **By-value structs are supported only on x86-64 Linux using the SysV ABI**, and only when the complete struct fits the available argument or return registers (at most 16 bytes). Larger structs and signatures that exhaust the argument registers are rejected. On Apple Silicon and other targets, pass the struct by pointer; `layout(C)` alone does not enable by-value calls there.

C-owned memory returns as `raw` because a C pointer carries no length. Use `raw.load` to read values, or obtain and validate the length before constructing a view.

## The discipline

Keep native integration in a small, reviewable module. It owns the `extern` declarations and `unsafe` blocks, passes views with their lengths, handles and frees `raw` pointers, and converts errors to `Result`. Its public API must uphold Align's safety guarantees for callers. Searching for `unsafe` helps locate code that needs manual review, including the assumptions made by its wrappers.
