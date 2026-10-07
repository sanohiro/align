//! Physical C-record plans for the supported little-endian ARM64 ABIs (plan138).

use std::num::NonZeroU32;

use align_sema::{StructDef, Ty};
use inkwell::{
    AddressSpace,
    context::Context,
    targets::{ByteOrdering, TargetData},
    types::{BasicTypeEnum, StructType},
};

use super::CodegenError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Abi {
    Linux,
    Darwin,
}

impl Abi {
    pub(super) fn for_target(triple: &str, data: &TargetData) -> Option<Self> {
        if data.get_byte_ordering() != ByteOrdering::LittleEndian
            || data.get_pointer_byte_size(None) != 8
        {
            return None;
        }
        let mut parts = triple.split('-');
        let arch = parts.next()?;
        let vendor = parts.next()?;
        let os = parts.next()?;
        let environment = parts.next();
        if parts.next().is_some() {
            return None;
        }
        if arch == "aarch64" && os == "linux" && matches!(environment, Some("gnu" | "musl")) {
            return Some(Self::Linux);
        }
        if matches!(arch, "arm64" | "aarch64") && vendor == "apple" && environment.is_none() {
            for prefix in ["macosx", "macos", "darwin"] {
                if let Some(version) = os.strip_prefix(prefix)
                    && version
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || byte == b'.')
                {
                    return Some(Self::Darwin);
                }
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Coerce {
    Integer(u32),
    Pointer,
    Pair { pointers: bool },
    Homogeneous { bits: u8, members: u32 },
    Indirect,
}

impl Coerce {
    fn llvm<'c>(self, ctx: &'c Context) -> Result<BasicTypeEnum<'c>, CodegenError> {
        Ok(match self {
            Self::Integer(bits) => ctx
                .custom_width_int_type(
                    NonZeroU32::new(bits)
                        .ok_or_else(|| CodegenError::Lowering("zero ARM64 coerce width".into()))?,
                )
                .map_err(|error| CodegenError::Lowering(error.into()))?
                .into(),
            Self::Pointer | Self::Indirect => ctx.ptr_type(AddressSpace::default()).into(),
            Self::Pair { pointers: true } => {
                ctx.ptr_type(AddressSpace::default()).array_type(2).into()
            }
            Self::Pair { pointers: false } => ctx.i64_type().array_type(2).into(),
            Self::Homogeneous { bits, members } => {
                if bits == 32 {
                    ctx.f32_type().array_type(members).into()
                } else {
                    ctx.f64_type().array_type(members).into()
                }
            }
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct Record {
    pub(super) id: u32,
    pub(super) alignment: u32,
    /// Full padded storage, rounded up for coercion without discarding tail bytes.
    pub(super) scratch_eightbytes: u32,
    pub(super) scratch_alignment: u32,
    argument: Coerce,
    result: Coerce,
}

impl Record {
    pub(super) fn classify(
        id: u32,
        record: StructType<'_>,
        definition: &StructDef,
        data: &TargetData,
        target: Abi,
    ) -> Result<Self, CodegenError> {
        let invalid = || CodegenError::Lowering("invalid ARM64 C-record ABI layout".into());
        if !definition.c_repr
            || definition.fields.is_empty()
            || !definition.fields.iter().all(|field| match field.ty {
                Ty::Raw => true,
                Ty::Int(ty) => matches!(ty.bits, 8 | 16 | 32 | 64),
                Ty::Float(ty) => matches!(ty.bits, 32 | 64),
                _ => false,
            })
        {
            return Err(invalid());
        }
        let size = data.get_abi_size(&record);
        let alignment = data
            .get_abi_alignment(&record)
            .max(definition.align.unwrap_or(1));
        if size == 0 || !alignment.is_power_of_two() {
            return Err(invalid());
        }
        let homogeneous = definition.fields.first().and_then(|first| {
            let Ty::Float(float) = first.ty else {
                return None;
            };
            let members = u32::try_from(definition.fields.len()).ok()?;
            (members <= 4
                && size == u64::from(members) * u64::from(float.bits / 8)
                && definition.fields.iter().all(|field| field.ty == first.ty))
            .then_some(Coerce::Homogeneous {
                bits: float.bits,
                members,
            })
        });
        let (argument, result) = if let Some(hfa) = homogeneous {
            (hfa, hfa)
        } else if size > 16 {
            (Coerce::Indirect, Coerce::Indirect)
        } else {
            let pointers = definition.fields.iter().all(|field| field.ty == Ty::Raw);
            let argument = if target == Abi::Darwin && alignment >= 16 {
                Coerce::Integer(128)
            } else if size > 8 {
                Coerce::Pair { pointers }
            } else if pointers {
                Coerce::Pointer
            } else {
                Coerce::Integer(64)
            };
            let result = if size <= 8 {
                Coerce::Integer(u32::try_from(size * 8).map_err(|_| invalid())?)
            } else if alignment >= 16 {
                Coerce::Integer(128)
            } else {
                Coerce::Pair { pointers: false }
            };
            (argument, result)
        };
        let coerce_alignment = if argument == Coerce::Integer(128) || result == Coerce::Integer(128)
        {
            16
        } else {
            8
        };
        Ok(Self {
            id,
            alignment,
            scratch_eightbytes: u32::try_from(size.div_ceil(8)).map_err(|_| invalid())?,
            scratch_alignment: alignment.max(coerce_alignment),
            argument,
            result,
        })
    }

    pub(super) fn argument_type<'c>(
        &self,
        ctx: &'c Context,
    ) -> Result<BasicTypeEnum<'c>, CodegenError> {
        self.argument.llvm(ctx)
    }

    pub(super) fn result_type<'c>(
        &self,
        ctx: &'c Context,
        record: StructType<'c>,
    ) -> Result<Option<BasicTypeEnum<'c>>, CodegenError> {
        match self.result {
            Coerce::Indirect => Ok(None),
            Coerce::Homogeneous { .. } => Ok(Some(record.into())),
            other => other.llvm(ctx).map(Some),
        }
    }

    pub(super) fn indirect(&self) -> bool {
        self.argument == Coerce::Indirect
    }

    pub(super) fn argument_stack_alignment(&self, target: Abi) -> Option<u64> {
        (target == Abi::Linux && matches!(self.argument, Coerce::Homogeneous { .. })).then_some(8)
    }
}

/// Darwin's scalar extension is shared by the declaration and call plan. It does not apply
/// to an integer coercion of a source record, whose native return has different ABI rules.
pub(super) fn narrow_attribute(
    ctx: &Context,
    target: Abi,
    ty: Ty,
) -> Option<inkwell::attributes::Attribute> {
    let Ty::Int(integer) = ty else { return None };
    if target != Abi::Darwin || !matches!(integer.bits, 8 | 16) {
        return None;
    }
    let name = if integer.signed { "signext" } else { "zeroext" };
    Some(ctx.create_enum_attribute(
        inkwell::attributes::Attribute::get_named_enum_kind_id(name),
        0,
    ))
}
