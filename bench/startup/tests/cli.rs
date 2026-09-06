use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static WRAPPER_LOCK: Mutex<()> = Mutex::new(());
const OUTPUT_MAX: u64 = 4 * 1024 * 1024;

fn wait_bounded(child: &mut Child, timeout: Duration) -> ExitStatus {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("query child status") {
            return status;
        }
        if started.elapsed() >= timeout {
            let pid = i32::try_from(child.id()).expect("test child pid fits pid_t");
            // SAFETY: process_group(0) made the direct test child its group leader.
            let _ = unsafe { libc::kill(-pid, libc::SIGKILL) };
            let _ = child.kill();
            child.wait().expect("reap timed-out test child");
            panic!("test subprocess exceeded {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn bounded_status(command: &mut Command, timeout: Duration) -> ExitStatus {
    command.process_group(0);
    let mut child = command.spawn().expect("spawn bounded test child");
    wait_bounded(&mut child, timeout)
}

fn bounded_output(command: &mut Command, timeout: Duration) -> Output {
    command
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().expect("spawn bounded output child");
    bounded_child_output(child, timeout)
}

fn bounded_child_output(mut child: Child, timeout: Duration) -> Output {
    let group = i32::try_from(child.id()).expect("test child pid fits pid_t");
    let mut stdout = child.stdout.take().expect("captured stdout");
    let mut stderr = child.stderr.take().expect("captured stderr");
    let (stdout_sender, stdout_receiver) = std::sync::mpsc::channel();
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = Read::by_ref(&mut stdout)
            .take(OUTPUT_MAX + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = stdout_sender.send(result);
    });
    let (stderr_sender, stderr_receiver) = std::sync::mpsc::channel();
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = Read::by_ref(&mut stderr)
            .take(OUTPUT_MAX + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = stderr_sender.send(result);
    });
    let status = wait_bounded(&mut child, timeout);
    let capture_timeout = Duration::from_secs(2);
    let stdout = match stdout_receiver.recv_timeout(capture_timeout) {
        Ok(result) => result.expect("read child stdout"),
        Err(_) => {
            // A reader can remain blocked here only while a descendant in the
            // child's still-live process group retains the pipe writer.
            // SAFETY: that member pins the group identity after direct reap.
            let _ = unsafe { libc::kill(-group, libc::SIGKILL) };
            panic!("child stdout did not close after direct reap");
        }
    };
    let stderr = match stderr_receiver.recv_timeout(capture_timeout) {
        Ok(result) => result.expect("read child stderr"),
        Err(_) => {
            // SAFETY: the same retained-writer argument pins this group.
            let _ = unsafe { libc::kill(-group, libc::SIGKILL) };
            panic!("child stderr did not close after direct reap");
        }
    };
    stdout_reader.join().expect("join stdout reader");
    stderr_reader.join().expect("join stderr reader");
    assert!(stdout.len() as u64 <= OUTPUT_MAX, "child stdout overflow");
    assert!(stderr.len() as u64 <= OUTPUT_MAX, "child stderr overflow");
    Output {
        status,
        stdout,
        stderr,
    }
}

fn wrapper() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("run.sh")
}

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn malformed_public_shapes_exit_two() {
    let cases: &[&[&str]] = &[
        &[],
        &["--unknown", "x"],
        &[
            "--observer",
            "/observer",
            "--observer",
            "/again",
            "--observer-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--work-dir",
            "/work",
            "--provenance",
            "/provenance",
        ],
        &[
            "--observer",
            "relative",
            "--observer-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--work-dir",
            "/work",
            "--provenance",
            "/provenance",
        ],
        &[
            "--observer",
            "/observer",
            "--observer-sha256",
            "abc",
            "--work-dir",
            "/work",
            "--provenance",
            "/provenance",
        ],
    ];
    for arguments in cases {
        let mut command = Command::new("/bin/bash");
        command.arg(wrapper()).args(*arguments).env_clear();
        let status = bounded_status(&mut command, Duration::from_secs(10));
        assert_eq!(status.code(), Some(2), "arguments {arguments:?}");
    }
}

