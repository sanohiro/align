//! Explicit Linux sealed-file, executable and descriptor authority.
use super::process_live::{disjoint, valid_pointer, valid_range};
use super::{AL_INVALID, AlignStr, Command, Reader, io_error_to_status, panic_abort};
#[cfg(target_os = "linux")]
use std::ffi::CString;
use std::ffi::c_void;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};

#[cfg(target_os = "linux")]
const REQUIRED_SEALS: i32 = libc::F_SEAL_WRITE
    | libc::F_SEAL_GROW
    | libc::F_SEAL_SHRINK
    | libc::F_SEAL_EXEC
    | libc::F_SEAL_SEAL;
fn ranges_disjoint(a: *const u8, a_len: usize, b: *const u8, b_len: usize) -> bool {
    match (a.addr().checked_add(a_len), b.addr().checked_add(b_len)) {
        (Some(a_end), Some(b_end)) => a_end <= b.addr() || b_end <= a.addr(),
        _ => false,
    }
}
fn error() -> i32 {
    io_error_to_status(&std::io::Error::last_os_error())
}
fn unsupported() -> i32 {
    io_error_to_status(&std::io::Error::from_raw_os_error(libc::ENOTSUP))
}
fn supported() -> Result<(), i32> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        Err(unsupported())
    }
}
fn owned(raw: i32) -> Result<OwnedFd, i32> {
    if raw < 0 {
        Err(error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(raw) })
    }
}
pub(crate) fn duplicate(fd: &OwnedFd, minimum: i32) -> Result<OwnedFd, i32> {
    owned(unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, minimum) })
}
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // Retains the identical opaque Drop layout on unsupported hosts.
struct MemoryWriter {
    fd: OwnedFd,
    kind: i32,
    cap: i64,
    length: i64,
    poisoned: bool,
}
pub(crate) struct Sealed {
    pub(crate) fd: OwnedFd,
    kind: i32,
    length: i64,
}
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // No valid instance can be acquired on macOS.
pub(crate) enum Inheritance {
    File(Sealed),
    Namespace(OwnedFd),
}
impl Sealed {
    fn duplicate(&self) -> Result<Self, i32> {
        Ok(Self {
            fd: duplicate(&self.fd, 3)?,
            kind: self.kind,
            length: self.length,
        })
    }
}
fn memory_file(kind: i32, cap: i64) -> Result<Box<MemoryWriter>, i32> {
    if !matches!(kind, 0 | 1) || cap < 0 {
        return Err(AL_INVALID);
    }
    supported()?;
    #[cfg(target_os = "linux")]
    {
        let flags: u32 = libc::MFD_CLOEXEC
            | libc::MFD_ALLOW_SEALING
            | if kind == 0 {
                libc::MFD_NOEXEC_SEAL
            } else {
                libc::MFD_EXEC
            };
        let fd = owned(unsafe { libc::memfd_create(c"align".as_ptr(), flags) })?;
        Ok(Box::new(MemoryWriter {
            fd,
            kind,
            cap,
            length: 0,
            poisoned: false,
        }))
    }
    #[cfg(not(target_os = "linux"))]
    Err(unsupported())
}
impl MemoryWriter {
    fn write(&mut self, bytes: &[u8]) -> Result<(), i32> {
        if self.poisoned {
            return Err(AL_INVALID);
        }
        let count = i64::try_from(bytes.len()).map_err(|_| AL_INVALID)?;
        let total = self
            .length
            .checked_add(count)
            .filter(|n| *n <= self.cap)
            .ok_or(AL_INVALID)?;
        let mut done = 0;
        while done < bytes.len() {
            let count = unsafe {
                libc::write(
                    self.fd.as_raw_fd(),
                    bytes[done..].as_ptr().cast(),
                    bytes.len() - done,
                )
            };
            if count < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            let failure = if count < 0 { error() } else { AL_INVALID };
            let count = usize::try_from(count)
                .ok()
                .filter(|n| *n > 0 && *n <= bytes.len() - done);
            let Some(count) = count else {
                self.poisoned = true;
                return Err(failure);
            };
            done += count;
        }
        self.length = total;
        Ok(())
    }
    fn seal(self) -> Result<Sealed, i32> {
        if self.poisoned {
            return Err(AL_INVALID);
        }
        #[cfg(target_os = "linux")]
        {
            if unsafe {
                libc::fchmod(
                    self.fd.as_raw_fd(),
                    if self.kind == 0 { 0o400 } else { 0o500 },
                )
            } != 0
            {
                return Err(error());
            }
            if unsafe { libc::fcntl(self.fd.as_raw_fd(), libc::F_ADD_SEALS, REQUIRED_SEALS) } != 0 {
                return Err(error());
            }
            check_seals(&self.fd)?;
            let stat = metadata(&self.fd)?;
            if stat.st_size < 0 || stat.st_size > self.cap {
                return Err(AL_INVALID);
            }
            Ok(Sealed {
                fd: self.fd,
                kind: self.kind,
                length: stat.st_size,
            })
        }
        #[cfg(not(target_os = "linux"))]
        Err(unsupported())
    }
}
fn metadata(fd: &OwnedFd) -> Result<libc::stat, i32> {
    let mut stat = unsafe { core::mem::zeroed() };
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } != 0 {
        Err(error())
    } else {
        Ok(stat)
    }
}
#[cfg(target_os = "linux")]
fn check_seals(fd: &OwnedFd) -> Result<(), i32> {
    #[cfg(target_os = "linux")]
    {
        let bits = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GET_SEALS) };
        if bits < 0 {
            return Err(error());
        }
        if bits & REQUIRED_SEALS != REQUIRED_SEALS {
            return Err(AL_INVALID);
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = fd;
        Err(unsupported())
    }
}
fn read_at(file: &Sealed, offset: i64, bytes: &mut [u8]) -> Result<i64, i32> {
    if offset < 0 || bytes.is_empty() {
        return Err(AL_INVALID);
    }
    let count = unsafe {
        libc::pread(
            file.fd.as_raw_fd(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
            offset,
        )
    };
    if count < 0 {
        Err(error())
    } else {
        i64::try_from(count).map_err(|_| AL_INVALID)
    }
}
fn executable(file: &Sealed) -> Result<Sealed, i32> {
    if file.kind != 1 || file.length < 64 {
        return Err(AL_INVALID);
    }
    supported()?;
    let mut header = [0u8; 64];
    if read_at(file, 0, &mut header)? != 64 {
        return Err(AL_INVALID);
    }
    let machine: u16 = if cfg!(target_arch = "x86_64") {
        62
    } else {
        183
    };
    if header[..7] != [127, b'E', b'L', b'F', 2, 1, 1]
        || !matches!(u16::from_le_bytes([header[16], header[17]]), 2 | 3)
        || u16::from_le_bytes([header[18], header[19]]) != machine
        || u32::from_le_bytes([header[20], header[21], header[22], header[23]]) != 1
        || u16::from_le_bytes([header[52], header[53]]) != 64
    {
        return Err(AL_INVALID);
    }
    file.duplicate()
}
#[cfg(target_os = "linux")]
pub(crate) fn launch_bindings(command: &Command, minimum: i32) -> Result<Vec<(i32, OwnedFd)>, i32> {
    let mut result = Vec::with_capacity(command.inheritance.len());
    for (slot, binding) in &command.inheritance {
        let fd = match binding {
            Inheritance::Namespace(fd) => duplicate(fd, minimum)?,
            Inheritance::File(file) => {
                let before = metadata(&file.fd)?;
                let path = CString::new(format!("/proc/self/fd/{}", file.fd.as_raw_fd()))
                    .map_err(|_| AL_INVALID)?;
                let reopened =
                    owned(unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) })?;
                let after = metadata(&reopened)?;
                check_seals(&reopened)?;
                if (before.st_dev, before.st_ino, before.st_size)
                    != (after.st_dev, after.st_ino, after.st_size)
                    || after.st_size != file.length
                {
                    return Err(AL_INVALID);
                }
                duplicate(&reopened, minimum)?
            }
        };
        result.push((*slot, fd));
    }
    Ok(result)
}
fn namespace(path: &[u8]) -> Result<OwnedFd, i32> {
    if path.is_empty() || path.contains(&0) || std::str::from_utf8(path).is_err() {
        return Err(AL_INVALID);
    }
    supported()?;
    #[cfg(target_os = "linux")]
    {
        let path = CString::new(path).map_err(|_| AL_INVALID)?;
        let fd = owned(unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) })?;
        // _IO(0xb7,3): NS_GET_NSTYPE has no pointer-sized argument or host-width encoding.
        let kind = unsafe { libc::ioctl(fd.as_raw_fd(), 0xb703) };
        if kind < 0 {
            return Err(error());
        }
        if kind != libc::CLONE_NEWUSER {
            return Err(AL_INVALID);
        }
        Ok(fd)
    }
    #[cfg(not(target_os = "linux"))]
    Err(unsupported())
}

