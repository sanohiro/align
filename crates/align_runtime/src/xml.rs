use super::{AL_INVALID, AlignStr, align_rt_alloc, align_rt_free};
use core::{mem, ptr};

const XML_MAGIC: u64 = 0x414c_584d_4c52_4452;
const MAX_DEPTH: usize = 256;
const MAX_ATTRIBUTES: usize = 256;

const CURRENT_NONE: u8 = 0;
const CURRENT_START: u8 = 1;
const CURRENT_END: u8 = 2;
const CURRENT_TEXT: u8 = 3;
const TEXT_NONE: u8 = 0;
const TEXT_ORDINARY: u8 = 1;
const TEXT_CDATA: u8 = 2;

#[cfg(test)]
struct XmlWork {
    name_bytes: core::cell::Cell<usize>,
    normalize_passes: core::cell::Cell<usize>,
}

#[cfg(test)]
impl XmlWork {
    fn name_bytes(&self) -> usize {
        self.name_bytes.get()
    }

    fn set_name_bytes(&self, value: usize) {
        self.name_bytes.set(value);
    }

    fn normalize_passes(&self) -> usize {
        self.normalize_passes.get()
    }

    fn set_normalize_passes(&self, value: usize) {
        self.normalize_passes.set(value);
    }

    fn reset(&self) {
        self.name_bytes.set(0);
        self.normalize_passes.set(0);
    }
}

#[cfg(test)]
std::thread_local! {
    static XML_WORK: XmlWork = const { XmlWork {
        name_bytes: core::cell::Cell::new(0),
        normalize_passes: core::cell::Cell::new(0),
    } };
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

impl Span {
    fn valid_in(self, len: usize) -> bool {
        self.start <= self.end && self.end <= len
    }

    fn nonempty_in(self, len: usize) -> bool {
        self.start < self.end && self.end <= len
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AttributeSpan {
    name: Span,
    value: Span,
    hash: u64,
    quote: u8,
}

#[repr(C)]
pub struct XmlReader {
    magic: u64,
    input: *mut u8,
    len: usize,
    cursor: usize,
    depth: usize,
    current_name: Span,
    current_text: Span,
    pending_name: Span,
    open_names: [Span; MAX_DEPTH],
    attrs: [AttributeSpan; MAX_ATTRIBUTES],
    attr_count: usize,
    current: u8,
    text_kind: u8,
    pending_end: u8,
    seen_root: u8,
    root_done: u8,
    state_tag: u64,
}

#[derive(Clone, Copy)]
struct StartTag {
    name: Span,
    end: usize,
    attr_count: usize,
    empty: bool,
}

#[derive(Clone, Copy)]
struct EndTag {
    name: Span,
    end: usize,
}

#[derive(Clone, Copy)]
enum DecodeMode {
    Ordinary,
    Cdata,
    Attribute,
}

fn range_end(address: usize, size: usize) -> Option<usize> {
    address.checked_add(size)
}

fn ranges_overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start < b_end && b_start < a_end
}

fn tag_byte(tag: u64, byte: u8) -> u64 {
    (tag ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
}

fn tag_usize(mut tag: u64, value: usize) -> u64 {
    for byte in value.to_le_bytes() {
        tag = tag_byte(tag, byte);
    }
    tag
}

fn tag_span(tag: u64, span: Span) -> u64 {
    tag_usize(tag_usize(tag, span.start), span.end)
}

// Authenticate every live private field without touching historic input bytes. The tag is an
// invariant checksum, not a security boundary: callers cannot construct `xml.reader`, while raw
// corruption of any one in-range field must still fail before a view or cursor action is exposed.
fn shell_state_tag(shell: &XmlReader) -> u64 {
    let mut tag = 0xcbf2_9ce4_8422_2325;
    tag = tag_usize(tag, shell.input.addr());
    for value in [shell.len, shell.cursor, shell.depth, shell.attr_count] {
        tag = tag_usize(tag, value);
    }
    for value in [
        shell.current,
        shell.text_kind,
        shell.pending_end,
        shell.seen_root,
        shell.root_done,
    ] {
        tag = tag_byte(tag, value);
    }
    tag = tag_span(tag, shell.current_name);
    tag = tag_span(tag, shell.current_text);
    tag = tag_span(tag, shell.pending_name);
    for span in &shell.open_names[..shell.depth] {
        tag = tag_span(tag, *span);
    }
    for attr in &shell.attrs[..shell.attr_count] {
        tag = tag_span(tag, attr.name);
        tag = tag_span(tag, attr.value);
        for byte in attr.hash.to_le_bytes() {
            tag = tag_byte(tag, byte);
        }
        tag = tag_byte(tag, attr.quote);
    }
    tag
}

fn seal_shell(shell: &mut XmlReader) {
    shell.state_tag = shell_state_tag(shell);
}

fn pointer_shape<T>(value: *const T) -> Option<(usize, usize)> {
    if value.is_null() {
        return None;
    }
    let start = value.addr();
    if !start.is_multiple_of(mem::align_of::<T>()) {
        return None;
    }
    Some((start, range_end(start, mem::size_of::<T>())?))
}

fn input_shape(input: *const u8, len: i64) -> Option<(usize, usize, usize)> {
    let len = usize::try_from(len).ok()?;
    if len > isize::MAX as usize {
        return None;
    }
    if len == 0 {
        return input.is_null().then_some((0, 0, 0));
    }
    if input.is_null() {
        return None;
    }
    let start = input.addr();
    let end = range_end(start, len)?;
    Some((start, end, len))
}

fn xml_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

fn skip_space(bytes: &[u8], pos: &mut usize) {
    while bytes.get(*pos).is_some_and(|byte| xml_space(*byte)) {
        *pos += 1;
    }
}

fn next_char(bytes: &[u8], pos: usize) -> Option<(char, usize)> {
    let first = *bytes.get(pos)?;
    let width = match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let end = pos.checked_add(width)?;
    let ch = core::str::from_utf8(bytes.get(pos..end)?)
        .ok()?
        .chars()
        .next()?;
    Some((ch, end))
}

fn xml_char(ch: char) -> bool {
    matches!(ch as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

fn name_start(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3A | 0x41..=0x5A | 0x5F | 0x61..=0x7A | 0xC0..=0xD6 | 0xD8..=0xF6
            | 0xF8..=0x2FF | 0x370..=0x37D | 0x37F..=0x1FFF | 0x200C..=0x200D
            | 0x2070..=0x218F | 0x2C00..=0x2FEF | 0x3001..=0xD7FF | 0xF900..=0xFDCF
            | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF
    )
}

fn name_char(ch: char) -> bool {
    name_start(ch)
        || matches!(ch as u32, 0x2D | 0x2E | 0x30..=0x39 | 0xB7 | 0x300..=0x36F | 0x203F..=0x2040)
}

fn parse_name(bytes: &[u8], pos: &mut usize) -> Option<Span> {
    let start = *pos;
    let (first, next) = next_char(bytes, *pos)?;
    if !name_start(first) {
        return None;
    }
    *pos = next;
    while let Some((ch, next)) = next_char(bytes, *pos) {
        if !name_char(ch) {
            break;
        }
        *pos = next;
    }
    #[cfg(test)]
    XML_WORK.with(|work| work.set_name_bytes(work.name_bytes().saturating_add(*pos - start)));
    Some(Span { start, end: *pos })
}

fn name_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn attributes_unique(bytes: &[u8], attrs: &[AttributeSpan]) -> bool {
    for (index, attr) in attrs.iter().enumerate() {
        for previous in &attrs[..index] {
            if previous.hash == attr.hash
                && bytes.get(previous.name.start..previous.name.end)
                    == bytes.get(attr.name.start..attr.name.end)
            {
                return false;
            }
        }
    }
    true
}

fn parse_reference(bytes: &[u8], start: usize) -> Option<(char, usize)> {
    if bytes.get(start) != Some(&b'&') {
        return None;
    }
    let body = start.checked_add(1)?;
    for (name, value) in [
        (&b"amp;"[..], '&'),
        (&b"lt;"[..], '<'),
        (&b"gt;"[..], '>'),
        (&b"apos;"[..], '\''),
        (&b"quot;"[..], '"'),
    ] {
        if bytes.get(body..body.checked_add(name.len())?) == Some(name) {
            return Some((value, body + name.len()));
        }
    }
    if bytes.get(body) != Some(&b'#') {
        return None;
    }
    let mut pos = body + 1;
    let hexadecimal = matches!(bytes.get(pos), Some(b'x'));
    if hexadecimal {
        pos += 1;
    }
    let digit_start = pos;
    let mut value = 0u32;
    while let Some(byte) = bytes.get(pos).copied() {
        let digit = if hexadecimal {
            match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'f' => u32::from(byte - b'a') + 10,
                b'A'..=b'F' => u32::from(byte - b'A') + 10,
                _ => break,
            }
        } else {
            match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                _ => break,
            }
        };
        value = value
            .checked_mul(if hexadecimal { 16 } else { 10 })?
            .checked_add(digit)?;
        pos += 1;
    }
    if pos == digit_start || bytes.get(pos) != Some(&b';') {
        return None;
    }
    let ch = char::from_u32(value)?;
    xml_char(ch).then_some((ch, pos + 1))
}

fn validate_text(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut pos = start;
    while pos < end {
        if bytes.get(pos) == Some(&b'&') {
            let Some((_, next)) = parse_reference(bytes, pos) else {
                return false;
            };
            if next > end {
                return false;
            }
            pos = next;
            continue;
        }
        if bytes.get(pos..pos.saturating_add(3)) == Some(&b"]]>"[..]) {
            return false;
        }
        let Some((ch, next)) = next_char(bytes, pos) else {
            return false;
        };
        if next > end || !xml_char(ch) {
            return false;
        }
        pos = next;
    }
    true
}

fn validate_cdata(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut pos = start;
    while pos < end {
        let Some((ch, next)) = next_char(bytes, pos) else {
            return false;
        };
        if next > end || !xml_char(ch) {
            return false;
        }
        pos = next;
    }
    true
}

fn scan_ordinary_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|byte| *byte == b'<')
        .map_or(bytes.len(), |offset| start + offset)
}

