//! HTTPS clients with an immutable origin, verification store, and optional identity.
use super::crypto_asymmetric::{SensitiveDer, decode_base64_digits};
use super::{
    AL_CODE, AL_DENIED, AL_INVALID, HttpClient, HttpRequestView, HttpScheme, TlsCtx, http_is_token,
    http_split_authority, http_split_url, http_upgrade_header_value_valid,
};
use std::{
    ffi::c_void,
    fmt::Write,
    net::{Ipv4Addr, Ipv6Addr},
    ptr,
};

#[cfg(test)]
#[path = "http_tls_tests.rs"]
mod tests;

const MAX_PEM: usize = 4 << 20;
const MAX_ORIGIN: usize = 268;
const MAX_KEY: usize = 64 << 10;

#[derive(Eq, PartialEq)]
struct Origin {
    host: String,
    port: i64,
}

pub(super) struct Policy {
    origin: Origin,
    pub(super) ctx: TlsCtx,
    pub(super) revocation: bool,
    #[cfg(test)]
    verification_time: std::sync::atomic::AtomicI64,
}

impl Origin {
    fn parse(authority: &str, scheme: HttpScheme) -> Result<Self, i32> {
        let default = if scheme == HttpScheme::Https { 443 } else { 80 };
        let (host, port) = http_split_authority(authority, default).ok_or(AL_INVALID)?;
        let bracketed = authority.starts_with('[');
        let suffix = if bracketed {
            authority.split_once(']').ok_or(AL_INVALID)?.1
        } else {
            &authority[host.len()..]
        };
        if !suffix.is_empty()
            && !suffix.strip_prefix(':').is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
            })
        {
            return Err(AL_INVALID);
        }
        if host.len() > 253 || host.is_empty() {
            return Err(AL_INVALID);
        }
        let mut canonical = String::new();
        canonical
            .try_reserve_exact(host.len().max(46))
            .map_err(|_| AL_CODE)?;
        if bracketed {
            let ip = host.parse::<Ipv6Addr>().map_err(|_| AL_INVALID)?;
            write!(&mut canonical, "{ip}").map_err(|_| AL_CODE)?;
        } else if let Ok(ip) = host.parse::<Ipv4Addr>() {
            write!(&mut canonical, "{ip}").map_err(|_| AL_CODE)?;
        } else {
            // Exclude forms libc resolvers can interpret as historical numeric addresses.
            let numeric = |label: &str| {
                label.bytes().all(|b| b.is_ascii_digit())
                    || label
                        .strip_prefix("0x")
                        .or_else(|| label.strip_prefix("0X"))
                        .is_some_and(|hex| {
                            !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit())
                        })
            };
            if host.split('.').all(numeric)
                || !host.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                return Err(AL_INVALID);
            }
            canonical.push_str(host);
            canonical.make_ascii_lowercase();
        }
        Ok(Self {
            host: canonical,
            port,
        })
    }
}

pub(super) fn destination<'a>(
    client: Option<&'a HttpClient>,
    req: HttpRequestView<'a>,
) -> Result<(HttpScheme, &'a str, i64), i32> {
    if req.method == "CONNECT" {
        return Err(AL_INVALID);
    }
    let scoped = client.and_then(|c| c.policy.as_ref());
    if scoped.is_some() && !req.method.is_empty() && !http_is_token(req.method.as_bytes()) {
        return Err(AL_INVALID);
    }
    let (scheme, authority, path) = http_split_url(req.url).ok_or(AL_INVALID)?;
    if let Some(policy) = scoped {
        let candidate = Origin::parse(authority, scheme)?;
        if !path
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b'#')
            || req.headers.iter().any(|(name, value)| {
                !http_is_token(name.as_bytes())
                    || name.eq_ignore_ascii_case("host")
                    || name.eq_ignore_ascii_case("content-length")
                    || !http_upgrade_header_value_valid(value.as_bytes())
            })
        {
            return Err(AL_INVALID);
        }
        if scheme != HttpScheme::Https || candidate != policy.origin {
            return Err(AL_DENIED);
        }
        Ok((scheme, &policy.origin.host, policy.origin.port))
    } else {
        let default = if scheme == HttpScheme::Https { 443 } else { 80 };
        let (host, port) = http_split_authority(authority, default).ok_or(AL_INVALID)?;
        Ok((scheme, host, port))
    }
}

#[repr(C)]
struct DerView {
    ptr: *const u8,
    len: usize,
}
unsafe extern "C" {
    fn align_tls_policy_init(system_trust: i32, out: *mut *mut c_void) -> i32;
    fn align_tls_policy_field(
        ctx: *mut c_void,
        field: usize,
        items: *const DerView,
        count: usize,
    ) -> i32;
    fn align_tls_crls_current(ctx: *mut c_void, at: *const libc::time_t) -> i32;
}

impl Policy {
    pub(super) fn evidence_current(&self) -> bool {
        #[cfg(test)]
        let at = match self
            .verification_time
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0 => None,
            seconds => Some(seconds as libc::time_t),
        };
        #[cfg(not(test))]
        let at: Option<libc::time_t> = None;
        !self.revocation
            || unsafe {
                align_tls_crls_current(self.ctx.0, at.as_ref().map_or(ptr::null(), ptr::from_ref))
                    == 1
            }
    }
}

fn pem_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

