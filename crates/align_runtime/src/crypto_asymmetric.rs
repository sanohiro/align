//! Algorithm-specific RS256, ES256, and Ed25519 owners for `std.crypto`.
//!
//! This boundary deliberately uses the OpenSSL 3 provider API directly. Each published key owns
//! an isolated library context and its explicitly loaded default provider; neither key construction
//! nor an operation is allowed to fall back to process-global provider state.

use super::{AL_CODE, AL_INVALID, Buffer, free, malloc};
use core::alloc::Layout;
use core::ffi::{c_char, c_int, c_long, c_ulong, c_void};
use core::mem::{align_of, size_of};
use core::ptr::{self, NonNull};

const PROPQ: &core::ffi::CStr = c"provider=default";
const PEM_MAX: usize = 65_536;
const EVP_PKEY_PUBLIC_KEY: c_int = 0x86;
const RSA_PKCS1_PADDING: c_int = 1;

#[cfg(all(feature = "crypto-asymmetric-probe", not(test)))]
mod probe {
    use super::{AL_INVALID, c_int};
    use core::sync::atomic::{AtomicI64, Ordering};

    pub static LIVE_KEYS: AtomicI64 = AtomicI64::new(0);
    pub static PEAK_KEYS: AtomicI64 = AtomicI64::new(0);
    pub static LIVE_SENSITIVE: AtomicI64 = AtomicI64::new(0);
    pub static PEAK_SENSITIVE: AtomicI64 = AtomicI64::new(0);
    pub static SENSITIVE_CLEANSES: AtomicI64 = AtomicI64::new(0);

    pub fn key_published() {
        let live = LIVE_KEYS.fetch_add(1, Ordering::Relaxed) + 1;
        PEAK_KEYS.fetch_max(live, Ordering::Relaxed);
    }

