//! M15 S1b: per-unit sema against imported interface summaries.
//!
//! Gate 1 (differential equivalence): per-unit-over-summaries checking produces the SAME accept/reject
//! verdict as whole-program `check_program` across a corpus of multi-file programs.
//! Gate 2 (blindness): a per-unit check reads only a unit's AST + its imports' summaries.
//! Gate 5 (transitive): a `pub`-surface change in a transitive dependency changes a unit's recorded
//! dependency hash set; a private-body change does not.

mod common;
use common::*;

// ---- Gate 1: differential equivalence over a corpus -----------------------------------------------

#[test]
fn single_file_program_accepts() {
    // N=1: a single-file program is one unit with no dependencies — the per-unit path degenerates to
    // the whole-program check, and records an empty dependency set.
    let main = "module main\nfn helper(x: i64) -> i64 = x + 1\nfn main() -> i32 = helper(41) as i32\n";
    let r = assert_same_verdict("s1b-single", &[("main.align", main)], "main.align");
    assert!(!r.diags.has_errors());
    assert_eq!(r.summaries.len(), 1);
    assert_eq!(r.dep_interface_hashes.len(), 1);
    assert!(r.dep_interface_hashes[0].1.is_empty(), "a single-file program has no dependencies");
}

#[test]
fn cross_module_pub_call_accepts() {
    let geom = "module geom\npub fn square(x: i64) -> i64 = x * x\n";
    let main = "module main\nimport geom\nfn main() -> i32 {\n  return geom.square(5) as i32\n}\n";
    let r = assert_same_verdict("s1b-cross-call", &[("geom.align", geom), ("main.align", main)], "main.align");
    assert!(!r.diags.has_errors());
    // Both units summarized (bottom-up: geom before main).
    assert_eq!(r.summaries.len(), 2);
    assert_eq!(r.summaries[0].unit, "geom");
    assert_eq!(r.summaries[1].unit, "main");
}

#[test]
fn cross_module_type_accepts() {
    let geom = concat!(
        "module geom\n",
        "pub Point { x: i64, y: i64 }\n",
        "pub fn origin() -> Point = Point { x: 0, y: 0 }\n",
        "pub fn getx(p: Point) -> i64 = p.x\n",
    );
    let main = concat!(
        "module main\n",
        "import geom\n",
        "fn main() -> i32 {\n",
        "  p := geom.origin()\n",
        "  return geom.getx(p) as i32\n",
        "}\n",
    );
    assert_same_verdict("s1b-cross-type", &[("geom.align", geom), ("main.align", main)], "main.align");
}

#[test]
fn cross_module_sum_type_accepts() {
    let shapes = concat!(
        "module shapes\n",
        "pub Shape { Circle(i64), Square(i64) }\n",
        "pub fn make() -> Shape = Circle(3)\n",
        "pub fn area(s: Shape) -> i64 {\n",
        "  return match s {\n",
        "    Circle(r) => r * r * 3\n",
        "    Square(w) => w * w\n",
        "  }\n",
        "}\n",
    );
    let main = concat!(
        "module main\n",
        "import shapes\n",
        "fn main() -> i32 {\n",
        "  s := shapes.make()\n",
        "  return shapes.area(s) as i32\n",
        "}\n",
    );
    assert_same_verdict("s1b-cross-sum", &[("shapes.align", shapes), ("main.align", main)], "main.align");
}

#[test]
fn cross_module_const_accepts() {
    let cfg = "module cfg\npub LIMIT: i64 := 42\n";
    let main = "module main\nimport cfg\nfn main() -> i32 = cfg.LIMIT as i32\n";
    assert_same_verdict("s1b-cross-const", &[("cfg.align", cfg), ("main.align", main)], "main.align");
}

#[test]
fn diamond_import_accepts() {
    // main imports b and c; both import base. Legal reconvergence; base checked once, first.
    let base = "module base\npub fn one() -> i64 = 1\n";
    let b = "module b\nimport base\npub fn twice() -> i64 = base.one() + base.one()\n";
    let c = "module c\nimport base\npub fn thrice() -> i64 = base.one() + base.one() + base.one()\n";
    let main = concat!(
        "module main\n",
        "import b\n",
        "import c\n",
        "fn main() -> i32 = (b.twice() + c.thrice()) as i32\n",
    );
    let r = assert_same_verdict(
        "s1b-diamond",
        &[("base.align", base), ("b.align", b), ("c.align", c), ("main.align", main)],
        "main.align",
    );
    assert_eq!(r.summaries.len(), 4);
}

