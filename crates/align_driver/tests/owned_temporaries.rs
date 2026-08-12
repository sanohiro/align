//! Synthetic ownership for unbound Move temporaries. Scalar consumers release a fresh owner as
//! soon as their result is available; borrowed views retain the hidden owner through function or
//! loop cleanup. A path-selected bound local is never transferred to that hidden owner.

mod common;
use common::*;

fn mir_text(src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "owned-temporaries.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}

fn optimized_llvm(src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "owned-temporaries-ir.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    emit_llvm_ir(&mir, BuildTarget::Baseline, true, &[], false).expect("optimized LLVM IR")
}

fn function<'a>(mir: &'a str, name: &str) -> &'a str {
    let marker = format!("fn {name}");
    let start = mir.find(&marker).unwrap_or_else(|| panic!("missing {marker} in MIR:\n{mir}"));
    let body = &mir[start..];
    let end = body.find("\n}").map_or(body.len(), |i| i + 2);
    &body[..end]
}

fn real_drop_count(body: &str) -> usize {
    body.lines().filter(|line| line.trim_start().starts_with("drop _")).count()
}

#[test]
fn scalar_consumers_drop_all_confirmed_temporary_shapes() {
    let src = r#"
fn probe(n: i64) -> i64 {
  mut b: array_builder<i64> := array_builder()
  b.push(7)
  return "x".clone().len()
    + [n, n + 1].to_array().len()
    + [n, n + 1].to_array()[0]
    + [1, 2, 3].chunks(2).len()
    + b.build().len()
}
fn main() -> i32 = probe(2) as i32
"#;
    let mir = mir_text(src);
    let probe = function(&mir, "probe");
    assert!(probe.contains("str_clone"), "string producer missing:\n{probe}");
    assert!(probe.contains("heap_alloc"), "owned array producer missing:\n{probe}");
    assert!(
        !probe.contains("chunks("),
        "a direct chunks length must use its virtual scalar form:\n{probe}"
    );
    assert!(probe.contains("array_builder_build"), "builder freeze missing:\n{probe}");
    assert!(
        real_drop_count(probe) >= 4,
        "each confirmed unbound owner needs a real drop edge:\n{probe}"
    );
    if backend_available() {
        let ir = optimized_llvm(src);
        assert_eq!(
            ir.matches("call void @align_rt_free").count(),
            4,
            "the four heap-producing examples must each retain exactly one optimized free:\n{ir}"
        );
    }
}

#[test]
fn moved_slots_emit_no_known_null_destructor_calls() {
    let src = r#"
fn take_string() -> string {
  s := "x".clone()
  return s
}
fn choose(flag: bool) -> string {
  s := "kept".clone()
  if flag { return s }
  return "other".clone()
}
fn check(flag: bool) -> Result<(), i64> {
  if flag { return Ok(()) }
  return Err(9)
}
fn via_try(flag: bool) -> Result<string, i64> {
  s := "try".clone()
  check(flag)?
  return Ok(s)
}
fn main() -> i32 {
  mut b: array_builder<i64> := array_builder()
  b.push(7)
  xs := b.build()
  tried := match via_try(true) { Ok(s) => s.len() Err(_) => 100 }
  failed := match via_try(false) { Ok(s) => s.len() Err(e) => e }
  return (take_string().len() + choose(true).len() + choose(false).len()
    + xs[0] + tried + failed) as i32
}
"#;
    if backend_available() {
        let ir = optimized_llvm(src);
        assert!(
            !ir.contains("@align_rt_free(ptr null)"),
            "definitely moved string/array slots must not call free(null):\n{ir}"
        );
        assert!(
            !ir.contains("@align_rt_array_builder_free(ptr null)"),
            "a frozen builder must not retain a null handle destructor:\n{ir}"
        );
        assert!(
            ir.contains("call void @align_rt_free("),
            "live allocations still need real destructor calls:\n{ir}"
        );
        assert_eq!(build_and_run("known-null-drops", src).status.code(), Some(29));
    }
}

#[test]
fn major_string_and_array_producers_feed_scalar_consumers_without_leaks() {
    let src = r#"
import std.path
import std.encoding
fn dbl(x: i64) -> i64 = x * 2
fn probe() -> i64 {
  return path.base(path.join("a", "b")).len()
    + encoding.hex_encode("AB").len()
    + [3, 1, 2].sort().len()
    + [1, 2, 3].par_map(dbl).len()
}
fn main() -> i32 = probe() as i32
"#;
    let mir = mir_text(src);
    let probe = function(&mir, "probe");
    assert!(probe.contains("path_join"), "path producer missing:\n{probe}");
    assert!(probe.contains("encode_Hex"), "encoding producer missing:\n{probe}");
    assert!(
        probe.contains("drop_value") && probe.contains("heap_alloc"),
        "sort materialization missing:\n{probe}"
    );
    assert!(probe.contains("par_map"), "par_map producer missing:\n{probe}");
    assert!(
        real_drop_count(probe) >= 4,
        "every directly consumed producer needs a synthetic-owner drop:\n{probe}"
    );
}