/// # Safety
/// out is an aligned writable pointer slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_memory_file(
    kind: i32,
    cap: i64,
    out: *mut *mut c_void,
) -> i32 {
    if !valid_pointer(out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(core::ptr::null_mut());
    }
    match memory_file(kind, cap) {
        Ok(value) => {
            unsafe {
                out.write(Box::into_raw(value).cast());
            }
            0
        }
        Err(error) => error,
    }
}
/// # Safety
/// writer is exclusively borrowed and data is a disjoint valid readonly range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_memory_write(
    writer: *mut c_void,
    data: *const u8,
    len: i64,
) -> i32 {
    let writer = writer.cast::<MemoryWriter>();
    let Some((start, end)) = valid_range(data, len) else {
        return AL_INVALID;
    };
    let count = end - start;
    if !valid_pointer(writer)
        || !ranges_disjoint(
            writer.cast(),
            core::mem::size_of::<MemoryWriter>(),
            data,
            count,
        )
    {
        return AL_INVALID;
    }
    let bytes = if count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, count) }
    };
    match unsafe { &mut *writer }.write(bytes) {
        Ok(()) => 0,
        Err(error) => error,
    }
}
/// # Safety
/// writer transfers one owned shell; out is a disjoint aligned writable pointer slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_memory_seal(
    writer: *mut c_void,
    out: *mut *mut c_void,
) -> i32 {
    let writer = writer.cast::<MemoryWriter>();
    if !valid_pointer(writer) || !valid_pointer(out) || !disjoint(writer, out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(core::ptr::null_mut());
    }
    let value = unsafe { Box::from_raw(writer) };
    match value.seal() {
        Ok(file) => {
            unsafe {
                out.write(Box::into_raw(Box::new(file)).cast());
            }
            0
        }
        Err(error) => error,
    }
}
/// # Safety
/// file is shared-borrowed; out is a disjoint writable pointer slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_executable(
    file: *const c_void,
    out: *mut *mut c_void,
) -> i32 {
    let file = file.cast::<Sealed>();
    if !valid_pointer(file) || !valid_pointer(out) || !disjoint(file, out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(core::ptr::null_mut());
    }
    match executable(unsafe { &*file }) {
        Ok(image) => {
            unsafe {
                out.write(Box::into_raw(Box::new(image)).cast());
            }
            0
        }
        Err(error) => error,
    }
}
unsafe fn length(file: *const c_void) -> i64 {
    let file = file.cast::<Sealed>();
    if !valid_pointer(file) {
        panic_abort("invalid sealed owner");
    }
    unsafe { (*file).length }
}
unsafe fn positional(
    file: *const c_void,
    offset: i64,
    data: *mut u8,
    cap: i64,
    out: *mut i64,
) -> i32 {
    let file = file.cast::<Sealed>();
    let Some((start, end)) = valid_range(data, cap) else {
        return AL_INVALID;
    };
    let count = end - start;
    if count == 0
        || offset < 0
        || !valid_pointer(file)
        || !valid_pointer(out)
        || !disjoint(file, out)
        || !ranges_disjoint(file.cast(), core::mem::size_of::<Sealed>(), data, count)
        || !ranges_disjoint(out.cast(), core::mem::size_of::<i64>(), data, count)
    {
        return AL_INVALID;
    }
    unsafe {
        out.write(0);
    }
    match read_at(unsafe { &*file }, offset, unsafe {
        core::slice::from_raw_parts_mut(data, count)
    }) {
        Ok(count) => {
            unsafe {
                out.write(count);
            }
            0
        }
        Err(error) => error,
    }
}
/// # Safety
/// file is a valid shared sealed owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_sealed_len(file: *const c_void) -> i64 {
    unsafe { length(file) }
}
/// # Safety
/// image is a valid shared image owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_image_len(image: *const c_void) -> i64 {
    unsafe { length(image) }
}
/// # Safety
/// file, data and output are valid disjoint shared/writable ranges.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_sealed_read_at(
    file: *const c_void,
    offset: i64,
    data: *mut u8,
    cap: i64,
    out: *mut i64,
) -> i32 {
    unsafe { positional(file, offset, data, cap, out) }
}
/// # Safety
/// image, data and output are valid disjoint shared/writable ranges.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_image_read_at(
    image: *const c_void,
    offset: i64,
    data: *mut u8,
    cap: i64,
    out: *mut i64,
) -> i32 {
    unsafe { positional(image, offset, data, cap, out) }
}
/// # Safety
/// owner is null or one owned memory writer allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_memory_free(owner: *mut c_void) {
    if !owner.is_null() {
        unsafe {
            drop(Box::from_raw(owner.cast::<MemoryWriter>()));
        }
    }
}
/// # Safety
/// owner is null or one owned sealed file allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_sealed_free(owner: *mut c_void) {
    if !owner.is_null() {
        unsafe {
            drop(Box::from_raw(owner.cast::<Sealed>()));
        }
    }
}
/// # Safety
/// owner is null or one owned image allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_image_free(owner: *mut c_void) {
    if !owner.is_null() {
        unsafe {
            drop(Box::from_raw(owner.cast::<Sealed>()));
        }
    }
}
/// # Safety
/// owner is null or one owned namespace allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_user_namespace_free(owner: *mut c_void) {
    if !owner.is_null() {
        unsafe {
            drop(Box::from_raw(owner.cast::<OwnedFd>()));
        }
    }
}
/// # Safety
/// path is valid text; out is an aligned disjoint writable owner slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_user_namespace(
    path: *const u8,
    len: i64,
    out: *mut *mut c_void,
) -> i32 {
    let Some((start, end)) = valid_range(path, len) else {
        return AL_INVALID;
    };
    let count = end - start;
    if !valid_pointer(out)
        || !ranges_disjoint(out.cast(), core::mem::size_of::<*mut c_void>(), path, count)
    {
        return AL_INVALID;
    }
    unsafe {
        out.write(core::ptr::null_mut());
    }
    let bytes = if count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(path, count) }
    };
    match namespace(bytes) {
        Ok(fd) => {
            unsafe {
                out.write(Box::into_raw(Box::new(fd)).cast());
            }
            0
        }
        Err(error) => error,
    }
}
/// # Safety
/// out is an aligned writable reader-owner slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_process_current_image(out: *mut *mut Reader) -> i32 {
    if !valid_pointer(out) {
        return AL_INVALID;
    }
    unsafe {
        out.write(core::ptr::null_mut());
    }
    let result = (|| {
        supported()?;
        let fd = owned(unsafe {
            libc::open(c"/proc/self/exe".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC)
        })?;
        if metadata(&fd)?.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(AL_INVALID);
        }
        Ok(Box::new(Reader::unbuffered(fd.into_raw_fd(), true)))
    })();
    match result {
        Ok(reader) => {
            unsafe {
                out.write(Box::into_raw(reader));
            }
            0
        }
        Err(error) => error,
    }
}

