//! Retained directory identity, independent raw-name enumeration and descriptor observations.
//! Traversal, sorting, evidence policy and recursive cleanup remain application operations.

use super::*;
use core::ffi::c_void;
use core::mem::{align_of, size_of};

#[cfg(test)]
#[derive(Clone, Copy, Eq, PartialEq)]
enum Failure {
    CursorOpen,
    CursorStat,
    CursorStream,
    CursorRead,
    CreateStat,
    CreateName,
}
#[cfg(test)]
thread_local! {
    static FAILURE: core::cell::Cell<Option<Failure>> = const { core::cell::Cell::new(None) };
    static PARTIAL_FD: core::cell::Cell<i32> = const { core::cell::Cell::new(-1) };
}
#[cfg(test)]
fn inject(point: Failure) -> Result<(), i32> {
    if FAILURE.with(|value| value.get() == Some(point)) {
        Err(AL_CODE + libc::EIO)
    } else {
        Ok(())
    }
}

struct Directory {
    fd: BeneathFd,
}
struct Cursor {
    stream: *mut libc::DIR,
    terminal: i32,
}

impl Drop for Cursor {
    fn drop(&mut self) {
        // SAFETY: fdopendir transferred one independently opened descriptor into this stream.
        unsafe { libc::closedir(self.stream) };
    }
}

/// Physical order of the ordinary fs.metadata record, independently checked by compiler tests.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Metadata {
    device: u64,
    inode: u64,
    links: u64,
    size: i64,
    modified_seconds: i64,
    changed_seconds: i64,
    kind: u32,
    mode: u32,
    modified_nanoseconds: u32,
    changed_nanoseconds: u32,
}

#[derive(Clone, Copy)]
struct Extent {
    start: usize,
    end: usize,
}
impl Extent {
    fn checked(pointer: *const u8, len: usize, alignment: usize) -> Result<Self, i32> {
        if pointer.is_null() || !pointer.addr().is_multiple_of(alignment) {
            return Err(AL_INVALID);
        }
        Ok(Self {
            start: pointer.addr(),
            end: pointer.addr().checked_add(len).ok_or(AL_INVALID)?,
        })
    }
    fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[derive(Clone, Copy)]
struct Output {
    pointer: *mut u8,
    len: usize,
    alignment: usize,
}
impl Output {
    fn typed<T>(pointer: *mut T) -> Self {
        Self {
            pointer: pointer.cast(),
            len: size_of::<T>(),
            alignment: align_of::<T>(),
        }
    }
    fn extent(self) -> Result<Extent, i32> {
        Extent::checked(self.pointer, self.len, self.alignment)
    }
}

/// Numerical checks precede every dereference/write. Live allocations and writable fresh scratch
/// are ABI caller obligations; this function does not authenticate arbitrary native addresses.
unsafe fn prepare(
    outputs: &[Output],
    owner: Option<(*const u8, usize, usize)>,
    input: Option<(*const u8, i64)>,
) -> Result<(), i32> {
    unsafe { prepare_inputs(outputs, owner, input.as_slice()) }
}

unsafe fn prepare_inputs(
    outputs: &[Output],
    owner: Option<(*const u8, usize, usize)>,
    inputs: &[(*const u8, i64)],
) -> Result<(), i32> {
    for output in outputs {
        output.extent()?;
    }
    let owner = owner
        .map(|(p, n, a)| Extent::checked(p, n, a))
        .transpose()?;
    for &(pointer, len) in inputs {
        let len = safe_len(len).map_err(|_| AL_INVALID)?;
        len.checked_add(1)
            .filter(|n| *n <= isize::MAX.unsigned_abs())
            .ok_or(AL_INVALID)?;
        if len != 0 {
            let extent = Extent::checked(pointer, len, 1)?;
            if owner.is_some_and(|shell| extent.overlaps(shell)) { return Err(AL_INVALID); }
        }
    }
    for (index, output) in outputs.iter().enumerate() {
        let extent = output.extent()?;
        if owner.is_some_and(|other| extent.overlaps(other)) {
            return Err(AL_INVALID);
        }
        for &(pointer, len) in inputs {
            let len = safe_len(len).map_err(|_| AL_INVALID)?;
            if len != 0 && extent.overlaps(Extent::checked(pointer, len, 1)?) {
                return Err(AL_INVALID);
            }
        }
        for other in &outputs[..index] {
            if extent.overlaps(other.extent()?) {
                return Err(AL_INVALID);
            }
        }
    }
    for output in outputs {
        // SAFETY: caller grants fresh writable scratch; all extent/disjointness checks completed.
        unsafe { core::ptr::write_bytes(output.pointer, 0, output.len) };
    }
    Ok(())
}

fn owner_extent<T>(pointer: *const T) -> Option<(*const u8, usize, usize)> {
    Some((pointer.cast(), size_of::<T>(), align_of::<T>()))
}
fn status(result: Result<(), i32>) -> i32 {
    result.err().unwrap_or(0)
}
fn os_status(result: i32) -> Result<(), i32> {
    if result == 0 {
        Ok(())
    } else {
        Err(io_error_to_status(&std::io::Error::last_os_error()))
    }
}
fn checked_mode(mode: u32) -> Result<libc::mode_t, i32> {
    if mode & !0o7777 != 0 {
        return Err(AL_INVALID);
    }
    libc::mode_t::try_from(mode).map_err(|_| AL_INVALID)
}

#[allow(clippy::useless_conversion)] // Native stat widths differ across the supported Unix targets.
fn metadata(stat: &libc::stat) -> Result<Metadata, i32> {
    #[cfg(target_os = "linux")]
    let device = u64::try_from(stat.st_dev).map_err(|_| AL_INVALID)?;
    #[cfg(target_os = "macos")]
    let device = u64::from(u32::from_ne_bytes(stat.st_dev.to_ne_bytes()));
    let nanoseconds = |value: i64| {
        u32::try_from(value)
            .ok()
            .filter(|n| *n < 1_000_000_000)
            .ok_or(AL_INVALID)
    };
    let mode = u32::try_from(stat.st_mode).map_err(|_| AL_INVALID)?;
    let kind = match stat.st_mode & libc::S_IFMT {
        libc::S_IFREG => 0,
        libc::S_IFDIR => 1,
        libc::S_IFLNK => 2,
        _ => 3,
    };
    Ok(Metadata {
        device,
        inode: u64::try_from(stat.st_ino).map_err(|_| AL_INVALID)?,
        links: u64::try_from(stat.st_nlink).map_err(|_| AL_INVALID)?,
        size: i64::try_from(stat.st_size).map_err(|_| AL_INVALID)?,
        modified_seconds: i64::try_from(stat.st_mtime).map_err(|_| AL_INVALID)?,
        changed_seconds: i64::try_from(stat.st_ctime).map_err(|_| AL_INVALID)?,
        modified_nanoseconds: nanoseconds(
            i64::try_from(stat.st_mtime_nsec).map_err(|_| AL_INVALID)?,
        )?,
        changed_nanoseconds: nanoseconds(
            i64::try_from(stat.st_ctime_nsec).map_err(|_| AL_INVALID)?,
        )?,
        kind,
        mode: mode & 0o7777,
    })
}

/// Retain the current ancestor, with the supplied root borrowed until the operation finishes.
fn parent(
    directory: &Directory,
    path: &BeneathPath,
) -> Result<(Option<BeneathFd>, i32, *const libc::c_char), i32> {
    let mut retained = None;
    let mut fd = directory.fd.0;
    let Some(last) = path.components.len().checked_sub(1) else {
        return Err(AL_INVALID);
    };
    for index in 0..last {
        let next = beneath_open_directory(fd, path.component_ptr(index))?;
        fd = next.0;
        retained = Some(next);
    }
    Ok((retained, fd, path.component_ptr(last)))
}

/// # Safety
/// Path is readable for len bytes; out is fresh exclusive pointer scratch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_open(
    path: *const u8,
    len: i64,
    out: *mut *mut c_void,
) -> i32 {
    status((|| {
        unsafe {
            prepare(&[Output::typed(out)], None, Some((path, len)))?;
        }
        let path = unsafe { abi_beneath_path_impl(path, len, true, true)? };
        let mut fd = beneath_open_start(path.absolute)?;
        for index in 0..path.components.len() {
            fd = beneath_open_directory(fd.0, path.component_ptr(index))?;
        }
        unsafe {
            out.write(Box::into_raw(Box::new(Directory { fd })).cast());
        }
        Ok(())
    })())
}

