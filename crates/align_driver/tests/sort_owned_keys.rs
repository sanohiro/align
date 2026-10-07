//! Owned sort keys retain their return cleanup bit without copying key ownership.
mod common;
use common::*;

fn fixture(files: &[(&str, &str)]) -> align_driver::ArtifactStage {
    let stage =
        align_driver::ArtifactStage::temp("sort-owned-keys").expect("exclusive owned-key fixture");
    for (name, source) in files {
        let path = stage.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    stage
}

fn run(
    stage: &align_driver::ArtifactStage,
    per_unit: bool,
    omit_drop: bool,
    external_keys: bool,
) -> std::process::Output {
    use std::io::ErrorKind;
    use std::time::{Duration, Instant};
    let entry = stage.path().join("main.align");
    let source = std::fs::read_to_string(&entry).unwrap();
    let mut sm = SourceMap::new();
    let mut programs = if per_unit {
        let built = build_per_unit(&mut sm, entry.to_str().unwrap(), &source);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &built.diags)
        );
        for unit in &built.units {
            let bytes = align_interface::serialize(&unit.summary);
            let replayed = align_interface::deserialize(&bytes).unwrap();
            assert_eq!(bytes, align_interface::serialize(&replayed));
        }
        built
            .units
            .into_iter()
            .map(|unit| unit.mir)
            .collect::<Vec<_>>()
    } else {
        let checked = check(&mut sm, entry.to_str().unwrap(), &source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &checked.diags)
        );
        vec![lower_to_mir(&checked.hir)]
    };
    if external_keys {
        let mut producers = 0;
        let mut consumers = 0;
        for program in &mut programs {
            for function in &mut program.fns {
                if function.name.as_str().ends_with("$external_key") {
                    for block in &mut function.blocks {
                        if let align_mir::Term::ReturnWithCleanup(returned) = &mut block.term {
                            returned.1 = align_mir::Operand::Const(align_mir::Const::Bool(false));
                            producers += 1;
                        }
                    }
                }
                if function.name.as_str().ends_with("$exercise") {
                    retain_external_keys(function);
                    consumers += 1;
                }
            }
            align_mir::producer::validate_mir_producers(program)
                .expect("external ownership ABI fixture");
        }
        assert!(
            producers > 0 && consumers > 0,
            "ABI fixture must bind both boundaries"
        );
    }
    if omit_drop {
        let mut removed = 0;
        for program in &mut programs {
            for function in &mut program.fns {
                if !function.name.as_str().ends_with("$ordered") {
                    continue;
                }
                for block in &mut function.blocks {
                    block.stmts.retain(|statement| {
                        let remove = matches!(statement, align_mir::Stmt::DropValue(align_mir::Operand::Value(value))
                            if function.value_tys[*value as usize] == align_sema::Ty::DynArray(align_sema::Scalar::String));
                        removed += usize::from(remove);
                        !remove
                    });
                    block.stmt_lines.clear();
                }
            }
        }
        assert!(removed > 0, "negative control removed no owned-key Drop");
    }
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, mir) in programs.iter().enumerate() {
        let object = stage.path().join(format!("{per_unit}-{index}.o"));
        emit_object_file(
            mir,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
        )
        .expect("owned-key codegen");
        objects.push(object);
        for library in &mir.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let executable = stage.path().join(format!("run-{per_unit}"));
    let refs = objects
        .iter()
        .map(|path| path.as_path())
        .collect::<Vec<_>>();
    link_objects(
        &align_driver::CDriver::default(),
        &refs,
        &executable,
        &libraries,
        Profile::Release,
    )
    .expect("owned-key link");

    struct ChildOwner {
        child: Option<std::process::Child>,
        deadline: Instant,
    }
    impl Drop for ChildOwner {
        fn drop(&mut self) {
            let Some(child) = self.child.as_mut() else {
                return;
            };
            // This fixture only computes and prints; it cannot launch descendants.
            loop {
                match child.kill() {
                    Err(error)
                        if error.kind() == ErrorKind::Interrupted
                            && Instant::now() < self.deadline => {}
                    _ => break,
                }
            }
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) if Instant::now() < self.deadline => {
                        std::thread::sleep(Duration::from_millis(1))
                    }
                    Err(error)
                        if error.kind() == ErrorKind::Interrupted
                            && Instant::now() < self.deadline => {}
                    _ => {
                        eprintln!("owned-key child did not reap before deadline");
                        return;
                    }
                }
            }
        }
    }
    let stdout = stage.path().join(format!("stdout-{per_unit}"));
    let stderr = stage.path().join(format!("stderr-{per_unit}"));
    let mut command = std::process::Command::new(executable);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::fs::File::create(&stdout).unwrap())
        .stderr(std::fs::File::create(&stderr).unwrap());
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut owner = ChildOwner {
        child: Some(command.spawn().expect("spawn owned-key owner")),
        deadline,
    };
    let status = loop {
        assert!(
            Instant::now() + Duration::from_secs(5) < owner.deadline,
            "owned-key child exceeded work deadline"
        );
        match owner.child.as_mut().unwrap().try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(1)),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => panic!("poll owned-key child: {error}"),
        }
    };
    owner.child.take();
    std::process::Output {
        status,
        stdout: std::fs::read(stdout).unwrap(),
        stderr: std::fs::read(stderr).unwrap(),
    }
}

