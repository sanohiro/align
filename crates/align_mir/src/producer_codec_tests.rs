//! Codec validation produces one nominal tagged value; every extraction proves its active arm.
use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn program() -> Result<Program, Box<dyn std::error::Error>> {
    let source = "import core.codec\n\
        pub fn open(input: slice<u8>) -> Result<codec.batch, Error> = codec.open(input)\n\
        pub fn ignored(input: slice<u8>) { _ := codec.open(input) }\n\
        pub fn unwrap(input: slice<u8>, prior: codec.batch) -> codec.batch = codec.open(input) else prior\n\
        pub fn rows(input: slice<u8>) -> i64 { batch := codec.open(input) else { return 0 }; return batch.rows() }\n\
        pub fn discarded(input: slice<u8>) { _ := codec.open(input) else { return } }\n\
        pub fn error(input: slice<u8>, prior: Error) -> Error = match codec.open(input) { Ok(_) => prior, Err(failure) => failure }\n\
        pub fn helper(input: slice<u8>, prior: codec.batch) -> codec.batch = open(input) else prior\n\
        pub fn parameter(result: Result<codec.batch, Error>, prior: codec.batch) -> codec.batch = result else prior\n\
        pub fn helper_error(input: slice<u8>, prior: Error) -> Error = match open(input) { Ok(_) => prior, Err(failure) => failure }\n\
        pub fn parameter_error(result: Result<codec.batch, Error>, prior: Error) -> Error = match result { Ok(_) => prior, Err(failure) => failure }\n\
        fn main() {}\n";
    let mut diagnostics = align_diag::Diagnostics::new();
    let tokens = align_lexer::tokenize(0, source, &mut diagnostics);
    let ast = align_parser::parse_file(tokens, &mut diagnostics);
    let hir = align_sema::check_file(&ast, &mut diagnostics);
    assert!(
        !diagnostics.has_errors(),
        "{:?}",
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
        validate_tagged_program(program).and_then(|_| validate_mir_producers(program).map(|_| ())),
        validate_partition_tagged_program(program, &defined)
            .and_then(|_| validate_partition_resource_rvalues(program, &defined)),
    ] {
        assert_eq!(result.is_ok(), accepted, "{label}: {result:?}");
    }
}

