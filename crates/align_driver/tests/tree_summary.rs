//! Execute the retained-directory composition example, including its failure cleanup.
mod common;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use common::*;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

fn source() -> &'static str {
    fixture("examples/tree_summary.align")
}

struct Fixture {
    directory: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "align-tree-summary-{}-{name}-{}",
            std::process::id(),
            thin_nonce()
        ));
        std::fs::create_dir(&directory).expect("create fixture");
        // Arm cleanup before canonicalization, directory creation or any other fallible step.
        let mut fixture = Self {
            root: directory.join("tree"),
            directory,
        };
        fixture.root = std::fs::canonicalize(&fixture.directory)
            .expect("canonical fixture")
            .join("tree");
        std::fs::create_dir(&fixture.root).expect("create tree");
        fixture
    }

    fn path(&self) -> &str {
        self.root.to_str().expect("fixture UTF-8 root")
    }

    fn populated(&self) -> (i64, i64) {
        let nested = self.root.join("nested");
        std::fs::create_dir(&nested).expect("nested");
        std::fs::create_dir(nested.join("empty")).expect("empty");
        std::fs::write(self.root.join("clip.wav"), b"abc").expect("audio");
        std::fs::hard_link(self.root.join("clip.wav"), self.root.join("clip-alias"))
            .expect("hard link");
        std::fs::write(nested.join("frame.ppm"), b"12345").expect("image");
        std::fs::write(self.root.join("字幕"), b"1234567").expect("Unicode name");
        let outside = self.directory.join("outside");
        std::fs::create_dir(&outside).expect("outside");
        std::fs::write(outside.join("excluded"), [0; 100]).expect("outside content");
        std::os::unix::fs::symlink(&outside, self.root.join("external")).expect("external link");
        std::os::unix::fs::symlink("missing", self.root.join("dangling")).expect("dangling link");
        std::os::unix::fs::symlink(".", self.root.join("cycle")).expect("cycle link");
        let fifo = std::ffi::CString::new(self.root.join("fifo").as_os_str().as_bytes())
            .expect("FIFO path");
        // SAFETY: a live terminated fixture path; mkfifo creates no open descriptor.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let (mut files, mut bytes) = (4, 18);
        if cfg!(target_os = "linux") {
            std::fs::write(
                self.root.join(std::ffi::OsStr::from_bytes(b"raw-\xff")),
                [0; 11],
            )
            .expect("raw byte name");
            std::fs::write(self.root.join("line\nname"), [0; 2]).expect("newline name");
            files += 2;
            bytes += 13;
        }
        (files, bytes)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn expected(files: i64, bytes: i64) -> String {
    format!(
        "entries\n{}\ndirectories\n3\nregular_files\n{files}\nsymlinks\n3\nother\n1\nlogical_bytes\n{bytes}\n",
        files + 7
    )
}

#[test]
fn byte_names_kinds_and_logical_sizes() {
    if !backend_available() {
        return;
    }
    let fixture = Fixture::new("kinds");
    let (files, bytes) = fixture.populated();
    let executable = build_exe("tree-summary-kinds", source());
    let defaults = std::process::Command::new(&executable.exe)
        .current_dir(&fixture.root)
        .output()
        .expect("default root");
    assert!(defaults.status.success());
    assert_eq!(defaults.stdout, expected(files, bytes).as_bytes());
    let exact = (files + 7).to_string();
    for extra in [vec![], vec!["--max-depth=2", "--max-entries", &exact]] {
        let output = std::process::Command::new(&executable.exe)
            .args(["--root", fixture.path()])
            .args(extra)
            .output()
            .expect("run example");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected(files, bytes).as_bytes());
    }
    let exceeded = std::process::Command::new(&executable.exe)
        .args([
            "--root",
            fixture.path(),
            "--max-entries",
            &(files + 6).to_string(),
        ])
        .output()
        .expect("shared recursive budget");
    assert!(!exceeded.status.success());
    assert!(exceeded.stdout.is_empty());
}