    pub fn key_freed() {
        LIVE_KEYS.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn sensitive_allocated() {
        let live = LIVE_SENSITIVE.fetch_add(1, Ordering::Relaxed) + 1;
        PEAK_SENSITIVE.fetch_max(live, Ordering::Relaxed);
    }

    pub fn sensitive_cleansed() {
        LIVE_SENSITIVE.fetch_sub(1, Ordering::Relaxed);
        SENSITIVE_CLEANSES.fetch_add(1, Ordering::Relaxed);
    }

    pub fn reset() -> c_int {
        if live_keys() != 0 || live_sensitive() != 0 {
            return AL_INVALID;
        }
        PEAK_KEYS.store(0, Ordering::Relaxed);
        PEAK_SENSITIVE.store(0, Ordering::Relaxed);
        SENSITIVE_CLEANSES.store(0, Ordering::Relaxed);
        0
    }

    pub fn live_keys() -> i64 {
        LIVE_KEYS.load(Ordering::Relaxed)
    }

    pub fn peak_keys() -> i64 {
        PEAK_KEYS.load(Ordering::Relaxed)
    }

    pub fn live_sensitive() -> i64 {
        LIVE_SENSITIVE.load(Ordering::Relaxed)
    }

    pub fn peak_sensitive() -> i64 {
        PEAK_SENSITIVE.load(Ordering::Relaxed)
    }

    pub fn sensitive_cleanses() -> i64 {
        SENSITIVE_CLEANSES.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod probe {
    use super::{AL_INVALID, c_int};
    use core::cell::Cell;

    std::thread_local! {
        static LIVE_KEYS: Cell<i64> = const { Cell::new(0) };
        static PEAK_KEYS: Cell<i64> = const { Cell::new(0) };
        static LIVE_SENSITIVE: Cell<i64> = const { Cell::new(0) };
        static PEAK_SENSITIVE: Cell<i64> = const { Cell::new(0) };
        static SENSITIVE_CLEANSES: Cell<i64> = const { Cell::new(0) };
    }

    fn increment(
        live: &'static std::thread::LocalKey<Cell<i64>>,
        peak: &'static std::thread::LocalKey<Cell<i64>>,
    ) {
        let current = live.with(|value| {
            let current = value.get() + 1;
            value.set(current);
            current
        });
        peak.with(|value| value.set(value.get().max(current)));
    }

    pub fn key_published() {
        increment(&LIVE_KEYS, &PEAK_KEYS);
    }

    pub fn key_freed() {
        LIVE_KEYS.with(|value| value.set(value.get() - 1));
    }

    pub fn sensitive_allocated() {
        increment(&LIVE_SENSITIVE, &PEAK_SENSITIVE);
    }

    pub fn sensitive_cleansed() {
        LIVE_SENSITIVE.with(|value| value.set(value.get() - 1));
        SENSITIVE_CLEANSES.with(|value| value.set(value.get() + 1));
    }

    pub fn reset() -> c_int {
        if live_keys() != 0 || live_sensitive() != 0 {
            return AL_INVALID;
        }
        PEAK_KEYS.with(|value| value.set(0));
        PEAK_SENSITIVE.with(|value| value.set(0));
        SENSITIVE_CLEANSES.with(|value| value.set(0));
        0
    }

    pub fn live_keys() -> i64 {
        LIVE_KEYS.with(Cell::get)
    }

    pub fn peak_keys() -> i64 {
        PEAK_KEYS.with(Cell::get)
    }

    pub fn live_sensitive() -> i64 {
        LIVE_SENSITIVE.with(Cell::get)
    }

    pub fn peak_sensitive() -> i64 {
        PEAK_SENSITIVE.with(Cell::get)
    }

    pub fn sensitive_cleanses() -> i64 {
        SENSITIVE_CLEANSES.with(Cell::get)
    }
}

#[cfg(all(not(test), not(feature = "crypto-asymmetric-probe")))]
mod probe {
    pub fn key_published() {}
    pub fn key_freed() {}
    pub fn sensitive_allocated() {}
    pub fn sensitive_cleansed() {}
}

#[cfg(test)]
std::thread_local! {
    static ALLOCATION_FAIL_AFTER: core::cell::Cell<Option<usize>> = const { core::cell::Cell::new(None) };
    static ALLOCATION_FAIL_TRIGGERED: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

#[cfg(test)]
fn set_allocation_failpoint(after: Option<usize>) {
    ALLOCATION_FAIL_AFTER.with(|value| value.set(after));
    ALLOCATION_FAIL_TRIGGERED.with(|value| value.set(false));
}

#[cfg(test)]
fn allocation_failpoint() -> Result<(), c_int> {
    ALLOCATION_FAIL_AFTER.with(|value| match value.get() {
        Some(0) => {
            value.set(None);
            ALLOCATION_FAIL_TRIGGERED.with(|triggered| triggered.set(true));
            Err(AL_CODE)
        }
        Some(remaining) => {
            value.set(Some(remaining - 1));
            Ok(())
        }
        None => Ok(()),
    })
}

#[cfg(test)]
fn allocation_failpoint_triggered() -> bool {
    ALLOCATION_FAIL_TRIGGERED.with(core::cell::Cell::get)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum Algorithm {
    Rs256 = 0,
    Es256 = 1,
    Ed25519 = 2,
}

impl Algorithm {
    fn parse(value: c_int) -> Option<Self> {
        match value {
            0 => Some(Self::Rs256),
            1 => Some(Self::Es256),
            2 => Some(Self::Ed25519),
            _ => None,
        }
    }

    const fn key_name(self) -> &'static core::ffi::CStr {
        match self {
            Self::Rs256 => c"RSA",
            Self::Es256 => c"EC",
            Self::Ed25519 => c"ED25519",
        }
    }

    const fn private_kind(self) -> KeyKind {
        match self {
            Self::Rs256 => KeyKind::Rs256Private,
            Self::Es256 => KeyKind::Es256Private,
            Self::Ed25519 => KeyKind::Ed25519Private,
        }
    }

    const fn public_kind(self) -> KeyKind {
        match self {
            Self::Rs256 => KeyKind::Rs256Public,
            Self::Es256 => KeyKind::Es256Public,
            Self::Ed25519 => KeyKind::Ed25519Public,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
enum KeyKind {
    Rs256Private = 0,
    Rs256Public = 1,
    Es256Private = 2,
    Es256Public = 3,
    Ed25519Private = 4,
    Ed25519Public = 5,
}

#[repr(C)]
pub struct CryptoKey {
    // Keep the repeated ABI tag as raw storage. Reading an invalid discriminant through a Rust
    // enum would itself be undefined behavior before the boundary could reject a future/corrupt
    // value; comparison against the closed `KeyKind` byte keeps malformed shells fail-closed.
    kind: u8,
    libctx: *mut c_void,
    provider: *mut c_void,
    pkey: *mut c_void,
}

struct KeyParts {
    libctx: *mut c_void,
    provider: *mut c_void,
    pkey: *mut c_void,
}

impl KeyParts {
    fn new() -> Result<Self, c_int> {
        #[cfg(test)]
        allocation_failpoint()?;
        clear_errors();
        let libctx = unsafe { OSSL_LIB_CTX_new() };
        let _libctx_errors = drain_errors();
        if libctx.is_null() {
            return Err(AL_CODE);
        }
        #[cfg(test)]
        if let Err(status) = allocation_failpoint() {
            unsafe {
                OPENSSL_thread_stop_ex(libctx);
                OSSL_LIB_CTX_free(libctx);
            }
            return Err(status);
        }
        clear_errors();
        let provider = unsafe { OSSL_PROVIDER_load(libctx, c"default".as_ptr()) };
        let _provider_errors = drain_errors();
        if provider.is_null() {
            unsafe {
                OPENSSL_thread_stop_ex(libctx);
                OSSL_LIB_CTX_free(libctx);
            }
            return Err(AL_CODE);
        }
        Ok(Self {
            libctx,
            provider,
            pkey: ptr::null_mut(),
        })
    }

    fn publish(mut self, kind: KeyKind, out: *mut *mut CryptoKey) -> c_int {
        if self.pkey.is_null() {
            return AL_CODE;
        }
        #[cfg(test)]
        if let Err(status) = allocation_failpoint() {
            return status;
        }
        let storage = unsafe { malloc(size_of::<CryptoKey>()) }.cast::<CryptoKey>();
        if storage.is_null() {
            return AL_CODE;
        }
        unsafe {
            storage.write(CryptoKey {
                kind: kind as u8,
                libctx: self.libctx,
                provider: self.provider,
                pkey: self.pkey,
            });
            *out = storage;
        }
        self.libctx = ptr::null_mut();
        self.provider = ptr::null_mut();
        self.pkey = ptr::null_mut();
        probe::key_published();
        0
    }
}

impl Drop for KeyParts {
    fn drop(&mut self) {
        unsafe {
            EVP_PKEY_free(self.pkey);
            if !self.libctx.is_null() {
                OPENSSL_thread_stop_ex(self.libctx);
            }
            unload_provider(self.provider);
            OSSL_LIB_CTX_free(self.libctx);
        }
    }
}

struct SensitiveDer {
    ptr: NonNull<u8>,
    len: usize,
}

impl SensitiveDer {
    fn new(len: usize) -> Result<Self, c_int> {
        if len == 0 {
            return Err(AL_INVALID);
        }
        #[cfg(test)]
        allocation_failpoint()?;
        clear_errors();
        let storage = unsafe { CRYPTO_malloc(len, c"align_runtime".as_ptr(), 0) };
        let _errors = drain_errors();
        let ptr = NonNull::new(storage.cast::<u8>()).ok_or(AL_CODE)?;
        probe::sensitive_allocated();
        Ok(Self { ptr, len })
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for SensitiveDer {
    fn drop(&mut self) {
        unsafe {
            CRYPTO_clear_free(
                self.ptr.as_ptr().cast(),
                self.len,
                c"align_runtime".as_ptr(),
                0,
            )
        };
        probe::sensitive_cleansed();
    }
}

fn output_slot<T>(out: *mut T) -> Option<()> {
    (!out.is_null() && (out as usize) % align_of::<T>() == 0).then_some(())
}

fn input_view<'a>(input: *const u8, len: i64) -> Result<&'a [u8], c_int> {
    let len = usize::try_from(len)
        .ok()
        .filter(|len| *len <= isize::MAX as usize)
        .ok_or(AL_INVALID)?;
    if len == 0 {
        return Ok(&[]);
    }
    if input.is_null() {
        return Err(AL_INVALID);
    }
    Ok(unsafe { core::slice::from_raw_parts(input, len) })
}

unsafe fn checked_key<'a>(key: *mut CryptoKey, expected: KeyKind) -> Result<&'a CryptoKey, c_int> {
    if key.is_null() || (key as usize) % align_of::<CryptoKey>() != 0 {
        return Err(AL_INVALID);
    }
    let key = unsafe { &*key };
    if key.kind != expected as u8
        || key.libctx.is_null()
        || key.provider.is_null()
        || key.pkey.is_null()
    {
        return Err(AL_INVALID);
    }
    Ok(key)
}

fn clear_errors() {
    unsafe { ERR_clear_error() };
}

fn unload_provider(provider: *mut c_void) {
    if provider.is_null() {
        return;
    }
    clear_errors();
    let _status = unsafe { OSSL_PROVIDER_unload(provider) };
    let _errors = drain_errors();
}

#[derive(Clone, Copy)]
struct ErrorQueue {
    empty: bool,
    decoder_input_only: bool,
    import_input_only: bool,
    verify_mismatch_only: bool,
}

fn drain_errors() -> ErrorQueue {
    let mut queue = ErrorQueue {
        empty: true,
        decoder_input_only: true,
        import_input_only: true,
        verify_mismatch_only: true,
    };
    loop {
        let error = unsafe { ERR_get_error() };
        if error == 0 {
            break;
        }
        queue.empty = false;
        let parts = error_parts(error);
        queue.decoder_input_only &=
            parts.is_some_and(|(lib, reason)| lib == ERR_LIB_ASN1 && asn1_input_reason(reason));
        queue.import_input_only &=
            parts.is_some_and(|(lib, reason)| import_input_reason(lib, reason));
        queue.verify_mismatch_only &=
            parts.is_some_and(|(lib, reason)| verify_mismatch_reason(lib, reason));
    }
    unsafe { ERR_clear_error() };
    queue
}

fn b64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn decode_base64(lines: &[&[u8]]) -> Result<SensitiveDer, c_int> {
    let chars = lines.iter().try_fold(0usize, |sum, line| {
        sum.checked_add(line.len()).ok_or(AL_CODE)
    })?;
    if chars == 0 || chars % 4 != 0 {
        return Err(AL_INVALID);
    }
    let last = *lines.last().ok_or(AL_INVALID)?;
    let padding = if last.ends_with(b"==") {
        2
    } else if last.ends_with(b"=") {
        1
    } else {
        0
    };
    let decoded = chars
        .checked_div(4)
        .and_then(|n| n.checked_mul(3))
        .and_then(|n| n.checked_sub(padding))
        .ok_or(AL_CODE)?;
    let out = SensitiveDer::new(decoded)?;
    let mut quartet = [0u8; 4];
    let mut qlen = 0usize;
    let mut written = 0usize;
    let mut position = 0usize;
    for line in lines {
        for &byte in *line {
            let final_two = position + 2 >= chars;
            let in_padding = position >= chars - padding;
            quartet[qlen] = match b64_value(byte) {
                Some(value) if !in_padding => value,
                None if byte == b'=' && final_two && in_padding => 0,
                None => return Err(AL_INVALID),
                Some(_) => return Err(AL_INVALID),
            };
            qlen += 1;
            position += 1;
            if qlen == 4 {
                let block = [
                    (quartet[0] << 2) | (quartet[1] >> 4),
                    (quartet[1] << 4) | (quartet[2] >> 2),
                    (quartet[2] << 6) | quartet[3],
                ];
                let remaining = decoded.saturating_sub(written).min(3);
                unsafe {
                    ptr::copy_nonoverlapping(
                        block.as_ptr(),
                        out.ptr.as_ptr().add(written),
                        remaining,
                    )
                };
                written += remaining;
                qlen = 0;
            }
        }
    }
    let canonical_padding = match padding {
        0 => true,
        1 => {
            last[last.len() - 1] == b'='
                && b64_value(last[last.len() - 2]).is_some_and(|v| v & 0x03 == 0)
        }
        2 => {
            last[last.len() - 2..] == *b"=="
                && b64_value(last[last.len() - 3]).is_some_and(|v| v & 0x0f == 0)
        }
        _ => false,
    };
    if !canonical_padding || written != decoded {
        return Err(AL_INVALID);
    }
    Ok(out)
}

fn strip_line<'a>(input: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], c_int> {
    let start = *cursor;
    let rel = input[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or(AL_INVALID)?;
    let end = start + rel;
    *cursor = end + 1;
    if end > start && input[end - 1] == b'\r' {
        Ok(&input[start..end - 1])
    } else {
        Ok(&input[start..end])
    }
}

fn parse_pem(input: &[u8], private: bool) -> Result<SensitiveDer, c_int> {
    if input.is_empty() || input.len() > PEM_MAX || input.contains(&0) {
        return Err(AL_INVALID);
    }
    let begin = if private {
        b"-----BEGIN PRIVATE KEY-----".as_slice()
    } else {
        b"-----BEGIN PUBLIC KEY-----".as_slice()
    };
    let end = if private {
        b"-----END PRIVATE KEY-----".as_slice()
    } else {
        b"-----END PUBLIC KEY-----".as_slice()
    };
    let mut cursor = 0usize;
    if strip_line(input, &mut cursor)? != begin {
        return Err(AL_INVALID);
    }
    let mut lines: [&[u8]; 1024] = [&[]; 1024];
    let mut line_count = 0usize;
    loop {
        let line_start = cursor;
        let line = match strip_line(input, &mut cursor) {
            Ok(line) => line,
            Err(_) if input[line_start..] == *end => {
                cursor = input.len();
                &input[line_start..]
            }
            Err(status) => return Err(status),
        };
        if line == end {
            break;
        }
        if line.is_empty() || line.len() > 64 || line.len() % 4 != 0 {
            return Err(AL_INVALID);
        }
        if line_count == lines.len() {
            return Err(AL_INVALID);
        }
        lines[line_count] = line;
        line_count += 1;
    }
    if cursor != input.len() {
        return Err(AL_INVALID);
    }
    let lines = &lines[..line_count];
    if lines.is_empty() || lines[..lines.len() - 1].iter().any(|line| line.len() != 64) {
        return Err(AL_INVALID);
    }
    let Some(last) = lines.last() else {
        return Err(AL_INVALID);
    };
    if !(4..=64).contains(&last.len()) {
        return Err(AL_INVALID);
    }
    decode_base64(&lines)
}

struct DerCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> DerCursor<'a> {
    fn tlv(&mut self, expected_tag: u8) -> Result<&'a [u8], c_int> {
        if self.bytes.get(self.pos) != Some(&expected_tag) {
            return Err(AL_INVALID);
        }
        self.pos += 1;
        let first = *self.bytes.get(self.pos).ok_or(AL_INVALID)?;
        self.pos += 1;
        let len = if first < 0x80 {
            usize::from(first)
        } else {
            let count = usize::from(first & 0x7f);
            if count == 0
                || count > size_of::<usize>()
                || self.pos + count > self.bytes.len()
                || self.bytes[self.pos] == 0
            {
                return Err(AL_INVALID);
            }
            let mut value = 0usize;
            for byte in &self.bytes[self.pos..self.pos + count] {
                value = value
                    .checked_mul(256)
                    .and_then(|n| n.checked_add(usize::from(*byte)))
                    .ok_or(AL_INVALID)?;
            }
            self.pos += count;
            if value < 128 {
                return Err(AL_INVALID);
            }
            value
        };
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(AL_INVALID)?;
        let content = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(content)
    }
}

const RSA_ALG: &[u8] = b"\x30\x0d\x06\x09\x2a\x86\x48\x86\xf7\x0d\x01\x01\x01\x05\x00";
const EC_ALG: &[u8] =
    b"\x30\x13\x06\x07\x2a\x86\x48\xce\x3d\x02\x01\x06\x08\x2a\x86\x48\xce\x3d\x03\x01\x07";
const ED_ALG: &[u8] = b"\x30\x05\x06\x03\x2b\x65\x70";

fn expected_alg(algorithm: Algorithm) -> &'static [u8] {
    match algorithm {
        Algorithm::Rs256 => RSA_ALG,
        Algorithm::Es256 => EC_ALG,
        Algorithm::Ed25519 => ED_ALG,
    }
}

fn validate_der_envelope(der: &[u8], algorithm: Algorithm, private: bool) -> Result<(), c_int> {
    let mut outer = DerCursor { bytes: der, pos: 0 };
    let sequence = outer.tlv(0x30)?;
    if outer.pos != der.len() {
        return Err(AL_INVALID);
    }
    let mut inner = DerCursor {
        bytes: sequence,
        pos: 0,
    };
    if private && inner.tlv(0x02)? != [0] {
        return Err(AL_INVALID);
    }
    let alg_start = inner.pos;
    let _ = inner.tlv(0x30)?;
    if &sequence[alg_start..inner.pos] != expected_alg(algorithm) {
        return Err(AL_INVALID);
    }
    if private {
        let _ = inner.tlv(0x04)?;
        if inner.pos < sequence.len() {
            let _ = inner.tlv(0xa0)?;
        }
    } else {
        let bit_string = inner.tlv(0x03)?;
        if bit_string.first() != Some(&0) {
            return Err(AL_INVALID);
        }
    }
    (inner.pos == sequence.len())
        .then_some(())
        .ok_or(AL_INVALID)
}

const ERR_SYSTEM_FLAG: c_ulong = 1 << 31;
const ERR_LIB_OFFSET: c_ulong = 23;
const ERR_LIB_MASK: c_ulong = 0xff;
const ERR_REASON_MASK: c_ulong = 0x7f_ffff;
const ERR_RFLAG_FATAL: c_ulong = 1 << 18;
const ERR_RFLAG_COMMON: c_ulong = 2 << 18;
const ERR_LIB_RSA: c_ulong = 4;
const ERR_LIB_EVP: c_ulong = 6;
const ERR_LIB_ASN1: c_ulong = 13;
const ERR_LIB_EC: c_ulong = 16;
const ERR_LIB_PROV: c_ulong = 57;

// OpenSSL exposes these as C preprocessor constants rather than linkable symbols. Keep the Rust
// names identical to the installed OpenSSL 3 headers so classifier changes are reviewed by reason,
// never by an unexplained numeric range.
const ASN1_R_BAD_OBJECT_HEADER: c_ulong = 102;
const ASN1_R_DATA_IS_WRONG: c_ulong = 109;
const ASN1_R_DECODE_ERROR: c_ulong = 110;
const ASN1_R_EXPECTING_AN_INTEGER: c_ulong = 115;
const ASN1_R_EXPECTING_AN_OBJECT: c_ulong = 116;
const ASN1_R_EXPLICIT_LENGTH_MISMATCH: c_ulong = 119;
const ASN1_R_EXPLICIT_TAG_NOT_CONSTRUCTED: c_ulong = 120;
const ASN1_R_FIELD_MISSING: c_ulong = 121;
const ASN1_R_HEADER_TOO_LONG: c_ulong = 123;
const ASN1_R_ILLEGAL_NULL: c_ulong = 125;
const ASN1_R_INTEGER_TOO_LARGE_FOR_LONG: c_ulong = 128;
const ASN1_R_MISSING_EOC: c_ulong = 137;
const ASN1_R_NOT_ENOUGH_DATA: c_ulong = 142;
const ASN1_R_NULL_IS_WRONG_LENGTH: c_ulong = 144;
const ASN1_R_SEQUENCE_LENGTH_MISMATCH: c_ulong = 148;
const ASN1_R_SEQUENCE_NOT_CONSTRUCTED: c_ulong = 149;
const ASN1_R_TOO_LONG: c_ulong = 155;
const ASN1_R_TYPE_NOT_CONSTRUCTED: c_ulong = 156;
const ASN1_R_UNEXPECTED_EOC: c_ulong = 159;
const ASN1_R_UNKNOWN_OBJECT_TYPE: c_ulong = 162;
const ASN1_R_WRONG_TAG: c_ulong = 168;
const ASN1_R_ILLEGAL_BITSTRING_FORMAT: c_ulong = 175;
const ASN1_R_ILLEGAL_INTEGER: c_ulong = 180;
const ASN1_R_INVALID_OBJECT_ENCODING: c_ulong = 216;
const ASN1_R_INVALID_BIT_STRING_BITS_LEFT: c_ulong = 220;
const ASN1_R_ILLEGAL_PADDING: c_ulong = 221;
const ASN1_R_ILLEGAL_ZERO_CONTENT: c_ulong = 222;
const ASN1_R_TOO_LARGE: c_ulong = 223;
const ASN1_R_TOO_SMALL: c_ulong = 224;
const ASN1_R_WRONG_INTEGER_TYPE: c_ulong = 225;
const ASN1_R_ILLEGAL_NEGATIVE_VALUE: c_ulong = 226;
const ASN1_R_BAD_TEMPLATE: c_ulong = 230;
const ASN1_R_LENGTH_TOO_LONG: c_ulong = 231;
const ERR_R_NESTED_ASN1_ERROR: c_ulong = 266 | ERR_RFLAG_COMMON;
const ERR_R_MISSING_ASN1_EOS: c_ulong = 267 | ERR_RFLAG_COMMON;

const EVP_R_DECODE_ERROR: c_ulong = 114;
const EVP_R_INVALID_KEY_LENGTH: c_ulong = 130;
const EVP_R_PRIVATE_KEY_DECODE_ERROR: c_ulong = 145;
const EVP_R_INVALID_KEY: c_ulong = 163;
const EVP_R_INVALID_SEED_LENGTH: c_ulong = 220;
const EVP_R_PROVIDER_SIGNATURE_FAILURE: c_ulong = 234;

const PROV_R_INVALID_KEY_LENGTH: c_ulong = 105;
const PROV_R_INVALID_DATA: c_ulong = 115;
const PROV_R_BAD_ENCODING: c_ulong = 141;
const PROV_R_BAD_LENGTH: c_ulong = 142;
const PROV_R_INVALID_SEED_LENGTH: c_ulong = 154;
const PROV_R_INVALID_KEY: c_ulong = 158;
const PROV_R_INVALID_SIGNATURE_SIZE: c_ulong = 179;

const RSA_R_ALGORITHM_MISMATCH: c_ulong = 100;
const RSA_R_BAD_E_VALUE: c_ulong = 101;
const RSA_R_BAD_PAD_BYTE_COUNT: c_ulong = 103;
const RSA_R_BAD_SIGNATURE: c_ulong = 104;
const RSA_R_BLOCK_TYPE_IS_NOT_01: c_ulong = 106;
const RSA_R_BLOCK_TYPE_IS_NOT_02: c_ulong = 107;
const RSA_R_DATA_GREATER_THAN_MOD_LEN: c_ulong = 108;
const RSA_R_DATA_TOO_LARGE: c_ulong = 109;
const RSA_R_DATA_TOO_LARGE_FOR_KEY_SIZE: c_ulong = 110;
const RSA_R_DATA_TOO_SMALL: c_ulong = 111;
const RSA_R_DIGEST_TOO_BIG_FOR_RSA_KEY: c_ulong = 112;
const RSA_R_NULL_BEFORE_BLOCK_MISSING: c_ulong = 113;
const RSA_R_PADDING_CHECK_FAILED: c_ulong = 114;
const RSA_R_KEY_SIZE_TOO_SMALL: c_ulong = 120;
const RSA_R_D_E_NOT_CONGRUENT_TO_1: c_ulong = 123;
const RSA_R_DMP1_NOT_CONGRUENT_TO_D: c_ulong = 124;
const RSA_R_DMQ1_NOT_CONGRUENT_TO_D: c_ulong = 125;
const RSA_R_IQMP_NOT_INVERSE_OF_Q: c_ulong = 126;
const RSA_R_N_DOES_NOT_EQUAL_P_Q: c_ulong = 127;
const RSA_R_P_NOT_PRIME: c_ulong = 128;
const RSA_R_Q_NOT_PRIME: c_ulong = 129;
const RSA_R_INVALID_MESSAGE_LENGTH: c_ulong = 131;
const RSA_R_DATA_TOO_LARGE_FOR_MODULUS: c_ulong = 132;
const RSA_R_FIRST_OCTET_INVALID: c_ulong = 133;
const RSA_R_LAST_OCTET_INVALID: c_ulong = 134;
const RSA_R_SLEN_RECOVERY_FAILED: c_ulong = 135;
const RSA_R_SLEN_CHECK_FAILED: c_ulong = 136;
const RSA_R_INVALID_HEADER: c_ulong = 137;
const RSA_R_INVALID_PADDING: c_ulong = 138;
const RSA_R_NO_PUBLIC_EXPONENT: c_ulong = 140;
const RSA_R_INVALID_DIGEST_LENGTH: c_ulong = 143;
const RSA_R_DIGEST_DOES_NOT_MATCH: c_ulong = 158;
const RSA_R_PKCS_DECODING_ERROR: c_ulong = 159;
const RSA_R_KEY_PRIME_NUM_INVALID: c_ulong = 165;
const RSA_R_INVALID_MULTI_PRIME_KEY: c_ulong = 167;
const RSA_R_MP_COEFFICIENT_NOT_INVERSE_OF_R: c_ulong = 168;
const RSA_R_MP_EXPONENT_NOT_CONGRUENT_TO_D: c_ulong = 169;
const RSA_R_MP_R_NOT_PRIME: c_ulong = 170;
const RSA_R_INVALID_KEYPAIR: c_ulong = 171;
const RSA_R_N_DOES_NOT_EQUAL_PRODUCT_OF_PRIMES: c_ulong = 172;
const RSA_R_INVALID_KEY_LENGTH: c_ulong = 173;
const RSA_R_INVALID_MODULUS: c_ulong = 174;
const RSA_R_INVALID_REQUEST: c_ulong = 175;
const RSA_R_INVALID_STRENGTH: c_ulong = 176;
const RSA_R_PAIRWISE_TEST_FAILURE: c_ulong = 177;
const RSA_R_PUB_EXPONENT_OUT_OF_RANGE: c_ulong = 178;
const RSA_R_MISSING_PRIVATE_KEY: c_ulong = 179;
const RSA_R_INVALID_LENGTH: c_ulong = 181;

const EC_R_INCOMPATIBLE_OBJECTS: c_ulong = 101;
const EC_R_INVALID_ENCODING: c_ulong = 102;
const EC_R_INVALID_FIELD: c_ulong = 103;
const EC_R_INVALID_FORM: c_ulong = 104;
const EC_R_POINT_AT_INFINITY: c_ulong = 106;
const EC_R_POINT_IS_NOT_ON_CURVE: c_ulong = 107;
const EC_R_INVALID_COMPRESSION_BIT: c_ulong = 109;
const EC_R_INVALID_COMPRESSED_POINT: c_ulong = 110;
const EC_R_INVALID_ARGUMENT: c_ulong = 112;
const EC_R_UNDEFINED_GENERATOR: c_ulong = 113;
const EC_R_UNKNOWN_ORDER: c_ulong = 114;
const EC_R_ASN1_ERROR: c_ulong = 115;
const EC_R_INVALID_KEY: c_ulong = 116;
const EC_R_INVALID_LENGTH: c_ulong = 117;
const EC_R_DISCRIMINANT_IS_ZERO: c_ulong = 118;
const EC_R_INVALID_GROUP_ORDER: c_ulong = 122;
const EC_R_INVALID_PRIVATE_KEY: c_ulong = 123;
const EC_R_MISSING_PARAMETERS: c_ulong = 124;
const EC_R_MISSING_PRIVATE_KEY: c_ulong = 125;
const EC_R_EXPLICIT_PARAMS_NOT_SUPPORTED: c_ulong = 127;
const EC_R_UNDEFINED_ORDER: c_ulong = 128;
const EC_R_UNKNOWN_GROUP: c_ulong = 129;
const EC_R_WRONG_ORDER: c_ulong = 130;
const EC_R_UNSUPPORTED_FIELD: c_ulong = 131;
const EC_R_INVALID_PEER_KEY: c_ulong = 133;
const EC_R_NOT_A_NIST_PRIME: c_ulong = 135;
const EC_R_NO_PARAMETERS_SET: c_ulong = 139;
const EC_R_INVALID_CURVE: c_ulong = 141;
const EC_R_DECODE_ERROR: c_ulong = 142;
const EC_R_FIELD_TOO_LARGE: c_ulong = 143;
const EC_R_BIGNUM_OUT_OF_RANGE: c_ulong = 144;
const EC_R_WRONG_CURVE_PARAMETERS: c_ulong = 145;
const EC_R_COORDINATES_OUT_OF_RANGE: c_ulong = 146;
const EC_R_NO_PRIVATE_VALUE: c_ulong = 154;
const EC_R_BAD_SIGNATURE: c_ulong = 156;
const EC_R_CANNOT_INVERT: c_ulong = 165;
const EC_R_MISSING_OID: c_ulong = 167;
const EC_R_INVALID_A: c_ulong = 168;
const EC_R_INVALID_B: c_ulong = 169;
const EC_R_INVALID_COFACTOR: c_ulong = 171;
const EC_R_INVALID_P: c_ulong = 172;
const EC_R_INVALID_GENERATOR: c_ulong = 173;
const EC_R_INVALID_NAMED_GROUP_CONVERSION: c_ulong = 174;
const EC_R_INVALID_SEED: c_ulong = 175;

const RSA_IMPORT_INPUT_REASONS: &[c_ulong] = &[
    RSA_R_ALGORITHM_MISMATCH,
    RSA_R_BAD_E_VALUE,
    RSA_R_KEY_SIZE_TOO_SMALL,
    RSA_R_D_E_NOT_CONGRUENT_TO_1,
    RSA_R_DMP1_NOT_CONGRUENT_TO_D,
    RSA_R_DMQ1_NOT_CONGRUENT_TO_D,
    RSA_R_IQMP_NOT_INVERSE_OF_Q,
    RSA_R_N_DOES_NOT_EQUAL_P_Q,
    RSA_R_P_NOT_PRIME,
    RSA_R_Q_NOT_PRIME,
    RSA_R_NO_PUBLIC_EXPONENT,
    RSA_R_KEY_PRIME_NUM_INVALID,
    RSA_R_INVALID_MULTI_PRIME_KEY,
    RSA_R_MP_COEFFICIENT_NOT_INVERSE_OF_R,
    RSA_R_MP_EXPONENT_NOT_CONGRUENT_TO_D,
    RSA_R_MP_R_NOT_PRIME,
    RSA_R_INVALID_KEYPAIR,
    RSA_R_N_DOES_NOT_EQUAL_PRODUCT_OF_PRIMES,
    RSA_R_INVALID_KEY_LENGTH,
    RSA_R_INVALID_MODULUS,
    RSA_R_INVALID_REQUEST,
    RSA_R_INVALID_STRENGTH,
    RSA_R_PAIRWISE_TEST_FAILURE,
    RSA_R_PUB_EXPONENT_OUT_OF_RANGE,
    RSA_R_MISSING_PRIVATE_KEY,
    RSA_R_INVALID_LENGTH,
];

const EC_IMPORT_INPUT_REASONS: &[c_ulong] = &[
    EC_R_INCOMPATIBLE_OBJECTS,
    EC_R_INVALID_ENCODING,
    EC_R_INVALID_FIELD,
    EC_R_INVALID_FORM,
    EC_R_POINT_AT_INFINITY,
    EC_R_POINT_IS_NOT_ON_CURVE,
    EC_R_INVALID_COMPRESSION_BIT,
    EC_R_INVALID_COMPRESSED_POINT,
    EC_R_INVALID_ARGUMENT,
    EC_R_UNDEFINED_GENERATOR,
    EC_R_UNKNOWN_ORDER,
    EC_R_ASN1_ERROR,
    EC_R_INVALID_KEY,
    EC_R_INVALID_LENGTH,
    EC_R_DISCRIMINANT_IS_ZERO,
    EC_R_INVALID_GROUP_ORDER,
    EC_R_INVALID_PRIVATE_KEY,
    EC_R_MISSING_PARAMETERS,
    EC_R_MISSING_PRIVATE_KEY,
    EC_R_EXPLICIT_PARAMS_NOT_SUPPORTED,
    EC_R_UNDEFINED_ORDER,
    EC_R_UNKNOWN_GROUP,
    EC_R_WRONG_ORDER,
    EC_R_UNSUPPORTED_FIELD,
    EC_R_INVALID_PEER_KEY,
    EC_R_NOT_A_NIST_PRIME,
    EC_R_NO_PARAMETERS_SET,
    EC_R_INVALID_CURVE,
    EC_R_DECODE_ERROR,
    EC_R_FIELD_TOO_LARGE,
    EC_R_BIGNUM_OUT_OF_RANGE,
    EC_R_WRONG_CURVE_PARAMETERS,
    EC_R_COORDINATES_OUT_OF_RANGE,
    EC_R_NO_PRIVATE_VALUE,
    EC_R_CANNOT_INVERT,
    EC_R_MISSING_OID,
    EC_R_INVALID_A,
    EC_R_INVALID_B,
    EC_R_INVALID_COFACTOR,
    EC_R_INVALID_P,
    EC_R_INVALID_GENERATOR,
    EC_R_INVALID_NAMED_GROUP_CONVERSION,
    EC_R_INVALID_SEED,
];

const RSA_VERIFY_MISMATCH_REASONS: &[c_ulong] = &[
    RSA_R_BAD_PAD_BYTE_COUNT,
    RSA_R_BAD_SIGNATURE,
    RSA_R_BLOCK_TYPE_IS_NOT_01,
    RSA_R_BLOCK_TYPE_IS_NOT_02,
    RSA_R_DATA_GREATER_THAN_MOD_LEN,
    RSA_R_DATA_TOO_LARGE,
    RSA_R_DATA_TOO_LARGE_FOR_KEY_SIZE,
    RSA_R_DATA_TOO_SMALL,
    RSA_R_DIGEST_TOO_BIG_FOR_RSA_KEY,
    RSA_R_NULL_BEFORE_BLOCK_MISSING,
    RSA_R_PADDING_CHECK_FAILED,
    RSA_R_INVALID_MESSAGE_LENGTH,
    RSA_R_DATA_TOO_LARGE_FOR_MODULUS,
    RSA_R_FIRST_OCTET_INVALID,
    RSA_R_LAST_OCTET_INVALID,
    RSA_R_SLEN_RECOVERY_FAILED,
    RSA_R_SLEN_CHECK_FAILED,
    RSA_R_INVALID_HEADER,
    RSA_R_INVALID_PADDING,
    RSA_R_INVALID_DIGEST_LENGTH,
    RSA_R_DIGEST_DOES_NOT_MATCH,
    RSA_R_PKCS_DECODING_ERROR,
];

fn error_parts(error: c_ulong) -> Option<(c_ulong, c_ulong)> {
    if error & ERR_SYSTEM_FLAG != 0 {
        return None;
    }
    let reason = error & ERR_REASON_MASK;
    if reason & ERR_RFLAG_FATAL != 0 {
        return None;
    }
    Some(((error >> ERR_LIB_OFFSET) & ERR_LIB_MASK, reason))
}

fn asn1_input_reason(reason: c_ulong) -> bool {
    matches!(
        reason,
        ASN1_R_BAD_OBJECT_HEADER
            | ASN1_R_DATA_IS_WRONG
            | ASN1_R_DECODE_ERROR
            | ASN1_R_EXPECTING_AN_INTEGER
            | ASN1_R_EXPECTING_AN_OBJECT
            | ASN1_R_EXPLICIT_LENGTH_MISMATCH
            | ASN1_R_EXPLICIT_TAG_NOT_CONSTRUCTED
            | ASN1_R_FIELD_MISSING
            | ASN1_R_HEADER_TOO_LONG
            | ASN1_R_ILLEGAL_NULL
            | ASN1_R_INTEGER_TOO_LARGE_FOR_LONG
            | ASN1_R_MISSING_EOC
            | ASN1_R_NOT_ENOUGH_DATA
            | ASN1_R_NULL_IS_WRONG_LENGTH
            | ASN1_R_SEQUENCE_LENGTH_MISMATCH
            | ASN1_R_SEQUENCE_NOT_CONSTRUCTED
            | ASN1_R_TOO_LONG
            | ASN1_R_TYPE_NOT_CONSTRUCTED
            | ASN1_R_UNEXPECTED_EOC
            | ASN1_R_UNKNOWN_OBJECT_TYPE
            | ASN1_R_WRONG_TAG
            | ASN1_R_ILLEGAL_BITSTRING_FORMAT
            | ASN1_R_ILLEGAL_INTEGER
            | ASN1_R_INVALID_OBJECT_ENCODING
            | ASN1_R_INVALID_BIT_STRING_BITS_LEFT
            | ASN1_R_ILLEGAL_PADDING
            | ASN1_R_ILLEGAL_ZERO_CONTENT
            | ASN1_R_TOO_LARGE
            | ASN1_R_TOO_SMALL
            | ASN1_R_WRONG_INTEGER_TYPE
            | ASN1_R_ILLEGAL_NEGATIVE_VALUE
            | ASN1_R_BAD_TEMPLATE
            | ASN1_R_LENGTH_TOO_LONG
            | ERR_R_NESTED_ASN1_ERROR
            | ERR_R_MISSING_ASN1_EOS
    )
}

fn import_input_reason(lib: c_ulong, reason: c_ulong) -> bool {
    match lib {
        ERR_LIB_ASN1 => asn1_input_reason(reason),
        ERR_LIB_EVP => matches!(
            reason,
            EVP_R_DECODE_ERROR
                | EVP_R_INVALID_KEY_LENGTH
                | EVP_R_PRIVATE_KEY_DECODE_ERROR
                | EVP_R_INVALID_KEY
                | EVP_R_INVALID_SEED_LENGTH
        ),
        ERR_LIB_PROV => matches!(
            reason,
            PROV_R_INVALID_KEY_LENGTH
                | PROV_R_INVALID_DATA
                | PROV_R_BAD_ENCODING
                | PROV_R_BAD_LENGTH
                | PROV_R_INVALID_SEED_LENGTH
                | PROV_R_INVALID_KEY
        ),
        ERR_LIB_RSA => RSA_IMPORT_INPUT_REASONS.contains(&reason),
        ERR_LIB_EC => EC_IMPORT_INPUT_REASONS.contains(&reason),
        _ => false,
    }
}

fn failed_status(errors: ErrorQueue, decoder_only: bool) -> c_int {
    if !errors.empty
        && if decoder_only {
            errors.decoder_input_only
        } else {
            errors.import_input_only
        }
    {
        AL_INVALID
    } else {
        AL_CODE
    }
}

fn check_result_status(rc: c_int, errors: ErrorQueue) -> Result<(), c_int> {
    match rc {
        1 => Ok(()),
        0 if errors.empty || errors.import_input_only => Err(AL_INVALID),
        _ => Err(AL_CODE),
    }
}

fn verify_result_status(rc: c_int, errors: ErrorQueue) -> Result<bool, c_int> {
    match rc {
        1 => Ok(true),
        0 if errors.empty || errors.verify_mismatch_only => Ok(false),
        _ => Err(AL_CODE),
    }
}

fn verify_mismatch_reason(lib: c_ulong, reason: c_ulong) -> bool {
    match lib {
        ERR_LIB_RSA => RSA_VERIFY_MISMATCH_REASONS.contains(&reason),
        ERR_LIB_EC => reason == EC_R_BAD_SIGNATURE,
        ERR_LIB_EVP => reason == EVP_R_PROVIDER_SIGNATURE_FAILURE,
        ERR_LIB_PROV => {
            reason == PROV_R_INVALID_SIGNATURE_SIZE || reason == (ERR_RFLAG_COMMON | ERR_LIB_RSA)
        }
        _ => false,
    }
}

unsafe fn decode_private(parts: &mut KeyParts, der: &SensitiveDer) -> Result<(), c_int> {
    let Ok(length) = c_long::try_from(der.len) else {
        return Err(AL_INVALID);
    };
    let mut cursor = der.ptr.as_ptr().cast_const();
    #[cfg(test)]
    allocation_failpoint()?;
    clear_errors();
    let info = unsafe { d2i_PKCS8_PRIV_KEY_INFO(ptr::null_mut(), &mut cursor, length) };
    let errors = drain_errors();
    if info.is_null() {
        return Err(failed_status(errors, true));
    }
    let result: Result<(), c_int> = (|| {
        if cursor != unsafe { der.ptr.as_ptr().add(der.len) }.cast_const() {
            return Err(AL_INVALID);
        }
        let mut object = ptr::null();
        let mut private_octets = ptr::null();
        let mut private_len = 0;
        let mut algorithm = ptr::null();
        clear_errors();
        let got = unsafe {
            PKCS8_pkey_get0(
                &mut object,
                &mut private_octets,
                &mut private_len,
                &mut algorithm,
                info,
            )
        };
        let get_errors = drain_errors();
        if got != 1
            || object.is_null()
            || private_octets.is_null()
            || private_len <= 0
            || algorithm.is_null()
        {
            return Err(failed_status(get_errors, false));
        }
        clear_errors();
        let encoded_len = unsafe { i2d_PKCS8_PRIV_KEY_INFO(info, ptr::null_mut()) };
        let len_errors = drain_errors();
        if encoded_len <= 0 {
            return Err(failed_status(len_errors, false));
        }
        let encoded_len = usize::try_from(encoded_len).map_err(|_| AL_CODE)?;
        let encoded = SensitiveDer::new(encoded_len)?;
        let mut encoded_cursor = encoded.ptr.as_ptr();
        clear_errors();
        let written = unsafe { i2d_PKCS8_PRIV_KEY_INFO(info, &mut encoded_cursor) };
        let encode_errors = drain_errors();
        if usize::try_from(written).ok() != Some(encoded_len)
            || encoded_cursor != unsafe { encoded.ptr.as_ptr().add(encoded_len) }
        {
            return Err(failed_status(encode_errors, false));
        }
        if encoded.as_slice() != der.as_slice() {
            return Err(AL_INVALID);
        }
        #[cfg(test)]
        allocation_failpoint()?;
        clear_errors();
        let pkey = unsafe { EVP_PKCS82PKEY_ex(info, parts.libctx, PROPQ.as_ptr()) };
        let import_errors = drain_errors();
        if pkey.is_null() {
            return Err(failed_status(import_errors, false));
        }
        parts.pkey = pkey;
        Ok(())
    })();
    unsafe { PKCS8_PRIV_KEY_INFO_free(info) };
    result
}

unsafe fn decode_public(parts: &mut KeyParts, der: &SensitiveDer) -> Result<(), c_int> {
    let Ok(length) = c_long::try_from(der.len) else {
        return Err(AL_INVALID);
    };
    let mut cursor = der.ptr.as_ptr().cast_const();
    #[cfg(test)]
    allocation_failpoint()?;
    clear_errors();
    let pkey = unsafe {
        d2i_PUBKEY_ex(
            ptr::null_mut(),
            &mut cursor,
            length,
            parts.libctx,
            PROPQ.as_ptr(),
        )
    };
    let errors = drain_errors();
    if pkey.is_null() {
        return Err(failed_status(errors, true));
    }
    parts.pkey = pkey;
    if cursor != unsafe { der.ptr.as_ptr().add(der.len) }.cast_const() {
        return Err(AL_INVALID);
    }
    clear_errors();
    let encoded_len = unsafe { i2d_PUBKEY(pkey, ptr::null_mut()) };
    let len_errors = drain_errors();
    if encoded_len <= 0 {
        return Err(failed_status(len_errors, false));
    }
    let encoded_len = usize::try_from(encoded_len).map_err(|_| AL_CODE)?;
    let encoded = SensitiveDer::new(encoded_len)?;
    let mut encoded_cursor = encoded.ptr.as_ptr();
    clear_errors();
    let written = unsafe { i2d_PUBKEY(pkey, &mut encoded_cursor) };
    let encode_errors = drain_errors();
    if usize::try_from(written).ok() != Some(encoded_len)
        || encoded_cursor != unsafe { encoded.ptr.as_ptr().add(encoded_len) }
    {
        return Err(failed_status(encode_errors, false));
    }
    if encoded.as_slice() != der.as_slice() {
        return Err(AL_INVALID);
    }
    Ok(())
}

struct Bn(*mut c_void);

impl Drop for Bn {
    fn drop(&mut self) {
        unsafe { BN_free(self.0) };
    }
}

fn get_bn(pkey: *mut c_void, name: &core::ffi::CStr) -> Result<Bn, c_int> {
    let mut bn = ptr::null_mut();
    #[cfg(test)]
    allocation_failpoint()?;
    clear_errors();
    let rc = unsafe { EVP_PKEY_get_bn_param(pkey, name.as_ptr(), &mut bn) };
    let errors = drain_errors();
    if rc != 1 || bn.is_null() {
        Err(failed_status(errors, false))
    } else {
        Ok(Bn(bn))
    }
}

fn pkey_bits(pkey: *mut c_void) -> Result<c_int, c_int> {
    clear_errors();
    let bits = unsafe { EVP_PKEY_get_bits(pkey) };
    let _errors = drain_errors();
    (bits > 0).then_some(bits).ok_or(AL_CODE)
}

fn pkey_size(pkey: *mut c_void) -> Result<usize, c_int> {
    clear_errors();
    let size = unsafe { EVP_PKEY_get_size(pkey) };
    let _errors = drain_errors();
    let size = usize::try_from(size).map_err(|_| AL_CODE)?;
    (size > 0).then_some(size).ok_or(AL_CODE)
}

fn validate_rsa(pkey: *mut c_void) -> Result<(), c_int> {
    let bits = pkey_bits(pkey)?;
    if !(2048..=8192).contains(&bits) {
        return Err(AL_INVALID);
    }
    let n = get_bn(pkey, c"n")?;
    let e = get_bn(pkey, c"e")?;
    if unsafe { BN_num_bits(n.0) } != bits || unsafe { BN_is_bit_set(n.0, 0) } != 1 {
        return Err(AL_INVALID);
    }
    let e_bits = unsafe { BN_num_bits(e.0) };
    if !(2..=64).contains(&e_bits) || unsafe { BN_is_bit_set(e.0, 0) } != 1 {
        return Err(AL_INVALID);
    }
    let exponent = unsafe { BN_get_word(e.0) };
    if exponent < 3 {
        return Err(AL_INVALID);
    }
    Ok(())
}

fn validate_ec(pkey: *mut c_void) -> Result<(), c_int> {
    let mut group = [0u8; 32];
    let mut written = 0usize;
    clear_errors();
    let rc = unsafe {
        EVP_PKEY_get_utf8_string_param(
            pkey,
            c"group".as_ptr(),
            group.as_mut_ptr().cast(),
            group.len(),
            &mut written,
        )
    };
    let errors = drain_errors();
    if rc != 1 {
        return Err(failed_status(errors, false));
    }
    if group.get(..written) != Some(b"prime256v1".as_slice()) {
        return Err(AL_INVALID);
    }
    Ok(())
}

fn raw_ed25519_public(pkey: *mut c_void) -> Result<[u8; 32], c_int> {
    let mut public = [0u8; 32];
    let mut len = public.len();
    clear_errors();
    let rc = unsafe { EVP_PKEY_get_raw_public_key(pkey, public.as_mut_ptr(), &mut len) };
    let errors = drain_errors();
    if rc != 1 || len != public.len() {
        return Err(failed_status(errors, false));
    }
    Ok(public)
}

struct BnContext(*mut c_void);

impl BnContext {
    fn new(libctx: *mut c_void) -> Result<Self, c_int> {
        #[cfg(test)]
        allocation_failpoint()?;
        clear_errors();
        let context = unsafe { BN_CTX_new_ex(libctx) };
        let _errors = drain_errors();
        if context.is_null() {
            return Err(AL_CODE);
        }
        unsafe { BN_CTX_start(context) };
        Ok(Self(context))
    }

    fn temps<const N: usize>(&self) -> Result<[*mut c_void; N], c_int> {
        let mut values = [ptr::null_mut(); N];
        for value in &mut values {
            #[cfg(test)]
            allocation_failpoint()?;
            clear_errors();
            *value = unsafe { BN_CTX_get(self.0) };
            let _errors = drain_errors();
            if value.is_null() {
                return Err(AL_CODE);
            }
        }
        Ok(values)
    }
}

impl Drop for BnContext {
    fn drop(&mut self) {
        unsafe {
            BN_CTX_end(self.0);
            BN_CTX_free(self.0);
        }
    }
}

fn validate_ed25519(encoded: &[u8; 32], libctx: *mut c_void) -> Result<(), c_int> {
    const P: [u8; 32] = [
        0xed, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0x7f,
    ];
    const D: [u8; 32] = [
        0xa3, 0x78, 0x59, 0x13, 0xca, 0x4d, 0xeb, 0x75, 0xab, 0xd8, 0x41, 0x41, 0x4d, 0x0a, 0x70,
        0x00, 0x98, 0xe8, 0x79, 0x77, 0x79, 0x40, 0xc7, 0x8c, 0x73, 0xfe, 0x6f, 0x2b, 0xee, 0x6c,
        0x03, 0x52,
    ];
    const EXPONENT: [u8; 32] = [
        0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0x0f,
    ];
    const SQRT_MINUS_ONE: [u8; 32] = [
        0xb0, 0xa0, 0x0e, 0x4a, 0x27, 0x1b, 0xee, 0xc4, 0x78, 0xe4, 0x2f, 0xad, 0x06, 0x18, 0x43,
        0x2f, 0xa7, 0xd7, 0xfb, 0x3d, 0x99, 0x00, 0x4d, 0x2b, 0x0b, 0xdf, 0xc1, 0x4f, 0x80, 0x24,
        0x83, 0x2b,
    ];

    let sign = encoded[31] >> 7;
    let mut y_bytes = *encoded;
    y_bytes[31] &= 0x7f;
    let context = BnContext::new(libctx)?;
    let [
        p,
        d,
        exponent,
        sqrt_minus_one,
        one,
        x,
        y,
        y2,
        x2,
        numerator,
        denominator,
        inverse,
        candidate,
        check,
        lhs,
        rhs,
        a,
        b,
        c,
        e,
        f,
        g,
        h,
        t,
        z,
        scratch,
    ] = context.temps::<26>()?;
    macro_rules! op {
        ($call:expr) => {{
            clear_errors();
            let rc = unsafe { $call };
            let _errors = drain_errors();
            if rc != 1 {
                return Err(AL_CODE);
            }
        }};
    }
    macro_rules! load_le {
        ($bytes:expr, $out:expr) => {{
            clear_errors();
            let loaded = unsafe { BN_lebin2bn($bytes.as_ptr(), 32, $out) };
            let _errors = drain_errors();
            if loaded.is_null() {
                return Err(AL_CODE);
            }
        }};
    }
    load_le!(P, p);
    load_le!(D, d);
    load_le!(EXPONENT, exponent);
    load_le!(SQRT_MINUS_ONE, sqrt_minus_one);
    load_le!(y_bytes, y);
    op!(BN_set_word(one, 1));
    if unsafe { BN_cmp(y, p) } >= 0 {
        return Err(AL_INVALID);
    }
    op!(BN_mod_mul(y2, y, y, p, context.0));
    op!(BN_mod_sub(numerator, y2, one, p, context.0));
    op!(BN_mod_mul(denominator, d, y2, p, context.0));
    op!(BN_mod_add(denominator, denominator, one, p, context.0));
    if unsafe { BN_is_zero(denominator) } == 1 {
        return Err(AL_INVALID);
    }
    clear_errors();
    let inverted = unsafe { BN_mod_inverse(inverse, denominator, p, context.0) };
    let _inverse_errors = drain_errors();
    if inverted.is_null() {
        return Err(AL_CODE);
    }
    op!(BN_mod_mul(candidate, numerator, inverse, p, context.0));
    op!(BN_mod_exp(x, candidate, exponent, p, context.0));
    op!(BN_mod_mul(check, x, x, p, context.0));
    if unsafe { BN_cmp(check, candidate) } != 0 {
        op!(BN_mod_mul(x, x, sqrt_minus_one, p, context.0));
        op!(BN_mod_mul(check, x, x, p, context.0));
        if unsafe { BN_cmp(check, candidate) } != 0 {
            return Err(AL_INVALID);
        }
    }
    if unsafe { BN_is_bit_set(x, 0) } != c_int::from(sign) {
        op!(BN_mod_sub(x, p, x, p, context.0));
    }
    if unsafe { BN_is_zero(x) } == 1 && sign == 1 {
        return Err(AL_INVALID);
    }
    op!(BN_mod_mul(x2, x, x, p, context.0));
    op!(BN_mod_sub(lhs, y2, x2, p, context.0));
    op!(BN_mod_mul(rhs, x2, y2, p, context.0));
    op!(BN_mod_mul(rhs, d, rhs, p, context.0));
    op!(BN_mod_add(rhs, rhs, one, p, context.0));
    if unsafe { BN_cmp(lhs, rhs) } != 0 {
        return Err(AL_INVALID);
    }
    let mut roundtrip = [0u8; 32];
    clear_errors();
    let encoded_len = unsafe { BN_bn2lebinpad(y, roundtrip.as_mut_ptr(), 32) };
    let _encode_errors = drain_errors();
    if encoded_len != 32 {
        return Err(AL_CODE);
    }
    roundtrip[31] |= sign << 7;
    if roundtrip != *encoded {
        return Err(AL_INVALID);
    }

    // Extended-coordinate point doubling, three times. Eight times any point in the complete
    // small-order subgroup is the projective identity; ordinary prime-order public keys are not.
    op!(BN_set_word(z, 1));
    op!(BN_mod_mul(t, x, y, p, context.0));
    for _ in 0..3 {
        op!(BN_mod_mul(a, x, x, p, context.0));
        op!(BN_mod_mul(b, y, y, p, context.0));
        op!(BN_mod_mul(c, z, z, p, context.0));
        op!(BN_mod_add(c, c, c, p, context.0));
        op!(BN_mod_sub(scratch, p, a, p, context.0)); // D = -A
        op!(BN_mod_add(e, x, y, p, context.0));
        op!(BN_mod_mul(e, e, e, p, context.0));
        op!(BN_mod_sub(e, e, a, p, context.0));
        op!(BN_mod_sub(e, e, b, p, context.0));
        op!(BN_mod_add(g, scratch, b, p, context.0));
        op!(BN_mod_sub(f, g, c, p, context.0));
        op!(BN_mod_sub(h, scratch, b, p, context.0));
        op!(BN_mod_mul(x, e, f, p, context.0));
        op!(BN_mod_mul(y, g, h, p, context.0));
        op!(BN_mod_mul(t, e, h, p, context.0));
        op!(BN_mod_mul(z, f, g, p, context.0));
    }
    if unsafe { BN_is_zero(x) } == 1 && unsafe { BN_cmp(y, z) } == 0 {
        return Err(AL_INVALID);
    }
    Ok(())
}

fn validate_provider_key(
    parts: &KeyParts,
    algorithm: Algorithm,
    private: bool,
) -> Result<(), c_int> {
    clear_errors();
    let is_algorithm = unsafe { EVP_PKEY_is_a(parts.pkey, algorithm.key_name().as_ptr()) };
    let is_errors = drain_errors();
    if is_algorithm != 1 {
        return Err(if is_algorithm == 0 && is_errors.empty {
            AL_INVALID
        } else {
            AL_CODE
        });
    }
    if unsafe { EVP_PKEY_get0_provider(parts.pkey) } != parts.provider.cast_const() {
        return Err(AL_CODE);
    }
    let ed25519_public = match algorithm {
        Algorithm::Rs256 => {
            validate_rsa(parts.pkey)?;
            None
        }
        Algorithm::Es256 => {
            validate_ec(parts.pkey)?;
            None
        }
        Algorithm::Ed25519 => Some(raw_ed25519_public(parts.pkey)?),
    };
    clear_errors();
    #[cfg(test)]
    allocation_failpoint()?;
    let check = unsafe { EVP_PKEY_CTX_new_from_pkey(parts.libctx, parts.pkey, PROPQ.as_ptr()) };
    let context_errors = drain_errors();
    if check.is_null() {
        return Err(failed_status(context_errors, false));
    }
    let result: Result<(), c_int> = (|| {
        let run = |operation: unsafe extern "C" fn(*mut c_void) -> c_int| {
            clear_errors();
            let rc = unsafe { operation(check) };
            let errors = drain_errors();
            check_result_status(rc, errors)
        };
        if private {
            run(EVP_PKEY_private_check)?;
            run(EVP_PKEY_pairwise_check)?;
        } else {
            run(EVP_PKEY_public_check)?;
        }
        Ok(())
    })();
    unsafe { EVP_PKEY_CTX_free(check) };
    result?;
    if let Some(public) = ed25519_public {
        validate_ed25519(&public, parts.libctx)?;
    }
    Ok(())
}

unsafe fn key_from_pem(
    algorithm: c_int,
    pem_ptr: *const u8,
    pem_len: i64,
    out: *mut *mut CryptoKey,
    private: bool,
) -> c_int {
    if output_slot(out).is_none() {
        return AL_INVALID;
    }
    unsafe { *out = ptr::null_mut() };
    let Some(algorithm) = Algorithm::parse(algorithm) else {
        return AL_INVALID;
    };
    let pem = match input_view(pem_ptr, pem_len) {
        Ok(pem) => pem,
        Err(status) => return status,
    };
    let der = match parse_pem(pem, private) {
        Ok(der) => der,
        Err(status) => return status,
    };
    if let Err(status) = validate_der_envelope(der.as_slice(), algorithm, private) {
        return status;
    }
    let mut parts = match KeyParts::new() {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let decoded = if private {
        unsafe { decode_private(&mut parts, &der) }
    } else {
        unsafe { decode_public(&mut parts, &der) }
    };
    if let Err(status) = decoded.and_then(|()| validate_provider_key(&parts, algorithm, private)) {
        return status;
    }
    parts.publish(
        if private {
            algorithm.private_kind()
        } else {
            algorithm.public_kind()
        },
        out,
    )
}

/// Construct one algorithm-specific private key from canonical unencrypted PKCS#8 PEM.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_crypto_private_key_from_pem(
    algorithm: c_int,
    pem_ptr: *const u8,
    pem_len: i64,
    out: *mut *mut CryptoKey,
) -> c_int {
    unsafe { key_from_pem(algorithm, pem_ptr, pem_len, out, true) }
}

/// Construct one algorithm-specific public key from canonical SPKI PEM.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_crypto_public_key_from_pem(
    algorithm: c_int,
    pem_ptr: *const u8,
    pem_len: i64,
    out: *mut *mut CryptoKey,
) -> c_int {
    unsafe { key_from_pem(algorithm, pem_ptr, pem_len, out, false) }
}

fn bn_from_be(bytes: &[u8]) -> Result<Bn, c_int> {
    let len = c_int::try_from(bytes.len()).map_err(|_| AL_INVALID)?;
    #[cfg(test)]
    allocation_failpoint()?;
    clear_errors();
    let bn = unsafe { BN_bin2bn(bytes.as_ptr(), len, ptr::null_mut()) };
    let _errors = drain_errors();
    if bn.is_null() {
        Err(AL_CODE)
    } else {
        Ok(Bn(bn))
    }
}

fn validate_jwk_shape(algorithm: Algorithm, first: &[u8], second: &[u8]) -> Result<(), c_int> {
    match algorithm {
        Algorithm::Rs256 => {
            if !(256..=1024).contains(&first.len())
                || first.first() == Some(&0)
                || first.last().is_none_or(|byte| byte & 1 == 0)
            {
                return Err(AL_INVALID);
            }
            let leading = first[0].leading_zeros() as usize;
            let bits = first
                .len()
                .checked_mul(8)
                .and_then(|bits| bits.checked_sub(leading))
                .ok_or(AL_INVALID)?;
            if !(2048..=8192).contains(&bits)
                || !(1..=8).contains(&second.len())
                || second.first() == Some(&0)
            {
                return Err(AL_INVALID);
            }
            let exponent = second
                .iter()
                .fold(0u64, |value, byte| (value << 8) | u64::from(*byte));
            if exponent < 3 || exponent & 1 == 0 {
                return Err(AL_INVALID);
            }
        }
        Algorithm::Es256 => {
            if first.len() != 32 || second.len() != 32 {
                return Err(AL_INVALID);
            }
        }
        Algorithm::Ed25519 => {
            if first.len() != 32 || !second.is_empty() {
                return Err(AL_INVALID);
            }
        }
    }
    Ok(())
}

fn import_jwk(
    parts: &mut KeyParts,
    algorithm: Algorithm,
    first: &[u8],
    second: &[u8],
) -> Result<(), c_int> {
    validate_jwk_shape(algorithm, first, second)?;
    let mut ec_point = [0u8; 65];
    let mut rsa_n = None;
    let mut rsa_e = None;
    match algorithm {
        Algorithm::Rs256 => {
            rsa_n = Some(bn_from_be(first)?);
            rsa_e = Some(bn_from_be(second)?);
        }
        Algorithm::Es256 => {
            ec_point[0] = 4;
            ec_point[1..33].copy_from_slice(first);
            ec_point[33..].copy_from_slice(second);
        }
        Algorithm::Ed25519 => {}
    }

    #[cfg(test)]
    allocation_failpoint()?;
    clear_errors();
    let builder = unsafe { OSSL_PARAM_BLD_new() };
    let _builder_errors = drain_errors();
    if builder.is_null() {
        return Err(AL_CODE);
    }
    macro_rules! push_param {
        ($call:expr) => {{
            #[cfg(test)]
            if let Err(status) = allocation_failpoint() {
                unsafe { OSSL_PARAM_BLD_free(builder) };
                return Err(status);
            }
            clear_errors();
            let rc = unsafe { $call };
            let _errors = drain_errors();
            if rc != 1 {
                unsafe { OSSL_PARAM_BLD_free(builder) };
                return Err(AL_CODE);
            }
        }};
    }
    match algorithm {
        Algorithm::Rs256 => {
            let (Some(n), Some(e)) = (rsa_n.as_ref(), rsa_e.as_ref()) else {
                unsafe { OSSL_PARAM_BLD_free(builder) };
                return Err(AL_CODE);
            };
            push_param!(OSSL_PARAM_BLD_push_BN(builder, c"n".as_ptr(), n.0,));
            push_param!(OSSL_PARAM_BLD_push_BN(builder, c"e".as_ptr(), e.0,));
        }
        Algorithm::Es256 => {
            push_param!(OSSL_PARAM_BLD_push_utf8_string(
                builder,
                c"group".as_ptr(),
                c"prime256v1".as_ptr(),
                0,
            ));
            push_param!(OSSL_PARAM_BLD_push_octet_string(
                builder,
                c"pub".as_ptr(),
                ec_point.as_ptr().cast(),
                ec_point.len(),
            ));
        }
        Algorithm::Ed25519 => push_param!(OSSL_PARAM_BLD_push_octet_string(
            builder,
            c"pub".as_ptr(),
            first.as_ptr().cast(),
            first.len(),
        )),
    }
    #[cfg(test)]
    if let Err(status) = allocation_failpoint() {
        unsafe { OSSL_PARAM_BLD_free(builder) };
        return Err(status);
    }
    clear_errors();
    let params = unsafe { OSSL_PARAM_BLD_to_param(builder) };
    let _params_errors = drain_errors();
    if params.is_null() {
        unsafe { OSSL_PARAM_BLD_free(builder) };
        return Err(AL_CODE);
    }
    #[cfg(test)]
    if let Err(status) = allocation_failpoint() {
        unsafe {
            OSSL_PARAM_free(params);
            OSSL_PARAM_BLD_free(builder);
        }
        return Err(status);
    }
    clear_errors();
    let context = unsafe {
        EVP_PKEY_CTX_new_from_name(parts.libctx, algorithm.key_name().as_ptr(), PROPQ.as_ptr())
    };
    let context_errors = drain_errors();
    if context.is_null() {
        unsafe {
            OSSL_PARAM_free(params);
            OSSL_PARAM_BLD_free(builder);
        }
        return Err(failed_status(context_errors, false));
    }
    let result = (|| {
        clear_errors();
        let init = unsafe { EVP_PKEY_fromdata_init(context) };
        let init_errors = drain_errors();
        if init != 1 {
            return Err(failed_status(init_errors, false));
        }
        let mut pkey = ptr::null_mut();
        #[cfg(test)]
        allocation_failpoint()?;
        clear_errors();
        let rc = unsafe { EVP_PKEY_fromdata(context, &mut pkey, EVP_PKEY_PUBLIC_KEY, params) };
        let errors = drain_errors();
        // OpenSSL normally leaves `pkey` null on failure, but the out-parameter contract does not
        // make that a wrapper ownership invariant. Install any returned owner before inspecting
        // `rc`, so the `KeyParts` unwind frees it on every mixed result/out state.
        parts.pkey = pkey;
        if rc != 1 || parts.pkey.is_null() {
            return Err(failed_status(errors, false));
        }
        Ok(())
    })();
    unsafe {
        EVP_PKEY_CTX_free(context);
        OSSL_PARAM_free(params);
        OSSL_PARAM_BLD_free(builder);
    }
    result
}

/// Construct a public key from already-base64url-decoded JWK components.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_crypto_public_key_from_jwk(
    algorithm: c_int,
    first_ptr: *const u8,
    first_len: i64,
    second_ptr: *const u8,
    second_len: i64,
    out: *mut *mut CryptoKey,
) -> c_int {
    if output_slot(out).is_none() {
        return AL_INVALID;
    }
    unsafe { *out = ptr::null_mut() };
    let Some(algorithm) = Algorithm::parse(algorithm) else {
        return AL_INVALID;
    };
    let first = match input_view(first_ptr, first_len) {
        Ok(first) => first,
        Err(status) => return status,
    };
    if algorithm == Algorithm::Ed25519 && (!second_ptr.is_null() || second_len != 0) {
        return AL_INVALID;
    }
    let second = match input_view(second_ptr, second_len) {
        Ok(second) => second,
        Err(status) => return status,
    };
    if let Err(status) = validate_jwk_shape(algorithm, first, second) {
        return status;
    }
    let mut parts = match KeyParts::new() {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if let Err(status) = import_jwk(&mut parts, algorithm, first, second)
        .and_then(|()| validate_provider_key(&parts, algorithm, false))
    {
        return status;
    }
    parts.publish(algorithm.public_kind(), out)
}

struct MdContext(*mut c_void);

impl MdContext {
    fn new() -> Result<Self, c_int> {
        #[cfg(test)]
        allocation_failpoint()?;
        clear_errors();
        let context = unsafe { EVP_MD_CTX_new() };
        let _errors = drain_errors();
        if context.is_null() {
            Err(AL_CODE)
        } else {
            Ok(Self(context))
        }
    }
}

impl Drop for MdContext {
    fn drop(&mut self) {
        unsafe { EVP_MD_CTX_free(self.0) };
    }
}

fn operation_init(
    key: &CryptoKey,
    algorithm: Algorithm,
    signing: bool,
) -> Result<(MdContext, *mut c_void), c_int> {
    let context = MdContext::new()?;
    let digest = if algorithm == Algorithm::Ed25519 {
        ptr::null()
    } else {
        c"SHA256".as_ptr()
    };
    let mut pkey_context = ptr::null_mut();
    #[cfg(test)]
    allocation_failpoint()?;
    clear_errors();
    let rc = unsafe {
        if signing {
            EVP_DigestSignInit_ex(
                context.0,
                &mut pkey_context,
                digest,
                key.libctx,
                PROPQ.as_ptr(),
                key.pkey,
                ptr::null(),
            )
        } else {
            EVP_DigestVerifyInit_ex(
                context.0,
                &mut pkey_context,
                digest,
                key.libctx,
                PROPQ.as_ptr(),
                key.pkey,
                ptr::null(),
            )
        }
    };
    let errors = drain_errors();
    if rc != 1 || pkey_context.is_null() {
        let _ = errors;
        return Err(AL_CODE);
    }
    if unsafe { EVP_PKEY_CTX_get0_provider(pkey_context) } != key.provider.cast_const() {
        return Err(AL_CODE);
    }
    if algorithm == Algorithm::Rs256 {
        clear_errors();
        let rc = unsafe { EVP_PKEY_CTX_set_rsa_padding(pkey_context, RSA_PKCS1_PADDING) };
        drain_errors();
        if rc != 1 {
            return Err(AL_CODE);
        }
    }
    Ok((context, pkey_context))
}

fn reserve_zeroed(len: usize) -> Result<Vec<u8>, c_int> {
    #[cfg(test)]
    allocation_failpoint()?;
    let mut output = Vec::new();
    output.try_reserve_exact(len).map_err(|_| AL_CODE)?;
    output.resize(len, 0);
    Ok(output)
}

fn buffer_owner(value: Vec<u8>) -> Result<*mut Buffer, c_int> {
    #[cfg(test)]
    allocation_failpoint()?;
    let storage = unsafe { std::alloc::alloc(Layout::new::<Buffer>()) }.cast::<Buffer>();
    if storage.is_null() {
        return Err(AL_CODE);
    }
    let len = value.len();
    let cap = value.capacity();
    unsafe {
        storage.write(Buffer {
            data: value,
            cap,
            len,
        })
    };
    Ok(storage)
}

fn es_der_to_raw(der: &[u8]) -> Result<Vec<u8>, c_int> {
    let len = c_long::try_from(der.len()).map_err(|_| AL_CODE)?;
    let mut cursor = der.as_ptr();
    #[cfg(test)]
    allocation_failpoint()?;
    clear_errors();
    let signature = unsafe { d2i_ECDSA_SIG(ptr::null_mut(), &mut cursor, len) };
    let errors = drain_errors();
    if signature.is_null() || cursor != unsafe { der.as_ptr().add(der.len()) } {
        if !signature.is_null() {
            unsafe { ECDSA_SIG_free(signature) };
        }
        return Err(failed_status(errors, false));
    }
    let result = (|| {
        let mut r = ptr::null();
        let mut s = ptr::null();
        unsafe { ECDSA_SIG_get0(signature, &mut r, &mut s) };
        if r.is_null()
            || s.is_null()
            || unsafe { BN_is_negative(r) } != 0
            || unsafe { BN_is_negative(s) } != 0
            || unsafe { BN_is_zero(r) } == 1
            || unsafe { BN_is_zero(s) } == 1
            || unsafe { BN_num_bits(r) } > 256
            || unsafe { BN_num_bits(s) } > 256
        {
            return Err(AL_CODE);
        }
        let mut raw = reserve_zeroed(64)?;
        clear_errors();
        let r_len = unsafe { BN_bn2binpad(r, raw.as_mut_ptr(), 32) };
        let _r_errors = drain_errors();
        if r_len != 32 {
            return Err(AL_CODE);
        }
        clear_errors();
        let s_len = unsafe { BN_bn2binpad(s, raw.as_mut_ptr().add(32), 32) };
        let _s_errors = drain_errors();
        if s_len != 32 {
            return Err(AL_CODE);
        }
        Ok(raw)
    })();
    unsafe { ECDSA_SIG_free(signature) };
    result
}

fn es_raw_to_der(raw: &[u8]) -> Result<Option<Vec<u8>>, c_int> {
    let r = bn_from_be(&raw[..32])?;
    let s = bn_from_be(&raw[32..])?;
    if unsafe { BN_is_zero(r.0) } == 1 || unsafe { BN_is_zero(s.0) } == 1 {
        return Ok(None);
    }
    #[cfg(test)]
    allocation_failpoint()?;
    clear_errors();
    let signature = unsafe { ECDSA_SIG_new() };
    let _new_errors = drain_errors();
    if signature.is_null() {
        return Err(AL_CODE);
    }
    clear_errors();
    let set = unsafe { ECDSA_SIG_set0(signature, r.0, s.0) };
    let _set_errors = drain_errors();
    if set != 1 {
        unsafe { ECDSA_SIG_free(signature) };
        return Err(AL_CODE);
    }
    core::mem::forget(r);
    core::mem::forget(s);
    clear_errors();
    let len = unsafe { i2d_ECDSA_SIG(signature, ptr::null_mut()) };
    let _len_errors = drain_errors();
    if len <= 0 {
        unsafe { ECDSA_SIG_free(signature) };
        return Err(AL_CODE);
    }
    let len = usize::try_from(len).map_err(|_| AL_CODE)?;
    let mut der = reserve_zeroed(len)?;
    let mut cursor = der.as_mut_ptr();
    clear_errors();
    let written = unsafe { i2d_ECDSA_SIG(signature, &mut cursor) };
    let errors = drain_errors();
    unsafe { ECDSA_SIG_free(signature) };
    if usize::try_from(written).ok() != Some(len) || cursor != unsafe { der.as_mut_ptr().add(len) }
    {
        return Err(failed_status(errors, false));
    }
    Ok(Some(der))
}

fn sign(key: &CryptoKey, algorithm: Algorithm, message: &[u8]) -> Result<Vec<u8>, c_int> {
    let (context, _) = operation_init(key, algorithm, true)?;
    if algorithm == Algorithm::Ed25519 {
        let mut output = reserve_zeroed(64)?;
        let mut len = output.len();
        clear_errors();
        let rc = unsafe {
            EVP_DigestSign(
                context.0,
                output.as_mut_ptr(),
                &mut len,
                message.as_ptr(),
                message.len(),
            )
        };
        drain_errors();
        if rc != 1 || len != 64 {
            return Err(AL_CODE);
        }
        return Ok(output);
    }
    clear_errors();
    let updated =
        unsafe { EVP_DigestSignUpdate(context.0, message.as_ptr().cast(), message.len()) };
    drain_errors();
    if updated != 1 {
        return Err(AL_CODE);
    }
    let mut len = 0usize;
    clear_errors();
    let sized = unsafe { EVP_DigestSignFinal(context.0, ptr::null_mut(), &mut len) };
    drain_errors();
    if sized != 1 || len == 0 {
        return Err(AL_CODE);
    }
    let mut output = reserve_zeroed(len)?;
    clear_errors();
    let signed = unsafe { EVP_DigestSignFinal(context.0, output.as_mut_ptr(), &mut len) };
    drain_errors();
    if signed != 1 || len > output.len() || len == 0 {
        return Err(AL_CODE);
    }
    output.truncate(len);
    if algorithm == Algorithm::Rs256 {
        let expected = pkey_size(key.pkey)?;
        if output.len() != expected || !(256..=1024).contains(&expected) {
            return Err(AL_CODE);
        }
        Ok(output)
    } else {
        es_der_to_raw(&output)
    }
}

fn verify(
    key: &CryptoKey,
    algorithm: Algorithm,
    message: &[u8],
    signature: &[u8],
) -> Result<bool, c_int> {
    let engine_signature;
    let signature = if algorithm == Algorithm::Es256 {
        engine_signature = match es_raw_to_der(signature)? {
            Some(der) => der,
            None => return Ok(false),
        };
        engine_signature.as_slice()
    } else {
        signature
    };
    let (context, _) = operation_init(key, algorithm, false)?;
    let (rc, errors) = if algorithm == Algorithm::Ed25519 {
        clear_errors();
        let rc = unsafe {
            EVP_DigestVerify(
                context.0,
                signature.as_ptr(),
                signature.len(),
                message.as_ptr(),
                message.len(),
            )
        };
        (rc, drain_errors())
    } else {
        clear_errors();
        let update =
            unsafe { EVP_DigestVerifyUpdate(context.0, message.as_ptr().cast(), message.len()) };
        let update_errors = drain_errors();
        if update != 1 {
            let _ = update_errors;
            return Err(AL_CODE);
        }
        clear_errors();
        let rc = unsafe { EVP_DigestVerifyFinal(context.0, signature.as_ptr(), signature.len()) };
        (rc, drain_errors())
    };
    verify_result_status(rc, errors)
}

/// Sign the complete message with the exact algorithm selected by the private-key type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_crypto_sign(
    algorithm: c_int,
    key: *mut CryptoKey,
    message_ptr: *const u8,
    message_len: i64,
    out: *mut *mut Buffer,
) -> c_int {
    if output_slot(out).is_none() {
        return AL_INVALID;
    }
    unsafe { *out = ptr::null_mut() };
    let Some(algorithm) = Algorithm::parse(algorithm) else {
        return AL_INVALID;
    };
    let key = match unsafe { checked_key(key, algorithm.private_kind()) } {
        Ok(key) => key,
        Err(status) => return status,
    };
    let message = match input_view(message_ptr, message_len) {
        Ok(message) => message,
        Err(status) => return status,
    };
    match sign(key, algorithm, message) {
        Ok(signature) => match buffer_owner(signature) {
            Ok(buffer) => {
                unsafe { *out = buffer };
                0
            }
            Err(status) => status,
        },
        Err(status) => status,
    }
}

/// Verify an exact public wire signature, publishing one `i32` truth value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_crypto_verify(
    algorithm: c_int,
    key: *mut CryptoKey,
    message_ptr: *const u8,
    message_len: i64,
    signature_ptr: *const u8,
    signature_len: i64,
    out: *mut c_int,
) -> c_int {
    if output_slot(out).is_none() {
        return AL_INVALID;
    }
    unsafe { *out = 0 };
    let Some(algorithm) = Algorithm::parse(algorithm) else {
        return AL_INVALID;
    };
    let key = match unsafe { checked_key(key, algorithm.public_kind()) } {
        Ok(key) => key,
        Err(status) => return status,
    };
    let message = match input_view(message_ptr, message_len) {
        Ok(message) => message,
        Err(status) => return status,
    };
    let signature = match input_view(signature_ptr, signature_len) {
        Ok(signature) => signature,
        Err(status) => return status,
    };
    let expected = match algorithm {
        Algorithm::Rs256 => match pkey_size(key.pkey) {
            Ok(size) => size,
            Err(status) => return status,
        },
        Algorithm::Es256 | Algorithm::Ed25519 => 64,
    };
    if signature.len() != expected {
        return 0;
    }
    match verify(key, algorithm, message, signature) {
        Ok(verified) => {
            unsafe { *out = c_int::from(verified) };
            0
        }
        Err(status) => status,
    }
}

/// Release an asymmetric key owner. Null-safe for moved-from slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_crypto_key_free(key: *mut CryptoKey) {
    if key.is_null() {
        return;
    }
    let storage = key;
    let key = unsafe { key.read() };
    unsafe {
        EVP_PKEY_free(key.pkey);
        OPENSSL_thread_stop_ex(key.libctx);
        unload_provider(key.provider);
        OSSL_LIB_CTX_free(key.libctx);
        free(storage.cast());
    }
    probe::key_freed();
}

#[cfg(feature = "crypto-asymmetric-probe")]
pub fn align_rt_crypto_probe_reset() -> c_int {
    probe::reset()
}

#[cfg(feature = "crypto-asymmetric-probe")]
pub fn align_rt_crypto_probe_live_keys() -> i64 {
    probe::live_keys()
}

#[cfg(feature = "crypto-asymmetric-probe")]
pub fn align_rt_crypto_probe_peak_keys() -> i64 {
    probe::peak_keys()
}

#[cfg(feature = "crypto-asymmetric-probe")]
pub fn align_rt_crypto_probe_live_sensitive() -> i64 {
    probe::live_sensitive()
}

#[cfg(feature = "crypto-asymmetric-probe")]
pub fn align_rt_crypto_probe_peak_sensitive() -> i64 {
    probe::peak_sensitive()
}

#[cfg(feature = "crypto-asymmetric-probe")]
pub fn align_rt_crypto_probe_sensitive_cleanses() -> i64 {
    probe::sensitive_cleanses()
}

#[link(name = "crypto")]
unsafe extern "C" {
    fn CRYPTO_malloc(size: usize, file: *const c_char, line: c_int) -> *mut c_void;
    fn CRYPTO_clear_free(addr: *mut c_void, num: usize, file: *const c_char, line: c_int);
    fn ERR_clear_error();
    fn ERR_get_error() -> c_ulong;
    fn OSSL_LIB_CTX_new() -> *mut c_void;
    fn OSSL_LIB_CTX_free(ctx: *mut c_void);
    fn OPENSSL_thread_stop_ex(ctx: *mut c_void);
    fn OSSL_PROVIDER_load(ctx: *mut c_void, name: *const c_char) -> *mut c_void;
    fn OSSL_PROVIDER_unload(provider: *mut c_void) -> c_int;
    fn EVP_PKEY_free(pkey: *mut c_void);
    fn EVP_PKEY_get0_provider(pkey: *const c_void) -> *const c_void;
    fn EVP_PKEY_is_a(pkey: *const c_void, name: *const c_char) -> c_int;
    fn EVP_PKEY_get_bits(pkey: *const c_void) -> c_int;
    fn EVP_PKEY_get_bn_param(
        pkey: *const c_void,
        name: *const c_char,
        bn: *mut *mut c_void,
    ) -> c_int;
    fn EVP_PKEY_get_utf8_string_param(
        pkey: *const c_void,
        name: *const c_char,
        out: *mut c_char,
        max: usize,
        len: *mut usize,
    ) -> c_int;
    fn EVP_PKEY_get_raw_public_key(pkey: *const c_void, out: *mut u8, len: *mut usize) -> c_int;
    fn EVP_PKEY_CTX_new_from_pkey(
        libctx: *mut c_void,
        pkey: *mut c_void,
        propq: *const c_char,
    ) -> *mut c_void;
    fn EVP_PKEY_CTX_get0_provider(ctx: *const c_void) -> *const c_void;
    fn EVP_PKEY_CTX_free(ctx: *mut c_void);
    fn EVP_PKEY_private_check(ctx: *mut c_void) -> c_int;
    fn EVP_PKEY_public_check(ctx: *mut c_void) -> c_int;
    fn EVP_PKEY_pairwise_check(ctx: *mut c_void) -> c_int;
    fn d2i_PKCS8_PRIV_KEY_INFO(
        info: *mut *mut c_void,
        input: *mut *const u8,
        len: c_long,
    ) -> *mut c_void;
    fn i2d_PKCS8_PRIV_KEY_INFO(info: *const c_void, out: *mut *mut u8) -> c_int;
    fn PKCS8_PRIV_KEY_INFO_free(info: *mut c_void);
    fn PKCS8_pkey_get0(
        object: *mut *const c_void,
        private: *mut *const u8,
        private_len: *mut c_int,
        algorithm: *mut *const c_void,
        info: *const c_void,
    ) -> c_int;
    fn EVP_PKCS82PKEY_ex(
        info: *const c_void,
        libctx: *mut c_void,
        propq: *const c_char,
    ) -> *mut c_void;
    fn d2i_PUBKEY_ex(
        pkey: *mut *mut c_void,
        input: *mut *const u8,
        len: c_long,
        libctx: *mut c_void,
        propq: *const c_char,
    ) -> *mut c_void;
    fn i2d_PUBKEY(pkey: *const c_void, out: *mut *mut u8) -> c_int;
    fn BN_free(bn: *mut c_void);
    fn BN_num_bits(bn: *const c_void) -> c_int;
    fn BN_is_bit_set(bn: *const c_void, bit: c_int) -> c_int;
    fn BN_get_word(bn: *const c_void) -> c_ulong;
    fn BN_bin2bn(input: *const u8, len: c_int, out: *mut c_void) -> *mut c_void;
    fn BN_CTX_new_ex(libctx: *mut c_void) -> *mut c_void;
    fn BN_CTX_start(ctx: *mut c_void);
    fn BN_CTX_get(ctx: *mut c_void) -> *mut c_void;
    fn BN_CTX_end(ctx: *mut c_void);
    fn BN_CTX_free(ctx: *mut c_void);
    fn BN_lebin2bn(input: *const u8, len: c_int, out: *mut c_void) -> *mut c_void;
    fn BN_bn2lebinpad(bn: *const c_void, out: *mut u8, len: c_int) -> c_int;
    fn BN_set_word(bn: *mut c_void, value: c_ulong) -> c_int;
    fn BN_cmp(a: *const c_void, b: *const c_void) -> c_int;
    fn BN_is_zero(a: *const c_void) -> c_int;
    fn BN_mod_add(
        out: *mut c_void,
        a: *const c_void,
        b: *const c_void,
        modulus: *const c_void,
        ctx: *mut c_void,
    ) -> c_int;
    fn BN_mod_sub(
        out: *mut c_void,
        a: *const c_void,
        b: *const c_void,
        modulus: *const c_void,
        ctx: *mut c_void,
    ) -> c_int;
    fn BN_mod_mul(
        out: *mut c_void,
        a: *const c_void,
        b: *const c_void,
        modulus: *const c_void,
        ctx: *mut c_void,
    ) -> c_int;
    fn BN_mod_exp(
        out: *mut c_void,
        a: *const c_void,
        exponent: *const c_void,
        modulus: *const c_void,
        ctx: *mut c_void,
    ) -> c_int;
    fn BN_mod_inverse(
        out: *mut c_void,
        a: *const c_void,
        modulus: *const c_void,
        ctx: *mut c_void,
    ) -> *mut c_void;
    fn OSSL_PARAM_BLD_new() -> *mut c_void;
    fn OSSL_PARAM_BLD_free(builder: *mut c_void);
    fn OSSL_PARAM_BLD_push_BN(
        builder: *mut c_void,
        key: *const c_char,
        value: *const c_void,
    ) -> c_int;
    fn OSSL_PARAM_BLD_push_utf8_string(
        builder: *mut c_void,
        key: *const c_char,
        value: *const c_char,
        len: usize,
    ) -> c_int;
    fn OSSL_PARAM_BLD_push_octet_string(
        builder: *mut c_void,
        key: *const c_char,
        value: *const c_void,
        len: usize,
    ) -> c_int;
    fn OSSL_PARAM_BLD_to_param(builder: *mut c_void) -> *mut c_void;
    fn OSSL_PARAM_free(params: *mut c_void);
    fn EVP_PKEY_CTX_new_from_name(
        libctx: *mut c_void,
        name: *const c_char,
        propq: *const c_char,
    ) -> *mut c_void;
    fn EVP_PKEY_fromdata_init(ctx: *mut c_void) -> c_int;
    fn EVP_PKEY_fromdata(
        ctx: *mut c_void,
        pkey: *mut *mut c_void,
        selection: c_int,
        params: *mut c_void,
    ) -> c_int;
    fn EVP_PKEY_get_size(pkey: *const c_void) -> c_int;
    fn EVP_MD_CTX_new() -> *mut c_void;
    fn EVP_MD_CTX_free(ctx: *mut c_void);
    fn EVP_DigestSignInit_ex(
        ctx: *mut c_void,
        pkey_ctx: *mut *mut c_void,
        digest: *const c_char,
        libctx: *mut c_void,
        props: *const c_char,
        pkey: *mut c_void,
        params: *const c_void,
    ) -> c_int;
    fn EVP_DigestVerifyInit_ex(
        ctx: *mut c_void,
        pkey_ctx: *mut *mut c_void,
        digest: *const c_char,
        libctx: *mut c_void,
        props: *const c_char,
        pkey: *mut c_void,
        params: *const c_void,
    ) -> c_int;
    fn EVP_PKEY_CTX_set_rsa_padding(ctx: *mut c_void, padding: c_int) -> c_int;
    fn EVP_DigestSignUpdate(ctx: *mut c_void, data: *const c_void, len: usize) -> c_int;
    fn EVP_DigestSignFinal(ctx: *mut c_void, signature: *mut u8, len: *mut usize) -> c_int;
    fn EVP_DigestSign(
        ctx: *mut c_void,
        signature: *mut u8,
        signature_len: *mut usize,
        message: *const u8,
        message_len: usize,
    ) -> c_int;
    fn EVP_DigestVerifyUpdate(ctx: *mut c_void, data: *const c_void, len: usize) -> c_int;
    fn EVP_DigestVerifyFinal(ctx: *mut c_void, signature: *const u8, len: usize) -> c_int;
    fn EVP_DigestVerify(
        ctx: *mut c_void,
        signature: *const u8,
        signature_len: usize,
        message: *const u8,
        message_len: usize,
    ) -> c_int;
    fn ECDSA_SIG_new() -> *mut c_void;
    fn ECDSA_SIG_free(signature: *mut c_void);
    fn ECDSA_SIG_get0(signature: *const c_void, r: *mut *const c_void, s: *mut *const c_void);
    fn ECDSA_SIG_set0(signature: *mut c_void, r: *mut c_void, s: *mut c_void) -> c_int;
    fn d2i_ECDSA_SIG(
        signature: *mut *mut c_void,
        input: *mut *const u8,
        len: c_long,
    ) -> *mut c_void;
    fn i2d_ECDSA_SIG(signature: *const c_void, output: *mut *mut u8) -> c_int;
    fn BN_is_negative(bn: *const c_void) -> c_int;
    fn BN_bn2binpad(bn: *const c_void, out: *mut u8, len: c_int) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIVATE_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
MC4CAQAwBQYDK2VwBCIEIJ1hsZ3v/VpguoRK9JLsLMREScVpezJpGXA7rAMcrn9g\n\
-----END PRIVATE KEY-----\n";
    const PUBLIC_PEM: &[u8] = b"-----BEGIN PUBLIC KEY-----\n\
MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=\n\
-----END PUBLIC KEY-----\n";
    const RSA_PRIVATE_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCntoYmRzhBZchy\n\
CTn3CquQE/F44qDNLA/kksO+cAT5ke3KSSUaGzpEYqd9YzpHD7wMbaZmL6i6YlhB\n\
3RHxnq5G4hgkNeZwZdxhl4lsovwqzqkbOVua0MBRk8bg756eOxHxBidK2ws0wbTj\n\
KFgW/4Ze8VQCmAcnnOUBC/vpH2+kXNNbd7WshpytqO6gKC1CiVGn6367m3KcBf85\n\
oVA0WKsR8EU6Rt0AFvZ09rPORhVGPlmKY11swnZXUAWwoV+f386jaMAEOsiYcv1U\n\
wR3FB3uiETZnTDVzKTYSPfaP/J3sK5vHGg8JKz2YgwufuiDOfQgfuBg5wlvgpI2Z\n\
g2udZD91AgMBAAECggEAAtcukntmUoU8zeGmH68IlNohnuCHxLHYWxC5TAJtsyRr\n\
mJ+Ah16qr/nTyGXj2MxFbRh2Vwi7nNyJjiQGZ8c+QGkX65UWIBU5lFWSuEpSZw81\n\
AgcIrCiY+Ui9d5XXY+qwNRrbk4L+P5iATdCjHVCaoAUKXTjh9iPMJSZkz9/+bhQ1\n\
V5Nxb/y90CED43Cv0ZKIsAYRVP9X9kKz5iWUQFhaRV+5o+RCS4+Ieipgid/tYkCS\n\
9hI953Csv2ytFFrTgLt2kkn3ssH/lZSHR6Nt2AU+S+7Mk4Xj3bIceBZ58+U+2PZp\n\
3TSKOgrYb7K5rbKP0D4qOu95XvFy48U4QurMFHy/yQKBgQDoVEDu59bt3qpG81jz\n\
szT9dOpCf8V1c5B5b0RZSAzKQHiO0h77dR3sRiMl+DgEderAjcXz7aBwWd1uzwFc\n\
vr4EwMi7GAaj/u6p7vZr/IcEMOPzTPIHUUzioTSxCLxI7sg81++FFUV8KfR4VmiC\n\
YbWv94DDJNQ80Ira8AlnsIAkLwKBgQC4zOn5LVnvLzUUbXvoUXBn56Uhsjlmaro2\n\
uAh2P6U4hVxDpCSb7xsqFFZjzaURseSdc1fEcD0xyXU9nsOPqnr4/bcV0nFpbL+h\n\
T/LxkHA/0cn5OhJdRsRNZjWLF8+c5gjF4ihdE3or25uiaRcmxc6djcMGHB8Ntw08\n\
jCxLH+ZZmwKBgQC95wiAf27WVlg20HiYEpawyh0lqAz9+S1RpVpn5BXYSCSDEUuT\n\
3OJNm/Lk+WTIeJ4fMINq5IRs8XenOHtzlNH7Tp2FGJls+VeZ/aLdF7hA+7mHyRBY\n\
XOMMqBpKzsogj1WLLjIxRPbcC+sxZefdEwMQx60vVW5KG2g0l1oIsGO9rwKBgDvx\n\
a+uBhGyLOYJ4yPpggD+T6gJ2Fxxbfi+FnmkM2ADvcTAXrDBQbNVHZ4ZUDDkjJO7V\n\
nSCA77iYikkEmJafS+g8FAkmC9eQiNBAaKNmoKJy4DrRVWegLsiUYMXPYW6ZRzs4\n\
0rLuQHC9eUxDHllbTFvawenXcVM3jzmWlj+AB24FAoGBAMZ/bb1JYdVk3OOiRmiY\n\
Le1cLRA3t9Ub1RVVy2JbFG4AlMgHO6laP1t95suuqvQHT57diz+fcpse9pfdLpN9\n\
WD8KnrfXnjvjRN8fsIw+EFDFV+tTnLXtmTRsM0mLHwX6MLWM3liS/1fO9AiOQvSm\n\
34WP4HBG+HR3VIURDl0+e/8Q\n\
-----END PRIVATE KEY-----\n";
    const RSA_PUBLIC_PEM: &[u8] = b"-----BEGIN PUBLIC KEY-----\n\
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAp7aGJkc4QWXIcgk59wqr\n\
kBPxeOKgzSwP5JLDvnAE+ZHtykklGhs6RGKnfWM6Rw+8DG2mZi+oumJYQd0R8Z6u\n\
RuIYJDXmcGXcYZeJbKL8Ks6pGzlbmtDAUZPG4O+enjsR8QYnStsLNMG04yhYFv+G\n\
XvFUApgHJ5zlAQv76R9vpFzTW3e1rIacrajuoCgtQolRp+t+u5tynAX/OaFQNFir\n\
EfBFOkbdABb2dPazzkYVRj5ZimNdbMJ2V1AFsKFfn9/Oo2jABDrImHL9VMEdxQd7\n\
ohE2Z0w1cyk2Ej32j/yd7CubxxoPCSs9mIMLn7ogzn0IH7gYOcJb4KSNmYNrnWQ/\n\
dQIDAQAB\n\
-----END PUBLIC KEY-----\n";
    const EC_PRIVATE_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgvokXYIx6qhF+pXSV\n\
+sk+WJiqw56qUSH+h51EbXb85NihRANCAAQbFZTloBPKHu4ljOfnn1DsU2i1gg59\n\
97jgq4/2RCs8My6It54/isiD6+mniBB7UEePi0TY4NaXmuXXSq+WiA4L\n\
-----END PRIVATE KEY-----\n";
    const EC_PUBLIC_PEM: &[u8] = b"-----BEGIN PUBLIC KEY-----\n\
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEGxWU5aATyh7uJYzn559Q7FNotYIO\n\
ffe44KuP9kQrPDMuiLeeP4rIg+vpp4gQe1BHj4tE2ODWl5rl10qvlogOCw==\n\
-----END PUBLIC KEY-----\n";
    const EMPTY_SIGNATURE: [u8; 64] = [
        0xe5, 0x56, 0x43, 0x00, 0xc3, 0x60, 0xac, 0x72, 0x90, 0x86, 0xe2, 0xcc, 0x80, 0x6e, 0x82,
        0x8a, 0x84, 0x87, 0x7f, 0x1e, 0xb8, 0xe5, 0xd9, 0x74, 0xd8, 0x73, 0xe0, 0x65, 0x22, 0x49,
        0x01, 0x55, 0x5f, 0xb8, 0x82, 0x15, 0x90, 0xa3, 0x3b, 0xac, 0xc6, 0x1e, 0x39, 0x70, 0x1c,
        0xf9, 0xb4, 0x6b, 0xd2, 0x5b, 0xf5, 0xf0, 0x59, 0x5b, 0xbe, 0x24, 0x65, 0x51, 0x41, 0x43,
        0x8e, 0x7a, 0x10, 0x0b,
    ];

    fn base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let a = chunk[0];
            let b = chunk.get(1).copied().unwrap_or(0);
            let c = chunk.get(2).copied().unwrap_or(0);
            output.push(char::from(ALPHABET[usize::from(a >> 2)]));
            output.push(char::from(
                ALPHABET[usize::from(((a & 0x03) << 4) | (b >> 4))],
            ));
            output.push(if chunk.len() > 1 {
                char::from(ALPHABET[usize::from(((b & 0x0f) << 2) | (c >> 6))])
            } else {
                '='
            });
            output.push(if chunk.len() > 2 {
                char::from(ALPHABET[usize::from(c & 0x3f)])
            } else {
                '='
            });
        }
        output
    }

