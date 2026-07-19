//! Move **handle** struct fields (F1② of the pkg.web plan — the request `Ctx` owning its
//! `http_request_ctx`). A struct field may be a bare pointer handle (a `buffer`, `file`,
//! reader/writer, socket, http request/response/…/ctx/stream, cli command/parsed). Such a field
//! makes the enclosing struct a **Move** type whose recursive drop closes/frees the handle exactly
//! once (`drop_struct_fields`'s handle arm → the null-safe `*_free`, shared with a standalone
//! handle local via `handle_free_fn`). Move-once discipline is enforced by `MoveCheck`.

mod common;
use common::*;

#[test]
fn handle_field_construct_and_drop() {
    if !backend_available() {
        return;
    }
    // A struct owning a `buffer` handle: build it, read a scalar field, then let it drop at scope
    // exit. A clean exit (0) proves `drop_struct_fields` freed the handle exactly once (its
    // `buffer_free` is null-safe; a leak/double-free would abort).
    let src = concat!(
        "Holder { buf: buffer, tag: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  h := Holder { buf: buffer(64), tag: 7 }\n",
        "  print(h.tag)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("handlefield-drop", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn handle_field_struct_moved_into_fn_no_double_free() {
    if !backend_available() {
        return;
    }
    // The Move struct is consumed by a by-value call: the callee drops the handle, and the caller's
    // drop flag must suppress a second free (a double `buffer_free` would abort). Clean exit proves
    // the handle is freed exactly once across the move.
    let src = concat!(
        "Holder { buf: buffer, tag: i64 }\n",
        "fn consume(h: Holder) -> i64 = h.tag\n",
        "fn main() -> Result<(), Error> {\n",
        "  h := Holder { buf: buffer(64), tag: 7 }\n",
        "  print(consume(h))\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("handlefield-move", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn handle_field_owned_struct_returned_then_dropped() {
    if !backend_available() {
        return;
    }
    // A struct that *owns* its handle borrows nothing → its region is Static, so it is freely
    // returnable (the owner moves with it). The caller then drops it once.
    let src = concat!(
        "Holder { buf: buffer, tag: i64 }\n",
        "fn make(n: i64) -> Holder = Holder { buf: buffer(64), tag: n }\n",
        "fn main() -> Result<(), Error> {\n",
        "  h := make(42)\n",
        "  print(h.tag)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("handlefield-return", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn handle_field_use_after_move_rejected() {
    // Consuming the Move struct twice is a use-after-move — a clean error, not a double-free.
    assert!(check_errs(
        "handlefield-uam",
        "Holder { buf: buffer, tag: i64 }\nfn consume(h: Holder) -> i64 = h.tag\nfn main() -> Result<(), Error> {\n  h := Holder { buf: buffer(64), tag: 7 }\n  print(consume(h))\n  print(consume(h))\n  return Ok(())\n}\n"
    ));
}

#[test]
fn handle_field_partial_move_out_rejected() {
    // Moving the handle field out of the struct (a partial move) is deferred — a clean diagnostic,
    // never a silent double-free of both the field and the struct's drop.
    assert!(check_errs(
        "handlefield-partial",
        "Holder { buf: buffer, tag: i64 }\nfn main() -> Result<(), Error> {\n  h := Holder { buf: buffer(64), tag: 7 }\n  b := h.buf\n  return Ok(())\n}\n"
    ));
}

#[test]
fn http_request_ctx_field_type_checks() {
    // The pkg.web target shape: the request `Ctx` **owns** its `http_request_ctx` handle (now a
    // nameable surface type) alongside a `str` view field. It type-checks (running needs a live
    // server — exercised by the W2 integration tests later).
    assert!(!check_errs(
        "ctx-field",
        "Ctx { req: http_request_ctx, path: str }\nfn take(c: Ctx) -> str = c.path\nfn main() -> Result<(), Error> { return Ok(()) }\n"
    ));
}

#[test]
fn option_of_handle_field_stays_rejected() {
    // `is_field_ok` admits a bare handle, and `Option<T>` recurses into its payload — so
    // `Option<http_request_ctx>` would reach `is_field_ok = true`. But `drop_struct_fields` has no
    // Option-with-Move-payload arm (it would leak the handle when `Some`), so the owned-Option-
    // payload rejection (pass 0b-2) MUST still fire, keeping that leak path unreachable. Defense in
    // depth pinned here because F1② made handle scalars field-eligible.
    assert!(check_errs(
        "opt-handle",
        "H { x: Option<http_request_ctx> }\nfn main() -> Result<(), Error> { return Ok(()) }\n"
    ));
}

#[test]
fn all_field_kinds_coexist_and_drop_cleanly() {
    if !backend_available() {
        return;
    }
    // All four F1 field kinds in one Move struct: a handle (`buffer`), an owned `string`, a
    // `slice<str>` view, a fn value, and a scalar. Call the fn field, read a view element, then let
    // the struct drop — the handle + string are freed once each, the slice/fn/scalar skipped.
    let src = concat!(
        "fn hd(n: i64) -> i64 = n + 1\n",
        "Mix { req: buffer, title: string, tags: slice<str>, handler: fn(i64) -> i64, n: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  ts := [\"a\", \"b\"]\n",
        "  m := Mix { req: buffer(32), title: \"t\".clone(), tags: ts, handler: hd, n: 5 }\n",
        "  print(m.handler(m.n))\n",
        "  print(m.tags[0])\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("mix-fields", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\na\n");
}
