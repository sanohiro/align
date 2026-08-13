//! M15 S1a gate tests: the interface summary's determinism, the interface/impl hash split, canonical
//! round-trip (+ fail-closed versioning), `out[i]` markers, and per-unit capability capture.

use std::collections::HashMap;

use align_interface::{
    DecodeError, Effect, FORMAT_VERSION, Hash128, IParam, IType, ITypeParam,
    ImportCompatibilityError,
    InterfaceSummary, ParamMode, ReturnBorrowSummary, ReturnRegionSummary, build_summaries,
    deserialize, encode_interface_surface, serialize, summary_to_source,
    validate_for_import,
};

/// One in-memory source module for a test program.
struct Unit {
    path: &'static str,
    is_entry: bool,
    src: String,
}

fn unit(path: &'static str, is_entry: bool, src: impl Into<String>) -> Unit {
    Unit { path, is_entry, src: src.into() }
}

fn render_test_type(ty: &IType) -> String {
    match ty {
        IType::Named { path, args } => {
            if args.is_empty() {
                path.clone()
            } else {
                format!(
                    "{path}<{}>",
                    args.iter()
                        .map(render_test_type)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        IType::Tuple(elements) => format!(
            "({})",
            elements
                .iter()
                .map(render_test_type)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        IType::Fn { params, ret, .. } => {
            let params = params
                .iter()
                .map(|param| {
                    let mode = match param.mode {
                        ParamMode::ByValue => "",
                        ParamMode::Out => "out ",
                        ParamMode::Borrow => "borrow ",
                        ParamMode::BorrowMut => "borrow mut ",
                    };
                    format!("{mode}{}", render_test_type(&param.ty))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("fn({params}) -> {}", render_test_type(ret))
        }
    }
}

fn render_test_type_params(params: &[ITypeParam]) -> String {
    let params = params
        .iter()
        .map(|param| match &param.bound {
            Some(bound) => format!("{}: {bound}", param.name),
            None => param.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{params}>")
}

fn sync_generic_type_bodies(summary: &mut InterfaceSummary) {
    for structure in &mut summary.structs {
        if structure.type_params.is_empty() {
            continue;
        }
        let mut body = format!(
            "{}{} {{\n",
            structure.name,
            render_test_type_params(&structure.type_params)
        );
        for (name, ty) in &structure.fields {
            body.push_str(&format!("  {name}: {},\n", render_test_type(ty)));
        }
        body.push('}');
        structure.generic_body = Some(body);
    }
    for enumeration in &mut summary.enums {
        if enumeration.type_params.is_empty() {
            continue;
        }
        let mut body = format!(
            "{}{} {{\n",
            enumeration.name,
            render_test_type_params(&enumeration.type_params)
        );
        for (name, payload) in &enumeration.variants {
            body.push_str(&format!("  {name}"));
            if !payload.is_empty() {
                body.push('(');
                body.push_str(
                    &payload
                        .iter()
                        .map(render_test_type)
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                body.push(')');
            }
            body.push_str(",\n");
        }
        body.push('}');
        enumeration.generic_body = Some(body);
    }
}

/// Parse + check + lower the given units and build their interface summaries. Asserts the program
/// type-checks (a summary of an ill-typed program is meaningless). Fresh data structures every call,
/// so building twice exercises determinism against any internal HashMap iteration order.
fn summaries(units: &[Unit]) -> Vec<InterfaceSummary> {
    let mut diags = align_diag::Diagnostics::new();
    let asts: Vec<align_ast::File> = units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let toks = align_lexer::tokenize(i as u32, &u.src, &mut diags);
            align_parser::parse_file(toks, &mut diags)
        })
        .collect();
    let modules: Vec<align_sema::Module> = units
        .iter()
        .zip(&asts)
        .map(|(u, ast)| align_sema::Module { path: u.path.to_string(), file: ast, is_entry: u.is_entry, interface_only: false })
        .collect();
    let hir = align_sema::check_program(&modules, &mut diags);
    let messages: Vec<&str> =
        diags.iter().map(|diagnostic| diagnostic.message.as_str()).collect();
    assert!(
        !diags.has_errors(),
        "program should type-check: {messages:?}"
    );
    let mir = align_mir::lower_program(&hir);
    let sources: HashMap<String, String> =
        units.iter().map(|u| (u.path.to_string(), u.src.clone())).collect();
    build_summaries(&modules, &hir, &mir, &sources)
}

/// A single-entry-module program.
fn one(src: impl Into<String>) -> Vec<InterfaceSummary> {
    summaries(&[unit("main", true, src)])
}

fn find<'a>(sums: &'a [InterfaceSummary], unit: &str) -> &'a InterfaceSummary {
    sums.iter().find(|s| s.unit == unit).unwrap_or_else(|| panic!("no unit `{unit}`"))
}

fn rehash(summary: &mut InterfaceSummary) {
    summary.interface_hash = Hash128::of(&encode_interface_surface(summary));
}

fn resource_summaries() -> Vec<InterfaceSummary> {
    summaries(&[
        unit(
            "pkg.db.internal.resource",
            false,
            "module pkg.db.internal.resource\npub fn drop_conn(handle: raw) { unsafe { raw.free(handle) } }\n",
        ),
        unit(
            "pkg.db",
            false,
            "module pkg.db\nimport pkg.db.internal.resource\npub resource conn = pkg.db.internal.resource.drop_conn\npub resource stmt<T> = pkg.db.internal.resource.drop_conn\n",
        ),
        unit("main", true, "module main\nimport pkg.db\nfn main() -> i32 = 0\n"),
    ])
}

// ---- 1. determinism -----------------------------------------------------------------------------

#[test]
fn determinism_same_source_twice_is_byte_identical() {
    let src = "pub fn add(a: i64, b: i64) -> i64 = a + b\n\
               pub fn shout(s: str) { print(s) }\n\
               pub Point { x: i64, y: i64 }\n\
               pub MAX: i64 := 100\n\
               fn main() -> i32 = 0\n";
    let a = one(src);
    let b = one(src);
    assert_eq!(a, b, "summaries must be equal across builds");
    assert_eq!(serialize(&a[0]), serialize(&b[0]), "serialization must be byte-identical");
}

#[test]
fn interface_is_independent_of_pub_fn_declaration_order() {
    // Reordering two unrelated `pub` fns is NOT an interface change (the exported set is order-free).
    let ab = one("pub fn a() -> i64 = 1\npub fn b() -> i64 = 2\nfn main() -> i32 = 0\n");
    let ba = one("pub fn b() -> i64 = 2\npub fn a() -> i64 = 1\nfn main() -> i32 = 0\n");
    assert_eq!(
        find(&ab, "main").interface_hash,
        find(&ba, "main").interface_hash,
        "reordering pub fns must not change the interface hash"
    );
    // The exported fns come out name-sorted (canonicalization pin).
    let names: Vec<&str> = find(&ab, "main").fns.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b"]);
}

// ---- 2. hash-split semantics (the headline) -----------------------------------------------------

/// A two-module program: entry `main` imports `lib` and calls only its STABLE fn, so edits to `lib`'s
/// other items never break `main`'s check. `{TARGET}` / `{GEN}` / `{SECRET}` are the edit points.
fn two_module(target: &str, generic: &str, secret: &str) -> Vec<InterfaceSummary> {
    let main = "import lib\nfn main() -> i32 {\n  x := lib.stable()\n  return x as i32\n}\n";
    let lib = format!(
        "module lib\n\
         pub fn stable() -> i64 = 7\n\
         pub fn target(a: i64, b: i64) -> i64 = {target}\n\
         pub fn pick<T>(a: T, b: T) -> T = {generic}\n\
         fn secret(x: i64) -> i64 = {secret}\n"
    );
    summaries(&[unit("main", true, main), unit("lib", false, lib)])
}

#[test]
fn split_a_private_body_edit_keeps_interface_changes_impl() {
    let v1 = two_module("a + b", "a", "x * 2");
    let v2 = two_module("a + b + 0", "a", "x * 2"); // target body differs, same sig + effect
    let l1 = find(&v1, "lib");
    let l2 = find(&v2, "lib");
    assert_eq!(l1.interface_hash, l2.interface_hash, "body edit must NOT change interface hash");
    assert_ne!(l1.impl_hash, l2.impl_hash, "body edit MUST change impl hash");
    // Headline win: the dependent unit's interface is entirely untouched.
    assert_eq!(find(&v1, "main").interface_hash, find(&v2, "main").interface_hash);
}

#[test]
fn split_b_signature_edit_changes_interface() {
    let v1 = two_module("a + b", "a", "x * 2");
    // Change `target`'s signature (a param type).
    let main = "import lib\nfn main() -> i32 {\n  x := lib.stable()\n  return x as i32\n}\n";
    let lib = "module lib\n\
               pub fn stable() -> i64 = 7\n\
               pub fn target(a: i32, b: i64) -> i64 = a as i64 + b\n\
               pub fn pick<T>(a: T, b: T) -> T = a\n\
               fn secret(x: i64) -> i64 = x * 2\n";
    let v2 = summaries(&[unit("main", true, main), unit("lib", false, lib)]);
    assert_ne!(
        find(&v1, "lib").interface_hash,
        find(&v2, "lib").interface_hash,
        "a signature change must change the interface hash"
    );
}

#[test]
fn split_c_effect_flip_pure_to_impure_changes_interface() {
    // The most-likely-to-get-wrong case: adding a side effect to a pub fn's body flips its effect
    // bit, which lives IN the interface.
    let v1 = two_module("a + b", "a", "x * 2");
    let main = "import lib\nfn main() -> i32 {\n  x := lib.stable()\n  return x as i32\n}\n";
    let lib = "module lib\n\
               pub fn stable() -> i64 = 7\n\
               pub fn target(a: i64, b: i64) -> i64 {\n  print(a)\n  return a + b\n}\n\
               pub fn pick<T>(a: T, b: T) -> T = a\n\
               fn secret(x: i64) -> i64 = x * 2\n";
    let v2 = summaries(&[unit("main", true, main), unit("lib", false, lib)]);
    // Sanity: the effect bit really flipped.
    let e1 = find(&v1, "lib").fns.iter().find(|f| f.name == "target").unwrap().effect;
    let e2 = find(&v2, "lib").fns.iter().find(|f| f.name == "target").unwrap().effect;
    assert_eq!(e1, align_interface::Effect::Pure);
    assert_eq!(e2, align_interface::Effect::Impure);
    assert_ne!(
        find(&v1, "lib").interface_hash,
        find(&v2, "lib").interface_hash,
        "a Pure->Impure effect flip must change the interface hash"
    );
}

#[test]
fn exportable_callback_parameters_remain_open_world_in_effect_summaries() {
    let main = "import lib\nfn main() -> i32 = 0\n";
    let lib_src = "\
module lib
pub Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
pub fn apply(callback: fn(i64) -> i64, x: i64) -> i64 {
  return callback(x)
}
fn helper(
  holder: Holder<fn(i64) -> i64>,
  x: i64,
) -> i64 {
  return holder.callback(x)
}
pub fn apply_holder(
  holder: Holder<fn(i64) -> i64>,
  x: i64,
) -> i64 {
  return helper(holder, x)
}
pub fn fixed(x: i64) -> i64 {
  return helper(Holder { callback: quiet }, x)
}
fn seed_direct() -> i64 = apply(quiet, 1)
fn seed_holder() -> i64 {
  return apply_holder(Holder { callback: quiet }, 1)
}
";
    let sums = summaries(&[
        unit("main", true, main),
        unit("lib", false, lib_src),
    ]);
    let lib_summary = find(&sums, "lib");
    for name in ["apply", "apply_holder"] {
        assert_eq!(
            lib_summary
                .fns
                .iter()
                .find(|function| function.name == name)
                .unwrap_or_else(|| panic!("missing exported function `{name}`"))
                .effect,
            Effect::Unknown,
            "an exportable callback-bearing parameter must stay open-world even when provider-local calls pass only Pure values"
        );
    }
    assert_eq!(
        lib_summary
            .fns
            .iter()
            .find(|function| function.name == "fixed")
            .expect("missing exported function `fixed`")
            .effect,
        Effect::Pure,
        "one open-world export must not contaminate an unrelated export that reaches the same private helper with a fixed Pure callback"
    );

    let mut external_effects = HashMap::new();
    external_effects.insert(
        "lib$apply".to_string(),
        align_sema::FnEffect::Unknown,
    );
    let consumer = "\
import lib
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn worker(x: i64) -> i64 = lib.apply(loud, x)
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    let mut diags = align_diag::Diagnostics::new();
    let lib_tokens = align_lexer::tokenize(0, lib_src, &mut diags);
    let lib_file = align_parser::parse_file(lib_tokens, &mut diags);
    let consumer_tokens =
        align_lexer::tokenize(1, consumer, &mut diags);
    let consumer_file =
        align_parser::parse_file(consumer_tokens, &mut diags);
    assert!(
        !diags.has_errors(),
        "provider and consumer fixtures must parse before effect checking"
    );
    let modules = [
        align_sema::Module {
            path: "lib".to_string(),
            file: &lib_file,
            is_entry: false,
            interface_only: true,
        },
        align_sema::Module {
            path: "main".to_string(),
            file: &consumer_file,
            is_entry: true,
            interface_only: false,
        },
    ];
    align_sema::check_program_with_effects(
        &modules,
        &external_effects,
        &mut diags,
    );
    assert!(
        diags.has_errors(),
        "a dependent must reject an Impure callback passed through the provider's Unknown effect summary under par_map"
    );
}

#[test]
fn exportable_internal_parallel_boundaries_are_open_world() {
    let main = "import lib\nfn main() -> i32 = 0\n";
    let provider = "\
module lib
pub Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn apply(holder: Holder<fn(i64) -> i64>) -> i64 {
  return holder.callback(1)
}
pub fn run(
  holders: array<Holder<fn(i64) -> i64>>,
) -> Result<(), Error> {
  ys := holders.par_map(apply)
  return Ok(())
}
fn provider_use() -> Result<(), Error> {
  holders: array<Holder<fn(i64) -> i64>> :=
    [Holder { callback: quiet }].to_array()
  return run(holders)
}
";

    let check = |provider: &str| {
        let mut diags = align_diag::Diagnostics::new();
        let main_tokens =
            align_lexer::tokenize(0, main, &mut diags);
        let main_file =
            align_parser::parse_file(main_tokens, &mut diags);
        let provider_tokens =
            align_lexer::tokenize(1, provider, &mut diags);
        let provider_file =
            align_parser::parse_file(provider_tokens, &mut diags);
        assert!(
            !diags.has_errors(),
            "open-world parallel fixtures must parse before checking"
        );
        let modules = [
            align_sema::Module {
                path: "main".to_string(),
                file: &main_file,
                is_entry: true,
                interface_only: false,
            },
            align_sema::Module {
                path: "lib".to_string(),
                file: &provider_file,
                is_entry: false,
                interface_only: false,
            },
        ];
        align_sema::check_program(&modules, &mut diags);
        diags.has_errors()
    };

    assert!(
        check(provider),
        "an exportable body must reject an internal par_map whose callback origin can come from an unseen caller"
    );
    let private = provider.replace("pub fn run(", "fn run(");
    assert!(
        !check(&private),
        "the same private closed-world body must retain provider-local Pure precision"
    );
}

#[test]
fn exportable_callback_dispatch_cannot_hide_a_parallel_boundary() {
    let main = "import lib\nfn main() -> i32 = 0\n";
    let provider = "\
module lib
pub Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn apply(holder: Holder<fn(i64) -> i64>) -> i64 {
  return holder.callback(1)
}
fn helper(
  holders: array<Holder<fn(i64) -> i64>>,
) -> Result<(), Error> {
  ys := holders.par_map(apply)
  return Ok(())
}
fn seed() -> Result<(), Error> {
  holders: array<Holder<fn(i64) -> i64>> :=
    [Holder { callback: quiet }].to_array()
  return helper(holders)
}
pub fn run(
  holders: array<Holder<fn(i64) -> i64>>,
) -> Result<(), Error> {
  indirect := helper
  return indirect(holders)
}
";

    let check = |provider: &str| {
        let mut diags = align_diag::Diagnostics::new();
        let main_tokens =
            align_lexer::tokenize(0, main, &mut diags);
        let main_file =
            align_parser::parse_file(main_tokens, &mut diags);
        let provider_tokens =
            align_lexer::tokenize(1, provider, &mut diags);
        let provider_file =
            align_parser::parse_file(provider_tokens, &mut diags);
        assert!(
            !diags.has_errors(),
            "open-world indirect-dispatch fixtures must parse before checking"
        );
        let modules = [
            align_sema::Module {
                path: "main".to_string(),
                file: &main_file,
                is_entry: true,
                interface_only: false,
            },
            align_sema::Module {
                path: "lib".to_string(),
                file: &provider_file,
                is_entry: false,
                interface_only: false,
            },
        ];
        align_sema::check_program(&modules, &mut diags);
        diags
            .iter()
            .filter(|diagnostic| {
                diagnostic.severity == align_diag::Severity::Error
            })
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>()
    };
    assert!(
        !check(provider).is_empty(),
        "an exported callback-bearing root must not hide a possible internal parallel boundary behind unresolved dispatch"
    );
    let private = provider.replace("pub fn run(", "fn run(");
    let private_diagnostics = check(&private);
    assert!(
        private_diagnostics.is_empty(),
        "the same unresolved dispatch remains legal in a private closed-world function when every parallel target is known Pure: {private_diagnostics:?}"
    );

    let captured = "\
module lib
pub Holder<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn apply(holder: Holder<fn(i64) -> i64>) -> i64 {
  return holder.callback(1)
}
pub fn run(holder: Holder<fn(i64) -> i64>) -> i64 {
  dispatch := fn {
    print(0)
    holders: array<Holder<fn(i64) -> i64>> :=
      [holder].to_array()
    ys := holders.par_map(apply)
    ys.sum()
  }
  return dispatch()
}
fn provider_use() -> i64 {
  return run(Holder { callback: quiet })
}
";
    assert!(
        !check(captured).is_empty(),
        "a zero-argument indirect closure must not hide an internal parallel boundary affected by an open-world capture"
    );
    let private_captured =
        captured.replace("pub fn run(", "fn run(");
    let private_captured_diagnostics = check(&private_captured);
    assert!(
        private_captured_diagnostics.is_empty(),
        "the same captured closure remains precise for a private closed-world caller: {private_captured_diagnostics:?}"
    );

    let map_err_provider = "\
module lib
pub Wrap<T> { callback: T }
fn quiet(x: i64) -> i64 = x + 1
fn apply(wrap: Wrap<fn(i64) -> i64>) -> i64 {
  return wrap.callback(1)
}
fn convert(wrap: Wrap<fn(i64) -> i64>) -> Error {
  wraps: array<Wrap<fn(i64) -> i64>> :=
    [wrap].to_array()
  ys := wraps.par_map(apply)
  return Error.Invalid
}
pub fn run(
  result: Result<i64, Wrap<fn(i64) -> i64>>,
) -> Result<i64, Error> {
  return result.map_err(convert)
}
fn provider_use() -> Result<i64, Error> {
  result: Result<i64, Wrap<fn(i64) -> i64>> :=
    Err(Wrap { callback: quiet })
  return run(result)
}
";
    assert!(
        !check(map_err_provider).is_empty(),
        "a static map_err converter must be in named reachability when it contains an internal parallel boundary"
    );
    let private_map_err =
        map_err_provider.replace("pub fn run(", "fn run(");
    let private_map_err_diagnostics = check(&private_map_err);
    assert!(
        private_map_err_diagnostics.is_empty(),
        "the same map_err converter remains precise for a private closed-world caller: {private_map_err_diagnostics:?}"
    );
}

#[test]
fn split_d_generic_template_body_edit_changes_interface() {
    // A generic pub fn's body is part of its interface (C++-template-like).
    let v1 = two_module("a + b", "a", "x * 2"); // pick<T> body = `a`
    let v2 = two_module("a + b", "b", "x * 2"); // pick<T> body = `b` (still type-correct)
    assert_ne!(
        find(&v1, "lib").interface_hash,
        find(&v2, "lib").interface_hash,
        "editing a generic template body must change the interface hash"
    );
}

#[test]
fn split_e_private_fn_edit_does_not_change_interface() {
    let v1 = two_module("a + b", "a", "x * 2");
    let v2 = two_module("a + b", "a", "x * 3"); // only `secret` (private) differs
    assert_eq!(
        find(&v1, "lib").interface_hash,
        find(&v2, "lib").interface_hash,
        "editing a private fn must NOT change the interface hash"
    );
    assert_ne!(find(&v1, "lib").impl_hash, find(&v2, "lib").impl_hash, "impl hash still changes");
}

// ---- 2b. aggregate constants (S1) ---------------------------------------------------------------

#[test]
fn an_aggregate_constant_exports_its_literal_source() {
    // A `pub` aggregate constant is part of the exported surface; its value is carried as the array
    // literal's source text (consumers rematerialize the slice against their own per-unit rodata).
    let sums = one("pub TABLE: slice<i64> := [1, 2, 3]\nfn main() -> i32 = 0\n");
    let c = &find(&sums, "main").consts[0];
    assert_eq!(c.name, "TABLE");
    assert_eq!(c.value_src, "[1, 2, 3]");
}

#[test]
fn editing_an_aggregate_constant_value_changes_the_interface_hash() {
    // The initializer source is folded into the interface hash (`IConst.value_src`), so changing an
    // element invalidates every dependent unit — the M15 cross-unit cache gate, no FORMAT_VERSION bump.
    let v1 = one("pub TABLE := [1, 2, 3]\nfn main() -> i32 = 0\n");
    let v2 = one("pub TABLE := [1, 2, 4]\nfn main() -> i32 = 0\n");
    assert_ne!(
        find(&v1, "main").interface_hash,
        find(&v2, "main").interface_hash,
        "editing an aggregate constant's value must change the interface hash"
    );
}

// ---- 3. round-trip + fail-closed version --------------------------------------------------------

#[test]
fn round_trip_equality() {
    let sums = one("pub fn add(a: i64, b: i64) -> i64 = a + b\n\
                    pub fn shout(s: str) { print(s) }\n\
                    pub fn pick<T>(a: T, b: T) -> T = a\n\
                    pub Point { x: i64, y: i64 }\n\
                    pub layout(C) align(16) Wide { a: i64, b: i64 }\n\
                    pub Color { Red, Green, Blue }\n\
                    pub MAX: i64 := 100\n\
                    fn main() -> i32 = 0\n");
    let s = &sums[0];
    let bytes = serialize(s);
    let back = deserialize(&bytes).expect("round-trip should succeed");
    assert_eq!(*s, back);
}

#[test]
fn resource_metadata_round_trips_and_rejects_each_corruption_class() {
    let sums = resource_summaries();
    let summary = find(&sums, "pkg.db");
    assert_eq!(summary.resources.len(), 2);
    assert_eq!(summary.resources[0].name, "conn");
    assert_eq!(summary.resources[0].generic_arity, 0);
    assert_eq!(summary.resources[0].representation_version, 1);
    assert_eq!(
        summary.resources[0].drop_thunk,
        "__align_resource_drop$pkg.db$conn"
    );
    assert_eq!(summary.resources[0].drop_abi_fingerprint, *b"align-res-drop-1");
    assert_eq!(summary.resources[1].name, "stmt");
    assert_eq!(summary.resources[1].generic_arity, 1);
    assert_eq!(deserialize(&serialize(summary)).unwrap(), *summary);
    let source = summary_to_source(summary, &[]).unwrap();
    assert!(source.contains("pub resource conn = pkg.db.internal.resource.__align_interface_drop_conn"));

    let rejected = |mut candidate: InterfaceSummary, expected| {
        rehash(&mut candidate);
        assert_eq!(validate_for_import(&candidate), Err(expected));
    };

    let mut arity = summary.clone();
    arity.resources[0].generic_arity = 1;
    rejected(
        arity,
        ImportCompatibilityError::ResourceArityMismatch("conn".into()),
    );

    let mut version = summary.clone();
    version.resources[0].representation_version = 2;
    rejected(
        version,
        ImportCompatibilityError::ResourceRepresentationVersion {
            name: "conn".into(),
            version: 2,
        },
    );

    let mut thunk = summary.clone();
    thunk.resources[0].drop_thunk = "redirected".into();
    rejected(
        thunk,
        ImportCompatibilityError::ResourceDropThunk("conn".into()),
    );

    let mut fingerprint = summary.clone();
    fingerprint.resources[0].drop_abi_fingerprint = [0; 16];
    rejected(
        fingerprint,
        ImportCompatibilityError::ResourceDropAbi("conn".into()),
    );

    let mut duplicate = summary.clone();
    duplicate.resources.push(duplicate.resources[0].clone());
    rejected(
        duplicate,
        ImportCompatibilityError::DuplicateLocalType("conn".into()),
    );
}

#[test]
fn summary_source_deduplicates_builtin_and_dependency_imports() {
    let sums = summaries(&[
        unit(
            "lib",
            false,
            "module lib\n\
             import std.crypto\n\
             import std.regex\n\
             pub fn builtin_value(p: crypto.argon2_params, m: regex.regex_match) -> i64 = p.parallelism + m.end\n",
        ),
        unit("main", true, "module main\nimport lib\nfn main() -> i32 = 0\n"),
    ]);
    let summary = find(&sums, "lib");
    let rendered = summary_to_source(summary, &["std.regex", "std.crypto"]).unwrap();
    assert_eq!(
        rendered.lines().filter(|line| *line == "import std.crypto").count(),
        1,
        "builtin capability import must not be repeated when it is also a dependency"
    );
    assert_eq!(
        rendered.lines().filter(|line| *line == "import std.regex").count(),
        1,
        "builtin capability import must not be repeated when it is also a dependency"
    );
}

#[test]
fn deserialize_unknown_version_fails_closed() {
    let sums = one("pub fn f() -> i64 = 1\nfn main() -> i32 = 0\n");
    let mut bytes = serialize(&sums[0]);
    // Corrupt the leading format-version u32.
    bytes[0] = bytes[0].wrapping_add(7);
    match deserialize(&bytes) {
        Err(DecodeError::UnknownVersion(v)) => assert_ne!(v, FORMAT_VERSION),
        other => panic!("expected UnknownVersion, got {other:?}"),
    }
}

#[test]
fn deserialize_truncated_and_trailing_fail_closed() {
    let sums = one("pub fn f() -> i64 = 1\nfn main() -> i32 = 0\n");
    let bytes = serialize(&sums[0]);
    // Truncated.
    assert_eq!(deserialize(&bytes[..bytes.len() - 3]), Err(DecodeError::Truncated));
    // Trailing bytes.
    let mut extra = bytes.clone();
    extra.push(0);
    assert_eq!(deserialize(&extra), Err(DecodeError::TrailingBytes));
}

#[test]
fn deserialize_stale_effect_bit_fails_closed() {
    let sums = one("pub fn noisy(x: i64) -> i64 {\n  print(x)\n  return x\n}\nfn main() -> i32 = 0\n");
    let mut stale = sums[0].clone();
    let noisy = stale.fns.iter_mut().find(|f| f.name == "noisy").expect("pub fn present");
    assert_eq!(noisy.effect, Effect::Impure);

    // Simulate a stale/tampered artifact whose effect byte says Pure while its stored surface hash
    // still describes the original Impure interface. A consumer must reject it before effect seeding.
    noisy.effect = Effect::Pure;
    let bytes = serialize(&stale);
    assert_eq!(deserialize(&bytes), Err(DecodeError::InterfaceHashMismatch));
}

// ---- 4. out[i] markers --------------------------------------------------------------------------

#[test]
fn out_param_marker_is_recorded() {
    let sums = one("pub fn put(out dst: slice<i64>, k: i64) {\n  dst[k] = 42\n}\nfn main() -> i32 = 0\n");
    let put = find(&sums, "main").fns.iter().find(|f| f.name == "put").unwrap();
    assert_eq!(put.params.len(), 2);
    assert_eq!(put.params[0].mode, ParamMode::Out);
    assert_eq!(put.params[1].mode, ParamMode::ByValue);
    assert_eq!(put.return_borrow, ReturnBorrowSummary::None);
    assert_eq!(put.return_region, ReturnRegionSummary::None);
    // And the out-param's type survived.
    assert!(matches!(&put.params[0].ty, IType::Named { path, .. } if path == "slice"));
}

#[test]
fn parameter_mode_and_return_summaries_have_canonical_codec_identity() {
    let base = one(
        "pub fn inspect(first: slice<i64>, second: slice<i64>) -> slice<i64> = first\nfn main() -> i32 = 0\n",
    );
    assert_eq!(
        base[0].fns[0].return_borrow,
        ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: vec![],
        },
        "the producer records the selected parameter root"
    );
    let mut mode = base[0].clone();
    mode.fns[0].params[0].mode = ParamMode::Out;
    rehash(&mut mode);
    assert_ne!(base[0].interface_hash, mode.interface_hash, "parameter mode is hash identity");

    let mut roots = base[0].clone();
    roots.fns[0].return_borrow = ReturnBorrowSummary::Roots {
        params: vec![1],
        captures: vec![],
    };
    roots.fns[0].return_region = ReturnRegionSummary::Roots {
        params: vec![1],
        captures: vec![],
    };
    rehash(&mut roots);
    assert_ne!(base[0].interface_hash, roots.interface_hash, "return roots are hash identity");
    assert_ne!(mode.interface_hash, roots.interface_hash, "mode and summary encode independently");
    let decoded = deserialize(&serialize(&roots)).expect("known canonical summary tag round-trips");
    assert_eq!(decoded, roots);
    assert_eq!(
        validate_for_import(&decoded),
        Ok(()),
        "L2b consumes canonical return summaries"
    );
}

#[test]
fn return_cleanup_metadata_is_exact_for_functions_and_nested_function_values() {
    let mut summary = one(
        "pub Route { owned: fn() -> string, copied: fn() -> i64 }\n\
         pub fn owned() -> string = \"owned\".clone()\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    let owned = summary.fns.iter().find(|function| function.name == "owned").unwrap();
    assert_eq!(
        owned.return_cleanup,
        align_sema::hir::ReturnCleanupAbi::DynamicBit
    );
    let fields = &summary.structs.iter().find(|definition| definition.name == "Route").unwrap().fields;
    assert!(matches!(
        &fields[0].1,
        IType::Fn {
            return_cleanup: align_sema::hir::ReturnCleanupAbi::DynamicBit,
            ..
        }
    ));
    assert!(matches!(
        &fields[1].1,
        IType::Fn {
            return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
            ..
        }
    ));
    assert_eq!(validate_for_import(&summary), Ok(()));

    summary.fns.iter_mut().find(|function| function.name == "owned").unwrap().return_cleanup =
        align_sema::hir::ReturnCleanupAbi::None;
    assert_eq!(
        validate_for_import(&summary),
        Err(ImportCompatibilityError::ReturnCleanupMismatch)
    );

    let mut nested = one(
        "pub Route { owned: fn() -> string, copied: fn() -> i64 }\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    let IType::Fn { return_cleanup, .. } = &mut nested.structs[0].fields[1].1 else {
        panic!("expected copied function field");
    };
    *return_cleanup = align_sema::hir::ReturnCleanupAbi::DynamicBit;
    assert_eq!(
        validate_for_import(&nested),
        Err(ImportCompatibilityError::ReturnCleanupMismatch)
    );
}

/// Import validation compares a recorded `ReturnCleanupAbi` against a re-derivation, so the two
/// must be one rule. The re-derivation used to be a hand-written table of droppable builtin
/// spellings here; it is now `align_sema`'s ownership authority reached through its spelling
/// bridge. Pin the classification of every builtin spelling an interface can carry, so a bridge
/// that stops recognising one (or starts calling a borrow owned) fails here rather than rejecting
/// a valid interface as a return-cleanup mismatch.
#[test]
fn builtin_spelling_ownership_matches_the_producer_for_every_interface_spelling() {
    for owned in [
        "array",
        "array_builder",
        "string",
        "reader",
        "writer",
        "buffer",
        "file",
        "regex",
        "captures",
        "tcp_conn",
        "tcp_listener",
        "udp_socket",
        "child",
        "http_request_ctx",
        "response_builder",
        "http_stream",
    ] {
        assert_eq!(
            align_sema::builtin_spelling_needs_return_cleanup(owned),
            Some(true),
            "`{owned}` owns droppable storage, so a function returning it needs the cleanup bit"
        );
    }
    for plain in [
        "()", "bool", "char", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
        "str", "raw", "region", "rng", "slice", "http_headers", "json.doc",
        // `box<T>` / `Task<R>` are freed with their region, never by a per-return cleanup bit.
        "box",
    ] {
        assert_eq!(
            align_sema::builtin_spelling_needs_return_cleanup(plain),
            Some(false),
            "`{plain}` carries no return-cleanup obligation"
        );
    }
    // A user-defined or nominal-argument spelling is not the bridge's business; the analysis
    // answers those from its own definition index.
    for local in ["Route", "pkg.db.conn", "soa", "json.scanner", "Option", "Result"] {
        assert_eq!(
            align_sema::builtin_spelling_needs_return_cleanup(local),
            None,
            "`{local}` must be resolved by the importing analysis, not the builtin bridge"
        );
    }
}

#[test]
fn semantic_import_rejects_return_roots_incapable_of_borrowing() {
    let mut non_borrowing_return =
        one("pub fn inspect(value: str) -> i64 = value.len()\nfn main() -> i32 = 0\n").remove(0);
    non_borrowing_return.fns[0].return_borrow = ReturnBorrowSummary::Roots {
        params: vec![0],
        captures: vec![],
    };
    non_borrowing_return.fns[0].return_region = ReturnRegionSummary::Roots {
        params: vec![0],
        captures: vec![],
    };
    assert_eq!(
        validate_for_import(&non_borrowing_return),
        Err(align_interface::ImportCompatibilityError::ReturnSummaryOnNonBorrowingType)
    );

    let mut non_borrowing_root =
        one("pub fn inspect(value: i64) -> str = \"fixed\"\nfn main() -> i32 = 0\n").remove(0);
    non_borrowing_root.fns[0].return_region = ReturnRegionSummary::Roots {
        params: vec![0],
        captures: vec![],
    };
    non_borrowing_root.fns[0].return_borrow = ReturnBorrowSummary::Roots {
        params: vec![0],
        captures: vec![],
    };
    assert_eq!(
        validate_for_import(&non_borrowing_root),
        Err(align_interface::ImportCompatibilityError::ReturnSummaryRootCannotBorrow(0))
    );

    non_borrowing_root.fns[0].params[0].ty = IType::Named {
        path: "buffer".to_string(),
        args: vec![],
    };
    assert_eq!(
        validate_for_import(&non_borrowing_root),
        Err(align_interface::ImportCompatibilityError::ReturnSummaryRootCannotBorrow(0)),
        "owned builtin handles must not be mistaken for generic nominal types"
    );

    for builtin in [
        "Error",
        "core.Error",
        "argon2_params",
        "crypto.argon2_params",
        "regex_match",
        "regex.regex_match",
    ] {
        let mut builtin_root = non_borrowing_root.clone();
        builtin_root.fns[0].params[0].ty = IType::Named {
            path: builtin.to_string(),
            args: vec![],
        };
        assert_eq!(
            validate_for_import(&builtin_root),
            Err(ImportCompatibilityError::ReturnSummaryRootCannotBorrow(0)),
            "compiler-known non-borrowing builtin parameter `{builtin}` must reject provenance"
        );

        let mut builtin_return = non_borrowing_return.clone();
        builtin_return.fns[0].ret = IType::Named {
            path: builtin.to_string(),
            args: vec![],
        };
        assert_eq!(
            validate_for_import(&builtin_return),
            Err(ImportCompatibilityError::ReturnSummaryOnNonBorrowingType),
            "compiler-known non-borrowing builtin return `{builtin}` must reject provenance"
        );
    }
}

#[test]
fn semantic_import_rejects_disagreeing_l2b_a1_summaries() {
    let mut summary =
        one("pub fn inspect(value: str) -> str = value\nfn main() -> i32 = 0\n").remove(0);
    summary.fns[0].return_region = ReturnRegionSummary::None;
    assert_eq!(
        validate_for_import(&summary),
        Err(ImportCompatibilityError::ReturnSummaryDisagreement)
    );
}

#[test]
fn semantic_import_prefers_local_nominals_over_builtin_aliases() {
    for alias in ["Error", "argon2_params", "regex_match"] {
        let source = format!(
            "module alias\npub {alias} {{ view: str }}\npub fn project(value: {alias}) -> str = value.view\n"
        );
        let produced = summaries(&[
            unit("alias", false, source),
            unit("main", true, "module main\nfn main() -> i32 = 0\n"),
        ]);
        assert_eq!(
            validate_for_import(find(&produced, "alias")),
            Ok(()),
            "semantic import must analyze local `{alias}` rather than the non-borrowing builtin alias"
        );
    }
}

#[test]
fn semantic_import_does_not_resolve_foreign_qualified_nominals_as_local() {
    let mut summary = one(
        "pub Payload { value: i64 }\n\
         pub fn identity(value: str) -> str = value\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    let foreign = IType::Named {
        path: "dep.Payload".to_string(),
        args: vec![],
    };
    summary.fns[0].params[0].ty = foreign.clone();
    summary.fns[0].ret = foreign;
    assert_eq!(
        validate_for_import(&summary),
        Ok(()),
        "an unresolved foreign nominal remains conservatively borrow-capable even when this interface defines a scalar local type with the same bare name"
    );
}

#[test]
fn semantic_import_substitutes_local_generic_nominal_arguments() {
    let mut base = one(
        "pub Wrapper<T> { value: T }\n\
         pub Choice<T> { Some(T), None }\n\
         pub fn identity(value: str) -> str = value\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    let wrapper = base
        .structs
        .iter()
        .find(|definition| definition.name == "Wrapper")
        .expect("Wrapper definition")
        .clone();
    let mut inner = wrapper.clone();
    inner.name = "Inner".to_string();
    let mut outer = wrapper;
    outer.name = "Outer".to_string();
    outer.fields[0].1 = IType::Named {
        path: "Inner".to_string(),
        args: vec![IType::Named {
            path: "T".to_string(),
            args: vec![],
        }],
    };
    base.structs.extend([inner, outer]);
    sync_generic_type_bodies(&mut base);
    let named = |path: &str, argument: &str| IType::Named {
        path: path.to_string(),
        args: vec![IType::Named {
            path: argument.to_string(),
            args: vec![],
        }],
    };

    let mut scalar_parameter = base.clone();
    scalar_parameter.fns[0].params[0].ty = named("Wrapper", "i64");
    assert_eq!(
        validate_for_import(&scalar_parameter),
        Err(ImportCompatibilityError::ReturnSummaryRootCannotBorrow(0))
    );

    let mut scalar_struct_return = base.clone();
    scalar_struct_return.fns[0].ret = named("Wrapper", "i64");
    assert_eq!(
        validate_for_import(&scalar_struct_return),
        Err(ImportCompatibilityError::ReturnSummaryOnNonBorrowingType)
    );

    let mut scalar_enum_return = base.clone();
    scalar_enum_return.fns[0].ret = named("Choice", "i64");
    assert_eq!(
        validate_for_import(&scalar_enum_return),
        Err(ImportCompatibilityError::ReturnSummaryOnNonBorrowingType)
    );

    let mut nested_scalar_return = base.clone();
    nested_scalar_return.fns[0].ret = named("Outer", "i64");
    assert_eq!(
        validate_for_import(&nested_scalar_return),
        Err(ImportCompatibilityError::ReturnSummaryOnNonBorrowingType)
    );

    let mut borrowing = base;
    borrowing.fns[0].params[0].ty = named("Outer", "str");
    borrowing.fns[0].ret = named("Choice", "str");
    assert_eq!(validate_for_import(&borrowing), Ok(()));
}

#[test]
fn semantic_import_distinguishes_transformed_generic_cycle_instantiations() {
    let mut summary = one(
        "pub Wrapper<T> { value: T }\n\
         pub Choice<T> { Some(T), None }\n\
         pub fn identity(value: str) -> str = value\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    let template = summary
        .structs
        .iter()
        .find(|definition| definition.name == "Wrapper")
        .expect("Wrapper definition")
        .clone();
    let named = |path: &str, args: Vec<IType>| IType::Named {
        path: path.to_string(),
        args,
    };
    let parameter = || named("T", vec![]);

    let mut a = template.clone();
    a.name = "A".to_string();
    a.fields = vec![(
        "bs".to_string(),
        named("array", vec![named("B", vec![parameter()])]),
    )];

    let mut b = template;
    b.name = "B".to_string();
    b.fields = vec![
        (
            "links".to_string(),
            named("array", vec![named("A", vec![named("str", vec![])])]),
        ),
        ("value".to_string(), parameter()),
    ];
    summary.structs.extend([a, b]);
    sync_generic_type_bodies(&mut summary);

    let root = named("A", vec![named("i64", vec![])]);
    summary.fns[0].params[0].ty = root.clone();
    summary.fns[0].ret = root;
    summary.fns[0].return_cleanup = align_sema::hir::ReturnCleanupAbi::DynamicBit;
    assert_eq!(
        validate_for_import(&summary),
        Ok(()),
        "`B<i64>` and `B<str>` are distinct concrete capability nodes; the latter exposes the reachable `str` leaf"
    );

    let mut finite = summary.clone();
    let mut shift = finite
        .structs
        .iter()
        .find(|definition| definition.name == "Wrapper")
        .expect("Wrapper definition")
        .clone();
    shift.name = "FiniteShift".to_string();
    let mut second_parameter = shift.type_params[0].clone();
    second_parameter.name = "U".to_string();
    shift.type_params.push(second_parameter);
    shift.fields = vec![
        (
            "next".to_string(),
            named(
                "FiniteShift",
                vec![
                    named("T", vec![]),
                    named("Option", vec![named("str", vec![])]),
                ],
            ),
        ),
        ("value".to_string(), named("U", vec![])),
    ];
    finite.structs.push(shift);
    sync_generic_type_bodies(&mut finite);
    let root = named(
        "FiniteShift",
        vec![named("i64", vec![]), named("bool", vec![])],
    );
    finite.fns[0].params[0].ty = root.clone();
    finite.fns[0].ret = root;
    finite.fns[0].return_cleanup = align_sema::hir::ReturnCleanupAbi::None;
    assert_eq!(
        validate_for_import(&finite),
        Ok(()),
        "preserving one argument while replacing another with a larger constant type reaches an exact finite cycle"
    );

    let mut finite_constant = summary.clone();
    let mut constant = finite_constant
        .structs
        .iter()
        .find(|definition| definition.name == "Wrapper")
        .expect("Wrapper definition")
        .clone();
    constant.name = "FiniteConstant".to_string();
    constant.fields = vec![
        (
            "next".to_string(),
            named(
                "FiniteConstant",
                vec![named("Option", vec![named("i64", vec![])])],
            ),
        ),
        ("view".to_string(), named("str", vec![])),
    ];
    finite_constant.structs.push(constant);
    sync_generic_type_bodies(&mut finite_constant);
    let root = named("FiniteConstant", vec![named("i64", vec![])]);
    finite_constant.fns[0].params[0].ty = root.clone();
    finite_constant.fns[0].ret = root;
    finite_constant.fns[0].return_cleanup = align_sema::hir::ReturnCleanupAbi::None;
    assert_eq!(
        validate_for_import(&finite_constant),
        Ok(()),
        "a larger constant actual that happens to contain the prior concrete type is not parameter-driven growth"
    );

    let mut generative = summary.clone();
    let mut grow = generative
        .structs
        .iter()
        .find(|definition| definition.name == "Wrapper")
        .expect("Wrapper definition")
        .clone();
    grow.name = "Grow".to_string();
    grow.fields = vec![
        (
            "next".to_string(),
            named(
                "Grow",
                vec![named("Option", vec![named("T", vec![])])],
            ),
        ),
        ("view".to_string(), named("str", vec![])),
    ];
    generative.structs.push(grow);
    sync_generic_type_bodies(&mut generative);
    let root = named("Grow", vec![named("i64", vec![])]);
    generative.fns[0].params[0].ty = root.clone();
    generative.fns[0].ret = root;
    assert_eq!(
        validate_for_import(&generative),
        Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph),
        "a struct argument that embeds and grows its prior actual must reject even after a borrowing leaf is found"
    );

    let mut empty_summary = generative.clone();
    empty_summary.fns[0].return_borrow = ReturnBorrowSummary::None;
    empty_summary.fns[0].return_region = ReturnRegionSummary::None;
    assert_eq!(
        validate_for_import(&empty_summary),
        Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph),
        "an empty provenance summary must not skip signature type-graph validation"
    );

    let mut unreferenced = generative;
    unreferenced.fns[0].params[0].ty = named("str", vec![]);
    unreferenced.fns[0].ret = named("str", vec![]);
    unreferenced.fns[0].return_borrow = ReturnBorrowSummary::None;
    unreferenced.fns[0].return_region = ReturnRegionSummary::None;
    assert_eq!(
        validate_for_import(&unreferenced),
        Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph),
        "an unreferenced public definition must still have a finite capability graph"
    );

    let mut generative_pair = summary.clone();
    let mut grow = generative_pair
        .structs
        .iter()
        .find(|definition| definition.name == "Wrapper")
        .expect("Wrapper definition")
        .clone();
    grow.name = "GrowPair".to_string();
    let mut second_parameter = grow.type_params[0].clone();
    second_parameter.name = "U".to_string();
    grow.type_params.push(second_parameter);
    grow.fields = vec![(
        "next".to_string(),
        named(
            "GrowPair",
            vec![
                named("Option", vec![named("U", vec![])]),
                named("T", vec![]),
            ],
        ),
    )];
    generative_pair.structs.push(grow);
    sync_generic_type_bodies(&mut generative_pair);
    let root = named(
        "GrowPair",
        vec![named("i64", vec![]), named("bool", vec![])],
    );
    generative_pair.fns[0].params[0].ty = root.clone();
    generative_pair.fns[0].ret = root;
    assert_eq!(
        validate_for_import(&generative_pair),
        Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph),
        "a growing permutation of all prior arguments must reject"
    );

    let mut generative_enum = summary;
    let mut grow = generative_enum
        .enums
        .iter()
        .find(|definition| definition.name == "Choice")
        .expect("Choice definition")
        .clone();
    grow.name = "GrowChoice".to_string();
    grow.variants = vec![
        (
            "Next".to_string(),
            vec![named(
                "GrowChoice",
                vec![named("Option", vec![named("T", vec![])])],
            )],
        ),
        ("View".to_string(), vec![named("str", vec![])]),
    ];
    generative_enum.enums.push(grow);
    sync_generic_type_bodies(&mut generative_enum);
    let root = named("GrowChoice", vec![named("i64", vec![])]);
    generative_enum.fns[0].params[0].ty = root.clone();
    generative_enum.fns[0].ret = root;
    assert_eq!(
        validate_for_import(&generative_enum),
        Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph),
        "an enum argument that embeds and grows its prior actual must reject even after a borrowing leaf is found"
    );
}

#[test]
fn semantic_import_growth_transport_distinguishes_exposure_and_convergence() {
    let base = one(
        "pub Wrapper<T> { value: T }\n\
         pub Leaf { view: str }\n\
         pub fn identity(value: str) -> str = value\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    let template = base
        .structs
        .iter()
        .find(|definition| definition.name == "Wrapper")
        .expect("Wrapper definition")
        .clone();
    let named = |path: &str, args: Vec<IType>| IType::Named {
        path: path.to_string(),
        args,
    };
    let parameter = |name: &str| named(name, vec![]);

    let mut whole_actual = base.clone();
    let root = named("Wrapper", vec![named("Leaf", vec![])]);
    whole_actual.fns[0].params[0].ty = root.clone();
    whole_actual.fns[0].ret = root;
    assert_eq!(
        validate_for_import(&whole_actual),
        Ok(()),
        "an ordinary whole local nominal actual is finite and borrow-capable"
    );

    let mut converge = base.clone();
    let mut definition = template.clone();
    definition.name = "Converge".to_string();
    let mut second = definition.type_params[0].clone();
    second.name = "U".to_string();
    definition.type_params.push(second);
    definition.fields = vec![(
        "next".to_string(),
        named(
            "Converge",
            vec![
                named("Option", vec![parameter("U")]),
                named("str", vec![]),
            ],
        ),
    )];
    converge.structs.push(definition);
    sync_generic_type_bodies(&mut converge);
    assert_eq!(
        validate_for_import(&converge),
        Ok(()),
        "the wrapped parameter moves into a slot that the next transition replaces"
    );

    let mut exposed = base.clone();
    let mut identity = template.clone();
    identity.name = "Id".to_string();
    let mut grow = template.clone();
    grow.name = "GrowThroughId".to_string();
    grow.fields = vec![(
        "next".to_string(),
        named(
            "Id",
            vec![named(
                "GrowThroughId",
                vec![named("Option", vec![parameter("T")])],
            )],
        ),
    )];
    exposed.structs.extend([identity, grow]);
    sync_generic_type_bodies(&mut exposed);
    assert_eq!(
        validate_for_import(&exposed),
        Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph),
        "Id exposes its whole actual and reveals the positive recursive edge"
    );

    let mut hidden = base.clone();
    let mut sink = template.clone();
    sink.name = "Sink".to_string();
    sink.fields[0].1 = named("i64", vec![]);
    let mut grow = template.clone();
    grow.name = "HiddenBySink".to_string();
    grow.fields = vec![(
        "next".to_string(),
        named(
            "Sink",
            vec![named(
                "HiddenBySink",
                vec![named("Option", vec![parameter("T")])],
            )],
        ),
    )];
    hidden.structs.extend([sink, grow]);
    sync_generic_type_bodies(&mut hidden);
    assert_eq!(
        validate_for_import(&hidden),
        Ok(()),
        "Sink removes its actual from capability traversal"
    );

    let mut direct_opaque_growth = base.clone();
    let mut grow = template.clone();
    grow.name = "OpaqueGrow".to_string();
    grow.fields = vec![(
        "next".to_string(),
        named(
            "OpaqueGrow",
            vec![named("box", vec![parameter("T")])],
        ),
    )];
    direct_opaque_growth.structs.push(grow);
    sync_generic_type_bodies(&mut direct_opaque_growth);
    assert_eq!(
        validate_for_import(&direct_opaque_growth),
        Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph),
        "a direct actual measures its complete syntax even when its outer constructor is opaque"
    );

    for (name, suffix, opaque) in [
        (
            "box",
            "Box",
            named("box", vec![parameter("T")]),
        ),
        (
            "function type",
            "Fn",
            IType::Fn {
                params: vec![IParam {
                    mode: ParamMode::ByValue,
                    ty: parameter("T"),
                }],
                ret: Box::new(parameter("T")),
                return_borrow: ReturnBorrowSummary::None,
                return_region: ReturnRegionSummary::None,
                return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
            },
        ),
    ] {
        let mut composed = base.clone();
        let mut identity = template.clone();
        identity.name = format!("IdThrough{suffix}");
        let mut shield = template.clone();
        shield.name = format!("ShieldThrough{suffix}");
        shield.fields = vec![(
            "value".to_string(),
            named(&identity.name, vec![opaque]),
        )];
        let mut consumer = template.clone();
        consumer.name = format!("ConsumerThrough{suffix}");
        consumer.fields = vec![(
            "next".to_string(),
            named(
                &shield.name,
                vec![named(
                    &consumer.name,
                    vec![named("Option", vec![parameter("T")])],
                )],
            ),
        )];
        composed.structs.extend([identity, shield, consumer]);
        sync_generic_type_bodies(&mut composed);
        assert_eq!(
            validate_for_import(&composed),
            Ok(()),
            "{name} below an exposed local actual must stop transport before an enclosing consumer"
        );
    }

    let mut nested_opaque = base;
    let mut identity = template.clone();
    identity.name = "OpaqueId".to_string();
    let mut cycle = template;
    cycle.name = "OpaqueBoundary".to_string();
    cycle.fields = vec![(
        "next".to_string(),
        named(
            "OpaqueId",
            vec![named(
                "box",
                vec![named("OpaqueBoundary", vec![parameter("T")])],
            )],
        ),
    )];
    nested_opaque.structs.extend([identity, cycle]);
    sync_generic_type_bodies(&mut nested_opaque);
    assert_eq!(
        validate_for_import(&nested_opaque),
        Ok(()),
        "dependency discovery stops below an exposed opaque actual"
    );
}

#[test]
fn semantic_import_growth_graph_handles_mutual_permuted_and_parallel_edges() {
    let base = one(
        "pub Wrapper<T> { value: T }\n\
         pub fn identity(value: str) -> str = value\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    let template = base.structs[0].clone();
    let named = |path: &str, args: Vec<IType>| IType::Named {
        path: path.to_string(),
        args,
    };
    let parameter = |name: &str| named(name, vec![]);

    let mut borrow_free = base.clone();
    let mut grow = template.clone();
    grow.name = "BorrowFreeGrow".to_string();
    grow.fields = vec![(
        "next".to_string(),
        named(
            "BorrowFreeGrow",
            vec![named("Option", vec![parameter("T")])],
        ),
    )];
    borrow_free.structs.push(grow);
    sync_generic_type_bodies(&mut borrow_free);
    assert_eq!(
        validate_for_import(&borrow_free),
        Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph),
        "growth transport remains live even when the borrow summary is empty"
    );

    let mut mutual = base.clone();
    let mut left = template.clone();
    left.name = "MutualLeft".to_string();
    left.fields = vec![(
        "next".to_string(),
        named(
            "MutualRight",
            vec![named("Option", vec![parameter("T")])],
        ),
    )];
    let mut right = template.clone();
    right.name = "MutualRight".to_string();
    right.type_params[0].name = "U".to_string();
    right.fields = vec![(
        "next".to_string(),
        named("MutualLeft", vec![parameter("U")]),
    )];
    mutual.structs.extend([left, right]);
    sync_generic_type_bodies(&mut mutual);
    assert_eq!(
        validate_for_import(&mutual),
        Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph),
        "a positive edge in a mutual dependency SCC rejects"
    );

    let mut zero_cycle = base.clone();
    let mut left = template.clone();
    left.name = "ZeroLeft".to_string();
    left.fields = vec![(
        "next".to_string(),
        named("ZeroRight", vec![parameter("T")]),
    )];
    let mut right = template.clone();
    right.name = "ZeroRight".to_string();
    right.type_params[0].name = "U".to_string();
    right.fields = vec![(
        "next".to_string(),
        named("ZeroLeft", vec![parameter("U")]),
    )];
    zero_cycle.structs.extend([left, right]);
    sync_generic_type_bodies(&mut zero_cycle);
    assert_eq!(
        validate_for_import(&zero_cycle),
        Ok(()),
        "a zero-weight mutual cycle is finite"
    );

    let mut permutation = base.clone();
    let mut swap = template.clone();
    swap.name = "Swap".to_string();
    let mut second = swap.type_params[0].clone();
    second.name = "U".to_string();
    swap.type_params.push(second);
    swap.fields = vec![(
        "next".to_string(),
        named("Swap", vec![parameter("U"), parameter("T")]),
    )];
    permutation.structs.push(swap);
    sync_generic_type_bodies(&mut permutation);
    assert_eq!(
        validate_for_import(&permutation),
        Ok(()),
        "a zero-weight parameter permutation is finite"
    );

    let mut duplication = base.clone();
    let mut duplicate = template.clone();
    duplicate.name = "Duplicate".to_string();
    let mut second = duplicate.type_params[0].clone();
    second.name = "U".to_string();
    duplicate.type_params.push(second);
    duplicate.fields = vec![(
        "next".to_string(),
        named("Duplicate", vec![parameter("T"), parameter("T")]),
    )];
    duplication.structs.push(duplicate);
    sync_generic_type_bodies(&mut duplication);
    assert_eq!(
        validate_for_import(&duplication),
        Ok(()),
        "zero-weight duplication into a slot removed by the greatest fixed point is finite"
    );

    let mut parallel = base;
    let mut grow = template;
    grow.name = "ParallelEdges".to_string();
    grow.fields = vec![
        (
            "same".to_string(),
            named("ParallelEdges", vec![parameter("T")]),
        ),
        (
            "larger".to_string(),
            named(
                "ParallelEdges",
                vec![named("Option", vec![parameter("T")])],
            ),
        ),
    ];
    parallel.structs.push(grow);
    sync_generic_type_bodies(&mut parallel);
    assert_eq!(
        validate_for_import(&parallel),
        Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph),
        "a positive parallel edge must not be deduplicated behind a zero edge"
    );
}

