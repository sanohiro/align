//! Complete Copy prefix states, independent contents and ordinary array ownership.
#![cfg(unix)]
mod common;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use common::*;

fn assert_output(output: std::process::Output, expected: &str) {
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

const MODEL: &str = r#"module model
pub Meta<T> {value:T,fixed:[i64;2],pair:(i64,bool)}
pub Choice {Value(i64),Empty}
pub Acc {sum:i64,count:i64,small:u8,flag:bool,fraction:f64,meta:Meta<i64>,optional:Option<i64>,result:Result<i64,Error>,choice:Choice}
pub Row {value:i64,keep:bool}
pub fn identity<T>(x:T)->T = x
pub fn state(sum:i64,count:i64)->Acc = Acc {sum:sum,count:count,small:251,flag:count%2==0,fraction:1.5,meta:Meta {value:sum+1,fixed:[sum+2,count+3],pair:(sum+4,true)},optional:if count%2==0 {Some(sum)} else {None},result:if count%2==0 {Ok(sum)} else {Err(Error.Invalid)},choice:if count%2==0 {Choice.Value(sum)} else {Choice.Empty}}
pub fn step(a:Acc,row:Row)->Acc = state(identity(a.sum)+row.value,a.count+1)
pub fn valid(a:Acc,sum:i64,count:i64)->bool {
  expected:=if count%2==0 {sum} else {-1}
  optional:=a.optional else -1
  result:=match a.result {Ok(x)=>x,Err(e)=>-1}
  choice:=match a.choice {Value(x)=>x,Empty=>-1}
  return a.sum==sum && a.count==count && a.small==251 && a.flag==(count%2==0) && a.fraction==1.5 && a.meta.value==sum+1 && a.meta.fixed[0]==sum+2 && a.meta.fixed[1]==count+3 && a.meta.pair.0==sum+4 && a.meta.pair.1 && optional==expected && result==expected && choice==expected
}
pub fn agrees(xs:slice<i64>,expected:slice<i64>)->bool {
  rows:=xs.map(fn n:i64 {Row {value:n,keep:n%2==0}}).to_array()
  prefix:=rows.where(.keep).scan(state(10,0),step)
  if prefix.len()!=expected.len() {return false}
  mut i:=0
  loop {if i==prefix.len() {break};if !valid(prefix[i],expected[i],i+1) {return false};i=i+1}
  return rows.len()==xs.len()
}
"#;

#[test]
fn record_scan_order_layout_and_replay() {
    owned_fixture::run("record_scan_order_layout_and_replay", |_| {
        assert!(backend_available(), "record scan owner requires LLVM");
        let mut source = String::from(
            "module main\nimport model\nWrap {row:model.Row}\nView {text:str}\nfn main(){\n",
        );
        let mut expected = String::new();
        for n in [0, 1, 2, 17, 65] {
            let xs: Vec<i64> = (0..n).map(|i| (i * 7 + 3) % 13 - 6).collect();
            let mut sum = 10;
            let prefix: Vec<_> = xs
                .iter()
                .filter(|x| *x % 2 == 0)
                .map(|x| {
                    sum += x;
                    sum
                })
                .collect();
            let view = |values: &[i64]| {
                if values.is_empty() {
                    "[0][0..0]".to_string()
                } else {
                    format!(
                        "[{}][..]",
                        values
                            .iter()
                            .map(i64::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                }
            };
            source.push_str(&format!(
                "print(model.agrees({},{}))\n",
                view(&xs),
                view(&prefix)
            ));
            expected.push_str("true\n");
        }
        source.push_str(
            r#"
  rows:=[model.Row {value:2,keep:true},model.Row {value:3,keep:false}]
  fixed:=rows.scan(model.state(10,0),model.step)
  sliced:=rows[..].scan(model.state(10,0),model.step)
  mapped:=[2,3].map(fn x:i64 {model.Row {value:x,keep:true}}).scan(model.state(10,0),model.step)
  scalar:=[2,3].scan(model.state(10,0),fn a:model.Acc,x:i64 {model.state(a.sum+x,a.count+1)})
  wraps:=[Wrap {row:model.Row {value:2,keep:true}},Wrap {row:model.Row {value:3,keep:true}}]
  projected:=wraps.row.scan(model.state(10,0),model.step)
  print(model.valid(fixed[0],12,1) && model.valid(fixed[1],15,2))
  print(model.valid(sliced[1],15,2) && model.valid(mapped[1],15,2))
  print(model.valid(scalar[1],15,2) && model.valid(projected[1],15,2))
  projected_scalar:=rows.value.scan(0,fn a:i64,x:i64 {a+x})
  print(projected_scalar[0]==2 && projected_scalar[1]==5)
  views:=[View {text:"ab"},View {text:"xyz"}]
  lengths:=views.scan(0,fn a:i64,row:View {a+row.text.len()})
  print(lengths[0]==2 && lengths[1]==5)
  arena {
    columns:=rows.to_soa()
    from_columns:=columns.scan(model.state(10,0),model.step)
    print(model.valid(from_columns[0],12,1) && model.valid(from_columns[1],15,2))
  }
}
"#,
        );
        expected.push_str(&"true\n".repeat(6));
        let files = [("main.align", source.as_str()), ("model.align", MODEL)];
        assert_output(
            build_and_run_multi("record-scan-whole", &files, "main.align"),
            &expected,
        );
        let project = Proj::new("record-scan-replay", &files, "main.align");
        for round in 0..2 {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
                .args(["run", "main.align", "--cache-stats", "-j", "1"])
                .current_dir(&project.dir)
                .env("ALIGNC_CACHE", project.cache_root())
                .env_remove("ALIGN_BUFFER_DONATE")
                .output()
                .unwrap();
            let cache = if round == 0 {
                "2 frontend: 0 hit, 2 miss"
            } else {
                "2 frontend: 2 hit, 0 miss"
            };
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(cache),
                "{output:?}"
            );
            assert_output(output, &expected);
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
fn record_scan_lifetimes_and_cleanup() {
    owned_fixture::run("record_scan_lifetimes_and_cleanup", |_| {
        let declarations = "Acc {sum:i64,count:i64,pad:u8}\nView {text:str}\n";
        for body in [
            "fn bad()->array<Acc> = arena {[1].scan(Acc {sum:0,count:0,pad:251},fn a:Acc,x:i64 {a})}",
            "fn bad()->array<str> {owner:=\"local\".clone();view:str:=owner;return [1].scan(\"\",fn a:str,x:i64 {view})}",
            "Choice {Text(str),Empty}\nfn bad()->array<Choice> {owner:=\"local\".clone();view:str:=owner;return [1].scan(Choice.Empty,fn a:Choice,x:i64 {Choice.Text(view)})}",
            "fn main(){mut owner:=\"local\".clone();values:=[1].scan(\"\",fn a:str,x:i64 {owner.trim()});owner=\"new\".clone();print(values[0])}",
            "fn main(){mut rows:=[1,2].to_array();result:=rows.scan({rows=[3,4].to_array();0},fn a:i64,x:i64 {a+x});print(result.sum())}",
            "fn main(){mut owner:=\"local\".clone();view:str:=owner;result:=[1].map(fn x:i64 {x+view.len()}).scan({owner=\"new\".clone();0},fn a:i64,x:i64 {a+x});print(result.sum())}",
            "fn main(){mut result:=[1].scan(0,fn a:i64,x:i64 {a+x});view:=result[..];result=[2].scan(0,fn a:i64,x:i64 {a+x});print(view[0])}",
            "fn main(){mut result:=[1].scan(Acc {sum:0,count:0,pad:251},fn a:Acc,x:i64 {a});view:=result[..];result=[2].scan(Acc {sum:0,count:0,pad:251},fn a:Acc,x:i64 {a});print(view[0].sum)}",
            "Owned {text:string}\nfn bad(xs:slice<Owned>)->array<i64> = xs.scan(0,fn a:i64,x:Owned {a})",
            "fn noisy(a:Acc,x:i64)->Acc {print(x);return a}\nfn worker(x:i64)->i64 {a:=[x].scan(Acc {sum:0,count:0,pad:251},noisy);return a.len()}\nfn main(){print([1].par_map(worker).sum())}",
        ] {
            let source = format!("{declarations}{body}");
            let checked = diff_check_multi(
                "record-scan-invalid",
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
            "Acc {value:str}",
            "Acc {value:string}",
            "Acc {value:[str;0]}",
            "Choice {Text(str),Empty}\nAcc {value:Choice}",
            "Acc {value:fn(i64)->i64}",
            "layout(C) Acc {value:raw}",
            "align(32) Acc {value:i64}",
        ] {
            let source = format!(
                "{declaration}\nfn bad(a:Acc) {{values:=[1].scan(a,fn previous:Acc,x:i64 {{previous}})}}"
            );
            let checked = diff_check_multi(
                "record-scan-domain",
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
        assert!(backend_available(), "record scan owner requires LLVM");
        let source = format!("{COUNTERS}\n{declarations}\n{LIFETIMES}");
        let expected = format!(
            "{}100\n2\n3\ntrue\n100\ntrue\n100\ntrue\n",
            "true\n".repeat(8)
        );
        let files = [("main.align", source.as_str())];
        assert_output(build_and_run("record-scan-lifetime", &source), &expected);
        assert_output(
            build_per_unit_multi("record-scan-lifetime-units", &files, "main.align").link_and_run(),
            &expected,
        );
    });
}

const LIFETIMES: &str = r#"
Zero {empty:[i64;0]}
fn seed()->Acc = Acc {sum:0,count:0,pad:251}
fn step(a:Acc,x:i64)->Acc = Acc {sum:a.sum+x,count:a.count+1,pad:251}
fn produced()->array<Acc> {
  owner:="ab".clone();view:str:=owner
  rows:=[View {text:view},View {text:view}]
  return rows.map(fn row:View {row.text.len()+view.len()}).scan(seed(),fn a:Acc,x:i64 {step(a,x+view.len())})
}
fn scalar()->array<i64> {owner:="ab".clone();view:str:=owner;return [1,2].scan(0,fn a:i64,x:i64 {a+x+view.len()})}
fn fallible(fail:bool)->Result<array<Acc>,Error> {if fail {return Err(Error.Invalid)};return Ok(produced())}
fn propagate()->Result<i64,Error> {a:=fallible(false).map_err(fn e:Error {e})?;return Ok(a[1].sum)}
fn early(flag:bool)->i64 {a:=[1].scan(if flag {return 7} else {seed()},step);return a[0].sum}
fn early_source(flag:bool)->i64 {xs:=[1];a:=(if flag {return 8} else {xs[..]}).scan(seed(),step);return a[0].sum}
fn initial()->Acc {print(100);return seed()}
fn noisy(a:Acc,x:i64)->Acc {print(x);return step(a,x)}
fn effects(){
  a:=[1,2,3].where(fn x:i64 {x>1}).scan(initial(),noisy);print(a.len()==2 && a[0].sum==2 && a[1].sum==5)
  b:=[1,2].where(fn x:i64 {false}).scan(initial(),noisy);print(b.len()==0)
  c:=[1][0..0].scan(initial(),noisy);print(c.len()==0)
}
fn transitions()->bool {
  selected:=if true {produced()} else {produced()}
  fallback:=fallible(true) else produced()
  optional:Option<array<Acc>>:=Some(produced())
  matched:=match optional {Some(a)=>a,None=>produced()}
  looped:=loop {break produced()}
  mut replaced:=produced();replaced=produced()
  moved:=replaced
  produced()
  temp:=[1,2,3].to_array().scan(seed(),step)
  return selected[1].sum==12 && fallback[1].count==2 && matched[1].pad==251 && looped[0].sum==6 && moved[1].sum==12 && temp[2].sum==6
}
fn independent()->bool {
  mut owner:="ab".clone();view:str:=owner
  a:=[1,2].scan(seed(),fn acc:Acc,x:i64 {step(acc,x+view.len())})
  b:=[1,2].scan(0,fn acc:i64,x:i64 {acc+x+view.len()})
  owner="new".clone()
  return a[0].sum==3 && a[1].sum==7 && b[1]==7
}
fn arena_check()->bool = arena {values:=[1,2,3].scan(seed(),step);values[0].sum==1 && values[2].sum==6}
fn zeros()->bool {
  a:=[1,2,3].scan(Zero {empty:[]},fn z:Zero,x:i64 {z})
  empty:=[1][0..0].scan(seed(),step)
  filtered:=[1,2].where(fn x:i64 {false}).scan(seed(),step)
  return a.len()==3 && empty.len()==0 && filtered.len()==0
}
fn main(){
  unsafe {align_rt_requested_live_reset()}
  a:=allocated();f:=freed()
  print(transitions());print(independent());print(arena_check());print(zeros())
  scalar_result:=scalar();print(scalar_result[0]==3 && scalar_result[1]==7)
  value:=propagate() else -1;print(value==12)
  print(early(true)==7 && early(false)==1 && early_source(true)==8 && early_source(false)==1)
  print(allocated()-a==freed()-f+1 && live()==16)
  effects()
}
"#;
