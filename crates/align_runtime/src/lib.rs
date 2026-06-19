//! ABI sketch of the minimal runtime (`docs/impl/06-runtime-std.md`).
//!
//! No GC. Holds only "the minimum the language requires" such as arena / parallelism
//! / panic / mutable buffers. Lifetimes and free points are already settled by the
//! compiler (MIR); the runtime allocates/frees exactly as told.
//!
//! M1 wires the first real entry point: the builtin `print` lowers to a call to
//! [`align_rt_print_i64`]. Formatting lives here (not in codegen) so it can later be
//! swapped for a SIMD itoa without touching the compiler (`open-questions.md` Future).

/// Builtin `print` for integers: write the decimal value + newline to stdout.
///
/// M1 widens every integer argument to `i64` in codegen and routes it here. `bool`,
/// strings, and a no-newline variant arrive with `std.io` (M5). The C ABI (`extern "C"`
/// + no mangling) is what the generated `call` targets.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_print_i64(x: i64) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    // A failed write to a closed pipe is ignored here (EPIPE handling is a std.io concern).
    let _ = writeln!(out, "{x}");
}

/// Builtin `print` for strings: write the bytes + a newline to stdout. `str` is a
/// `{ ptr, len }` view (`docs/impl/06-runtime-std.md` §2).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_print_str(ptr: *const u8, len: i64) {
    use std::io::Write;
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(bytes);
    let _ = out.write_all(b"\n");
}

/// Builtin `print` for booleans: write `true`/`false` + a newline.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_print_bool(v: i32) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    // Write the constant bytes directly (no formatting machinery).
    let _ = out.write_all(if v != 0 { &b"true\n"[..] } else { &b"false\n"[..] });
}

/// Builtin `print` for a `char` (a Unicode scalar value): write its UTF-8 + a newline.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_print_char(c: u32) {
    use std::io::Write;
    let ch = char::from_u32(c).unwrap_or('\u{FFFD}');
    let mut tmp = [0u8; 4];
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(ch.encode_utf8(&mut tmp).as_bytes());
    let _ = out.write_all(b"\n");
}

/// Append a float's shortest round-trip decimal (Rust's `Display`), ensuring it reads as a
/// float: if the rendering has no `.`/exponent and isn't `inf`/`NaN`, a `.0` is appended.
/// Generic over `Display` so the value is written straight into `buf` (no temporary `String`).
fn push_float<T: std::fmt::Display>(buf: &mut Vec<u8>, x: T) {
    use std::io::Write;
    let start = buf.len();
    let _ = write!(buf, "{x}");
    let looks_float = buf[start..].iter().any(|&b| matches!(b, b'.' | b'e' | b'E') || b.is_ascii_alphabetic());
    if !looks_float {
        buf.extend_from_slice(b".0");
    }
}

/// Builtin `print` for `f64`: shortest round-trip decimal + a newline.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_print_f64(x: f64) {
    use std::io::Write;
    let mut line = Vec::with_capacity(32);
    push_float(&mut line, x);
    line.push(b'\n');
    let _ = std::io::stdout().lock().write_all(&line);
}

/// Builtin `print` for `f32`: shortest round-trip decimal + a newline.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_print_f32(x: f32) {
    use std::io::Write;
    let mut line = Vec::with_capacity(32);
    push_float(&mut line, x);
    line.push(b'\n');
    let _ = std::io::stdout().lock().write_all(&line);
}

/// A `str` view passed/returned across the ABI: `{ ptr, len }` (`06-runtime-std.md` §2).
#[repr(C)]
pub struct AlignStr {
    pub ptr: *const u8,
    pub len: i64,
}

/// An append-oriented string builder (`06-runtime-std.md` §7), backing `template`
/// desugaring. M5: heap-backed; the finished buffer is leaked (no ownership/free yet —
/// arena-tied builders come later).
pub struct Builder {
    buf: Vec<u8>,
    /// Where the finished bytes live: an arena (bulk-freed) or null (leaked).
    arena: *mut Arena,
}

/// Open a builder. If `arena` is non-null, the finished string is allocated in that
/// arena (freed in bulk at the block's end); otherwise it is leaked (no owner yet).
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_new(arena: *mut Arena) -> *mut Builder {
    Box::into_raw(Box::new(Builder { buf: Vec::new(), arena }))
}

/// Append raw bytes (a static template part or a `str` value).
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write(b: *mut Builder, ptr: *const u8, len: i64) {
    let b = unsafe { &mut *b };
    if len > 0 {
        b.buf.extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, len as usize) });
    }
}

