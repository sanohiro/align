//! Plan196: indexed owning array fields expose only their scalar length.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
#[path = "helpers/indexed_array_field.rs"]
mod indexed_array_field;
use indexed_array_field::{execute, programs, run_programs};
use align_span::SourceMap;
use align_driver::{BuildTarget, Profile};

#[test]
fn values_and_storage() {
    owned_fixture::run("values_and_storage", |stage| {
        execute(stage, r#"
R {ids:array<i64>}
Outer {rows:array<R>}
fn shared(borrow rows:array<R>,i:i64)->i64=rows[i].ids.len()
fn fixed(borrow rows:[R;1],i:i64)->i64=rows[i].ids.len()
fn sliced(rows:slice<R>,i:i64)->i64=rows[i].ids.len()
fn fresh()->array<R> {mut b:array_builder<R>:=array_builder();b.push(R{ids:[1,2].to_array()});return b.build()}
fn main()->i32 {
 rows:=fresh()
 i:=0
 if rows[i].ids.len()!=2 || shared(rows,i)!=2 || sliced(rows[..],i)!=2 {return 1}
 fixed_rows:=[R{ids:[3,4,5].to_array()}]
 if fixed_rows[i].ids.len()!=3 {return 2}
 outer:=Outer{rows:fresh()}
 if outer.rows[i].ids.len()!=2 {return 3}
 return 0
}"#, "values").unwrap();
        execute(stage, r#"
Leaf {n:i64}
Data {owned:array<string>,records:array<Leaf>}
Texts {text:array<str>}
Row {inner:Data}
fn owned()->array<string>{mut b:array_builder<string>:=array_builder();b.push("x".clone());return b.build()}
fn make()->array<Row>{
 mut b:array_builder<Row>:=array_builder()
 b.push(Row{inner:Data{owned:owned(),records:[Leaf{n:1},Leaf{n:2},Leaf{n:3}].to_array()}})
 return b.build()
}
fn lengths(borrow rows:array<Row>,i:i64)->i64=rows[i].inner.owned.len()+rows[i].inner.records.len()
fn exclusive(borrow mut rows:array<Row>,i:i64)->i64=rows[i].inner.owned.len()
fn matched(borrow maybe:Option<array<Row>>)->i64=match maybe {Some(rows)=>rows[0].inner.records.len(),None=>0}
fn main()->i32{
 mut rows:=make()
 text_rows:=[Texts{text:["a","b"].to_array()}]
 if text_rows[0].text.len()!=2 {return 3}
 if lengths(rows,0)!=4 || exclusive(rows,0)!=1 {return 1}
 maybe:=Some(rows)
 if matched(maybe)!=3 {return 2}
 return 0
}"#, "kinds").unwrap();
    });
}

fn diagnostics(source: &str) -> String {
    let mut sources = SourceMap::new();
    let checked = align_driver::check(&mut sources, "negative.align", source);
    assert!(
        checked.diags.has_errors(),
        "unexpected acceptance: {source}"
    );
    align_driver::format_diagnostics(&sources, &checked.diags)
}

#[test]
fn evaluation_and_control() {
    owned_fixture::run("evaluation_and_control", |stage| {
        let prefix = r#"
R {ids:array<i64>}
Outer {rows:array<R>}
fn fresh()->array<R> {mut b:array_builder<R>:=array_builder();b.push(R{ids:[1,2].to_array()});return b.build()}
fn consume(rows:array<R>)->i64=0
fn replace(borrow mut rows:array<R>)->i64 {rows=fresh();return 0}
fn mutable(borrow mut rows:array<R>)->i64=rows.len()-1
fn step(borrow mut count:i64)->i64 {count=count+1;return 0}
fn fail()->Result<i64,Error> = Err(Error.Invalid)
fn map_error(e:Error)->Error=e
fn attempt(borrow rows:array<R>)->Result<i64,Error> = Ok(rows[fail().map_err(map_error)?].ids.len())
fn early(borrow rows:array<R>)->i64=rows[{return 7;0}].ids.len()
"#;
        for (label, base, setup) in [
            ("dynamic", "rows", "mut rows:=fresh()"),
            ("field", "outer.rows", "mut outer:=Outer{rows:fresh()}"),
        ] {
            for operation in [
                format!("consume({base})"),
                format!("replace({base})"),
                format!("mutable({base})"),
            ] {
                let source = format!(
                    "{prefix}fn main()->i32 {{{setup};return {base}[{operation}].ids.len() as i32}}"
                );
                let errors = diagnostics(&source);
                assert!(
                    errors.contains("invalidated") || errors.contains("moved"),
                    "{label}/{operation}: {errors}"
                );
            }
        }
        for (setup, base, index) in [
            ("mut rows:=fresh();view:=rows[..]", "view", "consume(rows)"),
            (
                "mut rows:=fresh();mut view:=rows[..];other:=fresh()",
                "view",
                "{view=other[..];0}",
            ),
            (
                "mut rows:=[R{ids:[1].to_array()}]",
                "rows",
                "{rows=[R{ids:[2].to_array()}];0}",
            ),
        ] {
            let errors = diagnostics(&format!(
                "{prefix}fn main()->i32 {{{setup};return {base}[{index}].ids.len() as i32}}"
            ));
            assert!(
                errors.contains("invalidated") || errors.contains("moved"),
                "{base}/{index}: {errors}"
            );
        }
        let source = format!(
            r#"{prefix}
fn main()->i32 {{
 mut rows:=fresh();mut other:=fresh();mut count:=0
 if rows[step(count)].ids.len()!=2 || count!=1 {{return 1}}
 if rows[replace(other)].ids.len()!=2 {{return 2}}
 if early(rows)!=7 {{return 3}}
 match attempt(rows) {{Ok(_)=>{{return 4}},Err(_)=>{{}}}}
 n:=if true {{rows[0].ids.len()}}else{{0}}
 m:=match 1 {{1=>rows[0].ids.len(),_=>0}}
 none:Option<i64>:=None
 a:=none else {{rows[0].ids.len()}}
 err:Result<i64,Error>:=Err(Error.Invalid)
 b:=err else {{rows[0].ids.len()}}
 c:=loop {{break rows[0].ids.len()}}
 d:=loop {{unused:=rows[{{break 2;0}}].ids.len();break 0}}
 saved:=rows[0].ids.len();unused:=consume(rows)
 if n+m+a+b+c+d+saved!=14 {{return 5}}
 return 0
}}"#
        );
        execute(stage, &source, "evaluation").unwrap();
        for (name, body) in [
            ("negative", "rows:=fresh();i:=-1;print(rows[i].ids.len())"),
            ("high", "rows:=fresh();i:=1;print(rows[i].ids.len())"),
            (
                "empty",
                "mut b:array_builder<R>:=array_builder();rows:=b.build();print(rows[0].ids.len())",
            ),
            (
                "fixed-negative",
                "rows:=[R{ids:[1].to_array()}];i:=-1;print(rows[i].ids.len())",
            ),
            (
                "fixed-empty",
                "rows:[R;0]:=[];i:=0;print(rows[i].ids.len())",
            ),
        ] {
            let source = format!("{prefix}fn main(){{{body}}}");
            let built = programs(stage, &source, false).unwrap();
            let output = run_programs(stage, &built, name, false).unwrap();
            assert!(!output.status.success(), "{name}: {output:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("bounds"),
                "{name}: {output:?}"
            );
        }
        execute(stage,&format!("import std.process\n{prefix}fn main(){{rows:=fresh();print(rows[process.exit(0)].ids.len())}}"),"exit-index").unwrap();
        for expression in [
            "alias:=rows[0].ids",
            "view:slice<i64>:=rows[0].ids",
        ] {
            let errors = diagnostics(&format!("{prefix}fn main(){{rows:=fresh();{expression}}}"));
            assert!(errors.contains("Move-type field"), "{expression}: {errors}");
        }
    });
}

#[test]
fn allocation_and_drop() {
    owned_fixture::run("allocation_and_drop", |stage| {
        execute(stage, r#"
import core.json
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
R {ids:array<i64>}
fn fresh()->array<R>{mut b:array_builder<R>:=array_builder();b.push(R{ids:[1,2].to_array()});return b.build()}
fn consume(rows:array<R>){}
J {text:string,ids:array<i64>}
fn decoded()->Result<i64,Error> {rows:array<J>:=json.decode("[{\"text\":\"x\",\"ids\":[1,2,3]}]")?;return Ok(rows[0].ids.len())}
fn exercise()->i64 {
 mut rows:=fresh()
 if unsafe{align_rt_requested_live_bytes()}<=0 {return -4}
 before:=unsafe{align_rt_alloc_count()};freed:=unsafe{align_rt_free_count()}
 mut total:=0;mut i:=0
 loop {if i==20 {break};total=total+rows[0].ids.len();i=i+1}
 if unsafe{align_rt_alloc_count()}!=before || unsafe{align_rt_free_count()}!=freed {return -1}
 saved:=rows[0].ids.len()
 rows=fresh();moved:=rows;consume(moved)
 if saved!=2 {return -2}
 arena {
  arena_rows:=[R{ids:[3,4,5].to_array()}]
  total=total+arena_rows[0].ids.len()
 }
 mut empty:array_builder<i64>:=array_builder()
 fixed:=[R{ids:empty.build()}]
 if fixed[0].ids.len()!=0 {return -3}
 return total
}
fn main()->i32 {
 unsafe{align_rt_requested_live_reset()}
 live:=unsafe{align_rt_requested_live_bytes()}
 decoded_length:=decoded() else {return 3}
 if exercise()!=43 || decoded_length!=3 {return 1}
 if unsafe{align_rt_requested_live_bytes()}!=live {return 2}
 return 0
}"#, "lifecycle").unwrap();
    });
}

#[test]
fn imports_and_cache() {
    owned_fixture::run("imports_and_cache", |stage| {
        let module = r#"module rows
pub Row {ids:array<i64>}
pub fn make()->array<Row> {mut b:array_builder<Row>:=array_builder();b.push(Row{ids:[1,2].to_array()});return b.build()}
pub fn count<T>(borrow values:array<Row>,index:i64,marker:T)->i64=values[index].ids.len()
"#;
        let source = "import rows\nfn main()->i32 {values:=rows.make();if rows.count(values,0,true)!=2 || rows.count(values,0,1)!=2 {return 1};return 0}\n";
        std::fs::write(stage.join("rows.align"), module).unwrap();
        execute(stage, source, "imported").unwrap();
        for (label, module) in [
            ("cold", module.to_owned()),
            ("hit", module.to_owned()),
            (
                "type-edit",
                module
                    .replace("array<i64>", "array<i32>")
                    .replace("[1,2]", "[1 as i32,2 as i32]"),
            ),
            ("restore", module.to_owned()),
        ] {
            std::fs::write(stage.join("rows.align"), module).unwrap();
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
                .args(["build", "main.align"])
                .current_dir(stage)
                .env("ALIGNC_CACHE", stage.join("cache"))
                .output()
                .unwrap();
            assert!(output.status.success(), "{label}: {output:?}");
            let output = std::process::Command::new(stage.join("main"))
                .output()
                .unwrap();
            assert!(output.status.success(), "{label}: {output:?}");
        }
    });
}

fn checked_hir(source: &str) -> align_sema::hir::Program {
    let mut sources = SourceMap::new();
    let checked = align_driver::check(&mut sources, "matrix.align", source);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    checked.hir
}
const MATRIX: &str = "pub R {ids:array<i64>}\npub fn size(borrow rows:array<R>,i:i64)->i64=rows[i].ids.len()\nfn main(){}";
fn length_hir(program: &mut align_sema::hir::Program) -> &mut align_sema::hir::Expr {
    program
        .fns
        .iter_mut()
        .find(|f| f.name == "size")
        .unwrap()
        .body
        .value
        .as_deref_mut()
        .unwrap()
}

#[test]
fn indexed_array_field_length_hir_matrix() {
    use align_sema::{Scalar, Ty, hir::ExprKind};
    let original = checked_hir(MATRIX);
    for per_unit in [false, true] {
        align_mir::lower_program_checked(&original, per_unit, None).unwrap();
        for mutation in 0..8 {
            let mut bad = original.clone();
            let length = length_hir(&mut bad);
            let ExprKind::Len(field) = &mut length.kind else {
                panic!("length")
            };
            match mutation {
                0 => {
                    let original = field.as_ref().clone();
                    *length = original;
                }
                1 => {
                    length.ty = Ty::Bool;
                }
                2 => {
                    field.ty = Ty::DynArray(Scalar::Bool);
                }
                3 => {
                    let ExprKind::ElemField { path, .. } = &mut field.kind else {
                        panic!("field")
                    };
                    path.clear();
                }
                4 => {
                    let ExprKind::ElemField { struct_id, .. } = &mut field.kind else {
                        panic!("field")
                    };
                    *struct_id = u32::MAX;
                }
                5 => {
                    let ExprKind::ElemField { index, .. } = &mut field.kind else {
                        panic!("field")
                    };
                    index.kind = ExprKind::Bool(false);
                    index.ty = Ty::Bool;
                }
                6 => {
                    let ExprKind::ElemField { recv, .. } = &mut field.kind else {
                        panic!("field")
                    };
                    recv.kind = ExprKind::Local(u32::MAX);
                }
                _ => {
                    let original = field.as_ref().clone();
                    field.kind = ExprKind::ArrayToSlice(Box::new(original));
                    field.ty = Ty::Slice(Scalar::Int(align_sema::IntTy {
                        bits: 64,
                        signed: true,
                    }));
                }
            }
            // Plan242 makes the explicit full-field view followed by Len valid HIR.
            assert_eq!(
                align_mir::lower_program_checked(&bad, per_unit, None).is_err(),
                mutation != 7,
                "mutation {mutation}/{per_unit}"
            );
        }
    }
}

fn length_operand(program: &mut align_mir::Program) -> &mut align_mir::BorrowedElementPlace {
    use align_mir::{Operand, Rvalue, Stmt};
    for statement in program
        .fns
        .iter_mut()
        .find(|f| f.name.as_str() == "size")
        .unwrap()
        .blocks
        .iter_mut()
        .flat_map(|b| &mut b.stmts)
    {
        if let Stmt::Let(_, Rvalue::SliceLen(Operand::BorrowedElementPlace(place))) = statement {
            return place;
        }
    }
    panic!("missing indexed scalar observer")
}
fn llvm(program: &align_mir::Program) -> Result<String, String> {
    align_codegen_llvm::emit_llvm_ir(
        program,
        &BuildTarget::Baseline,
        Profile::Release,
        false,
        &[],
        None,
    )
    .map_err(|e| e.to_string())
}

#[test]
fn indexed_array_field_length_mir_matrix() {
    use align_mir::{Const, Operand, Rvalue, Stmt, Term};
    use align_sema::{Scalar, Ty};
    let original = align_mir::lower_program_checked(&checked_hir(MATRIX), false, None).unwrap();
    assert!(llvm(&original).is_ok());
    for mutation in 0..14 {
        let mut bad = original.clone();
        let place = length_operand(&mut bad);
        match mutation {
            0 => place.element_ty = Ty::DynArray(Scalar::Bool),
            1 => place.field_path.clear(),
            2 => place.field_path.push(u32::MAX),
            3 => place.base.slot = u32::MAX,
            4 => place.index = Operand::Const(Const::Bool(false)),
            5 => {
                place.guard.len = Operand::Const(Const::Int(
                    1,
                    Ty::Int(align_sema::IntTy {
                        bits: 64,
                        signed: true,
                    }),
                ))
            }
            6 => place.guard.reservation = u32::MAX,
            7 => place.base.ty = Ty::Slice(Scalar::Bool),
            8 => {
                for block in &mut bad
                    .fns
                    .iter_mut()
                    .find(|f| f.name.as_str() == "size")
                    .unwrap()
                    .blocks
                {
                    block
                        .stmts
                        .retain(|s| !matches!(s, Stmt::BorrowedElementReservation { .. }));
                }
            }
            9 => {
                let descriptor = Operand::BorrowedElementPlace(Box::new(place.clone()));
                let f = bad
                    .fns
                    .iter_mut()
                    .find(|f| f.name.as_str() == "size")
                    .unwrap();
                f.ret = length_operand(&mut original.clone()).element_ty;
                for block in &mut f.blocks {
                    if matches!(block.term, Term::Return(_)) {
                        block.term = Term::Return(Some(descriptor.clone()));
                    }
                }
            }
            10 => {
                let descriptor = Operand::BorrowedElementPlace(Box::new(place.clone()));
                let f = bad
                    .fns
                    .iter_mut()
                    .find(|f| f.name.as_str() == "size")
                    .unwrap();
                for statement in f.blocks.iter_mut().flat_map(|b| &mut b.stmts) {
                    if let Stmt::Let(_, r @ Rvalue::SliceLen(Operand::BorrowedElementPlace(_))) =
                        statement
                    {
                        *r = Rvalue::Use(descriptor.clone());
                    }
                }
            }
            11 => {
                let f = bad
                    .fns
                    .iter_mut()
                    .find(|f| f.name.as_str() == "size")
                    .unwrap();
                for block in &mut f.blocks {
                    if matches!(block.term, Term::Return(_)) {
                        block.term = Term::Return(Some(Operand::Const(Const::Int(
                            0,
                            Ty::Int(align_sema::IntTy {
                                bits: 64,
                                signed: true,
                            }),
                        ))));
                    }
                }
                length_operand(&mut bad).element_ty = Ty::DynArray(Scalar::Bool);
            }
            _ => {
                let selected = place.element_ty;
                let descriptor = Operand::BorrowedElementPlace(Box::new(place.clone()));
                let f = bad
                    .fns
                    .iter_mut()
                    .find(|f| f.name.as_str() == "size")
                    .unwrap();
                let slot = u32::try_from(f.slots.len()).unwrap();
                f.slots.push(selected);
                f.slot_align.push(None);
                for block in &mut f.blocks {
                    for statement in &mut block.stmts {
                        if matches!(
                            statement,
                            Stmt::Let(_, Rvalue::SliceLen(Operand::BorrowedElementPlace(_)))
                        ) {
                            *statement = if mutation == 12 {
                                Stmt::Store(slot, descriptor.clone())
                            } else {
                                Stmt::RawFree(descriptor.clone())
                            };
                        }
                    }
                    if matches!(block.term, Term::Return(_)) {
                        block.term = Term::Return(Some(Operand::Const(Const::Int(
                            0,
                            Ty::Int(align_sema::IntTy {
                                bits: 64,
                                signed: true,
                            }),
                        ))));
                    }
                }
            }
        }
        assert!(llvm(&bad).is_err(), "MIR mutation {mutation}");
    }
}
