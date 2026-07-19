//! pkg.web router core — W1 slice 1 (the pattern matcher + linear-scan dispatch oracle).
//!
//! The router module `apps/web/pkg/web/internal/router.align` is compiled as the real shipped
//! source (via `include_str!`), wired under a minimal `pkg.web` root + a `main` entry that exercises
//! it. This is both the framework's first integration test and the differential-testing **oracle** a
//! later slice's flat radix tree is checked against. The import chain (main -> pkg.web ->
//! pkg.web.internal.router) also exercises the F0 package rules end-to-end (D7: only `pkg.web` may
//! reach `pkg.web.internal.*`).

mod common;
use common::*;

/// The real, shipped router source.
const ROUTER: &str = include_str!("../../../apps/web/pkg/web/internal/router.align");

/// A minimal `pkg.web` root that re-exports the internal router entry points under test (the full
/// public surface — `get`/`serve`/`param`/… — is W2). Kept in the test (not the shipped tree) so the
/// repo carries only the real router until the surface lands.
const WEB_ROOT: &str = "module pkg.web\n\
import pkg.web.internal.router\n\
pub fn dispatch(patterns: slice<str>, path: str) -> i64 = pkg.web.internal.router.dispatch(patterns, path)\n\
pub fn match_score(pattern: str, path: str) -> i64 = pkg.web.internal.router.match_score(pattern, path)\n\
pub fn tree_dispatch(patterns: slice<str>, path: str) -> i64 = pkg.web.internal.router.tree_dispatch(patterns, path)\n";

fn web_project(entry_main: &str) -> Vec<(&'static str, String)> {
    vec![
        ("pkg/web/internal/router.align", ROUTER.to_string()),
        ("pkg/web.align", WEB_ROOT.to_string()),
        ("main.align", entry_main.to_string()),
    ]
}

fn run_web(name: &str, entry_main: &str) -> std::process::Output {
    let files = web_project(entry_main);
    let refs: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    build_and_run_multi(name, &refs, "main.align")
}

