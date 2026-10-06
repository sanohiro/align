//! Regex span semantics, independent output ownership, and staging-free construction.
use super::*;

struct RegexOwner(*mut RegexHandle);
impl RegexOwner {
    fn new(pattern: &str) -> Self {
        let mut handle = core::ptr::null_mut();
        assert_eq!(
            unsafe {
                align_rt_regex_compile(pattern.as_ptr(), regex_offset(pattern.len()), &mut handle)
            },
            0
        );
        assert!(!handle.is_null());
        Self(handle)
    }
}
impl Drop for RegexOwner {
    fn drop(&mut self) {
        unsafe { align_rt_regex_free(self.0) };
    }
}
struct Spans(AlignMatchArray);
impl Spans {
    fn new(re: &RegexOwner, text: &str, split: bool) -> Self {
        let mut output = AlignMatchArray {
            ptr: core::ptr::null_mut(),
            len: 0,
        };
        assert_eq!(
            unsafe { entry(split)(re.0, text.as_ptr(), regex_offset(text.len()), &mut output) },
            0
        );
        Self(output)
    }
    fn slice(&self) -> &[AlignRegexMatch] {
        if self.0.len == 0 {
            assert!(self.0.ptr.is_null());
            return &[];
        }
        assert!(!self.0.ptr.is_null());
        assert_eq!(
            self.0.ptr.addr() % core::mem::align_of::<AlignRegexMatch>(),
            0
        );
        unsafe { core::slice::from_raw_parts(self.0.ptr, usize::try_from(self.0.len).unwrap()) }
    }
    fn assert_pairs(&self, expected: &[(i64, i64)]) {
        assert_eq!(self.0.len, regex_offset(expected.len()));
        for (actual, expected) in self.slice().iter().zip(expected) {
            assert_eq!((actual.start, actual.end), *expected);
        }
    }
}
impl Drop for Spans {
    fn drop(&mut self) {
        unsafe { align_rt_free(self.0.ptr.cast()) };
    }
}
type Entry = unsafe extern "C" fn(*const RegexHandle, *const u8, i64, *mut AlignMatchArray) -> i32;
fn entry(split: bool) -> Entry {
    if split {
        align_rt_regex_split
    } else {
        align_rt_regex_find_all
    }
}

#[test]
fn spans_match_engine_and_explicit_empty_goldens() {
    for pattern in ["z", "x", "a+", "a*", "", "^|$", "π|🦀"] {
        let re = RegexOwner::new(pattern);
        for count in [0, 1, 3, 4, 5, 7, 8, 9, 257] {
            for unit in ["x", "aaab", "πa🦀"] {
                let text = unit.repeat(count);
                for split in [false, true] {
                    let mut expected = Vec::new();
                    let mut last = 0;
                    for m in unsafe { &*re.0 }.inner.find_iter(&text) {
                        expected.push((
                            regex_offset(if split { last } else { m.start() }),
                            regex_offset(if split { m.start() } else { m.end() }),
                        ));
                        last = m.end();
                    }
                    if split {
                        expected.push((regex_offset(last), regex_offset(text.len())));
                    }
                    Spans::new(&re, &text, split).assert_pairs(&expected);
                }
            }
        }
    }
    for (pattern, text, matches, fields) in [
        ("x", "", vec![], vec![(0, 0)]),
        ("", "", vec![(0, 0)], vec![(0, 0), (0, 0)]),
        (
            "",
            "πa",
            vec![(0, 0), (2, 2), (3, 3)],
            vec![(0, 0), (0, 2), (2, 3), (3, 3)],
        ),
        ("a*", "aaa", vec![(0, 3)], vec![(0, 0), (3, 3)]),
    ] {
        let re = RegexOwner::new(pattern);
        Spans::new(&re, text, false).assert_pairs(&matches);
        Spans::new(&re, text, true).assert_pairs(&fields);
    }
}

#[test]
fn invalid_views_publish_zero_before_storage() {
    #[cfg(feature = "alloc-count")]
    if isolated("regex_match_storage_tests::invalid_views_publish_zero_before_storage") {
        return;
    }
    let re = RegexOwner::new("");
    for split in [false, true] {
        #[cfg(feature = "alloc-count")]
        align_rt_requested_live_reset();
        for (handle, ptr, len) in [
            (core::ptr::null(), b"x".as_ptr(), 1),
            (re.0.cast_const(), b"x".as_ptr(), -1),
            (re.0.cast_const(), core::ptr::null(), 1),
            (re.0.cast_const(), b"\xff".as_ptr(), 1),
            (core::ptr::null(), core::ptr::null(), -1),
        ] {
            let mut out = AlignMatchArray {
                ptr: core::ptr::dangling_mut(),
                len: 99,
            };
            assert_eq!(unsafe { entry(split)(handle, ptr, len, &mut out) }, 0);
            #[cfg(feature = "alloc-count")]
            assert_eq!(align_rt_requested_live_peak(), 0);
            assert!(out.ptr.is_null() && out.len == 0);
        }
        assert_eq!(
            unsafe {
                entry(split)(
                    core::ptr::null(),
                    core::ptr::null(),
                    -1,
                    core::ptr::null_mut(),
                )
            },
            0
        );
        let mut out = AlignMatchArray {
            ptr: core::ptr::null_mut(),
            len: 0,
        };
        assert_eq!(
            unsafe { entry(split)(re.0, core::ptr::null(), 0, &mut out) },
            0
        );
        Spans(out).assert_pairs(if split { &[(0, 0), (0, 0)] } else { &[(0, 0)] });
    }
}