    fn pem_block(label: &str, der: &[u8]) -> Vec<u8> {
        let encoded = base64(der);
        let mut output = format!("-----BEGIN {label}-----\n");
        for line in encoded.as_bytes().chunks(64) {
            output.push_str(std::str::from_utf8(line).unwrap());
            output.push('\n');
        }
        output.push_str(&format!("-----END {label}-----\n"));
        output.into_bytes()
    }

    fn private_key_octets(pem: &[u8]) -> Vec<u8> {
        let der = parse_pem(pem, true).unwrap();
        let mut outer = DerCursor {
            bytes: der.as_slice(),
            pos: 0,
        };
        let sequence = outer.tlv(0x30).unwrap();
        let mut inner = DerCursor {
            bytes: sequence,
            pos: 0,
        };
        let _ = inner.tlv(0x02).unwrap();
        let _ = inner.tlv(0x30).unwrap();
        inner.tlv(0x04).unwrap().to_vec()
    }

    unsafe fn construct_private(algorithm: Algorithm, pem: &[u8]) -> (*mut CryptoKey, c_int) {
        let mut key = 1usize as *mut CryptoKey;
        let status = unsafe {
            align_rt_crypto_private_key_from_pem(
                algorithm as c_int,
                pem.as_ptr(),
                pem.len() as i64,
                &mut key,
            )
        };
        (key, status)
    }

