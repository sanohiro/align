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
/// `pkg.web.types` — the dependency-free leaf holding `Ctx`/`Route`, which the router imports.
const TYPES: &str = include_str!("../../../apps/web/pkg/web/types.align");

/// A minimal `pkg.web` root that re-exports the internal router entry points under test (the full
/// public surface — `get`/`serve`/`param`/… — is W2). Kept in the test (not the shipped tree) so the
/// repo carries only the real router until the surface lands.
const WEB_ROOT: &str = "module pkg.web\n\
import pkg.web.internal.router\n\
pub fn dispatch(patterns: slice<str>, path: str) -> i64 = pkg.web.internal.router.dispatch(patterns, path)\n\
pub fn match_score(pattern: str, path: str) -> i64 = pkg.web.internal.router.match_score(pattern, path)\n\
pub fn param_value(pattern: str, path: str, name: str) -> str = pkg.web.internal.router.param_value(pattern, path, name)\n";

fn web_project(entry_main: &str) -> Vec<(&'static str, String)> {
    vec![
        ("pkg/web/internal/router.align", ROUTER.to_string()),
        ("pkg/web/types.align", TYPES.to_string()),
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

// ── The flat SoA radix tree (table-built, backtracking walk) is differential-tested against the
// linear oracle in `best_path_route_tree_agrees_with_the_linear_oracle` below; the W1 `slice<str>`
// tree (`tree_dispatch`) was removed outright when the table tree became the production path. ──

// ── W2 bridge: :param / *wildcard value capture ───────────────────────────────────────────────

#[test]
fn param_value_captures_named_segments() {
    if !backend_available() {
        return;
    }
    // The zero-copy value of each `:param` / `*wildcard`, and "" for a name the matched route does
    // not declare. These are `str` views into the request path — what `pkg.web.param(c, name)` returns.
    let main = "module main\n\
import pkg.web\n\
fn main() -> Result<(), Error> {\n\
  print(pkg.web.param_value(\"/v1/models/:id\", \"/v1/models/42\", \"id\"))            // 42\n\
  print(pkg.web.param_value(\"/users/:uid/posts/:pid\", \"/users/7/posts/9\", \"uid\")) // 7\n\
  print(pkg.web.param_value(\"/users/:uid/posts/:pid\", \"/users/7/posts/9\", \"pid\")) // 9\n\
  print(pkg.web.param_value(\"/files/*path\", \"/files/a/b/c\", \"path\"))              // a/b/c\n\
  print(pkg.web.param_value(\"/v1/models/:id\", \"/v1/models/42\", \"nope\").len())     // 0 (absent)\n\
  print(pkg.web.param_value(\"/static/x\", \"/static/x\", \"id\").len())                // 0 (no params)\n\
  return Ok(())\n\
}\n";
    let out = run_web("web-param", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n7\n9\na/b/c\n0\n0\n");
}

// ── W2: method-aware route-table dispatch (404 vs 405) ────────────────────────────────────────

/// The public shape: a table of `Route` values built by the per-method constructors, dispatched by
/// (method, path). Two routes may share a path with different methods; a path that matches but has
/// no route for the method is the 405 case, distinct from a 404.
#[test]
fn route_table_dispatches_by_method_and_separates_404_from_405() {
    if !backend_available() {
        return;
    }
    let web_root = "module pkg.web\n\
import pkg.web.types\n\
import pkg.web.internal.router\n\
pub fn get(pattern: str, handler: fn(pkg.web.types.Ctx) -> Result<response_builder, Error>) -> pkg.web.types.Route =\n\
  pkg.web.types.Route { method: \"GET\", pattern: pattern, handler: handler }\n\
pub fn post(pattern: str, handler: fn(pkg.web.types.Ctx) -> Result<response_builder, Error>) -> pkg.web.types.Route =\n\
  pkg.web.types.Route { method: \"POST\", pattern: pattern, handler: handler }\n\
pub fn dispatch(routes: slice<pkg.web.types.Route>, method: str, path: str) -> i64 =\n\
  pkg.web.internal.router.dispatch_routes(routes, method, path)\n\
pub fn method_not_allowed(routes: slice<pkg.web.types.Route>, method: str, path: str) -> bool =\n\
  pkg.web.internal.router.method_not_allowed(routes, method, path)\n";
    let main = "module main\n\
import std.http\n\
import pkg.web\n\
import pkg.web.types\n\
fn h1(c: pkg.web.types.Ctx) -> Result<response_builder, Error> = Ok(http.response(200))\n\
fn h2(c: pkg.web.types.Ctx) -> Result<response_builder, Error> = Ok(http.response(200))\n\
fn main() -> Result<(), Error> {\n\
  routes := [\n\
    pkg.web.get(\"/v1/models\", h1),\n\
    pkg.web.get(\"/v1/models/:id\", h2),\n\
    pkg.web.post(\"/v1/models\", h1),\n\
  ]\n\
  print(pkg.web.dispatch(routes, \"GET\", \"/v1/models\"))\n\
  print(pkg.web.dispatch(routes, \"POST\", \"/v1/models\"))\n\
  print(pkg.web.dispatch(routes, \"GET\", \"/v1/models/42\"))\n\
  print(pkg.web.dispatch(routes, \"DELETE\", \"/v1/models\"))\n\
  print(pkg.web.method_not_allowed(routes, \"DELETE\", \"/v1/models\"))\n\
  print(pkg.web.dispatch(routes, \"GET\", \"/nope\"))\n\
  print(pkg.web.method_not_allowed(routes, \"GET\", \"/nope\"))\n\
  return Ok(())\n\
}\n";
    let files: Vec<(&str, String)> = vec![
        ("pkg/web/internal/router.align", ROUTER.to_string()),
        ("pkg/web/types.align", TYPES.to_string()),
        ("pkg/web.align", web_root.to_string()),
        ("main.align", main.to_string()),
    ];
    let refs: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    let out = build_and_run_multi("web-methods", &refs, "main.align");
    assert_eq!(out.status.code(), Some(0));
    // GET/POST on the same path resolve to different routes; DELETE there is 405 (matched path, no
    // method), while /nope is 404 (no path at all).
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n2\n1\n-1\ntrue\n-1\nfalse\n");
}

/// The route-TABLE radix tree (`best_path_route`, the production dispatch path) agrees with the
/// linear `match_score` oracle (`best_path_route_linear`) on every path, including same-pattern
/// rows that differ only in method (both sides must return the FIRST such row: shared leaf /
/// strict `>`) and the BACKTRACKING shapes the adversarial review of #591 surfaced — a static
/// branch that dead-ends where a sibling `:param` (or `*wildcard`) branch matches deeper
/// (`/v1/models/featured/versions`, `/files/special/deep`).
#[test]
fn best_path_route_tree_agrees_with_the_linear_oracle() {
    if !backend_available() {
        return;
    }
    let web_root = "module pkg.web\n\
import pkg.web.types\n\
import pkg.web.internal.router\n\
pub fn get(pattern: str, handler: fn(pkg.web.types.Ctx) -> Result<response_builder, Error>) -> pkg.web.types.Route =\n\
  pkg.web.types.Route { method: \"GET\", pattern: pattern, handler: handler }\n\
pub fn post(pattern: str, handler: fn(pkg.web.types.Ctx) -> Result<response_builder, Error>) -> pkg.web.types.Route =\n\
  pkg.web.types.Route { method: \"POST\", pattern: pattern, handler: handler }\n\
pub fn best(routes: slice<pkg.web.types.Route>, path: str) -> i64 =\n\
  pkg.web.internal.router.best_path_route(routes, path)\n\
pub fn best_linear(routes: slice<pkg.web.types.Route>, path: str) -> i64 =\n\
  pkg.web.internal.router.best_path_route_linear(routes, path)\n";
    let main = "module main\n\
import std.http\n\
import pkg.web\n\
import pkg.web.types\n\
fn h(c: pkg.web.types.Ctx) -> Result<response_builder, Error> = Ok(http.response(200))\n\
fn check(routes: slice<pkg.web.types.Route>, path: str, idx: i64) -> i64 {\n\
  a := pkg.web.best_linear(routes, path)\n\
  b := pkg.web.best(routes, path)\n\
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
  routes := [\n\
    pkg.web.get(\"/\", h),\n\
    pkg.web.get(\"/v1/models\", h),\n\
    pkg.web.post(\"/v1/models\", h),\n\
    pkg.web.get(\"/v1/models/:id\", h),\n\
    pkg.web.get(\"/v1/models/featured\", h),\n\
    pkg.web.get(\"/v1/models/:id/versions\", h),\n\
    pkg.web.get(\"/files/*path\", h),\n\
    pkg.web.get(\"/users/:uid/posts/:pid\", h),\n\
    pkg.web.get(\"/health\", h),\n\
    pkg.web.get(\"/files/special\", h),\n\
  ]\n\
  paths := [\"/\", \"/v1/models\", \"/v1/models/42\", \"/v1/models/featured\", \"/v1/models/42/versions\", \"/files/a/b/c\", \"/files/x\", \"/users/7/posts/9\", \"/health\", \"/nope\", \"/v1\", \"/v1/models/\", \"/users/7/posts\", \"/files\", \"/v1/models/featured/versions\", \"/files/special\", \"/files/special/deep\"]\n\
  mut mism := 0\n\
  mut i := 0\n\
  loop {\n\
    if i >= paths.len() { break }\n\
    mism = mism + check(routes, paths[i], i)\n\
    i = i + 1\n\
  }\n\
  // The EMPTY table walks the cap=2 boundary (root-only columns): both sides must say -1.\n\
  empty := routes[0..0]\n\
  mism = mism + check(empty, \"/\", 100)\n\
  mism = mism + check(empty, \"/x/y\", 101)\n\
  print(mism)\n\
  return Ok(())\n\
}\n";
    let files: Vec<(&str, String)> = vec![
        ("pkg/web/internal/router.align", ROUTER.to_string()),
        ("pkg/web/types.align", TYPES.to_string()),
        ("pkg/web.align", web_root.to_string()),
        ("main.align", main.to_string()),
    ];
    let refs: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    let out = build_and_run_multi("web-table-tree-diff", &refs, "main.align");
    assert_eq!(out.status.code(), Some(0));
    // "0" alone = every path agreed (a mismatch prints a -999 block before it).
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

/// Backtracking dispatches ABSOLUTELY (not just oracle-agreement): a static branch that dead-ends
/// must unwind to the sibling `:param` (and, in the wildcard shape, to the `*wildcard`) — the exact
/// regression the adversarial review of #591 caught (the no-backtracking walk 404'd these while the
/// linear scan, production dispatch before the tree, matched them).
#[test]
fn best_path_route_backtracks_from_a_static_dead_end() {
    if !backend_available() {
        return;
    }
    let web_root = "module pkg.web\n\
import pkg.web.types\n\
import pkg.web.internal.router\n\
pub fn get(pattern: str, handler: fn(pkg.web.types.Ctx) -> Result<response_builder, Error>) -> pkg.web.types.Route =\n\
  pkg.web.types.Route { method: \"GET\", pattern: pattern, handler: handler }\n\
pub fn best(routes: slice<pkg.web.types.Route>, path: str) -> i64 =\n\
  pkg.web.internal.router.best_path_route(routes, path)\n";
    let main = "module main\n\
import std.http\n\
import pkg.web\n\
import pkg.web.types\n\
fn h(c: pkg.web.types.Ctx) -> Result<response_builder, Error> = Ok(http.response(200))\n\
fn main() -> Result<(), Error> {\n\
  routes := [\n\
    pkg.web.get(\"/v1/models/featured\", h),\n\
    pkg.web.get(\"/v1/models/:id/versions\", h),\n\
    pkg.web.get(\"/files/special\", h),\n\
    pkg.web.get(\"/files/*path\", h),\n\
  ]\n\
  print(pkg.web.best(routes, \"/v1/models/featured\"))          // 0 (static wins outright)\n\
  print(pkg.web.best(routes, \"/v1/models/featured/versions\")) // 1 (static dead-ends -> :id branch)\n\
  print(pkg.web.best(routes, \"/v1/models/42/versions\"))       // 1 (plain :id path)\n\
  print(pkg.web.best(routes, \"/files/special\"))               // 2 (static wins outright)\n\
  print(pkg.web.best(routes, \"/files/special/deep\"))          // 3 (static dead-ends -> *path)\n\
  print(pkg.web.best(routes, \"/v1/models/featured/nope\"))     // -1 (no branch survives)\n\
  return Ok(())\n\
}\n";
    let files: Vec<(&str, String)> = vec![
        ("pkg/web/internal/router.align", ROUTER.to_string()),
        ("pkg/web/types.align", TYPES.to_string()),
        ("pkg/web.align", web_root.to_string()),
        ("main.align", main.to_string()),
    ];
    let refs: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    let out = build_and_run_multi("web-table-backtrack", &refs, "main.align");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n1\n1\n2\n3\n-1\n");
}
