use super::*;

fn index_drop_plan(
    cache: &mut HashMap<Ty, DropPlan>,
    root_ty: Ty,
    root: &DropPlan,
    structs: &[StructDef],
    enums: &[EnumDef],
    tagged: &[hir::TaggedType],
) {
    let mut work = vec![(root_ty, root)];
    while let Some((ty, plan)) = work.pop() {
        if cache.insert(ty, plan.clone()).is_some() {
            continue;
        }
        match (ty, plan) {
            (Ty::Tagged(id), _) => {
                let Some(expanded) = tagged.get(id as usize).map(|entry| match *entry {
                    hir::TaggedType::Option(payload) => Ty::Option(payload),
                    hir::TaggedType::Result(ok, err) => Ty::Result(ok, err),
                }) else {
                    continue;
                };
                work.push((expanded, plan));
            }
            (Ty::Struct(id), DropPlan::Struct { fields, .. }) => {
                let Some(definition) = structs.get(id as usize) else {
                    continue;
                };
                for (index, child) in fields.iter().rev() {
                    if let Some(field) = definition.fields.get(*index as usize) {
                        work.push((field.ty, child.as_ref()));
                    }
                }
            }
            (Ty::Option(payload), DropPlan::Option(child)) => {
                work.push((scalar_to_ty(payload), child.as_ref()));
            }
            (Ty::Result(ok, err), DropPlan::Result { ok: ok_plan, err: err_plan }) => {
                work.push((scalar_to_ty(err), err_plan.as_ref()));
                work.push((scalar_to_ty(ok), ok_plan.as_ref()));
            }
            (Ty::Enum(id), DropPlan::Enum { variants, .. }) => {
                let Some(definition) = enums.get(id as usize) else {
                    continue;
                };
                for (variant, plans) in definition.variants.iter().zip(variants).rev() {
                    for (payload, child) in variant.payload.iter().zip(plans).rev() {
                        work.push((scalar_to_ty(*payload), child.as_ref()));
                    }
                }
            }
            _ => {}
        }
    }
}

fn cached_plan_needs_drop(
    cache: &mut HashMap<Ty, DropPlan>,
    ty: Ty,
    structs: &[StructDef],
    enums: &[EnumDef],
    tagged: &[hir::TaggedType],
) -> bool {
    if !cache.contains_key(&ty) {
        let plan = drop_plan(ty, structs, enums, tagged);
        index_drop_plan(cache, ty, &plan, structs, enums, tagged);
    }
    cache.get(&ty).is_none_or(DropPlan::needs_drop)
}