#[test]
fn compiler_produced_builtin_name_collisions_follow_source_resolution_precedence() {
    let units = [
        unit(
            "names",
            false,
            "\
module names
pub Option { value: i64 }
pub Task { value: i64 }
pub Combined {
  builtin_value: Option<str>,
  local_option: names.Option,
  local_task: Task,
}
",
        ),
        unit(
            "params",
            false,
            "\
module params
pub BuiltinHolder<Option> { value: Option<str> }
pub BuiltinChoice<Option> { Some(Option<str>) }
pub fn builtin<Option>(value: Option<str>) -> Option<str> = value
",
        ),
        unit(
            "main",
            true,
            "import names\nimport params\nfn main() -> i32 = 0\n",
        ),
    ];
    let produced = summaries(&units);
    let names = find(&produced, "names");
    assert_eq!(
        validate_for_import(names),
        Ok(()),
        "every compiler-produced public name collision must remain importable"
    );
    assert_eq!(
        validate_for_import(find(&produced, "params")),
        Ok(()),
        "a non-shadowing type parameter may reuse a source-builtin spelling"
    );

    let json_units = [
        unit(
            "json",
            false,
            "\
module json
pub doc { value: i64 }
pub Row { value: i64 }
pub fn local(value: doc) -> doc = value
pub fn builtins(
  document: json.doc,
  kind: json.kind,
  scanner: json.scanner<Row>,
) -> i64 = 0
",
        ),
        unit("main", true, "import json\nfn main() -> i32 = 0\n"),
    ];
    let produced = summaries(&json_units);
    assert_eq!(
        validate_for_import(find(&produced, "json")),
        Ok(()),
        "qualified json builtins must retain precedence over a same-unit local `doc`"
    );
}