#[test]
fn a_view_keeps_string_array_and_call_argument_owners_live() {
    let src = r#"
fn tail(s: str) -> str = s[1..]
fn probe() -> i64 {
  text := tail(" abc ".clone()).trim()
  nums := [10, 20, 30].to_array()[1..]
  return text.len() + nums[0]
}
fn main() -> i32 = probe() as i32
"#;
    let mir = mir_text(src);
    let probe = function(&mir, "probe");
    let last_subslice = probe.rfind("subslice").expect("view lowering");
    let first_drop = probe.find("drop _").expect("hidden owner cleanup");
    assert!(
        first_drop > last_subslice,
        "hidden owners must outlive construction and use of their borrowed views:\n{probe}"
    );
    if backend_available() {
        assert_eq!(build_and_run("owned-temp-views", src).status.code(), Some(23));
    }
}

/// A borrowed temporary produced by a value-carrying `if` selects one arm's ownership at runtime:
/// the fresh arm hands its buffer to the hidden owner, the bound arm stays borrowed in place and
/// keeps its own local Drop.
///
/// This shape used to lower the `if` **twice** and was quarantined against
/// `docs/impl/05-backend-llvm.md` §1 while codegen's module verification stayed debug-only. Both
/// are closed: the eager worklist no longer pre-lowers a `str.borrow` child whose borrow-mode
/// lowering differs, and the verifier runs in every profile.
/// `borrowed_control_flow_temporaries_lower_exactly_once` sweeps the rest of the matrix.
#[test]
fn mixed_if_arms_drop_only_the_selected_temporary() {
    let src = r#"
fn probe(flag: bool) -> i64 {
  bound := "bound".clone()
  return (if flag { " tmp ".clone() } else { bound }).trim().len()
}
fn main() -> i32 = (probe(true) + probe(false)) as i32
"#;
    let mir = mir_text(src);
    let probe = function(&mir, "probe");
    assert!(probe.contains("<- false"), "bound arm must select a false temporary bit:\n{probe}");
    assert!(probe.contains("<- true"), "fresh arm must select a true temporary bit:\n{probe}");
    if backend_available() {
        assert_eq!(build_and_run("owned-temp-if", src).status.code(), Some(8));
    }
}

/// Closure matrix for a borrowed owned temporary produced by value-carrying control flow
/// (`docs/impl/05-backend-llvm.md` §1): `if` / `match` / `else`-unwrap crossed with fresh, bound,
/// and mixed arms.
///
/// Every cell used to lower its control flow twice. `str.borrow` (`string` used as `str`) took the
/// eager worklist, which lowers a parent's children *ahead of* the parent, so the control flow was
/// lowered once in non-borrow mode — which also nulls a bound arm's source local — and then again
/// in borrow mode by `lower_borrowed_owned`. The second copy stored the first copy's per-arm SSA
/// values into the hidden owner's join slot from blocks that do not dominate their definitions
/// (`Instruction does not dominate all uses`).
///
/// The witness is the **exact** number of `branch` terminators `probe` lowers: the construct's own
/// discriminator test plus its drop guards. Counting `str_clone` would not do — in a bound cell
/// every `.clone()` sits in a `let` *outside* the duplicated subtree, so that count is unchanged by
/// the defect. Every expected number below was mutation-verified against the pre-fix compiler,
/// where the same cells lower 2/4/5, 3/5/6, and 5/7/6 branches respectively. The exit code then
/// proves the selected arm — and only it — is released.
#[test]
fn borrowed_control_flow_temporaries_lower_exactly_once() {
    // (cell, `probe` body, `branch` terminators, exit code of `probe(true) + probe(false)`)
    let cells: [(&str, &str, usize, i32); 9] = [
        (
            "if-fresh",
            r#"  return (if flag { " tmp ".clone() } else { " other ".clone() }).trim().len()"#,
            1,
            8,
        ),
        (
            "if-bound",
            r#"  a := " aaa ".clone()
  b := " bb ".clone()
  return (if flag { a } else { b }).trim().len()"#,
            1,
            5,
        ),
        (
            "if-mixed",
            r#"  bound := " bound ".clone()
  return (if flag { " tmp ".clone() } else { bound }).trim().len()"#,
            3,
            8,
        ),
        (
            "match-fresh",
            r#"  choice: Option<bool> := if flag { Some(true) } else { None }
  return (match choice {
    Some(_) => " tmp ".clone()
    None => " other ".clone()
  }).trim().len()"#,
            2,
            8,
        ),
        (
            "match-bound",
            r#"  a := " aaa ".clone()
  b := " bb ".clone()
  choice: Option<bool> := if flag { Some(true) } else { None }
  return (match choice {
    Some(_) => a
    None => b
  }).trim().len()"#,
            2,
            5,
        ),
        (
            "match-mixed",
            r#"  bound := " bound ".clone()
  choice: Option<bool> := if flag { Some(true) } else { None }
  return (match choice {
    Some(_) => " tmp ".clone()
    None => bound
  }).trim().len()"#,
            4,
            8,
        ),
        (
            "else-fresh",
            r#"  opt: Option<string> := if flag { Some(" tmp ".clone()) } else { None }
  return (opt else " other ".clone()).trim().len()"#,
            4,
            8,
        ),
        (
            "else-bound",
            r#"  a := " aaa ".clone()
  b := " bb ".clone()
  opt: Option<string> := if flag { Some(a) } else { None }
  return (opt else b).trim().len()"#,
            5,
            5,
        ),
        (
            "else-mixed",
            r#"  bound := " bound ".clone()
  opt: Option<string> := if flag { Some(" tmp ".clone()) } else { None }
  return (opt else bound).trim().len()"#,
            4,
            8,
        ),
    ];
    for (cell, body, branches, exit) in cells {
        let src = format!(
            "fn probe(flag: bool) -> i64 {{\n{body}\n}}\nfn main() -> i32 = (probe(true) + probe(false)) as i32\n"
        );
        let mir = mir_text(&src);
        let probe = function(&mir, "probe");
        assert_eq!(
            probe.matches("branch ").count(),
            branches,
            "{cell}: the borrowed control flow must be lowered exactly once:\n{probe}"
        );
        if backend_available() {
            assert_eq!(
                build_and_run(&format!("owned-temp-{cell}"), &src).status.code(),
                Some(exit),
                "{cell}: only the selected arm's temporary may be released"
            );
        }
    }
}

