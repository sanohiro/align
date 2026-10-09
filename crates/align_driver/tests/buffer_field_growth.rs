//! Stable growth authenticates complete owners across source, HIR, MIR and native execution.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{
    BuildTarget, Profile, backend_available, build_per_unit, check, emit_object_file, link_objects,
    lower_to_mir,
};
use align_span::SourceMap;
use std::path::Path;

#[test]
fn stable_buffer_fields_grow_without_transferring_ownership() {
    owned_fixture::run("stable_buffer_fields_grow_without_transferring_ownership", exercise);
}

fn exercise(stage: &Path) {
    let helper = r#"module helper
pub Holder<T> { bytes: buffer, tag: T }
pub Session { state: Holder<i64>, other: buffer, counts: [i64; 4] }
pub fn make(capacity: i64) -> Result<Session, Error> {
  bytes := buffer.try_new(capacity, 64)?
  return Ok(Session { state: Holder { bytes: bytes, tag: 7 }, other: buffer.filled(2, 7), counts: [0, 0, 0, 0] })
}

pub fn grow(borrow mut holder: Session) {
  holder.state.bytes.append_filled(8, 65 as u8)
  holder.state.bytes.append(holder.state.bytes.bytes())
  holder.state.bytes.put_u16_le(16961 as u16)
}
pub fn keep(owner: Session) -> Session = owner
pub fn fail(fail: bool) -> Result<i64, Error> {
  mut value := make(8)?
  grow(value)
  if fail { return Err(Error.Invalid) }
  return Ok(value.state.bytes.len())
}
pub fn stopped(borrow mut holder: Session, value: bool) {
  if value { holder.state.bytes.append_filled(9, { return; 0 as u8 }) }
  holder.state.bytes.append_filled({ return; 9 }, 0 as u8)
}
"#;
    let source = r#"import helper
Mixed { bytes: buffer, view: slice<u8> }
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
fn exercise() -> i32 {
  mut small := helper.make(8) else { return 1 }
  if unsafe { align_rt_requested_live_bytes() } != 138 { return 11 }
  helper.stopped(small, false)
  helper.stopped(small, true)
  if small.state.bytes.len() != 0 { return 12 }
  helper.grow(small)
  print(small.state.bytes.len())
  print(small.state.bytes.bytes().as_str() else "invalid")
  mut large := helper.make(64) else { return 2 }
  before := unsafe { align_rt_requested_live_bytes() }
  helper.grow(large)
  if unsafe { align_rt_requested_live_bytes() } != before { return 13 }
  mut iteration := 0
  loop {
    if iteration == 2 { break }
    large.state.bytes.append_filled(0, 0 as u8)
    iteration = iteration + 1
  }
  print(large.state.bytes.len())
  print(large.state.bytes.bytes().as_str() else "invalid")
  mut moved := helper.keep(small)
  moved.state.bytes.append_filled(0, 0 as u8)
  print(moved.state.bytes.len())
  moved = helper.make(8) else { return 14 }
  match helper.fail(false) { Ok(n) => { if n != 18 { return 15 } }, Err(_) => { return 16 } }
  if (helper.fail(true).map_err(fn e: Error { e }) else 99) != 99 { return 17 }
  mut mixed := Mixed { bytes: buffer(8), view: "xyz".bytes() }
  independent := mixed.view
  mixed.bytes.append_filled(1, 7 as u8)
  if independent.u8(0) != 120 as u8 || mixed.bytes.bytes().u8(0) != 7 as u8 { return 19 }
  return 0
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  status := exercise()
  if status != 0 { return status }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 18 }
  return 0
}
"#;
    std::fs::write(stage.join("helper.align"), helper).unwrap();
    let entry = stage.join("main.align");
    std::fs::write(&entry, source).unwrap();
    // The shared parent guard bounds backend discovery, compilation, linking and execution.
    assert!(
        backend_available(),
        "self-append owner requires the native backend"
    );
    let context = align_driver::CacheContext::at(stage.join("cache"));
    let mut cached_mir = Vec::new();
    for mode in 0..4 {
        let per_unit = mode != 0;
        let mut sources = SourceMap::new();
        let programs = if mode >= 2 {
            let mut built = align_driver::build_package(&mut sources, entry.to_str().unwrap(), source,
                &context, align_driver::UnitReuse::Allowed);
            assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
            assert_eq!(built.units.len(), 2);
            for unit in &built.units { assert_eq!(unit.frontend.as_ref().unwrap().hit, mode == 3); }
            let programs = (0..built.units.len()).map(|i| built.materialize(i).unwrap().clone()).collect::<Vec<_>>();
            let printed = programs.iter().map(align_mir::print::program_to_string).collect::<Vec<_>>();
            if mode == 2 { cached_mir = printed; } else { assert_eq!(printed, cached_mir); }
            programs
        } else if per_unit {
            let walk = build_per_unit(&mut sources, entry.to_str().unwrap(), source);
            assert!(
                !walk.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &walk.diags)
            );
            assert_eq!(walk.units.len(), 2);
            walk.units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>()
        } else {
            let checked = check(&mut sources, entry.to_str().unwrap(), source);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &checked.diags)
            );
            vec![lower_to_mir(&checked.hir)]
        };
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for (index, program) in programs.iter().enumerate() {
            let object = stage.join(format!("{per_unit}-{index}.o"));
            emit_object_file(
                program,
                &object,
                BuildTarget::Baseline,
                Profile::Release,
                &[],
                false,
            )
            .unwrap();
            objects.push(object);
            for library in &program.link_libs {
                if !libraries.contains(library) {
                    libraries.push(library.clone());
                }
            }
        }
        let executable = stage.join(format!("program-{per_unit}"));
        link_objects(
            &align_driver::CDriver::default(),
            &objects
                .iter()
                .map(|path| path.as_path())
                .collect::<Vec<_>>(),
            &executable,
            &libraries,
            Profile::Release,
        )
        .unwrap();
        let output = std::process::Command::new(executable).output().unwrap();
        assert!(
            output.status.success(),
            "{:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "18\nAAAAAAAAAAAAAAAAAB\n18\nAAAAAAAAAAAAAAAAAB\n18\n"
        );
    }
}

