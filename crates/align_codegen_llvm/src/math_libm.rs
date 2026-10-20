//! Width-exact native identities for elementary math and target-combined helpers.
use align_sema::hir::MathFn;
use inkwell::{
    AddressSpace,
    context::Context,
    types::{BasicTypeEnum, FunctionType},
};

pub(super) const ROWS: [(&str, MathFn, u8); 4] = [
    ("expm1f", MathFn::Expm1, 32),
    ("expm1", MathFn::Expm1, 64),
    ("log1pf", MathFn::Log1p, 32),
    ("log1p", MathFn::Log1p, 64),
];

const TRIG_ROWS: [(&str, MathFn, u8); 6] = [
    ("sinf", MathFn::Sin, 32),
    ("sin", MathFn::Sin, 64),
    ("cosf", MathFn::Cos, 32),
    ("cos", MathFn::Cos, 64),
    ("atan2f", MathFn::Atan2, 32),
    ("atan2", MathFn::Atan2, 64),
];

const HELPERS: [&str; 4] = ["sincosf", "sincos", "__sincosf_stret", "__sincos_stret"];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeTarget {
    Linux,
    DarwinArm64,
    Unsupported,
}

impl NativeTarget {
    pub(super) fn new(arm: Option<super::ffi_aarch64::Abi>, sysv: bool) -> Self {
        match arm {
            Some(super::ffi_aarch64::Abi::Darwin) => Self::DarwinArm64,
            Some(super::ffi_aarch64::Abi::Linux) => Self::Linux,
            None if sysv => Self::Linux,
            None => Self::Unsupported,
        }
    }
}

fn fixed_rows() -> impl Iterator<Item = &'static (&'static str, MathFn, u8)> {
    ROWS.iter().chain(TRIG_ROWS.iter())
}

pub(super) fn symbols() -> impl Iterator<Item = &'static str> {
    fixed_rows().map(|row| row.0).chain(HELPERS)
}

pub(super) const LOWERING_REASON: &str =
    "LLVM 22 has no vector intrinsic; emitted lane-wise libm calls";

pub(super) fn contains_symbol(symbol: &str) -> bool {
    symbols().any(|native| native == symbol)
}

pub(super) fn symbol(operation: MathFn, bits: u8) -> Option<&'static str> {
    fixed_rows()
        .find(|row| row.1 == operation && row.2 == bits)
        .map(|row| row.0)
}

pub(super) fn function_type<'c>(ctx: &'c Context, symbol: &str) -> Option<FunctionType<'c>> {
    let row = fixed_rows().find(|row| row.0 == symbol)?;
    let float = match row.2 {
        32 => ctx.f32_type(),
        64 => ctx.f64_type(),
        _ => return None,
    };
    let arguments = if row.1 == MathFn::Atan2 { 2 } else { 1 };
    Some(float.fn_type(&vec![float.into(); arguments], false))
}

pub(super) fn extern_abi_matches<'c>(
    symbol: &str,
    actual: FunctionType<'c>,
    ctx: &'c Context,
    target: NativeTarget,
) -> bool {
    if let Some(expected) = function_type(ctx, symbol) {
        return actual == expected;
    }
    let (bits, pair_return) = match symbol {
        "sincosf" => (32, false),
        "sincos" => (64, false),
        "__sincosf_stret" => (32, true),
        "__sincos_stret" => (64, true),
        _ => return true,
    };
    let float = if bits == 32 {
        ctx.f32_type()
    } else {
        ctx.f64_type()
    };
    if !pair_return {
        let pointer = ctx.ptr_type(AddressSpace::default());
        return target == NativeTarget::Linux
            && actual
                == ctx
                    .void_type()
                    .fn_type(&[float.into(), pointer.into(), pointer.into()], false);
    }
    if target != NativeTarget::DarwinArm64
        || actual.is_var_arg()
        || actual.get_param_types() != [float.into()]
    {
        return false;
    }
    // Darwin returns this two-member HFA in FP registers. Source records are nominal LLVM
    // structs, so compare the already-classified physical HFA shape, not the struct identity.
    // A padded/non-HFA source record was coerced to another return class by the C ABI owner.
    let Some(BasicTypeEnum::StructType(record)) = actual.get_return_type() else {
        return false;
    };
    if record.is_packed() {
        return false;
    }
    record
        .get_field_types()
        .into_iter()
        .try_fold(0u32, |count, field| {
            let members = match field {
                BasicTypeEnum::FloatType(ty) if ty == float => 1,
                BasicTypeEnum::ArrayType(ty) if ty.get_element_type() == float.into() => ty.len(),
                _ => return None,
            };
            count.checked_add(members).filter(|total| *total <= 2)
        })
        == Some(2)
}
