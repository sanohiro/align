//! Lower the checked operations on borrowed packed bits. Address calculations use byte
//! quotients plus low three-bit remainders, including when the logical index is i64::MAX.
use super::*;
use align_mir::BitsetOp;
use inkwell::values::{PhiValue, PointerValue, StructValue};

#[derive(Clone, Copy)]
struct BitView<'c> {
    base: PointerValue<'c>,
    head: IntValue<'c>,
    len: IntValue<'c>,
}

/// Quotient and low bits keep the signed logical index representable at i64::MAX.
fn bit_position<'c>(
    builder: &inkwell::builder::Builder<'c>,
    head: IntValue<'c>,
    index: IntValue<'c>,
) -> Result<(IntValue<'c>, IntValue<'c>), inkwell::builder::BuilderError> {
    let ty = index.get_type();
    let three = ty.const_int(3, false);
    let seven = ty.const_int(7, false);
    let whole = builder.build_right_shift(index, three, false, "bits.whole")?;
    let low = builder.build_and(index, seven, "bits.low")?;
    let relative = builder.build_int_add(head, low, "bits.relative")?;
    let carry = builder.build_right_shift(relative, three, false, "bits.carry")?;
    let byte = builder.build_int_add(whole, carry, "bits.byte")?;
    let bit = builder.build_and(relative, seven, "bits.bit")?;
    Ok((byte, bit))
}