#[test]
fn semantic_import_type_shape_errors_are_exact_and_precede_headers() {
    let base = one(
        "pub Wrapper<T> { value: T }\n\
         pub fn identity(value: str) -> str = value\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    let named = |path: &str, args: Vec<IType>| IType::Named {
        path: path.to_string(),
        args,
    };

    for alias in ["Error", "argon2_params", "regex_match"] {
        let mut local_struct = base.clone();
        local_struct.structs[0].name = alias.to_string();
        sync_generic_type_bodies(&mut local_struct);
        assert_eq!(
            validate_for_import(&local_struct),
            Ok(()),
            "non-entry local struct name `{alias}` must remain a valid nominal"
        );

        let mut local_enum = base.clone();
        let mut enumeration =
            one("pub Choice { A }\nfn main() -> i32 = 0\n").remove(0).enums.remove(0);
        enumeration.name = alias.to_string();
        local_enum.enums.push(enumeration);
        assert_eq!(
            validate_for_import(&local_enum),
            Ok(()),
            "non-entry local sum-type name `{alias}` must remain a valid nominal"
        );
    }

    let mut duplicate_local = base.clone();
    duplicate_local.structs.push(duplicate_local.structs[0].clone());
    duplicate_local.fns[0].params[0].ty = named("Missing", vec![]);
    assert_eq!(
        validate_for_import(&duplicate_local),
        Err(ImportCompatibilityError::DuplicateLocalType(
            "Wrapper".to_string()
        )),
        "definition-index errors precede all type-shape errors"
    );

    let mut cross_kind_collision = base.clone();
    let mut enumeration =
        one("pub Choice { A }\nfn main() -> i32 = 0\n").remove(0).enums.remove(0);
    enumeration.name = "Wrapper".to_string();
    cross_kind_collision.enums.push(enumeration);
    assert_eq!(
        validate_for_import(&cross_kind_collision),
        Err(ImportCompatibilityError::DuplicateLocalType(
            "Wrapper".to_string()
        )),
        "struct and enum names share one exact local-definition namespace"
    );

    let mut duplicate_parameter = base.clone();
    let duplicate = duplicate_parameter.structs[0].type_params[0].clone();
    duplicate_parameter.structs[0].type_params.push(duplicate);
    assert_eq!(
        validate_for_import(&duplicate_parameter),
        Err(ImportCompatibilityError::DuplicateTypeParameter(
            "T".to_string()
        ))
    );

    let type_parameter = |name: &str| ITypeParam {
        name: name.to_string(),
        bound: None,
    };
    let mut function_shadow = base.clone();
    function_shadow.fns[0].type_params = vec![type_parameter("Wrapper")];
    function_shadow.fns[0].generic_body =
        Some("pub fn identity<Wrapper>(value: str) -> str = value".to_string());
    assert_eq!(
        validate_for_import(&function_shadow),
        Err(ImportCompatibilityError::TypeParameterShadowsLocalType(
            "Wrapper".to_string()
        ))
    );

    let mut struct_shadow = base.clone();
    struct_shadow.structs[0].type_params = vec![type_parameter("Wrapper")];
    assert_eq!(
        validate_for_import(&struct_shadow),
        Err(ImportCompatibilityError::TypeParameterShadowsLocalType(
            "Wrapper".to_string()
        ))
    );

    let mut enum_shadow = base.clone();
    let mut generic_enum =
        one("pub Choice<T> { Some(T) }\nfn main() -> i32 = 0\n")
            .remove(0)
            .enums
            .remove(0);
    generic_enum.type_params = vec![type_parameter("Wrapper")];
    enum_shadow.enums.push(generic_enum);
    assert_eq!(
        validate_for_import(&enum_shadow),
        Err(ImportCompatibilityError::TypeParameterShadowsLocalType(
            "Wrapper".to_string()
        ))
    );

    let mut duplicate_and_shadow = function_shadow;
    duplicate_and_shadow.fns[0].type_params =
        vec![type_parameter("Wrapper"), type_parameter("Wrapper")];
    assert_eq!(
        validate_for_import(&duplicate_and_shadow),
        Err(ImportCompatibilityError::DuplicateTypeParameter(
            "Wrapper".to_string()
        )),
        "duplicate type parameters precede local-type shadowing"
    );

    let mut parameter_arguments = base.clone();
    parameter_arguments.structs[0].fields[0].1 =
        named("T", vec![named("i64", vec![])]);
    assert_eq!(
        validate_for_import(&parameter_arguments),
        Err(ImportCompatibilityError::TypeParameterWithArguments(
            "T".to_string()
        ))
    );

    let mut local_arity = base.clone();
    local_arity.fns[0].params[0].ty = named("Wrapper", vec![]);
    assert_eq!(
        validate_for_import(&local_arity),
        Err(ImportCompatibilityError::InvalidTypeArity {
            name: "Wrapper".to_string(),
            expected: 1,
            actual: 0,
        })
    );

    let mut builtin_arity = base.clone();
    builtin_arity.fns[0].params[0].ty =
        named("Result", vec![named("str", vec![])]);
    assert_eq!(
        validate_for_import(&builtin_arity),
        Err(ImportCompatibilityError::InvalidTypeArity {
            name: "Result".to_string(),
            expected: 2,
            actual: 1,
        })
    );

    let mut unresolved = base.clone();
    unresolved.fns[0].params[0].mode = ParamMode::Borrow;
    unresolved.fns[0].params[0].ty = named("Missing", vec![]);
    assert_eq!(
        validate_for_import(&unresolved),
        Err(ImportCompatibilityError::UnresolvedBareType(
            "Missing".to_string()
        )),
        "complete type shape precedes ownership-dependent mode validation"
    );

    let mut qualified_local = base.clone();
    let qualified = named("main.Wrapper", vec![named("str", vec![])]);
    qualified_local.fns[0].params[0].ty = qualified.clone();
    qualified_local.fns[0].ret = qualified;
    assert_eq!(validate_for_import(&qualified_local), Ok(()));

    let mut unit_prefix_foreign = base.clone();
    unit_prefix_foreign.fns[0].params[0].ty =
        named("main.child.Foreign", vec![]);
    assert_eq!(
        validate_for_import(&unit_prefix_foreign),
        Ok(()),
        "a longer qualified module sharing the unit prefix is foreign, not a missing local"
    );

    let mut missing_qualified_local = base.clone();
    missing_qualified_local.fns[0].params[0].ty =
        named("main.Missing", vec![]);
    assert_eq!(
        validate_for_import(&missing_qualified_local),
        Err(ImportCompatibilityError::UnresolvedBareType(
            "main.Missing".to_string()
        ))
    );

    let mut malformed_nested = base;
    malformed_nested.structs[0].fields[0].1 = IType::Fn {
        params: vec![IParam {
            mode: ParamMode::Borrow,
            ty: named("NestedMissing", vec![]),
        }],
        ret: Box::new(named("str", vec![])),
        return_borrow: ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: vec![],
        },
        return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
        return_region: ReturnRegionSummary::None,
    };
    assert_eq!(
        validate_for_import(&malformed_nested),
        Err(ImportCompatibilityError::UnresolvedBareType(
            "NestedMissing".to_string()
        )),
        "nested function children participate in the complete shape walk before header errors"
    );

    let mut missing_definition_body = one(
        "pub Wrapper<T> { value: T }\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    missing_definition_body.structs[0].generic_body = None;
    assert_eq!(
        validate_for_import(&missing_definition_body),
        Err(ImportCompatibilityError::GenericBodyMismatch(
            "Wrapper".to_string()
        )),
        "generic definition parameters require their transported generic body"
    );

    let mut malformed_enum =
        one("pub Choice<T> { Some(T), None }\nfn main() -> i32 = 0\n").remove(0);
    malformed_enum.enums[0].variants[0].1[0] = named("MissingEnumType", vec![]);
    assert_eq!(
        validate_for_import(&malformed_enum),
        Err(ImportCompatibilityError::UnresolvedBareType(
            "MissingEnumType".to_string()
        )),
        "sum payloads participate in the complete shape walk"
    );

    let mut malformed_constant =
        one("pub LIMIT: i64 := 1\nfn main() -> i32 = 0\n").remove(0);
    malformed_constant.consts[0].ty = Some(named("MissingConstType", vec![]));
    assert_eq!(
        validate_for_import(&malformed_constant),
        Err(ImportCompatibilityError::UnresolvedBareType(
            "MissingConstType".to_string()
        )),
        "constant annotations participate in the complete shape walk"
    );
}

