//! Destination-width JSON decimal conversion using integer significands and rounding.
//!
//! Native floating-point multiply/divide fast paths depend on the caller's rounding mode.
//! Use minimal-lexical's integer Eisel-Lemire path and bounded stack bigint fallback instead.
//! The pinned dependency exposes these components; grammar is owned by JsonParser::number_span.
use minimal_lexical::{Float, extended_float::extended_to_float, number::Number};

#[inline]
fn magnitude<F: Float>(bytes: &[u8]) -> F {
    let mut mantissa = 0_u64;
    let mut kept = 0_u32;
    let mut discarded = 0_i128;
    let mut point = None;
    let mut end = 0;
    while let Some(&digit) = bytes.get(end) {
        if digit == b'e' || digit == b'E' {
            break;
        }
        if digit == b'.' {
            point = Some(end);
        } else if kept != 0 || digit != b'0' {
            if kept < 19 {
                mantissa = mantissa * 10 + u64::from(digit - b'0');
                kept += 1;
            } else {
                discarded += 1;
            }
        }
        end += 1;
    }
    let mut exponent = 0_i128;
    if end < bytes.len() {
        let mut digits = &bytes[end + 1..];
        let negative = digits.first() == Some(&b'-');
        if matches!(digits.first(), Some(b'+' | b'-')) {
            digits = &digits[1..];
        }
        for &digit in digits {
            exponent = exponent
                .saturating_mul(10)
                .saturating_add(i128::from(digit - b'0'));
        }
        if negative {
            exponent = -exponent;
        }
    }
    let fraction_len = point.map_or(0, |point| end - point - 1);
    let decimal_exponent = exponent
        .saturating_sub(fraction_len as i128)
        .saturating_add(discarded);
    let num = Number {
        mantissa,
        exponent: i32::try_from(decimal_exponent).unwrap_or(if decimal_exponent < 0 {
            i32::MIN
        } else {
            i32::MAX
        }),
        many_digits: discarded != 0,
    };
    let mut fp = minimal_lexical::parse::moderate_path::<F>(&num);
    if fp.exp < 0 {
        fp.exp -= F::INVALID_FP;
        let integer = bytes[..point.unwrap_or(end)].trim_ascii_start_zero();
        let fraction = point
            .map_or(&[][..], |point| &bytes[point + 1..end])
            .trim_ascii_end_zero();
        fp = minimal_lexical::slow::slow::<F, _, _>(num, fp, integer.iter(), fraction.iter());
    }
    extended_to_float::<F>(fp)
}

// Slice-only trimming preserves input borrowing and never allocates or rewrites a token.
trait DecimalDigits {
    fn trim_ascii_start_zero(&self) -> &[u8];
    fn trim_ascii_end_zero(&self) -> &[u8];
}
impl DecimalDigits for [u8] {
    fn trim_ascii_start_zero(&self) -> &[u8] {
        &self[self.iter().position(|&b| b != b'0').unwrap_or(self.len())..]
    }
    fn trim_ascii_end_zero(&self) -> &[u8] {
        &self[..self
            .iter()
            .rposition(|&b| b != b'0')
            .map_or(0, |index| index + 1)]
    }
}

pub(super) fn f32(text: &[u8]) -> Option<f32> {
    let positive = text.strip_prefix(b"-").unwrap_or(text);
    let value = magnitude::<f32>(positive);
    let value = if text.starts_with(b"-") {
        -value
    } else {
        value
    };
    value.is_finite().then_some(value)
}

pub(super) fn f64(text: &[u8]) -> Option<f64> {
    let positive = text.strip_prefix(b"-").unwrap_or(text);
    let value = magnitude::<f64>(positive);
    let value = if text.starts_with(b"-") {
        -value
    } else {
        value
    };
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn r63_numeric_host_rounding_independence() {
        unsafe extern "C" {
            fn fegetround() -> i32;
            fn fesetround(mode: i32) -> i32;
        }
        struct Restore(i32);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    fesetround(self.0);
                }
            }
        }
        let _restore = Restore(unsafe { fegetround() });
        #[cfg(target_arch = "x86_64")]
        let modes = [0, 0x400, 0x800, 0xc00];
        #[cfg(target_arch = "aarch64")]
        let modes = [0, 0x400000, 0x800000, 0xc00000];
        for mode in modes {
            assert_eq!(unsafe { fesetround(mode) }, 0);
            crate::r63_tests::r63_numeric_vectors();
            assert_eq!(
                unsafe { fegetround() },
                mode,
                "conversion does not mutate caller state"
            );
        }
    }

    #[test]
    fn r63_numeric_long_significands_and_exponents() {
        for text in [
            format!("0.{}1e10001", "0".repeat(10000)),
            format!("1{}e-10000", "0".repeat(10000)),
            format!("0.3{}", "0".repeat(10000)),
            format!("1.000000059604644775390625{}1", "0".repeat(10000)),
            "1e999999999999999999999999999999999999999999999999999".into(),
            "-1e-999999999999999999999999999999999999999999999999999".into(),
            "0e999999999999999999999999999999999999999999999999999".into(),
        ] {
            assert_eq!(
                super::f32(text.as_bytes()).map(f32::to_bits),
                text.parse::<f32>()
                    .ok()
                    .filter(|x| x.is_finite())
                    .map(f32::to_bits)
            );
            assert_eq!(
                super::f64(text.as_bytes()).map(f64::to_bits),
                text.parse::<f64>()
                    .ok()
                    .filter(|x| x.is_finite())
                    .map(f64::to_bits)
            );
        }
    }
}
