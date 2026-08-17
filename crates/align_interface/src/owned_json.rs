use crate::{Hash128, IStructDef, IType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnedJsonObjectFormat {
    Elf,
    MachO,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedJsonTarget {
    pub triple: String,
    pub object_format: OwnedJsonObjectFormat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedJsonInterfaceEntry {
    pub type_name: String,
    pub envelope: Vec<u8>,
}

#[derive(Clone, Copy)]
struct FieldShape {
    tag: u8,
    bits: Option<u8>,
    unsigned: Option<bool>,
    size: u32,
    align: u32,
    allocation: u8,
    drop_tag: u8,
    optional: bool,
    array: bool,
}

fn named(ty: &IType, expected: &str) -> bool {
    matches!(ty, IType::Named { path, args } if path == expected && args.is_empty())
}

fn integer_shape(path: &str) -> Option<FieldShape> {
    let (unsigned, digits) = match path.as_bytes().first().copied() {
        Some(b'i') => (false, &path[1..]),
        Some(b'u') => (true, &path[1..]),
        _ => return None,
    };
    let bits: u8 = digits.parse().ok()?;
    if !matches!(bits, 8 | 16 | 32 | 64) {
        return None;
    }
    let bytes = u32::from(bits / 8);
    Some(FieldShape {
        tag: 0x01,
        bits: Some(bits),
        unsigned: Some(unsigned),
        size: bytes,
        align: bytes,
        allocation: 0,
        drop_tag: 0,
        optional: false,
        array: false,
    })
}

fn field_shape(ty: &IType) -> Option<FieldShape> {
    if let IType::Named { path, args } = ty {
        if args.is_empty() {
            if let Some(shape) = integer_shape(path) {
                return Some(shape);
            }
            if path == "bool" {
                return Some(FieldShape {
                    tag: 0x03,
                    bits: None,
                    unsigned: None,
                    size: 1,
                    align: 1,
                    allocation: 0,
                    drop_tag: 0,
                    optional: false,
                    array: false,
                });
            }
            if path == "string" {
                return Some(FieldShape {
                    tag: 0x10,
                    bits: None,
                    unsigned: None,
                    size: 16,
                    align: 8,
                    allocation: 1,
                    drop_tag: 1,
                    optional: false,
                    array: false,
                });
            }
        }
        if path == "Option" && args.len() == 1 && named(&args[0], "string") {
            return Some(FieldShape {
                tag: 0x11,
                bits: None,
                unsigned: None,
                size: 24,
                align: 8,
                allocation: 1,
                drop_tag: 2,
                optional: true,
                array: false,
            });
        }
        if path == "array" && args.len() == 1 && named(&args[0], "string") {
            return Some(FieldShape {
                tag: 0x12,
                bits: None,
                unsigned: None,
                size: 16,
                align: 8,
                allocation: 1,
                drop_tag: 3,
                optional: false,
                array: true,
            });
        }
    }
    None
}

pub(crate) fn is_owned_json_struct(definition: &IStructDef) -> bool {
    definition.type_params.is_empty()
        && definition.align.is_none()
        && !definition.c_repr
        && definition
            .fields
            .iter()
            .all(|(_, ty)| field_shape(ty).is_some())
        && definition
            .fields
            .iter()
            .any(|(_, ty)| field_shape(ty).is_some_and(|shape| shape.allocation == 1))
}

fn align_up(value: u32, align: u32) -> Option<u32> {
    value
        .checked_add(align.checked_sub(1)?)
        .map(|sum| sum & !(align - 1))
}

fn descriptor_layout(definition: &IStructDef) -> Option<(Vec<FieldShape>, Vec<u32>)> {
    if !is_owned_json_struct(definition) || definition.fields.is_empty() {
        return None;
    }
    let shapes = definition
        .fields
        .iter()
        .map(|(_, ty)| field_shape(ty))
        .collect::<Option<Vec<_>>>()?;
    let mut physical = (0..shapes.len()).collect::<Vec<_>>();
    physical.sort_by_key(|&index| std::cmp::Reverse(shapes[index].align));
    let mut offsets = vec![0u32; shapes.len()];
    let mut cursor = 0u32;
    for index in physical {
        cursor = align_up(cursor, shapes[index].align)?;
        offsets[index] = cursor;
        cursor = cursor.checked_add(shapes[index].size)?;
    }
    Some((shapes, offsets))
}

pub fn encode_owned_json_descriptor(definition: &IStructDef) -> Option<Vec<u8>> {
    let (shapes, offsets) = descriptor_layout(definition)?;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[1, 0, 1]);
    bytes.extend_from_slice(&u32::try_from(definition.fields.len()).ok()?.to_le_bytes());
    for (index, (name, _)) in definition.fields.iter().enumerate() {
        if name.is_empty()
            || !name.is_ascii()
            || !name.bytes().enumerate().all(|(i, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || (i > 0 && byte.is_ascii_digit())
            })
        {
            return None;
        }
        let shape = shapes[index];
        bytes.extend_from_slice(&u32::try_from(name.len()).ok()?.to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(shape.tag);
        if let Some(bits) = shape.bits {
            bytes.push(bits);
            bytes.push(u8::from(shape.unsigned?));
        }
        if shape.array {
            bytes.extend_from_slice(&[0x10, 1]);
        }
        let base = offsets[index];
        let payload = if shape.optional {
            base.checked_add(8)?
        } else {
            base
        };
        let optional_tag = if shape.optional { base } else { u32::MAX };
        bytes.extend_from_slice(&payload.to_le_bytes());
        bytes.extend_from_slice(&optional_tag.to_le_bytes());
        bytes.extend_from_slice(&shape.size.to_le_bytes());
        bytes.extend_from_slice(&shape.align.to_le_bytes());
        bytes.push(shape.allocation);
        bytes.push(shape.drop_tag);
    }
    Some(bytes)
}

const ABI_CELLS: [u8; 11] = [0, 8, 8, 16, 8, 16, 8, 24, 8, 0, 8];

pub fn encode_owned_json_envelope(target: &OwnedJsonTarget, descriptor: &[u8]) -> Option<Vec<u8>> {
    if target.triple.is_empty()
        || !target.triple.is_ascii()
        || target.triple.as_bytes().contains(&0)
    {
        return None;
    }
    let mut prefix = Vec::new();
    prefix.push(1);
    prefix.extend_from_slice(&u32::try_from(target.triple.len()).ok()?.to_le_bytes());
    prefix.extend_from_slice(target.triple.as_bytes());
    prefix.push(match target.object_format {
        OwnedJsonObjectFormat::Elf => 0,
        OwnedJsonObjectFormat::MachO => 1,
    });
    prefix.extend_from_slice(&ABI_CELLS);
    let hash = Hash128::of(&prefix);
    let mut envelope = prefix;
    envelope.extend_from_slice(&hash.lo.to_le_bytes());
    envelope.extend_from_slice(&hash.hi.to_le_bytes());
    envelope.extend_from_slice(&u32::try_from(descriptor.len()).ok()?.to_le_bytes());
    envelope.extend_from_slice(descriptor);
    Some(envelope)
}

pub(crate) fn entries_for_structs(
    structs: &[IStructDef],
    target: &OwnedJsonTarget,
) -> Option<Vec<OwnedJsonInterfaceEntry>> {
    structs
        .iter()
        .filter(|definition| is_owned_json_struct(definition))
        .map(|definition| {
            let descriptor = encode_owned_json_descriptor(definition)?;
            Some(OwnedJsonInterfaceEntry {
                type_name: definition.name.clone(),
                envelope: encode_owned_json_envelope(target, &descriptor)?,
            })
        })
        .collect()
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], &'static str> {
        let end = self.pos.checked_add(count).ok_or("length overflow")?;
        let value = self.bytes.get(self.pos..end).ok_or("truncated envelope")?;
        self.pos = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| "truncated u32")?,
        ))
    }

    fn u64(&mut self) -> Result<u64, &'static str> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().map_err(|_| "truncated u64")?,
        ))
    }
}