#[test]
fn semantic_import_generic_fragments_match_their_structured_records() {
    let valid = one(
        "pub fn identity<T: Eq>(value: T) -> T = value\n\
         pub align(16) Wrapper<T> { value: T }\n\
         pub Choice<T> { Some(T), None }\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    assert_eq!(validate_for_import(&valid), Ok(()));
    let rendered = summary_to_source(&valid, &[]).expect("valid generic fragments render");
    assert!(rendered.contains("pub fn identity<T: Eq>"));
    assert!(rendered.contains("pub align(16) Wrapper<T>"));
    assert!(rendered.contains("pub Choice<T>"));

    let mut generic_c_layout = valid.clone();
    generic_c_layout.structs[0].c_repr = true;
    generic_c_layout.structs[0].generic_body = Some("not valid".to_string());
    assert_eq!(
        validate_for_import(&generic_c_layout),
        Err(ImportCompatibilityError::GenericCLayoutUnsupported(
            "Wrapper".to_string()
        )),
        "producer-forbidden generic C layout precedes fragment syntax"
    );

    for (case, body) in [
        (
            "extra-pub",
            "pub fn identity<T: Eq>(value: T) -> T = value",
        ),
        (
            "module",
            "module forged\nfn identity<T: Eq>(value: T) -> T = value",
        ),
        (
            "import",
            "import forged\nfn identity<T: Eq>(value: T) -> T = value",
        ),
        (
            "second-item",
            "fn identity<T: Eq>(value: T) -> T = value\nfn other() -> i64 = 0",
        ),
        (
            "trailing-token",
            "fn identity<T: Eq>(value: T) -> T = value @",
        ),
        ("malformed", "fn identity<T: Eq>("),
    ] {
        let mut forged = valid.clone();
        forged.fns[0].generic_body = Some(body.to_string());
        assert_eq!(
            validate_for_import(&forged),
            Err(ImportCompatibilityError::GenericBodySyntax(
                "identity".to_string()
            )),
            "{case} must reject as fragment syntax before structured comparison"
        );
    }

    let mut wrong_function = valid.clone();
    wrong_function.fns[0].generic_body =
        Some("fn renamed<T: Eq>(value: T) -> T = value".to_string());
    assert_eq!(
        validate_for_import(&wrong_function),
        Err(ImportCompatibilityError::GenericBodyMismatch(
            "identity".to_string()
        ))
    );

    let mut wrong_function_header = valid.clone();
    wrong_function_header.fns[0].generic_body =
        Some("fn identity<T: Ord>(out value: T) -> i64 = 0".to_string());
    assert_eq!(
        validate_for_import(&wrong_function_header),
        Err(ImportCompatibilityError::GenericBodyMismatch(
            "identity".to_string()
        ))
    );

    let mut wrong_struct = valid.clone();
    wrong_struct.structs[0].generic_body =
        Some("Wrapper<T> { other: T }".to_string());
    assert_eq!(
        validate_for_import(&wrong_struct),
        Err(ImportCompatibilityError::GenericBodyMismatch(
            "Wrapper".to_string()
        ))
    );

    let mut wrong_kind = valid.clone();
    wrong_kind.structs[0].align = None;
    wrong_kind.structs[0].generic_body =
        Some("Wrapper<T> { Some(T) }".to_string());
    assert_eq!(
        validate_for_import(&wrong_kind),
        Err(ImportCompatibilityError::GenericBodyMismatch(
            "Wrapper".to_string()
        ))
    );

    let mut wrong_enum = valid;
    wrong_enum.enums[0].generic_body =
        Some("Choice<T> { Other(T), None }".to_string());
    assert_eq!(
        validate_for_import(&wrong_enum),
        Err(ImportCompatibilityError::GenericBodyMismatch(
            "Choice".to_string()
        ))
    );
}

#[test]
fn semantic_import_validates_nested_function_type_summaries() {
    let mut summary = one("pub Holder { value: i64 }\nfn main() -> i32 = 0\n").remove(0);
    summary.structs[0].fields[0].1 = IType::Fn {
        params: vec![IParam {
            mode: ParamMode::ByValue,
            ty: IType::Named {
                path: "str".to_string(),
                args: vec![],
            },
        }],
        ret: Box::new(IType::Named {
            path: "i64".to_string(),
            args: vec![],
        }),
        return_borrow: ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: vec![],
        },
        return_region: ReturnRegionSummary::Roots {
            params: vec![0],
            captures: vec![],
        },
        return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
    };
    assert_eq!(
        validate_for_import(&summary),
        Err(ImportCompatibilityError::ReturnSummaryOnUnsupportedSignature)
    );
}

