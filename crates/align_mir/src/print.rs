//! Textual output of MIR (`alignc emit-mir`, `docs/impl/04-mir.md` §8).
//! Used to inspect the CFG and confirm lowering / optimizations (predictability).

use crate::{ty_name, Block, Const, Function, Operand, Program, Rvalue, Stmt, Term};
use align_ast::{BinOp, UnOp};
use std::fmt::Write;

pub fn program_to_string(p: &Program) -> String {
    let mut out = String::new();
    for f in &p.fns {
        fn_to_string(&mut out, f);
        out.push('\n');
    }
    out
}

fn fn_to_string(out: &mut String, f: &Function) {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|s| format!("_{s}: {}", ty_name(f.slots[*s as usize])))
        .collect();
    let _ = writeln!(out, "fn {}({}) -> {} {{", f.name, params.join(", "), ty_name(f.ret));
    for b in &f.blocks {
        block_to_string(out, b);
    }
    let _ = writeln!(out, "}}");
}

fn block_to_string(out: &mut String, b: &Block) {
    let _ = writeln!(out, "  bb{}:", b.id);
    for s in &b.stmts {
        match s {
            Stmt::Let(v, rv) => {
                let _ = writeln!(out, "    %{v} = {}", rvalue_str(rv));
            }
            Stmt::Store(slot, op) => {
                let _ = writeln!(out, "    _{slot} <- {}", operand_str(op));
            }
            Stmt::StoreField(slot, path, op) => {
                let path = path.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(".");
                let _ = writeln!(out, "    _{slot}.{path} <- {}", operand_str(op));
            }
            Stmt::StoreIndex(slot, idx, val) => {
                let _ = writeln!(out, "    _{slot}[{}] <- {}", operand_str(idx), operand_str(val));
            }
            Stmt::StoreElemField(slot, idx, path, val) => {
                let _ = writeln!(out, "    _{slot}[{}]{} <- {}", operand_str(idx), path_str(path), operand_str(val));
            }
            Stmt::PtrStore(ptr, idx, val) => {
                let _ = writeln!(out, "    {}[{}] <- {}", operand_str(ptr), operand_str(idx), operand_str(val));
            }
            Stmt::PtrStoreNoalias { ptr, index, value, scope } => {
                let _ = writeln!(out, "    {}[{}] <- {}  !alias.scope(out#{scope})", operand_str(ptr), operand_str(index), operand_str(value));
            }
            Stmt::VecStore { slice, index, value, n, .. } => {
                let _ = writeln!(out, "    {}[{}..+{n}] <- {}", operand_str(slice), operand_str(index), operand_str(value));
            }
            Stmt::StoreColumn { base, len, index, field, struct_id, value } => {
                let _ = writeln!(
                    out,
                    "    soa#{struct_id}({}, len={}).col{field}[{}] <- {}",
                    operand_str(base),
                    operand_str(len),
                    operand_str(index),
                    operand_str(value)
                );
            }
            Stmt::StoreElemFieldPtr { base, index, path, struct_id, value } => {
                let _ = writeln!(
                    out,
                    "    aos#{struct_id}({})[{}]{} <- {}",
                    operand_str(base),
                    operand_str(index),
                    path_str(path),
                    operand_str(value)
                );
            }
            Stmt::DropFlagInit(slot) => {
                let _ = writeln!(out, "    drop_init _{slot}");
            }
            Stmt::NullTupleField(slot, idx) => {
                let _ = writeln!(out, "    null _{slot}.{idx}");
            }
            Stmt::NullStructField(slot, idx) => {
                let _ = writeln!(out, "    null _{slot}.{idx}");
            }
            Stmt::Drop(slot) => {
                let _ = writeln!(out, "    drop _{slot}");
            }
            Stmt::DropElem(slot, idx, sid) => {
                let _ = writeln!(out, "    drop_elem _{slot}[{}] (struct#{sid})", operand_str(idx));
            }
            Stmt::DropElemField(slot, idx, path) => {
                let _ = writeln!(out, "    drop_elem_field _{slot}[{}]{}", operand_str(idx), path_str(path));
            }
            Stmt::DropValue(op) => {
                let _ = writeln!(out, "    drop_value {}", operand_str(op));
            }
            Stmt::ArenaEnd(op) => {
                let _ = writeln!(out, "    arena_end {}", operand_str(op));
            }
            Stmt::RawFree(op) => {
                let _ = writeln!(out, "    raw_free {}", operand_str(op));
            }
            Stmt::RawStore { ptr, offset, value } => {
                let _ = writeln!(out, "    raw_store {}[{}] <- {}", operand_str(ptr), operand_str(offset), operand_str(value));
            }
            Stmt::TgWait(op) => {
                let _ = writeln!(out, "    tg_wait {}", operand_str(op));
            }
            Stmt::TgEnd(op) => {
                let _ = writeln!(out, "    tg_end {}", operand_str(op));
            }
        }
    }
    match &b.term {
        Term::Goto(t) => {
            let _ = writeln!(out, "    goto bb{t}");
        }
        Term::Branch(c, t, e) => {
            let _ = writeln!(out, "    branch {} ? bb{t} : bb{e}", operand_str(c));
        }
        Term::Return(Some(op)) => {
            let _ = writeln!(out, "    return {}", operand_str(op));
        }
        Term::Return(None) => {
            let _ = writeln!(out, "    return");
        }
        Term::Unreachable => {
            let _ = writeln!(out, "    unreachable");
        }
    }
}

