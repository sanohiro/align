//! Plan195: readable payloads and exclusive owner storage are independent proofs.
use super::*;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn lower(source: &str) -> TestResult<Program> {
    let mut diagnostics = align_diag::Diagnostics::new();
    let tokens = align_lexer::tokenize(0, source, &mut diagnostics);
    let ast = align_parser::parse_file(tokens, &mut diagnostics);
    let hir = align_sema::check_file(&ast, &mut diagnostics);
    assert!(
        !diagnostics.has_errors(),
        "{source}\n{:?}",
        diagnostics.iter().collect::<Vec<_>>()
    );
    Ok(crate::lower_program_checked(&hir, false, None).map_err(|e| format!("{e:?}"))?)
}
fn validate(program: &Program, accepted: bool, label: &str) {
    let defined = program
        .fns
        .iter()
        .map(|f| f.name.clone())
        .collect::<BTreeSet<_>>();
    for result in [
        validate_mir_producers(program).map(|_| ()),
        validate_partition_resource_rvalues(program, &defined),
    ] {
        assert_eq!(result.is_ok(), accepted, "{label}: {result:?}");
    }
}

#[test]
fn mutable_borrow_call_shape_arena_matrix() -> TestResult {
    for arena in [false, true] {
        for text in [false, true] {
            for shape in ["root", "record", "fixed"] {
                let (ty, old, new) = if text {
                    ("str", "\"old\"", "\"new\"")
                } else {
                    ("i64", "1", "2")
                };
                let (decl, argument, field, init) = match shape {
                    "root" => (
                        String::new(),
                        format!("array<{ty}>"),
                        "r",
                        format!("[{old}].to_array()"),
                    ),
                    "record" => (
                        format!("R {{a:array<{ty}>}}"),
                        "R".into(),
                        "r.a",
                        format!("R{{a:[{old}].to_array()}}"),
                    ),
                    _ => (
                        format!("R {{a:[{ty};1]}}"),
                        "R".into(),
                        "r.a",
                        format!("R{{a:[{old}]}}"),
                    ),
                };
                let body = format!("mut r:={init};change(r);print({field}[0])");
                let body = if arena {
                    format!("arena {{{body}}}")
                } else {
                    body
                };
                let source = format!(
                    "{decl}\nfn change(borrow mut r:{argument}){{{field}[0]={new}}}\nfn main(){{{body}}}"
                );
                let program = lower(&source)?;
                validate(&program, true, &format!("{shape}/{text}/{arena}"));
            }
        }
    }
    Ok(())
}

#[test]
fn mutable_borrow_call_authority_matrix() -> TestResult {
    for nested in [false, true] {
        for owner in ["array<i64>", "string"] {
            let fresh = if owner == "string" {
                "\"own\".clone()"
            } else {
                "[1].to_array()"
            };
            let row = format!("Row{{text:\"live\",numbers:{fresh}}}");
            let (ty, init) = if nested {
                ("array<Row>", "builder.build()".to_string())
            } else {
                ("Row", row)
            };
            let setup = if nested {
                format!(
                    "mut builder:array_builder<Row>:=array_builder();builder.push(Row{{text:\"live\".clone(),numbers:{fresh}}});"
                )
            } else {
                String::new()
            };
            let text = if nested { "string" } else { "str" };
            let source = format!(
                "Row{{text:{text},numbers:{owner}}}\nfn change(borrow mut rows:{ty}){{}}\nfn forward(borrow shared:{owner}){{{setup}mut rows:={init};change(rows)}}\nfn main(){{}}"
            );
            let base = lower(&source)?;
            validate(&base, true, &source);
            for mode in [align_ast::ParamMode::Borrow, align_ast::ParamMode::Out] {
                let mut bad = base.clone();
                let row_id = u32::try_from(
                    bad.structs
                        .iter()
                        .position(|s| s.name == "Row")
                        .ok_or("Row")?,
                )?;
                let f = bad
                    .fns
                    .iter_mut()
                    .find(|f| f.name.as_str() == "forward")
                    .ok_or("forward")?;
                f.param_modes[0] = mode;
                let mut changed = 0;
                for statement in f.blocks.iter_mut().flat_map(|b| &mut b.stmts) {
                    if let Stmt::StoreField(slot, fields, input) = statement
                        && f.slots.get(*slot as usize) == Some(&Ty::Struct(row_id))
                        && fields.as_slice() == [1]
                    {
                        *input = Operand::Arg(0);
                        changed += 1;
                    }
                }
                assert_eq!(changed, 1, "exact inner owner writer");
                validate(&bad, false, &format!("nested={nested}/{owner}/{mode:?}"));
            }
        }
    }
    Ok(())
}

