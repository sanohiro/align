//! Complete CLI help bytes and allocation-free temporary rendering.

use super::*;
use core::mem::MaybeUninit;

struct Command(*mut CliCommand);
impl Command {
    fn new(name: &[u8]) -> Self {
        Self(unsafe { align_rt_cli_command_new(name.as_ptr(), length(name)) })
    }

    fn bool(&self, name: &[u8]) {
        unsafe { align_rt_cli_flag_bool(self.0, name.as_ptr(), length(name)) };
    }

    fn string(&self, name: &[u8], value: &[u8]) {
        unsafe {
            align_rt_cli_flag_str(
                self.0,
                name.as_ptr(),
                length(name),
                value.as_ptr(),
                length(value),
            )
        };
    }

    fn integer(&self, name: &[u8], value: i64) {
        unsafe { align_rt_cli_flag_i64(self.0, name.as_ptr(), length(name), value) };
    }

    fn usage(&self) -> Usage {
        Usage(unsafe { align_rt_cli_usage(self.0) })
    }
}
impl Drop for Command {
    fn drop(&mut self) {
        unsafe { align_rt_cli_command_free(self.0) };
    }
}

struct Usage(AlignStr);
impl Usage {
    fn bytes(&self) -> &[u8] {
        unsafe { bytes_view(self.0.ptr, self.0.len) }
    }
}
impl Drop for Usage {
    fn drop(&mut self) {
        unsafe { align_rt_free(self.0.ptr.cast_mut()) };
    }
}

fn length(bytes: &[u8]) -> i64 {
    i64::try_from(bytes.len()).unwrap()
}

#[test]
fn complete_usage_preserves_native_text_and_independent_ownership() {
    let empty = Command::new(b"");
    assert_eq!(empty.usage().bytes(), b"usage:  [flags]\n");
    let null = Usage(unsafe { align_rt_cli_usage(core::ptr::null()) });
    assert!(null.0.ptr.is_null() && null.0.len == 0);

    let command = Command::new(b"tool\0\n\xff");
    command.bool(b"");
    command.bool(b"same");
    command.string(b"same", b"\xff\xc0\0\n");
    command.string("名前".as_bytes(), "値🦀".as_bytes());
    command.string(b"empty", b"");
    command.integer(b"low", i64::MIN);
    command.integer(b"high", i64::MAX);
    let first = command.usage();
    let second = command.usage();
    command.bool(b"later");
    drop(command);
    let expected = concat!(
        "usage: tool\0\n\u{fffd} [flags]\n",
        "  --  (bool)\n",
        "  --same  (bool)\n",
        "  --same  (str, default: \u{fffd}\u{fffd}\0\n)\n",
        "  --名前  (str, default: 値🦀)\n",
        "  --empty  (str, default: )\n",
        "  --low  (i64, default: -9223372036854775808)\n",
        "  --high  (i64, default: 9223372036854775807)\n",
    );
    assert_eq!(first.bytes(), expected.as_bytes());
    assert_eq!(second.bytes(), expected.as_bytes());
    assert_ne!(first.0.ptr, second.0.ptr);
}

#[test]
fn usage_decimal_boundaries_and_long_defaults_are_complete() {
    let command = Command::new(b"numbers");
    let mut values = vec![i64::MIN, i64::MAX, 0];
    for exponent in 0..=18 {
        let power = 10_i64.pow(exponent);
        for delta in [-1, 0, 1] {
            values.push(power + delta);
            values.push(-(power + delta));
        }
    }
    let mut expected = String::from("usage: numbers [flags]\n");
    for value in values {
        command.integer(b"n", value);
        expected.push_str(&format!("  --n  (i64, default: {value})\n"));
    }
    let long = "日本語\0\n".repeat(4096);
    command.string(b"long", long.as_bytes());
    expected.push_str(&format!("  --long  (str, default: {long})\n"));
    assert_eq!(command.usage().bytes(), expected.as_bytes());
}

#[test]
fn usage_extent_admission_and_fill_refuse_incomplete_publication() {
    let maximum = usize::try_from(isize::MAX).unwrap();
    for (total, part, expected) in [
        (0, 0, Some(0)),
        (maximum - 1, 1, Some(maximum)),
        (maximum, 0, Some(maximum)),
        (maximum, 1, None),
        (usize::MAX, 1, None),
        (1, usize::MAX, None),
    ] {
        assert_eq!(cli_usage_add_len(total, part), expected);
    }
    let command = Command::new(b"x");
    command.integer(b"n", i64::MIN);
    let expected = b"usage: x [flags]\n  --n  (i64, default: -9223372036854775808)\n";
    for capacity in [0, 1, expected.len() - 1, expected.len(), expected.len() + 1] {
        let mut storage = vec![MaybeUninit::new(0xa5); capacity + 2];
        let result = cli_usage_fill(unsafe { &*command.0 }, &mut storage[1..capacity + 1]);
        assert_eq!(result.is_ok(), capacity == expected.len());
        // Every element starts initialized; failed fills may leave the original sentinel.
        let bytes: Vec<u8> = storage
            .into_iter()
            .map(|b| unsafe { b.assume_init() })
            .collect();
        assert_eq!(bytes[0], 0xa5);
        assert_eq!(bytes[capacity + 1], 0xa5);
        if result.is_ok() {
            assert_eq!(&bytes[1..capacity + 1], expected);
        }
    }
}

#[cfg(feature = "alloc-count")]
#[test]
fn usage_allocates_no_rust_temporary_storage() {
    // Initialize only the alloc-count feature's requested-live probe. Its mutex may allocate
    // lazily on macOS; no CLI rendering is warmed before the complete-entry measurement.
    let probe_witness = align_rt_alloc(1);
    unsafe { align_rt_free(probe_witness) };
    let before = global_alloc_count();
    let witness = std::hint::black_box(vec![std::hint::black_box(7_u8); 32]);
    assert!(global_alloc_count() > before, "allocator counter is active");
    drop(witness);
    for n in [0, 4, 64, 1024] {
        let command = Command::new(b"tool");
        for i in 0..n {
            let name = format!("flag-{i}");
            command.integer(name.as_bytes(), i64::MIN + i);
        }
        for long in [false, true] {
            if long {
                command.string(b"long", "文🦀".repeat(4096).as_bytes());
            }
            let before = global_alloc_count();
            let output = command.usage();
            let allocations = global_alloc_count() - before;
            assert_eq!(allocations, 0, "n={n}, long={long}");
            assert!(output.bytes().starts_with(b"usage: tool [flags]\n"));
        }
    }
}
