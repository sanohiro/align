//! Initialized Out backing stays readable across producer and import boundaries.
#![cfg(unix)]
#[path = "helpers/indexed_array_field.rs"]
mod execution;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_span::SourceMap;

const OWNER: &str = r#"module owner
pub fn append(borrow mut destination:buffer,out bytes:slice<u8>) {
  destination.append(bytes[..])
  if bytes.len()>0 {
    bytes[0]=7 as u8
    destination.append(bytes[0..1])
  }
}
pub fn read(out bytes:slice<u8>)->u8=bytes[0]
pub fn view(out bytes:slice<u8>)->slice<u8> = bytes[..]
pub Text {value:str}
pub fn read_text(out texts:slice<str>)->string=texts[0].clone()
pub fn read_record(out texts:slice<Text>)->string=texts[0].value.clone()
pub fn copy(out bytes:slice<u8>)->buffer {
  mut destination:=buffer(bytes.len())
  destination.append(bytes)
  return destination
}
pub fn tagged<T>(borrow mut destination:buffer,out bytes:slice<u8>,tag:T)->T {
  destination.append(bytes)
  return tag
}
pub fn control(out bytes:slice<u8>,stop:bool)->Result<u8,Error> {
  if stop {return Err(Error.Invalid)}
  mut i:=0
  loop {
    if i>=bytes.len() {break}
    bytes[i]=bytes[i]+1 as u8
    i=i+1
  }
  return Ok(if bytes.len()==0 {0 as u8}else{bytes[0]})
}
"#;

const MAIN: &str = r#"module main
import owner
fn main()->i32 {
  mut bytes:=[3 as u8,4 as u8]
  if owner.read(bytes)!=3 as u8 {return 1}
  mut destination:=buffer(0)
  owner.append(destination,bytes)
  if destination.bytes().len()!=3 || destination.bytes()[0]!=3 as u8 || destination.bytes()[1]!=4 as u8 || destination.bytes()[2]!=7 as u8 {return 2}
  owner.append(destination,bytes)
  if destination.bytes().len()!=6 || bytes[0]!=7 as u8 {return 3}
  returned:=owner.view(bytes)
  if returned[1]!=4 as u8 {return 4}
  destination.append(returned)
  mut texts:=["abc"]
  if owner.read_text(texts)!="abc" {return 12}
  mut records:=[owner.Text{value:"def"}]
  if owner.read_record(records)!="def" {return 13}
  independent:=owner.copy(bytes)
  bytes[0]=9 as u8
  if independent.bytes()[0]!=7 as u8 {return 5}
  if owner.tagged(destination,bytes,42)!=42 {return 6}
  match owner.control(bytes,true) {Ok(_)=>{return 7},Err(_)=>{}}
  if (owner.control(bytes,false) else {return 8})!=10 as u8 {return 9}
  owner.append(destination,bytes[0..0])
  if (owner.control(bytes[0..0],false) else {return 10})!=0 as u8 {return 11}
  return 0
}
"#;

#[test]
fn source_and_imports() {
    owned_fixture::run("source_and_imports", |stage| {
        std::fs::write(stage.join("owner.align"), OWNER).unwrap();
        execution::execute(stage, MAIN, "out-reads").unwrap();
        let entry = stage.join("main.align");
        let context = align_driver::CacheContext::at(stage.join("cache"));
        for pass in 0..3 {
            if pass == 2 {
                std::fs::write(
                    stage.join("owner.align"),
                    OWNER.replace("bytes[0]\n", "bytes[0]+0 as u8\n"),
                )
                .unwrap();
            }
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry.to_str().unwrap(),
                MAIN,
                &context,
                align_driver::UnitReuse::Allowed,
            );
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &built.diags)
            );
            assert_eq!(built.units.len(), 2);
            for unit in &built.units {
                if pass < 2 {
                    assert_eq!(unit.frontend.as_ref().unwrap().hit, pass == 1);
                }
            }
            if pass == 2 {
                assert!(
                    built
                        .units
                        .iter()
                        .any(|unit| !unit.frontend.as_ref().unwrap().hit)
                );
            }
            let programs = (0..built.units.len())
                .map(|index| built.materialize(index).unwrap().clone())
                .collect::<Vec<_>>();
            let output =
                execution::run_programs(stage, &programs, &format!("cached-{pass}"), false)
                    .unwrap();
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    });
}

