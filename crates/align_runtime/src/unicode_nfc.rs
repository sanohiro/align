//! Unicode 16 NFC. All temporary and result storage has one fallible owner.
use crate::unicode_data::{CCC, COMPOSITION, DECOMPOSITION};
use crate::{AL_CODE, AL_INVALID, AlignStr, align_rt_free, try_owned_alloc};
use core::{mem, ptr, slice};

#[derive(Clone, Copy)]
struct Unit {
    value: char,
    class: u8,
    ordinal: usize,
}

struct Allocation(*mut u8);
impl Allocation {
    fn new(size: usize) -> Result<Self, i32> {
        let pointer = try_owned_alloc(size);
        if pointer.is_null() {
            Err(AL_CODE + libc::ENOMEM)
        } else {
            Ok(Self(pointer))
        }
    }
    fn publish(mut self) -> *mut u8 {
        let pointer = self.0;
        self.0 = ptr::null_mut();
        pointer
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns a compatible allocation or canonical null.
        unsafe { align_rt_free(self.0) };
    }
}

fn extent(count: usize, width: usize) -> Option<usize> {
    let bytes = count.checked_mul(width)?;
    (bytes <= usize::MAX / 2 && i64::try_from(bytes).is_ok()).then_some(bytes)
}

fn combining_class(value: char) -> u8 {
    CCC.binary_search_by_key(&value, |entry| entry.0)
        .ok()
        .map_or(0, |index| CCC[index].1)
}

fn decompose(value: char, mut emit: impl FnMut(char) -> Result<(), i32>) -> Result<(), i32> {
    let code = u32::from(value);
    if (0xac00..0xac00 + 11172).contains(&code) {
        let offset = code - 0xac00;
        emit(char::from_u32(0x1100 + offset / 588).ok_or(AL_INVALID)?)?;
        emit(char::from_u32(0x1161 + (offset % 588) / 28).ok_or(AL_INVALID)?)?;
        if !offset.is_multiple_of(28) {
            emit(char::from_u32(0x11a7 + offset % 28).ok_or(AL_INVALID)?)?;
        }
    } else if let Ok(index) = DECOMPOSITION.binary_search_by_key(&value, |entry| entry.0) {
        for &part in DECOMPOSITION[index].1 {
            emit(part)?;
        }
    } else {
        emit(value)?;
    }
    Ok(())
}

fn compose(left: char, right: char) -> Option<char> {
    let (a, b) = (u32::from(left), u32::from(right));
    if (0x1100..0x1100 + 19).contains(&a) && (0x1161..0x1161 + 21).contains(&b) {
        return char::from_u32(0xac00 + (a - 0x1100) * 588 + (b - 0x1161) * 28);
    }
    if (0xac00..0xac00 + 11172).contains(&a)
        && (a - 0xac00).is_multiple_of(28)
        && (0x11a8..0x11a7 + 28).contains(&b)
    {
        return char::from_u32(a + b - 0x11a7);
    }
    COMPOSITION
        .binary_search_by_key(&(left, right), |entry| (entry.0, entry.1))
        .ok()
        .map(|index| COMPOSITION[index].2)
}

