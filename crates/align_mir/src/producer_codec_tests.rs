//! Nominal codec batches use exact, private, shared native outputs even when discarded.
use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn program() -> Result<Program, Box<dyn std::error::Error>> {
    let source = "import core.codec\n\
        pub fn open(input: slice<u8>, prior: codec.batch) -> Result<codec.batch, Error> = codec.open(input)\n\
        pub fn ignored(input: slice<u8>, prior: codec.batch) { _ := codec.open(input) }\n\
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
fn codec_open_has_a_typed_shared_batch_output() -> TestResult {
    let program = program()?;
    validate(&program, true, "baseline");
    for function in program.fns.iter().filter(|f| f.name.as_str() != "main") {
        let (status, operation, out) = function
            .blocks
            .iter()
            .flat_map(|b| &b.stmts)
            .find_map(|statement| match statement {
                Stmt::Let(status, operation @ Rvalue::CodecOpen { out, .. }) => {
                    Some((*status, operation, *out))
                }
                _ => None,
            })
            .ok_or("codec operation")?;
        let contract =
            native_owner_mir_contract(&program, function, operation).ok_or("codec contract")?;
        assert_eq!(
            function.value_tys.get(status as usize),
            Some(&contract.result)
        );
        assert_eq!(function.slots.get(out as usize), Some(&Ty::CodecBatch));
        assert_eq!(
            xml_written_slots(operation),
            vec![(out, XmlAccessProvenance::Shared)]
        );
        assert_eq!(xml_out_producer_operands(operation).len(), 1);
        for statement in function.blocks.iter().flat_map(|b| &b.stmts) {
            if let Stmt::Let(_, Rvalue::ResultOk(operand)) = statement {
                assert_eq!(
                    preflight_operand_ty(function, operand),
                    Some(Ty::CodecBatch)
                );
            }
        }
    }
    Ok(())
}

#[test]
fn codec_open_rejects_malformed_outputs_even_when_unused() -> TestResult {
    let baseline = program()?;
    for name in ["open", "ignored"] {
        for axis in [
            "input-type",
            "input-id",
            "uninitialized-input",
            "output-type",
            "output-id",
            "parameter-output",
            "stored-output",
            "duplicate-output",
            "status-type",
            "raw-result",
        ] {
            let mut bad = baseline.clone();
            let function = bad
                .fns
                .iter_mut()
                .find(|f| f.name.as_str() == name)
                .ok_or("function")?;
            let (block_index, statement_index, status, input, out) = function
                .blocks
                .iter()
                .enumerate()
                .find_map(|(block_index, block)| {
                    block
                        .stmts
                        .iter()
                        .enumerate()
                        .find_map(|(statement_index, statement)| {
                            if let Stmt::Let(status, Rvalue::CodecOpen { input, out }) = statement {
                                Some((block_index, statement_index, *status, input.clone(), *out))
                            } else {
                                None
                            }
                        })
                })
                .ok_or("codec operation")?;
            let mut operation = Rvalue::CodecOpen {
                input: input.clone(),
                out,
            };
            match axis {
                "input-type" => {
                    operation = Rvalue::CodecOpen {
                        input: Operand::Const(Const::Bool(true)),
                        out,
                    }
                }
                "input-id" => {
                    operation = Rvalue::CodecOpen {
                        input: Operand::Value(u32::MAX),
                        out,
                    }
                }
                "uninitialized-input" => {
                    let slot = u32::try_from(function.slots.len())?;
                    function
                        .slots
                        .push(preflight_operand_ty(function, &input).ok_or("input type")?);
                    let value = u32::try_from(function.value_tys.len())?;
                    function
                        .value_tys
                        .push(preflight_operand_ty(function, &input).ok_or("input type")?);
                    let block = function.blocks.get_mut(block_index).ok_or("block")?;
                    block.stmts.insert(0, Stmt::Let(value, Rvalue::Load(slot)));
                    block.stmt_lines.insert(0, (0, 0));
                    operation = Rvalue::CodecOpen {
                        input: Operand::Value(value),
                        out,
                    };
                }
                "output-type" => *function.slots.get_mut(out as usize).ok_or("out slot")? = Ty::Str,
                "output-id" => {
                    operation = Rvalue::CodecOpen {
                        input: input.clone(),
                        out: u32::MAX,
                    }
                }
                "parameter-output" => {
                    operation = Rvalue::CodecOpen {
                        input: input.clone(),
                        out: *function.params.get(1).ok_or("prior parameter")?,
                    }
                }
                "stored-output" => {
                    let block = function.blocks.get_mut(block_index).ok_or("block")?;
                    block.stmts.push(Stmt::Store(out, Operand::Arg(1)));
                    block.stmt_lines.push((0, 0));
                }
                "duplicate-output" => {
                    let value = u32::try_from(function.value_tys.len())?;
                    function.value_tys.push(
                        *function
                            .value_tys
                            .get(status as usize)
                            .ok_or("status type")?,
                    );
                    let block = function.blocks.get_mut(block_index).ok_or("block")?;
                    block.stmts.push(Stmt::Let(value, operation.clone()));
                    block.stmt_lines.push((0, 0));
                }
                "status-type" => {
                    *function
                        .value_tys
                        .get_mut(status as usize)
                        .ok_or("status type")? = Ty::Bool
                }
                "raw-result" => {
                    let payload = function
                        .blocks
                        .iter_mut()
                        .flat_map(|b| &mut b.stmts)
                        .find_map(|s| {
                            if let Stmt::Let(_, Rvalue::ResultOk(payload)) = s {
                                Some(payload)
                            } else {
                                None
                            }
                        })
                        .ok_or("ResultOk")?;
                    *payload = input.clone();
                }
                _ => return Err("unknown axis".into()),
            }
            let position = statement_index + usize::from(axis == "uninitialized-input");
            let statement = function
                .blocks
                .get_mut(block_index)
                .and_then(|b| b.stmts.get_mut(position))
                .ok_or("codec statement")?;
            *statement = Stmt::Let(status, operation);
            validate(&bad, false, &format!("{name}/{axis}"));
        }
    }
    Ok(())
}
