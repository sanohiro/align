//! Plan244: initialized external view authority and synchronous positional byte writes.
mod common;
use common::*;

#[allow(dead_code)]
#[path = "helpers/indexed_array_field.rs"]
mod indexed_array_field;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

const DROP: &str =
    "module pkg.window.internal.drop\npub fn release(value: raw) { unsafe { raw.free(value) } }\n";
const ROOT: &str = r#"module pkg.window
import pkg.window.internal.drop
import std.fs
pub resource memory = pkg.window.internal.drop.release
pub fn new() -> memory {
  unsafe {
    pointer := raw.alloc(8)
    raw.store(pointer, 0, 0 as i64)
    resource.from_raw(pointer)
  }
}
"#;
fn check_window(body: &str) -> bool {
    let root = format!("{ROOT}\n{body}");
    let files = [
        ("pkg/window.align", root.as_str()),
        ("pkg/window/internal/drop.align", DROP),
        ("main.align", "import pkg.window\nfn main() {}"),
    ];
    let result = assert_same_verdict("external-view", &files, "main.align");
    if result.diags.has_errors() {
        eprintln!(
            "{}",
            check_multi_diagnostics("external-view", &files, "main.align")
        );
    }
    !result.diags.has_errors()
}

fn verdict(body: &str, rejected: bool, needle: &str) {
    let root = format!("{ROOT}\n{body}");
    let result = diff_check_multi(
        "external-view-matrix",
        &[
            ("pkg/window.align", &root),
            ("pkg/window/internal/drop.align", DROP),
            ("main.align", "import pkg.window\nfn main() {}"),
        ],
        "main.align",
    );
    for (errors, diagnostics) in [
        (result.whole_errors, result.whole_diags),
        (result.per_unit_errors, result.per_unit_diags),
    ] {
        assert_eq!(errors, rejected, "{body}\n{diagnostics}");
        if rejected {
            assert!(diagnostics.contains(needle), "{body}\n{diagnostics}");
        }
    }
}

const VIEW: &str = "pointer := resource.raw(resource.borrow(owner)); mut bytes: slice<u8> := resource.view_mut_from_raw(owner, pointer, 8) else { return }";

#[test]
fn external_view_lifetime_and_timing_matrix() {
    for action in ["", "bytes[0] = 65", "mut part := bytes[1..3]; part[0] = 66"] {
        verdict(
            &format!(
                "pub fn probe(borrow mut owner: memory) {{ unsafe {{ {VIEW}; {action}; print(bytes.len()) }} }}"
            ),
            false,
            "",
        );
    }
    for action in [
        "moved := owner",
        "owner = new()",
        "raw.free(resource.into_raw(owner))",
        "touch(owner)",
    ] {
        verdict(
            &format!(
                "fn touch(borrow mut value: memory) {{}}\npub fn probe() {{ mut owner := new(); unsafe {{ {VIEW}; {action}; print(bytes.len()) }} }}"
            ),
            true,
            "invalidated",
        );
    }
    for length in ["{ owner = new(); 8 }", "{ touch(owner); 8 }"] {
        verdict(
            &format!(
                "fn touch(borrow mut value: memory) {{}}\npub fn probe() {{ mut owner := new(); unsafe {{ pointer := resource.raw(resource.borrow(owner)); value: Option<slice<u8>> := resource.view_mut_from_raw(owner, pointer, {length}) }} }}"
            ),
            true,
            "",
        );
    }
    for select in [
        "if flag { bytes } else { bytes }",
        "match 1 { 0 => bytes, _ => bytes }",
        "{ bytes }",
    ] {
        verdict(
            &format!(
                "pub fn probe<T>(borrow mut owner: memory, flag: bool, tag: T) {{ unsafe {{ {VIEW}; mut selected := {select}; selected[0] = 65; print(bytes[0]) }} }}"
            ),
            false,
            "",
        );
    }
    verdict(
        &format!(
            "pub fn probe(borrow mut owner: memory) {{ unsafe {{ {VIEW}; mut i := 0; loop {{ if i == 2 {{ break }}; bytes[i] = 65; i = i + 1 }}; print(bytes[0]) }} }}"
        ),
        false,
        "",
    );
    verdict(
        "pub fn escape() -> slice<u8> { mut owner := new(); unsafe { pointer := resource.raw(resource.borrow(owner)); value: slice<u8> := resource.view_mut_from_raw(owner, pointer, 0) else { return [] }; value } }",
        true,
        "",
    );
}