#[test]
fn transitive_type_through_two_modules_accepts() {
    // main -> b -> base; b re-exposes a base type through its own pub signature (transitive surface).
    let base = "module base\npub Point { x: i64, y: i64 }\npub fn origin() -> Point = Point { x: 0, y: 0 }\n";
    let b = "module b\nimport base\npub fn start() -> base.Point = base.origin()\n";
    let main = concat!(
        "module main\n",
        "import b\n",
        "import base\n",
        "fn main() -> i32 {\n",
        "  p := b.start()\n",
        "  return p.x as i32\n",
        "}\n",
    );
    assert_same_verdict(
        "s1b-transitive-type",
        &[("base.align", base), ("b.align", b), ("main.align", main)],
        "main.align",
    );
}

// An owned `string` field makes a struct a Move type; a `str` field would be Copy (a view). MoveCheck
// on the consumer is derivable from the imported type definition (fields) the summary carries.

#[test]
fn cross_module_move_type_struct_accepts() {
    let doc = concat!(
        "module doc\n",
        "pub User { name: string, age: i64 }\n",
        "pub fn mk(a: i64) -> User = User { name: \"x\".clone(), age: a }\n",
        "pub fn age_of(u: User) -> i64 = u.age\n",
    );
    let main = concat!(
        "module main\n",
        "import doc\n",
        "fn main() -> i32 {\n",
        "  u := doc.mk(7)\n",
        "  return doc.age_of(u) as i32\n",
        "}\n",
    );
    assert_same_verdict("s1b-move-struct", &[("doc.align", doc), ("main.align", main)], "main.align");
}

#[test]
fn cross_module_move_misuse_rejects() {
    // Use-after-move of an imported Move struct: consumed by `age_of`, then used again — both checkers
    // reject (MoveCheck is a pure function of the imported type definition).
    let doc = concat!(
        "module doc\n",
        "pub User { name: string, age: i64 }\n",
        "pub fn mk(a: i64) -> User = User { name: \"x\".clone(), age: a }\n",
        "pub fn age_of(u: User) -> i64 = u.age\n",
    );
    let main = concat!(
        "module main\n",
        "import doc\n",
        "fn main() -> i32 {\n",
        "  u := doc.mk(7)\n",
        "  a := doc.age_of(u)\n",
        "  b := doc.age_of(u)\n", // u already moved into the first age_of
        "  return (a + b) as i32\n",
        "}\n",
    );
    let r = assert_same_verdict("s1b-move-misuse", &[("doc.align", doc), ("main.align", main)], "main.align");
    assert!(r.diags.has_errors());
}

// ---- Gate 1: negative cases (both must reject) ----------------------------------------------------

#[test]
fn type_mismatch_against_imported_signature_rejects() {
    let geom = "module geom\npub fn square(x: i64) -> i64 = x * x\n";
    // Passing a `str` where the imported signature wants `i64`.
    let main = "module main\nimport geom\nfn main() -> i32 = geom.square(\"nope\") as i32\n";
    let r = assert_same_verdict("s1b-arg-mismatch", &[("geom.align", geom), ("main.align", main)], "main.align");
    assert!(r.diags.has_errors());
}

#[test]
fn arity_mismatch_against_imported_signature_rejects() {
    let geom = "module geom\npub fn square(x: i64) -> i64 = x * x\n";
    let main = "module main\nimport geom\nfn main() -> i32 = geom.square(1, 2) as i32\n";
    assert_same_verdict("s1b-arity", &[("geom.align", geom), ("main.align", main)], "main.align");
}

#[test]
fn calling_a_private_function_rejects() {
    // A private dependency function is absent from the interface: rejected (verdict parity; the
    // message differs — "private" whole-program vs "unknown" per-unit — a principled S1b difference).
    let geom = "module geom\nfn helper(x: i64) -> i64 = x + 1\npub fn pub_one() -> i64 = 1\n";
    let main = "module main\nimport geom\nfn main() -> i32 = geom.helper(5) as i32\n";
    let r = assert_same_verdict("s1b-private", &[("geom.align", geom), ("main.align", main)], "main.align");
    assert!(r.diags.has_errors());
}

