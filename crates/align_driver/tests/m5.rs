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
    let src = "fn parse(s: str) -> Result<array<i64>, Error> {\n  xs: array<i64> := json.decode(s)?\n  return Ok(xs)\n}\nfn main() -> Result<(), Error> {\n  xs := parse(\"[10, 20, 30, 40]\")?\n  print(xs.sum())\n  print(xs.len())\n  return Ok(())\n}\n";
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
    let src = "fn main() -> Result<(), Error> {\n  xs: array<f64> := json.decode(\"[1.5, 2.5, 3.0]\")?\n  print(xs.sum())\n  return Ok(())\n}\n";
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
    let src = "fn main() -> Result<(), Error> {\n  xs: array<i64> := json.decode(\"[1, 2, oops]\")?\n  print(xs.sum())\n  return Ok(())\n}\n";
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
    let src = "fn main() -> Result<(), Error> {\n  b := builder()\n  b.write(\"[1, 2, 3]\")\n  doc := b.to_string()\n  xs: array<i64> := json.decode(doc)?\n  print(xs.sum())\n  print(xs.len())\n  return Ok(())\n}\n";
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
    let src = "fn main() -> Result<(), Error> {\n  xs: array<i64> := json.decode(\"[]\")?\n  print(xs.sum())\n  print(xs.len())\n  return Ok(())\n}\n";
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
    let src = "User { id: i64, name: str, active: bool }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"name\\\":\\\"ann\\\",\\\"active\\\":true},{\\\"id\\\":2,\\\"name\\\":\\\"bob\\\",\\\"active\\\":false}]\")?\n  print(users.len())\n  return Ok(())\n}\n";
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
    let src = "P { x: i64, y: i64 }\nfn main() -> Result<(), Error> {\n  ps: array<P> := json.decode(\"[{\\\"x\\\":1,\\\"y\\\":2},{\\\"x\\\":3,\\\"y\\\":4},{\\\"x\\\":5,\\\"y\\\":6}]\")?\n  print(ps.len())\n  return Ok(())\n}\n";
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
    let src = "User { id: i64, name: str }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users.len())\n  return Ok(())\n}\n";
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
    let src = "User { id: i64, name: str, active: bool }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"name\\\":\\\"ann\\\"}]\")?\n  print(users.len())\n  return Ok(())\n}\n";
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
    let src = "User { id: i64, name: str, active: bool, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"name\\\":\\\"ann\\\",\\\"active\\\":true,\\\"score\\\":10},{\\\"id\\\":2,\\\"name\\\":\\\"bob\\\",\\\"active\\\":false,\\\"score\\\":99},{\\\"id\\\":3,\\\"name\\\":\\\"cyd\\\",\\\"active\\\":true,\\\"score\\\":5}]\")?\n  total := users.where(.active).score.sum()\n  print(total)\n  return Ok(())\n}\n";
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
    let src = "User { id: i64, active: bool, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"active\\\":true,\\\"score\\\":10},{\\\"id\\\":2,\\\"active\\\":false,\\\"score\\\":99},{\\\"id\\\":3,\\\"active\\\":true,\\\"score\\\":5}]\")?\n  print(users.score.sum())\n  print(users.where(.active).score.count())\n  return Ok(())\n}\n";
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
    let src = "User { id: i64, active: bool, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users.where(.active).score.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("json-decode-struct-array-pipeline-empty", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
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
