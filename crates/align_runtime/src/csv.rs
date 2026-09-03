use super::{align_rt_alloc_size_fail, safe_len, soa_column_start, AlignStr, Arena};
use align_hash::{wyhash, WyHashStream};
use core::{mem, ptr, slice, str};

const STATUS_OK: i32 = 0;
const STATUS_INVALID: i32 = 1;
const STATUS_LIMIT: i32 = 2;
const STATUS_BAD_ABI: i32 = -1;
const HEADER_PRESENT: i32 = 0;
const HEADER_ABSENT: i32 = 1;
const EOL_CRLF: i32 = 0;
const EOL_LF: i32 = 1;
const HEADER_CAP: usize = 1024;
const HEADER_TABLE_CAP: usize = HEADER_CAP * 2;

const KIND_INT: u32 = 0;
const KIND_BOOL: u32 = 1;
const KIND_FLOAT: u32 = 2;
const KIND_STR: u32 = 3;
const KIND_CHAR: u32 = 4;

#[cfg(test)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Probe {
    descriptors: u64,
    header_comparisons: u64,
    conversions: u64,
    allocations: u64,
}

#[cfg(test)]
std::thread_local! {
    static PROBE: std::cell::Cell<Probe> = const { std::cell::Cell::new(Probe {
        descriptors: 0,
        header_comparisons: 0,
        conversions: 0,
        allocations: 0,
    }) };
}

#[cfg(test)]
fn probe_update(update: impl FnOnce(&mut Probe)) {
    PROBE.with(|probe| {
        let mut value = probe.get();
        update(&mut value);
        probe.set(value);
    });
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CsvField {
    pub name_ptr: *const u8,
    pub name_len: i64,
    pub name_hash: u64,
    pub tag: i32,
    pub reserved: i32,
}

#[derive(Clone, Copy, Default)]
struct Cell {
    start: usize,
    end: usize,
    escaped: bool,
}

impl Cell {
    fn decoded_len(self, src: &[u8]) -> Option<usize> {
        let raw = self.end.checked_sub(self.start)?;
        if !self.escaped {
            return Some(raw);
        }
        let mut n = 0usize;
        let mut p = self.start;
        while p < self.end {
            if src[p] == b'"' {
                if p + 1 >= self.end || src[p + 1] != b'"' {
                    return None;
                }
                p += 2;
            } else {
                p += 1;
            }
            n = n.checked_add(1)?;
        }
        Some(n)
    }

    fn chunks(self, src: &[u8], mut f: impl FnMut(&[u8])) -> Option<()> {
        if !self.escaped {
            f(&src[self.start..self.end]);
            return Some(());
        }
        let mut p = self.start;
        let mut run = p;
        while p < self.end {
            if src[p] != b'"' {
                p += 1;
                continue;
            }
            if p + 1 >= self.end || src[p + 1] != b'"' {
                return None;
            }
            f(&src[run..p]);
            f(b"\"");
            p += 2;
            run = p;
        }
        f(&src[run..self.end]);
        Some(())
    }

    fn hash(self, src: &[u8]) -> Option<u64> {
        let n = self.decoded_len(src)?;
        let mut stream = WyHashStream::for_len(0, n);
        let mut valid = true;
        self.chunks(src, |chunk| valid &= stream.update(chunk))?;
        valid.then(|| stream.finish()).flatten()
    }

    fn equals(self, src: &[u8], expected: &[u8]) -> bool {
        if self.decoded_len(src) != Some(expected.len()) {
            return false;
        }
        let mut at = 0usize;
        self.chunks(src, |chunk| {
            let end = at.checked_add(chunk.len());
            if let Some(end) = end.filter(|end| *end <= expected.len())
                && expected[at..end] == *chunk
            {
                at = end;
            } else {
                at = expected.len() + 1;
            }
        }).is_some() && at == expected.len()
    }

    fn equals_cell(self, src: &[u8], other: Cell) -> bool {
        if self.decoded_len(src) != other.decoded_len(src) {
            return false;
        }
        DecodedBytes::new(self, src).eq(DecodedBytes::new(other, src))
    }

    unsafe fn copy_decoded(self, src: &[u8], dst: *mut u8) -> Option<usize> {
        let mut at = 0usize;
        self.chunks(src, |chunk| {
            unsafe { ptr::copy_nonoverlapping(chunk.as_ptr(), dst.add(at), chunk.len()) };
            at += chunk.len();
        })?;
        Some(at)
    }
}

struct DecodedBytes<'a> {
    src: &'a [u8],
    pos: usize,
    end: usize,
    escaped: bool,
}

impl<'a> DecodedBytes<'a> {
    fn new(cell: Cell, src: &'a [u8]) -> Self {
        Self { src, pos: cell.start, end: cell.end, escaped: cell.escaped }
    }
}

impl Iterator for DecodedBytes<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        let byte = *self.src.get(self.pos..self.end)?.first()?;
        self.pos += if self.escaped && byte == b'"' { 2 } else { 1 };
        Some(byte)
    }
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    eol: i32,
}

impl<'a> Parser<'a> {
    fn new(src: &'a [u8], eol: i32) -> Self {
        Self { src, pos: if src.starts_with(&[0xef, 0xbb, 0xbf]) { 3 } else { 0 }, eol }
    }

