//! Expression-nesting-depth ceiling (`align_parser::MAX_EXPR_DEPTH` + `cap_expr_depths`).
//!
//! The parser's recursion guard only bounds *recursively* parsed nesting; the left-associative
//! binary-operator loop and the postfix-method loop grow the AST **iteratively**, so before the fix
//! a ~1000-term chain (`1+1+1+…`, ~2 KB of source — plausible machine-generated code) parsed cleanly
//! and then overflowed the native stack in a downstream recursive walk (sema type-check / move /
//! escape / effect, then MIR lowering), aborting the process instead of emitting a diagnostic
//! (`docs/open-questions.md`, 2026-07-02 internal review). A post-parse pass now truncates any
//! over-deep expression, so both the recursive and the iterative vectors end in one clean
//! diagnostic. The chain lengths here (2000+) all overflowed the pre-fix compiler.

mod common;
use common::*;

/// The number of "expression nests too deeply" diagnostics produced by checking `src`.
fn too_deep_count(name: &str, src: &str) -> usize {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    checked
        .diags
        .iter()
        .filter(|d| d.message.contains("expression nests too deeply"))
        .count()
}

#[test]
fn long_binary_chain_is_rejected_cleanly_not_by_ice() {
    // 2000-term `+` chain: iteratively parsed to a 2000-deep AST, which overflowed sema pre-fix.
    let chain = std::iter::repeat("1").take(2000).collect::<Vec<_>>().join("+");
    let src = format!("fn main() -> i32 {{\n  x := {chain}\n  return 0\n}}\n");
    // The test process merely *surviving* this call is the no-ICE assertion (a stack overflow aborts
    // the whole test binary, it is not catchable). Exactly one clean diagnostic — not one per
    // sibling leaf at the truncation boundary.
    assert_eq!(
        too_deep_count("long-add-chain", &src),
        1,
        "an over-deep binary chain must yield exactly one clean diagnostic"
    );
}

#[test]
fn long_postfix_chain_is_rejected_cleanly_not_by_ice() {
    // 2000-deep postfix method chain `0.abs().abs()…`, grown iteratively by the postfix loop.
    let mut src = String::from("fn main() -> i32 {\n  x := 0");
    for _ in 0..2000 {
        src.push_str(".abs()");
    }
    src.push_str("\n  return 0\n}\n");
    assert_eq!(
        too_deep_count("long-postfix-chain", &src),
        1,
        "an over-deep postfix chain must yield exactly one clean diagnostic"
    );
}

#[test]
fn deeply_nested_parens_still_guarded() {
    // The pre-existing recursion guard (deep `((((…))))` overflows the *parser* itself, before any
    // downstream pass) must keep firing — this fix must not regress it.
    let src = format!(
        "fn main() -> i32 {{\n  x := {}1{}\n  return 0\n}}\n",
        "(".repeat(400),
        ")".repeat(400)
    );
    assert!(
        too_deep_count("deep-parens", &src) >= 1,
        "deeply nested parentheses must still be diagnosed, not overflow the stack"
    );
}

#[test]
fn deep_within_limit_expression_is_accepted() {
    // A 120-term chain is under the ceiling: it must type-check with no depth error (and no ICE),
    // proving the cap does not wrongly reject reasonably deep machine-generated expressions.
    let chain = std::iter::repeat("1").take(120).collect::<Vec<_>>().join("+");
    let src = format!("fn main() -> i32 {{\n  return {chain}\n}}\n");
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "within-limit", &src);
    assert!(
        !checked.diags.has_errors(),
        "a within-limit expression must be accepted:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
}

#[test]
fn within_limit_chain_compiles_and_runs() {
    if !backend_available() {
        return;
    }
    // End-to-end: a 40-term chain (safe for the full codegen pipeline on the test thread) must
    // build and return the correct value — the truncation pass leaves in-limit trees untouched.
    let chain = std::iter::repeat("1").take(40).collect::<Vec<_>>().join("+");
    let src = format!("fn main() -> i32 {{\n  return {chain}\n}}\n");
    let out = build_and_run("within-limit-run", &src);
    assert_eq!(out.status.code(), Some(40));
}
