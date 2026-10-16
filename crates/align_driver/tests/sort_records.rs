//! Independent Copy records use the existing stable comparator and complete AoS storage.
#![cfg(unix)]
mod common;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use common::*;

const MODEL: &str = r#"module model
import std.rand
pub Meta<T> { value:T, fixed:[i64;3], pair:(i64,bool) }
pub Choice { Value(i64), Empty }
pub Row { id:i64, key:i64, small:u8, flag:bool, fraction:f32, meta:Meta<i64>, optional:Option<i64>, result:Result<i64,Error>, choice:Choice, random:rng }
pub fn identity<T>(value:T)->T = value
pub fn make(id:i64,key:i64)->Row = Row { id:id,key:key,small:251,flag:id%2==0,fraction:1.5,meta:Meta {value:id+11,fixed:[id,id+1,id+2],pair:(id+3,true)},optional:if id%2==0 {Some(id)} else {None},result:if id%2==0 {Ok(id)} else {Err(Error.Invalid)},choice:if id%2==0 {Choice.Value(id)} else {Choice.Empty},random:rand.seed_with(id) }
pub fn before(a:Row,b:Row)->bool = identity(a.key) < identity(b.key)
pub fn valid(row:Row)->bool {
  mut random:=row.random
  mut reference:=rand.seed_with(row.id)
  optional:=row.optional else -1
  result:=match row.result {Ok(value)=>value,Err(error)=>-1}
  choice:=match row.choice {Value(value)=>value,Empty=>-1}
  expected:=if row.id%2==0 {row.id} else {-1}
  return row.small==251 && row.flag==(row.id%2==0) && row.fraction==1.5 && row.meta.value==row.id+11 && row.meta.fixed[0]==row.id && row.meta.fixed[1]==row.id+1 && row.meta.fixed[2]==row.id+2 && row.meta.pair.0==row.id+3 && row.meta.pair.1 && optional==expected && result==expected && choice==expected && random.next()==reference.next()
}
pub fn agrees(keys:slice<i64>,expected:slice<i64>)->bool {
  mut builder:array_builder<i64>:=array_builder()
  mut i:=0
  loop {if i==keys.len() {break};builder.push(i);i=i+1}
  indices:=builder.build()
  source:=indices.map(fn index:i64 {make(index,keys[index])}).to_array()
  sorted:=source.sort_by(before)
  if sorted.len()!=expected.len() {return false}
  i=0
  loop {if i==sorted.len() {break};row:=sorted[i];if row.id!=expected[i] || row.key!=keys[row.id] || !valid(row) || source[i].id!=i {return false};i=i+1}
  return true
}
pub fn permutation(n:i64,cyclic:bool)->bool {
  mut builder:array_builder<i64>:=array_builder()
  mut i:=0
  loop {if i==n {break};builder.push(i);i=i+1}
  indices:=builder.build()
  source:=indices.map(fn index:i64 {make(index,index)}).to_array()
  sorted:=source.sort_by(fn a:Row,b:Row {if cyclic {(a.id%3+1)%3==b.id%3} else {true}})
  if sorted.len()!=n {return false}
  i=0
  loop {
    if i==n {break}
    mut occurrences:=0
    mut j:=0
    loop {if j==n {break};row:=sorted[j];if !valid(row) {return false};if row.id==i {occurrences=occurrences+1};j=j+1}
    if occurrences!=1 {return false}
    i=i+1
  }
  return true
}
"#;