    #[test]
    fn ed25519_rfc_vector_round_trips_through_owned_contexts() {
        unsafe {
            let der = parse_pem(PRIVATE_PEM, true).unwrap();
            validate_der_envelope(der.as_slice(), Algorithm::Ed25519, true).unwrap();
            let mut decoded = KeyParts::new().unwrap();
            decode_private(&mut decoded, &der).unwrap();
            assert_eq!(
                validate_provider_key(&decoded, Algorithm::Ed25519, true),
                Ok(())
            );
            drop(decoded);
            let mut private = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_private_key_from_pem(
                    Algorithm::Ed25519 as c_int,
                    PRIVATE_PEM.as_ptr(),
                    PRIVATE_PEM.len() as i64,
                    &mut private,
                ),
                0,
            );
            let mut public = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_public_key_from_pem(
                    Algorithm::Ed25519 as c_int,
                    PUBLIC_PEM.as_ptr(),
                    PUBLIC_PEM.len() as i64,
                    &mut public,
                ),
                0,
            );
            let mut signature = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_sign(2, private, ptr::null(), 0, &mut signature),
                0
            );
            let signature_bytes = &(&(*signature).data)[..(*signature).len];
            assert_eq!(signature_bytes, EMPTY_SIGNATURE);
            let mut verified = 0;
            assert_eq!(
                align_rt_crypto_verify(
                    2,
                    public,
                    ptr::null(),
                    0,
                    signature_bytes.as_ptr(),
                    signature_bytes.len() as i64,
                    &mut verified,
                ),
                0,
            );
            assert_eq!(verified, 1);
            let mut wrong = EMPTY_SIGNATURE;
            wrong[0] ^= 1;
            assert_eq!(
                align_rt_crypto_verify(
                    2,
                    public,
                    ptr::null(),
                    0,
                    wrong.as_ptr(),
                    64,
                    &mut verified
                ),
                0
            );
            assert_eq!(verified, 0);
            super::super::align_rt_buffer_free(signature);
            align_rt_crypto_key_free(private);
            align_rt_crypto_key_free(public);
        }
    }

    #[test]
    fn jwk_ed25519_rejects_every_small_order_encoding_and_accepts_the_rfc_public_key() {
        const PUBLIC: [u8; 32] = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ];
        unsafe {
            let mut imported = KeyParts::new().unwrap();
            assert_eq!(
                import_jwk(&mut imported, Algorithm::Ed25519, &PUBLIC, &[]),
                Ok(())
            );
            assert_eq!(
                validate_provider_key(&imported, Algorithm::Ed25519, false),
                Ok(())
            );
            drop(imported);
            let mut key = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_public_key_from_jwk(
                    2,
                    PUBLIC.as_ptr(),
                    32,
                    ptr::null(),
                    0,
                    &mut key
                ),
                0
            );
            align_rt_crypto_key_free(key);
            let mut order_two = [0xff; 32];
            order_two[0] = 0xec;
            order_two[31] = 0x7f;
            let mut order_four_negative = [0u8; 32];
            order_four_negative[31] = 0x80;
            let order_eight_a = [
                0xc7, 0x17, 0x6a, 0x70, 0x3d, 0x4d, 0xd8, 0x4f, 0xba, 0x3c, 0x0b, 0x76, 0x0d, 0x10,
                0x67, 0x0f, 0x2a, 0x20, 0x53, 0xfa, 0x2c, 0x39, 0xcc, 0xc6, 0x4e, 0xc7, 0xfd, 0x77,
                0x92, 0xac, 0x03, 0x7a,
            ];
            let mut order_eight_a_negative = order_eight_a;
            order_eight_a_negative[31] |= 0x80;
            let order_eight_b = [
                0x26, 0xe8, 0x95, 0x8f, 0xc2, 0xb2, 0x27, 0xb0, 0x45, 0xc3, 0xf4, 0x89, 0xf2, 0xef,
                0x98, 0xf0, 0xd5, 0xdf, 0xac, 0x05, 0xd3, 0xc6, 0x33, 0x39, 0xb1, 0x38, 0x02, 0x86,
                0xd5, 0x3f, 0xc0, 0x05,
            ];
            let mut order_eight_b_negative = order_eight_b;
            order_eight_b_negative[31] |= 0x80;
            let mut identity = [0u8; 32];
            identity[0] = 1;
            for rejected in [
                [0u8; 32],
                order_four_negative,
                identity,
                order_two,
                order_eight_a,
                order_eight_a_negative,
                order_eight_b,
                order_eight_b_negative,
            ] {
                assert_eq!(
                    align_rt_crypto_public_key_from_jwk(
                        2,
                        rejected.as_ptr(),
                        32,
                        ptr::null(),
                        0,
                        &mut key,
                    ),
                    AL_INVALID,
                );
                assert!(key.is_null());

                let mut spki = b"\x30\x2a\x30\x05\x06\x03\x2b\x65\x70\x03\x21\x00".to_vec();
                spki.extend_from_slice(&rejected);
                let pem = pem_block("PUBLIC KEY", &spki);
                assert_eq!(
                    align_rt_crypto_public_key_from_pem(
                        Algorithm::Ed25519 as c_int,
                        pem.as_ptr(),
                        pem.len() as i64,
                        &mut key,
                    ),
                    AL_INVALID,
                );
                assert!(key.is_null());
            }
        }
    }

    #[test]
    fn rsa_and_p256_use_fixed_public_signature_formats() {
        const MESSAGE: &[u8] = b"binary\0message";
        for (algorithm, private_pem, public_pem, signature_len) in [
            (Algorithm::Rs256, RSA_PRIVATE_PEM, RSA_PUBLIC_PEM, 256usize),
            (Algorithm::Es256, EC_PRIVATE_PEM, EC_PUBLIC_PEM, 64usize),
        ] {
            unsafe {
                let mut private = ptr::null_mut();
                assert_eq!(
                    align_rt_crypto_private_key_from_pem(
                        algorithm as c_int,
                        private_pem.as_ptr(),
                        private_pem.len() as i64,
                        &mut private,
                    ),
                    0,
                );
                let mut public = ptr::null_mut();
                assert_eq!(
                    align_rt_crypto_public_key_from_pem(
                        algorithm as c_int,
                        public_pem.as_ptr(),
                        public_pem.len() as i64,
                        &mut public,
                    ),
                    0,
                );
                let mut signature = ptr::null_mut();
                assert_eq!(
                    align_rt_crypto_sign(
                        algorithm as c_int,
                        private,
                        MESSAGE.as_ptr(),
                        MESSAGE.len() as i64,
                        &mut signature,
                    ),
                    0,
                );
                let bytes = &(&(*signature).data)[..(*signature).len];
                assert_eq!(bytes.len(), signature_len);
                let mut verified = 0;
                assert_eq!(
                    align_rt_crypto_verify(
                        algorithm as c_int,
                        public,
                        MESSAGE.as_ptr(),
                        MESSAGE.len() as i64,
                        bytes.as_ptr(),
                        bytes.len() as i64,
                        &mut verified,
                    ),
                    0,
                );
                assert_eq!(verified, 1);
                let mut wrong = bytes.to_vec();
                let last = wrong.len() - 1;
                wrong[last] ^= 1;
                assert_eq!(
                    align_rt_crypto_verify(
                        algorithm as c_int,
                        public,
                        MESSAGE.as_ptr(),
                        MESSAGE.len() as i64,
                        wrong.as_ptr(),
                        wrong.len() as i64,
                        &mut verified,
                    ),
                    0,
                );
                assert_eq!(verified, 0);
                super::super::align_rt_buffer_free(signature);
                align_rt_crypto_key_free(private);
                align_rt_crypto_key_free(public);
            }
        }
    }

    #[test]
    fn es256_raw_signature_conversion_preserves_padding_and_rejects_zero_components() {
        let mut padded = [0u8; 64];
        padded[31] = 1;
        padded[63] = 1;
        let der = es_raw_to_der(&padded)
            .unwrap()
            .expect("nonzero padded components");
        assert_eq!(es_der_to_raw(&der).unwrap(), padded);

        let mut zero_r = padded;
        zero_r[31] = 0;
        assert!(es_raw_to_der(&zero_r).unwrap().is_none());
        let mut zero_s = padded;
        zero_s[63] = 0;
        assert!(es_raw_to_der(&zero_s).unwrap().is_none());
    }

    #[test]
    fn rsa_and_p256_jwk_components_construct_equivalent_public_keys() {
        const MESSAGE: &[u8] = b"jwk verification";
        const P256_X: [u8; 32] = [
            0x1b, 0x15, 0x94, 0xe5, 0xa0, 0x13, 0xca, 0x1e, 0xee, 0x25, 0x8c, 0xe7, 0xe7, 0x9f,
            0x50, 0xec, 0x53, 0x68, 0xb5, 0x82, 0x0e, 0x7d, 0xf7, 0xb8, 0xe0, 0xab, 0x8f, 0xf6,
            0x44, 0x2b, 0x3c, 0x33,
        ];
        const P256_Y: [u8; 32] = [
            0x2e, 0x88, 0xb7, 0x9e, 0x3f, 0x8a, 0xc8, 0x83, 0xeb, 0xe9, 0xa7, 0x88, 0x10, 0x7b,
            0x50, 0x47, 0x8f, 0x8b, 0x44, 0xd8, 0xe0, 0xd6, 0x97, 0x9a, 0xe5, 0xd7, 0x4a, 0xaf,
            0x96, 0x88, 0x0e, 0x0b,
        ];
        unsafe {
            let mut rsa_private = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_private_key_from_pem(
                    Algorithm::Rs256 as c_int,
                    RSA_PRIVATE_PEM.as_ptr(),
                    RSA_PRIVATE_PEM.len() as i64,
                    &mut rsa_private,
                ),
                0,
            );
            let mut rsa_signature = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_sign(
                    Algorithm::Rs256 as c_int,
                    rsa_private,
                    MESSAGE.as_ptr(),
                    MESSAGE.len() as i64,
                    &mut rsa_signature,
                ),
                0,
            );
            let modulus = get_bn((*rsa_private).pkey, c"n").unwrap();
            let mut n = vec![0u8; 256];
            clear_errors();
            let written = BN_bn2binpad(modulus.0, n.as_mut_ptr(), n.len() as c_int);
            let _errors = drain_errors();
            assert_eq!(written, n.len() as c_int);
            let e = [0x01, 0x00, 0x01];
            let mut rsa_public = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_public_key_from_jwk(
                    Algorithm::Rs256 as c_int,
                    n.as_ptr(),
                    n.len() as i64,
                    e.as_ptr(),
                    e.len() as i64,
                    &mut rsa_public,
                ),
                0,
            );
            let signature = &(&(*rsa_signature).data)[..(*rsa_signature).len];
            let mut verified = 0;
            assert_eq!(
                align_rt_crypto_verify(
                    Algorithm::Rs256 as c_int,
                    rsa_public,
                    MESSAGE.as_ptr(),
                    MESSAGE.len() as i64,
                    signature.as_ptr(),
                    signature.len() as i64,
                    &mut verified,
                ),
                0,
            );
            assert_eq!(verified, 1);
            let mut leading_zero_n = vec![0];
            leading_zero_n.extend_from_slice(&n);
            let mut rejected = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_public_key_from_jwk(
                    Algorithm::Rs256 as c_int,
                    leading_zero_n.as_ptr(),
                    leading_zero_n.len() as i64,
                    e.as_ptr(),
                    e.len() as i64,
                    &mut rejected,
                ),
                AL_INVALID,
            );
            assert!(rejected.is_null());

            let mut ec_private = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_private_key_from_pem(
                    Algorithm::Es256 as c_int,
                    EC_PRIVATE_PEM.as_ptr(),
                    EC_PRIVATE_PEM.len() as i64,
                    &mut ec_private,
                ),
                0,
            );
            let mut ec_signature = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_sign(
                    Algorithm::Es256 as c_int,
                    ec_private,
                    MESSAGE.as_ptr(),
                    MESSAGE.len() as i64,
                    &mut ec_signature,
                ),
                0,
            );
            let mut ec_public = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_public_key_from_jwk(
                    Algorithm::Es256 as c_int,
                    P256_X.as_ptr(),
                    P256_X.len() as i64,
                    P256_Y.as_ptr(),
                    P256_Y.len() as i64,
                    &mut ec_public,
                ),
                0,
            );
            let signature = &(&(*ec_signature).data)[..(*ec_signature).len];
            assert_eq!(
                align_rt_crypto_verify(
                    Algorithm::Es256 as c_int,
                    ec_public,
                    MESSAGE.as_ptr(),
                    MESSAGE.len() as i64,
                    signature.as_ptr(),
                    signature.len() as i64,
                    &mut verified,
                ),
                0,
            );
            assert_eq!(verified, 1);
            assert_eq!(
                align_rt_crypto_public_key_from_jwk(
                    Algorithm::Es256 as c_int,
                    P256_X.as_ptr(),
                    31,
                    P256_Y.as_ptr(),
                    32,
                    &mut rejected,
                ),
                AL_INVALID,
            );
            assert!(rejected.is_null());

            super::super::align_rt_buffer_free(rsa_signature);
            super::super::align_rt_buffer_free(ec_signature);
            align_rt_crypto_key_free(rsa_private);
            align_rt_crypto_key_free(rsa_public);
            align_rt_crypto_key_free(ec_private);
            align_rt_crypto_key_free(ec_public);
        }
    }

    #[test]
    fn pem_grammar_is_exact_and_the_byte_limit_is_closed() {
        assert!(parse_pem(PRIVATE_PEM, true).is_ok());
        let crlf = PRIVATE_PEM
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .flat_map(|line| line.iter().copied().chain([b'\r', b'\n']))
            .collect::<Vec<_>>();
        assert!(parse_pem(&crlf, true).is_ok());

        let private_text = std::str::from_utf8(PRIVATE_PEM).unwrap();
        for malformed in [
            [b"prefix".as_slice(), PRIVATE_PEM].concat(),
            [PRIVATE_PEM, b"x"].concat(),
            [PRIVATE_PEM, PRIVATE_PEM].concat(),
            private_text
                .replace("PRIVATE KEY", "RSA PRIVATE KEY")
                .into_bytes(),
            private_text.replace("crn9g", "crn9!").into_bytes(),
        ] {
            assert_eq!(parse_pem(&malformed, true).err(), Some(AL_INVALID));
        }

        let mut at_limit = b"-----BEGIN PRIVATE KEY-----\n".to_vec();
        for index in 0..1007 {
            at_limit.extend_from_slice(&[b'A'; 64]);
            at_limit.extend_from_slice(if index < 3 { b"\r\n" } else { b"\n" });
        }
        at_limit.extend_from_slice(&[b'A'; 24]);
        at_limit.extend_from_slice(b"\n-----END PRIVATE KEY-----");
        assert_eq!(at_limit.len(), PEM_MAX);
        assert!(parse_pem(&at_limit, true).is_ok());
        at_limit.push(b'\n');
        assert_eq!(parse_pem(&at_limit, true).err(), Some(AL_INVALID));
    }

    #[test]
    fn constructor_matrix_rejects_legacy_version_trailing_and_noncanonical_der() {
        unsafe {
            for (algorithm, private_pem) in [
                (Algorithm::Rs256, RSA_PRIVATE_PEM),
                (Algorithm::Es256, EC_PRIVATE_PEM),
            ] {
                let legacy = pem_block("PRIVATE KEY", &private_key_octets(private_pem));
                let (key, status) = construct_private(algorithm, &legacy);
                assert_eq!(status, AL_INVALID, "relabeled legacy {algorithm:?}");
                assert!(key.is_null());
            }

            let (key, status) = construct_private(Algorithm::Ed25519, RSA_PRIVATE_PEM);
            assert_eq!(status, AL_INVALID, "wrong algorithm");
            assert!(key.is_null());

            let canonical = parse_pem(PRIVATE_PEM, true).unwrap().as_slice().to_vec();
            let version_at = canonical
                .windows(3)
                .position(|window| window == [0x02, 0x01, 0x00])
                .expect("PKCS#8 version");
            let mut version_one = canonical.clone();
            version_one[version_at + 2] = 1;
            let (key, status) =
                construct_private(Algorithm::Ed25519, &pem_block("PRIVATE KEY", &version_one));
            assert_eq!(status, AL_INVALID, "OneAsymmetricKey version");
            assert!(key.is_null());

            let mut trailing = canonical.clone();
            trailing.push(0);
            let (key, status) =
                construct_private(Algorithm::Ed25519, &pem_block("PRIVATE KEY", &trailing));
            assert_eq!(status, AL_INVALID, "trailing DER octet");
            assert!(key.is_null());

            assert!(canonical[1] < 0x80);
            let mut noncanonical = Vec::with_capacity(canonical.len() + 1);
            noncanonical.extend_from_slice(&[canonical[0], 0x81, canonical[1]]);
            noncanonical.extend_from_slice(&canonical[2..]);
            let (key, status) =
                construct_private(Algorithm::Ed25519, &pem_block("PRIVATE KEY", &noncanonical));
            assert_eq!(status, AL_INVALID, "noncanonical DER length");
            assert!(key.is_null());

            let mut with_attributes = canonical;
            with_attributes[1] += 2;
            with_attributes.extend_from_slice(&[0xa0, 0x00]);
            let (key, status) = construct_private(
                Algorithm::Ed25519,
                &pem_block("PRIVATE KEY", &with_attributes),
            );
            assert_eq!(status, 0, "canonical empty PKCS#8 attributes");
            align_rt_crypto_key_free(key);
        }
    }

    #[test]
    fn native_result_and_error_queue_products_are_disjoint() {
        let empty = ErrorQueue {
            empty: true,
            decoder_input_only: true,
            import_input_only: true,
            verify_mismatch_only: true,
        };
        let input = ErrorQueue {
            empty: false,
            decoder_input_only: true,
            import_input_only: true,
            verify_mismatch_only: true,
        };
        let code = ErrorQueue {
            empty: false,
            decoder_input_only: false,
            import_input_only: false,
            verify_mismatch_only: false,
        };
        for queue in [empty, input, code] {
            assert_eq!(check_result_status(1, queue), Ok(()));
            assert_eq!(verify_result_status(1, queue), Ok(true));
            assert_eq!(check_result_status(-1, queue), Err(AL_CODE));
            assert_eq!(check_result_status(-2, queue), Err(AL_CODE));
            assert_eq!(verify_result_status(-1, queue), Err(AL_CODE));
            assert_eq!(verify_result_status(2, queue), Err(AL_CODE));
        }
        assert_eq!(check_result_status(0, empty), Err(AL_INVALID));
        assert_eq!(check_result_status(0, input), Err(AL_INVALID));
        assert_eq!(check_result_status(0, code), Err(AL_CODE));
        assert_eq!(verify_result_status(0, empty), Ok(false));
        assert_eq!(verify_result_status(0, input), Ok(false));
        assert_eq!(verify_result_status(0, code), Err(AL_CODE));
    }

    #[test]
    fn stale_and_other_thread_error_queues_do_not_cross_an_operation() {
        use std::sync::{Arc, Barrier};

        unsafe {
            clear_errors();
            let missing = EVP_PKEY_CTX_new_from_name(
                ptr::null_mut(),
                c"ALIGN-NOT-A-REAL-ALGORITHM".as_ptr(),
                ptr::null(),
            );
            assert!(missing.is_null());
            let mut key = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_public_key_from_pem(
                    Algorithm::Ed25519 as c_int,
                    PUBLIC_PEM.as_ptr(),
                    PUBLIC_PEM.len() as i64,
                    &mut key,
                ),
                0,
            );
            assert!(drain_errors().empty);
            align_rt_crypto_key_free(key);
        }

        let barrier = Arc::new(Barrier::new(2));
        let child_barrier = Arc::clone(&barrier);
        let child = std::thread::spawn(move || unsafe {
            clear_errors();
            let missing = EVP_PKEY_CTX_new_from_name(
                ptr::null_mut(),
                c"ALIGN-NOT-A-REAL-ALGORITHM".as_ptr(),
                ptr::null(),
            );
            assert!(missing.is_null());
            child_barrier.wait();
            child_barrier.wait();
            assert_ne!(ERR_get_error(), 0);
            clear_errors();
        });
        barrier.wait();
        unsafe {
            let mut key = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_public_key_from_pem(
                    Algorithm::Ed25519 as c_int,
                    PUBLIC_PEM.as_ptr(),
                    PUBLIC_PEM.len() as i64,
                    &mut key,
                ),
                0,
            );
            assert!(drain_errors().empty);
            align_rt_crypto_key_free(key);
        }
        barrier.wait();
        child.join().unwrap();
    }

    #[test]
    fn isolated_key_context_ignores_hostile_global_provider_state() {
        const CHILD: &str = "ALIGN_CRYPTO_HOSTILE_GLOBAL_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "crypto_asymmetric::tests::isolated_key_context_ignores_hostile_global_provider_state",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .status()
                .expect("spawn hostile-global crypto child");
            assert!(status.success());
            return;
        }

        unsafe extern "C" {
            fn EVP_set_default_properties(libctx: *mut c_void, propq: *const c_char) -> c_int;
        }
        unsafe {
            clear_errors();
            let null_provider = OSSL_PROVIDER_load(ptr::null_mut(), c"null".as_ptr());
            let _errors = drain_errors();
            assert!(!null_provider.is_null());
            assert_eq!(
                EVP_set_default_properties(
                    ptr::null_mut(),
                    c"provider=align-no-such-provider".as_ptr(),
                ),
                1,
            );
            let mut key = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_public_key_from_pem(
                    Algorithm::Ed25519 as c_int,
                    PUBLIC_PEM.as_ptr(),
                    PUBLIC_PEM.len() as i64,
                    &mut key,
                ),
                0,
            );
            align_rt_crypto_key_free(key);
            assert_eq!(EVP_set_default_properties(ptr::null_mut(), ptr::null()), 1);
            assert_eq!(OSSL_PROVIDER_unload(null_provider), 1);
            assert!(drain_errors().empty);
        }
    }

    #[test]
    fn abi_precedence_zeroes_outputs_before_rejecting_inputs() {
        #[cfg(target_pointer_width = "32")]
        assert_eq!(
            input_view(
                NonNull::<u8>::dangling().as_ptr(),
                i64::try_from(isize::MAX).unwrap() + 1,
            ),
            Err(AL_INVALID),
            "a byte view larger than isize::MAX must reject before from_raw_parts",
        );

        unsafe {
            let sentinel = 1usize as *mut CryptoKey;
            let mut key = sentinel;
            assert_eq!(
                align_rt_crypto_public_key_from_jwk(99, ptr::null(), 0, ptr::null(), 0, &mut key,),
                AL_INVALID,
            );
            assert!(key.is_null());

            assert_eq!(
                align_rt_crypto_public_key_from_jwk(
                    Algorithm::Ed25519 as c_int,
                    ptr::null(),
                    32,
                    ptr::null(),
                    0,
                    &mut key,
                ),
                AL_INVALID,
            );
            assert!(key.is_null());
            let public = [0u8; 32];
            assert_eq!(
                align_rt_crypto_public_key_from_jwk(
                    Algorithm::Ed25519 as c_int,
                    public.as_ptr(),
                    32,
                    public.as_ptr(),
                    0,
                    &mut key,
                ),
                AL_INVALID,
            );
            assert!(key.is_null());

            let mut signature = 1usize as *mut Buffer;
            assert_eq!(
                align_rt_crypto_sign(
                    Algorithm::Ed25519 as c_int,
                    1usize as *mut CryptoKey,
                    ptr::null(),
                    0,
                    &mut signature,
                ),
                AL_INVALID,
            );
            assert!(signature.is_null());
            let mut verified = 7;
            assert_eq!(
                align_rt_crypto_verify(
                    Algorithm::Ed25519 as c_int,
                    1usize as *mut CryptoKey,
                    ptr::null(),
                    0,
                    ptr::null(),
                    0,
                    &mut verified,
                ),
                AL_INVALID,
            );
            assert_eq!(verified, 0);
        }
    }

    #[test]
    fn abi_kind_recheck_rejects_swaps_without_consuming_either_key() {
        unsafe {
            let mut private = ptr::null_mut();
            let mut public = ptr::null_mut();
            assert_eq!(
                align_rt_crypto_private_key_from_pem(
                    Algorithm::Ed25519 as c_int,
                    PRIVATE_PEM.as_ptr(),
                    PRIVATE_PEM.len() as i64,
                    &mut private,
                ),
                0,
            );
            assert_eq!(
                align_rt_crypto_public_key_from_pem(
                    Algorithm::Ed25519 as c_int,
                    PUBLIC_PEM.as_ptr(),
                    PUBLIC_PEM.len() as i64,
                    &mut public,
                ),
                0,
            );

            let mut signature = 1usize as *mut Buffer;
            let private_kind = (*private).kind;
            (*private).kind = u8::MAX;
            assert_eq!(
                align_rt_crypto_sign(
                    Algorithm::Ed25519 as c_int,
                    private,
                    ptr::null(),
                    0,
                    &mut signature,
                ),
                AL_INVALID,
                "an unknown future shell kind must reject without interpreting an enum discriminant",
            );
            assert!(signature.is_null());
            (*private).kind = private_kind;

            assert_eq!(
                align_rt_crypto_sign(
                    Algorithm::Ed25519 as c_int,
                    public,
                    ptr::null(),
                    0,
                    &mut signature,
                ),
                AL_INVALID,
            );
            assert!(signature.is_null());
            assert_eq!(
                align_rt_crypto_sign(
                    Algorithm::Rs256 as c_int,
                    private,
                    ptr::null(),
                    0,
                    &mut signature,
                ),
                AL_INVALID,
            );
            assert!(signature.is_null());

            assert_eq!(
                align_rt_crypto_sign(
                    Algorithm::Ed25519 as c_int,
                    private,
                    ptr::null(),
                    0,
                    &mut signature,
                ),
                0,
            );
            let bytes = &(&(*signature).data)[..(*signature).len];
            let mut verified = 7;
            assert_eq!(
                align_rt_crypto_verify(
                    Algorithm::Ed25519 as c_int,
                    private,
                    ptr::null(),
                    0,
                    bytes.as_ptr(),
                    bytes.len() as i64,
                    &mut verified,
                ),
                AL_INVALID,
            );
            assert_eq!(verified, 0);
            assert_eq!(
                align_rt_crypto_verify(
                    Algorithm::Ed25519 as c_int,
                    public,
                    ptr::null(),
                    0,
                    bytes.as_ptr(),
                    bytes.len() as i64,
                    &mut verified,
                ),
                0,
            );
            assert_eq!(verified, 1);

            super::super::align_rt_buffer_free(signature);
            align_rt_crypto_key_free(private);
            align_rt_crypto_key_free(public);
        }
    }

    fn sweep_key_allocation_failures(
        label: &str,
        construct: impl Fn(*mut *mut CryptoKey) -> c_int,
    ) -> usize {
        for after in 0..128 {
            set_allocation_failpoint(Some(after));
            let mut key = 1usize as *mut CryptoKey;
            let status = construct(&mut key);
            if allocation_failpoint_triggered() {
                assert_eq!(status, AL_CODE, "{label} failpoint {after}");
                assert!(key.is_null(), "{label} failpoint {after} published a key");
                assert_eq!(probe::live_keys(), 0, "{label} failpoint {after}");
                assert_eq!(probe::live_sensitive(), 0, "{label} failpoint {after}");
                continue;
            }
            assert_eq!(status, 0, "{label} terminal construction");
            assert!(!key.is_null(), "{label} terminal construction");
            unsafe { align_rt_crypto_key_free(key) };
            assert_eq!(probe::live_keys(), 0, "{label} terminal free");
            assert_eq!(probe::live_sensitive(), 0, "{label} terminal free");
            set_allocation_failpoint(None);
            return after;
        }
        panic!("{label} allocation sweep did not reach success");
    }

    #[test]
    fn constructor_and_operation_allocation_failpoints_publish_nothing_and_unwind() {
        const ED25519_PUBLIC: [u8; 32] = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ];
        assert_eq!(probe::reset(), 0);
        set_allocation_failpoint(Some(0));
        let mut rejected = 1usize as *mut CryptoKey;
        assert_eq!(
            unsafe {
                align_rt_crypto_public_key_from_jwk(
                    Algorithm::Ed25519 as c_int,
                    ED25519_PUBLIC.as_ptr(),
                    31,
                    ptr::null(),
                    0,
                    &mut rejected,
                )
            },
            AL_INVALID,
        );
        assert!(rejected.is_null());
        assert!(
            !allocation_failpoint_triggered(),
            "cheap JWK shape validation must precede context allocation",
        );
        let private_points = sweep_key_allocation_failures("private PEM", |out| unsafe {
            align_rt_crypto_private_key_from_pem(
                Algorithm::Ed25519 as c_int,
                PRIVATE_PEM.as_ptr(),
                PRIVATE_PEM.len() as i64,
                out,
            )
        });
        let public_points = sweep_key_allocation_failures("public PEM", |out| unsafe {
            align_rt_crypto_public_key_from_pem(
                Algorithm::Ed25519 as c_int,
                PUBLIC_PEM.as_ptr(),
                PUBLIC_PEM.len() as i64,
                out,
            )
        });
        let jwk_points = sweep_key_allocation_failures("public JWK", |out| unsafe {
            align_rt_crypto_public_key_from_jwk(
                Algorithm::Ed25519 as c_int,
                ED25519_PUBLIC.as_ptr(),
                ED25519_PUBLIC.len() as i64,
                ptr::null(),
                0,
                out,
            )
        });
        assert!(
            private_points >= 30,
            "private sweep covered {private_points}"
        );
        assert!(public_points >= 30, "public sweep covered {public_points}");
        assert!(jwk_points >= 35, "JWK sweep covered {jwk_points}");
        assert!(probe::sensitive_cleanses() > 0);

        set_allocation_failpoint(None);
        let mut private = ptr::null_mut();
        let mut public = ptr::null_mut();
        unsafe {
            assert_eq!(
                align_rt_crypto_private_key_from_pem(
                    Algorithm::Ed25519 as c_int,
                    PRIVATE_PEM.as_ptr(),
                    PRIVATE_PEM.len() as i64,
                    &mut private,
                ),
                0,
            );
            assert_eq!(
                align_rt_crypto_public_key_from_pem(
                    Algorithm::Ed25519 as c_int,
                    PUBLIC_PEM.as_ptr(),
                    PUBLIC_PEM.len() as i64,
                    &mut public,
                ),
                0,
            );
        }
        assert_eq!(probe::live_keys(), 2);

        let mut signature = ptr::null_mut();
        let sign_points = (0..32)
            .find(|after| {
                set_allocation_failpoint(Some(*after));
                signature = 1usize as *mut Buffer;
                let status = unsafe {
                    align_rt_crypto_sign(
                        Algorithm::Ed25519 as c_int,
                        private,
                        b"message".as_ptr(),
                        7,
                        &mut signature,
                    )
                };
                if allocation_failpoint_triggered() {
                    assert_eq!(status, AL_CODE, "sign failpoint {after}");
                    assert!(signature.is_null(), "sign failpoint {after}");
                    assert_eq!(probe::live_keys(), 2);
                    false
                } else {
                    assert_eq!(status, 0);
                    true
                }
            })
            .expect("sign allocation sweep reaches success");
        assert!(sign_points >= 3, "sign sweep covered {sign_points}");

        let signature_bytes = unsafe { &(&(*signature).data)[..(*signature).len] };
        let mut verified = 7;
        let verify_points = (0..32)
            .find(|after| {
                set_allocation_failpoint(Some(*after));
                verified = 7;
                let status = unsafe {
                    align_rt_crypto_verify(
                        Algorithm::Ed25519 as c_int,
                        public,
                        b"message".as_ptr(),
                        7,
                        signature_bytes.as_ptr(),
                        signature_bytes.len() as i64,
                        &mut verified,
                    )
                };
                if allocation_failpoint_triggered() {
                    assert_eq!(status, AL_CODE, "verify failpoint {after}");
                    assert_eq!(verified, 0, "verify failpoint {after}");
                    assert_eq!(probe::live_keys(), 2);
                    false
                } else {
                    assert_eq!(status, 0);
                    assert_eq!(verified, 1);
                    true
                }
            })
            .expect("verify allocation sweep reaches success");
        assert!(verify_points >= 2, "verify sweep covered {verify_points}");

        set_allocation_failpoint(None);
        unsafe {
            super::super::align_rt_buffer_free(signature);
            align_rt_crypto_key_free(private);
            align_rt_crypto_key_free(public);
        }
        assert_eq!(probe::live_keys(), 0);
        assert_eq!(probe::live_sensitive(), 0);
        assert!(probe::peak_keys() >= 2);
        assert!(probe::peak_sensitive() >= 2);
    }

    #[test]
    fn malformed_inner_der_and_closed_reason_sets_classify_without_stale_state() {
        let der = SensitiveDer::new(2).unwrap();
        unsafe { ptr::copy_nonoverlapping(b"\x30\x00".as_ptr(), der.ptr.as_ptr(), 2) };
        let mut parts = KeyParts::new().unwrap();
        assert_eq!(unsafe { decode_private(&mut parts, &der) }, Err(AL_INVALID));
        assert!(parts.pkey.is_null());
        assert!(drain_errors().empty);

        for reason in [
            ASN1_R_BAD_OBJECT_HEADER,
            ASN1_R_DECODE_ERROR,
            ASN1_R_WRONG_TAG,
            ERR_R_NESTED_ASN1_ERROR,
        ] {
            assert!(asn1_input_reason(reason), "ASN.1 reason {reason}");
            assert!(import_input_reason(ERR_LIB_ASN1, reason));
        }
        for &reason in RSA_IMPORT_INPUT_REASONS {
            assert!(import_input_reason(ERR_LIB_RSA, reason));
        }
        for &reason in EC_IMPORT_INPUT_REASONS {
            assert!(import_input_reason(ERR_LIB_EC, reason));
        }
        for &reason in RSA_VERIFY_MISMATCH_REASONS {
            assert!(verify_mismatch_reason(ERR_LIB_RSA, reason));
        }
        assert_eq!(error_parts(ERR_SYSTEM_FLAG | 7), None);
        assert_eq!(
            error_parts((ERR_LIB_ASN1 << ERR_LIB_OFFSET) | ERR_RFLAG_FATAL | ASN1_R_WRONG_TAG),
            None,
        );
        assert!(!import_input_reason(ERR_LIB_EVP, 1));
        assert!(!verify_mismatch_reason(ERR_LIB_PROV, 1));
    }

    #[test]
    fn source_audit_keeps_private_material_behind_high_level_provider_operations() {
        let source = include_str!("crypto_asymmetric.rs");
        let production = &source[..source
            .rfind("#[cfg(test)]\nmod tests")
            .expect("test module marker")];
        for forbidden in [
            concat!("d2i_Auto", "PrivateKey_ex"),
            concat!("RSA_", "get0_key"),
            concat!("EC_KEY_", "get0_private_key"),
            concat!("EVP_PKEY_get_raw_", "private_key"),
            concat!("OSSL_PROVIDER_set_default_", "search_path"),
            concat!("EVP_set_default_", "properties"),
        ] {
            assert!(
                !production.contains(forbidden),
                "forbidden production API {forbidden}"
            );
        }
        for required in [
            "EVP_PKCS82PKEY_ex",
            "EVP_DigestSignInit_ex",
            "EVP_DigestSignUpdate",
            "EVP_DigestSignFinal",
            "EVP_DigestSign(",
            "EVP_PKEY_CTX_set_rsa_padding",
            "EVP_PKEY_get0_provider",
            "EVP_PKEY_CTX_get0_provider",
            "provider=default",
            "CRYPTO_clear_free",
        ] {
            assert!(
                production.contains(required),
                "missing audited API {required}"
            );
        }
    }
}