#[test]
fn wrong_field_on_imported_type_rejects() {
    let geom = "module geom\npub Point { x: i64, y: i64 }\npub fn origin() -> Point = Point { x: 0, y: 0 }\n";
    let main = concat!(
        "module main\n",
        "import geom\n",
        "fn main() -> i32 {\n",
        "  p := geom.origin()\n",
        "  return p.z as i32\n", // no field `z`
        "}\n",
    );
    assert_same_verdict("s1b-badfield", &[("geom.align", geom), ("main.align", main)], "main.align");
}

#[test]
fn generic_pub_fn_body_referencing_private_item_rejects_in_both() {
    // Gate finding D1: a generic `pub` fn's body ships in the interface summary (it is monomorphized
    // in the importing module), so a reference to a private same-module item was accepted whole-program
    // but rejected per-unit — an accept/reject divergence, plus a synthesized `<interface:gen>` span
    // leaking into the user diagnostic. Now rejected at the PRODUCER in ordinary sema, so both checkers
    // agree at the template's real source span.
    let producer = "module gen\nfn helper(x: i64) -> i64 = x + 1\npub fn bump<T>(x: T, n: i64) -> i64 = helper(n)\n";
    let main = "module main\nimport gen\nfn main() -> i32 = gen.bump(0, 41) as i32\n";
    let files = [("gen.align", producer), ("main.align", main)];
    let r = assert_same_verdict("s1b-generic-body-private", &files, "main.align");
    assert!(r.diags.has_errors());
    // The whole-program message names the private reference at the template's real span — no
    // synthesized interface-file location leaks through.
    let d = check_multi_diagnostics("s1b-generic-body-private-msg", &files, "main.align");
    assert!(
        d.contains("pub generic fn 'bump' references private fn 'helper' in its body"),
        "expected the producer-side body-reference error, got:\n{d}"
    );
    assert!(!d.contains("<interface:"), "no synthesized interface-file span may leak into the diagnostic:\n{d}");
}

#[test]
fn generic_pub_fn_body_over_pub_items_and_qualified_import_accepts() {
    // A generic `pub` template may reference `pub` same-module items and qualified `mod.f` imports —
    // both are part of the interface (or already cross-module `pub`-enforced). Both checkers accept.
    let cfg = "module cfg\npub fn scale(x: i64) -> i64 = x * 2\n";
    let producer = concat!(
        "module gen\n",
        "import cfg\n",
        "pub fn helper(x: i64) -> i64 = x + 1\n",
        "pub fn bump<T>(x: T, n: i64) -> i64 = cfg.scale(helper(n))\n",
    );
    let main = "module main\nimport gen\nfn main() -> i32 = gen.bump(0, 20) as i32\n";
    let r = assert_same_verdict(
        "s1b-generic-body-pub",
        &[("cfg.align", cfg), ("gen.align", producer), ("main.align", main)],
        "main.align",
    );
    assert!(!r.diags.has_errors());
}

// ---- Gate 3: effect bits fail-closed across units -------------------------------------------------

// `par_map` requires a named local function (a qualified `mod.fn` is rejected by both checkers), so a
// cross-unit effect reaches the parallel boundary through a thin local wrapper — which is exactly how
// the imported fn's effect bit must propagate.

#[test]
fn par_map_over_pure_imported_fn_accepts() {
    let geom = "module geom\npub fn dbl(x: i64) -> i64 = x * 2\n";
    let main = concat!(
        "module main\n",
        "import geom\n",
        "fn w(x: i64) -> i64 = geom.dbl(x)\n",
        "fn main() -> Result<(), Error> {\n",
        "  out := [1, 2, 3].par_map(w)\n",
        "  print(out.sum())\n",
        "  return Ok(())\n",
        "}\n",
    );
    let r = assert_same_verdict("s1b-parmap-pure", &[("geom.align", geom), ("main.align", main)], "main.align");
    assert!(!r.diags.has_errors());
}