#[test]
fn mutable_borrow_call_malformed_matrix() -> TestResult {
    let base = lower(
        "R{a:array<str>} fn write(borrow mut r:R){r.a[0]=\"new\"} fn main(){arena {mut r:=R{a:[\"old\"].to_array()};write(r)}}",
    )?;
    validate(&base, true, "arena base");
    for mutation in 0..9 {
        let mut bad = base.clone();
        let f = bad
            .fns
            .iter_mut()
            .find(|f| f.name.as_str() == "main")
            .ok_or("main")?;
        let (block, statement, allocation) = f
            .blocks
            .iter()
            .enumerate()
            .find_map(|(b, body)| {
                body.stmts
                    .iter()
                    .enumerate()
                    .find_map(|(s, node)| match node {
                        Stmt::Let(value, Rvalue::ArenaAlloc { .. }) => Some((b, s, *value)),
                        _ => None,
                    })
            })
            .ok_or("arena allocation")?;
        match mutation {
            0..=2 => {
                let Stmt::Let(
                    _,
                    Rvalue::ArenaAlloc {
                        count,
                        handle,
                        elem,
                    },
                ) = &mut f.blocks[block].stmts[statement]
                else {
                    panic!("allocation")
                };
                match mutation {
                    0 => *count = Operand::Const(Const::Bool(true)),
                    1 => *handle = Operand::Const(Const::Bool(true)),
                    _ => *elem = Ty::Bool,
                }
            }
            3 => {
                f.blocks[block].stmts[statement] =
                    Stmt::Let(allocation, Rvalue::Use(Operand::Value(allocation)))
            }
            4 => {
                let duplicate = f.blocks[block].stmts[statement].clone();
                f.blocks[block].stmts.push(duplicate);
            }
            5 => {
                f.blocks[block].stmts.remove(statement);
            }
            _ => {
                let place = f
                    .blocks
                    .iter_mut()
                    .flat_map(|b| &mut b.stmts)
                    .find_map(|s| match s {
                        Stmt::Let(_, Rvalue::Call(DirectCall::Program(target), args))
                            if target.as_str() == "write" =>
                        {
                            args.iter_mut().find_map(|a| match a {
                                Operand::BorrowedPlace(p) => Some(p),
                                _ => None,
                            })
                        }
                        _ => None,
                    })
                    .ok_or("borrow descriptor")?;
                match mutation {
                    6 => place.slot = u32::MAX,
                    7 => place
                        .path
                        .push(hir::BorrowedPathSegment::StructField(u32::MAX)),
                    _ => place.ty = Ty::Bool,
                }
            }
        }
        validate(&bad, false, &format!("allocation/descriptor/{mutation}"));
    }
    let base = lower(
        "R{a:[str;2]} fn write(borrow mut r:R){r.a[0]=\"new\"} fn main(){mut r:=R{a:[\"one\",\"two\"]};write(r)}",
    )?;
    validate(&base, true, "fixed base");
    for mutation in 0..3 {
        let mut bad = base.clone();
        let f = bad
            .fns
            .iter_mut()
            .find(|f| f.name.as_str() == "main")
            .ok_or("main")?;
        let (b, s) = f
            .blocks
            .iter()
            .enumerate()
            .find_map(|(b, body)| {
                body.stmts
                    .iter()
                    .position(|s| matches!(s, Stmt::StoreIndex(..)))
                    .map(|s| (b, s))
            })
            .ok_or("fixed writer")?;
        match mutation {
            0 => {
                f.blocks[b].stmts.remove(s);
            }
            1 => {
                let duplicate = f.blocks[b].stmts[s].clone();
                f.blocks[b].stmts.insert(s, duplicate);
            }
            _ => {
                let Stmt::StoreIndex(_, index, _) = &mut f.blocks[b].stmts[s] else {
                    panic!("index")
                };
                *index = Operand::Const(Const::Bool(true));
            }
        }
        validate(&bad, false, &format!("fixed construction/{mutation}"));
    }
    Ok(())
}

