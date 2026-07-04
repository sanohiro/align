# Generics and modules

> 🌐 **English** · [Japanese](./ja/09-generics-and-modules.md)

Two chapters' worth of machinery in most languages, kept deliberately small in Align: generics with three built-in bounds and full inference, and modules that are just files.

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

Type parameters are **always inferred from the arguments** — there is no turbofish, no explicit instantiation syntax to learn or to read. Generics are monomorphized: each used instantiation compiles to its own specialized code, exactly as if you had written it by hand. Zero runtime dispatch.

## The bounds: `Num` ⊃ `Ord` ⊃ `Eq`

An unbounded `T` is opaque — you can move it, store it, return it, and nothing else. Capabilities come from exactly three built-in bounds:

- `T: Eq` — `==`, `!=`
- `T: Ord` — comparisons (implies `Eq`)
- `T: Num` — arithmetic (implies `Ord`)

Use an operation the bound doesn't grant and the *definition* fails to compile — not the call site, later, in someone else's build.

That is the entire constraint system, on purpose. **There are no user-defined traits or interfaces** (a decision, not a gap): trait hierarchies are where languages grow a second, Turing-complete type-level dialect that humans skim and AIs hallucinate. Align's bet is that three bounds plus concrete types cover the real generic code a data-oriented program contains, and everything else is better said with a plain function.

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

`Option<T>` and `Result<T, E>` are exactly this mechanism, shipped with the language. Current limits, honestly labeled: a generic *function* over a generic *struct* (`fn first<T>(p: Pair<T>) -> T`) is implementation in progress, and constructing a payload-less variant alone (`Opt.Empty`) needs context to pin `T`.

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

`alignc run main.align` finds `geom.align` next to the entry file; `import util.math` maps to `util/math.align`. The qualification rule is absolute — an imported type is `geom.Point`, never a bare `Point`; an imported sum-type variant is `geom.Color.Red`. Any name in any file tells you exactly where it came from, with no import-list archaeology. There is no aliasing (`import x as y`) and no glob — one way to refer to a thing.

The same `import` keyword also switches on the built-in capability modules — `import std.fs`, `import core.json` — which is how a file declares, at the top, which parts of the outside world it touches. A file with no `std` imports provably does no I/O; chapter [13](13-std-os.md) builds on that.

## Program shape

A small program is one file. When it grows, the seams are data boundaries: the record types and the functions over them move to a module (`records.align`), the I/O edge stays in `main.align`, and `pub` marks the deliberate surface. Because references are qualified and visibility is explicit, a module's true interface is greppable: `pub` lines are the contract.
