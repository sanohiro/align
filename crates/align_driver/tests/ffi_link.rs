//! `extern "C" link("name")` — declaring an external library to link (`-lname`), beyond the
//! always-linked C runtime (libc/libm). The clause composes with the single- and braced-group extern
//! forms; names are deduped and validated (a linker gets them verbatim).

mod common;
use common::*;

fn ok(src: &str) -> bool {
    let mut sm = SourceMap::new();
    !check(&mut sm, "ffi_link", src).diags.has_errors()
}

#[test]
fn link_clause_links_and_runs() {
    if !backend_available() {
        return;
    }
    // `link("m")` names libm; `sqrt(81.0)` → 9. Proves the clause parses, threads to the linker, and
    // composes with a braced group. (libm is also auto-linked, so this shows the flag does no harm;
    // the negative test below proves the `-l` is actually emitted.)
    let out = build_and_run(
        "ffi-link-m",
        "extern \"C\" link(\"m\") {\n  fn sqrt(x: f64) -> f64\n}\n\nfn main() -> i32 {\n  unsafe {\n    return sqrt(81.0) as i32\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(9));
}

#[test]
fn bogus_library_fails_to_link() {
    if !backend_available() {
        return;
    }
    // A `link("<nonexistent>")` must actually reach the linker — proven by the link *failing* with
    // that exact library. This is the routing proof (a real extra lib can't be assumed in CI). Driven
    // manually because the `build_and_run` helper asserts linking succeeds.
    let mut sm = SourceMap::new();
    let src = "extern \"C\" link(\"align_no_such_lib_zzz\") fn whatever(x: i32) -> i32\nfn main() -> i32 {\n  unsafe { return whatever(1) }\n}\n";
    let checked = check(&mut sm, "ffi-link-bogus", src);
    assert!(!checked.diags.has_errors(), "should type-check (the library is a link-time concern)");
    let mir = lower_to_mir(&checked.hir);
    assert_eq!(mir.link_libs, vec!["align_no_such_lib_zzz".to_string()]);
    let pid = std::process::id();
    let dir = std::env::temp_dir();
    let obj = dir.join(format!("align-link-{pid}.o"));
    let exe = dir.join(format!("align-link-{pid}"));
    emit_object_file(&mir, &obj, BuildTarget::Baseline, Profile::Release, &[], false).expect("codegen");
    let linked = link_executable(&obj, &exe, &mir.link_libs, Profile::Release);
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&exe);
    assert!(linked.is_err(), "linking against a nonexistent library must fail");
}

#[test]
fn link_name_is_deduped_across_blocks() {
    if !backend_available() {
        return;
    }
    // Two blocks naming the same library collapse to one `-l` — the program still links and runs.
    let out = build_and_run(
        "ffi-link-dedup",
        "extern \"C\" link(\"m\") fn sqrt(x: f64) -> f64\nextern \"C\" link(\"m\") fn cbrt(x: f64) -> f64\n\nfn main() -> i32 {\n  unsafe {\n    return (sqrt(16.0) + cbrt(27.0)) as i32\n  }\n}\n",
    );
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn invalid_library_name_is_rejected() {
    // A name with a space / flag-like content (an injection attempt) is rejected in sema.
    assert!(!ok("extern \"C\" link(\"foo -Wl,bad\") fn f(x: i32) -> i32\nfn main() -> i32 {\n  return 0\n}\n"));
    assert!(!ok("extern \"C\" link(\"\") fn f(x: i32) -> i32\nfn main() -> i32 {\n  return 0\n}\n"));
    // A leading `-` (a flag-looking name) is rejected even though every character is otherwise valid.
    assert!(!ok("extern \"C\" link(\"-lfoo\") fn f(x: i32) -> i32\nfn main() -> i32 {\n  return 0\n}\n"));
}

#[test]
fn link_on_single_decl_form_parses() {
    assert!(ok("extern \"C\" link(\"m\") fn sqrt(x: f64) -> f64\nfn main() -> i32 {\n  unsafe {\n    return sqrt(4.0) as i32\n  }\n}\n"));
}
