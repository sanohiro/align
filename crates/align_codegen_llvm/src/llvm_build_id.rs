//! Nominal identity of the dynamically loaded LLVM library.
//!
//! Codegen cache entries must distinguish two LLVM builds that report the same
//! semantic version. The identity is read from the loader-owned mapped image
//! containing `LLVMGetVersion`, never by reopening its pathname: replacing a
//! package on disk cannot relabel code emitted by an already-running process.

use std::sync::OnceLock;

use align_interface::Hash128;

const PT_LOAD: u32 = 1;
const PT_NOTE: u32 = 4;
const NT_GNU_BUILD_ID: u32 = 3;
#[cfg(any(target_os = "macos", test))]
const LC_UUID: u32 = 0x1b;
const MAX_ELF_PROGRAM_HEADERS: usize = 1_024;
const MAX_ELF_NOTE_BYTES: usize = 1024 * 1024;
#[cfg(any(target_os = "macos", test))]
const MAX_DYLD_IMAGES: u32 = 4_096;
#[cfg(any(target_os = "macos", test))]
const MAX_MACH_LOAD_COMMANDS: usize = 4_096;
#[cfg(any(target_os = "macos", test))]
const MAX_MACH_LOAD_COMMAND_BYTES: usize = 16 * 1024 * 1024;

/// Return the nominal build identity of the loaded library that provides LLVM.
///
/// The result is memoized once per process. `None` means codegen cache reuse
/// must be disabled; uncached code generation remains available.
pub fn loaded_llvm_build_id() -> Option<Hash128> {
    static ID: OnceLock<Option<Hash128>> = OnceLock::new();
    *ID.get_or_init(resolve_loaded_llvm_build_id)
}

fn resolve_loaded_llvm_build_id() -> Option<Hash128> {
    let base = loaded_llvm_base()?;
    #[cfg(target_os = "linux")]
    return mapped_elf_build_id(base);
    #[cfg(target_os = "macos")]
    return mapped_macho_build_id(base);
    #[allow(unreachable_code)]
    None
}

fn loaded_llvm_base() -> Option<usize> {
    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::zeroed();
    // SAFETY: `LLVMGetVersion` is a linked function symbol, `dladdr` only
    // inspects the address, and `info` points to writable initialized storage.
    let found = unsafe {
        libc::dladdr(
            llvm_sys::core::LLVMGetVersion as *const () as *const libc::c_void,
            info.as_mut_ptr(),
        )
    };
    if found == 0 {
        return None;
    }
    // SAFETY: successful `dladdr` initializes `Dl_info`. `dli_fbase` is the
    // loader-owned mapped image base; no borrowed pointer escapes this call.
    let info = unsafe { info.assume_init() };
    (!info.dli_fbase.is_null()).then_some(info.dli_fbase as usize)
}

fn tagged_identity(tag: u8, raw: &[u8]) -> Hash128 {
    let mut tagged = Vec::with_capacity(raw.len().saturating_add(1));
    tagged.push(tag);
    tagged.extend_from_slice(raw);
    Hash128::of(&tagged)
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

fn read_u32(bytes: &[u8], offset: usize, endian: Endian) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(match endian {
        Endian::Little => u32::from_le_bytes(raw),
        Endian::Big => u32::from_be_bytes(raw),
    })
}

fn range(bytes: &[u8], offset: usize, len: usize) -> Option<&[u8]> {
    bytes.get(offset..offset.checked_add(len)?)
}

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|n| n & !3)
}

