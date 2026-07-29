//! Regression tests for coverage holes in the escape / effect / move analyses — cases where a
//! value that must not escape (or an impurity that must be seen) slipped through because a
//! per-`ExprKind` hand-written traversal was missing an arm. Each is a program that used to
//! type-check (allowing a use-after-free or a purity bypass) and must now be rejected, plus the
//! matching false-positive fix. Found by an external multi-agent audit (report 2026-07-02).

mod common;
use common::*;

#[test]
fn terminating_break_payload_does_not_taint_parallel_callback_effect() {
    let pure = "\
fn pure(value: i64) -> i64 = value + 1
fn impure(value: i64) -> i64 {
  print(value)
  return value
}
fn wrapper(value: i64) -> i64 {
  f: fn(i64) -> i64 := loop {
    break { break pure; impure }
  }
  return f(value)
}
fn main() -> i32 {
  print([1, 2].par_map(wrapper).sum())
  return 0
}
";
    let pure_diagnostics =
        check_diagnostics("effect-terminating-break-payload", pure);
    assert!(
        pure_diagnostics.is_empty(),
        "a callback after an inner terminating break is dead and must not taint the outer loop result:\n{pure_diagnostics}"
    );

    let impure = pure.replace(
        "break { break pure; impure }",
        "break impure",
    );
    let diagnostics =
        check_diagnostics("effect-fallthrough-break-payload", &impure);
    assert!(
        diagnostics.contains("'par_map' requires a Pure function"),
        "an ordinary fallthrough break must still join its Impure callback into the loop result:\n{diagnostics}"
    );
}

#[test]
fn recursive_drop_plan_is_cycle_safe() {
    let src = "\
Node { next: Option<Node> }
fn main() -> i32 = 0
";
    assert!(
        check_errs("recursive-drop-plan", src),
        "an inline recursive tagged field must diagnose instead of recursing or panicking"
    );
}

#[test]
fn owned_array_option_field_is_admitted_after_l1b() {
    let src = "\
Holder { values: Option<array<i64>> }
fn main() -> i32 = 0
";
    assert!(
        !check_errs("owned-array-option-field", src),
        "L1b must admit an owned array Option field through its recursive DropPlan"
    );
}

#[test]
fn move_enum_option_field_is_admitted_after_l1b() {
    let src = "\
Content { Empty, Data(array<i64>) }
Holder { value: Option<Content> }
fn main() -> i32 = 0
";
    assert!(
        !check_errs("move-enum-option-field", src),
        "L1b must admit an Option payload whose resolved enum DropPlan owns storage"
    );
}

#[test]
fn move_struct_enum_payload_is_admitted_independent_of_declaration_order() {
    let src = "\
Inner { content: Content }
Wrapper { Wrapped(Inner) }
Content { Empty, Data(array<i64>) }
fn main() -> i32 = 0
";
    assert!(
        !check_errs("late-enum-move-struct-payload", src),
        "a later-resolved enum must give its enclosing Move struct a valid recursive payload plan"
    );
}

#[test]
fn generic_struct_owned_option_fields_are_revalidated_after_monomorphization() {
    let direct = "\
Owned { name: string }
Wrap<T> { value: Option<T> }
Holder { wrapped: Wrap<Owned> }
fn main() -> i32 = 0
";
    assert!(
        !check_errs("generic-struct-direct-owned-option", direct),
        "a generic struct monomorph must admit Option<MoveStruct>"
    );

    let transitive = "\
Inner { content: Content }
Wrap<T> { value: Option<T> }
Holder { wrapped: Wrap<Inner> }
Content { Empty, Data(array<i64>) }
fn main() -> i32 = 0
";
    assert!(
        !check_errs("generic-struct-enum-owned-option", transitive),
        "a cached generic struct must retain a valid DropPlan when its payload becomes Move through an enum"
    );

    let array = "\
Wrap<T> { value: Option<T> }
Holder { wrapped: Wrap<array<i64>> }
fn main() -> i32 = 0
";
    assert!(
        !check_errs("generic-struct-array-owned-option", array),
        "a generic struct monomorph must admit another finite owned Option payload"
    );
}

#[test]
fn generic_enum_move_payloads_are_revalidated_after_monomorphization() {
    let direct = "\
Inner { content: Content }
Wrap<T> { Wrapped(T), Empty }
Holder { wrapped: Wrap<Inner> }
Content { Empty, Data(array<i64>) }
fn main() -> i32 = 0
";
    assert!(
        !check_errs("generic-enum-move-struct-payload", direct),
        "a cached generic enum must retain a valid DropPlan when its struct payload becomes Move"
    );

    let array = "\
Inner { content: Content }
Wrap<T> { Wrapped(T), Empty }
Holder { wrapped: Wrap<array<Inner>> }
Content { Empty, Data(array<i64>) }
fn main() -> i32 = 0
";
    assert!(
        check_errs("generic-enum-move-array-payload", array),
        "a cached generic enum must still reject the separately deferred array-of-Move payload"
    );
}

// --- 1-2: an arena value escaping through a `match` arm (region_of lacked `Match`) ---
#[test]
fn arena_value_escaping_via_match_arm_is_rejected() {
    let src = "\
Tag { A, B }
fn main() -> i32 {
  v := Tag.A
  s := arena {
    n := 42
    t := template \"n={n}\"
    match v { A => t, B => t }
  }
  print(s.len())
  return 0
}
";
    assert!(check_errs("match-escape", src), "arena str escaping via a match arm must be rejected");
}

