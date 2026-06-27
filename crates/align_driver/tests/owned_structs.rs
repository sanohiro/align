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
    let u = "User { name: string, age: i64 }\n";
    // An array of Move structs (per-element drop = a later slice).
    assert!(check_errs(
        "ms-array",
        &format!("{u}fn main() -> i32 {{\n  us := [User{{name: \"a\".clone(), age: 1}}]\n  return 0\n}}\n")
    ));
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

#[test]
fn partial_owned_field_move_out_rejected() {
    // Moving an owned field *out* of a struct (`n := u.name`, consuming just the `string`) is
    // deferred — bind/clone instead. Must be a clean sema error, not a miscompile.
    assert!(check_errs(
        "partial-move",
        concat!(
            "User { name: string, age: i64 }\n",
            "fn main() -> i32 {\n",
            "  u := User{name: \"z\".clone(), age: 1}\n",
            "  n := u.name\n",
            "  return n.len() as i32\n",
            "}\n",
        )
    ));
}