#[test]
fn wrapper_is_bash_syntax_valid() {
    let mut command = Command::new("/bin/bash");
    command.args(["-n"]).arg(wrapper()).env_clear();
    assert!(bounded_status(&mut command, Duration::from_secs(10)).success());
}

#[test]
fn wrapper_transfers_the_unlinked_digest_bound_image() {
    let _guard = WRAPPER_LOCK.lock().unwrap();
    let observer = PathBuf::from(env!("CARGO_BIN_EXE_align-startup-observer"));
    let bytes = std::fs::read(&observer).expect("read observer");
    let digest = format!("{:x}", Sha256::digest(bytes));
    let bash_env = std::env::temp_dir().join(format!(
        "align-startup-hostile-bash-env-{}",
        std::process::id()
    ));
    std::fs::write(
        &bash_env,
        b"cp() { exit 91; }\nstat() { exit 92; }\nsh() { exit 93; }\n",
    )
    .expect("write hostile BASH_ENV");
    let mut command = Command::new("/bin/bash");
    command
        .arg(wrapper())
        .args([
            "--observer",
            observer.to_str().expect("UTF-8 test path"),
            "--observer-sha256",
            &digest,
            "--work-dir",
            "/definitely-missing-align-startup-work-root",
            "--provenance",
            "/definitely-missing-align-startup-provenance",
        ])
        .env("PATH", "/definitely-untrusted")
        .env("LC_ALL", "C.UTF-8")
        .env("BASH_ENV", &bash_env)
        .env("ALIGNC_CACHE", "/ambient-cache");
    let output = bounded_output(&mut command, Duration::from_secs(10));
    std::fs::remove_file(bash_env).expect("remove hostile BASH_ENV");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("provenance"),
        "wrapper did not reach public input validation: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let private_parent =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/startup-observer");
    if private_parent.exists() {
        let leaked = std::fs::read_dir(private_parent)
            .expect("private parent")
            .collect::<Result<Vec<_>, _>>()
            .expect("enumerate private parent");
        assert!(leaked.is_empty(), "private wrapper directory leaked");
    }
}