#[test]
fn semantic_import_rejects_generic_and_recursive_capability_summaries() {
    let mut generic =
        one("pub fn identity<T>(value: T) -> T = value\nfn main() -> i32 = 0\n").remove(0);
    generic.fns[0].return_borrow = ReturnBorrowSummary::Roots {
        params: vec![0],
        captures: vec![],
    };
    generic.fns[0].return_region = ReturnRegionSummary::Roots {
        params: vec![0],
        captures: vec![],
    };
    assert_eq!(
        validate_for_import(&generic),
        Err(ImportCompatibilityError::ReturnSummaryOnUnsupportedSignature),
        "generic template roots have no imported side-channel before L2b-b"
    );
    let mut mismatched_generic_shape = generic.clone();
    mismatched_generic_shape.fns[0].type_params.clear();
    assert_eq!(
        validate_for_import(&mismatched_generic_shape),
        Err(ImportCompatibilityError::UnresolvedBareType("T".to_string())),
        "the complete type-shape walk precedes generic-body/header classification"
    );
    let mut missing_generic_body = generic.clone();
    missing_generic_body.fns[0].generic_body = None;
    assert_eq!(
        validate_for_import(&missing_generic_body),
        Err(ImportCompatibilityError::GenericBodyMismatch(
            "identity".to_string()
        )),
        "declared type parameters require the generic body transported by their interface"
    );

    let mut recursive = one(
        "pub Wrapper<T> { value: T }\n\
         pub fn identity(value: str) -> str = value\n\
         fn main() -> i32 = 0\n",
    )
    .remove(0);
    recursive.fns[0].params[0].ty = IType::Named {
        path: "Wrapper".to_string(),
        args: vec![IType::Named {
            path: "T".to_string(),
            args: vec![],
        }],
    };
    assert_eq!(
        validate_for_import(&recursive),
        Err(ImportCompatibilityError::UnresolvedBareType("T".to_string())),
        "an undeclared type-parameter name is a malformed bare type"
    );

    let mut wrong_arity = recursive;
    wrong_arity.fns[0].params[0].ty = IType::Named {
        path: "Wrapper".to_string(),
        args: vec![],
    };
    assert_eq!(
        validate_for_import(&wrong_arity),
        Err(ImportCompatibilityError::InvalidTypeArity {
            name: "Wrapper".to_string(),
            expected: 1,
            actual: 0,
        }),
        "a malformed local generic application reports its exact arity"
    );
}

