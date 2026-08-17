//! Canonical, deterministic, dependency-free serialization for [`InterfaceSummary`].
//!
//! Design rules (`docs/impl/10-cache-first-optimization.md` §6.4):
//! * **No process-local ids, no pointers, no HashMap iteration order.** Types are recorded by name;
//!   every collection the encoder writes is either semantically ordered (fields/variants/params) or
//!   pre-sorted by name at build time (fns/structs/enums/consts/capabilities).
//! * **No float formatting ambiguity.** No `f64` is ever encoded — const values ride as source text.
//! * **Versioned.** A leading [`FORMAT_VERSION`] `u32`; an unknown version is a loud, fail-closed
//!   error on read.
//! * **Length-prefixed, little-endian, self-delimiting.** Every read is bounds-checked; a truncated
//!   or malformed buffer returns [`DecodeError`], never a panic.

use crate::{
    Effect, Hash128, IConst, IEnumDef, IFnSig, IParam, IResourceDef, IStructDef, IType, ITypeParam,
    InterfaceSummary, OwnedJsonInterfaceEntry, OwnedJsonTarget, ParamMode, ReturnBorrowSummary,
    ReturnRegionSummary,
};

/// The interface-artifact format version. Bump on ANY encoding change; a bump invalidates every
/// cached summary (an old version fails closed on read) and changes `interface_hash` (the version is
/// part of the hashed surface).
pub const FORMAT_VERSION: u32 = 7;

/// Narrow a length to the format's `u32` length-prefix width, or panic loudly. This is
/// producer-side, compiler-internal data (interface surfaces built from the compiler's own source
/// text) — never user input — so a hard panic is the correct fail-loud behavior here, matching the
/// repo convention that panics are for compiler-internal invariants. (The reader stays Err-based:
/// a malformed/truncated buffer from disk is untrusted and must return [`DecodeError`], never panic.)
fn u32_len(n: usize) -> u32 {
    u32::try_from(n)
        .unwrap_or_else(|_| panic!("interface summary field exceeds u32::MAX bytes — the format uses u32 length prefixes"))
}

// ---- writer -------------------------------------------------------------------------------------

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Writer {
        Writer { buf: Vec::new() }
    }
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn bool(&mut self, v: bool) {
        self.u8(v as u8);
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn opt_u32(&mut self, v: Option<u32>) {
        match v {
            Some(x) => {
                self.u8(1);
                self.u32(x);
            }
            None => self.u8(0),
        }
    }
    fn str(&mut self, s: &str) {
        self.u32(u32_len(s.len()));
        self.buf.extend_from_slice(s.as_bytes());
    }
    fn opt_str(&mut self, s: &Option<String>) {
        match s {
            Some(x) => {
                self.u8(1);
                self.str(x);
            }
            None => self.u8(0),
        }
    }
    /// Write a length prefix and then invoke `f` once per element.
    fn seq<T>(&mut self, items: &[T], mut f: impl FnMut(&mut Writer, &T)) {
        self.u32(u32_len(items.len()));
        for it in items {
            f(self, it);
        }
    }
}

fn write_type(w: &mut Writer, t: &IType) {
    match t {
        IType::Named { path, args } => {
            w.u8(0);
            w.str(path);
            w.seq(args, write_type);
        }
        IType::Tuple(elems) => {
            w.u8(1);
            w.seq(elems, write_type);
        }
        IType::Fn {
            params,
            ret,
            return_borrow,
            return_region,
            return_cleanup,
        } => {
            w.u8(2);
            w.seq(params, write_param);
            write_type(w, ret);
            write_return_borrow(w, return_borrow);
            write_return_region(w, return_region);
            write_return_cleanup(w, *return_cleanup);
        }
    }
}

fn write_param(w: &mut Writer, p: &IParam) {
    w.u8(match p.mode {
        ParamMode::ByValue => 0,
        ParamMode::Out => 1,
        ParamMode::Borrow => 2,
        ParamMode::BorrowMut => 3,
    });
    write_type(w, &p.ty);
}