fn parse_comment(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start..start.checked_add(4)?) != Some(&b"<!--"[..]) {
        return None;
    }
    let mut pos = start + 4;
    loop {
        if bytes.get(pos..pos.checked_add(3)?) == Some(&b"-->"[..]) {
            return Some(pos + 3);
        }
        if bytes.get(pos..pos.checked_add(2)?) == Some(&b"--"[..]) {
            return None;
        }
        let (ch, next) = next_char(bytes, pos)?;
        if !xml_char(ch) {
            return None;
        }
        pos = next;
    }
}

fn parse_cdata(bytes: &[u8], start: usize) -> Option<(Span, usize)> {
    const OPEN: &[u8] = b"<![CDATA[";
    if bytes.get(start..start.checked_add(OPEN.len())?) != Some(OPEN) {
        return None;
    }
    let content = start + OPEN.len();
    let tail = bytes.get(content..)?;
    let offset = tail.windows(3).position(|window| window == b"]]>")?;
    let end = content + offset;
    validate_cdata(bytes, content, end).then_some((
        Span {
            start: content,
            end,
        },
        end + 3,
    ))
}

fn parse_quoted(bytes: &[u8], pos: &mut usize) -> Option<Span> {
    let quote = *bytes.get(*pos)?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    *pos += 1;
    let start = *pos;
    while let Some(byte) = bytes.get(*pos).copied() {
        if byte == quote {
            let value = Span { start, end: *pos };
            *pos += 1;
            return Some(value);
        }
        if byte == b'<' {
            return None;
        }
        if byte == b'&' {
            let (_, next) = parse_reference(bytes, *pos)?;
            *pos = next;
            continue;
        }
        let (ch, next) = next_char(bytes, *pos)?;
        if !xml_char(ch) {
            return None;
        }
        *pos = next;
    }
    None
}

fn parse_start_tag(
    bytes: &[u8],
    start: usize,
    attrs: &mut [AttributeSpan; MAX_ATTRIBUTES],
) -> Option<StartTag> {
    if bytes.get(start) != Some(&b'<')
        || matches!(bytes.get(start + 1), Some(b'/') | Some(b'!') | Some(b'?'))
    {
        return None;
    }
    let mut pos = start + 1;
    let name = parse_name(bytes, &mut pos)?;
    let mut attr_count = 0usize;
    loop {
        if bytes.get(pos..pos.checked_add(2)?) == Some(&b"/>"[..]) {
            return Some(StartTag {
                name,
                end: pos + 2,
                attr_count,
                empty: true,
            });
        }
        if bytes.get(pos) == Some(&b'>') {
            return Some(StartTag {
                name,
                end: pos + 1,
                attr_count,
                empty: false,
            });
        }
        let before_space = pos;
        skip_space(bytes, &mut pos);
        if pos == before_space {
            return None;
        }
        if bytes.get(pos..pos.checked_add(2)?) == Some(&b"/>"[..]) {
            return Some(StartTag {
                name,
                end: pos + 2,
                attr_count,
                empty: true,
            });
        }
        if bytes.get(pos) == Some(&b'>') {
            return Some(StartTag {
                name,
                end: pos + 1,
                attr_count,
                empty: false,
            });
        }
        if attr_count == MAX_ATTRIBUTES {
            return None;
        }
        let attr_name = parse_name(bytes, &mut pos)?;
        skip_space(bytes, &mut pos);
        if bytes.get(pos) != Some(&b'=') {
            return None;
        }
        pos += 1;
        skip_space(bytes, &mut pos);
        let quote = *bytes.get(pos)?;
        let value = parse_quoted(bytes, &mut pos)?;
        attrs[attr_count] = AttributeSpan {
            name: attr_name,
            value,
            hash: name_hash(bytes.get(attr_name.start..attr_name.end)?),
            quote,
        };
        attr_count += 1;
        if !attributes_unique(bytes, &attrs[..attr_count]) {
            return None;
        }
    }
}

fn parse_end_tag(bytes: &[u8], start: usize) -> Option<EndTag> {
    if bytes.get(start..start.checked_add(2)?) != Some(&b"</"[..]) {
        return None;
    }
    let mut pos = start + 2;
    let name = parse_name(bytes, &mut pos)?;
    skip_space(bytes, &mut pos);
    if bytes.get(pos) != Some(&b'>') {
        return None;
    }
    Some(EndTag { name, end: pos + 1 })
}

fn parse_decl_value<'a>(bytes: &'a [u8], pos: &mut usize, name: &[u8]) -> Option<&'a [u8]> {
    if bytes.get(*pos..pos.checked_add(name.len())?) != Some(name) {
        return None;
    }
    *pos += name.len();
    skip_space(bytes, pos);
    if bytes.get(*pos) != Some(&b'=') {
        return None;
    }
    *pos += 1;
    skip_space(bytes, pos);
    let quote = *bytes.get(*pos)?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    *pos += 1;
    let start = *pos;
    while bytes.get(*pos).is_some_and(|byte| *byte != quote) {
        *pos += 1;
    }
    if bytes.get(*pos) != Some(&quote) {
        return None;
    }
    let value = bytes.get(start..*pos)?;
    *pos += 1;
    Some(value)
}

fn parse_xml_declaration(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start..start.checked_add(5)?) != Some(&b"<?xml"[..]) {
        return None;
    }
    let mut pos = start + 5;
    if !bytes.get(pos).is_some_and(|byte| xml_space(*byte)) {
        return None;
    }
    skip_space(bytes, &mut pos);
    if parse_decl_value(bytes, &mut pos, b"version")? != b"1.0" {
        return None;
    }
    let mut stage = 0u8;
    loop {
        if bytes.get(pos..pos.checked_add(2)?) == Some(&b"?>"[..]) {
            return Some(pos + 2);
        }
        if !bytes.get(pos).is_some_and(|byte| xml_space(*byte)) {
            return None;
        }
        skip_space(bytes, &mut pos);
        if bytes.get(pos..pos.checked_add(2)?) == Some(&b"?>"[..]) {
            return Some(pos + 2);
        }
        if stage == 0 && bytes.get(pos..pos.saturating_add(8)) == Some(&b"encoding"[..]) {
            let value = parse_decl_value(bytes, &mut pos, b"encoding")?;
            if !value.eq_ignore_ascii_case(b"UTF-8") {
                return None;
            }
            stage = 1;
            continue;
        }
        if stage <= 1 && bytes.get(pos..pos.saturating_add(10)) == Some(&b"standalone"[..]) {
            let value = parse_decl_value(bytes, &mut pos, b"standalone")?;
            if !matches!(value, b"yes" | b"no") {
                return None;
            }
            stage = 2;
            continue;
        }
        return None;
    }
}