    fn at_eol(&self, p: usize) -> Result<Option<usize>, ()> {
        match self.eol {
            EOL_LF => match self.src.get(p) {
                Some(b'\n') => Ok(Some(1)),
                Some(b'\r') => Err(()),
                _ => Ok(None),
            },
            EOL_CRLF => match self.src.get(p) {
                Some(b'\r') if self.src.get(p + 1) == Some(&b'\n') => Ok(Some(2)),
                Some(b'\r' | b'\n') => Err(()),
                _ => Ok(None),
            },
            _ => Err(()),
        }
    }

    /// Parses one record and visits fields in physical order. A trailing line ending does not
    /// synthesize an extra empty record.
    fn record(
        &mut self,
        field_limit: Option<(usize, i32)>,
        mut visit: impl FnMut(usize, Cell) -> Result<(), i32>,
    ) -> Result<Option<usize>, i32> {
        if self.pos == self.src.len() {
            return Ok(None);
        }
        let mut ordinal = 0usize;
        loop {
            if let Some((limit, status)) = field_limit
                && ordinal == limit
            {
                return Err(status);
            }
            let cell = if self.src.get(self.pos) == Some(&b'"') {
                self.pos += 1;
                let start = self.pos;
                let mut escaped = false;
                loop {
                    let Some(&b) = self.src.get(self.pos) else { return Err(STATUS_INVALID) };
                    if b != b'"' {
                        self.pos += 1;
                        continue;
                    }
                    if self.src.get(self.pos + 1) == Some(&b'"') {
                        escaped = true;
                        self.pos += 2;
                        continue;
                    }
                    let end = self.pos;
                    self.pos += 1;
                    break Cell { start, end, escaped };
                }
            } else {
                let start = self.pos;
                while self.pos < self.src.len() {
                    if self.src[self.pos] == b',' || self.at_eol(self.pos).map_err(|()| STATUS_INVALID)?.is_some() {
                        break;
                    }
                    if self.src[self.pos] == b'"' {
                        return Err(STATUS_INVALID);
                    }
                    self.pos += 1;
                }
                Cell { start, end: self.pos, escaped: false }
            };
            visit(ordinal, cell)?;
            ordinal = ordinal.checked_add(1).ok_or(STATUS_INVALID)?;
            if self.pos == self.src.len() {
                return Ok(Some(ordinal));
            }
            if self.src[self.pos] == b',' {
                self.pos += 1;
                continue;
            }
            let eol = self.at_eol(self.pos).map_err(|()| STATUS_INVALID)?.ok_or(STATUS_INVALID)?;
            self.pos += eol;
            return Ok(Some(ordinal));
        }
    }
}

fn field_shape(tag: i32) -> Option<(u32, bool, usize)> {
    let bits = u32::try_from(tag).ok()?;
    let signed = bits >> 16;
    let kind = (bits >> 8) & 0xff;
    let width = usize::try_from(bits & 0xff).ok()?;
    if bits & 0xff00_0000 != 0 || signed > 1 {
        return None;
    }
    match (kind, width, signed) {
        (KIND_INT, 1 | 2 | 4 | 8, 0 | 1)
        | (KIND_BOOL, 1, 0)
        | (KIND_FLOAT, 4 | 8, 0)
        | (KIND_STR, 16, 0)
        | (KIND_CHAR, 4, 0) => Some((kind, signed != 0, width)),
        _ => None,
    }
}

fn field_width(tag: i32) -> Option<usize> {
    field_shape(tag).map(|(_, _, width)| width)
}

fn header_slot(hash: u64) -> usize {
    // The fixed 2048-entry table makes both conversions lossless on every Rust target.
    let mask = u64::try_from(HEADER_TABLE_CAP - 1).unwrap_or_else(|_| align_rt_alloc_size_fail());
    usize::try_from(hash & mask).unwrap_or_else(|_| align_rt_alloc_size_fail())
}

fn source_identifier(bytes: &[u8]) -> bool {
    let Some((&first, rest)) = bytes.split_first() else { return false };
    if !(first == b'_' || first.is_ascii_alphabetic())
        || !rest.iter().all(|b| *b == b'_' || b.is_ascii_alphanumeric())
    {
        return false;
    }
    !matches!(bytes,
        b"as" | b"arena" | b"break" | b"else" | b"extern" | b"false" | b"fn" | b"if" |
        b"import" | b"loop" | b"match" | b"module" | b"mut" | b"pub" | b"return" |
        b"task_group" | b"template" | b"true" | b"unsafe")
}

fn descriptor_name(field: &CsvField) -> Result<&[u8], i32> {
    let n = safe_len(field.name_len).map_err(|()| STATUS_BAD_ABI)?;
    if n == 0 || field.name_ptr.is_null() || n > isize::MAX.unsigned_abs() {
        return Err(STATUS_BAD_ABI);
    }
    let start = field.name_ptr.addr();
    start.checked_add(n).filter(|end| *end >= start).ok_or(STATUS_BAD_ABI)?;
    Ok(unsafe { slice::from_raw_parts(field.name_ptr, n) })
}

fn validate_descriptors(fields: &[CsvField]) -> Result<(), i32> {
    for field in fields {
        #[cfg(test)]
        probe_update(|probe| probe.descriptors += 1);
        let name = descriptor_name(field)?;
        if !source_identifier(name) || wyhash(name, 0) != field.name_hash { return Err(STATUS_BAD_ABI); }
        if field_width(field.tag).is_none() { return Err(STATUS_BAD_ABI); }
        if field.reserved != 0 { return Err(STATUS_BAD_ABI); }
    }
    Ok(())
}

fn decoded_ascii(cell: Cell, src: &[u8]) -> Option<&[u8]> {
    (!cell.escaped).then(|| &src[cell.start..cell.end])
}

