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
#[cfg(target_os = "macos")]
fn error() -> i32 {
    io_error_to_status(&std::io::Error::last_os_error())
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
fn observe(pid: i32) -> Result<Option<Observation>, i32> {
    use std::io::Read;
    let file = match std::fs::File::open(format!("/proc/{pid}/stat")) {
        Ok(file) => file,
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ESRCH)) => {
            return Ok(None);
        }
        Err(error) => return Err(io_error_to_status(&error)),
    };
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error_to_status(&e))?;
    if bytes.len() > 65536 {
        return Err(AL_INVALID);
    }
    parse_stat(pid, &bytes).map(Some)
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
    let fields: Vec<_> = tail.split_ascii_whitespace().collect();
    let number = |index: usize| fields.get(index).and_then(|text| text.parse::<i128>().ok());
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
    if used < 0 {
        return Err(error());
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
fn observe(pid: i32) -> Result<Option<Observation>, i32> {
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
        for pid in pids {
            if let Some(observation) = observe(pid)? {
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
        for pid in candidates(maximum)? {
            if let Some(observation) = observe(pid)?
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
        let observation = observe(pid).unwrap().unwrap();
        assert_eq!(observation.row.pid, i64::from(pid));
        assert!(observation.row.parent_pid >= 0);
        assert!(matches!(observation.row.threads,OptionalCount { tag:1,value,.. } if value>=1));
        if pids.len() > 1 {
            assert!(candidates(1).is_err());
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
