//! Shared recursive-Drop codegen owner (`docs/impl/21-build-perf-plan.md`, item 3a).
//!
//! A deep finite by-value graph must compile and execute without turning nominal type depth into
//! generated-program call-stack depth. The one root helper expands the existing iterative Drop CFG
//! and therefore never calls another compiler-generated destructor.

mod common;
use common::*;

use align_mir::{Block, Const, Function, Operand, Program, ProgramCall, Stmt, Term};
use align_sema::{FieldDef, IntTy, StructDef, Ty};
use std::time::{Duration, Instant};

struct TempArtifacts([std::path::PathBuf; 2]);

impl Drop for TempArtifacts {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn run_bounded(executable: &std::path::Path) -> std::process::ExitStatus {
    let mut child = std::process::Command::new(executable)
        .spawn()
        .expect("spawn deep Drop executable");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().expect("poll deep Drop executable") {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("deep Drop executable exceeded its 10-second deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn deep_drop_program(depth: usize) -> Program {
    assert!(depth > 0);
    let mut structs = Vec::with_capacity(depth);
    structs.push(StructDef {
        name: "Deep0".to_owned(),
        source_name: "Deep0".to_owned(),
        fields: vec![FieldDef {
            name: "text".to_owned(),
            ty: Ty::String,
        }],
        align: None,
        c_repr: false,
    });
    for index in 1..depth {
        structs.push(StructDef {
            name: format!("Deep{index}"),
            source_name: format!("Deep{index}"),
            fields: vec![FieldDef {
                name: "next".to_owned(),
                ty: Ty::Struct((index - 1) as u32),
            }],
            align: None,
            c_repr: false,
        });
    }

    let i32_ty = Ty::Int(IntTy {
        bits: 32,
        signed: true,
    });
    Program {
        fns: vec![Function {
            name: ProgramCall::try_from_logical("main").expect("valid program call"),
            params: vec![],
            param_modes: vec![],
            borrow_mut_cleanup_slots: vec![],
            ret: i32_ty,
            return_borrow: align_sema::hir::ReturnBorrowSummary::None,
            return_region: align_sema::hir::ReturnRegionSummary::None,
            return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
            slots: vec![Ty::Struct((depth - 1) as u32)],
            slot_align: vec![None],
            value_tys: vec![],
            blocks: vec![Block {
                id: 0,
                stmts: vec![Stmt::DropFlagInit(0), Stmt::Drop(0)],
                stmt_lines: vec![(0, 0), (0, 0)],
                term: Term::Return(Some(Operand::Const(Const::Int(0, i32_ty)))),
            }],
            entry: 0,
            exportable: false,
        }],
        structs,
        ..Program::default()
    }
}

#[test]
fn deep_finite_drop_graph_executes_with_one_helper_frame() {
    if !backend_available() {
        return;
    }

    const DEPTH: usize = 4_096;
    let program = deep_drop_program(DEPTH);
    let ir = emit_llvm_ir(&program, BuildTarget::Baseline, false, &[], false)
        .expect("deep Drop graph must emit raw LLVM");
    let helper_name = format!("__align_drop_struct${}", DEPTH - 1);
    assert_eq!(
        ir.lines()
            .filter(|line| line.starts_with("define private") && line.contains(&helper_name))
            .count(),
        1,
        "the root Drop helper must be defined exactly once"
    );
    assert_eq!(
        ir.lines()
            .filter(|line| line.starts_with("define private") && line.contains("__align_drop_struct$"))
            .count(),
        1,
        "nested records stay in the root helper's iterative CFG"
    );
    assert_eq!(
        ir.lines()
            .filter(|line| line.contains("call void") && line.contains(&helper_name))
            .count(),
        1,
        "main must execute the root Drop helper exactly once"
    );
    let helper_start = ir
        .match_indices("define private")
        .find_map(|(start, _)| {
            let header_end = ir[start..].find('\n').map_or(ir.len(), |end| start + end);
            ir[start..header_end].contains(&helper_name).then_some(start)
        })
        .expect("root helper definition");
    let helper_body = ir[helper_start..]
        .split_once("{\n")
        .and_then(|(_, body)| body.split("\n}").next())
        .expect("root helper body");
    assert!(
        !helper_body.contains("__align_drop_struct$"),
        "a generated helper must not call another generated helper"
    );

    let stem = std::env::temp_dir().join(format!(
        "align-deep-drop-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("worker")
    ));
    let object = stem.with_extension("o");
    let executable = stem.with_extension(std::env::consts::EXE_EXTENSION);
    let _artifacts = TempArtifacts([object.clone(), executable.clone()]);
    emit_object_file(
        &program,
        &object,
        BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
    )
    .expect("deep Drop object emission");
    link_objects(&[object.as_path()], &executable, &[], Profile::Release)
        .expect("deep Drop executable link");
    let status = run_bounded(&executable);
    assert_eq!(status.code(), Some(0), "deep Drop executable must finish normally");
}
