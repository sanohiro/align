//! M5 end-to-end: strings (literals + `print`). Requires LLVM/cc, so skip where absent.


mod common;
use common::*;

/// Build `src` and run it, forwarding `prog_args` to the program (its `main(args)`); argv[0] is
/// the executable, then `prog_args`.
#[test]
fn print_string_literal_and_returned_str() {
    if !backend_available() {
        return;
    }
    let src = "fn greet() -> str = \"hello, align\"\nfn main() -> i32 {\n  print(\"strings work!\")\n  print(greet())\n  print(7)\n  return 0\n}\n";
    let out = build_and_run("strings", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "strings work!\nhello, align\n7\n"
    );
}

#[test]
fn string_equality() {
    if !backend_available() {
        return;
    }
    // check("yes")=1, check("maybe")=2 → 1 + 2*10 = 21.
    let src = "fn check(s: str) -> i32 {\n  if s == \"yes\" { return 1 }\n  if s != \"no\" { return 2 }\n  return 0\n}\nfn main() -> i32 {\n  return check(\"yes\") + check(\"maybe\") * 10\n}\n";
    let out = build_and_run("str-eq", src);
    assert_eq!(out.status.code(), Some(21));
}

#[test]
fn empty_string_equality() {
    if !backend_available() {
        return;
    }
    // "" == "" is true (1); "" == "x" is false. 1 + 0 = 1.
    let src = "fn b2i(b: bool) -> i32 {\n  if b { return 1 }\n  return 0\n}\nfn main() -> i32 {\n  return b2i(\"\" == \"\") + b2i(\"\" == \"x\")\n}\n";
    let out = build_and_run("str-empty-eq", src);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn template_interpolation() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  name := \"align\"\n  score := 42\n  print(template \"Hello {name}, score={score}!\")\n  return 0\n}\n";
    let out = build_and_run("template", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Hello align, score=42!\n");
}

#[test]
fn template_expression_holes() {
    if !backend_available() {
        return;
    }
    // `{expr}` holes accept arithmetic; literal text can surround a string-valued hole.
    let src = "fn main() -> i32 {\n  a := 20\n  b := 22\n  name := \"world\"\n  print(template \"sum={a + b} dbl={a * 2} hi={name}!\")\n  return 0\n}\n";
    let out = build_and_run("template-expr", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "sum=42 dbl=40 hi=world!\n");
}

#[test]
fn len_of_str_slice_array() {
    if !backend_available() {
        return;
    }
    // str.len() is the byte length (UTF-8: 'あ' = 3 bytes); slice/array lengths too.
    let src = "fn slen(xs: slice<i32>) -> i64 = xs.len()\nfn main() -> i32 {\n  s := \"hello\"\n  a := [10, 20, 30, 40]\n  print(s.len())\n  print(a.len())\n  print(slen([1, 2, 3]))\n  print(\"あ\".len())\n  return 0\n}\n";
    let out = build_and_run("len", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n4\n3\n3\n");
}

#[test]
fn str_bytes_is_a_zero_copy_view_for_str_and_owned_string() {
    if !backend_available() {
        return;
    }
    // The continuation byte of `é` is not a valid standalone `str`, but it is directly readable
    // through `bytes`. Passing an owned string to `view` borrows it; the owner remains usable.
    let src = "fn view(s: str) -> slice<u8> = s.bytes()\nfn main() -> i32 {\n  owned := \"é!\".clone()\n  bytes := view(owned)\n  print(owned)\n  return bytes[1] as i32\n}\n";
    let out = build_and_run("str-bytes", src);
    assert_eq!(out.status.code(), Some(0xa9));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "é!\n");
}

#[test]
fn str_bytes_preserves_borrow_regions() {
    let ok = "fn view(s: str) -> slice<u8> = s.bytes()\nfn literal() -> slice<u8> = \"abc\".bytes()\nfn main() -> i32 = 0\n";
    assert!(!check_errs("str-bytes-static", ok), "caller and static storage may be re-viewed");

    let owned_escape = "fn bad() -> slice<u8> {\n  s := \"abc\".clone()\n  return s.bytes()\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("str-bytes-owned-escape", owned_escape));

    let arena_escape = "fn bad(s: str) -> slice<u8> {\n  arena {\n    built := template \"{s}!\"\n    return built.bytes()\n  }\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("str-bytes-arena-escape", arena_escape));

    let invalidated_owner = "fn bad() -> i32 {\n  mut s := \"abc\".clone()\n  bytes := s.bytes()\n  s = \"def\".clone()\n  return bytes[0] as i32\n}\nfn main() -> i32 = bad()\n";
    assert!(
        check_errs("str-bytes-invalidated-owner", invalidated_owner),
        "a bytes view must keep its owned string source borrowed"
    );

    assert!(check_errs(
        "str-bytes-args",
        "fn main() -> i32 {\n  bytes := \"abc\".bytes(1)\n  return bytes.len() as i32\n}\n"
    ));
}

#[test]
fn str_bytes_lowers_without_a_mir_operation() {
    let src = "fn view(s: str) -> slice<u8> = s.bytes()\nfn main() -> i32 = 0\n";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "str-bytes-mir.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = align_mir::print::program_to_string(&lower_to_mir(&checked.hir));
    assert!(!mir.contains("buffer_bytes"), "str.bytes() must not use the buffer runtime path:\n{mir}");
    assert!(!mir.contains("str_bytes"), "the descriptor retype must not add a MIR operation:\n{mir}");

    let ir = emit_llvm_with_exports(src, &["view"]);
    let view = ir
        .split("define ")
        .find(|body| body.contains("@view("))
        .expect("exported view function in LLVM IR");
    let view = view.split("\n}").next().expect("view function body");
    assert!(!view.contains(" call "), "str.bytes() must not introduce a codegen call:\n{view}");
}

#[test]
fn json_decode_str_field_zero_copy() {
    if !backend_available() {
        return;
    }
    // A `str` field decodes as a zero-copy view into the input buffer (MMv2 slice 6): the
    // printed name comes straight from the input bytes, no allocation. id=7, name="alice".
    let src = "import core.json\nUser { id: i64, name: str, active: bool }\nfn parse(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> Result<(), Error> {\n  u := parse(\"{\\\"id\\\": 7, \\\"name\\\": \\\"alice\\\", \\\"active\\\": true}\")?\n  print(u.id)\n  print(u.name)\n  if u.active { print(1) }\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-str", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\nalice\n1\n");
}

#[test]
fn json_decode_phf_two_end_match_all_fields() {
    if !backend_available() {
        return;
    }
    // Cross-stage PHF byte-match guard (Gate 4). Eight fields force a compile-time perfect-hash
    // table (codegen `build_phf`), and the JSON keys arrive in *reverse* order, so every name must
    // be re-hashed at decode time (runtime `find_field`) and routed to its own slot. Both ends call
    // the one `align_hash::wyhash`, so a byte- or seed-level divergence would misroute at least one
    // field and change the printed sequence. Values 1..=8 are assigned per name (not per position),
    // so a correct decode prints them in *declaration* order: 1,2,…,8.
    let src = "import core.json\n\
        Rec { alpha: i64, bravo: i64, charlie: i64, delta: i64, echo: i64, foxtrot: i64, golf: i64, hotel: i64 }\n\
        fn parse(s: str) -> Result<Rec, Error> {\n  r: Rec := json.decode(s)?\n  return Ok(r)\n}\n\
        fn main() -> Result<(), Error> {\n  \
        r := parse(\"{\\\"hotel\\\":8,\\\"golf\\\":7,\\\"foxtrot\\\":6,\\\"echo\\\":5,\\\"delta\\\":4,\\\"charlie\\\":3,\\\"bravo\\\":2,\\\"alpha\\\":1}\")?\n  \
        print(r.alpha)\n  print(r.bravo)\n  print(r.charlie)\n  print(r.delta)\n  \
        print(r.echo)\n  print(r.foxtrot)\n  print(r.golf)\n  print(r.hotel)\n  return Ok(())\n}\n";
    let out = build_and_run("json-phf-two-end", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n3\n4\n5\n6\n7\n8\n");
}

#[test]
fn json_decode_skips_unknown_numeric_fields() {
    if !backend_available() {
        return;
    }
    // A narrow struct (`id` only) over an object carrying extra numeric fields the decoder does
    // not store: the unknown `score`/`ratio`/`exp` values are skipped lexically (no float parse),
    // and `id` still decodes. Exercises `skip_number` across int, fraction, and exponent forms.
    let src = "import core.json\nUser { id: i64 }\nfn parse(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> Result<(), Error> {\n  u := parse(\"{\\\"score\\\": -3.14, \\\"id\\\": 7, \\\"ratio\\\": 0.5, \\\"exp\\\": 6.022e23}\")?\n  print(u.id)\n  return Ok(())\n}\n";
    let out = build_and_run("json-skip-numbers", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn json_decode_long_str_field_via_simd_scan() {
    if !backend_available() {
        return;
    }
    // A `str` value longer than the scalar prefix (16B) exercises the memchr2 SIMD scan in
    // `string()`; the zero-copy view must still be byte-exact. The 64-char name (len 64) is
    // printed back verbatim and its length checked.
    let name = "abcdefghijklmnopqrstuvwxyz-0123456789-ABCDEFGHIJKLMNOPQRSTUVWXYZ"; // 63 chars
    let json = format!(r#"{{"id": 7, "name": "{name}"}}"#);
    let src = format!(
        "import core.json\nUser {{ id: i64, name: str }}\nfn parse(s: str) -> Result<User, Error> {{\n  u: User := json.decode(s)?\n  return Ok(u)\n}}\nfn main() -> Result<(), Error> {{\n  u := parse({json:?})?\n  print(u.name)\n  print(u.name.len())\n  return Ok(())\n}}\n",
    );
    let out = build_and_run("json-long-str", &src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{name}\n{}\n", name.len()));
}

#[test]
fn json_decode_encode_nested_struct_roundtrip() {
    if !backend_available() {
        return;
    }
    // REST-gateway runway, Slice A: a struct with a nested-struct field decodes recursively (the
    // runtime kind-4 path), its fields read at any depth (`o.inner.x`, `o.inner.name`), and
    // `json.encode` renders the nested object back — a byte-exact round trip in declaration order.
    let json = r#"{"id":1,"inner":{"x":5,"name":"hi"},"count":9}"#;
    let src = format!(
        "import core.json\n\
         Inner {{ x: i64, name: str }}\n\
         Outer {{ id: i64, inner: Inner, count: i64 }}\n\
         fn parse(s: str) -> Result<Outer, Error> {{\n  o: Outer := json.decode(s)?\n  return Ok(o)\n}}\n\
         fn main() -> Result<(), Error> {{\n  \
         o := parse({json:?})?\n  \
         print(o.id)\n  print(o.inner.x)\n  print(o.inner.name)\n  print(o.count)\n  \
         print(json.encode(o))\n  return Ok(())\n}}\n",
    );
    let out = build_and_run("json-nested-roundtrip", &src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("1\n5\nhi\n9\n{json}\n"));
}

#[test]
fn json_decode_nested_struct_array_mison() {
    if !backend_available() {
        return;
    }
    // The Mison speculative array path (`json_speculate`/`write_field_indexed`) recurses into each
    // element's nested-struct field. Field order is shuffled across records to force key re-verify;
    // nested `str` views stay zero-copy into the input.
    let json = r#"[{"id":1,"inner":{"x":5,"name":"a"},"count":9},{"count":8,"inner":{"name":"bb","x":6},"id":2}]"#;
    let src = format!(
        "import core.json\n\
         Inner {{ x: i64, name: str }}\n\
         Outer {{ id: i64, inner: Inner, count: i64 }}\n\
         fn main() -> Result<(), Error> {{\n  \
         s := {json:?}\n  \
         xs: array<Outer> := json.decode(s)?\n  \
         print(xs[0].inner.name)\n  print(xs[0].count)\n  print(xs[1].inner.x)\n  print(xs[1].inner.name)\n  \
         return Ok(())\n}}\n",
    );
    let out = build_and_run("json-nested-array", &src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\n9\n6\nbb\n");
}

#[test]
fn json_encode_fixed_struct_array_with_nested() {
    if !backend_available() {
        return;
    }
    // `json.encode` over a fixed struct array whose element has a nested-struct field: the unrolled
    // encode reads each nested leaf through the generalized `IndexField` path (`base[e].inner.x`),
    // proving the `Vec<u32>` path (elem_field_ptr / phys_field_indices) handles nested element fields.
    let src = "import core.json\n\
        Inner { x: i64, name: str }\n\
        Outer { id: i64, inner: Inner }\n\
        fn main() -> i32 {\n  \
        a := [Outer{id: 1, inner: Inner{x: 5, name: \"a\"}}, Outer{id: 2, inner: Inner{x: 6, name: \"b\"}}]\n  \
        print(json.encode(a))\n  return 0\n}\n";
    let out = build_and_run("json-encode-fixed-nested", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "[{\"id\":1,\"inner\":{\"x\":5,\"name\":\"a\"}},{\"id\":2,\"inner\":{\"x\":6,\"name\":\"b\"}}]\n"
    );
}

#[test]
fn json_decode_option_fields_null_policy() {
    if !backend_available() {
        return;
    }
    // REST-gateway runway Slice B: `Option<T>` decode fields with the settled null policy —
    // present value → Some, JSON `null` → None, missing key → None, type mismatch → Err. This is the
    // `temperature?`/`stream?` shape of an OpenAI request.
    let json = r#"{"model":"gpt","temperature":0.5,"stream":null}"#;
    let src = format!(
        "import core.json\n\
         Req {{ model: str, temperature: Option<f64>, stream: Option<bool>, tag: Option<str> }}\n\
         fn parse(s: str) -> Result<Req, Error> {{\n  r: Req := json.decode(s)?\n  return Ok(r)\n}}\n\
         fn main() -> Result<(), Error> {{\n  \
         r := parse({json:?})?\n  \
         print(r.model)\n  \
         match r.temperature {{ Some(t) => print(t) None => print(0.0 - 1.0) }}\n  \
         match r.stream {{ Some(b) => {{ if b {{ print(1) }} else {{ print(2) }} }} None => print(9) }}\n  \
         match r.tag {{ Some(s) => print(s) None => print(0 - 5) }}\n  return Ok(())\n}}\n",
    );
    let out = build_and_run("json-option-nullpolicy", &src);
    assert_eq!(out.status.code(), Some(0));
    // temperature present (0.5), stream null → None (9), tag missing → None (-5).
    assert_eq!(String::from_utf8_lossy(&out.stdout), "gpt\n0.5\n9\n-5\n");
}

#[test]
fn json_decode_option_struct_field_in_array() {
    if !backend_available() {
        return;
    }
    // `Option<Struct>` fields decode through the Mison array path: present nested object → Some,
    // missing → None. Varying optionality across records forces both speculate + fallback.
    let json = r#"[{"id":1,"inner":{"x":7}},{"id":2},{"id":3,"inner":{"x":9}}]"#;
    let src = format!(
        "import core.json\n\
         Inner {{ x: i64 }}\n\
         Row {{ id: i64, inner: Option<Inner> }}\n\
         fn main() -> Result<(), Error> {{\n  \
         s := {json:?}\n  \
         xs: array<Row> := json.decode(s)?\n  \
         match xs[0].inner {{ Some(v) => print(v.x) None => print(0 - 1) }}\n  \
         match xs[1].inner {{ Some(v) => print(v.x) None => print(0 - 1) }}\n  \
         match xs[2].inner {{ Some(v) => print(v.x) None => print(0 - 1) }}\n  return Ok(())\n}}\n",
    );
    let out = build_and_run("json-option-struct-array", &src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n-1\n9\n");
}

#[test]
fn json_encode_option_fields_omit_none() {
    if !backend_available() {
        return;
    }
    // REST-gateway runway Slice B: `json.encode` omits a `None` `Option` field entirely (never
    // `"k":null`), with correct commas — the trailing-comma + pop-comma scheme. Covers some-present,
    // all-none (→ `{}`-tail), and all-present.
    let src = "import core.json\n\
        Req { model: str, temperature: Option<f64>, stream: Option<bool>, tag: Option<str> }\n\
        fn main() -> i32 {\n  \
        a := Req{model: \"gpt\", temperature: Some(0.5), stream: None, tag: Some(\"x\")}\n  \
        print(json.encode(a))\n  \
        b := Req{model: \"m\", temperature: None, stream: None, tag: None}\n  \
        print(json.encode(b))\n  \
        c := Req{model: \"z\", temperature: None, stream: Some(false), tag: None}\n  \
        print(json.encode(c))\n  return 0\n}\n";
    let out = build_and_run("json-option-encode", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        // a: stream omitted; b: all optionals omitted; c: only stream present.
        "{\"model\":\"gpt\",\"temperature\":0.5,\"tag\":\"x\"}\n{\"model\":\"m\"}\n{\"model\":\"z\",\"stream\":false}\n"
    );
}

#[test]
fn json_option_field_decode_encode_roundtrip() {
    if !backend_available() {
        return;
    }
    // Decode a payload with a present and an absent optional, then re-encode: the round trip is
    // stable (the absent field stays absent — `decode(encode(x))` fixpoint by construction).
    let json = r#"{"model":"gpt","temperature":0.7}"#;
    let src = format!(
        "import core.json\n\
         Req {{ model: str, temperature: Option<f64>, stream: Option<bool> }}\n\
         fn main() -> Result<(), Error> {{\n  \
         r: Req := json.decode({json:?})?\n  \
         print(json.encode(r))\n  return Ok(())\n}}\n",
    );
    let out = build_and_run("json-option-roundtrip", &src);
    assert_eq!(out.status.code(), Some(0));
    // stream was absent → stays omitted; temperature round-trips.
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{json}\n"));
}

#[test]
fn json_decode_skips_unknown_nested_objects_arrays_and_null() {
    if !backend_available() {
        return;
    }
    // The projection rail: a narrow struct (`id` only) decodes from an object whose other fields
    // are a nested object, an array, `null`, and a string with structural bytes (`}`/`]`/escaped
    // quote) inside it — all skipped without breaking the parse. `id` still decodes to 7.
    let json = r#"{"meta": {"a": 1, "b": [2, 3]}, "id": 7, "tags": [1, [2], {"c": 4}], "note": null, "s": "has } and ] and \" inside"}"#;
    let src = format!(
        "import core.json\nUser {{ id: i64 }}\nfn parse(s: str) -> Result<User, Error> {{\n  u: User := json.decode(s)?\n  return Ok(u)\n}}\nfn main() -> Result<(), Error> {{\n  u := parse({json:?})?\n  print(u.id)\n  return Ok(())\n}}\n",
    );
    let out = build_and_run("json-skip-nested", &src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn str_clone_escapes_arena_as_owned_string() {
    if !backend_available() {
        return;
    }
    // `str.clone()` deep-copies into a heap-owned `string` that outlives the arena its source
    // was built in (MMv2 slice 7): the template `c` lives in the arena (freed at `}`), but the
    // returned clone owns its own buffer. `print` borrows (so `s` is still usable for `.len()`),
    // and the owned string is freed once at function exit.
    let src = "fn longer(a: str, b: str) -> string {\n  arena {\n    c := template \"{a}{b}\"\n    return c.clone()\n  }\n}\nfn main() -> i32 {\n  s := longer(\"foo\", \"bar\")\n  print(s)\n  print(s.len())\n  return 0\n}\n";
    let out = build_and_run("str-clone-escape", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "foobar\n6\n");
}

#[test]
fn str_clone_of_decoded_field_is_owned() {
    if !backend_available() {
        return;
    }
    // Clone a zero-copy decoded `str` field into an owned `string` — the explicit escape hatch
    // out of the borrow. name="alice" (len 5).
    let src = "import core.json\nUser { id: i64, name: str }\nfn decode(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> Result<(), Error> {\n  u := decode(\"{\\\"id\\\": 7, \\\"name\\\": \\\"alice\\\"}\")?\n  owned := u.name.clone()\n  print(owned)\n  print(owned.len())\n  return Ok(())\n}\n";
    let out = build_and_run("str-clone-decoded", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "alice\n5\n");
}

#[test]
fn owned_string_moved_into_callee_is_freed_once() {
    if !backend_available() {
        return;
    }
    // A `string` passed by value is *moved* into the callee, which owns and drops it (the caller
    // nulls its slot on the move, so no double-free). Exercises that an owned-`string` *parameter*
    // is NOT entry-null-initialised (which would clobber the incoming argument). len("hello") = 5.
    let src = "fn consume(s: string) -> i64 = s.len()\nfn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  x := mk(\"hello\")\n  n := consume(x)\n  if n == 5 { return 0 }\n  return 1\n}\n";
    let out = build_and_run("string-move-param", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn owned_string_borrowed_as_str_arg() {
    if !backend_available() {
        return;
    }
    // An owned `string` is passed to a `str` parameter by *borrowing* it (MMv2 slice 7b): the
    // view shares the `{ptr,len}` layout (zero-cost), and the `string` is NOT consumed — `s` is
    // still usable after the call (`s.len()` below) and freed exactly once at function exit.
    // `show` borrows the string twice across the call boundary; output: "foobar\n6\n6\n".
    let src = "fn show(label: str) -> i64 {\n  print(label)\n  return label.len()\n}\nfn mk(a: str, b: str) -> string {\n  arena {\n    c := template \"{a}{b}\"\n    return c.clone()\n  }\n}\nfn main() -> i32 {\n  s := mk(\"foo\", \"bar\")\n  n := show(s)\n  print(n)\n  print(s.len())\n  return 0\n}\n";
    let out = build_and_run("string-borrow-arg", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "foobar\n6\n6\n");
}

#[test]
fn builder_constructs_owned_string() {
    if !backend_available() {
        return;
    }
    // MMv2 slice 7c: the canonical string-construction API (draft.md §12). `builder()` opens a
    // writer, `.write(str)` / `.write_int(i64)` append, `.to_string()` finishes into an owned
    // `string` that outlives the builder. The result is `print`ed and its byte length checked:
    // "hello, align! score=42" is 22 bytes. Output: "hello, align! score=42\n22\n".
    let src = "fn make(name: str, score: i64) -> string {\n  b := builder()\n  b.write(\"hello, \")\n  b.write(name)\n  b.write(\"! score=\")\n  b.write_int(score)\n  return b.to_string()\n}\nfn main() -> i32 {\n  s := make(\"align\", 42)\n  print(s)\n  print(s.len())\n  return 0\n}\n";
    let out = build_and_run("builder", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, align! score=42\n22\n");
}

#[test]
fn builder_with_capacity() {
    if !backend_available() {
        return;
    }
    // `builder(n)` pre-sizes the backing buffer (so appends don't reallocate); the result is
    // identical to `builder()`. "hello, align! score=42" = 22 bytes; capacity is just a hint.
    let src = "fn make(name: str, score: i64) -> string {\n  b := builder(64)\n  b.write(\"hello, \")\n  b.write(name)\n  b.write(\"! score=\")\n  b.write_int(score)\n  return b.to_string()\n}\nfn main() -> i32 {\n  print(make(\"align\", 42).len())\n  return 0\n}\n";
    let out = build_and_run("builder-cap", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "22\n");
}

#[test]
fn builder_capacity_must_be_int() {
    // `builder(capacity)` takes an `i64`; a non-integer capacity is a type error.
    assert!(check_errs("builder-cap-bad", "fn main() -> i32 { b := builder(\"x\")\n return 0 }\n"));
}

#[test]
fn builder_write_borrows_owned_string() {
    if !backend_available() {
        return;
    }
    // `b.write(owned)` borrows a `string` argument (slice 7b coercion), so the source stays
    // usable afterwards (`owned.len()` below). An unfinished builder is also created and freed at
    // exit (no leak / double-free). Output: "hi world\n5\n".
    let src = "fn dup(s: str) -> string = s.clone()\nfn main() -> i32 {\n  owned := dup(\"world\")\n  b := builder()\n  b.write(\"hi \")\n  b.write(owned)\n  msg := b.to_string()\n  print(msg)\n  print(owned.len())\n  unfinished := builder()\n  unfinished.write(\"x\")\n  return 0\n}\n";
    let out = build_and_run("builder-write-string", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi world\n5\n");
}

#[test]
fn string_borrowed_into_str_let_binding() {
    if !backend_available() {
        return;
    }
    // MMv2 slice 7e: a `str`-annotated `let` borrows an owned `string` (the slice-7b coercion now
    // also applies at let bindings). The borrow is non-consuming, so `owned` stays usable after.
    // Output: "hello\n5\n".
    let src = "fn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  owned := mk(\"hello\")\n  view: str := owned\n  print(view)\n  print(owned.len())\n  return 0\n}\n";
    let out = build_and_run("string-let-borrow", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n5\n");
}

#[test]
fn result_string_payload_unwrapped_and_freed() {
    if !backend_available() {
        return;
    }
    // MMv2 slice 8a: a fallible function returns an owned `string` in a `Result`; `?` moves it
    // out (the source Result's payload is nulled so it isn't double-freed), and the unwrapped
    // `string` is freed once at scope exit. len("hello") = 5 → "5\n".
    let src = "fn mk(a: str) -> Result<string, Error> = Ok(a.clone())\nfn greet(name: str) -> Result<i64, Error> {\n  r := mk(name)\n  s := r?\n  return Ok(s.len())\n}\nfn main() -> Result<(), Error> {\n  n := greet(\"hello\")?\n  print(n)\n  return Ok(())\n}\n";
    let out = build_and_run("result-string", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn option_string_payload_else_unwrap() {
    if !backend_available() {
        return;
    }
    // `Option<string>` carries an owned payload; `else` moves it out on `Some`, and a `None`
    // result owns no buffer (its payload is {null,0}, so the drop is a no-op). first(false) =
    // Some("hi") → len 2; first(true) = None → else returns 0. Output "2\n", exit 0.
    let src = "fn first(empty: bool) -> Option<string> {\n  if empty { return None }\n  return Some(\"hi\".clone())\n}\nfn main() -> i32 {\n  s := first(false) else { return 9 }\n  print(s.len())\n  t := first(true) else { return 0 }\n  print(t.len())\n  return 1\n}\n";
    let out = build_and_run("option-string", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

#[test]
fn result_string_err_path_frees_nothing() {
    if !backend_available() {
        return;
    }
    // On the `Err` arm the owned `ok` payload is zeroed at construction, so propagating the error
    // frees no garbage and leaks nothing. mk(true) = Err(7) → `?` propagates → exit code 7.
    let src = "fn mk(fail: bool) -> Result<string, Error> {\n  if fail { return Err(error(7)) }\n  return Ok(\"ok\".clone())\n}\nfn main() -> Result<(), Error> {\n  s := mk(true)?\n  print(s.len())\n  return Ok(())\n}\n";
    let out = build_and_run("result-string-err", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn result_array_payload_unwrapped_and_freed() {
    if !backend_available() {
        return;
    }
    // MMv2 slice 8b: a fallible function returns an owned `array<i64>` in a `Result`; the
    // heap-allocated buffer is moved out of `mk` into the Result, unwrapped by `?`, summed, and
    // freed exactly once at scope exit (the source aggregate is nulled on the Ok edge). 1+2+3 = 6.
    let src = "fn mk() -> Result<array<i64>, Error> = Ok([1, 2, 3].to_array())\nfn use() -> Result<i64, Error> {\n  xs := mk()?\n  return Ok(xs.sum())\n}\nfn main() -> Result<(), Error> {\n  n := use()?\n  print(n)\n  return Ok(())\n}\n";
    let out = build_and_run("result-array", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n");
}

#[test]
fn option_array_payload_else_unwrap() {
    if !backend_available() {
        return;
    }
    // `Option<array<i64>>` carries an owned payload; `else` moves it out on `Some`, and `None`
    // owns no buffer (its payload is {null,0}). first(false) = Some([10,20]) → sum 30. Output "30\n".
    let src = "fn first(empty: bool) -> Option<array<i64>> {\n  if empty { return None }\n  return Some([10, 20].to_array())\n}\nfn main() -> i32 {\n  xs := first(false) else { return 9 }\n  print(xs.sum())\n  return 0\n}\n";
    let out = build_and_run("option-array", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "30\n");
}

#[test]
fn json_decode_scalar_array() {
    if !backend_available() {
        return;
    }
    // MMv2 slice 8c: `json.decode` into an owned `array<i64>` — the JSON array is parsed and the
    // elements copied into a fresh heap buffer (owned, returnable). sum = 100, len = 4. Also
    // exercises `return Ok(xs)` moving a bound owned local through the `Ok` wrapper (freed once).
    let src = "import core.json\nfn parse(s: str) -> Result<array<i64>, Error> {\n  xs: array<i64> := json.decode(s)?\n  return Ok(xs)\n}\nfn main() -> Result<(), Error> {\n  xs := parse(\"[10, 20, 30, 40]\")?\n  print(xs.sum())\n  print(xs.len())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-array", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "100\n4\n");
}

#[test]
fn json_decode_float_array() {
    if !backend_available() {
        return;
    }
    // `array<f64>` decode: 1.5 + 2.5 + 3.0 = 7.0.
    let src = "import core.json\nfn main() -> Result<(), Error> {\n  xs: array<f64> := json.decode(\"[1.5, 2.5, 3.0]\")?\n  print(xs.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-farray", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7.0\n");
}

#[test]
fn json_decode_array_malformed_errors() {
    if !backend_available() {
        return;
    }
    // A malformed element propagates an error (exit code 1), with no allocation leaked.
    let src = "import core.json\nfn main() -> Result<(), Error> {\n  xs: array<i64> := json.decode(\"[1, 2, oops]\")?\n  print(xs.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-array-bad", src);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn json_decode_array_from_owned_string() {
    if !backend_available() {
        return;
    }
    // The `json.decode` input accepts an owned `string` (auto-borrowed to `str` via the same
    // coercion as `let`/call args); the decoded `array<i64>` is copied, so it outlives the
    // borrow. Build "[1, 2, 3]" with a builder, decode it: sum = 6, len = 3.
    let src = "import core.json\nfn main() -> Result<(), Error> {\n  b := builder()\n  b.write(\"[1, 2, 3]\")\n  doc := b.to_string()\n  xs: array<i64> := json.decode(doc)?\n  print(xs.sum())\n  print(xs.len())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-array-owned", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n3\n");
}

#[test]
fn json_decode_empty_array_is_safe() {
    if !backend_available() {
        return;
    }
    // An empty JSON array decodes to a `{null, 0}` owned array — `.len()` is 0, `.sum()` is 0,
    // and the runtime must not `from_raw_parts(null, 0)` on either the empty result or, when the
    // source itself is an empty owned `string`, the input buffer. Output: "0\n0\n".
    let src = "import core.json\nfn main() -> Result<(), Error> {\n  xs: array<i64> := json.decode(\"[]\")?\n  print(xs.sum())\n  print(xs.len())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-empty-array", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n0\n");
}

#[test]
fn json_decode_struct_array_len() {
    if !backend_available() {
        return;
    }
    // MMv2 slice 8d (draft.md §19 headline): `json.decode` into an owned, dynamic `array<Struct>`
    // (AoS). The two objects parse into a heap buffer of `User` structs; `.len()` reads the count
    // (2), and the buffer is freed at scope exit (no double-free / leak crash). `str` fields are
    // zero-copy views into the input literal (Static), so no arena is needed here.
    let src = "import core.json\nUser { id: i64, name: str, active: bool }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"name\\\":\\\"ann\\\",\\\"active\\\":true},{\\\"id\\\":2,\\\"name\\\":\\\"bob\\\",\\\"active\\\":false}]\")?\n  print(users.len())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-struct-array", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

#[test]
fn json_decode_struct_array_scalar_only() {
    if !backend_available() {
        return;
    }
    // A scalar-only struct array (no `str` fields) decodes the same way; len = 3.
    let src = "import core.json\nP { x: i64, y: i64 }\nfn main() -> Result<(), Error> {\n  ps: array<P> := json.decode(\"[{\\\"x\\\":1,\\\"y\\\":2},{\\\"x\\\":3,\\\"y\\\":4},{\\\"x\\\":5,\\\"y\\\":6}]\")?\n  print(ps.len())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-struct-array-scalar", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n");
}

#[test]
fn json_decode_empty_struct_array_is_safe() {
    if !backend_available() {
        return;
    }
    // An empty `[]` decodes to a `{null, 0}` owned struct array — `.len()` is 0, and the runtime
    // must not `from_raw_parts(null, 0)` nor `free` a non-null buffer. Output: "0\n".
    let src = "import core.json\nUser { id: i64, name: str }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users.len())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-empty-struct-array", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

#[test]
fn json_decode_struct_array_malformed_errors() {
    if !backend_available() {
        return;
    }
    // A malformed element (missing the required `active` field) propagates an error (exit 1),
    // leaving the out slot `{null,0}` (nothing allocated / leaked).
    let src = "import core.json\nUser { id: i64, name: str, active: bool }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"name\\\":\\\"ann\\\"}]\")?\n  print(users.len())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-struct-array-bad", src);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn json_decode_struct_array_pipeline_sum() {
    if !backend_available() {
        return;
    }
    // MMv2 slice 8d-2 — the draft.md §19 headline runs end-to-end (compiler side): decode a JSON
    // array of objects into an owned `array<User>`, then fuse `where(.active).score.sum()` into a
    // single counted loop over the heap AoS (field access via `IndexFieldPtr`). The inactive `bob`
    // (score 99) is filtered out → 10 + 5 = 15.
    let src = "import core.json\nUser { id: i64, name: str, active: bool, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"name\\\":\\\"ann\\\",\\\"active\\\":true,\\\"score\\\":10},{\\\"id\\\":2,\\\"name\\\":\\\"bob\\\",\\\"active\\\":false,\\\"score\\\":99},{\\\"id\\\":3,\\\"name\\\":\\\"cyd\\\",\\\"active\\\":true,\\\"score\\\":5}]\")?\n  total := users.where(.active).score.sum()\n  print(total)\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-struct-array-pipeline", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "15\n");
}

#[test]
fn json_decode_struct_array_pipeline_project_and_count() {
    if !backend_available() {
        return;
    }
    // A bare field projection + `sum` (no `where`): sum all `score`s = 10 + 99 + 5 = 114. And a
    // `where(.active)` count of survivors = 2. Two pipelines over the same decoded array.
    let src = "import core.json\nUser { id: i64, active: bool, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"active\\\":true,\\\"score\\\":10},{\\\"id\\\":2,\\\"active\\\":false,\\\"score\\\":99},{\\\"id\\\":3,\\\"active\\\":true,\\\"score\\\":5}]\")?\n  print(users.score.sum())\n  print(users.where(.active).score.count())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-struct-array-project", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "114\n2\n");
}

#[test]
fn json_decode_struct_array_pipeline_empty() {
    if !backend_available() {
        return;
    }
    // A pipeline over an empty decoded array folds to the identity (sum = 0) without touching the
    // null buffer.
    let src = "import core.json\nUser { id: i64, active: bool, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users.where(.active).score.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-struct-array-pipeline-empty", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

#[test]
fn json_decode_struct_array_map_sum() {
    if !backend_available() {
        return;
    }
    // `map(f)` over whole struct elements of an owned, dynamic `array<Struct>` (decoded from
    // JSON), loaded through the buffer pointer (`IndexPtr`): dbl(u) = u.score * 2 → 20 + 22 = 42.
    let src = "import core.json\nUser { score: i64, active: bool }\nfn dbl(u: User) -> i64 = u.score * 2\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"score\\\":10,\\\"active\\\":true},{\\\"score\\\":11,\\\"active\\\":true}]\")?\n  print(users.map(dbl).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-struct-map", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn json_decode_struct_array_where_struct_predicate() {
    if !backend_available() {
        return;
    }
    // A whole-struct `where` predicate over a decoded dynamic `array<Struct>` (loaded by
    // `IndexPtr`): keep score > 8 AND active → 10 + 12 = 22 (bob inactive, dot too low).
    let src = "import core.json\nUser { active: bool, score: i64 }\nfn good(u: User) -> bool = u.score > 8 && u.active\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"active\\\":true,\\\"score\\\":10},{\\\"active\\\":false,\\\"score\\\":99},{\\\"active\\\":true,\\\"score\\\":12}]\")?\n  print(users.where(good).score.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-struct-where-pred", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "22\n");
}

#[test]
fn json_decode_struct_array_where_field_then_map() {
    if !backend_available() {
        return;
    }
    // `where(.active)` then `map(f)` over the surviving whole structs of a dynamic `array<Struct>`:
    // dbl(u) = u.score * 2 over active rows → 10*2 + 5*2 = 30 (bob, inactive, is skipped).
    let src = "import core.json\nUser { active: bool, score: i64 }\nfn dbl(u: User) -> i64 = u.score * 2\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"active\\\":true,\\\"score\\\":10},{\\\"active\\\":false,\\\"score\\\":99},{\\\"active\\\":true,\\\"score\\\":5}]\")?\n  print(users.where(.active).map(dbl).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-struct-wheremap", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "30\n");
}

#[test]
fn array_index_fixed_and_owned() {
    if !backend_available() {
        return;
    }
    // Element access `recv[index]` on a fixed stack array and on an owned `array<i64>` (from
    // `json.decode`). Fixed: xs[2] = 30. Owned: ys[0] + ys[3] = 5 + 35 = 40. A computed index
    // (1 + 1) exercises a non-constant subscript.
    let src = "import core.json\nfn main() -> Result<(), Error> {\n  xs := [10, 20, 30, 40]\n  print(xs[1 + 1])\n  ys: array<i64> := json.decode(\"[5, 15, 25, 35]\")?\n  print(ys[0] + ys[3])\n  return Ok(())\n}\n";
    let out = build_and_run("array-index", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "30\n40\n");
}

#[test]
fn slice_index_through_param() {
    if !backend_available() {
        return;
    }
    // Indexing a `slice<i32>` (a `{ptr,len}` view borrowed from the caller's array): s[1] = 20.
    let src = "fn second(s: slice<i32>) -> i32 = s[1]\nfn main() -> i32 {\n  return second([10, 20, 30])\n}\n";
    let out = build_and_run("slice-index", src);
    assert_eq!(out.status.code(), Some(20));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn array_index_out_of_bounds_aborts() {
    if !backend_available() {
        return;
    }
    // An out-of-range index is a hard error: the bounds check calls the runtime, which aborts
    // (no silent UB / OOB read). The process dies via SIGABRT (no clean exit code).
    let src = "fn main() -> i32 {\n  xs := [1, 2, 3]\n  return xs[5] as i32\n}\n";
    let out = build_and_run("array-index-oob", src);
    assert_ne!(out.status.code(), Some(0), "out-of-bounds must not exit cleanly");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("index out of bounds"),
        "expected an out-of-bounds panic message, got: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn array_index_negative_aborts() {
    if !backend_available() {
        return;
    }
    // A negative index also fails the `index < 0` half of the bounds check and aborts.
    let src = "fn main() -> i32 {\n  xs := [1, 2, 3]\n  mut i := 0\n  i = i - 1\n  return xs[i] as i32\n}\n";
    let out = build_and_run("array-index-neg", src);
    assert_ne!(out.status.code(), Some(0), "a negative index must not exit cleanly");
    assert!(String::from_utf8_lossy(&out.stderr).contains("index out of bounds"));
}

#[test]
fn struct_array_element_field_dynamic() {
    if !backend_available() {
        return;
    }
    // MMv2 slice 8f: `users[i].field` on an owned `array<Struct>` from `json.decode`. Reads a
    // `str` field (zero-copy view), an `i32`, and a `bool` from specific elements — bounds-checked,
    // no whole-struct copy. Output: "ann\n99\ntrue\n".
    let src = "import core.json\nUser { id: i64, name: str, active: bool, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"name\\\":\\\"ann\\\",\\\"active\\\":true,\\\"score\\\":10},{\\\"id\\\":2,\\\"name\\\":\\\"bob\\\",\\\"active\\\":false,\\\"score\\\":99}]\")?\n  print(users[0].name)\n  print(users[1].score)\n  print(users[0].active)\n  return Ok(())\n}\n";
    let out = build_and_run("struct-array-elem-field", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ann\n99\ntrue\n");
}

#[test]
fn struct_array_element_field_fixed() {
    if !backend_available() {
        return;
    }
    // `ps[1].x` on a fixed stack `array<Struct>` (slot-addressed `IndexField`). = 30.
    let src = "P { x: i32, y: i32 }\nfn main() -> i32 {\n  ps := [P{x: 10, y: 20}, P{x: 30, y: 40}]\n  return ps[1].x\n}\n";
    let out = build_and_run("struct-array-elem-field-fixed", src);
    assert_eq!(out.status.code(), Some(30));
}

#[test]
fn struct_array_element_field_out_of_bounds_aborts() {
    if !backend_available() {
        return;
    }
    // `ps[i].field` is bounds-checked like a scalar index: an out-of-range element aborts.
    let src = "P { x: i32 }\nfn main() -> i32 {\n  ps := [P{x: 1}]\n  return ps[3].x\n}\n";
    let out = build_and_run("struct-array-elem-field-oob", src);
    assert_ne!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("index out of bounds"));
}

#[test]
fn fs_read_file_reads_owned_string() {
    if !backend_available() {
        return;
    }
    // std.fs: `fs.read_file(path)` reads the file into an owned `string` (heap buffer freed by the
    // binding's Drop). Write a temp file, read it back, print its content + byte length.
    let path = std::env::temp_dir().join("align-fs-read.txt");
    std::fs::write(&path, "hello from align\n42").expect("write temp file");
    let src = format!(
        "import std.fs\nfn main() -> Result<(), Error> {{\n  data := fs.read_file(\"{}\")?\n  print(data)\n  print(data.len())\n  return Ok(())\n}}\n",
        path.display()
    );
    let out = build_and_run("fs-read-file", &src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello from align\n42\n19\n");
}

#[test]
fn fs_read_file_missing_propagates_err() {
    if !backend_available() {
        return;
    }
    // A missing file is an I/O error → `Err`, propagated by `?` out of `main` (exit code 1).
    let missing = std::env::temp_dir().join("align-fs-does-not-exist-xyzzy.txt");
    let _ = std::fs::remove_file(&missing);
    let src = format!(
        "import std.fs\nfn main() -> Result<(), Error> {{\n  data := fs.read_file(\"{}\")?\n  print(data.len())\n  return Ok(())\n}}\n",
        missing.display()
    );
    let out = build_and_run("fs-read-missing", &src);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn fs_read_file_feeds_json_decode() {
    if !backend_available() {
        return;
    }
    // The draft.md §19 flow (minus `main(args)` / `io.stdout.write`): read a file into an owned
    // `string`, decode it as `array<User>` (the decoded `str` fields view the owned buffer), then
    // aggregate + index — all inside an arena. `where(.active).score.sum()` = 10 + 5 = 15; the
    // first user's name is "ann".
    let path = std::env::temp_dir().join("align-fs-users.json");
    std::fs::write(
        &path,
        "[{\"id\":1,\"name\":\"ann\",\"active\":true,\"score\":10},{\"id\":2,\"name\":\"bob\",\"active\":false,\"score\":99},{\"id\":3,\"name\":\"cyd\",\"active\":true,\"score\":5}]",
    )
    .expect("write json");
    let src = format!(
        "import core.json\nimport std.fs\nUser {{ id: i64, name: str, active: bool, score: i32 }}\nfn main() -> Result<(), Error> {{\n  arena {{\n    data := fs.read_file(\"{}\")?\n    users: array<User> := json.decode(data)?\n    print(users.where(.active).score.sum())\n    print(users[0].name)\n  }}\n  return Ok(())\n}}\n",
        path.display()
    );
    let out = build_and_run("fs-read-json", &src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "15\nann\n");
}

#[test]
fn io_stdout_write_has_no_newline() {
    if !backend_available() {
        return;
    }
    // std.io: `io.stdout.write(s)` writes the bytes with no trailing newline (unlike `print`), so
    // three writes concatenate: "a" + "b" + "c\n" = "abc\n".
    let src = "import std.io\nfn main() -> Result<(), Error> {\n  io.stdout.write(\"a\")?\n  io.stdout.write(\"b\")?\n  io.stdout.write(\"c\\n\")?\n  return Ok(())\n}\n";
    let out = build_and_run("io-stdout-write", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "abc\n");
}

#[test]
fn io_stdout_buffered_writes_then_explicit_flush() {
    if !backend_available() {
        return;
    }
    // std.io: `io.stdout.buffered()` opens a buffered writer; `.write(s)?` appends without a
    // syscall, `.flush()?` drains the buffer to the OS in one write. The three appends concatenate.
    let src = "import std.io\nfn main() -> Result<(), Error> {\n  out := io.stdout.buffered()\n  out.write(\"hello \")?\n  out.write(\"buffered \")?\n  out.write(\"world\\n\")?\n  out.flush()?\n  return Ok(())\n}\n";
    let out = build_and_run("io-buffered-flush", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello buffered world\n");
}

#[test]
fn io_stdout_buffered_flushes_on_drop() {
    if !backend_available() {
        return;
    }
    // Without an explicit `flush()`, the writer's `Drop` flushes any buffered bytes best-effort at
    // scope exit — so the output still appears. (`write` returns `Result`, propagated with `?`.)
    let src = "import std.io\nfn main() -> Result<(), Error> {\n  out := io.stdout.buffered()\n  out.write(\"flushed on drop\\n\")?\n  return Ok(())\n}\n";
    let out = build_and_run("io-buffered-drop", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "flushed on drop\n");
}

#[test]
fn io_stdout_buffered_writes_owned_string_without_consuming_it() {
    if !backend_available() {
        return;
    }
    // `out.write(owned_string)` auto-borrows the `string` as a `str` (zero-cost, non-consuming), so
    // the owned string stays usable afterwards (`s.len()` = 10). The buffered "owned-text\n" is
    // flushed at `out.flush()`; `print` flushes immediately, so the length (10) prints first.
    let src = "import std.io\nfn mk(a: str, b: str) -> string {\n  arena {\n    c := template \"{a}{b}\"\n    return c.clone()\n  }\n}\nfn main() -> Result<(), Error> {\n  out := io.stdout.buffered()\n  s := mk(\"owned-\", \"text\")\n  out.write(s)?\n  out.write(\"\\n\")?\n  print(s.len())\n  out.flush()?\n  return Ok(())\n}\n";
    let out = build_and_run("io-buffered-owned", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\nowned-text\n");
}

#[test]
fn io_stdout_buffered_threaded_through_functions() {
    if !backend_available() {
        return;
    }
    // A `writer` is a Move handle with a surface type name, so it can be passed to helpers and
    // threaded through a loop by pass-and-return — and returned inside a `Result<writer, Error>`
    // (the `?` unwraps the writer back out, moving it on). `emit` writes a line and hands the writer
    // back; `loop_n` recurses (the `if i >= n { return Ok(w) }` guard relies on the diverging-branch
    // move fix). Three lines are buffered, then flushed once.
    let src = concat!(
        "import std.io\n",
        "fn emit(w: writer, msg: str) -> Result<writer, Error> {\n  w.write(msg)?\n  w.write(\"\\n\")?\n  return Ok(w)\n}\n",
        "fn loop_n(w: writer, i: i64, n: i64) -> Result<writer, Error> {\n  if i >= n { return Ok(w) }\n  return loop_n(emit(w, \"line\")?, i + 1, n)\n}\n",
        "fn main() -> Result<(), Error> {\n  w := io.stdout.buffered()\n  w2 := loop_n(w, 0, 3)?\n  w2.flush()?\n  return Ok(())\n}\n",
    );
    let out = build_and_run("io-buffered-thread", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "line\nline\nline\n");
}

#[test]
fn io_stdout_buffered_rejects_args_and_bad_methods() {
    // `io.stdout.buffered()` takes no arguments; `.flush()` is writer-only (not on an int).
    assert!(check_errs(
        "io-buffered-args",
        "import std.io\nfn main() -> i32 {\n  out := io.stdout.buffered(5)\n  return 0\n}\n",
    ));
    assert!(check_errs(
        "io-flush-non-writer",
        "import std.io\nfn main() -> i32 {\n  x := 7\n  x.flush()\n  return 0\n}\n",
    ));
}

#[test]
fn s19_full_flow_read_decode_aggregate_write() {
    if !backend_available() {
        return;
    }
    // The draft.md §19 pipeline, end-to-end bar `main(args)`: read a file → decode `array<User>`
    // → `where(.active).score.sum()` → format with a `builder` → `io.stdout.write`. The active
    // users score 10 + 5 = 15. Output: "active score: 15\n".
    let path = std::env::temp_dir().join("align-s19-users.json");
    std::fs::write(
        &path,
        "[{\"id\":1,\"name\":\"ann\",\"active\":true,\"score\":10},{\"id\":2,\"name\":\"bob\",\"active\":false,\"score\":99},{\"id\":3,\"name\":\"cyd\",\"active\":true,\"score\":5}]",
    )
    .expect("write json");
    let src = format!(
        "import core.json\nimport std.fs\nimport std.io\nUser {{ id: i64, name: str, active: bool, score: i32 }}\nfn main() -> Result<(), Error> {{\n  arena {{\n    data := fs.read_file(\"{}\")?\n    users: array<User> := json.decode(data)?\n    total := users.where(.active).score.sum()\n    out := builder()\n    out.write(\"active score: \")\n    out.write_int(total)\n    out.write(\"\\n\")\n    io.stdout.write(out.to_string())?\n  }}\n  return Ok(())\n}}\n",
        path.display()
    );
    let out = build_and_run("s19-full", &src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "active score: 15\n");
}

#[test]
fn io_stdout_write_accepts_builder_directly() {
    if !backend_available() {
        return;
    }
    // `io.stdout.write(b)` accepts a `builder` directly (writes its bytes, no `to_string()`), the
    // draft.md §19 output form. The builder is borrowed (not consumed) and dropped normally after.
    // Output: "n=7\n".
    let src = "import std.io\nfn main() -> Result<(), Error> {\n  b := builder()\n  b.write(\"n=\")\n  b.write_int(7)\n  b.write(\"\\n\")\n  io.stdout.write(b)?\n  return Ok(())\n}\n";
    let out = build_and_run("io-stdout-write-builder", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "n=7\n");
}

#[test]
fn s19_verbatim_output_via_builder() {
    if !backend_available() {
        return;
    }
    // The full draft.md §19 body verbatim bar `main(args)`: `io.stdout.write(out)` with `out` a
    // builder directly (no `to_string()`). Output: "active score: 15\n".
    let path = std::env::temp_dir().join("align-s19v-users.json");
    std::fs::write(
        &path,
        "[{\"id\":1,\"name\":\"ann\",\"active\":true,\"score\":10},{\"id\":2,\"name\":\"bob\",\"active\":false,\"score\":99},{\"id\":3,\"name\":\"cyd\",\"active\":true,\"score\":5}]",
    )
    .expect("write json");
    let src = format!(
        "import core.json\nimport std.fs\nimport std.io\nUser {{ id: i64, name: str, active: bool, score: i32 }}\nfn main() -> Result<(), Error> {{\n  arena {{\n    data := fs.read_file(\"{}\")?\n    users: array<User> := json.decode(data)?\n    total := users.where(.active).score.sum()\n    out := builder()\n    out.write(\"active score: \")\n    out.write_int(total)\n    out.write(\"\\n\")\n    io.stdout.write(out)?\n  }}\n  return Ok(())\n}}\n",
        path.display()
    );
    let out = build_and_run("s19-verbatim", &src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "active score: 15\n");
}

#[test]
fn option_str_payload_construct_and_unwrap() {
    if !backend_available() {
        return;
    }
    // `str` is now a composite payload (`Scalar::Str`): `Option<str>` carries a `{ptr,len}` view
    // (Copy, region-tracked, never dropped). `Some("yes")` / `None` + `else`-unwrap. Output:
    // "yes\nno\n".
    let src = "fn pick(b: bool) -> Option<str> {\n  if b { return Some(\"yes\") }\n  return None\n}\nfn main() -> i32 {\n  print(pick(true) else \"no\")\n  print(pick(false) else \"no\")\n  return 0\n}\n";
    let out = build_and_run("option-str", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "yes\nno\n");
}

#[test]
fn result_str_payload_via_try() {
    if !backend_available() {
        return;
    }
    // `Result<str, Error>`: `?` unwraps to the `str` view. "hi" → printed.
    let src = "fn first(s: str) -> Result<str, Error> {\n  if s == \"\" { return Err(error(1)) }\n  return Ok(s)\n}\nfn main() -> Result<(), Error> {\n  v := first(\"hi\")?\n  print(v)\n  return Ok(())\n}\n";
    let out = build_and_run("result-str", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi\n");
}

#[test]
fn str_array_literal_index_and_len() {
    if !backend_available() {
        return;
    }
    // PR-B: `array<str>` — a fixed array of `str` views. Indexing yields a `str` (Copy view); the
    // element store/load reuses the scalar-array machinery (`[N x {ptr,len}]`). Output:
    // "beta\n3\nalpha\n".
    let src = "fn main() -> i32 {\n  xs := [\"alpha\", \"beta\", \"gamma\"]\n  print(xs[1])\n  print(xs.len())\n  print(xs[0])\n  return 0\n}\n";
    let out = build_and_run("str-array", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "beta\n3\nalpha\n");
}

#[test]
fn slice_str_param_index_and_len() {
    if !backend_available() {
        return;
    }
    // `slice<str>` — an array<str> literal coerces to a slice<str> at the call (ArrayToSlice). The
    // callee indexes it (`xs[1]`) and reads its length. `second` prints "bb"; `count` returns 2.
    let src = "fn second(xs: slice<str>) -> str = xs[1]\nfn count(xs: slice<str>) -> i64 = xs.len()\nfn main() -> i32 {\n  print(second([\"a\", \"bb\", \"ccc\"]))\n  return count([\"x\", \"y\"]) as i32\n}\n";
    let out = build_and_run("slice-str", src);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "bb\n");
}

#[test]
fn main_args_argv_marshalling() {
    if !backend_available() {
        return;
    }
    // PR-C: `main(args: array<str>)` — the C `main` wrapper marshals argv into an `array<str>`.
    // argv[0] is the executable, then the forwarded args. With ["one", "two"]: len = 3, args[1] =
    // "one". `io.stdout.write` (no newline) on args[1], then "\n".
    let src = "import std.io\npub fn main(args: array<str>) -> Result<(), Error> {\n  print(args.len())\n  io.stdout.write(args[1])?\n  io.stdout.write(\"\\n\")?\n  return Ok(())\n}\n";
    let out = build_and_run_args("main-args", src, &["one", "two"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\none\n");
}

#[test]
fn s19_main_args_reads_file() {
    if !backend_available() {
        return;
    }
    // The draft.md §19 program (bar the `json.decode<…>` generic-call syntax): `main(args)` reads
    // the file named by `args[1]`, decodes `array<User>`, aggregates, and writes via a builder.
    // = "active score: 15\n".
    let path = std::env::temp_dir().join("align-s19-args.json");
    std::fs::write(
        &path,
        "[{\"id\":1,\"name\":\"ann\",\"active\":true,\"score\":10},{\"id\":2,\"name\":\"bob\",\"active\":false,\"score\":99},{\"id\":3,\"name\":\"cyd\",\"active\":true,\"score\":5}]",
    )
    .expect("write json");
    let src = "import core.json\nimport std.fs\nimport std.io\nUser { id: i64, name: str, active: bool, score: i32 }\npub fn main(args: array<str>) -> Result<(), Error> {\n  arena {\n    data := fs.read_file(args[1])?\n    users: array<User> := json.decode(data)?\n    total := users.where(.active).score.sum()\n    out := builder()\n    out.write(\"active score: \")\n    out.write_int(total)\n    out.write(\"\\n\")\n    io.stdout.write(out)?\n  }\n  return Ok(())\n}\n";
    let out = build_and_run_args("s19-main-args", src, &[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "active score: 15\n");
}

#[test]
fn builder_writes_all_scalar_kinds() {
    if !backend_available() {
        return;
    }
    // MMv2 slice 7d: the builder's scalar writers match `print`/`template` coverage —
    // `write_int`/`write_bool`/`write_char`/`write_float` alongside `write` (str).
    // Output: "n=7 ok=true c=A pi=3.5\n".
    let src = "fn render() -> string {\n  b := builder()\n  b.write(\"n=\")\n  b.write_int(7)\n  b.write(\" ok=\")\n  b.write_bool(true)\n  b.write(\" c=\")\n  b.write_char('A')\n  b.write(\" pi=\")\n  b.write_float(3.5)\n  return b.to_string()\n}\nfn main() -> i32 {\n  print(render())\n  return 0\n}\n";
    let out = build_and_run("builder-scalars", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "n=7 ok=true c=A pi=3.5\n");
}

#[test]
fn empty_builder_to_string_is_safe() {
    if !backend_available() {
        return;
    }
    // An empty `builder().to_string()` yields an owned `string` with a *null* buffer and len 0.
    // `print` must not `from_raw_parts(null, 0)` (UB) — it emits just a newline; `.len()` is 0.
    // Output: "\n0\n".
    let src = "fn main() -> i32 {\n  b := builder()\n  s := b.to_string()\n  print(s)\n  print(s.len())\n  return 0\n}\n";
    let out = build_and_run("builder-empty", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "\n0\n");
}

#[test]
fn json_decode_flat_struct() {
    if !backend_available() {
        return;
    }
    // Decode {"id":40,"active":true} into User; `?` unwraps the struct. Unknown keys are
    // ignored and field order is irrelevant.
    let src = "import core.json\nUser { id: i64, active: bool }\nfn parse(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> Result<(), Error> {\n  u := parse(\"{\\\"active\\\": true, \\\"x\\\": 9, \\\"id\\\": 40}\")?\n  print(u.id)\n  if u.active { print(2) }\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "40\n2\n");
}

#[test]
fn json_decode_errors_on_missing_or_malformed() {
    if !backend_available() {
        return;
    }
    // A missing field (`active`) makes decode fail; `?` propagates → nonzero exit.
    let src = "import core.json\nUser { id: i64, active: bool }\nfn parse(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> Result<(), Error> {\n  u := parse(\"{\\\"id\\\": 40}\")?\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-missing", src);
    assert_eq!(out.status.code(), Some(1), "a missing field propagates an Err");
}

#[test]
fn json_decode_rejects_out_of_range_integers() {
    if !backend_available() {
        return;
    }
    // An integer that overflows the target field's width/sign must be rejected (Err → nonzero
    // exit), never silently truncated or sign-wrapped: `300` into a `u8` was becoming `44`.
    let parse = "import core.json\nBox { n: u8 }\nfn parse(s: str) -> Result<Box, Error> {\n  b: Box := json.decode(s)?\n  return Ok(b)\n}\n";
    for bad in ["{\\\"n\\\": 300}", "{\\\"n\\\": -1}"] {
        let src = format!("{parse}fn main() -> Result<(), Error> {{\n  b := parse(\"{bad}\")?\n  print(b.n)\n  return Ok(())\n}}\n");
        let out = build_and_run("json-decode-u8-oor", &src);
        assert_eq!(out.status.code(), Some(1), "out-of-range u8 value {bad} rejected");
    }
    // An in-range boundary value still decodes and reads back exactly.
    let ok = format!("{parse}fn main() -> Result<(), Error> {{\n  b := parse(\"{{\\\"n\\\": 255}}\")?\n  print(b.n)\n  return Ok(())\n}}\n");
    let out = build_and_run("json-decode-u8-ok", &ok);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "255\n");
}

#[test]
fn json_decode_float_and_mixed_scalars() {
    if !backend_available() {
        return;
    }
    // Decode f64 / f32 / i32 / bool fields together.
    let src = "import core.json\nPt { x: f64, y: f32, n: i32, on: bool }\nfn parse(s: str) -> Result<Pt, Error> {\n  p: Pt := json.decode(s)?\n  return Ok(p)\n}\nfn main() -> Result<(), Error> {\n  p := parse(\"{\\\"x\\\": 1.5, \\\"y\\\": 0.25, \\\"n\\\": 40, \\\"on\\\": true}\")?\n  print(p.x)\n  print(p.y)\n  print(p.n)\n  if p.on { print(1) }\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-float", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1.5\n0.25\n40\n1\n");
}

#[test]
fn json_decode_skips_unknown_string_value() {
    if !backend_available() {
        return;
    }
    // An unknown key with a string value is skipped, not a parse error.
    let src = "import core.json\nP { a: i32 }\nfn parse(s: str) -> Result<P, Error> {\n  p: P := json.decode(s)?\n  return Ok(p)\n}\nfn main() -> Result<(), Error> {\n  p := parse(\"{\\\"note\\\": \\\"hi\\\", \\\"a\\\": 42}\")?\n  print(p.a)\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-skipstr", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn json_decode_then_encode_roundtrips() {
    if !backend_available() {
        return;
    }
    // §19 spirit: decode → (re-)encode. {"id":7,"active":false} round-trips.
    let src = "import core.json\nUser { id: i64, active: bool }\nfn run(s: str) -> Result<(), Error> {\n  u: User := json.decode(s)?\n  print(json.encode(u))\n  return Ok(())\n}\nfn main() -> Result<(), Error> {\n  run(\"{\\\"id\\\": 7, \\\"active\\\": false}\")?\n  return Ok(())\n}\n";
    let out = build_and_run("json-roundtrip", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "{\"id\":7,\"active\":false}\n");
}

#[test]
fn json_encode_flat_struct() {
    if !backend_available() {
        return;
    }
    // A struct of i64/str/bool encodes to a JSON object; the str field's embedded quote
    // is JSON-escaped.
    let src = "import core.json\nUser { id: i64, name: str, active: bool }\nfn main() -> i32 {\n  u := User{id: 7, name: \"a\\\"b\", active: true}\n  print(json.encode(u))\n  return 0\n}\n";
    let out = build_and_run("json-encode", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "{\"id\":7,\"name\":\"a\\\"b\",\"active\":true}\n");
}

#[test]
fn json_encode_struct_array() {
    if !backend_available() {
        return;
    }
    // A fixed struct array encodes to a JSON array of objects (str fields escaped).
    let src = "import core.json\nUser { id: i64, name: str, active: bool }\nfn main() -> i32 {\n  us := [User{id: 1, name: \"a\", active: true}, User{id: 2, name: \"b\\n\", active: false}]\n  print(json.encode(us))\n  return 0\n}\n";
    let out = build_and_run("json-encode-array", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "[{\"id\":1,\"name\":\"a\",\"active\":true},{\"id\":2,\"name\":\"b\\n\",\"active\":false}]\n"
    );
}

#[test]
fn struct_str_field_roundtrips() {
    if !backend_available() {
        return;
    }
    // A str struct field can be stored and read back.
    let src = "P { tag: str, n: i32 }\nfn main() -> i32 {\n  p := P{tag: \"hi\", n: 5}\n  print(p.tag)\n  return p.n\n}\n";
    let out = build_and_run("struct-str-field", src);
    assert_eq!(out.status.code(), Some(5));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi\n");
}

#[test]
fn print_and_template_float() {
    if !backend_available() {
        return;
    }
    // f64 and f32 render as shortest round-trip decimals; whole values get a `.0`.
    let src = "fn main() -> i32 {\n  x := 3.14\n  y := 1.0\n  z: f32 := 0.5\n  print(x)\n  print(y)\n  print(template \"x={x} half={z}\")\n  return 0\n}\n";
    let out = build_and_run("float-print", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3.14\n1.0\nx=3.14 half=0.5\n");
}

#[test]
fn print_and_template_bool_char() {
    if !backend_available() {
        return;
    }
    // print/templates render bool as true/false and char as its UTF-8 (incl. multibyte).
    let src = "fn main() -> i32 {\n  flag := 3 > 2\n  c := 'A'\n  m := 'あ'\n  print(flag)\n  print(c)\n  print(template \"flag={flag} ch={c} uc={m} cmp={1 > 5}\")\n  return 0\n}\n";
    let out = build_and_run("bool-char", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\nA\nflag=true ch=A uc=あ cmp=false\n");
}

#[test]
fn string_concatenation_is_rejected() {
    let src = "fn main() -> i32 {\n  a := \"foo\"\n  b := \"bar\"\n  print(a + b)\n  return 0\n}\n";
    assert!(check_errs("concat-error", src));
}

#[test]
fn empty_string_build() {
    if !backend_available() {
        return;
    }
    // Empty template exercises the zero-length dangling-pointer path.
    let src = "fn main() -> i32 {\n  e := \"\"\n  arena {\n    print(template \"{e}\")\n  }\n  return 0\n}\n";
    let out = build_and_run("empty-build", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "\n");
}

#[test]
fn string_escapes() {
    if !backend_available() {
        return;
    }
    // The closed literal escape set is decoded by the lexer and then emitted as UTF-8 constants.
    let src = "fn main() -> i32 {\n  print(\"a\\tb\")\n  print(\"\\u{41}\\u{1F600}\")\n  print('\\u{41}')\n  return \"\\r\\0\\\\\\\"\\'\".len() as i32\n}\n";
    let out = build_and_run("str-escape", src);
    assert_eq!(out.status.code(), Some(5));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\tb\nA😀\nA\n");
}

#[test]
fn str_predicates_basic() {
    if !backend_available() {
        return;
    }
    // contains/starts_with/ends_with on a `str` literal. Bits: c1=1 c2=0 s1=4 s2=0 e1=16 e2=0 → 21.
    let src = "fn b2i(b: bool) -> i32 {\n  if b { return 1 }\n  return 0\n}\nfn main() -> i32 {\n  s := \"hello, align\"\n  c1 := b2i(s.contains(\"align\"))\n  c2 := b2i(s.contains(\"xyz\"))\n  s1 := b2i(s.starts_with(\"hello\"))\n  s2 := b2i(s.starts_with(\"align\"))\n  e1 := b2i(s.ends_with(\"align\"))\n  e2 := b2i(s.ends_with(\"hello\"))\n  return c1 + c2 * 2 + s1 * 4 + s2 * 8 + e1 * 16 + e2 * 32\n}\n";
    let out = build_and_run("str-pred-basic", src);
    assert_eq!(out.status.code(), Some(21));
}

#[test]
fn str_predicates_edge_cases() {
    if !backend_available() {
        return;
    }
    // Empty needle is always present (1+2+4); a needle longer than the haystack is absent (0). → 7.
    let src = "fn b2i(b: bool) -> i32 {\n  if b { return 1 }\n  return 0\n}\nfn main() -> i32 {\n  s := \"abc\"\n  empty := b2i(s.contains(\"\"))\n  estart := b2i(s.starts_with(\"\"))\n  eend := b2i(s.ends_with(\"\"))\n  toolong := b2i(s.contains(\"abcd\"))\n  return empty + estart * 2 + eend * 4 + toolong * 8\n}\n";
    let out = build_and_run("str-pred-edge", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn str_predicates_on_owned_string_dont_consume() {
    if !backend_available() {
        return;
    }
    // The receiver is an owned `string` (auto-borrowed): the predicate reads bytes, never moves it,
    // so `s` is still usable afterwards (s.len() == 6). Bits 1+2+4+8 → 15.
    let src = "fn b2i(b: bool) -> i32 {\n  if b { return 1 }\n  return 0\n}\nfn mk(a: str, b: str) -> string {\n  arena {\n    c := template \"{a}{b}\"\n    return c.clone()\n  }\n}\nfn main() -> i32 {\n  s := mk(\"foo\", \"bar\")\n  r := b2i(s.contains(\"oba\")) + b2i(s.starts_with(\"foo\")) * 2 + b2i(s.ends_with(\"bar\")) * 4\n  return r + b2i(s.len() == 6) * 8\n}\n";
    let out = build_and_run("str-pred-owned", src);
    assert_eq!(out.status.code(), Some(15));
}

#[test]
fn str_predicates_are_byte_oriented_utf8() {
    if !backend_available() {
        return;
    }
    // Multi-byte UTF-8: contains/starts_with/ends_with operate on bytes, so whole-character
    // needles match. Bits 1+2+4 → 7.
    let src = "fn b2i(b: bool) -> i32 {\n  if b { return 1 }\n  return 0\n}\nfn main() -> i32 {\n  s := \"café みかん\"\n  c1 := b2i(s.contains(\"みかん\"))\n  c2 := b2i(s.ends_with(\"ん\"))\n  c3 := b2i(s.starts_with(\"café\"))\n  return c1 + c2 * 2 + c3 * 4\n}\n";
    let out = build_and_run("str-pred-utf8", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn str_trim_strips_ascii_whitespace() {
    if !backend_available() {
        return;
    }
    // trim/trim_start/trim_end yield borrowed sub-str views. Print each (visible bounds) + length.
    let src = "fn main() -> i32 {\n  s := \"  hi  \"\n  print(s.trim())\n  print(s.trim().len())\n  print(s.trim_start())\n  print(s.trim_start().len())\n  print(s.trim_end())\n  print(s.trim_end().len())\n  return 0\n}\n";
    let out = build_and_run("str-trim", src);
    assert_eq!(out.status.code(), Some(0));
    // "hi"(2), "hi  "(4), "  hi"(4). The whitespace is visible in the start/end variants.
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "hi\n2\nhi  \n4\n  hi\n4\n"
    );
}

#[test]
fn str_trim_edge_cases() {
    if !backend_available() {
        return;
    }
    // All-whitespace → empty (len 0); no-whitespace → unchanged; empty → empty. Bits 1+2+4 → 7.
    let src = "fn b2i(b: bool) -> i32 {\n  if b { return 1 }\n  return 0\n}\nfn main() -> i32 {\n  allws := \" \\t\\n \"\n  none := \"abc\"\n  empty := \"\"\n  r1 := b2i(allws.trim().len() == 0)\n  r2 := b2i(none.trim() == \"abc\")\n  r3 := b2i(empty.trim().len() == 0)\n  return r1 + r2 * 2 + r3 * 4\n}\n";
    let out = build_and_run("str-trim-edge", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn str_trim_on_owned_string_view_is_borrowed() {
    if !backend_available() {
        return;
    }
    // The receiver is an owned `string`; trim borrows it (no move), so it stays usable after.
    // The trimmed view ("foobar", len 6) feeds an equality, then s.len() (8, incl. the padding).
    let src = "fn b2i(b: bool) -> i32 {\n  if b { return 1 }\n  return 0\n}\nfn mk(a: str, b: str) -> string {\n  arena {\n    c := template \"{a}{b}\"\n    return c.clone()\n  }\n}\nfn main() -> i32 {\n  s := mk(\" foobar \", \"\")\n  trimmed := b2i(s.trim() == \"foobar\")\n  return trimmed + b2i(s.len() == 8) * 2\n}\n";
    let out = build_and_run("str-trim-owned", src);
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn io_stderr_buffered_writes_to_stderr_not_stdout() {
    if !backend_available() {
        return;
    }
    // `io.stderr.buffered()` opens the same buffered writer over fd 2. The bytes land on stderr
    // (drained at `flush()?`), and stdout stays empty — the writer is fd-parameterized.
    let src = "import std.io\nfn main() -> Result<(), Error> {\n  log := io.stderr.buffered()\n  log.write(\"warn: \")?\n  log.write(\"disk low\\n\")?\n  log.flush()?\n  return Ok(())\n}\n";
    let out = build_and_run("io-stderr-buffered", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "warn: disk low\n");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "", "nothing leaks to stdout");
}

#[test]
fn io_stderr_buffered_flushes_on_drop() {
    if !backend_available() {
        return;
    }
    // No explicit flush: the writer's Drop drains the buffer to fd 2 at scope exit.
    let src = "import std.io\nfn main() -> Result<(), Error> {\n  log := io.stderr.buffered()\n  log.write(\"dropped to stderr\\n\")?\n  return Ok(())\n}\n";
    let out = build_and_run("io-stderr-drop", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "dropped to stderr\n");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn io_stdout_and_stderr_buffered_are_independent_sinks() {
    if !backend_available() {
        return;
    }
    // Two buffered writers over different fds in the same program route to the right streams.
    let src = "import std.io\nfn main() -> Result<(), Error> {\n  out := io.stdout.buffered()\n  err := io.stderr.buffered()\n  out.write(\"to-out\\n\")?\n  err.write(\"to-err\\n\")?\n  out.flush()?\n  err.flush()?\n  return Ok(())\n}\n";
    let out = build_and_run("io-both-sinks", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "to-out\n");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "to-err\n");
}

#[test]
fn str_range_slice_all_forms() {
    if !backend_available() {
        return;
    }
    // `s[a..b]` / `s[a..]` / `s[..b]` / `s[..]` all yield a borrowed sub-`str` (no allocation).
    let src = "fn main() -> i32 {\n  s := \"hello world\"\n  print(s[0..5])\n  print(s[6..])\n  print(s[..5])\n  print(s[..])\n  print(s[0..5].len())\n  return 0\n}\n";
    let out = build_and_run("str-range-forms", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\nworld\nhello\nhello world\n5\n");
}

#[test]
fn str_range_slice_of_owned_string() {
    if !backend_available() {
        return;
    }
    // An owned `string` receiver auto-borrows to a `str`; the sub-slice views it without moving it,
    // so the string stays usable (its full len is 8). The slice "ooba" feeds an equality.
    let src = "fn b2i(b: bool) -> i32 {\n  if b { return 1 }\n  return 0\n}\nfn mk(a: str, b: str) -> string {\n  arena {\n    c := template \"{a}{b}\"\n    return c.clone()\n  }\n}\nfn main() -> i32 {\n  s := mk(\"foob\", \"arxx\")\n  hit := b2i(s[2..6] == \"obar\")\n  return hit + b2i(s.len() == 8) * 2\n}\n";
    let out = build_and_run("str-range-owned", src);
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn array_range_slice_yields_slice() {
    if !backend_available() {
        return;
    }
    // `a[1..4]` borrows a `slice<i64>` over elements [20,30,40]; index + len work on the sub-slice.
    let src = "fn main() -> i32 {\n  a := [10, 20, 30, 40, 50]\n  s := a[1..4]\n  if s.len() == 3 {\n    return (s[0] + s[1] + s[2]) as i32\n  }\n  return 0\n}\n";
    let out = build_and_run("array-range", src);
    assert_eq!(out.status.code(), Some(90)); // 20 + 30 + 40
}

#[test]
fn slice_reslice_composes() {
    if !backend_available() {
        return;
    }
    // Re-slicing a slice: `a[1..5]` = [20,30,40,50], then `[1..3]` = [30,40].
    let src = "fn main() -> i32 {\n  a := [10, 20, 30, 40, 50]\n  s := a[1..5]\n  t := s[1..3]\n  return (t[0] + t[1]) as i32\n}\n";
    let out = build_and_run("slice-reslice", src);
    assert_eq!(out.status.code(), Some(70)); // 30 + 40
}

#[test]
fn str_range_slice_out_of_bounds_aborts() {
    if !backend_available() {
        return;
    }
    // `end > len` fails the range bounds check and aborts (no silent OOB read).
    let src = "fn main() -> i32 {\n  s := \"abc\"\n  print(s[1..10])\n  return 0\n}\n";
    let out = build_and_run("str-range-oob", src);
    assert_ne!(out.status.code(), Some(0), "an out-of-range slice must not exit cleanly");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("out of bounds"),
        "expected a bounds panic, got: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn str_range_slice_accepts_utf8_scalar_boundaries() {
    if !backend_available() {
        return;
    }
    // Byte indices at every scalar boundary remain valid for 1/2/3/4-byte UTF-8 scalars.
    let src = "fn main() -> i32 {\n  s := \"aé日🦀z\"\n  print(s[0..1])\n  print(s[1..3])\n  print(s[3..6])\n  print(s[6..10])\n  print(s[10..])\n  print(s[..])\n  return 0\n}\n";
    let out = build_and_run("str-range-utf8-boundaries", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\né\n日\n🦀\nz\naé日🦀z\n");
}

#[test]
fn str_range_slice_rejects_split_utf8_scalar() {
    if !backend_available() {
        return;
    }
    for (name, range) in [("start", "2..3"), ("end", "1..2")] {
        let src = format!("fn main() -> i32 {{\n  s := \"aéz\"\n  print(s[{range}])\n  return 0\n}}\n");
        let out = build_and_run(&format!("str-range-utf8-{name}"), &src);
        assert_ne!(out.status.code(), Some(0), "a range that splits a UTF-8 scalar must abort");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("not a UTF-8 boundary"),
            "expected a UTF-8-boundary panic, got: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn array_range_slice_inverted_bounds_aborts() {
    if !backend_available() {
        return;
    }
    // `start > end` also fails the range check and aborts. Both 3 and 1 are individually valid
    // indices for length 4, so the message must report the whole range (`3..1`), not a misleading
    // single (index, len) pair.
    let src = "fn main() -> i32 {\n  a := [1, 2, 3, 4]\n  s := a[3..1]\n  return s.len() as i32\n}\n";
    let out = build_and_run("array-range-inverted", src);
    assert_ne!(out.status.code(), Some(0), "inverted bounds must not exit cleanly");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("out of bounds"), "got: {err}");
    assert!(err.contains("3..1"), "the range failure must report the offending range, got: {err}");
}

#[test]
fn str_find_returns_byte_index_option() {
    if !backend_available() {
        return;
    }
    // `find` yields Option<i64>: the first byte index, or None (here unwrapped to -1 via `else`).
    // An empty needle matches at 0.
    let src = "fn main() -> i32 {\n  s := \"hello world\"\n  print(s.find(\"world\") else { -1 })\n  print(s.find(\"xyz\") else { -1 })\n  print(s.find(\"hello\") else { -1 })\n  print(s.find(\"\") else { -1 })\n  return 0\n}\n";
    let out = build_and_run("str-find", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n-1\n0\n0\n");
}

#[test]
fn str_find_then_slice_splits_key_value() {
    if !backend_available() {
        return;
    }
    // The motivating composition: find the separator, then slice around it (zero-copy) — `find`
    // returning an index is useful precisely because range slicing exists.
    let src = "fn main() -> i32 {\n  s := \"key=value\"\n  i := s.find(\"=\") else { return 1 }\n  print(s[..i])\n  print(s[i+1..])\n  return 0\n}\n";
    let out = build_and_run("str-find-slice", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "key\nvalue\n");
}

#[test]
fn str_find_on_owned_string() {
    if !backend_available() {
        return;
    }
    // An owned `string` receiver auto-borrows; `find` reads it without moving it (still usable).
    let src = "fn mk(a: str, b: str) -> string {\n  arena {\n    c := template \"{a}{b}\"\n    return c.clone()\n  }\n}\nfn main() -> i32 {\n  s := mk(\"abc\", \"def\")\n  i := s.find(\"cd\") else { return 9 }\n  return (i + s.len()) as i32\n}\n";
    let out = build_and_run("str-find-owned", src);
    assert_eq!(out.status.code(), Some(8)); // index 2 + len 6
}

#[test]
fn str_rfind_returns_last_index() {
    if !backend_available() {
        return;
    }
    // `rfind` is the from-the-end sibling: last occurrence, or None (→ -1 via else). Classic use:
    // the final "." of a filename. Empty needle matches at the end (len).
    let src = "fn main() -> i32 {\n  s := \"a.b.c\"\n  print(s.rfind(\".\") else { -1 })\n  print(s.find(\".\") else { -1 })\n  print(s.rfind(\"x\") else { -1 })\n  print(s.rfind(\"\") else { -1 })\n  return 0\n}\n";
    let out = build_and_run("str-rfind", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n1\n-1\n5\n");
}

#[test]
fn str_eq_ignore_ascii_case() {
    if !backend_available() {
        return;
    }
    // ASCII case-insensitive equality (for headers/protocols). Different lengths never match; a
    // non-ASCII byte compares exactly (so "é" upper vs lower would differ — here all-ASCII cases).
    let src = "fn b2i(b: bool) -> i32 {\n  if b { return 1 }\n  return 0\n}\nfn main() -> i32 {\n  a := b2i(\"Content-Type\".eq_ignore_ascii_case(\"content-type\"))\n  b := b2i(\"GET\".eq_ignore_ascii_case(\"get\"))\n  c := b2i(\"abc\".eq_ignore_ascii_case(\"abd\"))\n  d := b2i(\"abc\".eq_ignore_ascii_case(\"abcd\"))\n  return a + b * 2 + c * 4 + d * 8\n}\n";
    let out = build_and_run("str-eq-ic", src);
    assert_eq!(out.status.code(), Some(3)); // a=1, b=1, c=0, d=0 → 1 + 2 = 3
}
