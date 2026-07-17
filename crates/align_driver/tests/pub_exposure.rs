//! M15 S1b entry gate: sema rejects a `pub` interface that exposes a NON-`pub` type (`draft.md`
//! §17). A private type reachable from a module's public interface would be named in the unit's
//! interface summary WITHOUT its definition, so its layout change could not flip the unit's interface
//! hash → a stale-object miscompile once summaries are consumed. The rule: any type reachable from a
//! `pub` fn's params/return, a `pub` struct's fields, or a `pub` sum type's payloads must itself be
//! `pub`. Cross-module `mod.Type` access already requires `pub`, so this is a same-module check; the
//! recursion walks every `Type` constructor (named-generic args, tuple elements, fn params/return).

mod common;
use common::*;

// ---- negative: a pub interface may not expose a private type -------------------------------------

#[test]
fn pub_fn_returning_private_type_is_rejected() {
    let src = "Secret { v: i64 }\n\
               pub fn make() -> Secret = Secret{ v: 1 }\n\
               fn main() -> i32 = 0\n";
    let d = check_diagnostics("pub-ret-private", src);
    assert!(
        d.contains("pub fn 'make' exposes private type 'Secret' in its return type"),
        "expected the return-type exposure error, got:\n{d}"
    );
}

#[test]
fn pub_fn_private_parameter_type_is_rejected() {
    let src = "Secret { v: i64 }\n\
               pub fn take(s: Secret) -> i64 = s.v\n\
               fn main() -> i32 = 0\n";
    let d = check_diagnostics("pub-param-private", src);
    assert!(
        d.contains("pub fn 'take' exposes private type 'Secret' in its parameter type"),
        "expected the parameter-type exposure error, got:\n{d}"
    );
}

#[test]
fn pub_struct_private_field_type_is_rejected() {
    let src = "Secret { v: i64 }\n\
               pub Wrapper { s: Secret }\n\
               fn main() -> i32 = 0\n";
    let d = check_diagnostics("pub-field-private", src);
    assert!(
        d.contains("pub struct 'Wrapper' exposes private type 'Secret' in field 's'"),
        "expected the field exposure error, got:\n{d}"
    );
}

#[test]
fn pub_sum_type_private_payload_is_rejected() {
    // A plain-data struct payload is otherwise legal (Pass 0c), so the exposure check is what fires.
    let src = "Secret { v: i64 }\n\
               pub Shape { Circle(Secret) }\n\
               fn main() -> i32 = 0\n";
    let d = check_diagnostics("pub-payload-private", src);
    assert!(
        d.contains("pub sum type 'Shape' exposes private type 'Secret' in the payload of variant 'Circle'"),
        "expected the payload exposure error, got:\n{d}"
    );
}

#[test]
fn nested_reach_option_of_array_of_private_is_rejected() {
    // The recursion must reach a private type nested under builtin generics: Option<array<Secret>>.
    let src = "Secret { v: i64 }\n\
               pub fn f(xs: Option<array<Secret>>) -> i64 = 0\n\
               fn main() -> i32 = 0\n";
    let d = check_diagnostics("pub-nested-private", src);
    assert!(
        d.contains("exposes private type 'Secret'"),
        "expected a nested exposure error naming Secret, got:\n{d}"
    );
}

#[test]
fn nested_reach_tuple_and_fn_type_are_rejected() {
    // Tuple element and fn-type parameter positions must also be walked.
    let tup = "Secret { v: i64 }\n\
               pub fn g(p: (i64, Secret)) -> i64 = 0\n\
               fn main() -> i32 = 0\n";
    assert!(
        check_diagnostics("pub-tuple-private", tup).contains("exposes private type 'Secret'"),
        "a tuple element must be walked"
    );
    let fnty = "Secret { v: i64 }\n\
                pub fn h(cb: fn(Secret) -> i64) -> i64 = 0\n\
                fn main() -> i32 = 0\n";
    assert!(
        check_diagnostics("pub-fnty-private", fnty).contains("exposes private type 'Secret'"),
        "a fn-type parameter must be walked"
    );
}

#[test]
fn cross_module_pub_fn_exposing_own_private_type_is_rejected() {
    // The check is per-module: a `pub` fn in `lib` exposing `lib`'s OWN private type is rejected in
    // `lib` (same-module visibility), independent of the entry module.
    let lib = "module lib\n\
               Secret { v: i64 }\n\
               pub fn make() -> Secret = Secret{ v: 1 }\n";
    let main = "module main\nimport lib\nfn main() -> i32 = 0\n";
    let d = check_multi_diagnostics("pub-cross-private", &[("lib.align", lib), ("main.align", main)], "main.align");
    assert!(
        d.contains("pub fn 'make' exposes private type 'Secret' in its return type"),
        "expected the exposure error to fire in the lib module, got:\n{d}"
    );
}