fn parse_elf_notes(notes: &[u8], endian: Endian) -> Result<Option<&[u8]>, ()> {
    let mut cursor = 0usize;
    let mut found = None;
    while cursor < notes.len() {
        let namesz = usize::try_from(read_u32(notes, cursor, endian).ok_or(())?).map_err(|_| ())?;
        let descsz = usize::try_from(read_u32(notes, cursor.checked_add(4).ok_or(())?, endian).ok_or(())?)
            .map_err(|_| ())?;
        let kind = read_u32(notes, cursor.checked_add(8).ok_or(())?, endian).ok_or(())?;
        cursor = cursor.checked_add(12).ok_or(())?;
        let padded_namesz = align4(namesz).ok_or(())?;
        let name_region = range(notes, cursor, padded_namesz).ok_or(())?;
        let name = name_region.get(..namesz).ok_or(())?;
        cursor = cursor.checked_add(padded_namesz).ok_or(())?;
        let padded_descsz = align4(descsz).ok_or(())?;
        let desc_region = range(notes, cursor, padded_descsz).ok_or(())?;
        let desc = desc_region.get(..descsz).ok_or(())?;
        cursor = cursor.checked_add(padded_descsz).ok_or(())?;
        if kind == NT_GNU_BUILD_ID && name == b"GNU\0" {
            if desc.is_empty() || found.is_some() {
                return Err(());
            }
            found = Some(desc);
        }
    }
    Ok(found)
}

fn elf_inventory_is_bounded(program_headers: usize, note_bytes: usize) -> bool {
    program_headers <= MAX_ELF_PROGRAM_HEADERS && note_bytes <= MAX_ELF_NOTE_BYTES
}

#[cfg(any(target_os = "macos", test))]
fn dyld_image_count_is_bounded(images: u32) -> bool {
    images <= MAX_DYLD_IMAGES
}

#[cfg(any(target_os = "macos", test))]
fn mach_commands_are_bounded(commands: usize, bytes: usize) -> bool {
    commands <= MAX_MACH_LOAD_COMMANDS
        && bytes <= MAX_MACH_LOAD_COMMAND_BYTES
        && commands.checked_mul(8).is_some_and(|minimum| minimum <= bytes)
}

#[cfg(target_os = "linux")]
#[cfg(target_pointer_width = "64")]
type NativePhdr = libc::Elf64_Phdr;
#[cfg(target_os = "linux")]
#[cfg(target_pointer_width = "32")]
type NativePhdr = libc::Elf32_Phdr;

#[cfg(target_os = "linux")]
struct ElfSearch {
    base: usize,
    matched: bool,
    invalid: bool,
    raw_id: Option<Vec<u8>>,
}

#[cfg(target_os = "linux")]
fn phdr_range(phdr: &NativePhdr) -> Option<(usize, usize, usize)> {
    Some((
        usize::try_from(phdr.p_vaddr).ok()?,
        usize::try_from(phdr.p_filesz).ok()?,
        usize::try_from(phdr.p_memsz).ok()?,
    ))
}

#[cfg(target_os = "linux")]
fn note_is_inside_readable_load(phdrs: &[NativePhdr], start: usize, len: usize) -> bool {
    let Some(end) = start.checked_add(len) else { return false };
    phdrs.iter().any(|load| {
        if load.p_type != PT_LOAD || load.p_flags & libc::PF_R == 0 {
            return false;
        }
        let Some((load_start, load_len, _)) = phdr_range(load) else { return false };
        load_start <= start
            && load_start.checked_add(load_len).is_some_and(|load_end| end <= load_end)
    })
}

