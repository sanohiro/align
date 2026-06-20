//! M5 end-to-end: strings (literals + `print`). Requires LLVM/cc, so skip where absent.

use align_driver::{backend_available, check, emit_object_file, link_executable, lower_to_mir};
use align_span::SourceMap;

fn build_and_run(name: &str, src: &str) -> std::process::Output {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let obj = dir.join(format!("align-test-{name}.o"));
    let exe = dir.join(format!("align-test-{name}"));
    emit_object_file(&mir, &obj).expect("codegen");
    link_executable(&obj, &exe).expect("link");
    std::process::Command::new(&exe).output().expect("run")
}

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
    // `{expr}` holes: arithmetic and an inline str concat, not just bare names.
    let src = "fn main() -> i32 {\n  a := 20\n  b := 22\n  name := \"world\"\n  print(template \"sum={a + b} dbl={a * 2} hi={name + \\\"!\\\"}\")\n  return 0\n}\n";
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
fn json_decode_str_field_zero_copy() {
    if !backend_available() {
        return;
    }
    // A `str` field decodes as a zero-copy view into the input buffer (MMv2 slice 6): the
    // printed name comes straight from the input bytes, no allocation. id=7, name="alice".
    let src = "User { id: i64, name: str, active: bool }\nfn parse(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> Result<(), Error> {\n  u := parse(\"{\\\"id\\\": 7, \\\"name\\\": \\\"alice\\\", \\\"active\\\": true}\")?\n  print(u.id)\n  print(u.name)\n  if u.active { print(1) }\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-str", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\nalice\n1\n");
}

#[test]
fn str_clone_escapes_arena_as_owned_string() {
    if !backend_available() {
        return;
    }
    // `str.clone()` deep-copies into a heap-owned `string` that outlives the arena its source
    // was built in (MMv2 slice 7): the concat `c` lives in the arena (freed at `}`), but the
    // returned clone owns its own buffer. `print` borrows (so `s` is still usable for `.len()`),
    // and the owned string is freed once at function exit.
    let src = "fn longer(a: str, b: str) -> string {\n  arena {\n    c := a + b\n    return c.clone()\n  }\n}\nfn main() -> i32 {\n  s := longer(\"foo\", \"bar\")\n  print(s)\n  print(s.len())\n  return 0\n}\n";
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
    let src = "User { id: i64, name: str }\nfn decode(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> Result<(), Error> {\n  u := decode(\"{\\\"id\\\": 7, \\\"name\\\": \\\"alice\\\"}\")?\n  owned := u.name.clone()\n  print(owned)\n  print(owned.len())\n  return Ok(())\n}\n";
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
    let src = "fn show(label: str) -> i64 {\n  print(label)\n  return label.len()\n}\nfn mk(a: str, b: str) -> string {\n  arena {\n    c := a + b\n    return c.clone()\n  }\n}\nfn main() -> i32 {\n  s := mk(\"foo\", \"bar\")\n  n := show(s)\n  print(n)\n  print(s.len())\n  return 0\n}\n";
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
    let src = "User { id: i64, active: bool }\nfn parse(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> Result<(), Error> {\n  u := parse(\"{\\\"active\\\": true, \\\"x\\\": 9, \\\"id\\\": 40}\")?\n  print(u.id)\n  if u.active { print(2) }\n  return Ok(())\n}\n";
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
    let src = "User { id: i64, active: bool }\nfn parse(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> Result<(), Error> {\n  u := parse(\"{\\\"id\\\": 40}\")?\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-missing", src);
    assert_eq!(out.status.code(), Some(1), "a missing field propagates an Err");
}

#[test]
fn json_decode_float_and_mixed_scalars() {
    if !backend_available() {
        return;
    }
    // Decode f64 / f32 / i32 / bool fields together.
    let src = "Pt { x: f64, y: f32, n: i32, on: bool }\nfn parse(s: str) -> Result<Pt, Error> {\n  p: Pt := json.decode(s)?\n  return Ok(p)\n}\nfn main() -> Result<(), Error> {\n  p := parse(\"{\\\"x\\\": 1.5, \\\"y\\\": 0.25, \\\"n\\\": 40, \\\"on\\\": true}\")?\n  print(p.x)\n  print(p.y)\n  print(p.n)\n  if p.on { print(1) }\n  return Ok(())\n}\n";
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
    let src = "P { a: i32 }\nfn parse(s: str) -> Result<P, Error> {\n  p: P := json.decode(s)?\n  return Ok(p)\n}\nfn main() -> Result<(), Error> {\n  p := parse(\"{\\\"note\\\": \\\"hi\\\", \\\"a\\\": 42}\")?\n  print(p.a)\n  return Ok(())\n}\n";
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
    let src = "User { id: i64, active: bool }\nfn run(s: str) -> Result<(), Error> {\n  u: User := json.decode(s)?\n  print(json.encode(u))\n  return Ok(())\n}\nfn main() -> Result<(), Error> {\n  run(\"{\\\"id\\\": 7, \\\"active\\\": false}\")?\n  return Ok(())\n}\n";
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
    let src = "User { id: i64, name: str, active: bool }\nfn main() -> i32 {\n  u := User{id: 7, name: \"a\\\"b\", active: true}\n  print(json.encode(u))\n  return 0\n}\n";
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
    let src = "User { id: i64, name: str, active: bool }\nfn main() -> i32 {\n  us := [User{id: 1, name: \"a\", active: true}, User{id: 2, name: \"b\\n\", active: false}]\n  print(json.encode(us))\n  return 0\n}\n";
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
fn string_concatenation() {
    if !backend_available() {
        return;
    }
    // a + b + "!" inside an arena (arena-backed, no leak); also outside (leaked).
    let src = "fn main() -> i32 {\n  a := \"foo\"\n  b := \"bar\"\n  arena {\n    print(a + b + \"!\")\n  }\n  print(a + b)\n  return 0\n}\n";
    let out = build_and_run("concat", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "foobar!\nfoobar\n");
}

#[test]
fn empty_string_build() {
    if !backend_available() {
        return;
    }
    // Empty concat/template (exercises the zero-length dangling-pointer path).
    let src = "fn main() -> i32 {\n  e := \"\"\n  arena {\n    print(e + e)\n    print(template \"{e}\")\n  }\n  return 0\n}\n";
    let out = build_and_run("empty-build", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "\n\n");
}

#[test]
fn string_escapes() {
    if !backend_available() {
        return;
    }
    // \t and \n inside a literal are decoded by the lexer.
    let src = "fn main() -> i32 {\n  print(\"a\\tb\")\n  return 0\n}\n";
    let out = build_and_run("str-escape", src);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\tb\n");
}