fn view(values: &[i64]) -> String {
    if values.is_empty() {
        return "[0][0..0]".into();
    }
    format!(
        "[{}][..]",
        values
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn assert_output(output: std::process::Output, expected: &str) {
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn comparator_record_order_layout_and_replay() {
    owned_fixture::run("comparator_record_order_layout_and_replay", |_| {
        if !backend_available() {
            return;
        }
        let mut source = String::from(
            "module main\nimport model\nlayout(C) Native {small:u8,wide:i64}\nFlat {key:i64,id:i64}\nWrap {row:Flat}\nSpan {start:i64,end:i64,id:i64}\nView {text:str,id:i64}\nfn main() {\n",
        );
        let mut expected = String::new();
        for n in [0, 1, 31, 32, 33, 63, 64, 65, 129] {
            let random: Vec<i64> = (0..n).map(|i| (i * 37 + 11) % 19 - 9).collect();
            let mut ordered = random.clone();
            ordered.sort();
            for keys in [random, ordered.clone(), ordered.into_iter().rev().collect()] {
                let mut indices: Vec<i64> = (0..n).collect();
                indices.sort_by_key(|i| keys[*i as usize]);
                source.push_str(&format!(
                    "print(model.agrees({},{}))\n",
                    view(&keys),
                    view(&indices)
                ));
                expected.push_str("true\n");
            }
            for cyclic in [false, true] {
                source.push_str(&format!("print(model.permutation({n},{cyclic}))\n"));
                expected.push_str("true\n");
            }
        }
        source.push_str(r#"
  fixed:=[Flat {key:2,id:0},Flat {key:1,id:1},Flat {key:2,id:2}]
  direct:=fixed.sort_by(fn a:Flat,b:Flat {a.key<b.key})
  sliced:=fixed[..].sort_by(fn a:Flat,b:Flat {a.key<b.key})
  multi:=fixed.sort_by(fn a:Flat,b:Flat {if a.key==b.key {a.id>b.id} else {a.key<b.key}})
  print(multi[0].id==1 && multi[1].id==2 && multi[2].id==0)
  print(direct[0].id==1 && direct[1].id==0 && direct[2].id==2)
  print(sliced[0].id==1 && sliced[1].id==0 && sliced[2].id==2)
  mapped:=[2,1,3].map(fn n:i64 {Flat {key:n,id:n+10}}).where(fn row:Flat {row.key>1}).sort_by(fn a:Flat,b:Flat {a.key>b.key})
  print(mapped.len()==2 && mapped[0].id==13 && mapped[1].id==12)
  wraps:=[Wrap {row:Flat {key:2,id:20}},Wrap {row:Flat {key:1,id:10}}]
  projected:=wraps.row.sort_by(fn a:Flat,b:Flat {a.key<b.key})
  print(projected[0].id==10 && projected[1].id==20)
  natives:=[Native {small:251,wide:9},Native {small:7,wide:-5}]
  native:=natives.sort_by(fn a:Native,b:Native {a.wide<b.wide})
  print(native[0].small==7 && native[0].wide== -5 && native[1].small==251 && native[1].wide==9)
  text:="zab"
  spans:=[Span {start:0,end:1,id:0},Span {start:2,end:3,id:2},Span {start:1,end:2,id:1}]
  by_text:=spans.sort_by(fn a:Span,b:Span {text[a.start..a.end]<text[b.start..b.end]})
  print(by_text[0].id==1 && by_text[1].id==2 && by_text[2].id==0)
  views:=[View {text:"long",id:0},View {text:"x",id:1}]
  lengths:=views.map(fn row:View {Flat {key:row.text.len(),id:row.id}}).sort_by(fn a:Flat,b:Flat {a.key<b.key})
  print(lengths[0].id==1 && lengths[1].id==0)
  ids:=fixed.id.sort_by(fn a:i64,b:i64 {a>b})
  print(ids[0]==2 && ids[2]==0)
  arena {
    columns:=fixed.to_soa()
    transposed:=columns.sort_by(fn a:Flat,b:Flat {a.key<b.key})
    print(transposed[0].id==1 && transposed[1].id==0 && transposed[2].id==2)
  }
}
"#);
        expected.push_str(&"true\n".repeat(10));
        let files = [("main.align", source.as_str()), ("model.align", MODEL)];
        assert_output(
            build_and_run_multi("record-sort-whole", &files, "main.align"),
            &expected,
        );
        assert_output(
            build_per_unit_multi("record-sort-units", &files, "main.align").link_and_run(),
            &expected,
        );
        let project = Proj::new("record-sort-cache", &files, "main.align");
        let entry = project.dir.join("main.align");
        let cache = project.cache();
        for round in 0..2 {
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry.to_str().unwrap(),
                &source,
                &cache,
                align_driver::UnitReuse::Allowed,
            );
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &built.diags)
            );
            let mut objects = Vec::new();
            let mut libraries = Vec::new();
            for index in 0..built.units.len() {
                assert_eq!(
                    built.units[index].frontend.as_ref().unwrap().hit,
                    round == 1
                );
                let mir = built.materialize(index).unwrap();
                if round == 1 {
                    let object = project.dir.join(format!("cached-{index}.o"));
                    emit_object_file(
                        mir,
                        &object,
                        BuildTarget::Baseline,
                        Profile::Release,
                        &[],
                        false,
                    )
                    .unwrap();
                    objects.push(object);
                    for library in &mir.link_libs {
                        if !libraries.contains(library) {
                            libraries.push(library.clone());
                        }
                    }
                }
            }
            if round == 1 {
                let binary = project.dir.join("cache-hit");
                link_objects(
                    &align_driver::CDriver::default(),
                    &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
                    &binary,
                    &libraries,
                    Profile::Release,
                )
                .unwrap();
                assert_output(
                    std::process::Command::new(binary).output().unwrap(),
                    &expected,
                );
            }
        }
    });
}