/// The other half of the matrix: the five **borrow-transparent scopes** a borrowed owned temporary
/// can be produced by (`{ }`, `unsafe`, `arena`, a named arena, `task_group`).
///
/// These never violated SSA dominance — a scope has no join — which is exactly why they needed
/// their own cells: the pre-fix compiler mis-lowered them *silently*. Each scope was lowered twice,
/// and while the eager memo happened to deduplicate the body's expressions, the statement stores
/// and the scope's own framing were not memoized, so `main` emits the `n = 41` store twice and a
/// second, empty `arena_begin`/`arena_end` (or `tg_begin`/`tg_wait`/`tg_end`) pair around nothing —
/// a spurious runtime region and task group per evaluation. The duplicated store happens to be
/// idempotent here, so the exit code is 44 on both compilers: only the structural counts
/// discriminate, and both were mutation-verified at 2 against the pre-fix compiler.
#[test]
fn borrowed_scope_temporaries_lower_their_scope_exactly_once() {
    // (cell, scope opener, scope-framing op, framing occurrences)
    let cells: [(&str, &str, &str, usize); 5] = [
        ("block", "{", "", 0),
        ("unsafe", "unsafe {", "", 0),
        ("arena", "arena {", "arena_begin", 1),
        ("named-arena", "arena scope {", "arena_begin", 1),
        ("task-group", "task_group {", "tg_begin", 1),
    ];
    for (cell, opener, framing, framings) in cells {
        let src = format!(
            r#"fn probe() -> i64 {{
  mut n := 0
  s := ({opener}
    n = 41
    " tmp ".clone()
  }}).trim().len()
  return s + n
}}
fn main() -> i32 = probe() as i32
"#
        );
        let mir = mir_text(&src);
        let probe = function(&mir, "probe");
        assert_eq!(
            probe.matches("<- 41_i64").count(),
            1,
            "{cell}: the scope's statements must be lowered exactly once:\n{probe}"
        );
        if !framing.is_empty() {
            assert_eq!(
                probe.matches(framing).count(),
                framings,
                "{cell}: the scope must open exactly once — a second empty {framing} pair is a \
                 spurious runtime region:\n{probe}"
            );
        }
        if backend_available() {
            assert_eq!(
                build_and_run(&format!("owned-temp-scope-{cell}"), &src).status.code(),
                Some(44),
                "{cell}: the borrowed scope temporary must still compute 3 + 41"
            );
        }
    }
}