fn decimal_shape(bytes: &[u8], float: bool) -> bool {
    if bytes.is_empty() { return false; }
    let mut p = usize::from(bytes[0] == b'-');
    let int_start = p;
    while p < bytes.len() && bytes[p].is_ascii_digit() { p += 1; }
    if p == int_start { return false; }
    if float && p < bytes.len() && bytes[p] == b'.' {
        p += 1;
        let frac = p;
        while p < bytes.len() && bytes[p].is_ascii_digit() { p += 1; }
        if p == frac { return false; }
    }
    if float && p < bytes.len() && matches!(bytes[p], b'e' | b'E') {
        p += 1;
        if p < bytes.len() && matches!(bytes[p], b'+' | b'-') { p += 1; }
        let exp = p;
        while p < bytes.len() && bytes[p].is_ascii_digit() { p += 1; }
        if p == exp { return false; }
    }
    p == bytes.len()
}

fn parse_integer(bytes: &[u8], signed: bool, width: usize) -> Option<u64> {
    if !decimal_shape(bytes, false) { return None; }
    let negative = bytes[0] == b'-';
    if negative && !signed { return None; }
    let start = usize::from(bytes[0] == b'-');
    let mut magnitude = 0u128;
    for &b in &bytes[start..] {
        magnitude = magnitude.checked_mul(10)?.checked_add(u128::from(b - b'0'))?;
    }
    let bits = width * 8;
    if signed {
        let max = (1u128 << (bits - 1)) - 1;
        if negative {
            if magnitude > max + 1 { return None; }
            let value = (0u128.wrapping_sub(magnitude)) & ((1u128 << bits) - 1);
            u64::try_from(value).ok()
        } else {
            (magnitude <= max).then(|| u64::try_from(magnitude).ok()).flatten()
        }
    } else {
        let max = (1u128 << bits) - 1;
        (magnitude <= max).then(|| u64::try_from(magnitude).ok()).flatten()
    }
}

fn validate_cell(cell: Cell, src: &[u8], field: &CsvField) -> bool {
    #[cfg(test)]
    probe_update(|probe| probe.conversions += 1);
    let Some((kind, signed, width)) = field_shape(field.tag) else { return false };
    let Some(bytes) = decoded_ascii(cell, src) else {
        return kind == KIND_STR || (kind == KIND_CHAR && cell.decoded_len(src) == Some(1));
    };
    match kind {
        KIND_INT => parse_integer(bytes, signed, width).is_some(),
        KIND_BOOL => matches!(bytes, b"true" | b"false"),
        KIND_FLOAT => decimal_shape(bytes, true) && str::from_utf8(bytes).ok().and_then(|s| match width { 4 => s.parse::<f32>().ok().map(|_| ()), 8 => s.parse::<f64>().ok().map(|_| ()), _ => None }).is_some(),
        KIND_STR => true,
        KIND_CHAR => str::from_utf8(bytes).ok().and_then(|s| { let mut chars = s.chars(); let one = chars.next()?; chars.next().is_none().then_some(one) }).is_some(),
        _ => false,
    }
}

unsafe fn write_cell(cell: Cell, src: &[u8], field: &CsvField, dst: *mut u8, text: &mut *mut u8) -> Option<()> {
    let (kind, signed, width) = field_shape(field.tag)?;
    match kind {
        KIND_INT => {
            let value = parse_integer(decoded_ascii(cell, src)?, signed, width)?;
            match width {
                1 => unsafe { ptr::write(dst, u8::try_from(value).ok()?) },
                2 => unsafe { ptr::write_unaligned(dst.cast::<u16>(), u16::try_from(value).ok()?) },
                4 => unsafe { ptr::write_unaligned(dst.cast::<u32>(), u32::try_from(value).ok()?) },
                8 => unsafe { ptr::write_unaligned(dst.cast::<u64>(), value) },
                _ => return None,
            }
        }
        KIND_BOOL => unsafe { *dst = u8::from(decoded_ascii(cell, src)? == b"true") },
        KIND_FLOAT => {
            let s = str::from_utf8(decoded_ascii(cell, src)?).ok()?;
            match width {
                4 => unsafe { ptr::write_unaligned(dst.cast::<f32>(), s.parse().ok()?) },
                8 => unsafe { ptr::write_unaligned(dst.cast::<f64>(), s.parse().ok()?) },
                _ => return None,
            }
        }
        KIND_STR => {
            let len = cell.decoded_len(src)?;
            let data = if cell.escaped {
                let p = *text;
                if unsafe { cell.copy_decoded(src, p) }? != len { return None; }
                *text = unsafe { p.add(len) };
                p.cast_const()
            } else {
                unsafe { src.as_ptr().add(cell.start) }
            };
            unsafe { ptr::write_unaligned(dst.cast::<AlignStr>(), AlignStr { ptr: data, len: i64::try_from(len).ok()? }) };
        }
        KIND_CHAR => {
            let value = if cell.escaped {
                if cell.decoded_len(src)? != 1 { return None; }
                u32::from(b'"')
            } else {
                let s = str::from_utf8(&src[cell.start..cell.end]).ok()?;
                let mut chars = s.chars();
                let one = chars.next()?;
                if chars.next().is_some() { return None; }
                u32::from(one)
            };
            unsafe { ptr::write_unaligned(dst.cast::<u32>(), value) };
        }
        _ => return None,
    }
    Some(())
}