fn rvalue_str(rv: &Rvalue) -> String {
    match rv {
        Rvalue::Use(op) => operand_str(op),
        Rvalue::Load(slot) => format!("load _{slot}"),
        Rvalue::Un(op, a) => format!("{}{}", unop_str(*op), operand_str(a)),
        Rvalue::Cast { operand, from, to } => {
            format!("{} as {} (from {})", operand_str(operand), ty_name(*to), ty_name(*from))
        }
        Rvalue::Bin(op, a, b) => {
            format!("{} {} {}", operand_str(a), binop_str(*op), operand_str(b))
        }
        Rvalue::IntArith { op, mode, int_ty, a, b } => {
            let m = match mode {
                align_sema::ArithMode::Saturating => "saturating",
                align_sema::ArithMode::Checked => "checked",
            };
            format!("{m}({} {} {} : {})", operand_str(a), binop_str(*op), operand_str(b), ty_name(*int_ty))
        }
        Rvalue::MathOp { fn_, ty, operands } => {
            let f = match fn_ {
                align_sema::MathFn::Abs => "abs",
                align_sema::MathFn::Min => "min",
                align_sema::MathFn::Max => "max",
                align_sema::MathFn::Sqrt => "sqrt",
                align_sema::MathFn::Floor => "floor",
                align_sema::MathFn::Ceil => "ceil",
                align_sema::MathFn::Round => "round",
                align_sema::MathFn::Trunc => "trunc",
                align_sema::MathFn::Pow => "pow",
                align_sema::MathFn::Fma => "fma",
            };
            let a: Vec<String> = operands.iter().map(operand_str).collect();
            format!("{f}({}) : {}", a.join(", "), ty_name(*ty))
        }
        Rvalue::Call(name, args) => {
            let a: Vec<String> = args.iter().map(operand_str).collect();
            format!("call {name}({})", a.join(", "))
        }
        Rvalue::FnAddr(name) => format!("fn_addr {name}"),
        Rvalue::Closure { lifted, captures, .. } => {
            let c: Vec<String> = captures.iter().map(operand_str).collect();
            format!("closure {lifted} [{}]", c.join(", "))
        }
        Rvalue::CallIndirect { callee, args, .. } => {
            let a: Vec<String> = args.iter().map(operand_str).collect();
            format!("call_indirect {}({})", operand_str(callee), a.join(", "))
        }
        Rvalue::Field(slot, path) => format!("_{slot}.{}", path.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(".")),
        Rvalue::Select { cond, a, b } => format!("select({}, {}, {})", operand_str(cond), operand_str(a), operand_str(b)),
        Rvalue::SoaColumn { base, struct_id, field } => format!("soa_col(_{base}: struct#{struct_id}, .{field})"),
        Rvalue::OptionSome(op) => format!("Some({})", operand_str(op)),
        Rvalue::OptionNone => "None".to_string(),
        Rvalue::OptionIsSome(op) => format!("is_some({})", operand_str(op)),
        Rvalue::OptionUnwrap(op) => format!("unwrap({})", operand_str(op)),
        Rvalue::ResultOk(op) => format!("Ok({})", operand_str(op)),
        Rvalue::ResultErr(op) => format!("Err({})", operand_str(op)),
        Rvalue::ResultIsOk(op) => format!("is_ok({})", operand_str(op)),
        Rvalue::ResultUnwrapOk(op) => format!("unwrap_ok({})", operand_str(op)),
        Rvalue::ResultUnwrapErr(op) => format!("unwrap_err({})", operand_str(op)),
        Rvalue::MakeEnum { enum_id, variant, payload } => {
            let ps: Vec<String> = payload.iter().map(operand_str).collect();
            format!("enum#{enum_id}.{variant}({})", ps.join(", "))
        }
        Rvalue::EnumTagEq { enum_id, scrutinee, variant } => {
            format!("tag_eq(enum#{enum_id}, {}, {variant})", operand_str(scrutinee))
        }
        Rvalue::EnumPayload { enum_id, variant, slot, operand } => {
            format!("enum_payload(enum#{enum_id}.{variant}[{slot}], {})", operand_str(operand))
        }
        Rvalue::ArenaBegin => "arena_begin".to_string(),
        Rvalue::TgBegin => "tg_begin".to_string(),
        Rvalue::SpawnTask { closure, fallible, .. } => {
            format!("spawn_task{} {}", if *fallible { " fallible" } else { "" }, operand_str(closure))
        }
        Rvalue::TgWaitResult { tg, fallible } => {
            format!("tg_wait{} {}", if *fallible { " fallible" } else { "" }, operand_str(tg))
        }
        Rvalue::HeapAlloc(h, init) => format!("heap_alloc({}, {})", operand_str(h), operand_str(init)),
        Rvalue::RawAlloc(size) => format!("raw_alloc({})", operand_str(size)),
        Rvalue::RawLoad { ptr, offset, .. } => format!("raw_load({}[{}])", operand_str(ptr), operand_str(offset)),
        Rvalue::RawOffset { ptr, offset } => format!("raw_offset({}, {})", operand_str(ptr), operand_str(offset)),
        Rvalue::BoxGet(op) => format!("box_get({})", operand_str(op)),
        Rvalue::BoxClone(h, src) => format!("box_clone({}, {})", operand_str(h), operand_str(src)),
        Rvalue::Index(slot, idx) => format!("_{slot}[{}]", operand_str(idx)),
        Rvalue::IndexField(slot, idx, field) => format!("_{slot}[{}].{field}", operand_str(idx)),
        Rvalue::MakeVec { elems, elem, n } => {
            let parts: Vec<String> = elems.iter().map(operand_str).collect();
            format!("vec{n}<{}>[{}]", ty_name(*elem), parts.join(", "))
        }
        Rvalue::VecExtract { vec, lane, .. } => format!("{}[{lane}]", operand_str(vec)),
        Rvalue::VecInsert { vec, value, lane } => format!("insert({}, [{lane}] <- {})", operand_str(vec), operand_str(value)),
        Rvalue::VecSumWhere { vec, mask, .. } => format!("sum_where({}, {})", operand_str(vec), operand_str(mask)),
        Rvalue::VecDot { a, b, .. } => format!("dot({}, {})", operand_str(a), operand_str(b)),
        Rvalue::VecMinMax { vec, max, .. } => format!("{}({})", if *max { "vmax" } else { "vmin" }, operand_str(vec)),
        Rvalue::VecSum { vec, .. } => format!("vsum({})", operand_str(vec)),
        Rvalue::MaskAny { mask, .. } => format!("mask_any({})", operand_str(mask)),
        Rvalue::VecLoad { slice, index, n, .. } => format!("{}[{}..+{n}]", operand_str(slice), operand_str(index)),
        Rvalue::IndexFieldPtr { base, index, field, struct_id } => {
            format!("{}[{}].{field} (struct#{struct_id})", operand_str(base), operand_str(index))
        }
        Rvalue::IndexColumn { base, index, field, struct_id } => {
            format!("{}.col{field}[{}] (soa struct#{struct_id})", operand_str(base), operand_str(index))
        }
        Rvalue::SoaGather { base, index, struct_id } => {
            format!("soa_gather({}[{}] : struct#{struct_id})", operand_str(base), operand_str(index))
        }
        Rvalue::IndexPtr { base, index, struct_id } => {
            format!("{}[{}] (struct#{struct_id})", operand_str(base), operand_str(index))
        }
        Rvalue::MakeTuple { tuple_id, elems } => {
            let parts: Vec<String> = elems.iter().map(operand_str).collect();
            format!("tuple#{tuple_id}({})", parts.join(", "))
        }
        Rvalue::TupleIndex { tuple, index } => format!("{}.{index}", operand_str(tuple)),
        Rvalue::MakeSlice(slot, n) => format!("slice(_{slot}, {n})"),
        Rvalue::ArenaAlloc { handle, count, elem } => {
            format!("arena_alloc({}, {} x {})", operand_str(handle), operand_str(count), crate::ty_name(*elem))
        }
        Rvalue::HeapAllocBuf { count, elem } => {
            format!("heap_alloc({} x {})", operand_str(count), crate::ty_name(*elem))
        }
        Rvalue::SoaAlloc { handle, len, struct_id } => {
            format!("soa_alloc({}, {} rows x struct#{struct_id})", operand_str(handle), operand_str(len))
        }
        Rvalue::MakeDynArray { ptr, len } => {
            format!("array({}, {})", operand_str(ptr), operand_str(len))
        }
        Rvalue::GroupAgg { keys, vals, out_keys, out_vals, op } => {
            format!(
                "group_{:?}(keys={}, vals={} -> {}, {})",
                op,
                operand_str(keys),
                operand_str(vals),
                operand_str(out_keys),
                operand_str(out_vals)
            )
        }
        Rvalue::GroupAggStrCols { keys, vals, out_keys, out_vals, op } => {
            format!(
                "group_{:?}_str_cols(keys={}, vals={} -> {}, {})",
                op,
                operand_str(keys),
                operand_str(vals),
                operand_str(out_keys),
                operand_str(out_vals)
            )
        }
        Rvalue::GroupAggStr { base, struct_id, key_field, value_field, op, out_keys, out_vals } => {
            let val = value_field.map(|v| format!(".val{v}")).unwrap_or_default();
            format!(
                "group_{op:?}_str(base=slot{base} struct#{struct_id}.key{key_field}{val} -> {}, {})",
                operand_str(out_keys),
                operand_str(out_vals)
            )
        }
        Rvalue::GroupAggMultiStr { base, struct_id, key_field, aggs, out_keys, out_vals } => {
            let specs: Vec<String> = aggs
                .iter()
                .map(|(op, vf)| format!("{op:?}{}", vf.map(|v| format!("(.val{v})")).unwrap_or_default()))
                .collect();
            let outs: Vec<String> = out_vals.iter().map(operand_str).collect();
            format!(
                "group_multi_str(base=slot{base} struct#{struct_id}.key{key_field} [{}] -> keys={}, vals=[{}])",
                specs.join(", "),
                operand_str(out_keys),
                outs.join(", ")
            )
        }
        Rvalue::DictEncode { base, struct_id, key_field, out_ids, out_dict } => {
            format!(
                "dict_encode(base=slot{base} struct#{struct_id}.key{key_field} -> ids={}, dict={})",
                operand_str(out_ids),
                operand_str(out_dict)
            )
        }
        Rvalue::MakeDictEncoded { source, ids, dict } => {
            format!("dict_encoded{{source={}, ids={}, dict={}}}", operand_str(source), operand_str(ids), operand_str(dict))
        }
        Rvalue::DictField { base, idx } => format!("dict_field(slot{base}.{idx})"),
        Rvalue::GatherColumnI64 { source, struct_id, field, out } => {
            format!("gather_i64({} struct#{struct_id}.field{field} -> {})", operand_str(source), operand_str(out))
        }
        Rvalue::DictLookup { ids, n, dict, out } => {
            format!("dict_lookup(ids={}, n={}, dict={} -> {})", operand_str(ids), operand_str(n), operand_str(dict), operand_str(out))
        }
        Rvalue::Chunks { src, n, elem } => {
            format!("chunks({}, {} x {})", operand_str(src), operand_str(n), crate::ty_name(*elem))
        }
        Rvalue::ParMapParallel { src, func, elem_in, elem_out } => {
            format!("par_map[{}]({}: {} -> {})", func, operand_str(src), crate::ty_name(*elem_in), crate::ty_name(*elem_out))
        }
        Rvalue::SliceLen(op) => format!("slice_len({})", operand_str(op)),
        Rvalue::SlicePtr(op) => format!("slice_ptr({})", operand_str(op)),
        Rvalue::SliceIndex(s, idx) => format!("{}[{}]", operand_str(s), operand_str(idx)),
        Rvalue::SliceIndexNoalias { slice, index, scope } => format!("{}[{}] !alias.scope(in#{scope})", operand_str(slice), operand_str(index)),
        Rvalue::SubSlice { base, start, len, elem } => {
            format!("subslice({}, +{}, len={} : {})", operand_str(base), operand_str(start), operand_str(len), ty_name(*elem))
        }
        Rvalue::StrLit(s) => format!("{s:?}"),
        Rvalue::StrClone(op) => format!("str_clone({})", operand_str(op)),
        Rvalue::StrPredicate { kind, haystack, needle } => {
            let name = match kind {
                align_sema::hir::StrPredKind::Contains => "str_contains",
                align_sema::hir::StrPredKind::StartsWith => "str_starts_with",
                align_sema::hir::StrPredKind::EndsWith => "str_ends_with",
                align_sema::hir::StrPredKind::Find => "str_find",
                align_sema::hir::StrPredKind::Rfind => "str_rfind",
                align_sema::hir::StrPredKind::EqIgnoreCase => "str_eq_ignore_case",
            };
            format!("{name}({}, {})", operand_str(haystack), operand_str(needle))
        }
        Rvalue::StrTrim { kind, recv } => {
            let name = match kind {
                align_sema::hir::StrTrimKind::Both => "str_trim",
                align_sema::hir::StrTrimKind::Start => "str_trim_start",
                align_sema::hir::StrTrimKind::End => "str_trim_end",
            };
            format!("{name}({})", operand_str(recv))
        }
        Rvalue::BuilderNew { capacity } => format!("builder_new(cap={})", operand_str(capacity)),
        Rvalue::BuilderWriteStr(b, s) => format!("builder_write({}, {})", operand_str(b), operand_str(s)),
        Rvalue::BuilderWriteInt(b, n) => format!("builder_write_int({}, {})", operand_str(b), operand_str(n)),
        Rvalue::BuilderWriteBool(b, v) => format!("builder_write_bool({}, {})", operand_str(b), operand_str(v)),
        Rvalue::BuilderWriteChar(b, c) => format!("builder_write_char({}, {})", operand_str(b), operand_str(c)),
        Rvalue::BuilderWriteFloat(b, x) => format!("builder_write_float({}, {})", operand_str(b), operand_str(x)),
        Rvalue::BuilderWriteStrIntStr(b, s1, n, s2) => format!(
            "builder_write_str_int_str({}, {}, {}, {})",
            operand_str(b),
            operand_str(s1),
            operand_str(n),
            operand_str(s2)
        ),
        Rvalue::BuilderToString(op) => format!("builder_to_string({})", operand_str(op)),
        Rvalue::Template(pieces, _arena) => {
            let ps: Vec<String> = pieces
                .iter()
                .map(|p| match p {
                    crate::TemplatePiece::Static(s) => format!("{s:?}"),
                    crate::TemplatePiece::IntHole(o) => format!("int({})", operand_str(o)),
                    crate::TemplatePiece::StrHole(o) => format!("str({})", operand_str(o)),
                    crate::TemplatePiece::BoolHole(o) => format!("bool({})", operand_str(o)),
                    crate::TemplatePiece::CharHole(o) => format!("char({})", operand_str(o)),
                    crate::TemplatePiece::FloatHole(o) => format!("float({})", operand_str(o)),
                    crate::TemplatePiece::JsonStrHole(o) => format!("json_str({})", operand_str(o)),
                })
                .collect();
            format!("template[{}]", ps.join(", "))
        }
        Rvalue::JsonDecode { struct_id, input, out } => {
            format!("json_decode(struct#{struct_id}, {}, -> _{out})", operand_str(input))
        }
        Rvalue::JsonDecodeArray { elem, input, out } => {
            format!("json_decode_array({} x {}, -> _{out})", operand_str(input), crate::ty_name(*elem))
        }
        Rvalue::JsonDecodeStructArray { struct_id, input, out } => {
            format!("json_decode_struct_array(struct#{struct_id}, {}, -> _{out})", operand_str(input))
        }
        Rvalue::JsonDecodeSoa { struct_id, input, out, arena } => {
            format!("json_decode_soa(struct#{struct_id}, {}, arena={}, -> _{out})", operand_str(input), operand_str(arena))
        }
        Rvalue::FsReadFile { path, out } => {
            format!("fs_read_file({}, -> _{out})", operand_str(path))
        }
        Rvalue::ReaderOpen { path, out } => format!("fs_open({}, -> _{out})", operand_str(path)),
        Rvalue::WriterCreate { path, out } => format!("fs_create({}, -> _{out})", operand_str(path)),
        Rvalue::ReaderStdin => "io_reader_stdin()".to_string(),
        Rvalue::WriterStd { fd, buffered } => format!("io_writer_std(fd={fd}, buffered={buffered})"),
        Rvalue::ReaderRead(r, buf) => format!("io_read({}, {})", operand_str(r), operand_str(buf)),
        Rvalue::IoCopy(r, w) => format!("io_copy({}, {})", operand_str(r), operand_str(w)),
        Rvalue::WriterWrite(w, s) => format!("io_write({}, {})", operand_str(w), operand_str(s)),
        Rvalue::WriterWriteBuilder(w, b) => format!("io_write_builder({}, {})", operand_str(w), operand_str(b)),
        Rvalue::WriterFlush(w) => format!("io_flush({})", operand_str(w)),
        Rvalue::BufferNew(cap) => format!("buffer_new({})", operand_str(cap)),
        Rvalue::BufferBytes(buf) => format!("buffer_bytes({})", operand_str(buf)),
        Rvalue::BufferLen(buf) => format!("buffer_len({})", operand_str(buf)),
        Rvalue::FsWriteFile { path, data } => format!("fs_write_file({}, {})", operand_str(path), operand_str(data)),
        Rvalue::FsWriteFileBuilder { path, builder } => format!("fs_write_file_builder({}, {})", operand_str(path), operand_str(builder)),
        Rvalue::FsExists { path } => format!("fs_exists({})", operand_str(path)),
        Rvalue::FsRemove { path } => format!("fs_remove({})", operand_str(path)),
        Rvalue::FsReadDir { path, out } => format!("fs_read_dir({}, -> _{out})", operand_str(path)),
        Rvalue::DnsResolve { host, out } => format!("dns_resolve({}, -> _{out})", operand_str(host)),
        Rvalue::TcpConnect { host, port, out } => format!("tcp_connect({}, {}, -> _{out})", operand_str(host), operand_str(port)),
        Rvalue::ConnReader(c) => format!("conn_reader({})", operand_str(c)),
        Rvalue::ConnWriter(c) => format!("conn_writer({})", operand_str(c)),
        Rvalue::TcpListen { host, port, out } => format!("tcp_listen({}, {}, -> _{out})", operand_str(host), operand_str(port)),
        Rvalue::TcpAccept { listener, out } => format!("tcp_accept({}, -> _{out})", operand_str(listener)),
        Rvalue::UdpBind { host, port, out } => format!("udp_bind({}, {}, -> _{out})", operand_str(host), operand_str(port)),
        Rvalue::UdpSendTo { sock, data, host, port } => format!("udp_send_to({}, {}, {}, {})", operand_str(sock), operand_str(data), operand_str(host), operand_str(port)),
        Rvalue::UdpRecvFrom { sock, buffer } => format!("udp_recv_from({}, {})", operand_str(sock), operand_str(buffer)),
        Rvalue::ProcessSpawn { cmd, args, out } => format!("process_spawn({}, {}, -> _{out})", operand_str(cmd), operand_str(args)),
        Rvalue::ChildWait { child } => format!("child_wait({})", operand_str(child)),
        Rvalue::ChildKill { child, sig } => format!("child_kill({}, {})", operand_str(child), operand_str(sig)),
        Rvalue::ProcessExec { cmd, args } => format!("process_exec({}, {})", operand_str(cmd), operand_str(args)),
        Rvalue::FsReadFileView { path, arena, out } => {
            format!("fs_read_file_view({}, {}, -> _{out})", operand_str(path), operand_str(arena))
        }
        Rvalue::MakeError { enum_id, tag, code } => {
            format!("make_error(enum#{enum_id}, tag={}, code={})", operand_str(tag), operand_str(code))
        }
        Rvalue::PathJoin { a, b } => format!("path_join({}, {})", operand_str(a), operand_str(b)),
        Rvalue::PathComponent { kind, path } => format!("path_{kind:?}({})", operand_str(path)),
        Rvalue::PathNormalize { path } => format!("path_normalize({})", operand_str(path)),
        Rvalue::EnvGet { name, out } => format!("env_get({}, -> _{out})", operand_str(name)),
        Rvalue::EnvSet { name, value } => format!("env_set({}, {})", operand_str(name), operand_str(value)),
        Rvalue::TimeNow => "time_now()".to_string(),
        Rvalue::TimeInstant => "time_instant()".to_string(),
        Rvalue::TimeSleep { ns } => format!("time_sleep({})", operand_str(ns)),
        Rvalue::EncodingEncode { kind, data } => format!("encode_{kind:?}({})", operand_str(data)),
        Rvalue::EncodingDecode { kind, input, out } => format!("decode_{kind:?}({}, -> _{out})", operand_str(input)),
        Rvalue::CompressCompress { kind, data, level, out } => {
            format!("compress_{kind:?}({}, {}, -> _{out})", operand_str(data), operand_str(level))
        }
        Rvalue::CompressDecompress { kind, data, out } => {
            format!("decompress_{kind:?}({}, -> _{out})", operand_str(data))
        }
        Rvalue::Utf8Valid { data } => format!("utf8_valid({})", operand_str(data)),
        Rvalue::CryptoCtEqual { a, b } => format!("crypto_ct_equal({}, {})", operand_str(a), operand_str(b)),
        Rvalue::CryptoRandom { out } => format!("crypto_random({})", operand_str(out)),
        Rvalue::CryptoHash { algo, data } => {
            let name = match algo {
                align_sema::hir::HashAlgo::Sha256 => "sha256",
                align_sema::hir::HashAlgo::Sha512 => "sha512",
            };
            format!("crypto_{name}({})", operand_str(data))
        }
        Rvalue::CryptoHmac { key, data } => {
            format!("crypto_hmac_sha256({}, {})", operand_str(key), operand_str(data))
        }
        Rvalue::CryptoHkdf { salt, ikm, info, len, out } => format!(
            "crypto_hkdf_sha256({}, {}, {}, {}, -> _{out})",
            operand_str(salt),
            operand_str(ikm),
            operand_str(info),
            operand_str(len)
        ),
        Rvalue::CryptoAead { cipher, dir, key, nonce, input, aad, out } => {
            use align_sema::hir::{AeadCipher, AeadDir};
            let name = match (cipher, dir) {
                (AeadCipher::Aes256Gcm, AeadDir::Seal) => "aes_gcm_seal",
                (AeadCipher::Aes256Gcm, AeadDir::Open) => "aes_gcm_open",
                (AeadCipher::ChaCha20Poly1305, AeadDir::Seal) => "chacha20_poly1305_seal",
                (AeadCipher::ChaCha20Poly1305, AeadDir::Open) => "chacha20_poly1305_open",
            };
            format!(
                "crypto_{name}({}, {}, {}, {}, -> _{out})",
                operand_str(key),
                operand_str(nonce),
                operand_str(input),
                operand_str(aad)
            )
        }
        Rvalue::CryptoArgon2(a) => format!(
            "crypto_argon2id({}, {}, m={}, t={}, p={}, len={}, -> _{})",
            operand_str(&a.password),
            operand_str(&a.salt),
            operand_str(&a.m_cost),
            operand_str(&a.t_cost),
            operand_str(&a.parallelism),
            operand_str(&a.len),
            a.out
        ),
        Rvalue::RandSeed { seed: Some(s), out } => format!("rng_seed_with({}, -> _{out})", operand_str(s)),
        Rvalue::RandSeed { seed: None, out } => format!("rng_seed_os(-> _{out})"),
        Rvalue::RandNext { rng } => format!("rng_next(_{rng})"),
        Rvalue::RandRange { rng, lo, hi } => format!("rng_range(_{rng}, {}, {})", operand_str(lo), operand_str(hi)),
        Rvalue::RandShuffle { rng, xs, .. } => format!("rng_shuffle(_{rng}, {})", operand_str(xs)),
        Rvalue::RandSample { rng, xs, k, .. } => format!("rng_sample(_{rng}, {}, {})", operand_str(xs), operand_str(k)),
        Rvalue::CliCommand { name } => format!("cli_command({})", operand_str(name)),
        Rvalue::CliFlag { cmd, kind, name, default } => match default {
            Some(d) => format!("cli_flag_{kind:?}({}, {}, {})", operand_str(cmd), operand_str(name), operand_str(d)),
            None => format!("cli_flag_{kind:?}({}, {})", operand_str(cmd), operand_str(name)),
        },
        Rvalue::CliParse { cmd, args, out } => format!("cli_parse({}, {}, -> _{out})", operand_str(cmd), operand_str(args)),
        Rvalue::CliGetBool { parsed, name } => format!("cli_get_bool({}, {})", operand_str(parsed), operand_str(name)),
        Rvalue::CliGetI64 { parsed, name } => format!("cli_get_i64({}, {})", operand_str(parsed), operand_str(name)),
        Rvalue::CliGetStr { parsed, name } => format!("cli_get_str({}, {})", operand_str(parsed), operand_str(name)),
        Rvalue::CliUsage { cmd } => format!("cli_usage({})", operand_str(cmd)),
        Rvalue::HttpRequest { method, url } => format!("http_request({}, {})", operand_str(method), operand_str(url)),
        Rvalue::HttpHeader { req, name, value } => format!("http_header({}, {}, {})", operand_str(req), operand_str(name), operand_str(value)),
        Rvalue::HttpBody { req, data } => format!("http_body({}, {})", operand_str(req), operand_str(data)),
        Rvalue::HttpParse { data, out } => format!("http_parse({}, -> _{out})", operand_str(data)),
        Rvalue::HttpRespStatus { resp } => format!("http_resp_status({})", operand_str(resp)),
        Rvalue::HttpRespHeader { resp, name, out } => format!("http_resp_header({}, {}, -> _{out})", operand_str(resp), operand_str(name)),
        Rvalue::HttpRespBody { resp } => format!("http_resp_body({})", operand_str(resp)),
    }
}

/// A field path (`[0, 2]`) rendered as a dotted suffix (`.0.2`) for a place display.
fn path_str(path: &[u32]) -> String {
    path.iter().map(|i| format!(".{i}")).collect::<String>()
}

fn operand_str(op: &Operand) -> String {
    match op {
        Operand::Const(Const::Int(v, ty)) => format!("{v}_{}", ty_name(*ty)),
        Operand::Const(Const::Float(v, ty)) => format!("{v}_{}", ty_name(*ty)),
        Operand::Const(Const::Char(v)) => format!("'\\u{{{v:x}}}'"),
        Operand::Const(Const::Bool(v)) => v.to_string(),
        Operand::Const(Const::Unit) => "()".to_string(),
        Operand::Value(v) => format!("%{v}"),
        Operand::Arg(i) => format!("arg{i}"),
    }
}

fn unop_str(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "-",
        UnOp::Not => "!",
        UnOp::BitNot => "~",
    }
}

fn binop_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
    }
}
