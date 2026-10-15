//! Plan200: the shared harness owns each filesystem stage from acquisition through Drop.
#![cfg(unix)]
mod common;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

use common::*;
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

fn children(root: &Path) -> BTreeSet<PathBuf> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect()
}

fn clean_after_panic(root: &Path, action: impl FnOnce() + std::panic::UnwindSafe) {
    let before = children(root);
    assert!(
        std::panic::catch_unwind(action).is_err(),
        "failure injection did not fail"
    );
    assert_eq!(children(root), before, "partially constructed stage leaked");
}

fn assert_code(exe: &Path, expected: i32) {
    assert_eq!(Command::new(exe).status().unwrap().code(), Some(expected));
}

#[test]
fn legacy_module_roots_and_nested_sources() {
    owned_fixture::run("legacy_module_roots_and_nested_sources", |root| {
        let target = root.join("unrelated");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("sentinel"), "untouched").unwrap();
        for is_link in [false, true] {
            let name = if is_link {
                "legacy-link"
            } else {
                "legacy-directory"
            };
            let legacy = root.join(format!("align-mtest-{}-{name}", std::process::id()));
            if is_link {
                symlink(&target, &legacy).unwrap();
            } else {
                fs::create_dir(&legacy).unwrap();
                fs::write(legacy.join("sentinel"), "untouched").unwrap();
            }
            let before = children(root);
            let checked = diff_check_multi(
                name,
                &[
                    (
                        "./nested/main.align",
                        "import helper\nfn main() -> i32 = helper.answer()\n",
                    ),
                    (
                        "nested/helper.align",
                        "module helper\npub fn answer() -> i32 = 42\n",
                    ),
                ],
                "./nested/main.align",
            );
            assert!(!checked.whole_errors, "{}", checked.whole_diags);
            assert!(!checked.per_unit_errors, "{}", checked.per_unit_diags);
            assert_eq!(children(root), before);
            assert_eq!(
                fs::read_to_string(legacy.join("sentinel")).unwrap(),
                "untouched"
            );
            assert_eq!(
                fs::symlink_metadata(&legacy)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                is_link
            );
        }
        assert_eq!(
            fs::read_to_string(target.join("sentinel")).unwrap(),
            "untouched"
        );
    });
}

#[test]
fn partial_writes_and_invalid_paths_retire_the_stage() {
    owned_fixture::run(
        "partial_writes_and_invalid_paths_retire_the_stage",
        |root| {
            clean_after_panic(root, || {
                diff_check_multi(
                    "conflict",
                    &[("conflict", "first"), ("conflict/child", "second")],
                    "main.align",
                );
            });
            clean_after_panic(root, || {
                Proj::new(
                    "conflict",
                    &[("conflict", "first"), ("conflict/child", "second")],
                    "main.align",
                );
            });
            let absolute = root.join("outside.align");
            for name in ["", ".", "../outside.align", absolute.to_str().unwrap()] {
                clean_after_panic(root, || {
                    diff_check_multi("invalid", &[(name, "first")], "main.align");
                });
                clean_after_panic(root, || {
                    Proj::new("invalid", &[(name, "first")], "main.align");
                });
                clean_after_panic(root, || {
                    diff_check_multi("invalid", &[("main.align", "fn main() {}")], name);
                });
                clean_after_panic(root, || {
                    Proj::new("invalid", &[], name);
                });
            }
            assert!(!absolute.exists());
        },
    );
}

#[test]
fn simultaneous_projects_and_retargeted_paths_keep_their_own_guard() {
    owned_fixture::run(
        "simultaneous_projects_and_retargeted_paths_keep_their_own_guard",
        |root| {
            let left =
                std::thread::spawn(|| Proj::new("same", &[("main.align", "left")], "main.align"));
            let right =
                std::thread::spawn(|| Proj::new("same", &[("main.align", "right")], "main.align"));
            let mut left = left.join().unwrap();
            let right = right.join().unwrap();
            let left_root = left.dir.clone();
            let right_root = right.dir.clone();
            assert_ne!(left_root, right_root);
            assert_eq!(
                fs::read_to_string(left_root.join("main.align")).unwrap(),
                "left"
            );
            assert_eq!(
                fs::read_to_string(right_root.join("main.align")).unwrap(),
                "right"
            );
            let sentinel = root.join("unowned");
            fs::create_dir(&sentinel).unwrap();
            fs::write(sentinel.join("sentinel"), "untouched").unwrap();
            left.dir = sentinel.clone();
            drop(left);
            assert!(!left_root.exists());
            assert!(right_root.exists());
            assert_eq!(
                fs::read_to_string(sentinel.join("sentinel")).unwrap(),
                "untouched"
            );
            drop(right);
            assert!(!right_root.exists());
            clean_after_panic(root, || {
                let _project = Proj::new("unwind", &[("main.align", "first")], "main.align");
                panic!("post-construction failure");
            });
        },
    );
}

