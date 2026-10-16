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
pub fn key(row:Row)->i64 = identity(row.key)
pub fn agrees(keys:slice<i64>,expected:slice<i64>,keyed:bool)->bool {
  mut builder:array_builder<i64>:=array_builder()
  mut i:=0
  loop {if i==keys.len() {break};builder.push(i);i=i+1}
  indices:=builder.build()
  source:=indices.map(fn index:i64 {make(index,keys[index])}).to_array()
  sorted:=if keyed {source.sort_by_key(key)} else {source.sort_by(before)}
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
                    "print(model.agrees({},{},false))\nprint(model.agrees({},{},true))\n",
                    view(&keys),
                    view(&indices),
                    view(&keys),
                    view(&indices)
                ));
                expected.push_str("true\ntrue\n");
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
  key_fixed:=fixed.sort_by_key(fn row:Flat {row.key})
  key_slice:=fixed[..].sort_by_key(fn row:Flat {row.key})
  key_map:=[2,1,3].map(fn n:i64 {Flat {key:n,id:n+10}}).where(fn row:Flat {row.key>1}).sort_by_key(fn row:Flat {-row.key})
  key_project:=wraps.row.sort_by_key(fn row:Flat {row.key})
  key_native:=natives.sort_by_key(fn row:Native {row.wide})
  key_text:=spans.sort_by_key(fn row:Span {text[row.start..row.end]})
  key_float:=fixed.sort_by_key(fn row:Flat {row.key as f64})
  key_char:=fixed.sort_by_key(fn row:Flat {if row.key==1 {'a'} else {'z'}})
  print(key_float[0].id==1 && key_float[1].id==0 && key_float[2].id==2)
  print(key_char[0].id==1 && key_char[1].id==0 && key_char[2].id==2)
  key_lengths:=views.map(fn row:View {Flat {key:row.text.len(),id:row.id}}).sort_by_key(fn row:Flat {row.key})
  print(key_fixed[0].id==1 && key_fixed[1].id==0 && key_fixed[2].id==2)
  print(key_slice[0].id==1 && key_slice[1].id==0 && key_slice[2].id==2)
  print(key_map.len()==2 && key_map[0].id==13 && key_map[1].id==12)
  print(key_project[0].id==10 && key_project[1].id==20)
  print(key_native[0].small==7 && key_native[1].small==251)
  print(key_text[0].id==1 && key_text[1].id==2 && key_text[2].id==0)
  print(key_lengths[0].id==1 && key_lengths[1].id==0)
  arena {
    columns:=fixed.to_soa()
    key_columns:=columns.sort_by_key(fn row:Flat {row.key})
    print(key_columns[0].id==1 && key_columns[1].id==0 && key_columns[2].id==2)
    transposed:=columns.sort_by(fn a:Flat,b:Flat {a.key<b.key})
    print(transposed[0].id==1 && transposed[1].id==0 && transposed[2].id==2)
  }
}
"#);
        expected.push_str(&"true\n".repeat(20));
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
fn key_record_lifetimes_and_cleanup() {
    owned_fixture::run("key_record_lifetimes_and_cleanup", |_| {
        let declarations = "Row {key:i64,tag:i64,pad:u8}\n";
        for body in [
            "fn bad()->array<Row> = arena {rows:=[Row {key:1,tag:2,pad:3}];rows.sort_by_key(fn row:Row {row.key})}",
            "fn main() {mut rows:=[Row {key:1,tag:2,pad:3}].to_array();values:=rows.sort_by_key(fn row:Row {rows[0].key=9;row.key});print(values[0].key)}",
            "fn noisy(row:Row)->i64 {print(row.tag);return row.key}\nfn worker(n:i64)->i64 {rows:=[Row {key:n,tag:0,pad:3}];values:=rows.sort_by_key(noisy);return values[0].key}\nfn main() {print([1,2].par_map(worker).sum())}",
            "fn bad()->array<str> {owner:=\"local\".clone();view:str:=owner;return [view].sort_by_key(fn text:str {\"stable\"})}",
            "fn main() {mut owner:=\"local\".clone();values:=[owner.trim()].sort_by_key(fn text:str {\"key\".clone()});owner=\"new\".clone();print(values[0])}",
            "fn main() {rows:=[Row {key:1,tag:2,pad:3}];values:=rows.sort_by_key(fn row:Row {true})}",
        ] {
            let source = format!("{declarations}{body}");
            let checked = diff_check_multi(
                "key-record-invalid",
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
                "{declaration}\nfn bad(xs:slice<Row>) {{values:=xs.sort_by_key(fn row:Row {{0}})}}"
            );
            let checked = diff_check_multi(
                "key-record-domain",
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
        assert!(backend_available(), "record key owner requires LLVM");
        let mut source = format!("{COUNTERS}\n{declarations}\nZero {{empty:[i64;0]}}\n");
        let terminal = "if kind==0 {xs.sort_by_key(fn row:Row {print(row.tag);row.key})} else if kind==1 {xs.sort_by_key(fn row:Row {print(row.tag);text[row.key..row.key+1]})} else {xs.sort_by_key(fn row:Row {print(row.tag);text[row.key..row.key+1].clone()})}";
        source.push_str(&format!(r#"
fn agrees(values:slice<Row>,expected:slice<i64>)->bool {{
  if values.len()!=expected.len() {{return false}}
  mut i:=0
  loop {{if i==values.len() {{break}};row:=values[i];if row.tag!=expected[i] || row.pad!=251 {{return false}};i=i+1}}
  return true
}}
fn sorted(xs:slice<Row>,kind:i64)->array<Row> {{owner:="abcdefghij".clone();text:str:=owner;return {terminal}}}
fn region_check(xs:slice<Row>,expected:slice<i64>,kind:i64)->bool = arena {{owner:="abcdefghij".clone();text:str:=owner;values:={terminal};agrees(values[..],expected)}}
fn heap_check(xs:slice<Row>,expected:slice<i64>,kind:i64)->bool {{values:=sorted(xs,kind);return agrees(values[..],expected)}}
fn measure(xs:slice<Row>,expected:slice<i64>,kind:i64,region:bool,count:i64) {{
  a:=allocated();f:=freed();bytes:=live()
  ok:=if region {{region_check(xs,expected,kind)}} else {{heap_check(xs,expected,kind)}}
  print(ok && allocated()-a==count && freed()-f==count && live()==bytes)
}}
fn scalar_capture()->array<i64> {{owner:="temporary".clone();text:str:=owner;return [2,1].sort_by_key(fn value:i64 {{text}})}}
fn scalar_text_capture()->array<str> {{owner:="temporary".clone();text:str:=owner;return ["z","a"].sort_by_key(fn value:str {{text}})}}
fn scalar_owned_capture()->array<i64> {{owner:="temporary".clone();text:str:=owner;return [2,1].sort_by_key(fn value:i64 {{text.clone()}})}}
fn fallible(xs:slice<Row>,fail:bool)->Result<array<Row>,Error> {{if fail {{return Err(Error.Invalid)}};return Ok(sorted(xs,2))}}
fn early(flag:bool)->i64 {{rows:=[Row {{key:1,tag:1,pad:251}}];values:=(if flag {{return 7}} else {{rows[..]}}).sort_by_key(fn row:Row {{row.key}});return values[0].key}}
fn scalar_branch(xs:slice<i64>,flag:bool,compare:bool)->array<i64> {{
  ascending:=1;descending:= -1
  return if compare {{if flag {{xs.sort_by(fn a:i64,b:i64 {{a*ascending<b*ascending}})}} else {{xs.sort_by(fn a:i64,b:i64 {{a*descending<b*descending}})}}}} else {{if flag {{xs.sort_by_key(fn value:i64 {{value*ascending}})}} else {{xs.sort_by_key(fn value:i64 {{value*descending}})}}}}
}}
fn record_branch(xs:slice<Row>,flag:bool)->array<Row> {{
  ascending:=1;descending:= -1
  return if flag {{xs.sort_by(fn a:Row,b:Row {{a.key*ascending<b.key*ascending}})}} else {{xs.sort_by(fn a:Row,b:Row {{a.key*descending<b.key*descending}})}}
}}
fn transitions() {{
  rows:=[Row {{key:2,tag:20,pad:251}},Row {{key:1,tag:10,pad:251}}]
  selected:=if true {{sorted(rows[..],1)}} else {{sorted(rows[..],1)}}
  fallback:=fallible(rows[..],false) else {{sorted(rows[..],2)}}
  optional:Option<array<Row>>:=Some(sorted(rows[..],2))
  matched:=match optional {{Some(value)=>value,None=>sorted(rows[..],2)}}
  looped:=loop {{break sorted(rows[..],2)}}
  mut replacement:=sorted(rows[..],2)
  replacement=sorted(rows[..],2)
  moved:=replacement
  print(selected[0].tag==10 && fallback[0].tag==10 && matched[0].tag==10 && looped[0].tag==10 && moved[0].tag==10 && rows[0].tag==20)
  scalar:=scalar_capture();text:=scalar_text_capture();owned:=scalar_owned_capture()
  print(scalar[0]==2 && text[0]=="z" && owned[0]==2 && early(true)==7)
  mut owner:="key".clone()
  view:str:=owner
  independent:=rows.sort_by_key(fn row:Row {{view}})
  owner="new".clone()
  print(independent[0].tag==20 && owner.len()==3)
  mut original:=rows.to_array()
  copied:=original.sort_by_key(fn row:Row {{row.key}})
  original=[Row {{key:0,tag:0,pad:251}}].to_array()
  print(copied[0].tag==10 && original[0].tag==0)
  match fallible(rows[..],true) {{Ok(value)=>print(false),Err(error)=>print(true)}}
  ascending_key:=scalar_branch([2,1][..],true,false);descending_key:=scalar_branch([2,1][..],false,false)
  ascending_cmp:=scalar_branch([2,1][..],true,true);descending_cmp:=scalar_branch([2,1][..],false,true)
  ascending_row:=record_branch(rows[..],true);descending_row:=record_branch(rows[..],false)
  print(ascending_key[0]==1 && descending_key[0]==2 && ascending_cmp[0]==1 && descending_cmp[0]==2 && ascending_row[0].tag==10 && descending_row[0].tag==20)
}}
fn propagated(xs:slice<Row>)->Result<i64,Error> {{values:=fallible(xs,false).map_err(fn error:Error {{error}})?;return Ok(values[0].tag)}}
fn zero(xs:slice<Zero>,region:bool) {{
  if region {{arena {{values:=xs.sort_by_key(fn row:Zero {{print(row.empty.len());0}});print(values.len()==xs.len())}};return}}
  values:=xs.sort_by_key(fn row:Zero {{print(row.empty.len());0}});print(values.len()==xs.len())
}}
fn traced()->bool {{values:=[3,1,2].map(fn n:i64 {{print(n+100);Row {{key:n,tag:n,pad:251}}}}).sort_by_key(fn row:Row {{print(row.tag);row.key}});return values[0].tag==1 && values[2].tag==3}}
fn main() {{unsafe {{align_rt_requested_live_reset()}}
"#));
        let mut expected = String::new();
        let adaptive = std::env::var("ALIGN_SORT_ADAPTIVE").as_deref() != Ok("off");
        for n in [0, 1, 32, 33, 65] {
            let mut keys: Vec<i64> = (0..n).map(|i| (i * 7) % 9).collect();
            for ordered in [false, true] {
                if ordered {
                    keys.sort();
                }
                let mut ids: Vec<usize> = (0..keys.len()).collect();
                ids.sort_by_key(|&i| keys[i]);
                let elements = keys
                    .iter()
                    .enumerate()
                    .map(|(i, key)| format!("Row {{key:{key},tag:{i},pad:251}}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let rows = if n == 0 {
                    "[Row {key:0,tag:0,pad:251}][0..0]".into()
                } else {
                    format!("[{elements}][..]")
                };
                let indices: Vec<i64> =
                    ids.into_iter().map(|i| i64::try_from(i).unwrap()).collect();
                for kind in 0..3 {
                    for region in [false, true] {
                        let nonempty = usize::from(n > 0);
                        let scratch = if adaptive {
                            usize::from(n > 32 && !ordered) * 2
                        } else {
                            nonempty * 2
                        };
                        let count = 1
                            + usize::from(n > 0 && !region)
                            + nonempty
                            + scratch
                            + if kind == 2 { nonempty + keys.len() } else { 0 };
                        source.push_str(&format!(
                            "measure({rows},{},{kind},{region},{count})\n",
                            view(&indices)
                        ));
                        for i in 0..n {
                            expected.push_str(&format!("{i}\n"));
                        }
                        expected.push_str("true\n");
                    }
                }
            }
        }
        source.push_str("a:=allocated();f:=freed();transitions();print(allocated()-a==freed()-f && live()==0)\nresult:=propagated([Row {key:1,tag:10,pad:251}][..]) else -1;print(result==10)\n");
        for _ in 0..6 {
            expected.push_str("20\n10\n");
        }
        expected.push_str("true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n10\ntrue\n");
        source.push_str("print(traced())\n");
        expected.push_str("103\n101\n102\n3\n1\n2\ntrue\n");
        let zeros = vec!["Zero {empty:[]}"; 65].join(",");
        source.push_str(&format!("zeros:=[{zeros}]\nza:=allocated();zf:=freed()\n"));
        let mut zero_allocations = 0;
        for n in [0, 1, 32, 33, 65] {
            for region in [false, true] {
                source.push_str(&format!("zero(zeros[0..{n}],{region})\n"));
                expected.push_str(&"0\n".repeat(n));
                expected.push_str("true\n");
                zero_allocations += usize::from(n > 0) * if adaptive { 1 } else { 2 };
            }
        }
        // The result and element scratch have zero-byte extent; only key columns allocate.
        source.push_str(&format!("print(allocated()-za=={zero_allocations} && freed()-zf=={zero_allocations} && live()==0)\n}}\n"));
        expected.push_str("true\n");
        let files = [("main.align", source.as_str())];
        assert_output(
            build_and_run_multi("record-key-lifecycle", &files, "main.align"),
            &expected,
        );
        assert_output(
            build_per_unit_multi("record-key-lifecycle-units", &files, "main.align").link_and_run(),
            &expected,
        );
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
