//! Independent generic-format oracle for the fixed decimal stack writer.
use super::*;

fn oracle(ns: i64, kind: i32) -> Option<String> {
    let precision = resolution(kind)?;
    let rounded = i128::from(ns).div_euclid(precision) * precision;
    i64::try_from(rounded).ok()?;
    let days = i64::try_from(rounded.div_euclid(DAY)).ok()?;
    // Calendar conversion is unchanged and has its own exhaustive sequential oracle.
    let (year, month, day) = civil_date(days)?;
    let within = rounded.rem_euclid(DAY);
    let hour = within / (3600 * SECOND);
    let minute = (within / (60 * SECOND)) % 60;
    let second = (within / SECOND) % 60;
    let fraction = within % SECOND;
    Some(match kind {
        0 => {
            let suffix = if fraction == 0 {
                String::new()
            } else {
                std::format!(".{fraction:09}")
                    .trim_end_matches('0')
                    .to_owned()
            };
            std::format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{suffix}Z")
        }
        1 => std::format!(
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
            fraction / 1_000_000
        ),
        2 => {
            let weekday = WEEKDAYS[usize::try_from((days + 4).rem_euclid(7)).unwrap()];
            let month = MONTHS[usize::try_from(month - 1).unwrap()];
            std::format!(
                "{weekday}, {day:02} {month} {year:04} {hour:02}:{minute:02}:{second:02} GMT"
            )
        }
        3 => std::format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z"),
        4 => std::format!("{year:04}{month:02}{day:02}"),
        _ => return None,
    })
}

fn assert_format(ns: i64, kind: i32) {
    let expected = oracle(ns, kind);
    let actual = format(ns, kind);
    assert_eq!(
        actual.as_ref().map(|text| &text.bytes[..text.len]),
        expected.as_ref().map(|text| text.as_bytes()),
        "ns={ns}, kind={kind}"
    );
}

#[test]
fn fixed_fields_match_generic_output_on_every_reachable_date() {
    let first = i128::from(i64::MIN).div_euclid(DAY);
    let last = i128::from(i64::MAX).div_euclid(DAY);
    for day in first..=last {
        // Midday is representable even on both endpoint dates, and exercises all fields.
        let ns = i64::try_from(day * DAY + 45_296 * SECOND + 123_456_789).unwrap();
        for kind in 0..5 {
            assert_format(ns, kind);
        }
    }
}

#[test]
fn fixed_fields_preserve_endpoints_fraction_widths_and_seeded_instants() {
    for ns in [i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX - 1, i64::MAX] {
        for kind in -1..=5 {
            assert_format(ns, kind);
        }
    }
    for base in [-951_782_400_000_000_000, 0, 951_782_400_000_000_000] {
        for power in 0..=9 {
            let scale = 10_i64.pow(power);
            let fraction = 123_456_789 / scale * scale;
            for kind in 0..5 {
                assert_format(base + fraction, kind);
            }
        }
    }
    let mut seed = 0x9213c724a261534du64;
    for _ in 0..4096 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let ns = i64::from_le_bytes(seed.to_le_bytes());
        for kind in 0..5 {
            assert_format(ns, kind);
        }
    }
}

fn assert_decimal<const WIDTH: usize>(value: i128) {
    let mut out = Text {
        bytes: [0; 32],
        len: 0,
    };
    assert_eq!(out.digits::<WIDTH>(value), Some(()));
    let expected = std::format!("{value:0width$}", width = WIDTH);
    assert_eq!(&out.bytes[..out.len], expected.as_bytes());
}

#[test]
fn fixed_decimal_widths_and_stack_bounds_are_checked() {
    for value in 0..100 {
        assert_decimal::<2>(value);
    }
    for value in 0..1000 {
        assert_decimal::<3>(value);
    }
    for value in 0..10_000 {
        assert_decimal::<4>(value);
    }
    for value in [0, 1, 10, 100, 999_999, 10_000_000, 100_000_000, 999_999_999] {
        assert_decimal::<9>(value);
    }
    let mut out = Text {
        bytes: [0; 32],
        len: 0,
    };
    out.push(b"prefix").unwrap();
    assert_eq!(out.digits::<2>(100), None);
    assert_eq!(out.digits::<3>(1000), None);
    assert_eq!(out.digits::<4>(10_000), None);
    assert_eq!(out.digits::<9>(1_000_000_000), None);
    assert_eq!(out.digits::<2>(-1), None);
    assert_eq!(out.digits::<9>(i128::from(u32::MAX) + 1), None);
    assert_eq!(&out.bytes[..out.len], b"prefix");
    out.push(&[b'x'; 25]).unwrap();
    assert_eq!(out.digits::<2>(1), None);
    assert_eq!(out.push(b"ab"), None);
    assert_eq!(out.len, 31);
    out.digits::<1>(9).unwrap();
    assert_eq!(out.len, 32);
    assert_eq!(out.bytes[31], b'9');
    assert_eq!(out.digits::<1>(0), None);
}

#[cfg(feature = "alloc-count")]
#[test]
fn fixed_field_rendering_keeps_private_text_on_the_stack() {
    let before = crate::global_alloc_count();
    let witness = std::hint::black_box(vec![1u8; 4096]);
    assert!(
        crate::global_alloc_count() > before,
        "counter must observe allocations"
    );
    drop(witness);
    for ns in [i64::MIN, -1, 0, 1, 951_782_400_123_456_789, i64::MAX] {
        for kind in -1..=5 {
            let before = crate::global_alloc_count();
            let out = std::hint::black_box(format(std::hint::black_box(ns), kind));
            let allocations = crate::global_alloc_count() - before;
            assert_eq!(allocations, 0);
            if let Some(out) = out {
                assert!((8..=30).contains(&out.len));
            }
        }
    }
}
