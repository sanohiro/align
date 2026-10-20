use super::*;

const SOURCE: &str = "fn alloc(size:i64)->Result<raw,Error> = unsafe {raw.try_alloc(size)}\nfn pair(value:raw)->(raw,bool)=(value,true)\nfn maybe(value:raw)->Option<raw> = Some(value)\nfn main() {}\n";

const NATIVE: &str = r#"
Record { pointer:raw, tagged:Result<raw,Error>, pair:(raw,i64) }
Choice { Pointer(raw), Empty }
fn identity<T>(value:T)->Result<T,Error> = Ok(value)
fn run()->Result<(),Error> {unsafe {
  zero:=raw.try_alloc(0)?
  if !zero.is_null() {return Err(Error.Code(1))}
  match raw.try_alloc(-1) {Ok(_)=>{return Err(Error.Code(2))},Err(error)=>{
    match error {Invalid=>{},_=>{return Err(Error.Code(3))}}
  }}
  pointer:=raw.try_alloc(37)?
  raw.store(pointer,0,0x12345678 as i64)
  mut record:=Record {pointer:pointer,tagged:identity(pointer),pair:(pointer,37)}
  copied:=record
  record.pointer=zero
  from_result:=copied.tagged?
  from_sum:=match Choice.Pointer(copied.pair.0) {Pointer(value)=>value,Empty=>zero}
  optional:Option<raw>:=Some(from_sum)
  from_option:=optional else zero
  a:i64:=raw.load(from_result,0)
  b:i64:=raw.load(from_option,0)
  if a!=0x12345678 || b!=a || copied.pair.1!=37 {return Err(Error.Code(4))}
  raw.free(pointer)
  raw.free(zero)
  return Ok(())
}}
fn main()->Result<(),Error> = run()
"#;

#[test]
#[ignore = "explicit three-target native qualification export"]
fn export_fallible_raw_objects() -> Result<(), String> {
    use inkwell::targets::FileType;
    let root = std::env::var_os("ALIGN_RAW_NATIVE_ARTIFACTS").ok_or("native artifact directory")?;
    let root = std::path::Path::new(&root);
    std::fs::create_dir(root).map_err(|e| e.to_string())?;
    struct Owned<'a>(&'a std::path::Path, bool);
    impl Drop for Owned<'_> {
        fn drop(&mut self) { if !self.1 { let _ = std::fs::remove_dir_all(self.0); } }
    }
    let mut owned = Owned(root, false);
    Target::initialize_x86(&InitializationConfig::default());
    Target::initialize_aarch64(&InitializationConfig::default());
    let program = tests::mir(NATIVE);
    std::fs::write(root.join("fixture.align"), NATIVE).map_err(|e| e.to_string())?;
    for triple in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "arm64-apple-macosx26.0"] {
        let target = TargetTriple::create(triple);
        let machine = Target::from_triple(&target).map_err(|e| e.to_string())?
            .create_target_machine(&target, if triple.starts_with("x86") {"x86-64-v2"} else {"generic"},
                "", OptimizationLevel::Default, RelocMode::PIC, CodeModel::Default).ok_or("target")?;
        let context = Context::create();
        let module = context.create_module("raw-native");
        module.set_triple(&target);
        module.set_data_layout(&machine.get_target_data().get_data_layout());
        build_module(&context, &module, &program, &machine, None, &[], false,
            ModuleScope::Whole).map_err(|e| e.to_string())?;
        module.verify().map_err(|e| e.to_string())?;
        module.print_to_file(root.join(format!("{triple}.ll"))).map_err(|e| e.to_string())?;
        machine.write_to_file(&module, FileType::Object, &root.join(format!("{triple}.o")))
            .map_err(|e| e.to_string())?;
    }
    owned.1 = true;
    Ok(())
}

#[test]
fn fallible_raw_native_result_and_malformed_mir() -> Result<(), String> {
    let program = tests::mir(SOURCE);
    let context = Context::create();
    let (module, machine) = build_program_module(&context, &program, &BuildTarget::Baseline,
        Profile::Release, &[], None, ModuleScope::Whole).map_err(|e| e.to_string())?;
    module.verify().map_err(|e| e.to_string())?;
    let runtime = module.get_function("align_rt_raw_try_alloc").ok_or("runtime function")?;
    assert_eq!(runtime.get_type().print_to_string().to_string(), "i32 (i64, ptr)");
    let ir = module.print_to_string().to_string();
    assert!(ir.contains("raw.try_alloc.refused"));
    assert!(ir.contains("raw.try_alloc.abort"));
    assert!(ir.contains("store ptr null"));
    assert!(ir.contains("i32 17, label"));
    assert!(ir.contains("i32 2, label"));
    let layout = machine.get_target_data();
    for name in ["pair", "maybe"] {
        let definition = program.fns.iter().find(|f| f.name.as_str() == name).ok_or("function")?;
        let function = module.get_function(&encoded_program_symbol(&definition.name)).ok_or("LLVM function")?;
        let sret = function.get_enum_attribute(inkwell::attributes::AttributeLoc::Param(0),
            inkwell::attributes::Attribute::get_named_enum_kind_id("sret"));
        let ty = match function.get_type().get_return_type() {
            Some(ty) => ty,
            None => sret.as_ref().ok_or("Copy return storage")?.get_type_value().into_struct_type().into(),
        };
        assert_eq!((layout.get_abi_size(&ty), layout.get_abi_alignment(&ty)), (16, 8));
    }
    for mutation in 0..3 {
        let mut bad = program.clone();
        let function = bad.fns.iter_mut().find(|f| f.name.as_str() == "alloc").ok_or("alloc")?;
        let (value, operation) = function.blocks.iter_mut().flat_map(|b| b.stmts.iter_mut())
            .find_map(|statement| match statement {
                Stmt::Let(value, operation @ Rvalue::RawTryAlloc { .. }) => Some((*value, operation)),
                _ => None,
            }).ok_or("allocation operation")?;
        let Rvalue::RawTryAlloc { size, error_enum } = operation else { return Err("operation".into()); };
        match mutation {
            0 => *size = Operand::Const(align_mir::Const::Bool(true)),
            1 => *error_enum = u32::MAX,
            _ => function.value_tys[value as usize] = Ty::Raw,
        }
        let malformed = context.create_module("malformed");
        assert!(build_module(&context, &malformed, &bad, &machine, None, &[], false,
            ModuleScope::Whole).is_err(), "mutation {mutation}");
        assert_eq!(malformed.get_functions().count(), 0, "published malformed operation");
    }
    Ok(())
}
