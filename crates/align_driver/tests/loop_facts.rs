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

// ── G2, plan 69 PR 2 (issue 1081) ───────────────────────────────────────────────────────────────
//
// Two changes, one capability. **Fusion** (§3.1) collapses each emitted guard to its minimal
// unsigned form: one `Ge` on `u64` for an element index, two unsigned compares and an `Or` for a
// range. **Versioning** (§3.2) proves, once in a new preheader, that every guarded access of a
// monotone-index loop is in range for every iteration it will run, and selects a fast copy whose
// proved guards are bypassed — or the original slow copy, which keeps every one of them.
//
// Nothing is deleted. The slow copy is byte-identical to what lowering emitted, so a failing run
// traps at the same iteration, with the same `(index, len)`, after the same observable prefix.
// That is why versioning, not relocation, is the shape: `scan_report` below prints `0` and *then*
// traps, exactly as it did before this pass existed.
//
// These owners are arch-neutral: MIR text, `explain-opt` reason codes, and executables. Vector
// widths stay in `vectorize_shapes.rs`, which names its target tier.

use align_mir::loop_facts::LOOP_FACTS_VERSION_BUDGET;

/// Whole-program MIR text for `src`.
fn mir_text(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}

/// The blocks of one MIR function that hold a **guard**: their terminator branches to a block whose
/// only statement is a bounds/range trap. Fusion assertions belong here and not on the whole
/// function, because the preheader's own admission arithmetic legitimately compares against `0`.
fn guard_blocks(body: &str) -> Vec<String> {
    let blocks: Vec<&str> = body.split("  bb").skip(1).collect();
    let trap_labels: Vec<String> = blocks
        .iter()
        .filter(|block| block.contains("bounds_fail") || block.contains("range_fail"))
        .map(|block| block.split(':').next().expect("a block is labelled").to_string())
        .collect();
    assert!(!trap_labels.is_empty(), "no trap block in:\n{body}");
    blocks
        .iter()
        .filter(|block| {
            block.lines().any(|line| {
                line.trim_start().starts_with("branch")
                    && trap_labels
                        .iter()
                        .any(|label| line.contains(&format!("? bb{label} ")))
            })
        })
        .map(|block| format!("  bb{block}"))
        .collect()
}

/// One function's MIR text, from its `fn` line to its closing brace.
fn mir_fn(mir: &str, name: &str) -> String {
    let needle = format!("fn {name}(");
    let start = if mir.starts_with(&needle) {
        0
    } else {
        mir.find(&format!("\n{needle}"))
            .unwrap_or_else(|| panic!("function `{name}` is not in the MIR:\n{mir}"))
            + 1
    };
    let rest = &mir[start..];
    let end = rest.find("\n}").expect("a MIR function closes") + 2;
    rest[..end].to_string()
}

/// The `explain-opt` decision lines (§3.2.3) for one program, in report order.
fn loop_facts_report(name: &str, src: &str) -> Vec<String> {
    let path = std::env::temp_dir().join(format!("align-lf-{}-{name}.align", std::process::id()));
    std::fs::write(&path, src).expect("write fixture");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
        .env("ALIGNC_CACHE", "off")
        .args(["explain-opt", path.to_str().expect("utf-8 path")])
        .output()
        .expect("run explain-opt");
    let _ = std::fs::remove_file(&path);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "explain-opt must succeed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
        .lines()
        .filter(|line| line.starts_with("loop facts "))
        .map(str::to_string)
        .collect()
}

/// The decision one named function's first loop received, with the `loop ` prefix stripped.
fn decision(report: &[String], function: &str) -> String {
    let prefix = format!("loop facts `{function}` #1: loop ");
    report
        .iter()
        .find_map(|line| line.strip_prefix(&prefix).map(str::to_string))
        .unwrap_or_else(|| panic!("no decision for `{function}`:\n{report:#?}"))
}

const SUM: &str = "\
fn sum(borrow xs: slice<i64>) -> i64 {
  mut total := 0
  mut i := 0
  loop {
    if i >= xs.len() { break }
    total = total + xs[i]
    i = i + 1
  }
  return total
}
";