// --- NEW-1: an arena value escaping through an indirect call (region_of lacked `CallFnValue`) ---
#[test]
fn arena_value_escaping_via_indirect_call_is_rejected() {
    let src = "\
fn idstr(s: str) -> str = s
fn main() -> i32 {
  g := idstr
  out := arena {
    n := 5
    t := template \"n={n}\"
    g(t)
  }
  print(out.len())
  return 0
}
";
    assert!(check_errs("callfnvalue-escape", src), "arena str escaping via an indirect call must be rejected");
}

#[test]
fn arena_capture_returned_by_zero_arg_closure_is_rejected() {
    let src = "\
fn main() -> i32 {
  v := arena {
    n := 7
    s := template \"hello {n}\"
    f := fn { s }
    f()
  }
  return v.len() as i32
}
";
    assert!(
        check_errs("callfnvalue-capture-result-escape", src),
        "an indirect call result must not outlive the closure environment it can borrow"
    );
}

// A direct call of the same borrow-returning function is (and stays) rejected — the control that
// proves the indirect path was the only gap.
#[test]
fn arena_value_escaping_via_direct_call_is_rejected() {
    let src = "\
fn idstr(s: str) -> str = s
fn main() -> i32 {
  out := arena {
    n := 5
    t := template \"n={n}\"
    idstr(t)
  }
  print(out.len())
  return 0
}
";
    assert!(check_errs("call-escape", src), "arena str escaping via a direct call must be rejected");
}

// --- 1-5: a slice viewing a local array, returned (slice_is_local lacked `SliceRange`) ---
#[test]
fn returning_range_slice_of_local_array_is_rejected() {
    let src = "\
fn f() -> slice<i64> {
  xs := [1, 2, 3]
  return xs[0..2]
}
fn main() -> i32 { return 0 }
";
    assert!(check_errs("slicerange-return", src), "returning a range slice of a local array must be rejected");
}

// A wrapper must not hide the same frame-local borrow from the return escape check.
#[test]
fn returning_local_array_slice_inside_result_is_rejected() {
    let src = "\
fn f() -> Result<slice<i64>, Error> {
  xs := [1, 2, 3]
  return Ok(xs[..])
}
fn main() -> i32 { return 0 }
";
    assert!(
        check_errs("wrapped-slicerange-return", src),
        "Result must not hide a returned slice of a local array"
    );
}

#[test]
fn returning_local_array_slice_through_wrapped_local_is_rejected() {
    let src = "\
fn f() -> Option<slice<i64>> {
  xs := [1, 2, 3]
  wrapped := Some(xs[..])
  return wrapped
}
fn main() -> i32 { return 0 }
";
    assert!(
        check_errs("wrapped-slice-local-return", src),
        "a wrapper local must retain local-slice provenance"
    );
}

#[test]
fn returning_local_array_slice_through_match_payload_is_rejected() {
    let src = "\
fn f() -> Result<slice<i64>, Error> {
  xs := [1, 2, 3]
  wrapped: Result<slice<i64>, Error> := Ok(xs[..])
  return match wrapped {
    Ok(s) => Ok(s),
    Err(e) => Err(e),
  }
}
fn main() -> i32 { return 0 }
";
    assert!(
        check_errs("wrapped-slice-match-return", src),
        "a match payload must retain local-slice provenance"
    );
}

#[test]
fn returning_caller_slice_inside_result_is_allowed() {
    let src = "\
fn pass(xs: slice<i64>) -> Result<slice<i64>, Error> {
  wrapped: Result<slice<i64>, Error> := Ok(xs)
  return wrapped
}
fn main() -> i32 { return 0 }
";
    assert!(
        !check_errs("wrapped-slice-param-return", src),
        "a wrapped caller-provided slice remains returnable"
    );
}

// --- 1-6: an arena str stored into an outer array element (AssignIndex lacked a region check) ---
#[test]
fn storing_arena_str_into_outer_array_element_is_rejected() {
    let src = "\
fn main() -> i32 {
  mut arr := [\"aa\", \"bb\"]
  arena {
    n := 5
    t := template \"n={n}\"
    arr[0] = t
  }
  print(arr[0].len())
  return 0
}
";
    assert!(check_errs("elem-assign-escape", src), "storing an arena str into an outer array element must be rejected");
}

// --- 1-4: an impure function laundered through a fn value, used in par_map (EffectScan lacked
//          the `FnValue` call edge) ---
#[test]
fn impure_fn_via_fn_value_rejected_in_par_map() {
    let src = "\
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn sneaky(x: i64) -> i64 {
  g := loud
  return g(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(sneaky)
  print(ys.sum())
  return Ok(())
}
";
    assert!(check_errs("parmap-fnvalue-purity", src), "an impure fn laundered through a fn value must be rejected by par_map");
}

#[test]
fn unused_impure_capturing_closure_does_not_taint_purity() {
    let src = "\
fn worker(x: i64) -> i64 {
  k := 100
  f := fn y: i64 {
    print(y + k)
    y
  }
  return x
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        !check_errs("parmap-unused-capturing-closure-purity", src),
        "constructing an Impure closure without invoking it has no observable effect"
    );
}

#[test]
fn pure_fn_type_effect_allows_indirect_par_map_worker() {
    let src = "\
fn inc(x: i64) -> i64 = x + 1
fn worker(x: i64) -> i64 {
  f := inc
  return f(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        !check_errs("parmap-pure-fn-type-effect", src),
        "a known-Pure function value must remain Pure through an indirect local call"
    );
}

#[test]
fn pure_capturing_closure_type_effect_allows_indirect_par_map_worker() {
    let src = "\
fn worker(x: i64) -> i64 {
  k := 10
  f := fn y: i64 { y + k }
  return f(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        !check_errs("parmap-pure-closure-type-effect", src),
        "a known-Pure lifted closure must carry Pure through its function type"
    );
}

#[test]
fn pure_recursive_fn_value_cycle_reaches_least_effect_fixpoint() {
    let src = "\
fn countdown(x: i64) -> i64 {
  f := countdown
  if x == 0 { return 0 }
  return f(x - 1)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(countdown)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        !check_errs("parmap-pure-recursive-fn-type-effect", src),
        "a recursive concrete function value must converge to the least Pure effect"
    );
}

