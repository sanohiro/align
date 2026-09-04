# Generics and modules

> 🌐 **English** · [Japanese](./ja/09-generics-and-modules.md)

This chapter covers generic functions and types, the built-in constraints on type parameters, and file-based modules.

## Generic functions

```align
fn max<T: Ord>(a: T, b: T) -> T = if a > b { a } else { b }
fn add<T: Num>(a: T, b: T) -> T = a + b
fn same<T: Eq>(a: T, b: T) -> bool = a == b
fn unwrap_or<T>(o: Option<T>, fallback: T) -> T = o else fallback

fn main() -> i32 {
    print(max(7, 12))       // 12   — T = i64, inferred
    print(max(1.5, 0.5))    // 1.5  — T = f64
    print(add(40, 2))       // 42
    print(same("a", "a"))   // true
    print(unwrap_or(Some(5), 0))    // 5
    return 0
}
```

Type parameters are inferred from the arguments or the expected result type. If the arguments do not determine the type, add a binding annotation, as with `json.decode` in chapter [08](08-json.md). Calls do not take written type arguments such as `f<T>(x)`. Generics are monomorphized: each used instantiation compiles to specialized code without runtime dispatch.

## The bounds: `Num` ⊃ `Ord` ⊃ `Eq`

An unbounded `T` is opaque — you can move it, store it, return it, and nothing else. Capabilities come from three built-in bounds:

- `T: Eq` — `==`, `!=`
- `T: Ord` — comparisons (implies `Eq`)
- `T: Num` — arithmetic (implies `Ord`)

Use an operation the bound doesn't grant and the *definition* fails to compile — not the call site, later, in someone else's build.

These three bounds describe the operations available on a type parameter. Structural bounds serve a separate purpose: for example, `RegionPlain` admits recursively plain data for region-backed construction, without granting arithmetic or equality operations. **User-defined traits and interfaces are not supported.** Other behavior is expressed with concrete types and functions.

## Generic types

Structs and sum types take parameters the same way, inferred from construction:

```align
Pair<T> { a: T, b: T }

Opt<T> { Has(T), Empty }

fn sum_ints(p: Pair<i64>) -> i64 = p.a + p.b

fn main() -> i32 {
    p := Pair { a: 40, b: 2 }       // Pair<i64>, inferred
    q := Pair { a: 1.5, b: 2.5 }    // Pair<f64>
    print(sum_ints(p))              // 42
    print(q.a + q.b)                // 4.0
    o := Opt.Has(9)                 // Opt<i64>, inferred from the payload
    v := match o {
        Has(n) => n,
        Empty  => 0,
    }
    print(v)                        // 9
    return 0
}
```

`Option<T>` and `Result<T, E>` use this same mechanism. A generic function can take a generic struct (`fn first<T>(p: Pair<T>) -> T`). Type parameters can also occur inside `array`, `slice`, or another top-level generic type. Constructing a payload-less variant such as `Opt.Empty` needs surrounding type information to determine `T`.

## Modules are files

One file = one module; the `module` name must match the filename. `import` brings a sibling file in; everything is private unless marked `pub`; cross-module references are always qualified. No headers, no manifest, no search-path ritual.

```align
// geom.align
module geom

pub Point { x: i64, y: i64 }
pub SCALE: i64 := 3
pub fn area(p: Point) -> i64 = p.x * p.y

fn hidden(x: i64) -> i64 = x        // private: invisible to importers
```

```align
// main.align
module main

import geom

fn main() -> i32 {
    p := geom.Point { x: 4, y: 5 }
    print(geom.area(p) * geom.SCALE)    // 60
    return 0
}
```

`alignc run main.align` finds `geom.align` next to the entry file; `import util.math` maps to `util/math.align`. Imported names must be qualified: write `geom.Point`, and `geom.Color.Red` for a sum-type variant. The qualifier identifies the module at the use site. Import aliases (`import x as y`) and glob imports are not supported.

The same `import` keyword enables built-in modules such as `std.fs` and `core.json`. Imports show which APIs a file uses directly; they do not prove that it performs no I/O. For example, `print` needs no import, and an imported application or package function may perform I/O. The compiler infers effects through function calls when checking purity (chapter [10](10-closures-and-parallelism.md)).

## Program shape

A small program is one file. When it grows, the seams are data boundaries: the record types and the functions over them move to a module (`records.align`), the I/O edge stays in `main.align`, and `pub` marks the deliberate surface. Because references are qualified and visibility is explicit, a module's true interface is greppable: `pub` lines are the contract.