fn initial_content_start(bytes: &[u8]) -> Option<usize> {
    let mut pos = 0usize;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        pos = 3;
    }
    if bytes.get(pos..pos.saturating_add(5)) == Some(&b"<?xml"[..]) {
        pos = parse_xml_declaration(bytes, pos)?;
    }
    Some(pos)
}

fn validate_document(bytes: &[u8]) -> bool {
    if bytes.is_empty() || core::str::from_utf8(bytes).is_err() {
        return false;
    }
    let Some(mut pos) = initial_content_start(bytes) else {
        return false;
    };
    let mut stack = [Span::default(); MAX_DEPTH];
    let mut attrs = [AttributeSpan::default(); MAX_ATTRIBUTES];
    let mut depth = 0usize;
    let mut seen_root = false;
    let mut root_done = false;

    while pos < bytes.len() {
        if bytes.get(pos..pos.saturating_add(4)) == Some(&b"<!--"[..]) {
            let Some(end) = parse_comment(bytes, pos) else {
                return false;
            };
            pos = end;
            continue;
        }
        if bytes.get(pos..pos.saturating_add(9)) == Some(&b"<![CDATA["[..]) {
            if depth == 0 {
                return false;
            }
            let Some((_, end)) = parse_cdata(bytes, pos) else {
                return false;
            };
            pos = end;
            continue;
        }
        if bytes.get(pos..pos.saturating_add(2)) == Some(&b"</"[..]) {
            if depth == 0 {
                return false;
            }
            let Some(tag) = parse_end_tag(bytes, pos) else {
                return false;
            };
            let open = stack[depth - 1];
            if bytes.get(open.start..open.end) != bytes.get(tag.name.start..tag.name.end) {
                return false;
            }
            depth -= 1;
            pos = tag.end;
            if depth == 0 {
                root_done = true;
            }
            continue;
        }
        if bytes.get(pos) == Some(&b'<') {
            if root_done || matches!(bytes.get(pos + 1), Some(b'!') | Some(b'?')) {
                return false;
            }
            let Some(tag) = parse_start_tag(bytes, pos, &mut attrs) else {
                return false;
            };
            if depth == 0 {
                if seen_root {
                    return false;
                }
                seen_root = true;
            }
            if depth == MAX_DEPTH {
                return false;
            }
            pos = tag.end;
            if tag.empty {
                if depth == 0 {
                    root_done = true;
                }
            } else {
                stack[depth] = tag.name;
                depth += 1;
            }
            continue;
        }

        let end = scan_ordinary_end(bytes, pos);
        if depth == 0 {
            if bytes[pos..end].iter().any(|byte| !xml_space(*byte)) {
                return false;
            }
        } else if !validate_text(bytes, pos, end) {
            return false;
        }
        pos = end;
    }
    seen_root && root_done && depth == 0
}

fn normalized_len(raw: &[u8], mode: DecodeMode) -> Option<usize> {
    #[cfg(test)]
    XML_WORK.with(|work| work.set_normalize_passes(work.normalize_passes() + 1));
    let mut pos = 0usize;
    let mut output = 0usize;
    while pos < raw.len() {
        if matches!(mode, DecodeMode::Ordinary | DecodeMode::Attribute) && raw[pos] == b'&' {
            let (ch, next) = parse_reference(raw, pos)?;
            output = output.checked_add(ch.len_utf8())?;
            pos = next;
            continue;
        }
        if raw[pos] == b'\r' {
            pos += 1;
            if raw.get(pos) == Some(&b'\n') {
                pos += 1;
            }
            output = output.checked_add(1)?;
            continue;
        }
        if matches!(mode, DecodeMode::Attribute) && matches!(raw[pos], b'\n' | b'\t') {
            pos += 1;
            output = output.checked_add(1)?;
            continue;
        }
        let (ch, next) = next_char(raw, pos)?;
        output = output.checked_add(ch.len_utf8())?;
        pos = next;
    }
    Some(output)
}

fn normalize_into(raw: &[u8], mode: DecodeMode, output: &mut [u8], expected_len: usize) -> bool {
    #[cfg(test)]
    XML_WORK.with(|work| work.set_normalize_passes(work.normalize_passes() + 1));
    if output.len() != expected_len {
        return false;
    }
    let mut pos = 0usize;
    let mut written = 0usize;
    while pos < raw.len() {
        if matches!(mode, DecodeMode::Ordinary | DecodeMode::Attribute) && raw[pos] == b'&' {
            let Some((ch, next)) = parse_reference(raw, pos) else {
                return false;
            };
            let mut encoded = [0u8; 4];
            let bytes = ch.encode_utf8(&mut encoded).as_bytes();
            if written.checked_add(bytes.len()).is_none_or(|end| end > output.len()) {
                return false;
            }
            output[written..written + bytes.len()].copy_from_slice(bytes);
            written += bytes.len();
            pos = next;
            continue;
        }
        if raw[pos] == b'\r' {
            pos += 1;
            if raw.get(pos) == Some(&b'\n') {
                pos += 1;
            }
            let Some(slot) = output.get_mut(written) else {
                return false;
            };
            *slot = if matches!(mode, DecodeMode::Attribute) {
                b' '
            } else {
                b'\n'
            };
            written += 1;
            continue;
        }
        if matches!(mode, DecodeMode::Attribute) && matches!(raw[pos], b'\n' | b'\t') {
            let Some(slot) = output.get_mut(written) else {
                return false;
            };
            *slot = b' ';
            written += 1;
            pos += 1;
            continue;
        }
        let Some((_, next)) = next_char(raw, pos) else {
            return false;
        };
        let count = next - pos;
        if written.checked_add(count).is_none_or(|end| end > output.len()) {
            return false;
        }
        output[written..written + count].copy_from_slice(&raw[pos..next]);
        written += count;
        pos = next;
    }
    written == output.len()
}

unsafe fn publish_owned(raw: &[u8], mode: DecodeMode, out: *mut AlignStr) -> i32 {
    let Some(len) = normalized_len(raw, mode) else {
        return AL_INVALID;
    };
    if len == 0 {
        return 0;
    }
    let Ok(len_i64) = i64::try_from(len) else {
        return AL_INVALID;
    };
    let allocation = align_rt_alloc(len_i64);
    if allocation.is_null() {
        return AL_INVALID;
    }
    let target = unsafe { core::slice::from_raw_parts_mut(allocation, len) };
    if !normalize_into(raw, mode, target, len) {
        unsafe { align_rt_free(allocation) };
        return AL_INVALID;
    }
    unsafe {
        out.write(AlignStr {
            ptr: allocation,
            len: len_i64,
        })
    };
    0
}

fn shell_shape(reader: *const XmlReader) -> Option<(usize, usize)> {
    pointer_shape(reader)
}