#[test]
fn retained_launcher_executes_the_same_descriptor_after_readiness() {
    let observer = PathBuf::from(env!("CARGO_BIN_EXE_align-startup-observer"));
    let image = File::open(&observer).expect("open observer");
    // Keep every source above the fixed transfer slots used in pre_exec.
    // SAFETY: fcntl returns a new owned descriptor on success.
    let image_fd = unsafe { libc::fcntl(image.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    assert!(image_fd >= 10);
    // SAFETY: successful fcntl returned a fresh descriptor.
    let image = unsafe { File::from_raw_fd(image_fd) };
    let mut pipe = [-1; 2];
    // SAFETY: pipe names writable storage for two descriptors.
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    for descriptor in pipe {
        // SAFETY: both descriptors are live and this only sets close-on-exec.
        assert_ne!(
            unsafe { libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC) },
            -1
        );
    }
    // SAFETY: duplicate the read end above the fixed transfer slots.
    let ready_read_fd = unsafe { libc::fcntl(pipe[0], libc::F_DUPFD_CLOEXEC, 10) };
    assert!(ready_read_fd >= 10);
    // SAFETY: close the superseded owned read descriptor exactly once.
    assert_eq!(unsafe { libc::close(pipe[0]) }, 0);
    // SAFETY: successful fcntl returned one owned descriptor.
    let ready_read = unsafe { File::from_raw_fd(ready_read_fd) };
    // SAFETY: successful pipe returned two owned descriptors.
    let mut ready_write = unsafe { File::from_raw_fd(pipe[1]) };
    let image_source = image.as_raw_fd();
    let ready_source = ready_read.as_raw_fd();
    let mut command = Command::new(&observer);
    command
        .args([
            "--exec-tool-fd",
            "4",
            "--argv0",
            "align-startup-observer",
            "--self-probe",
        ])
        .env_clear()
        .env("ALIGNC_CACHE", "off")
        .env("ALIGNC_LINKER", "system")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .env("TZ", "UTC");
    // SAFETY: only async-signal-safe syscalls run between fork and exec, and
    // all captured descriptors remain live until spawn returns.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(image_source, 3) == -1
                || libc::dup2(image_source, 4) == -1
                || libc::dup2(ready_source, 5) == -1
            {
                return Err(std::io::Error::last_os_error());
            }
            let file_limit = libc::rlimit {
                rlim_cur: 64 * 1024 * 1024,
                rlim_max: 64 * 1024 * 1024,
            };
            let core_limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &file_limit) == -1
                || libc::setrlimit(libc::RLIMIT_CORE, &core_limit) == -1
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().expect("spawn launcher");
    drop(ready_read);
    ready_write.write_all(b"G").expect("write readiness");
    drop(ready_write);
    let output = bounded_child_output(child, Duration::from_secs(10));
    assert!(
        output.status.success(),
        "launcher stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn wrapper_preexec_copy_is_bounded_and_cleans_its_private_path() {
    let _guard = WRAPPER_LOCK.lock().unwrap();
    let temporary = std::env::temp_dir().join(format!(
        "align-startup-wrapper-timeout-{}",
        std::process::id()
    ));
    std::fs::create_dir(&temporary).expect("create timeout owner directory");
    let fifo = temporary.join("observer-fifo");
    let fifo_c = CString::new(fifo.to_str().expect("UTF-8 test path").as_bytes())
        .expect("test path has no NUL");
    // SAFETY: fifo_c is a NUL-terminated pathname below the test-owned directory.
    assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
    let started = Instant::now();
    let mut command = Command::new("/bin/bash");
    command
        .arg(wrapper())
        .args([
            "--observer",
            fifo.to_str().expect("UTF-8 test path"),
            "--observer-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--work-dir",
            "/definitely-missing-align-startup-work-root",
            "--provenance",
            "/definitely-missing-align-startup-provenance",
        ])
        .env_clear();
    let status = bounded_status(&mut command, Duration::from_secs(28));
    let elapsed = started.elapsed();
    assert_eq!(status.code(), Some(1));
    assert!(
        elapsed >= Duration::from_secs(19),
        "returned too early: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(27),
        "returned too late: {elapsed:?}"
    );
    std::fs::remove_file(&fifo).expect("remove test FIFO");
    std::fs::remove_dir(&temporary).expect("remove timeout owner directory");

    let private_parent =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/startup-observer");
    let leaked = std::fs::read_dir(private_parent)
        .expect("private parent")
        .collect::<Result<Vec<_>, _>>()
        .expect("enumerate private parent");
    assert!(leaked.is_empty(), "private wrapper directory leaked");
}

#[test]
fn committed_fixture_inventory_passes_alignc_check() {
    let repository = repository();
    for fixture in [
        "empty-i32",
        "empty-result",
        "argv",
        "primitive-output",
        "arena-reset",
        "par-map-below-floor",
        "par-map-first-pool",
        "par-map-reused-pool",
        "task-group-single",
        "readonly-first-touch",
    ] {
        let source = repository
            .join("bench/startup/fixtures")
            .join(format!("{fixture}.align"));
        let mut command = Command::new("/bin/bash");
        command
            .arg(repository.join("scripts/cargo.sh"))
            .args([
                "run",
                "--quiet",
                "-p",
                "align_driver",
                "--bin",
                "alignc",
                "--",
                "check",
            ])
            .arg(&source)
            .current_dir(&repository);
        let output = bounded_output(&mut command, Duration::from_secs(30));
        assert!(
            output.status.success(),
            "fixture {fixture} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