/// # Safety
/// Owner is live and borrowed; out is fresh exclusive pointer scratch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_cursor(
    owner: *mut c_void,
    out: *mut *mut c_void,
) -> i32 {
    status((|| {
        let owner = owner.cast::<Directory>();
        unsafe {
            prepare(&[Output::typed(out)], owner_extent(owner), None)?;
        }
        let directory = unsafe { &*owner };
        let observed = beneath_stat_fd(directory.fd.0)?;
        #[cfg(test)]
        inject(Failure::CursorOpen)?;
        let fd = unsafe {
            libc::openat(
                directory.fd.0,
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io_error_to_status(&std::io::Error::last_os_error()));
        }
        let fd = BeneathFd(fd);
        #[cfg(test)]
        {
            PARTIAL_FD.with(|value| value.set(fd.0));
            inject(Failure::CursorStat)?;
        }
        let opened = beneath_stat_fd(fd.0)?;
        if !beneath_is_dir(&opened) || !beneath_same_identity(&observed, &opened) {
            return Err(AL_INVALID);
        }
        #[cfg(test)]
        inject(Failure::CursorStream)?;
        let stream = unsafe { libc::fdopendir(fd.0) };
        if stream.is_null() {
            return Err(io_error_to_status(&std::io::Error::last_os_error()));
        }
        let _ = fd.into_raw();
        unsafe {
            out.write(
                Box::into_raw(Box::new(Cursor {
                    stream,
                    terminal: 0,
                }))
                .cast(),
            );
        }
        Ok(())
    })())
}

/// # Safety
/// Cursor is live and exclusive; the two outputs are fresh writable disjoint scratch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_cursor_next(
    owner: *mut c_void,
    out: *mut AlignStr,
    present: *mut u8,
) -> i32 {
    status((|| {
        let owner = owner.cast::<Cursor>();
        unsafe {
            prepare(
                &[Output::typed(out), Output::typed(present)],
                owner_extent(owner),
                None,
            )?;
        }
        let cursor = unsafe { &mut *owner };
        if cursor.terminal == -1 {
            return Ok(());
        }
        if cursor.terminal != 0 {
            return Err(cursor.terminal);
        }
        loop {
            #[cfg(test)]
            if let Err(error) = inject(Failure::CursorRead) {
                cursor.terminal = error;
                return Err(error);
            }
            #[cfg(target_os = "linux")]
            unsafe {
                *libc::__errno_location() = 0;
            }
            #[cfg(target_os = "macos")]
            unsafe {
                *libc::__error() = 0;
            }
            let entry = unsafe { libc::readdir(cursor.stream) };
            if entry.is_null() {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(0) {
                    cursor.terminal = -1;
                    return Ok(());
                }
                cursor.terminal = io_error_to_status(&error);
                return Err(cursor.terminal);
            }
            // SAFETY: readdir supplies a NUL-terminated native basename valid until its next call.
            let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            let len = match i64::try_from(name.len()) {
                Ok(len) => len,
                Err(_) => {
                    cursor.terminal = AL_INVALID;
                    return Err(AL_INVALID);
                }
            };
            let allocation = align_rt_alloc(len);
            unsafe {
                core::ptr::copy_nonoverlapping(name.as_ptr(), allocation, name.len());
                out.write(AlignStr {
                    ptr: allocation,
                    len,
                });
                present.write(1);
            }
            return Ok(());
        }
    })())
}

/// # Safety
/// Owner is null (moved) or one exclusively owned live Directory, released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_free(owner: *mut c_void) {
    if !owner.is_null() {
        unsafe {
            drop(Box::from_raw(owner.cast::<Directory>()));
        }
    }
}
/// # Safety
/// Owner is null (moved) or one exclusively owned live Cursor, released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_cursor_free(owner: *mut c_void) {
    if !owner.is_null() {
        unsafe {
            drop(Box::from_raw(owner.cast::<Cursor>()));
        }
    }
}

unsafe fn descriptor_metadata<T>(owner: *mut T, out: *mut u8, fd: impl FnOnce(&T) -> i32) -> i32 {
    status((|| {
        let out = out.cast::<Metadata>();
        unsafe {
            prepare(&[Output::typed(out)], owner_extent(owner), None)?;
        }
        let value = metadata(&beneath_stat_fd(fd(unsafe { &*owner }))?)?;
        unsafe {
            out.write(value);
        }
        Ok(())
    })())
}
unsafe fn descriptor_mode<T>(owner: *mut T, mode: u32, fd: impl FnOnce(&T) -> i32) -> i32 {
    status((|| {
        unsafe {
            prepare(&[], owner_extent(owner), None)?;
        }
        let mode = checked_mode(mode)?;
        os_status(unsafe { libc::fchmod(fd(&*owner), mode) })
    })())
}