fn write_roots(w: &mut Writer, params: &[u32], captures: &[u32]) {
    w.seq(params, |w, root| w.u32(*root));
    w.seq(captures, |w, root| w.u32(*root));
}

fn write_return_borrow(w: &mut Writer, summary: &ReturnBorrowSummary) {
    match summary {
        ReturnBorrowSummary::None => w.u8(0),
        ReturnBorrowSummary::Roots { params, captures } => {
            w.u8(1);
            write_roots(w, params, captures);
        }
    }
}

fn write_return_region(w: &mut Writer, summary: &ReturnRegionSummary) {
    match summary {
        ReturnRegionSummary::None => w.u8(0),
        ReturnRegionSummary::Roots { params, captures } => {
            w.u8(1);
            write_roots(w, params, captures);
        }
    }
}

fn write_type_params(w: &mut Writer, tps: &[ITypeParam]) {
    w.seq(tps, |w, tp| {
        w.str(&tp.name);
        w.opt_str(&tp.bound);
    });
}

fn write_effect(w: &mut Writer, e: Effect) {
    w.u8(match e {
        Effect::Pure => 0,
        Effect::Impure => 1,
        Effect::Unknown => 2,
    });
}

fn write_return_cleanup(w: &mut Writer, value: align_sema::hir::ReturnCleanupAbi) {
    w.u8(match value {
        align_sema::hir::ReturnCleanupAbi::None => 0,
        align_sema::hir::ReturnCleanupAbi::DynamicBit => 1,
    });
}

fn write_fn(w: &mut Writer, f: &IFnSig) {
    w.str(&f.name);
    write_type_params(w, &f.type_params);
    w.seq(&f.params, write_param);
    write_type(w, &f.ret);
    write_return_borrow(w, &f.return_borrow);
    write_return_region(w, &f.return_region);
    write_return_cleanup(w, f.return_cleanup);
    write_effect(w, f.effect);
    w.seq(&f.parallel_transfer_params, |w, root| w.u32(*root));
    w.bool(f.resource_hook_body);
    w.opt_str(&f.generic_body);
}

fn write_struct(w: &mut Writer, s: &IStructDef) {
    w.str(&s.name);
    write_type_params(w, &s.type_params);
    w.seq(&s.fields, |w, (name, ty)| {
        w.str(name);
        write_type(w, ty);
    });
    w.opt_u32(s.align);
    w.bool(s.c_repr);
    w.opt_str(&s.generic_body);
}

fn write_owned_json_entry(w: &mut Writer, entry: &OwnedJsonInterfaceEntry) {
    w.str(&entry.type_name);
    w.u32(u32_len(entry.envelope.len()));
    w.buf.extend_from_slice(&entry.envelope);
}

fn write_enum(w: &mut Writer, e: &IEnumDef) {
    w.str(&e.name);
    write_type_params(w, &e.type_params);
    w.seq(&e.variants, |w, (name, payload)| {
        w.str(name);
        w.seq(payload, write_type);
    });
    w.opt_str(&e.generic_body);
}

fn write_resource(w: &mut Writer, resource: &IResourceDef) {
    w.str(&resource.name);
    write_type_params(w, &resource.type_params);
    w.u32(resource.generic_arity);
    w.u32(resource.representation_version);
    w.str(&resource.drop_thunk);
    w.buf.extend_from_slice(&resource.drop_abi_fingerprint);
}

fn write_const(w: &mut Writer, c: &IConst) {
    w.str(&c.name);
    match &c.ty {
        Some(t) => {
            w.u8(1);
            write_type(w, t);
        }
        None => w.u8(0),
    }
    w.str(&c.value_src);
}

