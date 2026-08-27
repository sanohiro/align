//! Nominal identity of the dynamically loaded LLVM library.
//!
//! Codegen cache entries must distinguish two LLVM builds that report the same
//! semantic version. The loader gives us the object containing `LLVMGetVersion`;
//! this module reads that exact file and extracts its producer-owned ELF GNU
//! build-id or Mach-O UUID. Every malformed or unavailable input fails closed.

use std::ffi::CStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;

use align_interface::Hash128;

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const PT_NOTE: u32 = 4;
const NT_GNU_BUILD_ID: u32 = 3;
const LC_UUID: u32 = 0x1b;

/// Return the nominal build identity of the loaded library that provides LLVM.
///
/// The result is memoized once per process. `None` means codegen cache reuse
/// must be disabled; uncached code generation remains available.
pub fn loaded_llvm_build_id() -> Option<Hash128> {
    static ID: OnceLock<Option<Hash128>> = OnceLock::new();
    *ID.get_or_init(resolve_loaded_llvm_build_id)
}

fn resolve_loaded_llvm_build_id() -> Option<Hash128> {
    let path = loaded_llvm_path()?;
    let bytes = std::fs::read(path).ok()?;
    parse_object_build_id(&bytes)
}

fn loaded_llvm_path() -> Option<std::path::PathBuf> {
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
    // SAFETY: successful `dladdr` initializes `Dl_info`. A non-null `dli_fname`
    // is a borrowed NUL-terminated loader path for the duration of the process.
    let info = unsafe { info.assume_init() };
    if info.dli_fname.is_null() {
        return None;
    }
    // SAFETY: `dli_fname` has the `dladdr(3)` NUL-terminated-string contract.
    let bytes = unsafe { CStr::from_ptr(info.dli_fname) }.to_bytes();
    loader_path(bytes)
}

fn loader_path(bytes: &[u8]) -> Option<std::path::PathBuf> {
    if bytes.is_empty() {
        return None;
    }
    Some(Path::new(std::ffi::OsStr::from_bytes(bytes)).to_path_buf())
}

fn parse_object_build_id(bytes: &[u8]) -> Option<Hash128> {
    let raw = if bytes.starts_with(ELF_MAGIC) {
        let id = parse_elf_build_id(bytes)?;
        tagged_identity(0, id)
    } else {
        let id = parse_macho_uuid(bytes)?;
        tagged_identity(1, id)
    };
    Some(Hash128::of(&raw))
}

fn tagged_identity(tag: u8, raw: &[u8]) -> Vec<u8> {
    let mut tagged = Vec::with_capacity(raw.len().saturating_add(1));
    tagged.push(tag);
    tagged.extend_from_slice(raw);
    tagged
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

fn read_u16(bytes: &[u8], offset: usize, endian: Endian) -> Option<u16> {
    let raw: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(match endian {
        Endian::Little => u16::from_le_bytes(raw),
        Endian::Big => u16::from_be_bytes(raw),
    })
}

fn read_u32(bytes: &[u8], offset: usize, endian: Endian) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(match endian {
        Endian::Little => u32::from_le_bytes(raw),
        Endian::Big => u32::from_be_bytes(raw),
    })
}

fn read_u64(bytes: &[u8], offset: usize, endian: Endian) -> Option<u64> {
    let raw: [u8; 8] = bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(match endian {
        Endian::Little => u64::from_le_bytes(raw),
        Endian::Big => u64::from_be_bytes(raw),
    })
}

fn usize_from_u64(value: u64) -> Option<usize> {
    usize::try_from(value).ok()
}

fn range(bytes: &[u8], offset: usize, len: usize) -> Option<&[u8]> {
    bytes.get(offset..offset.checked_add(len)?)
}

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|n| n & !3)
}