#[test]
fn mutable_fn_type_effect_joins_impure_assignment() {
    let src = "\
fn inc(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  mut f := inc
  if x > 0 { f = loud }
  return f(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_errs("parmap-joined-fn-type-effect", src),
        "a mutable function value must conservatively join every assigned target's effect"
    );
}

#[test]
fn generic_fn_wrapper_keeps_same_signature_effect_origins_distinct() {
    let pure = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn quiet_worker(x: i64) -> i64 {
  holder := Holder { callback: quiet }
  return holder.callback(x)
}
fn loud_worker(x: i64) -> i64 {
  holder := Holder { callback: loud }
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(quiet_worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        !check_errs("parmap-generic-fn-wrapper-pure", pure),
        "an unrelated Impure wrapper with the same source signature must not poison the Pure origin"
    );

    let impure = pure.replace("par_map(quiet_worker)", "par_map(loud_worker)");
    assert!(
        check_errs("parmap-generic-fn-wrapper-impure", &impure),
        "a generic wrapper must not reuse a same-signature Pure field type for an Impure function value"
    );
}

#[test]
fn explicit_generic_callback_boundaries_preserve_origins() {
    let returned = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn make() -> Holder<fn(i64) -> i64> {
  return Holder { callback: quiet }
}
fn worker(x: i64) -> i64 {
  holder := make()
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    let returned_diagnostics =
        check_diagnostics("parmap-explicit-generic-fn-return-pure", returned);
    assert!(
        returned_diagnostics.is_empty(),
        "an explicit callback-bearing return must retain its known-Pure origin:\n{returned_diagnostics}"
    );
    let impure_returned = returned.replace(
        "return Holder { callback: quiet }",
        "return Holder { callback: loud }",
    );
    assert!(
        check_diagnostics(
            "parmap-explicit-generic-fn-return-impure",
            &impure_returned,
        )
        .contains("has an observable side effect"),
        "an explicit callback-bearing return must retain its Impure origin"
    );

    let parameter = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn apply(holder: Holder<fn(i64) -> i64>, x: i64) -> i64 {
  return holder.callback(x)
}
fn worker(x: i64) -> i64 {
  return apply(Holder { callback: quiet }, x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    let parameter_diagnostics =
        check_diagnostics("parmap-explicit-generic-fn-param-pure", parameter);
    assert!(
        parameter_diagnostics.is_empty(),
        "an explicit callback-bearing parameter must retain its known-Pure origin:\n{parameter_diagnostics}"
    );
    let impure_parameter = parameter.replace(
        "apply(Holder { callback: quiet }, x)",
        "apply(Holder { callback: loud }, x)",
    );
    assert!(
        check_diagnostics(
            "parmap-explicit-generic-fn-param-impure",
            &impure_parameter,
        )
        .contains("has an observable side effect"),
        "an explicit callback-bearing parameter must retain its Impure origin"
    );

    let isolated_parameters = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn apply_quiet(holder: Holder<fn(i64) -> i64>, x: i64) -> i64 {
  return holder.callback(x)
}
fn apply_loud(holder: Holder<fn(i64) -> i64>, x: i64) -> i64 {
  return holder.callback(x)
}
fn quiet_worker(x: i64) -> i64 {
  return apply_quiet(Holder { callback: quiet }, x)
}
fn loud_worker(x: i64) -> i64 {
  return apply_loud(Holder { callback: loud }, x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(quiet_worker)
  print(ys.sum())
  return Ok(())
}
";
    let isolated_diagnostics = check_diagnostics(
        "parmap-explicit-generic-fn-param-isolated",
        isolated_parameters,
    );
    assert!(
        isolated_diagnostics.is_empty(),
        "an Impure peer with the same written signature must not poison a Pure parameter:\n{isolated_diagnostics}"
    );
    let isolated_impure = isolated_parameters.replace(
        "par_map(quiet_worker)",
        "par_map(loud_worker)",
    );
    assert!(
        check_diagnostics(
            "parmap-explicit-generic-fn-param-isolated-impure",
            &isolated_impure,
        )
        .contains("has an observable side effect"),
        "per-function parameter isolation must still reject the Impure peer"
    );

    let loop_returned = returned.replace(
        "return Holder { callback: quiet }",
        "return loop {\n    break Holder { callback: quiet }\n  }",
    );
    let loop_returned_diagnostics = check_diagnostics(
        "parmap-explicit-generic-fn-loop-return-pure",
        &loop_returned,
    );
    assert!(
        loop_returned_diagnostics.is_empty(),
        "a callback-bearing loop result must retain its known-Pure origin across an explicit return:\n{loop_returned_diagnostics}"
    );
    let impure_loop_returned =
        loop_returned.replace("callback: quiet", "callback: loud");
    assert!(
        check_diagnostics(
            "parmap-explicit-generic-fn-loop-return-impure",
            &impure_loop_returned,
        )
        .contains("has an observable side effect"),
        "a callback-bearing loop result must retain its Impure origin across an explicit return"
    );

    let loop_parameter = parameter.replace(
        "return apply(Holder { callback: quiet }, x)",
        "holder := loop {\n    break Holder { callback: quiet }\n  }\n  return apply(holder, x)",
    );
    let loop_parameter_diagnostics = check_diagnostics(
        "parmap-explicit-generic-fn-loop-param-pure",
        &loop_parameter,
    );
    assert!(
        loop_parameter_diagnostics.is_empty(),
        "a callback-bearing loop result must retain its known-Pure origin across an explicit parameter:\n{loop_parameter_diagnostics}"
    );
    let impure_loop_parameter =
        loop_parameter.replace("callback: quiet", "callback: loud");
    assert!(
        check_diagnostics(
            "parmap-explicit-generic-fn-loop-param-impure",
            &impure_loop_parameter,
        )
        .contains("has an observable side effect"),
        "a callback-bearing loop result must retain its Impure origin across an explicit parameter"
    );

    let nested_loop_parameter = parameter.replace(
        "return apply(Holder { callback: quiet }, x)",
        "holder := loop {\n    unused := loop {\n      break Holder { callback: loud }\n    }\n    break Holder { callback: quiet }\n  }\n  return apply(holder, x)",
    );
    let nested_loop_diagnostics = check_diagnostics(
        "parmap-explicit-generic-fn-nested-loop-isolation",
        &nested_loop_parameter,
    );
    assert!(
        nested_loop_diagnostics.is_empty(),
        "an unused inner Impure loop result must not contaminate the outer Pure loop boundary:\n{nested_loop_diagnostics}"
    );
}

#[test]
fn source_compatible_callback_signatures_and_pipeline_inputs() {
    let pure = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn invoke(holder: Holder<fn(i64) -> i64>, x: i64) -> i64 {
  return holder.callback(x)
}
fn keep(holder: Holder<fn(i64) -> i64>) -> bool = true
fn identity(
  holder: Holder<fn(i64) -> i64>,
) -> Holder<fn(i64) -> i64> = holder
fn apply(holder: Holder<fn(i64) -> i64>) -> i64 {
  return holder.callback(1)
}
fn main() -> Result<(), Error> {
  indirect := invoke
  print(indirect(Holder { callback: quiet }, 41))
  holders: array<Holder<fn(i64) -> i64>> :=
    [Holder { callback: quiet }].to_array()
  ys := holders.where(keep).map(identity).par_map(apply)
  print(ys.sum())
  return Ok(())
}
";
    let explicit_diagnostics = check_diagnostics(
        "source-compatible-callback-signatures-explicit",
        pure,
    );
    assert!(
        explicit_diagnostics.is_empty(),
        "source-compatible indirect and pipeline signatures must accept matching callback aggregates:\n{explicit_diagnostics}"
    );

    let inferred = pure.replace(
        "holders: array<Holder<fn(i64) -> i64>> :=",
        "holders :=",
    );
    let inferred_diagnostics = check_diagnostics(
        "source-compatible-callback-signatures-inferred",
        &inferred,
    );
    assert!(
        inferred_diagnostics.is_empty(),
        "origin-aware inferred pipeline inputs must remain source-compatible and preserve Pure callback origins:\n{inferred_diagnostics}"
    );

    let impure = inferred.replace("callback: quiet", "callback: loud");
    assert!(
        check_diagnostics(
            "source-compatible-callback-signatures-impure",
            &impure,
        )
        .contains("has an observable side effect"),
        "pipeline parameter propagation must still reject an Impure callback origin"
    );
}

#[test]
fn callback_bearing_indirect_consumers_transfer_or_fail_closed() {
    let indirect = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn apply(holder: Holder<fn(i64) -> i64>, x: i64) -> i64 {
  return holder.callback(x)
}
fn seed() -> i64 = apply(Holder { callback: quiet }, 0)
fn worker(x: i64) -> i64 {
  indirect := apply
  return indirect(Holder { callback: loud }, x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    let indirect_diagnostics = check_diagnostics(
        "parmap-callback-bearing-indirect-fail-closed",
        indirect,
    );
    assert!(
        indirect_diagnostics.contains(
            "calls a function value whose effect is not statically known"
        ),
        "a callback-bearing actual passed through an unresolved function-value target must fail closed at a parallel boundary:\n{indirect_diagnostics}"
    );

    let map_err = "\
Wrap<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn convert(wrap: Wrap<fn(i64) -> i64>) -> Error {
  wrap.callback(1)
  return Error.Invalid
}
fn seed() -> Result<i64, Error> {
  result: Result<i64, Wrap<fn(i64) -> i64>> :=
    Err(Wrap { callback: quiet })
  return result.map_err(convert)
}
fn worker(x: i64) -> i64 {
  result: Result<i64, Wrap<fn(i64) -> i64>> :=
    Err(Wrap { callback: loud })
  mapped := result.map_err(convert)
  return x
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    let map_err_diagnostics = check_diagnostics(
        "parmap-callback-bearing-map-err-transfer",
        map_err,
    );
    assert!(
        map_err_diagnostics.contains("has an observable side effect"),
        "map_err must transfer its Result error producer into the named conversion boundary:\n{map_err_diagnostics}"
    );

    let question = "\
Wrap<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn convert(wrap: Wrap<fn(i64) -> i64>) -> Error {
  wrap.callback(1)
  return Error.Invalid
}
fn pass(
  result: Result<i64, Wrap<fn(i64) -> i64>>,
) -> Result<i64, Wrap<fn(i64) -> i64>> {
  value := result?
  return Ok(value)
}
fn seed_convert() -> Error {
  return convert(Wrap { callback: quiet })
}
fn worker(x: i64) -> i64 {
  result: Result<i64, Wrap<fn(i64) -> i64>> :=
    Err(Wrap { callback: loud })
  mapped := pass(result).map_err(convert)
  return x
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    let question_diagnostics = check_diagnostics(
        "parmap-callback-bearing-question-err-transfer",
        question,
    );
    assert!(
        question_diagnostics.contains("has an observable side effect"),
        "a question-mark early return must transfer its Err origin into the enclosing Result boundary:\n{question_diagnostics}"
    );
}

#[test]
fn map_err_accepts_source_compatible_callback_aggregates() {
    let src = "\
Wrap<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn keep_error(
  wrap: Wrap<fn(i64) -> i64>,
) -> Wrap<fn(i64) -> i64> = wrap
fn fail<E>(error: E) -> Result<i64, E> = Err(error)
fn main() -> Result<(), Error> {
  mapped := fail(Wrap { callback: quiet }).map_err(keep_error)
  return Ok(())
}
";
    let diagnostics = check_diagnostics(
        "map-err-source-compatible-callback-aggregate",
        src,
    );
    assert!(
        diagnostics.is_empty(),
        "map_err must compare its callback-bearing error parameter by source identity:\n{diagnostics}"
    );
}

#[test]
fn generic_fn_wrapper_matches_an_explicit_source_signature() {
    if !backend_available() {
        return;
    }
    let src = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn use(h: Holder<fn(i64) -> i64>, x: i64) -> i64 = h.callback(x)
fn main() -> i32 {
  first := Holder { callback: quiet }
  second := Holder { callback: quiet }
  return (use(first, 20) + use(second, 20)) as i32
}
";
    assert!(
        !check_errs("generic-fn-wrapper-source-signature", src),
        "an inferred concrete function origin must retain the source-visible generic nominal identity"
    );
    assert_eq!(
        build_and_run("generic-fn-wrapper-source-signature", src)
            .status
            .code(),
        Some(42)
    );
    let option = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn use(maybe: Option<Holder<fn(i64) -> i64>>, x: i64) -> i64 {
  holder := maybe else { return 0 }
  return holder.callback(x)
}
fn main() -> i32 {
  holder := Holder { callback: quiet }
  return use(Some(holder), 41) as i32
}
";
    assert_eq!(
        build_and_run("generic-fn-wrapper-source-signature-option", option)
            .status
            .code(),
        Some(42),
        "source nominal compatibility must recurse through aggregate payloads"
    );
    let array = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn main() -> i32 {
  holders := [
    Holder { callback: quiet },
    Holder { callback: quiet },
  ]
  return holders[1].callback(41) as i32
}
";
    assert_eq!(
        build_and_run("generic-fn-wrapper-source-signature-array", array)
            .status
            .code(),
        Some(42),
        "a struct array must accept elements with one source nominal identity"
    );
    let slice = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn use(holders: slice<Holder<fn(i64) -> i64>>) -> i64 = holders.len()
fn main() -> i32 {
  holders := [
    Holder { callback: quiet },
    Holder { callback: quiet },
  ]
  return (use(holders) + 40) as i32
}
";
    assert_eq!(
        build_and_run("generic-fn-wrapper-source-signature-slice", slice)
            .status
            .code(),
        Some(42),
        "a source-compatible generic struct array must borrow as its declared slice type"
    );
}

#[test]
fn reassigned_generic_fn_wrapper_joins_effect_origins() {
    let src = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  mut holder := Holder { callback: quiet }
  if x > 0 { holder = Holder { callback: loud } }
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_errs("parmap-reassigned-generic-fn-wrapper", src),
        "assigning another concrete origin into a generic wrapper must join its effect"
    );
    let field = src.replace(
        "if x > 0 { holder = Holder { callback: loud } }",
        "holder.callback = loud",
    );
    assert!(
        check_errs("parmap-reassigned-generic-fn-field", &field),
        "assigning another concrete origin into a generic wrapper field must join its effect"
    );
    let joined = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  holder := if x > 0 {
    Holder { callback: quiet }
  } else {
    Holder { callback: loud }
  }
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_errs("parmap-joined-generic-fn-wrapper", joined),
        "joining concrete origins into a generic wrapper value must join their effects"
    );
    let array_field = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  mut holders := [Holder { callback: quiet }]
  holders[0].callback = loud
  return holders[0].callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_errs("parmap-reassigned-generic-fn-array-field", array_field),
        "assigning another concrete origin into a generic wrapper array field must join its effect"
    );
    let array_literal = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  holders := [
    Holder { callback: quiet },
    Holder { callback: loud },
  ]
  return holders[0].callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_errs("parmap-joined-generic-fn-array-literal", array_literal),
        "source-compatible generic wrapper array elements must join their concrete effects"
    );
    let dynamic_array = array_literal.replace(
        "  ]\n  return holders[0].callback(x)",
        "  ].to_array()\n  return holders[x % 2].callback(x)",
    );
    assert!(
        check_diagnostics("parmap-joined-generic-fn-dynamic-array", &dynamic_array)
            .contains("has an observable side effect"),
        "dynamic materialization must preserve the joined effects of its generic struct elements"
    );
    let replaced_dynamic_array = dynamic_array.replace(
        "  holders := [\n    Holder { callback: quiet },\n    Holder { callback: loud },\n  ].to_array()",
        "  mut holders := [\n    Holder { callback: quiet },\n    Holder { callback: quiet },\n  ].to_array()\n  holders = [\n    Holder { callback: quiet },\n    Holder { callback: loud },\n  ].to_array()",
    );
    assert!(
        check_diagnostics(
            "parmap-reassigned-generic-fn-dynamic-array",
            &replaced_dynamic_array,
        )
        .contains("has an observable side effect"),
        "dynamic array replacement must join the effects of every replacement element"
    );
}

#[test]
fn tagged_generic_fn_wrappers_join_effect_origins() {
    let option = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  mut maybe := Some(Holder { callback: quiet })
  if x > 0 { maybe = Some(Holder { callback: loud }) }
  holder := maybe else { return x }
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_diagnostics("parmap-reassigned-option-fn-wrapper", option)
            .contains("has an observable side effect"),
        "Option reassignment must join a nested callback's concrete effects"
    );

    let nested_option = option
        .replace(
            "mut maybe := Some(Holder { callback: quiet })",
            "mut maybe := Some(Some(Holder { callback: quiet }))",
        )
        .replace(
            "maybe = Some(Holder { callback: loud })",
            "maybe = Some(Some(Holder { callback: loud }))",
        )
        .replace(
            "holder := maybe else { return x }",
            "inner := maybe else { return x }\n  holder := inner else { return x }",
        );
    assert!(
        check_diagnostics(
            "parmap-reassigned-nested-option-fn-wrapper",
            &nested_option,
        )
        .contains("has an observable side effect"),
        "nested tagged payloads must recursively join callback effects"
    );

    let impure_fallback = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  mut maybe := Some(Holder { callback: quiet })
  if x < 0 { maybe = None }
  holder := maybe else { Holder { callback: loud } }
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_diagnostics("parmap-option-impure-fallback-fn-wrapper", impure_fallback)
            .contains("has an observable side effect"),
        "a non-diverging else fallback must join its callback effect with the success payload"
    );

    let result = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn wrap<T>(value: T) -> Result<T, Error> = Ok(value)
fn worker(x: i64) -> i64 {
  mut result := wrap(Holder { callback: quiet })
  if x > 0 { result = wrap(Holder { callback: loud }) }
  holder := result else { return x }
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_diagnostics("parmap-reassigned-result-fn-wrapper", result)
            .contains("has an observable side effect"),
        "Result reassignment must join a nested callback's concrete effects"
    );

    let sum = "\
Holder<T> { callback: T }
Choice<T> { Some(T), None }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  mut choice := Choice.Some(Holder { callback: quiet })
  if x > 0 { choice = Choice.Some(Holder { callback: loud }) }
  return match choice {
    Some(holder) => holder.callback(x),
    None => x,
  }
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_diagnostics("parmap-reassigned-sum-fn-wrapper", sum)
            .contains("has an observable side effect"),
        "a generic sum reassignment must join the selected payload's callback effects"
    );
}

