//! A cached build-script executable must consume the checkout Cargo is building now.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Running {
    child: Option<Child>,
    deadline: Instant,
}

impl Drop for Running {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // Every command owns a process group, including rustc started by build.rs.
        #[cfg(unix)]
        unsafe {
            if let Ok(pid) = i32::try_from(child.id()) {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
        let _ = child.kill();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    eprintln!("build-input child cleanup failed: {error}");
                    return;
                }
            }
            if Instant::now() >= self.deadline {
                eprintln!("build-input child could not be reaped before its deadline");
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn run(command: &mut Command, scratch: &Path, label: &str) -> Output {
    let stdout = scratch.join(format!("{label}.stdout"));
    let stderr = scratch.join(format!("{label}.stderr"));
    command
        .stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
        .stderr(Stdio::from(fs::File::create(&stderr).unwrap()));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = Running {
        child: Some(command.spawn().expect("spawn build-input owner command")),
        deadline: Instant::now() + Duration::from_secs(60),
    };
    let status = loop {
        match child.child.as_mut().unwrap().try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => panic!("{label}: wait failed: {error}"),
        }
        assert!(
            // Reserve the last five seconds of the same deadline for kill/reap.
            Instant::now() < child.deadline - Duration::from_secs(5),
            "{label}: child exceeded its deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    child.child = None;
    let output = Output {
        status,
        stdout: fs::read(stdout).unwrap(),
        stderr: fs::read(stderr).unwrap(),
    };
    assert!(
        output.status.success(),
        "{label}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn reused_build_script_selects_current_checkout_for_digest_and_bitcode() {
    let root = std::env::temp_dir().join(format!(
        "align-build-inputs-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).expect("exclusively acquire scratch directory");
    let scratch = Scratch(root);
    let root = &scratch.0;
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Resolve rustup while still in this repository; child checkouts have no toolchain file.
    let sysroot = run(
        Command::new("rustc")
            .args(["--print", "sysroot"])
            .current_dir(manifest),
        root,
        "sysroot",
    );
    let rustc = PathBuf::from(String::from_utf8(sysroot.stdout).unwrap().trim()).join("bin/rustc");
    let version = run(Command::new(&rustc).arg("-vV"), root, "version");
    let version = String::from_utf8(version.stdout).unwrap();
    let target = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("rustc host");
    let hash = root.join("libalign_hash.rlib");
    run(
        Command::new(&rustc)
            .args([
                "--crate-name",
                "align_hash",
                "--crate-type",
                "lib",
                "--edition",
                "2024",
            ])
            .arg(manifest.join("../align_hash/src/lib.rs"))
            .arg("-o")
            .arg(&hash),
        root,
        "hash",
    );
    let first = root.join("first/crates/align_driver");
    let second = root.join("second checkout/crates/align_driver");
    for (checkout, value) in [(&first, 1), (&second, 2)] {
        fs::create_dir_all(checkout).unwrap();
        let source = checkout.join("../align_runtime/src");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("str_prims.rs"),
            format!(
                "#[unsafe(no_mangle)] pub extern \"C\" fn checkout_value() -> u8 {{ {value} }}\n"
            ),
        )
        .unwrap();
        // The digest must watch all runtime sources, not just the bitcode input.
        fs::write(source.join("other.rs"), format!("// checkout {value}\n")).unwrap();
    }
    let binary = root.join("build-script");
    run(
        Command::new(&rustc)
            .arg(manifest.join("build.rs"))
            .args(["--edition", "2024", "--extern"])
            .arg(format!("align_hash={}", hash.display()))
            .arg("-o")
            .arg(&binary)
            .env("CARGO_MANIFEST_DIR", &first),
        root,
        "compile-script",
    );
    let mut artifacts = Vec::new();
    for (label, checkout) in [("first", &first), ("second", &second)] {
        let out = root.join(format!("{label}-out"));
        fs::create_dir(&out).unwrap();
        let result = run(
            Command::new(&binary)
                .current_dir(checkout)
                .env("CARGO_MANIFEST_DIR", checkout)
                .env("OUT_DIR", &out)
                .env("RUSTC", &rustc)
                .env("TARGET", target),
            root,
            label,
        );
        let stdout = String::from_utf8(result.stdout).unwrap();
        let source = checkout.join("../align_runtime/src");
        let digest = align_driver::runtime_src_digest(&source).expect("fixture digest");
        for expected in [
            format!("cargo:rustc-env=ALIGN_RUNTIME_SRC_DIGEST={digest}"),
            format!("cargo:rerun-if-changed={}", source.display()),
            format!(
                "cargo:rerun-if-changed={}",
                source.join("str_prims.rs").display()
            ),
        ] {
            assert!(
                stdout.lines().any(|line| line == expected),
                "{label}: missing {expected}\n{stdout}"
            );
        }
        artifacts.push((digest, fs::read(out.join("str_prims.bc")).unwrap()));
    }
    assert_ne!(
        artifacts[0].0, artifacts[1].0,
        "different checkout contents need different digests"
    );
    assert_ne!(
        artifacts[0].1, artifacts[1].1,
        "the reused script must compile the second checkout"
    );

    // Once the original checkout disappears, the same executable still uses the live checkout.
    fs::remove_dir_all(root.join("first")).unwrap();
    let out = root.join("after-removal-out");
    fs::create_dir(&out).unwrap();
    run(
        Command::new(&binary)
            .current_dir(&second)
            .env("CARGO_MANIFEST_DIR", &second)
            .env("OUT_DIR", &out)
            .env("RUSTC", &rustc)
            .env("TARGET", target),
        root,
        "after-removal",
    );
    assert_eq!(fs::read(out.join("str_prims.bc")).unwrap(), artifacts[1].1);
}
