//! Exclusive scratch and bounded compile/link/run ownership for Unix driver owners.
use align_driver::ArtifactStage;
use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::os::unix::{net::{UnixListener, UnixStream}, process::CommandExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

struct ChildGroup {
    child: Child,
    deadline: Instant,
}
impl ChildGroup {
    fn spawn(command: &mut Command) -> Self {
        Self::with_budget(command, Duration::from_secs(60))
    }
    fn with_budget(command: &mut Command, budget: Duration) -> Self {
        Self {
            child: command
                .process_group(0)
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
            deadline: Instant::now() + budget,
        }
    }
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "owned fixture exceeded work deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    fn wait(&mut self) -> ExitStatus {
        loop {
            self.tick();
            match self.child.try_wait() {
                // Keep ownership of the group even after the leader has exited and been reaped.
                Ok(Some(status)) => return status,
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll owned fixture: {error}"),
            }
        }
    }
}
impl Drop for ChildGroup {
    fn drop(&mut self) {
        let pid = i32::try_from(self.child.id()).expect("native child pid");
        loop {
            // SAFETY: this invocation owns the fresh process group, including surviving helpers.
            if unsafe { libc::kill(-pid, libc::SIGKILL) } == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != ErrorKind::Interrupted || Instant::now() >= self.deadline {
                break;
            }
        }
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => {
                    eprintln!("owned fixture failed to reap before deadline");
                    break;
                }
            }
        }
    }
}

/// Re-execute one exact test inside a bounded process group. The parent owns all
/// scratch and keeps the group guard alive until every descendant is retired.
/// The callback also contains transitive compiler/linker probes and generated rows.
pub(super) fn run(test: &str, action: impl FnOnce(&Path)) {
    if std::env::var("ALIGN_OWNED_FIXTURE_TEST").as_deref() == Ok(test) {
        let stage = std::env::var_os("ALIGN_OWNED_FIXTURE_STAGE").expect("parent-owned stage");
        action(Path::new(&stage));
        return;
    }
    let stage = ArtifactStage::temp("owned-driver-fixture").unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", test, "--nocapture"])
        .env("ALIGN_OWNED_FIXTURE_TEST", test)
        .env("ALIGN_OWNED_FIXTURE_STAGE", stage.path())
        .env("TMPDIR", stage.path())
        .stdout(File::create(stage.path().join("owner.stdout")).unwrap())
        .stderr(File::create(stage.path().join("owner.stderr")).unwrap());
    let mut group = ChildGroup::spawn(&mut command);
    let status = group.wait();
    drop(group);
    assert!(status.success(), "{test}\n{}\n{}",
        fs::read_to_string(stage.path().join("owner.stdout")).unwrap(),
        fs::read_to_string(stage.path().join("owner.stderr")).unwrap());
}

#[test]
#[ignore = "invoked only by the bounded fixture lifecycle owner"]
fn stalled_native_probe() {
    match std::env::var("ALIGN_FIXTURE_PROBE_ROLE").unwrap().as_str() {
        "link" => {
            // Use the production linker path, including any compiler/version subprocesses.
            let stage = std::env::var_os("ALIGN_FIXTURE_PROBE_STAGE").unwrap();
            align_driver::link_objects(&align_driver::CDriver::default(), &[],
                &Path::new(&stage).join("unused"), &[], align_driver::Profile::Release).unwrap();
            panic!("stalled native linker unexpectedly returned");
        }
        "descendant" => {
            let socket = std::env::var_os("ALIGN_FIXTURE_PROBE_SOCKET").unwrap();
            let _connection = UnixStream::connect(socket).unwrap();
            loop { std::thread::park(); }
        }
        _ => panic!("unknown fixture probe role"),
    }
}

#[test]
fn stalled_linker_is_retired_before_scratch() {
    use std::os::unix::fs::PermissionsExt;
    // macOS sockaddr_un requires a short root, independently of ambient TMPDIR.
    let stage = ArtifactStage::in_dir(Path::new("/tmp"), "owned-fixture").unwrap();
    let path = stage.path().to_path_buf();
    let socket = path.join("socket");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let shim = path.join("cc");
    fs::write(&shim, "#!/bin/sh\nexport ALIGN_FIXTURE_PROBE_ROLE=descendant\nexec \"$ALIGN_FIXTURE_PROBE_EXE\" --exact owned_fixture::stalled_native_probe --ignored --nocapture\n").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o700)).unwrap();
    let executable = std::env::current_exe().unwrap();
    let mut command = Command::new(&executable);
    command.args(["--exact", "owned_fixture::stalled_native_probe", "--ignored", "--nocapture"])
        .env("ALIGN_FIXTURE_PROBE_ROLE", "link")
        .env("ALIGN_FIXTURE_PROBE_EXE", &executable)
        .env("ALIGN_FIXTURE_PROBE_STAGE", &path)
        .env("ALIGN_FIXTURE_PROBE_SOCKET", &socket)
        .env("TMPDIR", &path).env("PATH", &path)
        .stdout(Stdio::null()).stderr(Stdio::null());
    // One deadline starts at spawn: one second of work and five for retirement.
    let mut owner = ChildGroup::with_budget(&mut command, Duration::from_secs(6));
    let deadline = owner.deadline;
    let pid = i32::try_from(owner.child.id()).unwrap();
    let (mut connection, _) = loop {
        owner.tick();
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
            Err(error) => panic!("stalled linker witness: {error}"),
        }
    };
    let timed_out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || owner.wait()));
    assert!(timed_out.is_err(), "stalled native operation must hit its work deadline");
    connection.set_nonblocking(true).unwrap();
    loop {
        assert!(Instant::now() < deadline, "native descendant retained its socket");
        match connection.read(&mut [0]) {
            Ok(0) => break,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                std::thread::sleep(Duration::from_millis(2));
            }
            result => panic!("unexpected descendant witness: {result:?}"),
        }
    }
    assert_eq!(unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) }, -1);
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
    drop(connection);
    drop(listener);
    drop(stage);
    assert!(!path.exists(), "scratch must retire after all native descendants");
}
