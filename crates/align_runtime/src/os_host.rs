//! Owned OS observations. Native text is validated before allocating any output.
use super::{AlignStr, AL_INVALID, io_error_to_status, owned_str_copy};

#[repr(C)]
struct OptionalString { tag: u8, padding: [u8; 7], value: AlignStr }
#[repr(C)]
struct OptionalCount { tag: u8, padding: [u8; 7], value: i64 }
#[repr(C)]
struct HostInfo {
    system: AlignStr,
    release: AlignStr,
    machine: AlignStr,
    cpu: OptionalString,
    logical_cpu_count: OptionalCount,
}
impl HostInfo {
    fn empty() -> Self {
        let empty = || AlignStr { ptr: core::ptr::null(), len: 0 };
        Self { system: empty(), release: empty(), machine: empty(),
            cpu: OptionalString { tag: 0, padding: [0; 7], value: empty() },
            logical_cpu_count: OptionalCount { tag: 0, padding: [0; 7], value: 0 } }
    }
}
fn field(bytes: &[u8]) -> Result<&[u8], i32> {
    let end = bytes.iter().position(|b| *b == 0).ok_or(AL_INVALID)?;
    let value = &bytes[..end];
    if value.is_empty() || core::str::from_utf8(value).is_err() { return Err(AL_INVALID); }
    Ok(value)
}
fn count(value: i128) -> OptionalCount {
    match i64::try_from(value).ok().filter(|n| *n > 0) {
        Some(value) => OptionalCount { tag: 1, padding: [0; 7], value },
        None => OptionalCount { tag: 0, padding: [0; 7], value: 0 },
    }
}
fn observe_fields(system: &[u8], release: &[u8], machine: &[u8], online: impl FnOnce() -> i128) -> Result<HostInfo, i32> {
    let system = field(system)?;
    let release = field(release)?;
    let machine = field(machine)?;
    let logical_cpu_count = count(online());
    Ok(HostInfo { system: owned_str_copy(system), release: owned_str_copy(release),
        machine: owned_str_copy(machine), logical_cpu_count, ..HostInfo::empty() })
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn observe() -> Result<HostInfo, i32> {
    observe_native(|| {
        let mut native: libc::utsname = unsafe { core::mem::zeroed() };
        if unsafe { libc::uname(&mut native) } != 0 {
            return Err(io_error_to_status(&std::io::Error::last_os_error()));
        }
        Ok(native)
    }, || i128::from(unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) }))
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn observe_native(uname: impl FnOnce() -> Result<libc::utsname, i32>, online: impl FnOnce() -> i128) -> Result<HostInfo, i32> {
    let native = uname()?;
    // Bounded initialized C arrays; c_char signedness does not alter their bytes.
    let bytes = |value: &[libc::c_char]| unsafe {
        core::slice::from_raw_parts(value.as_ptr().cast::<u8>(), value.len())
    };
    observe_fields(bytes(&native.sysname), bytes(&native.release), bytes(&native.machine), online)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn observe() -> Result<HostInfo, i32> { Err(AL_INVALID) }

/// Write one independently owned host record, or leave an empty scratch on error.
///
/// # Safety
/// `out` must designate an exclusive writable allocation of at least 88 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_os_host(out: *mut core::ffi::c_void) -> i32 {
    unsafe { write_observation(out, observe) }
}

unsafe fn write_observation(out: *mut core::ffi::c_void, query: impl FnOnce() -> Result<HostInfo, i32>) -> i32 {
    let address = out.addr();
    if out.is_null() || !address.is_multiple_of(core::mem::align_of::<HostInfo>())
        || address.checked_add(core::mem::size_of::<HostInfo>()).is_none() { return AL_INVALID; }
    let out = out.cast::<HostInfo>();
    unsafe { out.write(HostInfo::empty()) };
    match query() {
        Ok(value) => { unsafe { out.write(value) }; 0 }
        Err(status) => status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};
    #[test]
    fn host_native_layout() {
        assert_eq!((size_of::<HostInfo>(), align_of::<HostInfo>()), (88, 8));
        assert_eq!([offset_of!(HostInfo, system), offset_of!(HostInfo, release), offset_of!(HostInfo, machine), offset_of!(HostInfo, cpu), offset_of!(HostInfo, logical_cpu_count)], [0,16,32,48,72]);
        assert_eq!(offset_of!(OptionalString, value), 8);
        assert_eq!(offset_of!(OptionalCount, value), 8);
        let empty = HostInfo::empty();
        assert!(empty.cpu.value.ptr.is_null());
        assert_eq!(empty.cpu.value.len, 0);
        assert_eq!(empty.cpu.padding, [0;7]);
        assert_eq!(empty.logical_cpu_count.padding, [0;7]);
    }
    #[test]
    fn host_optional_count() {
        for input in [i128::MIN, -1, 0, i128::from(i64::MAX)+1] {
            let output = count(input);
            assert_eq!((output.tag, output.value), (0,0));
        }
        for input in [1, 1024, i64::MAX] {
            let output = count(i128::from(input));
            assert_eq!((output.tag, output.value), (1,input));
        }
    }
    #[test]
    fn host_validation_order() {
        let queried = core::cell::Cell::new(false);
        let result = observe_native(|| Err(super::super::AL_DENIED), || { queried.set(true); 1 });
        assert!(matches!(result, Err(super::super::AL_DENIED)));
        assert!(!queried.get());
        for invalid in [b"".as_slice(), b"\0", b"no terminator", b"\xff\0"] {
            for position in 0..3 {
                let mut fields: [&[u8];3] = [b"valid\0";3];
                fields[position] = invalid;
                let queried = core::cell::Cell::new(false);
                let result = observe_fields(fields[0],fields[1],fields[2], || { queried.set(true); 1 });
                assert!(matches!(result, Err(AL_INVALID)));
                assert!(!queried.get());
            }
        }
        assert_eq!(field(b"exact\0\xff"), Ok(b"exact".as_slice()));
    }
    #[test]
    fn host_invalid_out() {
        let mut scratch = core::mem::MaybeUninit::<HostInfo>::uninit();
        let status = unsafe { write_observation(scratch.as_mut_ptr().cast(), || Err(AL_INVALID)) };
        assert_eq!(status, AL_INVALID);
        let empty = unsafe { scratch.assume_init() };
        assert!(empty.system.ptr.is_null() && empty.release.ptr.is_null() && empty.machine.ptr.is_null());
        assert_eq!((empty.system.len,empty.release.len,empty.machine.len), (0,0,0));
        assert_eq!((empty.cpu.tag,empty.logical_cpu_count.tag), (0,0));
        for address in [0,1,usize::MAX-7] {
            // Each address fails the checked prefix before any dereference.
            assert_eq!(unsafe { align_rt_os_host(address as *mut core::ffi::c_void) }, AL_INVALID);
        }
    }
}