unsafe fn shell_fields_valid(reader: *const XmlReader) -> Option<(*mut u8, usize)> {
    // The caller has established only the fixed shell pointer shape and its raw-pointer access
    // preconditions. Read fields without forming a Rust reference: the stored input may still name
    // an overlapping range, and that must be rejected before any reference or slice exists.
    let magic = unsafe { ptr::addr_of!((*reader).magic).read() };
    let input = unsafe { ptr::addr_of!((*reader).input).read() };
    let len = unsafe { ptr::addr_of!((*reader).len).read() };
    let cursor = unsafe { ptr::addr_of!((*reader).cursor).read() };
    let depth = unsafe { ptr::addr_of!((*reader).depth).read() };
    let current_name = unsafe { ptr::addr_of!((*reader).current_name).read() };
    let current_text = unsafe { ptr::addr_of!((*reader).current_text).read() };
    let pending_name = unsafe { ptr::addr_of!((*reader).pending_name).read() };
    let attr_count = unsafe { ptr::addr_of!((*reader).attr_count).read() };
    let current = unsafe { ptr::addr_of!((*reader).current).read() };
    let text_kind = unsafe { ptr::addr_of!((*reader).text_kind).read() };
    let pending_end = unsafe { ptr::addr_of!((*reader).pending_end).read() };
    let seen_root = unsafe { ptr::addr_of!((*reader).seen_root).read() };
    let root_done = unsafe { ptr::addr_of!((*reader).root_done).read() };

    let initial = seen_root == 0
        && cursor == 0
        && depth == 0
        && attr_count == 0
        && current == CURRENT_NONE
        && text_kind == TEXT_NONE
        && pending_end == 0
        && root_done == 0
        && current_name == Span::default()
        && current_text == Span::default()
        && pending_name == Span::default();
    let eof = seen_root == 1
        && root_done == 1
        && cursor == len
        && depth == 0
        && attr_count == 0
        && current == CURRENT_NONE
        && text_kind == TEXT_NONE
        && pending_end == 0
        && current_name == Span::default()
        && current_text == Span::default()
        && pending_name == Span::default();
    let event_state = match current {
        CURRENT_START => {
            seen_root == 1
                && root_done == 0
                && cursor > 0
                && text_kind == TEXT_NONE
                && current_text == Span::default()
                && ((pending_end == 1 && pending_name == current_name)
                    || (pending_end == 0 && pending_name == Span::default() && depth > 0))
        }
        CURRENT_END => {
            seen_root == 1
                && cursor > 0
                && attr_count == 0
                && text_kind == TEXT_NONE
                && pending_end == 0
                && pending_name == Span::default()
                && current_text == Span::default()
                && ((root_done == 1 && depth == 0) || (root_done == 0 && depth > 0))
        }
        CURRENT_TEXT => {
            seen_root == 1
                && root_done == 0
                && cursor > 0
                && depth > 0
                && attr_count == 0
                && pending_end == 0
                && pending_name == Span::default()
                && current_name == Span::default()
                && matches!(text_kind, TEXT_ORDINARY | TEXT_CDATA)
        }
        CURRENT_NONE => initial || eof,
        _ => false,
    };

    if magic != XML_MAGIC
        || len == 0
        || len > isize::MAX as usize
        || input.is_null()
        || cursor > len
        || depth > MAX_DEPTH
        || attr_count > MAX_ATTRIBUTES
        || current > CURRENT_TEXT
        || text_kind > TEXT_CDATA
        || pending_end > 1
        || seen_root > 1
        || root_done > 1
        || !event_state
        || (current != CURRENT_START && attr_count != 0)
        || (matches!(current, CURRENT_START | CURRENT_END) && !current_name.nonempty_in(len))
        || (current == CURRENT_TEXT
            && (!current_text.nonempty_in(len) || !matches!(text_kind, TEXT_ORDINARY | TEXT_CDATA)))
        || (pending_end == 1
            && (current != CURRENT_START
                || !pending_name.nonempty_in(len)
                || pending_name.start != current_name.start
                || pending_name.end != current_name.end))
        || (root_done == 1 && (seen_root != 1 || depth != 0 || pending_end != 0))
    {
        return None;
    }

    let open_names = unsafe { ptr::addr_of!((*reader).open_names) }.cast::<Span>();
    for index in 0..depth {
        if !unsafe { open_names.add(index).read() }.nonempty_in(len) {
            return None;
        }
    }
    let attrs = unsafe { ptr::addr_of!((*reader).attrs) }.cast::<AttributeSpan>();
    for index in 0..attr_count {
        let attr = unsafe { attrs.add(index).read() };
        if !attr.name.nonempty_in(len)
            || !attr.value.valid_in(len)
            || !matches!(attr.quote, b'\'' | b'"')
        {
            return None;
        }
    }
    Some((input, len))
}

fn utf8_boundary(bytes: &[u8], offset: usize) -> bool {
    offset == bytes.len()
        || bytes
            .get(offset)
            .is_some_and(|byte| !matches!(byte, 0x80..=0xbf))
}

fn span_boundaries_valid(bytes: &[u8], span: Span) -> bool {
    span.valid_in(bytes.len()) && utf8_boundary(bytes, span.start) && utf8_boundary(bytes, span.end)
}

fn shell_content_fields_valid(shell: &XmlReader, bytes: &[u8]) -> bool {
    utf8_boundary(bytes, shell.cursor)
        && shell.open_names[..shell.depth]
            .iter()
            .all(|span| span.nonempty_in(bytes.len()) && span_boundaries_valid(bytes, *span))
        && (!matches!(shell.current, CURRENT_START | CURRENT_END)
            || (shell.current_name.nonempty_in(bytes.len())
                && span_boundaries_valid(bytes, shell.current_name)))
        && (shell.current != CURRENT_TEXT || span_boundaries_valid(bytes, shell.current_text))
        && (shell.current != CURRENT_START
            || shell.attrs[..shell.attr_count].iter().all(|attr| {
                attr.name.nonempty_in(bytes.len())
                    && span_boundaries_valid(bytes, attr.name)
                    && span_boundaries_valid(bytes, attr.value)
                    && attr.value.start > 0
                    && attr.value.end < bytes.len()
                    && bytes.get(attr.value.start - 1) == Some(&attr.quote)
                    && bytes.get(attr.value.end) == Some(&attr.quote)
            }))
        && (shell.pending_end == 0
            || (shell.pending_name.nonempty_in(bytes.len())
                && span_boundaries_valid(bytes, shell.pending_name)))
}

unsafe fn checked_shell(reader: *const XmlReader, out: *mut AlignStr) -> Option<*const XmlReader> {
    let (shell_start, shell_end) = shell_shape(reader)?;
    let (out_start, out_end) = pointer_shape(out)?;
    if ranges_overlap(shell_start, shell_end, out_start, out_end) {
        return None;
    }
    let (input, len) = unsafe { shell_fields_valid(reader) }?;
    let input_start = input.addr();
    let input_end = range_end(input_start, len)?;
    if ranges_overlap(shell_start, shell_end, input_start, input_end)
        || ranges_overlap(out_start, out_end, input_start, input_end)
    {
        return None;
    }
    let shell = unsafe { &*reader };
    if shell.state_tag != shell_state_tag(shell) {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(input, len) };
    if !shell_content_fields_valid(shell, bytes) {
        return None;
    }
    Some(reader)
}

unsafe fn checked_shell_mut(reader: *mut XmlReader) -> Option<*mut XmlReader> {
    let (shell_start, shell_end) = shell_shape(reader)?;
    let (input, len) = unsafe { shell_fields_valid(reader) }?;
    let input_start = input.addr();
    let input_end = range_end(input_start, len)?;
    if ranges_overlap(shell_start, shell_end, input_start, input_end) {
        return None;
    }
    let shell = unsafe { &*reader };
    if shell.state_tag != shell_state_tag(shell) {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(input, len) };
    if !shell_content_fields_valid(shell, bytes) {
        return None;
    }
    Some(reader)
}