#[test]
fn codec_open_returns_one_complete_nominal_result() -> TestResult {
    let program = program()?;
    validate(&program, true, "baseline");
    for function in program
        .fns
        .iter()
        .filter(|f| matches!(f.name.as_str(), "open" | "ignored"))
    {
        let operations = function
            .blocks
            .iter()
            .flat_map(|b| &b.stmts)
            .filter_map(|statement| match statement {
                Stmt::Let(value, Rvalue::CodecOpen { error_enum, .. }) => {
                    Some((*value, *error_enum))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(operations.len(), 1);
        let (value, error_enum) = operations[0];
        assert_eq!(
            function.value_tys.get(value as usize),
            Some(&Ty::Result(Scalar::CodecBatch, Scalar::Enum(error_enum)))
        );
        assert!(!function.slots.contains(&Ty::CodecBatch));
        assert!(
            function
                .blocks
                .iter()
                .all(|b| !matches!(b.term, Term::Branch(..)))
        );
    }
    Ok(())
}

#[test]
fn codec_open_rejects_malformed_results_even_when_unused() -> TestResult {
    let baseline = program()?;
    for name in ["open", "ignored"] {
        for axis in [
            "input-type",
            "input-id",
            "argument-id",
            "uninitialized-input",
            "result-type",
            "enum-id",
            "enum-definition",
            "duplicate-result",
            "raw-result",
        ] {
            let mut bad = baseline.clone();
            let function = bad
                .fns
                .iter_mut()
                .find(|f| f.name.as_str() == name)
                .ok_or("function")?;
            let (block_index, statement_index, value, input, error_enum) = function
                .blocks
                .iter()
                .enumerate()
                .find_map(|(bi, b)| {
                    b.stmts.iter().enumerate().find_map(|(si, s)| {
                        if let Stmt::Let(value, Rvalue::CodecOpen { input, error_enum }) = s {
                            Some((bi, si, *value, input.clone(), *error_enum))
                        } else {
                            None
                        }
                    })
                })
                .ok_or("codec operation")?;
            let mut operation = Rvalue::CodecOpen {
                input: input.clone(),
                error_enum,
            };
            match axis {
                "input-type" => {
                    operation = Rvalue::CodecOpen {
                        input: Operand::Const(Const::Bool(true)),
                        error_enum,
                    }
                }
                "input-id" => {
                    operation = Rvalue::CodecOpen {
                        input: Operand::Value(u32::MAX),
                        error_enum,
                    }
                }
                "argument-id" => {
                    operation = Rvalue::CodecOpen {
                        input: Operand::Arg(u32::MAX),
                        error_enum,
                    }
                }
                "uninitialized-input" => {
                    let slot = u32::try_from(function.slots.len())?;
                    let ty = preflight_operand_ty(function, &input).ok_or("input type")?;
                    function.slots.push(ty);
                    let value = u32::try_from(function.value_tys.len())?;
                    function.value_tys.push(ty);
                    let block = function.blocks.get_mut(block_index).ok_or("block")?;
                    block.stmts.insert(0, Stmt::Let(value, Rvalue::Load(slot)));
                    block.stmt_lines.insert(0, (0, 0));
                    operation = Rvalue::CodecOpen {
                        input: Operand::Value(value),
                        error_enum,
                    };
                }
                "result-type" => {
                    *function.value_tys.get_mut(value as usize).ok_or("result")? = Ty::Bool
                }
                "enum-id" => {
                    operation = Rvalue::CodecOpen {
                        input: input.clone(),
                        error_enum: u32::MAX,
                    }
                }
                "enum-definition" => {
                    bad.enums
                        .get_mut(error_enum as usize)
                        .ok_or("Error")?
                        .source_name = "WrongError".into()
                }
                "duplicate-result" => {
                    let block = function.blocks.get_mut(block_index).ok_or("block")?;
                    block.stmts.push(Stmt::Let(value, operation.clone()));
                    block.stmt_lines.push((0, 0));
                }
                "raw-result" => operation = Rvalue::ResultOk(input),
                _ => return Err("axis".into()),
            }
            let position = statement_index + usize::from(axis == "uninitialized-input");
            *function
                .blocks
                .get_mut(block_index)
                .and_then(|b| b.stmts.get_mut(position))
                .ok_or("statement")? = Stmt::Let(value, operation);
            validate(&bad, false, &format!("{name}/{axis}"));
        }
    }
    Ok(())
}

fn imported(mut program: Program) -> Result<Program, Box<dyn std::error::Error>> {
    let index = program
        .fns
        .iter()
        .position(|f| f.name.as_str() == "open")
        .ok_or("open")?;
    let function = program.fns.remove(index);
    program.imported_fns.push(crate::ImportedFn {
        name: function.name.clone(),
        params: function
            .params
            .iter()
            .map(|slot| function.slots[*slot as usize])
            .collect(),
        param_modes: function.param_modes,
        ret: function.ret,
        return_borrow: function.return_borrow,
        return_region: function.return_region,
        return_cleanup: function.return_cleanup,
        drop_state_effects: program
            .drop_state_effects
            .get(&function.name)
            .cloned()
            .ok_or("drop state")?,
        producer_certified: true,
    });
    Ok(program)
}

#[test]
fn codec_payload_extraction_requires_the_exact_unique_tag_guard() -> TestResult {
    let baseline = program()?;
    for (mode, baseline) in [
        ("local", baseline.clone()),
        ("imported", imported(baseline)?),
    ] {
        validate(&baseline, true, mode);
        for name in [
            "unwrap",
            "rows",
            "discarded",
            "error",
            "helper",
            "parameter",
            "helper_error",
            "parameter_error",
        ] {
            for axis in [
                "unguarded",
                "reversed",
                "unrelated",
                "same-successor",
                "strmatch-bypass",
                "duplicate-predicate",
                "duplicate-extraction",
            ] {
                let mut bad = baseline.clone();
                let function = bad
                    .fns
                    .iter_mut()
                    .find(|f| f.name.as_str() == name)
                    .ok_or("function")?;
                let (use_block, extraction, operand, is_ok) = function
                    .blocks
                    .iter()
                    .find_map(|b| {
                        b.stmts.iter().find_map(|s| match s {
                            Stmt::Let(value, Rvalue::ResultUnwrapOk(input)) => {
                                Some((b.id, *value, input.clone(), true))
                            }
                            Stmt::Let(value, Rvalue::ResultUnwrapErr(input)) => {
                                Some((b.id, *value, input.clone(), false))
                            }
                            _ => None,
                        })
                    })
                    .ok_or("extraction")?;
                let guard_index = function
                    .blocks
                    .iter()
                    .position(|b| {
                        b.stmts.iter().any(|s| {
                            matches!(s,
                    Stmt::Let(_, Rvalue::ResultIsOk(input)) if matches!((input, &operand), (Operand::Value(a), Operand::Value(b)) | (Operand::Arg(a), Operand::Arg(b)) if a == b))
                        })
                    })
                    .ok_or("guard")?;
                let block = function.blocks.get_mut(guard_index).ok_or("guard block")?;
                let Term::Branch(Operand::Value(predicate), yes, no) = block.term.clone() else {
                    return Err("branch".into());
                };
                let selected = if is_ok { yes } else { no };
                match axis {
                    "unguarded" => block.term = Term::Goto(selected),
                    "reversed" => block.term = Term::Branch(Operand::Value(predicate), no, yes),
                    "same-successor" => {
                        block.term = Term::Branch(Operand::Value(predicate), selected, selected)
                    }
                    "unrelated" => {
                        let statement = block
                            .stmts
                            .iter_mut()
                            .find(|s| matches!(s, Stmt::Let(v, _) if *v == predicate))
                            .ok_or("predicate")?;
                        *statement =
                            Stmt::Let(predicate, Rvalue::Use(Operand::Const(Const::Bool(true))));
                    }
                    "duplicate-predicate" => {
                        block.stmts.push(Stmt::Let(
                            predicate,
                            Rvalue::Use(Operand::Const(Const::Bool(true))),
                        ));
                        block.stmt_lines.push((0, 0));
                    }
                    "duplicate-extraction" => {
                        let block = function
                            .blocks
                            .get_mut(use_block as usize)
                            .ok_or("use block")?;
                        let statement = block
                            .stmts
                            .iter()
                            .find(|s| matches!(s, Stmt::Let(v, _) if *v == extraction))
                            .ok_or("extraction")?
                            .clone();
                        block.stmts.push(statement);
                        block.stmt_lines.push((0, 0));
                    }
                    "strmatch-bypass" => {
                        let entry = u32::try_from(function.blocks.len())?;
                        let text = u32::try_from(function.value_tys.len())?;
                        function.value_tys.push(Ty::Str);
                        function.blocks.push(Block {
                            id: entry,
                            stmts: vec![Stmt::Let(text, Rvalue::StrLit("bypass".into()))],
                            stmt_lines: vec![(0, 0)],
                            term: Term::StrMatch {
                                scrutinee: Operand::Value(text),
                                cases: vec![("bypass".into(), selected)],
                                otherwise: function.entry,
                            },
                        });
                        function.entry = entry;
                    }
                    _ => return Err("axis".into()),
                }
                validate(&bad, false, &format!("{mode}/{name}/{axis}"));
            }
        }
    }
    Ok(())
}