#[cfg(target_os = "linux")]
unsafe extern "C" fn find_elf_image(
    info: *mut libc::dl_phdr_info,
    size: libc::size_t,
    data: *mut libc::c_void,
) -> libc::c_int {
    if info.is_null() || data.is_null() {
        return 0;
    }
    let required = std::mem::offset_of!(libc::dl_phdr_info, dlpi_phnum)
        .saturating_add(std::mem::size_of::<u16>());
    if size < required {
        return 0;
    }
    // SAFETY: `dl_iterate_phdr` supplies both pointers for this callback invocation.
    let info = unsafe { &*info };
    let search = unsafe { &mut *(data as *mut ElfSearch) };
    let Ok(image_base) = usize::try_from(info.dlpi_addr) else { return 0 };
    if image_base != search.base {
        return 0;
    }
    search.matched = true;
    let phnum = usize::from(info.dlpi_phnum);
    if !elf_inventory_is_bounded(phnum, 0) || phnum == 0 || info.dlpi_phdr.is_null() {
        search.invalid = true;
        return 1;
    }
    // SAFETY: the loader owns `dlpi_phdr` and reports exactly `dlpi_phnum`
    // initialized entries for the duration of this callback.
    let phdrs = unsafe { std::slice::from_raw_parts(info.dlpi_phdr, phnum) };

    // First validate every range and the aggregate byte bound. No note bytes
    // are sliced or copied until the complete mapped-image inventory passes.
    let mut total = 0usize;
    for phdr in phdrs.iter().filter(|phdr| phdr.p_type == PT_NOTE) {
        let Some((start, file_len, mem_len)) = phdr_range(phdr) else {
            search.invalid = true;
            return 1;
        };
        if file_len > mem_len || !note_is_inside_readable_load(phdrs, start, file_len) {
            search.invalid = true;
            return 1;
        }
        let Some(next) = total.checked_add(file_len) else {
            search.invalid = true;
            return 1;
        };
        total = next;
    }
    if !elf_inventory_is_bounded(phnum, total) {
        search.invalid = true;
        return 1;
    }

    let endian = if cfg!(target_endian = "little") { Endian::Little } else { Endian::Big };
    for phdr in phdrs.iter().filter(|phdr| phdr.p_type == PT_NOTE) {
        let Some((start, len, _)) = phdr_range(phdr) else {
            search.invalid = true;
            return 1;
        };
        if len == 0 {
            continue;
        }
        let Some(address) = search.base.checked_add(start) else {
            search.invalid = true;
            return 1;
        };
        if address == 0 {
            search.invalid = true;
            return 1;
        }
        // SAFETY: the first pass proved this file-backed note range lies
        // completely inside a readable PT_LOAD mapping owned by this image.
        let notes = unsafe { std::slice::from_raw_parts(address as *const u8, len) };
        match parse_elf_notes(notes, endian) {
            Ok(Some(id)) if search.raw_id.is_none() => search.raw_id = Some(id.to_vec()),
            Ok(Some(_)) | Err(()) => {
                search.invalid = true;
                return 1;
            }
            Ok(None) => {}
        }
    }
    1
}

#[cfg(target_os = "linux")]
fn mapped_elf_build_id(base: usize) -> Option<Hash128> {
    let mut search = ElfSearch { base, matched: false, invalid: false, raw_id: None };
    // SAFETY: `find_elf_image` obeys the callback ABI, retains no loader
    // pointers, and `search` remains live for the synchronous traversal.
    unsafe {
        libc::dl_iterate_phdr(
            Some(find_elf_image),
            (&mut search as *mut ElfSearch).cast::<libc::c_void>(),
        );
    }
    if !search.matched || search.invalid {
        return None;
    }
    search.raw_id.as_deref().map(|id| tagged_identity(0, id))
}

#[cfg(any(target_os = "macos", test))]
fn parse_macho_commands(commands: &[u8], ncmds: usize, endian: Endian) -> Result<Option<&[u8]>, ()> {
    let mut cursor = 0usize;
    let mut found = None;
    for _ in 0..ncmds {
        let cmd = read_u32(commands, cursor, endian).ok_or(())?;
        let cmdsize = usize::try_from(
            read_u32(commands, cursor.checked_add(4).ok_or(())?, endian).ok_or(())?,
        )
        .map_err(|_| ())?;
        if cmdsize < 8 || cmdsize % 4 != 0 {
            return Err(());
        }
        let command = range(commands, cursor, cmdsize).ok_or(())?;
        if cmd == LC_UUID {
            if cmdsize != 24 || found.is_some() {
                return Err(());
            }
            found = Some(command.get(8..24).ok_or(())?);
        }
        cursor = cursor.checked_add(cmdsize).ok_or(())?;
    }
    if cursor != commands.len() {
        return Err(());
    }
    Ok(found)
}

