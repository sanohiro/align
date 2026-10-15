//! Plan205: immediate independent Copy reads retain both bounds and sole parent ownership.
#![cfg(unix)]
#[path = "helpers/indexed_array_field.rs"]
mod indexed_array_field;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_span::SourceMap;
use indexed_array_field::{execute, programs, run_programs};

const PREFIX: &str = r#"
R {ids:array<i64>}
Outer {rows:array<R>}
fn fresh()->array<R> {mut b:array_builder<R>:=array_builder();b.push(R{ids:[7,9].to_array()});return b.build()}
fn consume(rows:array<R>)->i64=0
fn replace(borrow mut rows:array<R>)->i64 {rows=fresh();return 0}
fn mutable(borrow mut rows:array<R>)->i64=rows.len()-1
fn step(borrow mut count:i64)->i64 {count=count+1;return 0}
"#;

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
fn values_and_storage() {
    owned_fixture::run("values_and_storage", |stage| {
        execute(
            stage,
            &format!(
                r#"{PREFIX}
fn shared(borrow rows:array<R>,i:i64,j:i64)->i64=rows[i].ids[j]
fn fixed(borrow rows:[R;1],i:i64,j:i64)->i64=rows[i].ids[j]
fn sliced(rows:slice<R>,i:i64,j:i64)->i64=rows[i].ids[j]
fn main()->i32 {{
 rows:=fresh();i:=0;j:=1
 if rows[i].ids[j]!=9 || shared(rows,i,j)!=9 || sliced(rows[..],i,j)!=9 {{return 1}}
 fixed_rows:=[R{{ids:[3,4,5].to_array()}}]
 if fixed_rows[i].ids[j]!=4 {{return 2}}
 outer:=Outer{{rows:fresh()}}
 if outer.rows[i].ids[j]!=9 {{return 3}}
 return 0
}}"#
            ),
            "values",
        )
        .unwrap();
        execute(stage, r#"
Leaf {n:i64,flag:bool}
Data {records:array<Leaf>,flags:array<bool>,chars:array<char>}
Row {inner:Data}
fn make()->array<Row>{
 mut b:array_builder<Row>:=array_builder()
 b.push(Row{inner:Data{records:[Leaf{n:12,flag:true}].to_array(),flags:[true,false].to_array(),chars:['a','z'].to_array()}})
 return b.build()
}
fn exclusive(borrow mut rows:array<Row>,i:i64)->i64 {leaf:=rows[i].inner.records[0];return leaf.n}
fn matched(borrow maybe:Option<array<Row>>)->i64=match maybe {Some(rows)=>{leaf:=rows[0].inner.records[0];leaf.n},None=>0}
fn main()->i32 {
 mut rows:=make()
 leaf:=rows[0].inner.records[0]
 if leaf.n!=12 || !leaf.flag {return 1}
 if !rows[0].inner.flags[0] || rows[0].inner.flags[1] || rows[0].inner.chars[1]!='z' {return 2}
 if exclusive(rows,0)!=12 {return 3}
 maybe:=Some(rows)
 if matched(maybe)!=12 {return 4}
 return 0
}"#, "records").unwrap();
        execute(stage, r#"
Choice {One(i64),Two(bool)}
Inline {pair:(i64,bool),maybe:Option<i64>,result:Result<i64,Error>,bytes:[i64;2],choice:Choice}
Row {items:array<Inline>}
fn copied()->Inline {
 arena {
  rows:=[Row{items:[Inline{pair:(3,false),maybe:Some(4),result:Ok(5),bytes:[6,7],choice:Choice.One(8)}].to_array()}]
  return rows[0].items[0]
 }
}
fn main()->i32 {
 leaf:=copied()
 if leaf.pair.0!=3 || (leaf.maybe else {0})!=4 || (leaf.result else {0})!=5 || leaf.bytes[1]!=7 {return 1}
 value:=match leaf.choice {One(n)=>n,Two(_)=>0}
 return if value==8 {0}else{2}
}"#, "inline-copy").unwrap();
        // Source qualification checks every reachable result leaf, including inactive views.
        for (declaration, field) in [
            ("V {value:str}", "array<V>"),
            ("V {value:Option<str>}", "array<V>"),
            ("V {value:Result<i64,str>}", "array<V>"),
            ("V {value:string}", "array<V>"),
            ("", "array<str>"),
            ("", "array<string>"),
        ] {
            let errors = diagnostics(&format!("{declaration}\nRows {{items:{field}}}\nfn read(borrow rows:array<Rows>) {{unused:=rows[0].items[0]}}\nfn main(){{}}"));
            assert!(
                errors.contains("Move-type field"),
                "{field}/{declaration}: {errors}"
            );
        }
    });
}

#[test]
fn evaluation_and_control() {
    owned_fixture::run("evaluation_and_control", |stage| {
        for (setup, base) in [
            ("mut rows:=fresh()", "rows"),
            ("mut outer:=Outer{rows:fresh()}", "outer.rows"),
        ] {
            for operation in [
                format!("consume({base})"),
                format!("replace({base})"),
                format!("mutable({base})"),
            ] {
                for (outer, inner) in [(operation.as_str(), "0"), ("0", operation.as_str())] {
                    let errors=diagnostics(&format!("{PREFIX}fn main()->i32 {{{setup};return {base}[{outer}].ids[{inner}] as i32}}"));
                    assert!(
                        errors.contains("invalidated") || errors.contains("moved"),
                        "{errors}"
                    );
                }
            }
        }
        for (setup, base, effect) in [
            ("rows:=fresh();view:=rows[..]", "view", "consume(rows)"),
            (
                "rows:=fresh();mut view:=rows[..];other:=fresh()",
                "view",
                "{view=other[..];0}",
            ),
            (
                "mut rows:=[R{ids:[7].to_array()}]",
                "rows",
                "{rows=[R{ids:[9].to_array()}];0}",
            ),
        ] {
            for (outer, inner) in [(effect, "0"), ("0", effect)] {
                let errors = diagnostics(&format!(
                    "{PREFIX}fn main()->i32 {{{setup};return {base}[{outer}].ids[{inner}] as i32}}"
                ));
                assert!(
                    errors.contains("invalidated") || errors.contains("moved"),
                    "{errors}"
                );
            }
        }
        execute(
            stage,
            &format!(
                r#"{PREFIX}
fn fail()->Result<i64,Error> =Err(Error.Invalid)
fn map_error(e:Error)->Error=e
fn attempt(borrow rows:array<R>)->Result<i64,Error> =Ok(rows[0].ids[fail().map_err(map_error)?])
fn outer_early(borrow rows:array<R>)->i64=rows[{{return 21;0}}].ids[0]
fn inner_early(borrow rows:array<R>)->i64=rows[0].ids[{{return 22;0}}]
fn main()->i32 {{
 rows:=fresh();mut other:=fresh();mut count:=0
 if rows[step(count)].ids[{{if count!=1 {{return 1}};step(count)}}]!=7 || count!=2 {{return 2}}
 if rows[replace(other)].ids[replace(other)]!=7 {{return 3}}
 if outer_early(rows)!=21 || inner_early(rows)!=22 {{return 4}}
 match attempt(rows) {{Ok(_)=>{{return 5}},Err(_)=>{{}}}}
 n:=if true {{rows[0].ids[1]}}else{{0}}
 m:=match 1 {{1=>rows[0].ids[1],_=>0}}
 none:Option<i64>:=None;a:=none else {{rows[0].ids[1]}}
 err:Result<i64,Error>:=Err(Error.Invalid);b:=err else {{rows[0].ids[1]}}
 c:=loop {{break rows[0].ids[1]}}
 d:=loop {{unused:=rows[0].ids[{{break 9;0}}];break 0}}
 saved:=rows[0].ids[1];unused:=consume(rows)
 if n+m+a+b+c+d+saved!=63 {{return 6}}
 return 0
}}"#
            ),
            "evaluation",
        )
        .unwrap();
        for (name, setup, outer, inner) in [
            ("outer-negative", "rows:=fresh()", "-1", "{print(99);0}"),
            ("outer-high", "rows:=fresh()", "1", "{print(99);0}"),
            (
                "outer-empty",
                "mut b:array_builder<R>:=array_builder();rows:=b.build()",
                "0",
                "{print(99);0}",
            ),
            ("fixed-empty", "rows:[R;0]:=[]", "0", "{print(99);0}"),
            ("inner-negative", "rows:=fresh()", "0", "-1"),
            ("inner-high", "rows:=fresh()", "0", "2"),
            (
                "inner-empty",
                "mut b:array_builder<i64>:=array_builder();rows:=[R{ids:b.build()}]",
                "0",
                "0",
            ),
        ] {
            let source = format!("{PREFIX}fn main(){{{setup};print(rows[{outer}].ids[{inner}])}}");
            let built = programs(stage, &source, false).unwrap();
            let output = run_programs(stage, &built, name, false).unwrap();
            assert!(
                !output.status.success()
                    && output.stdout.is_empty()
                    && String::from_utf8_lossy(&output.stderr).contains("bounds"),
                "{name}: {output:?}"
            );
        }
        for (label, outer, inner) in [
            ("outer-exit", "process.exit(0)", "0"),
            ("inner-exit", "0", "process.exit(0)"),
        ] {
            execute(stage,&format!("import std.process\n{PREFIX}fn main(){{rows:=fresh();print(rows[{outer}].ids[{inner}])}}"),label).unwrap();
        }
        for expression in [
            "alias:=rows[0].ids",
            "unused:=fresh()[0].ids[0]",
            "view:=rows[0].ids[..]",
            "unused:=rows[0].ids.map(fn x { x }).to_array()",
        ] {
            let errors = diagnostics(&format!("{PREFIX}fn main(){{rows:=fresh();{expression}}}"));
            assert!(
                errors.contains("Move-type field")
                    || errors.contains("indexing an array of the Move type"),
                "{expression}: {errors}"
            );
        }
    });
}

#[test]
fn allocation_and_drop() {
    owned_fixture::run("allocation_and_drop", |stage| {
        execute(stage,&format!(r#"
import core.json
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
{PREFIX}
Leaf {{n:i64,maybe:Option<i64>}}
RecordRow {{items:array<Leaf>}}
fn record_rows(n:i64)->array<RecordRow> {{mut b:array_builder<RecordRow>:=array_builder();b.push(RecordRow{{items:[Leaf{{n:n,maybe:Some(2)}}].to_array()}});return b.build()}}
fn consume_records(rows:array<RecordRow>) {{}}
fn exercise()->i64 {{
 mut rows:=fresh()
 mut records:=record_rows(13)
 if unsafe{{align_rt_requested_live_bytes()}}<=0 {{return -4}}
 before:=unsafe{{align_rt_alloc_count()}};freed:=unsafe{{align_rt_free_count()}}
 mut total:=0;mut i:=0
 loop {{if i==20 {{break}};total=total+rows[0].ids[1];i=i+1}}
 leaf:=records[0].items[0]
 if unsafe{{align_rt_alloc_count()}}!=before || unsafe{{align_rt_free_count()}}!=freed {{return -1}}
 records=record_rows(99);consume_records(records)
 saved:=rows[0].ids[1];rows=fresh();moved:=rows;unused:=consume(moved)
 if saved!=9 || leaf.n!=13 || (leaf.maybe else {{0}})!=2 {{return -2}}
 arena {{arena_rows:=[R{{ids:[3,4,5].to_array()}}];total=total+arena_rows[0].ids[2]}}
 return total
}}
fn decoded()->Result<i64,Error> {{rows:array<R>:=json.decode("[{{\"ids\":[1,2,3]}}]")?;return Ok(rows[0].ids[2])}}
fn main()->i32 {{
 unsafe{{align_rt_requested_live_reset()}};live:=unsafe{{align_rt_requested_live_bytes()}}
 decoded_value:=decoded() else {{return 3}}
 if exercise()!=185 || decoded_value!=3 {{return 1}}
 if unsafe{{align_rt_requested_live_bytes()}}!=live {{return 2}}
 return 0
}}"#),"lifecycle").unwrap();
    });
}

#[test]
fn imports_and_cache() {
    owned_fixture::run("imports_and_cache", |stage| {
        let module = r#"module rows
pub Leaf {value:i64}
pub Row {ids:array<i64>,items:array<Leaf>}
pub fn make()->array<Row> {mut b:array_builder<Row>:=array_builder();b.push(Row{ids:[7,9].to_array(),items:[Leaf{value:11}].to_array()});return b.build()}
pub fn read<T>(borrow values:array<Row>,i:i64,j:i64,marker:T)->i64=values[i].ids[j] as i64
pub fn copied<T>(borrow values:array<Row>,i:i64,j:i64,marker:T)->Leaf=values[i].items[j]
"#;
        let source="import rows\nfn main()->i32 {values:=rows.make();a:=rows.copied(values,0,0,true);b:=rows.copied(values,0,0,1);if rows.read(values,0,1,true)!=9 || rows.read(values,0,0,1)!=7 || a.value!=11 || b.value!=11 {return 1};return 0}\n";
        std::fs::write(stage.join("rows.align"), module).unwrap();
        execute(stage, source, "imported").unwrap();
        for (label, module) in [
            ("cold", module.to_owned()),
            ("hit", module.to_owned()),
            (
                "type-edit",
                module
                    .replace("array<i64>", "array<i32>")
                    .replace("[7,9]", "[7 as i32,9 as i32]"),
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

fn llvm(program: &align_mir::Program) -> Result<String, String> {
    align_codegen_llvm::emit_llvm_ir(
        program,
        &align_driver::BuildTarget::Baseline,
        align_driver::Profile::Release,
        false,
        &[],
        None,
    )
    .map_err(|error| error.to_string())
}

#[test]
fn producer_and_bounds() {
    use align_mir::{Const, Operand, Rvalue, Stmt, Term};
    use align_sema::{IntTy, Scalar, Ty};
    owned_fixture::run("producer_and_bounds", |stage| {
        let source="pub R {ids:array<i64>,other:array<i64>}\npub fn read(borrow rows:array<R>,i:i64,j:i64)->i64=rows[i].ids[j]\nfn main(){}";
        let original = programs(stage, source, false).unwrap().remove(0);
        assert!(llvm(&original).is_ok());
        let i64_ty = Ty::Int(IntTy {
            bits: 64,
            signed: true,
        });
        let zero = Operand::Const(Const::Int(0, i64_ty));
        let mut accepted = Vec::new();
        for unused in [false, true] {
            for mutation in 0..27 {
                let mut bad = original.clone();
                let function = bad
                    .fns
                    .iter_mut()
                    .find(|f| f.name.as_str() == "read")
                    .unwrap();
                let (action_block, action_statement, value, descriptor, inner_index) = function
                    .blocks
                    .iter()
                    .find_map(|block| {
                        block.stmts.iter().enumerate().find_map(|(position, stmt)| {
                            if let Stmt::Let(
                                value,
                                Rvalue::SliceIndex(Operand::BorrowedElementPlace(place), index),
                            ) = stmt
                            {
                                Some((
                                    block.id as usize,
                                    position,
                                    *value,
                                    place.as_ref().clone(),
                                    index.clone(),
                                ))
                            } else {
                                None
                            }
                        })
                    })
                    .unwrap();
                let root = descriptor.base.slot;
                let mut place = descriptor.clone();
                match mutation {
                    0 => place.element_ty = Ty::DynArray(Scalar::Bool),
                    1 => place.field_path.clear(),
                    2 => place.field_path.push(u32::MAX),
                    3 => place.base.slot = u32::MAX,
                    4 => place.index = Operand::Const(Const::Bool(false)),
                    5 => place.guard.len = zero.clone(),
                    6 => place.guard.reservation = u32::MAX,
                    7 => place.base.ty = Ty::Slice(Scalar::Bool),
                    _ => {}
                }
                function.blocks[action_block].stmts[action_statement] = Stmt::Let(
                    value,
                    Rvalue::SliceIndex(Operand::BorrowedElementPlace(Box::new(place)), inner_index),
                );
                match mutation {
                    0..=7 => {}
                    8 => {
                        for block in &mut function.blocks {
                            block
                                .stmts
                                .retain(|s| !matches!(s, Stmt::BorrowedElementReservation { .. }));
                        }
                    }
                    9 => function.value_tys[value as usize] = Ty::Bool,
                    10 => {
                        if let Stmt::Let(_, Rvalue::SliceIndex(_, index)) =
                            &mut function.blocks[action_block].stmts[action_statement]
                        {
                            *index = zero.clone();
                        }
                    }
                    11 => {
                        for stmt in function.blocks.iter_mut().flat_map(|b| &mut b.stmts) {
                            if let Stmt::Let(
                                _,
                                rvalue @ Rvalue::SliceLen(Operand::BorrowedElementPlace(_)),
                            ) = stmt
                            {
                                *rvalue = Rvalue::Use(zero.clone());
                            }
                        }
                    }
                    // Bypass each level's guard independently, keeping the load and producer valid.
                    12 | 13 => {
                        let guards = function
                            .blocks
                            .iter()
                            .enumerate()
                            .filter_map(|(i, b)| {
                                if matches!(b.term, Term::Branch(..)) {
                                    Some(i)
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>();
                        let block = &mut function.blocks[guards[mutation - 12]];
                        let Term::Branch(_, _, success) = block.term else {
                            unreachable!()
                        };
                        block.term = Term::Goto(success);
                    }
                    14 => function.blocks[action_block]
                        .stmts
                        .insert(action_statement, Stmt::Drop(root)),
                    15 => {
                        let descriptor =
                            Operand::BorrowedElementPlace(Box::new(descriptor.clone()));
                        function.blocks[action_block].stmts[action_statement] =
                            Stmt::Let(value, Rvalue::Use(descriptor));
                    }
                    16 => {
                        let descriptor =
                            Operand::BorrowedElementPlace(Box::new(descriptor.clone()));
                        function.blocks[action_block]
                            .stmts
                            .insert(action_statement, Stmt::DropValue(descriptor));
                    }
                    17 => {
                        let slot = u32::try_from(function.slots.len()).unwrap();
                        function.slots.push(descriptor.element_ty);
                        function.slot_align.push(None);
                        function.blocks[action_block].stmts.insert(
                            action_statement,
                            Stmt::Store(
                                slot,
                                Operand::BorrowedElementPlace(Box::new(descriptor.clone())),
                            ),
                        );
                    }
                    18 => {
                        function.ret = descriptor.element_ty;
                        for block in &mut function.blocks {
                            if matches!(block.term, Term::Return(_)) {
                                block.term = Term::Return(Some(Operand::BorrowedElementPlace(
                                    Box::new(descriptor.clone()),
                                )));
                            }
                        }
                    }
                    19 => {
                        for block in &mut function.blocks {
                            if let Some(position) = block.stmts.iter().position(|s| {
                                matches!(
                                    s,
                                    Stmt::Let(
                                        _,
                                        Rvalue::SliceLen(Operand::BorrowedElementPlace(_))
                                    )
                                )
                            }) {
                                block.stmts.insert(position + 1, Stmt::Drop(root));
                                break;
                            }
                        }
                    }
                    20 => {
                        // Even a matching scalar result cannot hide a view-bearing physical field.
                        let Ty::DynStructArray(id, _) = descriptor.base.ty else {
                            unreachable!()
                        };
                        bad.structs[id as usize].fields[0].ty = Ty::DynArray(Scalar::Str);
                    }
                    21 => {
                        for block in &mut function.blocks {
                            if let Some(position) = block
                                .stmts
                                .iter()
                                .position(|s| matches!(s, Stmt::BorrowedElementReservation { .. }))
                            {
                                block.stmts.insert(position + 1, Stmt::Drop(root));
                                break;
                            }
                        }
                    }
                    22 => {
                        let Stmt::Let(_, Rvalue::SliceIndex(slice, index)) =
                            function.blocks[action_block].stmts[action_statement].clone()
                        else {
                            unreachable!()
                        };
                        function.blocks[action_block].stmts[action_statement] = Stmt::Let(
                            value,
                            Rvalue::SliceIndexNoalias {
                                slice,
                                index,
                                scope: 0,
                            },
                        );
                    }
                    23 => {
                        let guard = function
                            .blocks
                            .iter_mut()
                            .rev()
                            .find(|b| matches!(b.term, Term::Branch(..)))
                            .unwrap();
                        let Term::Branch(condition, failure, success) = guard.term.clone() else {
                            unreachable!()
                        };
                        guard.term = Term::Branch(condition, success, failure);
                    }
                    24 => {
                        // Same physical array/element types, but a different selected header.
                        if let Stmt::Let(
                            _,
                            Rvalue::SliceIndex(Operand::BorrowedElementPlace(place), _),
                        ) = &mut function.blocks[action_block].stmts[action_statement]
                        {
                            place.field_path[0] = 1;
                        }
                    }
                    _ => {
                        // Keep the canonical Branch and its comparisons, but reach its success
                        // without executing that guard. Length/index SSA still dominate their
                        // real uses, so LLVM verification is not the owner of this rejection.
                        let guard_index = function
                            .blocks
                            .iter()
                            .enumerate()
                            .filter_map(|(i, block)| {
                                matches!(block.term, Term::Branch(..)).then_some(i)
                            })
                            .nth(mutation - 25)
                            .unwrap();
                        let split = function.blocks[guard_index]
                            .stmts
                            .iter()
                            .position(|stmt| {
                                if mutation == 25 {
                                    matches!(
                                        stmt,
                                        Stmt::Let(_, Rvalue::SliceLen(Operand::BorrowedPlace(_)))
                                    )
                                } else {
                                    matches!(
                                        stmt,
                                        Stmt::Let(_, Rvalue::Bin(align_ast::BinOp::Lt, _, _))
                                    )
                                }
                            })
                            .unwrap();
                        let mut guard = function.blocks[guard_index].clone();
                        guard.stmts = function.blocks[guard_index].stmts.split_off(split);
                        let Term::Branch(_, _, success) = guard.term else {
                            unreachable!()
                        };
                        function.blocks[guard_index].term = Term::Unreachable;
                        let inserted = u32::try_from(guard_index + 1).unwrap();
                        function.blocks.insert(guard_index + 1, guard);
                        let shift = |target: &mut u32| {
                            if *target >= inserted {
                                *target += 1;
                            }
                        };
                        for (id, block) in function.blocks.iter_mut().enumerate() {
                            block.id = u32::try_from(id).unwrap();
                            match &mut block.term {
                                Term::Goto(target) => shift(target),
                                Term::Branch(_, failure, success) => {
                                    shift(failure);
                                    shift(success);
                                }
                                _ => {}
                            }
                        }
                        for edge in &mut function.exceptional_edges {
                            if edge.block == u32::try_from(guard_index).unwrap() {
                                edge.block = inserted;
                            } else {
                                shift(&mut edge.block);
                            }
                        }
                        let mut success = success;
                        shift(&mut success);
                        function.blocks[guard_index].term =
                            Term::Branch(Operand::Const(Const::Bool(false)), inserted, success);
                    }
                }
                if unused {
                    for block in &mut function.blocks {
                        if matches!(block.term, Term::Return(_)) {
                            block.term = Term::Return(Some(zero.clone()));
                        }
                    }
                }
                if llvm(&bad).is_ok() {
                    accepted.push((mutation, unused));
                }
            }
        }
        assert!(accepted.is_empty(), "accepted malformed MIR: {accepted:?}");
    });
}
