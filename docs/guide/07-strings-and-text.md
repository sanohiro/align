# Strings and text

> 🌐 **English** · [Japanese](./ja/07-strings-and-text.md)

Text in Align follows the memory model of chapter [05](05-memory.md) exactly — there are two string types because there are two lifetimes, and every allocation is visible. Once you see the pattern here, you have seen it for every resource in the language.

## `str` and `string`

- **`str`** — a borrowed, immutable **view**: a pointer and a byte length. String literals are `str`. Copying one is free; it carries the region of the data it points into.
- **`string`** — an **owned** heap buffer. A Move type: assignment transfers ownership, the buffer is dropped when its owner dies.

You get a `string` from `.clone()` (deep copy) or from a `builder` (below). And an owned `string` **auto-borrows** wherever a `str` is expected — passing it to a `str` parameter costs nothing and consumes nothing:

```align
fn greet(who: str) -> i64 = who.len()

fn main() -> i32 {
    owned := "align".clone()    // string
    print(greet(owned))         // borrows — owned is still alive
    print(owned.len())          // 5
    return 0
}
```

The default in signatures is `str`: take views, return views when the data already exists, and return `string` only when the function genuinely creates new text.

## Literals, escapes, bytes

Double quotes, with `\n`, `\t`, `\"` escapes. `char` literals use single quotes (`'A'`, `'あ'`) and hold one Unicode scalar. Strings are UTF-8, and `.len()` is the **byte** length:

```align
print("あ".len())    // 3 — UTF-8 bytes, not characters
```

## The methods

```align
fn main() -> i32 {
    s := "hello, align"
    print(s.contains("align"))      // true
    print(s.starts_with("hello"))   // true
    print(s.ends_with("!"))         // false
    t := "  padded  "
    print(t.trim())                 // "padded" — a zero-copy sub-view
    return 0
}
```

`len` / `contains` / `starts_with` / `ends_with` / `find` / `rfind` / `eq_ignore_ascii_case` / `trim` / `trim_start` / `trim_end` / `clone` — that is the current set. All byte-oriented, and the searching ones are SIMD under the hood (`contains` is a vectorized scan, not a naive loop). `find`/`rfind` return `Option<i64>` — a byte index, or `None` — and pair with range slicing, which works on strings too:

```align
fn main() -> i32 {
    path := "align/docs/guide.md"
    j := path.rfind("/") else -1
    print(path[j + 1..path.len()])      // guide.md — a zero-copy view
    return 0
}
```

(There is no `path[i]` single-byte access — a byte index is for slicing, not for walking.) A `split` does not exist yet (implementation in progress); today `find`/`rfind` + `[a..b]` compose the manual split, or you write a real parser.

## Concatenation: allowed where the allocation has a home

`a + b` on strings allocates — so Align insists the allocation have a visible lifetime. Inside an `arena`, concatenation is the natural way to build a temporary:

```align
fn shout(name: str) -> string {
    arena {
        s := "hey, " + name + "!"   // arena-backed temporaries
        return s.clone()            // copy the survivor out
    }
}

fn main() -> i32 {
    print(shout("align"))           // hey, align!
    return 0
}
```

But `+` inside a **pipeline lambda** is a compile error: `xs.reduce("", fn acc, x { acc + x })` would allocate per element with no owner — a hidden quadratic leak in one innocent line. The compiler rejects it and the fix is the builder, which is the next section. This is "nothing hidden" applied to text: every string allocation belongs to an arena, an owner, or a builder — never to the middle of a fused loop.

## The builder

For assembling text piece by piece — the append-in-a-loop shape — use `builder`:

```align
fn label(name: str, score: i64) -> string {
    b := builder()          // or builder(64) with a capacity hint
    b.write(name)
    b.write(": ")
    b.write_int(score)
    return b.to_string()    // finish → owned string
}

fn main() -> i32 {
    print(label("ada", 95))     // ada: 95
    return 0
}
```

One growable buffer, amortized appends, one final `string`. `write` takes a `str` (or an owned `string`); `write_int` formats an integer straight into the buffer with no temporary. The compiler even fuses adjacent writes (`"lit"` + int + `"lit"` becomes a single runtime call), so the builder is not just the safe way — it is the fast way.

## Template strings

For one-shot formatting, `template` interpolates full expressions:

```align
fn main() -> i32 {
    name := "align"
    score := 40
    print(template "Hello {name}, score={score + 2}")   // Hello align, score=42
    return 0
}
```

Templates cover the `print`-a-composed-line case; the builder covers the build-a-document case; there is no printf-style format-string mini-language. (Inside pipeline lambdas, `template` is rejected for the same hidden-allocation reason as `+` — format the results after the pipeline, not per element inside it.)

## Choosing, at a glance

| you want | reach for |
|---|---|
| pass text around, inspect it | `str` (views, free) |
| keep text beyond its source's lifetime | `.clone()` → `string` |
| glue a few pieces once, in a scope | `+` inside an `arena` |
| assemble text incrementally / in bulk | `builder` |
| one formatted line | `template "..."` |