fn normalize(input: &str) -> Result<AlignStr, i32> {
    if input.is_empty() {
        return Ok(AlignStr {
            ptr: ptr::null(),
            len: 0,
        });
    }
    let mut count = 0usize;
    for value in input.chars() {
        decompose(value, |_| {
            count = count.checked_add(1).ok_or(AL_INVALID)?;
            Ok(())
        })?;
    }
    let bytes = extent(count, mem::size_of::<Unit>()).ok_or(AL_INVALID)?;
    let scratch = Allocation::new(bytes)?;
    let pointer = scratch.0.cast::<Unit>();
    let mut filled = 0usize;
    for value in input.chars() {
        decompose(value, |part| {
            // The same immutable table/count traversal supplies exactly count elements.
            // Keep the write guard even if a future table-generation change breaks that proof.
            if filled >= count {
                return Err(AL_INVALID);
            }
            // SAFETY: malloc aligns Unit, admitted extent bounds this unique initialized slot.
            unsafe {
                pointer.add(filled).write(Unit {
                    value: part,
                    class: combining_class(part),
                    ordinal: filled,
                });
            }
            filled += 1;
            Ok(())
        })?;
    }
    if filled != count {
        return Err(AL_INVALID);
    }
    // SAFETY: every element was initialized; the unique guard outlives this slice.
    let units = unsafe { slice::from_raw_parts_mut(pointer, count) };
    let mut start = 0;
    for index in 0..count {
        if units[index].class == 0 {
            units[start..index].sort_unstable_by_key(|unit| (unit.class, unit.ordinal));
            start = index + 1;
        }
    }
    units[start..].sort_unstable_by_key(|unit| (unit.class, unit.ordinal));
    let mut written = 0;
    let mut starter: Option<usize> = None;
    let mut last_class = 0;
    for index in 0..count {
        let unit = units[index];
        if let Some(position) = starter
            && (last_class == 0 || last_class < unit.class)
            && let Some(composed) = compose(units[position].value, unit.value)
        {
            units[position].value = composed;
            continue;
        }
        if unit.class == 0 {
            starter = Some(written);
        }
        last_class = unit.class;
        units[written] = unit;
        written += 1;
    }
    let mut length = 0usize;
    for unit in &units[..written] {
        length = length
            .checked_add(unit.value.len_utf8())
            .ok_or(AL_INVALID)?;
    }
    let length = extent(length, 1).ok_or(AL_INVALID)?;
    let output = Allocation::new(length)?;
    let mut offset = 0usize;
    for unit in &units[..written] {
        let mut encoded = [0u8; 4];
        let bytes = unit.value.encode_utf8(&mut encoded).as_bytes();
        // SAFETY: exact byte count above bounds each write; stack and output are disjoint.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), output.0.add(offset), bytes.len()) };
        offset += bytes.len();
    }
    let len = i64::try_from(length).map_err(|_| AL_INVALID)?;
    Ok(AlignStr {
        ptr: output.publish(),
        len,
    })
}

/// Explicit NFC; errors leave canonical zero output after output-metadata admission.
///
/// # Safety
/// A nonnull aligned nonwrapping out span names writable AlignStr storage without a live
/// owner. No reference may forbid output writes, even if input overlaps it. An admitted
/// nonempty, disjoint input span names initialized readable bytes for the call duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_unicode_nfc(
    out: *mut AlignStr,
    input: *const u8,
    input_len: i64,
) -> i32 {
    let address = out.addr();
    if out.is_null() || !address.is_multiple_of(mem::align_of::<AlignStr>()) {
        return AL_INVALID;
    }
    let Some(end) = address.checked_add(mem::size_of::<AlignStr>()) else {
        return AL_INVALID;
    };
    // SAFETY: the output span is admitted; caller supplies its writable provenance.
    unsafe {
        out.write(AlignStr {
            ptr: ptr::null(),
            len: 0,
        })
    };
    let Ok(length) = usize::try_from(input_len) else {
        return AL_INVALID;
    };
    if extent(length, 1).is_none() {
        return AL_INVALID;
    }
    let start = input.addr();
    let Some(input_end) = start.checked_add(length) else {
        return AL_INVALID;
    };
    if length > 0 && (input.is_null() || (start < end && address < input_end)) {
        return AL_INVALID;
    }
    let bytes = if length == 0 {
        &[]
    } else {
        // SAFETY: metadata/overlap admitted before making a reference; caller owns liveness.
        unsafe { slice::from_raw_parts(input, length) }
    };
    let Ok(text) = core::str::from_utf8(bytes) else {
        return AL_INVALID;
    };
    match normalize(text) {
        Ok(value) => {
            // SAFETY: output is still uniquely writable; transfer exactly one fresh owner.
            unsafe { out.write(value) };
            0
        }
        Err(code) => code,
    }
}

#[cfg(test)]
mod tests;