// These wrappers preserve distinct receiver ownership domains despite their identical C shapes.
macro_rules! descriptor_operations {
    ($metadata:ident, $mode:ident, $owner:ty, $fd:expr) => {
        /// # Safety
        /// Owner is live and borrowed; out is fresh, exclusive 64-byte metadata scratch.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $metadata(owner: *mut c_void, out: *mut u8) -> i32 {
            unsafe { descriptor_metadata(owner.cast::<$owner>(), out, $fd) }
        }
        /// # Safety
        /// Owner is a live borrowed descriptor owner of this exact receiver class.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $mode(owner: *mut c_void, mode: u32) -> i32 {
            unsafe { descriptor_mode(owner.cast::<$owner>(), mode, $fd) }
        }
    };
}
descriptor_operations!(
    align_rt_fs_directory_metadata,
    align_rt_fs_directory_set_mode,
    Directory,
    |owner: &Directory| owner.fd.0
);
descriptor_operations!(
    align_rt_fs_reader_metadata,
    align_rt_fs_reader_set_mode,
    Reader,
    |owner: &Reader| owner.fd
);
descriptor_operations!(
    align_rt_fs_writer_metadata,
    align_rt_fs_writer_set_mode,
    Writer,
    |owner: &Writer| owner.fd
);
descriptor_operations!(
    align_rt_fs_file_metadata,
    align_rt_fs_file_set_mode,
    RwFile,
    |owner: &RwFile| owner.fd
);

#[derive(Clone, Copy)]
enum Relative {
    Metadata,
    OpenDir,
    OpenRead,
    OpenReadSingleLink,
    CreateNew,
    CreateDir(u32),
    RemoveFile,
    RemoveDir,
}

unsafe fn relative(
    owner: *mut c_void,
    path: *const u8,
    len: i64,
    out: *mut u8,
    operation: Relative,
) -> i32 {
    status((|| {
        let owner = owner.cast::<Directory>();
        let output = match operation {
            Relative::Metadata => Some(Output::typed(out.cast::<Metadata>())),
            Relative::OpenDir
            | Relative::OpenRead
            | Relative::OpenReadSingleLink
            | Relative::CreateNew => Some(Output::typed(out.cast::<*mut c_void>())),
            Relative::CreateDir(_) | Relative::RemoveFile | Relative::RemoveDir => None,
        };
        unsafe {
            prepare(output.as_slice(), owner_extent(owner), Some((path, len)))?;
        }
        let path = unsafe { abi_beneath_path_impl(path, len, false, false)? };
        let mode = if let Relative::CreateDir(mode) = operation {
            Some(checked_mode(mode)?)
        } else {
            None
        };
        let (_retained, fd, name) = parent(unsafe { &*owner }, &path)?;
        match operation {
            Relative::Metadata => {
                let value = metadata(&beneath_stat_at(fd, name)?)?;
                unsafe {
                    out.cast::<Metadata>().write(value);
                }
            }
            Relative::OpenDir => {
                let fd = beneath_open_directory(fd, name)?;
                unsafe {
                    out.cast::<*mut c_void>()
                        .write(Box::into_raw(Box::new(Directory { fd })).cast());
                }
            }
            Relative::OpenRead | Relative::OpenReadSingleLink => {
                let fd = beneath_open_regular(
                    fd,
                    name,
                    matches!(operation, Relative::OpenReadSingleLink),
                )?;
                let reader = Box::new(Reader::unbuffered(fd.into_raw(), true));
                unsafe {
                    out.cast::<*mut c_void>()
                        .write(Box::into_raw(reader).cast());
                }
            }
            Relative::CreateNew => {
                let created = beneath_create_exclusive(fd, name)?;
                #[cfg(test)]
                {
                    PARTIAL_FD.with(|value| value.set(created.0));
                    inject(Failure::CreateStat)?;
                }
                let opened = beneath_stat_fd(created.0)?;
                if !beneath_is_regular(&opened) {
                    return Err(AL_INVALID);
                }
                #[cfg(test)]
                inject(Failure::CreateName)?;
                let named = beneath_stat_at(fd, name)?;
                if !beneath_is_regular(&named) || !beneath_same_identity(&named, &opened) {
                    return Err(AL_INVALID);
                }
                let writer = Box::new(Writer::generic_fd(created.into_raw(), true, true));
                unsafe {
                    out.cast::<*mut c_void>()
                        .write(Box::into_raw(writer).cast());
                }
            }
            Relative::CreateDir(_) => {
                let Some(mode) = mode else {
                    return Err(AL_INVALID);
                };
                os_status(unsafe { libc::mkdirat(fd, name, mode) })?;
            }
            Relative::RemoveFile => {
                os_status(unsafe { libc::unlinkat(fd, name, 0) })?;
            }
            Relative::RemoveDir => {
                os_status(unsafe { libc::unlinkat(fd, name, libc::AT_REMOVEDIR) })?;
            }
        }
        Ok(())
    })())
}

macro_rules! relative_output {
    ($name:ident, $operation:ident) => {
        /// # Safety
        /// Directory and input bytes are live borrowed inputs; out is fresh exclusive scratch
        /// of the exact result class documented in plan 45.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            owner: *mut c_void,
            path: *const u8,
            len: i64,
            out: *mut u8,
        ) -> i32 {
            unsafe { relative(owner, path, len, out, Relative::$operation) }
        }
    };
}
relative_output!(align_rt_fs_directory_metadata_at, Metadata);
relative_output!(align_rt_fs_directory_open_dir, OpenDir);
relative_output!(align_rt_fs_directory_open_read, OpenRead);
relative_output!(
    align_rt_fs_directory_open_read_single_link,
    OpenReadSingleLink
);
relative_output!(align_rt_fs_directory_create_new, CreateNew);

/// # Safety
/// Directory is live and borrowed; path is readable for len bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_create_dir(
    owner: *mut c_void,
    path: *const u8,
    len: i64,
    mode: u32,
) -> i32 {
    unsafe {
        relative(
            owner,
            path,
            len,
            core::ptr::null_mut(),
            Relative::CreateDir(mode),
        )
    }
}
/// # Safety
/// Directory is live and borrowed; path is readable for len bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_remove_file(
    owner: *mut c_void,
    path: *const u8,
    len: i64,
) -> i32 {
    unsafe {
        relative(
            owner,
            path,
            len,
            core::ptr::null_mut(),
            Relative::RemoveFile,
        )
    }
}
/// # Safety
/// Directory is live and borrowed; path is readable for len bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_remove_dir(
    owner: *mut c_void,
    path: *const u8,
    len: i64,
) -> i32 {
    unsafe { relative(owner, path, len, core::ptr::null_mut(), Relative::RemoveDir) }
}

/// Validate the extra truncation-detection byte against both native signed counts.
fn link_capacity(cap: i64) -> Result<usize, i32> {
    if !(1..i64::from(i32::MAX)).contains(&cap) { return Err(AL_INVALID); }
    usize::try_from(cap.checked_add(1).ok_or(AL_INVALID)?).map_err(|_| AL_INVALID)
}