#[test]
fn external_view_safe_writer_authority() {
    for producer in [
        "view_mut_from_raw(owner",
        "view_from_raw(resource.borrow(owner)",
    ] {
        for writer in [
            "bytes[0] = 1",
            "bytes.fill(1)",
            "write(bytes)",
            "ignored := input.read_at(0, bytes)",
        ] {
            verdict(
                &format!(
                    "fn write(borrow mut value: slice<u8>) {{ value[0] = 1 }}\npub fn probe(borrow input: file, borrow mut owner: memory) {{ unsafe {{ pointer := resource.raw(resource.borrow(owner)); mut bytes: slice<u8> := resource.{producer}, pointer, 8) else {{ return }}; {writer} }} }}"
                ),
                !producer.starts_with("view_mut"),
                "read-only",
            );
        }
    }
    for same in [false, true] {
        let second = if same { "owner" } else { "other" };
        verdict(
            &format!(
                "fn pair(borrow mut a: slice<u8>, borrow mut b: slice<u8>) {{ a[0] = 1; b[0] = 2 }}\npub fn probe(borrow mut owner: memory, borrow mut other: memory) {{ unsafe {{ {VIEW}; pointer2 := resource.raw(resource.borrow({second})); mut second: slice<u8> := resource.view_mut_from_raw({second}, pointer2, 8) else {{ return }}; pair(bytes, second) }} }}"
            ),
            same,
            "aliases",
        );
    }
    for ty in ["str", "slice<bool>", "slice<raw>"] {
        verdict(
            &format!(
                "pub fn probe(borrow mut owner: memory) {{ unsafe {{ pointer := resource.raw(resource.borrow(owner)); value: Option<{ty}> := resource.view_mut_from_raw(owner, pointer, 0) }} }}"
            ),
            true,
            "",
        );
    }
    for owner in ["new()", "holder.owner"] {
        verdict(
            &format!(
                "Holder {{ owner: memory }}\npub fn probe() {{ mut holder := Holder {{ owner: new() }}; unsafe {{ pointer := raw.alloc(8); value: Option<slice<u8>> := resource.view_mut_from_raw({owner}, pointer, 0); raw.free(pointer) }} }}"
            ),
            true,
            "",
        );
    }
}

#[test]
fn external_view_authority_matrix() {
    assert!(check_window(
        r#"
pub fn fill(borrow mut owner: memory) {
  unsafe {
    pointer := resource.raw(resource.borrow(owner))
    mut bytes: slice<u8> := resource.view_mut_from_raw(owner, pointer, 8) else { return }
    bytes[0] = 65
    print(bytes[0])
  }
}
"#
    ));
    for owner in ["borrow owner: memory", "reference: resource_ref<memory>"] {
        let select = if owner.starts_with("borrow") {
            "owner"
        } else {
            "reference"
        };
        assert!(!check_window(&format!(
            r#"
pub fn fill({owner}) {{
  unsafe {{
    pointer := raw.alloc(8)
    mut bytes: slice<u8> := resource.view_mut_from_raw({select}, pointer, 8) else {{ raw.free(pointer); return }}
    bytes[0] = 65
    raw.free(pointer)
  }}
}}
"#
        )));
    }
    assert!(!check_window(
        r#"
pub fn fill(borrow mut owner: memory) {
  unsafe {
    pointer := resource.raw(resource.borrow(owner))
    mut bytes: slice<u8> := resource.view_from_raw(resource.borrow(owner), pointer, 8) else { return }
    bytes[0] = 65
  }
}
"#
    ));
}