#[test]
fn pure_tagged_unwraps_preserve_nested_fn_effects() {
    let option = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn worker(x: i64) -> i64 {
  mut maybe := Some(Holder { callback: quiet })
  if x < 0 { maybe = None }
  holder := maybe else { return x }
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        !check_errs("parmap-pure-option-unwrapped-fn-wrapper", option),
        "an absent Option payload must not turn its known-Pure success callback into Unknown"
    );

    let result = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn wrap<T>(value: T) -> Result<T, Error> = Ok(value)
fn through(x: i64) -> Result<i64, Error> {
  mut result := wrap(Holder { callback: quiet })
  if x < 0 { result = Err(error(1)) }
  holder := result?
  return Ok(holder.callback(x))
}
fn worker(x: i64) -> i64 {
  result := through(x)
  return result else { return x }
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        !check_errs("parmap-pure-result-try-fn-wrapper", result),
        "`?` and Result else-unwrap must preserve a known-Pure nested callback"
    );

    let option_match = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  maybe := Some(Holder { callback: quiet })
  return match maybe {
    Some(holder) => holder.callback(x),
    None => x,
  }
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    let option_match_diagnostics = check_diagnostics(
        "parmap-pure-option-match-fn-wrapper",
        option_match,
    );
    assert!(
        option_match_diagnostics.is_empty(),
        "an Option match payload must preserve its known-Pure callback origin:\n{option_match_diagnostics}"
    );
    let impure_option_match =
        option_match.replace("callback: quiet", "callback: loud");
    assert!(
        check_diagnostics(
            "parmap-impure-option-match-fn-wrapper",
            &impure_option_match,
        )
        .contains("has an observable side effect"),
        "an Option match payload must preserve its Impure callback origin"
    );

    let result_match = option_match.replace(
        "maybe := Some(Holder { callback: quiet })",
        "maybe: Result<Holder<fn(i64) -> i64>, Error> :=\n    Ok(Holder { callback: quiet })",
    ).replace(
        "Some(holder) => holder.callback(x),\n    None => x,",
        "Ok(holder) => holder.callback(x),\n    Err(err) => x,",
    );
    let result_match_diagnostics = check_diagnostics(
        "parmap-pure-result-match-fn-wrapper",
        &result_match,
    );
    assert!(
        result_match_diagnostics.is_empty(),
        "a Result match payload must preserve its known-Pure callback origin:\n{result_match_diagnostics}"
    );
    let impure_result_match =
        result_match.replace("callback: quiet", "callback: loud");
    assert!(
        check_diagnostics(
            "parmap-impure-result-match-fn-wrapper",
            &impure_result_match,
        )
        .contains("has an observable side effect"),
        "a Result match payload must preserve its Impure callback origin"
    );
}

