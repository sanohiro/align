# JSON

> 🌐 **English** · [Japanese](./ja/08-json.md)

`core.json` converts between JSON text and typed values with `json.encode` and
`json.decode`. When the schema is not known, `json.doc` provides an arena-backed
document view that you can inspect or encode without declaring a record type.

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

Typed encoding writes through the string builder from chapter [07](07-strings-and-text.md), without reflection or an intermediate DOM.

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

Align does not accept expression-position type arguments such as `json.decode<User>(...)`. Put the target type on the binding; the annotation also supplies the expected type through `?`.

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

## When the schema is not known

Use typed decoding when the fields have a known shape. For an arbitrary JSON
payload, parse a `json.doc` inside an arena and select the values you need:

```align
import core.json

fn forward_payload(input: str) -> Result<string, Error> {
    return arena {
        document := json.doc(input)?
        payload := document.get("payload")
        json.encode_bounded(payload, 4096)
    }
}

fn main() -> Result<(), Error> {
    output := forward_payload("{\"payload\": [1e999, null, \"hi\"]}")?
    print(output)    // [1e999,null,"hi"] — the parse arena is already gone
    return Ok(())
}
```

`get(name)` selects an object member, and `at(index)` selects an array element.
Missing members, wrong container kinds and out-of-range indices produce a
Missing document; navigation can continue without throwing. Leaf methods
`as_str()`, `as_i64()`, `as_f64()` and `as_bool()` return `Option`. For example,
`payload.get("title").as_str() else "untitled"` supplies a default. Use `kind()`
when absence, null and a value of the wrong type must remain distinct.

Keep the input and parse arena alive while using the document or its views.
For an object, `keys()` and `elems()` materialize matching slices in the enclosing
arena, in document order, including duplicate keys. Bind a selected document
before encoding it, as `payload` above. A Missing document returns `Error.Invalid`;
JSON null is a valid value.

Document encoding produces an independent owned string. Default output removes
structural whitespace and preserves number tokens, original string escapes,
member order and duplicate occurrences. That is why `1e999` survives forwarding
without conversion to a machine float. To choose separator spacing and re-escape
strings, pass an explicit final argument in the same arena as the document:

```align
options := json.encode_options {
    space_after_comma: true,
    space_after_colon: true,
    reescape_strings: true,
}
output := json.encode_bounded(payload, 4096, options)?
```

All three fields are required; omitting the options argument means all false.
Re-escaping decodes JSON escapes and writes the standard JSON string escapes
and UTF-8, without Unicode normalization or HTML escaping. Number tokens remain
exact. Options apply to document encoding; typed encoders keep their existing
format. Every output byte, including spaces and escapes, counts toward the
inclusive cap. Negative or exceeded caps return `Error.Invalid` with no partial
output. The cap does not bound the parse arena or native allocator overhead.

## The shape of a JSON program

Decode known JSON shapes into record types at input, process those records with pipelines, and encode them at output. This keeps JSON handling at the program's boundaries. The main computation works with types such as `soa<User>` and `array<i64>`. Use a document view when inspecting or forwarding a schema-unknown payload.
