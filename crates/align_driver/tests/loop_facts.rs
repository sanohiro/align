//! Loop facts: the borrowed view header, its alias facts, and the length fact.
//!
//! The owner for plan 69 PR 1 (`docs/impl/69-loop-facts-plan.md` §2), which implements plan 68's G1
//! and the `!range` half of G2's proof surface (issues 1079 and 1080). Every assertion here is on
//! **emitted (raw) LLVM IR** at `BuildTarget::Baseline`, so it pins the promise itself — the
//! metadata names, the parameter attribute text, one header materialization per borrowed view, and
//! `!range` on the length load — and never a vector width. That makes this suite arch-neutral: it
//! runs identically on x86-64 and on aarch64, which is the host every measurement in 1079/1080 was
//! taken on. Vector widths are a target property and stay in `vectorize_shapes.rs`, which pins its
//! CPU tier; acceptance here is IR shape, not vectorization.
//!
//! The facts, and why each is sound:
//!
//!   * **one materialization.** A borrowed `slice`/`array` parameter is a pointer to its
//!     `{ptr,len}` header, so the header is loaded once in the entry block — which dominates every
//!     use — instead of at every use. Sound only while nothing in the body can write header memory,
//!     which is what the whole-body proof in codegen's `view_facts_plan` establishes; it fails
//!     closed, and the negative owners below pin that.
//!   * **`!tbaa`.** `align.view.header` and `align.elem` are siblings under one root, so a header
//!     access and an element access are proven not to alias. There is exactly one element class for
//!     every element type, so two element accesses still may alias.
//!   * **`noalias`.** Only on a read-only `borrow` header, and only when the body writes no header
//!     at all. A `borrow mut` header has no `readonly` companion claim, so `noalias` on it would
//!     turn an aliasing pair of arguments into undefined behaviour instead of a diagnostic.
//!   * **`!range !{i64 0, i64 -9223372036854775808}`.** A length is non-negative by construction.
//!     It is attached to the length *load*; LLVM accepts `!range` on a load and not on an
//!     `extractvalue`, so a by-value header carries no range fact — pinned negatively below.

mod common;
use common::*;

/// One function's emitted IR, from its `define` line through its closing brace.
fn function_ir(ir: &str, name: &str) -> String {
    let needle = format!(" @{name}(");
    let start = ir
        .lines()
        .position(|line| line.starts_with("define") && line.contains(&needle))
        .unwrap_or_else(|| panic!("function `{name}` is not in the emitted IR:\n{ir}"));
    ir.lines()
        .skip(start)
        .take_while(|line| *line != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

/// The `define` line of one function (its signature, with every parameter attribute).
fn signature(ir: &str, name: &str) -> String {
    function_ir(ir, name)
        .lines()
        .next()
        .expect("a function body starts with its define line")
        .to_string()
}

/// Everything after a function's entry block — the part a per-iteration reload would show up in.
fn after_entry(ir: &str, name: &str) -> String {
    let body = function_ir(ir, name);
    let (_, rest) = body
        .split_once("\n\nbb")
        .unwrap_or_else(|| panic!("function `{name}` has no second block:\n{body}"));
    rest.to_string()
}

const SCALE_ONLY: &str = "\
fn scale_only(borrow mut out: array<f32>, k: f32) {
  mut i := 0
  loop {
    if i >= out.len() { break }
    out[i] = out[i] * k
    i = i + 1
  }
}
";

const DOT: &str = "\
fn dot(borrow a: slice<f32>, borrow b: slice<f32>) -> f32 {
  mut i := 0
  mut total := 0.0 as f32
  loop {
    if i >= a.len() { break }
    total = total + a[i] * b[i]
    i = i + 1
  }
  return total
}
";

/// G1's shape promise: one header materialization per borrowed view, in the entry block, and no
/// reload anywhere in the loop. This is the fact 1079 measured as missing — the data pointer and
/// the length were both re-read every iteration because an element store could alias the header.
#[test]
fn g1_view_header_hoisted() {
    let ir = emit_llvm_with_exports(&format!("{SCALE_ONLY}{DOT}"), &["scale_only", "dot"]);
    for (name, views) in [("scale_only", 1usize), ("dot", 2usize)] {
        let body = function_ir(&ir, name);
        assert_eq!(
            body.matches("load ptr, ptr %view.ptr").count(),
            views,
            "[{name}] exactly one data-pointer materialization per borrowed view:\n{body}"
        );
        assert_eq!(
            body.matches("load i64, ptr %view.len").count(),
            views,
            "[{name}] exactly one length materialization per borrowed view:\n{body}"
        );
        let loop_body = after_entry(&ir, name);
        assert!(
            !loop_body.contains("load { ptr, i64 }"),
            "[{name}] the whole header must not be reloaded outside the entry block:\n{loop_body}"
        );
        assert!(
            !loop_body.contains("load ptr, ptr %view") && !loop_body.contains("load i64, ptr %view"),
            "[{name}] every header field load belongs to the entry block:\n{loop_body}"
        );
    }
}

