//! Text-only HTML admission, native rejection ordering, and independent owned output.
use super::*;
use std::io::ErrorKind;
use std::os::unix::{fs::DirBuilderExt, process::ExitStatusExt};
use std::time::{Duration, Instant};

struct ChildOwner {
    child: Option<std::process::Child>,
    deadline: Instant,
}
impl ChildOwner {
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "HTML child exceeded work deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    fn wait(&mut self) -> std::process::ExitStatus {
        loop {
            self.tick();
            match self.child.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    self.child.take();
                    return status;
                }
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll HTML child: {error}"),
            }
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // The exact-filter child uses native argument buffers only and has no descendants.
        loop {
            match child.kill() {
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => break,
            }
        }
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => {
                    eprintln!("HTML child did not reap before deadline");
                    return;
                }
            }
        }
    }
}

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("align-html-text-{}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .expect("exclusive HTML fixture");
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Owned(AlignStr);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { align_rt_free(self.0.ptr.cast_mut()) };
    }
}
fn oracle(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for byte in input {
        out.extend_from_slice(match byte {
            b'&' => b"&amp;",
            b'<' => b"&lt;",
            b'>' => b"&gt;",
            b'\"' => b"&quot;",
            b'\'' => b"&#39;",
            _ => std::slice::from_ref(byte),
        });
    }
    out
}

#[test]
fn html_text_native_admission_and_owned_output() {
    const CHILD: &str = "ALIGN_HTML_TEXT_CHILD";
    const NAME: &str = "html_text_tests::html_text_native_admission_and_owned_output";
    let invalid: &[&[u8]] = &[
        b"\xff",
        b"\x80",
        b"\xc0\x80",
        b"\xc1\xbf",
        b"\xc3",
        b"\xe0\x80\x80",
        b"\xed\xa0\x80",
        b"\xe2\x28\xa1",
        b"\xf0\x80\x80\x80",
        b"\xf4\x90\x80\x80",
        b"\xf5\x80\x80\x80",
        b"\xf0\x9f\x92",
    ];
    if let Some(mode) = std::env::var_os(CHILD) {
        let mode = mode.to_str().unwrap();
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) }, 0);
        if mode == "valid" {
            #[cfg(feature = "alloc-count")]
            {
                assert!(!REQUESTED_LIVE_PROBE.lock().unwrap().active);
                let witness = align_rt_alloc(1);
                unsafe { align_rt_free(witness) };
            }
            let cases = [
                vec![],
                (0u8..=127).collect(),
                b"&<>\"'&amp;&#39;".to_vec(),
                "\u{80}\u{7ff}\u{800}\u{d7ff}\u{e000}\u{ffff}\u{10000}\u{10ffff}日本🦀"
                    .as_bytes()
                    .to_vec(),
                vec![b'&'; 4096],
            ];
            for input in cases {
                let expected = oracle(&input);
                #[cfg(feature = "alloc-count")]
                let counts = (align_rt_alloc_count(), align_rt_free_count());
                let out = Owned(unsafe {
                    align_rt_html_escape(input.as_ptr(), i64::try_from(input.len()).unwrap())
                });
                assert_eq!(usize::try_from(out.0.len).unwrap(), expected.len());
                if expected.is_empty() {
                    assert!(out.0.ptr.is_null());
                } else {
                    assert_ne!(out.0.ptr, input.as_ptr());
                    // Assert the producer length before forming the observed reference.
                    let actual = unsafe { core::slice::from_raw_parts(out.0.ptr, expected.len()) };
                    assert_eq!(actual, expected);
                    assert!(std::str::from_utf8(actual).is_ok());
                }
                #[cfg(feature = "alloc-count")]
                assert_eq!(
                    align_rt_alloc_count() - counts.0,
                    i64::from(!input.is_empty())
                );
                drop(out);
                #[cfg(feature = "alloc-count")]
                assert_eq!(
                    align_rt_free_count() - counts.1,
                    i64::from(!input.is_empty())
                );
                assert_eq!(oracle(&input), expected, "input survives output Drop");
            }
            OWNED_ALLOC_FAIL_AFTER.store(0, core::sync::atomic::Ordering::Relaxed);
            let empty = Owned(unsafe { align_rt_html_escape(core::ptr::null(), 0) });
            assert!(empty.0.ptr.is_null() && empty.0.len == 0);
            OWNED_ALLOC_FAIL_AFTER.store(-1, core::sync::atomic::Ordering::Relaxed);
            return;
        }
        let mut input = b"valid & text".to_vec();
        let (ptr, len) = match mode {
            "negative" => (input.as_ptr(), -1),
            "negative-null" => (core::ptr::null(), i64::MIN),
            "null" => (core::ptr::null(), 1),
            "large-null" => (core::ptr::null(), i64::MAX),
            "wrap" => (core::ptr::without_provenance(usize::MAX - 1), 4),
            "oom" => (input.as_ptr(), i64::try_from(input.len()).unwrap()),
            _ => {
                let index: usize = mode.parse().unwrap();
                let bad = invalid[index / 3];
                input = match index % 3 {
                    0 => [bad, b"<&tail"].concat(),
                    1 => [b"<&head", bad, b"tail>"].concat(),
                    _ => [vec![b'&'; 4096], bad.to_vec()].concat(),
                };
                (input.as_ptr(), i64::try_from(input.len()).unwrap())
            }
        };
        OWNED_ALLOC_FAIL_AFTER.store(0, core::sync::atomic::Ordering::Relaxed);
        let _ = unsafe { align_rt_html_escape(ptr, len) };
        panic!("malformed input or forced allocation failure returned");
    }
    let fixture = Fixture::new();
    let modes = [
        "valid",
        "negative",
        "negative-null",
        "null",
        "large-null",
        "wrap",
        "oom",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain((0..invalid.len() * 3).map(|n| n.to_string()));
    for mode in modes {
        let stderr = fixture.0.join(&mode);
        let mut child = ChildOwner {
            child: Some(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", NAME, "--nocapture"])
                    .env(CHILD, &mode)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::fs::File::create_new(&stderr).unwrap())
                    .spawn()
                    .expect("spawn HTML owner"),
            ),
            deadline: Instant::now() + Duration::from_secs(20),
        };
        let status = child.wait();
        assert!(std::fs::metadata(&stderr).unwrap().len() <= 16_384);
        let diagnostic = std::fs::read_to_string(&stderr).unwrap();
        if mode == "valid" {
            assert!(status.success(), "{diagnostic}");
        } else {
            assert_eq!(
                status.signal(),
                Some(libc::SIGABRT),
                "{mode}: {status}: {diagnostic}"
            );
            let expected = if mode == "oom" {
                "out of memory"
            } else {
                "html escape input is not valid text"
            };
            assert!(diagnostic.contains(expected), "{mode}: {diagnostic}");
        }
    }
}
