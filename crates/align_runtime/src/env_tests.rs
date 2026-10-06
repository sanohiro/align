//! Strict environment text admission with process-isolated inherited byte values.
use super::*;
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::time::{Duration, Instant};

struct EnvChild {
    child: Option<std::process::Child>,
    deadline: Instant,
}
impl EnvChild {
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "environment child exceeded work deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    fn wait(&mut self) {
        loop {
            self.tick();
            match self.child.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    self.child.take();
                    assert!(status.success(), "environment child: {status}");
                    return;
                }
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll environment child: {error}"),
            }
        }
    }
}
impl Drop for EnvChild {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // The exact-filter child reads environment only and has no descendants.
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
                    eprintln!("environment child did not reap before deadline");
                    return;
                }
            }
        }
    }
}

struct Owned(AlignStr);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { align_rt_free(self.0.ptr.cast_mut()) };
    }
}
fn vacant() -> AlignStr {
    AlignStr {
        ptr: core::ptr::null(),
        len: 0,
    }
}
fn cases() -> Vec<Vec<u8>> {
    let mut cases = vec![
        b"ordinary=value".to_vec(),
        Vec::new(),
        "日本語🦀".as_bytes().to_vec(),
        vec![0xff],
        vec![0x80],
        vec![0xc0, 0x80],
        vec![0xed, 0xa0, 0x80],
        vec![0xc3],
        vec![0xf4, 0x90, 0x80, 0x80],
        vec![0xe2, 0x28, 0xa1],
    ];
    let mut late_invalid = vec![b'x'; 4096];
    late_invalid.push(0xff);
    cases.push(late_invalid);
    cases
}

#[test]
fn inherited_values_status_ownership_and_allocation() {
    const CHILD: &str = "ALIGN_ENV_TEXT_ADMISSION_CHILD";
    const ABSENT: &str = "ALIGN_ENV_TEXT_ABSENT";
    let cases = cases();
    if std::env::var_os(CHILD).as_deref() != Some(OsStr::new("1")) {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "env_tests::inherited_values_status_ownership_and_allocation",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env_remove(ABSENT)
            .stdin(std::process::Stdio::null());
        for (i, value) in cases.iter().enumerate() {
            command.env(format!("ALIGN_ENV_TEXT_{i}"), OsStr::from_bytes(value));
        }
        command.env("ALIGN_ENV_名前 with space", "unicode name");
        let mut child = EnvChild {
            child: Some(command.spawn().expect("spawn environment owner")),
            deadline: Instant::now() + Duration::from_secs(20),
        };
        child.wait();
        return;
    }
    #[cfg(feature = "alloc-count")]
    {
        assert!(!REQUESTED_LIVE_PROBE.lock().unwrap().active);
        let witness = align_rt_alloc(1);
        unsafe { align_rt_free(witness) };
    }
    for (name, expected) in cases
        .iter()
        .enumerate()
        .map(|(i, value)| (format!("ALIGN_ENV_TEXT_{i}"), Some(value.as_slice())))
        .chain([
            (ABSENT.to_owned(), None),
            (
                "ALIGN_ENV_名前 with space".to_owned(),
                Some(b"unicode name".as_slice()),
            ),
        ])
    {
        let mut output = Owned(vacant());
        #[cfg(feature = "alloc-count")]
        let (allocated, freed) = (align_rt_alloc_count(), align_rt_free_count());
        let status = unsafe {
            align_rt_env_get(
                name.as_ptr(),
                i64::try_from(name.len()).unwrap(),
                &mut output.0,
            )
        };
        let admitted = expected.filter(|bytes| std::str::from_utf8(bytes).is_ok());
        let expected_status = match (expected, admitted) {
            (None, _) => 0,
            (Some(_), None) => -AL_INVALID,
            (Some(_), Some(_)) => 1,
        };
        assert_eq!(status, expected_status, "{name}");
        assert_eq!(
            unsafe { bytes_view(output.0.ptr, output.0.len) },
            admitted.unwrap_or_default()
        );
        if admitted.is_none_or(|bytes| bytes.is_empty()) {
            assert!(output.0.ptr.is_null() && output.0.len == 0);
        } else {
            let cname = std::ffi::CString::new(name.as_bytes()).unwrap();
            assert_ne!(
                output.0.ptr,
                unsafe { getenv(cname.as_ptr().cast()) },
                "owned copy"
            );
        }
        drop(output);
        #[cfg(feature = "alloc-count")]
        {
            let count = i64::from(admitted.is_some_and(|bytes| !bytes.is_empty()));
            assert_eq!(
                align_rt_alloc_count() - allocated,
                count,
                "{name}: output allocation"
            );
            assert_eq!(
                align_rt_free_count() - freed,
                count,
                "{name}: output cleanup"
            );
        }
    }
}

#[test]
fn malformed_names_zero_output_before_lookup() {
    for (name, length) in [
        (b"".as_slice(), 0),
        (b"x".as_slice(), -1),
        (b"x".as_slice(), i64::MAX),
        (b"a\0b".as_slice(), 3),
        (b"a=b".as_slice(), 3),
        (b"\xff".as_slice(), 1),
    ] {
        let mut out = AlignStr {
            ptr: core::ptr::null(),
            len: 17,
        };
        assert_eq!(
            unsafe { align_rt_env_get(name.as_ptr(), length, &mut out) },
            -AL_INVALID
        );
        assert!(out.ptr.is_null() && out.len == 0);
    }
    for length in [-1, 0, 1] {
        let mut out = AlignStr {
            ptr: core::ptr::null(),
            len: 17,
        };
        assert_eq!(
            unsafe { align_rt_env_get(core::ptr::null(), length, &mut out) },
            -AL_INVALID
        );
        assert!(out.ptr.is_null() && out.len == 0);
    }
    assert_eq!(
        unsafe { align_rt_env_get(core::ptr::null(), 1, core::ptr::null_mut()) },
        -AL_INVALID
    );
}