#[test]
fn mutable_borrow_call_forms_and_modes() -> TestResult {
    for indirect in [false, true] {
        for cleanup in [false, true] {
            let (ret, body) = if cleanup {
                ("array<i64>", "return [1].to_array()")
            } else {
                ("()", "")
            };
            let invoke = if indirect {
                "f:=change;unused:=f(r)"
            } else {
                "unused:=change(r)"
            };
            let source = format!(
                "R{{a:array<str>}} fn change(borrow mut r:R)->{ret}{{r.a[0]=\"new\";{body}}} fn forward(borrow mut r:R){{{invoke}}} fn main(){{}}"
            );
            let base = lower(&source)?;
            validate(&base, true, &source);
            let f = base
                .fns
                .iter()
                .find(|f| f.name.as_str() == "forward")
                .ok_or("forward")?;
            assert_eq!(
                f.blocks
                    .iter()
                    .flat_map(|b| &b.stmts)
                    .filter(|s| match s {
                        Stmt::Let(_, Rvalue::Call(DirectCall::Program(t), _)) =>
                            !indirect && !cleanup && t.as_str() == "change",
                        Stmt::Let(_, Rvalue::CallWithCleanup(c)) =>
                            !indirect && cleanup && c.target.as_str() == "change",
                        Stmt::Let(_, Rvalue::CallIndirect { .. }) => indirect && !cleanup,
                        Stmt::Let(_, Rvalue::CallIndirectWithCleanup(_)) => indirect && cleanup,
                        _ => false,
                    })
                    .count(),
                1,
                "exact call form"
            );
            for mode in [align_ast::ParamMode::Borrow, align_ast::ParamMode::Out] {
                let mut bad = base.clone();
                bad.fns
                    .iter_mut()
                    .find(|f| f.name.as_str() == "forward")
                    .ok_or("forward")?
                    .param_modes[0] = mode;
                validate(&bad, false, &format!("{indirect}/{cleanup}/{mode:?}"));
            }
        }
    }
    Ok(())
}

#[test]
fn mutable_borrow_call_owner_inventory() -> TestResult {
    let mut program = lower("Row{text:str,numbers:array<i64>} fn main(){}")?;
    let id = u32::try_from(
        program
            .structs
            .iter()
            .position(|s| s.name == "Row")
            .ok_or("Row")?,
    )?;
    let root = Ty::DynStructArray(id, Layout::Aos);
    let integer = Scalar::Int(IntTy {
        bits: 64,
        signed: true,
    });
    assert_eq!(
        xml_access_paths(&program, root, true),
        Some(vec![
            (root, vec![]),
            (
                Ty::DynArray(integer),
                vec![
                    XmlAccessPathSegment::Element,
                    XmlAccessPathSegment::StructField(1)
                ]
            ),
        ])
    );
    assert_eq!(
        xml_access_paths(&program, Ty::Slice(Scalar::Struct(id)), true),
        Some(vec![])
    );
    assert_eq!(
        xml_access_paths(&program, Ty::StructArray(id, 0), true),
        Some(vec![])
    );
    program.structs[id as usize].fields[1].ty = root;
    assert!(
        xml_access_paths(&program, root, true).is_none(),
        "recursive owner graph"
    );
    assert!(
        xml_access_paths(&program, Ty::Struct(u32::MAX), true).is_none(),
        "missing definition"
    );
    Ok(())
}
