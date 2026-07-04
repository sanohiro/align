# JSON

> 🌐 **English** · [Japanese](./ja/08-json.md)

JSON is in `core` — not because Align wants a framework in the language, but because "typed records in, typed records out" is the boundary of nearly every data program, and a data-oriented language should make that boundary fast and typed. One import, two functions.

## Encoding

```align
import core.json

User { id: i64, name: str, active: bool }

fn main() -> i32 {
    u := User { id: 7, name: "ada", active: true }
    print(json.encode(u))       // {"id":7,"name":"ada","active":true}
    return 0
}
```

`json.encode(x)` renders a struct as a JSON object `str`; string fields are escaped for you. Under the hood it is the string builder from chapter [07](07-strings-and-text.md) — no reflection, no intermediate DOM.

## Decoding — the type comes from the annotation

`json.decode(s)` parses into whatever type the binding asks for, and returns `Result` because input is input:

```align
import core.json

User { id: i64, active: bool }

fn parse(s: str) -> Result<User, Error> {
    u: User := json.decode(s)?      // target type = the annotation
    return Ok(u)
}

fn main() -> Result<(), Error> {
    u := parse("{\"active\": true, \"x\": 9, \"id\": 42}")?
    print(u.id)                     // 42 — field order free, unknown keys ignored
    return Ok(())
}
```

There is no `json.decode<User>(...)` call form yet (implementation in progress) — today the annotation carries the type, which reads naturally through `?`.

Malformed input, a missing field, a type mismatch, an out-of-range number — all are an `Err`, never a panic and never a silently-wrong value:

```align
r: Result<User, Error> := json.decode("{\"id\": oops}")
match r {
    Ok(u)  => print(u.id),
    Err(_) => print("invalid json"),    // this one
}
```

## Decoding collections

Arrays decode to `array<T>` — scalars or structs:

```align
xs: array<i64> := json.decode("[3, 1, 4, 1, 5]")?
print(xs.sum())     // 14
```

And here is the data-oriented payoff — decode **straight into structure-of-arrays**:

```align
import core.json

User { name: str, age: i64, active: bool }

fn main() -> Result<(), Error> {
    data := "[{\"name\":\"alice\",\"age\":30,\"active\":true},{\"name\":\"bob\",\"age\":25,\"active\":false},{\"name\":\"carol\",\"age\":41,\"active\":true}]"
    arena {
        s: soa<User> := json.decode(data)?      // parse directly into columns
        print(s.len())                          // 3
        print(s.age.sum())                      // 96
        print(s.where(.active).age.sum())       // 71
        print(s[0].name)                        // alice — a zero-copy view into `data`
    }
    return Ok(())
}
```

`soa<User>` (chapter [11](11-data-oriented.md)) stores each field as its own contiguous column. Decoding into it builds the columns **directly while parsing** — no array-of-structs intermediate, no transpose afterwards — and string columns are zero-copy views borrowing the input text. That is why the decode lives in an `arena`: the columns share the arena's lifetime, the whole batch dies together, and the compiler holds you to it. This one-liner outruns typical hand-tuned decoders (it benchmarks at parity with Rust's serde_json at a million rows) because the *layout decision* removed the work, not a clever inner loop.

## The shape of a JSON program

Parse at the boundary into real types, process in the middle with pipelines over those types, encode at the far boundary. The middle of your program never sees JSON — it sees `soa<User>` and `array<i64>`, which is what the pipeline and SIMD machinery eat. If you find yourself wanting a dynamic "JSON value" type to pass around, that is the design asking you to declare the record type instead.