#[cfg(any(target_os = "macos", test))]
#[repr(C)]
struct MachHeader {
    magic: u32,
    cpu_type: i32,
    cpu_subtype: i32,
    file_type: u32,
    ncmds: u32,
    sizeofcmds: u32,
    flags: u32,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn _dyld_image_count() -> u32;
    fn _dyld_get_image_header(index: u32) -> *const MachHeader;
}

#[cfg(any(target_os = "macos", test))]
fn mapped_macho_build_id_from(
    base: usize,
    image_count: u32,
    mut image_header: impl FnMut(u32) -> *const MachHeader,
) -> Option<Hash128> {
    if !dyld_image_count_is_bounded(image_count) {
        return None;
    }
    for index in 0..image_count {
        let header = image_header(index);
        if header.is_null() || header as usize != base {
            continue;
        }
        // SAFETY: the caller supplies loader-owned mapped headers for every
        // index below `image_count` and retains them through this call.
        let header = unsafe { &*header };
        const MH_MAGIC: u32 = 0xfeed_face;
        const MH_MAGIC_64: u32 = 0xfeed_facf;
        let header_len = match header.magic {
            MH_MAGIC => 28usize,
            MH_MAGIC_64 => 32usize,
            _ => return None,
        };
        let ncmds = usize::try_from(header.ncmds).ok()?;
        let command_bytes = usize::try_from(header.sizeofcmds).ok()?;
        if !mach_commands_are_bounded(ncmds, command_bytes) {
            return None;
        }
        let address = base.checked_add(header_len)?;
        // SAFETY: dyld accepts and maps the complete load-command region after
        // the returned header. Both counts have passed the explicit bounds,
        // and the non-null image base makes the zero-length case valid too.
        let commands = unsafe { std::slice::from_raw_parts(address as *const u8, command_bytes) };
        let id = parse_macho_commands(commands, ncmds, Endian::Little).ok()??;
        return Some(tagged_identity(1, id));
    }
    None
}