fn parse_elf_build_id(bytes: &[u8]) -> Option<&[u8]> {
    if !bytes.starts_with(ELF_MAGIC) || *bytes.get(6)? != 1 {
        return None;
    }
    let class = *bytes.get(4)?;
    let endian = match *bytes.get(5)? {
        1 => Endian::Little,
        2 => Endian::Big,
        _ => return None,
    };
    let (header_len, phoff, phentsize_offset, phnum_offset, min_phentsize) = match class {
        1 => (
            52usize,
            read_u32(bytes, 28, endian)? as u64,
            42usize,
            44usize,
            32usize,
        ),
        2 => (
            64usize,
            read_u64(bytes, 32, endian)?,
            54usize,
            56usize,
            56usize,
        ),
        _ => return None,
    };
    range(bytes, 0, header_len)?;
    let phoff = usize_from_u64(phoff)?;
    let phentsize = usize::from(read_u16(bytes, phentsize_offset, endian)?);
    let phnum = usize::from(read_u16(bytes, phnum_offset, endian)?);
    if phentsize < min_phentsize {
        return None;
    }
    let table_len = phentsize.checked_mul(phnum)?;
    range(bytes, phoff, table_len)?;

    let mut found = None;
    for index in 0..phnum {
        let header = phoff.checked_add(index.checked_mul(phentsize)?)?;
        if read_u32(bytes, header, endian)? != PT_NOTE {
            continue;
        }
        let (note_offset, note_len) = if class == 1 {
            (
                read_u32(bytes, header.checked_add(4)?, endian)? as u64,
                read_u32(bytes, header.checked_add(16)?, endian)? as u64,
            )
        } else {
            (
                read_u64(bytes, header.checked_add(8)?, endian)?,
                read_u64(bytes, header.checked_add(32)?, endian)?,
            )
        };
        let notes = range(
            bytes,
            usize_from_u64(note_offset)?,
            usize_from_u64(note_len)?,
        )?;
        let mut cursor = 0usize;
        while cursor < notes.len() {
            let namesz = usize::try_from(read_u32(notes, cursor, endian)?).ok()?;
            let descsz = usize::try_from(read_u32(notes, cursor.checked_add(4)?, endian)?).ok()?;
            let kind = read_u32(notes, cursor.checked_add(8)?, endian)?;
            cursor = cursor.checked_add(12)?;
            let name = range(notes, cursor, namesz)?;
            cursor = cursor.checked_add(align4(namesz)?)?;
            let desc = range(notes, cursor, descsz)?;
            cursor = cursor.checked_add(align4(descsz)?)?;
            if kind == NT_GNU_BUILD_ID && name == b"GNU\0" {
                if desc.is_empty() || found.is_some() {
                    return None;
                }
                found = Some(desc);
            }
        }
    }
    found
}