fn column_layout(fields: &[CsvField], rows: usize) -> Option<(usize, usize)> {
    let mut off = 0usize;
    let mut max_align = 1usize;
    for (index, field) in fields.iter().enumerate() {
        let width = field_width(field.tag)?;
        off = soa_column_start(off, width, index == 0)?;
        off = off.checked_add(rows.checked_mul(width)?)?;
        max_align = max_align.max(width);
    }
    Some((off, max_align))
}

unsafe fn checked_slice<'a, T>(raw: *const T, count: i64) -> Result<&'a [T], i32> {
    let n = safe_len(count).map_err(|()| STATUS_BAD_ABI)?;
    if n == 0 || raw.is_null() || !raw.addr().is_multiple_of(mem::align_of::<T>()) {
        return Err(STATUS_BAD_ABI);
    }
    let bytes = n.checked_mul(mem::size_of::<T>()).filter(|n| *n <= isize::MAX.unsigned_abs()).ok_or(STATUS_BAD_ABI)?;
    raw.addr().checked_add(bytes).ok_or(STATUS_BAD_ABI)?;
    Ok(unsafe { slice::from_raw_parts(raw, n) })
}

/// Typed RFC-4180 CSV direct-fill entry point. Validation and counting complete before the sole
/// arena allocation; pass two only writes values already proven valid.
///
/// # Safety
/// Raw arguments must satisfy the private compiler/runtime ABI documented by A123.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_csv_decode_soa_v1(
    input: *const u8,
    input_len: i64,
    fields: *const CsvField,
    n_fields: i64,
    arena: *mut Arena,
    header: i32,
    line_ending: i32,
    max_rows: i64,
    out: *mut AlignStr,
) -> i32 {
    if out.is_null() || !out.addr().is_multiple_of(mem::align_of::<AlignStr>()) || arena.is_null()
        || !arena.addr().is_multiple_of(mem::align_of::<Arena>())
    {
        return STATUS_BAD_ABI;
    }
    unsafe { ptr::write(out, AlignStr { ptr: ptr::null(), len: 0 }) };
    if !matches!(header, HEADER_PRESENT | HEADER_ABSENT) || !matches!(line_ending, EOL_CRLF | EOL_LF) || max_rows < 0 {
        return STATUS_INVALID;
    }
    let fields = match unsafe { checked_slice(fields, n_fields) } { Ok(v) => v, Err(e) => return e };
    if let Err(e) = validate_descriptors(fields) { return e; }
    let input_len = match safe_len(input_len) { Ok(n) => n, Err(()) => return STATUS_BAD_ABI };
    if input_len > 0 && input.is_null() { return STATUS_BAD_ABI; }
    if input.addr().checked_add(input_len).is_none() { return STATUS_BAD_ABI; }
    let src = if input_len == 0 { &[][..] } else { unsafe { slice::from_raw_parts(input, input_len) } };
    if str::from_utf8(src).is_err() { return STATUS_BAD_ABI; }

    let mut physical = [Cell::default(); HEADER_CAP];
    let mut hashes = [0u64; HEADER_CAP];
    let mut header_table = [0usize; HEADER_TABLE_CAP];
    let mut selected = [usize::MAX; HEADER_CAP];
    let mut parser = Parser::new(src, line_ending);
    let mut header_count = 0usize;
    if header == HEADER_PRESENT {
        header_count = match parser.record(Some((HEADER_CAP, STATUS_LIMIT)), |ordinal, cell| {
            physical[ordinal] = cell;
            Ok(())
        }) {
            Ok(Some(n)) => n,
            Ok(None) => return STATUS_INVALID,
            Err(e) => return e,
        };
        for i in 0..header_count {
            if physical[i].decoded_len(src) == Some(0) { return STATUS_INVALID; }
            hashes[i] = match physical[i].hash(src) { Some(h) => h, None => return STATUS_INVALID };
            let mut slot = header_slot(hashes[i]);
            loop {
                let stored = header_table[slot];
                if stored == 0 {
                    header_table[slot] = i + 1;
                    break;
                }
                let prior = stored - 1;
                if hashes[i] == hashes[prior] {
                    #[cfg(test)]
                    probe_update(|probe| probe.header_comparisons += 1);
                    if physical[i].equals_cell(src, physical[prior]) {
                        return STATUS_INVALID;
                    }
                }
                slot = (slot + 1) & (HEADER_TABLE_CAP - 1);
            }
        }
        if fields.len() > HEADER_CAP { return STATUS_INVALID; }
        for (field_index, field) in fields.iter().enumerate() {
            let name = match descriptor_name(field) { Ok(v) => v, Err(e) => return e };
            let mut slot = header_slot(field.name_hash);
            let found = loop {
                let stored = header_table[slot];
                if stored == 0 {
                    break None;
                }
                let candidate = stored - 1;
                if hashes[candidate] == field.name_hash {
                    #[cfg(test)]
                    probe_update(|probe| probe.header_comparisons += 1);
                    if physical[candidate].equals(src, name) {
                        break Some(candidate);
                    }
                }
                slot = (slot + 1) & (HEADER_TABLE_CAP - 1);
            };
            let Some(found) = found else {
                return STATUS_INVALID;
            };
            selected[found] = field_index;
        }
    }

    let data_start = parser.pos;
    let mut rows = 0usize;
    let mut normalized = 0usize;
    loop {
        let record = parser.record(None, |ordinal, cell| {
            if header == HEADER_ABSENT {
                let Some(field) = fields.get(ordinal) else { return Err(STATUS_INVALID) };
                if !validate_cell(cell, src, field) { return Err(STATUS_INVALID); }
                if field_shape(field.tag).is_some_and(|(kind, _, _)| kind == KIND_STR) && cell.escaped {
                    normalized = normalized.checked_add(cell.decoded_len(src).ok_or(STATUS_INVALID)?).ok_or(STATUS_LIMIT)?;
                }
            } else if let Some(&field_index) = selected.get(ordinal).filter(|index| **index != usize::MAX) {
                let field = &fields[field_index];
                if !validate_cell(cell, src, field) { return Err(STATUS_INVALID); }
                if field_shape(field.tag).is_some_and(|(kind, _, _)| kind == KIND_STR) && cell.escaped {
                    normalized = normalized.checked_add(cell.decoded_len(src).ok_or(STATUS_INVALID)?).ok_or(STATUS_LIMIT)?;
                }
            }
            Ok(())
        });
        let columns = match record { Ok(Some(n)) => n, Ok(None) => break, Err(e) => return e };
        let expected = if header == HEADER_PRESENT { header_count } else { fields.len() };
        if columns != expected { return STATUS_INVALID; }
        if i64::try_from(rows).map_or(true, |n| n >= max_rows) { return STATUS_LIMIT; }
        rows = match rows.checked_add(1) { Some(n) => n, None => return STATUS_LIMIT };
    }
    let (columns_bytes, align) = match column_layout(fields, rows) { Some(v) => v, None => return STATUS_LIMIT };
    let total = match columns_bytes.checked_add(normalized) { Some(n) if n <= isize::MAX.unsigned_abs() => n, _ => return STATUS_LIMIT };
    if rows == 0 { return STATUS_OK; }
    #[cfg(test)]
    probe_update(|probe| probe.allocations += 1);
    let base = unsafe { (&mut *arena).alloc_zeroed(total, align) };
    let mut text = unsafe { base.add(columns_bytes) };
    let mut present_offsets = [0usize; HEADER_CAP];
    if header == HEADER_PRESENT {
        let mut off = 0usize;
        for (index, field) in fields.iter().enumerate() {
            let width = field_width(field.tag).unwrap_or_else(|| align_rt_alloc_size_fail());
            off = soa_column_start(off, width, index == 0).unwrap_or_else(|| align_rt_alloc_size_fail());
            present_offsets[index] = off;
            off = off.checked_add(rows.checked_mul(width).unwrap_or_else(|| align_rt_alloc_size_fail()))
                .unwrap_or_else(|| align_rt_alloc_size_fail());
        }
    }

    let mut fill = Parser { src, pos: data_start, eol: line_ending };
    for row in 0..rows {
        let mut absent_off = 0usize;
        let result = fill.record(None, |ordinal, cell| {
            let field_index = if header == HEADER_ABSENT { ordinal } else {
                match selected.get(ordinal).copied().filter(|index| *index != usize::MAX) { Some(i) => i, None => return Ok(()) }
            };
            let field = &fields[field_index];
            let width = field_width(field.tag).ok_or(STATUS_BAD_ABI)?;
            let col = if header == HEADER_PRESENT {
                present_offsets[field_index]
            } else {
                absent_off = soa_column_start(absent_off, width, ordinal == 0).ok_or(STATUS_BAD_ABI)?;
                let current = absent_off;
                absent_off = absent_off.checked_add(rows.checked_mul(width).ok_or(STATUS_BAD_ABI)?).ok_or(STATUS_BAD_ABI)?;
                current
            };
            let dst = unsafe { base.add(col + row * width) };
            unsafe { write_cell(cell, src, field, dst, &mut text) }.ok_or(STATUS_BAD_ABI)
        });
        let expected = if header == HEADER_PRESENT { header_count } else { fields.len() };
        if !matches!(result, Ok(Some(columns)) if columns == expected) {
            align_rt_alloc_size_fail()
        }
    }
    if fill.pos != src.len() || text != unsafe { base.add(total) } {
        align_rt_alloc_size_fail()
    }
    unsafe { ptr::write(out, AlignStr { ptr: base.cast_const(), len: rows as i64 }) };
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{align_rt_arena_begin, align_rt_arena_end};

    struct TestArena(*mut Arena);

    impl TestArena {
        fn new() -> Self { Self(align_rt_arena_begin()) }
    }

    impl Drop for TestArena {
        fn drop(&mut self) { unsafe { align_rt_arena_end(self.0) } }
    }

    fn field(name: &'static [u8], tag: i32) -> CsvField {
        CsvField {
            name_ptr: name.as_ptr(),
            name_len: name.len() as i64,
            name_hash: wyhash(name, 0),
            tag,
            reserved: 0,
        }
    }

    unsafe fn decode(input: &[u8], fields: &[CsvField], header: i32, eol: i32, max: i64) -> (i32, AlignStr, TestArena) {
        let arena = TestArena::new();
        let mut out = AlignStr { ptr: 1usize as *const u8, len: -1 };
        let status = unsafe {
            align_rt_csv_decode_soa_v1(
                input.as_ptr(), input.len() as i64, fields.as_ptr(), fields.len() as i64,
                arena.0, header, eol, max, &mut out,
            )
        };
        (status, out, arena)
    }

    fn reset_probe() { PROBE.with(|probe| probe.set(Probe::default())) }
    fn probe() -> Probe { PROBE.with(std::cell::Cell::get) }

    #[test]
    fn absent_decodes_columns_and_normalizes_only_doubled_quotes() {
        let fields = [field(b"score", 0x10008), field(b"ok", 0x0101), field(b"note", 0x0310)];
        let input = b"-7,true,clean\n42,false,\"say \"\"hi\"\"\"";
        let (status, out, _arena) = unsafe { decode(input, &fields, HEADER_ABSENT, EOL_LF, 2) };
        assert_eq!(status, STATUS_OK);
        assert_eq!(out.len, 2);
        let base = out.ptr;
        assert_eq!(unsafe { *base.cast::<i64>() }, -7);
        assert_eq!(unsafe { *base.add(8).cast::<i64>() }, 42);
        assert_eq!(unsafe { *base.add(16) }, 1);
        assert_eq!(unsafe { *base.add(17) }, 0);
        let notes = unsafe { slice::from_raw_parts(base.add(32).cast::<AlignStr>(), 2) };
        assert_eq!(unsafe { slice::from_raw_parts(notes[0].ptr, usize::try_from(notes[0].len).unwrap_or_default()) }, b"clean");
        assert_eq!(unsafe { slice::from_raw_parts(notes[1].ptr, usize::try_from(notes[1].len).unwrap_or_default()) }, b"say \"hi\"");
        assert!(notes[0].ptr.addr() >= input.as_ptr().addr() && notes[0].ptr.addr() < input.as_ptr().addr() + input.len());
        assert!(notes[1].ptr.addr() >= out.ptr.addr());
    }

    #[test]
    fn present_reorders_fields_and_skips_extra_conversion() {
        let fields = [field(b"score", 0x0008), field(b"symbol", 0x0310)];
        let input = b"ignored,symbol,score\nnot-a-number,AX,9\n";
        let (status, out, _arena) = unsafe { decode(input, &fields, HEADER_PRESENT, EOL_LF, 1) };
        assert_eq!(status, STATUS_OK);
        assert_eq!(unsafe { *out.ptr.cast::<i64>() }, 9);
        let symbol = unsafe { *out.ptr.add(16).cast::<AlignStr>() };
        assert_eq!(unsafe { slice::from_raw_parts(symbol.ptr, usize::try_from(symbol.len).unwrap_or_default()) }, b"AX");
    }

    #[test]
    fn bom_empty_and_selected_line_endings_are_exact() {
        let fields = [field(b"x", 0x0008)];
        let (status, out, _arena) = unsafe { decode(&[0xef, 0xbb, 0xbf], &fields, HEADER_ABSENT, EOL_LF, 0) };
        assert_eq!((status, out.ptr, out.len), (STATUS_OK, ptr::null(), 0));
        for (input, eol, expected) in [
            (&b"1\n2"[..], EOL_LF, STATUS_OK),
            (&b"1\r\n2"[..], EOL_CRLF, STATUS_OK),
            (&b"1\r\n2"[..], EOL_LF, STATUS_INVALID),
            (&b"1\n2"[..], EOL_CRLF, STATUS_INVALID),
        ] {
            let (status, _, _arena) = unsafe { decode(input, &fields, HEADER_ABSENT, eol, 2) };
            assert_eq!(status, expected, "{input:?} eol={eol}");
        }
    }

    #[test]
    fn conversion_header_and_limit_errors_leave_zero_output() {
        let fields = [field(b"x", 0x10001)];
        for (input, header, max, expected) in [
            (&b"+1"[..], HEADER_ABSENT, 1, STATUS_INVALID),
            (&b"128"[..], HEADER_ABSENT, 1, STATUS_INVALID),
            (&b"1\n2"[..], HEADER_ABSENT, 1, STATUS_LIMIT),
            (&b"x\nx"[..], HEADER_PRESENT, 1, STATUS_INVALID),
            (&b"x,x\n1,2"[..], HEADER_PRESENT, 1, STATUS_INVALID),
        ] {
            let (status, out, _arena) = unsafe { decode(input, &fields, header, EOL_LF, max) };
            assert_eq!(status, expected, "{input:?}");
            assert!(out.ptr.is_null());
            assert_eq!(out.len, 0);
        }
    }

    #[test]
    fn malformed_private_abi_and_option_precedence() {
        let good = field(b"x", 0x0008);
        let mut bad = good;
        bad.name_hash ^= 1;
        let arena = TestArena::new();
        let mut out = AlignStr { ptr: ptr::null(), len: 0 };
        assert_eq!(unsafe { align_rt_csv_decode_soa_v1(ptr::null(), 0, &bad, 1, arena.0, HEADER_ABSENT, EOL_LF, -1, &mut out) }, STATUS_INVALID);
        assert_eq!(unsafe { align_rt_csv_decode_soa_v1(ptr::null(), 0, &bad, 1, arena.0, HEADER_ABSENT, EOL_LF, 0, &mut out) }, STATUS_BAD_ABI);
        assert_eq!(unsafe { align_rt_csv_decode_soa_v1(b"\xff".as_ptr(), 1, &good, 1, arena.0, HEADER_ABSENT, EOL_LF, 1, &mut out) }, STATUS_BAD_ABI);

        for (header, eol, max) in [(-1, EOL_LF, 0), (2, EOL_LF, 0), (HEADER_ABSENT, -1, 0), (HEADER_ABSENT, 2, 0)] {
            assert_eq!(unsafe {
                align_rt_csv_decode_soa_v1(ptr::null(), 0, &good, 1, arena.0, header, eol, max, &mut out)
            }, STATUS_INVALID);
        }
        assert_eq!(unsafe {
            align_rt_csv_decode_soa_v1(ptr::null(), 1, &good, 1, arena.0, HEADER_ABSENT, EOL_LF, 1, &mut out)
        }, STATUS_BAD_ABI);
        assert_eq!(unsafe {
            align_rt_csv_decode_soa_v1(ptr::null(), 0, ptr::null(), 1, arena.0, HEADER_ABSENT, EOL_LF, 0, &mut out)
        }, STATUS_BAD_ABI);
        assert_eq!(unsafe {
            align_rt_csv_decode_soa_v1(ptr::null(), 0, &good, -1, arena.0, HEADER_ABSENT, EOL_LF, 0, &mut out)
        }, STATUS_BAD_ABI);

        let mut aligned_out = mem::MaybeUninit::<AlignStr>::uninit();
        let unaligned_out = unsafe { aligned_out.as_mut_ptr().cast::<u8>().add(1).cast::<AlignStr>() };
        assert_eq!(unsafe {
            align_rt_csv_decode_soa_v1(ptr::null(), 0, &good, 1, arena.0, HEADER_ABSENT, EOL_LF, 0, unaligned_out)
        }, STATUS_BAD_ABI);
        let unaligned_arena = unsafe { arena.0.cast::<u8>().add(1).cast::<Arena>() };
        assert_eq!(unsafe {
            align_rt_csv_decode_soa_v1(ptr::null(), 0, &good, 1, unaligned_arena, HEADER_ABSENT, EOL_LF, 0, &mut out)
        }, STATUS_BAD_ABI);
    }

    #[test]
    fn scalar_edges_and_lexical_twins_are_exact() {
        for (tag, accepted, rejected) in [
            (0x10001, &b"-128"[..], &b"-129"[..]),
            (0x10002, &b"32767"[..], &b"32768"[..]),
            (0x10004, &b"-2147483648"[..], &b"-2147483649"[..]),
            (0x10008, &b"9223372036854775807"[..], &b"9223372036854775808"[..]),
            (0x0001, &b"255"[..], &b"256"[..]),
            (0x0002, &b"65535"[..], &b"65536"[..]),
            (0x0004, &b"4294967295"[..], &b"4294967296"[..]),
            (0x0008, &b"18446744073709551615"[..], &b"18446744073709551616"[..]),
        ] {
            let fields = [field(b"value", tag)];
            assert_eq!(unsafe { decode(accepted, &fields, HEADER_ABSENT, EOL_LF, 1) }.0, STATUS_OK);
            assert_eq!(unsafe { decode(rejected, &fields, HEADER_ABSENT, EOL_LF, 1) }.0, STATUS_INVALID);
        }

        let i64_fields = [field(b"value", 0x10008)];
        let (_, out, _arena) = unsafe { decode(b"-9223372036854775808", &i64_fields, HEADER_ABSENT, EOL_LF, 1) };
        assert_eq!(unsafe { *out.ptr.cast::<i64>() }, i64::MIN);
        let u64_fields = [field(b"value", 0x0008)];
        let (_, out, _arena) = unsafe { decode(b"18446744073709551615", &u64_fields, HEADER_ABSENT, EOL_LF, 1) };
        assert_eq!(unsafe { *out.ptr.cast::<u64>() }, u64::MAX);

        for (tag, input, expected_bits) in [
            (0x0204, &b"1.25e2"[..], u64::from(125.0f32.to_bits())),
            (0x0204, &b"1e999"[..], u64::from(f32::INFINITY.to_bits())),
            (0x0208, &b"-2.5E-1"[..], (-0.25f64).to_bits()),
            (0x0208, &b"1e-999"[..], 0.0f64.to_bits()),
        ] {
            let fields = [field(b"value", tag)];
            let (status, out, _arena) = unsafe { decode(input, &fields, HEADER_ABSENT, EOL_LF, 1) };
            assert_eq!(status, STATUS_OK, "{input:?}");
            let bits = if tag == 0x0204 {
                u64::from(unsafe { *out.ptr.cast::<f32>() }.to_bits())
            } else {
                unsafe { *out.ptr.cast::<f64>() }.to_bits()
            };
            assert_eq!(bits, expected_bits, "{input:?}");
        }
        for input in [&b"+1.0"[..], &b".5"[..], &b"1."[..], &b"NaN"[..], &b"inf"[..]] {
            let fields = [field(b"value", 0x0208)];
            assert_eq!(unsafe { decode(input, &fields, HEADER_ABSENT, EOL_LF, 1) }.0, STATUS_INVALID);
        }

        let bool_fields = [field(b"value", 0x0101)];
        assert_eq!(unsafe { decode(b"true", &bool_fields, HEADER_ABSENT, EOL_LF, 1) }.0, STATUS_OK);
        assert_eq!(unsafe { decode(b"True", &bool_fields, HEADER_ABSENT, EOL_LF, 1) }.0, STATUS_INVALID);
        let char_fields = [field(b"value", 0x0404)];
        let (_, out, _arena) = unsafe { decode("🦀".as_bytes(), &char_fields, HEADER_ABSENT, EOL_LF, 1) };
        assert_eq!(unsafe { *out.ptr.cast::<u32>() }, u32::from('🦀'));
        assert_eq!(unsafe { decode("ab".as_bytes(), &char_fields, HEADER_ABSENT, EOL_LF, 1) }.0, STATUS_INVALID);
        let (_, out, _arena) = unsafe { decode(b"\"\"\"\"", &char_fields, HEADER_ABSENT, EOL_LF, 1) };
        assert_eq!(unsafe { *out.ptr.cast::<u32>() }, u32::from('"'));
    }

    #[test]
    fn csv_grammar_and_header_boundaries_are_closed() {
        let strings = [field(b"value", 0x0310)];
        for (input, expected) in [
            (&b"plain"[..], &b"plain"[..]),
            (&b"\"a,b\""[..], &b"a,b"[..]),
            (&b"\"a\nb\""[..], &b"a\nb"[..]),
            (&b"\"a\"\"b\""[..], &b"a\"b"[..]),
            (&b"a\0b"[..], &b"a\0b"[..]),
            (&b"\xef\xbb\xbfplain"[..], &b"plain"[..]),
        ] {
            let (status, out, _arena) = unsafe { decode(input, &strings, HEADER_ABSENT, EOL_LF, 1) };
            assert_eq!(status, STATUS_OK, "{input:?}");
            let value = unsafe { *out.ptr.cast::<AlignStr>() };
            assert_eq!(unsafe { slice::from_raw_parts(value.ptr, usize::try_from(value.len).unwrap_or_default()) }, expected);
        }
        for input in [&b"\"unterminated"[..], &b"a\"b"[..], &b"\"a\"x"[..], &b"a\rb"[..]] {
            assert_eq!(unsafe { decode(input, &strings, HEADER_ABSENT, EOL_LF, 1) }.0, STATUS_INVALID, "{input:?}");
        }

        let numeric = [field(b"x", 0x0008)];
        for (input, expected) in [
            (&b"x\n1"[..], STATUS_OK),
            (&b"X\n1"[..], STATUS_INVALID),
            (&b"x,x\n1,2"[..], STATUS_INVALID),
            (&b",extra\n1,2"[..], STATUS_INVALID),
            (&b"y\n1"[..], STATUS_INVALID),
        ] {
            assert_eq!(unsafe { decode(input, &numeric, HEADER_PRESENT, EOL_LF, 1) }.0, expected, "{input:?}");
        }

        let header = |count: usize| {
            let mut bytes = Vec::new();
            for index in 0..count {
                if index != 0 { bytes.push(b','); }
                if index == 0 { bytes.extend_from_slice(b"x"); }
                else { bytes.extend_from_slice(format!("extra_{index}").as_bytes()); }
            }
            bytes
        };
        let mut at_cap = header(HEADER_CAP);
        at_cap.push(b'\n');
        at_cap.extend_from_slice(b"1");
        for _ in 1..HEADER_CAP { at_cap.extend_from_slice(b",ignored"); }
        assert_eq!(unsafe { decode(&at_cap, &numeric, HEADER_PRESENT, EOL_LF, 1) }.0, STATUS_OK);
        assert_eq!(unsafe { decode(&header(HEADER_CAP + 1), &numeric, HEADER_PRESENT, EOL_LF, 0) }.0, STATUS_LIMIT);
    }

    #[test]
    fn descriptor_vectors_and_malformed_records_are_exact() {
        for (name, hash) in [
            (&b"a"[..], 0x28d2_0533_09d2_8531),
            (&b"_"[..], 0xa648_b00f_869d_e2d5),
            (&b"field_1025"[..], 0x3009_310c_4017_8c99),
            (&b"common_prefix_0001"[..], 0x9451_6051_bd42_91e2),
            (&b"score"[..], 0x1300_a50c_fadb_78d9),
        ] {
            assert_eq!(wyhash(name, 0), hash, "{name:?}");
        }
        let arena = TestArena::new();
        let mut out = AlignStr { ptr: ptr::null(), len: 0 };
        let good = field(b"value", 0x0008);
        let mut bad_reserved = good;
        bad_reserved.reserved = 1;
        let mut bad_tag = good;
        bad_tag.tag = 0;
        for bad in [
            bad_reserved,
            bad_tag,
            field(b"fn", 0x0008),
            field(b"bad-name", 0x0008),
            field(b"9bad", 0x0008),
            field(b"bad\0name", 0x0008),
            field("méchant".as_bytes(), 0x0008),
        ] {
            assert_eq!(unsafe {
                align_rt_csv_decode_soa_v1(ptr::null(), 0, &bad, 1, arena.0, HEADER_ABSENT, EOL_LF, 0, &mut out)
            }, STATUS_BAD_ABI);
        }
        assert_eq!(unsafe {
            align_rt_csv_decode_soa_v1(ptr::null(), 0, &good, 0, arena.0, HEADER_ABSENT, EOL_LF, 0, &mut out)
        }, STATUS_BAD_ABI);
        let misaligned = unsafe { (&good as *const CsvField).cast::<u8>().add(1).cast::<CsvField>() };
        assert_eq!(unsafe {
            align_rt_csv_decode_soa_v1(ptr::null(), 0, misaligned, 1, arena.0, HEADER_ABSENT, EOL_LF, 0, &mut out)
        }, STATUS_BAD_ABI);
    }

    #[test]
    fn probes_pin_descriptor_conversion_and_allocation_work() {
        let fields = [field(b"score", 0x10008), field(b"name", 0x0310)];
        reset_probe();
        let (status, _, _arena) = unsafe {
            decode(b"extra,name,score\nignored,A,1\nignored,B,2", &fields, HEADER_PRESENT, EOL_LF, 2)
        };
        assert_eq!(status, STATUS_OK);
        let measured = probe();
        assert_eq!(measured.descriptors, 2);
        assert_eq!(measured.conversions, 4, "the unselected extra column is not converted");
        assert_eq!(measured.allocations, 1);
        assert!(measured.header_comparisons <= fields.len() as u64 * 3);

        reset_probe();
        let (status, _, arena) = unsafe {
            decode(b"extra,name,score\nignored,A,bad", &fields, HEADER_PRESENT, EOL_LF, 1)
        };
        assert_eq!(status, STATUS_INVALID);
        assert_eq!(probe().allocations, 0);
        assert!(unsafe { (*arena.0).chunks.is_empty() });
    }
}