/// Write the interface **surface** (version + unit path + fns + structs + enums + consts). This is
/// exactly the input to `interface_hash` — capabilities (link-summary) and the hashes themselves are
/// excluded.
fn write_surface(w: &mut Writer, s: &InterfaceSummary) {
    w.u32(FORMAT_VERSION);
    w.str(&s.unit);
    w.seq(&s.fns, write_fn);
    w.seq(&s.structs, write_struct);
    w.seq(&s.owned_json_descriptors, write_owned_json_entry);
    w.seq(&s.enums, write_enum);
    w.seq(&s.resources, write_resource);
    w.seq(&s.consts, write_const);
}

/// The canonical bytes of the interface surface — the input to `interface_hash`.
pub fn encode_interface_surface(s: &InterfaceSummary) -> Vec<u8> {
    let mut w = Writer::new();
    write_surface(&mut w, s);
    w.buf
}

/// Serialize a complete summary (surface + capabilities + both hashes) into the on-disk artifact
/// byte form. Round-trips through [`deserialize`].
pub fn serialize(s: &InterfaceSummary) -> Vec<u8> {
    let mut w = Writer::new();
    write_surface(&mut w, s);
    w.seq(&s.capabilities, |w, c| w.str(c));
    w.u64(s.interface_hash.lo);
    w.u64(s.interface_hash.hi);
    w.u64(s.impl_hash.lo);
    w.u64(s.impl_hash.hi);
    w.buf
}

// ---- reader -------------------------------------------------------------------------------------

/// A failure decoding an interface artifact. Every variant is a fail-closed rejection (never a
/// partial / guessed value).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The leading format version is not one this build understands (a newer/older/foreign artifact).
    UnknownVersion(u32),
    /// The buffer ended mid-field.
    Truncated,
    /// An enum discriminant tag was out of range.
    BadTag { what: &'static str, tag: u8 },
    /// A length-prefixed string was not valid UTF-8.
    BadUtf8,
    /// Bytes remained after the summary was fully read (a length/format mismatch).
    TrailingBytes,
    /// The decoded public surface does not match the fingerprint stored in the artifact.
    /// This catches a stale or modified effect bit/signature/layout before a consumer can trust it.
    InterfaceHashMismatch,
    /// A provenance summary was not canonical or referenced a root unavailable in an exported
    /// interface.
    InvalidSummary(&'static str),
    /// A target-bound owned-JSON envelope or descriptor was malformed, non-canonical, or belonged
    /// to a different compiler target.
    InvalidOwnedJson(&'static str),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::UnknownVersion(v) => {
                write!(f, "unknown interface format version {v} (this build understands {FORMAT_VERSION})")
            }
            DecodeError::Truncated => write!(f, "interface artifact is truncated"),
            DecodeError::BadTag { what, tag } => write!(f, "invalid {what} tag byte {tag}"),
            DecodeError::BadUtf8 => write!(f, "interface artifact contains invalid UTF-8"),
            DecodeError::TrailingBytes => write!(f, "interface artifact has trailing bytes"),
            DecodeError::InterfaceHashMismatch => write!(f, "interface artifact surface does not match its stored hash"),
            DecodeError::InvalidSummary(reason) => {
                write!(f, "invalid interface provenance summary: {reason}")
            }
            DecodeError::InvalidOwnedJson(reason) => {
                write!(f, "invalid owned JSON interface descriptor: {reason}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        let s = self.buf.get(self.pos..end).ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(DecodeError::BadTag { what: "bool", tag }),
        }
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn opt_u32(&mut self) -> Result<Option<u32>, DecodeError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u32()?)),
            tag => Err(DecodeError::BadTag { what: "option", tag }),
        }
    }
    fn str(&mut self) -> Result<String, DecodeError> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes).map(|s| s.to_string()).map_err(|_| DecodeError::BadUtf8)
    }
    fn opt_str(&mut self) -> Result<Option<String>, DecodeError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.str()?)),
            tag => Err(DecodeError::BadTag { what: "option", tag }),
        }
    }
    /// Read a length prefix, then `f` that many times.
    fn seq<T>(&mut self, mut f: impl FnMut(&mut Reader<'a>) -> Result<T, DecodeError>) -> Result<Vec<T>, DecodeError> {
        let n = self.u32()? as usize;
        let mut out = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            out.push(f(self)?);
        }
        Ok(out)
    }
    fn finish(self) -> Result<(), DecodeError> {
        if self.pos == self.buf.len() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes)
        }
    }
}