const SCAN_REPORT: &str = "\
fn scan_report(borrow xs: slice<i64>, off: i64) -> i64 {
  mut total := 0
  mut i := 0
  loop {
    if i >= xs.len() { break }
    print(i)
    total = total + xs[i + off]
    i = i + 1
  }
  return total
}
";

const SHIFTED_SUM: &str = "\
fn shifted_sum(borrow xs: slice<i64>) -> i64 {
  mut total := 0
  mut i := 0
  loop {
    if i >= xs.len() { break }
    i = i + 1
    total = total + xs[i]
  }
  return total
}
";

const STRIDE_SUM: &str = "\
fn stride_sum(borrow xs: slice<i64>) -> i64 {
  mut total := 0
  mut i := 0
  loop {
    if i >= xs.len() { break }
    total = total + xs[i * 2]
    i = i + 1
  }
  return total
}
";

const MATVEC: &str = "\
fn matvec(borrow w: slice<i64>, borrow x: slice<i64>, d: i64, r: i64) -> i64 {
  mut acc := 0
  mut c := 0
  loop {
    if c >= d { break }
    acc = acc + w[r * d + c] * x[c]
    c = c + 1
  }
  return acc
}
";

const WINDOW_FIRST: &str = "\
fn window_first(borrow xs: slice<i64>, k: i64) -> i64 {
  mut seen := 0
  mut i := 0
  loop {
    if i >= xs.len() { break }
    w := xs[i..i + k]
    seen = seen + w.len()
    i = i + 1
  }
  return seen
}
";

const SKIP_ZEROS: &str = "\
fn skip_zeros(borrow xs: slice<i64>) -> i64 {
  mut i := 0
  mut seen := 0
  loop {
    if i >= xs.len() { break }
    seen = seen + xs[i]
    if xs[i] == 0 { i = i + 2 } else { i = i + 1 }
  }
  return seen
}
";

const DRAIN: &str = "\
fn drain(borrow mut data: array<i64>) -> i64 {
  mut sum := 0
  mut i := 0
  loop {
    if i >= data.len() { break }
    sum = sum + data[i]
    if i == 1 { data.truncate(2) }
    i = i + 1
  }
  return sum
}
";