#[test]
fn external_view_validation_matrix() {
    for observe in [
        "text: str := resource.view_from_raw(resource.borrow(owner), pointer, 8) else { return }",
        "shared: slice<u8> := resource.view_from_raw(resource.borrow(owner), pointer, 8) else { return }; text := shared.as_str() else { return }",
    ] {
        let body = format!(
            r#"
pub fn mutate(borrow mut owner: memory) {{
  unsafe {{
    pointer := resource.raw(resource.borrow(owner))
    {observe}
    mut bytes: slice<u8> := resource.view_mut_from_raw(owner, pointer, 8) else {{ return }}
    bytes[0] = 255
    print(text)
  }}
}}
"#
        );
        assert!(!check_window(&body), "stale native UTF-8 view: {observe}");
    }
}

#[test]
fn external_file_read_at_source() {
    assert!(check_window(
        r#"
fn change(borrow mut bytes: slice<u8>) { bytes[0] = 66 }
pub fn fill(borrow input: file, borrow mut owner: memory) -> Result<i64, Error> {
  unsafe {
    pointer := resource.raw(resource.borrow(owner))
    mut bytes: slice<u8> := resource.view_mut_from_raw(owner, pointer, 8) else { return Err(Error.Invalid) }
    change(bytes)
    count := input.read_at(2, bytes)?
    print(bytes[0])
    Ok(count)
  }
}

"#
    ));
}

#[test]
fn external_file_windows_native() {
    owned_fixture::run("external_file_windows_native", |stage| {
        std::fs::create_dir_all(stage.join("pkg/window/internal")).unwrap();
        std::fs::write(stage.join("input"), b"abcdefgh").unwrap();
        // Each owned allocation prints exactly once at Drop, after all reads.
        std::fs::write(
            stage.join("pkg/window/internal/drop.align"),
            DROP.replace(
                "unsafe { raw.free(value) }",
                "print(99); unsafe { raw.free(value) }",
            ),
        )
        .unwrap();
        let root = format!(
            r#"{ROOT}
pub fn fill<T>(borrow input: file, borrow mut owner: memory, marker: T) -> Result<bool, Error> {{
 unsafe {{
  pointer := resource.raw(resource.borrow(owner))
  mut bytes: slice<u8> := resource.view_mut_from_raw(owner, pointer, 8) else {{ return Err(Error.Invalid) }}
  bytes.fill(88)
  mut window := bytes[2..6]
  if input.read_at(2, window)? != 4 {{ return Ok(false) }}
  if bytes[0] != 88 || bytes[1] != 88 || bytes[2] != 99 || bytes[5] != 102 || bytes[6] != 88 || bytes[7] != 88 {{ return Ok(false) }}
  if input.read_at(6, window)? != 2 {{ return Ok(false) }}
  if bytes[2] != 103 || bytes[3] != 104 || bytes[4] != 101 || bytes[5] != 102 {{ return Ok(false) }}
  if input.read_at(8, window)? != 0 {{ return Ok(false) }}
  mut empty := bytes[4..4]
  if input.read_at(0, empty)? != 0 {{ return Ok(false) }}
  match input.read_at(-1, window) {{ Ok(_) => {{ return Ok(false) }}, Err(_) => {{}} }}
  // A second foreign read observes the bytes without another allocation or copy.
  actual: u8 := raw.load(pointer, 2)
  Ok(actual == 103 && bytes[6] == 88)
 }}
}}
pub fn run(path: str) -> Result<bool, Error> {{
 input := fs.open_rw(path)?
 mut owner := new()
 fill(input, owner, true)
}}
"#
        );
        std::fs::write(stage.join("pkg/window.align"), root).unwrap();
        let source = format!(
            "import pkg.window\nfn main() -> i32 {{ if pkg.window.run({:?}) else false {{ 0 }} else {{ 1 }} }}",
            stage.join("input").to_str().unwrap()
        );
        for per_unit in [false, true] {
            let programs = indexed_array_field::programs(stage, &source, per_unit).unwrap();
            let output = indexed_array_field::run_programs(
                stage,
                &programs,
                &format!("external-{per_unit}"),
                false,
            )
            .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, b"99\n");
            assert!(output.stderr.is_empty(), "{output:?}");
        }
        let project = Proj::new("external-cache", &[], "main.align");
        std::fs::create_dir_all(project.dir.join("pkg/window/internal")).unwrap();
        for path in ["pkg/window.align", "pkg/window/internal/drop.align"] {
            project.write(path, &std::fs::read_to_string(stage.join(path)).unwrap());
        }
        project.write(
            "main.align",
            &format!(
                "import pkg.window\nfn main() {{ print(pkg.window.run({:?}) else false) }}",
                stage.join("input").to_str().unwrap()
            ),
        );
        let cache = project.cache();
        for hit in [false, true] {
            let built = thin_build(&project, &cache, 1);
            assert_eq!(built.all_hit(), hit);
            assert_eq!(built.run(&project), "99\ntrue\n");
        }
    });
}

