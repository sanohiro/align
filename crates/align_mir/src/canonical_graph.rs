use align_ast::ParamMode;
use align_sema::{hir, Layout, PrimScalar, Scalar, Ty};

#[derive(Clone, Debug)]
pub struct FunctionTypeDef {
    pub params: Vec<(ParamMode, Scalar)>,
    pub ret: Ty,
    pub return_borrow: hir::ReturnBorrowSummary,
    pub return_region: hir::ReturnRegionSummary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum CanonicalGraphError {
    EmbeddedNul,
    InvalidWidth,
    InvalidCount,
    MissingReference,
    DuplicateMember,
    InvalidSummary,
    InvalidGraph,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Node {
    Struct(u32),
    Enum(u32),
    Tuple(u32),
    Tagged(u32),
    Fn(u32),
}

#[allow(dead_code)]
fn checked_count(len: usize) -> Result<u32, CanonicalGraphError> {
    u32::try_from(len).map_err(|_| CanonicalGraphError::InvalidCount)
}

#[allow(dead_code)]
fn text(out: &mut Vec<u8>, value: &str) -> Result<(), CanonicalGraphError> {
    if value.as_bytes().contains(&0) {
        return Err(CanonicalGraphError::EmbeddedNul);
    }
    out.extend(checked_count(value.len())?.to_le_bytes());
    out.extend(value.as_bytes());
    Ok(())
}

#[allow(dead_code)]
fn int(out: &mut Vec<u8>, signed: bool, bits: u8) -> Result<(), CanonicalGraphError> {
    if !matches!(bits, 8 | 16 | 32 | 64) {
        return Err(CanonicalGraphError::InvalidWidth);
    }
    out.extend([u8::from(signed), bits]);
    Ok(())
}

#[allow(dead_code)]
fn float(out: &mut Vec<u8>, bits: u8) -> Result<(), CanonicalGraphError> {
    if !matches!(bits, 32 | 64) {
        return Err(CanonicalGraphError::InvalidWidth);
    }
    out.push(bits);
    Ok(())
}

#[allow(dead_code)]
fn encode_param_mode(out: &mut Vec<u8>, mode: ParamMode) -> Result<(), CanonicalGraphError> {
    match mode {
        ParamMode::ByValue => out.push(0),
        ParamMode::Out => out.push(1),
        ParamMode::Borrow | ParamMode::BorrowMut => {
            return Err(CanonicalGraphError::InvalidGraph);
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn append_transactional(
    out: &mut Vec<u8>,
    append: impl FnOnce(&mut Vec<u8>) -> Result<(), CanonicalGraphError>,
) -> Result<(), CanonicalGraphError> {
    let start = out.len();
    let result = append(out);
    if result.is_err() {
        out.truncate(start);
    }
    result
}

#[allow(dead_code)]
fn prim(out: &mut Vec<u8>, scalar: PrimScalar) -> Result<(), CanonicalGraphError> {
    append_transactional(out, |out| match scalar {
        PrimScalar::Int(ty) => {
            out.push(0);
            int(out, ty.signed, ty.bits)
        }
        PrimScalar::Float(ty) => {
            out.push(1);
            float(out, ty.bits)
        }
        PrimScalar::Bool => {
            out.push(2);
            Ok(())
        }
        PrimScalar::Char => {
            out.push(3);
            Ok(())
        }
        PrimScalar::Str => {
            out.push(4);
            Ok(())
        }
        PrimScalar::String => {
            out.push(5);
            Ok(())
        }
    })
}

#[allow(dead_code)]
fn scalar(
    out: &mut Vec<u8>,
    value: Scalar,
    ordinal: &impl Fn(Node) -> Result<u32, CanonicalGraphError>,
) -> Result<(), CanonicalGraphError> {
    append_transactional(out, |out| {
        macro_rules! leaf {
            ($tag:expr) => {{
                out.push($tag);
                Ok(())
            }};
        }
        macro_rules! node {
            ($tag:expr, $kind:ident, $id:expr) => {{
                out.push($tag);
                out.extend(ordinal(Node::$kind($id))?.to_le_bytes());
                Ok(())
            }};
        }
        match value {
            Scalar::Int(ty) => {
                out.push(0);
                int(out, ty.signed, ty.bits)
            }
            Scalar::Float(ty) => {
                out.push(1);
                float(out, ty.bits)
            }
            Scalar::Bool => leaf!(2),
            Scalar::Char => leaf!(3),
            Scalar::Unit => leaf!(4),
            Scalar::Struct(id) => node!(5, Struct, id),
            Scalar::String => leaf!(6),
            Scalar::DynArray(elem) => {
                out.push(7);
                prim(out, elem)
            }
            Scalar::DynStructArray(id) => node!(8, Struct, id),
            Scalar::DynResponseArray => leaf!(9),
            Scalar::Str => leaf!(10),
            Scalar::Slice(elem) => {
                out.push(11);
                prim(out, elem)
            }
            Scalar::Enum(id) => node!(12, Enum, id),
            Scalar::Tagged(id) => node!(13, Tagged, id),
            Scalar::Soa(id) => node!(14, Struct, id),
            Scalar::JsonDoc => leaf!(15),
            Scalar::Reader => leaf!(16),
            Scalar::Writer => leaf!(17),
            Scalar::Buffer => leaf!(18),
            Scalar::Regex => leaf!(19),
            Scalar::Captures => leaf!(20),
            Scalar::CliParsed => leaf!(21),
            Scalar::TcpConn => leaf!(22),
            Scalar::TcpListener => leaf!(23),
            Scalar::UdpSocket => leaf!(24),
            Scalar::Child => leaf!(25),
            Scalar::File => leaf!(26),
            Scalar::HttpResponse => leaf!(27),
            Scalar::HttpServer => leaf!(28),
            Scalar::HttpRequestCtx => leaf!(29),
            Scalar::ResponseBuilder => leaf!(30),
            Scalar::HttpStream => leaf!(31),
            Scalar::RunOutput => leaf!(32),
            Scalar::Fn(id) => node!(33, Fn, id),
            Scalar::Param(_) => Err(CanonicalGraphError::InvalidGraph),
        }
    })
}

#[allow(dead_code)]
fn ty(
    out: &mut Vec<u8>,
    value: Ty,
    ordinal: &impl Fn(Node) -> Result<u32, CanonicalGraphError>,
) -> Result<(), CanonicalGraphError> {
    append_transactional(out, |out| {
        macro_rules! leaf {
            ($tag:expr) => {{
                out.push($tag);
                Ok(())
            }};
        }
        macro_rules! node {
            ($tag:expr, $kind:ident, $id:expr) => {{
                out.push($tag);
                out.extend(ordinal(Node::$kind($id))?.to_le_bytes());
                Ok(())
            }};
        }
        match value {
            Ty::Int(v) => {
                out.push(0);
                int(out, v.signed, v.bits)
            }
            Ty::Float(v) => {
                out.push(1);
                float(out, v.bits)
            }
            Ty::Bool => leaf!(2),
            Ty::Char => leaf!(3),
            Ty::Option(v) => {
                out.push(4);
                scalar(out, v, ordinal)
            }
            Ty::Result(a, b) => {
                out.push(5);
                scalar(out, a, ordinal)?;
                scalar(out, b, ordinal)
            }
            Ty::Tagged(id) => node!(6, Tagged, id),
            Ty::Box(v) => {
                out.push(7);
                scalar(out, v, ordinal)
            }
            Ty::Array(v, n) => {
                out.push(8);
                scalar(out, v, ordinal)?;
                out.extend(n.to_le_bytes());
                Ok(())
            }
            Ty::Vec(v, n) | Ty::Mask(v, n) => {
                out.push(if matches!(value, Ty::Vec(..)) { 9 } else { 10 });
                if !matches!(v, Scalar::Int(_) | Scalar::Float(_)) || !matches!(n, 2 | 4 | 8 | 16) {
                    return Err(CanonicalGraphError::InvalidWidth);
                }
                scalar(out, v, ordinal)?;
                out.extend(n.to_le_bytes());
                Ok(())
            }
            Ty::StructArray(id, n) => {
                out.push(11);
                out.extend(ordinal(Node::Struct(id))?.to_le_bytes());
                out.extend(n.to_le_bytes());
                Ok(())
            }
            Ty::DynStructArray(id, layout) => {
                out.push(12);
                out.extend(ordinal(Node::Struct(id))?.to_le_bytes());
                out.push(match layout {
                    Layout::Aos => 0,
                    Layout::Soa => 1,
                });
                Ok(())
            }
            Ty::Slice(v) => {
                out.push(13);
                scalar(out, v, ordinal)
            }
            Ty::Soa(id) => node!(14, Struct, id),
            Ty::DynSliceArray(v) => {
                out.push(15);
                prim(out, v)
            }
            Ty::DynArray(v) => {
                out.push(16);
                scalar(out, v, ordinal)
            }
            Ty::DynResponseArray => leaf!(17),
            Ty::Str => leaf!(18),
            Ty::String => leaf!(19),
            Ty::ArenaHandle => leaf!(20),
            Ty::Raw => leaf!(21),
            Ty::Builder => leaf!(22),
            Ty::Writer => leaf!(23),
            Ty::Reader => leaf!(24),
            Ty::Buffer => leaf!(25),
            Ty::ArrayBuilder(v) => {
                out.push(26);
                scalar(out, v, ordinal)
            }
            Ty::StrFinder => leaf!(27),
            Ty::File => leaf!(28),
            Ty::Rng => leaf!(29),
            Ty::Regex => leaf!(30),
            Ty::Captures => leaf!(31),
            Ty::CliCommand => leaf!(32),
            Ty::CliParsed => leaf!(33),
            Ty::TcpConn => leaf!(34),
            Ty::TcpListener => leaf!(35),
            Ty::UdpSocket => leaf!(36),
            Ty::Child => leaf!(37),
            Ty::Command => leaf!(38),
            Ty::RunOutput => leaf!(39),
            Ty::HttpRequest => leaf!(40),
            Ty::HttpResponse => leaf!(41),
            Ty::HttpClient => leaf!(42),
            Ty::HttpServer => leaf!(43),
            Ty::HttpRequestCtx => leaf!(44),
            Ty::ResponseBuilder => leaf!(45),
            Ty::HttpStream => leaf!(46),
            Ty::HttpHeaders => leaf!(47),
            Ty::JsonDoc => leaf!(48),
            Ty::JsonScanner(id) => node!(49, Struct, id),
            Ty::Struct(id) => node!(50, Struct, id),
            Ty::Tuple(id) => node!(51, Tuple, id),
            Ty::Fn(id) => node!(52, Fn, id),
            Ty::Enum(id) => node!(53, Enum, id),
            Ty::Task(v) => {
                out.push(54);
                scalar(out, v, ordinal)
            }
            Ty::DictEncoded(id, field) => {
                out.push(55);
                out.extend(ordinal(Node::Struct(id))?.to_le_bytes());
                out.extend(field.to_le_bytes());
                Ok(())
            }
            Ty::Unit => leaf!(56),
            Ty::Param(_) | Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Error => {
                Err(CanonicalGraphError::InvalidGraph)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use align_sema::{FloatTy, IntTy};

    use super::*;

    fn i(bits: u8) -> IntTy {
        IntTy { bits, signed: true }
    }

    fn f(bits: u8) -> FloatTy {
        FloatTy { bits }
    }

    fn ordinal(node: Node) -> Result<u32, CanonicalGraphError> {
        Ok(match node {
            Node::Struct(id) => 0x1000 + id,
            Node::Enum(id) => 0x2000 + id,
            Node::Tuple(id) => 0x3000 + id,
            Node::Tagged(id) => 0x4000 + id,
            Node::Fn(id) => 0x5000 + id,
        })
    }

    fn appended(
        encode: impl FnOnce(&mut Vec<u8>) -> Result<(), CanonicalGraphError>,
    ) -> Result<Vec<u8>, CanonicalGraphError> {
        let mut out = vec![0xa5, 0x5a];
        encode(&mut out)?;
        assert_eq!(&out[..2], [0xa5, 0x5a]);
        Ok(out.split_off(2))
    }

    fn encoded_ty(value: Ty) -> Result<Vec<u8>, CanonicalGraphError> {
        appended(|out| ty(out, value, &ordinal))
    }

    fn encoded_scalar(value: Scalar) -> Result<Vec<u8>, CanonicalGraphError> {
        appended(|out| scalar(out, value, &ordinal))
    }

    fn encoded_prim(value: PrimScalar) -> Result<Vec<u8>, CanonicalGraphError> {
        appended(|out| prim(out, value))
    }

    macro_rules! cases {
        ($encoder:ident; $($value:expr => $bytes:expr),+ $(,)?) => {
            $(assert_eq!($encoder($value).unwrap(), $bytes, "{:?}", $value);)+
        };
    }

    macro_rules! bytes {
        ($actual:expr, $expected:expr) => {
            assert_eq!($actual.unwrap(), $expected)
        };
    }

    macro_rules! error {
        ($actual:expr, $expected:expr) => {
            assert_eq!($actual, Err($expected))
        };
    }

    #[test]
    fn canonical_field_codec_covers_every_primitive_and_scalar_tag() {
        cases!(encoded_prim;
            PrimScalar::Int(i(8)) => [0, 1, 8], PrimScalar::Float(f(32)) => [1, 32],
            PrimScalar::Bool => [2], PrimScalar::Char => [3],
            PrimScalar::Str => [4], PrimScalar::String => [5],
        );
        cases!(encoded_scalar;
            Scalar::Int(i(8)) => [0, 1, 8], Scalar::Float(f(32)) => [1, 32],
            Scalar::Bool => [2], Scalar::Char => [3], Scalar::Unit => [4],
            Scalar::Struct(1) => [5, 1, 0x10, 0, 0], Scalar::String => [6],
            Scalar::DynArray(PrimScalar::Bool) => [7, 2],
            Scalar::DynStructArray(1) => [8, 1, 0x10, 0, 0],
            Scalar::DynResponseArray => [9], Scalar::Str => [10],
            Scalar::Slice(PrimScalar::Char) => [11, 3],
            Scalar::Enum(1) => [12, 1, 0x20, 0, 0],
            Scalar::Tagged(1) => [13, 1, 0x40, 0, 0],
            Scalar::Soa(1) => [14, 1, 0x10, 0, 0], Scalar::JsonDoc => [15],
            Scalar::Reader => [16], Scalar::Writer => [17], Scalar::Buffer => [18],
            Scalar::Regex => [19], Scalar::Captures => [20], Scalar::CliParsed => [21],
            Scalar::TcpConn => [22], Scalar::TcpListener => [23], Scalar::UdpSocket => [24],
            Scalar::Child => [25], Scalar::File => [26], Scalar::HttpResponse => [27],
            Scalar::HttpServer => [28], Scalar::HttpRequestCtx => [29],
            Scalar::ResponseBuilder => [30], Scalar::HttpStream => [31],
            Scalar::RunOutput => [32], Scalar::Fn(1) => [33, 1, 0x50, 0, 0],
        );
    }

    #[test]
    fn canonical_field_codec_covers_every_root_tag() {
        cases!(encoded_ty;
            Ty::Int(i(8)) => [0, 1, 8], Ty::Float(f(32)) => [1, 32],
            Ty::Bool => [2], Ty::Char => [3], Ty::Option(Scalar::Bool) => [4, 2],
            Ty::Result(Scalar::Bool, Scalar::Char) => [5, 2, 3],
            Ty::Tagged(1) => [6, 1, 0x40, 0, 0], Ty::Box(Scalar::Bool) => [7, 2],
            Ty::Array(Scalar::Bool, 2) => [8, 2, 2, 0, 0, 0],
            Ty::Vec(Scalar::Int(i(8)), 2) => [9, 0, 1, 8, 2, 0, 0, 0],
            Ty::Mask(Scalar::Float(f(32)), 2) => [10, 1, 32, 2, 0, 0, 0],
            Ty::StructArray(1, 2) => [11, 1, 0x10, 0, 0, 2, 0, 0, 0],
            Ty::DynStructArray(1, Layout::Aos) => [12, 1, 0x10, 0, 0, 0],
            Ty::Slice(Scalar::Bool) => [13, 2], Ty::Soa(1) => [14, 1, 0x10, 0, 0],
            Ty::DynSliceArray(PrimScalar::Bool) => [15, 2],
            Ty::DynArray(Scalar::Bool) => [16, 2], Ty::DynResponseArray => [17],
            Ty::Str => [18], Ty::String => [19], Ty::ArenaHandle => [20], Ty::Raw => [21],
            Ty::Builder => [22], Ty::Writer => [23], Ty::Reader => [24], Ty::Buffer => [25],
            Ty::ArrayBuilder(Scalar::Bool) => [26, 2], Ty::StrFinder => [27],
            Ty::File => [28], Ty::Rng => [29], Ty::Regex => [30], Ty::Captures => [31],
            Ty::CliCommand => [32], Ty::CliParsed => [33], Ty::TcpConn => [34],
            Ty::TcpListener => [35], Ty::UdpSocket => [36], Ty::Child => [37],
            Ty::Command => [38], Ty::RunOutput => [39], Ty::HttpRequest => [40],
            Ty::HttpResponse => [41], Ty::HttpClient => [42], Ty::HttpServer => [43],
            Ty::HttpRequestCtx => [44], Ty::ResponseBuilder => [45], Ty::HttpStream => [46],
            Ty::HttpHeaders => [47], Ty::JsonDoc => [48],
            Ty::JsonScanner(1) => [49, 1, 0x10, 0, 0],
            Ty::Struct(1) => [50, 1, 0x10, 0, 0], Ty::Tuple(1) => [51, 1, 0x30, 0, 0],
            Ty::Fn(1) => [52, 1, 0x50, 0, 0], Ty::Enum(1) => [53, 1, 0x20, 0, 0],
            Ty::Task(Scalar::Bool) => [54, 2],
            Ty::DictEncoded(1, 2) => [55, 1, 0x10, 0, 0, 2, 0, 0, 0], Ty::Unit => [56],
        );
    }

    #[test]
    fn canonical_field_codec_encodes_payloads_and_modes_exactly() {
        let mut out = vec![0xa5];
        text(&mut out, "é").unwrap();
        assert_eq!(out, [0xa5, 2, 0, 0, 0, 0xc3, 0xa9]);

        bytes!(encoded_scalar(Scalar::Struct(7)), [5, 7, 0x10, 0, 0]);
        bytes!(
            encoded_scalar(Scalar::DynArray(PrimScalar::Int(i(16)))),
            [7, 0, 1, 16]
        );
        bytes!(
            encoded_ty(Ty::Result(Scalar::Bool, Scalar::Fn(3))),
            [5, 2, 33, 3, 0x50, 0, 0]
        );
        bytes!(
            encoded_ty(Ty::Array(Scalar::Char, 0x0102_0304)),
            [8, 3, 4, 3, 2, 1]
        );
        bytes!(
            encoded_ty(Ty::DynStructArray(5, Layout::Soa)),
            [12, 5, 0x10, 0, 0, 1]
        );
        bytes!(
            encoded_ty(Ty::DictEncoded(6, 0x0102_0304)),
            [55, 6, 0x10, 0, 0, 4, 3, 2, 1]
        );

        out.truncate(1);
        encode_param_mode(&mut out, ParamMode::ByValue).unwrap();
        encode_param_mode(&mut out, ParamMode::Out).unwrap();
        assert_eq!(out, [0xa5, 0, 1]);
    }

    #[test]
    fn canonical_field_codec_accepts_only_settled_widths_and_lanes() {
        for bits in u8::MIN..=u8::MAX {
            for signed in [false, true] {
                assert_eq!(
                    appended(|out| int(out, signed, bits)).is_ok(),
                    matches!(bits, 8 | 16 | 32 | 64)
                );
            }
            assert_eq!(
                appended(|out| float(out, bits)).is_ok(),
                matches!(bits, 32 | 64)
            );
        }
        for lanes in [2, 4, 8, 16] {
            encoded_ty(Ty::Vec(Scalar::Int(i(32)), lanes)).unwrap();
            encoded_ty(Ty::Vec(Scalar::Float(f(32)), lanes)).unwrap();
            encoded_ty(Ty::Mask(Scalar::Int(i(64)), lanes)).unwrap();
            encoded_ty(Ty::Mask(Scalar::Float(f(64)), lanes)).unwrap();
        }
        for lanes in [0, 1, 3, 5, 7, 9, 15, 17, u32::MAX] {
            for value in [
                Ty::Vec(Scalar::Int(i(32)), lanes),
                Ty::Mask(Scalar::Float(f(64)), lanes),
            ] {
                error!(encoded_ty(value), CanonicalGraphError::InvalidWidth);
            }
        }
        for value in [Ty::Vec(Scalar::Bool, 4), Ty::Mask(Scalar::Char, 4)] {
            error!(encoded_ty(value), CanonicalGraphError::InvalidWidth);
        }
    }

    #[test]
    fn canonical_field_codec_maps_typed_semantic_errors_exactly() {
        let mut out = vec![0xa5, 0x5a];
        error!(text(&mut out, "a\0b"), CanonicalGraphError::EmbeddedNul);
        assert_eq!(out, [0xa5, 0x5a]);
        error!(
            encoded_scalar(Scalar::Param(0)),
            CanonicalGraphError::InvalidGraph
        );
        for value in [Ty::Param(0), Ty::IntVar(0), Ty::FloatVar(0), Ty::Error] {
            error!(encoded_ty(value), CanonicalGraphError::InvalidGraph);
        }
        for mode in [ParamMode::Borrow, ParamMode::BorrowMut] {
            error!(
                encode_param_mode(&mut out, mode),
                CanonicalGraphError::InvalidGraph
            );
            assert_eq!(out, [0xa5, 0x5a]);
        }
        error!(
            prim(&mut out, PrimScalar::Int(i(24))),
            CanonicalGraphError::InvalidWidth
        );
        assert_eq!(out, [0xa5, 0x5a]);
        error!(
            scalar(&mut out, Scalar::Struct(0), &|_| Err(
                CanonicalGraphError::MissingReference
            )),
            CanonicalGraphError::MissingReference
        );
        assert_eq!(out, [0xa5, 0x5a]);
        error!(
            ty(
                &mut out,
                Ty::Result(Scalar::Bool, Scalar::Param(0)),
                &ordinal
            ),
            CanonicalGraphError::InvalidGraph
        );
        assert_eq!(out, [0xa5, 0x5a]);
    }

    #[test]
    fn canonical_field_codec_checks_counts_and_forms_function_records() {
        assert_eq!(checked_count(0), Ok(0));
        assert_eq!(checked_count(u32::MAX as usize), Ok(u32::MAX));
        #[cfg(target_pointer_width = "64")]
        error!(
            checked_count(u32::MAX as usize + 1),
            CanonicalGraphError::InvalidCount
        );

        let definition = FunctionTypeDef {
            params: vec![(ParamMode::Out, Scalar::Bool)],
            ret: Ty::Unit,
            return_borrow: hir::ReturnBorrowSummary::None,
            return_region: hir::ReturnRegionSummary::None,
        };
        assert_eq!(definition.params, [(ParamMode::Out, Scalar::Bool)]);
        assert_eq!(definition.ret, Ty::Unit);
        assert!(matches!(
            (definition.return_borrow, definition.return_region),
            (
                hir::ReturnBorrowSummary::None,
                hir::ReturnRegionSummary::None
            )
        ));
    }
}