/// The same borrowed control flow crossed with the early-exit paths: an arm that `return`s and an
/// arm that propagates with `?`. Each leaves the join without a value on one edge, so the hidden
/// owner must already exist when that edge runs exit cleanup and must stay disarmed there.
#[test]
fn borrowed_control_flow_temporaries_survive_diverging_and_try_arms() {
    let src = r#"
fn fail(flag: bool) -> Result<string, i64> {
  if flag { return Ok(" ok ".clone()) }
  return Err(7)
}
fn diverging_arm(flag: bool) -> i64 {
  bound := " bound ".clone()
  return (if flag { " tmp ".clone() } else { return 42 }).trim().len() + bound.len()
}
fn try_in_arm(flag: bool) -> Result<i64, i64> {
  bound := " bound ".clone()
  n := (if flag { fail(flag)? } else { bound }).trim().len()
  return Ok(n)
}
fn main() -> i32 {
  a := diverging_arm(true) + diverging_arm(false)
  b := match try_in_arm(true) { Ok(v) => v Err(e) => e }
  c := match try_in_arm(false) { Ok(v) => v Err(e) => e }
  return (a + b + c) as i32
}
"#;
    let mir = mir_text(src);
    // One `str_clone` per source `.clone()` in each probe: two in `diverging_arm` (the bound local
    // and the fresh arm), one in `try_in_arm` (the bound local; `fail` owns the other).
    for (probe, clones) in [("diverging_arm", 2), ("try_in_arm", 1)] {
        let body = function(&mir, probe);
        assert_eq!(
            body.matches("str_clone").count(),
            clones,
            "{probe}: the borrowed `if` must be lowered exactly once:\n{body}"
        );
    }
    if backend_available() {
        assert_eq!(build_and_run("owned-temp-early-exit", src).status.code(), Some(59));
    }
}

#[test]
fn match_and_try_preserve_temporary_ownership() {
    let src = r#"
fn make() -> Result<string, i64> = Ok("try".clone())
fn via_try() -> Result<i64, i64> = Ok(make()?.len())
fn via_match(flag: bool) -> i64 {
  bound := "hello".clone()
  choice: Option<bool> := if flag { Some(true) } else { None }
  return (match choice {
    Some(_) => "new".clone()
    None => bound
  }).len()
}
fn main() -> i32 {
  n := match via_try() { Ok(v) => v Err(_) => 100 }
  return (n + via_match(true) + via_match(false)) as i32
}
"#;
    let mir = mir_text(src);
    assert!(real_drop_count(function(&mir, "via_try")) >= 1, "`?` result owner must be dropped:\n{mir}");
    let matched = function(&mir, "via_match");
    assert!(matched.contains("<- false") && matched.contains("<- true"), "match must join its temporary bit:\n{matched}");
    if backend_available() {
        assert_eq!(build_and_run("owned-temp-control", src).status.code(), Some(11));
    }
}

#[test]
fn loop_temporaries_are_dropped_on_each_back_edge_and_break() {
    let src = r#"
fn main() -> i32 {
  mut i := 0
  mut total := 0
  loop {
    total = total + "x".clone().len() as i32
    total = total + [1, 2, 3].to_array()[0] as i32
    i = i + 1
    if i >= 20000 { break }
  }
  return total % 251
}
"#;
    let mir = mir_text(src);
    let main = function(&mir, "main");
    assert!(real_drop_count(main) >= 2, "loop temporaries need per-iteration drop edges:\n{main}");
    if backend_available() {
        assert_eq!(build_and_run("owned-temp-loop", src).status.code(), Some(91));
    }
}

#[test]
fn nested_loop_temporary_cleanup_stays_in_the_innermost_loop() {
    let src = r#"
fn main() -> i32 {
  mut outer := 0
  mut total := 0
  loop {
    mut inner := 0
    loop {
      total = total + "x".clone().len() as i32
      inner = inner + 1
      if inner >= 2 { break }
    }
    outer = outer + 1
    if outer >= 3 { break }
  }
  return total
}
"#;
    let mir = mir_text(src);
    let main = function(&mir, "main");
    assert_eq!(
        real_drop_count(main),
        1,
        "the scalar release is live, while known-dead inner edges and function cleanup must be removed:\n{main}"
    );
    if backend_available() {
        assert_eq!(build_and_run("owned-temp-nested-loop", src).status.code(), Some(6));
    }
}

#[test]
fn borrowed_views_of_fresh_owners_cannot_escape() {
    let cases = [
        ("string", "fn bad() -> str = \"abc\".clone().trim()\nfn main() -> i32 = 0\n"),
        (
            "subslice",
            "fn bad() -> slice<i64> = [1, 2, 3].to_array()[1..]\nfn main() -> i32 = 0\n",
        ),
        (
            "chunk-element",
            "fn bad() -> slice<i64> = [1, 2, 3].to_array().chunks(2)[0]\nfn main() -> i32 = 0\n",
        ),
    ];
    for (name, src) in cases {
        let diagnostics = check_diagnostics(name, src);
        assert!(
            diagnostics.contains("cannot return") || diagnostics.contains("cannot escape"),
            "{name} must reject a view outliving its synthetic owner:\n{diagnostics}"
        );
    }
}