fn shape_program(used: bool) -> align_sema::hir::Program {
    let produce = if used {
        "-> Option<slice<u8>> = unsafe { resource.view_mut_from_raw(owner, pointer, length) }"
    } else {
        "{ unsafe { unused: Option<slice<u8>> := resource.view_mut_from_raw(owner, pointer, length) } }"
    };
    let read = if used {
        "-> Result<i64, Error> = input.read_at(offset, bytes)"
    } else {
        "{ unused := input.read_at(offset, bytes) }"
    };
    let root = format!(
        "{ROOT}\npub fn produce(borrow mut owner: memory, pointer: raw, length: i64) {produce}\npub fn read(borrow input: file, offset: i64, borrow mut bytes: slice<u8>) {read}\n"
    );
    let source = "import pkg.window\nfn main() {}";
    let project = Proj::new("external-shape", &[], "main.align");
    std::fs::create_dir_all(project.dir.join("pkg/window/internal")).unwrap();
    for (path, source) in [
        ("pkg/window.align", root.as_str()),
        ("pkg/window/internal/drop.align", DROP),
        ("main.align", source),
    ] {
        project.write(path, source);
    }
    let mut sm = SourceMap::new();
    let checked = check(
        &mut sm,
        project.dir.join("main.align").to_str().unwrap(),
        source,
    );
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    checked.hir
}

#[test]
fn external_view_hir_shape() {
    use align_sema::{Ty, hir::ExprKind};
    for used in [false, true] {
        let original = shape_program(used);
        assert!(align_mir::lower_program_checked(&original, false, None).is_ok());
        for mutation in 0..9 {
            let mut bad = original.clone();
            let function = bad
                .fns
                .iter_mut()
                .find(|f| f.name.ends_with("produce"))
                .unwrap();
            if mutation == 0 {
                function.param_modes[0] = align_ast::ParamMode::Borrow;
            } else if mutation == 1 {
                function.locals[function.params[0] as usize].is_mut = false;
            } else {
                let outer = function.body.value.as_deref_mut().unwrap();
                let ExprKind::Unsafe(block) = &mut outer.kind else {
                    panic!("unsafe")
                };
                let expression = if used {
                    block.value.as_deref_mut().unwrap()
                } else {
                    let align_sema::hir::Stmt::Let { init, .. } = &mut block.stmts[0] else {
                        panic!("let")
                    };
                    init
                };
                let ExprKind::ResourceViewFromRaw {
                    owner,
                    ptr,
                    len,
                    view,
                    resource,
                } = &mut expression.kind
                else {
                    panic!("view")
                };
                match mutation {
                    2 => owner.ty = Ty::ResourceRef(*resource),
                    3 => owner.kind = ExprKind::Local(u32::MAX),
                    4 => ptr.ty = Ty::Bool,
                    5 => len.ty = Ty::Bool,
                    6 => *resource = u32::MAX,
                    7 => *view = align_sema::hir::ResourceViewKind::StrUtf8,
                    _ => expression.ty = Ty::Bool,
                }
            }
            assert!(
                align_mir::lower_program_checked(&bad, false, None).is_err(),
                "accepted HIR {used}/{mutation}"
            );
            let sm = SourceMap::new();
            for program in [
                align_mir::lower_program(&bad),
                align_mir::lower_program_per_unit(&bad),
                align_mir::lower_program_located(&bad, &sm),
                align_mir::lower_program_per_unit_located(&bad, &sm),
            ] {
                assert!(
                    program.fns.is_empty(),
                    "malformed HIR escaped public entrypoint"
                );
            }
        }
    }
}