#[test]
fn par_map_over_impure_imported_fn_rejects() {
    // An imported fn that performs I/O is Impure in its summary; a `par_map` over a wrapper that
    // calls it must fail closed (the impurity crosses the unit boundary via the effect bit).
    let geom = "module geom\npub fn noisy(x: i64) -> i64 {\n  print(x)\n  return x\n}\n";
    let main = concat!(
        "module main\n",
        "import geom\n",
        "fn w(x: i64) -> i64 = geom.noisy(x)\n",
        "fn main() -> Result<(), Error> {\n",
        "  out := [1, 2, 3].par_map(w)\n",
        "  print(out.sum())\n",
        "  return Ok(())\n",
        "}\n",
    );
    let r = assert_same_verdict("s1b-parmap-impure", &[("geom.align", geom), ("main.align", main)], "main.align");
    assert!(r.diags.has_errors());
}

#[test]
fn sequential_impure_imported_call_stays_legal() {
    // Calling an impure imported fn sequentially (not under par_map) is fine in both checkers.
    let geom = "module geom\npub fn noisy(x: i64) -> i64 {\n  print(x)\n  return x\n}\n";
    let main = "module main\nimport geom\nfn main() -> i32 = geom.noisy(7) as i32\n";
    let r = assert_same_verdict("s1b-seq-impure", &[("geom.align", geom), ("main.align", main)], "main.align");
    assert!(!r.diags.has_errors());
}

// ---- Gate 4: generics across units ---------------------------------------------------------------

#[test]
fn cross_unit_generic_fn_instantiated_at_multiple_types() {
    // `gen.id<T>` instantiated at i64 and str in the consumer; gen itself never instantiates it.
    let genmod = "module gen\npub fn id<T>(x: T) -> T = x\n";
    let main = concat!(
        "module main\n",
        "import gen\n",
        "fn main() -> i32 {\n",
        "  a := gen.id(5)\n",
        "  s := gen.id(\"hi\")\n",
        "  return (a + s.len()) as i32\n",
        "}\n",
    );
    let r = assert_same_verdict("s1b-generic-multi", &[("gen.align", genmod), ("main.align", main)], "main.align");
    assert!(!r.diags.has_errors());
}

#[test]
fn cross_unit_generic_struct_instantiated() {
    let genmod = "module gen\npub Pair<T> { a: T, b: T }\npub fn mk<T>(x: T, y: T) -> Pair<T> = Pair { a: x, b: y }\n";
    let main = concat!(
        "module main\n",
        "import gen\n",
        "fn main() -> i32 {\n",
        "  p := gen.mk(3, 4)\n",
        "  return (p.a + p.b) as i32\n",
        "}\n",
    );
    assert_same_verdict("s1b-generic-struct", &[("gen.align", genmod), ("main.align", main)], "main.align");
}

#[test]
fn impure_generic_body_rejected_under_par_map_after_instantiation() {
    // A generic imported fn whose body performs I/O: its consumer-side monomorph recomputes the
    // effect from the instantiated body, so a par_map over a wrapper that calls it fails closed.
    let genmod = "module gen\npub fn logid<T>(x: T) -> T {\n  print(1) else {}\n  return x\n}\n";
    let main = concat!(
        "module main\n",
        "import gen\n",
        "fn wrap(x: i64) -> i64 = gen.logid(x)\n",
        "fn main() -> Result<(), Error> {\n",
        "  out := [1, 2, 3].par_map(wrap)\n",
        "  print(out.sum())\n",
        "  return Ok(())\n",
        "}\n",
    );
    let r = assert_same_verdict("s1b-generic-impure", &[("gen.align", genmod), ("main.align", main)], "main.align");
    assert!(r.diags.has_errors());
}

// ---- Gate 5: transitive interface-hash dependency set --------------------------------------------

fn dep_hashes_for<'a>(r: &'a PerUnitCheck, unit: &str) -> &'a [(String, Hash128)] {
    &r.dep_interface_hashes.iter().find(|(u, _)| u == unit).expect("unit present").1
}