/// # Safety
/// Directory and path are live call borrows; out is fresh exclusive byte-header scratch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_read_link(
    owner: *mut c_void, path: *const u8, len: i64, max_bytes: i64, out: *mut AlignStr,
) -> i32 {
    status((|| {
        let owner = owner.cast::<Directory>();
        unsafe { prepare(&[Output::typed(out)], owner_extent(owner), Some((path,len)))?; }
        let path = unsafe { abi_beneath_path_impl(path,len,false,false)? };
        let capacity = link_capacity(max_bytes)?;
        let (_retained,fd,name) = parent(unsafe { &*owner }, &path)?;
        let mut scratch = vec![0u8;capacity];
        let count = unsafe { libc::readlinkat(fd,name,scratch.as_mut_ptr().cast(),capacity) };
        if count < 0 { return Err(io_error_to_status(&std::io::Error::last_os_error())); }
        let count = usize::try_from(count).map_err(|_| AL_INVALID)?;
        if count >= capacity { return Err(AL_INVALID); }
        unsafe { out.write(owned_str_copy(&scratch[..count])); }
        Ok(())
    })())
}

/// # Safety
/// Directory and path are live call borrows; out is fresh exclusive metadata scratch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_metadata_follow(
    owner: *mut c_void, path: *const u8, len: i64, out: *mut u8,
) -> i32 {
    status((|| {
        let owner = owner.cast::<Directory>();
        unsafe { prepare(&[Output::typed(out.cast::<Metadata>())],owner_extent(owner),Some((path,len)))?; }
        let path = unsafe { abi_beneath_path_impl(path,len,false,false)? };
        let (_retained,fd,name) = parent(unsafe { &*owner },&path)?;
        let mut native: libc::stat = unsafe { core::mem::zeroed() };
        os_status(unsafe { libc::fstatat(fd,name,&mut native,0) })?;
        let value = metadata(&native)?;
        unsafe { out.cast::<Metadata>().write(value); }
        Ok(())
    })())
}

/// # Safety
/// Directory and both byte inputs are live for this call; no ownership is transferred.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_create_symlink(
    owner: *mut c_void, path: *const u8, len: i64, target: *const u8, target_len: i64,
) -> i32 {
    status((|| {
        let owner = owner.cast::<Directory>();
        unsafe { prepare_inputs(&[],owner_extent(owner),&[(path,len),(target,target_len)])?; }
        let path = unsafe { abi_beneath_path_impl(path,len,false,false)? };
        let length = safe_len(target_len).map_err(|_| AL_INVALID)?;
        if length == 0 { return Err(AL_INVALID); }
        let target = unsafe { core::slice::from_raw_parts(target,length) };
        let target = std::ffi::CString::new(target).map_err(|_| AL_INVALID)?;
        let (_retained,fd,name) = parent(unsafe { &*owner },&path)?;
        os_status(unsafe { libc::symlinkat(target.as_ptr(),fd,name) })
    })())
}

fn access_mode(read: u8, write: u8, execute: u8) -> Result<i32,i32> {
    if read > 1 || write > 1 || execute > 1 || (read|write|execute)==0 { return Err(AL_INVALID); }
    Ok(i32::from(read)*libc::R_OK | i32::from(write)*libc::W_OK | i32::from(execute)*libc::X_OK)
}

/// Only access-query errors can mean a completed negative decision.
fn access_result(result: i32, errno: i32) -> Result<bool,i32> {
    if result == 0 { return Ok(true); }
    match errno {
        libc::EACCES | libc::EPERM | libc::EROFS | libc::ETXTBSY => Ok(false),
        libc::ELOOP => Err(AL_INVALID),
        _ => Err(io_error_to_status(&std::io::Error::from_raw_os_error(errno))),
    }
}

fn native_access(fd: i32, name: &core::ffi::CStr, mode: i32, flags: i32) -> Result<bool,i32> {
    #[cfg(target_os="linux")]
    let result = unsafe { libc::syscall(libc::SYS_faccessat2,fd,name.as_ptr(),mode,flags) };
    #[cfg(target_os="macos")]
    let result = unsafe { libc::faccessat(fd,name.as_ptr(),mode,flags) };
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EIO);
    access_result(if result == 0 {0} else {-1},errno)
}

fn relative_access(directory: &Directory, path: &BeneathPath, mode: i32) -> Result<bool,i32> {
    let mut retained = None;
    let mut fd = directory.fd.0;
    let last = path.components.len().checked_sub(1).ok_or(AL_INVALID)?;
    for index in 0..=last {
        if !native_access(fd,c".",libc::X_OK,0)? { return Ok(false); }
        if index != last {
            let next = beneath_open_directory(fd,path.component_ptr(index))?;
            fd = next.0;
            retained = Some(next);
        }
    }
    let name = path.component_ptr(last);
    #[cfg(target_os="linux")]
    let result = {
        let raw = unsafe { libc::openat(fd,name,libc::O_PATH|libc::O_NOFOLLOW|libc::O_CLOEXEC) };
        if raw < 0 { return Err(io_error_to_status(&std::io::Error::last_os_error())); }
        let target = BeneathFd(raw);
        let observed = beneath_stat_fd(target.0)?;
        if observed.st_mode & libc::S_IFMT == libc::S_IFLNK { return Err(AL_INVALID); }
        native_access(target.0,c"",mode,libc::AT_EMPTY_PATH)
    };
    #[cfg(target_os="macos")]
    let result = {
        // Public macOS 15 XNU bsd/sys/fcntl.h; absent from some Rust libc versions.
        const AT_SYMLINK_NOFOLLOW_ANY: i32 = 0x0800;
        native_access(fd,unsafe { core::ffi::CStr::from_ptr(name) },mode,AT_SYMLINK_NOFOLLOW_ANY)
    };
    drop(retained);
    result
}

/// # Safety
/// Directory is live and borrowed; out is fresh exclusive one-byte Bool scratch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_access(
    owner: *mut c_void, read: u8, write: u8, execute: u8, out: *mut u8,
) -> i32 {
    status((|| {
        let owner = owner.cast::<Directory>();
        unsafe { prepare(&[Output::typed(out)],owner_extent(owner),None)?; }
        let mode = access_mode(read,write,execute)?;
        let answer = native_access(unsafe { (*owner).fd.0 },c".",mode,0)?;
        unsafe { out.write(u8::from(answer)); }
        Ok(())
    })())
}

