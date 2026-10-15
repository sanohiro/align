# JSON

> 🌐 **English** · [Japanese](./ja/08-json.md)

`core.json` converts between JSON text and typed records. It provides two main functions: `json.encode` and `json.decode`.

## Encoding

```align
import core.json

User { id: i64, name: str, active: bool }

fn main() -> Result<(), Error> {
    u := User { id: 7, name: "ada", active: true }
    print(json.encode(u)?)      // {"id":7,"name":"ada","active":true}
    return Ok(())
}
```

Both encoders return an owned string through `Result<string, Error>`.
`json.encode(x)` returns `Result<string, Error>`: success owns the JSON text and
nonfinite float input returns `Error.Invalid`. Use `?` to propagate that error.
`json.encode_bounded(x, max_bytes)` also rejects negative or exceeded byte limits.
The result outlives the source and any enclosing arena; string fields are escaped
for you. See the [contract](../impl/47-json-numeric-contract.md).

Under the hood it is the string builder from chapter [07](07-strings-and-text.md) — no reflection, no intermediate DOM.

## Decoding — the type comes from the annotation

`json.decode(s)` uses the binding's type annotation to choose the result type. Parsing can fail, so it returns `Result`:

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

There is deliberately no `json.decode<User>(...)` call form: Align has no expression-position type arguments. The annotation carries the type, which reads naturally through `?`.

Malformed input, a missing field, a type mismatch or an out-of-range number returns `Err`:

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

You can also decode directly into **structure-of-arrays**:

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
        print(s[0].name)                        // alice — clean strings view `data`
    }
    return Ok(())
}
```

`soa<User>` (chapter [11](11-data-oriented.md)) stores each field in a contiguous column. The decoder fills these columns **while parsing**, with no intermediate array of structs or later transpose. Unescaped string fields borrow the input text; selected escaped strings are decoded once into the enclosing arena. The columns share the arena's lifetime.

## Choosing borrowed or owned text

Use `string` fields when the decoded value must outlive the input. Owned record arrays and an `array<string>` root are supported too:

```align
import core.json

User { name: string, age: i64 }

fn load_users() -> Result<array<User>, Error> {
    input := "[{\"name\":\"Ada\",\"age\":37}]".clone()
    users: array<User> := json.decode(input)?
    return Ok(users)
}

fn main() -> Result<(), Error> {
    users := load_users()?
    print(users[0].name)    // Ada — input has already been dropped
    names: array<string> := json.decode("[\"Ada\",\"Lin\"]")?
    print(names[1])         // Lin — indexing borrows str from names
    return Ok(())
}
```

The decoder allocates each owned string and array spine independently, including inside an arena; ordinary recursive Drop releases them. A fully owned result retains neither the input nor its arena. This avoids decoding borrowed text and then cloning each surviving field manually.

A supported recursive record graph may mix `str` and `string`, including optional text and arrays. Each `string` owns its bytes; clean `str` still borrows input, and escaped `str` needs an explicit enclosing arena. Without one, decoding a selected escaped `str` returns `Error.Code(1)`. A mixed result remains tied to both input and enclosing arena; owning its array spine does not own its borrowed text. Encoding either form returns independent owned output. The [JSON design](../impl/core-design/json.md) lists the complete supported shapes and error rules.

## The shape of a JSON program

Decode JSON into record types at input, process those records with pipelines, and encode them at output. This keeps JSON handling at the program's boundaries. The main computation works with types such as `soa<User>` and `array<i64>`. Declare a record type for the data you need to process.