#[test]
fn external_view_mir_shape() {
    use align_mir::{Const, Operand, Rvalue, Stmt};
    use align_sema::{Ty, hir::ResourceViewKind};
    let emit = |program: &align_mir::Program| {
        align_codegen_llvm::emit_llvm_ir(
            program,
            &align_driver::BuildTarget::Baseline,
            align_driver::Profile::Release,
            false,
            &[],
            None,
        )
    };
    for used in [false, true] {
        let original = align_mir::lower_program_checked(&shape_program(used), false, None).unwrap();
        emit(&original).unwrap();
        for mutation in 0..11 {
            let mut bad = original.clone();
            let function = bad
                .fns
                .iter_mut()
                .find(|f| f.name.as_str().ends_with("produce"))
                .unwrap();
            if mutation == 0 {
                function.param_modes[0] = align_ast::ParamMode::Borrow;
            } else {
                let (id, expression) = function
                    .blocks
                    .iter_mut()
                    .flat_map(|b| &mut b.stmts)
                    .find_map(|s| match s {
                        Stmt::Let(id, value @ Rvalue::ResourceViewFromRaw { .. }) => {
                            Some((*id, value))
                        }
                        _ => None,
                    })
                    .unwrap();
                let Rvalue::ResourceViewFromRaw {
                    owner,
                    ptr,
                    len,
                    resource,
                    view,
                    allow_null_if_empty,
                    check_nonnegative_len,
                    check_alignment,
                    check_utf8,
                } = expression
                else {
                    unreachable!()
                };
                match mutation {
                    1 => *owner = Operand::Const(Const::Unit),
                    2 => *ptr = Operand::Const(Const::Bool(false)),
                    3 => *len = Operand::Const(Const::Bool(false)),
                    4 => *resource = u32::MAX,
                    5 => {
                        *view =
                            ResourceViewKind::Slice(align_sema::Scalar::Int(align_sema::IntTy {
                                bits: 8,
                                signed: false,
                            }))
                    }
                    6 => *allow_null_if_empty = false,
                    7 => *check_nonnegative_len = false,
                    8 => *check_alignment = 8,
                    9 => *check_utf8 = true,
                    _ => function.value_tys[id as usize] = Ty::Bool,
                }
            }
            assert!(
                emit(&bad).is_err(),
                "accepted MIR producer {used}/{mutation}"
            );
        }
        for mutation in 0..5 {
            let mut bad = original.clone();
            let function = bad
                .fns
                .iter_mut()
                .find(|f| f.name.as_str().ends_with("read"))
                .unwrap();
            if mutation == 0 {
                function.param_modes[2] = align_ast::ParamMode::Borrow;
            } else {
                let (id, expression) = function
                    .blocks
                    .iter_mut()
                    .flat_map(|b| &mut b.stmts)
                    .find_map(|s| match s {
                        Stmt::Let(id, value @ Rvalue::FileReadAt { .. }) => Some((*id, value)),
                        _ => None,
                    })
                    .unwrap();
                let Rvalue::FileReadAt {
                    file,
                    offset,
                    destination,
                } = expression
                else {
                    unreachable!()
                };
                match mutation {
                    1 => *file = Operand::Const(Const::Unit),
                    2 => *offset = Operand::Const(Const::Bool(false)),
                    3 => *destination = Operand::Const(Const::Unit),
                    _ => function.value_tys[id as usize] = Ty::Bool,
                }
            }
            assert!(emit(&bad).is_err(), "accepted MIR writer {used}/{mutation}");
        }
    }
}