#[test]
fn stable_buffer_growth_rejects_invalid_authority_and_old_views() {
    owned_fixture::run("stable_buffer_growth_rejects_invalid_authority_and_old_views", |stage| {
        let definitions = "Holder { bytes: buffer, other: buffer }\nfn make() -> Holder = Holder { bytes: buffer.filled(8, 1), other: buffer.filled(8, 2) }\nfn consume(value: Holder) -> i64 = 1\nfn replace(borrow mut value: Holder) -> i64 { value = make(); return 1 }\n";
        for operation in ["append_filled(1, 0 as u8)", "append(\"a\")", "put_u16_le(1 as u16)"] {
            for (name, body) in [
                ("shared", format!("fn probe(borrow value: Holder) {{ value.bytes.{operation} }}\nfn main() {{}}")),
                ("borrowed_stale", format!("fn probe(borrow mut value: Holder) {{ old := value.bytes.bytes(); value.bytes.{operation}; print(old.len()) }}\nfn main() {{}}")),
                ("immutable", format!("fn main() {{ value := make(); value.bytes.{operation} }}")),
                ("stale", format!("fn main() {{ mut value := make(); old := value.bytes.bytes(); value.bytes.{operation}; print(old.len()) }}")),
                ("sibling", format!("fn main() {{ mut value := make(); old := value.other.bytes(); value.bytes.{operation}; print(old.len()) }}")),
                ("moved", format!("fn main() {{ mut value := make(); consume(value); value.bytes.{operation} }}")),
            ] {
                assert_refused(stage, &format!("{definitions}{body}\n"), &format!("{operation}/{name}"));
            }
        }
        for body in [
            "fn main() { mut value := make(); value.bytes.append_filled(consume(value), 0 as u8) }",
            "fn main() { mut value := make(); value.bytes.append_filled(1, { consume(value); 0 as u8 }) }",
            "fn main() { mut value := make(); value.bytes.append_filled(replace(value), 0 as u8) }",
            "fn main() { mut value := make(); value.bytes.append_filled(1, { replace(value); 0 as u8 }) }",
            "fn main() { mut value := make(); value.bytes.put_u16_le({ replace(value); 1 as u16 }) }",
            "fn main() { mut value := make(); value.bytes.append({ replace(value); \"a\" }) }",
            "fn grow(borrow mut value: buffer) { value.append_filled(1, 0 as u8) }\nfn main() { mut value := make(); grow(value.bytes) }",
            "fn escape() -> slice<u8> { mut value := make(); value.bytes.append_filled(1, 0 as u8); return value.bytes.bytes() }\nfn main() {}",
            "fn probe(borrow value: Option<buffer>) { match value { Some(bytes) => { bytes.append_filled(1, 0 as u8) }, None => {} } }\nfn main() {}",
        ] {
            assert_refused(stage, &format!("{definitions}{body}\n"), body);
        }
    });
}

