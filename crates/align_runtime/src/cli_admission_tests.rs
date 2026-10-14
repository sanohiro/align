//! Plan181: strict finite numeric flags and explicit option occurrences.
use super::*;

struct Command(*mut CliCommand);
impl Command {
    fn new() -> Self {
        Self(unsafe { align_rt_cli_command_new(b"test".as_ptr(), 4) })
    }
    fn float(&self, name: &str, value: f64) {
        unsafe { align_rt_cli_flag_f64(self.0, name.as_ptr(), len(name.as_bytes()), value) };
    }
    fn parse(&self, tokens: &[&[u8]]) -> Result<Parsed, i32> {
        let args: Vec<_> = tokens
            .iter()
            .map(|token| AlignStr {
                ptr: token.as_ptr(),
                len: len(token),
            })
            .collect();
        let mut out = core::ptr::null_mut();
        let status = unsafe {
            align_rt_cli_parse(
                self.0,
                args.as_ptr(),
                i64::try_from(args.len()).unwrap(),
                &mut out,
            )
        };
        if status == 0 {
            assert!(!out.is_null());
            Ok(Parsed(out))
        } else {
            assert!(out.is_null());
            Err(status)
        }
    }
}
impl Drop for Command {
    fn drop(&mut self) {
        unsafe { align_rt_cli_command_free(self.0) };
    }
}
struct Parsed(*mut CliParsed);
impl Parsed {
    fn float(&self, name: &str) -> f64 {
        unsafe { align_rt_cli_get_f64(self.0, name.as_ptr(), len(name.as_bytes())) }
    }
    fn count(&self, name: &str) -> i64 {
        unsafe { align_rt_cli_occurrences(self.0, name.as_ptr(), len(name.as_bytes())) }
    }
}
impl Drop for Parsed {
    fn drop(&mut self) {
        unsafe { align_rt_cli_parsed_free(self.0) };
    }
}
fn len(value: &[u8]) -> i64 {
    i64::try_from(value.len()).unwrap()
}

#[test]
fn cli_finite_number_grammar_and_exact_bits() {
    let command = Command::new();
    command.float("scale", -0.0);
    assert_eq!(
        command.parse(&[b"prog"]).unwrap().float("scale").to_bits(),
        (-0.0_f64).to_bits()
    );
    for (token, expected) in [
        ("0", 0.0_f64),
        ("-0", -0.0),
        ("0.5", 0.5),
        ("-2E+2", -200.0),
        ("1e-2", 0.01),
        ("1.7976931348623157e308", f64::MAX),
        ("2.2250738585072014e-308", f64::MIN_POSITIVE),
        ("5e-324", f64::from_bits(1)),
        ("-1e-9999", -0.0),
        (
            "1.00000000000000011102230246251565404236316680908203125",
            1.0,
        ),
    ] {
        let equals = format!("--scale={token}");
        for args in [
            vec![&b"prog"[..], &b"--scale"[..], token.as_bytes()],
            vec![&b"prog"[..], equals.as_bytes()],
        ] {
            let parsed = command.parse(&args).unwrap();
            assert_eq!(
                parsed.float("scale").to_bits(),
                expected.to_bits(),
                "{token}"
            );
            assert_eq!(parsed.count("scale"), 1);
        }
    }
    for token in [
        "",
        "+1",
        "01",
        "-01",
        ".5",
        "1.",
        "1e",
        "1e+",
        "--scale",
        " 1",
        "1 ",
        "1\n",
        "1\0",
        "1x",
        "0x1",
        "1_0",
        "NaN",
        "nan",
        "inf",
        "Infinity",
        "-Infinity",
        "1e9999",
        "-1e9999",
    ] {
        let equals = format!("--scale={token}");
        for args in [
            vec![&b"prog"[..], &b"--scale"[..], token.as_bytes()],
            vec![&b"prog"[..], equals.as_bytes()],
        ] {
            assert!(matches!(command.parse(&args), Err(AL_INVALID)), "{token:?}");
        }
    }
    assert!(matches!(
        command.parse(&[b"prog", b"--scale", b"\xff"]),
        Err(AL_INVALID)
    ));
    assert!(matches!(
        command.parse(&[b"prog", b"--scale"]),
        Err(AL_INVALID)
    ));
    assert_eq!(
        command
            .parse(&[b"prog", b"--scale=0.25"])
            .unwrap()
            .float("scale"),
        0.25
    );
}