// All blocks are bounded and counted before any native object is decoded.
fn decode_list(
    mut input: &[u8],
    begin: &[u8],
    end: &[u8],
    key: bool,
) -> Result<Vec<SensitiveDer>, i32> {
    let mut blocks = Vec::new();
    let nonempty = !input.is_empty();
    loop {
        let start = input
            .iter()
            .position(|&b| !pem_space(b))
            .unwrap_or(input.len());
        input = &input[start..];
        if input.is_empty() {
            break;
        }
        if blocks.len() >= (if key { 1 } else { 64 }) {
            return Err(AL_INVALID);
        }
        let header = input.strip_prefix(begin).ok_or(AL_INVALID)?;
        input = header
            .strip_prefix(b"\r\n")
            .or_else(|| header.strip_prefix(b"\n"))
            .ok_or(AL_INVALID)?;
        let body = input;
        let mut chars = 0;
        while !input.starts_with(end) {
            let nl = memchr::memchr(b'\n', input).ok_or(AL_INVALID)?;
            let line = input[..nl].strip_suffix(b"\r").unwrap_or(&input[..nl]);
            if line.is_empty()
                || !line
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
            {
                return Err(AL_INVALID);
            }
            chars += line.len(); // bounded by the admitted 4 MiB descriptor
            input = &input[nl + 1..];
        }
        let encoded = &body[..body.len() - input.len()];
        input = &input[end.len()..];
        if chars == 0 || input.first().is_some_and(|b| !pem_space(*b)) {
            return Err(AL_INVALID);
        }
        blocks.try_reserve(1).map_err(|_| AL_CODE)?;
        blocks.push((encoded, chars));
    }
    if nonempty && blocks.is_empty() {
        return Err(AL_INVALID);
    }
    let mut ders = Vec::new();
    ders.try_reserve_exact(blocks.len()).map_err(|_| AL_CODE)?;
    for (encoded, chars) in blocks {
        let digits = encoded
            .iter()
            .copied()
            .filter(|b| !matches!(b, b'\r' | b'\n'));
        ders.push(decode_base64_digits(
            digits,
            chars,
            if key { MAX_KEY } else { MAX_PEM },
        )?);
    }
    Ok(ders)
}

unsafe fn view<'a>(ptr: *const u8, len: i64, cap: usize) -> Result<&'a [u8], i32> {
    let n = usize::try_from(len).map_err(|_| AL_INVALID)?;
    if n == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() || n > cap || ptr.addr().checked_add(n).is_none() {
        return Err(AL_INVALID);
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, n) })
}

fn create(
    origin: &[u8],
    trust: &[u8],
    chain: &[u8],
    key: &[u8],
    crls: &[u8],
) -> Result<Policy, i32> {
    let text = std::str::from_utf8(origin).map_err(|_| AL_INVALID)?;
    let (scheme, authority, path) = http_split_url(text).ok_or(AL_INVALID)?;
    if scheme != HttpScheme::Https || path != "/" {
        return Err(AL_INVALID);
    }
    let origin = Origin::parse(authority, scheme)?;
    if chain.is_empty() != key.is_empty() || (!crls.is_empty() && trust.is_empty()) {
        return Err(AL_INVALID);
    }
    let mut native = ptr::null_mut();
    let status = unsafe { align_tls_policy_init(i32::from(trust.is_empty()), &mut native) };
    if status != 0 {
        return Err(status);
    }
    if native.is_null() {
        return Err(AL_CODE);
    }
    let ctx = TlsCtx(native);
    let cert = (
        b"-----BEGIN CERTIFICATE-----".as_slice(),
        b"-----END CERTIFICATE-----".as_slice(),
    );
    let labels = [
        cert,
        cert,
        (b"-----BEGIN PRIVATE KEY-----", b"-----END PRIVATE KEY-----"),
        (b"-----BEGIN X509 CRL-----", b"-----END X509 CRL-----"),
    ];
    for (field, (pem, (begin, end))) in [trust, chain, key, crls]
        .into_iter()
        .zip(labels)
        .enumerate()
    {
        if pem.is_empty() {
            continue;
        }
        let decoded = decode_list(pem, begin, end, field == 2)?;
        let mut views = Vec::new();
        views
            .try_reserve_exact(decoded.len())
            .map_err(|_| AL_CODE)?;
        views.extend(decoded.iter().map(|der| DerView {
            ptr: der.as_slice().as_ptr(),
            len: der.as_slice().len(),
        }));
        let status = unsafe { align_tls_policy_field(ctx.0, field, views.as_ptr(), views.len()) };
        if status != 0 {
            return Err(status);
        }
    }
    Ok(Policy {
        origin,
        ctx,
        revocation: !crls.is_empty(),
        #[cfg(test)]
        verification_time: Default::default(),
    })
}

/// Construct a client from five borrowed, explicitly sized inputs.
/// # Safety
/// Each nonempty descriptor must designate readable bytes and the output must be writable, disjoint storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_http_tls_client_new(
    origin: *const u8,
    origin_len: i64,
    trust: *const u8,
    trust_len: i64,
    chain: *const u8,
    chain_len: i64,
    key: *const u8,
    key_len: i64,
    crls: *const u8,
    crls_len: i64,
    out: *mut *mut HttpClient,
) -> i32 {
    if out.is_null() || !out.is_aligned() {
        return AL_INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    let construct = || unsafe {
        let policy = create(
            view(origin, origin_len, MAX_ORIGIN)?,
            view(trust, trust_len, MAX_PEM)?,
            view(chain, chain_len, MAX_PEM)?,
            view(key, key_len, MAX_PEM)?,
            view(crls, crls_len, MAX_PEM)?,
        )?;
        let raw = std::alloc::alloc(std::alloc::Layout::new::<HttpClient>()).cast::<HttpClient>();
        if raw.is_null() {
            return Err(AL_CODE);
        }
        raw.write(HttpClient::new(Some(policy)));
        out.write(raw);
        Ok(())
    };
    construct().err().unwrap_or(0)
}
