//! The nominal records returned by standard socket observations.
use crate::{IntTy, Ty, hir::{FieldDef, StructDef}};

pub const ENDPOINT: &str = "net.endpoint";
pub const DATAGRAM: &str = "net.datagram";

pub fn definitions(endpoint: u32) -> [StructDef; 2] {
    let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
    let record = |name: &str, fields: [(&str, Ty); 2]| StructDef {
        name: name.into(), source_name: name.into(),
        fields: fields.into_iter().map(|(name, ty)| FieldDef { name: name.into(), ty }).collect(),
        align: None, c_repr: false,
    };
    [record(ENDPOINT, [("host", Ty::String), ("port", i64_ty)]),
     record(DATAGRAM, [("n", i64_ty), ("peer", Ty::Struct(endpoint))])]
}

pub fn record_id(structs: &[StructDef], name: &str) -> Option<u32> {
    u32::try_from(structs.iter().position(|d| d.name == name)?).ok()
}

pub fn schemas_valid(structs: &[StructDef]) -> bool {
    let endpoint = record_id(structs, ENDPOINT);
    let mut seen = [false; 2];
    for actual in structs {
        let index = match (actual.name.as_str(), actual.source_name.as_str()) {
            (ENDPOINT, ENDPOINT) => 0,
            (DATAGRAM, DATAGRAM) if endpoint.is_some() => 1,
            (ENDPOINT | DATAGRAM, _) | (_, ENDPOINT | DATAGRAM) => return false,
            _ => continue,
        };
        if std::mem::replace(&mut seen[index], true) { return false; }
        let expected = &definitions(endpoint.unwrap_or(0))[index];
        if actual.align.is_some() || actual.c_repr || actual.fields.len() != expected.fields.len()
            || !actual.fields.iter().zip(&expected.fields).all(|(a,b)| a.name == b.name && a.ty == b.ty) {
            return false;
        }
    }
    true
}

pub fn socket_handle(ty: Ty) -> bool {
    matches!(ty, Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket)
}