fn assert_refused(stage: &Path, source: &str, label: &str) {
    let entry = stage.join("negative.align");
    std::fs::write(&entry, source).unwrap();
    for per_unit in [false, true] {
        let mut sources = SourceMap::new();
        let diagnostics = if per_unit {
            build_per_unit(&mut sources, entry.to_str().unwrap(), source).diags
        } else { check(&mut sources, entry.to_str().unwrap(), source).diags };
        let rendered = align_driver::format_diagnostics(&sources, &diagnostics);
        assert!(diagnostics.has_errors(), "{label}/{per_unit}: expected refusal");
        assert!(!rendered.contains("unknown") && !rendered.contains("expected identifier"), "{label}: {rendered}");
        assert!(rendered.contains("borrow") || rendered.contains("move") || rendered.contains("outlive")
            || rendered.contains("invalidated") || rendered.contains("freed"), "{label}: {rendered}");
    }
}

#[test]
fn stable_buffer_growth_mir_requires_founded_exclusive_write_authority() {
    use align_mir::{Operand, Rvalue, Stmt};
    for operation in ["append_filled(1, 0 as u8)", "append(\"x\")", "put_u16_le(1 as u16)"] {
        let source = format!("Inner {{ bytes: buffer }}\nHolder {{ inner: Inner }}\nfn grow(borrow mut holder: Holder) = holder.inner.bytes.{operation}\nfn main() {{}}\n");
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "growth-mir.align", &source);
        assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &checked.diags));
        let base = lower_to_mir(&checked.hir);
        let verify = |program: &align_mir::Program, valid, mutation| {
            let whole = align_mir::producer::validate_mir_producers(program);
            assert_eq!(whole.is_ok(), valid, "{operation}/{mutation}: {whole:?}");
            let defined = program.fns.iter().map(|f| f.name.clone()).collect();
            let partition = align_mir::producer::validate_partition_resource_rvalues(program, &defined);
            assert_eq!(partition.is_ok(), valid, "{operation}/{mutation}/partition: {partition:?}");
        };
        verify(&base, true, 0);
        for mutation in 1..5 {
            let mut bad = base.clone();
            let function = bad.fns.iter_mut().find(|f| f.name.as_str() == "grow").unwrap();
            if mutation == 1 {
                function.param_modes[0] = align_ast::ParamMode::Borrow;
            } else {
                let mut changed = false;
                for statement in function.blocks.iter_mut().flat_map(|b| &mut b.stmts) {
                    if mutation >= 3 {
                        if let Stmt::Let(_, Rvalue::Field(slot, path)) = statement {
                            if mutation == 3 { path.push(0); } else { *slot = u32::MAX; }
                            changed = true;
                        }
                        continue;
                    }
                    let receiver = match statement {
                        Stmt::Let(_, Rvalue::BufferPut { buffer, .. } | Rvalue::BufferAppend { buffer, .. }
                            | Rvalue::BufferAppendFilled { buffer, .. }) => buffer,
                        _ => continue,
                    };
                    *receiver = Operand::Value(u32::MAX);
                    changed = true;
                }
                assert!(changed);
            }
            verify(&bad, false, mutation);
        }
    }
}