const TOTAL_INSPECT: &str = "\
Record { value: string }
fn inspect(borrow record: Record) -> i64 = record.value.len()
fn total() -> i64 {
  mut b: array_builder<Record> := array_builder()
  b.push(Record { value: \"align\".clone() })
  b.push(Record { value: \"lang\".clone() })
  rows := b.build()
  mut sum := 0
  mut i := 0
  loop {
    if i >= rows.len() { break }
    sum = sum + inspect(rows[i])
    i = i + 1
  }
  return sum
}
";

const BUILD_ARRAY: &str = "\
fn build(n: i64) -> array<i64> {
  mut b: array_builder<i64> := array_builder()
  mut i := 0
  loop {
    if i >= n { break }
    b.push(i * 10)
    i = i + 1
  }
  return b.build()
}
";

// ── Fusion (§3.1) ───────────────────────────────────────────────────────────────────────────────

/// An element guard is one unsigned compare. `(idx < 0) || (idx >= len)` is exact only because the
/// length is non-negative, and under that precondition `(u64)idx >= (u64)len` decides it alone: a
/// negative index reinterprets as a `u64` above every non-negative length. The trap keeps the
/// original **signed** operands, so the reported index is still `-1` and not `2^64 - 1`.
#[test]
fn g2_element_guard_is_one_unsigned_compare() {
    let body = mir_fn(&mir_text("g2-fuse-element", SUM), "sum");
    let guards = guard_blocks(&body);
    assert_eq!(guards.len(), 1, "the slow copy holds the one live guard:\n{body}");
    let guard = &guards[0];
    assert_eq!(
        guard.matches("as u64 (from i64)").count(),
        2,
        "one index cast and one length cast, and nothing else:\n{guard}"
    );
    assert!(
        !guard.contains("< 0_i64") && !guard.contains("||"),
        "the signed negative-index arm and its `Or` are both gone:\n{guard}"
    );
    assert!(
        guard.contains(" >= "),
        "what is left is one unsigned `>=`:\n{guard}"
    );
    let traps: Vec<&str> = body
        .lines()
        .filter(|line| line.contains("call runtime bounds_fail"))
        .collect();
    assert_eq!(traps.len(), 1, "exactly one trap action survives:\n{body}");
    assert!(
        !traps[0].contains("u64"),
        "the trap reports the original signed index and length:\n{body}"
    );
}

/// A range guard is two unsigned compares and one `Or`. There is no single unsigned predicate over
/// three operands that reproduces `start < 0 || start > end || end > len`, and claiming one would
/// be unsound — so the minimal form is stated as two, not one.
#[test]
fn g2_range_guard_is_two_unsigned_compares() {
    let body = mir_fn(&mir_text("g2-fuse-range", WINDOW_FIRST), "window_first");
    let guards = guard_blocks(&body);
    assert_eq!(guards.len(), 1, "the slow copy holds the one live guard:\n{body}");
    let guard = &guards[0];
    assert!(
        !guard.contains("< 0_i64"),
        "the signed negative-start arm is gone from a range guard:\n{guard}"
    );
    assert_eq!(
        guard.matches("as u64 (from i64)").count(),
        3,
        "start, end and len are each cast once:\n{guard}"
    );
    assert_eq!(
        guard.matches(" > ").count(),
        2,
        "exactly two unsigned compares — there is no single predicate over three operands:\n{guard}"
    );
    assert_eq!(guard.matches("||").count(), 1, "joined by one `Or`:\n{guard}");
    assert!(
        body.contains("call runtime range_fail"),
        "the range trap and its three signed operands are unchanged:\n{body}"
    );
}

/// The one element guard that keeps its signed form. `lower_borrowed_place` publishes its guard as
/// a `BorrowedElementGuard`, and codegen's `checked_borrowed_element_guard` re-derives
/// `Or(Lt(index, 0), Ge(index, len))` **literally** before it forms the element pointer. Fusing it
/// would fail that second MIR-to-codegen safety contract, so §3.1 exempts it by name — and §3.6
/// owns lifting the exemption. The fused form must still appear elsewhere in the same program, or
/// this test would also pass if fusion had simply been reverted.
#[test]
fn g2_borrowed_element_guard_keeps_its_signed_form() {
    let program = format!("{TOTAL_INSPECT}{SUM}");
    let mir = mir_text("g2-borrowed-element-signed", &program);
    let borrowed = guard_blocks(&mir_fn(&mir, "total")).join("\n");
    assert!(
        borrowed.contains("< 0_i64") && borrowed.contains("||"),
        "the borrowed-element guard keeps its three-way signed predicate:\n{borrowed}"
    );
    assert!(
        !borrowed.contains("as u64 (from i64)"),
        "and is not fused:\n{borrowed}"
    );
    let fused = mir_fn(&mir, "sum");
    assert!(
        fused.contains("as u64 (from i64)"),
        "while every other emitter site in the same program fuses:\n{fused}"
    );
}

/// The §3.7 deviation, pinned. A byte accessor's range guard also keeps the signed form, for the
/// same class of reason: `byte_ranges::simplify` re-derives `Or(Or(Lt(start, 0), Gt(start, end)),
/// Gt(end, len))` literally before it may prove a byte recurrence safe. Fusing it would not be
/// unsound, but it would silently disable plan 64's proof — which `runway_a2_binary_codec` owns.
#[test]
fn g2_byte_accessor_guard_keeps_its_signed_form() {
    let src = "\
fn read_at(borrow b: slice<u8>, off: i64) -> u32 = b.u32_le(off)
";
    let body = mir_fn(&mir_text("g2-byte-signed", src), "read_at");
    let guard = guard_blocks(&body).join("\n");
    assert!(
        guard.contains("< 0_i64") && body.contains("call runtime range_fail"),
        "a byte accessor keeps the signed three-way range predicate:\n{body}"
    );
    assert!(
        !guard.contains("as u64 (from i64)"),
        "a byte accessor guard is not fused:\n{guard}"
    );
}

// ── Versioning shape (§3.2) ─────────────────────────────────────────────────────────────────────

/// The shape itself: one source loop becomes a preheader, a fast copy and a slow copy. The fast
/// copy reaches its element load with no branch on the guard; the slow copy keeps the branch and
/// the trap. The count of trap actions in the whole function is what proves nothing was deleted.
#[test]
fn g2_versioned_loop_has_a_preheader_and_two_copies() {
    let body = mir_fn(&mir_text("g2-versioned-shape", SUM), "sum");
    let reported = decision(
        &loop_facts_report("versioned-shape", &format!("{SUM}fn main() {{ }}\n")),
        "sum",
    );
    let used: usize = reported
        .strip_prefix("versioned ")
        .and_then(|rest| rest.split('/').next())
        .and_then(|used| used.parse().ok())
        .unwrap_or_else(|| panic!("the report names the budget use: `{reported}`"));
    assert!(
        (1..=LOOP_FACTS_VERSION_BUDGET).contains(&used),
        "the loop is versioned within budget, and the report says how much it spent: `{reported}`"
    );
    assert!(
        reported.ends_with(&format!("/{LOOP_FACTS_VERSION_BUDGET}")),
        "the report names the budget it was measured against: `{reported}`"
    );
    assert_eq!(
        body.matches("call runtime bounds_fail").count(),
        1,
        "versioning moves a guard and never deletes one, so the trap call-site count is unchanged:\n{body}"
    );
    // Both copies compute the fused predicate; only the slow one still branches on it, which is
    // what `guard_blocks` counts.
    assert_eq!(
        body.matches("as u64 (from i64)").count(),
        4,
        "the fused predicate is cloned into the fast copy and only its branch is bypassed:\n{body}"
    );
    assert_eq!(
        guard_blocks(&body).len(),
        1,
        "exactly one copy still branches to a trap:\n{body}"
    );
    // The preheader is the one block that computes the admission test: it must hold the
    // division-free overflow guards §3.2.1 names, and end in a two-way selection.
    assert!(
        body.contains("9223372036854775806_i64") && body.contains("9223372036854775807_i64"),
        "the preheader proves `N <= i64::MAX - s` and the `a*imax` / `amax + b` overflow bounds:\n{body}"
    );
}

/// The budget is a named constant, counted in body statements including terminators, and pinned.
/// Code growth from versioning is real; an owner that can see it is what keeps it bounded.
#[test]
fn g2_version_budget_is_pinned() {
    assert_eq!(
        LOOP_FACTS_VERSION_BUDGET, 256,
        "changing the versioning budget changes emitted code size for every program"
    );
}

// ── Index forms admitted and refused (§3.4) ─────────────────────────────────────────────────────

/// Every admitted index form, and every refusal, in one table. The reason **codes** are the stable
/// surface §3.2.3 promises; the prose around them is not, which is why this asserts the code.
#[test]
fn g2_index_forms_get_their_stated_decision() {
    let program = format!(
        "{SUM}{SCAN_REPORT}{SHIFTED_SUM}{STRIDE_SUM}{MATVEC}{WINDOW_FIRST}{SKIP_ZEROS}{DRAIN}\
fn main() {{ }}\n"
    );
    let report = loop_facts_report("index-forms", &program);
    for (function, expected) in [
        // `i`: the base case.
        ("sum", "versioned"),
        // `i + off`, `off` an unknown possibly-negative parameter: both ends of the range proved.
        ("scan_report", "versioned"),
        // `i * 2`: `a = 2`, and the bound is `a*imax + b`, not `imax`.
        ("stride_sum", "versioned"),
        // The guard length differs from the trip-count bound, and each guard is proved against its
        // own length — 1081's matvec is exactly this shape.
        ("matvec", "versioned"),
        // A range access `xs[i..i+k]`, including the loop-invariant overflow test.
        ("window_first", "versioned"),
        // The step precedes the access, so the value reaching it is not the header's.
        ("shifted_sum", "kept checks: step-precedes-access"),
        // The step is data-dependent, so there is no single monotone recurrence.
        ("skip_zeros", "kept checks: multiple-index-writes"),
        // `truncate` changes the published length inside the body.
        ("drain", "kept checks: root-killed:ArrayTruncate"),
    ] {
        let found = decision(&report, function);
        assert!(
            found.starts_with(expected),
            "[{function}] expected `{expected}`, got `{found}`"
        );
    }
}

/// The IR-identity refusal. `Stmt::BorrowedElementReservation` is a function-unique token that
/// codegen's `unique_reservation` requires to occur exactly once; a cloned body would carry it
/// twice and the fast copy would carry no guard at all. So a loop that borrows an element into a
/// call is refused outright — and still compiles and runs.
#[test]
fn g2_borrowed_element_loop_is_not_versioned_and_still_runs() {
    let program = format!("{TOTAL_INSPECT}fn main() -> i32 {{ print(total()); return 0 }}\n");
    assert_eq!(
        decision(&loop_facts_report("borrowed-element", &program), "total"),
        "kept checks: borrowed-element"
    );
    let out = build_and_run("loop-facts-borrowed-element", &program);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "9\n",
        "the refused loop still produces its value"
    );
}

/// A body this pass does not model, and a body whose enclosing loop holds a nested one, are both
/// fail-closed refusals with their own codes rather than silent non-decisions. Every source loop
/// gets exactly one line.
#[test]
fn g2_unmodelled_and_nested_bodies_are_refused_by_name() {
    let nested = "\
fn grid(borrow xs: slice<i64>, rows: i64) -> i64 {
  mut acc := 0
  mut r := 0
  loop {
    if r >= rows { break }
    mut c := 0
    loop {
      if c >= xs.len() { break }
      acc = acc + xs[c]
      c = c + 1
    }
    r = r + 1
  }
  return acc
}
";
    let program = format!("{nested}{BUILD_ARRAY}fn main() {{ }}\n");
    let report = loop_facts_report("nested-and-unmodelled", &program);
    // Innermost-first: the inner loop is decided first and is versioned; the outer one then holds
    // both of its copies and is refused for containing a nested loop.
    let grid: Vec<&String> = report
        .iter()
        .filter(|line| line.contains("`grid`"))
        .collect();
    assert_eq!(grid.len(), 2, "one line per source loop:\n{report:#?}");
    assert!(grid[0].contains("versioned"), "the inner loop is versioned first:\n{grid:#?}");
    assert!(
        grid[1].contains("kept checks: nested-loop"),
        "the enclosing loop is refused by name:\n{grid:#?}"
    );
    assert_eq!(
        decision(&report, "build"),
        "kept checks: unmodelled-statement",
        "an array-builder body is not modelled, and says so"
    );
}

// ── Trap parity (§3.3) ──────────────────────────────────────────────────────────────────────────

/// The whole reason versioning is the shape rather than relocation. With a negative `off`,
/// `scan_report` prints `0` and *then* traps: the failing run takes the unmodified slow loop, so
/// the observable prefix, the failing iteration, and the byte-identical trap text all survive.
/// A guard relocated to the preheader would trap before printing anything.
#[test]
fn g2_trap_parity_keeps_the_effect_prefix_and_the_exact_text() {
    let program = format!(
        "{SCAN_REPORT}fn main() -> i32 {{\n  data := [10, 20, 30]\n  print(scan_report(data, -1))\n  return 0\n}}\n"
    );
    let out = build_and_run("loop-facts-trap-prefix", &program);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "0\n",
        "the iterations before the failure happen, in order"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("align: panic: index out of bounds: the len is 3 but the index is -1"),
        "byte-identical trap text, with the original signed index:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "an out-of-bounds access is a hard error");
}

/// The view shapes §3.4 names: a zero-length view runs no iteration and traps not at all, and a
/// length-1 view's single iteration is identical. Both go through the *fast* copy, so these pin
/// that the admission arithmetic's zero-trip and one-trip cases are right.
#[test]
fn g2_zero_length_and_length_one_views_behave_identically() {
    let program = format!(
        "{SUM}fn main() -> i32 {{\n  \
           mut b: array_builder<i64> := array_builder()\n  \
           empty := b.build()\n  \
           print(sum(empty))\n  \
           one := [7]\n  \
           print(sum(one))\n  \
           return 0\n\
         }}\n"
    );
    let out = build_and_run("loop-facts-edge-lengths", &program);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "0\n7\n",
        "zero-trip and one-trip admitted loops keep their values:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The two refusal witnesses, executable. `shifted_sum` reads `1..n` while the header sees
/// `0..n-1`, so it traps at `xs[n]` exactly as it does today; `drain` truncates inside the body and
/// so re-reads the shortened length every iteration, exactly as it does today. Neither is
/// versioned, and both must behave as they did before this pass existed.
#[test]
fn g2_refused_loops_keep_their_exact_behaviour() {
    let shifted = format!(
        "{SHIFTED_SUM}fn main() -> i32 {{\n  data := [1, 2, 3]\n  print(shifted_sum(data))\n  return 0\n}}\n"
    );
    let out = build_and_run("loop-facts-shifted", &shifted);
    assert!(!out.status.success(), "`shifted_sum` still traps at `xs[n]`");
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("align: panic: index out of bounds: the len is 3 but the index is 3"),
        "with the same (index, len):\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let drain = format!(
        "{DRAIN}fn main() -> i32 {{\n  \
           mut b: array_builder<i64> := array_builder()\n  \
           b.push(1)\n  b.push(2)\n  b.push(3)\n  b.push(4)\n  \
           mut d := b.build()\n  \
           print(drain(d))\n  \
           return 0\n\
         }}\n"
    );
    let out = build_and_run("loop-facts-drain", &drain);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "3\n",
        "`drain` re-reads the truncated length every iteration, as it does today:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Every admitted index form, executed. An IR-shape owner cannot see an off-by-one in the
/// admission arithmetic; only running the fast copy against the slow copy's answers can.
#[test]
fn g2_admitted_loops_compute_the_same_values() {
    let program = format!(
        "{SUM}{SCAN_REPORT}{STRIDE_SUM}{MATVEC}{WINDOW_FIRST}fn main() -> i32 {{\n  \
           data := [1, 2, 3, 4]\n  \
           print(sum(data))\n  \
           print(scan_report(data, 0))\n  \
           head : slice<i64> := data[0..1]\n  \
           print(stride_sum(head))\n  \
           print(matvec(data, data, 2, 1))\n  \
           print(window_first(data, 0))\n  \
           return 0\n\
         }}\n"
    );
    let out = build_and_run("loop-facts-values", &program);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        // sum = 10; scan_report prints 0..3 then 10; stride_sum over a length-1 view reads index 0
        // only (a length-2 view would step to `xs[2]` and take the slow copy); matvec row 1 of a
        // 2-wide matrix = 3*1 + 4*2 = 11; window_first sums four empty windows.
        "10\n0\n1\n2\n3\n10\n1\n11\n0\n",
        "every admitted form keeps its value:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ── Composition (§3.4) ──────────────────────────────────────────────────────────────────────────

/// Invariant I8. `annotate_par_map_work` derives a `par_map` work weight from `block.stmts.len()`
/// and runs in `lower_program_unchecked_with_plans`, *before* `loop_facts`. Versioning a kernel
/// must therefore leave the annotated weight byte-identical — otherwise the `ParMapReduce`
/// partition, and with it float reduction order, would change as a side effect of a bounds-check
/// transform. The weight is printed in MIR text, so the pin is exact; §3.7 records that the bucket
/// boundaries already put every *versionable* body above the top bucket, so this owner pins the
/// ordering's observable consequence — a versioned kernel whose weight and reduction are unchanged
/// — rather than a value versioning could otherwise move.
#[test]
fn g2_par_map_work_weight_is_derived_before_versioning() {
    let src = "\
fn scale(x: i64) -> i64 {
  w := [1, 2]
  mut acc := x
  mut i := 0
  loop {
    if i >= 2 { break }
    acc = acc + w[i]
    i = i + 1
  }
  return acc
}
fn run(xs: slice<i64>) -> i64 = xs.par_map(scale).sum()
fn main() -> i32 {
  data := [1, 2, 3, 4]
  print(run(data))
  return 0
}
";
    assert!(
        decision(&loop_facts_report("par-map-weight", src), "scale").starts_with("versioned"),
        "the kernel body is the one being versioned, so the ordering is what this owner tests"
    );
    let mir = mir_text("g2-par-map-weight", src);
    let nodes: Vec<&str> = mir
        .lines()
        .filter(|line| line.contains("par_map_reduce"))
        .collect();
    assert_eq!(nodes.len(), 1, "one parallel reduction node:\n{mir}");
    assert!(
        nodes[0].contains("work=4"),
        "the weight is the unversioned body's, computed before this pass ran:\n{}",
        nodes[0]
    );
    let out = build_and_run("loop-facts-par-map", src);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "22\n",
        "the partition and the reduction are unchanged:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `byte_prepare` runs in codegen, **after** `loop_facts`, and its leaf-eligibility budget counts
/// the body it actually splices — the post-versioning one. A leaf that straddles that budget is
/// therefore inlined without versioning and not inlined with it, deterministically. This owner
/// pins the determinism, not a particular side of the straddle: the same input must version the
/// same way and produce the same answer in a whole-program build.
#[test]
fn g2_byte_prepare_composes_with_a_versioned_leaf() {
    let src = "\
fn leaf(borrow b: slice<u8>, off: i64) -> i64 {
  mut acc := 0
  mut i := 0
  loop {
    if i >= 2 { break }
    acc = acc + (b.u32_le(off + i * 4) as i64)
    i = i + 1
  }
  return acc
}
fn caller(borrow b: slice<u8>) -> i64 = leaf(b, 0) + leaf(b, 4)
fn main() -> i32 {
  mut w: array_builder<u8> := array_builder()
  mut i := 0
  loop {
    if i >= 16 { break }
    w.push(1)
    i = i + 1
  }
  bytes := w.build()
  print(caller(bytes))
  return 0
}
";
    let report = loop_facts_report("byte-prepare-straddle", src);
    assert_eq!(
        decision(&report, "leaf"),
        "kept checks: guard-not-fused",
        "a leaf holding a byte-accessor guard keeps its checks, and says which channel refused it"
    );
    let out = build_and_run("loop-facts-byte-prepare", src);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "67372036\n",
        "the composed byte reader is unchanged:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Ownership across the duplicated body, closed by construction rather than by bookkeeping. Drop
/// flags are per-binding slots shared by both copies, and a source-level test cannot discriminate a
/// flag defect in the copy that did not execute — so instead of duplicating that bookkeeping and
/// then testing it, admission refuses any body it does not model, and every rvalue that *produces*
/// an individually owned value (`StrClone`, `HeapAlloc`, `ArenaBegin`, the builders) is unmodelled.
/// A loop carrying an owned per-iteration value is therefore never versioned, which is what this
/// owner pins; `loop_expr::a_per_iteration_owned_string_is_freed_each_pass` remains the executable
/// backstop for the behaviour itself.
#[test]
fn g2_a_loop_with_an_owned_per_iteration_value_is_never_versioned() {
    let src = "\
fn tag(borrow xs: slice<i64>) -> i64 {
  mut n := 0
  mut i := 0
  loop {
    if i >= xs.len() { break }
    label := \"row\".clone()
    n = n + label.len() + xs[i]
    i = i + 1
  }
  return n
}
fn main() -> i32 {
  data := [1, 2, 3]
  print(tag(data))
  return 0
}
";
    assert_eq!(
        decision(&loop_facts_report("owned-per-iteration", src), "tag"),
        "kept checks: unmodelled-statement",
        "an owned per-iteration value is unmodelled, so the body is never duplicated"
    );
    let body = mir_fn(&mir_text("g2-owned-iteration", src), "tag");
    assert_eq!(
        body.matches("call runtime bounds_fail").count(),
        1,
        "one copy, one guard, one trap:\n{body}"
    );
    let out = build_and_run("loop-facts-owned-iteration", src);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "15\n",
        "the per-iteration owned string is still freed each pass:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
