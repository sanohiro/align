//! Advisory OS RAM observations, with fixed scratch and no retained native resource.

use super::{AL_INVALID, io_error_to_status};

#[cfg(any(target_os = "linux", test))]
const MEMINFO_CAP: usize = 8192;

#[cfg(any(target_os = "macos", test))]
fn total_bytes(value: u64) -> Result<i64, i32> {
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(AL_INVALID)
}

#[cfg(any(target_os = "linux", test))]
fn kib_bytes(mut input: &[u8]) -> Result<i64, i32> {
    input = input.strip_suffix(b"\r").unwrap_or(input);
    let space = |byte: u8| matches!(byte, b' ' | b'\t');
    let gap = input.iter().take_while(|byte| space(**byte)).count();
    if gap == 0 {
        return Err(AL_INVALID);
    }
    input = input.get(gap..).ok_or(AL_INVALID)?;
    let mut count = 0u64;
    let digits = input
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return Err(AL_INVALID);
    }
    for byte in input.get(..digits).ok_or(AL_INVALID)? {
        count = count
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(*byte - b'0')))
            .ok_or(AL_INVALID)?;
    }
    input = input.get(digits..).ok_or(AL_INVALID)?;
    let gap = input.iter().take_while(|byte| space(**byte)).count();
    if gap == 0 {
        return Err(AL_INVALID);
    }
    let tail = input
        .get(gap..)
        .and_then(|tail| tail.strip_prefix(b"kB"))
        .ok_or(AL_INVALID)?;
    if !tail.iter().all(|byte| space(*byte)) {
        return Err(AL_INVALID);
    }
    let bytes = count.checked_mul(1024).ok_or(AL_INVALID)?;
    i64::try_from(bytes).map_err(|_| AL_INVALID)
}

#[cfg(any(target_os = "linux", test))]
fn meminfo_with(
    input: &[u8],
    available: bool,
    mut parse: impl FnMut(&[u8]) -> Result<i64, i32>,
) -> Result<i64, i32> {
    if input.len() > MEMINFO_CAP {
        return Err(AL_INVALID);
    }
    let (mut total, mut spare) = (None, None);
    let (mut total_duplicate, mut spare_duplicate) = (false, false);
    for line in input.split(|byte| *byte == b'\n') {
        if let Some(field) = line.strip_prefix(b"MemTotal:") {
            if total.replace(field).is_some() {
                total_duplicate = true;
            }
        } else if available {
            if let Some(field) = line.strip_prefix(b"MemAvailable:") {
                if spare.replace(field).is_some() {
                    spare_duplicate = true;
                }
            }
        }
    }
    if total_duplicate {
        return Err(AL_INVALID);
    }
    let total = parse(total.ok_or(AL_INVALID)?)?;
    if total <= 0 {
        return Err(AL_INVALID);
    }
    if !available {
        return Ok(total);
    }
    if spare_duplicate {
        return Err(AL_INVALID);
    }
    let spare = parse(spare.ok_or(AL_INVALID)?)?;
    if spare < 0 || spare > total {
        return Err(AL_INVALID);
    }
    Ok(spare)
}

