//! Plan194: indexed field writes retain exact backing and borrowed element dependencies.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile};
use align_span::SourceMap;
use std::path::Path;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn run_programs(
    stage: &Path,
    programs: &[align_mir::Program],
    label: &str,
    rt_lto: bool,
) -> TestResult<std::process::Output> {
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{index}.o"));
        align_driver::emit_object_file(
            program,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            rt_lto,
        )?;
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let binary = stage.join(label);
    align_driver::link_objects(
        &align_driver::CDriver::default(),
        &objects
            .iter()
            .map(|path| path.as_path())
            .collect::<Vec<_>>(),
        &binary,
        &libraries,
        Profile::Release,
    )?;
    let output = std::process::Command::new(binary).output()?;
    Ok(output)
}
fn programs(stage: &Path, source: &str, per_unit: bool) -> TestResult<Vec<align_mir::Program>> {
    let path = stage.join("main.align");
    std::fs::write(&path, source)?;
    let entry = path.to_str().ok_or("non-UTF8 fixture path")?;
    let mut sources = SourceMap::new();
    if per_unit {
        let built = align_driver::build_per_unit(&mut sources, entry, source);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        Ok(built.units.into_iter().map(|unit| unit.mir).collect())
    } else {
        let checked = align_driver::check(&mut sources, entry, source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        Ok(vec![
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources))
                .map_err(|error| format!("{error:?}"))?,
        ])
    }
}

fn execute(stage: &Path, source: &str, name: &str) -> TestResult {
    for per_unit in [false, true] {
        let built = programs(stage, source, per_unit)
            .map_err(|error| format!("{name}/{per_unit}: {error}"))?;
        let output = run_programs(stage, &built, &format!("{name}-{per_unit}"), false)
            .map_err(|error| format!("{name}/{per_unit}: {error}"))?;
        assert!(
            output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
            "{name}/{per_unit}: {output:?}"
        );
    }
    Ok(())
}
fn checked(source: &str, expected: Option<&[&str]>) {
    let mut sources = SourceMap::new();
    let result = align_driver::check(&mut sources, "dynamic-field.align", source);
    let diagnostics = align_driver::format_diagnostics(&sources, &result.diags);
    if let Some(words) = expected {
        assert!(
            result.diags.has_errors() && words.iter().any(|word| diagnostics.contains(word)),
            "expected {words:?}: {diagnostics}\n{source}"
        );
    } else {
        assert!(!result.diags.has_errors(), "{diagnostics}\n{source}");
    }
}