#[test]
fn borrowed_parameter_modes_round_trip_and_import() {
    for source in [
        "pub fn inspect(borrow value: string) -> i64 = value.len()\nfn main() -> i32 = 0\n",
        "pub fn inspect(borrow value: i64) -> i64 = value\nfn main() -> i32 = 0\n",
        "pub fn increment(borrow mut value: i64) { value = value + 1 }\nfn main() -> i32 = 0\n",
    ] {
        let mut summary = one(source).remove(0);
        rehash(&mut summary);
        let decoded = deserialize(&serialize(&summary)).expect("borrowed mode tag round-trips");
        assert_eq!(decoded, summary);
        assert_eq!(validate_for_import(&decoded), Ok(()));
    }

    let mut copy_borrow =
        one("pub fn inspect(value: slice<i64>) -> i64 = value.len()\nfn main() -> i32 = 0\n").remove(0);
    copy_borrow.fns[0].params[0].mode = ParamMode::Borrow;
    rehash(&mut copy_borrow);
    assert_eq!(validate_for_import(&copy_borrow), Ok(()));

    let mut borrowed_region =
        one("pub fn inspect(value: region) -> i64 = 0\nfn main() -> i32 = 0\n").remove(0);
    borrowed_region.fns[0].params[0].mode = ParamMode::Borrow;
    rehash(&mut borrowed_region);
    assert_eq!(
        validate_for_import(&borrowed_region),
        Err(ImportCompatibilityError::BorrowParamRegion)
    );
}

