//! Owned (`string`-bearing) struct fields + recursive struct **Drop** (Slice 3 of
//! `docs/impl/08-nested-structs.md`). A struct that (transitively) owns a heap buffer — a `string`
//! field, or a nested struct that does — becomes a **Move** type: it gets a recursive Drop (free
//! each owned field in order, recursing into nested Move structs) and whole-struct move semantics
//! (return / pass / assign by value nulls the source so its exit Drop is a no-op — no double-free).
//!
//! Drop correctness can't be asserted by a return value alone, so these run the program and rely on
//! a double-free / use-after-free aborting the process (a wrong free corrupts the allocator). A
//! *leak* is undetectable here, but the value assertions plus the no-crash guarantee lock the shape.

mod common;
use common::*;

#[test]
fn string_field_construct_and_drop() {
    if !backend_available() {
        return;
    }
    // Construct a struct that owns a `string` (the cloned literal), read a scalar field, then let it
    // drop at scope exit — the runtime frees the `name` buffer once. Returns age = 7.
    let src = concat!(
        "User { name: string, age: i64 }\n",
        "fn main() -> i32 {\n",
        "  u := User{name: \"hello\".clone(), age: 7}\n",
        "  return u.age as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("owned-struct", src).status.code(), Some(7));
}

#[test]
fn nested_owned_struct_recursive_drop() {
    if !backend_available() {
        return;
    }
    // `User` owns a `string` AND a nested `Address` that itself owns a `string`. Drop of `u` frees
    // `name`, then recurses into `addr` to free `street`. Returns age = 9.
    let src = concat!(
        "Address { street: string }\n",
        "User { name: string, addr: Address, age: i64 }\n",
        "fn main() -> i32 {\n",
        "  u := User{name: \"alice\".clone(), addr: Address{street: \"main st\".clone()}, age: 9}\n",
        "  return u.age as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("nested-owned-struct", src).status.code(), Some(9));
}

#[test]
fn owned_struct_returned_by_value() {
    if !backend_available() {
        return;
    }
    // `mk` builds an owned struct and returns it by value: the move nulls `mk`'s slot so its exit
    // Drop is a no-op, and `main`'s `u` frees the buffer once. No double-free. Returns age = 42.
    let src = concat!(
        "User { name: string, age: i64 }\n",
        "fn mk() -> User = User{name: \"bob\".clone(), age: 42}\n",
        "fn main() -> i32 {\n",
        "  u := mk()\n",
        "  return u.age as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("owned-struct-ret", src).status.code(), Some(42));
}

#[test]
fn owned_struct_passed_by_value() {
    if !backend_available() {
        return;
    }
    // Passing an owned struct by value transfers ownership into `age_of`, which drops it at its own
    // exit — the caller's construction site does not also drop. Returns age = 5.
    let src = concat!(
        "User { name: string, age: i64 }\n",
        "fn age_of(u: User) -> i64 = u.age\n",
        "fn main() -> i32 {\n",
        "  return age_of(User{name: \"carol\".clone(), age: 5}) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("owned-struct-arg", src).status.code(), Some(5));
}

#[test]
fn owned_struct_in_a_branch() {
    if !backend_available() {
        return;
    }
    // Two owned structs on different control-flow paths; only the taken one is constructed, and its
    // single Drop runs at function exit (the untaken path's slot stays null → free(null) no-op).
    let src = concat!(
        "User { name: string, age: i64 }\n",
        "fn pick(b: bool) -> i64 {\n",
        "  if b {\n",
        "    u := User{name: \"x\".clone(), age: 1}\n",
        "    return u.age\n",
        "  }\n",
        "  v := User{name: \"yy\".clone(), age: 2}\n",
        "  return v.age\n",
        "}\n",
        "fn main() -> i32 = pick(false) as i32\n",
    );
    assert_eq!(build_and_run("owned-struct-branch", src).status.code(), Some(2));
}

// ---- negative (sema) tests ----

#[test]
fn recursive_struct_still_rejected() {
    // Self-recursion without a `box` indirection is infinite layout — still rejected, now that owned
    // fields are allowed (the acyclicity check is independent of ownership).
    assert!(check_errs(
        "rec-struct",
        "Node { next: Node, v: i64 }\nfn main() -> i32 { return 0 }\n"
    ));
}

#[test]
fn move_struct_in_unsupported_containers_rejected() {
    // A Move struct can't yet live where the container's drop is scalar-shaped and wouldn't recurse
    // into the struct's owned fields (leak / double-free). Each must be a clean sema error.
    // (A fixed *array* of Move structs is now supported — Slice 4a — see `owned_structs_arrays.rs`.)
    let u = "User { name: string, age: i64 }\n";
    // `Option` / `Result` / user-enum payloads (their drop frees a flat `{ptr,len}`, not a struct).
    assert!(check_errs("ms-option", &format!("{u}fn f() -> Option<User> = Some(User{{name: \"a\".clone(), age: 1}})\nfn main() -> i32 = 0\n")));
    assert!(check_errs("ms-result", &format!("{u}fn f() -> Result<User, Error> = Ok(User{{name: \"a\".clone(), age: 1}})\nfn main() -> i32 = 0\n")));
    assert!(check_errs("ms-enum", &format!("{u}Wrap {{ W(User) }}\nfn main() -> i32 = 0\n")));
}

#[test]
fn cyclic_struct_used_by_value_errors_gracefully() {
    // A recursive struct is reported by the acyclicity pass, but the compiler keeps running later
    // passes (move-check / drop-set) on the erroneous program — which call the Move-struct walk. That
    // walk must be cycle-safe (not overflow the stack) when the struct is used in a value position.
    assert!(check_errs(
        "cyclic-byvalue",
        "Node { next: Node, v: i64 }\nfn f(n: Node) -> i64 = n.v\nfn main() -> i32 = 0\n"
    ));
}

// --- Partial owned-field *move* out of a struct (`n := u.name`) ---
// A depth-1 owned `string` field can be moved out: the buffer transfers to the new binding, the
// struct's slot field is nulled, and the struct's recursive Drop frees null there — so the buffer is
// freed exactly once (by the new owner). The struct can no longer move as a whole / the field be
// reused, but its other fields stay readable. Deeper paths and whole nested Move-struct fields are
// still deferred.

#[test]
fn owned_field_moved_out() {
    if !backend_available() {
        return;
    }
    // Move `u.name` into `n`, then still read the Copy sibling `u.age`. The buffer is freed once (via
    // `n`); `u`'s Drop frees null for `name`. 5 + 9 = 14, no double-free.
    let src = concat!(
        "User { name: string, age: i64 }\n",
        "fn main() -> i32 {\n",
        "  u := User{name: \"hello\".clone(), age: 9}\n",
        "  n := u.name\n",
        "  return (n.len() + u.age) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("ownfield-move", src).status.code(), Some(14));
}

#[test]
fn owned_field_move_then_reuse_rejected() {
    // After moving the field out, the field itself is gone — reusing it is a use-after-move error
    // (the other fields remain usable, tested above at runtime).
    assert!(check_errs(
        "ownfield-move-reuse",
        concat!(
            "User { name: string, age: i64 }\n",
            "fn main() -> i32 {\n",
            "  u := User{name: \"z\".clone(), age: 1}\n",
            "  n := u.name\n",
            "  return (n.len() + u.name.len()) as i32\n",
            "}\n",
        )
    ));
}

#[test]
fn owned_field_move_then_whole_struct_move_rejected() {
    // A partially-moved struct can't be moved as a whole (one field's buffer already left).
    assert!(check_errs(
        "ownfield-move-whole",
        concat!(
            "User { name: string, age: i64 }\n",
            "fn take(x: User) -> i32 = x.age as i32\n",
            "fn main() -> i32 {\n",
            "  u := User{name: \"z\".clone(), age: 1}\n",
            "  n := u.name\n",
            "  return take(u)\n",
            "}\n",
        )
    ));
}

#[test]
fn nested_path_owned_field_move_still_rejected() {
    // Moving an owned field out through a *nested* path (`n := u.addr.name`) is still deferred (only
    // a depth-1 field move is supported).
    assert!(check_errs(
        "ownfield-move-deep",
        concat!(
            "Addr { name: string }\n",
            "User { addr: Addr }\n",
            "fn main() -> i32 {\n",
            "  u := User{addr: Addr{name: \"z\".clone()}}\n",
            "  n := u.addr.name\n",
            "  return n.len() as i32\n",
            "}\n",
        )
    ));
}

#[test]
fn nested_move_struct_field_move_still_rejected() {
    // Moving a whole nested Move-struct field out (`a := u.addr`) is still deferred (it needs the
    // sub-struct nulled, not a single `{ptr,len}`).
    assert!(check_errs(
        "ownfield-move-substruct",
        concat!(
            "Addr { name: string }\n",
            "User { addr: Addr }\n",
            "fn main() -> i32 {\n",
            "  u := User{addr: Addr{name: \"z\".clone()}}\n",
            "  a := u.addr\n",
            "  return a.name.len() as i32\n",
            "}\n",
        )
    ));
}

// --- Owned-field *borrow* out of a struct (read, non-consuming) ---
// Slice 3 made owned struct fields constructible/writable but their contents were unreadable. A
// `string` field can now be borrowed as a zero-copy `str` view (`u.name.len()`, a `str` argument, a
// `str` binding): non-consuming (the struct keeps owning the buffer, dropped once) and `Frame`-
// regioned (the view can't escape the struct's frame). *Moving* the field out stays deferred (above).

#[test]
fn owned_field_borrowed_via_len() {
    if !backend_available() {
        return;
    }
    // `u.name.len()` borrows the `string` field as a `str` and reads its length; `u` is still dropped
    // once at exit (the borrowed buffer is freed by the struct's Drop, not separately). len = 5.
    let src = concat!(
        "User { name: string, age: i64 }\n",
        "fn main() -> i32 {\n",
        "  u := User{name: \"hello\".clone(), age: 9}\n",
        "  return u.name.len() as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("ownfield-len", src).status.code(), Some(5));
}

#[test]
fn owned_field_borrow_does_not_consume() {
    if !backend_available() {
        return;
    }
    // Borrowing the field leaves the struct fully usable: read `u.name.len()`, then still read a
    // Copy field `u.age`. 5 + 9 = 14, and `u` drops once (no double-free).
    let src = concat!(
        "User { name: string, age: i64 }\n",
        "fn main() -> i32 {\n",
        "  u := User{name: \"hello\".clone(), age: 9}\n",
        "  a := u.name.len()\n",
        "  return (a + u.age) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("ownfield-noconsume", src).status.code(), Some(14));
}

#[test]
fn nested_owned_field_borrowed() {
    if !backend_available() {
        return;
    }
    // Borrow a `string` reached through a nested Move-struct field (`u.addr.name`). The whole `u`
    // (which transitively owns both buffers) drops once. len("world!!") = 7.
    let src = concat!(
        "Addr { name: string }\n",
        "User { addr: Addr }\n",
        "fn main() -> i32 {\n",
        "  u := User{addr: Addr{name: \"world!!\".clone()}}\n",
        "  return u.addr.name.len() as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("ownfield-nested", src).status.code(), Some(7));
}

#[test]
fn owned_field_str_binding() {
    if !backend_available() {
        return;
    }
    // `s: str := u.name` binds a borrow of the field to a `str` local (used after); the struct keeps
    // ownership and drops once. len = 4.
    let src = concat!(
        "User { name: string }\n",
        "fn main() -> i32 {\n",
        "  u := User{name: \"abcd\".clone()}\n",
        "  s: str := u.name\n",
        "  return s.len() as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("ownfield-strbind", src).status.code(), Some(4));
}

#[test]
fn owned_field_str_borrow_cannot_escape() {
    // A `str` view borrowing an owned field is `Frame`-regioned: returning it (here via a `str`
    // local) would dangle once the struct is dropped — rejected by the escape check.
    assert!(check_errs(
        "ownfield-escape",
        concat!(
            "User { name: string }\n",
            "fn leak(u: User) -> str {\n",
            "  s: str := u.name\n",
            "  return s\n",
            "}\n",
            "fn main() -> i32 = 0\n",
        )
    ));
}

#[test]
fn owned_local_moved_into_struct_field_no_double_free() {
    if !backend_available() {
        return;
    }
    // Moving a **named owned local** into a struct-literal field consumes it: the field takes the
    // buffer and the source local must be nulled so its exit Drop doesn't double-free (an inline
    // temporary — `User{name: "x".clone()}` — never had a source local to null; a named local did,
    // and `store_value_at` was not nulling it). A clean run (exit 4) proves the buffer frees once.
    let src = concat!(
        "User { name: string, age: i64 }\n",
        "fn main() -> i32 {\n",
        "  s := \"abcd\".clone()\n",
        "  u := User { name: s, age: 7 }\n",
        "  return u.name.len() as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("ownfield-localmove", src).status.code(), Some(4));
}

#[test]
fn owned_array_local_moved_into_struct_field_no_double_free() {
    if !backend_available() {
        return;
    }
    // The same nulling for an owned `array<T>` field built via `array_builder` and moved in from a
    // `mut` local (the radix-router build shape): the source array local must be nulled so the
    // struct's Drop frees the buffer exactly once.
    let src = concat!(
        "fn filled(m: i64) -> array<i64> {\n",
        "  mut b: array_builder<i64> := array_builder()\n",
        "  mut i := 0\n",
        "  loop {\n",
        "    if i >= m { break }\n",
        "    b.push(-1)\n",
        "    i = i + 1\n",
        "  }\n",
        "  b.build()\n",
        "}\n",
        "T { col: array<i64> }\n",
        "fn main() -> i32 {\n",
        "  mut xs := filled(4)\n",
        "  xs[2] = 99\n",
        "  t := T { col: xs }\n",
        "  return t.col[2] as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("ownfield-arrmove", src).status.code(), Some(99));
}