#[test]
fn dispatch_picks_static_over_param_over_wildcard() {
    if !backend_available() {
        return;
    }
    // routes[0] static prefix, [1] :param, [2] static leaf, [3] *wildcard. The oracle must pick the
    // most specific match at each path: static leaf beats the :param, the :param beats the wildcard.
    let main = "module main\n\
import pkg.web\n\
fn main() -> Result<(), Error> {\n\
  routes := [\"/v1/models\", \"/v1/models/:id\", \"/v1/models/featured\", \"/files/*path\"]\n\
  print(pkg.web.dispatch(routes, \"/v1/models\"))            // 0\n\
  print(pkg.web.dispatch(routes, \"/v1/models/42\"))         // 1 (:id)\n\
  print(pkg.web.dispatch(routes, \"/v1/models/featured\"))   // 2 (static beats :id)\n\
  print(pkg.web.dispatch(routes, \"/files/a/b/c\"))          // 3 (*path)\n\
  print(pkg.web.dispatch(routes, \"/nope\"))                 // -1 (no match)\n\
  return Ok(())\n\
}\n";
    let out = run_web("web-dispatch", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n1\n2\n3\n-1\n");
}

#[test]
fn match_score_semantics() {
    if !backend_available() {
        return;
    }
    // match_score is the per-route reference: >= 0 (more specific = higher) on a match, -1 otherwise.
    let main = "module main\n\
import pkg.web\n\
fn main() -> Result<(), Error> {\n\
  print(pkg.web.match_score(\"/\", \"/\"))                          //  2 (root, one empty literal seg)\n\
  print(pkg.web.match_score(\"/a/b\", \"/a/b\"))                    //  8 (a=2, then 2*3+2)\n\
  print(pkg.web.match_score(\"/a/:x\", \"/a/b\"))                   //  7 (a=2, then 2*3+1 — param < static)\n\
  print(pkg.web.match_score(\"/a/:x\", \"/a/\"))                    // -1 (:param rejects an empty seg)\n\
  print(pkg.web.match_score(\"/a/\", \"/a\"))                       // -1 (trailing slash is exact)\n\
  print(pkg.web.match_score(\"/a/b\", \"/a\"))                      // -1 (path too short)\n\
  print(pkg.web.match_score(\"/x/*rest\", \"/x/a/b\"))              //  2 (wildcard scores below param)\n\
  return Ok(())\n\
}\n";
    let out = run_web("web-score", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n8\n7\n-1\n-1\n-1\n2\n");
}

#[test]
fn param_beats_wildcard_at_same_depth() {
    if !backend_available() {
        return;
    }
    // A `:param` and a `*wildcard` both match `/files/x`; the param is more specific and must win.
    let main = "module main\n\
import pkg.web\n\
fn main() -> Result<(), Error> {\n\
  routes := [\"/files/*path\", \"/files/:name\"]\n\
  print(pkg.web.dispatch(routes, \"/files/x\"))     // 1 (:name beats *path)\n\
  print(pkg.web.dispatch(routes, \"/files/a/b\"))   // 0 (only *path matches multi-segment)\n\
  return Ok(())\n\
}\n";
    let out = run_web("web-param-wild", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n0\n");
}

// ── W1 slice 2: the flat SoA radix tree, differential-tested against the oracle ────────────────

#[test]
fn tree_dispatch_matches_the_expected_routes() {
    if !backend_available() {
        return;
    }
    // The radix tree resolves the same shapes the oracle does: static leaf > :param > *wildcard,
    // an interior node with no leaf misses, and a bare miss is -1.
    let main = "module main\n\
import pkg.web\n\
fn main() -> Result<(), Error> {\n\
  routes := [\"/v1/models\", \"/v1/models/:id\", \"/v1/models/featured\", \"/files/*path\"]\n\
  print(pkg.web.tree_dispatch(routes, \"/v1/models\"))            // 0\n\
  print(pkg.web.tree_dispatch(routes, \"/v1/models/42\"))         // 1\n\
  print(pkg.web.tree_dispatch(routes, \"/v1/models/featured\"))   // 2 (static beats :id)\n\
  print(pkg.web.tree_dispatch(routes, \"/files/a/b/c\"))          // 3 (*path)\n\
  print(pkg.web.tree_dispatch(routes, \"/nope\"))                 // -1\n\
  print(pkg.web.tree_dispatch(routes, \"/v1\"))                   // -1 (interior node, no leaf)\n\
  return Ok(())\n\
}\n";
    let out = run_web("web-tree", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n1\n2\n3\n-1\n-1\n");
}

#[test]
fn tree_dispatch_agrees_with_the_linear_oracle() {
    if !backend_available() {
        return;
    }
    // The differential test: over a diverse (non-backtracking) route table and many paths — nested
    // params, a tail wildcard, static-beats-param, trailing slash, interior misses — the radix tree
    // and the linear-scan oracle must return the SAME route index for every path. The Align harness
    // counts disagreements (printing the path index + the two verdicts on any mismatch) and prints
    // the total; 0 means every path agreed.
    let main = "module main\n\
import pkg.web\n\
fn check(routes: slice<str>, path: str, idx: i64) -> i64 {\n\
  a := pkg.web.dispatch(routes, path)\n\
  b := pkg.web.tree_dispatch(routes, path)\n\
  if a == b {\n\
    0\n\
  } else {\n\
    print(-999)\n\
    print(idx)\n\
    print(a)\n\
    print(b)\n\
    1\n\
  }\n\
}\n\
fn main() -> Result<(), Error> {\n\
  routes := [\"/\", \"/v1/models\", \"/v1/models/:id\", \"/v1/models/featured\", \"/v1/models/:id/versions\", \"/files/*path\", \"/users/:uid/posts/:pid\", \"/health\"]\n\
  paths := [\"/\", \"/v1/models\", \"/v1/models/42\", \"/v1/models/featured\", \"/v1/models/42/versions\", \"/files/a/b/c\", \"/files/x\", \"/users/7/posts/9\", \"/health\", \"/nope\", \"/v1\", \"/v1/models/\", \"/users/7/posts\", \"/files\"]\n\
  mut mism := 0\n\
  mut i := 0\n\
  loop {\n\
    if i >= paths.len() { break }\n\
    mism = mism + check(routes, paths[i], i)\n\
    i = i + 1\n\
  }\n\
  print(mism)\n\
  return Ok(())\n\
}\n";
    let out = run_web("web-tree-diff", main);
    assert_eq!(out.status.code(), Some(0));
    // "0" alone = every path agreed (a mismatch would print a -999 block before it).
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}