/// Append a decimal integer.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_int(b: *mut Builder, v: i64) {
    use std::io::Write;
    let b = unsafe { &mut *b };
    let _ = write!(b.buf, "{v}");
}

/// Append `true`/`false`.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_bool(b: *mut Builder, v: i32) {
    let b = unsafe { &mut *b };
    b.buf.extend_from_slice(if v != 0 { &b"true"[..] } else { &b"false"[..] });
}

/// Append a `char`'s UTF-8 encoding.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_char(b: *mut Builder, c: u32) {
    let b = unsafe { &mut *b };
    let ch = char::from_u32(c).unwrap_or('\u{FFFD}');
    let mut tmp = [0u8; 4];
    b.buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
}

/// Append an `f64`'s shortest round-trip decimal.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_f64(b: *mut Builder, x: f64) {
    let b = unsafe { &mut *b };
    push_float(&mut b.buf, x);
}

/// Append an `f32`'s shortest round-trip decimal.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_write_f32(b: *mut Builder, x: f32) {
    let b = unsafe { &mut *b };
    push_float(&mut b.buf, x);
}

/// Append a `str` as a JSON string literal: a leading/trailing `"` with the content
/// escaped per RFC 8259 (`"`, `\`, and the C0 control set; the rest passes through as
/// UTF-8). Backs `json.encode` for `str` fields.
///
/// # Safety
/// `ptr`/`len` must describe a valid byte range for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_builder_write_json_str(b: *mut Builder, ptr: *const u8, len: i64) {
    let b = unsafe { &mut *b };
    b.buf.push(b'"');
    if len > 0 {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        for &c in bytes {
            match c {
                b'"' => b.buf.extend_from_slice(b"\\\""),
                b'\\' => b.buf.extend_from_slice(b"\\\\"),
                0x08 => b.buf.extend_from_slice(b"\\b"),
                0x0c => b.buf.extend_from_slice(b"\\f"),
                b'\n' => b.buf.extend_from_slice(b"\\n"),
                b'\r' => b.buf.extend_from_slice(b"\\r"),
                b'\t' => b.buf.extend_from_slice(b"\\t"),
                c if c < 0x20 => {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    b.buf.extend_from_slice(b"\\u00");
                    b.buf.push(HEX[(c >> 4) as usize]);
                    b.buf.push(HEX[(c & 0xf) as usize]);
                }
                c => b.buf.push(c),
            }
        }
    }
    b.buf.push(b'"');
}

/// One field descriptor for `json.decode` (matches the codegen layout):
/// `{ name_ptr, name_len, tag, offset }`. `tag`: byte width for ints (1/2/4/8), 0 for bool.
#[repr(C)]
pub struct JsonField {
    pub name_ptr: *const u8,
    pub name_len: i64,
    pub tag: i32,
    pub offset: i64,
}

