//! Plan257: execute the emitted guard's predicate over boundary values without accessing memory.
mod common;
use align_ast::BinOp;
use align_mir::{Const, DirectCall, Function, Operand, Rvalue, Stmt, Term};
use align_sema::{IntTy, Ty};
use common::*;
use std::collections::BTreeMap;

#[cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

fn signed(bits: u64) -> i64 {
    i64::from_ne_bytes(bits.to_ne_bytes())
}
fn bits(value: i64) -> u64 {
    u64::from_ne_bytes(value.to_ne_bytes())
}

/// Interpret actual lowered MIR, independently of the emitter's choice of predicate.
fn eval(op: &Operand, f: &Function, defs: &BTreeMap<u32, &Rvalue>, offset: i64, len: i64) -> u64 {
    let Operand::Value(id) = op else {
        let Operand::Const(Const::Int(value, _)) = op else {
            panic!("unexpected operand {op:?}")
        };
        return bits(i64::try_from(*value).unwrap());
    };
    match defs.get(id).copied().unwrap() {
        Rvalue::SliceLen(_) => bits(len),
        Rvalue::Load(slot) if f.params.get(1) == Some(slot) => bits(offset),
        Rvalue::Use(value) | Rvalue::Cast { operand: value, .. } => {
            eval(value, f, defs, offset, len)
        }
        Rvalue::Bin(kind, left, right) => {
            let a = eval(left, f, defs, offset, len);
            let b = eval(right, f, defs, offset, len);
            let unsigned = match left {
                Operand::Value(id) => matches!(
                    f.value_tys.get(*id as usize),
                    Some(Ty::Int(IntTy { signed: false, .. }))
                ),
                _ => false,
            };
            match kind {
                BinOp::Add => a.wrapping_add(b),
                BinOp::Or => a | b,
                BinOp::Lt => u64::from(if unsigned {
                    a < b
                } else {
                    signed(a) < signed(b)
                }),
                BinOp::Gt => u64::from(if unsigned {
                    a > b
                } else {
                    signed(a) > signed(b)
                }),
                BinOp::Ge => u64::from(if unsigned {
                    a >= b
                } else {
                    signed(a) >= signed(b)
                }),
                _ => panic!("unexpected predicate operation {kind:?}"),
            }
        }
        value => panic!("unexpected guard value {value:?}"),
    }
}

#[test]
fn byte_access_guards_use_unsigned_predicates() {
    for scalar in [
        "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64", "f32", "f64",
    ] {
        let width: i64 = scalar[1..].parse::<i64>().unwrap() / 8;
        for endian in if width == 1 {
            &[""][..]
        } else {
            &["_le", "_be"][..]
        } {
            for write in [false, true] {
                let source = if write {
                    format!(
                        "fn access(borrow mut data: slice<u8>, offset: i64, value: {scalar}) {{ data.set_{scalar}{endian}(offset, value) }}"
                    )
                } else {
                    format!(
                        "fn access(data: slice<u8>, offset: i64) -> {scalar} = data.{scalar}{endian}(offset)"
                    )
                };
                let mut sm = SourceMap::new();
                let checked = check(&mut sm, "byte-guard-matrix", &source);
                assert!(
                    !checked.diags.has_errors(),
                    "{source}\n{}",
                    align_driver::format_diagnostics(&sm, &checked.diags)
                );
                let program = lower_to_mir(&checked.hir);
                let f = &program.fns[0];
                let defs: BTreeMap<_, _> = f
                    .blocks
                    .iter()
                    .flat_map(|b| &b.stmts)
                    .filter_map(|s| {
                        if let Stmt::Let(id, rv) = s {
                            Some((*id, rv))
                        } else {
                            None
                        }
                    })
                    .collect();
                let guards: Vec<_> = f
                    .blocks
                    .iter()
                    .filter_map(|block| {
                        let Term::Branch(condition, fail, _) = &block.term else {
                            return None;
                        };
                        let [
                            Stmt::Let(
                                _,
                                Rvalue::Call(
                                    DirectCall::Runtime(align_mir::RuntimeKey::RangeFail),
                                    args,
                                ),
                            ),
                        ] = f.blocks[*fail as usize].stmts.as_slice()
                        else {
                            return None;
                        };
                        Some((condition, args))
                    })
                    .collect();
                assert_eq!(guards.len(), 1, "{source}");
                let (condition, args) = guards[0];
                assert_eq!(
                    defs.values()
                        .filter(|rv| matches!(rv, Rvalue::Bin(BinOp::Lt, ..)))
                        .count(),
                    0,
                    "signed guard survived: {source}"
                );
                assert_eq!(
                    defs.values()
                        .filter(|rv| matches!(rv, Rvalue::Bin(BinOp::Ge | BinOp::Gt, ..)))
                        .count(),
                    if width == 1 { 1 } else { 2 },
                    "{source}"
                );
                for len in [0, width - 1, width, width + 1, 19, i64::MAX - 1, i64::MAX] {
                    for offset in [
                        i64::MIN,
                        -width,
                        -1,
                        0,
                        1,
                        len.saturating_sub(width),
                        len - 1,
                        len,
                        i64::MAX - width,
                        i64::MAX,
                    ] {
                        let end = offset.wrapping_add(width);
                        let invalid = offset < 0 || offset > end || end > len;
                        assert_eq!(
                            eval(condition, f, &defs, offset, len) != 0,
                            invalid,
                            "{source}: {offset}..{end}/{len}"
                        );
                        assert_eq!(args.len(), 3);
                        assert_eq!(signed(eval(&args[0], f, &defs, offset, len)), offset);
                        assert_eq!(signed(eval(&args[1], f, &defs, offset, len)), end);
                        assert_eq!(signed(eval(&args[2], f, &defs, offset, len)), len);
                    }
                }
            }
        }
    }
}

#[test]
#[cfg(unix)]
fn byte_access_native_imports_preserve_values_and_traps() {
    owned_fixture::run(
        "byte_access_native_imports_preserve_values_and_traps",
        |_| {
            if !backend_available() {
                return;
            }
            let provider = "module probe\npub fn read(data: slice<u8>, at: i64) -> u8 = data.u8(at)\npub fn edit(borrow mut data: slice<u8>, at: i64, value: u32) { data.set_u32_be(at, value) }\npub fn word(data: slice<u8>, at: i64) -> u32 = data.u32_be(at)\n";
            let main = "module main\nimport probe\nfn main() -> i32 { mut data := [0 as u8, 0 as u8, 0 as u8, 0 as u8, 0 as u8]; mut view := data[..]; probe.edit(view, 1, 16909060); if probe.word(data[..], 1) != 16909060 { return 1 }; if probe.read(data[..], 4) != 4 { return 2 }; return 0 }\n";
            let files = [("probe.align", provider), ("main.align", main)];
            let whole = build_and_run_multi("byte-guard-whole", &files, "main.align");
            assert_eq!(
                whole.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&whole.stderr)
            );
            let units = build_per_unit_multi("byte-guard-units", &files, "main.align");
            let output = units.link_and_run();
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            for (access, width) in [("u8", 1i64), ("u32_le", 4)] {
                for offset in [-1i64, 1, i64::MAX] {
                    let source = format!(
                        "fn main() {{ data := [7 as u8]; print(data[..].{access}({offset})) }}"
                    );
                    let output = build_and_run("byte-guard-trap", &source);
                    assert!(!output.status.success());
                    let expected = format!(
                        "slice range out of bounds: {offset}..{} is not within length 1",
                        offset.wrapping_add(width)
                    );
                    assert!(
                        String::from_utf8_lossy(&output.stderr).contains(&expected),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
        },
    );
}
