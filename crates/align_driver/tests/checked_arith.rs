//! Explicit-overflow integer arithmetic (`core.math`): `wrapping_*` / `saturating_*` / `checked_*`
//! for `add`/`sub`/`mul`. `wrapping_*` is the default two's-complement wrap; `saturating_*` clamps
//! to the type's MIN/MAX; `checked_*` yields `Option<T>` (`None` on overflow). Lowered to LLVM
//! `{s,u}OP.sat` / `{s,u}OP.with.overflow` intrinsics.

use align_driver::{backend_available, check, emit_object_file, link_executable, lower_to_mir};
use align_span::SourceMap;

fn build_and_run(name: &str, src: &str) -> std::process::Output {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let obj = dir.join(format!("align-test-{name}.o"));
    let exe = dir.join(format!("align-test-{name}"));
    emit_object_file(&mir, &obj).expect("codegen");
    link_executable(&obj, &exe).expect("link");
    let out = std::process::Command::new(&exe).output().expect("run");
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&exe);
    out
}

fn check_errs(name: &str, src: &str) -> bool {
    let mut sm = SourceMap::new();
    check(&mut sm, name, src).diags.has_errors()
}

#[test]
fn wrapping_wraps() {
    if !backend_available() {
        return;
    }
    // i8: 100 + 100 wraps to -56. (wrapping_* = the default arithmetic, made explicit.)
    let src = "fn main() -> Result<(), Error> {\n  a: i8 := 100\n  b: i8 := 100\n  print(a.wrapping_add(b))\n  return Ok(())\n}\n";
    let out = build_and_run("ca-wrap", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-56\n");
}

#[test]
fn saturating_clamps_signed_and_unsigned() {
    if !backend_available() {
        return;
    }
    // i8: 100+100 → 127 (MAX); u8: 200+100 → 255 (MAX); i8: -100-100 → -128 (MIN).
    let src = "fn main() -> Result<(), Error> {\n  a: i8 := 100\n  print(a.saturating_add(100))\n  u: u8 := 200\n  print(u.saturating_add(100))\n  n: i8 := -100\n  print(n.saturating_sub(100))\n  return Ok(())\n}\n";
    let out = build_and_run("ca-sat", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "127\n255\n-128\n");
}

#[test]
fn checked_yields_option() {
    if !backend_available() {
        return;
    }
    // Overflow → None (else -1); in range → Some(value); mul in range → Some(200).
    let src = "fn main() -> Result<(), Error> {\n  a: i32 := 2000000000\n  print(a.checked_add(a) else { -1 })\n  print(a.checked_add(5) else { -99 })\n  print((10).checked_mul(20) else { -1 })\n  return Ok(())\n}\n";
    let out = build_and_run("ca-checked", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-1\n2000000005\n200\n");
}

#[test]
fn checked_mul_unsigned_overflow() {
    if !backend_available() {
        return;
    }
    // u8: 200 * 2 = 400 overflows → None (else 0); 10*10 = 100 → Some(100).
    let src = "fn main() -> Result<(), Error> {\n  a: u8 := 200\n  print(a.checked_mul(2) else { 0 })\n  b: u8 := 10\n  print(b.checked_mul(10) else { 0 })\n  return Ok(())\n}\n";
    let out = build_and_run("ca-umul", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n100\n");
}

#[test]
fn non_integer_receiver_rejected() {
    // These ops are integer-only — a float receiver is rejected.
    assert!(check_errs("ca-float", "fn main() -> i32 {\n  x: f64 := 1.0\n  return x.checked_add(2.0) else { 0 }\n}\n"));
}