struct DescriptorCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> DescriptorCursor<'a> {
    fn take(&mut self, count: usize, reason: &'static str) -> Result<&'a [u8], &'static str> {
        let end = self.pos.checked_add(count).ok_or(reason)?;
        let value = self.bytes.get(self.pos..end).ok_or(reason)?;
        self.pos = end;
        Ok(value)
    }

    fn u8(&mut self, reason: &'static str) -> Result<u8, &'static str> {
        Ok(self.take(1, reason)?[0])
    }

    fn u32(&mut self, reason: &'static str) -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(
            self.take(4, reason)?.try_into().map_err(|_| reason)?,
        ))
    }
}

fn valid_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name.is_ascii()
        && name.iter().copied().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        })
}

fn validate_descriptor(definition: &IStructDef, descriptor: &[u8]) -> Result<(), &'static str> {
    let (shapes, offsets) = descriptor_layout(definition).ok_or("owned JSON descriptor graph")?;
    let mut cursor = DescriptorCursor {
        bytes: descriptor,
        pos: 0,
    };
    let header = cursor.take(7, "owned JSON descriptor header")?;
    if header[0] != 1 {
        return Err("owned JSON descriptor version");
    }
    if header[1] != 0 {
        return Err("owned JSON descriptor layout mode");
    }
    if header[2] != 1 {
        return Err("owned JSON descriptor layout algorithm");
    }
    let field_count = u32::from_le_bytes(
        header[3..7]
            .try_into()
            .map_err(|_| "owned JSON descriptor field count")?,
    );
    if field_count == 0 {
        return Err("owned JSON descriptor zero field count");
    }
    if usize::try_from(field_count).ok() != Some(definition.fields.len()) {
        return Err("owned JSON descriptor field count");
    }

    let mut names = std::collections::HashSet::with_capacity(definition.fields.len());
    for (index, (expected_name, _)) in definition.fields.iter().enumerate() {
        let name_len = usize::try_from(cursor.u32("owned JSON descriptor field name length")?)
            .map_err(|_| "owned JSON descriptor field name length")?;
        let name = cursor.take(name_len, "owned JSON descriptor field name")?;
        if !valid_name(name) {
            return Err("owned JSON descriptor field name grammar");
        }
        if !names.insert(name) {
            return Err("owned JSON descriptor duplicate field name");
        }
        if name != expected_name.as_bytes() {
            return Err("owned JSON descriptor field name/order");
        }

        let shape = shapes[index];
        let tag = cursor.u8("owned JSON descriptor field type tag")?;
        if !matches!(tag, 0x01 | 0x03 | 0x10 | 0x11 | 0x12) || tag != shape.tag {
            return Err("owned JSON descriptor field type tag");
        }
        if tag == 0x01 {
            let bits = cursor.u8("owned JSON descriptor integer width")?;
            if !matches!(bits, 8 | 16 | 32 | 64) || Some(bits) != shape.bits {
                return Err("owned JSON descriptor integer width");
            }
            let unsigned = cursor.u8("owned JSON descriptor integer signedness")?;
            if unsigned > 1 || Some(unsigned != 0) != shape.unsigned {
                return Err("owned JSON descriptor integer signedness");
            }
        } else if tag == 0x12 {
            if cursor.u8("owned JSON descriptor array element tag")? != 0x10 {
                return Err("owned JSON descriptor array element tag");
            }
            if cursor.u8("owned JSON descriptor array drop plan")? != 1 {
                return Err("owned JSON descriptor array drop plan");
            }
        }

        let base = offsets[index];
        let expected_payload = if shape.optional {
            base.checked_add(8)
                .ok_or("owned JSON descriptor payload offset")?
        } else {
            base
        };
        if cursor.u32("owned JSON descriptor payload offset")? != expected_payload {
            return Err("owned JSON descriptor payload offset");
        }
        let expected_optional = if shape.optional { base } else { u32::MAX };
        if cursor.u32("owned JSON descriptor optional tag offset")? != expected_optional {
            return Err("owned JSON descriptor optional tag offset");
        }
        if cursor.u32("owned JSON descriptor field size")? != shape.size {
            return Err("owned JSON descriptor field size");
        }
        if cursor.u32("owned JSON descriptor field alignment")? != shape.align {
            return Err("owned JSON descriptor field alignment");
        }
        if cursor.u8("owned JSON descriptor allocation mode")? != shape.allocation {
            return Err("owned JSON descriptor allocation mode");
        }
        if cursor.u8("owned JSON descriptor drop tag")? != shape.drop_tag {
            return Err("owned JSON descriptor drop tag");
        }
    }
    if cursor.pos != descriptor.len() {
        return Err("owned JSON descriptor trailing bytes");
    }
    Ok(())
}