// The fixture, rather than the sort, owns false-bit keys in a second column. It releases
// them only AFTER the sort's cleanup. Thus ignoring the false bit causes a double free,
// and leaking either ownership class fails the ordinary allocation-balance observation.
// This is a post-lowering ABI owner: current source String producers always return true.
fn retain_external_keys(function: &mut align_mir::Function) {
    use align_mir::{Block, Operand, Rvalue, Stmt, Term};
    use align_sema::{Scalar, Ty};
    let pointer = u32::try_from(function.value_tys.len()).unwrap();
    let array = pointer + 1;
    let header = pointer + 2;
    function.value_tys.extend([
        Ty::Box(Scalar::String),
        Ty::DynArray(Scalar::String),
        Ty::String,
    ]);
    let slot = u32::try_from(function.slots.len()).unwrap();
    function.slots.push(Ty::String);
    function.slot_align.push(None);
    let (key, cleanup) = function
        .blocks
        .iter()
        .flat_map(|b| &b.stmts)
        .find_map(|s| match s {
            Stmt::Let(v, Rvalue::CallWithCleanup(call))
                if call.target.as_str().ends_with("$mixed_key") =>
            {
                Some((*v, call.cleanup))
            }
            _ => None,
        })
        .unwrap();
    let original = function
        .blocks
        .iter()
        .flat_map(|b| &b.stmts)
        .find_map(|s| match s {
            Stmt::Let(
                v,
                Rvalue::HeapAllocBuf {
                    elem: Ty::String, ..
                },
            ) => Some(*v),
            _ => None,
        })
        .unwrap();
    let false_block = u32::try_from(function.blocks.len()).unwrap();
    let mut continuation = None;
    for block in &mut function.blocks {
        let mut statements = Vec::new();
        for statement in &block.stmts {
            statements.push(statement.clone());
            match statement {
                Stmt::Let(v, Rvalue::HeapAllocBuf { count, .. }) if *v == original => {
                    statements.push(Stmt::Let(
                        pointer,
                        Rvalue::HeapAllocBuf {
                            count: count.clone(),
                            elem: Ty::String,
                        },
                    ));
                    statements.push(Stmt::Let(
                        array,
                        Rvalue::MakeDynArray {
                            ptr: Operand::Value(pointer),
                            len: count.clone(),
                        },
                    ));
                }
                Stmt::PtrStore(Operand::Value(p), index, _) if *p == original => {
                    statements.push(Stmt::Let(header, Rvalue::Load(slot)));
                    statements.push(Stmt::PtrStore(
                        Operand::Value(pointer),
                        index.clone(),
                        Operand::Value(header),
                    ));
                }
                Stmt::DropValue(Operand::Value(v))
                    if function.value_tys[*v as usize] == Ty::DynArray(Scalar::String) =>
                {
                    statements.push(Stmt::DropValue(Operand::Value(array)));
                }
                _ => {}
            }
        }
        if let Term::Branch(Operand::Value(bit), _, otherwise) = &mut block.term {
            if *bit == cleanup {
                statements.push(Stmt::DropFlagInit(slot));
                continuation = Some(*otherwise);
                *otherwise = false_block;
            }
        }
        block.stmts = statements;
        block.stmt_lines.clear();
    }
    function.blocks.push(Block {
        id: false_block,
        stmts: vec![Stmt::Store(slot, Operand::Value(key))],
        stmt_lines: Vec::new(),
        term: Term::Goto(continuation.expect("exact key cleanup branch")),
    });
}

