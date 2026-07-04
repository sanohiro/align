# The edges: unsafe and C FFI

> 🌐 **English** · [Japanese](./ja/15-unsafe-and-ffi.md)

Every guarantee so far — no dangling views, no double-free, no data races in `par_map` — holds because the compiler can see everything. At the edge of the world (a C library, a hand-managed buffer) it can't. Align's answer is the standard one, kept small: an `unsafe {}` block marks exactly where the guarantees are yours to uphold, and nothing outside one can break them.

## `unsafe {}` and `raw.*`

`raw` is a bare pointer type; the five `raw.*` operations are the only way to touch one, and they are legal **only** inside `unsafe`:

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

`alloc` / `free` / `load` / `store` / `offset` — that is the entire unsafe vocabulary. No pointer arithmetic operators, no casts-through-pointers dialect: five named operations you can grep for. *Holding* a `raw` is safe (it's a Copy value; pass it around freely); only operating on one needs the block.

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

Declare the C signature; call it inside `unsafe` (the compiler can't check what C does — the block says *you* did). libc and libm resolve automatically; anything else names its library with `link`:

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

**The one FFI rule to tattoo somewhere:** Align strings are *not* NUL-terminated. Length-taking APIs (`write`, `memcmp`, `memcpy`) are safe; `strlen`/`printf("%s")` will read past the end. When C wants a struct, pin the layout with `layout(C)` — declaration order, C alignment rules, no field reordering (without it, Align reorders fields for density):

```align
layout(C) Point { x: i32, y: i32 }      // matches `struct { int32_t x, y; }`
```

`layout(C)` structs cross by pointer (through `raw`) or **by value** (SysV x86-64 ABI, ≤16-byte register-class structs — matching clang exactly; larger-by-value is implementation in progress). C-owned memory comes back as `raw` — C pointers carry no length, so nothing pretends to be a view — and you read it with `raw.load` or wrap it in a length you got some honest way.

## The discipline

Keep the edge thin and audited: one module owns the `extern` declarations and the `unsafe` blocks, converts at the boundary (views + lengths in, `raw` handled and freed, `Result` out), and exports a fully safe API. Callers of that module get the same guarantees as pure Align — because outside an `unsafe` block, nothing *can* be unsound. `grep unsafe` is the audit surface, and in a healthy program it is a page, not a codebase.