pub(crate) fn validate_entries(
    structs: &[IStructDef],
    entries: &[OwnedJsonInterfaceEntry],
    current: Option<&OwnedJsonTarget>,
) -> Result<(), &'static str> {
    let expected = structs
        .iter()
        .filter(|definition| is_owned_json_struct(definition))
        .collect::<Vec<_>>();
    if entries.len() != expected.len() {
        return Err("owned JSON descriptor cardinality");
    }
    for (entry, definition) in entries.iter().zip(expected) {
        if !valid_name(entry.type_name.as_bytes()) {
            return Err("owned JSON descriptor entry name grammar");
        }
        if entry.type_name != definition.name {
            return Err("owned JSON descriptor name/order");
        }
        let mut cursor = Cursor {
            bytes: &entry.envelope,
            pos: 0,
        };
        if cursor.u8()? != 1 {
            return Err("owned JSON envelope version");
        }
        let triple_len = usize::try_from(cursor.u32()?).map_err(|_| "target triple length")?;
        if triple_len == 0 {
            return Err("empty target triple");
        }
        let triple = cursor.take(triple_len)?;
        if !triple.is_ascii() || triple.contains(&0) {
            return Err("invalid target triple");
        }
        let object = cursor.u8()?;
        if object > 1 {
            return Err("object format");
        }
        let cells = cursor.take(ABI_CELLS.len())?;
        if cells != ABI_CELLS {
            return Err("target ABI cells");
        }
        let prefix_end = cursor.pos;
        let stored = Hash128 {
            lo: cursor.u64()?,
            hi: cursor.u64()?,
        };
        if stored != Hash128::of(&entry.envelope[..prefix_end]) {
            return Err("target ABI hash");
        }
        if let Some(target) = current {
            if triple != target.triple.as_bytes() {
                return Err("target triple mismatch");
            }
            let expected_object = match target.object_format {
                OwnedJsonObjectFormat::Elf => 0,
                OwnedJsonObjectFormat::MachO => 1,
            };
            if object != expected_object {
                return Err("object format mismatch");
            }
        }
        let descriptor_len =
            usize::try_from(cursor.u32().map_err(|_| "owned JSON descriptor length")?)
                .map_err(|_| "owned JSON descriptor length")?;
        let descriptor = cursor
            .take(descriptor_len)
            .map_err(|_| "owned JSON descriptor length/bound")?;
        if cursor.pos != entry.envelope.len() {
            return Err("owned JSON envelope trailing bytes");
        }
        validate_descriptor(definition, descriptor)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(path: &str) -> IType {
        IType::Named {
            path: path.to_string(),
            args: Vec::new(),
        }
    }

    fn app(path: &str, argument: IType) -> IType {
        IType::Named {
            path: path.to_string(),
            args: vec![argument],
        }
    }

    fn owned_task() -> IStructDef {
        IStructDef {
            name: "OwnedTask".to_string(),
            type_params: Vec::new(),
            fields: vec![
                ("id".to_string(), ty("string")),
                ("priority".to_string(), ty("i64")),
                ("attempts".to_string(), ty("u16")),
                ("limit".to_string(), ty("u64")),
                ("enabled".to_string(), ty("bool")),
                ("argv".to_string(), app("array", ty("string"))),
                ("note".to_string(), app("Option", ty("string"))),
            ],
            align: None,
            c_repr: false,
            generic_body: None,
        }
    }

    fn hex(bytes: &str) -> Vec<u8> {
        bytes
            .split_ascii_whitespace()
            .map(|pair| u8::from_str_radix(pair, 16).expect("hex byte"))
            .collect()
    }

    #[test]
    fn owned_task_descriptor_and_target_envelope_match_the_normative_goldens() {
        let expected = hex(
            "01 00 01 07 00 00 00
             02 00 00 00 69 64 10 00 00 00 00 ff ff ff ff 10 00 00 00 08 00 00 00 01 01
             08 00 00 00 70 72 69 6f 72 69 74 79 01 40 00 10 00 00 00 ff ff ff ff 08 00 00 00 08 00 00 00 00 00
             08 00 00 00 61 74 74 65 6d 70 74 73 01 10 01 48 00 00 00 ff ff ff ff 02 00 00 00 02 00 00 00 00 00
             05 00 00 00 6c 69 6d 69 74 01 40 01 18 00 00 00 ff ff ff ff 08 00 00 00 08 00 00 00 00 00
             07 00 00 00 65 6e 61 62 6c 65 64 03 4a 00 00 00 ff ff ff ff 01 00 00 00 01 00 00 00 00 00
             04 00 00 00 61 72 67 76 12 10 01 20 00 00 00 ff ff ff ff 10 00 00 00 08 00 00 00 01 03
             04 00 00 00 6e 6f 74 65 11 38 00 00 00 30 00 00 00 18 00 00 00 08 00 00 00 01 02",
        );
        let descriptor = encode_owned_json_descriptor(&owned_task()).expect("descriptor");
        assert_eq!(descriptor.len(), 214);
        assert_eq!(descriptor, expected);

        let target = OwnedJsonTarget {
            triple: "x86_64-pc-linux-gnu".to_string(),
            object_format: OwnedJsonObjectFormat::Elf,
        };
        let envelope = encode_owned_json_envelope(&target, &descriptor).expect("envelope");
        assert_eq!(envelope.len(), 270);
        assert_eq!(
            &envelope[..36],
            hex(
                "01 13 00 00 00 78 38 36 5f 36 34 2d 70 63 2d 6c 69 6e 75 78 2d 67 6e 75 00 00 08 08 10 08 10 08 18 08 00 08"
            )
        );
        assert_eq!(
            &envelope[36..52],
            hex("d4 df f2 a5 8e c8 21 27 2a f3 26 2f 96 1a eb a5")
        );
        assert_eq!(&envelope[52..56], &[0xd6, 0, 0, 0]);
        assert_eq!(&envelope[56..], descriptor);
    }

    #[test]
    fn envelope_validation_rejects_target_drift_before_descriptor_use() {
        let definition = owned_task();
        let linux = OwnedJsonTarget {
            triple: "x86_64-pc-linux-gnu".to_string(),
            object_format: OwnedJsonObjectFormat::Elf,
        };
        let entry = OwnedJsonInterfaceEntry {
            type_name: definition.name.clone(),
            envelope: encode_owned_json_envelope(
                &linux,
                &encode_owned_json_descriptor(&definition).unwrap(),
            )
            .unwrap(),
        };
        let apple = OwnedJsonTarget {
            triple: "x86_64-apple-darwin".to_string(),
            object_format: OwnedJsonObjectFormat::MachO,
        };
        assert_eq!(
            validate_entries(&[definition], &[entry], Some(&apple)),
            Err("target triple mismatch")
        );
    }

    #[test]
    fn envelope_and_descriptor_mutations_fail_closed_in_boundary_order() {
        let definition = owned_task();
        let target = OwnedJsonTarget {
            triple: "x86_64-pc-linux-gnu".to_string(),
            object_format: OwnedJsonObjectFormat::Elf,
        };
        let descriptor = encode_owned_json_descriptor(&definition).unwrap();
        let canonical = encode_owned_json_envelope(&target, &descriptor).unwrap();
        let check = |envelope: Vec<u8>| {
            validate_entries(
                std::slice::from_ref(&definition),
                &[OwnedJsonInterfaceEntry {
                    type_name: definition.name.clone(),
                    envelope,
                }],
                Some(&target),
            )
        };

        let mut version = canonical.clone();
        version[0] = 2;
        assert_eq!(check(version), Err("owned JSON envelope version"));

        let mut empty_triple = canonical.clone();
        empty_triple[1..5].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(check(empty_triple), Err("empty target triple"));

        let mut long_triple = canonical.clone();
        long_triple[1..5].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(check(long_triple), Err("truncated envelope"));

        let mut invalid_triple = canonical.clone();
        invalid_triple[5] = 0;
        assert_eq!(check(invalid_triple), Err("invalid target triple"));

        let mut object = canonical.clone();
        object[24] = 2;
        assert_eq!(check(object), Err("object format"));

        let mut abi = canonical.clone();
        abi[25] = 1;
        assert_eq!(check(abi), Err("target ABI cells"));

        let mut hash = canonical.clone();
        hash[36] ^= 1;
        assert_eq!(check(hash), Err("target ABI hash"));

        for current in [
            OwnedJsonTarget {
                triple: "aarch64-unknown-linux-gnu".to_string(),
                object_format: OwnedJsonObjectFormat::Elf,
            },
            OwnedJsonTarget {
                triple: "x86_64-apple-darwin".to_string(),
                object_format: OwnedJsonObjectFormat::MachO,
            },
            OwnedJsonTarget {
                triple: "aarch64-apple-darwin".to_string(),
                object_format: OwnedJsonObjectFormat::MachO,
            },
        ] {
            let entry = OwnedJsonInterfaceEntry {
                type_name: definition.name.clone(),
                envelope: canonical.clone(),
            };
            assert_eq!(
                validate_entries(std::slice::from_ref(&definition), &[entry], Some(&current),),
                Err("target triple mismatch"),
            );
        }
        let wrong_object = OwnedJsonTarget {
            triple: target.triple.clone(),
            object_format: OwnedJsonObjectFormat::MachO,
        };
        assert_eq!(
            validate_entries(
                std::slice::from_ref(&definition),
                &[OwnedJsonInterfaceEntry {
                    type_name: definition.name.clone(),
                    envelope: canonical.clone(),
                }],
                Some(&wrong_object),
            ),
            Err("object format mismatch"),
        );

        let mut descriptor_bound = canonical.clone();
        descriptor_bound[52..56].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            check(descriptor_bound),
            Err("owned JSON descriptor length/bound")
        );

        let mut descriptor_version = canonical.clone();
        descriptor_version[56] = 2;
        assert_eq!(
            check(descriptor_version),
            Err("owned JSON descriptor version")
        );

        let mut trailing = canonical.clone();
        trailing.push(0);
        assert_eq!(check(trailing), Err("owned JSON envelope trailing bytes"));

        assert_eq!(
            validate_entries(std::slice::from_ref(&definition), &[], Some(&target)),
            Err("owned JSON descriptor cardinality")
        );

        assert_eq!(
            validate_entries(
                std::slice::from_ref(&definition),
                &[OwnedJsonInterfaceEntry {
                    type_name: "0bad".to_string(),
                    envelope: canonical.clone(),
                }],
                Some(&target),
            ),
            Err("owned JSON descriptor entry name grammar")
        );

        let mut second = definition.clone();
        second.name = "ZOwnedTask".to_string();
        let second_entry = OwnedJsonInterfaceEntry {
            type_name: second.name.clone(),
            envelope: encode_owned_json_envelope(
                &target,
                &encode_owned_json_descriptor(&second).unwrap(),
            )
            .unwrap(),
        };
        let first_entry = OwnedJsonInterfaceEntry {
            type_name: definition.name.clone(),
            envelope: canonical,
        };
        assert_eq!(
            validate_entries(
                &[definition, second],
                &[second_entry, first_entry],
                Some(&target),
            ),
            Err("owned JSON descriptor name/order")
        );
    }

    #[derive(Clone, Copy)]
    struct FieldPositions {
        name_len: usize,
        name: usize,
        tag: usize,
        payload: usize,
        layout: usize,
    }

    fn field_positions(descriptor: &[u8]) -> Vec<FieldPositions> {
        let mut cursor = 7usize;
        let mut positions = Vec::new();
        while cursor < descriptor.len() {
            let name_len = cursor;
            let len =
                u32::from_le_bytes(descriptor[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let name = cursor;
            cursor += len;
            let tag = cursor;
            cursor += 1;
            let payload = cursor;
            cursor += match descriptor[tag] {
                0x01 | 0x12 => 2,
                _ => 0,
            };
            let layout = cursor;
            cursor += 18;
            positions.push(FieldPositions {
                name_len,
                name,
                tag,
                payload,
                layout,
            });
        }
        assert_eq!(cursor, descriptor.len());
        positions
    }

    #[test]
    fn descriptor_mutation_matrix_rejects_every_component_in_contract_order() {
        let definition = owned_task();
        let target = OwnedJsonTarget {
            triple: "x86_64-pc-linux-gnu".to_string(),
            object_format: OwnedJsonObjectFormat::Elf,
        };
        let canonical = encode_owned_json_descriptor(&definition).unwrap();
        let positions = field_positions(&canonical);
        assert_eq!(positions.len(), definition.fields.len());
        let check = |descriptor: Vec<u8>| {
            let entry = OwnedJsonInterfaceEntry {
                type_name: definition.name.clone(),
                envelope: encode_owned_json_envelope(&target, &descriptor).unwrap(),
            };
            validate_entries(std::slice::from_ref(&definition), &[entry], Some(&target))
        };
        let mutate = |offset: usize, value: u8| {
            let mut descriptor = canonical.clone();
            descriptor[offset] = value;
            descriptor
        };

        assert_eq!(
            check(canonical[..6].to_vec()),
            Err("owned JSON descriptor header")
        );
        assert_eq!(check(mutate(0, 2)), Err("owned JSON descriptor version"));
        assert_eq!(
            check(mutate(1, 1)),
            Err("owned JSON descriptor layout mode")
        );
        assert_eq!(
            check(mutate(2, 2)),
            Err("owned JSON descriptor layout algorithm")
        );
        let mut zero_count = canonical.clone();
        zero_count[3..7].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            check(zero_count),
            Err("owned JSON descriptor zero field count")
        );
        let mut wrong_count = canonical.clone();
        wrong_count[3..7].copy_from_slice(&8u32.to_le_bytes());
        assert_eq!(wrong_count[7], 2, "field bytes must remain unread");
        assert_eq!(check(wrong_count), Err("owned JSON descriptor field count"));

        let id = positions[0];
        let priority = positions[1];
        let attempts = positions[2];
        let argv = positions[5];
        let note = positions[6];
        let mut name_overflow = canonical.clone();
        name_overflow[id.name_len..id.name_len + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            check(name_overflow),
            Err("owned JSON descriptor field name")
        );
        assert_eq!(
            check(canonical[..id.name + 1].to_vec()),
            Err("owned JSON descriptor field name")
        );
        assert_eq!(
            check(mutate(id.name, b'0')),
            Err("owned JSON descriptor field name grammar")
        );
        let mut reordered = canonical.clone();
        reordered[priority.name..priority.name + 8]
            .copy_from_slice(&canonical[attempts.name..attempts.name + 8]);
        assert_eq!(
            check(reordered),
            Err("owned JSON descriptor field name/order")
        );
        let mut duplicate = canonical.clone();
        duplicate[note.name..note.name + 4].copy_from_slice(&canonical[argv.name..argv.name + 4]);
        assert_eq!(
            check(duplicate),
            Err("owned JSON descriptor duplicate field name")
        );

        assert_eq!(
            check(mutate(id.tag, 0x02)),
            Err("owned JSON descriptor field type tag")
        );
        assert_eq!(
            check(mutate(priority.payload, 7)),
            Err("owned JSON descriptor integer width")
        );
        assert_eq!(
            check(mutate(priority.payload + 1, 2)),
            Err("owned JSON descriptor integer signedness")
        );
        assert_eq!(
            check(mutate(argv.payload, 0x03)),
            Err("owned JSON descriptor array element tag")
        );
        assert_eq!(
            check(mutate(argv.payload + 1, 2)),
            Err("owned JSON descriptor array drop plan")
        );
        assert_eq!(
            check(mutate(id.layout, 1)),
            Err("owned JSON descriptor payload offset")
        );
        assert_eq!(
            check(mutate(note.layout + 4, 0)),
            Err("owned JSON descriptor optional tag offset")
        );
        assert_eq!(
            check(mutate(id.layout + 8, 15)),
            Err("owned JSON descriptor field size")
        );
        assert_eq!(
            check(mutate(id.layout + 12, 4)),
            Err("owned JSON descriptor field alignment")
        );
        assert_eq!(
            check(mutate(id.layout + 16, 0)),
            Err("owned JSON descriptor allocation mode")
        );
        assert_eq!(
            check(mutate(id.layout + 17, 0)),
            Err("owned JSON descriptor drop tag")
        );
        let mut trailing = canonical;
        trailing.push(0);
        assert_eq!(check(trailing), Err("owned JSON descriptor trailing bytes"));
    }
}