#[test]
fn nested_fn_signatures_compare_by_source_identity() {
    let src = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn main() -> i32 {
  make: fn() -> Holder<fn(i64) -> i64> := fn {
    holder := Holder { callback: quiet }
    holder
  }
  holder := make()
  return holder.callback(41) as i32
}
";
    assert!(
        !check_errs("nested-fn-signature-source-identity", src),
        "a function signature must compare nested generic aggregates by source identity"
    );
    if backend_available() {
        assert_eq!(
            build_and_run("nested-fn-signature-source-identity", src)
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn generic_calls_join_source_compatible_argument_effect_origins() {
    let src = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn choose<T>(first: T, second: T, take_second: bool) -> T {
  if take_second { return second }
  return first
}
fn worker(x: i64) -> i64 {
  holder := choose(
    Holder { callback: quiet },
    Holder { callback: loud },
    x > 0,
  )
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_diagnostics("parmap-generic-call-joined-fn-wrapper", src)
            .contains("has an observable side effect"),
        "one generic monomorph must join every source-compatible actual argument, not keep the first callback origin"
    );
}

#[test]
fn loop_carried_callback_effects_reach_a_fixpoint() {
    let direct = "\
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  mut f := quiet
  mut i := 0
  loop {
    g := f
    if i == 1 { return g(x) }
    f = loud
    i = i + 1
  }
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_diagnostics("parmap-loop-carried-direct-callback", direct)
            .contains("has an observable side effect"),
        "a callback assigned at the loop tail must reach consumers on the next iteration"
    );

    let aggregate = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  mut holder := Holder { callback: quiet }
  mut i := 0
  loop {
    current := holder
    if i == 1 { return current.callback(x) }
    holder = Holder { callback: loud }
    i = i + 1
  }
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_diagnostics("parmap-loop-carried-aggregate-callback", aggregate)
            .contains("has an observable side effect"),
        "loop backedges must converge recursively through callback-bearing aggregates"
    );
}

#[test]
fn pure_callback_projections_preserve_exact_origins() {
    let field = "\
Envelope<T> { value: T }
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn worker(x: i64) -> i64 {
  envelope := Envelope { value: Holder { callback: quiet } }
  holder := envelope.value
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        !check_errs("parmap-pure-field-callback-projection", field),
        "a direct field projection must preserve its nested Pure callback origin"
    );

    let element_field = field.replace(
        "  envelope := Envelope { value: Holder { callback: quiet } }\n  holder := envelope.value",
        "  envelopes := [Envelope { value: Holder { callback: quiet } }]\n  holder := envelopes[0].value",
    );
    assert!(
        !check_errs(
            "parmap-pure-element-field-callback-projection",
            &element_field,
        ),
        "a struct-array element field must preserve its nested Pure callback origin"
    );

    let slice = "\
Holder<T> { callback: T }
Choice<T> { Some(T), None }
fn quiet(x: i64) -> i64 = x + 1
fn worker(x: i64) -> i64 {
  choices := [
    Choice.Some(Holder { callback: quiet }),
    Choice.Some(Holder { callback: quiet }),
  ]
  view := choices[0..2]
  choice := view[x % 2]
  return match choice {
    Some(holder) => holder.callback(x),
    None => x,
  }
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    let slice_diagnostics =
        check_diagnostics("parmap-pure-slice-callback-projection", slice);
    assert!(
        slice_diagnostics.is_empty(),
        "an identity slice view must preserve its array element callback origins:\n{slice_diagnostics}"
    );

    let map_err = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn keep_error(e: Error) -> Error = e
fn wrap<T>(value: T) -> Result<T, Error> = Ok(value)
fn worker(x: i64) -> i64 {
  mapped := wrap(Holder { callback: quiet }).map_err(keep_error)
  holder := mapped else { return x }
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        !check_errs("parmap-pure-map-err-ok-callback", map_err),
        "map_err must preserve the exact callback origin in its unchanged Ok payload"
    );

    let loop_value = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn worker(x: i64) -> i64 {
  holder := loop {
    if x < 0 { break Holder { callback: quiet } }
    break Holder { callback: quiet }
  }
  return holder.callback(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        !check_errs("parmap-pure-loop-break-callback", loop_value),
        "a value-producing loop must join only its own known-Pure break values"
    );

    let impure_loop_value = loop_value
        .replace(
            "fn quiet(x: i64) -> i64 = x + 1",
            "fn quiet(x: i64) -> i64 = x + 1\nfn loud(x: i64) -> i64 { print(x); return x }",
        )
        .replacen(
            "break Holder { callback: quiet }",
            "break Holder { callback: loud }",
            1,
        );
    assert!(
        check_diagnostics(
            "parmap-joined-loop-break-callback",
            &impure_loop_value,
        )
        .contains("has an observable side effect"),
        "a loop result must conservatively join every reachable break callback origin"
    );
}

#[test]
fn tuple_callback_aggregates_use_recursive_source_identity_and_effects() {
    let pure = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn make() -> (array<Holder<fn(i64) -> i64>>, i64) {
  holders := [Holder { callback: quiet }].to_array()
  return (holders, 41)
}
fn main() -> i32 {
  (holders, value) := make()
  return holders[0].callback(value) as i32
}
";
    assert!(
        !check_errs("tuple-callback-aggregate-source-identity", pure),
        "tuple source equality must recurse through callback-bearing array elements"
    );
    if backend_available() {
        assert_eq!(
            build_and_run("tuple-callback-aggregate-source-identity", pure)
                .status
                .code(),
            Some(42)
        );
    }

    let joined = "\
Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  (holders, ignored) := if x < 0 {
    ([Holder { callback: quiet }].to_array(), 0)
  } else {
    ([Holder { callback: loud }].to_array(), 0)
  }
  return holders[0].callback(x + ignored)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_diagnostics("parmap-joined-tuple-callback-aggregate", joined)
            .contains("has an observable side effect"),
        "tuple joins and destructuring must retain every nested callback effect origin"
    );

    let matched = "\
Holder<T> { callback: T }
Choice<T> { Some(T), None }
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  return match if x < 0 {
    Choice.Some(Holder { callback: quiet })
  } else {
    Choice.Some(Holder { callback: loud })
  } {
    Some(holder) => holder.callback(x),
    None => x,
  }
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_diagnostics("parmap-direct-match-binding-callback", matched)
            .contains("has an observable side effect"),
        "match payload bindings must join every direct scrutinee callback origin"
    );
    let pure_match = matched.replace(
        "Choice.Some(Holder { callback: loud })",
        "Choice.Some(Holder { callback: quiet })",
    );
    assert!(
        !check_errs("parmap-pure-direct-match-binding-callback", &pure_match),
        "matching direct known-Pure origins must remain Pure"
    );
}