/// # Safety
/// image is shared-borrowed; argv views and out are valid disjoint ranges.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_command_image(
    image: *const c_void,
    argv: *const AlignStr,
    argc: i64,
    out: *mut *mut Command,
) -> i32 {
    let image = image.cast::<Sealed>();
    let Some((start, end)) = valid_range(argv, argc) else {
        return AL_INVALID;
    };
    if argc <= 0
        || !valid_pointer(image)
        || !valid_pointer(out)
        || !disjoint(image, out)
        || !ranges_disjoint(
            out.cast(),
            core::mem::size_of::<*mut Command>(),
            argv.cast(),
            end - start,
        )
    {
        return AL_INVALID;
    }
    let Ok(count) = usize::try_from(argc) else {
        return AL_INVALID;
    };
    for arg in unsafe { core::slice::from_raw_parts(argv, count) } {
        let Some((start, end)) = valid_range(arg.ptr, arg.len) else {
            return AL_INVALID;
        };
        if !ranges_disjoint(
            out.cast(),
            core::mem::size_of::<*mut Command>(),
            arg.ptr,
            end - start,
        ) {
            return AL_INVALID;
        }
        let Some(text) = (unsafe { super::abi_str_view(arg.ptr, arg.len) }) else {
            return AL_INVALID;
        };
        if text.as_bytes().contains(&0) {
            return AL_INVALID;
        }
    }
    unsafe {
        out.write(core::ptr::null_mut());
    }
    let result = (|| {
        supported()?;
        let (_, arguments, _) =
            unsafe { super::marshal_cmd_argv(b"image".as_ptr(), 5, argv, argc) }?;
        let fd = duplicate(unsafe { &(*image).fd }, 3)?;
        Ok(Box::new(Command {
            target: super::CommandTarget::Image(fd),
            inheritance: std::collections::BTreeMap::new(),
            argv: arguments,
            cwd: None,
            timeout_ns: 0,
            max_capture_bytes: None,
            new_session: false,
            stdout_binding: None,
            stderr_binding: None,
            env: Vec::new(),
            env_clear: false,
        }))
    })();
    match result {
        Ok(command) => {
            unsafe {
                out.write(Box::into_raw(command));
            }
            0
        }
        Err(error) => error,
    }
}
unsafe fn inherit(command: *mut Command, source: *const c_void, slot: i64, file: bool) -> i32 {
    if !valid_pointer(command) {
        return AL_INVALID;
    }
    if (file && !valid_pointer(source.cast::<Sealed>()))
        || (!file && !valid_pointer(source.cast::<OwnedFd>()))
    {
        return AL_INVALID;
    }
    if !(3..=1023).contains(&slot) {
        return AL_INVALID;
    }
    let Ok(slot) = i32::try_from(slot) else {
        return AL_INVALID;
    };
    if (file && !disjoint(command, source.cast::<Sealed>()))
        || (!file && !disjoint(command, source.cast::<OwnedFd>()))
    {
        return AL_INVALID;
    }
    let command = unsafe { &mut *command };
    if command.inheritance.contains_key(&slot) {
        return AL_INVALID;
    }
    if let Err(error) = supported() {
        return error;
    }
    let binding = if file {
        unsafe { &*source.cast::<Sealed>() }
            .duplicate()
            .map(Inheritance::File)
    } else {
        duplicate(unsafe { &*source.cast::<OwnedFd>() }, 3).map(Inheritance::Namespace)
    };
    match binding {
        Ok(binding) => {
            command.inheritance.insert(slot, binding);
            0
        }
        Err(error) => error,
    }
}
/// # Safety
/// command is exclusively borrowed and file is a distinct shared sealed owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_command_inherit_file(
    command: *mut Command,
    file: *const c_void,
    slot: i64,
) -> i32 {
    unsafe { inherit(command, file, slot, true) }
}
/// # Safety
/// command is exclusively borrowed and namespace is a distinct shared namespace owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_command_inherit_namespace(
    command: *mut Command,
    namespace: *const c_void,
    slot: i64,
) -> i32 {
    unsafe { inherit(command, namespace, slot, false) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scalar_ranges_and_unsupported_acquisition() {
        for (kind, cap) in [(-1, 0), (2, 0), (0, -1), (i32::MAX, i64::MAX)] {
            assert!(matches!(memory_file(kind, cap), Err(AL_INVALID)));
        }
        assert!(matches!(namespace(b""), Err(AL_INVALID)));
        assert!(matches!(namespace(b"x\0y"), Err(AL_INVALID)));
        assert!(matches!(namespace(&[255]), Err(AL_INVALID)));
        #[cfg(target_os = "macos")]
        {
            assert!(
                matches!(memory_file(0, 0), Err(value) if value == super::super::AL_CODE + libc::ENOTSUP)
            );
            assert!(
                matches!(memory_file(1, 0), Err(value) if value == super::super::AL_CODE + libc::ENOTSUP)
            );
            assert!(
                matches!(namespace(b"/valid"), Err(value) if value == super::super::AL_CODE + libc::ENOTSUP)
            );
            let mut out = core::ptr::null_mut();
            assert_eq!(
                unsafe { align_rt_process_current_image(&mut out) },
                super::super::AL_CODE + libc::ENOTSUP
            );
            assert!(out.is_null());
        }
    }
    #[cfg(target_os = "linux")]
    fn sealed(kind: i32, bytes: &[u8]) -> Sealed {
        let mut writer = memory_file(kind, i64::try_from(bytes.len()).unwrap()).unwrap();
        writer.write(bytes).unwrap();
        writer.seal().unwrap()
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn seals_caps_poison_aliases_and_positional_prefix() {
        for kind in [0, 1] {
            let mut writer = memory_file(kind, 3).unwrap();
            writer.write(b"ab").unwrap();
            assert_eq!(writer.write(b"cd"), Err(AL_INVALID));
            writer.write(b"c").unwrap();
            let alias = duplicate(&writer.fd, 3).unwrap();
            let file = writer.seal().unwrap();
            assert_eq!(file.length, 3);
            let mut bytes = [99; 5];
            assert_eq!(read_at(&file, 1, &mut bytes), Ok(2));
            assert_eq!(bytes, [b'b', b'c', 99, 99, 99]);
            assert_eq!(read_at(&file, 3, &mut bytes), Ok(0));
            assert_eq!(read_at(&file, -1, &mut bytes), Err(AL_INVALID));
            for fd in [&file.fd, &alias] {
                assert_eq!(
                    unsafe { libc::pwrite(fd.as_raw_fd(), b"x".as_ptr().cast(), 1, 0) },
                    -1
                );
                for size in [0, 4] {
                    assert_eq!(unsafe { libc::ftruncate(fd.as_raw_fd(), size) }, -1);
                }
                assert_eq!(
                    unsafe { libc::fchmod(fd.as_raw_fd(), if kind == 0 { 0o500 } else { 0o400 }) },
                    -1
                );
                check_seals(fd).unwrap();
            }
        }
        let mut poisoned = memory_file(0, 3).unwrap();
        assert_eq!(
            unsafe {
                libc::fcntl(
                    poisoned.fd.as_raw_fd(),
                    libc::F_ADD_SEALS,
                    libc::F_SEAL_WRITE,
                )
            },
            0
        );
        assert!(poisoned.write(b"x").is_err());
        assert_eq!(poisoned.write(b""), Err(AL_INVALID));
        assert!(matches!(poisoned.seal(), Err(AL_INVALID)));
        let writer = memory_file(0, 1).unwrap();
        assert_eq!(
            unsafe { libc::pwrite(writer.fd.as_raw_fd(), b"xy".as_ptr().cast(), 2, 0) },
            2
        );
        assert!(matches!(writer.seal(), Err(AL_INVALID)));
        let mut writer = memory_file(0, 4096).unwrap();
        writer.write(&[0; 4096]).unwrap();
        let mapping = unsafe {
            libc::mmap(
                core::ptr::null_mut(),
                4096,
                libc::PROT_WRITE,
                libc::MAP_SHARED,
                writer.fd.as_raw_fd(),
                0,
            )
        };
        assert_ne!(mapping, libc::MAP_FAILED);
        let result = writer.seal();
        assert_eq!(unsafe { libc::munmap(mapping, 4096) }, 0);
        assert!(result.is_err());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn elf_admission_and_descriptor_selected_execution() {
        const MARKER: &str = "ALIGN_R65_VERIFIED_EXEC";
        if std::env::var_os(MARKER).is_none() {
            crate::process_launch::tests::isolated(
                "process_verified::tests::elf_admission_and_descriptor_selected_execution",
                MARKER,
            );
            return;
        }
        assert!(matches!(executable(&sealed(0, b"")), Err(AL_INVALID)));
        for bytes in [&b"#!/bin/sh\nexit 0\n"[..], &[0; 64][..]] {
            assert!(matches!(executable(&sealed(1, bytes)), Err(AL_INVALID)));
        }
        let bytes = std::fs::read("/bin/true").unwrap();
        for index in [0, 4, 5, 6, 16, 18, 20, 52] {
            let mut bad = bytes[..64].to_vec();
            bad[index] = 255;
            assert!(
                matches!(executable(&sealed(1, &bad)), Err(AL_INVALID)),
                "ELF byte {index}"
            );
        }
        let file = sealed(1, &bytes);
        let image = executable(&file).unwrap();
        let name = b"unrelated-argv-zero";
        let arguments = [AlignStr {
            ptr: name.as_ptr(),
            len: i64::try_from(name.len()).unwrap(),
        }];
        let mut command = core::ptr::null_mut();
        assert_eq!(
            unsafe {
                align_rt_command_image(
                    (&raw const image).cast(),
                    arguments.as_ptr(),
                    1,
                    &mut command,
                )
            },
            0
        );
        let command = unsafe { Box::from_raw(command) };
        drop(image);
        drop(file);
        for fallback in [false, true] {
            crate::process_launch::FORCE_FD_SCAN
                .store(fallback, std::sync::atomic::Ordering::Relaxed);
            let mut child = super::super::process_launch::launch(&command, false, false).unwrap();
            assert_eq!(child.wait().unwrap().termination.exited, 0);
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn opened_namespace_and_running_main_image() {
        let ns = namespace(b"/proc/self/ns/user").unwrap();
        assert_eq!(
            unsafe { libc::ioctl(ns.as_raw_fd(), 0xb703) },
            libc::CLONE_NEWUSER
        );
        assert!(namespace(b"/proc/self/ns/mnt").is_err());
        assert!(namespace(b"/dev/null").is_err());
        let mut reader = core::ptr::null_mut();
        assert_eq!(unsafe { align_rt_process_current_image(&mut reader) }, 0);
        let reader = unsafe { Box::from_raw(reader) };
        let mut bytes = [0; 4];
        assert_eq!(
            unsafe { libc::read(reader.fd, bytes.as_mut_ptr().cast(), 4) },
            4
        );
        assert_eq!(bytes, [127, b'E', b'L', b'F']);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn native_layout_and_ranges() {
        unsafe {
            let mut writer = core::ptr::null_mut();
            assert_eq!(align_rt_fs_memory_file(0, 3, &mut writer), 0);
            assert_eq!(
                align_rt_fs_memory_write(writer, writer.cast(), 1),
                AL_INVALID
            );
            assert_eq!(
                align_rt_fs_memory_write(writer, b"a".as_ptr(), -1),
                AL_INVALID
            );
            assert_eq!(align_rt_fs_memory_write(writer, b"abc".as_ptr(), 3), 0);
            assert_eq!(align_rt_fs_memory_seal(writer, writer.cast()), AL_INVALID);
            let mut file = core::ptr::null_mut();
            assert_eq!(align_rt_fs_memory_seal(writer, &mut file), 0);
            let mut bytes = [99u8; 8];
            let mut count = 55i64;
            assert_eq!(
                align_rt_fs_sealed_read_at(
                    file,
                    0,
                    bytes.as_mut_ptr(),
                    3,
                    bytes.as_mut_ptr().cast()
                ),
                AL_INVALID
            );
            assert_eq!(bytes, [99; 8]);
            assert_eq!(
                align_rt_fs_sealed_read_at(file, 0, file.cast(), 1, &mut count),
                AL_INVALID
            );
            assert_eq!(count, 55);
            assert_eq!(
                align_rt_fs_sealed_read_at(file, 0, bytes.as_mut_ptr(), 0, &mut count),
                AL_INVALID
            );
            assert_eq!(
                align_rt_fs_sealed_read_at(file, 0, bytes.as_mut_ptr(), 8, &mut count),
                0
            );
            assert_eq!(count, 3);
            assert_eq!(bytes, [b'a', b'b', b'c', 99, 99, 99, 99, 99]);
            align_rt_fs_sealed_free(file);
            for free in [
                align_rt_fs_memory_free,
                align_rt_fs_sealed_free,
                align_rt_process_image_free,
                align_rt_process_user_namespace_free,
            ] {
                free(core::ptr::null_mut());
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn inheritance_matrix() {
        const MARKER: &str = "ALIGN_R65_VERIFIED_INHERIT";
        if std::env::var_os(MARKER).is_none() {
            crate::process_launch::tests::isolated(
                "process_verified::tests::inheritance_matrix",
                MARKER,
            );
            return;
        }
        let file = sealed(0, b"abc");
        let raw = file.fd.as_raw_fd();
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
        let mut command = crate::process_launch::tests::command(
            "test -e /dev/fd/3 && test -e /dev/fd/17 && test ! -e /dev/fd/1500",
        );
        let sentinel = duplicate(&file.fd, 1500).unwrap();
        for slot in [i64::MIN, 0, 1, 2, 1024, i64::MAX] {
            assert_eq!(
                unsafe {
                    align_rt_command_inherit_file(&raw mut command, (&raw const file).cast(), slot)
                },
                AL_INVALID
            );
            assert!(command.inheritance.is_empty());
        }
        for slot in [3, 17] {
            assert_eq!(
                unsafe {
                    align_rt_command_inherit_file(&raw mut command, (&raw const file).cast(), slot)
                },
                0
            );
            assert_eq!(
                unsafe {
                    align_rt_command_inherit_file(&raw mut command, (&raw const file).cast(), slot)
                },
                AL_INVALID
            );
        }
        for fallback in [false, true] {
            crate::process_launch::FORCE_FD_SCAN
                .store(fallback, std::sync::atomic::Ordering::Relaxed);
            let bindings = launch_bindings(&command, 18).unwrap();
            assert_eq!(
                bindings.iter().map(|(slot, _)| *slot).collect::<Vec<_>>(),
                [3, 17]
            );
            for (_, fd) in &bindings {
                assert!(fd.as_raw_fd() >= 18);
                assert_eq!(
                    unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) } & libc::O_ACCMODE,
                    libc::O_RDONLY
                );
                assert_eq!(unsafe { libc::lseek(fd.as_raw_fd(), 0, libc::SEEK_CUR) }, 0);
                assert_eq!(unsafe { libc::lseek(fd.as_raw_fd(), 2, libc::SEEK_SET) }, 2);
            }
            assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, flags);
            assert_eq!(unsafe { libc::lseek(raw, 0, libc::SEEK_CUR) }, 3);
            let mut child = crate::process_launch::launch(&command, true, false).unwrap();
            child.stdout.fd.take();
            child.stderr.fd.take();
            assert_eq!(child.wait().unwrap().termination.exited, 0);
        }
        drop(sentinel);
    }

    // A local resource measurement, deliberately not a latency/correctness gate.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn fixed_scratch_resource_measurement() {
        fn rss() -> u64 {
            std::fs::read_to_string("/proc/self/status")
                .unwrap()
                .lines()
                .find(|line| line.starts_with("VmRSS:"))
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse()
                .unwrap()
        }
        let scratch = [7u8; 65536];
        for length in [4 * 1024 * 1024, 64 * 1024 * 1024] {
            let before = rss();
            let mut writer = memory_file(0, length).unwrap();
            for _ in 0..length / 65536 {
                writer.write(&scratch).unwrap();
            }
            let file = writer.seal().unwrap();
            eprintln!(
                "sealed bytes={} fixed_scratch={} rss_before_kib={} rss_after_kib={}",
                file.length,
                scratch.len(),
                before,
                rss()
            );
        }
    }
}