impl<'c> FnGen<'c, '_> {
    fn pack_bits(&self, view: BitView<'c>) -> Result<BasicValueEnum<'c>, CodegenError> {
        let descriptor = bitset_type(self.ctx).const_zero();
        let descriptor = self
            .builder
            .build_insert_value(descriptor, view.base, 0, "bits.base")
            .map_err(|error| self.err(error))?
            .into_struct_value();
        let descriptor = self
            .builder
            .build_insert_value(descriptor, view.head, 1, "bits.head")
            .map_err(|error| self.err(error))?
            .into_struct_value();
        Ok(self
            .builder
            .build_insert_value(descriptor, view.len, 2, "bits.view")
            .map_err(|error| self.err(error))?
            .into_struct_value()
            .into())
    }

    fn unpack_bits(&self, descriptor: StructValue<'c>) -> Result<BitView<'c>, CodegenError> {
        let field = |index, name| {
            self.builder
                .build_extract_value(descriptor, index, name)
                .map_err(|error| self.err(error))
        };
        Ok(BitView {
            base: field(0, "bits.base")?.into_pointer_value(),
            head: field(1, "bits.head")?.into_int_value(),
            len: field(2, "bits.len")?.into_int_value(),
        })
    }

    fn bit_byte_ptr(
        &self,
        base: PointerValue<'c>,
        byte: IntValue<'c>,
    ) -> Result<PointerValue<'c>, CodegenError> {
        // SAFETY: the dominated index/range guard or nonempty cardinality loop
        // establishes this exact byte within the borrowed allocation.
        unsafe {
            self.builder
                .build_gep(self.ctx.i8_type(), base, &[byte], "bits.address")
        }
        .map_err(|error| self.err(error))
    }

    pub(super) fn gen_bitset(
        &mut self,
        op: BitsetOp,
        operands: &[Operand],
    ) -> Result<Option<BasicValueEnum<'c>>, CodegenError> {
        let ty = self.ctx.i64_type();
        let zero = ty.const_zero();
        let byte = self.ctx.i8_type();
        let value = match (op, operands) {
            (BitsetOp::View, [storage, count]) => {
                let (base, capacity) = self.split_str(storage)?;
                let len = self.operand(count)?.into_int_value();
                let whole = self
                    .builder
                    .build_right_shift(len, ty.const_int(3, false), false, "bits.whole")
                    .map_err(|error| self.err(error))?;
                let tail = self
                    .builder
                    .build_and(len, ty.const_int(7, false), "bits.tail")
                    .map_err(|error| self.err(error))?;
                let partial = self
                    .builder
                    .build_int_compare(IntPredicate::NE, tail, zero, "bits.partial")
                    .map_err(|error| self.err(error))?;
                let extra = self
                    .builder
                    .build_int_z_extend(partial, ty, "bits.extra")
                    .map_err(|error| self.err(error))?;
                let span = self
                    .builder
                    .build_int_add(whole, extra, "bits.span")
                    .map_err(|error| self.err(error))?;
                let fits = self
                    .builder
                    .build_int_compare(
                        IntPredicate::SLE,
                        span,
                        capacity.into_int_value(),
                        "bits.fits",
                    )
                    .map_err(|error| self.err(error))?;
                let nonnegative = self
                    .builder
                    .build_int_compare(IntPredicate::SGE, len, zero, "bits.nonnegative")
                    .map_err(|error| self.err(error))?;
                let valid = self
                    .builder
                    .build_and(nonnegative, fits, "bits.valid")
                    .map_err(|error| self.err(error))?;
                self.xml_abort_unless(valid, "bitset.view")?;
                self.pack_bits(BitView {
                    base: base.into_pointer_value(),
                    head: zero,
                    len,
                })?
            }
            (BitsetOp::Window, [source, lo, hi]) => {
                let view = self.unpack_bits(self.operand_by_value(source)?.into_struct_value())?;
                let lo = self.operand(lo)?.into_int_value();
                let hi = self.operand(hi)?.into_int_value();
                let ordered = self
                    .builder
                    .build_int_compare(IntPredicate::ULE, lo, hi, "bits.ordered")
                    .map_err(|error| self.err(error))?;
                let bounded = self
                    .builder
                    .build_int_compare(IntPredicate::ULE, hi, view.len, "bits.bounded")
                    .map_err(|error| self.err(error))?;
                let valid = self
                    .builder
                    .build_and(ordered, bounded, "bits.valid")
                    .map_err(|error| self.err(error))?;
                self.xml_abort_unless(valid, "bitset.window")?;
                let len = self
                    .builder
                    .build_int_sub(hi, lo, "bits.length")
                    .map_err(|error| self.err(error))?;
                let empty = self
                    .builder
                    .build_int_compare(IntPredicate::EQ, len, zero, "bits.empty")
                    .map_err(|error| self.err(error))?;
                let empty_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| self.err("missing bit window block"))?;
                let nonempty = self.ctx.append_basic_block(self.func, "bits.nonempty");
                let join = self.ctx.append_basic_block(self.func, "bits.window");
                self.builder
                    .build_conditional_branch(empty, join, nonempty)
                    .map_err(|error| self.err(error))?;
                self.builder.position_at_end(nonempty);
                let (offset, head) =
                    bit_position(self.builder, view.head, lo).map_err(|e| self.err(e))?;
                let base = self.bit_byte_ptr(view.base, offset)?;
                self.builder
                    .build_unconditional_branch(join)
                    .map_err(|error| self.err(error))?;
                self.builder.position_at_end(join);
                let out_base = self
                    .builder
                    .build_phi(base.get_type(), "bits.window.base")
                    .map_err(|error| self.err(error))?;
                out_base.add_incoming(&[(&view.base, empty_block), (&base, nonempty)]);
                let out_head = self
                    .builder
                    .build_phi(ty, "bits.window.head")
                    .map_err(|error| self.err(error))?;
                out_head.add_incoming(&[(&zero, empty_block), (&head, nonempty)]);
                self.pack_bits(BitView {
                    base: out_base.as_basic_value().into_pointer_value(),
                    head: out_head.as_basic_value().into_int_value(),
                    len,
                })?
            }
            (BitsetOp::Len, [source]) => {
                let descriptor = self.operand_by_value(source)?.into_struct_value();
                self.builder
                    .build_extract_value(descriptor, 2, "bits.len")
                    .map_err(|error| self.err(error))?
            }
            (BitsetOp::Read, [source, index]) | (BitsetOp::Write, [source, index, _]) => {
                let view = self.unpack_bits(self.operand_by_value(source)?.into_struct_value())?;
                let index = self.operand(index)?.into_int_value();
                let valid = self
                    .builder
                    .build_int_compare(IntPredicate::ULT, index, view.len, "bits.bounded")
                    .map_err(|error| self.err(error))?;
                self.xml_abort_unless(valid, "bitset.index")?;
                let (offset, bit) =
                    bit_position(self.builder, view.head, index).map_err(|e| self.err(e))?;
                let address = self.bit_byte_ptr(view.base, offset)?;
                let bit = self
                    .builder
                    .build_int_truncate(bit, byte, "bits.shift")
                    .map_err(|error| self.err(error))?;
                let mask = self
                    .builder
                    .build_left_shift(byte.const_int(1, false), bit, "bits.mask")
                    .map_err(|error| self.err(error))?;
                let old = self
                    .builder
                    .build_load(byte, address, "bits.old")
                    .map_err(|error| self.err(error))?
                    .into_int_value();
                if let [_, _, flag] = operands {
                    let flag = self.operand(flag)?.into_int_value();
                    let yes = self
                        .builder
                        .build_or(old, mask, "bits.set")
                        .map_err(|error| self.err(error))?;
                    let inverse = self
                        .builder
                        .build_not(mask, "bits.inverse")
                        .map_err(|error| self.err(error))?;
                    let no = self
                        .builder
                        .build_and(old, inverse, "bits.clear")
                        .map_err(|error| self.err(error))?;
                    let new = self
                        .builder
                        .build_select(flag, yes, no, "bits.new")
                        .map_err(|error| self.err(error))?;
                    self.builder
                        .build_store(address, new)
                        .map_err(|error| self.err(error))?;
                    return Ok(None);
                }
                let selected = self
                    .builder
                    .build_and(old, mask, "bits.selected")
                    .map_err(|error| self.err(error))?;
                self.builder
                    .build_int_compare(IntPredicate::NE, selected, byte.const_zero(), "bits.read")
                    .map_err(|error| self.err(error))?
                    .into()
            }
            (BitsetOp::Count | BitsetOp::Any | BitsetOp::All, [source]) => {
                let view = self.unpack_bits(self.operand_by_value(source)?.into_struct_value())?;
                self.bit_terminal(view, op)?.into()
            }
            _ => return Err(self.err("malformed packed bit operands")),
        };
        Ok(Some(value))
    }

    /// A partial byte contributes its active bits only. Predicates reduce to a
    /// single question: did we see a one (any) or a zero (all)?
    fn bit_contribution(
        &self,
        base: PointerValue<'c>,
        index: IntValue<'c>,
        mask: IntValue<'c>,
        op: BitsetOp,
    ) -> Result<IntValue<'c>, CodegenError> {
        let b = self.builder;
        let ty = mask.get_type();
        let address = self.bit_byte_ptr(base, index)?;
        let raw = b
            .build_load(ty, address, "bits.raw")
            .map_err(|e| self.err(e))?;
        raw.as_instruction_value()
            .ok_or_else(|| self.err("missing packed load"))?
            .set_alignment(1)
            .map_err(|e| self.err(e))?;
        let raw = raw.into_int_value();
        let selected = b
            .build_and(raw, mask, "bits.active")
            .map_err(|e| self.err(e))?;
        if op == BitsetOp::Count {
            let pop = self
                .call_intrinsic("llvm.ctpop", &[ty.into()], &[selected.into()])?
                .into_int_value();
            return b
                .build_int_cast(pop, self.ctx.i64_type(), "bits.pop")
                .map_err(|e| self.err(e));
        }
        let expected = if op == BitsetOp::All {
            mask
        } else {
            ty.const_zero()
        };
        b.build_int_compare(IntPredicate::NE, selected, expected, "bits.decisive")
            .map_err(|e| self.err(e))
    }

    /// Scan [start, end) in complete units. The caller rounds this interval to
    /// a multiple of the unit width. A predicate leaves the entire reduction
    /// immediately on its first decisive load.
    fn bit_scan_units(
        &self,
        view: BitView<'c>,
        range: (IntValue<'c>, IntValue<'c>),
        ty: IntType<'c>,
        initial: IntValue<'c>,
        op: BitsetOp,
        result: PhiValue<'c>,
    ) -> Result<IntValue<'c>, CodegenError> {
        let (start, end) = range;
        let b = self.builder;
        let word = self.ctx.i64_type();
        let entry = b
            .get_insert_block()
            .ok_or_else(|| self.err("missing packed scan entry"))?;
        let body = self.ctx.append_basic_block(self.func, "bits.scan");
        let after = self.ctx.append_basic_block(self.func, "bits.scanned");
        let empty = b
            .build_int_compare(IntPredicate::EQ, start, end, "bits.skip")
            .map_err(|e| self.err(e))?;
        b.build_conditional_branch(empty, after, body)
            .map_err(|e| self.err(e))?;
        b.position_at_end(body);
        let offset = b.build_phi(word, "bits.offset").map_err(|e| self.err(e))?;
        let total = if op == BitsetOp::Count {
            Some(b.build_phi(word, "bits.total").map_err(|e| self.err(e))?)
        } else {
            None
        };
        let index = offset.as_basic_value().into_int_value();
        let contribution = self.bit_contribution(view.base, index, ty.const_all_ones(), op)?;
        let (latch, sum) = if let Some(total) = total {
            let sum = b
                .build_int_add(
                    total.as_basic_value().into_int_value(),
                    contribution,
                    "bits.sum",
                )
                .map_err(|e| self.err(e))?;
            total.add_incoming(&[(&initial, entry), (&sum, body)]);
            (body, sum)
        } else {
            let latch = self.ctx.append_basic_block(self.func, "bits.advance");
            let exit = result
                .as_instruction()
                .get_parent()
                .ok_or_else(|| self.err("missing packed result block"))?;
            b.build_conditional_branch(contribution, exit, latch)
                .map_err(|e| self.err(e))?;
            result.add_incoming(&[(&self.ctx.bool_type().const_all_ones(), body)]);
            b.position_at_end(latch);
            (latch, initial)
        };
        let step = word.const_int(u64::from(ty.get_bit_width() / 8), false);
        let next = b
            .build_int_add(index, step, "bits.next")
            .map_err(|e| self.err(e))?;
        offset.add_incoming(&[(&start, entry), (&next, latch)]);
        let done = b
            .build_int_compare(IntPredicate::EQ, next, end, "bits.end")
            .map_err(|e| self.err(e))?;
        b.build_conditional_branch(done, after, body)
            .map_err(|e| self.err(e))?;
        b.position_at_end(after);
        if op == BitsetOp::Count {
            let complete = b
                .build_phi(word, "bits.subtotal")
                .map_err(|e| self.err(e))?;
            complete.add_incoming(&[(&initial, entry), (&sum, latch)]);
            Ok(complete.as_basic_value().into_int_value())
        } else {
            Ok(initial)
        }
    }

    fn bit_terminal(&self, view: BitView<'c>, op: BitsetOp) -> Result<IntValue<'c>, CodegenError> {
        let b = self.builder;
        let word = self.ctx.i64_type();
        let byte = self.ctx.i8_type();
        let zero = word.const_zero();
        let one = word.const_int(1, false);
        let entry = b
            .get_insert_block()
            .ok_or_else(|| self.err("missing packed terminal entry"))?;
        let first = self.ctx.append_basic_block(self.func, "bits.first");
        let interior = self.ctx.append_basic_block(self.func, "bits.interior");
        let exit = self.ctx.append_basic_block(self.func, "bits.done");
        let result_ty = if op == BitsetOp::Count {
            word
        } else {
            self.ctx.bool_type()
        };
        b.position_at_end(exit);
        let result = b
            .build_phi(result_ty, "bits.result")
            .map_err(|e| self.err(e))?;
        result.add_incoming(&[(&result_ty.const_zero(), entry)]);
        b.position_at_end(entry);
        let empty = b
            .build_int_compare(IntPredicate::EQ, view.len, zero, "bits.empty")
            .map_err(|e| self.err(e))?;
        b.build_conditional_branch(empty, exit, first)
            .map_err(|e| self.err(e))?;
        b.position_at_end(first);
        let head = b
            .build_int_truncate(view.head, byte, "bits.head")
            .map_err(|e| self.err(e))?;
        let left = b
            .build_left_shift(byte.const_all_ones(), head, "bits.left")
            .map_err(|e| self.err(e))?;
        // A view of at least one byte contains every active bit in this
        // first byte irrespective of its head. Resolve a decisive predicate
        // before computing the far boundary of a potentially large view.
        let masked_first = if op == BitsetOp::Count {
            first
        } else {
            let observed = self.bit_contribution(view.base, zero, left, op)?;
            let full_first = b
                .build_int_compare(
                    IntPredicate::UGE,
                    view.len,
                    word.const_int(8, false),
                    "bits.full.first",
                )
                .map_err(|e| self.err(e))?;
            let decided = b
                .build_and(full_first, observed, "bits.early")
                .map_err(|e| self.err(e))?;
            let bounds = self.ctx.append_basic_block(self.func, "bits.bounds");
            b.build_conditional_branch(decided, exit, bounds)
                .map_err(|e| self.err(e))?;
            result.add_incoming(&[(&result_ty.const_all_ones(), first)]);
            b.position_at_end(bounds);
            bounds
        };
        let last_index = b
            .build_int_sub(view.len, one, "bits.last.index")
            .map_err(|e| self.err(e))?;
        let (last_byte, last_bit) =
            bit_position(b, view.head, last_index).map_err(|e| self.err(e))?;
        let last = b
            .build_int_truncate(last_bit, byte, "bits.last.bit")
            .map_err(|e| self.err(e))?;
        let shift = b
            .build_int_sub(byte.const_int(7, false), last, "bits.tail.shift")
            .map_err(|e| self.err(e))?;
        let right = b
            .build_right_shift(byte.const_all_ones(), shift, false, "bits.right")
            .map_err(|e| self.err(e))?;
        let single = b
            .build_int_compare(IntPredicate::EQ, last_byte, zero, "bits.single")
            .map_err(|e| self.err(e))?;
        let first_right = b
            .build_select(single, right, byte.const_all_ones(), "bits.first.right")
            .map_err(|e| self.err(e))?
            .into_int_value();
        let first_mask = b
            .build_and(left, first_right, "bits.first.mask")
            .map_err(|e| self.err(e))?;
        let initial = self.bit_contribution(view.base, zero, first_mask, op)?;
        let done = if op == BitsetOp::Count {
            single
        } else {
            b.build_or(single, initial, "bits.first.done")
                .map_err(|e| self.err(e))?
        };
        b.build_conditional_branch(done, exit, interior)
            .map_err(|e| self.err(e))?;
        result.add_incoming(&[(&initial, masked_first)]);

        b.position_at_end(interior);
        // The first and last bytes are distinct here. Load only complete units
        // between them, then handle at most unit_bytes - 1 remaining bytes.
        let length = b
            .build_int_sub(last_byte, one, "bits.middle.len")
            .map_err(|e| self.err(e))?;
        let unit_end = |width: u64| -> Result<IntValue<'c>, CodegenError> {
            let whole = b
                .build_and(
                    length,
                    word.const_int(!(width - 1), false),
                    "bits.middle.whole",
                )
                .map_err(|e| self.err(e))?;
            b.build_int_add(one, whole, "bits.middle.end")
                .map_err(|e| self.err(e))
        };
        let word_end = unit_end(8)?;
        let (start, count) = if op == BitsetOp::Count {
            (one, initial)
        } else {
            // Combine a full cache line into one predicate before branching.
            // LLVM divides the wide equality into the target's legal widths.
            let width =
                std::num::NonZeroU32::new(512).ok_or_else(|| self.err("zero packed scan width"))?;
            let wide = self
                .ctx
                .custom_width_int_type(width)
                .map_err(|e| self.err(e))?;
            let wide_end = unit_end(64)?;
            let count = self.bit_scan_units(view, (one, wide_end), wide, initial, op, result)?;
            (wide_end, count)
        };
        let count = self.bit_scan_units(view, (start, word_end), word, count, op, result)?;
        let count = self.bit_scan_units(view, (word_end, last_byte), byte, count, op, result)?;
        let tail = self.bit_contribution(view.base, last_byte, right, op)?;
        let final_value = if op == BitsetOp::Count {
            b.build_int_add(count, tail, "bits.count")
                .map_err(|e| self.err(e))?
        } else {
            tail
        };
        let last_block = b
            .get_insert_block()
            .ok_or_else(|| self.err("missing packed tail block"))?;
        b.build_unconditional_branch(exit)
            .map_err(|e| self.err(e))?;
        result.add_incoming(&[(&final_value, last_block)]);
        b.position_at_end(exit);
        let value = result.as_basic_value().into_int_value();
        if op == BitsetOp::All {
            b.build_not(value, "bits.all").map_err(|e| self.err(e))
        } else {
            Ok(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_packed_offsets_and_final_byte_span_match_wide_integer_oracle()
    -> Result<(), Box<dyn std::error::Error>> {
        let context = Context::create();
        let module = context.create_module("offsets");
        let builder = context.create_builder();
        let function =
            module.add_function("offsets", context.void_type().fn_type(&[], false), None);
        builder.position_at_end(context.append_basic_block(function, "entry"));
        let ty = context.i64_type();
        for head in 0..8u64 {
            for distance in [0, 1, 7, 8, 9, 63, 64, 65] {
                let index = i64::MAX as u64 - distance;
                let absolute = u128::from(index) + u128::from(head);
                let (byte, bit) = bit_position(
                    &builder,
                    ty.const_int(head, false),
                    ty.const_int(index, false),
                )?;
                assert_eq!(
                    byte.get_zero_extended_constant().map(u128::from),
                    Some(absolute / 8)
                );
                assert_eq!(
                    bit.get_zero_extended_constant().map(u128::from),
                    Some(absolute % 8)
                );
                let (last, _) = bit_position(
                    &builder,
                    ty.const_int(head, false),
                    ty.const_int(index - 1, false),
                )?;
                assert_eq!(
                    last.get_zero_extended_constant().map(|n| u128::from(n) + 1),
                    Some(absolute.div_ceil(8))
                );
            }
        }
        Ok(())
    }
}