/// # Safety
/// Directory and path are live call borrows; out is fresh exclusive one-byte Bool scratch.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_fs_directory_access_at(
    owner: *mut c_void, path: *const u8, len: i64, read: u8, write: u8, execute: u8, out: *mut u8,
) -> i32 {
    status((|| {
        let owner = owner.cast::<Directory>();
        unsafe { prepare(&[Output::typed(out)],owner_extent(owner),Some((path,len)))?; }
        let path = unsafe { abi_beneath_path_impl(path,len,false,false)? };
        let mode = access_mode(read,write,execute)?;
        let answer = relative_access(unsafe { &*owner },&path,mode)?;
        unsafe { out.write(u8::from(answer)); }
        Ok(())
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> std::io::Result<Self> {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("align-retained-{}-{id}", std::process::id()));
            std::fs::create_dir(&path)?;
            Ok(Self(path))
        }
        fn open(&self) -> Result<*mut c_void, Box<dyn std::error::Error>> {
            let canonical = std::fs::canonicalize(&self.0)?;
            let path = canonical.as_os_str().as_bytes();
            let mut owner = core::ptr::null_mut();
            assert_eq!(
                unsafe {
                    align_rt_fs_directory_open(
                        path.as_ptr(),
                        i64::try_from(path.len())?,
                        &mut owner,
                    )
                },
                0
            );
            assert!(!owner.is_null());
            Ok(owner)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    unsafe fn names(cursor: *mut c_void) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
        let mut names = Vec::new();
        loop {
            let mut header = AlignStr {
                ptr: core::ptr::null(),
                len: 0,
            };
            let mut present = 0;
            assert_eq!(
                unsafe { align_rt_fs_cursor_next(cursor, &mut header, &mut present) },
                0
            );
            if present == 0 {
                assert!(header.ptr.is_null());
                assert_eq!(header.len, 0);
                break;
            }
            assert_eq!(present, 1);
            names.push(
                unsafe { std::slice::from_raw_parts(header.ptr, usize::try_from(header.len)?) }
                    .to_vec(),
            );
            unsafe {
                align_rt_free(header.ptr.cast_mut());
            }
        }
        names.sort();
        Ok(names)
    }

    #[test]
    fn paths_and_admission() -> TestResult {
        let fixture = Fixture::new()?;
        let directory = fixture.open()?;
        for path in [
            b"".as_slice(),
            b"/a",
            b"a/",
            b"a//b",
            b".",
            b"..",
            b"a/../b",
            b"a\0b",
        ] {
            let mut out = 1_usize as *mut c_void;
            assert_eq!(
                unsafe {
                    align_rt_fs_directory_open_dir(
                        directory,
                        path.as_ptr(),
                        i64::try_from(path.len())?,
                        (&mut out as *mut *mut c_void).cast(),
                    )
                },
                AL_INVALID
            );
            assert!(out.is_null());
        }
        let mut output = 1_usize as *mut c_void;
        assert_eq!(
            unsafe { align_rt_fs_directory_open(1_usize as *const u8, i64::MAX, &mut output) },
            AL_INVALID
        );
        assert_eq!(output.addr(), 1); // terminator capacity rejected before dereference or scratch write
        assert_eq!(
            unsafe { align_rt_fs_directory_open(core::ptr::null(), 1, &mut output) },
            AL_INVALID
        );
        assert_eq!(output.addr(), 1);
        let owner_output = directory.cast::<*mut c_void>();
        assert_eq!(
            unsafe { align_rt_fs_directory_cursor(directory, owner_output) },
            AL_INVALID
        );
        std::fs::write(fixture.0.join("regular"), b"data")?;
        std::os::unix::fs::symlink("regular", fixture.0.join("link"))?;
        let mut reader: *mut c_void = core::ptr::null_mut();
        assert_eq!(
            unsafe {
                align_rt_fs_directory_open_read(
                    directory,
                    b"link".as_ptr(),
                    4,
                    (&mut reader as *mut *mut c_void).cast(),
                )
            },
            AL_INVALID
        );
        assert!(reader.is_null());
        assert_eq!(
            unsafe {
                align_rt_fs_directory_open_read(
                    directory,
                    b"regular".as_ptr(),
                    7,
                    (&mut reader as *mut *mut c_void).cast(),
                )
            },
            0
        );
        let mut bytes = [0_u8; 4];
        assert_eq!(
            unsafe {
                libc::read(
                    (*reader.cast::<Reader>()).fd,
                    bytes.as_mut_ptr().cast(),
                    bytes.len(),
                )
            },
            4
        );
        assert_eq!(&bytes, b"data");
        let descriptor = unsafe { (*reader.cast::<Reader>()).fd };
        let before = unsafe { libc::lseek(descriptor, 0, libc::SEEK_CUR) };
        let mut observation = Metadata::default();
        assert_eq!(
            unsafe {
                align_rt_fs_reader_metadata(reader, (&mut observation as *mut Metadata).cast())
            },
            0
        );
        assert_eq!(unsafe { align_rt_fs_reader_set_mode(reader, 0o600) }, 0);
        assert_eq!(
            unsafe { libc::lseek(descriptor, 0, libc::SEEK_CUR) },
            before
        );

        unsafe {
            align_rt_io_reader_free(reader.cast());
            align_rt_fs_directory_free(directory);
        }
        Ok(())
    }

    #[test]
    fn cursor_states() -> TestResult {
        let fixture = Fixture::new()?;
        std::fs::write(fixture.0.join("one"), b"1")?;
        std::fs::write(fixture.0.join("two"), b"2")?;
        #[cfg(target_os = "linux")]
        std::fs::write(
            fixture.0.join(std::ffi::OsStr::from_bytes(b"raw-\xff")),
            b"3",
        )?;
        let directory = fixture.open()?;
        let mut first = core::ptr::null_mut();
        let mut second = core::ptr::null_mut();
        assert_eq!(
            unsafe { align_rt_fs_directory_cursor(directory, &mut first) },
            0
        );
        assert_eq!(
            unsafe { align_rt_fs_directory_cursor(directory, &mut second) },
            0
        );
        unsafe {
            align_rt_fs_directory_free(directory);
        }
        let first_names = unsafe { names(first)? };
        let second_names = unsafe { names(second)? };
        assert_eq!(first_names, second_names);
        assert!(first_names.contains(&b"one".to_vec()));
        assert!(first_names.contains(&b"two".to_vec()));
        #[cfg(target_os = "linux")]
        assert!(first_names.contains(&b"raw-\xff".to_vec()));
        assert!(unsafe { names(first)? }.is_empty());
        unsafe {
            align_rt_fs_cursor_free(first);
            align_rt_fs_cursor_free(second);
        }
        let directory = fixture.open()?;
        let mut cursor = core::ptr::null_mut();
        assert_eq!(
            unsafe { align_rt_fs_directory_cursor(directory, &mut cursor) },
            0
        );
        FAILURE.with(|value| value.set(Some(Failure::CursorRead)));
        let mut header = AlignStr {
            ptr: core::ptr::null(),
            len: 0,
        };
        let mut present = 99;
        let error = unsafe { align_rt_fs_cursor_next(cursor, &mut header, &mut present) };
        FAILURE.with(|value| value.set(None));
        assert_eq!(error, AL_CODE + libc::EIO);
        assert_eq!(present, 0);
        assert!(header.ptr.is_null());
        assert_eq!(
            unsafe { align_rt_fs_cursor_next(cursor, &mut header, &mut present) },
            error
        );
        unsafe {
            align_rt_fs_cursor_free(cursor);
            align_rt_fs_directory_free(directory);
        }
        Ok(())
    }

    #[test]
    fn metadata_conversion_and_modes() -> TestResult {
        let fixture = Fixture::new()?;
        let directory = fixture.open()?;
        let mut observed = Metadata::default();
        assert_eq!(
            unsafe {
                align_rt_fs_directory_metadata(directory, (&mut observed as *mut Metadata).cast())
            },
            0
        );
        assert_eq!(size_of::<Metadata>(), 64);
        assert_eq!(align_of::<Metadata>(), 8);
        assert_eq!(observed.kind, 1);
        let native = beneath_stat_fd(unsafe { (*directory.cast::<Directory>()).fd.0 })
            .map_err(|code| format!("native status {code}"))?;
        assert_eq!(
            observed,
            metadata(&native).map_err(|code| format!("metadata status {code}"))?
        );
        let mut malformed = native;
        malformed.st_mtime_nsec = -1;
        assert_eq!(metadata(&malformed), Err(AL_INVALID));
        malformed.st_mtime_nsec = 1_000_000_000;
        assert_eq!(metadata(&malformed), Err(AL_INVALID));
        assert_eq!(
            unsafe { align_rt_fs_directory_set_mode(directory, 0o700) },
            0
        );
        assert_eq!(
            std::fs::metadata(&fixture.0)?.permissions().mode() & 0o7777,
            0o700
        );
        assert_eq!(
            unsafe { align_rt_fs_directory_set_mode(directory, 0o10000) },
            AL_INVALID
        );
        assert_eq!(
            unsafe { align_rt_fs_directory_create_dir(directory, b"child".as_ptr(), 5, 0o700) },
            0
        );
        assert_eq!(
            unsafe {
                align_rt_fs_directory_metadata_at(
                    directory,
                    b"child".as_ptr(),
                    5,
                    (&mut observed as *mut Metadata).cast(),
                )
            },
            0
        );
        assert_eq!(observed.kind, 1);
        std::os::unix::fs::symlink("child", fixture.0.join("link"))?;
        assert_eq!(
            unsafe {
                align_rt_fs_directory_metadata_at(
                    directory,
                    b"link".as_ptr(),
                    4,
                    (&mut observed as *mut Metadata).cast(),
                )
            },
            0
        );
        assert_eq!(observed.kind, 2);
        assert_eq!(
            unsafe { align_rt_fs_directory_remove_file(directory, b"link".as_ptr(), 4) },
            0
        );
        assert!(fixture.0.join("child").is_dir());
        assert_eq!(
            unsafe { align_rt_fs_directory_remove_dir(directory, b"child".as_ptr(), 5) },
            0
        );
        unsafe {
            align_rt_fs_directory_free(directory);
        }
        Ok(())
    }

    #[test]
    fn retained_rename_raw_paths_and_special_entries() -> TestResult {
        let fixture = Fixture::new()?;
        std::fs::create_dir(fixture.0.join("original"))?;
        let root = fixture.open()?;
        let mut child = core::ptr::null_mut::<c_void>();
        assert_eq!(
            unsafe {
                align_rt_fs_directory_open_dir(
                    root,
                    b"original".as_ptr(),
                    8,
                    (&mut child as *mut *mut c_void).cast(),
                )
            },
            0
        );
        std::fs::rename(fixture.0.join("original"), fixture.0.join("renamed"))?;
        std::fs::create_dir(fixture.0.join("original"))?;
        #[cfg(target_os = "linux")]
        let raw = b"raw-\xff".as_slice();
        #[cfg(target_os = "macos")]
        let raw = b"raw-name".as_slice();
        let name = std::ffi::OsStr::from_bytes(raw);
        let mut writer = core::ptr::null_mut::<c_void>();
        assert_eq!(
            unsafe {
                align_rt_fs_directory_create_new(
                    child,
                    raw.as_ptr(),
                    i64::try_from(raw.len())?,
                    (&mut writer as *mut *mut c_void).cast(),
                )
            },
            0
        );
        unsafe {
            align_rt_io_writer_free(writer.cast());
        }
        assert!(fixture.0.join("renamed").join(name).exists());
        assert!(!fixture.0.join("original").join(name).exists());
        std::fs::hard_link(
            fixture.0.join("renamed").join(name),
            fixture.0.join("renamed/alias"),
        )?;
        let mut reader = core::ptr::null_mut::<c_void>();
        assert_eq!(
            unsafe {
                align_rt_fs_directory_open_read_single_link(
                    child,
                    raw.as_ptr(),
                    i64::try_from(raw.len())?,
                    (&mut reader as *mut *mut c_void).cast(),
                )
            },
            AL_INVALID
        );
        assert!(reader.is_null());
        assert_eq!(
            unsafe {
                align_rt_fs_directory_open_read(
                    child,
                    raw.as_ptr(),
                    i64::try_from(raw.len())?,
                    (&mut reader as *mut *mut c_void).cast(),
                )
            },
            0
        );
        unsafe {
            align_rt_io_reader_free(reader.cast());
        }
        let fifo = std::ffi::CString::new(fixture.0.join("renamed/fifo").as_os_str().as_bytes())?;
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        reader = core::ptr::null_mut();
        assert_eq!(
            unsafe {
                align_rt_fs_directory_open_read(
                    child,
                    b"fifo".as_ptr(),
                    4,
                    (&mut reader as *mut *mut c_void).cast(),
                )
            },
            AL_INVALID
        );
        assert!(reader.is_null());
        let mut observed = Metadata::default();
        assert_eq!(
            unsafe {
                align_rt_fs_directory_metadata_at(
                    child,
                    b"fifo".as_ptr(),
                    4,
                    (&mut observed as *mut Metadata).cast(),
                )
            },
            0
        );
        assert_eq!(observed.kind, 3);
        let mut cursor = core::ptr::null_mut();
        assert_eq!(
            unsafe { align_rt_fs_directory_cursor(child, &mut cursor) },
            0
        );
        unsafe {
            align_rt_fs_directory_free(child);
            align_rt_fs_directory_free(root);
        }
        assert_eq!(
            unsafe { names(cursor)? },
            vec![b"alias".to_vec(), b"fifo".to_vec(), raw.to_vec()]
        );
        unsafe {
            align_rt_fs_cursor_free(cursor);
        }
        Ok(())
    }

    #[test]
    fn output_admission_preserves_scratch_and_cursor() -> TestResult {
        let fixture = Fixture::new()?;
        std::fs::write(fixture.0.join("only"), b"value")?;
        let directory = fixture.open()?;
        let mut cursor = core::ptr::null_mut();
        assert_eq!(
            unsafe { align_rt_fs_directory_cursor(directory, &mut cursor) },
            0
        );
        let mut header = AlignStr {
            ptr: 1_usize as *const u8,
            len: 77,
        };
        let header_ptr = &mut header as *mut AlignStr;
        assert_eq!(
            unsafe { align_rt_fs_cursor_next(cursor, header_ptr, header_ptr.cast()) },
            AL_INVALID
        );
        assert_eq!(header.ptr.addr(), 1);
        assert_eq!(header.len, 77);
        let mut present = 99;
        assert_eq!(
            unsafe { align_rt_fs_cursor_next(cursor, cursor.cast(), &mut present) },
            AL_INVALID
        );
        assert_eq!(present, 99);
        assert_eq!(
            unsafe { align_rt_fs_cursor_next(cursor, header_ptr, &mut present) },
            0
        );
        assert_eq!(present, 1);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(header.ptr, usize::try_from(header.len)?) },
            b"only"
        );
        unsafe {
            align_rt_free(header.ptr.cast_mut());
        }
        for size in [8_usize, 16, 64] {
            let mut storage = [0xABAB_ABAB_ABAB_ABAB_u64; 16];
            let pointer = storage.as_mut_ptr().cast::<u8>();
            let output = Output {
                pointer,
                len: size,
                alignment: 8,
            };
            assert_eq!(
                unsafe { prepare(&[output], None, Some((pointer, 4))) },
                Err(AL_INVALID)
            );
            assert!(storage.iter().all(|word| *word == 0xABAB_ABAB_ABAB_ABAB));
            assert_eq!(
                unsafe { prepare(&[output], None, Some((1_usize as *const u8, i64::MAX))) },
                Err(AL_INVALID)
            );
            assert!(storage.iter().all(|word| *word == 0xABAB_ABAB_ABAB_ABAB));
        }
        unsafe {
            align_rt_fs_cursor_free(cursor);
            align_rt_fs_directory_free(directory);
        }
        Ok(())
    }

    #[test]
    fn mode_validation_and_umask() -> TestResult {
        if std::env::var_os("ALIGN_RETAINED_UMASK_CHILD").is_none() {
            let result = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "fs_retained_tree::tests::mode_validation_and_umask",
                    "--nocapture",
                ])
                .env("ALIGN_RETAINED_UMASK_CHILD", "1")
                .output()?;
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            return Ok(());
        }
        let fixture = Fixture::new()?;
        let directory = fixture.open()?;
        let prior = unsafe { libc::umask(0o077) };
        assert_eq!(
            unsafe { align_rt_fs_directory_create_dir(directory, b"masked".as_ptr(), 6, 0o777) },
            0
        );
        assert_eq!(
            std::fs::metadata(fixture.0.join("masked"))?
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            unsafe {
                align_rt_fs_directory_create_dir(directory, b"missing/child".as_ptr(), 13, 0o10000)
            },
            AL_INVALID
        );
        assert!(!fixture.0.join("missing").exists());
        assert_eq!(unsafe { libc::umask(prior) }, 0o077);
        unsafe {
            align_rt_fs_directory_free(directory);
        }
        Ok(())
    }

    #[test]
    fn constructor_failure_cleanup() -> TestResult {
        // FD-number reuse is process-global. Isolate the actual close oracle from other tests.
        if std::env::var_os("ALIGN_RETAINED_CONSTRUCTOR_CHILD").is_none() {
            let result = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "fs_retained_tree::tests::constructor_failure_cleanup",
                    "--nocapture",
                ])
                .env("ALIGN_RETAINED_CONSTRUCTOR_CHILD", "1")
                .output()?;
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            return Ok(());
        }
        let fixture = Fixture::new()?;
        let directory = fixture.open()?;
        for failure in [
            Failure::CursorOpen,
            Failure::CursorStat,
            Failure::CursorStream,
            Failure::CreateStat,
            Failure::CreateName,
        ] {
            FAILURE.with(|value| value.set(Some(failure)));
            PARTIAL_FD.with(|value| value.set(-1));
            let mut out: *mut c_void = core::ptr::null_mut();
            let result = if matches!(failure, Failure::CreateStat | Failure::CreateName) {
                unsafe {
                    align_rt_fs_directory_create_new(
                        directory,
                        b"created".as_ptr(),
                        7,
                        (&mut out as *mut *mut c_void).cast(),
                    )
                }
            } else {
                unsafe { align_rt_fs_directory_cursor(directory, &mut out) }
            };
            FAILURE.with(|value| value.set(None));
            assert_eq!(result, AL_CODE + libc::EIO);
            assert!(out.is_null());
            let fd = PARTIAL_FD.with(|value| value.get());
            if fd >= 0 {
                assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
            }
            if matches!(failure, Failure::CreateStat | Failure::CreateName) {
                assert!(fixture.0.join("created").is_file()); // no rollback unlink after a failed publication
                std::fs::remove_file(fixture.0.join("created"))?;
            }
        }
        unsafe {
            align_rt_fs_directory_free(directory);
        }
        Ok(())
    }
    struct OwnedDirectory(*mut c_void);
    impl Drop for OwnedDirectory {
        fn drop(&mut self) { unsafe { align_rt_fs_directory_free(self.0) }; }
    }
    struct OwnedBytes(AlignStr);
    impl Drop for OwnedBytes {
        fn drop(&mut self) { unsafe { align_rt_free(self.0.ptr.cast_mut()) }; }
    }

    #[test]
    fn link_observations_and_exclusive_creation() -> TestResult {
        let fixture = Fixture::new()?;
        let owner = OwnedDirectory(fixture.open()?);
        std::fs::write(fixture.0.join("file"), b"payload")?;
        let raw_target = b"../missing/\xff";
        assert_eq!(unsafe { align_rt_fs_directory_create_symlink(owner.0,b"link".as_ptr(),4,raw_target.as_ptr(),i64::try_from(raw_target.len())?) },0);
        let mut bytes = OwnedBytes(AlignStr { ptr: core::ptr::null(),len:0 });
        assert_eq!(unsafe { align_rt_fs_directory_read_link(owner.0,b"link".as_ptr(),4,i64::try_from(raw_target.len())?,&mut bytes.0) },0);
        assert_eq!(unsafe { core::slice::from_raw_parts(bytes.0.ptr,usize::try_from(bytes.0.len)?) },raw_target);
        for name in [b"link".as_slice(),b"file"] {
            assert_ne!(unsafe { align_rt_fs_directory_create_symlink(owner.0,name.as_ptr(),i64::try_from(name.len())?,b"other".as_ptr(),5) },0);
        }
        assert_eq!(std::fs::read(fixture.0.join("file"))?,b"payload");
        for cap in [-1,0,1,i64::from(i32::MAX),i64::MAX] {
            let mut out = AlignStr { ptr: core::ptr::dangling(),len:99 };
            assert_eq!(unsafe { align_rt_fs_directory_read_link(owner.0,b"link".as_ptr(),4,cap,&mut out) },AL_INVALID);
            assert!(out.ptr.is_null()); assert_eq!(out.len,0);
        }
        assert_eq!(link_capacity(i64::from(i32::MAX)-1),Ok(usize::try_from(i32::MAX)?));
        // A missing path with the largest valid cap fails admission before allocating scratch.
        let mut empty = AlignStr { ptr: core::ptr::null(),len:0 };
        assert_ne!(unsafe { align_rt_fs_directory_read_link(owner.0,b"missing/x".as_ptr(),9,i64::from(i32::MAX)-1,&mut empty) },0);
        std::os::unix::fs::symlink("file",fixture.0.join("follow"))?;
        let mut meta = core::mem::MaybeUninit::<Metadata>::uninit();
        assert_eq!(unsafe { align_rt_fs_directory_metadata_follow(owner.0,b"follow".as_ptr(),6,meta.as_mut_ptr().cast()) },0);
        assert_eq!(unsafe { meta.assume_init() }.size,7);
        let fifo = std::ffi::CString::new(fixture.0.join("fifo").as_os_str().as_bytes())?;
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(),0o600) },0);
        std::os::unix::fs::symlink("fifo",fixture.0.join("pipe-link"))?;
        assert_eq!(unsafe { align_rt_fs_directory_metadata_follow(owner.0,b"pipe-link".as_ptr(),9,meta.as_mut_ptr().cast()) },0);
        Ok(())
    }

    #[test]
    fn access_queries_and_validation() -> TestResult {
        let fixture = Fixture::new()?;
        let owner = OwnedDirectory(fixture.open()?);
        std::fs::write(fixture.0.join("file"),b"data")?;
        std::fs::set_permissions(fixture.0.join("file"),std::fs::Permissions::from_mode(0o400))?;
        std::os::unix::fs::symlink("file",fixture.0.join("link"))?;
        let native_path = std::ffi::CString::new(fixture.0.join("file").as_os_str().as_bytes())?;
        for bits in 1u8..8 {
            let (r,w,x) = (bits&1,(bits>>1)&1,(bits>>2)&1);
            let mode=access_mode(r,w,x).map_err(|e| format!("mode: {e}"))?;
            let oracle=unsafe { libc::access(native_path.as_ptr(),mode) } == 0;
            let mut out=77;
            assert_eq!(unsafe { align_rt_fs_directory_access_at(owner.0,b"file".as_ptr(),4,r,w,x,&mut out) },0);
            assert_eq!(out,u8::from(oracle));
        }
        for (path,r,w,x) in [(b"link".as_slice(),1,0,0),(b"file",0,0,0),(b"file",2,0,0),(b"../file",1,0,0)] {
            let mut out=77;
            assert_eq!(unsafe { align_rt_fs_directory_access_at(owner.0,path.as_ptr(),i64::try_from(path.len())?,r,w,x,&mut out) },AL_INVALID);
            assert_eq!(out,0);
        }
        for errno in [libc::EACCES,libc::EPERM,libc::EROFS,libc::ETXTBSY] { assert_eq!(access_result(-1,errno),Ok(false)); }
        assert_eq!(access_result(-1,libc::ELOOP),Err(AL_INVALID));
        assert!(access_result(-1,libc::ENOENT).is_err());
        assert!(access_result(-1,libc::ENOSYS).is_err());
        assert!(access_result(-1,libc::EINVAL).is_err());
        let mut out=77;
        assert_eq!(unsafe { align_rt_fs_directory_access_at(owner.0,core::ptr::null(),-1,1,0,0,&mut out) },AL_INVALID);
        assert_eq!(out,77);
        assert_eq!(unsafe { align_rt_fs_directory_access_at(owner.0,owner.0.cast(),1,1,0,0,&mut out) },AL_INVALID);
        assert_eq!(out,77);
        // Both numerical inputs precede the first byte read.
        assert_eq!(unsafe { align_rt_fs_directory_create_symlink(owner.0,core::ptr::dangling(),1,core::ptr::null(),-1) },AL_INVALID);
        Ok(())
    }

    #[test]
    fn observations_retain_inode_and_reject_ancestor_links() -> TestResult {
        #[cfg(target_os="linux")]
        use std::os::fd::AsRawFd;
        let fixture=Fixture::new()?;
        std::fs::create_dir(fixture.0.join("root"))?;
        let root=std::ffi::CString::new(fixture.0.join("root").as_os_str().as_bytes())?;
        let mut raw=core::ptr::null_mut();
        assert_eq!(unsafe { align_rt_fs_directory_open(root.as_ptr().cast(),i64::try_from(root.as_bytes().len())?,&mut raw) },0);
        let owner=OwnedDirectory(raw);
        std::fs::write(fixture.0.join("root/file"),b"retained")?;
        std::fs::rename(fixture.0.join("root"),fixture.0.join("moved"))?;
        std::fs::create_dir(fixture.0.join("root"))?;
        std::fs::write(fixture.0.join("root/file"),b"replacement")?;
        let mut metadata=core::mem::MaybeUninit::<Metadata>::uninit();
        assert_eq!(unsafe { align_rt_fs_directory_metadata_follow(owner.0,b"file".as_ptr(),4,metadata.as_mut_ptr().cast()) },0);
        assert_eq!(unsafe { metadata.assume_init() }.size,8);
        let mut allowed=0;
        assert_eq!(unsafe { align_rt_fs_directory_access_at(owner.0,b"file".as_ptr(),4,1,0,0,&mut allowed) },0);
        assert_eq!(allowed,1);
        std::os::unix::fs::symlink("../root",fixture.0.join("moved/ancestor"))?;
        assert_ne!(unsafe { align_rt_fs_directory_metadata_follow(owner.0,b"ancestor/file".as_ptr(),13,metadata.as_mut_ptr().cast()) },0);
        assert_ne!(unsafe { align_rt_fs_directory_access_at(owner.0,b"ancestor/file".as_ptr(),13,1,0,0,&mut allowed) },0);
        #[cfg(target_os="linux")]
        {
            let opened=std::fs::File::open(fixture.0.join("moved/file"))?;
            std::fs::remove_file(fixture.0.join("moved/file"))?;
            let proc_path=format!("/proc/self/fd/{}",opened.as_raw_fd());
            std::os::unix::fs::symlink(proc_path,fixture.0.join("moved/deleted"))?;
            assert_eq!(unsafe { align_rt_fs_directory_metadata_follow(owner.0,b"deleted".as_ptr(),7,metadata.as_mut_ptr().cast()) },0);
            let observed=unsafe { metadata.assume_init() };
            let native=beneath_stat_fd(opened.as_raw_fd()).map_err(|e|format!("stat: {e}"))?;
            assert_eq!(observed,super::metadata(&native).map_err(|e|format!("metadata: {e}"))?);
        }
        Ok(())
    }

}
