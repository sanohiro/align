//! Plan182: independent bits never confer source-storage or Move authority.
use super::*;

fn lower(source: &str) -> Result<Program, Box<dyn std::error::Error>> {
    let mut diagnostics = align_diag::Diagnostics::new();
    let tokens = align_lexer::tokenize(0, source, &mut diagnostics);
    let ast = align_parser::parse_file(tokens, &mut diagnostics);
    let hir = align_sema::check_file(&ast, &mut diagnostics);
    assert!(
        !diagnostics.has_errors(),
        "{:?}",
        diagnostics.iter().collect::<Vec<_>>()
    );
    Ok(crate::lower_program_checked(&hir, false, None).map_err(|error| format!("{error:?}"))?)
}

#[test]
fn producer_plain_copy_type_graph() -> Result<(), Box<dyn std::error::Error>> {
    let mut program = lower("Plain { value:i64 }\nfn main() {}\n")?;
    let plain = program
        .structs
        .iter()
        .position(|s| s.name == "Plain")
        .ok_or("Plain fixture type")?;
    let id = u32::try_from(plain)?;
    let int = Scalar::Int(IntTy {
        bits: 64,
        signed: true,
    });
    let mut leaves = vec![Ty::Unit, Ty::Bool, Ty::Char, Ty::Rng];
    for bits in [8, 16, 32, 64] {
        for signed in [false, true] {
            leaves.push(Ty::Int(IntTy { bits, signed }));
        }
    }
    leaves.extend([
        Ty::Float(FloatTy { bits: 32 }),
        Ty::Float(FloatTy { bits: 64 }),
    ]);
    for lanes in [2, 4, 8, 16] {
        leaves.extend([Ty::Vec(int, lanes), Ty::Mask(int, lanes)]);
    }
    for leaf in leaves {
        program.structs[plain].fields[0].ty = leaf;
        assert!(xml_plain_copy_type(&program, leaf), "{leaf:?}");
        for root in [
            Ty::Struct(id),
            Ty::StructArray(id, 0),
            Ty::StructArray(id, 3),
            Ty::Option(Scalar::Struct(id)),
            Ty::Result(int, Scalar::Struct(id)),
        ] {
            assert!(xml_plain_copy_type(&program, root), "{leaf:?}/{root:?}");
        }
    }
    let invalid = [
        Ty::Str,
        Ty::String,
        Ty::Raw,
        Ty::Slice(int),
        Ty::Soa(id),
        Ty::Fn(0),
        Ty::ArenaHandle,
        Ty::Buffer,
        Ty::Resource(0),
        Ty::ResourceRef(0),
        Ty::Box(int),
        Ty::DynArray(int),
        Ty::DynStructArray(id, Layout::Aos),
        Ty::XmlReader,
        Ty::Task(int),
        Ty::Error,
        Ty::Param(0),
        Ty::IntVar(0),
        Ty::FloatVar(0),
        Ty::Int(IntTy {
            bits: 0,
            signed: true,
        }),
        Ty::Int(IntTy {
            bits: 128,
            signed: false,
        }),
        Ty::Float(FloatTy { bits: 16 }),
        Ty::Vec(int, 3),
        Ty::Mask(int, 32),
        Ty::Vec(Scalar::Bool, 4),
        Ty::Mask(Scalar::Str, 4),
        Ty::Struct(u32::MAX),
        Ty::Tuple(u32::MAX),
        Ty::Enum(u32::MAX),
        Ty::Tagged(u32::MAX),
    ];
    for leaf in invalid {
        program.structs[plain].fields[0].ty = leaf;
        assert!(!xml_plain_copy_type(&program, leaf), "{leaf:?}");
        for root in [
            Ty::Struct(id),
            Ty::StructArray(id, 0),
            Ty::StructArray(id, 2),
            Ty::Option(Scalar::Struct(id)),
            Ty::Result(int, Scalar::Struct(id)),
        ] {
            assert!(!xml_plain_copy_type(&program, root), "{leaf:?}/{root:?}");
            if let Some(element) = align_sema::array_builder_elem(root) {
                assert!(align_sema::array_builder_copy_element(Ty::array_builder(element),
                    &program.structs, &program.tuples, &program.enums, &program.tagged_types).is_none(),
                    "builder read accepted {leaf:?}/{root:?}");
            }
        }
    }
    for leaf in [Scalar::Str, Scalar::String, Scalar::Fn(0)] {
        assert!(!xml_plain_copy_type(&program, Ty::Array(leaf, 0)));
    }
    // Iterative type traversal rejects cycles and shares completed DAG nodes.
    program.structs[plain].fields[0].ty = Ty::Struct(id);
    assert!(!xml_plain_copy_type(&program, Ty::Struct(id)));
    program.structs[plain].fields[0].ty = scalar_to_ty(int);
    let mut previous = id;
    for _ in 0..512 {
        let mut record = program.structs[plain].clone();
        record.fields[0].ty = Ty::Struct(previous);
        record.fields.push(record.fields[0].clone());
        previous = u32::try_from(program.structs.len())?;
        program.structs.push(record);
    }
    assert!(xml_plain_copy_type(&program, Ty::Struct(previous)));
    program.structs[plain].fields[0].ty = Ty::Struct(previous);
    assert!(!xml_plain_copy_type(&program, Ty::Struct(previous)));

    let mut carriers = lower(
        "Pair { a:i64,b:bool }\nChoice { One(Pair),None }\nfn pair() -> (i64,bool) = (1,true)\nfn main() {}\n",
    )?;
    let pair = carriers
        .structs
        .iter()
        .position(|s| s.name == "Pair")
        .ok_or("Pair fixture type")?;
    let choice = carriers
        .enums
        .iter()
        .position(|e| e.name == "Choice")
        .ok_or("Choice fixture type")?;
    let choice = u32::try_from(choice)?;
    let tuple = u32::try_from(
        carriers
            .tuples
            .len()
            .checked_sub(1)
            .ok_or("tuple fixture")?,
    )?;
    carriers
        .tagged_types
        .push(hir::TaggedType::Result(int, Scalar::Enum(choice)));
    let tagged = u32::try_from(
        carriers
            .tagged_types
            .len()
            .checked_sub(1)
            .ok_or("tagged fixture")?,
    )?;
    assert!(xml_plain_copy_type(&carriers, Ty::Tuple(tuple)));
    assert!(xml_plain_copy_type(&carriers, Ty::Tagged(tagged)));
    carriers.structs[pair].fields[1].ty = Ty::Str;
    assert!(!xml_plain_copy_type(&carriers, Ty::Enum(choice)));
    assert!(!xml_plain_copy_type(&carriers, Ty::Tagged(tagged)));
    carriers.tuples[tuple as usize].elems[0] = Scalar::String;
    assert!(!xml_plain_copy_type(&carriers, Ty::Tuple(tuple)));
    Ok(())
}