fn read_type(r: &mut Reader<'_>) -> Result<IType, DecodeError> {
    match r.u8()? {
        0 => Ok(IType::Named { path: r.str()?, args: r.seq(read_type)? }),
        1 => Ok(IType::Tuple(r.seq(read_type)?)),
        2 => {
            let params = r.seq(read_param)?;
            let ret = Box::new(read_type(r)?);
            let return_borrow = read_return_borrow(r, params.len())?;
            let return_region = read_return_region(r, params.len())?;
            let return_cleanup = read_return_cleanup(r)?;
            Ok(IType::Fn { params, ret, return_borrow, return_region, return_cleanup })
        }
        tag => Err(DecodeError::BadTag { what: "type", tag }),
    }
}

fn read_param(r: &mut Reader<'_>) -> Result<IParam, DecodeError> {
    let mode = match r.u8()? {
        0 => ParamMode::ByValue,
        1 => ParamMode::Out,
        2 => ParamMode::Borrow,
        3 => ParamMode::BorrowMut,
        tag => return Err(DecodeError::BadTag { what: "parameter mode", tag }),
    };
    Ok(IParam { mode, ty: read_type(r)? })
}

fn validate_roots(
    params: &[u32],
    captures: &[u32],
    param_count: usize,
) -> Result<(), DecodeError> {
    if params.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(DecodeError::InvalidSummary(
            "parameter roots must be strictly increasing",
        ));
    }
    if params.iter().any(|root| *root as usize >= param_count) {
        return Err(DecodeError::InvalidSummary(
            "parameter root is outside the signature",
        ));
    }
    if !captures.is_empty() {
        return Err(DecodeError::InvalidSummary(
            "capture roots are forbidden in exported interfaces",
        ));
    }
    if params.is_empty() {
        return Err(DecodeError::InvalidSummary(
            "an empty root set must use the canonical None tag",
        ));
    }
    Ok(())
}

fn validate_transfer_roots(params: &[u32], param_count: usize) -> Result<(), DecodeError> {
    if params.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(DecodeError::InvalidSummary(
            "parallel-transfer roots must be strictly increasing",
        ));
    }
    if params.iter().any(|root| *root as usize >= param_count) {
        return Err(DecodeError::InvalidSummary(
            "parallel-transfer root is outside the signature",
        ));
    }
    Ok(())
}

fn read_roots(
    r: &mut Reader<'_>,
    param_count: usize,
) -> Result<(Vec<u32>, Vec<u32>), DecodeError> {
    let params = r.seq(|r| r.u32())?;
    let captures = r.seq(|r| r.u32())?;
    validate_roots(&params, &captures, param_count)?;
    Ok((params, captures))
}

fn read_return_borrow(
    r: &mut Reader<'_>,
    param_count: usize,
) -> Result<ReturnBorrowSummary, DecodeError> {
    match r.u8()? {
        0 => Ok(ReturnBorrowSummary::None),
        1 => {
            let (params, captures) = read_roots(r, param_count)?;
            Ok(ReturnBorrowSummary::Roots { params, captures })
        }
        tag => Err(DecodeError::BadTag { what: "return-borrow summary", tag }),
    }
}

fn read_return_region(
    r: &mut Reader<'_>,
    param_count: usize,
) -> Result<ReturnRegionSummary, DecodeError> {
    match r.u8()? {
        0 => Ok(ReturnRegionSummary::None),
        1 => {
            let (params, captures) = read_roots(r, param_count)?;
            Ok(ReturnRegionSummary::Roots { params, captures })
        }
        tag => Err(DecodeError::BadTag { what: "return-region summary", tag }),
    }
}

