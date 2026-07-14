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

use crate::{Effect, Hash128, IConst, IEnumDef, IFnSig, IParam, IStructDef, ITypeParam, IType, InterfaceSummary};

/// The interface-artifact format version. Bump on ANY encoding change; a bump invalidates every
/// cached summary (an old version fails closed on read) and changes `interface_hash` (the version is
/// part of the hashed surface).
pub const FORMAT_VERSION: u32 = 1;

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
        IType::Fn { params, ret } => {
            w.u8(2);
            w.seq(params, write_type);
            write_type(w, ret);
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

fn write_fn(w: &mut Writer, f: &IFnSig) {
    w.str(&f.name);
    write_type_params(w, &f.type_params);
    w.seq(&f.params, |w, p: &IParam| {
        w.bool(p.is_out);
        write_type(w, &p.ty);
    });
    write_type(w, &f.ret);
    write_effect(w, f.effect);
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

fn write_enum(w: &mut Writer, e: &IEnumDef) {
    w.str(&e.name);
    write_type_params(w, &e.type_params);
    w.seq(&e.variants, |w, (name, payload)| {
        w.str(name);
        w.seq(payload, write_type);
    });
    w.opt_str(&e.generic_body);
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
    w.seq(&s.enums, write_enum);
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
        2 => Ok(IType::Fn { params: r.seq(read_type)?, ret: Box::new(read_type(r)?) }),
        tag => Err(DecodeError::BadTag { what: "type", tag }),
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

fn read_fn(r: &mut Reader<'_>) -> Result<IFnSig, DecodeError> {
    Ok(IFnSig {
        name: r.str()?,
        type_params: read_type_params(r)?,
        params: r.seq(|r| Ok(IParam { is_out: r.bool()?, ty: read_type(r)? }))?,
        ret: read_type(r)?,
        effect: read_effect(r)?,
        generic_body: r.opt_str()?,
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

fn read_const(r: &mut Reader<'_>) -> Result<IConst, DecodeError> {
    let name = r.str()?;
    let ty = match r.u8()? {
        0 => None,
        1 => Some(read_type(r)?),
        tag => return Err(DecodeError::BadTag { what: "option", tag }),
    };
    Ok(IConst { name, ty, value_src: r.str()? })
}

/// Deserialize a complete summary from its artifact byte form. Fail-closed: an unknown format
/// version, a truncated buffer, a bad tag, invalid UTF-8, or trailing bytes all return an error.
pub fn deserialize(bytes: &[u8]) -> Result<InterfaceSummary, DecodeError> {
    let mut r = Reader::new(bytes);
    let version = r.u32()?;
    if version != FORMAT_VERSION {
        return Err(DecodeError::UnknownVersion(version));
    }
    let unit = r.str()?;
    let fns = r.seq(read_fn)?;
    let structs = r.seq(read_struct)?;
    let enums = r.seq(read_enum)?;
    let consts = r.seq(read_const)?;
    let capabilities = r.seq(|r| r.str())?;
    let interface_hash = Hash128 { lo: r.u64()?, hi: r.u64()? };
    let impl_hash = Hash128 { lo: r.u64()?, hi: r.u64()? };
    r.finish()?;
    Ok(InterfaceSummary {
        unit,
        fns,
        structs,
        enums,
        consts,
        capabilities,
        interface_hash,
        impl_hash,
    })
}