#[cfg(target_os = "macos")]
fn mapped_macho_build_id(base: usize) -> Option<Hash128> {
    // SAFETY: dyld's image table is process-owned and these accessors do not
    // mutate it. `mapped_macho_build_id_from` copies the UUID before returning.
    let image_count = unsafe { _dyld_image_count() };
    mapped_macho_build_id_from(base, image_count, |index| {
        // SAFETY: every requested index is below the snapshot count.
        unsafe { _dyld_get_image_header(index) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32, endian: Endian) {
        let raw = match endian {
            Endian::Little => value.to_le_bytes(),
            Endian::Big => value.to_be_bytes(),
        };
        bytes[offset..offset + 4].copy_from_slice(&raw);
    }

    fn elf_notes(endian: Endian, id: &[u8]) -> Vec<u8> {
        let Some(aligned_id_len) = align4(id.len()) else { return Vec::new() };
        let Some(len) = 16usize.checked_add(aligned_id_len) else { return Vec::new() };
        let Ok(id_len) = u32::try_from(id.len()) else { return Vec::new() };
        let mut bytes = vec![0; len];
        put_u32(&mut bytes, 0, 4, endian);
        put_u32(&mut bytes, 4, id_len, endian);
        put_u32(&mut bytes, 8, NT_GNU_BUILD_ID, endian);
        bytes[12..16].copy_from_slice(b"GNU\0");
        bytes[16..16 + id.len()].copy_from_slice(id);
        bytes
    }

    fn macho_commands(endian: Endian, id: [u8; 16]) -> Vec<u8> {
        let mut bytes = vec![0; 24];
        put_u32(&mut bytes, 0, LC_UUID, endian);
        put_u32(&mut bytes, 4, 24, endian);
        bytes[8..24].copy_from_slice(&id);
        bytes
    }

    #[test]
    fn parses_elf_notes_in_both_endiannesses() {
        for endian in [Endian::Little, Endian::Big] {
            let bytes = elf_notes(endian, b"build-id");
            assert_eq!(parse_elf_notes(&bytes, endian), Ok(Some(&b"build-id"[..])));
        }
    }

    #[test]
    fn parses_macho_commands_in_both_endiannesses() {
        let id = *b"0123456789abcdef";
        for endian in [Endian::Little, Endian::Big] {
            let bytes = macho_commands(endian, id);
            assert_eq!(parse_macho_commands(&bytes, 1, endian), Ok(Some(&id[..])));
        }
    }

    #[test]
    fn rejects_every_truncation_and_duplicate_or_missing_identity() {
        for endian in [Endian::Little, Endian::Big] {
            let elf = elf_notes(endian, b"id");
            assert_eq!(parse_elf_notes(&[], endian), Ok(None));
            for len in 1..elf.len() {
                assert_eq!(parse_elf_notes(&elf[..len], endian), Err(()), "ELF truncation {len}");
            }
            let mut duplicate = elf.clone();
            duplicate.extend_from_slice(&elf);
            assert_eq!(parse_elf_notes(&duplicate, endian), Err(()));
            let mut missing = elf;
            put_u32(&mut missing, 8, 0, endian);
            assert_eq!(parse_elf_notes(&missing, endian), Ok(None));

            let macho = macho_commands(endian, *b"0123456789abcdef");
            for len in 0..macho.len() {
                assert_eq!(
                    parse_macho_commands(&macho[..len], 1, endian),
                    Err(()),
                    "Mach-O truncation {len}"
                );
            }
            let mut duplicate = macho.clone();
            duplicate.extend_from_slice(&macho);
            assert_eq!(parse_macho_commands(&duplicate, 2, endian), Err(()));
            assert_eq!(parse_macho_commands(&[], 0, endian), Ok(None));
        }
    }

    #[test]
    fn resource_bounds_pin_accepted_limit_and_rejected_next() {
        assert!(elf_inventory_is_bounded(MAX_ELF_PROGRAM_HEADERS, MAX_ELF_NOTE_BYTES));
        assert!(!elf_inventory_is_bounded(MAX_ELF_PROGRAM_HEADERS + 1, 0));
        assert!(!elf_inventory_is_bounded(0, MAX_ELF_NOTE_BYTES + 1));
        assert!(dyld_image_count_is_bounded(MAX_DYLD_IMAGES));
        assert!(!dyld_image_count_is_bounded(MAX_DYLD_IMAGES + 1));
        assert!(mach_commands_are_bounded(
            MAX_MACH_LOAD_COMMANDS,
            MAX_MACH_LOAD_COMMAND_BYTES
        ));
        assert!(!mach_commands_are_bounded(1, 7));
        assert!(!mach_commands_are_bounded(MAX_MACH_LOAD_COMMANDS + 1, 0));
        assert!(!mach_commands_are_bounded(0, MAX_MACH_LOAD_COMMAND_BYTES + 1));
    }

    #[test]
    fn macho_identity_is_bound_to_the_selected_mapped_header() {
        #[repr(C)]
        struct Fixture {
            header: MachHeader,
            reserved: u32,
            command: u32,
            command_size: u32,
            uuid: [u8; 16],
        }
        let fixture = Fixture {
            header: MachHeader {
                magic: 0xfeed_facf,
                cpu_type: 0,
                cpu_subtype: 0,
                file_type: 0,
                ncmds: 1,
                sizeofcmds: 24,
                flags: 0,
            },
            reserved: 0,
            command: LC_UUID,
            command_size: 24,
            uuid: *b"0123456789abcdef",
        };
        let base = &fixture.header as *const MachHeader as usize;
        assert_eq!(
            mapped_macho_build_id_from(base, 1, |_| &fixture.header),
            Some(tagged_identity(1, &fixture.uuid))
        );
        assert_eq!(mapped_macho_build_id_from(base + 4, 1, |_| &fixture.header), None);
    }

    #[test]
    fn tag_distinguishes_equal_raw_identity_bytes() {
        let raw = b"0123456789abcdef";
        assert_ne!(tagged_identity(0, raw), tagged_identity(1, raw));
    }

    #[test]
    fn current_process_resolves_the_mapped_llvm_image() {
        assert!(loaded_llvm_base().is_some());
        assert!(loaded_llvm_build_id().is_some());
    }
}
