//! Actual file-copy examples: preserve occupied destinations and observe final flush failure.

mod common;

use std::io::{ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

struct ChildOwner {
    child: Option<Child>,
    deadline: Instant,
}

impl Drop for ChildOwner {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else { return };
        if let Ok(pid) = i32::try_from(child.id()) {
            loop {
                // SAFETY: this live owned child leads a fresh process group.
                if unsafe { libc::kill(-pid, libc::SIGKILL) } == 0 { break; }
                if std::io::Error::last_os_error().kind() != ErrorKind::Interrupted
                    || Instant::now() >= self.deadline { break; }
            }
        }
        loop {
            match child.kill() {
                Err(error) if error.kind() == ErrorKind::Interrupted
                    && Instant::now() < self.deadline => {},
                _ => break,
            }
        }
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(1)),
            }
            if Instant::now() >= self.deadline {
                eprintln!("copy fixture kill/reap deadline exceeded");
                break;
            }
        }
    }
}

fn nonblocking(fd: std::os::fd::RawFd) {
    // SAFETY: both calls operate on a live pipe owned by this fixture.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0, "get pipe flags: {}", std::io::Error::last_os_error());
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) }, 0);
}

fn drain(pipe: &mut impl Read, bytes: &mut Vec<u8>) -> bool {
    let mut scratch = [0_u8; 4096];
    // One read per outer iteration keeps EINTR and continuous output within the deadline.
    match pipe.read(&mut scratch) {
        Ok(0) => true,
        Ok(n) => {
            assert!(bytes.len() + n <= 65536, "copy fixture output cap");
            bytes.extend_from_slice(&scratch[..n]);
            false
        }
        Err(error) if matches!(error.kind(), ErrorKind::Interrupted | ErrorKind::WouldBlock) => false,
        Err(error) => panic!("read copy fixture pipe: {error}"),
    }
}

