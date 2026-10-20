//! Width-exact native identities for cancellation-sensitive elementary operations.
use align_sema::hir::MathFn;
use inkwell::{context::Context, types::FunctionType};

pub(super) const ROWS: [(&str, MathFn, u8); 4] = [
    ("expm1f", MathFn::Expm1, 32),
    ("expm1", MathFn::Expm1, 64),
    ("log1pf", MathFn::Log1p, 32),
    ("log1p", MathFn::Log1p, 64),
];

pub(super) const LOWERING_REASON: &str =
    "LLVM 22 has no vector intrinsic; emitted lane-wise libm calls";

pub(super) fn contains_symbol(symbol: &str) -> bool {
    ROWS.iter().any(|row| row.0 == symbol)
}

pub(super) fn symbol(operation: MathFn, bits: u8) -> Option<&'static str> {
    ROWS.iter()
        .find(|row| row.1 == operation && row.2 == bits)
        .map(|row| row.0)
}

pub(super) fn function_type<'c>(ctx: &'c Context, symbol: &str) -> Option<FunctionType<'c>> {
    let row = ROWS.iter().find(|row| row.0 == symbol)?;
    let float = match row.2 {
        32 => ctx.f32_type(),
        64 => ctx.f64_type(),
        _ => return None,
    };
    Some(float.fn_type(&[float.into()], false))
}

pub(super) fn extern_abi_matches<'c>(
    symbol: &str,
    actual: FunctionType<'c>,
    ctx: &'c Context,
) -> bool {
    function_type(ctx, symbol).is_none_or(|expected| actual == expected)
}
