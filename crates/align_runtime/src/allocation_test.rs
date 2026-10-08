//! Exact-process owners for process-global native allocation probes.
//! A mutex between observers cannot exclude allocations in ordinary libtest workers.

use crate::tests::FileFixtureDir;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const CHILD: &str = "ALIGN_NATIVE_ALLOCATION_TEST";
const MODE: &str = "ALIGN_NATIVE_ALLOCATION_CONTROL";
const COMPLETE: i32 = 73;
const WORK: Duration = Duration::from_secs(25);
const RETIRE: Duration = Duration::from_secs(5);

/// Declare this before body locals: their Drop runs before the completion signal.
/// Unwinding must reach libtest's failure status, never a successful-body exit.
pub(crate) struct Completion;
impl Drop for Completion {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::process::exit(COMPLETE);
        }
    }
}

fn test_name() -> String {
    std::thread::current()
        .name()
        .expect("named libtest worker")
        .to_owned()
}

fn fresh_counters() {
    assert!(!crate::REQUESTED_LIVE_PROBE.lock().unwrap().active);
    assert_eq!(
        (crate::align_rt_alloc_count(), crate::align_rt_free_count()),
        (0, 0)
    );
    let ptr = crate::align_rt_alloc(1);
    assert!(!ptr.is_null());
    unsafe { crate::align_rt_free(ptr) };
    assert_eq!(
        (crate::align_rt_alloc_count(), crate::align_rt_free_count()),
        (1, 1)
    );
}

/// Parent returns None only after the selected child's body completed successfully.
/// The exact test name prevents a copied or stale selector from silently passing.
pub(crate) fn enter() -> Option<Completion> {
    let name = test_name();
    if std::env::var(CHILD).ok().as_deref() == Some(&name) {
        fresh_counters();
        return Some(Completion);
    }
    let outcome = run(&name, "body", WORK);
    assert!(outcome.completed(), "{name}: {outcome:?}");
    None
}

struct OwnedChild<'a> {
    child: &'a mut Child,
    deadline: Instant,
    status: Option<ExitStatus>,
}
impl OwnedChild<'_> {
    fn poll_until(&mut self, deadline: Instant) -> io::Result<Option<ExitStatus>> {
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.status = Some(status);
                    return Ok(self.status);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    fn retire(&mut self) -> io::Result<()> {
        if self.status.is_some() {
            return Ok(());
        }
        // No grandchildren: these owners execute only native in-process operations.
        loop {
            match self.child.kill() {
                Err(error)
                    if error.kind() == io::ErrorKind::Interrupted
                        && Instant::now() < self.deadline =>
                {
                    continue;
                }
                // A concurrently exited child still needs to be reaped below.
                Err(error) if error.kind() != io::ErrorKind::InvalidInput => return Err(error),
                _ => break,
            }
        }
        if self.poll_until(self.deadline)?.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "allocation owner was not reaped",
            ));
        }
        Ok(())
    }
}
impl Drop for OwnedChild<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.retire() {
            eprintln!("allocation owner cleanup failed: {error}");
            // Preserve a body panic, but never turn failed cleanup into a passing owner.
            if !std::thread::panicking() {
                panic!("allocation owner cleanup failed: {error}");
            }
        }
    }
}

#[derive(Debug)]
struct Outcome {
    status: ExitStatus,
    timed_out: bool,
    stdout: String,
    stderr: String,
    witness: Option<String>,
}
impl Outcome {
    fn completed(&self) -> bool {
        !self.timed_out && self.status.code() == Some(COMPLETE)
    }
}

fn read_log(path: &std::path::Path) -> String {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .unwrap()
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn spawn(name: &str, mode: &str, path: &std::path::Path) -> Child {
    let stdout = path.join("stdout");
    let stderr = path.join("stderr");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(CHILD, name)
        .env(MODE, mode)
        .env("TMPDIR", &path)
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&stdout).unwrap())
        .stderr(std::fs::File::create(&stderr).unwrap());
    command.spawn().unwrap()
}

