//! The 1:1 value-carrying-control-flow matrix: every form preserves the shortest escape Region
//! and the runtime individual-vs-arena ownership bit of an owned value.

mod common;
use common::*;

fn check_message(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(checked.diags.has_errors(), "{name} must reject an escaping arena value");
    align_driver::format_diagnostics(&sm, &checked.diags)
}

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

fn function_body<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text.find(&format!("fn {name}")).unwrap_or_else(|| panic!("no fn {name} in MIR:\n{text}"));
    let body = &text[start..];
    let end = body.find("\n}").map(|i| i + 2).unwrap_or(body.len());
    &body[..end]
}

fn assert_flag_is_forwarded(text: &str, function: &str, flag: &str) {
    let body = function_body(text, function);
    let prefix = format!("{flag} <- ");
    assert!(
        body.lines().any(|line| line.trim().starts_with(&prefix) && line.contains('%')),
        "{flag} must receive a path-specific runtime ownership bit:\n{body}"
    );
}

// Region row: the result Region is the trailing/selected payload Region. Each program attempts to
// carry an arena-backed `str` across the arena boundary and must fail at that boundary.

#[test]
fn block_value_preserves_region() {
    let msg = check_message(
        "value-region-block",
        "fn main() -> i32 {\n  out := arena {\n    n := 1\n    s := template \"{n}\"\n    { s }\n  }\n  return out.len() as i32\n}\n",
    );
    assert!(msg.contains("cannot escape"), "unexpected diagnostic:\n{msg}");
}

#[test]
fn if_value_joins_regions() {
    let msg = check_message(
        "value-region-if",
        "fn main() -> i32 {\n  out := arena {\n    n := 1\n    s := template \"{n}\"\n    if n > 0 { s } else { \"static\" }\n  }\n  return out.len() as i32\n}\n",
    );
    assert!(msg.contains("cannot escape"), "unexpected diagnostic:\n{msg}");
}

#[test]
fn match_value_joins_regions() {
    let msg = check_message(
        "value-region-match",
        "Tag { A, B }\nfn main() -> i32 {\n  out := arena {\n    n := 1\n    s := template \"{n}\"\n    match Tag.A { A => s, B => \"static\" }\n  }\n  return out.len() as i32\n}\n",
    );
    assert!(msg.contains("cannot escape"), "unexpected diagnostic:\n{msg}");
}

#[test]
fn else_unwrap_value_joins_regions() {
    let msg = check_message(
        "value-region-else",
        "fn main() -> i32 {\n  out := arena {\n    n := 1\n    s := template \"{n}\"\n    opt: Option<str> := Some(s)\n    opt else \"static\"\n  }\n  return out.len() as i32\n}\n",
    );
    assert!(msg.contains("cannot escape"), "unexpected diagnostic:\n{msg}");
}

#[test]
fn try_value_preserves_payload_region() {
    let msg = check_message(
        "value-region-try",
        "fn run() -> Result<i32, Error> {\n  out := arena {\n    n := 1\n    s := template \"{n}\"\n    r: Result<str, Error> := Ok(s)\n    r?\n  }\n  return Ok(out.len() as i32)\n}\nfn main() -> i32 = 0\n",
    );
    assert!(msg.contains("cannot escape"), "unexpected diagnostic:\n{msg}");
}

// Ownership row: an owned result may be arena-owned on one path and individually heap-owned on
// another. The destination flag must receive the selected path's runtime bit, not a conservative
// constant derived from the joined escape Region.

#[test]
fn block_value_forwards_owned_flag() {
    let text = mir_text(
        "value-owned-block",
        "fn make() -> array<i64> = [7, 8].to_array()\nfn run(cond: bool) -> i32 {\n  arena {\n    mut xs := [1, 2].to_array()\n    if cond { xs = make() }\n    ys := { xs }\n    return ys[0] as i32\n  }\n}\nfn main() -> i32 = run(true)\n",
    );
    // run locals: cond=_0, xs=_1, ys=_2; flags follow at _3 and _4.
    assert_flag_is_forwarded(&text, "run", "_4");
}

#[test]
fn if_value_forwards_selected_owned_flag() {
    let text = mir_text(
        "value-owned-if",
        "fn make() -> array<i64> = [7, 8].to_array()\nfn run(cond: bool) -> i32 {\n  arena {\n    xs := if cond { make() } else { [1, 2].to_array() }\n    return xs[0] as i32\n  }\n}\nfn main() -> i32 = run(true)\n",
    );
    // run locals: cond=_0, xs=_1; xs flag=_2.
    assert_flag_is_forwarded(&text, "run", "_2");
}

#[test]
fn match_value_forwards_selected_owned_flag() {
    let text = mir_text(
        "value-owned-match",
        "Choice { Heap, Arena }\nfn make() -> array<i64> = [7, 8].to_array()\nfn run(choice: Choice) -> i32 {\n  arena {\n    xs := match choice { Heap => make(), Arena => [1, 2].to_array() }\n    return xs[0] as i32\n  }\n}\nfn main() -> i32 = run(Choice.Heap)\n",
    );
    // run locals: choice=_0, xs=_1; xs flag=_2.
    assert_flag_is_forwarded(&text, "run", "_2");
}

#[test]
fn else_unwrap_forwards_selected_owned_flag() {
    let text = mir_text(
        "value-owned-else",
        "fn maybe(cond: bool) -> Option<array<i64>> {\n  if cond { return Some([7, 8].to_array()) }\n  return None\n}\nfn run(cond: bool) -> i32 {\n  arena {\n    opt := maybe(cond)\n    xs := opt else [1, 2].to_array()\n    return xs[0] as i32\n  }\n}\nfn main() -> i32 = run(true)\n",
    );
    // run locals: cond=_0, opt=_1, xs=_2; flags follow at _3 and _4.
    assert_flag_is_forwarded(&text, "run", "_4");
}

#[test]
fn try_forwards_unwrapped_owned_flag() {
    let text = mir_text(
        "value-owned-try",
        "fn make() -> array<i64> = [7, 8].to_array()\nfn run(cond: bool) -> Result<i32, Error> {\n  arena {\n    mut r: Result<array<i64>, Error> := Ok([1, 2].to_array())\n    if cond { r = Ok(make()) }\n    xs := r?\n    return Ok(xs[0] as i32)\n  }\n}\nfn main() -> i32 = 0\n",
    );
    // run locals: cond=_0, r=_1, xs=_2; flags follow at _3 and _4.
    assert_flag_is_forwarded(&text, "run", "_4");
}