/// The header parameter attributes. `noalias` lands on a read-only `borrow` header whose body
/// writes no header; `dereferenceable(16) align 8` is the header's own extent, never the buffer's.
#[test]
fn g1_borrow_header_attributes() {
    let ir = emit_llvm_with_exports(DOT, &["dot"]);
    let define = signature(&ir, "dot");
    for attribute in [
        "noalias",
        "nonnull",
        "readonly",
        "captures(none)",
        "dereferenceable(16)",
        "align 8",
    ] {
        assert_eq!(
            define.matches(attribute).count(),
            2,
            "both `borrow` headers carry `{attribute}`:\n{define}"
        );
    }
}

/// The TBAA pair: two classes under one Align root, a header access tagged with one and an element
/// access with the other. One element class for every element type, so element-versus-element
/// aliasing is unchanged.
#[test]
fn g1_tbaa_header_element_split() {
    let ir = emit_llvm_with_exports(DOT, &["dot"]);
    assert!(
        ir.contains("!{!\"align.tbaa.root\"}"),
        "the Align TBAA root must be emitted:\n{ir}"
    );
    assert_eq!(
        ir.matches("!\"align.view.header\"").count(),
        1,
        "exactly one header type node:\n{ir}"
    );
    assert_eq!(
        ir.matches("!\"align.elem\"").count(),
        1,
        "exactly one element type node — a per-element-type node would claim that two element \
         accesses cannot alias:\n{ir}"
    );
    let body = function_ir(&ir, "dot");
    let header_tag = body
        .lines()
        .find(|line| line.contains("load i64, ptr %view.len"))
        .and_then(|line| line.rsplit("!tbaa ").next().map(str::to_string))
        .expect("the length load carries a tbaa tag");
    let element_tag = body
        .lines()
        .find(|line| line.contains("load float, ptr %slcidx"))
        .and_then(|line| line.rsplit("!tbaa ").next().map(str::to_string))
        .expect("the element load carries a tbaa tag");
    assert_ne!(
        header_tag, element_tag,
        "the header access and the element access must carry different tags:\n{body}"
    );
}

/// I4: an element whose storage can itself hold a view header claims nothing. `slice<str>` elements
/// *are* headers, so the element read carries no tag while the view's own header still does.
#[test]
fn g1_tbaa_absent_for_header_bearing_elements() {
    let source = "fn first(borrow rows: slice<str>) -> i64 = rows[0].len()\n";
    let ir = emit_llvm_with_exports(source, &["first"]);
    let body = function_ir(&ir, "first");
    assert!(
        body.contains("load i64, ptr %view.len") && body.contains("!tbaa"),
        "the view's own header access is still tagged:\n{body}"
    );
    let element = body
        .lines()
        .find(|line| line.contains("%slcidx"))
        .unwrap_or_else(|| panic!("no element access in:\n{body}"));
    assert!(
        !element.contains("!tbaa"),
        "a `str` element is header-bearing storage and must claim nothing:\n{element}"
    );
}