#[test]
fn exclusive_source_rules_remain() {
    for source in [
        "fn read(out bytes:slice<u8>)->u8=bytes[0]\nfn main(){n:=read(\"x\".bytes())}",
        "fn read(out bytes:slice<u8>)->u8=bytes[0]\nfn shared(borrow bytes:array<u8>)->u8=read(bytes[..])\nfn main(){}",
        "fn join(out bytes:slice<u8>,other:slice<u8>)->u8=other[0]\nfn main(){mut bytes:=[1 as u8];n:=join(bytes,bytes[..])}",
        "fn view(out bytes:slice<u8>)->slice<u8> = bytes[..]\nfn invalid()->slice<u8>{mut bytes:=[1 as u8];return view(bytes)}\nfn main(){}",
    ] {
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, "negative.align", source);
        if !checked.diags.has_errors()
            && let Ok(program) =
                align_mir::lower_program_checked(&checked.hir, false, Some(&sources))
        {
            assert!(
                align_mir::producer::validate_mir_producers(&program).is_err(),
                "accepted {source}"
            );
        }
    }
}

#[test]
fn producer_authority() {
    let source = "fn append(borrow mut destination:buffer,bytes:slice<u8>){destination.append(bytes)}\nfn main(){}";
    let mut sources = SourceMap::new();
    let checked = align_driver::check(&mut sources, "producer.align", source);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    let base = align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
    let function = base
        .fns
        .iter()
        .position(|function| function.name.as_str() == "append")
        .unwrap();
    for mode in [
        align_ast::ParamMode::ByValue,
        align_ast::ParamMode::Borrow,
        align_ast::ParamMode::BorrowMut,
        align_ast::ParamMode::Out,
    ] {
        let mut candidate = base.clone();
        candidate.fns[function].param_modes[1] = mode;
        assert!(
            align_mir::producer::validate_mir_producers(&candidate).is_ok(),
            "read mode {mode:?}"
        );
    }
    for axis in [
        "owner-out",
        "shared-destination",
        "bad-slice-type",
        "missing-slot",
        "uninitialized",
        "cycle",
    ] {
        let mut candidate = base.clone();
        let body = &mut candidate.fns[function];
        body.param_modes[1] = align_ast::ParamMode::Out;
        match axis {
            "owner-out" => body.param_modes[0] = align_ast::ParamMode::Out,
            "shared-destination" => body.param_modes[0] = align_ast::ParamMode::Borrow,
            "bad-slice-type" => body.slots[body.params[1] as usize] = align_sema::Ty::Bool,
            "missing-slot" => body.params[1] = u32::MAX,
            "uninitialized" | "cycle" => {
                let slot = body.params[1];
                body.blocks.iter_mut().for_each(|block| {
                    block.stmts.retain(
                        |stmt| !matches!(stmt, align_mir::Stmt::Store(target, _) if *target==slot),
                    )
                });
                if axis == "cycle" {
                    let value = u32::try_from(body.value_tys.len()).unwrap();
                    body.value_tys.push(body.slots[slot as usize]);
                    body.blocks[0].stmts.insert(
                        0,
                        align_mir::Stmt::Store(slot, align_mir::Operand::Value(value)),
                    );
                    body.blocks[0].stmts.insert(
                        0,
                        align_mir::Stmt::Let(value, align_mir::Rvalue::Load(slot)),
                    );
                }
            }
            _ => unreachable!(),
        }
        assert!(
            align_mir::producer::validate_mir_producers(&candidate).is_err(),
            "accepted {axis}"
        );
        let defined = candidate.fns.iter().map(|f| f.name.clone()).collect();
        assert!(
            align_mir::producer::validate_partition_resource_rvalues(&candidate, &defined).is_err(),
            "partition accepted {axis}"
        );
    }
}

#[test]
fn view_authority_does_not_expand() {
    for source in [
        "fn write(out texts:slice<str>){mut bytes:=texts[0].bytes();bytes[0]=1 as u8}\nfn main(){}",
        "Text {value:str}\nfn write(out texts:slice<Text>){mut bytes:=texts[0].value.bytes();bytes[0]=1 as u8}\nfn main(){}",
        "Owned {value:string}\nfn take(out values:slice<Owned>)->Owned=values[0]\nfn main(){}",
        "import std.process\nfn view(out bytes:slice<u8>,yes:bool)->slice<u8>{return if yes {bytes[..]} else {\"x\".bytes()}}\nfn read(borrow mut child:child,out bytes:slice<u8>)->Result<Option<i64>,Error>{mut returned:=view(bytes,false);return child.read_stdout(returned)}\nfn main(){}",
    ] {
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, "negative.align", source);
        if !checked.diags.has_errors() {
            assert!(
                align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).is_err(),
                "accepted {source}"
            );
        }
    }
}