#[test]
fn producer_copy_record_authority_matrix() -> Result<(), Box<dyn std::error::Error>> {
    for mode in ["", "borrow ", "borrow mut "] {
        for result in [false, true] {
            let ret = if result {
                "Result<Owned,Error>"
            } else {
                "Owned"
            };
            let value = "Owned { values:values,capacity:capacity }";
            let expression = if result {
                format!("Ok({value})")
            } else {
                value.to_owned()
            };
            let source = format!(
                "Capacity {{ a:i64,b:i64,c:i64,d:i64 }}\nPlan {{ capacity:Capacity }}\nOwned {{ values:array<i64>,capacity:Capacity }}\nfn construct({mode}plan:Plan)->{ret} {{capacity:=plan.capacity; values:=[1,2].to_array();return {expression}}}\n"
            );
            let base = lower(&source)?;
            validate_mir_producers(&base)?;
            let defined = base
                .fns
                .iter()
                .map(|f| f.name.clone())
                .collect::<BTreeSet<_>>();
            validate_partition_resource_rvalues(&base, &defined)?;
            for mutation in 0..4 {
                let mut bad = base.clone();
                let f = bad
                    .fns
                    .iter_mut()
                    .find(|f| f.name.as_str() == "construct")
                    .ok_or("construct fixture function")?;
                let (block, statement, value) = f
                    .blocks
                    .iter()
                    .enumerate()
                    .find_map(|(b, body)| {
                        body.stmts
                            .iter()
                            .enumerate()
                            .find_map(|(s, node)| match node {
                                Stmt::Let(value, Rvalue::Field(..)) => Some((b, s, *value)),
                                _ => None,
                            })
                    })
                    .ok_or("field producer fixture")?;
                match mutation {
                    0 => f.param_modes[0] = align_ast::ParamMode::Out,
                    1 => {
                        let Stmt::Let(_, Rvalue::Field(_, path)) =
                            &mut f.blocks[block].stmts[statement]
                        else {
                            panic!("field")
                        };
                        path.push(u32::MAX);
                    }
                    2 => f.value_tys[value as usize] = Ty::Bool,
                    _ => {
                        let target = f
                            .blocks
                            .iter()
                            .flat_map(|b| &b.stmts)
                            .find_map(|s| match s {
                                Stmt::Store(slot, Operand::Value(v)) if *v == value => Some(*slot),
                                _ => None,
                            })
                            .ok_or("field publication fixture")?;
                        f.blocks[block].stmts[statement] = Stmt::Let(value, Rvalue::Load(target));
                    }
                }
                assert!(
                    validate_mir_producers(&bad).is_err(),
                    "{mode}/{result}/{mutation}"
                );
                assert!(validate_partition_resource_rvalues(&bad, &defined).is_err());
            }
        }
    }
    // An independent Copy sibling never launders borrowed owning storage.
    for owner in ["array<i64>", "string", "[string;0]"] {
        let source = format!(
            "Capacity {{ n:i64 }}\nPlan {{ capacity:Capacity }}\nOwned {{ values:{owner},capacity:Capacity }}\nfn construct(borrow plan:Plan,values:{owner})->Owned {{capacity:=plan.capacity;return Owned {{values:values,capacity:capacity}}}}\n"
        );
        let base = lower(&source)?;
        validate_mir_producers(&base)?;
        for mode in [
            align_ast::ParamMode::Borrow,
            align_ast::ParamMode::BorrowMut,
            align_ast::ParamMode::Out,
        ] {
            let mut bad = base.clone();
            let f = bad
                .fns
                .iter_mut()
                .find(|f| f.name.as_str() == "construct")
                .ok_or("construct fixture function")?;
            f.param_modes[1] = mode;
            assert!(validate_mir_producers(&bad).is_err(), "{owner}/{mode:?}");
        }
    }
    Ok(())
}