/// I3: a body that writes a view header anywhere loses `noalias` on *every* header parameter, and
/// loses the entry-block materialization with it. Both shapes below compile today: a borrowed
/// replacement through a `borrow mut` binding, and an owned struct field replacement.
#[test]
fn g1_noalias_absent_when_header_written() {
    let source = "\
Row { xs: slice<i64>, n: i64 }
fn replace_view(borrow mut cur: slice<i64>, next: slice<i64>) {
  cur = next
}
fn set_field(src: slice<i64>, other: slice<i64>) -> i64 {
  mut r := Row { xs: src, n: 1 }
  r.xs = other
  return r.n
}
fn observe(borrow xs: slice<i64>, borrow mut cur: slice<i64>) -> i64 {
  cur = xs
  return xs.len()
}
";
    let ir = emit_llvm_with_exports(source, &["replace_view", "set_field", "observe"]);
    for name in ["replace_view", "set_field", "observe"] {
        let body = function_ir(&ir, name);
        // Positive control: each body really does read a `{ptr,len}` header, per use — so the
        // absences below are the gate refusing, not a function with nothing to state facts about.
        assert!(
            body.contains("{ ptr, i64 }"),
            "[{name}] the fixture must actually handle a view header:\n{body}"
        );
        assert!(
            !body.contains("noalias"),
            "[{name}] a body that writes a header states no `noalias`:\n{body}"
        );
        assert!(
            !body.contains("!tbaa"),
            "[{name}] a body that writes a header states no alias class:\n{body}"
        );
        assert!(
            !body.contains("%view.len"),
            "[{name}] a header that may be written is not materialized once:\n{body}"
        );
    }
    // The read-only header of `observe` is a `borrow` — it would have carried `noalias` had the
    // body not replaced the *other* view's header. That is the whole-function gate, on purpose.
    let define = signature(&ir, "observe");
    assert!(
        define.contains("readonly"),
        "the read-only header keeps its ordinary contracts:\n{define}"
    );
}

/// `noalias` is never stated for a writable header, and `readonly` never is either. The structural
/// facts (`nonnull dereferenceable(16) align 8`) are stated for both borrowed modes — 1079 §1
/// records that a `borrow mut` header carried no parameter attribute at all.
#[test]
fn g1_noalias_absent_on_mutable_headers() {
    let source = "fn observe_mut(borrow mut xs: slice<i64>) -> i64 = xs.len()\n";
    let ir = emit_llvm_with_exports(source, &["observe_mut"]);
    let define = signature(&ir, "observe_mut");
    for attribute in ["nonnull", "dereferenceable(16)", "align 8"] {
        assert!(
            define.contains(attribute),
            "a `borrow mut` header states `{attribute}`:\n{define}"
        );
    }
    assert!(
        !define.contains("noalias"),
        "a writable header never states `noalias`:\n{define}"
    );
    assert!(
        !define.contains("readonly"),
        "a writable header is not read-only:\n{define}"
    );
}

/// I5: a function that forms a foreign pointer states no alias class at all. Provenance cannot be
/// tracked per access from inside one function, so the proof is whole-body and fails closed.
#[test]
fn g1_no_tbaa_on_foreign_pointers() {
    let source = "\
fn touch(borrow xs: slice<i64>) -> i64 {
  mut total := xs[0]
  unsafe {
    p := raw.alloc(64)
    raw.free(p)
  }
  return total
}
";
    let ir = emit_llvm_with_exports(source, &["touch"]);
    let body = function_ir(&ir, "touch");
    // Positive control: the body really does perform a view element access, so the absence below
    // is the gate refusing rather than an empty function.
    assert!(
        body.contains("%slcidx"),
        "the fixture must actually index a borrowed view:\n{body}"
    );
    assert!(
        !body.contains("!tbaa"),
        "a body reaching `unsafe raw` states no alias class:\n{body}"
    );
    assert!(
        !signature(&ir, "touch").contains("noalias"),
        "a body reaching `unsafe raw` states no `noalias`:\n{body}"
    );
}

/// 1080: every length that is materialized by a load says it is non-negative. The by-value header
/// path produces its length with `extractvalue`, which LLVM does not let carry `!range`, and a
/// fixed array's length is a constant where the fact is a no-op — both pinned negatively, so the
/// absence is a recorded decision rather than an oversight.
#[test]
fn g1_len_range_on_borrowed_and_fixed_lengths() {
    let borrowed = emit_llvm_with_exports(
        "fn borrowed_len(borrow xs: slice<i64>) -> i64 = xs.len()\n\
         fn borrowed_array_len(borrow xs: array<i64>) -> i64 = xs.len()\n",
        &["borrowed_len", "borrowed_array_len"],
    );
    for name in ["borrowed_len", "borrowed_array_len"] {
        let body = function_ir(&borrowed, name);
        let length = body
            .lines()
            .find(|line| line.contains("load i64, ptr %view.len"))
            .unwrap_or_else(|| panic!("[{name}] no length load in:\n{body}"));
        assert!(
            length.contains("!range !"),
            "[{name}] a length load states its non-negativity:\n{length}"
        );
    }
    assert!(
        borrowed.contains("!{i64 0, i64 -9223372036854775808}"),
        "the range is exactly the half-open `[0, 2^63)`:\n{borrowed}"
    );

    let by_value = emit_llvm_with_exports("fn value_len(xs: slice<i64>) -> i64 = xs.len()\n", &["value_len"]);
    let body = function_ir(&by_value, "value_len");
    assert!(
        body.contains("extractvalue"),
        "a by-value header produces its length with `extractvalue`:\n{body}"
    );
    assert!(
        !body.contains("!range"),
        "`extractvalue` cannot carry `!range`; no other mechanism is used here:\n{body}"
    );

    let fixed = emit_llvm_with_exports("fn fixed_len() -> i64 { xs := [1, 2, 3]\n  return xs[1] }\n", &["fixed_len"]);
    let body = function_ir(&fixed, "fixed_len");
    assert!(
        !body.contains("!range"),
        "a fixed array's length is a constant, so the fact is a no-op:\n{body}"
    );
}