const COUNTERS: &str = r#"extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
fn allocated()->i64 = unsafe {align_rt_alloc_count()}
fn freed()->i64 = unsafe {align_rt_free_count()}
fn live()->i64 = unsafe {align_rt_requested_live_bytes()}
"#;
const _: extern "C" fn() -> i64 = align_runtime::align_rt_alloc_count;
const _: extern "C" fn() -> i64 = align_runtime::align_rt_requested_live_bytes;

#[test]
fn comparator_record_scratch_and_drop() {
    owned_fixture::run("comparator_record_scratch_and_drop", |_| {
        if !backend_available() {
            return;
        }
        let mut source = format!("{COUNTERS}\nRow {{key:i64,tag:u8}}\nZero {{empty:[i64;0]}}\n");
        source.push_str(
            r#"
fn before(a:Row,b:Row)->bool = a.key<b.key
fn exercise(xs:slice<Row>,region:bool)->i64 {
  if region {return arena {sorted:=xs.sort_by(before);if sorted.len()==0 {0} else {sorted[0].key}}}
  sorted:=xs.sort_by(before)
  if sorted.len()==0 {return 0}
  return sorted[0].key
}
fn measure(xs:slice<Row>,region:bool,count:i64) {
  a:=allocated();f:=freed();bytes:=live()
  result:=exercise(xs,region)
  print(result==0);print(allocated()-a==count);print(freed()-f==count);print(live()==bytes)
}
fn move_out(xs:slice<Row>)->array<Row> = xs.sort_by(before)
fn lifecycle(xs:slice<Row>) {
  mut replacement:=move_out(xs)
  replacement=move_out(xs)
  moved:=replacement
  print(moved[0].key==0 && moved[0].tag==251 && xs[0].key==64)
  loop {discarded:=move_out(xs);print(discarded.len()==65);break}
}
fn zero_before(a:Zero,b:Zero)->bool {print(a.empty.len()==0 && b.empty.len()==0);return false}
fn zero(xs:slice<Zero>,region:bool) {
  if region {arena {sorted:=xs.sort_by(zero_before);print(sorted.len()==xs.len())};return}
  sorted:=xs.sort_by(zero_before);print(sorted.len()==xs.len())
}
fn main() {
  unsafe {align_rt_requested_live_reset()}
"#,
        );
        let rows = (0..65)
            .rev()
            .map(|i| format!("Row {{key:{i},tag:251}}"))
            .collect::<Vec<_>>()
            .join(",");
        source.push_str(&format!("rows:=[{rows}]\n"));
        let mut expected = String::new();
        for n in [0, 1, 32, 33, 65] {
            for region in [false, true] {
                // Arena handles/chunks use their existing allocator, not the owned-array counter.
                let count = usize::from(n > 0 && !region) + usize::from(n > 32);
                source.push_str(&format!("measure(rows[{}..65],{region},{count})\n", 65 - n));
                expected.push_str(&"true\n".repeat(4));
            }
        }
        source.push_str("a:=allocated();f:=freed();lifecycle(rows[..]);print(allocated()-a==6 && freed()-f==6 && live()==0)\n");
        expected.push_str("true\ntrue\ntrue\n");
        let zeros = vec!["Zero {empty:[]}"; 65].join(",");
        source.push_str(&format!("zeros:=[{zeros}]\nza:=allocated();zf:=freed()\n"));
        for (n, calls) in [(0, 0), (1, 0), (32, 31), (33, 63), (65, 158)] {
            for region in [false, true] {
                source.push_str(&format!("zero(zeros[0..{n}],{region})\n"));
                expected.push_str(&"true\n".repeat(calls + 1));
            }
        }
        source.push_str("print(allocated()==za && freed()==zf && live()==0)\n}\n");
        expected.push_str("true\n");
        assert_output(build_and_run("record-sort-allocation", &source), &expected);
    });
}

#[test]
fn comparator_record_control_lifetimes_and_effects() {
    owned_fixture::run("comparator_record_control_lifetimes_and_effects", |_| {
        let record = "Row {key:i64,tag:i64}\nfn before(a:Row,b:Row)->bool = a.key<b.key\n";
        for source in [
            "fn bad()->array<Row> = arena {rows:=[Row {key:1,tag:2}];rows.sort_by(before)}\nfn main() {}",
            "fn main() {mut source:=[Row {key:1,tag:2}].to_array();sorted:=source.sort_by(fn a:Row,b:Row {source[0].key=a.key;a.key<b.key});print(sorted[0].key)}",
            "fn noisy(a:Row,b:Row)->bool {print(b.tag);return a.key<b.key}\nfn worker(n:i64)->i64 {rows:=[Row {key:n,tag:0},Row {key:0,tag:n}];sorted:=rows.sort_by(noisy);return sorted[0].tag}\nfn main() {print([1,2].par_map(worker).sum())}",
        ] {
            let source = format!("{record}{source}");
            let checked = diff_check_multi(
                "record-sort-invalid-control",
                &[("main.align", &source)],
                "main.align",
            );
            assert!(
                checked.whole_errors && checked.per_unit_errors,
                "{source}\n{}\n{}",
                checked.whole_diags,
                checked.per_unit_diags
            );
        }
        for declaration in [
            "Row {value:str}",
            "Row {value:string}",
            "Row {value:[str;0]}",
            "Choice {Text(str),Empty}\nRow {value:Choice}",
            "Row {value:fn(i64)->i64}",
            "layout(C) Row {value:raw}",
            "align(32) Row {value:i64}",
        ] {
            let source = format!(
                "{declaration}\nfn bad(xs:slice<Row>) {{sorted:=xs.sort_by(fn a:Row,b:Row {{true}})}}\nfn main() {{}}"
            );
            let checked = diff_check_multi(
                "record-sort-invalid-domain",
                &[("main.align", &source)],
                "main.align",
            );
            assert!(
                checked.whole_errors && checked.per_unit_errors,
                "{source}\n{}\n{}",
                checked.whole_diags,
                checked.per_unit_diags
            );
        }
        let source = format!(
            r#"{record}
fn sorted(xs:slice<Row>)->array<Row> = xs.sort_by(before)
fn result(xs:slice<Row>,fail:bool)->Result<array<Row>,Error> {{if fail {{return Err(Error.Invalid)}};return Ok(sorted(xs))}}
fn independent()->array<Row> {{owner:="local".clone();view:str:=owner;rows:=[Row {{key:2,tag:20}},Row {{key:1,tag:10}}];return rows.sort_by(fn a:Row,b:Row {{if view.len()>0 {{a.key<b.key}} else {{a.key>b.key}}}})}}
fn early(flag:bool)->i64 {{rows:=[Row {{key:1,tag:2}}];sorted:=(if flag {{return 7}} else {{rows[..]}}).sort_by(before);return sorted[0].key}}
fn main()->Result<(),Error> {{
  rows:=[Row {{key:2,tag:20}},Row {{key:1,tag:10}}]
  selected:=if true {{sorted(rows[..])}} else {{sorted(rows[..])}}
  tried:=result(rows[..],false).map_err(fn e:Error {{e}})?
  fallback:=result(rows[..],false) else {{return Err(Error.Invalid)}}
  optional:Option<array<Row>>:=Some(sorted(rows[..]))
  matched:=match optional {{Some(value)=>value,None=>sorted(rows[..])}}
  looped:=loop {{break sorted(rows[..])}}
  local:=independent()
  mut original:=[Row {{key:1,tag:2}}].to_array()
  copied:=original.sort_by(before)
  original=[Row {{key:3,tag:4}}].to_array()
  print(copied[0].tag==2 && original[0].tag==4)
  print(selected[0].tag==10 && tried[0].tag==10 && fallback[0].tag==10 && matched[0].tag==10 && looped[0].tag==10 && local[0].tag==10 && early(true)==7)
  match result(rows[..],true) {{Ok(value)=>print(false),Err(error)=>print(true)}}
  traced:=[3,1,2].map(fn n:i64 {{print(n+100);Row {{key:n,tag:n}}}}).sort_by(fn a:Row,b:Row {{print(a.tag);print(b.tag);a.key<b.key}})
  print(traced[0].key==1 && traced[2].key==3)
  return Ok(())
}}
"#
        );
        if backend_available() {
            let files = [("main.align", source.as_str())];
            assert_output(
                build_and_run_multi("record-sort-control", &files, "main.align"),
                "true\ntrue\ntrue\n103\n101\n102\n1\n3\n2\n3\n2\n1\ntrue\n",
            );
            assert_output(
                build_per_unit_multi("record-sort-control-units", &files, "main.align")
                    .link_and_run(),
                "true\ntrue\ntrue\n103\n101\n102\n1\n3\n2\n3\n2\n1\ntrue\n",
            );
        }
    });
}

#[test]
fn comparator_record_mir_storage_contract() {
    owned_fixture::run("comparator_record_mir_storage_contract", |_| {
        use align_mir::{Const, Operand, Rvalue, Stmt};
        use align_sema::{IntTy, Scalar, Ty};
        for (element, comparison, ty) in [
            ("Row", "a.key<b.key", Ty::Struct(0)),
            (
                "i64",
                "a<b",
                Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                }),
            ),
        ] {
            for region in [false, true] {
                let terminal = if region {
                    "arena {sorted:=xs.sort_by(before);sorted.len()}"
                } else {
                    "{sorted:=xs.sort_by(before);sorted.len()}"
                };
                let source = format!(
                    "Row {{key:i64,tag:u8}}\nfn before(a:{element},b:{element})->bool = {comparison}\nfn sorted(xs:slice<{element}>)->i64 = {terminal}"
                );
                let mut sources = SourceMap::new();
                let checked = check(&mut sources, "record-storage", &source);
                assert!(
                    !checked.diags.has_errors(),
                    "{}",
                    align_driver::format_diagnostics(&sources, &checked.diags)
                );
                for per_unit in [false, true] {
                    let base =
                        align_mir::lower_program_checked(&checked.hir, per_unit, None).unwrap();
                    assert!(align_mir::producer::validate_mir_producers(&base).is_ok());
                    assert!(
                        align_codegen_llvm::validate_thin_partition_program(&base, &[]).is_ok()
                    );
                    let index = base
                        .fns
                        .iter()
                        .position(|function| function.name.as_str() == "sorted")
                        .unwrap();
                    let mut covered = [0; 4];
                    for (bi, block) in base.fns[index].blocks.iter().enumerate() {
                        for (si, statement) in block.stmts.iter().enumerate() {
                            let Stmt::Let(value, rvalue) = statement else {
                                continue;
                            };
                            let mutation = match rvalue {
                                Rvalue::MakeDynArray { .. } => 0,
                                Rvalue::SliceIndex(..)
                                    if base.fns[index].value_tys[*value as usize] == ty =>
                                {
                                    1
                                }
                                Rvalue::Call(_, args)
                                    if args.len() == 2
                                        && base.fns[index].operand_ty(&args[0]) == ty =>
                                {
                                    2
                                }
                                Rvalue::HeapAllocBuf { elem, .. }
                                | Rvalue::ArenaAlloc { elem, .. }
                                    if *elem == ty =>
                                {
                                    3
                                }
                                _ => continue,
                            };
                            covered[mutation] += 1;
                            let axes = if matches!(rvalue, Rvalue::ArenaAlloc { .. }) {
                                4
                            } else if mutation == 3 {
                                3
                            } else {
                                1
                            };
                            for axis in 0..axes {
                                let mut bad = base.clone();
                                let function = &mut bad.fns[index];
                                match mutation {
                                    0 => {
                                        function.value_tys[*value as usize] = if element == "Row" {
                                            Ty::DynArray(Scalar::Struct(0))
                                        } else {
                                            Ty::DynArray(Scalar::Bool)
                                        }
                                    }
                                    1 => function.value_tys[*value as usize] = Ty::Bool,
                                    2 => {
                                        let Stmt::Let(_, Rvalue::Call(_, args)) =
                                            &mut function.blocks[bi].stmts[si]
                                        else {
                                            panic!("call")
                                        };
                                        args[1] = Operand::Const(Const::Bool(false));
                                    }
                                    _ if axis == 0 => {
                                        function.value_tys[*value as usize] = Ty::Box(Scalar::Bool)
                                    }
                                    _ => {
                                        let Stmt::Let(_, allocation) =
                                            &mut function.blocks[bi].stmts[si]
                                        else {
                                            panic!("allocation")
                                        };
                                        match allocation {
                                            Rvalue::HeapAllocBuf { count, elem }
                                            | Rvalue::ArenaAlloc { count, elem, .. }
                                                if axis < 3 =>
                                            {
                                                if axis == 1 {
                                                    *count = Operand::Const(Const::Bool(false));
                                                } else {
                                                    *elem = Ty::Bool;
                                                }
                                            }
                                            Rvalue::ArenaAlloc { handle, .. } => {
                                                *handle = Operand::Const(Const::Bool(false))
                                            }
                                            _ => panic!("allocation axis"),
                                        }
                                    }
                                }
                                // Full, partition and native boundaries authenticate even locally dropped scratch.
                                let label = format!(
                                    "{element}/{region}/{per_unit}/{mutation}/{axis}/{bi}/{si}"
                                );
                                assert!(
                                    align_codegen_llvm::validate_thin_partition_program(&bad, &[])
                                        .is_err(),
                                    "{label}"
                                );
                                if mutation != 2 {
                                    assert!(
                                        align_mir::producer::validate_mir_producers(&bad).is_err(),
                                        "{label}"
                                    );
                                }
                                if backend_available() {
                                    assert!(
                                        emit_llvm_ir(
                                            &bad,
                                            BuildTarget::Baseline,
                                            Profile::Release,
                                            false,
                                            &[],
                                            false
                                        )
                                        .is_err(),
                                        "{label}"
                                    );
                                }
                            }
                        }
                    }
                    assert!(covered.iter().all(|count| *count > 0), "{covered:?}");
                }
            }
        }
    });
}
