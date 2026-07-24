# std: encoding, regex, rand, cli

> 🌐 **English** · [Japanese](./ja/14-std-encoding-rand-cli.md)

The second wave of `std`: converting and searching text/bytes at the boundary, random numbers, and command-line parsing. Same three rules as chapter [13](13-std-os.md) — explicit imports, explicit errors, Move where a resource is owned.

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

## `std.regex`

Compile a pattern once, bind its Move handle, and reuse it:

```align
import std.regex

pub fn main() -> Result<(), Error> {
    re := regex.compile("[A-Za-z_][A-Za-z0-9_]*")?
    print(re.is_match("answer = 42"))

    match re.find("answer = 42") {
        Some(m) => print("answer = 42"[m.start..m.end]),
        None    => print("no identifier"),
    }
    return Ok(())
}
```

`find` and `find_at` return `Option<regex_match>`. `start` and `end` are half-open UTF-8 byte offsets
at character boundaries, so they can be fed directly to a checked string slice. Invalid pattern
syntax or a resource-limit rejection is `Error.Invalid`; no match is simply `None`. A bad `find_at`
boundary aborts as a programming error. The automata engine deliberately omits look-around and
backreferences to keep matching predictable. There is no regex literal or hidden pattern cache—the
owned `regex` value is the explicit reusable compiled plan.

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

## `std.cli`

For more than a positional argument or two, register flags on a command and parse the one argv source, `main(args: array<str>)`:

```align
import std.cli

pub fn main(args: array<str>) -> Result<(), Error> {
    c := cli.command("tool")
    c.flag_bool("verbose")
    c.flag_str("input", "input.json")
    c.flag_i64("count", 1)

    p := c.parse(args)?
    if p.get_bool("verbose") { print(p.get_str("input")) }
    print(p.get_i64("count"))
    return Ok(())
}
```

`flag_bool` defaults to `false`; `flag_str` and `flag_i64` take explicit defaults. The accepted spelling is `--name value` (and `--name` for a bool). Unknown, duplicate, or malformed flags return `Error.Invalid`. After a successful parse, getters are total; asking for an undeclared name or the wrong type aborts as a programming error. `p.get_str` is a view into `p`, so clone it if the text must outlive the parsed handle.

Both the command and parsed result are Move handles. Bind them before calling methods; chained calls on unnamed owning receivers remain a separate v1 surface restriction even though general expression temporaries now clean up correctly. `c.usage()` returns generated usage text and remains available after either parse outcome. There are no derive macros or attribute DSLs: registration is ordinary code and visible at the call site.

The next wave — networking, HTTP/TLS, processes, compression, and cryptography — is also shipped. Chapter [18](18-std-services.md) introduces it.