fn read_type_params(r: &mut Reader<'_>) -> Result<Vec<ITypeParam>, DecodeError> {
    r.seq(|r| Ok(ITypeParam { name: r.str()?, bound: r.opt_str()? }))
}

fn read_effect(r: &mut Reader<'_>) -> Result<Effect, DecodeError> {
    match r.u8()? {
        0 => Ok(Effect::Pure),
        1 => Ok(Effect::Impure),
        2 => Ok(Effect::Unknown),
        tag => Err(DecodeError::BadTag { what: "effect", tag }),
    }
}

fn read_return_cleanup(
    r: &mut Reader<'_>,
) -> Result<align_sema::hir::ReturnCleanupAbi, DecodeError> {
    match r.u8()? {
        0 => Ok(align_sema::hir::ReturnCleanupAbi::None),
        1 => Ok(align_sema::hir::ReturnCleanupAbi::DynamicBit),
        tag => Err(DecodeError::BadTag { what: "return cleanup ABI", tag }),
    }
}

fn read_fn(r: &mut Reader<'_>) -> Result<IFnSig, DecodeError> {
    let name = r.str()?;
    let type_params = read_type_params(r)?;
    let params = r.seq(read_param)?;
    let ret = read_type(r)?;
    let return_borrow = read_return_borrow(r, params.len())?;
    let return_region = read_return_region(r, params.len())?;
    let return_cleanup = read_return_cleanup(r)?;
    let effect = read_effect(r)?;
    let parallel_transfer_params = r.seq(|r| r.u32())?;
    validate_transfer_roots(&parallel_transfer_params, params.len())?;
    let resource_hook_body = r.bool()?;
    let generic_body = r.opt_str()?;
    Ok(IFnSig {
        name,
        type_params,
        params,
        ret,
        return_borrow,
        return_region,
        return_cleanup,
        effect,
        parallel_transfer_params,
        resource_hook_body,
        generic_body,
    })
}

fn read_struct(r: &mut Reader<'_>) -> Result<IStructDef, DecodeError> {
    Ok(IStructDef {
        name: r.str()?,
        type_params: read_type_params(r)?,
        fields: r.seq(|r| Ok((r.str()?, read_type(r)?)))?,
        align: r.opt_u32()?,
        c_repr: r.bool()?,
        generic_body: r.opt_str()?,
    })
}

fn read_enum(r: &mut Reader<'_>) -> Result<IEnumDef, DecodeError> {
    Ok(IEnumDef {
        name: r.str()?,
        type_params: read_type_params(r)?,
        variants: r.seq(|r| Ok((r.str()?, r.seq(read_type)?)))?,
        generic_body: r.opt_str()?,
    })
}

fn read_resource(r: &mut Reader<'_>) -> Result<IResourceDef, DecodeError> {
    let name = r.str()?;
    let type_params = read_type_params(r)?;
    let generic_arity = r.u32()?;
    let representation_version = r.u32()?;
    let drop_thunk = r.str()?;
    let fingerprint = r.take(16)?;
    let mut drop_abi_fingerprint = [0u8; 16];
    drop_abi_fingerprint.copy_from_slice(fingerprint);
    Ok(IResourceDef {
        name,
        type_params,
        generic_arity,
        representation_version,
        drop_thunk,
        drop_abi_fingerprint,
    })
}

fn read_const(r: &mut Reader<'_>) -> Result<IConst, DecodeError> {
    let name = r.str()?;
    let ty = match r.u8()? {
        0 => None,
        1 => Some(read_type(r)?),
        tag => return Err(DecodeError::BadTag { what: "option", tag }),
    };
    Ok(IConst { name, ty, value_src: r.str()? })
}

fn read_owned_json_entry(
    r: &mut Reader<'_>,
) -> Result<OwnedJsonInterfaceEntry, DecodeError> {
    let type_name = r.str()?;
    let len = usize::try_from(r.u32()?)
        .map_err(|_| DecodeError::InvalidOwnedJson("owned JSON envelope length"))?;
    let envelope = r.take(len)?.to_vec();
    Ok(OwnedJsonInterfaceEntry {
        type_name,
        envelope,
    })
}