// Fail at link time if allocation accounting silently loses its feature owner.
const _: extern "C" fn() -> i64 = align_runtime::align_rt_alloc_count;
const _: extern "C" fn() -> i64 = align_runtime::align_rt_free_count;

const HELPERS: &str = r#"module ordering
pub fn key(value: i64) -> string {
  print(value)
  keys := ["", "a", "a\0", "a\0b", "aa", "ab", "z", "é", "中", "😀"]
  return keys[value % 10].clone()
}
pub fn ordered(xs: slice<i64>) -> array<i64> = xs.sort_by_key(key)
pub fn generic<T>(marker: T, xs: slice<i64>) -> array<i64> = ordered(xs)
pub fn agrees(xs: slice<i64>, expected: slice<i64>) -> bool {
  values := generic(true, xs)
  if values.len() != expected.len() { return false }
  mut i := 0
  loop {
    if i >= expected.len() { break }
    if values[i] != expected[i] { return false }
    i = i + 1
  }
  return true
}
fn fallible(text: str, good: bool) -> Result<string, Error> {
  if good { return Ok(text.clone()) }
  return Err(Error.Invalid)
}
fn relay(text: str, good: bool) -> Result<string, Error> {
  copied := fallible(text, good).map_err(fn e: Error { e })?
  return Ok(copied)
}
fn controlled(text: str) -> string {
  if text.len() == 0 { return "".clone() }
  selected := relay(text, text.len() > 1) else { text.clone() }
  optional: Option<string> := Some(selected)
  return match optional { Some(value) => loop { break value }, None => "".clone() }
}
fn early_source(flag: bool) -> i64 {
  source := [1, 2]
  values := (if flag { return 7 } else { source[..] }).sort_by_key(key)
  return values.len()
}
pub fn other_paths() -> bool {
  if early_source(true) != 7 { return false }
  prefix := "key"
  captured := [3, 1, 2].sort_by_key(fn value: i64 { prefix.clone() })
  if captured[0] != 3 || captured[2] != 2 { return false }
  owner := "borrowed".clone()
  strings := [owner.trim(), "", "b", "aa"].sort_by_key(controlled)
  if strings[0] != "" || strings[3] != "borrowed" { return false }
  filtered := [3, 0, 1, 2].where(fn x: i64 { x > 0 }).sort_by_key(key)
  if filtered[0] != 1 || filtered[2] != 3 { return false }
  arena out {
    // clone_in returns a borrowed str; clone still returns an independent heap String.
    text := "region".clone_in(out)
    region_capture := [3, 1].sort_by_key(fn value: i64 { text.clone() })
    if region_capture[0] != 3 || region_capture[1] != 1 { return false }
    region_source := [text, "a"].sort_by_key(fn value: str { value.clone() })
    if region_source[0] != "a" || region_source[1] != "region" { return false }
  }
  return true
}
"#;

