# std: encoding, rand, cli

> 🌐 **English** · [Japanese](./ja/14-std-encoding-rand-cli.md)

The second wave of `std`: converting bytes at the boundary, random numbers, and command-line parsing. Same three rules as chapter [13](13-std-os.md) — explicit imports, `Result` + the one errno table, Move where a resource is owned.

## `std.encoding`

Base64 (standard and URL-safe), hex, and UTF-8 validation:

```align
import std.encoding

pub fn main() -> Result<(), Error> {
    print(encoding.base64_encode("foobar"))     // Zm9vYmFy
    dec := encoding.base64_decode("Zm9vYmFy")?  // Result<buffer, Error>
    print(encoding.hex_encode(dec.bytes()))     // 666f6f626172
    print(encoding.utf8_valid(dec.bytes()))     // true
    match encoding.hex_decode("zz") {
        Ok(_)  => print("ok"),
        Err(_) => print("bad hex"),             // invalid input → Error.Invalid
    }
    return Ok(())
}
```

The types state the trust boundary. **Encoding** can't fail → returns `string` directly. **Decoding** is parsing untrusted input → returns `Result<buffer, Error>`, and the payload is a `buffer` — raw bytes — because decoded data carries no UTF-8 guarantee; run `utf8_valid` (or hand it to something binary-safe) before treating it as text. `base64url_*` uses the URL-safe alphabet without padding, and hex decoding accepts both cases.

## `std.rand`

```align
import std.rand

pub fn main() -> i32 {
    mut a := rand.seed_with(42)     // deterministic — same seed, same sequence
    mut b := rand.seed_with(42)
    print(a.next() == b.next())     // true — reproducible by construction

    mut r := rand.seed_with(123)    // rand.seed() for an OS-seeded generator
    d6 := r.range(1, 7)             // uniform in [1, 7) — a die roll

    mut xs := [10, 20, 30, 40, 50][0..5]
    r.shuffle(xs)                   // in-place permutation
    print(xs.sum())                 // 150 — same elements, new order

    hand := r.sample([1, 2, 3, 4, 5, 6][0..6], 3)   // 3 distinct picks
    print(hand.count())             // 3
    return 0
}
```

The design bets:

- **An `rng` is a value**, not a hidden global. `rand.seed()` asks the OS for entropy; `rand.seed_with(s)` is deterministic and portable — tests and simulations reproduce exactly. Every method needs a `mut` receiver, because advancing the state *is* mutation and Align doesn't hide mutation.
- Since drawing a number is visibly impure, an rng-using closure is **rejected by `par_map`** at compile time — the classic non-reproducible-parallel-simulation bug is unrepresentable. (Per-task generators via `task_group`, or pre-generate a column of randoms and pipeline over it.)
- `range` is half-open `[lo, hi)` and bias-free; `range(1, 7)` is a die. Nonsense arguments (`lo >= hi`, `sample` with `k > len`) abort loudly rather than return something plausible.

## `std.cli` — implementation in progress

Command-line parsing is designed but **not yet implemented** — today you read `main(args: array<str>)` by hand (chapters [04](04-errors.md), [13](13-std-os.md)), which honestly covers the small-tool case:

```align
pub fn main(args: array<str>) -> Result<(), Error> {
    if args.count() < 2 {
        print("usage: tool <input>")
        return Err(Error.Invalid)
    }
    input := args[1]
    // ...
    return Ok(())
}
```

The designed shape (for orientation; check the spec when it ships): declare flags on a `cli.command`, parse `args` into a typed result — unknown or malformed flags are `Error.Invalid`, and reading a flag you never declared is a hard programming error, not a silent default. No derive macros, no attribute DSL: flags are declared in ordinary code, One-way style.

---

Also designed at full depth and queued behind `cli` (all implementation in progress): `std.net` (TCP), `std.http` (a plaintext-v1 client), `std.process` (spawn/exec), `std.compress` (deflate/gzip), and `std.crypto` (hashes/HMAC, borrowing a constant-time-audited engine). Their designs are settled in `docs/impl/std-design/`; this book will grow their chapters as they land.