#[test]
fn malformed_return_roots_fail_closed_during_decode() {
    let mut duplicate =
        one("pub fn inspect(value: slice<i64>) -> i64 = value.len()\nfn main() -> i32 = 0\n").remove(0);
    duplicate.fns[0].return_region =
        ReturnRegionSummary::Roots { params: vec![0, 0], captures: vec![] };
    rehash(&mut duplicate);
    assert_eq!(
        deserialize(&serialize(&duplicate)),
        Err(DecodeError::InvalidSummary("parameter roots must be strictly increasing"))
    );

    let mut out_of_range = duplicate.clone();
    out_of_range.fns[0].return_region =
        ReturnRegionSummary::Roots { params: vec![1], captures: vec![] };
    rehash(&mut out_of_range);
    assert_eq!(
        deserialize(&serialize(&out_of_range)),
        Err(DecodeError::InvalidSummary("parameter root is outside the signature"))
    );

    let mut capture = duplicate;
    capture.fns[0].return_region =
        ReturnRegionSummary::Roots { params: vec![], captures: vec![0] };
    rehash(&mut capture);
    assert_eq!(
        deserialize(&serialize(&capture)),
        Err(DecodeError::InvalidSummary(
            "capture roots are forbidden in exported interfaces"
        ))
    );

    let mut empty = capture;
    empty.fns[0].return_region =
        ReturnRegionSummary::Roots { params: vec![], captures: vec![] };
    rehash(&mut empty);
    assert_eq!(
        deserialize(&serialize(&empty)),
        Err(DecodeError::InvalidSummary(
            "an empty root set must use the canonical None tag"
        ))
    );
}