#[test]
fn exact_limits_and_pre_io_validation() {
    if !backend_available() {
        return;
    }
    let fixture = Fixture::new("limits");
    let executable = build_exe("tree-summary-limits", source());
    let empty = std::process::Command::new(&executable.exe)
        .args([
            "--root",
            fixture.path(),
            "--max-depth",
            "0",
            "--max-entries",
            "1",
        ])
        .output()
        .expect("empty tree");
    assert!(empty.status.success());
    assert_eq!(
        empty.stdout,
        b"entries\n1\ndirectories\n1\nregular_files\n0\nsymlinks\n0\nother\n0\nlogical_bytes\n0\n"
    );
    std::fs::create_dir(fixture.root.join("child")).expect("empty child");
    for (depth, entries, success) in [(1, 2, true), (0, 2, false), (1, 1, false)] {
        let output = std::process::Command::new(&executable.exe)
            .args([
                "--root",
                fixture.path(),
                "--max-depth",
                &depth.to_string(),
                "--max-entries",
                &entries.to_string(),
            ])
            .output()
            .expect("boundary");
        assert_eq!(output.status.success(), success);
        if !success {
            assert!(output.stdout.is_empty(), "partial summary escaped");
        }
    }
    let missing = fixture.root.join("missing");
    let missing = missing.to_str().expect("missing path");
    for invalid in [
        vec!["--max-depth", "-1"],
        vec!["--max-depth", "65"],
        vec!["--max-entries", "0"],
        vec!["--max-entries", "1048577"],
        vec!["--max-depth", "bad"],
        vec!["--unknown"],
    ] {
        let output = std::process::Command::new(&executable.exe)
            .args(["--root", missing])
            .args(invalid)
            .output()
            .expect("invalid input");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).trim(),
            "error: code 2"
        );
    }
    let help = std::process::Command::new(&executable.exe)
        .args(["--help", "--root", missing, "--max-depth", "65"])
        .output()
        .expect("help without I/O");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--max-entries"));
}

fn probe_source(main: &str) -> String {
    format!(
        "{}\n{main}",
        source().replace("pub fn main(", "fn example_main(")
    )
}

#[test]
fn source_formation_and_native_per_unit_walk() {
    let checked = diff_check_multi(
        "tree-summary-formation",
        &[("main.align", source())],
        "main.align",
    );
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:{}\nunit:{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if !backend_available() {
        return;
    }
    let fixture = Fixture::new("per-unit");
    let (files, bytes) = fixture.populated();
    let source = probe_source(&format!(
        r#"
fn main() -> Result<(), Error> {{
  directory := fs.open_directory({:?})?
  counts := walk(directory, 2, {}, Counts {{ entries: 1, directories: 1, regular_files: 0, symlinks: 0, other: 0, logical_bytes: 0 }})?
  print(counts.entries)
  print(counts.logical_bytes)
  return Ok(())
}}
"#,
        fixture.path(),
        files + 7
    ));
    let built = build_per_unit_multi(
        "tree-summary-per-unit",
        &[("main.align", &source)],
        "main.align",
    );
    let output = built.link_and_run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        format!("{}\n{bytes}\n", files + 7).as_bytes()
    );
}