#[cfg(any(target_os = "linux", test))]
fn read_meminfo(
    available: bool,
    mut read: impl FnMut(&mut [u8]) -> std::io::Result<usize>,
) -> Result<i64, i32> {
    let mut bytes = [0u8; MEMINFO_CAP + 1];
    let mut filled = 0;
    loop {
        let output = bytes.get_mut(filled..).ok_or(AL_INVALID)?;
        let length = output.len();
        let count = match read(output) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error_to_status(&error)),
        };
        if count > length {
            return Err(AL_INVALID);
        }
        if count == 0 {
            return meminfo_with(bytes.get(..filled).ok_or(AL_INVALID)?, available, kib_bytes);
        }
        filled = filled.checked_add(count).ok_or(AL_INVALID)?;
        if filled > MEMINFO_CAP {
            return Err(AL_INVALID);
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_query(
    available: bool,
    open: impl FnOnce() -> Result<std::os::fd::OwnedFd, i32>,
    mut read: impl FnMut(i32, &mut [u8]) -> std::io::Result<usize>,
) -> Result<i64, i32> {
    use std::os::fd::AsRawFd;
    let fd = open()?;
    read_meminfo(available, |output| read(fd.as_raw_fd(), output))
}

#[cfg(target_os = "linux")]
fn observe(available: bool) -> Result<i64, i32> {
    use std::os::fd::{FromRawFd, OwnedFd};
    linux_query(
        available,
        || {
            let fd =
                unsafe { libc::open(c"/proc/meminfo".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
            if fd < 0 {
                return Err(io_error_to_status(&std::io::Error::last_os_error()));
            }
            // One successful acquisition, immediately transferred to its exclusive close owner.
            Ok(unsafe { OwnedFd::from_raw_fd(fd) })
        },
        |fd, output| {
            let count = unsafe { libc::read(fd, output.as_mut_ptr().cast(), output.len()) };
            if count < 0 {
                return Err(std::io::Error::last_os_error());
            }
            usize::try_from(count).map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))
        },
    )
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pages {
    free: u64,
    inactive: u64,
}

#[cfg(any(target_os = "macos", test))]
trait MacOps {
    fn total(&mut self) -> Result<u64, i32>;
    fn host(&mut self) -> u32;
    fn page_size(&mut self, host: u32) -> Result<u64, i32>;
    fn pages(&mut self, host: u32) -> Result<Pages, i32>;
    fn release(&mut self, host: u32) -> Result<(), i32>;
}

#[cfg(any(target_os = "macos", test))]
struct HostRight<'a, O: MacOps> {
    ops: &'a mut O,
    host: u32,
    owned: bool,
}

#[cfg(any(target_os = "macos", test))]
impl<O: MacOps> HostRight<'_, O> {
    fn finish(mut self) -> Result<(), i32> {
        self.owned = false;
        self.ops.release(self.host)
    }
}