#[test]
fn values_and_types() {
    owned_fixture::run("values_and_types", |stage| {
        let result = (|| -> TestResult {
            // Unchanged consumer witness (provider copy only).
            execute(
                stage,
                r#"module main
Weights {ids:array<i64>}
pub fn main()->Result<(),Error> {
 mut values:array_builder<i64>:=array_builder()
 values.push(0)
 mut weights:=Weights{ids:values.build()}
 weights.ids[0]=1
 return Ok(())
}"#,
                "media",
            )?;
            let mut declarations = String::new();
            let mut body = String::new();
            for (index, (ty, first, last)) in [
                ("i8", "1 as i8", "7 as i8"),
                ("u8", "1 as u8", "7 as u8"),
                ("i16", "1 as i16", "7 as i16"),
                ("u16", "1 as u16", "7 as u16"),
                ("i32", "1 as i32", "7 as i32"),
                ("u32", "1 as u32", "7 as u32"),
                ("i64", "1", "7"),
                ("u64", "1 as u64", "7 as u64"),
                ("f32", "1.0 as f32", "7.0 as f32"),
                ("f64", "1.0", "7.0"),
                ("bool", "false", "true"),
                ("char", "'a'", "'z'"),
            ]
            .into_iter()
            .enumerate()
            {
                declarations.push_str(&format!("H{index} {{a:array<{ty}>,mark:i64}}\n"));
                body.push_str(&format!("mut h{index}:=H{index}{{a:[{first},{first}].to_array(),mark:19}};h{index}.a[1]={last};if h{index}.a[0]!={first} || h{index}.a[1]!={last} || h{index}.mark!=19 {{return 1}}\n"));
            }
            execute(
                stage,
                &format!("{declarations}\nfn main()->i32 {{{body}return 0}}"),
                "primitive",
            )?;
            execute(
                stage,
                r#"
Inner {a:array<i64>,other:array<i64>}
Outer {inner:Inner,readonly:slice<u8>,name:string}
Text {a:array<str>,fixed:[str;2]}
fn change(borrow mut value:Outer){value.inner.a[0]=8}
fn main()->i32 {
 mut value:=Outer{inner:Inner{a:[1,2].to_array(),other:[3,4].to_array()},readonly:"ok".bytes(),name:"owner".clone()}
 change(value)
 view:=value.inner.a[..]
 value.inner.a[1]=9
 if value.inner.a[0]!=8 || view[1]!=9 || value.inner.other[0]!=3 || value.name!="owner" || value.readonly[0]!=111 {return 1}
 arena {
  source:="kept".clone()
  borrowed:str:=source
  mut text:=Text{a:["old","old"].to_array(),fixed:["old","old"]}
  alias:=text.a[..]
  text.a[1]=borrowed
  text.fixed[0]=borrowed
  if alias[1]!="kept" || text.fixed[0]!="kept" || text.fixed[1]!="old" {return 2}
 }
 return 0
}"#,
                "nested-text",
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

#[test]
fn evaluation_and_control() {
    owned_fixture::run("evaluation_and_control", |stage| {
        let result = (|| -> TestResult {
            execute(
                stage,
                r#"
R {a:array<i64>,s:string}
fn fresh()->R = R{a:[3,4].to_array(),s:"live".clone()}
fn fail()->Result<i64,Error> = Err(Error.Invalid)
fn mapper(e:Error)->Error=e
fn early()->i64 {mut r:=fresh();r.a[99]={return 7};return 0}
fn early_index()->i64 {mut r:=fresh();r.a[{return 9}]={print(88);7};return 0}
fn failing()->Result<(),Error>{mut r:=fresh();r.a[99]=fail().map_err(mapper)?;return Ok(())}
fn main()->i32 {
 mut r:=fresh()
 mut order:=0
 r.a[{order=order+1; r=fresh(); 0}]={if order!=1 {return 1};order=order+1;7}
 if order!=2 || r.a[0]!=7 {return 2}
 r.a[1]={r.a=[8,9].to_array();6}
 if r.a[0]!=8 || r.a[1]!=6 {return 3}
 mut i:=0
 loop {
  if i==3 {break}
  r.a[0]=if i==0 {1} else {match i {1=>2,_=>loop {break 3}}}
  i=i+1
 }
 some:Option<i64>:=None
 r.a[1]=some else {4}
 result:Result<i64,Error>:=Ok(5)
 r.a[0]=result else {return 4}
 loop {r.a[99]={break}}
 loop {r.a[{break}]={print(88);7}}
 if r.a[0]!=5 || r.a[1]!=4 || early()!=7 || early_index()!=9 {return 5}
 match failing(){Ok(_)=>{return 6},Err(_)=>{}}
 return 0
}"#,
                "control",
            )?;
            for (length, index) in [(0, 0), (1, -1), (2, 2)] {
                let init = if length == 0 {
                    "[]"
                } else if length == 1 {
                    "[1]"
                } else {
                    "[1,2]"
                };
                let source = format!(
                    "R {{a:array<i64>}} fn main()->i32 {{initial:[i64;{length}]:={init};mut r:=R{{a:initial.to_array()}};r.a[{{print(1);{index}}}]={{print(2);7}};return 0}}"
                );
                for per_unit in [false, true] {
                    let built = programs(stage, &source, per_unit)?;
                    let output =
                        run_programs(stage, &built, &format!("bounds-{length}-{per_unit}"), false)?;
                    assert!(!output.status.success(), "{output:?}");
                    assert_eq!(output.stdout, b"1\n2\n");
                    assert!(
                        String::from_utf8_lossy(&output.stderr).contains("bounds"),
                        "{output:?}"
                    );
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
    for nested in [false, true] {
        for index_moves in [false, true] {
            for padding in ["", "+0+0+0+0+0+0"] {
                let index = if index_moves {
                    format!("consume(r){padding}")
                } else {
                    format!("0{padding}")
                };
                let value = if index_moves { "7" } else { "consume(r)" };
                let statement = format!("r.a[{index}]={value}");
                let statement = if nested {
                    format!("unused:={{{{{statement}}}}}")
                } else {
                    statement
                };
                checked(
                    &format!(
                        "R {{a:array<i64>}} fn consume(r:R)->i64=0 fn main(){{mut r:=R{{a:[1].to_array()}};{statement}}}"
                    ),
                    Some(&["use of moved value"]),
                );
            }
        }
    }
}

#[test]
fn retention_and_aliases() {
    let lifetime = &["borrow", "escape", "longer-lived"];
    for collection in ["array<str>", "[str;1]"] {
        let init = if collection.starts_with("array") {
            "[\"old\"].to_array()"
        } else {
            "[\"old\"]"
        };
        for projection in ["r.a[0]", "alias[0]"] {
            for store in [
                "r.a[0]=borrowed",
                "loop {r.a[0]=borrowed;break}",
                "if true {r.a[0]=borrowed} else {r.a[0]=borrowed}",
            ] {
                checked(
                    &format!(
                        "R {{a:{collection},b:{collection}}} fn consume(text:string){{}} fn main(){{arena {{owner:=\"short\".clone();borrowed:str:=owner;mut r:=R{{a:{init},b:{init}}};alias:=r.a[..];{store};consume(owner);print({projection})}}}}"
                    ),
                    Some(lifetime),
                );
            }
        }
        checked(
            &format!(
                "R {{a:{collection}}} fn bad(borrow mut r:R){{owner:=\"short\".clone();borrowed:str:=owner;r.a[0]=borrowed}} fn main(){{}}"
            ),
            Some(lifetime),
        );
        checked(
            &format!(
                "R {{a:{collection}}} fn main(){{arena {{mut r:=R{{a:{init}}};arena {{text:=template \"{{1}}\";r.a[0]=text}};print(r.a[0])}}}}"
            ),
            Some(lifetime),
        );
        checked(
            &format!(
                "R {{a:{collection}}} fn bad()->str {{arena {{owner:=\"short\".clone();borrowed:str:=owner;mut r:=R{{a:{init}}};r.a[0]=borrowed;return r.a[0]}}}} fn main(){{}}"
            ),
            Some(lifetime),
        );
    }
    for source in [
        "R {a:array<i64>} fn bad(borrow r:R){r.a[0]=1} fn main(){}",
        "R {a:array<i64>} fn main(){r:=R{a:[1].to_array()};r.a[0]=1}",
        "R {a:slice<u8>} fn main(){mut r:=R{a:\"bad\".bytes()};r.a[0]=1}",
        "R {a:array<string>} fn bad(borrow mut r:R){r.a[0]=\"new\".clone()} fn main(){}",
        "E {n:i64} R {a:array<E>} fn bad(borrow mut r:R){r.a[0]=E{n:2}} fn main(){}",
        "R {a:array<i64>} fn helper(borrow mut a:array<i64>){a[0]=1} fn bad(borrow mut r:R){helper(r.a)} fn main(){}",
        "R {a:array<i64>} fn bad(borrow r:Option<R>){match r {Some(row)=>{row.a[0]=1},None=>{}}} fn main(){}",
    ] {
        checked(
            source,
            Some(&[
                "immutable",
                "indexed assignment",
                "element",
                "partial",
                "borrow",
            ]),
        );
    }
    checked(
        "R {a:array<i64>} fn both(borrow view:slice<i64>,borrow mut r:R){r.a[0]=7;print(view[0])} fn main(){mut r:=R{a:[1].to_array()};view:=r.a[..];both(view,r)}",
        Some(&["alias", "overlap", "borrow"]),
    );
    for after_wait in [false, true] {
        let wait = if after_wait { "wait()" } else { "" };
        checked(
            &format!(
                "R {{a:array<i64>}} fn main(){{mut r:=R{{a:[1].to_array()}};view:=r.a[..];task_group {{spawn(fn {{print(view[0])}});{wait};r.a[0]=7}}}}"
            ),
            if after_wait {
                None
            } else {
                Some(&["task", "parallel", "conflict"])
            },
        );
    }
    checked(
        "R {a:array<i64>} fn set(borrow mut row:R,value:i64)->i64 {row.a[0]=value;return value} fn main(){mut row:=R{a:[1].to_array()};values:=[1,2].par_map(fn value {set(row,value)})}",
        Some(&["Pure", "pure", "capture", "parallel"]),
    );
    owned_fixture::run("retention_and_aliases", |stage| {
        let result = execute(
            stage,
            r#"
T {a:array<str>,b:array<str>}
fn main()->i32 {
 arena {
  text:="live".clone()
  borrowed:str:=text
  mut row:=T{a:["old"].to_array(),b:["independent"].to_array()}
  alias:=row.a[..]
  sibling:=row.b[..]
  row.a[0]=borrowed
  if alias[0]!="live" || sibling[0]!="independent" {return 1}
 }
 return 0
}"#,
            "retention-positive",
        );
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

const MODEL: &str = r#"module model
pub Row {a:array<i64>,marker:string}
pub G<T> {a:array<T>}
pub Text {a:array<str>}
pub fn make()->Row = Row{a:[1 as i64,2 as i64].to_array(),marker:"kept".clone()}
pub fn set(borrow mut row:Row,value:i64){row.a[0]=value as i64}
pub fn read(borrow row:Row)->i64 = row.a[0] as i64
pub fn text(borrow mut row:Text,value:str){row.a[0]=value}
pub fn generic(borrow mut row:G<i64>,value:i64){row.a[0]=value}
"#;
const IMPORTED: &str = r#"module main
import model
fn main()->i32 {
 mut row:=model.make()
 model.set(row,3)
 model.set(row,7)
 if model.read(row)!=7 || row.marker!="kept" {return 1}
 mut generic:model.G<i64>:=model.G{a:[1].to_array()}
 model.generic(generic,9)
 if generic.a[0]!=9 {return 2}
 arena {
  owner:="live".clone();view:str:=owner
  mut text:=model.Text{a:["old"].to_array()}
  model.text(text,view)
  if text.a[0]!="live" {return 3}
 }
 return 0
}"#;
#[test]
fn imports_and_cache() {
    owned_fixture::run("imports_and_cache", |stage| {
        let result = (|| -> TestResult {
            let model = stage.join("model.align");
            std::fs::write(&model, MODEL)?;
            execute(stage, IMPORTED, "imported")?;
            // Indexed writes and whole-field replacement share the same call authority proof.
            let text_source = r#"module main
import model
fn main()->i32 {
 arena {owner:="live".clone();view:str:=owner;mut row:=model.Text{a:["old"].to_array()};model.text(row,view);if row.a[0]!="live" {return 1}}
 return 0
}"#;
            for body in ["row.a[0]=value", "row.a=[value].to_array()"] {
                std::fs::write(
                    &model,
                    MODEL.replace("row.a[0]=value}", &format!("{body}}}")),
                )?;
                let path = stage.join("main.align");
                std::fs::write(&path, text_source)?;
                let entry = path.to_str().ok_or("path")?;
                let mut sources = SourceMap::new();
                let checked = align_driver::check(&mut sources, entry, text_source);
                assert!(
                    !checked.diags.has_errors(),
                    "{}",
                    align_driver::format_diagnostics(&sources, &checked.diags)
                );
                assert!(align_sema::checked_hir_body_facts_are_valid(&checked.hir));
                let mut sources = SourceMap::new();
                let built = align_driver::check_per_unit(&mut sources, entry, text_source);
                assert!(
                    !built.diags.has_errors(),
                    "{}",
                    align_driver::format_diagnostics(&sources, &built.diags)
                );
                let callee = built
                    .summaries
                    .iter()
                    .flat_map(|summary| &summary.fns)
                    .find(|function| function.name == "text")
                    .ok_or("text summary")?;
                assert!(
                    callee
                        .mutable_retention
                        .as_ref()
                        .and_then(|outputs| outputs.first())
                        .is_some_and(|destination| destination.roots
                            .contains(&align_sema::hir::MutableRetentionRoot::Contained(1)))
                );
                execute(stage, text_source, "text-authority")?;
            }
            std::fs::write(&model, MODEL)?;
            for action in ["consume(owner)", "owner=\"replacement\".clone()"] {
                let source = format!(
                    "module main\nimport model\nfn consume(value:string){{}} fn main(){{arena {{mut owner:=\"short\".clone();view:str:=owner;mut row:=model.Text{{a:[\"old\"].to_array()}};model.text(row,view);{action};print(row.a[0])}}}}"
                );
                let path = stage.join("main.align");
                std::fs::write(&path, &source)?;
                for per_unit in [false, true] {
                    let mut sources = SourceMap::new();
                    let diags = if per_unit {
                        align_driver::build_per_unit(
                            &mut sources,
                            path.to_str().ok_or("path")?,
                            &source,
                        )
                        .diags
                    } else {
                        align_driver::check(&mut sources, path.to_str().ok_or("path")?, &source)
                            .diags
                    };
                    let text = align_driver::format_diagnostics(&sources, &diags);
                    assert!(
                        diags.has_errors() && (text.contains("borrow") || text.contains("escape")),
                        "{action}/{per_unit}: {text}"
                    );
                }
            }
            // A uniquely resolved function value consumes the same authenticated output facts.
            for body in ["row.a[0]=value as i64", "row.a=[value as i64].to_array()"] {
                std::fs::write(&model, MODEL.replace("row.a[0]=value as i64", body))?;
                let source = "module main\nimport model\nfn main()->i32{mut row:=model.make();write:=model.set;write(row,7);if row.a[0]!=7{return 1};return 0}";
                execute(stage, source, "indirect-field-authority")?;
            }
            std::fs::write(&model, MODEL)?;
            let entry = stage.join("main.align");
            std::fs::write(&entry, IMPORTED)?;
            let cache = align_driver::CacheContext::at(stage.join("cache"));
            let mut original = Vec::new();
            for (round, unsigned, hit) in [
                (0, false, false),
                (1, false, true),
                (2, true, false),
                (3, true, true),
                (4, false, true),
            ] {
                let source = if unsigned {
                    MODEL
                        .replace("a:array<i64>", "a:array<u64>")
                        .replace("1 as i64,2 as i64", "1 as u64,2 as u64")
                        .replace("=value as i64", "=value as u64")
                } else {
                    MODEL.into()
                };
                std::fs::write(&model, source)?;
                let mut sources = SourceMap::new();
                let mut built = align_driver::build_package(
                    &mut sources,
                    entry.to_str().ok_or("path")?,
                    IMPORTED,
                    &cache,
                    align_driver::UnitReuse::Allowed,
                );
                assert!(
                    !built.diags.has_errors(),
                    "{}",
                    align_driver::format_diagnostics(&sources, &built.diags)
                );
                assert!(
                    built
                        .units
                        .iter()
                        .all(|unit| unit.frontend.as_ref().is_some_and(|front| front.hit == hit)),
                    "round {round}"
                );
                let lowered = (0..built.units.len())
                    .map(|index| built.materialize(index).cloned())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("{e:?}"))?;
                let keys = lowered
                    .iter()
                    .map(align_interface::codegen_impl_hash)
                    .collect::<Vec<_>>();
                if round == 0 {
                    original = keys;
                } else if unsigned {
                    assert_ne!(keys, original);
                } else {
                    assert_eq!(keys, original);
                }
                let output = run_programs(stage, &lowered, &format!("cache-{round}"), false)?;
                assert!(
                    output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
                    "{output:?}"
                );
            }
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

#[test]
fn expired_contents_preserve_independent_siblings() {
    owned_fixture::run("expired_contents_preserve_independent_siblings", |stage| {
        let result = (|| -> TestResult {
            for (label, declaration, initialization, selected, sibling) in [
                (
                    "field",
                    "T {a:array<str>,b:array<str>}",
                    "mut row:=T{a:[\"old\"].to_array(),b:[\"sibling\"].to_array()}",
                    "row.a",
                    "row.b",
                ),
                (
                    "nested",
                    "T {a:array<str>,b:array<str>} Outer {inner:T}",
                    "mut row:=Outer{inner:T{a:[\"old\"].to_array(),b:[\"sibling\"].to_array()}}",
                    "row.inner.a",
                    "row.inner.b",
                ),
                (
                    "fixed",
                    "T {a:[str;1],b:[str;1]}",
                    "mut row:=T{a:[\"old\"],b:[\"sibling\"]}",
                    "row.a",
                    "row.b",
                ),
                (
                    "root",
                    "",
                    "mut first:=[\"old\"].to_array();second:=[\"sibling\"].to_array()",
                    "first",
                    "second",
                ),
            ] {
                let prefix = format!(
                    "{declaration} Views {{selected:slice<str>,other:slice<str>}} Observer {{inner:Views}} fn consume(value:string){{}} fn main()->i32{{arena {{owner:=\"live\".clone();view:str:=owner;{initialization};observer:=Observer{{inner:Views{{selected:{selected}[..],other:{sibling}[..]}}}};{selected}[0]=view;consume(owner);"
                );
                let positive = format!(
                    "{prefix}if {sibling}[0]!=\"sibling\" || observer.inner.other[0]!=\"sibling\" {{return 1}}}};return 0}}"
                );
                execute(stage, &positive, label)?;
                for observed in [
                    format!("{selected}[0]"),
                    "observer.inner.selected[0]".into(),
                ] {
                    let source = format!("{prefix}print({observed})}};return 0}}");
                    let path = stage.join("main.align");
                    std::fs::write(&path, &source)?;
                    for per_unit in [false, true] {
                        let mut sources = SourceMap::new();
                        let diags = if per_unit {
                            align_driver::check_per_unit(
                                &mut sources,
                                path.to_str().ok_or("path")?,
                                &source,
                            )
                            .diags
                        } else {
                            align_driver::check(&mut sources, path.to_str().ok_or("path")?, &source)
                                .diags
                        };
                        let text = align_driver::format_diagnostics(&sources, &diags);
                        assert!(
                            diags.has_errors() && text.contains("invalidated"),
                            "{label}/{observed}/{per_unit}: {text}"
                        );
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

#[test]
fn malformed_header_precedes_codegen() {
    owned_fixture::run("malformed_header_precedes_codegen", |stage| {
        let result = (|| -> TestResult {
            let source =
                "R {a:array<i64>,view:slice<i64>} fn store(borrow mut r:R){r.a[0]=1} fn main(){}";
            let mut built = programs(stage, source, false)?;
            let program = built.first_mut().ok_or("program")?;
            align_codegen_llvm::validate_thin_partition_program(program, &[])
                .map_err(|error| error.to_string())?;
            let peer = program
                .fns
                .iter_mut()
                .find(|function| function.name.as_str() == "store")
                .ok_or("peer")?;
            let path = peer
                .blocks
                .iter_mut()
                .flat_map(|block| &mut block.stmts)
                .find_map(|statement| match statement {
                    align_mir::Stmt::Let(_, align_mir::Rvalue::Field(_, path))
                        if path.as_slice() == [0] =>
                    {
                        Some(path)
                    }
                    _ => None,
                })
                .ok_or("header")?;
            *path = vec![1]; // Same-layout Slice cannot replace an owned array header.
            let error = align_codegen_llvm::validate_thin_partition_program(program, &[])
                .expect_err("malformed peer before partition formation");
            assert!(
                error.to_string().contains("collection field MIR"),
                "{error}"
            );
            let out = stage.join("invalid.o");
            let error = align_driver::emit_object_file(
                program,
                &out,
                BuildTarget::Baseline,
                Profile::Release,
                &[],
                false,
            )
            .expect_err("malformed full emission");
            assert!(
                error.to_string().contains("collection field MIR"),
                "{error}"
            );
            assert!(
                !out.exists(),
                "invalid header must precede artifact publication"
            );
            Ok(())
        })();
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}

#[test]
fn allocation_lifecycle() {
    owned_fixture::run("allocation_lifecycle", |stage| {
        let result = execute(
            stage,
            r#"
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes()->i64
extern "C" fn align_rt_alloc_count()->i64
extern "C" fn align_rt_free_count()->i64
Inner {a:array<i64>,b:array<i64>}
R {inner:Inner,text:string}
Text {a:array<str>}
fn fresh()->R = R{inner:Inner{a:[1,2].to_array(),b:[3].to_array()},text:"kept".clone()}
fn fallible(value:i64)->Result<i64,Error> {if value<0 {return Err(Error.Invalid)};return Ok(value)}
fn exercise(bad:bool)->Result<(),Error> {
 mut r:=fresh()
 before:=unsafe {align_rt_alloc_count()}
 free:=unsafe {align_rt_free_count()}
 mut i:=0
 loop {if i==20 {break};r.inner.a[i%2]=i;r.inner.b[0]=i+1;i=i+1}
 if unsafe {align_rt_alloc_count()}!=before || unsafe {align_rt_free_count()}!=free {return Err(Error.Invalid)}
 r.inner.a[0]=fallible(if bad {-1}else{7})?
 mut moved:=r
 moved.inner.a[1]=8
 moved.inner.a=[9,10].to_array()
 moved.inner.a[0]=11
 inner:=moved.inner
 if inner.a[0]!=11 || inner.a[1]!=10 || inner.b[0]!=20 || moved.text!="kept" {return Err(Error.Invalid)}
 arena {
  mut arrays:=Inner{a:[1].to_array(),b:[2].to_array()}
  arrays.a[0]=7
  arrays.b[0]=8
  source:="borrowed".clone()
  view:str:=source
  mut text:=Text{a:["old"].to_array()}
  allocations:=unsafe {align_rt_alloc_count()}
  frees:=unsafe {align_rt_free_count()}
  text.a[0]=view
  if text.a[0]!="borrowed" || unsafe {align_rt_alloc_count()}!=allocations || unsafe {align_rt_free_count()}!=frees {return Err(Error.Invalid)}
  if arrays.a[0]!=7 || arrays.b[0]!=8 {return Err(Error.Invalid)}
 }
 return Ok(())
}
fn main()->i32 {
 unsafe {align_rt_requested_live_reset()}
 before:=unsafe {align_rt_alloc_count()}
 free:=unsafe {align_rt_free_count()}
 match exercise(false){Ok(_)=>{},Err(_)=>{return 1}}
 match exercise(true){Ok(_)=>{return 2},Err(_)=>{}}
 count:=unsafe {align_rt_alloc_count()}-before
 if count<=0 || count!=unsafe {align_rt_free_count()}-free || unsafe {align_rt_requested_live_bytes()}!=0 {return 3}
 return 0
}"#,
            "lifecycle",
        );
        if let Err(error) = result {
            panic!("{error}");
        }
    });
}