#[test]
fn cli_nonfinite_defaults_reject_before_argv_even_when_hidden() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for duplicate in [false, true] {
            let command = Command::new();
            if duplicate {
                command.float("scale", 1.0);
            }
            command.float("scale", value);
            for args in [
                vec![&b"prog"[..]],
                vec![&b"prog"[..], &b"--scale=0.5"[..]],
                vec![&b"prog"[..], &b"--unknown"[..]],
            ] {
                assert!(matches!(command.parse(&args), Err(AL_INVALID)));
            }
            // No argv access can precede default rejection. Null is permitted for an empty span;
            // a negative count must not change the earlier Invalid classification to legacy status 1.
            let mut out = core::ptr::null_mut();
            assert_eq!(
                unsafe { align_rt_cli_parse(command.0, core::ptr::null(), -1, &mut out) },
                AL_INVALID
            );
            assert!(out.is_null());
        }
    }
}

#[test]
fn cli_occurrences_follow_token_consumption_and_reset() {
    let command = Command::new();
    unsafe {
        align_rt_cli_flag_bool(command.0, b"bool".as_ptr(), 4);
        align_rt_cli_flag_str(command.0, b"text".as_ptr(), 4, b"default".as_ptr(), 7);
        align_rt_cli_flag_i64(command.0, b"int".as_ptr(), 3, 4);
    }
    command.float("float", 1.0);
    // Duplicate registration preserves the first kind/default and one name's count.
    command.float("text", 2.0);
    let absent = command.parse(&[b"prog"]).unwrap();
    for name in ["bool", "text", "int", "float"] {
        assert_eq!(absent.count(name), 0);
    }
    let parsed = command
        .parse(&[
            b"prog",
            b"--bool",
            b"--text",
            b"--bool",
            b"--int=2",
            b"--float=0.5",
            b"--text=",
            b"--float",
            b"1.5",
            b"--bool",
            b"--int",
            b"9",
        ])
        .unwrap();
    for name in ["bool", "text", "int", "float"] {
        assert_eq!(parsed.count(name), 2, "{name}");
    }
    assert_eq!(parsed.float("float"), 1.5);
    assert_eq!(
        unsafe { align_rt_cli_get_i64(parsed.0, b"int".as_ptr(), 3) },
        9
    );
    assert_eq!(
        unsafe { align_rt_cli_get_str(parsed.0, b"text".as_ptr(), 4) }.len,
        0
    );
    for _ in 0..8 {
        assert!(matches!(
            command.parse(&[b"prog", b"--text=allocated", b"--float=NaN"]),
            Err(AL_INVALID)
        ));
        assert_eq!(command.parse(&[b"prog"]).unwrap().count("text"), 0);
    }
    drop(command);
    assert_eq!(parsed.float("float"), 1.5);
    assert_eq!(parsed.count("float"), 2);
    assert_eq!(absent.float("float"), 1.0);
}

#[test]
fn cli_count_overflow_keeps_existing_entry() {
    let mut entries = vec![CliEntry {
        name: "flag".into(),
        value: CliValue::I64(9),
        occurrences: i64::MAX - 1,
    }];
    assert_eq!(
        cli_set_value(&mut entries, "flag", CliValue::I64(10)),
        Some(())
    );
    assert_eq!(entries[0].occurrences, i64::MAX);
    assert_eq!(cli_set_value(&mut entries, "flag", CliValue::I64(11)), None);
    assert_eq!(entries[0].occurrences, i64::MAX);
    assert!(matches!(entries[0].value, CliValue::I64(10)));
}