#[cfg(any(target_os = "macos", test))]
impl<O: MacOps> Drop for HostRight<'_, O> {
    fn drop(&mut self) {
        if self.owned {
            let _ = self.ops.release(self.host);
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn mac_query(available: bool, ops: &mut impl MacOps) -> Result<i64, i32> {
    let total = total_bytes(ops.total()?)?;
    if !available {
        return Ok(total);
    }
    let host = ops.host();
    if host == 0 || host == u32::MAX {
        return Err(AL_INVALID);
    }
    let right = HostRight {
        ops,
        host,
        owned: true,
    };
    let queried = (|| {
        let page_size = right.ops.page_size(host)?;
        if page_size == 0 {
            return Err(AL_INVALID);
        }
        let pages = right.ops.pages(host)?;
        Ok((page_size, pages))
    })();
    let released = right.finish();
    let (page_size, pages) = queried?;
    released?;
    let bytes = pages
        .free
        .checked_add(pages.inactive)
        .and_then(|count| count.checked_mul(page_size))
        .ok_or(AL_INVALID)?;
    let bytes = i64::try_from(bytes).map_err(|_| AL_INVALID)?;
    if bytes > total {
        return Err(AL_INVALID);
    }
    Ok(bytes)
}

#[cfg(any(target_os = "macos", test))]
fn sysctl_value(value: u64, length: usize) -> Result<u64, i32> {
    if length != core::mem::size_of::<u64>() {
        return Err(AL_INVALID);
    }
    Ok(value)
}

#[cfg(target_os = "macos")]
struct NativeMac;

#[cfg(target_os = "macos")]
fn native_pages(
    stats: &libc::vm_statistics64,
    count: libc::mach_msg_type_number_t,
) -> Result<Pages, i32> {
    if count != libc::HOST_VM_INFO64_COUNT {
        return Err(AL_INVALID);
    }
    Ok(Pages {
        free: u64::from(stats.free_count),
        inactive: u64::from(stats.inactive_count),
    })
}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // Existing libc Mach port accessors retain the SDK C ABI.
impl MacOps for NativeMac {
    fn total(&mut self) -> Result<u64, i32> {
        let mut value = 0u64;
        let mut length = core::mem::size_of::<u64>();
        let status = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&mut value as *mut u64).cast(),
                &mut length,
                core::ptr::null_mut(),
                0,
            )
        };
        if status != 0 {
            return Err(io_error_to_status(&std::io::Error::last_os_error()));
        }
        sysctl_value(value, length)
    }
    fn host(&mut self) -> u32 {
        unsafe { libc::mach_host_self() }
    }
    fn page_size(&mut self, host: u32) -> Result<u64, i32> {
        // The kernel page size, not a possibly translated process's sysconf page size.
        unsafe extern "C" {
            fn host_page_size(
                host: libc::host_t,
                size: *mut libc::vm_size_t,
            ) -> libc::kern_return_t;
        }
        let mut size = 0;
        if unsafe { host_page_size(host, &mut size) } != libc::KERN_SUCCESS {
            return Err(AL_INVALID);
        }
        u64::try_from(size).map_err(|_| AL_INVALID)
    }
    fn pages(&mut self, host: u32) -> Result<Pages, i32> {
        let mut stats: libc::vm_statistics64 = unsafe { core::mem::zeroed() };
        let mut count = libc::HOST_VM_INFO64_COUNT;
        if unsafe {
            libc::host_statistics64(
                host,
                libc::HOST_VM_INFO64,
                (&mut stats as *mut libc::vm_statistics64).cast(),
                &mut count,
            )
        } != libc::KERN_SUCCESS
        {
            return Err(AL_INVALID);
        }
        native_pages(&stats, count)
    }
    fn release(&mut self, host: u32) -> Result<(), i32> {
        unsafe extern "C" {
            fn mach_port_deallocate(
                task: libc::mach_port_t,
                name: libc::mach_port_t,
            ) -> libc::kern_return_t;
        }
        if unsafe { mach_port_deallocate(libc::mach_task_self(), host) } == libc::KERN_SUCCESS {
            Ok(())
        } else {
            Err(AL_INVALID)
        }
    }
}

#[cfg(target_os = "macos")]
fn observe(available: bool) -> Result<i64, i32> {
    mac_query(available, &mut NativeMac)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn observe(_available: bool) -> Result<i64, i32> {
    Err(AL_INVALID)
}

unsafe fn write_count(out: *mut i64, query: impl FnOnce() -> Result<i64, i32>) -> i32 {
    if out.is_null()
        || !out.addr().is_multiple_of(8)
        || out
            .addr()
            .checked_add(core::mem::size_of::<i64>())
            .is_none()
    {
        return AL_INVALID;
    }
    unsafe { out.write(0) };
    match query() {
        Ok(value) => {
            unsafe { out.write(value) };
            0
        }
        Err(status) => status,
    }
}

/// Observe current kernel/VM RAM total in bytes.
///
/// # Safety
/// `out` must designate exclusive writable eight-byte scratch aligned to eight.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_os_physical_memory(out: *mut i64) -> i32 {
    unsafe { write_count(out, || observe(false)) }
}

/// Observe advisory available RAM in bytes; this never reserves or commits memory.
///
/// # Safety
/// `out` must designate exclusive writable eight-byte scratch aligned to eight.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_os_available_memory(out: *mut i64) -> i32 {
    unsafe { write_count(out, || observe(true)) }
}

const _: unsafe extern "C" fn(*mut i64) -> i32 = align_rt_os_physical_memory;
const _: unsafe extern "C" fn(*mut i64) -> i32 = align_rt_os_available_memory;

#[cfg(test)]
#[path = "os_memory_tests.rs"]
mod tests;