#[test]
fn lifted_callback_captures_join_concrete_origins() {
    let direct = "\
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  f := quiet
  g := fn y: i64 {
    h := f
    h(y)
  }
  return g(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    let direct_diagnostics =
        check_diagnostics("parmap-pure-direct-fn-capture", direct);
    assert!(
        direct_diagnostics.is_empty(),
        "a lifted lambda must receive the known-Pure origin of a direct function capture:\n{direct_diagnostics}"
    );
    let impure_direct = direct.replace("f := quiet", "f := loud");
    assert!(
        check_diagnostics("parmap-impure-direct-fn-capture", &impure_direct)
            .contains("has an observable side effect"),
        "an Impure direct function capture must remain Impure in the lifted lambda"
    );

    let pipeline = "\
fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 {
  f := quiet
  return [x].map(fn y { h := f; h(y) }).sum()
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    let pipeline_diagnostics =
        check_diagnostics("parmap-pure-pipeline-fn-capture", pipeline);
    assert!(
        pipeline_diagnostics.is_empty(),
        "a pipeline's lifted function must receive a known-Pure direct capture:\n{pipeline_diagnostics}"
    );
    let impure_pipeline = pipeline.replace("f := quiet", "f := loud");
    assert!(
        check_diagnostics(
            "parmap-impure-pipeline-fn-capture",
            &impure_pipeline,
        )
        .contains("has an observable side effect"),
        "a pipeline's lifted function must retain an Impure direct capture"
    );
}

#[test]
fn extern_fn_type_effect_is_impure_through_indirection() {
    let src = "\
extern \"C\" fn abs(x: i32) -> i32
fn worker(x: i32) -> i32 {
  f := abs
  return f(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_errs("parmap-extern-fn-type-effect", src),
        "an FFI pointer must carry Impure through its function type"
    );
}

#[test]
fn map_err_consumes_function_type_effect() {
    let src = "\
fn noisy(e: Error) -> Error {
  print(1)
  return e
}
fn worker(x: i64) -> i64 {
  r: Result<i64, Error> := Ok(x)
  mapped := r.map_err(noisy)
  return x
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_errs("parmap-map-err-fn-type-effect", src),
        "a function-valued consumer other than CallFnValue must also read the FnTy effect"
    );
}

#[test]
fn unknown_higher_order_effect_rejected_in_par_map() {
    let src = "\
fn apply(f: fn(i64) -> i64, x: i64) -> i64 = f(x)
pub fn run(f: fn(i64) -> i64) -> i64 {
  ys := [1, 2, 3].par_map(fn x { apply(f, x) })
  return ys.sum()
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics("parmap-hof-unknown-effect", src);
    assert!(
        diagnostics.contains("calls a function value whose effect is not statically known"),
        "a higher-order parameter with no concrete target must remain fail-closed at par_map:\n{diagnostics}"
    );
}

#[test]
fn pure_higher_order_call_remains_legal_sequentially() {
    if !backend_available() {
        return;
    }
    let src = "\
fn inc(x: i64) -> i64 = x + 1
fn apply(f: fn(i64) -> i64, x: i64) -> i64 = f(x)
fn main() -> i32 {
  return apply(inc, 4) as i32
}
";
    assert!(!check_errs("sequential-hof-unknown-effect", src), "sequential higher-order calls remain legal");
    assert_eq!(build_and_run("sequential-hof-unknown-effect", src).status.code(), Some(5));
}

// --- NEW-3: the MoveCheck false positive — the same move value consumed in mutually-exclusive
//            match arms must be accepted (arms now clone+join like if/else). ---
#[test]
fn same_move_value_in_exclusive_match_arms_is_accepted() {
    if !backend_available() {
        return;
    }
    let src = "\
Tag { A, B }
fn main() -> i32 {
  arena {
    v := Tag.A
    b := heap.new(5)
    r := match v {
      A => { c := b
             c.get() }
      B => { d := b
             d.get() }
    }
    print(r)
  }
  return 0
}
";
    assert!(!check_errs("match-move-join", src), "moving the same value in exclusive match arms must be accepted");
    let out = build_and_run("match-move-join", src);
    assert_eq!(out.status.code(), Some(0));
}

// --- gemini #270 review: a `task_group {}` opens a region (like `arena {}`), so a task/box value
//     must not escape it (region_of / slice_is_local gained the `TaskGroup` block-wrapping arms). ---
#[test]
fn task_group_value_cannot_escape() {
    let src = "\
fn main() -> i32 {
  t := task_group {
    a := spawn(fn { 5 })
    wait()
    a
  }
  return 0
}
";
    assert!(check_errs("task-group-escape", src), "a task value must not escape its task_group");
}

#[test]
fn lambda_capturing_arena_view_cannot_escape() {
    let src = "\
fn main() -> i32 {
  f := arena {
    n := 5
    v := template \"hello {n}\"
    fn { v.len() as i32 }
  }
  return f()
}
";
    assert!(check_errs("lambda-capture-escape", src), "a lambda capturing an arena view must not escape the arena");
}
