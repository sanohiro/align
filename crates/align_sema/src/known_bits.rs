//! Expression-local integer facts for diagnostics. These facts never authorize generated code.

use crate::{Expr, ExprKind, IntTy, Ty};
use align_ast::{BinOp, UnOp};

#[derive(Clone, Copy, Default)]
struct Bits {
    zero: u64,
    one: u64,
}

fn mask(bits: u8) -> Option<u64> {
    match bits {
        8 | 16 | 32 => Some((1u64 << bits) - 1),
        64 => Some(u64::MAX),
        _ => None,
    }
}

fn facts(expr: &Expr, budget: u8) -> Bits {
    let Ty::Int(integer) = expr.ty else {
        return Bits::default();
    };
    let Some(value_mask) = mask(integer.bits) else {
        return Bits::default();
    };
    if budget == 0 {
        return Bits::default();
    }
    let child = |value| facts(value, budget - 1);
    let result = match &expr.kind {
        ExprKind::Int(value) => {
            // Work modulo the already-finalized source width, including negative literals.
            let Ok(low) = u64::try_from(value.rem_euclid(1i128 << integer.bits)) else {
                return Bits::default();
            };
            Bits {
                zero: !low,
                one: low,
            }
        }
        ExprKind::Unary {
            op: UnOp::Neg,
            expr: inner,
        } => {
            let ExprKind::Int(value) = inner.kind else {
                return Bits::default();
            };
            let Some(negative) = value.checked_neg() else {
                return Bits::default();
            };
            let Ok(low) = u64::try_from(negative.rem_euclid(1i128 << integer.bits)) else {
                return Bits::default();
            };
            Bits {
                zero: !low,
                one: low,
            }
        }
        ExprKind::Unary {
            op: UnOp::BitNot,
            expr: inner,
        } => {
            let bits = child(inner);
            Bits {
                zero: bits.one,
                one: bits.zero,
            }
        }
        ExprKind::Binary { op, lhs, rhs, .. } if lhs.ty == expr.ty && rhs.ty == expr.ty => {
            let left = child(lhs);
            let right = child(rhs);
            match op {
                BinOp::BitAnd => Bits {
                    zero: left.zero | right.zero,
                    one: left.one & right.one,
                },
                BinOp::BitOr => Bits {
                    zero: left.zero & right.zero,
                    one: left.one | right.one,
                },
                BinOp::BitXor => Bits {
                    zero: (left.zero & right.zero) | (left.one & right.one),
                    one: (left.zero & right.one) | (left.one & right.zero),
                },
                BinOp::Shl | BinOp::Shr => {
                    let ExprKind::Int(amount) = rhs.kind else {
                        return Bits::default();
                    };
                    let Ok(shift) = u32::try_from(amount) else {
                        return Bits::default();
                    };
                    if shift >= u32::from(integer.bits) {
                        return Bits::default();
                    }
                    if *op == BinOp::Shl {
                        Bits {
                            zero: (left.zero << shift) | ((1u64 << shift) - 1),
                            one: left.one << shift,
                        }
                    } else {
                        let upper = value_mask ^ (value_mask >> shift);
                        let sign = 1u64 << (integer.bits - 1);
                        Bits {
                            zero: (left.zero >> shift)
                                | if !integer.signed || left.zero & sign != 0 {
                                    upper
                                } else {
                                    0
                                },
                            one: (left.one >> shift)
                                | if integer.signed && left.one & sign != 0 {
                                    upper
                                } else {
                                    0
                                },
                        }
                    }
                }
                _ => Bits::default(),
            }
        }
        ExprKind::Cast(inner) => {
            let Ty::Int(source) = inner.ty else {
                return Bits::default();
            };
            let Some(source_mask) = mask(source.bits) else {
                return Bits::default();
            };
            let mut bits = child(inner);
            if integer.bits > source.bits {
                let upper = value_mask ^ source_mask;
                let sign = 1u64 << (source.bits - 1);
                if !source.signed || bits.zero & sign != 0 {
                    bits.zero |= upper;
                } else if bits.one & sign != 0 {
                    bits.one |= upper;
                }
            }
            bits
        }
        _ => Bits::default(),
    };
    Bits {
        zero: result.zero & value_mask,
        one: result.one & value_mask,
    }
}

/// Prove the finalized expression's entire integer interval fits the target, including signedness.
pub(crate) fn lossless_integer_cast(expr: &Expr, target: Ty) -> bool {
    let (Ty::Int(source), Ty::Int(target)) = (expr.ty, target) else {
        return false;
    };
    let (Some(source_mask), Some(_)) = (mask(source.bits), mask(target.bits)) else {
        return false;
    };
    let bits = facts(expr, 48);
    let sign = 1u64 << (source.bits - 1);
    let maximum_bits = source_mask & !bits.zero;
    let (minimum, maximum) = if !source.signed {
        (i128::from(bits.one), i128::from(maximum_bits))
    } else {
        let signed = |value: u64| {
            i128::from(value)
                - if value & sign != 0 {
                    1i128 << source.bits
                } else {
                    0
                }
        };
        let minimum = if bits.zero & sign != 0 {
            bits.one
        } else {
            bits.one | sign
        };
        let maximum = if bits.one & sign != 0 {
            maximum_bits
        } else {
            maximum_bits & !sign
        };
        (signed(minimum), signed(maximum))
    };
    let IntTy {
        bits: width,
        signed,
    } = target;
    let (low, high) = if signed {
        (-(1i128 << (width - 1)), (1i128 << (width - 1)) - 1)
    } else {
        (0, (1i128 << width) - 1)
    };
    minimum >= low && maximum <= high
}