#[test]
fn task_owned_complete_graph() -> Result<(), Box<dyn std::error::Error>> {
    let mut program = lower("Plain { value:i64 }\nfn main() {}\n")?;
    let plain = program.structs.iter().position(|s| s.name == "Plain").ok_or("Plain type")?;
    let id = u32::try_from(plain)?;
    let admitted = |program: &Program, root| align_sema::task_result_type(root,
        &program.structs, &program.tuples, &program.enums, &program.tagged_types);
    let int=Scalar::Int(IntTy {bits:64,signed:true});
    for leaf in [Ty::String,Ty::Buffer,Ty::DynArray(int),Ty::DynArray(Scalar::String),
        Ty::Option(Scalar::String),Ty::Result(Scalar::String,int),Ty::Array(Scalar::String,0),
        Ty::Array(Scalar::String,2),Ty::DynStructArray(id,Layout::Aos)] {
        program.structs.get_mut(plain).ok_or("record")?.fields.get_mut(0).ok_or("field")?.ty=leaf;
        assert!(admitted(&program,Ty::Struct(id)),"{leaf:?}");
        assert!(admitted(&program,Ty::DynStructArray(id,Layout::Aos)),"{leaf:?}");
    }
    for leaf in [Ty::Str,Ty::Raw,Ty::Slice(int),Ty::Fn(0),Ty::Box(int),Ty::Task(int),
        Ty::ArenaHandle,Ty::Reader,Ty::Resource(0),Ty::ResourceRef(0),Ty::Error,
        Ty::DynArray(Scalar::Str),Ty::DynArray(Scalar::Struct(id)),
        Ty::DynArray(Scalar::DynArray(align_sema::PrimScalar::String)),Ty::Array(Scalar::Str,0),Ty::Option(Scalar::Str),
        Ty::DynStructArray(id,Layout::Soa),Ty::Struct(u32::MAX),Ty::Tuple(u32::MAX),
        Ty::Tagged(u32::MAX),Ty::Struct(id),Ty::StructArray(id,0)] {
        program.structs.get_mut(plain).ok_or("record")?.fields.get_mut(0).ok_or("field")?.ty=leaf;
        assert!(!admitted(&program,Ty::Struct(id)),"{leaf:?}");
        assert!(!admitted(&program,Ty::DynStructArray(id,Layout::Aos)),"{leaf:?}");
    }
    for root in [Ty::Array(int,2),Ty::Vec(int,4),Ty::Mask(int,4),Ty::Rng,
        Ty::Option(int),Ty::Result(int,int)] {
        assert!(!admitted(&program,root),"root {root:?}");
    }
    // A header-reached node must not hide its inline cycle when an inline sibling reaches it.
    let second_id=u32::try_from(program.structs.len())?;
    let mut second=program.structs.get(plain).ok_or("record")?.clone();
    second.fields.get_mut(0).ok_or("field")?.ty=Ty::Struct(id);
    program.structs.push(second);
    for reverse in [false,true] {
        let record=program.structs.get_mut(plain).ok_or("record")?;
        record.fields=vec![
            align_sema::FieldDef {name:"inline".into(),ty:Ty::Struct(second_id)},
            align_sema::FieldDef {name:"header".into(),ty:Ty::DynStructArray(second_id,Layout::Aos)},
        ];
        if reverse {record.fields.reverse();}
        assert!(!admitted(&program,Ty::Struct(id)),"header-masked inline cycle/{reverse}");
    }
    Ok(())
}
