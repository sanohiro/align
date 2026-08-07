//! Minimal generics — slice 4c-1 (the unconstrained walking skeleton). A generic function
//! `fn f<T>(...)` is monomorphized per distinct concrete instantiation (`id$i32`, `id$i64`, …):
//! type arguments are inferred (no turbofish), `Ty::Param` is substituted before the flow analyses
//! and MIR run, so move/drop and codegen only ever see concrete types. A type parameter is opaque
//! except for the closed builtin bounds; uninstantiated templates are structurally checked and
//! concrete monomorphs are checked again before publication.


mod common;
use common::*;

#[test]
fn identity_and_pick() {
    if !backend_available() {
        return;
    }
    let src = "fn id<T>(x: T) -> T = x\nfn pick<T>(a: T, b: T) -> T = a\nfn main() -> i32 {\n  x := id(5)\n  y := pick(10, 20)\n  return x + y\n}\n";
    let out = build_and_run("gen-id-pick", src);
    assert_eq!(out.status.code(), Some(15));
}

#[test]
fn same_generic_two_instantiations() {
    if !backend_available() {
        return;
    }
    // `id` at i32 and i64 — two distinct monomorphs (`id$i32`, `id$i64`).
    let src = "fn id<T>(x: T) -> T = x\nfn use_i64(n: i64) -> i64 = n\nfn main() -> i32 {\n  a := id(3)\n  b := use_i64(id(40))\n  return a + 9\n}\n";
    let out = build_and_run("gen-two-inst", src);
    assert_eq!(out.status.code(), Some(12)); // 3 + 9; exercises id$i32 and id$i64
}

#[test]
fn multi_type_params() {
    if !backend_available() {
        return;
    }
    let src = "fn fst<A, B>(a: A, b: B) -> A = a\nfn main() -> i32 = fst(7, true) + fst(5, 100)\n";
    let out = build_and_run("gen-multi", src);
    assert_eq!(out.status.code(), Some(12)); // 7 + 5
}

