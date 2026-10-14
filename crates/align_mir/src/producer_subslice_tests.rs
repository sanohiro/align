//! Existing AoS subslices must authenticate their source without inventing ownership.
use super::*;

#[test]
fn record_subslice_producer_relations() -> Result<(), Box<dyn std::error::Error>> {
    for field in ["i64", "string"] {
        let source = format!(
            r#"
Row {{ value: {field} }}
Other {{ value: i64 }}
Stage {{ data: buffer, count: i64 }}
fn inspect(view: slice<Row>) -> Result<i64, Error> = Ok(view.len())
fn selected(borrow rows: array<Row>, start: i64, end: i64) -> Result<Stage, Error> {{
  view := rows[start..end]
  count := inspect(view)?
  return Ok(Stage {{ data: buffer.try_new(count)?, count: count }})
}}
fn other(value: Other) -> i64 = value.value
"#
        );
        let mut diagnostics = align_diag::Diagnostics::new();
        let tokens = align_lexer::tokenize(0, &source, &mut diagnostics);
        let ast = align_parser::parse_file(tokens, &mut diagnostics);
        let hir = align_sema::check_file(&ast, &mut diagnostics);
        assert!(
            !diagnostics.has_errors(),
            "{:?}",
            diagnostics.iter().collect::<Vec<_>>()
        );
        let base = crate::lower_program_checked(&hir, false, None)
            .map_err(|error| format!("{error:?}"))?;
        let defined = base
            .fns
            .iter()
            .map(|f| f.name.clone())
            .collect::<BTreeSet<_>>();
        validate_mir_producers(&base)?;
        validate_partition_resource_rvalues(&base, &defined)?;
        let other = u32::try_from(
            base.structs
                .iter()
                .position(|s| s.name == "Other")
                .ok_or("other record")?,
        )?;
        for mutation in 0..9 {
            let mut bad = base.clone();
            let f = bad
                .fns
                .iter_mut()
                .find(|f| f.name.as_str() == "selected")
                .ok_or("selected fixture")?;
            let (block, statement, result) = f
                .blocks
                .iter()
                .enumerate()
                .find_map(|(b, block)| {
                    block.stmts.iter().enumerate().find_map(|(s, statement)| {
                        if let Stmt::Let(value, Rvalue::SubSlice { .. }) = statement {
                            Some((b, s, *value))
                        } else {
                            None
                        }
                    })
                })
                .ok_or("subslice fixture")?;
            let body = f.blocks.get_mut(block).ok_or("block")?;
            let Stmt::Let(
                _,
                Rvalue::SubSlice {
                    base: input,
                    start,
                    len,
                    elem,
                },
            ) = body.stmts.get_mut(statement).ok_or("statement")?
            else {
                return Err("subslice".into());
            };
            let Ty::Struct(row) = *elem else {
                return Err("record stride".into());
            };
            match mutation {
                0 => *elem = Ty::Bool,
                1 => *start = Operand::Const(Const::Bool(true)),
                2 => *len = Operand::Const(Const::Bool(false)),
                3 => *input = Operand::Const(Const::Unit),
                4 => {
                    *f.value_tys
                        .get_mut(usize::try_from(result)?)
                        .ok_or("result type")? = Ty::DynStructArray(row, Layout::Aos)
                }
                5 => {
                    *f.value_tys
                        .get_mut(usize::try_from(result)?)
                        .ok_or("result type")? = Ty::Slice(Scalar::Struct(other))
                }
                6 => *elem = Ty::Struct(other),
                7 => {
                    let array = Ty::DynStructArray(row, Layout::Aos);
                    let columnar = Ty::DynStructArray(row, Layout::Soa);
                    for ty in f.slots.iter_mut().chain(&mut f.value_tys) {
                        if *ty == array {
                            *ty = columnar;
                        }
                    }
                    for statement in f.blocks.iter_mut().flat_map(|b| &mut b.stmts) {
                        if let Stmt::Let(_, Rvalue::Use(Operand::BorrowedPlace(place))) = statement
                            && place.ty == array
                        {
                            place.ty = columnar;
                        }
                    }
                }
                _ => {
                    let slot = u32::try_from(f.slots.len())?;
                    f.slots.push(Ty::DynStructArray(row, Layout::Aos));
                    let value = u32::try_from(f.value_tys.len())?;
                    f.value_tys.push(Ty::DynStructArray(row, Layout::Aos));
                    *input = Operand::Value(value);
                    body.stmts
                        .insert(statement, Stmt::Let(value, Rvalue::Load(slot)));
                    body.stmt_lines.clear();
                }
            }
            assert!(
                validate_mir_producers(&bad).is_err(),
                "{field}, mutation {mutation}"
            );
            assert!(
                validate_partition_resource_rvalues(&bad, &defined).is_err(),
                "partition {field}, mutation {mutation}"
            );
        }
    }
    Ok(())
}