#[test]
fn parallel_transfer_roots_are_canonical_and_change_the_interface_hash() {
    let sequential =
        one("pub fn run(values: slice<i64>) -> i64 = values.sum()\nfn main() -> i32 = 0\n")
            .remove(0);
    let parallel = one(
        "fn keep(value: i64) -> i64 = value\npub fn run(values: slice<i64>) -> i64 = values.par_map(keep).sum()\nfn main() -> i32 = 0\n",
    )
    .remove(0);
    assert!(sequential.fns[0].parallel_transfer_params.is_empty());
    assert_eq!(parallel.fns[0].parallel_transfer_params, [0]);
    assert_ne!(sequential.interface_hash, parallel.interface_hash);
    assert_eq!(deserialize(&serialize(&parallel)), Ok(parallel.clone()));

    let mut duplicate = parallel.clone();
    duplicate.fns[0].parallel_transfer_params = vec![0, 0];
    assert_eq!(
        validate_for_import(&duplicate),
        Err(ImportCompatibilityError::ParallelTransferRootsNonCanonical),
    );
    rehash(&mut duplicate);
    assert_eq!(
        deserialize(&serialize(&duplicate)),
        Err(DecodeError::InvalidSummary(
            "parallel-transfer roots must be strictly increasing"
        )),
    );

    let mut out_of_range = parallel;
    out_of_range.fns[0].parallel_transfer_params = vec![1];
    rehash(&mut out_of_range);
    assert_eq!(
        deserialize(&serialize(&out_of_range)),
        Err(DecodeError::InvalidSummary(
            "parallel-transfer root is outside the signature"
        )),
    );
}

#[test]
fn parameter_mode_codec_has_a_byte_golden_and_rejects_unknown_tags() {
    let summary =
        one("pub fn inspect(out value: slice<i64>) -> i64 = value.len()\nfn main() -> i32 = 0\n").remove(0);
    let surface = encode_interface_surface(&summary);
    let hex = surface.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    assert_eq!(
        hex,
        "06000000040000006d61696e0100000007000000696e73706563740000000001000000010005000000736c696365010000000003000000693634000000000003000000693634000000000000000000000000000000000000000000000000000000000000"
    );

    let mut artifact = serialize(&summary);
    let mode_offset = 4 // format
        + 4 + summary.unit.len()
        + 4 // fn sequence
        + 4 + summary.fns[0].name.len()
        + 4 // type-parameter sequence
        + 4; // parameter sequence
    artifact[mode_offset] = 0xff;
    assert_eq!(
        deserialize(&artifact),
        Err(DecodeError::BadTag { what: "parameter mode", tag: 0xff })
    );

    // This one-function surface ends with the function's borrow tag, region tag, cleanup ABI,
    // effect, empty parallel-transfer sequence, resource-hook-body bit, generic body option, then
    // the empty top-level struct/enum/resource/const sequences.
    let mut bad_borrow = serialize(&summary);
    bad_borrow[surface.len() - 26] = 0xff;
    assert_eq!(
        deserialize(&bad_borrow),
        Err(DecodeError::BadTag { what: "return-borrow summary", tag: 0xff })
    );
    let mut bad_region = serialize(&summary);
    bad_region[surface.len() - 25] = 0xff;
    assert_eq!(
        deserialize(&bad_region),
        Err(DecodeError::BadTag { what: "return-region summary", tag: 0xff })
    );
    let mut bad_cleanup = serialize(&summary);
    bad_cleanup[surface.len() - 24] = 0xff;
    assert_eq!(
        deserialize(&bad_cleanup),
        Err(DecodeError::BadTag {
            what: "return cleanup ABI",
            tag: 0xff,
        })
    );
}

// ---- 5. capability set ---------------------------------------------------------------------------

#[test]
fn capabilities_captured_per_unit() {
    let main = "import zip\nfn main() -> Result<(), Error> {\n  n := zip.csize(\"hello\")?\n  print(n)\n  return Ok(())\n}\n";
    let zip = "module zip\nimport std.compress\n\
               pub fn csize(s: str) -> Result<i64, Error> {\n\
               \x20 c := compress.gzip_compress(s, 6)?\n\
               \x20 return Ok(c.len())\n}\n";
    let sums = summaries(&[unit("main", true, main), unit("zip", false, zip)]);
    assert_eq!(find(&sums, "zip").capabilities, vec!["Zlib".to_string()], "compress unit shows Zlib");
    assert!(find(&sums, "main").capabilities.is_empty(), "pure-numeric entry unit has no capabilities");
}