#[test]
fn recursion_error_cleanup_and_checked_sizes() {
    if !backend_available() || !cc_available() {
        return;
    }
    let fixture = Fixture::new("cleanup");
    let (files, _) = fixture.populated();
    let source = probe_source(&format!(
        r#"
extern "C" fn tree_fd_count() -> i64
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
extern "C" fn align_rt_alloc_count() -> i64
fn exercise(depth: i64, limit: i64) -> Result<(), Error> {{
  directory := fs.open_directory({:?})?
  counts := walk(directory, depth, limit, Counts {{ entries: 1, directories: 1, regular_files: 0, symlinks: 0, other: 0, logical_bytes: 0 }})?
  return Ok(())
}}
fn main() -> i32 {{
  if (add_size(9223372036854775807, 1) else {{ -1 }}) != -1 {{ return 1 }}
  if (add_size(0, -1) else {{ -1 }}) != -1 {{ return 2 }}
  if (add_size(9223372036854775806, 1) else {{ -1 }}) != 9223372036854775807 {{ return 3 }}
  unsafe {{ align_rt_requested_live_reset() }}
  before := unsafe {{ tree_fd_count() }}
  allocations := unsafe {{ align_rt_alloc_count() }}
  if before < 0 {{ return 4 }}
  mut iteration := 0
  loop {{
    if iteration == 50 {{ break }}
    match exercise(2, {}) {{ Ok(_) => {{}}, Err(_) => {{ return 5 }} }}
    match exercise(2, 1) {{ Ok(_) => {{ return 6 }}, Err(_) => {{}} }}
    match exercise(0, {}) {{ Ok(_) => {{ return 7 }}, Err(_) => {{}} }}
    match exercise(1, {}) {{ Ok(_) => {{ return 9 }}, Err(_) => {{}} }}
    if unsafe {{ tree_fd_count() }} != before || unsafe {{ align_rt_requested_live_bytes() }} != 0 {{ return 8 }}
    iteration = iteration + 1
  }}
  if unsafe {{ align_rt_alloc_count() }} == allocations {{ return 10 }}
  return 0
}}
"#,
        fixture.path(),
        files + 7,
        files + 7,
        files + 7
    ));
    let output = build_and_run_with_c(
        "tree-summary-cleanup",
        &source,
        r#"
#include <dirent.h>
#include <stdint.h>
#include <string.h>
int64_t tree_fd_count(void) {
    DIR *directory = opendir("/dev/fd");
    if (!directory) return -1;
    int64_t count = 0;
    struct dirent *entry;
    while ((entry = readdir(directory))) {
        if (strcmp(entry->d_name, ".") && strcmp(entry->d_name, "..")) ++count;
    }
    closedir(directory);
    return count;
}
"#,
    );
    assert!(
        output.status.success(),
        "exit={:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn checked_summary_and_help_output() {
    owned_fixture::run("checked_summary_and_help_output", |root| {
        assert!(backend_available(), "tree output owner requires LLVM");
        let fixture = Fixture::new("checked-output");
        let whole = build_exe("tree-checked-output-whole", source());
        let units = build_per_unit_multi("tree-checked-output-units",
            &[("main.align", source())], "main.align");
        let objects = units.emit_objects(false);
        let refs: Vec<_> = objects.iter().map(|path| path.as_path()).collect();
        let unit_exe = units.dir.join(format!("tree{}", std::env::consts::EXE_SUFFIX));
        link_objects(&align_driver::CDriver::default(), &refs, &unit_exe,
            &units.link_libs_union(), Profile::Release).unwrap();
        let readonly = root.join("readonly");
        std::fs::write(&readonly, b"unchanged").unwrap();
        let missing = fixture.directory.join("missing");
        let missing = missing.to_str().unwrap();
        for exe in [&whole.exe, &unit_exe] {
            for (args, error) in [
                (vec!["--root", fixture.path()], libc::EBADF),
                (vec!["--help", "--root", missing, "--max-depth", "65"], libc::EBADF),
                (vec!["--root", missing], 1),
                (vec!["--root", fixture.path(), "--max-depth", "65"], 2),
                (vec!["--help", "--unknown"], 2),
            ] {
                // All compile/link/run descendants remain in the parent's bounded group;
                // file-backed streams cannot deadlock on capture pipe capacity.
                let status = std::process::Command::new(exe).args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::fs::File::open(&readonly).unwrap())
                    .stderr(std::fs::File::create(root.join("stderr")).unwrap())
                    .status().unwrap();
                let stderr = std::fs::read(root.join("stderr")).unwrap();
                assert_eq!(status.code(), Some(error), "{args:?}: {stderr:?}");
                assert_eq!(stderr, format!("error: code {error}\n").as_bytes(), "{args:?}");
                assert_eq!(std::fs::read(&readonly).unwrap(), b"unchanged");
            }
            let output = std::process::Command::new(exe)
                .args(["--root", fixture.path()]).output().unwrap();
            assert_eq!(output.status.code(), Some(0), "{output:?}");
            assert!(output.stderr.is_empty());
            assert_eq!(output.stdout,
                b"entries\n1\ndirectories\n1\nregular_files\n0\nsymlinks\n0\nother\n0\nlogical_bytes\n0\n");
        }
    });
}
