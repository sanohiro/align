//! Bounded, non-atomic observations. Returned integers confer no process authority.
use super::process_live::{NativeChild, OptionalCount, disjoint, valid_pointer};
use super::{AL_INVALID, AlignStr, io_error_to_status};
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Snapshot {
    pid: i64,
    parent_pid: i64,
    rss_bytes: OptionalCount,
    cpu_ns: OptionalCount,
    threads: OptionalCount,
}
struct Observation {
    row: Snapshot,
    group: i64,
}
#[derive(Default)]
struct ObservationScratch {
    #[cfg(target_os = "linux")]
    bytes: Vec<u8>,
}
fn budget(value: i64) -> Result<usize, i32> {
    if !(1..=536870910).contains(&value) {
        return Err(AL_INVALID);
    }
    usize::try_from(value).map_err(|_| AL_INVALID)
}
#[cfg(target_os = "linux")]
fn candidates(maximum: usize) -> Result<Vec<i32>, i32> {
    use std::os::unix::ffi::OsStrExt;
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc").map_err(|e| io_error_to_status(&e))? {
        let entry = entry.map_err(|e| io_error_to_status(&e))?;
        let name = entry.file_name();
        let bytes = name.as_bytes();
        if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
            continue;
        }
        if pids.len() == maximum {
            return Err(AL_INVALID);
        }
        let pid = std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| text.parse::<i32>().ok())
            .filter(|pid| *pid > 0)
            .ok_or(AL_INVALID)?;
        pids.push(pid);
    }
    pids.sort_unstable();
    pids.dedup();
    Ok(pids)
}
#[cfg(target_os = "linux")]
fn observe(pid: i32, scratch: &mut ObservationScratch) -> Result<Option<Observation>, i32> {
    let mut path_bytes = [0; 22];
    let file = match std::fs::File::open(stat_path(pid, &mut path_bytes)?) {
        Ok(file) => file,
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ESRCH)) => {
            return Ok(None);
        }
        Err(error) => return Err(io_error_to_status(&error)),
    };
    let Some(bytes) = observation_bytes(file, &mut scratch.bytes)? else {
        return Ok(None);
    };
    parse_stat(pid, bytes).map(Some)
}
#[cfg(any(test, target_os = "linux"))]
fn stat_path(pid: i32, bytes: &mut [u8; 22]) -> Result<&std::ffi::OsStr, i32> {
    use std::os::unix::ffi::OsStrExt;
    // /proc/ + the longest signed i32 (-2147483648) + /stat fits exactly.
    let mut remaining = &mut bytes[..];
    std::io::Write::write_fmt(&mut remaining, format_args!("/proc/{pid}/stat"))
        .map_err(|_| AL_INVALID)?;
    let length = 22 - remaining.len();
    Ok(std::ffi::OsStr::from_bytes(&bytes[..length]))
}
#[cfg(any(test, target_os = "linux"))]
fn observation_bytes(reader: impl std::io::Read, bytes: &mut Vec<u8>) -> Result<Option<&[u8]>, i32> {
    use std::io::Read;
    bytes.clear();
    match reader.take(65537).read_to_end(bytes) {
        Ok(_) => {}
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ESRCH)) => {
            return Ok(None);
        }
        Err(error) => return Err(io_error_to_status(&error)),
    }
    if bytes.len() > 65536 {
        return Err(AL_INVALID);
    }
    Ok(Some(bytes.as_slice()))
}
#[cfg(target_os = "linux")]
fn parse_stat(pid: i32, bytes: &[u8]) -> Result<Observation, i32> {
    let open = bytes
        .iter()
        .position(|byte| *byte == b'(')
        .ok_or(AL_INVALID)?;
    let close = bytes
        .iter()
        .rposition(|byte| *byte == b')')
        .filter(|close| *close > open)
        .ok_or(AL_INVALID)?;
    let identity = std::str::from_utf8(&bytes[..open])
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .ok_or(AL_INVALID)?;
    if identity != pid || pid <= 0 {
        return Err(AL_INVALID);
    }
    let tail = std::str::from_utf8(&bytes[close + 1..]).map_err(|_| AL_INVALID)?;
    // Only this prefix supplies observed fields; later tokens still passed full UTF-8 validation.
    let mut fields = [None; 22];
    for (slot, text) in fields.iter_mut().zip(tail.split_ascii_whitespace()) {
        *slot = Some(text);
    }
    let number = |index: usize| fields.get(index).copied().flatten()
        .and_then(|text| text.parse::<i128>().ok());
    let parent = number(1)
        .and_then(|n| i64::try_from(n).ok())
        .filter(|n| *n >= 0)
        .ok_or(AL_INVALID)?;
    let group = number(2)
        .and_then(|n| i64::try_from(n).ok())
        .filter(|n| *n >= 0)
        .ok_or(AL_INVALID)?;
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let optional = |value: Option<i128>| {
        value
            .map(OptionalCount::from_nonnegative)
            .unwrap_or_default()
    };
    let rss = number(21).filter(|n| *n >= 0).and_then(|n| {
        (page > 0)
            .then_some(i128::from(page))
            .and_then(|p| n.checked_mul(p))
    });
    let cpu = number(11)
        .filter(|n| *n >= 0)
        .and_then(|u| {
            number(12)
                .filter(|n| *n >= 0)
                .and_then(|s| u.checked_add(s))
        })
        .and_then(|n| n.checked_mul(1_000_000_000))
        .and_then(|n| (ticks > 0).then(|| n / i128::from(ticks)));
    Ok(Observation {
        group,
        row: Snapshot {
            pid: i64::from(pid),
            parent_pid: parent,
            rss_bytes: optional(rss),
            cpu_ns: optional(cpu),
            threads: optional(number(17)),
        },
    })
}
#[cfg(target_os = "macos")]
fn candidates(maximum: usize) -> Result<Vec<i32>, i32> {
    let mut pids = vec![0i32; maximum + 1];
    let bytes =
        i32::try_from(pids.len().checked_mul(4).ok_or(AL_INVALID)?).map_err(|_| AL_INVALID)?;
    let used = unsafe {
        libc::proc_listpids(
            1, /* PROC_ALL_PIDS */
            0,
            pids.as_mut_ptr().cast(),
            bytes,
        )
    };
    let native_error = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    decode_pid_list(pids, used, bytes, native_error)
}
#[cfg(any(test, target_os = "macos"))]
fn decode_pid_list(
    mut pids: Vec<i32>,
    used: i32,
    bytes: i32,
    native_error: i32,
) -> Result<Vec<i32>, i32> {
    if used <= 0 {
        return Err(if native_error == 0 {
            AL_INVALID
        } else {
            io_error_to_status(&std::io::Error::from_raw_os_error(native_error))
        });
    }
    if used % 4 != 0 || used > bytes || used == bytes {
        return Err(AL_INVALID);
    }
    pids.truncate(usize::try_from(used / 4).map_err(|_| AL_INVALID)?);
    if pids.iter().any(|pid| *pid < 0) {
        return Err(AL_INVALID);
    }
    pids.retain(|pid| *pid > 0);
    pids.sort_unstable();
    pids.dedup();
    Ok(pids)
}
#[cfg(target_os = "macos")]
#[allow(deprecated)] // libc exposes the stable native Mach timebase ABI.
fn observe(pid: i32, _scratch: &mut ObservationScratch) -> Result<Option<Observation>, i32> {
    #[repr(C)]
    struct ShortBsd {
        pid: u32,
        parent: u32,
        group: u32,
        status: u32,
        name: [u8; 16],
        flags: u32,
        uid: u32,
        gid: u32,
        ruid: u32,
        rgid: u32,
        svuid: u32,
        svgid: u32,
        reserved: u32,
    }
    let mut identity: ShortBsd = unsafe { core::mem::zeroed() };
    let expected = i32::try_from(core::mem::size_of::<ShortBsd>()).map_err(|_| AL_INVALID)?;
    let count = unsafe {
        libc::proc_pidinfo(
            pid,
            13,
            1,
            (&mut identity as *mut ShortBsd).cast(),
            expected,
        )
    };
    if count <= 0 {
        let errno = std::io::Error::last_os_error();
        if matches!(errno.raw_os_error(), Some(libc::ESRCH | libc::ENOENT)) {
            return Ok(None);
        }
        return Err(io_error_to_status(&errno));
    }
    if count != expected || i64::from(identity.pid) != i64::from(pid) {
        return Err(AL_INVALID);
    }
    let mut row = Snapshot {
        pid: i64::from(pid),
        parent_pid: i64::from(identity.parent),
        ..Snapshot::default()
    };
    let mut task: libc::proc_taskinfo = unsafe { core::mem::zeroed() };
    let size = i32::try_from(core::mem::size_of_val(&task)).map_err(|_| AL_INVALID)?;
    let count = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut task as *mut libc::proc_taskinfo).cast(),
            size,
        )
    };
    if count == size {
        row.rss_bytes = OptionalCount::from_nonnegative(i128::from(task.pti_resident_size));
        row.threads = OptionalCount::from_nonnegative(i128::from(task.pti_threadnum));
        let mut timebase: libc::mach_timebase_info_data_t = unsafe { core::mem::zeroed() };
        if unsafe { libc::mach_timebase_info(&mut timebase) } == 0 && timebase.denom > 0 {
            let ticks = i128::from(task.pti_total_user) + i128::from(task.pti_total_system);
            row.cpu_ns = OptionalCount::from_nonnegative(
                ticks * i128::from(timebase.numer) / i128::from(timebase.denom),
            );
        }
    }
    Ok(Some(Observation {
        row,
        group: i64::from(identity.group),
    }))
}
fn publish<T: Copy>(rows: &[T]) -> Result<AlignStr, i32> {
    let bytes = rows
        .len()
        .checked_mul(core::mem::size_of::<T>())
        .and_then(|n| i64::try_from(n).ok())
        .ok_or(AL_INVALID)?;
    let pointer = super::align_rt_alloc(bytes);
    if !rows.is_empty() {
        unsafe {
            core::ptr::copy_nonoverlapping(rows.as_ptr(), pointer.cast::<T>(), rows.len());
        }
    }
    Ok(AlignStr {
        ptr: pointer,
        len: i64::try_from(rows.len()).map_err(|_| AL_INVALID)?,
    })
}
/// # Safety
/// out points to a writable array header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_table(max_scan: i64, out: *mut AlignStr) -> i32 {
    if !valid_pointer(out) {
        return AL_INVALID;
    }
    unsafe {
        *out = AlignStr {
            ptr: core::ptr::null(),
            len: 0,
        };
    }
    let run = || {
        let pids = candidates(budget(max_scan)?)?;
        let mut rows = Vec::with_capacity(pids.len());
        let mut scratch = ObservationScratch::default();
        for pid in pids {
            if let Some(observation) = observe(pid, &mut scratch)? {
                rows.push(observation.row);
            }
        }
        publish(&rows)
    };
    match run() {
        Ok(array) => {
            unsafe {
                *out = array;
            }
            0
        }
        Err(error) => error,
    }
}
/// # Safety
/// Child is live; out is disjoint writable array-header storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_child_group_members(
    child: *const NativeChild,
    max_scan: i64,
    out: *mut AlignStr,
) -> i32 {
    if !valid_pointer(child) || !valid_pointer(out) || !disjoint(child, out) {
        return AL_INVALID;
    }
    unsafe {
        *out = AlignStr {
            ptr: core::ptr::null(),
            len: 0,
        };
    }
    let child = unsafe { &*child };
    let run = || {
        let maximum = budget(max_scan)?;
        if child.lost || child.reaped.is_some() || !child.new_session || child.pid <= 0 {
            return Err(AL_INVALID);
        }
        let mut rows = Vec::new();
        let mut scratch = ObservationScratch::default();
        for pid in candidates(maximum)? {
            if let Some(observation) = observe(pid, &mut scratch)?
                && observation.group == i64::from(child.pid)
            {
                rows.push(i64::from(pid));
            }
        }
        publish(&rows)
    };
    match run() {
        Ok(array) => {
            unsafe {
                *out = array;
            }
            0
        }
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_path_spelling_and_storage() {
        use std::os::unix::ffi::OsStrExt;
        #[cfg(feature = "alloc-count")]
        {
            let before = super::super::global_alloc_count();
            let witness = std::hint::black_box(vec![std::hint::black_box(7_u8); 32]);
            assert!(super::super::global_alloc_count() > before, "allocation counter must be active");
            drop(witness);
        }
        let mut pids = vec![i32::MAX, 0, i32::MIN, 1, -1, i32::MAX - 1, i32::MIN + 1];
        for width in [1, 10, 100, 1000, 10000, 100000, 1000000, 10000000, 100000000, 1000000000] {
            for delta in [-1, 0, 1] {
                pids.push(width + delta);
                pids.push(-width + delta);
            }
        }
        let mut bits = 0x127a_d309_u32;
        for _ in 0..1024 {
            bits = bits.wrapping_mul(1664525).wrapping_add(1013904223);
            pids.push(i32::from_ne_bytes(bits.to_ne_bytes()));
        }
        let mut scratch = [b'!'; 22];
        for pid in pids {
            let expected = format!("/proc/{pid}/stat");
            let original = scratch;
            #[cfg(feature = "alloc-count")]
            let before = super::super::global_alloc_count();
            let actual = stat_path(pid, &mut scratch).unwrap();
            #[cfg(feature = "alloc-count")]
            assert_eq!(super::super::global_alloc_count() - before, 0);
            assert_eq!(actual.as_bytes(), expected.as_bytes());
            assert_eq!(&scratch[expected.len()..], &original[expected.len()..]);
        }
    }

    struct ChunkedInput<'a> {
        remaining: &'a [u8],
        consumed: usize,
        interrupt_at: Option<usize>,
        terminal_error: Option<i32>,
    }
    impl std::io::Read for ChunkedInput<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.interrupt_at.is_some_and(|at| self.consumed >= at) {
                self.interrupt_at = None;
                return Err(std::io::Error::from_raw_os_error(libc::EINTR));
            }
            if self.remaining.is_empty() {
                return match self.terminal_error {
                    Some(error) => Err(std::io::Error::from_raw_os_error(error)),
                    None => Ok(0),
                };
            }
            let count = out.len().min(self.remaining.len()).min(7);
            out[..count].copy_from_slice(&self.remaining[..count]);
            self.remaining = &self.remaining[count..];
            self.consumed += count;
            Ok(count)
        }
    }

    #[test]
    fn observation_input_reuse_and_failures() {
        let long = [b'x'; 1024];
        let mut scratch = Vec::new();
        for bytes in [&long[..], &b"short\0\xff"[..], &b""[..]] {
            let mut input = ChunkedInput {
                remaining: bytes, consumed: 0, interrupt_at: Some(7), terminal_error: None,
            };
            assert_eq!(observation_bytes(&mut input, &mut scratch), Ok(Some(bytes)));
            assert_eq!(input.consumed, bytes.len());
        }
        for error in [libc::ENOENT, libc::ESRCH, libc::EIO, libc::EACCES] {
            let input = ChunkedInput {
                remaining: b"partial", consumed: 0, interrupt_at: Some(7), terminal_error: Some(error),
            };
            let expected = if matches!(error, libc::ENOENT | libc::ESRCH) {
                Ok(None)
            } else {
                Err(io_error_to_status(&std::io::Error::from_raw_os_error(error)))
            };
            assert_eq!(observation_bytes(input, &mut scratch), expected);
            assert_eq!(observation_bytes(&b"new"[..], &mut scratch), Ok(Some(&b"new"[..])));
        }
    }

    #[test]
    fn observation_input_cap_and_read_error_order() {
        let bytes = [b'x'; 65538];
        let mut scratch = Vec::new();
        for len in [65536, 65537, 65538] {
            for terminal_error in [None, Some(libc::ENOENT), Some(libc::ESRCH), Some(libc::EIO)] {
                let mut input = ChunkedInput {
                    remaining: &bytes[..len], consumed: 0, interrupt_at: Some(65534), terminal_error,
                };
                let actual = observation_bytes(&mut input, &mut scratch);
                let expected = match (len, terminal_error) {
                    (65536, None) => Ok(Some(&bytes[..len])),
                    (65536, Some(libc::ENOENT | libc::ESRCH)) => Ok(None),
                    (65536, Some(error)) => Err(io_error_to_status(&std::io::Error::from_raw_os_error(error))),
                    _ => Err(AL_INVALID),
                };
                assert_eq!(actual, expected, "len={len}, error={terminal_error:?}");
                assert_eq!(input.consumed, len.min(65537));
                assert_eq!(observation_bytes(&b"ok"[..], &mut scratch), Ok(Some(&b"ok"[..])));
            }
        }
    }

    #[cfg(feature = "alloc-count")]
    #[test]
    fn observation_input_reuse_allocations() {
        use super::super::global_alloc_count;
        let witness_before = global_alloc_count();
        let witness = std::hint::black_box(vec![std::hint::black_box(7_u8); 32]);
        assert!(global_alloc_count() > witness_before, "allocation counter must be active");
        drop(witness);

        let bytes = [b'x'; 1024];
        let mut scratch = Vec::new();
        observation_bytes(&bytes[..], &mut scratch).unwrap().unwrap();
        for len in [1024, 0, 1, 127, 1024, 31] {
            let input = ChunkedInput {
                remaining: &bytes[..len], consumed: 0, interrupt_at: Some(7), terminal_error: None,
            };
            let before = global_alloc_count();
            let result = observation_bytes(input, &mut scratch);
            let allocations = global_alloc_count() - before;
            assert_eq!(result, Ok(Some(&bytes[..len])));
            assert_eq!(allocations, 0, "warmed input len={len}");
        }
    }

    #[cfg(all(target_os = "linux", feature = "alloc-count"))]
    #[test]
    fn observation_native_reuse_allocations() {
        use super::super::global_alloc_count;
        let pid = i32::try_from(std::process::id()).unwrap();
        let mut scratch = ObservationScratch::default();
        let before = global_alloc_count();
        let first = observe(pid, &mut scratch).unwrap().unwrap().row;
        let cold = global_alloc_count() - before;
        let mut warm = [0_u64; 32];
        for count in &mut warm {
            let before = global_alloc_count();
            let current = observe(pid, &mut scratch).unwrap().unwrap().row;
            *count = global_alloc_count() - before;
            assert_eq!((current.pid, current.parent_pid), (first.pid, first.parent_pid));
        }
        eprintln!("native process observation Rust allocations: cold={cold}, warm={warm:?}");
        // Input capacity is retained, and the pathname now uses only fixed stack storage.
        assert!(cold > 1, "cold read must demonstrate input storage growth");
        assert_eq!(warm, [0; 32]);
    }

    #[test]
    fn native_scan_failures_are_not_empty_observations() {
        for native_error in [libc::EACCES, libc::EIO] {
            assert_eq!(
                decode_pid_list(vec![0; 3], 0, 12, native_error),
                Err(io_error_to_status(&std::io::Error::from_raw_os_error(
                    native_error
                )))
            );
        }
        assert_eq!(decode_pid_list(vec![0; 3], 0, 12, 0), Err(AL_INVALID));
        assert_eq!(decode_pid_list(vec![3, 1, 0], 8, 12, 0), Ok(vec![1, 3]));
        for used in [-1, 1, 12, 16] {
            assert!(decode_pid_list(vec![0; 3], used, 12, 0).is_err());
        }
        struct Failure(i32);
        impl std::io::Read for Failure {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from_raw_os_error(self.0))
            }
        }
        let mut scratch = Vec::new();
        for vanished in [libc::ENOENT, libc::ESRCH] {
            assert_eq!(observation_bytes(Failure(vanished), &mut scratch), Ok(None));
        }
        assert!(observation_bytes(Failure(libc::EIO), &mut scratch).is_err());
        assert_eq!(
            observation_bytes(&b"record"[..], &mut scratch),
            Ok(Some(&b"record"[..]))
        );
    }
    #[test]
    fn bounded_table_layout_order_and_identity() {
        assert_eq!(core::mem::size_of::<Snapshot>(), 64);
        assert_eq!(core::mem::offset_of!(Snapshot, threads), 48);
        for invalid in [-1, 0, 536870911, i64::MAX] {
            assert!(budget(invalid).is_err());
        }
        let pids = candidates(100000).unwrap();
        assert!(pids.windows(2).all(|pair| pair[0] < pair[1]));
        let pid = i32::try_from(std::process::id()).unwrap();
        assert!(pids.contains(&pid));
        let observation = observe(pid, &mut ObservationScratch::default()).unwrap().unwrap();
        assert_eq!(observation.row.pid, i64::from(pid));
        assert!(observation.row.parent_pid >= 0);
        assert!(matches!(observation.row.threads,OptionalCount { tag:1,value,.. } if value>=1));
        if pids.len() > 1 {
            assert!(candidates(1).is_err());
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn stat_prefix_missing_fields_and_ignored_tail_preserve_admission() {
        let mut fields = ["0"; 22];
        fields[0] = "S";
        fields[1] = "1";
        fields[11] = "3";
        fields[12] = "4";
        fields[17] = "2";
        fields[21] = "5";
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        for count in 0..=fields.len() {
            let raw = format!("123 (a)\nb) {}", fields[..count].join("\t "));
            let observed = parse_stat(123, raw.as_bytes());
            if count < 3 {
                assert!(matches!(observed, Err(AL_INVALID)), "count={count}");
                continue;
            }
            let observed = observed.unwrap();
            assert_eq!((observed.row.parent_pid, observed.group), (1, 0));
            let expected = |present: bool, value: i128| {
                if present { OptionalCount::from_nonnegative(value) }
                else { OptionalCount::default() }
            };
            assert_eq!(observed.row.threads, expected(count > 17, 2));
            assert_eq!(observed.row.rss_bytes, expected(count > 21 && page > 0, i128::from(page) * 5));
            let cpu = if ticks > 0 { 7_000_000_000 / i128::from(ticks) } else { 0 };
            assert_eq!(observed.row.cpu_ns, expected(count > 12 && ticks > 0, cpu));
        }
        let prefix = format!("123 (x) {}", fields.join(" "));
        for tail in ["".to_owned(), " irrelevant -999 NaN".to_owned(), " x".repeat(32_000)] {
            let raw = prefix.clone() + &tail;
            let observed = parse_stat(123, raw.as_bytes()).unwrap();
            assert_eq!((observed.row.pid, observed.row.parent_pid, observed.group), (123, 1, 0));
            assert_eq!(observed.row.threads, OptionalCount::from_nonnegative(2));
        }
        let mut invalid_utf8 = prefix.clone().into_bytes();
        invalid_utf8.extend_from_slice(b" ignored \xff");
        assert!(matches!(parse_stat(123, &invalid_utf8), Err(AL_INVALID)));
        for invalid in ["-1", "bad", "170141183460469231731687303715884105728"] {
            for index in [1, 2] {
                let mut malformed = fields;
                malformed[index] = invalid;
                let raw = format!("123 (x) {} ignored", malformed.join(" "));
                assert!(matches!(parse_stat(123, raw.as_bytes()), Err(AL_INVALID)));
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stat_identity_and_optional_counters() {
        // Linux comm may contain parentheses or newlines. Field numbering begins after the last ')'.
        let mut fields = vec!["0"; 22];
        fields[0] = "S";
        fields[1] = "1";
        fields[2] = "0";
        fields[11] = "3";
        fields[12] = "4";
        fields[17] = "2";
        fields[21] = "5";
        let raw = format!("123 (a)\nb) {}", fields.join(" "));
        let observed = parse_stat(123, raw.as_bytes()).unwrap();
        assert_eq!(observed.group, 0); // ungrouped kernel tasks are valid observations.
        assert_eq!(observed.row.threads.value, 2);
        assert!(parse_stat(124, raw.as_bytes()).is_err());
        fields[1] = "-1";
        assert!(parse_stat(123, format!("123 (x) {}", fields.join(" ")).as_bytes()).is_err());
        fields[1] = "1";
        fields[11] = "bad";
        fields[17] = "-2";
        fields[21] = "99999999999999999999999999999999999999999999999";
        let row = parse_stat(123, format!("123 (x) {}", fields.join(" ")).as_bytes())
            .unwrap()
            .row;
        assert_eq!(row.cpu_ns.tag, 0);
        assert_eq!(row.rss_bytes.tag, 0);
        assert_eq!(row.threads.tag, 0);
    }
}