fn array(values: &[i64]) -> String {
    if values.is_empty() {
        return "[0][0..0]".into();
    }
    format!(
        "[{}][..]",
        values
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[test]
fn owned_string_keys_order_effects_and_cleanup() {
    assert!(backend_available(), "owned-key owner requires LLVM");
    let mut exercise = String::from("module main\nimport ordering\nfn exercise() -> bool {\n");
    let mut expected = String::new();
    for n in [0, 1, 2, 31, 32, 33, 65, 129] {
        let mixed: Vec<i64> = (0..n).map(|i| i * 7).collect();
        let mut ordered = mixed.clone();
        ordered.sort_by_key(|value| value % 10);
        for input in [
            ordered.clone(),
            ordered.iter().copied().rev().collect(),
            mixed,
        ] {
            let mut output = input.clone();
            output.sort_by_key(|value| value % 10);
            exercise.push_str(&format!(
                "  if !ordering.agrees({}, {}) {{ return false }}\n",
                array(&input),
                array(&output)
            ));
            for value in input {
                expected.push_str(&format!("{value}\n"));
            }
        }
    }
    exercise.push_str(
        r#"  return ordering.other_paths()
}
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn main() -> i32 {
  before_alloc := unsafe { align_rt_alloc_count() }
  before_free := unsafe { align_rt_free_count() }
  if !exercise() { return 2 }
  allocated := unsafe { align_rt_alloc_count() } - before_alloc
  freed := unsafe { align_rt_free_count() } - before_free
  if allocated < 100 { return 3 }
  if allocated != freed { return 1 }
  return 0
}
"#,
    );
    expected.push_str("3\n1\n2\n");
    let stage = fixture(&[("main.align", &exercise), ("ordering.align", HELPERS)]);
    for per_unit in [false, true] {
        for omit_drop in [false, true] {
            let output = run(&stage, per_unit, omit_drop, false);
            assert_eq!(
                output.status.code(),
                Some(if omit_drop { 1 } else { 0 }),
                "per_unit={per_unit}, omit_drop={omit_drop}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
        }
    }
}

#[test]
fn owned_keys_preserve_element_lifetimes_and_copy_boundaries() {
    for (name, source, needle) in [
        (
            "escape",
            "fn bad() -> array<str> { owner := \"text\".clone(); return [owner.trim()].sort_by_key(fn s: str { s.clone() }) }",
            "borrow",
        ),
        (
            "replace",
            "fn main() { mut owner := \"text\".clone(); sorted := [owner.trim()].sort_by_key(fn s: str { s.clone() }); owner = \"new\".clone(); print(sorted[0]) }",
            "borrow",
        ),
        (
            "move-element",
            "fn bad(xs: slice<string>) { values := xs.sort_by_key(fn s: str { s.clone() }) }",
            "Copy",
        ),
        (
            "map",
            "fn main() { values := [1].map(fn x: i64 { \"key\".clone() }).to_array() }",
            "Move",
        ),
        (
            "bool-key",
            "fn main() { values := [1].sort_by_key(fn x: i64 { true }) }",
            "orderable",
        ),
    ] {
        let stage = fixture(&[("main.align", source)]);
        let entry = stage.path().join("main.align");
        for per_unit in [false, true] {
            let mut sm = SourceMap::new();
            let diags = if per_unit {
                check_per_unit(&mut sm, entry.to_str().unwrap(), source).diags
            } else {
                check(&mut sm, entry.to_str().unwrap(), source).diags
            };
            let rendered = align_driver::format_diagnostics(&sm, &diags);
            assert!(
                diags.has_errors() && rendered.contains(needle),
                "{name}, per_unit={per_unit}: {rendered}"
            );
        }
    }
}

#[test]
fn mixed_return_cleanup_bits_leave_external_key_bytes_live() {
    assert!(backend_available(), "owned-key ABI owner requires LLVM");
    let helper = r#"module ordering
pub fn external_key(value: i64) -> string = "a".clone()
fn mixed_key(value: i64) -> string {
  if value % 2 == 0 { return external_key(value) }
  return "z".clone()
}
pub fn exercise(xs: slice<i64>) -> bool {
  sorted := xs.sort_by_key(mixed_key)
  // Every even key is equal; stable order remains visible in the elements.
  return sorted[0] == 2 && sorted[1] == 4 && sorted[2] == 1 && sorted[3] == 3
}
"#;
    let main = r#"module main
import ordering
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn main() -> i32 {
  a := unsafe { align_rt_alloc_count() }
  f := unsafe { align_rt_free_count() }
  if !ordering.exercise([1, 2, 3, 4][..]) { return 2 }
  allocated := unsafe { align_rt_alloc_count() } - a
  freed := unsafe { align_rt_free_count() } - f
  if allocated == 0 || allocated != freed { return 1 }
  return 0
}
"#;
    let stage = fixture(&[("main.align", main), ("ordering.align", helper)]);
    for per_unit in [false, true] {
        let output = run(&stage, per_unit, false, true);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