/// Deserialize a complete summary from its artifact byte form. Fail-closed: an unknown format
/// version, a truncated buffer, a bad tag, invalid UTF-8, trailing bytes, or a stale/mismatched
/// surface hash all return an error.
pub fn deserialize(bytes: &[u8]) -> Result<InterfaceSummary, DecodeError> {
    deserialize_impl(bytes, None)
}

/// Decode and additionally bind every owned-JSON descriptor envelope to `target` before returning
/// the summary to an interface consumer or cache lookup.
pub fn deserialize_for_target(
    bytes: &[u8],
    target: &OwnedJsonTarget,
) -> Result<InterfaceSummary, DecodeError> {
    deserialize_impl(bytes, Some(target))
}

fn deserialize_impl(
    bytes: &[u8],
    target: Option<&OwnedJsonTarget>,
) -> Result<InterfaceSummary, DecodeError> {
    let mut r = Reader::new(bytes);
    let version = r.u32()?;
    if version != FORMAT_VERSION {
        return Err(DecodeError::UnknownVersion(version));
    }
    let unit = r.str()?;
    let fns = r.seq(read_fn)?;
    let structs = r.seq(read_struct)?;
    let owned_json_descriptors = r.seq(read_owned_json_entry)?;
    let enums = r.seq(read_enum)?;
    let resources = r.seq(read_resource)?;
    let consts = r.seq(read_const)?;
    let surface_len = r.pos;
    let capabilities = r.seq(|r| r.str())?;
    let interface_hash = Hash128 { lo: r.u64()?, hi: r.u64()? };
    let impl_hash = Hash128 { lo: r.u64()?, hi: r.u64()? };
    r.finish()?;
    let summary = InterfaceSummary {
        unit,
        fns,
        structs,
        owned_json_descriptors,
        enums,
        resources,
        consts,
        capabilities,
        interface_hash,
        impl_hash,
    };
    if !crate::decoded_parallel_transfer_roots_are_borrow_capable(&summary) {
        return Err(DecodeError::InvalidSummary(
            "parallel-transfer root is not borrow-capable",
        ));
    }
    crate::owned_json::validate_entries(
        &summary.structs,
        &summary.owned_json_descriptors,
        target,
    )
    .map_err(DecodeError::InvalidOwnedJson)?;
    if Hash128::of(&bytes[..surface_len]) != summary.interface_hash {
        return Err(DecodeError::InterfaceHashMismatch);
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_effect_tag_is_rejected_before_sema() {
        let mut reader = Reader::new(&[3]);
        assert_eq!(
            read_effect(&mut reader),
            Err(DecodeError::BadTag {
                what: "effect",
                tag: 3,
            })
        );
    }

    #[test]
    fn run_bytes_named_type_has_the_exact_format_7_field_encoding() {
        let ty = IType::Named { path: "run_bytes".to_string(), args: Vec::new() };
        let mut writer = Writer { buf: Vec::new() };
        write_type(&mut writer, &ty);
        let expected = [
            0, 9, 0, 0, 0, b'r', b'u', b'n', b'_', b'b', b'y', b't', b'e', b's',
            0, 0, 0, 0,
        ];
        assert_eq!(writer.buf, expected);

        let mut reader = Reader::new(&expected);
        assert_eq!(read_type(&mut reader), Ok(ty));
        assert_eq!(reader.finish(), Ok(()));

        let mut unknown = Reader::new(&[3]);
        assert_eq!(
            read_type(&mut unknown),
            Err(DecodeError::BadTag { what: "type", tag: 3 })
        );
        for malformed in [
            &expected[..4],
            &[0, 1, 0, 0, 0, 0xff, 0, 0, 0, 0][..],
            &[expected.as_slice(), &[0]].concat()[..],
        ] {
            let mut reader = Reader::new(malformed);
            assert!(read_type(&mut reader).and_then(|_| reader.finish()).is_err());
        }
    }
}