#[test]
fn single_file_native_outputs_preserve_sentinels_and_live_guards() {
    owned_fixture::run(
        "single_file_native_outputs_preserve_sentinels_and_live_guards",
        |root| {
            assert!(
                backend_available() && cc_available(),
                "native owner requires LLVM and cc"
            );
            let sentinel = root.join("sentinel");
            fs::write(&sentinel, "untouched").unwrap();
            for suffix in [".o", std::env::consts::EXE_SUFFIX] {
                symlink(
                    &sentinel,
                    root.join(format!("align-test-{}-same{suffix}", std::process::id())),
                )
                .unwrap();
            }
            let before = children(root);
            let output = build_and_run("same", "fn main() -> i32 = 40");
            assert_eq!(output.status.code(), Some(40));
            assert_eq!(children(root), before);
            assert_eq!(fs::read_to_string(&sentinel).unwrap(), "untouched");
            let mut left = build_exe("same", "fn main() -> i32 = 41");
            let right = build_exe("same", "fn main() -> i32 = 42");
            assert_ne!(left.exe, right.exe);
            assert_code(&left.exe, 41);
            assert_code(&right.exe, 42);
            let left_root = left.exe.parent().unwrap().to_path_buf();
            left.exe = sentinel.clone();
            drop(left);
            assert!(!left_root.exists());
            assert_code(&right.exe, 42);
            drop(right);
            assert_eq!(children(root), before);
            assert_eq!(fs::read_to_string(&sentinel).unwrap(), "untouched");
            let output = build_and_run("path/../../label", "fn main() -> i32 = 43");
            assert_eq!(output.status.code(), Some(43));
            clean_after_panic(root, || {
                let _built = build_exe("unwind", "fn main() {}");
                panic!("post-link failure");
            });
        },
    );
}

const C_ALIGN: &str = "extern \"C\" fn fixture_value() -> i32\nfn main() -> i32 { unsafe { return fixture_value() } }\n";
const C_SOURCE: &str = "int fixture_value(void) { return 42; }\n";

#[test]
fn c_fixture_roots_and_failed_native_construction_are_owned() {
    owned_fixture::run(
        "c_fixture_roots_and_failed_native_construction_are_owned",
        |root| {
            assert!(
                backend_available() && cc_available(),
                "native owner requires LLVM and cc"
            );
            let sentinel = root.join("sentinel");
            fs::write(&sentinel, "untouched").unwrap();
            for suffix in [".o", ".c", "-helper.o", std::env::consts::EXE_SUFFIX] {
                symlink(
                    &sentinel,
                    root.join(format!("align-ffic-{}-same{suffix}", std::process::id())),
                )
                .unwrap();
            }
            let before = children(root);
            assert_eq!(
                build_and_run_with_c("same", C_ALIGN, C_SOURCE)
                    .status
                    .code(),
                Some(42)
            );
            assert_eq!(children(root), before);
            assert_eq!(fs::read_to_string(&sentinel).unwrap(), "untouched");
            clean_after_panic(root, || {
                build_and_run_with_c("bad-c", C_ALIGN, "#error injected failure\n");
            });
            clean_after_panic(root, || {
                build_exe("undefined-symbol", C_ALIGN);
            });
            assert_eq!(
                build_and_run_multi_with_c(
                    "path/../../label",
                    &[("main.align", C_ALIGN)],
                    "main.align",
                    C_SOURCE
                )
                .status
                .code(),
                Some(42)
            );
            assert_eq!(children(root), before);
        },
    );
}

#[test]
fn returned_multifile_and_per_unit_artifacts_live_until_drop() {
    owned_fixture::run(
        "returned_multifile_and_per_unit_artifacts_live_until_drop",
        |root| {
            assert!(
                backend_available() && cc_available(),
                "native owner requires LLVM and cc"
            );
            let before = children(root);
            let left = build_exe_multi(
                "same",
                &[("main.align", "fn main() -> i32 = 41")],
                "main.align",
            );
            let right = build_exe_multi(
                "same",
                &[("main.align", "fn main() -> i32 = 42")],
                "main.align",
            );
            assert_ne!(left.exe, right.exe);
            assert_code(&left.exe, 41);
            let left_root = left.exe.parent().unwrap().to_path_buf();
            drop(left);
            assert!(!left_root.exists());
            assert_code(&right.exe, 42);
            drop(right);
            let built = build_per_unit_multi(
                "path/../../label",
                &[("main.align", "fn main() -> i32 = 43")],
                "main.align",
            );
            let path = built.dir.clone();
            assert_eq!(built.link_and_run().status.code(), Some(43));
            assert!(path.join("unit0.o").is_file());
            drop(built);
            assert!(!path.exists());
            assert_eq!(children(root), before);
        },
    );
}