// A `pub const` can never name a user type: a constant's type is restricted to a scalar / `str` /
// `slice<T>` of a scalar (Pass 0d), so the exposure hole is structurally impossible for consts. This
// documents WHY the exposure check does not (and need not) walk pub consts.
#[test]
fn pub_const_cannot_name_a_user_type() {
    let src = "Secret { v: i64 }\n\
               pub BAD: Secret := 1\n\
               fn main() -> i32 = 0\n";
    let d = check_diagnostics("pub-const-user-type", src);
    assert!(
        d.contains("a constant's type must be a scalar, `str`, or `slice<T>`"),
        "a const naming a user type is caught by the scalar-only rule, not the exposure check, got:\n{d}"
    );
}

// ---- positive controls: legal shapes still compile ----------------------------------------------

#[test]
fn pub_interface_over_pub_types_compiles_and_runs() {
    if !backend_available() {
        return;
    }
    let src = "pub Point { x: i64, y: i64 }\n\
               pub fn origin() -> Point = Point{ x: 7, y: 0 }\n\
               fn main() -> i32 {\n  p := origin()\n  return p.x as i32\n}\n";
    let out = build_and_run("pub-exposes-pub", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn pub_struct_field_of_pub_struct_is_legal() {
    let src = "pub Inner { a: i64 }\n\
               pub Outer { i: Inner }\n\
               fn main() -> i32 = 0\n";
    assert!(!check_errs("pub-of-pub-struct", src), "pub-exposes-pub must be legal");
}

#[test]
fn private_fn_using_private_types_stays_legal() {
    // A PRIVATE interface may reference anything — only `pub` items are constrained.
    let src = "Secret { v: i64 }\n\
               fn make() -> Secret = Secret{ v: 1 }\n\
               fn use_it(s: Secret) -> i64 = s.v\n\
               fn main() -> i32 {\n  s := make()\n  return use_it(s) as i32\n}\n";
    assert!(!check_errs("private-uses-private", src), "a private interface may expose private types");
}

#[test]
fn pub_generic_fn_over_its_own_type_params_is_legal() {
    // A type-parameter name is `Ty::Param`, not a real type — it is never an exposure.
    let src = "pub fn pick<T>(a: T, b: T) -> T = a\n\
               fn main() -> i32 = pick(1, 2) as i32\n";
    assert!(!check_errs("pub-generic-params", src), "a generic pub fn over its own params must be legal");
}

// ---- generic `pub` fn BODY may reference only pub items (M15 S1b, gate finding D1) ----------------
//
// A generic `pub` fn's body is part of its interface: the summary ships the template and it is
// monomorphized in importing modules, where the defining module's PRIVATE items do not exist. So a
// generic `pub` fn body may reference only `pub` same-module items. Enforced at the producer in
// ordinary sema (both checkers), so `check` and `check-per-unit` agree — see the differential parity
// case in `per_unit.rs`. Non-generic `pub` fns and private generic fns are NOT affected.

#[test]
fn generic_pub_fn_body_referencing_private_fn_is_rejected() {
    // D1 (single-file view): the generic `pub` template `bump` calls the private `helper`.
    let src = "fn helper(x: i64) -> i64 = x + 1\n\
               pub fn bump<T>(x: T, n: i64) -> i64 = helper(n)\n\
               fn main() -> i32 = bump(0, 41) as i32\n";
    let d = check_diagnostics("gen-body-private-fn", src);
    assert!(
        d.contains("pub generic fn 'bump' references private fn 'helper' in its body"),
        "expected the private-fn body-reference error, got:\n{d}"
    );
}

#[test]
fn generic_pub_fn_body_referencing_private_type_annotation_is_rejected() {
    // A private type named in a `let` annotation inside the template body.
    let src = "Secret { v: i64 }\n\
               pub fn wrap<T>(x: T) -> i64 {\n  s: Secret := Secret { v: 1 }\n  return s.v\n}\n\
               fn main() -> i32 = wrap(0) as i32\n";
    let d = check_diagnostics("gen-body-private-type-anno", src);
    assert!(
        d.contains("pub generic fn 'wrap' references private type 'Secret' in its body"),
        "expected the private-type body-reference error, got:\n{d}"
    );
}

#[test]
fn generic_pub_fn_body_referencing_private_type_struct_literal_is_rejected() {
    // The struct-literal type name itself names a private type.
    let src = "Secret { v: i64 }\n\
               pub fn wrap<T>(x: T) -> i64 {\n  s := Secret { v: 1 }\n  return s.v\n}\n\
               fn main() -> i32 = wrap(0) as i32\n";
    let d = check_diagnostics("gen-body-private-type-lit", src);
    assert!(
        d.contains("pub generic fn 'wrap' references private type 'Secret' in its body"),
        "expected the private-type struct-literal error, got:\n{d}"
    );
}

#[test]
fn generic_pub_fn_body_referencing_private_const_is_rejected() {
    // A private top-level constant referenced from the template body.
    let src = "SECRET := 42\n\
               pub fn add<T>(x: T) -> i64 = SECRET + 1\n\
               fn main() -> i32 = add(0) as i32\n";
    let d = check_diagnostics("gen-body-private-const", src);
    assert!(
        d.contains("pub generic fn 'add' references private const 'SECRET' in its body"),
        "expected the private-const body-reference error, got:\n{d}"
    );
}

#[test]
fn generic_pub_fn_body_referencing_private_enum_construction_is_rejected() {
    // Constructing a private enum inside the template body names the private type as the receiver of
    // `Shape.Circle(..)` — a body leak even with no `match` (the construction alone is the reference).
    let src = "Shape { Circle(i64), Square(i64) }\n\
               pub fn mk<T>(x: T) -> i64 {\n  s := Shape.Circle(3)\n  return 0\n}\n\
               fn main() -> i32 = mk(0) as i32\n";
    let d = check_diagnostics("gen-body-private-enum", src);
    assert!(
        d.contains("pub generic fn 'mk' references private type 'Shape' in its body"),
        "expected the private-enum construction error, got:\n{d}"
    );
}

#[test]
fn generic_pub_fn_body_referencing_pub_helper_is_legal() {
    // A `pub` helper (fn / const / type) is part of the interface, so the template may use it.
    let src = "pub SCALE := 10\n\
               pub fn helper(x: i64) -> i64 = x + 1\n\
               pub Wrap { v: i64 }\n\
               pub fn bump<T>(x: T, n: i64) -> i64 {\n  w := Wrap { v: helper(n) + SCALE }\n  return w.v\n}\n\
               fn main() -> i32 = bump(0, 41) as i32\n";
    assert!(!check_errs("gen-body-pub-helper", src), "a generic pub body may reference pub items");
}

#[test]
fn generic_pub_fn_body_local_shadowing_private_fn_is_legal() {
    // A local named like a private fn shadows it (scoping respected) — no reference to the item.
    let src = "fn helper(x: i64) -> i64 = x + 1\n\
               pub fn bump<T>(x: T) -> i64 {\n  helper := 7\n  return helper + 1\n}\n\
               fn main() -> i32 = bump(0) as i32\n";
    assert!(!check_errs("gen-body-local-shadow", src), "a local shadowing a private fn must be legal");
}

#[test]
fn generic_pub_fn_body_using_only_type_params_is_legal() {
    // Type parameters are substituted at instantiation — never a same-module item reference.
    let src = "pub fn id<T>(x: T) -> T {\n  y: T := x\n  return y\n}\n\
               fn main() -> i32 = id(0) as i32\n";
    assert!(!check_errs("gen-body-type-params", src), "type params used in a generic body must be legal");
}

#[test]
fn non_generic_pub_fn_body_may_reference_private_items() {
    // A NON-generic `pub` fn's body stays in its own unit (never shipped as a template), so it may
    // freely reference private same-module items.
    let src = "fn helper(x: i64) -> i64 = x + 1\n\
               SECRET := 5\n\
               pub fn bump(n: i64) -> i64 = helper(n) + SECRET\n\
               fn main() -> i32 = bump(41) as i32\n";
    assert!(!check_errs("nongeneric-pub-body", src), "a non-generic pub body may reference private items");
}

#[test]
fn private_generic_fn_body_may_reference_private_items() {
    // A PRIVATE generic fn is not part of any interface, so its body may reference private items.
    let src = "fn helper(x: i64) -> i64 = x + 1\n\
               fn bump<T>(x: T, n: i64) -> i64 = helper(n)\n\
               fn main() -> i32 = bump(0, 41) as i32\n";
    assert!(!check_errs("private-generic-body", src), "a private generic body may reference private items");
}