/// Parse the JSON object in `input` into the zeroed struct at `out` (size `out_size`),
/// writing each known field per its descriptor. Returns 0 on success, nonzero on a parse
/// error or a missing/duplicate field. M5 cut: a flat object of integer / boolean values.
///
/// # Safety
/// `input`/`fields`/`out` must describe valid ranges for the call; `out` must have room for
/// the largest `offset + width`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_json_decode(
    input: *const u8,
    input_len: i64,
    fields: *const JsonField,
    n_fields: i64,
    out: *mut u8,
    out_size: i64,
) -> i32 {
    let src = unsafe { std::slice::from_raw_parts(input, input_len.max(0) as usize) };
    let descs = unsafe { std::slice::from_raw_parts(fields, n_fields.max(0) as usize) };
    let mut seen = vec![false; descs.len()];

    let mut p = JsonParser { src, pos: 0 };
    let ok = (|| -> Option<()> {
        p.ws();
        p.expect(b'{')?;
        p.ws();
        if p.peek() == Some(b'}') {
            p.pos += 1;
        } else {
            loop {
                p.ws();
                let key = p.string()?;
                p.ws();
                p.expect(b':')?;
                p.ws();
                // Find the matching field descriptor (unknown keys are skipped).
                let idx = descs.iter().position(|d| {
                    let name = unsafe { std::slice::from_raw_parts(d.name_ptr, d.name_len.max(0) as usize) };
                    name == key
                });
                match idx {
                    Some(i) => {
                        if seen[i] {
                            return None; // duplicate field
                        }
                        seen[i] = true;
                        let d = &descs[i];
                        // tag = (kind << 8) | byte-width. kind: 0 = int, 1 = bool, 2 = float.
                        let kind = (d.tag >> 8) & 0xff;
                        let width = (d.tag & 0xff) as i64;
                        // Defense in depth: never write outside the out struct, even if a
                        // descriptor offset/width were wrong.
                        if d.offset < 0 || d.offset + width > out_size {
                            return None;
                        }
                        let off = d.offset as usize;
                        let w = width as usize;
                        match kind {
                            1 => {
                                let v = p.boolean()?;
                                unsafe { *out.add(off) = v as u8 };
                            }
                            2 => {
                                let v = p.number()?;
                                // Write the float repr at the field width (f32 / f64).
                                if w == 4 {
                                    let bytes = (v as f32).to_le_bytes();
                                    for k in 0..4 {
                                        unsafe { *out.add(off + k) = bytes[k] };
                                    }
                                } else {
                                    let bytes = v.to_le_bytes();
                                    for k in 0..8 {
                                        unsafe { *out.add(off + k) = bytes[k] };
                                    }
                                }
                            }
                            _ => {
                                let v = p.integer()?;
                                let bytes = v.to_le_bytes();
                                for k in 0..w {
                                    unsafe { *out.add(off + k) = bytes[k] };
                                }
                            }
                        }
                    }
                    None => p.skip_value()?,
                }
                p.ws();
                match p.peek() {
                    Some(b',') => {
                        p.pos += 1;
                        continue;
                    }
                    Some(b'}') => {
                        p.pos += 1;
                        break;
                    }
                    _ => return None,
                }
            }
        }
        p.ws();
        // Trailing garbage after the object is an error.
        if p.pos != src.len() { return None; }
        Some(())
    })();
    // All declared fields must be present.
    if ok.is_some() && seen.iter().all(|&s| s) {
        0
    } else {
        1
    }
}

/// A minimal JSON scanner over a byte slice (just what `json.decode` needs: objects with
/// integer / boolean values; strings only as keys).
struct JsonParser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }
    fn expect(&mut self, c: u8) -> Option<()> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Some(())
        } else {
            None
        }
    }
    /// Read a `"..."` string key (no escapes for the M5 cut). Borrows the input (`&'a`), so
    /// it does not hold `self`, and the parser can keep advancing after.
    fn string(&mut self) -> Option<&'a [u8]> {
        self.expect(b'"')?;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'"' {
                let s = &self.src[start..self.pos];
                self.pos += 1;
                return Some(s);
            }
            if c == b'\\' {
                return None; // escapes in keys unsupported (M5 cut)
            }
            self.pos += 1;
        }
        None
    }
    fn integer(&mut self) -> Option<i64> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digits = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == digits {
            return None;
        }
        std::str::from_utf8(&self.src[start..self.pos]).ok()?.parse::<i64>().ok()
    }
    /// Read a JSON number (`-?digits(.digits)?([eE][+-]?digits)?`) as `f64`.
    fn number(&mut self) -> Option<f64> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if self.pos == start {
            return None;
        }
        std::str::from_utf8(&self.src[start..self.pos]).ok()?.parse::<f64>().ok()
    }
    fn boolean(&mut self) -> Option<bool> {
        if self.src[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Some(true)
        } else if self.src[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Some(false)
        } else {
            None
        }
    }
    /// Skip a value of an unknown key (number / bool / string for the M5 cut).
    fn skip_value(&mut self) -> Option<()> {
        match self.peek() {
            Some(b't' | b'f') => self.boolean().map(|_| ()),
            Some(b'-' | b'0'..=b'9') => self.number().map(|_| ()),
            Some(b'"') => self.string().map(|_| ()),
            _ => None,
        }
    }
}

/// Finish the builder, returning a `str` view over the (leaked) contents and freeing
/// the builder object.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_builder_finish(b: *mut Builder) -> AlignStr {
    let b = unsafe { Box::from_raw(b) };
    let len = b.buf.len() as i64;
    if len == 0 {
        // Empty: no allocation needed; a dangling non-null ptr is valid for a 0-len view.
        AlignStr { ptr: std::ptr::NonNull::dangling().as_ptr(), len: 0 }
    } else if b.arena.is_null() {
        // No arena: leak the buffer so the view stays valid (process-lifetime).
        let ptr = Box::leak(b.buf.into_boxed_slice()).as_ptr();
        AlignStr { ptr, len }
    } else {
        // Copy into the arena so the view is freed with it (no leak).
        let arena = unsafe { &mut *b.arena };
        let dst = arena.alloc(b.buf.len(), 1);
        unsafe { std::ptr::copy_nonoverlapping(b.buf.as_ptr(), dst, b.buf.len()) };
        AlignStr { ptr: dst, len }
    }
}

