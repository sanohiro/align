//! Codex audit item 9 — the `emit-llvm | llvm-as-22` textual round-trip gate.
//!
//! Background: before item 9, the runtime declarations' no-capture contract was emitted as the
//! removed `nocapture` attribute name (kind id 0 on LLVM 22), which inkwell's LLVM-22 printer
//! rendered as the bare shorthand `ptr none` — text that `llvm-as-22` CANNOT re-parse
//! (`error: expected ')' at end of argument list`). That broke the textual dev path
//! (`alignc emit-llvm | llvm-as-22`). Emitting the modern `captures(none)` attribute directly (the
//! `captures` kind id + value 0) makes the printer emit the canonical `captures(none)` spelling,
//! which `llvm-as-22` accepts — restoring the round-trip.
//!
//! This test proves the fix end to end: it feeds `emit_llvm` output to `llvm-as` and asserts it
//! assembles. Gating: LLVM backend + a discoverable version-matched `llvm-as` (`align_driver`'s
//! `llvm_tool`, same discovery as `alignc size`). Where either is absent the test skips cleanly.

mod common;
use common::*;

use std::io::Write;
use std::process::Command;

/// A program that exercises the pointer-carrying runtime readers (`hash64`, string compare) so the
/// emitted module actually contains the `captures(none)` param attribute the round-trip used to
/// choke on — plus ordinary bodies, allocation, and the full runtime declare set (every builtin is
/// declared unconditionally, so even a trivial program emits them, but a real one is a stronger
/// smoke test of the whole module).
const PROGRAM: &str = "\
fn key(s: str) -> i64 = hash64(s) as i64\n\
fn main() -> i32 {\n  \
  a := [\"align\", \"lang\", \"align\"]\n  \
  n := a.map(key).sum()\n  \
  if a[0] == a[2] { return (n % 128) as i32 }\n  \
  return 0\n\
}\n";

#[test]
fn emitted_ir_round_trips_through_llvm_as() {
    if !backend_available() {
        return;
    }
    let Some(llvm_as) = align_driver::llvm_tool("llvm-as") else {
        return; // no version-matched llvm-as on this machine — skip, not a failure.
    };

    let ir = emit_llvm(PROGRAM);
    // Sanity: the module must actually contain the modern attribute we are round-tripping (guards
    // against a future change that silently drops it and makes this test vacuously pass).
    assert!(
        ir.contains("captures(none)"),
        "emitted IR must carry the modern captures(none) attribute:\n{ir}"
    );
    assert!(
        !ir.contains("ptr none"),
        "emitted IR must NOT carry the un-reparseable `ptr none` shorthand:\n{ir}"
    );

    // Feed the IR to `llvm-as` on stdin, capture the bitcode on stdout (`-o -`). Success (exit 0)
    // proves the whole module re-parses; a regression to `ptr none` would make llvm-as reject it.
    let mut child = Command::new(&llvm_as)
        .arg("-o")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn llvm-as");
    child
        .stdin
        .take()
        .expect("llvm-as stdin")
        .write_all(ir.as_bytes())
        .expect("write IR to llvm-as");
    let out = child.wait_with_output().expect("await llvm-as");
    assert!(
        out.status.success(),
        "llvm-as rejected the emitted IR (the textual round-trip is broken).\nstderr:\n{}\nIR:\n{ir}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.stdout.is_empty(), "llvm-as produced empty bitcode");
}