#[test]
fn external_validation_carriers_and_writers() {
    let helpers = r#"
Holder { text: str }
fn text(borrow owner: memory) -> Option<str> = unsafe { resource.view_from_raw(resource.borrow(owner), resource.raw(resource.borrow(owner)), 8) }
fn change(borrow mut bytes: slice<u8>) { bytes[0] = 255 }
"#;
    for observe in [
        "text: str := resource.view_from_raw(resource.borrow(owner), pointer, 8) else { return }; held := Holder { text: text }; observed := held.text",
        "observed := text(owner) else { return }",
        "text: str := resource.view_from_raw(resource.borrow(owner), pointer, 8) else { return }; observed := if flag { text } else { text }",
    ] {
        for writer in [
            "bytes[0] = 255",
            "bytes.fill(255)",
            "change(bytes)",
            "ignored := input.read_at(0, bytes)",
        ] {
            for same in [false, true] {
                let target = if same { "owner" } else { "other" };
                let body = format!(
                    "{helpers}\npub fn probe(borrow input: file, borrow mut owner: memory, borrow mut other: memory, flag: bool) {{ unsafe {{ pointer := resource.raw(resource.borrow(owner)); {observe}; destination := resource.raw(resource.borrow({target})); mut bytes: slice<u8> := resource.view_mut_from_raw({target}, destination, 8) else {{ return }}; {writer}; print(observed) }} }}"
                );
                verdict(&body, same, "invalidated");
            }
        }
    }
    for writer in [
        "bytes[0] = 65",
        "bytes.fill(65)",
        "ignored := input.read_at(0, bytes)",
    ] {
        verdict(
            &format!(
                "pub fn probe(borrow input: file, borrow mut owner: memory) {{ unsafe {{ {VIEW}; {writer}; fresh := bytes.as_str() else {{ return }}; print(fresh) }} }}"
            ),
            false,
            "",
        );
    }
}

#[test]
fn external_view_return_privilege_and_parent_matrix() {
    let helpers = r#"
fn view(borrow mut owner: memory) -> Option<slice<u8>> = unsafe { resource.view_mut_from_raw(owner, resource.raw(resource.borrow(owner)), 8) }
fn write(borrow mut bytes: slice<u8>) { bytes[0] = 65 }
pub resource dependent_memory = pkg.window.internal.drop.release
fn child(parent: resource_ref<memory>) -> dependent_memory = unsafe { return resource.from_raw_borrowed(raw.alloc(8), parent) }
"#;
    for writer in [
        "bytes[0] = 65",
        "write(bytes)",
        "ignored := input.read_at(0, bytes)",
    ] {
        verdict(
            &format!(
                "{helpers}\npub fn probe(borrow input: file, borrow mut owner: memory) {{ mut bytes := view(owner) else {{ return }}; {writer} }}"
            ),
            true,
            "read-only",
        );
    }
    // A child whose destructor depends on the parent prevents a whole-owner mutation,
    // while a byte write retains both resource generations.
    for action in ["bytes[0] = 65", "owner = new()"] {
        verdict(
            &format!(
                "{helpers}\npub fn probe() {{ mut owner := new(); dependent := child(resource.borrow(owner)); unsafe {{ {VIEW}; {action} }}; unsafe {{ print(resource.raw(resource.borrow(dependent)).is_null()) }} }}"
            ),
            action.contains("new"),
            "",
        );
    }
    let body = format!(
        "pub fn probe(borrow mut owner: memory, pointer: raw) {{ value: Option<slice<u8>> := resource.view_mut_from_raw(owner, pointer, 0) }}"
    );
    verdict(&body, true, "unsafe");
    let root = ROOT.to_string();
    let main = "import pkg.window\nfn main() { mut owner := pkg.window.new(); unsafe { pointer := raw.alloc(8); view: Option<slice<u8>> := resource.view_mut_from_raw(owner, pointer, 0); raw.free(pointer) } }";
    let result = diff_check_multi(
        "external-privilege",
        &[
            ("pkg/window.align", &root),
            ("pkg/window/internal/drop.align", DROP),
            ("main.align", main),
        ],
        "main.align",
    );
    assert!(result.whole_errors && result.per_unit_errors);
    assert!(result.whole_diags.contains("private") && result.per_unit_diags.contains("private"));
}