/// Byte-equality of two `str` views (M5). Returns 1 if equal, else 0.
///
/// # Safety
/// Both `ptr`/`len` pairs must describe valid byte ranges for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_str_eq(a: *const u8, alen: i64, b: *const u8, blen: i64) -> i32 {
    if alen != blen {
        return 0;
    }
    // Same view, or both empty: equal without touching memory. This also avoids
    // `from_raw_parts` on a (possibly null) pointer of a zero-length view, which is UB.
    if a == b || alen == 0 {
        return 1;
    }
    let (x, y) = unsafe {
        (
            std::slice::from_raw_parts(a, alen as usize),
            std::slice::from_raw_parts(b, blen as usize),
        )
    };
    (x == y) as i32
}

/// Report an `Err` returned from `main` (`docs/impl/06-runtime-std.md` §9). M2's `Error`
/// is an i32 code; the original code is reported, and the returned value is the process
/// exit code — clamped to a nonzero `u8` so a failure never looks like success (exit 0)
/// and never wraps past the 8-bit Unix exit range. The eventual Error sum type will
/// carry a message/category here.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_report_error(code: i32) -> i32 {
    eprintln!("error: code {code}");
    code.clamp(1, 255)
}

/// Immediate abort called on arithmetic traps / invariant violations (`draft.md` §5).
/// Normally not called since overflow defaults to wrap.
pub fn panic_abort(msg: &str) -> ! {
    eprintln!("align: panic: {msg}");
    std::process::abort();
}

/// A bump allocator (`docs/impl/06-runtime-std.md` §3). Memory is carved from a list of
/// fixed-size chunks; individual allocations are never freed — the whole arena is
/// released at once by [`align_rt_arena_end`]. Chunk buffers are heap-stable (the outer
/// `Vec` growing never moves an inner buffer), so returned pointers stay valid until end.
pub struct Arena {
    chunks: Vec<Vec<u8>>,
    /// Byte offset into the last chunk.
    off: usize,
}

const CHUNK: usize = 64 * 1024;

impl Arena {
    fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        // The bit-trick below requires a power-of-two alignment; normalize so a future
        // ABI passing odd alignments stays correct.
        let align = align.max(1).next_power_of_two();
        let need = size.max(1);
        let aligned = (self.off + align - 1) & !(align - 1);
        let fits = self
            .chunks
            .last()
            .is_some_and(|c| aligned + need <= c.len());
        if !fits {
            self.chunks.push(vec![0u8; CHUNK.max(need + align)]);
            self.off = 0;
        }
        let off = (self.off + align - 1) & !(align - 1);
        let chunk = self.chunks.last_mut().unwrap();
        let ptr = unsafe { chunk.as_mut_ptr().add(off) };
        self.off = off + need;
        ptr
    }
}

/// Open a new arena. The returned handle is freed by [`align_rt_arena_end`].
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_arena_begin() -> *mut Arena {
    Box::into_raw(Box::new(Arena { chunks: Vec::new(), off: 0 }))
}

/// Bump-allocate `size` bytes (with `align`) from the arena.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_arena_alloc(arena: *mut Arena, size: i64, align: i64) -> *mut u8 {
    let arena = unsafe { &mut *arena };
    arena.alloc(size as usize, align as usize)
}

/// Bulk-release every allocation, keeping the arena for reuse.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_arena_reset(arena: *mut Arena) {
    let arena = unsafe { &mut *arena };
    arena.chunks.clear();
    arena.off = 0;
}

/// Release every allocation and the arena itself.
#[unsafe(no_mangle)]
pub extern "C" fn align_rt_arena_end(arena: *mut Arena) {
    drop(unsafe { Box::from_raw(arena) });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_alloc_is_stable_across_chunks() {
        let a = align_rt_arena_begin();
        // Many allocations spanning multiple chunks; earlier pointers stay writable.
        let first = align_rt_arena_alloc(a, 8, 8) as *mut i64;
        unsafe { *first = 42 };
        for _ in 0..50_000 {
            let p = align_rt_arena_alloc(a, 8, 8) as *mut i64;
            unsafe { *p = 1 };
        }
        assert_eq!(unsafe { *first }, 42, "earlier allocation must remain valid");
        align_rt_arena_end(a);
    }
}
