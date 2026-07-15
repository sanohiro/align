use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "align-artifact-staging-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn source(dir: &Path, value: i64) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("main.align");
    std::fs::write(
        &path,
        format!(
            "fn main() -> Result<(), Error> {{\n  print({value})\n  return Ok(())\n}}\n"
        ),
    )
    .unwrap();
    path
}

fn wait_pair(a: std::process::Child, b: std::process::Child) -> (Output, Output) {
    let ao = a.wait_with_output().unwrap();
    let bo = b.wait_with_output().unwrap();
    (ao, bo)
}

#[test]
fn same_basename_concurrent_run_and_build_stay_isolated() {
    let scratch = Scratch::new();
    let a_dir = scratch.0.join("a");
    let b_dir = scratch.0.join("b");
    let a_src = source(&a_dir, 111);
    let b_src = source(&b_dir, 222);
    let alignc = env!("CARGO_BIN_EXE_alignc");

    for round in 0..12 {
        let a = Command::new(alignc).env("ALIGNC_CACHE", "off").args(["run", a_src.to_str().unwrap()]).stdout(std::process::Stdio::piped()).spawn().unwrap();
        let b = Command::new(alignc).env("ALIGNC_CACHE", "off").args(["run", b_src.to_str().unwrap()]).stdout(std::process::Stdio::piped()).spawn().unwrap();
        let (a, b) = wait_pair(a, b);
        assert!(a.status.success(), "round {round}: first run failed: {a:?}");
        assert!(b.status.success(), "round {round}: second run failed: {b:?}");
        assert_eq!(a.stdout, b"111\n", "round {round}: first run used the wrong artifact");
        assert_eq!(b.stdout, b"222\n", "round {round}: second run used the wrong artifact");
    }

    let a_out = scratch.0.join("out-a");
    let b_out = scratch.0.join("out-b");
    std::fs::create_dir(&a_out).unwrap();
    std::fs::create_dir(&b_out).unwrap();
    for round in 0..12 {
        let a = Command::new(alignc).env("ALIGNC_CACHE", "off").arg("build").arg(&a_src).current_dir(&a_out).stdout(std::process::Stdio::null()).spawn().unwrap();
        let b = Command::new(alignc).env("ALIGNC_CACHE", "off").arg("build").arg(&b_src).current_dir(&b_out).stdout(std::process::Stdio::null()).spawn().unwrap();
        let (a, b) = wait_pair(a, b);
        assert!(a.status.success(), "round {round}: first build failed: {a:?}");
        assert!(b.status.success(), "round {round}: second build failed: {b:?}");
        let a_run = Command::new(a_out.join("main")).output().unwrap();
        let b_run = Command::new(b_out.join("main")).output().unwrap();
        assert_eq!(a_run.stdout, b"111\n", "round {round}: first build linked the wrong object");
        assert_eq!(b_run.stdout, b"222\n", "round {round}: second build linked the wrong object");
    }

    for round in 0..8 {
        let a = Command::new(alignc).env("ALIGNC_CACHE", "off").args(["size", a_src.to_str().unwrap()]).stdout(std::process::Stdio::piped()).spawn().unwrap();
        let b = Command::new(alignc).env("ALIGNC_CACHE", "off").args(["size", b_src.to_str().unwrap()]).stdout(std::process::Stdio::piped()).spawn().unwrap();
        let (a, b) = wait_pair(a, b);
        assert!(a.status.success(), "round {round}: first size report failed: {a:?}");
        assert!(b.status.success(), "round {round}: second size report failed: {b:?}");
        assert!(String::from_utf8_lossy(&a.stdout).contains("total size:"));
        assert!(String::from_utf8_lossy(&b.stdout).contains("total size:"));
    }
}