fn run(command: &mut Command) -> Output {
    command.process_group(0).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut owner = ChildOwner { child: Some(command.spawn().expect("spawn copy fixture")), deadline };
    let mut stdout = owner.child.as_mut().unwrap().stdout.take().unwrap();
    let mut stderr = owner.child.as_mut().unwrap().stderr.take().unwrap();
    nonblocking(stdout.as_raw_fd());
    nonblocking(stderr.as_raw_fd());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let (mut out_end, mut err_end) = (false, false);
    let mut status = None;
    loop {
        assert!(Instant::now() + Duration::from_secs(5) < deadline, "copy fixture work deadline");
        if !out_end { out_end = drain(&mut stdout, &mut out); }
        if !err_end { err_end = drain(&mut stderr, &mut err); }
        if let Some(child) = owner.child.as_mut() {
            match child.try_wait() {
                Ok(Some(exit)) => {
                    status = Some(exit);
                    // The known example/one-shot compiler has no detached helpers. Do not retain
                    // a stale PID after reaping while draining its already-written pipe bytes.
                    owner.child.take();
                }
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll copy fixture: {error}"),
            }
        }
        if let Some(status) = status && out_end && err_end {
            return Output { status, stdout: out, stderr: err };
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn build(stage: &Path, name: &str) -> std::path::PathBuf {
    build_source(stage, common::fixture(&format!("examples/{name}.align")))
}

fn build_source(stage: &Path, source: &str) -> std::path::PathBuf {
    std::fs::write(stage.join("main.align"), source).expect("write actual example");
    let result = run(Command::new(env!("CARGO_BIN_EXE_alignc"))
        .args(["build", "main.align", "--profile", "dev"])
        .current_dir(stage).env("ALIGNC_CACHE", "off").env("TMPDIR", stage));
    assert!(result.status.success(), "{result:?}");
    stage.join(format!("main{}", std::env::consts::EXE_SUFFIX))
}

fn assert_error(output: &Output, code: i32) {
    assert_eq!(output.status.code(), Some(code.clamp(1, 255)), "{output:?}");
    assert!(output.stdout.is_empty(), "no success count on failure: {output:?}");
    assert_eq!(output.stderr, format!("error: code {code}\n").as_bytes(), "{output:?}");
}

#[test]
fn actual_copy_example_propagates_read_window_refusal() {
    assert!(common::backend_available(), "example owner requires the LLVM backend");
    let source = common::fixture("examples/file_copy.align");
    assert_eq!(source.matches("buffer.try_new(65536)").count(), 1);
    let refused = format!("{}\nfn refused_window() -> Result<buffer, Error> = Err(Error.Code(12))\n",
        source.replace("buffer.try_new(65536)", "refused_window()"));
    let stage = align_driver::ArtifactStage::temp("copy-refused-window").unwrap();
    let root = stage.path();
    let exe = build_source(root, &refused);
    let input = root.join("source");
    let destination = root.join("destination");
    for bytes in [b"unread\0\xff".as_slice(), b""] {
        std::fs::write(&input, bytes).unwrap();
        let output = run(Command::new(&exe).arg(&input).arg(&destination));
        assert_error(&output, 12);
        assert_eq!(std::fs::read(&input).unwrap(), bytes);
        assert!(std::fs::read(&destination).unwrap().is_empty());
        std::fs::remove_file(&destination).unwrap();
    }
}

#[test]
fn actual_copy_examples_preserve_occupied_destinations_and_complete_binary_output() {
    assert!(common::backend_available(), "example owner requires the LLVM backend");
    for name in ["file_copy", "io_copy"] {
        let stage = align_driver::ArtifactStage::temp("copy-example").expect("exclusive fixture");
        let root = stage.path();
        let exe = build(root, name);
        let source = root.join("source");
        let destination = root.join("destination");
        for size in [0, 7, 2 * 65536 + 19] {
            let data: Vec<u8> = (0..size).map(|index| (index % 256) as u8).collect();
            std::fs::write(&source, &data).unwrap();
            let output = run(Command::new(&exe).arg(&source).arg(&destination));
            assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
            assert!(output.stderr.is_empty(), "{output:?}");
            let mut expected = data.clone();
            if name == "io_copy" { expected.push(b'\n'); }
            assert_eq!(std::fs::read(&destination).unwrap(), expected, "{name}");
            assert_eq!(std::fs::read(&source).unwrap(), data);
            assert_eq!(output.stdout, if name == "io_copy" { format!("{size}\n").into_bytes() } else { Vec::new() });
            std::fs::remove_file(&destination).unwrap();
        }
        if name == "io_copy" {
            let readonly = std::fs::File::open(&source).unwrap();
            let fd = readonly.as_raw_fd();
            let mut command = Command::new(&exe);
            command.arg(&source).arg(&destination);
            // SAFETY: the parent keeps this read-only descriptor live until the guarded
            // child exits. dup2 is async-signal-safe and changes only the child's stdout.
            unsafe { command.pre_exec(move || {
                if libc::dup2(fd, libc::STDOUT_FILENO) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            }); }
            let output = run(&mut command);
            assert_error(&output, libc::EBADF);
            let data = std::fs::read(&source).unwrap();
            let mut expected = data.clone();
            expected.push(b'\n');
            assert_eq!(std::fs::read(&destination).unwrap(), expected,
                "count failure keeps the completed copy");
            assert_eq!(data, (0..2 * 65536 + 19).map(|index| (index % 256) as u8).collect::<Vec<_>>());
            std::fs::remove_file(&destination).unwrap();
        }
        let original = b"input must survive\0\xff\n";
        std::fs::write(&source, original).unwrap();
        for kind in ["file", "same", "hardlink", "symlink", "dangling", "directory"] {
            match kind {
                "file" => std::fs::write(&destination, b"existing destination").unwrap(),
                "hardlink" => std::fs::hard_link(&source, &destination).unwrap(),
                "symlink" => std::os::unix::fs::symlink(&source, &destination).unwrap(),
                "dangling" => std::os::unix::fs::symlink(root.join("absent"), &destination).unwrap(),
                "directory" => std::fs::create_dir(&destination).unwrap(),
                "same" => {},
                _ => unreachable!(),
            }
            let target = if kind == "same" { &source } else { &destination };
            let before = std::fs::symlink_metadata(target).unwrap();
            let output = run(Command::new(&exe).arg(&source).arg(target));
            assert_error(&output, libc::EEXIST);
            assert_eq!(std::fs::read(&source).unwrap(), original, "{name}: {kind}");
            use std::os::unix::fs::MetadataExt;
            let after = std::fs::symlink_metadata(target).unwrap();
            assert_eq!((after.dev(), after.ino(), after.mode()), (before.dev(), before.ino(), before.mode()));
            if kind == "file" { assert_eq!(std::fs::read(target).unwrap(), b"existing destination"); }
            if kind == "symlink" { assert_eq!(std::fs::read_link(target).unwrap(), source); }
            if kind == "dangling" { assert_eq!(std::fs::read_link(target).unwrap(), root.join("absent")); }
            match kind {
                "same" => {},
                "directory" => std::fs::remove_dir(target).unwrap(),
                _ => std::fs::remove_file(target).unwrap(),
            }
        }
        for arguments in [vec![], vec![source.as_path()], vec![source.as_path(), destination.as_path(), source.as_path()]] {
            let output = run(Command::new(&exe).args(arguments));
            assert_error(&output, 2);
            assert!(!destination.exists());
            assert_eq!(std::fs::read(&source).unwrap(), original);
        }
        let output = run(Command::new(&exe).arg(root.join("missing")).arg(&destination));
        assert_error(&output, 1);
        assert!(!destination.exists());
    }
}

#[test]
fn actual_copy_examples_report_final_flush_failure_before_success() {
    assert!(common::backend_available(), "example owner requires the LLVM backend");
    for name in ["file_copy", "io_copy"] {
        let stage = align_driver::ArtifactStage::temp("copy-flush").expect("exclusive fixture");
        let root = stage.path();
        let exe = build(root, name);
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::write(&source, b"short buffered tail").unwrap();
        let mut command = Command::new(&exe);
        command.arg(&source).arg(&destination);
        // SAFETY: only async-signal-safe native setup runs after fork. The parent and other tests
        // retain their own limit and signal disposition; stdout/stderr are pipes, not regular files.
        unsafe { command.pre_exec(|| {
            let limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0
                || libc::signal(libc::SIGXFSZ, libc::SIG_IGN) == libc::SIG_ERR {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }); }
        let output = run(&mut command);
        assert_error(&output, libc::EFBIG);
        assert_eq!(std::fs::read(source).unwrap(), b"short buffered tail");
        assert_eq!(std::fs::metadata(destination).unwrap().len(), 0);
    }
}