#[test]
fn transitive_instantiation() {
    if !backend_available() {
        return;
    }
    // `wrap<T>` calls `id<T>`; instantiating `wrap` at i32 must instantiate `id` at i32.
    let src = "fn id<T>(x: T) -> T = x\nfn wrap<T>(x: T) -> T = id(x)\nfn main() -> i32 = wrap(42)\n";
    let out = build_and_run("gen-transitive", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn struct_type_argument() {
    if !backend_available() {
        return;
    }
    let src = "Point { x: i32, y: i32 }\nfn id<T>(v: T) -> T = v\nfn main() -> i32 {\n  p := id(Point { x: 4, y: 9 })\n  return p.x + p.y\n}\n";
    let out = build_and_run("gen-struct", src);
    assert_eq!(out.status.code(), Some(13));
}

#[test]
fn return_type_inferred_from_context() {
    if !backend_available() {
        return;
    }
    // The literal argument's type flows from the `-> i32` return through the generic result.
    let src = "fn id<T>(x: T) -> T = x\nfn main() -> i32 = id(99)\n";
    let out = build_and_run("gen-ret-infer", src);
    assert_eq!(out.status.code(), Some(99));
}

#[test]
fn owned_value_through_generic_drops() {
    if !backend_available() {
        return;
    }
    // An owned (Move) array flows through `id`; the monomorph + caller drop it correctly (no leak /
    // double-free) — the flow analyses run on the concrete instance.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn id<T>(x: T) -> T = x\nfn main() -> i32 {\n  xs := [1, 2, 3].map(dbl).to_array()\n  ys := id(xs)\n  return 0\n}\n";
    let out = build_and_run("gen-owned-drop", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn use_after_move_through_generic_rejected() {
    // Passing an owned value to a generic call moves it; a second use is use-after-move.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn id<T>(x: T) -> T = x\nfn main() -> i32 {\n  xs := [1, 2, 3].map(dbl).to_array()\n  ys := id(xs)\n  zs := id(xs)\n  return 0\n}\n";
    assert!(check_errs("gen-uam", src));
}

#[test]
fn operation_on_type_param_rejected() {
    // A type parameter is opaque in the skeleton: arithmetic on it has no constraint and is rejected.
    let src = "fn bad<T>(x: T) -> T = x + x\nfn main() -> i32 = bad(3)\n";
    assert!(check_errs("gen-op", src));
}

#[test]
fn uninferable_type_param_rejected() {
    let src = "fn make<T>() -> T = make()\nfn main() -> i32 {\n  make()\n  return 0\n}\n";
    assert!(check_errs("gen-uninfer", src));
}

#[test]
fn generic_owned_array_parameter_and_return() {
    if !backend_available() {
        return;
    }
    let src = "fn value(n: i32) -> i32 = n\nfn keep<T>(xs: array<T>) -> array<T> = xs\nfn main() -> i32 {\n  xs := [value(40), value(2)].to_array()\n  ys := keep(xs)\n  return ys[0] + ys[1]\n}\n";
    let out = build_and_run("gen-arrayparam", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn generic_fixed_array_rejects_copying_a_move_struct_value() {
    let src = "Boxed { value: string }\nHolder<T> { value: T }\nfn rows<T>(holder: Holder<T>) -> array<T> = [holder.value].to_array()\nfn main() -> i32 { holder := Holder { value: Boxed { value: \"owned\".clone() } }; rows(holder); return 0 }\n";
    assert!(check_errs("gen-fixed-move-struct", src));
}

#[test]
fn generic_slice_parameter_and_return() {
    if !backend_available() {
        return;
    }
    let src = "fn value(n: i32) -> i32 = n\nfn keep<T>(xs: slice<T>) -> slice<T> = xs\nfn main() -> i32 {\n  xs := [value(40), value(2)]\n  ys := keep(xs[..])\n  return ys[0] + ys[1]\n}\n";
    let out = build_and_run("gen-sliceparam", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn region_plain_bound_builds_a_generic_region_array() {
    if !backend_available() {
        return;
    }
    let src = "Row { id: i32, name: str }\nfn one<T: RegionPlain>(out: region, value: T) -> array<T> {\n  mut b: array_builder<T> := array_builder(out)\n  b.push(value)\n  return b.build()\n}\nfn all<T: RegionPlain>(out: region, value: T) -> array<T> = one(out, value)\nfn main() -> i32 {\n  arena out {\n    rows := all(out, Row { id: 42, name: \"ok\" })\n    return rows[0].id\n  }\n}\n";
    let out = build_and_run("gen-region-plain", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn region_plain_bound_rejects_owned_heap_fields() {
    let src = "Owned { value: string }\nfn one<T: RegionPlain>(out: region, value: T) -> array<T> {\n  mut b: array_builder<T> := array_builder(out)\n  b.push(value)\n  return b.build()\n}\nfn main() -> i32 {\n  arena out {\n    rows := one(out, Owned { value: \"x\".clone() })\n  }\n  return 0\n}\n";
    assert!(check_errs("gen-region-plain-owned", src));
}

#[test]
fn region_plain_builder_remaps_a_concrete_generic_struct_element() {
    if !backend_available() {
        return;
    }
    let src = "Wrap<T> { value: T }\nfn keep<T>(value: Wrap<T>) -> Wrap<T> = value\nfn one<T: RegionPlain>(out: region, value: T) -> array<T> {\n  mut b: array_builder<T> := array_builder(out)\n  b.push(value)\n  return b.build()\n}\nfn value(n: i32) -> i32 = n\nfn main() -> i32 { arena out { wrapped := keep(Wrap { value: value(42) }); values := one(out, wrapped); return values[0].value } }\n";
    let out = build_and_run("gen-region-plain-remap", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn region_plain_does_not_grant_equality() {
    let src = "fn same<T: RegionPlain>(a: T, b: T) -> bool = a == b\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-region-plain-eq", src));
}

#[test]
fn duplicate_type_param_rejected() {
    let src = "fn f<T, T>(a: T, b: T) -> T = a\nfn main() -> i32 = f(1, 2)\n";
    assert!(check_errs("gen-duptp", src));
}

#[test]
fn type_param_shadowing_type_rejected() {
    let src = "Point { x: i32, y: i32 }\nfn f<Point>(x: Point) -> Point = x\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-shadow", src));
}

#[test]
fn generic_call_arity_mismatch_rejected() {
    let src = "fn pick<T>(a: T, b: T) -> T = a\nfn main() -> i32 = pick(1)\n";
    assert!(check_errs("gen-arity", src));
}

#[test]
fn generic_main_rejected() {
    // `main` is the entry point and cannot be a generic template.
    let src = "fn main<T>() -> i32 = 0\n";
    assert!(check_errs("gen-main", src));
}

// ---- 4c-2: builtin bounds (Num / Ord / Eq) ----

#[test]
fn num_bound_enables_arithmetic() {
    if !backend_available() {
        return;
    }
    let src = "fn add<T: Num>(a: T, b: T) -> T = a + b\nfn main() -> i32 = add(10, 20) + add(5, 7)\n";
    let out = build_and_run("gen-num", src);
    assert_eq!(out.status.code(), Some(42)); // 30 + 12
}

#[test]
fn ord_bound_enables_comparison() {
    if !backend_available() {
        return;
    }
    let src = "fn max<T: Ord>(a: T, b: T) -> T = if a > b { a } else { b }\nfn main() -> i32 = max(7, 12) + max(30, 2)\n";
    let out = build_and_run("gen-ord", src);
    assert_eq!(out.status.code(), Some(42)); // 12 + 30
}

#[test]
fn eq_bound_enables_equality_on_char() {
    if !backend_available() {
        return;
    }
    let src = "fn same<T: Eq>(a: T, b: T) -> bool = a == b\nfn main() -> i32 = if same('x', 'x') { 42 } else { 0 }\n";
    let out = build_and_run("gen-eq", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn arithmetic_without_num_bound_rejected() {
    let src = "fn add<T>(a: T, b: T) -> T = a + b\nfn main() -> i32 = add(1, 2)\n";
    assert!(check_errs("gen-noarith", src));
}

#[test]
fn ordering_with_only_eq_rejected() {
    let src = "fn gt<T: Eq>(a: T, b: T) -> bool = a > b\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-eq-noord", src));
}

#[test]
fn ord_instantiated_with_non_ord_rejected() {
    // `bool` is not Ord — the instantiation must fail.
    let src = "fn max<T: Ord>(a: T, b: T) -> T = if a > b { a } else { b }\nfn main() -> i32 = if max(true, false) { 1 } else { 0 }\n";
    assert!(check_errs("gen-ord-bool", src));
}

#[test]
fn num_instantiated_with_char_rejected() {
    // `char` is Ord/Eq but not Num.
    let src = "fn add<T: Num>(a: T, b: T) -> T = a + b\nfn main() -> i32 {\n  add('a', 'b')\n  return 0\n}\n";
    assert!(check_errs("gen-num-char", src));
}

#[test]
fn unknown_bound_rejected() {
    let src = "fn f<T: Display>(x: T) -> T = x\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-badbound", src));
}

#[test]
fn equality_without_eq_bound_rejected() {
    // Regression: in 4c-1 `==` on an unconstrained `T` slipped through ungated; 4c-2 closes it.
    let src = "fn eq<T>(a: T, b: T) -> bool = a == b\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-eq-hole", src));
}

// ---- 4c-3: type parameters in Option / Result positions ----

#[test]
fn option_return_position() {
    if !backend_available() {
        return;
    }
    // `T` nested in the return type `Option<T>`; the binding annotation seeds `T = i32`.
    let src = "fn wrap<T>(x: T) -> Option<T> = Some(x)\nfn main() -> i32 {\n  o: Option<i32> := wrap(41)\n  return o else 0\n}\n";
    let out = build_and_run("gen-opt-ret", src);
    assert_eq!(out.status.code(), Some(41));
}

#[test]
fn option_param_position() {
    if !backend_available() {
        return;
    }
    // `T` nested in a parameter type `Option<T>`, inferred from the argument.
    let src = "fn unwrap_or<T>(o: Option<T>, d: T) -> T = o else d\nfn main() -> i32 {\n  a: Option<i32> := Some(7)\n  b: Option<i32> := None\n  return unwrap_or(a, 0) + unwrap_or(b, 5)\n}\n";
    let out = build_and_run("gen-opt-param", src);
    assert_eq!(out.status.code(), Some(12)); // 7 + 5
}

#[test]
fn result_return_and_question_mark() {
    if !backend_available() {
        return;
    }
    // `Result<T, Error>` return position, propagated through `?`.
    let src = concat!(
        "fn ok<T>(x: T) -> Result<T, Error> = Ok(x)\n",
        "fn run() -> Result<i32, Error> {\n",
        "  v: i32 := ok(40)?\n",
        "  return Ok(v + 2)\n",
        "}\n",
        "fn main() -> i32 = match run() { Ok(v) => v, Err(e) => 99 }\n",
    );
    let out = build_and_run("gen-result-ret", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn generic_box_param_rejected() {
    // `box<T>` over a type parameter is not supported yet (only Option/Result positions are).
    let src = "fn f<T>(b: box<T>) -> i32 = 0\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-box", src));
}

#[test]
fn deeper_abstract_nominal_applications_are_rejected() {
    let option = "Wrap<T> { value: T }\nfn keep<T>(value: Option<Wrap<T>>) -> Option<Wrap<T>> = value\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-deep-nominal-option", option));

    let nominal = "Inner<T> { value: T }\nOuter<T> { value: T }\nfn keep<T>(value: Outer<Inner<T>>) -> Outer<Inner<T>> = value\nfn main() -> i32 = 0\n";
    assert!(check_errs("gen-deep-nominal-outer", nominal));
}

// ---- 4c-5: generic structs ----

#[test]
fn generic_struct_construct_and_field_access() {
    if !backend_available() {
        return;
    }
    // `Pair<T>` declared, constructed (T inferred from the field values), fields read.
    let src = "Pair<T> { a: T, b: T }\nfn ident(x: i32) -> i32 = x\nfn main() -> i32 {\n  p := Pair { a: ident(10), b: ident(32) }\n  return p.a + p.b\n}\n";
    let out = build_and_run("gen-struct-pair", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn generic_struct_as_function_param() {
    if !backend_available() {
        return;
    }
    // `Pair<i32>` as a parameter type monomorphizes; a literal passed to it matches the instance.
    let src = "Pair<T> { a: T, b: T }\nfn sum(p: Pair<i32>) -> i32 = p.a + p.b\nfn ident(x: i32) -> i32 = x\nfn main() -> i32 = sum(Pair { a: ident(40), b: ident(2) })\n";
    let out = build_and_run("gen-struct-param", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn generic_struct_two_instantiations() {
    if !backend_available() {
        return;
    }
    // `Pair<i32>` and `Pair<bool>` are distinct monomorph instances.
    let src = "Pair<T> { a: T, b: T }\nfn ident(x: i32) -> i32 = x\nfn main() -> i32 {\n  pi := Pair { a: ident(40), b: ident(2) }\n  pb := Pair { a: true, b: false }\n  if pb.a { return pi.a + pi.b }\n  return 0\n}\n";
    let out = build_and_run("gen-struct-two", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn generic_struct_multi_type_params() {
    if !backend_available() {
        return;
    }
    let src = "Two<A, B> { x: A, y: B }\nfn ident(n: i32) -> i32 = n\nfn main() -> i32 {\n  t := Two { x: ident(42), y: true }\n  if t.y { return t.x }\n  return 0\n}\n";
    let out = build_and_run("gen-struct-multi", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn generic_struct_uninferable_rejected() {
    // No field pins `T`, so it cannot be inferred from the literal.
    let src = "Empty<T> { }\nfn main() -> i32 {\n  e := Empty { }\n  return 0\n}\n";
    assert!(check_errs("gen-struct-uninfer", src));
}

#[test]
fn generic_struct_with_type_param_argument() {
    if !backend_available() {
        return;
    }
    let src = "Pair<T> { a: T, b: T }\nfn mk<T>(x: T) -> Pair<T> = Pair { a: x, b: x }\nfn value(n: i32) -> i32 = n\nfn main() -> i32 {\n  pair := mk(value(21))\n  return pair.a + pair.b\n}\n";
    let out = build_and_run("gen-struct-in-fn", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn phantom_package_nominal_infers_from_expected_return() {
    let src = "Query<P, R> { }\nParams { id: i64 }\nRow { value: i64 }\nfn make<P, R>() -> Query<P, R> = make()\nfn keep<P, R>(query: Query<P, R>) -> Query<P, R> = query\nfn main() -> i32 {\n  query: Query<Params, Row> := make()\n  same: Query<Params, Row> := keep(query)\n  return 0\n}\n";
    assert!(!check_errs("gen-package-phantom", src));
}

// ---- 4c-6: generic sum types ----

#[test]
fn generic_enum_construct_and_match() {
    if !backend_available() {
        return;
    }
    // `Opt<T>` declared, a payload variant constructed (T inferred from the arg), matched.
    let src = "Opt<T> { Some(T), None }\nfn ident(x: i32) -> i32 = x\nfn main() -> i32 {\n  o := Opt.Some(ident(42))\n  return match o {\n    Some(x) => x\n    None => 0\n  }\n}\n";
    let out = build_and_run("gen-enum-opt", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn generic_enum_struct_payload() {
    if !backend_available() {
        return;
    }
    // A generic sum type whose payload is a (plain-data) struct.
    let src = "Point { x: i32, y: i32 }\nBox<T> { Has(T), Empty }\nfn main() -> i32 {\n  b := Box.Has(Point { x: 40, y: 2 })\n  return match b {\n    Has(p) => p.x + p.y\n    Empty => 0\n  }\n}\n";
    let out = build_and_run("gen-enum-box", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn generic_sum_application_in_generic_signature() {
    if !backend_available() {
        return;
    }
    let src = "Wrap<T> { Has(T), Empty }\nfn keep<T>(value: Wrap<T>) -> Wrap<T> = value\nfn value(n: i32) -> i32 = n\nfn main() -> i32 {\n  wrapped := keep(Wrap.Has(value(42)))\n  return match wrapped { Has(n) => n, Empty => 0 }\n}\n";
    let out = build_and_run("gen-sum-signature", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn generic_resource_application_in_generic_signature() {
    let internal = "module test.resource.internal\npub fn drop_handle(handle: raw) { unsafe { raw.free(handle) } }\n";
    let package = "module test.resource\nimport test.resource.internal\npub resource Handle<T> = test.resource.internal.drop_handle\npub fn open() -> Handle<i32> { unsafe { return resource.from_raw(raw.alloc(8)) } }\npub fn keep<T>(owner: Handle<T>) -> Handle<T> = owner\npub fn keep_ref<T>(reference: resource_ref<Handle<T>>) -> resource_ref<Handle<T>> = reference\n";
    let main = "module main\nimport test.resource\nfn main() -> i32 { owner := test.resource.keep(test.resource.open()); reference := test.resource.keep_ref(resource.borrow(owner)); return 42 }\n";
    let files = [
        ("test/resource/internal.align", internal),
        ("test/resource.align", package),
        ("main.align", main),
    ];
    let checked = assert_same_verdict("gen-resource-signature-check", &files, "main.align");
    assert!(!checked.diags.has_errors());
    if !backend_available() {
        return;
    }
    let out = build_and_run_multi("gen-resource-signature", &files, "main.align");
    assert_eq!(out.status.code(), Some(42));
    assert_eq!(
        build_per_unit_multi("gen-resource-signature-units", &files, "main.align")
            .link_and_run()
            .status
            .code(),
        Some(42)
    );
}

#[test]
fn nested_package_generics_match_whole_and_per_unit_compilation() {
    let package = "module pkg.query\npub Query<P, R> { params: P, row: R }\npub fn make<P, R>(params: P, row: R) -> Query<P, R> = Query { params: params, row: row }\npub fn rows<P, R>(query: Query<P, R>) -> array<R> = [query.row].to_array()\npub fn all<R: RegionPlain>(out: region, value: R) -> array<R> {\n  mut rows: array_builder<R> := array_builder(out)\n  rows.push(value)\n  return rows.build()\n}\n";
    let main = "module main\nimport pkg.query\nParams { id: i32 }\nRow { value: i32, name: str }\nfn main() -> i32 {\n  query := pkg.query.make(Params { id: 1 }, Row { value: 40, name: \"first\" })\n  heap_rows := pkg.query.rows(query)\n  arena out {\n    region_rows := pkg.query.all(out, Row { value: 2, name: \"second\" })\n    return heap_rows[0].value + region_rows[0].value\n  }\n}\n";
    let files = [("pkg/query.align", package), ("main.align", main)];
    let checked = assert_same_verdict("gen-package-units-check", &files, "main.align");
    assert!(
        !checked.diags.has_errors(),
        "nested generic package signatures must survive interface reconstruction"
    );
    if !backend_available() {
        return;
    }
    let whole = build_and_run_multi("gen-package-whole", &files, "main.align");
    assert_eq!(whole.status.code(), Some(42));
    let per_unit = build_per_unit_multi("gen-package-units", &files, "main.align");
    assert!(
        !per_unit.unit("main").mir.fns.is_empty(),
        "abstract generic state must not empty the consumer MIR"
    );
    assert_eq!(per_unit.link_and_run().status.code(), Some(42));
}

#[test]
fn generic_enum_no_payload_variant_uninferable() {
    // `Opt.None` alone gives nothing to infer `T` from (no payload), so it is uninferable here.
    let src = "Opt<T> { Some(T), None }\nfn main() -> i32 {\n  o := Opt.None\n  return 0\n}\n";
    assert!(check_errs("gen-enum-none", src));
}

#[test]
fn generic_enum_payload_follows_the_non_generic_rule() {
    // A monomorph payload must satisfy the same rule a non-generic enum enforces (`enum_payload_ok`).
    // J1 lifted the old `str`-field-struct restriction — an enum is now region-tracked — so a
    // `str`-bearing *plain-data* struct payload is ACCEPTED (mirrors `enum_match::str_field_struct_payload_accepted`).
    let ok = "Named { s: str }\nOpt<T> { Some(T), None }\nfn main() -> i32 {\n  o := Opt.Some(Named { s: \"hi\" })\n  return 0\n}\n";
    assert!(!check_errs("gen-enum-str-struct-ok", ok));
    // L1b recursively drops a concrete Move struct payload after substitution, so the generic path
    // follows the same acceptance rule as a non-generic sum.
    let owned = "Owned { s: string }\nOpt<T> { Some(T), None }\nfn main() -> i32 {\n  o := Opt.Some(Owned { s: \"hi\".clone() })\n  return 0\n}\n";
    assert!(!check_errs("gen-enum-move-struct", owned));
}

#[test]
fn concrete_nested_mismatch_rejected() {
    // A concrete part of a nested generic parameter type must still match: `Result<T, i32>` cannot
    // accept a `Result<_, bool>` (the `i32` vs `bool` mismatch must be a type error).
    let src = "fn f<T>(r: Result<T, i32>) -> i32 = 0\nfn main() -> i32 {\n  x: Result<f64, bool> := Ok(1.0)\n  return f(x)\n}\n";
    assert!(check_errs("gen-nested-mismatch", src));
}

#[test]
fn array_literal_in_generic_call() {
    let src = "
fn foo<T>(x: T) {}
fn main() -> i32 {
    foo([1, 2, 3])
    return 0
}
";
    assert!(common::check_errs("array_literal_in_generic_call", src));
}
