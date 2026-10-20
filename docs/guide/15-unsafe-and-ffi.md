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

The raw-pointer operations are `null`, `alloc`, `free`, `load`, `store` and `offset`. A `resource` type also has representation operations such as `resource.from_raw`, `.into_raw` and `.view_from_raw`, restricted to that type's privileged `unsafe` code. `raw.null()` supplies a native-ABI null sentinel. A `raw` is a Copy value that may be held and passed in safe code; operating on it requires `unsafe`.

`load` and `store` admit primitive scalars, `raw` pointers, and eligible `layout(C)` structs. A
native wrapper can therefore keep a C handle in a package-owned state block without casting the
address to an integer: `raw.store(state, 0, handle)` and `handle: raw := raw.load(state, 0)`. The
programmer remains responsible for the slot size, pointer validity, and effective type.

Arena escape checks, move checking and bounds checks on ordinary types remain active inside `unsafe`. Purity inference (chapter [10](10-closures-and-parallelism.md)) classifies functions containing `unsafe` as impure, so they cannot be passed to `par_map`.

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

Fields may be integers, floats, `raw` pointers, or positive-length fixed arrays
of C integer/float scalars, such as `dims: [i64; 8]`. Numeric arrays are inline
Copy data, including in raw storage and supported record-value calls; no child
allocation is added. Zero-length C fields, bool/char/raw arrays, nested arrays/records
and owning/view elements are excluded. Naked arrays cannot cross extern calls.
A pointer-bearing record is Copy;
copying, replacing or returning it never copies/frees the pointee or extends its
lifetime. Pointer validity and dereferencing remain your responsibility inside
`unsafe`. Direct raw fields in ordinary structs and generic C records are not supported.

`layout(C)` structs can cross through a `raw` pointer or by value on supported
ABIs. **LP64 x86-64 Linux (GNU/musl), Apple Silicon and Linux ARM64** support
large records and exhausted argument registers. On x86-64, the complete argument
moves to the stack when its register classes do not all fit. The compiler uses
the platform ABI, including aligned temporary stack
copies for large arguments and storage for large results. Native parameter writes
leave the original record unchanged; pointers inside it still refer to their
original pointees. These temporaries add no heap allocation and their addresses
must not escape the native call. Other targets remain pointer-only. See
[plan138](../impl/138-aarch64-c-record-values.md) and
[plan139](../impl/139-sysv-memory-c-record-values.md) for the exact platform contracts.

C-owned memory returns as `raw` because a C pointer carries no length. Use `raw.load` to read values, or obtain and validate the length before constructing a view.

## The discipline

Keep native integration in a small, reviewable module. It owns the `extern` declarations and `unsafe` blocks, passes views with their lengths, handles and frees `raw` pointers, and converts errors to `Result`. Its public API must uphold Align's safety guarantees for callers. Searching for `unsafe` helps locate code that needs manual review, including the assumptions made by its wrappers.