#[test]
fn transitive_pub_change_changes_dependent_hash_set_private_change_does_not() {
    // main -> B -> C. C's interface hash is what B and (transitively) main key on. A private-body
    // edit of C, AND a body-only edit of C's non-generic `pub` fn (a fn body is IMPLEMENTATION, not
    // interface — only its signature + effect bit are), both leave C's interface hash — and thus
    // main's transitive dep set — identical; a pub SIGNATURE change to C changes it.
    let c_v1 = "module c\npub fn base(x: i64) -> i64 = x + 1\nfn helper() -> i64 = 99\n";
    let c_priv = "module c\npub fn base(x: i64) -> i64 = x + 1\nfn helper() -> i64 = 12345\n"; // private body only
    let c_pub_body = "module c\npub fn base(x: i64) -> i64 = x + 100\nfn helper() -> i64 = 99\n"; // pub fn BODY only, same signature + effect
    let c_pub_sig = "module c\npub fn base(x: i64, y: i64) -> i64 = x + y\nfn helper() -> i64 = 99\n"; // pub SIGNATURE change
    let b = "module b\nimport c\npub fn mid(x: i64) -> i64 = c.base(x)\n";
    let main = "module main\nimport b\nfn main() -> i32 = b.mid(1) as i32\n";

    let r1 = check_per_unit_multi("s1b-tr-v1", &[("c.align", c_v1), ("b.align", b), ("main.align", main)], "main.align");
    let rp = check_per_unit_multi("s1b-tr-priv", &[("c.align", c_priv), ("b.align", b), ("main.align", main)], "main.align");
    let rb = check_per_unit_multi("s1b-tr-pubbody", &[("c.align", c_pub_body), ("b.align", b), ("main.align", main)], "main.align");
    let rs = check_per_unit_multi("s1b-tr-sig", &[("c.align", c_pub_sig), ("b.align", b), ("main.align", main)], "main.align");

    // main's transitive dep set must include both b and c.
    let base_set = dep_hashes_for(&r1, "main");
    assert!(base_set.iter().any(|(u, _)| u == "b") && base_set.iter().any(|(u, _)| u == "c"));

    let hash_of = |set: &[(String, Hash128)], unit: &str| {
        set.iter().find(|(u, _)| u == unit).map(|(_, h)| *h).expect("dep present")
    };

    // Private body edit of C: C's interface hash unchanged → main's transitive set unchanged.
    assert_eq!(hash_of(base_set, "c"), hash_of(dep_hashes_for(&rp, "main"), "c"), "private-body edit must not change C's interface hash");
    assert_eq!(hash_of(base_set, "b"), hash_of(dep_hashes_for(&rp, "main"), "b"), "B unaffected by C's private edit");

    // Body-only edit of C's non-generic pub fn (same signature + effect): interface hash unchanged.
    assert_eq!(hash_of(base_set, "c"), hash_of(dep_hashes_for(&rb, "main"), "c"), "a non-generic pub fn body is implementation, not interface");

    // Pub SIGNATURE change of C: C's interface hash changes → main's transitive set changes at C.
    assert_ne!(hash_of(base_set, "c"), hash_of(dep_hashes_for(&rs, "main"), "c"), "pub signature change must change C's interface hash");
}

// ---- Gate 2: blindness (dep private body invisible to a dependent's check) ------------------------

#[test]
fn dependent_check_is_blind_to_a_dependency_private_body() {
    // Change only geom's private helper body: main's verdict, main's own interface hash, and main's
    // transitive dep hash set must all be identical — the dependent never reads the dependency AST.
    let geom_a = "module geom\nfn secret() -> i64 = 1\npub fn pub_v(x: i64) -> i64 = x + secret()\n";
    let geom_b = "module geom\nfn secret() -> i64 = 999999\npub fn pub_v(x: i64) -> i64 = x + secret()\n";
    let main = "module main\nimport geom\nfn main() -> i32 = geom.pub_v(2) as i32\n";

    let ra = check_per_unit_multi("s1b-blind-a", &[("geom.align", geom_a), ("main.align", main)], "main.align");
    let rb = check_per_unit_multi("s1b-blind-b", &[("geom.align", geom_b), ("main.align", main)], "main.align");

    assert!(!ra.diags.has_errors() && !rb.diags.has_errors());

    let main_a = ra.summaries.iter().find(|s| s.unit == "main").expect("main summary");
    let main_b = rb.summaries.iter().find(|s| s.unit == "main").expect("main summary");
    assert_eq!(main_a.interface_hash, main_b.interface_hash, "main's interface hash must not depend on geom's private body");

    let geom_a_h = dep_hashes_for(&ra, "main").iter().find(|(u, _)| u == "geom").map(|(_, h)| *h);
    let geom_b_h = dep_hashes_for(&rb, "main").iter().find(|(u, _)| u == "geom").map(|(_, h)| *h);
    assert_eq!(geom_a_h, geom_b_h, "geom's interface hash (seen by main) must not depend on geom's private body");
}
