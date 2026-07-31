use std::collections::{HashMap, HashSet};

use super::{StructDef, Ty, expand_tagged_ty, hir, is_move_handle, scalar_to_ty};

fn align_up(n: u64, align: u64) -> u64 {
    if align <= 1 {
        n
    } else {
        (n + align - 1) & !(align - 1)
    }
}

/// Reusable natural-ABI layout state for all types in one program.
///
/// Sharing this state makes a deep nominal DAG linear across all codegen field-order queries instead
/// of restarting the same suffix traversal for every declaring struct.
pub struct TypeLayoutCache<'a> {
    structs: &'a [StructDef],
    enums: &'a [hir::EnumDef],
    tagged_types: &'a [hir::TaggedType],
    struct_cache: HashMap<u32, (u64, u64)>,
    enum_cache: HashMap<u32, (u64, u64)>,
    tagged_cache: HashMap<u32, (u64, u64)>,
}

impl<'a> TypeLayoutCache<'a> {
    pub fn new(
        structs: &'a [StructDef],
        enums: &'a [hir::EnumDef],
        tagged_types: &'a [hir::TaggedType],
    ) -> Self {
        Self {
            structs,
            enums,
            tagged_types,
            struct_cache: HashMap::new(),
            enum_cache: HashMap::new(),
            tagged_cache: HashMap::new(),
        }
    }