fn parse_macho_uuid(bytes: &[u8]) -> Option<&[u8]> {
    let magic: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    let (is_64, endian) = match magic {
        [0xce, 0xfa, 0xed, 0xfe] => (false, Endian::Little),
        [0xcf, 0xfa, 0xed, 0xfe] => (true, Endian::Little),
        [0xfe, 0xed, 0xfa, 0xce] => (false, Endian::Big),
        [0xfe, 0xed, 0xfa, 0xcf] => (true, Endian::Big),
        _ => return None,
    };
    let header_len = if is_64 { 32usize } else { 28usize };
    range(bytes, 0, header_len)?;
    let ncmds = usize::try_from(read_u32(bytes, 16, endian)?).ok()?;
    let sizeofcmds = usize::try_from(read_u32(bytes, 20, endian)?).ok()?;
    let commands = range(bytes, header_len, sizeofcmds)?;
    let mut cursor = 0usize;
    let mut found = None;
    for _ in 0..ncmds {
        let cmd = read_u32(commands, cursor, endian)?;
        let cmdsize = usize::try_from(read_u32(commands, cursor.checked_add(4)?, endian)?).ok()?;
        if cmdsize < 8 || cmdsize % 4 != 0 {
            return None;
        }
        let command = range(commands, cursor, cmdsize)?;
        if cmd == LC_UUID {
            if cmdsize != 24 || found.is_some() {
                return None;
            }
            found = Some(command.get(8..24)?);
        }
        cursor = cursor.checked_add(cmdsize)?;
    }
    if cursor != commands.len() {
        return None;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16, endian: Endian) {
        let raw = match endian {
            Endian::Little => value.to_le_bytes(),
            Endian::Big => value.to_be_bytes(),
        };
        bytes[offset..offset + 2].copy_from_slice(&raw);
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32, endian: Endian) {
        let raw = match endian {
            Endian::Little => value.to_le_bytes(),
            Endian::Big => value.to_be_bytes(),
        };
        bytes[offset..offset + 4].copy_from_slice(&raw);
    }

    fn put_u64(bytes: &mut [u8], offset: usize, value: u64, endian: Endian) {
        let raw = match endian {
            Endian::Little => value.to_le_bytes(),
            Endian::Big => value.to_be_bytes(),
        };
        bytes[offset..offset + 8].copy_from_slice(&raw);
    }

    fn elf(class: u8, endian: Endian, id: &[u8]) -> Vec<u8> {
        let header_len = if class == 1 { 52 } else { 64 };
        let ph_len = if class == 1 { 32 } else { 56 };
        let note_offset = header_len + ph_len;
        let note_len = 12 + 4 + align4(id.len()).unwrap();
        let mut bytes = vec![0; note_offset + note_len];
        bytes[..4].copy_from_slice(ELF_MAGIC);
        bytes[4] = class;
        bytes[5] = match endian {
            Endian::Little => 1,
            Endian::Big => 2,
        };
        bytes[6] = 1;
        if class == 1 {
            put_u32(&mut bytes, 28, header_len as u32, endian);
            put_u16(&mut bytes, 42, ph_len as u16, endian);
            put_u16(&mut bytes, 44, 1, endian);
            put_u32(&mut bytes, header_len, PT_NOTE, endian);
            put_u32(&mut bytes, header_len + 4, note_offset as u32, endian);
            put_u32(&mut bytes, header_len + 16, note_len as u32, endian);
        } else {
            put_u64(&mut bytes, 32, header_len as u64, endian);
            put_u16(&mut bytes, 54, ph_len as u16, endian);
            put_u16(&mut bytes, 56, 1, endian);
            put_u32(&mut bytes, header_len, PT_NOTE, endian);
            put_u64(&mut bytes, header_len + 8, note_offset as u64, endian);
            put_u64(&mut bytes, header_len + 32, note_len as u64, endian);
        }
        put_u32(&mut bytes, note_offset, 4, endian);
        put_u32(&mut bytes, note_offset + 4, id.len() as u32, endian);
        put_u32(&mut bytes, note_offset + 8, NT_GNU_BUILD_ID, endian);
        bytes[note_offset + 12..note_offset + 16].copy_from_slice(b"GNU\0");
        bytes[note_offset + 16..note_offset + 16 + id.len()].copy_from_slice(id);
        bytes
    }

    fn macho(is_64: bool, endian: Endian, id: [u8; 16]) -> Vec<u8> {
        let header_len = if is_64 { 32 } else { 28 };
        let mut bytes = vec![0; header_len + 24];
        bytes[..4].copy_from_slice(match (is_64, endian) {
            (false, Endian::Little) => &[0xce, 0xfa, 0xed, 0xfe],
            (true, Endian::Little) => &[0xcf, 0xfa, 0xed, 0xfe],
            (false, Endian::Big) => &[0xfe, 0xed, 0xfa, 0xce],
            (true, Endian::Big) => &[0xfe, 0xed, 0xfa, 0xcf],
        });
        put_u32(&mut bytes, 16, 1, endian);
        put_u32(&mut bytes, 20, 24, endian);
        put_u32(&mut bytes, header_len, LC_UUID, endian);
        put_u32(&mut bytes, header_len + 4, 24, endian);
        bytes[header_len + 8..header_len + 24].copy_from_slice(&id);
        bytes
    }

    #[test]
    fn parses_elf32_and_elf64_in_both_endiannesses() {
        for class in [1, 2] {
            for endian in [Endian::Little, Endian::Big] {
                let bytes = elf(class, endian, b"build-id");
                assert_eq!(parse_elf_build_id(&bytes), Some(&b"build-id"[..]));
            }
        }
    }

    #[test]
    fn parses_macho32_and_macho64_in_both_endiannesses() {
        let id = *b"0123456789abcdef";
        for is_64 in [false, true] {
            for endian in [Endian::Little, Endian::Big] {
                let bytes = macho(is_64, endian, id);
                assert_eq!(parse_macho_uuid(&bytes), Some(&id[..]));
            }
        }
    }

    #[test]
    fn rejects_every_truncation_and_duplicate_or_missing_identity() {
        let elf = elf(2, Endian::Little, b"id");
        for len in 0..elf.len() {
            assert_eq!(
                parse_elf_build_id(&elf[..len]),
                None,
                "ELF truncation {len}"
            );
        }
        let mut duplicate = elf.clone();
        duplicate.extend_from_slice(&elf[64 + 56..]);
        let duplicate_note_len = (duplicate.len() - 64 - 56) as u64;
        put_u64(&mut duplicate, 64 + 32, duplicate_note_len, Endian::Little);
        assert_eq!(parse_elf_build_id(&duplicate), None);
        let mut missing = elf;
        put_u32(&mut missing, 64 + 56 + 8, 0, Endian::Little);
        assert_eq!(parse_elf_build_id(&missing), None);

        let macho = macho(true, Endian::Little, *b"0123456789abcdef");
        for len in 0..macho.len() {
            assert_eq!(
                parse_macho_uuid(&macho[..len]),
                None,
                "Mach-O truncation {len}"
            );
        }
        let mut duplicate = macho.clone();
        duplicate.extend_from_slice(&macho[32..]);
        put_u32(&mut duplicate, 16, 2, Endian::Little);
        put_u32(&mut duplicate, 20, 48, Endian::Little);
        assert_eq!(parse_macho_uuid(&duplicate), None);
        let mut missing = macho;
        put_u32(&mut missing, 32, 0, Endian::Little);
        assert_eq!(parse_macho_uuid(&missing), None);
    }

    #[test]
    fn rejects_overflowing_ranges_and_preserves_non_utf8_loader_paths() {
        let mut elf = elf(2, Endian::Little, b"identity");
        put_u64(&mut elf, 32, u64::MAX, Endian::Little);
        assert_eq!(parse_elf_build_id(&elf), None);

        let mut macho = macho(true, Endian::Little, *b"0123456789abcdef");
        put_u32(&mut macho, 20, u32::MAX, Endian::Little);
        assert_eq!(parse_macho_uuid(&macho), None);
        assert_eq!(parse_object_build_id(b"not an object"), None);

        let raw = b"/tmp/libLLVM-\xff.so";
        let path = loader_path(raw).expect("native non-UTF-8 loader path");
        assert_eq!(path.as_os_str().as_bytes(), raw);
        assert_eq!(loader_path(b""), None);
    }

    #[test]
    fn tag_distinguishes_equal_raw_identity_bytes() {
        let raw = b"0123456789abcdef";
        assert_ne!(
            Hash128::of(&tagged_identity(0, raw)),
            Hash128::of(&tagged_identity(1, raw))
        );
    }

    #[test]
    fn current_process_loaded_llvm_has_an_identity() {
        assert!(loaded_llvm_build_id().is_some());
    }
}