#[test]
fn external_native_view_admission() {
    owned_fixture::run("external_native_view_admission", |stage| {
        std::fs::create_dir_all(stage.join("pkg/window/internal")).unwrap();
        std::fs::write(stage.join("pkg/window/internal/drop.align"), DROP).unwrap();
        let mut body = ROOT.to_string();
        // Integers are only used to forge numerical pointers that must be rejected before
        // dereference. The backing allocation itself remains real, initialized and owned.
        body.push_str("\nfn pointer(bits: i64) -> raw { unsafe { scratch := raw.alloc(8); raw.store(scratch, 0, bits); result: raw := raw.load(scratch, 0); raw.free(scratch); result } }\n");
        let mut probes = Vec::new();
        for (index, scalar) in [
            "i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64", "f32", "f64",
        ]
        .iter()
        .enumerate()
        {
            for mutable in [false, true] {
                let name = format!("probe_{index}_{mutable}");
                probes.push(format!("{name}()"));
                let owner = if mutable {
                    "owner"
                } else {
                    "resource.borrow(owner)"
                };
                let op = if mutable {
                    "view_mut_from_raw"
                } else {
                    "view_from_raw"
                };
                let width = if scalar.ends_with("64") {
                    8
                } else if scalar.ends_with("32") {
                    4
                } else if scalar.ends_with("16") {
                    2
                } else {
                    1
                };
                body.push_str(&format!(r#"
fn {name}() -> bool {{
 mut owner := new()
 unsafe {{
  valid := resource.raw(resource.borrow(owner))
  empty: slice<{scalar}> := resource.{op}({owner}, pointer(0), 0) else {{ return false }}
  full: slice<{scalar}> := resource.{op}({owner}, valid, 1) else {{ return false }}
  if empty.len() != 0 || full.len() != 1 {{ return false }}
  {{ absent: Option<slice<{scalar}>> := resource.{op}({owner}, pointer(0), 1); match absent {{ Some(_) => {{ return false }}, None => {{}} }} }}
  {{ negative: Option<slice<{scalar}>> := resource.{op}({owner}, valid, -1); match negative {{ Some(_) => {{ return false }}, None => {{}} }} }}
  {{ overflow: Option<slice<{scalar}>> := resource.{op}({owner}, pointer(-{width}), 2); match overflow {{ Some(_) => {{ return false }}, None => {{}} }} }}
"#));
                if width > 1 {
                    body.push_str(&format!("bad: Option<slice<{scalar}>> := resource.{op}({owner}, pointer(1), 1); match bad {{ Some(_) => {{ return false }}, None => {{}} }}\nwide: Option<slice<{scalar}>> := resource.{op}({owner}, valid, 9223372036854775807); match wide {{ Some(_) => {{ return false }}, None => {{}} }}\n"));
                }
                body.push_str("true } }\n");
            }
        }
        body.push_str(&format!(
            "\npub fn run() -> bool = {}\n",
            probes.join(" && ")
        ));
        std::fs::write(stage.join("pkg/window.align"), body).unwrap();
        indexed_array_field::execute(
            stage,
            "import pkg.window\nfn main() -> i32 { if pkg.window.run() { 0 } else { 1 } }",
            "admission",
        )
        .unwrap();
    });
}

#[test]
fn external_file_argument_and_hir_matrix() {
    use align_sema::{
        Ty,
        hir::{ExprKind, Stmt},
    };
    for used in [false, true] {
        let original = shape_program(used);
        for mutation in 0..7 {
            let mut bad = original.clone();
            let f = bad
                .fns
                .iter_mut()
                .find(|f| f.name.ends_with("read"))
                .unwrap();
            if mutation == 0 {
                f.param_modes[2] = align_ast::ParamMode::Borrow;
            } else {
                let expression = if used {
                    f.body.value.as_deref_mut().unwrap()
                } else {
                    let Stmt::Let { init, .. } = &mut f.body.stmts[0] else {
                        panic!("read let")
                    };
                    init
                };
                let ExprKind::FileReadAt {
                    file,
                    offset,
                    destination,
                } = &mut expression.kind
                else {
                    panic!("read")
                };
                match mutation {
                    1 => file.ty = Ty::Buffer,
                    2 => offset.ty = Ty::Bool,
                    3 => destination.ty = Ty::Str,
                    4 => destination.kind = ExprKind::Local(u32::MAX),
                    5 => expression.ty = Ty::Bool,
                    _ => {
                        let old = file.as_ref().clone();
                        file.kind = ExprKind::Block(align_sema::hir::Block {
                            stmts: vec![],
                            value: Some(Box::new(old)),
                        });
                    }
                }
            }
            assert!(
                align_mir::lower_program_checked(&bad, false, None).is_err(),
                "accepted reader {used}/{mutation}"
            );
        }
    }
    for mode in ["borrow mut", "out"] {
        verdict(
            &format!(
                "pub fn read(borrow input: file, {mode} bytes: slice<u8>) -> Result<i64, Error> = input.read_at(0, bytes)"
            ),
            false,
            "",
        );
    }
    verdict(
        "pub fn read(borrow input: file) -> Result<i64, Error> { mut bytes := [0 as u8, 0 as u8]; input.read_at(0, bytes) }",
        false,
        "",
    );
    verdict(
        "fn retire(input: file) -> i64 = 0\npub fn read(input: file, borrow mut bytes: slice<u8>) -> Result<i64, Error> = input.read_at(retire(input), bytes)",
        true,
        "",
    );
}

#[test]
fn external_parent_validation() {
    let helpers = r#"
pub resource dependent_memory = pkg.window.internal.drop.release
fn child(parent: resource_ref<memory>) -> dependent_memory { unsafe { pointer := raw.alloc(8); raw.store(pointer, 0, 0 as i64); return resource.from_raw_borrowed(pointer, parent) } }
"#;
    for write in [false, true] {
        let action = if write { "bytes[0] = 65" } else { "" };
        verdict(
            &format!(
                "{helpers}\npub fn probe() {{ owner := new(); mut dependent := child(resource.borrow(owner)); unsafe {{ pointer := resource.raw(resource.borrow(owner)); text: str := resource.view_from_raw(resource.borrow(owner), pointer, 8) else {{ return }}; destination := resource.raw(resource.borrow(dependent)); mut bytes: slice<u8> := resource.view_mut_from_raw(dependent, destination, 8) else {{ return }}; {action}; print(text) }} }}"
            ),
            write,
            "invalidated",
        );
    }
}

#[test]
fn external_inline_destination_authority() {
    for count in [0, 2] {
        for field in [false, true] {
            for used in [false, true] {
                let ty = if field {
                    "Holder".to_string()
                } else {
                    format!("[u8; {count}]")
                };
                let destination = if field { "bytes.data" } else { "bytes" };
                let call = format!("input.read_at(0, {destination})");
                let tail = if used {
                    format!("-> Result<i64, Error> = {call}")
                } else {
                    format!("{{ unused := {call} }}")
                };
                let source = format!(
                    "import std.fs\nHolder {{ data: [u8; {count}] }}\nfn read(borrow input: file, borrow mut bytes: {ty}) {tail}\nfn main() {{}}"
                );
                let mut sm = SourceMap::new();
                let checked = check(&mut sm, "inline-destination", &source);
                assert!(
                    !checked.diags.has_errors(),
                    "{}",
                    align_driver::format_diagnostics(&sm, &checked.diags)
                );
                let mut program =
                    align_mir::lower_program_checked(&checked.hir, false, None).unwrap();
                let emit = |p: &align_mir::Program| {
                    align_codegen_llvm::emit_llvm_ir(
                        p,
                        &align_driver::BuildTarget::Baseline,
                        align_driver::Profile::Release,
                        false,
                        &[],
                        None,
                    )
                };
                emit(&program).unwrap();
                let f = program
                    .fns
                    .iter_mut()
                    .find(|f| f.name.as_str().ends_with("read"))
                    .unwrap();
                f.param_modes[1] = align_ast::ParamMode::Borrow;
                assert!(
                    emit(&program).is_err(),
                    "shared inline backing accepted: {count}/{field}/{used}"
                );
            }
        }
    }
}