#[unsafe(no_mangle)]
/// Parse one owned UTF-8 XML buffer and publish its reader shell.
///
/// # Safety
///
/// `input` must be null exactly when `len` is zero or name a live runtime allocation of `len`
/// bytes. `out` must name writable, aligned pointer storage and must not overlap `input`.
pub unsafe extern "C" fn align_rt_xml_parse(
    input: *mut u8,
    len: i64,
    out: *mut *mut XmlReader,
) -> i32 {
    let Some((out_start, out_end)) = pointer_shape(out) else {
        return AL_INVALID;
    };
    let Some((input_start, input_end, input_len)) = input_shape(input, len) else {
        return AL_INVALID;
    };
    if input_len != 0 && ranges_overlap(out_start, out_end, input_start, input_end) {
        return AL_INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    if input_len == 0 {
        return -1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(input, input_len) };
    if !validate_document(bytes) {
        unsafe { align_rt_free(input) };
        return -1;
    }
    let Ok(shell_size) = i64::try_from(mem::size_of::<XmlReader>()) else {
        unsafe { align_rt_free(input) };
        return AL_INVALID;
    };
    let reader = align_rt_alloc(shell_size).cast::<XmlReader>();
    let shell = XmlReader {
        magic: XML_MAGIC,
        input,
        len: input_len,
        cursor: 0,
        depth: 0,
        current_name: Span::default(),
        current_text: Span::default(),
        pending_name: Span::default(),
        open_names: [Span::default(); MAX_DEPTH],
        attrs: [AttributeSpan::default(); MAX_ATTRIBUTES],
        attr_count: 0,
        current: CURRENT_NONE,
        text_kind: TEXT_NONE,
        pending_end: 0,
        seen_root: 0,
        root_done: 0,
        state_tag: 0,
    };
    unsafe { reader.write(shell) };
    seal_shell(unsafe { &mut *reader });
    unsafe { out.write(reader) };
    0
}

#[unsafe(no_mangle)]
/// Advance a validated XML reader by one event.
///
/// # Safety
///
/// `reader` must name a live reader shell returned by [`align_rt_xml_parse`].
pub unsafe extern "C" fn align_rt_xml_next(reader: *mut XmlReader) -> i32 {
    let Some(shell) = (unsafe { checked_shell_mut(reader) }) else {
        return -1;
    };
    let shell = unsafe { &mut *shell };
    let bytes = unsafe { core::slice::from_raw_parts(shell.input, shell.len) };
    if shell.pending_end == 1 {
        shell.pending_end = 0;
        shell.current = CURRENT_END;
        shell.current_name = shell.pending_name;
        shell.pending_name = Span::default();
        shell.current_text = Span::default();
        shell.text_kind = TEXT_NONE;
        shell.attr_count = 0;
        if shell.depth == 0 {
            shell.root_done = 1;
        }
        seal_shell(shell);
        return 2;
    }
    if shell.cursor == 0 {
        let Some(start) = initial_content_start(bytes) else {
            return -1;
        };
        shell.cursor = start;
    }
    loop {
        if shell.cursor == shell.len {
            shell.current = CURRENT_NONE;
            shell.current_name = Span::default();
            shell.current_text = Span::default();
            shell.pending_name = Span::default();
            shell.text_kind = TEXT_NONE;
            shell.attr_count = 0;
            seal_shell(shell);
            return 0;
        }
        let pos = shell.cursor;
        if bytes.get(pos..pos.saturating_add(4)) == Some(&b"<!--"[..]) {
            let Some(end) = parse_comment(bytes, pos) else {
                return -1;
            };
            shell.cursor = end;
            continue;
        }
        if bytes.get(pos..pos.saturating_add(9)) == Some(&b"<![CDATA["[..]) {
            let Some((content, end)) = parse_cdata(bytes, pos) else {
                return -1;
            };
            shell.cursor = end;
            if content.start == content.end {
                continue;
            }
            shell.current = CURRENT_TEXT;
            shell.current_name = Span::default();
            shell.text_kind = TEXT_CDATA;
            shell.current_text = content;
            shell.attr_count = 0;
            seal_shell(shell);
            return 3;
        }
        if bytes.get(pos..pos.saturating_add(2)) == Some(&b"</"[..]) {
            let Some(tag) = parse_end_tag(bytes, pos) else {
                return -1;
            };
            if shell.depth == 0 {
                return -1;
            }
            if bytes
                .get(shell.open_names[shell.depth - 1].start..shell.open_names[shell.depth - 1].end)
                != bytes.get(tag.name.start..tag.name.end)
            {
                return -1;
            }
            shell.depth -= 1;
            shell.cursor = tag.end;
            shell.current = CURRENT_END;
            shell.current_name = tag.name;
            shell.current_text = Span::default();
            shell.pending_name = Span::default();
            shell.text_kind = TEXT_NONE;
            shell.attr_count = 0;
            if shell.depth == 0 {
                shell.root_done = 1;
            }
            seal_shell(shell);
            return 2;
        }
        if bytes.get(pos) == Some(&b'<') {
            let Some(tag) = parse_start_tag(bytes, pos, &mut shell.attrs) else {
                return -1;
            };
            shell.cursor = tag.end;
            shell.current = CURRENT_START;
            shell.current_name = tag.name;
            shell.current_text = Span::default();
            shell.attr_count = tag.attr_count;
            shell.text_kind = TEXT_NONE;
            shell.seen_root = 1;
            if tag.empty {
                shell.pending_end = 1;
                shell.pending_name = tag.name;
            } else {
                if shell.depth == MAX_DEPTH {
                    return -1;
                }
                shell.open_names[shell.depth] = tag.name;
                shell.depth += 1;
                shell.pending_end = 0;
                shell.pending_name = Span::default();
            }
            seal_shell(shell);
            return 1;
        }
        let end = scan_ordinary_end(bytes, pos);
        shell.cursor = end;
        if shell.depth == 0 {
            continue;
        }
        if end == pos {
            return -1;
        }
        shell.current = CURRENT_TEXT;
        shell.current_name = Span::default();
        shell.pending_name = Span::default();
        shell.text_kind = TEXT_ORDINARY;
        shell.current_text = Span { start: pos, end };
        shell.attr_count = 0;
        seal_shell(shell);
        return 3;
    }
}

#[unsafe(no_mangle)]
/// Publish the current element name as a borrowed input view.
///
/// # Safety
///
/// `reader` must name a live reader shell and `out` must name writable, aligned `AlignStr`
/// storage disjoint from both the shell and its input allocation.
pub unsafe extern "C" fn align_rt_xml_name(reader: *const XmlReader, out: *mut AlignStr) -> i32 {
    let Some(shell) = (unsafe { checked_shell(reader, out) }) else {
        return AL_INVALID;
    };
    let shell = unsafe { &*shell };
    unsafe {
        out.write(AlignStr {
            ptr: ptr::null(),
            len: 0,
        })
    };
    if !matches!(shell.current, CURRENT_START | CURRENT_END) {
        return AL_INVALID;
    }
    let span = shell.current_name;
    let Ok(len) = i64::try_from(span.end - span.start) else {
        return AL_INVALID;
    };
    unsafe {
        out.write(AlignStr {
            ptr: shell.input.add(span.start),
            len,
        })
    };
    0
}

#[unsafe(no_mangle)]
/// Return the number of attributes on the current start event.
///
/// # Safety
///
/// `reader` must name a live reader shell returned by [`align_rt_xml_parse`].
pub unsafe extern "C" fn align_rt_xml_attribute_count(reader: *const XmlReader) -> i64 {
    let Some((shell_start, shell_end)) = shell_shape(reader) else {
        return -1;
    };
    let Some((input, len)) = (unsafe { shell_fields_valid(reader) }) else {
        return -1;
    };
    let Some(input_end) = range_end(input.addr(), len) else {
        return -1;
    };
    if ranges_overlap(shell_start, shell_end, input.addr(), input_end) {
        return -1;
    }
    let shell = unsafe { &*reader };
    if shell.state_tag != shell_state_tag(shell) {
        return -1;
    }
    let bytes = unsafe { core::slice::from_raw_parts(input, len) };
    if !shell_content_fields_valid(shell, bytes) || shell.current != CURRENT_START {
        return -1;
    }
    i64::try_from(shell.attr_count).unwrap_or(-1)
}

unsafe fn attribute_shell(
    reader: *const XmlReader,
    out: *mut AlignStr,
    index: i64,
) -> Result<(*const XmlReader, usize), i32> {
    let Some(shell) = (unsafe { checked_shell(reader, out) }) else {
        return Err(AL_INVALID);
    };
    let shell = unsafe { &*shell };
    unsafe {
        out.write(AlignStr {
            ptr: ptr::null(),
            len: 0,
        })
    };
    if shell.current != CURRENT_START {
        return Err(AL_INVALID);
    }
    let Ok(index) = usize::try_from(index) else {
        return Err(AL_INVALID);
    };
    if index >= shell.attr_count {
        return Err(AL_INVALID);
    }
    Ok((reader, index))
}

#[unsafe(no_mangle)]
/// Publish one current attribute name as a borrowed input view.
///
/// # Safety
///
/// `reader` must name a live reader shell and `out` must name writable, aligned `AlignStr`
/// storage disjoint from both the shell and its input allocation.
pub unsafe extern "C" fn align_rt_xml_attribute_name(
    reader: *const XmlReader,
    out: *mut AlignStr,
    index: i64,
) -> i32 {
    let Ok((shell, index)) = (unsafe { attribute_shell(reader, out, index) }) else {
        return AL_INVALID;
    };
    let shell = unsafe { &*shell };
    let span = shell.attrs[index].name;
    let Ok(len) = i64::try_from(span.end - span.start) else {
        return AL_INVALID;
    };
    unsafe {
        out.write(AlignStr {
            ptr: shell.input.add(span.start),
            len,
        })
    };
    0
}

#[unsafe(no_mangle)]
/// Decode one current attribute value into a newly allocated owned string.
///
/// # Safety
///
/// `reader` must name a live reader shell and `out` must name writable, aligned `AlignStr`
/// storage disjoint from both the shell and its input allocation.
pub unsafe extern "C" fn align_rt_xml_attribute_value(
    reader: *const XmlReader,
    out: *mut AlignStr,
    index: i64,
) -> i32 {
    let Ok((shell, index)) = (unsafe { attribute_shell(reader, out, index) }) else {
        return AL_INVALID;
    };
    let shell = unsafe { &*shell };
    let span = shell.attrs[index].value;
    let bytes =
        unsafe { core::slice::from_raw_parts(shell.input.add(span.start), span.end - span.start) };
    unsafe { publish_owned(bytes, DecodeMode::Attribute, out) }
}

#[unsafe(no_mangle)]
/// Decode the current text event into a newly allocated owned string.
///
/// # Safety
///
/// `reader` must name a live reader shell and `out` must name writable, aligned `AlignStr`
/// storage disjoint from both the shell and its input allocation.
pub unsafe extern "C" fn align_rt_xml_text(reader: *const XmlReader, out: *mut AlignStr) -> i32 {
    let Some(shell) = (unsafe { checked_shell(reader, out) }) else {
        return AL_INVALID;
    };
    let shell = unsafe { &*shell };
    unsafe {
        out.write(AlignStr {
            ptr: ptr::null(),
            len: 0,
        })
    };
    if shell.current != CURRENT_TEXT {
        return AL_INVALID;
    }
    let span = shell.current_text;
    let bytes =
        unsafe { core::slice::from_raw_parts(shell.input.add(span.start), span.end - span.start) };
    let mode = if shell.text_kind == TEXT_CDATA {
        DecodeMode::Cdata
    } else {
        DecodeMode::Ordinary
    };
    unsafe { publish_owned(bytes, mode, out) }
}

#[unsafe(no_mangle)]
/// Free one XML reader and its consumed source allocation.
///
/// # Safety
///
/// `reader` must be null or name a live shell returned by [`align_rt_xml_parse`] that has not
/// already been freed.
pub unsafe extern "C" fn align_rt_xml_free(reader: *mut XmlReader) {
    if reader.is_null() {
        return;
    }
    let Some(shell) = (unsafe { checked_shell_mut(reader) }) else {
        super::panic_abort("xml.reader: malformed private shell");
    };
    let shell = unsafe { &mut *shell };
    let input = shell.input;
    shell.magic = 0;
    unsafe {
        align_rt_free(input);
        align_rt_free(reader.cast::<u8>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn owned(bytes: &[u8]) -> *mut u8 {
        if bytes.is_empty() {
            return ptr::null_mut();
        }
        let allocation = align_rt_alloc(i64::try_from(bytes.len()).unwrap());
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), allocation, bytes.len()) };
        allocation
    }

    unsafe fn parse(bytes: &[u8]) -> Result<*mut XmlReader, i32> {
        let input = unsafe { owned(bytes) };
        let mut reader = ptr::null_mut();
        let status = unsafe { align_rt_xml_parse(input, bytes.len() as i64, &mut reader) };
        if status == 0 { Ok(reader) } else { Err(status) }
    }

    unsafe fn borrowed_view(
        reader: *const XmlReader,
        f: unsafe extern "C" fn(*const XmlReader, *mut AlignStr) -> i32,
    ) -> String {
        let mut out = AlignStr {
            ptr: ptr::null(),
            len: 0,
        };
        assert_eq!(unsafe { f(reader, &mut out) }, 0);
        let bytes = unsafe { core::slice::from_raw_parts(out.ptr, out.len as usize) };
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    unsafe fn owned_view(
        reader: *const XmlReader,
        f: unsafe extern "C" fn(*const XmlReader, *mut AlignStr) -> i32,
    ) -> String {
        let mut out = AlignStr {
            ptr: ptr::null(),
            len: 0,
        };
        assert_eq!(unsafe { f(reader, &mut out) }, 0);
        let bytes = if out.len == 0 {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(out.ptr, out.len as usize) }
        };
        let value = String::from_utf8(bytes.to_vec()).unwrap();
        unsafe { align_rt_free(out.ptr as *mut u8) };
        value
    }

    unsafe fn owned_value(reader: *const XmlReader, index: i64) -> String {
        let mut out = AlignStr {
            ptr: ptr::null(),
            len: 0,
        };
        assert_eq!(
            unsafe { align_rt_xml_attribute_value(reader, &mut out, index) },
            0
        );
        let bytes = if out.len == 0 {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(out.ptr, out.len as usize) }
        };
        let value = String::from_utf8(bytes.to_vec()).unwrap();
        unsafe { align_rt_free(out.ptr as *mut u8) };
        value
    }

    #[test]
    fn document_profile_and_event_stream_are_exact() {
        unsafe {
            let reader = parse(b"\xEF\xBB\xBF<?xml version='1.0' encoding='utf-8' standalone='yes'?><!--p--><a x='1\r\n&amp;&#13;'>left&amp;y&#33;<!--g--><![CDATA[<&\r]]><b/>right</a>").unwrap();
            assert_eq!(align_rt_xml_next(reader), 1);
            assert_eq!(borrowed_view(reader, align_rt_xml_name), "a");
            assert_eq!(align_rt_xml_attribute_count(reader), 1);
            let mut attr_name = AlignStr {
                ptr: ptr::null(),
                len: 0,
            };
            assert_eq!(align_rt_xml_attribute_name(reader, &mut attr_name, 0), 0);
            assert_eq!(
                core::slice::from_raw_parts(attr_name.ptr, attr_name.len as usize),
                b"x"
            );
            assert_eq!(owned_value(reader, 0), "1 &\r");
            assert_eq!(align_rt_xml_next(reader), 3);
            assert_eq!(owned_view(reader, align_rt_xml_text), "left&y!");
            assert_eq!(align_rt_xml_next(reader), 3);
            assert_eq!(owned_view(reader, align_rt_xml_text), "<&\n");
            assert_eq!(align_rt_xml_next(reader), 1);
            assert_eq!(borrowed_view(reader, align_rt_xml_name), "b");
            assert_eq!(align_rt_xml_next(reader), 2);
            assert_eq!(borrowed_view(reader, align_rt_xml_name), "b");
            assert_eq!(align_rt_xml_next(reader), 3);
            assert_eq!(owned_view(reader, align_rt_xml_text), "right");
            assert_eq!(align_rt_xml_next(reader), 2);
            assert_eq!(borrowed_view(reader, align_rt_xml_name), "a");
            assert_eq!(align_rt_xml_next(reader), 0);
            assert_eq!(align_rt_xml_next(reader), 0);
            align_rt_xml_free(reader);
        }
    }

    #[test]
    fn unicode_names_and_text_are_preserved() {
        unsafe {
            let reader = parse("<要素 属性='値'>雪&amp;☃</要素>".as_bytes()).unwrap();
            assert_eq!(align_rt_xml_next(reader), 1);
            assert_eq!(borrowed_view(reader, align_rt_xml_name), "要素");
            assert_eq!(align_rt_xml_next(reader), 3);
            assert_eq!(owned_view(reader, align_rt_xml_text), "雪&☃");
            align_rt_xml_free(reader);
        }
    }

    #[test]
    fn invalid_profile_matrix_rejects_before_publication() {
        for source in [
            "",
            "text",
            "<a>",
            "<a></b>",
            "<a/><b/>",
            "<a x='1' x='2'/>",
            "<a>&custom;</a>",
            "<a>&#0;</a>",
            "<a>]]></a>",
            "<a><!-- a--b --></a>",
            "<!DOCTYPE a><a/>",
            "<?pi x?><a/>",
            "<?xml version='1.1'?><a/>",
            "<?xml encoding='UTF-8' version='1.0'?><a/>",
            "<?xml version='1.0' encoding='UTF-16'?><a/>",
            "<?xml version='1.0' standalone='maybe'?><a/>",
            "<?xml version='1.0' standalone='yes' encoding='UTF-8'?><a/>",
            "<?xml version='1.0' extra='x'?><a/>",
            "<a><?xml version='1.0'?></a>",
            "<a><!--ends-with---></a>",
            "<a x='&missing;'/>",
            "<a x='&#x110000;'/>",
            "<a\u{b}/>",
        ] {
            unsafe { assert_eq!(parse(source.as_bytes()), Err(-1), "{source:?}") };
        }
    }

    #[test]
    fn declaration_name_reference_and_cloud_response_profiles_are_accepted() {
        for source in [
            "<?xml version = \"1.0\"?><a/>",
            "\u{feff}<?xml version='1.0' encoding='UtF-8' standalone='no'?><a/>",
            "<\u{c0}\u{37f}\u{200c}\u{10000} a-b.c='&#9;&#xA;&#13;&quot;&apos;&lt;&gt;&amp;'/>",
            "<ListBucketResult xmlns='http://s3.amazonaws.com/doc/2006-03-01/'><Name>bucket</Name><KeyCount>1</KeyCount><Contents><Key>a&amp;b</Key></Contents></ListBucketResult>",
            "<EnumerationResults ServiceEndpoint='https://example.blob.core.windows.net/' ContainerName='c'><Blobs><Blob><Name>a</Name></Blob></Blobs><NextMarker /></EnumerationResults>",
        ] {
            unsafe {
                let reader = parse(source.as_bytes()).unwrap_or_else(|status| {
                    panic!("valid profile rejected with {status}: {source:?}")
                });
                align_rt_xml_free(reader);
            }
        }
    }

    #[test]
    fn parse_shape_rejection_never_takes_ownership() {
        unsafe {
            let mut sentinel = 1usize as *mut XmlReader;
            let mut byte = 0u8;
            assert_eq!(
                align_rt_xml_parse(ptr::null_mut(), -1, &mut sentinel),
                AL_INVALID
            );
            assert_eq!(sentinel.addr(), 1);
            assert_eq!(
                align_rt_xml_parse(ptr::null_mut(), 1, &mut sentinel),
                AL_INVALID
            );
            assert_eq!(sentinel.addr(), 1);
            assert_eq!(
                align_rt_xml_parse(usize::MAX as *mut u8, 2, &mut sentinel),
                AL_INVALID
            );
            assert_eq!(sentinel.addr(), 1);
            assert_eq!(align_rt_xml_parse(&mut byte, 0, &mut sentinel), AL_INVALID);
            assert_eq!(sentinel.addr(), 1);
            let alias = (&mut sentinel as *mut *mut XmlReader).cast::<u8>();
            assert_eq!(align_rt_xml_parse(alias, 1, &mut sentinel), AL_INVALID);
            assert_eq!(sentinel.addr(), 1);
            assert_eq!(
                align_rt_xml_parse(ptr::null_mut(), 0, ptr::null_mut()),
                AL_INVALID
            );
            assert_eq!(
                align_rt_xml_parse(ptr::null_mut(), 0, 1usize as *mut *mut XmlReader),
                AL_INVALID
            );
            assert_eq!(align_rt_xml_parse(ptr::null_mut(), 0, &mut sentinel), -1);
            assert!(sentinel.is_null());
        }
    }

    #[test]
    fn getter_output_phases_are_distinct() {
        unsafe {
            let reader = parse(b"<a attribute='long-enough-for-output-alias'/>").unwrap();
            let mut out = AlignStr {
                ptr: 1usize as *const u8,
                len: 7,
            };
            assert_eq!(align_rt_xml_name(reader, &mut out), AL_INVALID);
            assert!(out.ptr.is_null());
            assert_eq!(out.len, 0);

            (*reader).magic = 0;
            out = AlignStr {
                ptr: 2usize as *const u8,
                len: 9,
            };
            assert_eq!(align_rt_xml_name(reader, &mut out), AL_INVALID);
            assert_eq!(out.ptr.addr(), 2);
            assert_eq!(out.len, 9);
            (*reader).magic = XML_MAGIC;

            macro_rules! malformed_field {
                ($field:ident, $bad:expr) => {{
                    let saved = (*reader).$field;
                    (*reader).$field = $bad;
                    out = AlignStr {
                        ptr: 2usize as *const u8,
                        len: 9,
                    };
                    assert_eq!(align_rt_xml_name(reader, &mut out), AL_INVALID);
                    assert_eq!((out.ptr.addr(), out.len), (2, 9));
                    (*reader).$field = saved;
                }};
            }
            malformed_field!(len, 0);
            malformed_field!(cursor, (*reader).len + 1);
            malformed_field!(depth, MAX_DEPTH + 1);
            malformed_field!(attr_count, MAX_ATTRIBUTES + 1);
            malformed_field!(current, CURRENT_TEXT + 1);
            malformed_field!(text_kind, TEXT_CDATA + 1);
            malformed_field!(pending_end, 2);
            malformed_field!(seen_root, 2);
            malformed_field!(root_done, 2);

            let input = (*reader).input;
            let len = (*reader).len;
            (*reader).input = reader.cast::<u8>();
            (*reader).len = 1;
            out = AlignStr {
                ptr: 4usize as *const u8,
                len: 13,
            };
            assert_eq!(align_rt_xml_name(reader, &mut out), AL_INVALID);
            assert_eq!((out.ptr.addr(), out.len), (4, 13));
            (*reader).input = input;
            (*reader).len = len;

            let input_before = core::slice::from_raw_parts(input, len).to_vec();
            assert_eq!(
                align_rt_xml_name(reader, input.cast::<AlignStr>()),
                AL_INVALID
            );
            assert_eq!(core::slice::from_raw_parts(input, len), input_before);

            assert_eq!(align_rt_xml_next(reader), 1);
            (*reader).current_name = Span { start: 1, end: 99 };
            out = AlignStr {
                ptr: 3usize as *const u8,
                len: 11,
            };
            assert_eq!(align_rt_xml_name(reader, &mut out), AL_INVALID);
            assert_eq!(out.ptr.addr(), 3);
            assert_eq!(out.len, 11);
            (*reader).current_name = Span { start: 1, end: 2 };

            let pending_end = (*reader).pending_end;
            let pending_name = (*reader).pending_name;
            (*reader).pending_end = 1;
            (*reader).pending_name = Span { start: 2, end: 3 };
            out = AlignStr {
                ptr: 5usize as *const u8,
                len: 15,
            };
            assert_eq!(align_rt_xml_name(reader, &mut out), AL_INVALID);
            assert_eq!((out.ptr.addr(), out.len), (5, 15));
            (*reader).pending_end = pending_end;
            (*reader).pending_name = pending_name;

            let alias = reader.cast::<AlignStr>();
            assert_eq!(align_rt_xml_name(reader, alias), AL_INVALID);
            assert_eq!((*reader).magic, XML_MAGIC);

            out = AlignStr {
                ptr: 6usize as *const u8,
                len: 17,
            };
            assert_eq!(
                align_rt_xml_attribute_name(reader, &mut out, -1),
                AL_INVALID
            );
            assert!(out.ptr.is_null());
            assert_eq!(out.len, 0);
            align_rt_xml_free(reader);
        }
    }

    #[test]
    fn malformed_utf8_cursor_boundary_is_rejected_without_unchecked_decoding() {
        unsafe {
            let reader = parse("<a>雪</a>".as_bytes()).unwrap();
            assert_eq!(align_rt_xml_next(reader), 1);
            (*reader).cursor += 1;
            assert_eq!(align_rt_xml_next(reader), -1);
            (*reader).cursor -= 1;
            align_rt_xml_free(reader);
        }
    }

    #[test]
    fn in_range_private_state_mutations_are_rejected_relationally() {
        unsafe {
            let reader = parse(b"<a x='v'/>").unwrap();

            macro_rules! rejects_initial_mutation {
                ($field:ident, $bad:expr) => {{
                    let saved = (*reader).$field;
                    (*reader).$field = $bad;
                    assert!(
                        checked_shell_mut(reader).is_none(),
                        "initial mutation of {} was accepted",
                        stringify!($field)
                    );
                    (*reader).$field = saved;
                }};
            }
            rejects_initial_mutation!(cursor, 1);
            rejects_initial_mutation!(depth, 1);
            rejects_initial_mutation!(attr_count, 1);
            rejects_initial_mutation!(current, CURRENT_START);
            rejects_initial_mutation!(text_kind, TEXT_ORDINARY);
            rejects_initial_mutation!(pending_end, 1);
            rejects_initial_mutation!(seen_root, 1);
            rejects_initial_mutation!(root_done, 1);

            assert_eq!(align_rt_xml_next(reader), 1);
            macro_rules! rejects_start_mutation {
                ($field:ident, $bad:expr) => {{
                    let saved = (*reader).$field;
                    (*reader).$field = $bad;
                    assert!(
                        checked_shell_mut(reader).is_none(),
                        "start-event mutation of {} was accepted",
                        stringify!($field)
                    );
                    (*reader).$field = saved;
                }};
            }
            rejects_start_mutation!(pending_end, 0);
            rejects_start_mutation!(seen_root, 0);
            rejects_start_mutation!(root_done, 1);
            rejects_start_mutation!(current, CURRENT_END);
            rejects_start_mutation!(text_kind, TEXT_ORDINARY);
            rejects_start_mutation!(cursor, (*reader).cursor - 1);
            let hash = (*reader).attrs[0].hash;
            (*reader).attrs[0].hash ^= 1;
            assert!(
                checked_shell_mut(reader).is_none(),
                "an authenticated in-range attribute mutation was accepted"
            );
            (*reader).attrs[0].hash = hash;
            align_rt_xml_free(reader);
        }
    }

    #[test]
    fn cursor_getters_do_not_rescan_historic_names_and_owned_views_take_two_passes() {
        let root = "r".repeat(8192);
        let children = "<b/>".repeat(100);
        let source = format!("<{root} a='&amp;'>{children}</{root}>");
        unsafe {
            let reader = parse(source.as_bytes()).unwrap();
            XML_WORK.with(XmlWork::reset);
            assert_eq!(align_rt_xml_next(reader), 1);
            assert_eq!(
                XML_WORK.with(XmlWork::name_bytes),
                root.len() + 1,
                "the start event scans only its root and one attribute name"
            );

            XML_WORK.with(XmlWork::reset);
            for _ in 0..50 {
                assert_eq!(borrowed_view(reader, align_rt_xml_name), root);
                assert_eq!(align_rt_xml_attribute_count(reader), 1);
                let mut out = AlignStr {
                    ptr: ptr::null(),
                    len: 0,
                };
                assert_eq!(align_rt_xml_attribute_name(reader, &mut out, 0), 0);
            }
            assert_eq!(XML_WORK.with(XmlWork::name_bytes), 0);

            assert_eq!(owned_value(reader, 0), "&");
            assert_eq!(XML_WORK.with(XmlWork::normalize_passes), 2);

            XML_WORK.with(XmlWork::reset);
            for _ in 0..100 {
                assert_eq!(align_rt_xml_next(reader), 1);
                assert_eq!(align_rt_xml_next(reader), 2);
            }
            assert_eq!(
                XML_WORK.with(XmlWork::name_bytes),
                100,
                "only each newly encountered one-byte child name may be scanned"
            );
            align_rt_xml_free(reader);
        }
    }

    #[test]
    fn forced_attribute_hash_collision_confirms_bytes() {
        let bytes = b"a b a";
        let distinct = [
            AttributeSpan {
                name: Span { start: 0, end: 1 },
                hash: 7,
                ..AttributeSpan::default()
            },
            AttributeSpan {
                name: Span { start: 2, end: 3 },
                hash: 7,
                ..AttributeSpan::default()
            },
        ];
        assert!(attributes_unique(bytes, &distinct));
        let duplicate = [
            distinct[0],
            AttributeSpan {
                name: Span { start: 4, end: 5 },
                hash: 7,
                ..AttributeSpan::default()
            },
        ];
        assert!(!attributes_unique(bytes, &duplicate));
    }

    #[test]
    fn depth_and_attribute_limits_are_inclusive() {
        let mut depth_256 = String::new();
        for _ in 0..MAX_DEPTH {
            depth_256.push_str("<a>");
        }
        for _ in 0..MAX_DEPTH {
            depth_256.push_str("</a>");
        }
        let mut depth_257 = String::from("<a>");
        depth_257.push_str(&depth_256);
        depth_257.push_str("</a>");
        unsafe {
            let reader = parse(depth_256.as_bytes()).unwrap();
            align_rt_xml_free(reader);
            assert_eq!(parse(depth_257.as_bytes()), Err(-1));
        }

        let attrs_256 = (0..MAX_ATTRIBUTES)
            .map(|i| format!(" a{i}='x'"))
            .collect::<String>();
        let valid = format!("<a{attrs_256}/>");
        let invalid = format!("<a{attrs_256} overflow='x'/>");
        unsafe {
            let reader = parse(valid.as_bytes()).unwrap();
            align_rt_xml_free(reader);
            assert_eq!(parse(invalid.as_bytes()), Err(-1));
        }
    }

    #[cfg(feature = "alloc-count")]
    #[test]
    fn allocation_ownership_is_exact_for_parse_getters_and_drop() {
        unsafe {
            let alloc_before = super::super::align_rt_alloc_count();
            let free_before = super::super::align_rt_free_count();
            let reader = parse(b"<a empty='' full='x'>text</a>").unwrap();
            assert_eq!(super::super::align_rt_alloc_count() - alloc_before, 2);
            assert_eq!(super::super::align_rt_free_count() - free_before, 0);

            let alloc_before_borrowed = super::super::align_rt_alloc_count();
            let free_before_borrowed = super::super::align_rt_free_count();
            assert_eq!(align_rt_xml_next(reader), 1);
            assert_eq!(borrowed_view(reader, align_rt_xml_name), "a");
            assert_eq!(align_rt_xml_attribute_count(reader), 2);
            let mut attr_name = AlignStr {
                ptr: ptr::null(),
                len: 0,
            };
            assert_eq!(align_rt_xml_attribute_name(reader, &mut attr_name, 0), 0);
            assert_eq!(
                super::super::align_rt_alloc_count() - alloc_before_borrowed,
                0
            );
            assert_eq!(
                super::super::align_rt_free_count() - free_before_borrowed,
                0
            );
            let alloc_before_empty = super::super::align_rt_alloc_count();
            assert_eq!(owned_value(reader, 0), "");
            assert_eq!(super::super::align_rt_alloc_count() - alloc_before_empty, 0);

            let alloc_before_full = super::super::align_rt_alloc_count();
            let free_before_full = super::super::align_rt_free_count();
            assert_eq!(owned_value(reader, 1), "x");
            assert_eq!(super::super::align_rt_alloc_count() - alloc_before_full, 1);
            assert_eq!(super::super::align_rt_free_count() - free_before_full, 1);

            let alloc_before_repeat = super::super::align_rt_alloc_count();
            let free_before_repeat = super::super::align_rt_free_count();
            assert_eq!(owned_value(reader, 1), "x");
            assert_eq!(
                super::super::align_rt_alloc_count() - alloc_before_repeat,
                1
            );
            assert_eq!(super::super::align_rt_free_count() - free_before_repeat, 1);

            assert_eq!(align_rt_xml_next(reader), 3);
            let alloc_before_text = super::super::align_rt_alloc_count();
            let free_before_text = super::super::align_rt_free_count();
            assert_eq!(owned_view(reader, align_rt_xml_text), "text");
            assert_eq!(super::super::align_rt_alloc_count() - alloc_before_text, 1);
            assert_eq!(super::super::align_rt_free_count() - free_before_text, 1);

            let free_before_drop = super::super::align_rt_free_count();
            align_rt_xml_free(reader);
            assert_eq!(super::super::align_rt_free_count() - free_before_drop, 2);

            let alloc_before_invalid = super::super::align_rt_alloc_count();
            let free_before_invalid = super::super::align_rt_free_count();
            assert_eq!(parse(b"<a>"), Err(-1));
            assert_eq!(
                super::super::align_rt_alloc_count() - alloc_before_invalid,
                1
            );
            assert_eq!(super::super::align_rt_free_count() - free_before_invalid, 1);

            let alloc_before_empty_parse = super::super::align_rt_alloc_count();
            let free_before_empty_parse = super::super::align_rt_free_count();
            assert_eq!(parse(b""), Err(-1));
            assert_eq!(
                super::super::align_rt_alloc_count() - alloc_before_empty_parse,
                0
            );
            assert_eq!(
                super::super::align_rt_free_count() - free_before_empty_parse,
                0
            );
        }
    }
}
