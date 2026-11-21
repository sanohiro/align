//! Lower the checked operations on borrowed packed bits. Address calculations use byte
//! quotients plus low three-bit remainders, including when the logical index is i64::MAX.
use super::*;
use align_mir::BitsetOp;
use inkwell::values::{PointerValue, StructValue};

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
                let count = self.bit_count(view)?;
                match op {
                    BitsetOp::Any => self
                        .builder
                        .build_int_compare(IntPredicate::NE, count, zero, "bits.any")
                        .map_err(|error| self.err(error))?
                        .into(),
                    BitsetOp::All => self
                        .builder
                        .build_int_compare(IntPredicate::EQ, count, view.len, "bits.all")
                        .map_err(|error| self.err(error))?
                        .into(),
                    _ => count.into(),
                }
            }
            _ => return Err(self.err("malformed packed bit operands")),
        };
        Ok(Some(value))
    }

    fn bit_count(&mut self, view: BitView<'c>) -> Result<IntValue<'c>, CodegenError> {
        let i64 = self.ctx.i64_type();
        let i8 = self.ctx.i8_type();
        let zero = i64.const_zero();
        let one = i64.const_int(1, false);
        let empty = self
            .builder
            .build_int_compare(IntPredicate::EQ, view.len, zero, "bits.empty")
            .map_err(|error| self.err(error))?;
        let entry = self
            .builder
            .get_insert_block()
            .ok_or_else(|| self.err("missing bit count block"))?;
        let init = self.ctx.append_basic_block(self.func, "bit_count.init");
        let body = self.ctx.append_basic_block(self.func, "bit_count.byte");
        let exit = self.ctx.append_basic_block(self.func, "bit_count.done");
        self.builder
            .build_conditional_branch(empty, exit, init)
            .map_err(|error| self.err(error))?;
        self.builder.position_at_end(init);
        let last_index = self
            .builder
            .build_int_sub(view.len, one, "bits.last.index")
            .map_err(|error| self.err(error))?;
        let (last_byte, last_bit) =
            bit_position(self.builder, view.head, last_index).map_err(|e| self.err(e))?;
        let head = self
            .builder
            .build_int_truncate(view.head, i8, "bits.head.shift")
            .map_err(|error| self.err(error))?;
        let last = self
            .builder
            .build_int_truncate(last_bit, i8, "bits.tail.bit")
            .map_err(|error| self.err(error))?;
        let shift = self
            .builder
            .build_int_sub(i8.const_int(7, false), last, "bits.tail.shift")
            .map_err(|error| self.err(error))?;
        let first_mask = self
            .builder
            .build_left_shift(i8.const_all_ones(), head, "bits.first.mask")
            .map_err(|error| self.err(error))?;
        let last_mask = self
            .builder
            .build_right_shift(i8.const_all_ones(), shift, false, "bits.last.mask")
            .map_err(|error| self.err(error))?;
        self.builder
            .build_unconditional_branch(body)
            .map_err(|error| self.err(error))?;
        self.builder.position_at_end(body);
        let position = self
            .builder
            .build_phi(i64, "bits.position")
            .map_err(|error| self.err(error))?;
        let total = self
            .builder
            .build_phi(i64, "bits.total")
            .map_err(|error| self.err(error))?;
        let index = position.as_basic_value().into_int_value();
        let is_first = self
            .builder
            .build_int_compare(IntPredicate::EQ, index, zero, "bits.first")
            .map_err(|error| self.err(error))?;
        let is_last = self
            .builder
            .build_int_compare(IntPredicate::EQ, index, last_byte, "bits.last")
            .map_err(|error| self.err(error))?;
        let left = self
            .builder
            .build_select(is_first, first_mask, i8.const_all_ones(), "bits.left")
            .map_err(|error| self.err(error))?
            .into_int_value();
        let right = self
            .builder
            .build_select(is_last, last_mask, i8.const_all_ones(), "bits.right")
            .map_err(|error| self.err(error))?
            .into_int_value();
        let mask = self
            .builder
            .build_and(left, right, "bits.mask")
            .map_err(|error| self.err(error))?;
        let address = self.bit_byte_ptr(view.base, index)?;
        let raw = self
            .builder
            .build_load(i8, address, "bits.byte")
            .map_err(|error| self.err(error))?
            .into_int_value();
        let selected = self
            .builder
            .build_and(raw, mask, "bits.active")
            .map_err(|error| self.err(error))?;
        let pop = self
            .call_intrinsic("llvm.ctpop", &[i8.into()], &[selected.into()])?
            .into_int_value();
        let add = self
            .builder
            .build_int_z_extend(pop, i64, "bits.popcount")
            .map_err(|error| self.err(error))?;
        let sum = self
            .builder
            .build_int_add(total.as_basic_value().into_int_value(), add, "bits.sum")
            .map_err(|error| self.err(error))?;
        let next = self
            .builder
            .build_int_add(index, one, "bits.next")
            .map_err(|error| self.err(error))?;
        position.add_incoming(&[(&zero, init), (&next, body)]);
        total.add_incoming(&[(&zero, init), (&sum, body)]);
        self.builder
            .build_conditional_branch(is_last, exit, body)
            .map_err(|error| self.err(error))?;
        self.builder.position_at_end(exit);
        let result = self
            .builder
            .build_phi(i64, "bits.count")
            .map_err(|error| self.err(error))?;
        result.add_incoming(&[(&zero, entry), (&sum, body)]);
        Ok(result.as_basic_value().into_int_value())
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