#[test]
fn returned_spans_outlive_text_and_regex() {
    for split in [false, true] {
        let re = RegexOwner::new("π");
        let mut text = "πxπ".to_string();
        let spans = Spans::new(&re, &text, split);
        text.clear();
        text.push_str("overwritten");
        drop(text);
        drop(re);
        spans.assert_pairs(if split {
            &[(0, 0), (2, 3), (5, 5)]
        } else {
            &[(0, 2), (3, 5)]
        });
    }
}

#[cfg(feature = "alloc-count")]
fn isolated(name: &str) -> bool {
    use std::time::{Duration, Instant};
    const MODE: &str = "ALIGN_REGEX_STORAGE_CHILD";
    if std::env::var_os(MODE).as_deref() == Some(std::ffi::OsStr::new(name)) {
        return false;
    }
    struct ChildOwner {
        child: Option<std::process::Child>,
        deadline: Instant,
    }
    impl Drop for ChildOwner {
        fn drop(&mut self) {
            let Some(child) = self.child.as_mut() else {
                return;
            };
            // The exact-filter child only executes this owner and has no descendants.
            loop {
                match child.kill() {
                    Err(e)
                        if e.kind() == std::io::ErrorKind::Interrupted
                            && Instant::now() < self.deadline => {}
                    _ => break,
                }
            }
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) if Instant::now() < self.deadline => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::Interrupted
                            && Instant::now() < self.deadline => {}
                    _ => {
                        eprintln!("regex storage child was not reaped before deadline");
                        return;
                    }
                }
            }
        }
    }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(MODE, name)
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("isolated regex storage owner");
    let mut owner = ChildOwner {
        child: Some(child),
        deadline: Instant::now() + Duration::from_secs(30),
    };
    let execution = owner.deadline - Duration::from_secs(5);
    let status = loop {
        assert!(
            Instant::now() < execution,
            "regex storage child exceeded deadline"
        );
        match owner.child.as_mut().unwrap().try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => panic!("poll regex storage child: {e}"),
        }
    };
    owner.child.take();
    assert!(status.success(), "regex storage child: {status}");
    true
}

#[cfg(feature = "alloc-count")]
#[test]
fn staging_allocation_matches_warmed_engine_only() {
    if isolated("regex_match_storage_tests::staging_allocation_matches_warmed_engine_only") {
        return;
    }
    assert!(!REQUESTED_LIVE_PROBE.lock().unwrap().active);
    let witness_before = global_alloc_count();
    let witness = std::hint::black_box(vec![std::hint::black_box(1_u8); 32]);
    assert!(global_alloc_count() > witness_before);
    drop(witness);
    for pattern in ["z", "x", "a+", ""] {
        let re = RegexOwner::new(pattern);
        for text in ["".to_string(), "x".into(), "xπaaa".repeat(2048)] {
            for split in [false, true] {
                for _ in 0..3 {
                    drop(Spans::new(&re, &text, split));
                }
                let engine = || {
                    for m in unsafe { &*re.0 }.inner.find_iter(&text) {
                        std::hint::black_box((m.start(), m.end()));
                    }
                };
                engine();
                let before = (global_alloc_count(), global_alloc_bytes());
                engine();
                let direct = (
                    global_alloc_count() - before.0,
                    global_alloc_bytes() - before.1,
                );
                let before = (global_alloc_count(), global_alloc_bytes());
                let output = Spans::new(&re, &text, split);
                let actual = (
                    global_alloc_count() - before.0,
                    global_alloc_bytes() - before.1,
                );
                assert_eq!(
                    actual,
                    direct,
                    "private staging: {pattern:?}, bytes={}, split={split}",
                    text.len()
                );
                eprintln!(
                    "pattern={pattern:?} text_bytes={} split={split} engine={direct:?} native={actual:?}",
                    text.len()
                );
                drop(output);
            }
        }
    }
}

#[cfg(feature = "alloc-count")]
#[test]
fn scoped_output_frees_or_transfers_once() {
    if isolated("regex_match_storage_tests::scoped_output_frees_or_transfers_once") {
        return;
    }
    align_rt_requested_live_reset();
    for count in [0, 1, 4, 5, 8, 9, 257] {
        for mode in 0..3 {
            let before = align_rt_free_count();
            let action = || {
                let mut output = RegexMatchOutput::new();
                for index in 0..count {
                    output.push(AlignRegexMatch {
                        start: index,
                        end: index + 1,
                    });
                }
                let capacity =
                    i64::try_from(output.0.cap * core::mem::size_of::<AlignRegexMatch>()).unwrap();
                assert_eq!(align_rt_requested_live_bytes(), capacity);
                if mode == 1 {
                    let result = Spans(output.finish());
                    assert_eq!(
                        align_rt_free_count(),
                        before,
                        "finish must detach before local Drop"
                    );
                    assert_eq!(align_rt_requested_live_bytes(), capacity);
                    assert_eq!(result.0.len, count);
                    assert_eq!(result.slice().len(), usize::try_from(count).unwrap());
                } else if mode == 2 {
                    panic!("exercise scoped output unwind");
                }
            };
            let result = std::panic::catch_unwind(action);
            assert_eq!(result.is_err(), mode == 2);
            assert_eq!(align_rt_requested_live_bytes(), 0);
            assert_eq!(align_rt_free_count() - before, i64::from(count != 0));
        }
    }
}
