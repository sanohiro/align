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

#[test]
fn packed_bit_access_rejects_unfounded_headers_and_shared_writes() -> Result<(), Box<dyn std::error::Error>> {
    let source = "import core.bitset\nfn write(input:bitset) { mut bytes:=[0 as u8]; mut bits:=bitset.view(bytes[..],8); bits[0]=true; print(input.count()) }\nfn main(){}";
    let mut diags = align_diag::Diagnostics::new();
    let ast = align_parser::parse_file(align_lexer::tokenize(0,source,&mut diags),&mut diags);
    let hir = align_sema::check_file(&ast,&mut diags);
    assert!(!diags.has_errors());
    let good = crate::lower_program_checked(&hir, false, None).map_err(|error| format!("{error:?}"))?;
    let defined = good.fns.iter().map(|f| f.name.clone()).collect::<BTreeSet<_>>();
    validate_mir_producers(&good)?;
    for mutation in 0..4 {
        let mut bad = good.clone();
        let function = bad.fns.iter_mut().find(|f| f.name.as_str()=="write").ok_or("missing function")?;
        let (id, operands) = function.blocks.iter_mut().flat_map(|b| &mut b.stmts).find_map(|s| match s {
            Stmt::Let(id, Rvalue::Bitset { op: crate::BitsetOp::Write, operands }) => Some((*id, operands)),
            _ => None,
        }).ok_or("missing write")?;
        match mutation {
            0 => operands[0] = Operand::Arg(0),
            1 => operands[1] = Operand::Const(Const::Bool(false)),
            2 => { operands.pop(); }
            _ => function.value_tys[id as usize] = Ty::Bool,
        }
        assert!(validate_mir_producers(&bad).is_err(), "whole, mutation {mutation}");
        assert!(validate_partition_resource_rvalues(&bad, &defined).is_err(), "partition, mutation {mutation}");
    }
    Ok(())
}

#[test]
fn packed_bit_callers_authenticate_current_backing() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"import core.bitset
fn keep(borrow mut target:bitset, replacement:bitset) { target[0] = true }
fn change(borrow mut target:bitset, replacement:bitset) { target = replacement }
fn caller(shared:bitset) {
  mut data := [0 as u8]
  mut bits := bitset.view(data[..],8)
  keep(bits, shared)
  bits[1] = true
}
fn main() {}"#;
    let mut diags = align_diag::Diagnostics::new();
    let ast = align_parser::parse_file(align_lexer::tokenize(0, source, &mut diags), &mut diags);
    let hir = align_sema::check_file(&ast, &mut diags);
    assert!(!diags.has_errors(), "{:?}", diags.iter().collect::<Vec<_>>());
    let good = crate::lower_program_checked(&hir, false, None).map_err(|error| format!("{error:?}"))?;
    let defined = good.fns.iter().map(|function| function.name.clone()).collect::<BTreeSet<_>>();
    let change = good.fns.iter().find(|f| f.name.as_str() == "change").ok_or("change")?.name.clone();
    validate_mir_producers(&good)?;
    validate_partition_resource_rvalues(&good, &defined)?;
    for rebind in [true, false] {
        let mut bad = good.clone();
        let function = bad.fns.iter_mut().find(|f| f.name.as_str() == "caller").ok_or("caller")?;
        let (callee, args) = function.blocks.iter_mut().flat_map(|block| &mut block.stmts).find_map(|stmt| match stmt {
            Stmt::Let(_, Rvalue::Call(DirectCall::Program(callee), args)) if callee.as_str() == "keep" => Some((callee, args)),
            _ => None,
        }).ok_or("call to keep")?;
        if rebind {
            *callee = change.clone();
        } else if let Some(Operand::BorrowedPlace(place)) = args.first_mut() {
            place.slot = function.params[0];
        } else {
            return Err("missing mutable place".into());
        }
        assert!(validate_mir_producers(&bad).is_err(), "whole, rebind={rebind}");
        assert!(validate_partition_resource_rvalues(&bad, &defined).is_err(), "partition, rebind={rebind}");
    }
    Ok(())
}

#[test]
fn packed_reads_require_a_dominating_header_and_native_value_lanes() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"import core.bitset
Holder { bits:bitset }
fn inspect(borrow header:bitset, borrow position:i64, borrow flag:bool) -> bool {
  mut bytes := [1 as u8]
  mut view := bitset.view(bytes[..],8)
  view[position] = flag
  return header[position]
}
fn card(data:slice<u8>, flag:bool) -> i64 {
  mut bits := bitset.view(data,8)
  if flag { bits = bitset.view(data,4) }
  return bits.count()
}
fn carried(data:slice<u8>, flag:bool) -> i64 {
  mut record := Holder {bits:bitset.view(data,8)}
  if flag { record.bits = bitset.view(data,4) }
  return record.bits.count()
}
fn bytes(data:slice<u8>, flag:bool) -> i64 {
  mut alias := data
  if flag { alias = data }
  return bitset.view(alias,1).count()
}
fn main() {}"#;
    let mut diags = align_diag::Diagnostics::new();
    let ast = align_parser::parse_file(align_lexer::tokenize(0, source, &mut diags), &mut diags);
    let hir = align_sema::check_file(&ast, &mut diags);
    assert!(!diags.has_errors(), "{:?}", diags.iter().collect::<Vec<_>>());
    let good = crate::lower_program_checked(&hir, false, None).map_err(|error| format!("{error:?}"))?;
    let defined = good.fns.iter().map(|function| function.name.clone()).collect::<BTreeSet<_>>();
    validate_mir_producers(&good)?;
    validate_partition_resource_rvalues(&good, &defined)?;
    for mutation in 0..7 {
        let mut bad = good.clone();
        let name = match mutation { 3 | 5 => "card", 4 => "carried", 6 => "bytes", _ => "inspect" };
        let f = bad.fns.iter_mut().find(|f| f.name.as_str() == name).ok_or("fixture function")?;
        if mutation < 3 {
            let op = if mutation == 2 { crate::BitsetOp::Write } else { crate::BitsetOp::Read };
            let args = f.blocks.iter_mut().flat_map(|b| &mut b.stmts).find_map(|s| match s {
                Stmt::Let(_, Rvalue::Bitset { op: found, operands }) if *found == op => Some(operands),
                _ => None,
            }).ok_or("packed operation")?;
            args[mutation] = Operand::Arg(u32::try_from(mutation)?);
        } else {
            let slot = f.blocks.iter().flat_map(|b| &b.stmts).rev().find_map(|s| match s {
                Stmt::Let(id, Rvalue::Load(slot) | Rvalue::Field(slot, _))
                    if (mutation != 6 && f.value_tys[*id as usize] == Ty::Bitset)
                        || (mutation == 6 && matches!(f.value_tys[*id as usize], Ty::Slice(_))) => Some(*slot),
                _ => None,
            }).ok_or("loaded descriptor")?;
            let entry = f.blocks.get_mut(f.entry as usize).ok_or("entry")?;
            let initial = entry.stmts.iter().position(|s| matches!(s,
                Stmt::Store(dest, _) | Stmt::StoreField(dest, _, _) if *dest == slot)).ok_or("initial store")?;
            let store = entry.stmts.remove(initial);
            if mutation == 5 {
                let final_block = f.blocks.iter_mut().find(|b| matches!(b.term, Term::Return(_))).ok_or("return")?;
                final_block.stmts.push(store);
            }
        }
        assert!(validate_mir_producers(&bad).is_err(), "whole, mutation {mutation}");
        assert!(validate_partition_resource_rvalues(&bad, &defined).is_err(), "partition, mutation {mutation}");
    }
    Ok(())
}