fn run(name: &str, mode: &str, work: Duration) -> Outcome {
    // The surviving parent owns all scratch, including child TMPDIR and logs.
    let scratch = FileFixtureDir::new("allocation-owner");
    let path = std::fs::canonicalize(&scratch.0).unwrap();
    let stdout = path.join("stdout");
    let stderr = path.join("stderr");
    let mut native = spawn(name, mode, &path);
    let started = Instant::now();
    let mut child = OwnedChild {
        child: &mut native,
        deadline: started + work + RETIRE,
        status: None,
    };
    let timed_out = child.poll_until(started + work).unwrap().is_none();
    child.retire().unwrap();
    let outcome = Outcome {
        status: child.status.unwrap(),
        timed_out,
        stdout: read_log(&stdout),
        stderr: read_log(&stderr),
        witness: std::fs::read_to_string(path.join("witness")).ok(),
    };
    drop(child);
    drop(scratch);
    assert!(
        !path.exists(),
        "parent removes child scratch after retirement"
    );
    outcome
}

#[test]
fn completion_and_retirement_protocol() {
    let name = test_name();
    if std::env::var(CHILD).ok().as_deref() == Some(&name) {
        let _completion = enter().unwrap();
        match std::env::var(MODE).unwrap().as_str() {
            "complete" => {
                struct BodyLocal;
                impl Drop for BodyLocal {
                    fn drop(&mut self) {
                        std::fs::write(std::env::temp_dir().join("witness"), "dropped").unwrap();
                    }
                }
                let _local = BodyLocal;
            }
            "panic" => panic!("injected allocation owner failure"),
            "ordinary" => std::mem::forget(_completion),
            "stall" => {
                std::fs::write(std::env::temp_dir().join("witness"), "stalled").unwrap();
                loop {
                    std::thread::park();
                }
            }
            mode => panic!("unknown control: {mode}"),
        }
        return;
    }
    let complete = run(&name, "complete", WORK);
    assert!(complete.completed(), "{complete:?}");
    assert_eq!(complete.witness.as_deref(), Some("dropped"));
    let panic = run(&name, "panic", WORK);
    assert!(
        !panic.completed() && panic.status.code() == Some(101),
        "{panic:?}"
    );
    assert!(
        panic.stderr.contains("injected allocation owner failure"),
        "{panic:?}"
    );
    let ordinary = run(&name, "ordinary", WORK);
    assert!(
        !ordinary.completed() && ordinary.status.success(),
        "{ordinary:?}"
    );
    let absent = run("allocation_test::nonexistent_selector", "complete", WORK);
    assert!(!absent.completed() && absent.status.success(), "{absent:?}");
    assert!(absent.stdout.contains("running 0 tests"), "{absent:?}");
    let stalled = run(&name, "stall", Duration::from_secs(3));
    assert!(stalled.timed_out && !stalled.completed(), "{stalled:?}");
    assert_eq!(stalled.witness.as_deref(), Some("stalled"));

    // Failure immediately after spawn must still kill and reap before scratch retires.
    // Borrow the Child so its cached reaped status remains observable after guard Drop.
    let scratch = FileFixtureDir::new("allocation-unwind");
    let path = std::fs::canonicalize(&scratch.0).unwrap();
    let mut native = spawn(&name, "stall", &path);
    let deadline = Instant::now() + RETIRE;
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _child = OwnedChild {
            child: &mut native,
            deadline,
            status: None,
        };
        panic!("injected failure immediately after guarded spawn");
    }));
    assert!(unwind.is_err());
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        native
            .try_wait()
            .unwrap()
            .expect("guard reaped child")
            .signal(),
        Some(libc::SIGKILL)
    );
    drop(scratch);
    assert!(
        !path.exists(),
        "unwind scratch removed after child retirement"
    );
}

#[test]
fn counters_exclude_foreign_worker_allocations() {
    let name = test_name();
    if std::env::var(CHILD).ok().as_deref() == Some(&name) {
        let Some(_isolated) = enter() else {
            unreachable!()
        };
        return;
    }
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Noise {
        stop: Arc<AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for Noise {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            self.worker.take().unwrap().join().unwrap();
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let running = Arc::clone(&stop);
    let (ready, started) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let allocate = || {
            let ptr = crate::align_rt_alloc(8);
            unsafe { crate::align_rt_free(ptr) };
        };
        allocate();
        ready.send(()).unwrap();
        while !running.load(Ordering::Relaxed) {
            allocate();
            std::thread::yield_now();
        }
    });
    let _noise = Noise {
        stop,
        worker: Some(worker),
    };
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(crate::align_rt_alloc_count() > 0 && crate::align_rt_free_count() > 0);
    let outcome = run(&name, "body", WORK);
    assert!(
        outcome.completed(),
        "foreign allocations must not enter the child: {outcome:?}"
    );
}