impl<'c, 'a> FnGen<'c, 'a> {
    /// Emit recursive cleanup with compiler-owned frames instead of the process stack.
    ///
    /// The worklist preserves declaration/payload order. Branch frames also retain the exact
    /// continuation block so a nested Option/Result/enum finishes before its following sibling.
    pub(super) fn emit_drop_at_iterative(
        &self,
        base: inkwell::values::PointerValue<'c>,
        ty: Ty,
    ) -> Result<(), CodegenError> {
        enum Work<'c> {
            Drop {
                base: inkwell::values::PointerValue<'c>,
                ty: Ty,
            },
            Struct {
                base: inkwell::values::PointerValue<'c>,
                id: u32,
                next: usize,
            },
            FixedStructArray {
                base: inkwell::values::PointerValue<'c>,
                id: u32,
                len: u32,
                next: u32,
            },
            AggregateFields {
                base: inkwell::values::PointerValue<'c>,
                aggregate: StructType<'c>,
                fields: Vec<(u32, Ty)>,
                next: usize,
            },
            BranchFields {
                block: BasicBlock<'c>,
                base: inkwell::values::PointerValue<'c>,
                aggregate: StructType<'c>,
                fields: Vec<(u32, Ty)>,
                cont: BasicBlock<'c>,
            },
            EndBranch(BasicBlock<'c>),
            Position(BasicBlock<'c>),
            FinishDeepArray {
                head: BasicBlock<'c>,
                done: BasicBlock<'c>,
                ptr: inkwell::values::PointerValue<'c>,
                phi: inkwell::values::PhiValue<'c>,
                index: inkwell::values::IntValue<'c>,
            },
        }

        let mut drop_plans = HashMap::new();
        let _ = cached_plan_needs_drop(
            &mut drop_plans,
            ty,
            self.structs,
            self.enums,
            self.tagged_defs,
        );
        let mut work = vec![Work::Drop { base, ty }];
        while let Some(item) = work.pop() {
            match item {
                Work::Drop { base, ty } => match ty {
                    Ty::Option(payload) => {
                        let option_ty = option_struct_type(
                            self.ctx,
                            payload,
                            self.struct_types,
                            self.enum_types,
                            self.tagged_types,
                        );
                        let tag_ptr = self
                            .builder
                            .build_struct_gep(option_ty, base, 0, "dropopttagp")
                            .map_err(|error| self.err(error))?;
                        let tag = self
                            .builder
                            .build_load(self.ctx.i8_type(), tag_ptr, "dropopttag")
                            .map_err(|error| self.err(error))?
                            .into_int_value();
                        let some = self.ctx.append_basic_block(self.func, "drop.opt.some");
                        let cont = self.ctx.append_basic_block(self.func, "drop.opt.cont");
                        let is_some = self
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                self.ctx.i8_type().const_int(1, false),
                                "dropoptissome",
                            )
                            .map_err(|error| self.err(error))?;
                        self.builder
                            .build_conditional_branch(is_some, some, cont)
                            .map_err(|error| self.err(error))?;
                        work.push(Work::Position(cont));
                        work.push(Work::BranchFields {
                            block: some,
                            base,
                            aggregate: option_ty,
                            fields: vec![(1, scalar_to_ty(payload))],
                            cont,
                        });
                    }
                    Ty::Result(ok, err) => {
                        let result_ty = result_struct_type(
                            self.ctx,
                            ok,
                            err,
                            self.struct_types,
                            self.enum_types,
                            self.tagged_types,
                        );
                        let tag_ptr = self
                            .builder
                            .build_struct_gep(result_ty, base, 0, "droprestagp")
                            .map_err(|error| self.err(error))?;
                        let tag = self
                            .builder
                            .build_load(self.ctx.i8_type(), tag_ptr, "droprestag")
                            .map_err(|error| self.err(error))?
                            .into_int_value();
                        let cont = self.ctx.append_basic_block(self.func, "drop.result.cont");
                        let candidates = [
                            (0u64, 1u32, scalar_to_ty(ok), "drop.result.ok"),
                            (1u64, 2u32, scalar_to_ty(err), "drop.result.err"),
                        ];
                        let branches = candidates
                            .into_iter()
                            .filter(|(_, _, ty, _)| {
                                cached_plan_needs_drop(
                                    &mut drop_plans,
                                    *ty,
                                    self.structs,
                                    self.enums,
                                    self.tagged_defs,
                                )
                            })
                            .map(|(tag, field, ty, name)| {
                                (tag, field, ty, self.ctx.append_basic_block(self.func, name))
                            })
                            .collect::<Vec<_>>();
                        let cases = branches
                            .iter()
                            .map(|(tag, _, _, block)| {
                                (self.ctx.i8_type().const_int(*tag, false), *block)
                            })
                            .collect::<Vec<_>>();
                        self.builder
                            .build_switch(tag, cont, &cases)
                            .map_err(|error| self.err(error))?;
                        work.push(Work::Position(cont));
                        for (_, field, ty, block) in branches.into_iter().rev() {
                            work.push(Work::BranchFields {
                                block,
                                base,
                                aggregate: result_ty,
                                fields: vec![(field, ty)],
                                cont,
                            });
                        }
                    }
                    Ty::Tagged(id) => {
                        let tagged = self.tagged_defs.get(id as usize).ok_or_else(|| {
                            self.err(format!("nested tagged type id {id} is missing"))
                        })?;
                        let ty = match *tagged {
                            hir::TaggedType::Option(payload) => Ty::Option(payload),
                            hir::TaggedType::Result(ok, err) => Ty::Result(ok, err),
                        };
                        work.push(Work::Drop { base, ty });
                    }
                    Ty::Struct(id) => work.push(Work::Struct { base, id, next: 0 }),
                    Ty::Enum(id) => {
                        let enum_ty = *self
                            .enum_types
                            .get(id as usize)
                            .ok_or_else(|| self.err(format!("enum type id {id} is missing")))?;
                        let definition = self.enums.get(id as usize).ok_or_else(|| {
                            self.err(format!("enum definition id {id} is missing"))
                        })?;
                        let owned = definition
                            .variants
                            .iter()
                            .enumerate()
                            .filter_map(|(variant_index, variant)| {
                                let fields = variant
                                    .payload
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(payload_index, scalar)| {
                                        let ty = scalar_to_ty(*scalar);
                                        cached_plan_needs_drop(
                                            &mut drop_plans,
                                            ty,
                                            self.structs,
                                            self.enums,
                                            self.tagged_defs,
                                        )
                                            .then_some((
                                                variant.field_base + payload_index as u32,
                                                ty,
                                            ))
                                    })
                                    .collect::<Vec<_>>();
                                (!fields.is_empty()).then_some((variant_index as u64, fields))
                            })
                            .collect::<Vec<_>>();
                        if owned.is_empty() {
                            continue;
                        }
                        let tag_ptr = self
                            .builder
                            .build_struct_gep(enum_ty, base, 0, "droptag")
                            .map_err(|error| self.err(error))?;
                        let tag = self
                            .builder
                            .build_load(self.ctx.i32_type(), tag_ptr, "droptagv")
                            .map_err(|error| self.err(error))?
                            .into_int_value();
                        let cont = self.ctx.append_basic_block(self.func, "drop.enum.cont");
                        let branches = owned
                            .into_iter()
                            .map(|(tag, fields)| {
                                (
                                    tag,
                                    fields,
                                    self.ctx.append_basic_block(self.func, "drop.enum.v"),
                                )
                            })
                            .collect::<Vec<_>>();
                        let cases = branches
                            .iter()
                            .map(|(tag, _, block)| {
                                (self.ctx.i32_type().const_int(*tag, false), *block)
                            })
                            .collect::<Vec<_>>();
                        self.builder
                            .build_switch(tag, cont, &cases)
                            .map_err(|error| self.err(error))?;
                        work.push(Work::Position(cont));
                        for (_, fields, block) in branches.into_iter().rev() {
                            work.push(Work::BranchFields {
                                block,
                                base,
                                aggregate: enum_ty,
                                fields,
                                cont,
                            });
                        }
                    }
                    Ty::DynStructArray(id, _)
                        if struct_is_move(id, self.structs, self.enums, self.tagged_defs) =>
                    {
                        let aggregate = self
                            .builder
                            .build_load(slice_struct_type(self.ctx), base, "dropdeeparrv")
                            .map_err(|error| self.err(error))?
                            .into_struct_value();
                        let ptr = self
                            .builder
                            .build_extract_value(aggregate, 0, "dropdeeparrptr")
                            .map_err(|error| self.err(error))?
                            .into_pointer_value();
                        let len = self
                            .builder
                            .build_extract_value(aggregate, 1, "dropdeeparrlen")
                            .map_err(|error| self.err(error))?
                            .into_int_value();
                        let element_ty = *self
                            .struct_types
                            .get(id as usize)
                            .ok_or_else(|| self.err(format!("struct type id {id} is missing")))?;
                        let i64_type = self.ctx.i64_type();
                        let head = self.ctx.append_basic_block(self.func, "dropdeep.head");
                        let body = self.ctx.append_basic_block(self.func, "dropdeep.body");
                        let done = self.ctx.append_basic_block(self.func, "dropdeep.done");
                        let predecessor = self
                            .builder
                            .get_insert_block()
                            .ok_or_else(|| self.err("no insert block"))?;
                        self.builder
                            .build_unconditional_branch(head)
                            .map_err(|error| self.err(error))?;
                        self.builder.position_at_end(head);
                        let phi = self
                            .builder
                            .build_phi(i64_type, "dropdeep.i")
                            .map_err(|error| self.err(error))?;
                        phi.add_incoming(&[(&i64_type.const_zero(), predecessor)]);
                        let index = phi.as_basic_value().into_int_value();
                        let condition = self
                            .builder
                            .build_int_compare(IntPredicate::ULT, index, len, "dropdeep.cmp")
                            .map_err(|error| self.err(error))?;
                        self.builder
                            .build_conditional_branch(condition, body, done)
                            .map_err(|error| self.err(error))?;
                        self.builder.position_at_end(body);
                        let element_ptr = unsafe {
                            self.builder
                                .build_in_bounds_gep(element_ty, ptr, &[index], "dropdeep.ep")
                                .map_err(|error| self.err(error))?
                        };
                        work.push(Work::FinishDeepArray {
                            head,
                            done,
                            ptr,
                            phi,
                            index,
                        });
                        work.push(Work::Drop {
                            base: element_ptr,
                            ty: Ty::Struct(id),
                        });
                    }
                    Ty::DynArray(Scalar::String) => {
                        let aggregate = self
                            .builder
                            .build_load(slice_struct_type(self.ctx), base, "dropstrarr")
                            .map_err(|error| self.err(error))?
                            .into_struct_value();
                        let ptr = self
                            .builder
                            .build_extract_value(aggregate, 0, "dropstrarrptr")
                            .map_err(|error| self.err(error))?;
                        let len = self
                            .builder
                            .build_extract_value(aggregate, 1, "dropstrarrlen")
                            .map_err(|error| self.err(error))?;
                        self.builder
                            .build_call(
                                self.funcs["free_string_array"],
                                &[ptr.into(), len.into()],
                                "",
                            )
                            .map_err(|error| self.err(error))?;
                    }
                    Ty::DynResponseArray => {
                        let aggregate = self
                            .builder
                            .build_load(slice_struct_type(self.ctx), base, "droprsparr")
                            .map_err(|error| self.err(error))?
                            .into_struct_value();
                        let ptr = self
                            .builder
                            .build_extract_value(aggregate, 0, "droprsparrptr")
                            .map_err(|error| self.err(error))?;
                        let len = self
                            .builder
                            .build_extract_value(aggregate, 1, "droprsparrlen")
                            .map_err(|error| self.err(error))?;
                        self.builder
                            .build_call(
                                self.funcs["free_response_array"],
                                &[ptr.into(), len.into()],
                                "",
                            )
                            .map_err(|error| self.err(error))?;
                    }
                    Ty::StructArray(id, len)
                        if struct_is_move(id, self.structs, self.enums, self.tagged_defs) =>
                    {
                        work.push(Work::FixedStructArray {
                            base,
                            id,
                            len,
                            next: 0,
                        });
                    }
                    ty if let Some(free_fn) = handle_free_fn(ty) => {
                        let ptr = self
                            .builder
                            .build_load(
                                self.ctx.ptr_type(AddressSpace::default()),
                                base,
                                "drophandlev",
                            )
                            .map_err(|error| self.err(error))?;
                        self.builder
                            .build_call(self.funcs[free_fn], &[ptr.into()], "")
                            .map_err(|error| self.err(error))?;
                    }
                    Ty::String
                    | Ty::DynArray(_)
                    | Ty::DynStructArray(..)
                    | Ty::DynSliceArray(_) => {
                        let aggregate = self
                            .builder
                            .build_load(slice_struct_type(self.ctx), base, "dropslicev")
                            .map_err(|error| self.err(error))?
                            .into_struct_value();
                        let ptr = self
                            .builder
                            .build_extract_value(aggregate, 0, "dropsliceptr")
                            .map_err(|error| self.err(error))?;
                        self.builder
                            .build_call(self.funcs["free"], &[ptr.into()], "")
                            .map_err(|error| self.err(error))?;
                    }
                    _ => {}
                },
                Work::Struct { base, id, next } => {
                    let definition = self
                        .structs
                        .get(id as usize)
                        .ok_or_else(|| self.err(format!("struct definition id {id} is missing")))?;
                    let Some(field) = definition.fields.get(next) else {
                        continue;
                    };
                    work.push(Work::Struct {
                        base,
                        id,
                        next: next + 1,
                    });
                    let ty = field.ty;
                    let fixed_move_array = matches!(
                        ty,
                        Ty::StructArray(element, _)
                            if struct_is_move(
                                element,
                                self.structs,
                                self.enums,
                                self.tagged_defs,
                            )
                    );
                    if !fixed_move_array
                        && !cached_plan_needs_drop(
                            &mut drop_plans,
                            ty,
                            self.structs,
                            self.enums,
                            self.tagged_defs,
                        )
                    {
                        continue;
                    }
                    let aggregate = *self
                        .struct_types
                        .get(id as usize)
                        .ok_or_else(|| self.err(format!("struct type id {id} is missing")))?;
                    let physical = self.pfield(id, next as u32);
                    let field_ptr = self
                        .builder
                        .build_struct_gep(aggregate, base, physical, "dropfield")
                        .map_err(|error| self.err(error))?;
                    work.push(Work::Drop {
                        base: field_ptr,
                        ty,
                    });
                }
                Work::FixedStructArray {
                    base,
                    id,
                    len,
                    next,
                } => {
                    if next >= len {
                        continue;
                    }
                    let element_ty = *self
                        .struct_types
                        .get(id as usize)
                        .ok_or_else(|| self.err(format!("struct type id {id} is missing")))?;
                    let array_ty = element_ty.array_type(len);
                    let zero = self.ctx.i64_type().const_zero();
                    let index = self.ctx.i64_type().const_int(next as u64, false);
                    let element_ptr = unsafe {
                        self.builder
                            .build_in_bounds_gep(array_ty, base, &[zero, index], "dropnestel")
                            .map_err(|error| self.err(error))?
                    };
                    work.push(Work::FixedStructArray {
                        base,
                        id,
                        len,
                        next: next + 1,
                    });
                    work.push(Work::Drop {
                        base: element_ptr,
                        ty: Ty::Struct(id),
                    });
                }
                Work::AggregateFields {
                    base,
                    aggregate,
                    fields,
                    next,
                } => {
                    let Some(&(field, ty)) = fields.get(next) else {
                        continue;
                    };
                    let field_ptr = self
                        .builder
                        .build_struct_gep(aggregate, base, field, "droppayload")
                        .map_err(|error| self.err(error))?;
                    work.push(Work::AggregateFields {
                        base,
                        aggregate,
                        fields,
                        next: next + 1,
                    });
                    work.push(Work::Drop {
                        base: field_ptr,
                        ty,
                    });
                }
                Work::BranchFields {
                    block,
                    base,
                    aggregate,
                    fields,
                    cont,
                } => {
                    self.builder.position_at_end(block);
                    work.push(Work::EndBranch(cont));
                    work.push(Work::AggregateFields {
                        base,
                        aggregate,
                        fields,
                        next: 0,
                    });
                }
                Work::EndBranch(cont) => {
                    self.builder
                        .build_unconditional_branch(cont)
                        .map_err(|error| self.err(error))?;
                }
                Work::Position(block) => self.builder.position_at_end(block),
                Work::FinishDeepArray {
                    head,
                    done,
                    ptr,
                    phi,
                    index,
                } => {
                    let after = self
                        .builder
                        .get_insert_block()
                        .ok_or_else(|| self.err("no insert block"))?;
                    let next = self
                        .builder
                        .build_int_add(
                            index,
                            self.ctx.i64_type().const_int(1, false),
                            "dropdeep.inext",
                        )
                        .map_err(|error| self.err(error))?;
                    phi.add_incoming(&[(&next, after)]);
                    self.builder
                        .build_unconditional_branch(head)
                        .map_err(|error| self.err(error))?;
                    self.builder.position_at_end(done);
                    self.builder
                        .build_call(self.funcs["free"], &[ptr.into()], "")
                        .map_err(|error| self.err(error))?;
                }
            }
        }
        Ok(())
    }
}