/// The non-negativity claim is about a *length*, not about the `{ptr,i64}` layout. Several types
/// share that layout and give the second field another meaning entirely: a `json.doc` is
/// `{tape, node}`, and its node index is `-1` for Missing. Claiming `!range !{i64 0, i64 …}` there
/// would turn a valid Missing handle into poison, so the header facts a `json.doc` parameter does
/// get — the materialization, the header alias class, `dereferenceable`/`align` — must arrive
/// *without* the length fact.
#[test]
fn g1_len_range_is_not_stated_for_a_non_length_header_field() {
    let source = "fn identity(borrow d: json.doc) -> json.doc = d\n";
    let ir = emit_llvm_with_exports(source, &["identity"]);
    let body = function_ir(&ir, "identity");
    // Positive control: the header really is materialized here, so the absence below is the length
    // predicate refusing rather than the whole fact set being off.
    assert!(
        body.contains("load i64, ptr %view.len"),
        "the fixture must materialize a `{{ptr,i64}}` header:\n{body}"
    );
    assert!(
        !body.contains("!range"),
        "a node index is not a length — `-1` is a valid Missing handle:\n{body}"
    );
    // The layout-only facts are unaffected: the parameter is still a 16-byte, 8-aligned header.
    let define = signature(&ir, "identity");
    assert!(
        define.contains("dereferenceable(16)") && define.contains("align 8"),
        "the layout facts do not depend on the second field's meaning:\n{define}"
    );
}

/// Two views of the same buffer stay correct. `noalias` lands only on read-only headers of bodies
/// that write no header, so passing one place as two `borrow` arguments — and as a
/// `borrow`/`borrow mut` pair — cannot be made unsound by it. This owner is executable, not
/// IR-shape: it runs the aliasing calls and checks the values.
#[test]
fn g1_two_views_of_one_buffer_stay_correct() {
    if !cc_available() {
        return;
    }
    let source = "\
fn sum_both(borrow a: slice<i64>, borrow b: slice<i64>) -> i64 {
  mut i := 0
  mut total := 0
  loop {
    if i >= a.len() { break }
    total = total + a[i] + b[i]
    i = i + 1
  }
  return total
}
fn main() -> i32 {
  mut data := [1, 2, 3, 4]
  xs : slice<i64> := data[0..4]
  print(sum_both(xs, xs))
  return 0
}
";
    let out = build_and_run("loop-facts-aliasing-views", source);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "20\n", "two aliasing read-only views keep their values:\n{stdout}");
}

/// The other half of the same cell: an aliasing `borrow`/`borrow mut` pair never reaches codegen at
/// all — sema rejects it — so `noalias` is not what makes that shape sound, and widening it to a
/// writable header would buy nothing while costing the diagnostic.
#[test]
fn g1_aliasing_exclusive_view_pair_is_still_rejected() {
    let source = "\
fn sum_len(borrow a: slice<i64>, borrow mut b: slice<i64>) -> i64 = a.len() + b.len()
fn main() -> i32 {
  mut data := [1, 2, 3, 4]
  mut xs : slice<i64> := data[0..4]
  print(sum_len(xs, xs))
  return 0
}
";
    let diagnostics = check_diagnostics("loop-facts-aliasing-exclusive-pair", source);
    assert!(
        diagnostics.contains("aliases argument"),
        "the existing sema gate still rejects an aliasing exclusive pair:\n{diagnostics}"
    );
}
