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
    let linked = link_executable(&align_driver::CDriver::default(), &obj, &exe, &mir.link_libs, Profile::Release);
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

#[cfg(target_os = "linux")]
mod archive_math {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Child, Command, ExitStatus};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            for _ in 0..128 {
                let path = std::env::temp_dir().join(format!(
                    "align-archive-math-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create fixture directory: {error}"),
                }
            }
            panic!("fixture directory collision limit exceeded");
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Process(Option<Child>);
    impl Drop for Process {
        fn drop(&mut self) {
            if let Some(child) = self.0.as_mut() {
                if let Ok(pid) = i32::try_from(child.id()) {
                    // SAFETY: every child starts a new process group whose id is its pid.
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    fn run(command: &mut Command) -> ExitStatus {
        let mut process = Process(Some(
            command
                .process_group(0)
                .spawn()
                .expect("spawn fixture tool"),
        ));
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            assert!(
                Instant::now() < deadline,
                "fixture tool exceeded 60 seconds: {command:?}"
            );
            match process.0.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    process.0.take();
                    return status;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => panic!("poll fixture tool: {error}"),
            }
        }
    }

    #[test]
    fn user_static_archive_resolves_automatic_math_support() {
        if !backend_available() {
            return;
        }
        let dir = Directory::new();
        std::fs::write(
            dir.0.join("fixture.c"),
            "#include <math.h>\nfloat fixture_exp(float value) { return expf(value); }\n",
        )
        .unwrap();
        std::fs::write(
            dir.0.join("shim.align"),
            "module shim\nextern \"C\" link(\"fixture\") { fn fixture_exp(value: f32) -> f32 }\n\
             pub fn value() -> f32 { unsafe { return fixture_exp(1.0) } }\n",
        )
        .unwrap();
        std::fs::write(
            dir.0.join("main.align"),
            "import shim\nfn main() -> i32 = shim.value() as i32\n",
        )
        .unwrap();
        assert!(
            run(Command::new("cc").current_dir(&dir.0).args([
                "-O0",
                "-fno-builtin",
                "-c",
                "fixture.c",
                "-o",
                "fixture.o"
            ]))
            .success()
        );
        assert!(
            run(Command::new("ar")
                .current_dir(&dir.0)
                .args(["crs", "libfixture.a", "fixture.o"]))
            .success()
        );
        let mut paths = vec![dir.0.clone()];
        if let Some(existing) = std::env::var_os("LIBRARY_PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        assert!(
            run(Command::new(env!("CARGO_BIN_EXE_alignc"))
                .current_dir(&dir.0)
                .args(["build", "main.align", "--no-rt-lto"])
                .env("ALIGNC_LINKER", "system")
                .env("ALIGNC_CACHE", "off")
                .env("LIBRARY_PATH", std::env::join_paths(paths).unwrap()))
            .success()
        );
        assert_eq!(run(&mut Command::new(dir.0.join("main"))).code(), Some(2));
    }
}