    /// Natural ABI `(size, align)` for a resolved type. Traversal is iterative, memoized by
    /// nominal/tagged id, cycle-safe, and bounds-safe. Definition validation owns diagnostics for
    /// malformed graphs; this consumer only terminates deterministically before layout use.
    pub fn layout(&mut self, ty: Ty) -> (u64, u64) {
        #[derive(Clone, Copy)]
        enum Work {
            Enter(Ty),
            ExitStruct {
                id: u32,
                fields: usize,
                c_repr: bool,
                over_align: Option<u32>,
            },
            ExitEnum {
                id: u32,
                payloads: usize,
            },
            ExitOption,
            ExitResult,
            ExitTagged(u32),
        }

        let mut work = vec![Work::Enter(ty)];
        let mut built = Vec::new();
        let mut active_structs = HashSet::new();
        let mut active_enums = HashSet::new();
        let mut active_tagged = HashSet::new();

        while let Some(item) = work.pop() {
            match item {
                Work::Enter(ty) => match ty {
                    Ty::Int(int) => {
                        let bytes = (int.bits / 8).max(1) as u64;
                        built.push((bytes, bytes));
                    }
                    Ty::Float(float) => {
                        let bytes = (float.bits / 8).max(1) as u64;
                        built.push((bytes, bytes));
                    }
                    Ty::Bool => built.push((1, 1)),
                    Ty::Char => built.push((4, 4)),
                    Ty::Unit => built.push((0, 1)),
                    Ty::Tagged(id) => {
                        if let Some(&layout) = self.tagged_cache.get(&id) {
                            built.push(layout);
                        } else if !active_tagged.insert(id) {
                            built.push((0, 1));
                        } else if self.tagged_types.get(id as usize).is_none() {
                            active_tagged.remove(&id);
                            built.push((0, 1));
                        } else {
                            work.push(Work::ExitTagged(id));
                            work.push(Work::Enter(expand_tagged_ty(
                                Ty::Tagged(id),
                                self.tagged_types,
                            )));
                        }
                    }
                    Ty::Struct(id) => {
                        if let Some(&layout) = self.struct_cache.get(&id) {
                            built.push(layout);
                        } else if !active_structs.insert(id) {
                            built.push((0, 1));
                        } else if let Some(definition) = self.structs.get(id as usize) {
                            work.push(Work::ExitStruct {
                                id,
                                fields: definition.fields.len(),
                                c_repr: definition.c_repr,
                                over_align: definition.align,
                            });
                            work.extend(
                                definition
                                    .fields
                                    .iter()
                                    .rev()
                                    .map(|field| Work::Enter(field.ty)),
                            );
                        } else {
                            active_structs.remove(&id);
                            built.push((0, 1));
                        }
                    }
                    Ty::Enum(id) => {
                        if let Some(&layout) = self.enum_cache.get(&id) {
                            built.push(layout);
                        } else if !active_enums.insert(id) {
                            built.push((4, 4));
                        } else if let Some(definition) = self.enums.get(id as usize) {
                            let payloads = definition
                                .variants
                                .iter()
                                .map(|variant| variant.payload.len())
                                .sum();
                            work.push(Work::ExitEnum { id, payloads });
                            work.extend(
                                definition
                                    .variants
                                    .iter()
                                    .flat_map(|variant| variant.payload.iter())
                                    .rev()
                                    .copied()
                                    .map(scalar_to_ty)
                                    .map(Work::Enter),
                            );
                        } else {
                            active_enums.remove(&id);
                            built.push((4, 4));
                        }
                    }
                    Ty::Option(payload) => {
                        work.push(Work::ExitOption);
                        work.push(Work::Enter(scalar_to_ty(payload)));
                    }
                    Ty::Result(ok, err) => {
                        work.push(Work::ExitResult);
                        work.push(Work::Enter(scalar_to_ty(err)));
                        work.push(Work::Enter(scalar_to_ty(ok)));
                    }
                    Ty::HttpHeaders => built.push((8, 8)),
                    ty if is_move_handle(ty) => built.push((8, 8)),
                    _ => built.push((16, 8)),
                },
                Work::ExitStruct {
                    id,
                    fields,
                    c_repr,
                    over_align,
                } => {
                    let start = built.len().saturating_sub(fields);
                    let mut layouts = built.drain(start..).collect::<Vec<_>>();
                    if !c_repr {
                        layouts.sort_by_key(|&(_, align)| std::cmp::Reverse(align.max(1)));
                    }
                    let mut size = 0u64;
                    let mut align = 1u64;
                    for (field_size, field_align) in layouts {
                        let field_align = field_align.max(1);
                        size = align_up(size, field_align) + field_size;
                        align = align.max(field_align);
                    }
                    let effective = over_align.map_or(align, |value| align.max(value as u64));
                    let layout = (align_up(size, effective), align);
                    active_structs.remove(&id);
                    self.struct_cache.insert(id, layout);
                    built.push(layout);
                }
                Work::ExitEnum { id, payloads } => {
                    let start = built.len().saturating_sub(payloads);
                    let layouts = built.drain(start..).collect::<Vec<_>>();
                    let mut size = 4u64;
                    let mut align = 4u64;
                    for (payload_size, payload_align) in layouts {
                        let payload_align = payload_align.max(1);
                        size = align_up(size, payload_align) + payload_size;
                        align = align.max(payload_align);
                    }
                    let layout = (align_up(size, align), align);
                    active_enums.remove(&id);
                    self.enum_cache.insert(id, layout);
                    built.push(layout);
                }
                Work::ExitOption => {
                    let (payload_size, payload_align) = built.pop().unwrap_or((0, 1));
                    let payload_align = payload_align.max(1);
                    let payload_offset = align_up(1, payload_align);
                    built.push((
                        align_up(payload_offset + payload_size, payload_align),
                        payload_align,
                    ));
                }
                Work::ExitResult => {
                    let (err_size, err_align) = built.pop().unwrap_or((0, 1));
                    let (ok_size, ok_align) = built.pop().unwrap_or((0, 1));
                    let ok_align = ok_align.max(1);
                    let err_align = err_align.max(1);
                    let align = ok_align.max(err_align);
                    let ok_offset = align_up(1, ok_align);
                    let err_offset = align_up(ok_offset + ok_size, err_align);
                    built.push((align_up(err_offset + err_size, align), align));
                }
                Work::ExitTagged(id) => {
                    let layout = built.pop().unwrap_or((0, 1));
                    active_tagged.remove(&id);
                    self.tagged_cache.insert(id, layout);
                    built.push(layout);
                }
            }
        }
        built.pop().unwrap_or((0, 1))
    }
}

pub fn ty_abi_layout(
    ty: Ty,
    structs: &[StructDef],
    enums: &[hir::EnumDef],
    tagged_types: &[hir::TaggedType],
) -> (u64, u64) {
    TypeLayoutCache::new(structs, enums, tagged_types).layout(ty)
}
