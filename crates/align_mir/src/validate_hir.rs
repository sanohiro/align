use std::collections::{HashMap, HashSet, VecDeque};

use align_sema::{Layout, PrimScalar, Scalar, Ty, hir};
use align_span::Span;

/// Validate the program-global HIR type domain before MIR construction.
pub(crate) fn global_type_metadata_is_valid(program: &hir::Program) -> bool {
    Validator::new(program).validate()
}

/// Validate the placement of body-independent HIR types.
///
/// [`global_type_metadata_is_valid`] deliberately checks only the graph domain: it answers
/// whether an id, scalar discriminator, width, and inline edge are well formed.  The sema has a
/// second, orthogonal contract for where each graph-valid type may occur (a field, payload,
/// collection element, function header, or C boundary).  Keeping that contract here means a
/// handcrafted HIR value cannot smuggle a graph-valid but unproducible placement into MIR.
pub(crate) fn type_placement_metadata_is_valid(program: &hir::Program) -> bool {
    PlacementValidator::new(program).validate()
}

/// Validate nominal identities, complete source shapes, enum ordinals, and linker library names.
///
/// This is intentionally separate from the graph and placement validators.  The graph validator
/// answers whether references are well formed, while this pass answers whether two producer-side
/// nominal records that claim one source identity really describe the same id-free definition.
pub(crate) fn nominal_link_metadata_is_valid(program: &hir::Program) -> bool {
    NominalLinkValidator::new(program).validate()
}

/// Validate body-independent declaration and header records before MIR copies HIR.
///
/// This pass intentionally does not inspect function bodies or `drop_individual_exprs`. Body
/// ownership/effect replay belongs to am-b4; the fields validated here are the producer-owned
/// declaration, signature, origin, local-header, and structural drop-set facts that do not require
/// expression semantics.
pub(crate) fn declaration_header_metadata_is_valid(program: &hir::Program) -> bool {
    DeclarationValidator::new(program).validate()
}

/// Validate the dormant am-b1 portion of stored HIR bodies.
///
/// This helper is deliberately not part of [`hir_program_is_valid`].  am-b1 owns the structural
/// and type envelope of the first body range; am-b4 owns the single public activation and the
/// replay of ownership, Drop, effects, and wait proofs.
#[allow(dead_code)]
pub(crate) fn body_core_metadata_is_valid(program: &hir::Program) -> bool {
    global_type_metadata_is_valid(program)
        && type_placement_metadata_is_valid(program)
        && nominal_link_metadata_is_valid(program)
        && body_core::validate(program)
}

#[cfg(test)]
pub(crate) use body_core::body_ty_mangle;

struct DeclarationValidator<'a> {
    program: &'a hir::Program,
    placement: PlacementValidator<'a>,
}

impl<'a> DeclarationValidator<'a> {
    fn new(program: &'a hir::Program) -> Self {
        Self {
            program,
            placement: PlacementValidator::new(program),
        }
    }

    fn validate(&self) -> bool {
        self.externs_valid()
            && self.imported_functions_valid()
            && self.function_types_valid()
            && self.stored_functions_valid()
    }

    fn externs_valid(&self) -> bool {
        let mut names = HashSet::new();
        for function in &self.program.externs {
            if function.name == "main"
                || !valid_declaration_name(&function.name)
                || !names.insert(function.name.as_str())
                || function.params.len() != function.param_modes.len()
                || !function
                    .param_modes
                    .iter()
                    .all(|mode| *mode == align_ast::ParamMode::ByValue)
                || !function
                    .params
                    .iter()
                    .all(|&ty| self.placement.ffi_parameter_ok(ty))
                || !self.placement.ffi_return_ok(function.ret)
                || !summary_is_none(&function.return_borrow, &function.return_region)
            {
                return false;
            }
        }
        true
    }

    fn imported_functions_valid(&self) -> bool {
        let mut names = HashSet::new();
        for function in &self.program.imported_fns {
            if function.name == "main"
                || !valid_declaration_name(&function.name)
                || !names.insert(function.name.as_str())
                || !self.source_signature_valid(
                    &function.params,
                    &function.param_modes,
                    function.ret,
                    &function.return_borrow,
                    &function.return_region,
                )
            {
                return false;
            }
            // `FnEffect` is a closed Rust enum. The producer has already normalized an absent
            // external-map entry to `Impure`; no body effect is inferred at this boundary.
            match function.effect {
                align_sema::FnEffect::Pure
                | align_sema::FnEffect::Impure
                | align_sema::FnEffect::Unknown => {}
            }
        }
        true
    }

    fn function_types_valid(&self) -> bool {
        for (id, function) in self.program.fn_types.iter().enumerate() {
            let Ok(id) = u32::try_from(id) else {
                return false;
            };
            let allow_param = self.placement.is_abstract(Node::Fn(id));
            if function.params.iter().any(|(mode, scalar)| {
                !mode_is_valid(*mode, align_sema::scalar_to_ty(*scalar), true)
                    || !self.placement.scalar_ok(
                        *scalar,
                        ScalarPlacement::FnParameter { allow_param },
                    )
            }) || !self.placement.resolve_type_ok(function.ret, allow_param)
                || !summary_valid(
                    self.program,
                    &function.return_borrow,
                    &function.return_region,
                    &function
                        .params
                        .iter()
                        .map(|(_, scalar)| align_sema::scalar_to_ty(*scalar))
                        .collect::<Vec<_>>(),
                )
            {
                return false;
            }
        }
        true
    }

    fn stored_functions_valid(&self) -> bool {
        let mut names = HashSet::new();
        for function in &self.program.fns {
            if !valid_declaration_name(&function.name)
                || !names.insert(function.name.as_str())
                || !valid_span(function.span)
                || !self.parameter_vector_valid(function)
                || !self.function_signature_valid(function)
                || !self.origin_valid(function)
                || !self.locals_valid(function)
                || !self.drop_sets_valid(function)
                || !self.main_valid(function)
            {
                return false;
            }
        }
        true
    }

    fn origin_valid(&self, function: &hir::Fn) -> bool {
        match function.origin {
            hir::FnOrigin::Source { .. } | hir::FnOrigin::Monomorph => true,
            hir::FnOrigin::Lifted { capture_count } => {
                usize::try_from(capture_count).is_ok_and(|count| count <= function.params.len())
                    && function
                        .param_modes
                        .iter()
                        .all(|mode| *mode == align_ast::ParamMode::ByValue)
                    && summary_is_none(
                        &function.return_borrow,
                        &function.return_region,
                    )
            }
        }
    }

    fn parameter_vector_valid(&self, function: &hir::Fn) -> bool {
        if function.params.len() != function.param_modes.len() {
            return false;
        }
        let mut seen = HashSet::new();
        for (&local_id, &mode) in function.params.iter().zip(&function.param_modes) {
            if !seen.insert(local_id)
                || !mode_is_valid(
                    mode,
                    function
                        .locals
                        .get(local_id as usize)
                        .map(|local| local.ty)
                        .unwrap_or(Ty::Error),
                    true,
                )
            {
                return false;
            }
            let Some(local) = function.locals.get(local_id as usize) else {
                return false;
            };
            if local.id != local_id {
                return false;
            }
        }
        true
    }

    fn locals_valid(&self, function: &hir::Fn) -> bool {
        let parameter_ids: HashSet<hir::LocalId> = function.params.iter().copied().collect();
        let lifted = matches!(function.origin, hir::FnOrigin::Lifted { .. });
        let mut names = HashSet::new();
        for (index, local) in function.locals.iter().enumerate() {
            let Ok(index) = u32::try_from(index) else {
                return false;
            };
            if local.id != index
                || !valid_local_name(&local.name)
                || !names.insert(local.name.as_str())
                || local.align.is_some_and(|align| {
                    !align.is_power_of_two() || align > (1u32 << 29)
                })
            {
                return false;
            }
            let is_signature_parameter = parameter_ids.contains(&local.id);
            if is_signature_parameter {
                if !valid_member_name(&local.name)
                    || local.is_param == lifted
                    || local.align.is_some()
                {
                    return false;
                }
            } else if local.is_param {
                return false;
            }
            if local.align.is_some()
                && (local.is_param
                    || !matches!(local.ty, Ty::Array(Scalar::Int(_) | Scalar::Float(_), _)))
            {
                return false;
            }
        }
        true
    }

    fn function_signature_valid(&self, function: &hir::Fn) -> bool {
        let parameter_types: Vec<Ty> = function
            .params
            .iter()
            .filter_map(|&id| function.locals.get(id as usize).map(|local| local.ty))
            .collect();
        parameter_types.len() == function.params.len()
            && parameter_types
                .iter()
                .all(|&ty| self.placement.source_function_type_ok(ty, true, false))
            && self
                .placement
                .source_function_type_ok(function.ret, false, true)
            && summary_valid(
                self.program,
                &function.return_borrow,
                &function.return_region,
                &parameter_types,
            )
    }

    fn drop_sets_valid(&self, function: &hir::Fn) -> bool {
        let valid_set = |ids: &[hir::LocalId]| {
            ids.windows(2).all(|pair| pair[0] < pair[1])
                && ids.iter().all(|&id| (id as usize) < function.locals.len())
        };
        valid_set(&function.drop_locals)
            && valid_set(&function.drop_individual_locals)
            && function
                .drop_individual_locals
                .iter()
                .all(|id| function.drop_locals.binary_search(id).is_ok())
    }

    fn main_valid(&self, function: &hir::Fn) -> bool {
        if function.name != "main" {
            return true;
        }
        if !matches!(
            function.origin,
            hir::FnOrigin::Source { is_entry: true, .. }
        ) {
            return false;
        }
        let exact_i32 = Ty::Int(align_sema::IntTy {
            bits: 32,
            signed: true,
        });
        let error_id = self.builtin_error_id();
        let result = error_id.map(|id| Ty::Result(Scalar::Unit, Scalar::Enum(id)));
        let return_ok = function.ret == Ty::Unit
            || function.ret == exact_i32
            || result == Some(function.ret);
        let no_args = function.params.is_empty();
        let argv = function.params.len() == 1
            && function.param_modes == [align_ast::ParamMode::ByValue]
            && function
                .locals
                .get(function.params[0] as usize)
                .is_some_and(|local| local.ty == Ty::DynArray(Scalar::Str));
        return_ok && (no_args || (argv && result == Some(function.ret)))
    }

    fn builtin_error_id(&self) -> Option<u32> {
        let mut found = None;
        for (id, definition) in self.program.enums.iter().enumerate() {
            if definition.name == "Error"
                && definition.source_name == "Error"
                && builtin_error_shape(definition)
            {
                if found.is_some() {
                    return None;
                }
                found = Some(u32::try_from(id).ok()?);
            }
        }
        found
    }

    fn source_signature_valid(
        &self,
        params: &[Ty],
        modes: &[align_ast::ParamMode],
        ret: Ty,
        borrow: &hir::ReturnBorrowSummary,
        region: &hir::ReturnRegionSummary,
    ) -> bool {
        params.len() == modes.len()
            && modes
                .iter()
                .zip(params)
                .all(|(&mode, &ty)| {
                    mode_is_valid(mode, ty, true)
                        && self.placement.source_function_type_ok(ty, true, false)
                })
            && self.placement.source_function_type_ok(ret, false, true)
            && summary_valid(self.program, borrow, region, params)
    }
}

fn valid_declaration_name(name: &str) -> bool {
    !name.is_empty() && !name.as_bytes().contains(&0)
}

fn valid_local_name(name: &str) -> bool {
    valid_declaration_name(name)
}

fn valid_span(span: Span) -> bool {
    span.lo <= span.hi
}

fn mode_is_valid(mode: align_ast::ParamMode, ty: Ty, allow_out: bool) -> bool {
    match mode {
        align_ast::ParamMode::ByValue => true,
        align_ast::ParamMode::Out => allow_out && matches!(ty, Ty::Slice(_)),
        align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut => false,
    }
}

fn summary_is_none(
    borrow: &hir::ReturnBorrowSummary,
    region: &hir::ReturnRegionSummary,
) -> bool {
    matches!(borrow, hir::ReturnBorrowSummary::None)
        && matches!(region, hir::ReturnRegionSummary::None)
}

fn summary_valid(
    program: &hir::Program,
    borrow: &hir::ReturnBorrowSummary,
    region: &hir::ReturnRegionSummary,
    params: &[Ty],
) -> bool {
    match (borrow, region) {
        (hir::ReturnBorrowSummary::None, hir::ReturnRegionSummary::None) => true,
        (
            hir::ReturnBorrowSummary::Roots {
                params: borrow_params,
                captures: borrow_captures,
            },
            hir::ReturnRegionSummary::Roots {
                params: region_params,
                captures: region_captures,
            },
        ) => {
            !borrow_params.is_empty()
                && borrow_params == region_params
                && borrow_captures.is_empty()
                && region_captures.is_empty()
                && borrow_params.windows(2).all(|pair| pair[0] < pair[1])
                && borrow_params.iter().all(|&id| {
                    params
                        .get(id as usize)
                        .is_some_and(|&ty| {
                            align_sema::ty_may_borrow(
                                ty,
                                &program.structs,
                                &program.tuples,
                                &program.enums,
                                &program.tagged_types,
                            )
                        })
                })
        }
        _ => false,
    }
}

fn builtin_error_shape(definition: &hir::EnumDef) -> bool {
    if definition.variants.len() != 5 {
        return false;
    }
    let names = ["NotFound", "Invalid", "Denied", "Timeout", "Code"];
    definition
        .variants
        .iter()
        .zip(names)
        .all(|(variant, name)| {
            variant.name == name
                && variant.field_base == 1
                && match name {
                    "Code" => {
                        variant.payload.as_slice()
                            == [Scalar::Int(align_sema::IntTy {
                                bits: 32,
                                signed: true,
                            })]
                    }
                    _ => variant.payload.is_empty(),
                }
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NominalKind {
    Struct,
    Enum,
}

struct NominalLinkValidator<'a> {
    program: &'a hir::Program,
    internal_names: HashSet<&'a str>,
    source_names: HashMap<&'a str, (NominalKind, u32)>,
    checked_source_shapes: HashSet<(Node, Node)>,
}

impl<'a> NominalLinkValidator<'a> {
    fn new(program: &'a hir::Program) -> Self {
        Self {
            program,
            internal_names: HashSet::new(),
            source_names: HashMap::new(),
            checked_source_shapes: HashSet::new(),
        }
    }

    fn validate(mut self) -> bool {
        self.structs_valid()
            && self.enums_valid()
            && self.tuples_valid()
            && self.link_libs_valid()
    }

    fn structs_valid(&mut self) -> bool {
        for (id, definition) in self.program.structs.iter().enumerate() {
            let Ok(id) = u32::try_from(id) else {
                return false;
            };
            if !valid_nominal_text(&definition.name)
                || !valid_nominal_text(&definition.source_name)
                || !self.internal_names.insert(definition.name.as_str())
                || !valid_alignment(definition.align)
                || !members_valid(definition.fields.iter().map(|field| field.name.as_str()))
            {
                return false;
            }
            if !self.register_source_name(
                definition.source_name.as_str(),
                NominalKind::Struct,
                id,
            ) {
                return false;
            }
        }
        true
    }

    fn enums_valid(&mut self) -> bool {
        for (id, definition) in self.program.enums.iter().enumerate() {
            let Ok(id) = u32::try_from(id) else {
                return false;
            };
            if !valid_nominal_text(&definition.name)
                || !valid_nominal_text(&definition.source_name)
                || !self.internal_names.insert(definition.name.as_str())
                || !members_valid(definition.variants.iter().map(|variant| variant.name.as_str()))
            {
                return false;
            }
            let mut expected_base = 1u32;
            for variant in &definition.variants {
                if variant.field_base != expected_base
                    || variant.payload.len() > u32::MAX as usize
                {
                    return false;
                }
                let Some(next_base) = expected_base.checked_add(variant.payload.len() as u32)
                else {
                    return false;
                };
                expected_base = next_base;
            }
            if !self.register_source_name(
                definition.source_name.as_str(),
                NominalKind::Enum,
                id,
            ) {
                return false;
            }
        }
        true
    }

    fn register_source_name(&mut self, source_name: &'a str, kind: NominalKind, id: u32) -> bool {
        let Some(&(existing_kind, existing_id)) = self.source_names.get(source_name) else {
            self.source_names.insert(source_name, (kind, id));
            return true;
        };
        existing_kind == kind
            && source_shape_equal(
                self.program,
                nominal_node(kind, existing_id),
                nominal_node(kind, id),
                &mut self.checked_source_shapes,
            )
    }

    fn tuples_valid(&self) -> bool {
        let mut seen = HashSet::new();
        self.program
            .tuples
            .iter()
            .all(|tuple| seen.insert(tuple.elems.clone()))
    }

    fn link_libs_valid(&self) -> bool {
        let mut seen = HashSet::new();
        self.program.link_libs.iter().all(|library| {
            !library.is_empty()
                && !library.as_bytes().starts_with(b"-")
                && library.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
                })
                && seen.insert(library.as_str())
        })
    }
}

fn valid_nominal_text(text: &str) -> bool {
    !text.is_empty() && !text.as_bytes().contains(&0)
}

fn valid_member_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn members_valid<'b>(names: impl IntoIterator<Item = &'b str>) -> bool {
    let mut seen = HashSet::new();
    names
        .into_iter()
        .all(|name| valid_member_name(name) && seen.insert(name))
}

fn valid_alignment(align: Option<u32>) -> bool {
    align.is_none_or(|value| value.is_power_of_two() && value <= (1u32 << 29))
}

fn nominal_node(kind: NominalKind, id: u32) -> Node {
    match kind {
        NominalKind::Struct => Node::Struct(id),
        NominalKind::Enum => Node::Enum(id),
    }
}

struct SourceShapeComparator<'a> {
    program: &'a hir::Program,
    known_shapes: &'a HashSet<(Node, Node)>,
    root: (Node, Node),
    cache_enabled: bool,
    pending: VecDeque<(Node, Node)>,
    seen: HashSet<(Node, Node)>,
    left_to_right: HashMap<Node, Node>,
    right_to_left: HashMap<Node, Node>,
}

fn source_shape_equal(
    program: &hir::Program,
    left: Node,
    right: Node,
    known_shapes: &mut HashSet<(Node, Node)>,
) -> bool {
    let mut comparator = SourceShapeComparator {
        program,
        known_shapes: &*known_shapes,
        root: (left, right),
        cache_enabled: true,
        pending: VecDeque::from([(left, right)]),
        seen: HashSet::new(),
        left_to_right: HashMap::new(),
        right_to_left: HashMap::new(),
    };
    let valid = comparator.run();
    let seen = comparator.seen.clone();
    drop(comparator);
    if valid {
        known_shapes.extend(seen);
    }
    valid
}

impl<'a> SourceShapeComparator<'a> {
    fn run(&mut self) -> bool {
        loop {
            let mut restart_without_cache = false;
            while let Some((left, right)) = self.pending.pop_front() {
                if !self.map_pair(left, right) {
                    return false;
                }

                // A cached pair is safe to skip only when it is the entire fresh comparison. If
                // another pair has already been mapped, or another pending sibling exists, the
                // cached pair may contain a shared node whose correspondence must be checked in
                // this context. Restart once without cache in that case; this keeps the common
                // deep-duplicate chain linear while preserving graph sharing for branching DAGs.
                if self.cache_enabled
                    && self.known_shapes.contains(&(left, right))
                {
                    if !self.seen.is_empty() || !self.pending.is_empty() {
                        restart_without_cache = true;
                        break;
                    }
                    self.seen.insert((left, right));
                    continue;
                }

                if !self.seen.insert((left, right)) {
                    continue;
                }
                if !self.nodes_equal(left, right) {
                    return false;
                }
            }
            if !restart_without_cache {
                return true;
            }

            self.cache_enabled = false;
            self.pending.clear();
            self.pending.push_back(self.root);
            self.seen.clear();
            self.left_to_right.clear();
            self.right_to_left.clear();
        }
    }

    fn map_pair(&mut self, left: Node, right: Node) -> bool {
        if self.left_to_right.get(&left).is_some_and(|mapped| *mapped != right)
            || self.right_to_left.get(&right).is_some_and(|mapped| *mapped != left)
        {
            return false;
        }
        self.left_to_right.insert(left, right);
        self.right_to_left.insert(right, left);
        true
    }

    fn nodes_equal(&mut self, left: Node, right: Node) -> bool {
        match (left, right) {
            (Node::Struct(left_id), Node::Struct(right_id)) => {
                let Some(left_def) = self.program.structs.get(left_id as usize) else {
                    return false;
                };
                let Some(right_def) = self.program.structs.get(right_id as usize) else {
                    return false;
                };
                if left_def.source_name != right_def.source_name
                    || left_def.align != right_def.align
                    || left_def.c_repr != right_def.c_repr
                    || left_def.fields.len() != right_def.fields.len()
                {
                    return false;
                }
                for (left_field, right_field) in left_def.fields.iter().zip(&right_def.fields) {
                    if left_field.name != right_field.name
                        || !self.types_equal(left_field.ty, right_field.ty)
                    {
                        return false;
                    }
                }
                true
            }
            (Node::Enum(left_id), Node::Enum(right_id)) => {
                let Some(left_def) = self.program.enums.get(left_id as usize) else {
                    return false;
                };
                let Some(right_def) = self.program.enums.get(right_id as usize) else {
                    return false;
                };
                if left_def.source_name != right_def.source_name
                    || left_def.variants.len() != right_def.variants.len()
                {
                    return false;
                }
                for (left_variant, right_variant) in
                    left_def.variants.iter().zip(&right_def.variants)
                {
                    if left_variant.name != right_variant.name
                        || left_variant.field_base != right_variant.field_base
                        || left_variant.payload.len() != right_variant.payload.len()
                        || !left_variant
                            .payload
                            .iter()
                            .zip(&right_variant.payload)
                            .all(|(&left, &right)| self.scalars_equal(left, right))
                    {
                        return false;
                    }
                }
                true
            }
            (Node::Tuple(left_id), Node::Tuple(right_id)) => {
                let Some(left_def) = self.program.tuples.get(left_id as usize) else {
                    return false;
                };
                let Some(right_def) = self.program.tuples.get(right_id as usize) else {
                    return false;
                };
                left_def.elems.len() == right_def.elems.len()
                    && left_def
                        .elems
                        .iter()
                        .zip(&right_def.elems)
                        .all(|(&left, &right)| self.scalars_equal(left, right))
            }
            (Node::Tagged(left_id), Node::Tagged(right_id)) => {
                let Some(left_entry) = self.program.tagged_types.get(left_id as usize) else {
                    return false;
                };
                let Some(right_entry) = self.program.tagged_types.get(right_id as usize) else {
                    return false;
                };
                match (*left_entry, *right_entry) {
                    (hir::TaggedType::Option(left), hir::TaggedType::Option(right)) => {
                        self.scalars_equal(left, right)
                    }
                    (
                        hir::TaggedType::Result(left_ok, left_err),
                        hir::TaggedType::Result(right_ok, right_err),
                    ) => self.scalars_equal(left_ok, right_ok) && self.scalars_equal(left_err, right_err),
                    _ => false,
                }
            }
            (Node::Fn(left_id), Node::Fn(right_id)) => {
                let Some(left_fn) = self.program.fn_types.get(left_id as usize) else {
                    return false;
                };
                let Some(right_fn) = self.program.fn_types.get(right_id as usize) else {
                    return false;
                };
                if left_fn.params.len() != right_fn.params.len()
                    || left_fn.return_borrow != right_fn.return_borrow
                    || left_fn.return_region != right_fn.return_region
                {
                    return false;
                }
                for ((left_mode, left_scalar), (right_mode, right_scalar)) in
                    left_fn.params.iter().zip(&right_fn.params)
                {
                    if left_mode != right_mode || !self.scalars_equal(*left_scalar, *right_scalar) {
                        return false;
                    }
                }
                self.types_equal(left_fn.ret, right_fn.ret)
            }
            _ => false,
        }
    }

    fn queue(&mut self, left: Node, right: Node) {
        self.pending.push_back((left, right));
    }

    fn scalars_equal(&mut self, left: Scalar, right: Scalar) -> bool {
        match (left, right) {
            (Scalar::Int(left), Scalar::Int(right)) => left == right,
            (Scalar::Float(left), Scalar::Float(right)) => left == right,
            (Scalar::Struct(left), Scalar::Struct(right)) => {
                self.queue(Node::Struct(left), Node::Struct(right));
                true
            }
            (Scalar::DynStructArray(left), Scalar::DynStructArray(right))
            | (Scalar::Soa(left), Scalar::Soa(right)) => {
                self.queue(Node::Struct(left), Node::Struct(right));
                true
            }
            (Scalar::Enum(left), Scalar::Enum(right)) => {
                self.queue(Node::Enum(left), Node::Enum(right));
                true
            }
            (Scalar::Tagged(left), Scalar::Tagged(right)) => {
                self.queue(Node::Tagged(left), Node::Tagged(right));
                true
            }
            (Scalar::Fn(left), Scalar::Fn(right)) => {
                self.queue(Node::Fn(left), Node::Fn(right));
                true
            }
            (Scalar::DynArray(left), Scalar::DynArray(right))
            | (Scalar::Slice(left), Scalar::Slice(right)) => left == right,
            (Scalar::Param(left), Scalar::Param(right)) => left == right,
            (Scalar::Bool, Scalar::Bool)
            | (Scalar::Char, Scalar::Char)
            | (Scalar::Unit, Scalar::Unit)
            | (Scalar::String, Scalar::String)
            | (Scalar::DynResponseArray, Scalar::DynResponseArray)
            | (Scalar::Str, Scalar::Str)
            | (Scalar::JsonDoc, Scalar::JsonDoc)
            | (Scalar::Reader, Scalar::Reader)
            | (Scalar::Writer, Scalar::Writer)
            | (Scalar::Buffer, Scalar::Buffer)
            | (Scalar::Regex, Scalar::Regex)
            | (Scalar::Captures, Scalar::Captures)
            | (Scalar::CliParsed, Scalar::CliParsed)
            | (Scalar::TcpConn, Scalar::TcpConn)
            | (Scalar::TcpListener, Scalar::TcpListener)
            | (Scalar::UdpSocket, Scalar::UdpSocket)
            | (Scalar::Child, Scalar::Child)
            | (Scalar::File, Scalar::File)
            | (Scalar::HttpResponse, Scalar::HttpResponse)
            | (Scalar::HttpServer, Scalar::HttpServer)
            | (Scalar::HttpRequestCtx, Scalar::HttpRequestCtx)
            | (Scalar::ResponseBuilder, Scalar::ResponseBuilder)
            | (Scalar::HttpStream, Scalar::HttpStream)
            | (Scalar::RunOutput, Scalar::RunOutput) => true,
            _ => false,
        }
    }

    fn types_equal(&mut self, left: Ty, right: Ty) -> bool {
        match (left, right) {
            (Ty::Int(left), Ty::Int(right)) => left == right,
            (Ty::Float(left), Ty::Float(right)) => left == right,
            (Ty::Param(left), Ty::Param(right))
            | (Ty::IntVar(left), Ty::IntVar(right))
            | (Ty::FloatVar(left), Ty::FloatVar(right)) => left == right,
            (Ty::Option(left), Ty::Option(right)) => self.scalars_equal(left, right),
            (Ty::Result(left_ok, left_err), Ty::Result(right_ok, right_err)) => {
                self.scalars_equal(left_ok, right_ok) && self.scalars_equal(left_err, right_err)
            }
            (Ty::Tagged(left), Ty::Tagged(right)) => {
                self.queue(Node::Tagged(left), Node::Tagged(right));
                true
            }
            (Ty::Box(left), Ty::Box(right)) => self.scalars_equal(left, right),
            (Ty::Array(left, left_len), Ty::Array(right, right_len)) => {
                left_len == right_len && self.scalars_equal(left, right)
            }
            (Ty::Vec(left, left_lanes), Ty::Vec(right, right_lanes))
            | (Ty::Mask(left, left_lanes), Ty::Mask(right, right_lanes)) => {
                left_lanes == right_lanes && self.scalars_equal(left, right)
            }
            (Ty::StructArray(left, left_len), Ty::StructArray(right, right_len)) => {
                if left_len != right_len {
                    return false;
                }
                self.queue(Node::Struct(left), Node::Struct(right));
                true
            }
            (Ty::DynStructArray(left, left_layout), Ty::DynStructArray(right, right_layout)) => {
                if left_layout != right_layout {
                    return false;
                }
                self.queue(Node::Struct(left), Node::Struct(right));
                true
            }
            (Ty::Slice(left), Ty::Slice(right)) => self.scalars_equal(left, right),
            (Ty::Soa(left), Ty::Soa(right)) | (Ty::JsonScanner(left), Ty::JsonScanner(right)) => {
                self.queue(Node::Struct(left), Node::Struct(right));
                true
            }
            (Ty::DynSliceArray(left), Ty::DynSliceArray(right)) => left == right,
            (Ty::DynArray(left), Ty::DynArray(right)) => self.scalars_equal(left, right),
            (Ty::ArrayBuilder(left), Ty::ArrayBuilder(right)) => self.scalars_equal(left, right),
            (Ty::Task(left), Ty::Task(right)) => self.scalars_equal(left, right),
            (Ty::DictEncoded(left, left_field), Ty::DictEncoded(right, right_field)) => {
                if left_field != right_field {
                    return false;
                }
                self.queue(Node::Struct(left), Node::Struct(right));
                true
            }
            (Ty::Struct(left), Ty::Struct(right)) => {
                self.queue(Node::Struct(left), Node::Struct(right));
                true
            }
            (Ty::Tuple(left), Ty::Tuple(right)) => {
                self.queue(Node::Tuple(left), Node::Tuple(right));
                true
            }
            (Ty::Fn(left), Ty::Fn(right)) => {
                self.queue(Node::Fn(left), Node::Fn(right));
                true
            }
            (Ty::Enum(left), Ty::Enum(right)) => {
                self.queue(Node::Enum(left), Node::Enum(right));
                true
            }
            (Ty::Bool, Ty::Bool)
            | (Ty::Char, Ty::Char)
            | (Ty::DynResponseArray, Ty::DynResponseArray)
            | (Ty::Str, Ty::Str)
            | (Ty::String, Ty::String)
            | (Ty::ArenaHandle, Ty::ArenaHandle)
            | (Ty::Raw, Ty::Raw)
            | (Ty::Builder, Ty::Builder)
            | (Ty::Writer, Ty::Writer)
            | (Ty::Reader, Ty::Reader)
            | (Ty::Buffer, Ty::Buffer)
            | (Ty::StrFinder, Ty::StrFinder)
            | (Ty::File, Ty::File)
            | (Ty::Rng, Ty::Rng)
            | (Ty::Regex, Ty::Regex)
            | (Ty::Captures, Ty::Captures)
            | (Ty::CliCommand, Ty::CliCommand)
            | (Ty::CliParsed, Ty::CliParsed)
            | (Ty::TcpConn, Ty::TcpConn)
            | (Ty::TcpListener, Ty::TcpListener)
            | (Ty::UdpSocket, Ty::UdpSocket)
            | (Ty::Child, Ty::Child)
            | (Ty::Command, Ty::Command)
            | (Ty::RunOutput, Ty::RunOutput)
            | (Ty::HttpRequest, Ty::HttpRequest)
            | (Ty::HttpResponse, Ty::HttpResponse)
            | (Ty::HttpClient, Ty::HttpClient)
            | (Ty::HttpServer, Ty::HttpServer)
            | (Ty::HttpRequestCtx, Ty::HttpRequestCtx)
            | (Ty::ResponseBuilder, Ty::ResponseBuilder)
            | (Ty::HttpStream, Ty::HttpStream)
            | (Ty::HttpHeaders, Ty::HttpHeaders)
            | (Ty::JsonDoc, Ty::JsonDoc)
            | (Ty::Unit, Ty::Unit)
            | (Ty::Error, Ty::Error) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Copy)]
enum ScalarPlacement {
    /// `scalar_arg(..., allow_param=...)`: Option/Result payloads and tagged entries.
    Payload { allow_param: bool },
    /// `collection_scalar_arg`: slice/array elements, with the explicit `Fn` extension.
    Collection,
    /// `ty_to_scalar`: an annotated `FnTy` parameter.  Buffers are legal here, unlike payloads.
    FnParameter { allow_param: bool },
}

struct PlacementValidator<'a> {
    program: &'a hir::Program,
    /// HIR may retain unreachable generic-template nodes containing `Param`.  The global graph
    /// validator already computes the same reverse dependency closure; placement uses it to keep
    /// those abstract nodes legal without weakening any concrete root.
    abstract_nodes: HashSet<Node>,
}

impl<'a> PlacementValidator<'a> {
    fn new(program: &'a hir::Program) -> Self {
        Self {
            program,
            abstract_nodes: abstract_nodes(program),
        }
    }

    fn validate(&self) -> bool {
        self.struct_fields_valid()
            && self.enum_payloads_valid()
            && self.tuples_valid()
            && self.tagged_payloads_valid()
            && self.function_types_valid()
            && self.source_functions_valid()
            && self.imported_functions_valid()
            && self.externs_valid()
    }

    fn is_abstract(&self, node: Node) -> bool {
        self.abstract_nodes.contains(&node)
    }

    fn struct_fields_valid(&self) -> bool {
        for (id, definition) in self.program.structs.iter().enumerate() {
            let abstract_node = self.is_abstract(Node::Struct(id as u32));
            for field in &definition.fields {
                if !self.field_type_ok(field.ty, abstract_node)
                    || (definition.c_repr && !matches!(field.ty, Ty::Int(_) | Ty::Float(_)))
                    || matches!(field.ty, Ty::DynArray(Scalar::String))
                    || !self.inline_structs_unaligned(field.ty)
                {
                    return false;
                }
            }
        }
        true
    }

    fn enum_payloads_valid(&self) -> bool {
        for (id, definition) in self.program.enums.iter().enumerate() {
            let abstract_node = self.is_abstract(Node::Enum(id as u32));
            for variant in &definition.variants {
                for &payload in &variant.payload {
                    let valid = if abstract_node {
                        self.scalar_ok(payload, ScalarPlacement::Payload { allow_param: true })
                    } else {
                        self.concrete_enum_payload_ok(payload)
                    };
                    if !valid || !self.inline_structs_unaligned(align_sema::scalar_to_ty(payload)) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn tuples_valid(&self) -> bool {
        for (id, tuple) in self.program.tuples.iter().enumerate() {
            for &element in &tuple.elems {
                if !self.tuple_element_ok(element)
                    || (self.is_abstract(Node::Tuple(id as u32))
                        && matches!(element, Scalar::Param(_)))
                {
                    // Tuple templates are not emitted by sema.  A `Param` here is therefore not
                    // a producer-valid tuple element even when the node is unreachable.
                    return false;
                }
            }
        }
        true
    }

    fn tagged_payloads_valid(&self) -> bool {
        for (id, entry) in self.program.tagged_types.iter().enumerate() {
            let mode = ScalarPlacement::Payload {
                allow_param: self.is_abstract(Node::Tagged(id as u32)),
            };
            let valid = match *entry {
                hir::TaggedType::Option(payload) => self.scalar_ok(payload, mode),
                hir::TaggedType::Result(ok, err) => {
                    self.scalar_ok(ok, mode) && self.scalar_ok(err, mode)
                }
            };
            if !valid {
                return false;
            }
        }
        true
    }

    fn function_types_valid(&self) -> bool {
        for (id, function) in self.program.fn_types.iter().enumerate() {
            let allow_param = self.is_abstract(Node::Fn(id as u32));
            if !function.params.iter().all(|(_, parameter)| {
                self.scalar_ok(*parameter, ScalarPlacement::FnParameter { allow_param })
            }) || !self.resolve_type_ok(function.ret, allow_param)
            {
                return false;
            }
        }
        true
    }

    fn source_functions_valid(&self) -> bool {
        for function in &self.program.fns {
            if !function.params.iter().all(|&local| {
                function
                    .locals
                    .get(local as usize)
                    .is_some_and(|local| self.source_function_type_ok(local.ty, true, false))
            }) || !self.source_function_type_ok(function.ret, false, true)
            {
                return false;
            }
        }
        true
    }

    fn imported_functions_valid(&self) -> bool {
        for function in &self.program.imported_fns {
            if !function
                .params
                .iter()
                .all(|&parameter| self.source_function_type_ok(parameter, true, false))
                || !self.source_function_type_ok(function.ret, false, true)
            {
                return false;
            }
        }
        true
    }

    fn externs_valid(&self) -> bool {
        for function in &self.program.externs {
            if !function
                .params
                .iter()
                .all(|&parameter| self.ffi_parameter_ok(parameter))
                || !self.ffi_return_ok(function.ret)
            {
                return false;
            }
        }
        true
    }

    fn field_type_ok(&self, ty: Ty, allow_param: bool) -> bool {
        match ty {
            Ty::Param(_) => allow_param,
            Ty::Int(integer) => valid_int(integer.bits),
            Ty::Float(float) => valid_float(float.bits),
            Ty::Bool | Ty::Char | Ty::Str | Ty::String => true,
            Ty::Struct(id) => self.program.structs.get(id as usize).is_some(),
            Ty::Enum(id) => self.program.enums.get(id as usize).is_some(),
            Ty::Option(payload) => self.field_scalar_ok(payload, allow_param),
            Ty::Result(ok, err) => {
                self.field_scalar_ok(ok, allow_param) && self.field_scalar_ok(err, allow_param)
            }
            Ty::Tagged(id) => self.field_tagged_payload_ok(id, allow_param),
            Ty::Slice(element) => self.scalar_ok(element, ScalarPlacement::Collection),
            Ty::DynArray(element) => {
                // `array<Struct>` has its own `DynStructArray` producer; a plain scalar array
                // with a struct element is a malformed HIR spelling of that type.
                !matches!(element, Scalar::Struct(_))
                    && self.scalar_ok(element, ScalarPlacement::Collection)
            }
            Ty::DynStructArray(id, Layout::Aos) => self.dynamic_struct_array_ok(id),
            Ty::Fn(id) => self.program.fn_types.get(id as usize).is_some(),
            _ if align_sema::is_move_handle(ty) => true,
            Ty::HttpHeaders => true,
            _ => false,
        }
    }

    fn field_scalar_ok(&self, initial: Scalar, allow_param: bool) -> bool {
        #[derive(Clone, Copy)]
        enum Work {
            Enter(Scalar),
            ExitTagged(u32),
        }

        let mut work = vec![Work::Enter(initial)];
        let mut active_tagged = HashSet::new();
        let mut completed_tagged = HashSet::new();
        while let Some(item) = work.pop() {
            match item {
                Work::ExitTagged(id) => {
                    if !active_tagged.remove(&id) {
                        return false;
                    }
                    completed_tagged.insert(id);
                }
                Work::Enter(Scalar::Tagged(id)) => {
                    if completed_tagged.contains(&id) {
                        continue;
                    }
                    if !active_tagged.insert(id) {
                        return false;
                    }
                    let Some(entry) = self.program.tagged_types.get(id as usize) else {
                        return false;
                    };
                    work.push(Work::ExitTagged(id));
                    match *entry {
                        hir::TaggedType::Option(payload) => work.push(Work::Enter(payload)),
                        hir::TaggedType::Result(ok, err) => {
                            work.push(Work::Enter(err));
                            work.push(Work::Enter(ok));
                        }
                    }
                }
                Work::Enter(Scalar::Param(_)) => {
                    if !allow_param {
                        return false;
                    }
                }
                Work::Enter(scalar) => {
                    if !self.scalar_ok(scalar, ScalarPlacement::Payload { allow_param })
                        || !self.field_type_ok(align_sema::scalar_to_ty(scalar), allow_param)
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn field_tagged_payload_ok(&self, id: u32, allow_param: bool) -> bool {
        let Some(entry) = self.program.tagged_types.get(id as usize) else {
            return false;
        };
        match *entry {
            hir::TaggedType::Option(payload) => self.field_scalar_ok(payload, allow_param),
            hir::TaggedType::Result(ok, err) => {
                self.field_scalar_ok(ok, allow_param) && self.field_scalar_ok(err, allow_param)
            }
        }
    }

    fn tuple_element_ok(&self, scalar: Scalar) -> bool {
        match scalar {
            Scalar::Int(integer) => valid_int(integer.bits),
            Scalar::Float(float) => valid_float(float.bits),
            Scalar::Bool | Scalar::Char | Scalar::Str | Scalar::String => true,
            Scalar::DynArray(element) => valid_prim(element),
            Scalar::DynStructArray(id) => self.dynamic_struct_array_ok(id),
            _ => false,
        }
    }

    fn scalar_ok(&self, initial: Scalar, mode: ScalarPlacement) -> bool {
        #[derive(Clone, Copy)]
        enum Work {
            Enter(Scalar),
            ExitTagged(u32),
        }

        let mut work = vec![Work::Enter(initial)];
        let mut active_tagged = HashSet::new();
        let mut completed_tagged = HashSet::new();
        while let Some(item) = work.pop() {
            match item {
                Work::ExitTagged(id) => {
                    if !active_tagged.remove(&id) {
                        return false;
                    }
                    completed_tagged.insert(id);
                }
                Work::Enter(Scalar::Tagged(id)) => {
                    if completed_tagged.contains(&id) {
                        continue;
                    }
                    if !active_tagged.insert(id) {
                        return false;
                    }
                    let Some(entry) = self.program.tagged_types.get(id as usize) else {
                        return false;
                    };
                    work.push(Work::ExitTagged(id));
                    match *entry {
                        hir::TaggedType::Option(payload) => work.push(Work::Enter(payload)),
                        hir::TaggedType::Result(ok, err) => {
                            work.push(Work::Enter(err));
                            work.push(Work::Enter(ok));
                        }
                    }
                }
                Work::Enter(scalar) => {
                    if !self.scalar_leaf_ok(scalar, mode) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn scalar_leaf_ok(&self, scalar: Scalar, mode: ScalarPlacement) -> bool {
        match scalar {
            Scalar::Int(integer) => valid_int(integer.bits),
            Scalar::Float(float) => valid_float(float.bits),
            Scalar::Param(_) => matches!(
                mode,
                ScalarPlacement::Payload { allow_param: true }
                    | ScalarPlacement::FnParameter { allow_param: true }
            ),
            Scalar::Struct(id) => self.program.structs.get(id as usize).is_some(),
            Scalar::Enum(id) => self.program.enums.get(id as usize).is_some(),
            Scalar::Fn(id) => {
                matches!(mode, ScalarPlacement::Collection)
                    && self.program.fn_types.get(id as usize).is_some()
            }
            Scalar::DynStructArray(id) => self.dynamic_struct_array_ok(id),
            Scalar::Soa(id) => self.soa_ok(id),
            Scalar::DynArray(element) => {
                !matches!(mode, ScalarPlacement::Collection) && valid_prim(element)
            }
            Scalar::Slice(element) => valid_prim(element),
            Scalar::Buffer => !matches!(
                mode,
                ScalarPlacement::Payload { .. } | ScalarPlacement::Collection
            ),
            Scalar::Reader
            | Scalar::Writer
            | Scalar::Regex
            | Scalar::Captures
            | Scalar::CliParsed
            | Scalar::TcpConn
            | Scalar::TcpListener
            | Scalar::UdpSocket
            | Scalar::Child
            | Scalar::HttpResponse
            | Scalar::HttpServer
            | Scalar::HttpRequestCtx
            | Scalar::HttpStream
            | Scalar::ResponseBuilder
            | Scalar::RunOutput => !matches!(mode, ScalarPlacement::Collection),
            Scalar::File => !matches!(mode, ScalarPlacement::Collection),
            Scalar::Bool
            | Scalar::Char
            | Scalar::Unit
            | Scalar::String
            | Scalar::DynResponseArray
            | Scalar::Str
            | Scalar::JsonDoc => true,
            // Expanded tagged nodes are consumed by `scalar_ok` before reaching this leaf arm.
            Scalar::Tagged(_) => false,
        }
    }

    fn tagged_payload_ok(&self, id: u32, mode: ScalarPlacement) -> bool {
        let Some(entry) = self.program.tagged_types.get(id as usize) else {
            return false;
        };
        match *entry {
            hir::TaggedType::Option(payload) => self.scalar_ok(payload, mode),
            hir::TaggedType::Result(ok, err) => {
                self.scalar_ok(ok, mode) && self.scalar_ok(err, mode)
            }
        }
    }

    fn concrete_enum_payload_ok(&self, scalar: Scalar) -> bool {
        match scalar {
            Scalar::Int(integer) => valid_int(integer.bits),
            Scalar::Float(float) => valid_float(float.bits),
            Scalar::Bool | Scalar::Char | Scalar::Str | Scalar::String => true,
            Scalar::Struct(id) => self.program.structs.get(id as usize).is_some(),
            Scalar::Enum(id) => self.program.enums.get(id as usize).is_some(),
            Scalar::Fn(id) => self.program.fn_types.get(id as usize).is_some(),
            Scalar::ResponseBuilder => true,
            Scalar::DynArray(PrimScalar::String) => false,
            Scalar::DynArray(_) => true,
            Scalar::DynStructArray(id) => {
                self.dynamic_struct_array_ok(id)
                    && !align_sema::struct_is_move(
                        id,
                        &self.program.structs,
                        &self.program.enums,
                        &self.program.tagged_types,
                    )
            }
            Scalar::Tagged(id) => {
                self.tagged_payload_ok(id, ScalarPlacement::Payload { allow_param: false })
            }
            _ => false,
        }
    }

    fn resolve_type_ok(&self, ty: Ty, allow_param: bool) -> bool {
        match ty {
            Ty::Param(_) => allow_param,
            Ty::Int(integer) => valid_int(integer.bits),
            Ty::Float(float) => valid_float(float.bits),
            Ty::Bool | Ty::Char | Ty::Str | Ty::String | Ty::Unit | Ty::Raw => true,
            Ty::Option(payload) => {
                self.scalar_ok(payload, ScalarPlacement::Payload { allow_param })
            }
            Ty::Result(ok, err) => {
                self.scalar_ok(ok, ScalarPlacement::Payload { allow_param })
                    && self.scalar_ok(err, ScalarPlacement::Payload { allow_param })
            }
            Ty::Tagged(id) => self.tagged_payload_ok(id, ScalarPlacement::Payload { allow_param }),
            // `scalar_arg` always resolves a box payload with `allow_param=false`, even while
            // validating an abstract generic template. A `box<Param>` HIR node is therefore
            // graph-valid but never producer-valid.
            Ty::Box(payload) => self.box_payload_ok(payload),
            Ty::Vec(element, lanes) | Ty::Mask(element, lanes) => {
                matches!(lanes, 2 | 4 | 8 | 16)
                    && matches!(element, Scalar::Int(_) | Scalar::Float(_))
                    && self.scalar_ok(element, ScalarPlacement::FnParameter { allow_param })
            }
            Ty::DynStructArray(id, Layout::Aos) => self.dynamic_struct_array_ok(id),
            Ty::Slice(element) => self.scalar_ok(element, ScalarPlacement::Collection),
            Ty::DynArray(element) => {
                !matches!(element, Scalar::Struct(_))
                    && self.scalar_ok(element, ScalarPlacement::Collection)
            }
            Ty::Soa(id) => self.soa_ok(id),
            Ty::ArrayBuilder(element) => matches!(
                element,
                Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char | Scalar::String
            ),
            Ty::JsonScanner(id) => self.program.structs.get(id as usize).is_some(),
            Ty::Struct(id) => self.program.structs.get(id as usize).is_some(),
            Ty::Tuple(id) => self.program.tuples.get(id as usize).is_some(),
            Ty::Fn(id) => self.program.fn_types.get(id as usize).is_some(),
            Ty::Enum(id) => self.program.enums.get(id as usize).is_some(),
            Ty::Writer
            | Ty::Reader
            | Ty::Buffer
            | Ty::Regex
            | Ty::Captures
            | Ty::TcpConn
            | Ty::TcpListener
            | Ty::UdpSocket
            | Ty::Child
            | Ty::File
            | Ty::HttpRequestCtx
            | Ty::ResponseBuilder
            | Ty::HttpStream => true,
            // These handles are body-produced only. They are valid local/expression types but have
            // no source `resolve_type` spelling and therefore cannot occur in a declaration header.
            Ty::CliParsed
            | Ty::HttpRequest
            | Ty::HttpResponse
            | Ty::HttpClient
            | Ty::HttpServer
            | Ty::Command
            | Ty::RunOutput => false,
            Ty::CliCommand => false,
            _ if align_sema::is_move_handle(ty) => true,
            Ty::Rng | Ty::HttpHeaders | Ty::JsonDoc => true,
            // These are body-produced or compiler-internal values, not results of `resolve_type`
            // in a declaration/header position.
            Ty::IntVar(_)
            | Ty::FloatVar(_)
            | Ty::Array(_, _)
            | Ty::StructArray(_, _)
            | Ty::DynSliceArray(_)
            | Ty::DynResponseArray
            | Ty::Task(_)
            | Ty::ArenaHandle
            | Ty::Builder
            | Ty::StrFinder
            | Ty::DictEncoded(..)
            | Ty::Error => false,
            Ty::DynStructArray(_, Layout::Soa) => false,
        }
    }

    fn source_function_type_ok(&self, ty: Ty, parameter: bool, return_position: bool) -> bool {
        self.resolve_type_ok(ty, false)
            && !(parameter && matches!(ty, Ty::Box(_)))
            && !(return_position && matches!(ty, Ty::Box(_) | Ty::Fn(_)))
    }

    fn box_payload_ok(&self, payload: Scalar) -> bool {
        if !self.scalar_ok(payload, ScalarPlacement::Payload { allow_param: false }) {
            return false;
        }
        match payload {
            Scalar::Struct(_) | Scalar::Enum(_) | Scalar::Str => false,
            Scalar::Tagged(id) => !align_sema::drop_plan(
                Ty::Tagged(id),
                &self.program.structs,
                &self.program.enums,
                &self.program.tagged_types,
            )
            .needs_drop(),
            other => !other.is_move(),
        }
    }

    fn dynamic_struct_array_ok(&self, id: u32) -> bool {
        self.program
            .structs
            .get(id as usize)
            .is_some_and(|definition| definition.align.is_none())
    }

    fn soa_ok(&self, id: u32) -> bool {
        self.program
            .structs
            .get(id as usize)
            .is_some_and(|definition| {
                !definition.fields.is_empty()
                    && definition.fields.iter().all(|field| {
                        matches!(
                            field.ty,
                            Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str
                        )
                    })
            })
    }

    fn inline_structs_unaligned(&self, ty: Ty) -> bool {
        enum Work {
            Enter(Ty),
            ExitTagged(u32),
            ExitEnum(u32),
        }
        let mut work = vec![Work::Enter(ty)];
        let mut active_tagged = HashSet::new();
        let mut completed_tagged = HashSet::new();
        let mut active_enums = HashSet::new();
        let mut completed_enums = HashSet::new();
        while let Some(item) = work.pop() {
            match item {
                Work::ExitTagged(id) => {
                    if !active_tagged.remove(&id) {
                        return false;
                    }
                    completed_tagged.insert(id);
                }
                Work::ExitEnum(id) => {
                    if !active_enums.remove(&id) {
                        return false;
                    }
                    completed_enums.insert(id);
                }
                Work::Enter(ty) => match ty {
                    Ty::Struct(id) => {
                        if self
                            .program
                            .structs
                            .get(id as usize)
                            .is_none_or(|definition| definition.align.is_some())
                        {
                            return false;
                        }
                    }
                    Ty::Option(payload) => {
                        work.push(Work::Enter(align_sema::scalar_to_ty(payload)))
                    }
                    Ty::Result(ok, err) => {
                        work.push(Work::Enter(align_sema::scalar_to_ty(err)));
                        work.push(Work::Enter(align_sema::scalar_to_ty(ok)));
                    }
                    Ty::Enum(id) => {
                        if completed_enums.contains(&id) {
                            continue;
                        }
                        if !active_enums.insert(id) {
                            return false;
                        }
                        let Some(definition) = self.program.enums.get(id as usize) else {
                            return false;
                        };
                        work.push(Work::ExitEnum(id));
                        for variant in &definition.variants {
                            for &payload in &variant.payload {
                                work.push(Work::Enter(align_sema::scalar_to_ty(payload)));
                            }
                        }
                    }
                    Ty::Tagged(id) => {
                        if completed_tagged.contains(&id) {
                            continue;
                        }
                        if !active_tagged.insert(id) {
                            return false;
                        }
                        let Some(entry) = self.program.tagged_types.get(id as usize) else {
                            return false;
                        };
                        work.push(Work::ExitTagged(id));
                        match *entry {
                            hir::TaggedType::Option(payload) => {
                                work.push(Work::Enter(align_sema::scalar_to_ty(payload)));
                            }
                            hir::TaggedType::Result(ok, err) => {
                                work.push(Work::Enter(align_sema::scalar_to_ty(err)));
                                work.push(Work::Enter(align_sema::scalar_to_ty(ok)));
                            }
                        }
                    }
                    _ => {}
                },
            }
        }
        true
    }

    fn ffi_parameter_ok(&self, ty: Ty) -> bool {
        match ty {
            Ty::Int(integer) => valid_int(integer.bits),
            Ty::Float(float) => valid_float(float.bits),
            Ty::Raw | Ty::Str => true,
            Ty::Slice(Scalar::Int(integer)) => valid_int(integer.bits),
            Ty::Slice(Scalar::Float(float)) => valid_float(float.bits),
            Ty::Struct(id) => self.ffi_struct_ok(id),
            _ => false,
        }
    }

    fn ffi_return_ok(&self, ty: Ty) -> bool {
        match ty {
            Ty::Unit => true,
            Ty::Int(integer) => valid_int(integer.bits),
            Ty::Float(float) => valid_float(float.bits),
            Ty::Raw => true,
            Ty::Struct(id) => self.ffi_struct_ok(id),
            _ => false,
        }
    }

    fn ffi_struct_ok(&self, id: u32) -> bool {
        self.program
            .structs
            .get(id as usize)
            .is_some_and(|definition| {
                definition.c_repr
                    && !definition.fields.is_empty()
                    && definition
                        .fields
                        .iter()
                        .all(|field| matches!(field.ty, Ty::Int(_) | Ty::Float(_)))
            })
    }
}

/// Compute the reverse dependency closure of `Param`-bearing graph nodes.  This mirrors the
/// global validator's template treatment but leaves the result available to the placement pass.
fn abstract_nodes(program: &hir::Program) -> HashSet<Node> {
    let mut validator = Validator::new(program);
    if !validator.collect_node_facts() {
        return HashSet::new();
    }
    let mut reverse = HashMap::<Node, Vec<Node>>::new();
    let mut queue = VecDeque::new();
    let mut abstract_nodes = HashSet::new();
    for (&node, facts) in &validator.facts {
        if facts.has_param && abstract_nodes.insert(node) {
            queue.push_back(node);
        }
        for &(dependency, _) in &facts.refs {
            reverse.entry(dependency).or_default().push(node);
        }
    }
    while let Some(node) = queue.pop_front() {
        for &dependent in reverse.get(&node).into_iter().flatten() {
            if abstract_nodes.insert(dependent) {
                queue.push_back(dependent);
            }
        }
    }
    abstract_nodes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Node {
    Struct(u32),
    Enum(u32),
    Tuple(u32),
    Tagged(u32),
    Fn(u32),
}

#[derive(Clone, Copy)]
enum Edge {
    Inline,
    Header,
}

#[derive(Default)]
struct NodeFacts {
    refs: Vec<(Node, Edge)>,
    has_param: bool,
}

struct Validator<'a> {
    program: &'a hir::Program,
    nodes: Vec<Node>,
    facts: HashMap<Node, NodeFacts>,
}

impl<'a> Validator<'a> {
    fn new(program: &'a hir::Program) -> Self {
        let mut nodes = Vec::with_capacity(
            program.structs.len()
                + program.enums.len()
                + program.tuples.len()
                + program.tagged_types.len()
                + program.fn_types.len(),
        );
        nodes.extend((0..program.structs.len()).map(|id| Node::Struct(id as u32)));
        nodes.extend((0..program.enums.len()).map(|id| Node::Enum(id as u32)));
        nodes.extend((0..program.tuples.len()).map(|id| Node::Tuple(id as u32)));
        nodes.extend((0..program.tagged_types.len()).map(|id| Node::Tagged(id as u32)));
        nodes.extend((0..program.fn_types.len()).map(|id| Node::Fn(id as u32)));
        Self {
            program,
            nodes,
            facts: HashMap::new(),
        }
    }

    fn validate(mut self) -> bool {
        self.collect_node_facts()
            && self.root_types_are_concrete()
            && self.inline_graph_is_acyclic()
    }

    fn collect_node_facts(&mut self) -> bool {
        for node in self.nodes.clone() {
            let mut facts = NodeFacts::default();
            let valid = match node {
                Node::Struct(id) => self.program.structs[id as usize]
                    .fields
                    .iter()
                    .all(|field| self.inspect_ty(field.ty, Edge::Inline, &mut facts)),
                Node::Enum(id) => self.program.enums[id as usize]
                    .variants
                    .iter()
                    .flat_map(|variant| &variant.payload)
                    .all(|&payload| self.inspect_scalar(payload, Edge::Inline, &mut facts)),
                Node::Tuple(id) => self.program.tuples[id as usize]
                    .elems
                    .iter()
                    .all(|&element| self.inspect_scalar(element, Edge::Inline, &mut facts)),
                Node::Tagged(id) => match self.program.tagged_types[id as usize] {
                    hir::TaggedType::Option(payload) => {
                        self.inspect_scalar(payload, Edge::Inline, &mut facts)
                    }
                    hir::TaggedType::Result(ok, err) => {
                        self.inspect_scalar(ok, Edge::Inline, &mut facts)
                            && self.inspect_scalar(err, Edge::Inline, &mut facts)
                    }
                },
                Node::Fn(id) => {
                    let function = &self.program.fn_types[id as usize];
                    function.params.iter().all(|(_, parameter)| {
                        self.inspect_scalar(*parameter, Edge::Header, &mut facts)
                    }) && self.inspect_ty(function.ret, Edge::Header, &mut facts)
                }
            };
            if !valid {
                return false;
            }
            self.facts.insert(node, facts);
        }
        true
    }

    fn inspect_ty(&self, ty: Ty, edge: Edge, facts: &mut NodeFacts) -> bool {
        match ty {
            Ty::Int(integer) => valid_int(integer.bits),
            Ty::Float(float) => valid_float(float.bits),
            Ty::Param(_) => {
                facts.has_param = true;
                true
            }
            Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Error | Ty::StrFinder => false,
            Ty::Option(payload) | Ty::Array(payload, _) => {
                self.inspect_scalar(payload, edge, facts)
            }
            Ty::Result(ok, err) => {
                self.inspect_scalar(ok, edge, facts) && self.inspect_scalar(err, edge, facts)
            }
            Ty::Vec(element, lanes) | Ty::Mask(element, lanes) => {
                matches!(lanes, 2 | 4 | 8 | 16)
                    && matches!(element, Scalar::Int(_) | Scalar::Float(_))
                    && self.inspect_scalar(element, edge, facts)
            }
            Ty::Tagged(id) => self.push_ref(Node::Tagged(id), edge, facts),
            Ty::Box(payload)
            | Ty::Slice(payload)
            | Ty::DynArray(payload)
            | Ty::ArrayBuilder(payload)
            | Ty::Task(payload) => self.inspect_scalar(payload, Edge::Header, facts),
            Ty::StructArray(id, _) => self.push_ref(Node::Struct(id), edge, facts),
            Ty::DynStructArray(id, _) | Ty::Soa(id) | Ty::JsonScanner(id) => {
                self.push_ref(Node::Struct(id), Edge::Header, facts)
            }
            Ty::DynSliceArray(element) => valid_prim(element),
            Ty::Struct(id) => self.push_ref(Node::Struct(id), edge, facts),
            Ty::Tuple(id) => self.push_ref(Node::Tuple(id), edge, facts),
            Ty::Fn(id) => self.push_ref(Node::Fn(id), Edge::Header, facts),
            Ty::Enum(id) => self.push_ref(Node::Enum(id), edge, facts),
            Ty::DictEncoded(id, field) => {
                let Some(definition) = self.program.structs.get(id as usize) else {
                    return false;
                };
                definition
                    .fields
                    .get(field as usize)
                    .is_some_and(|definition| definition.ty == Ty::Str)
                    && self.push_ref(Node::Struct(id), Edge::Header, facts)
            }
            Ty::Bool
            | Ty::Char
            | Ty::DynResponseArray
            | Ty::Str
            | Ty::String
            | Ty::ArenaHandle
            | Ty::Raw
            | Ty::Builder
            | Ty::Writer
            | Ty::Reader
            | Ty::Buffer
            | Ty::File
            | Ty::Rng
            | Ty::Regex
            | Ty::Captures
            | Ty::CliCommand
            | Ty::CliParsed
            | Ty::TcpConn
            | Ty::TcpListener
            | Ty::UdpSocket
            | Ty::Child
            | Ty::Command
            | Ty::RunOutput
            | Ty::HttpRequest
            | Ty::HttpResponse
            | Ty::HttpClient
            | Ty::HttpServer
            | Ty::HttpRequestCtx
            | Ty::ResponseBuilder
            | Ty::HttpStream
            | Ty::HttpHeaders
            | Ty::JsonDoc
            | Ty::Unit => true,
        }
    }

    fn inspect_scalar(&self, scalar: Scalar, edge: Edge, facts: &mut NodeFacts) -> bool {
        match scalar {
            Scalar::Int(integer) => valid_int(integer.bits),
            Scalar::Float(float) => valid_float(float.bits),
            Scalar::Param(_) => {
                facts.has_param = true;
                true
            }
            Scalar::Struct(id) => self.push_ref(Node::Struct(id), edge, facts),
            Scalar::Enum(id) => self.push_ref(Node::Enum(id), edge, facts),
            Scalar::Tagged(id) => self.push_ref(Node::Tagged(id), edge, facts),
            Scalar::Fn(id) => self.push_ref(Node::Fn(id), Edge::Header, facts),
            Scalar::DynStructArray(id) | Scalar::Soa(id) => {
                self.push_ref(Node::Struct(id), Edge::Header, facts)
            }
            Scalar::DynArray(element) | Scalar::Slice(element) => valid_prim(element),
            Scalar::Bool
            | Scalar::Char
            | Scalar::Unit
            | Scalar::String
            | Scalar::DynResponseArray
            | Scalar::Str
            | Scalar::JsonDoc
            | Scalar::Reader
            | Scalar::Writer
            | Scalar::Buffer
            | Scalar::Regex
            | Scalar::Captures
            | Scalar::CliParsed
            | Scalar::TcpConn
            | Scalar::TcpListener
            | Scalar::UdpSocket
            | Scalar::Child
            | Scalar::File
            | Scalar::HttpResponse
            | Scalar::HttpServer
            | Scalar::HttpRequestCtx
            | Scalar::ResponseBuilder
            | Scalar::HttpStream
            | Scalar::RunOutput => true,
        }
    }

    fn push_ref(&self, node: Node, edge: Edge, facts: &mut NodeFacts) -> bool {
        if !self.node_exists(node) {
            return false;
        }
        facts.refs.push((node, edge));
        true
    }

    fn node_exists(&self, node: Node) -> bool {
        match node {
            Node::Struct(id) => self.program.structs.get(id as usize).is_some(),
            Node::Enum(id) => self.program.enums.get(id as usize).is_some(),
            Node::Tuple(id) => self.program.tuples.get(id as usize).is_some(),
            Node::Tagged(id) => self.program.tagged_types.get(id as usize).is_some(),
            Node::Fn(id) => self.program.fn_types.get(id as usize).is_some(),
        }
    }

    fn root_types_are_concrete(&self) -> bool {
        let mut roots = Vec::new();
        roots.extend((0..self.program.structs.len()).map(|id| Node::Struct(id as u32)));
        roots.extend((0..self.program.enums.len()).map(|id| Node::Enum(id as u32)));
        roots.extend((0..self.program.tuples.len()).map(|id| Node::Tuple(id as u32)));

        for function in &self.program.fns {
            for local in &function.locals {
                if !self.inspect_root_ty(local.ty, &mut roots) {
                    return false;
                }
            }
            if !self.inspect_root_ty(function.ret, &mut roots) {
                return false;
            }
        }
        for function in &self.program.externs {
            for &parameter in &function.params {
                if !self.inspect_root_ty(parameter, &mut roots) {
                    return false;
                }
            }
            if !self.inspect_root_ty(function.ret, &mut roots) {
                return false;
            }
        }
        for function in &self.program.imported_fns {
            for &parameter in &function.params {
                if !self.inspect_root_ty(parameter, &mut roots) {
                    return false;
                }
            }
            if !self.inspect_root_ty(function.ret, &mut roots) {
                return false;
            }
        }

        let mut reverse = HashMap::<Node, Vec<Node>>::new();
        let mut templates = VecDeque::new();
        let mut template_nodes = HashSet::new();
        for (&node, facts) in &self.facts {
            if facts.has_param && template_nodes.insert(node) {
                templates.push_back(node);
            }
            for &(dependency, _) in &facts.refs {
                reverse.entry(dependency).or_default().push(node);
            }
        }
        while let Some(node) = templates.pop_front() {
            for &dependent in reverse.get(&node).into_iter().flatten() {
                if template_nodes.insert(dependent) {
                    templates.push_back(dependent);
                }
            }
        }
        roots.extend(
            self.nodes
                .iter()
                .copied()
                .filter(|node| !template_nodes.contains(node)),
        );

        let mut reachable = HashSet::new();
        let mut queue: VecDeque<_> = roots.into();
        while let Some(node) = queue.pop_front() {
            if !reachable.insert(node) {
                continue;
            }
            if self.facts[&node].has_param {
                return false;
            }
            queue.extend(
                self.facts[&node]
                    .refs
                    .iter()
                    .map(|&(dependency, _)| dependency),
            );
        }
        reachable.is_disjoint(&template_nodes)
    }

    fn inspect_root_ty(&self, ty: Ty, roots: &mut Vec<Node>) -> bool {
        let mut facts = NodeFacts::default();
        if !self.inspect_ty(ty, Edge::Inline, &mut facts) || facts.has_param {
            return false;
        }
        roots.extend(facts.refs.into_iter().map(|(node, _)| node));
        true
    }

    fn inline_graph_is_acyclic(&self) -> bool {
        #[derive(Clone, Copy)]
        enum Work {
            Enter(Node),
            Exit(Node),
        }

        let mut complete = HashSet::new();
        let mut active = HashSet::new();
        for &root in &self.nodes {
            let mut work = vec![Work::Enter(root)];
            while let Some(item) = work.pop() {
                match item {
                    Work::Enter(node) => {
                        if complete.contains(&node) {
                            continue;
                        }
                        if !active.insert(node) {
                            return false;
                        }
                        work.push(Work::Exit(node));
                        for &(dependency, edge) in self.facts[&node].refs.iter().rev() {
                            if matches!(edge, Edge::Inline) {
                                work.push(Work::Enter(dependency));
                            }
                        }
                    }
                    Work::Exit(node) => {
                        if !active.remove(&node) {
                            return false;
                        }
                        complete.insert(node);
                    }
                }
            }
        }
        true
    }
}

fn valid_int(bits: u8) -> bool {
    matches!(bits, 8 | 16 | 32 | 64)
}

fn valid_float(bits: u8) -> bool {
    matches!(bits, 32 | 64)
}

fn valid_prim(primitive: PrimScalar) -> bool {
    match primitive {
        PrimScalar::Int(integer) => valid_int(integer.bits),
        PrimScalar::Float(float) => valid_float(float.bits),
        PrimScalar::Bool | PrimScalar::Char | PrimScalar::Str | PrimScalar::String => true,
    }
}

#[allow(dead_code)]
mod body_core {
    use super::*;

    pub(super) fn validate(program: &hir::Program) -> bool {
        BodyValidator::new(program).validate()
    }

#[derive(Clone, Copy)]
struct SpawnContext {
    fallible: bool,
    ok: Scalar,
}

#[derive(Clone)]
struct BodyContext {
    function: usize,
    unsafe_depth: u32,
    arena_depth: u32,
    task_depth: u32,
    task_group_fallible: Vec<bool>,
    loop_targets: Vec<Ty>,
    spawn: Option<SpawnContext>,
    /// A pooled array literal is valid only as the direct initializer of the named immutable
    /// local.  The producer records no separate parent pointer in HIR, so the worklist carries
    /// this one narrow lexical fact while entering a `let` initializer and clears it for every
    /// retained child.
    pooled_initializer: Option<hir::LocalId>,
}

#[derive(Clone, Debug)]
struct BodyFlow {
    ty: Ty,
    /// Whether at least one control path reaches the end of this record.  This is the same
    /// conservative "always diverges" distinction used by sema: a branch with one falling
    /// alternative is considered falling.
    falls: bool,
    /// Accepted breaks that can reach the enclosing loop.  A nested Loop consumes its own breaks.
    breaks: Vec<Ty>,
}

struct BodySignature {
    params: Vec<Ty>,
    modes: Vec<align_ast::ParamMode>,
    ret: Ty,
    origin: Option<hir::FnOrigin>,
    is_extern: bool,
}

enum BodyWork<'a> {
    EnterBlock(&'a hir::Block, BodyContext),
    ExitBlock(&'a hir::Block, BodyContext),
    EnterStmt(&'a hir::Stmt, BodyContext),
    ExitStmt(&'a hir::Stmt, BodyContext),
    EnterExpr(&'a hir::Expr, BodyContext),
    ExitExpr(&'a hir::Expr, BodyContext),
    EnterArm(&'a hir::MatchArm, &'a hir::Expr, BodyContext),
    ExitArm(&'a hir::MatchArm),
}

#[derive(Clone, Copy)]
enum BodyJsonRoot {
    Struct(u32),
    Enum(u32),
}

struct BodyJsonUnionFrame {
    id: u32,
    next: usize,
    class_seen: [bool; align_sema::JSON_SHAPE_CLASSES],
    ok: bool,
}

enum BodyJsonWork {
    Start(BodyJsonRoot),
    Struct { id: u32, next: usize },
    AfterStruct { id: u32, next: usize },
    Union(BodyJsonUnionFrame),
    AfterUnion(BodyJsonUnionFrame),
}

struct BodyValidator<'a> {
    program: &'a hir::Program,
    placement: PlacementValidator<'a>,
    exprs: HashMap<usize, BodyFlow>,
    blocks: HashMap<usize, BodyFlow>,
    statements: HashMap<usize, BodyFlow>,
    arms: HashMap<usize, BodyFlow>,
    binding_counts: HashMap<(usize, hir::LocalId), usize>,
}

impl<'a> BodyValidator<'a> {
    fn new(program: &'a hir::Program) -> Self {
        Self {
            program,
            placement: PlacementValidator::new(program),
            exprs: HashMap::new(),
            blocks: HashMap::new(),
            statements: HashMap::new(),
            arms: HashMap::new(),
            binding_counts: HashMap::new(),
        }
    }

    fn validate(mut self) -> bool {
        for (function_index, function) in self.program.fns.iter().enumerate() {
            if !valid_span(function.span) || !self.locals_valid(function) {
                return false;
            }
            let context = BodyContext {
                function: function_index,
                unsafe_depth: 0,
                arena_depth: 0,
                task_depth: 0,
                task_group_fallible: Vec::new(),
                loop_targets: Vec::new(),
                spawn: None,
                pooled_initializer: None,
            };
            if !self.walk_block(&function.body, context.clone()) {
                return false;
            }
            if !self.bindings_valid(function_index, function) {
                return false;
            }
            let Some(root) = self.blocks.get(&ptr_key(&function.body)) else {
                return false;
            };
            if root.falls && root.ty != function.ret {
                return false;
            }
            if !self.body_ty_ok(function.ret) {
                return false;
            }
        }
        true
    }

    fn walk_block(&mut self, root: &'a hir::Block, context: BodyContext) -> bool {
        let mut work = vec![BodyWork::EnterBlock(root, context)];
        while let Some(item) = work.pop() {
            match item {
                BodyWork::EnterBlock(block, context) => {
                    work.push(BodyWork::ExitBlock(block, context.clone()));
                    if let Some(value) = block.value.as_deref() {
                        work.push(BodyWork::EnterExpr(value, context.clone()));
                    }
                    for statement in block.stmts.iter().rev() {
                        work.push(BodyWork::EnterStmt(statement, context.clone()));
                    }
                }
                BodyWork::ExitBlock(block, context) => {
                    if !self.finish_block(block, &context) {
                        return false;
                    }
                }
                BodyWork::EnterStmt(statement, context) => {
                    if !self.statement_envelope_ok(statement, &context) {
                        return false;
                    }
                    work.push(BodyWork::ExitStmt(statement, context.clone()));
                    let mut child_context = context.clone();
                    child_context.pooled_initializer = match statement {
                        hir::Stmt::Let { local, .. } => Some(*local),
                        _ => None,
                    };
                    let mut children = statement_children(statement);
                    while let Some(child) = children.pop() {
                        work.push(BodyWork::EnterExpr(child, child_context.clone()));
                    }
                }
                BodyWork::ExitStmt(statement, context) => {
                    if !self.finish_statement(statement, &context) {
                        return false;
                    }
                }
                BodyWork::EnterExpr(expression, context) => {
                    if !self.expression_envelope_ok(expression, &context) {
                        return false;
                    }
                    work.push(BodyWork::ExitExpr(expression, context.clone()));
                    self.push_expression_children(expression, &context, &mut work);
                }
                BodyWork::ExitExpr(expression, context) => {
                    if !self.finish_expression(expression, &context) {
                        return false;
                    }
                }
                BodyWork::EnterArm(arm, scrutinee, context) => {
                    let Some(scrutinee_flow) = self.exprs.get(&ptr_key(scrutinee)) else {
                        return false;
                    };
                    if !self.match_arm_envelope(arm, scrutinee_flow.ty, &context) {
                        return false;
                    }
                    work.push(BodyWork::ExitArm(arm));
                    work.push(BodyWork::EnterExpr(&arm.body, context));
                }
                BodyWork::ExitArm(arm) => {
                    let Some(flow) = self.exprs.get(&ptr_key(&arm.body)).cloned() else {
                        return false;
                    };
                    self.arms.insert(ptr_key(arm), flow);
                }
            }
        }
        true
    }

    fn locals_valid(&self, function: &hir::Fn) -> bool {
        function.locals.iter().enumerate().all(|(index, local)| {
            u32::try_from(index).ok() == Some(local.id) && self.body_ty_ok(local.ty)
        })
    }

    fn bindings_valid(&self, function_index: usize, function: &hir::Fn) -> bool {
        let parameters = function.params.iter().copied().collect::<HashSet<_>>();
        if parameters.len() != function.params.len() {
            return false;
        }
        function.locals.iter().all(|local| {
            let count = self
                .binding_counts
                .get(&(function_index, local.id))
                .copied()
                .unwrap_or(0);
            // Whether a local is a signature parameter is an am-h header fact.  The dormant
            // body-core pass must not infer that role from `Local::is_param`: a malformed header
            // must be rejected by the declaration validator before this pass is activated.  Body
            // validation only rejects a body declaration that tries to bind a declared parameter;
            // locals introduced by later body slices may remain unbound until their producer slice
            // supplies the corresponding declaration/initialization record.
            !parameters.contains(&local.id) || count == 0
        })
    }

    fn body_ty_ok(&self, ty: Ty) -> bool {
        match ty {
            Ty::Int(integer) => valid_int(integer.bits),
            Ty::Float(float) => valid_float(float.bits),
            Ty::Bool | Ty::Char | Ty::Str | Ty::String | Ty::Unit | Ty::Raw => true,
            Ty::Option(payload) => self.body_scalar_ok(payload),
            Ty::Result(ok, err) => self.body_scalar_ok(ok) && self.body_scalar_ok(err),
            Ty::Tagged(id) => self
                .program
                .tagged_types
                .get(id as usize)
                .is_some_and(|entry| match *entry {
                    hir::TaggedType::Option(payload) => self.body_scalar_ok(payload),
                    hir::TaggedType::Result(ok, err) => {
                        self.body_scalar_ok(ok) && self.body_scalar_ok(err)
                    }
                }),
            Ty::Box(payload) => {
                self.body_scalar_ok(payload) && self.placement.box_payload_ok(payload)
            }
            Ty::Array(element, len) => {
                len > 0 && !matches!(element, Scalar::Struct(_)) && self.body_scalar_ok(element)
            }
            Ty::Vec(element, lanes) | Ty::Mask(element, lanes) => {
                matches!(lanes, 2 | 4 | 8 | 16)
                    && matches!(element, Scalar::Int(_) | Scalar::Float(_))
                    && self.body_scalar_ok(element)
            }
            Ty::StructArray(id, len) => len > 0 && self.program.structs.get(id as usize).is_some(),
            Ty::DynStructArray(id, Layout::Aos) => self.placement.dynamic_struct_array_ok(id),
            Ty::DynStructArray(_, Layout::Soa) => false,
            Ty::Slice(element) => self.body_scalar_ok(element),
            Ty::DynSliceArray(element) => valid_prim(element),
            Ty::DynArray(element) => {
                !matches!(element, Scalar::Struct(_)) && self.body_scalar_ok(element)
            }
            Ty::DynResponseArray => true,
            Ty::Soa(id) => self.soa_type_ok(id),
            Ty::Struct(id) => self.program.structs.get(id as usize).is_some(),
            Ty::Fn(id) => self.program.fn_types.get(id as usize).is_some(),
            Ty::Enum(id) => self.program.enums.get(id as usize).is_some(),
            Ty::Tuple(id) => self.program.tuples.get(id as usize).is_some(),
            Ty::Task(payload) => primitive_task_scalar(payload) && self.body_scalar_ok(payload),
            Ty::ArenaHandle | Ty::Builder => true,
            Ty::ArrayBuilder(element) => self.body_scalar_ok(element),
            Ty::JsonScanner(id) => self.program.structs.get(id as usize).is_some(),
            Ty::DictEncoded(id, field) => self
                .program
                .structs
                .get(id as usize)
                .and_then(|definition| definition.fields.get(field as usize))
                .is_some_and(|field| field.ty == Ty::Str),
            Ty::Writer
            | Ty::Reader
            | Ty::Buffer
            | Ty::File
            | Ty::Rng
            | Ty::Regex
            | Ty::Captures
            | Ty::CliCommand
            | Ty::CliParsed
            | Ty::TcpConn
            | Ty::TcpListener
            | Ty::UdpSocket
            | Ty::Child
            | Ty::Command
            | Ty::RunOutput
            | Ty::HttpRequest
            | Ty::HttpResponse
            | Ty::HttpClient
            | Ty::HttpServer
            | Ty::HttpRequestCtx
            | Ty::HttpHeaders
            | Ty::ResponseBuilder
            | Ty::HttpStream
            | Ty::JsonDoc => true,
            Ty::Param(_)
            | Ty::IntVar(_)
            | Ty::FloatVar(_)
            | Ty::StrFinder
            | Ty::Error => false,
        }
    }

    fn body_scalar_ok(&self, scalar: Scalar) -> bool {
        match scalar {
            Scalar::Int(integer) => valid_int(integer.bits),
            Scalar::Float(float) => valid_float(float.bits),
            Scalar::Bool | Scalar::Char | Scalar::Unit | Scalar::Str | Scalar::String => true,
            Scalar::Struct(id) => self.program.structs.get(id as usize).is_some(),
            Scalar::Enum(id) => self.program.enums.get(id as usize).is_some(),
            Scalar::Tagged(id) => self.program.tagged_types.get(id as usize).is_some(),
            Scalar::Fn(id) => self.program.fn_types.get(id as usize).is_some(),
            Scalar::DynArray(element) | Scalar::Slice(element) => valid_prim(element),
            Scalar::DynStructArray(id) => self.placement.dynamic_struct_array_ok(id),
            Scalar::Soa(id) => self.soa_type_ok(id),
            Scalar::DynResponseArray
            | Scalar::JsonDoc
            | Scalar::Reader
            | Scalar::Writer
            | Scalar::Buffer
            | Scalar::Regex
            | Scalar::Captures
            | Scalar::File
            | Scalar::CliParsed
            | Scalar::TcpConn
            | Scalar::TcpListener
            | Scalar::UdpSocket
            | Scalar::Child
            | Scalar::HttpResponse
            | Scalar::HttpServer
            | Scalar::HttpRequestCtx
            | Scalar::ResponseBuilder
            | Scalar::HttpStream
            | Scalar::RunOutput => true,
            Scalar::Param(_) => false,
        }
    }

    fn array_literal_element_ok(&self, ty: Ty) -> bool {
        if !self.body_ty_ok(ty) {
            return false;
        }
        match ty {
            Ty::Struct(id) => self.program.structs.get(id as usize).is_some(),
            Ty::Slice(_) => false,
            Ty::Enum(id) => {
                self.program.enums.get(id as usize).is_some()
                    && !align_sema::enum_is_move(
                        id,
                        &self.program.structs,
                        &self.program.enums,
                        &self.program.tagged_types,
                    )
            }
            other if self.array_literal_contains_slice(other) => false,
            other => align_sema::ty_to_scalar(other).is_some_and(|scalar| {
                !matches!(scalar, Scalar::Struct(_))
                    && !scalar.is_move()
                    && self.scalar_copy_ok(scalar)
            }),
        }
    }

    fn array_literal_contains_slice(&self, ty: Ty) -> bool {
        let mut work = vec![ty];
        let mut tagged = HashSet::new();
        let mut tuples = HashSet::new();
        let mut structs = HashSet::new();
        while let Some(ty) = work.pop() {
            match ty {
                Ty::Slice(_) => return true,
                Ty::Option(payload) => work.push(align_sema::scalar_to_ty(payload)),
                Ty::Result(ok, err) => {
                    work.push(align_sema::scalar_to_ty(err));
                    work.push(align_sema::scalar_to_ty(ok));
                }
                Ty::Tagged(id) if tagged.insert(id) => {
                    let Some(entry) = self.program.tagged_types.get(id as usize) else {
                        return true;
                    };
                    match *entry {
                        hir::TaggedType::Option(payload) => {
                            work.push(align_sema::scalar_to_ty(payload));
                        }
                        hir::TaggedType::Result(ok, err) => {
                            work.push(align_sema::scalar_to_ty(err));
                            work.push(align_sema::scalar_to_ty(ok));
                        }
                    }
                }
                Ty::Tuple(id) if tuples.insert(id) => {
                    let Some(tuple) = self.program.tuples.get(id as usize) else {
                        return true;
                    };
                    work.extend(tuple.elems.iter().rev().copied().map(align_sema::scalar_to_ty));
                }
                Ty::Struct(id) if structs.insert(id) => {
                    let Some(definition) = self.program.structs.get(id as usize) else {
                        return true;
                    };
                    work.extend(definition.fields.iter().rev().map(|field| field.ty));
                }
                _ => {}
            }
        }
        false
    }

    fn pooled_array_literal_ok(&self, expression: &hir::Expr, context: &BodyContext) -> bool {
        let Some(local_id) = context.pooled_initializer else {
            return false;
        };
        let Some(function) = self.program.fns.get(context.function) else {
            return false;
        };
        let Some(local) = function.locals.get(local_id as usize) else {
            return false;
        };
        let hir::ExprKind::ArrayLit { elems, elem, .. } = &expression.kind else {
            return false;
        };
        let Ty::Array(scalar, length) = local.ty else {
            return false;
        };
        !local.is_mut
            && !local.is_param
            && local.align.is_none()
            && length >= BODY_CONST_POOL_MIN_ELEMS
            && matches!(
                scalar,
                Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char
            )
            && *elem == align_sema::scalar_to_ty(scalar)
            && expression.ty == local.ty
            && elems.iter().all(pooled_scalar_literal_ok)
    }

    fn primitive_store_ty_ok(&self, ty: Ty) -> bool {
        matches!(
            ty,
            Ty::Int(integer) if valid_int(integer.bits)
        ) || matches!(ty, Ty::Float(float) if valid_float(float.bits))
            || matches!(ty, Ty::Bool | Ty::Char)
    }

    fn soa_scalar_ty_ok(&self, ty: Ty) -> bool {
        self.primitive_store_ty_ok(ty) || ty == Ty::Str
    }

    fn soa_type_ok(&self, id: u32) -> bool {
        self.program.structs.get(id as usize).is_some_and(|definition| {
            !definition.fields.is_empty()
                && definition
                    .fields
                    .iter()
                    .all(|field| self.soa_scalar_ty_ok(field.ty))
        })
    }

    fn struct_array_store_ok(&self, id: u32, soa: bool) -> bool {
        let Some(definition) = self.program.structs.get(id as usize) else {
            return false;
        };
        if definition.fields.is_empty() {
            return false;
        }
        let plain = definition
            .fields
            .iter()
            .all(|field| self.primitive_store_ty_ok(field.ty));
        let str_view = soa
            && definition
                .fields
                .iter()
                .all(|field| self.soa_scalar_ty_ok(field.ty));
        plain
            || str_view
            || (!soa
                && align_sema::struct_is_move(
                    id,
                    &self.program.structs,
                    &self.program.enums,
                    &self.program.tagged_types,
                ))
    }

    fn element_field_store_ok(
        &self,
        base_ty: Ty,
        struct_id: u32,
        path: &[u32],
        soa: bool,
    ) -> bool {
        let Some(leaf) = self.field_path_ty(Some(Ty::Struct(struct_id)), path) else {
            return false;
        };
        if soa {
            return path.len() == 1 && self.soa_scalar_ty_ok(leaf);
        }
        match base_ty {
            Ty::DynStructArray(_, Layout::Aos) => self.primitive_store_ty_ok(leaf),
            Ty::StructArray(_, _) => {
                leaf == Ty::String
                    || !align_sema::drop_plan(
                        leaf,
                        &self.program.structs,
                        &self.program.enums,
                        &self.program.tagged_types,
                    )
                    .needs_drop()
            }
            _ => false,
        }
    }

    fn local_ok(&self, context: &BodyContext, id: hir::LocalId) -> bool {
        self.program
            .fns
            .get(context.function)
            .and_then(|function| function.locals.get(id as usize))
            .is_some_and(|local| local.id == id && self.body_ty_ok(local.ty))
    }

    fn mutable_local_ok(&self, context: &BodyContext, id: hir::LocalId) -> bool {
        self.program
            .fns
            .get(context.function)
            .and_then(|function| function.locals.get(id as usize))
            .is_some_and(|local| local.id == id && local.is_mut && self.body_ty_ok(local.ty))
    }

    fn statement_envelope_ok(&mut self, statement: &hir::Stmt, context: &BodyContext) -> bool {
        let valid = match statement {
            hir::Stmt::Let { local, .. } => self.local_ok(context, *local),
            hir::Stmt::LetTuple { locals, tuple_id, .. } => {
                let Some(tuple) = self.program.tuples.get(*tuple_id as usize) else {
                    return false;
                };
                locals.len() == tuple.elems.len()
                    && locals.iter().flatten().copied().collect::<HashSet<_>>().len()
                        == locals.iter().flatten().count()
                    && locals.iter().flatten().all(|&id| self.local_ok(context, id))
            }
            hir::Stmt::Assign { local, .. } => self.mutable_local_ok(context, *local),
            hir::Stmt::AssignIndex { base, .. } => {
                let Some(function) = self.program.fns.get(context.function) else {
                    return false;
                };
                let Some(local) = function.locals.get(*base as usize) else {
                    return false;
                };
                local.id == *base
                    && local.is_mut
                    && index_element_ty(local.ty).is_some_and(|ty| self.primitive_store_ty_ok(ty))
            }
            hir::Stmt::AssignVecLane { local, lane, .. } => {
                let Some(function) = self.program.fns.get(context.function) else {
                    return false;
                };
                let local_id = *local;
                let Some(local) = function.locals.get(local_id as usize) else {
                    return false;
                };
                local.id == local_id
                    && local.is_mut
                    && matches!(local.ty, Ty::Vec(_, width) if *lane < width)
            }
            hir::Stmt::AssignField { root, path, .. } => {
                self.mutable_local_ok(context, *root) && !path.is_empty()
            }
            hir::Stmt::AssignElemField {
                base,
                path,
                struct_id,
                soa,
                ..
            } => {
                if path.is_empty() || self.program.structs.get(*struct_id as usize).is_none() {
                    return false;
                }
                let Some(function) = self.program.fns.get(context.function) else {
                    return false;
                };
                let Some(local) = function.locals.get(*base as usize) else {
                    return false;
                };
                local.id == *base
                    && local.is_mut
                    && if *soa {
                        local.ty == Ty::Soa(*struct_id)
                    } else {
                        matches!(
                            local.ty,
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, Layout::Aos)
                                if id == *struct_id
                        )
                    }
                    && self.element_field_store_ok(local.ty, *struct_id, path, *soa)
            }
            hir::Stmt::AssignElem {
                base,
                struct_id,
                soa,
                ..
            } => {
                let Some(function) = self.program.fns.get(context.function) else {
                    return false;
                };
                let Some(local) = function.locals.get(*base as usize) else {
                    return false;
                };
                local.id == *base
                    && local.is_mut
                    && self.program.structs.get(*struct_id as usize).is_some()
                    && self.struct_array_store_ok(*struct_id, *soa)
                    && if *soa {
                        local.ty == Ty::Soa(*struct_id)
                    } else {
                        matches!(local.ty, Ty::StructArray(id, _) if id == *struct_id)
                    }
            }
            hir::Stmt::Return(_) | hir::Stmt::Break { .. } | hir::Stmt::Expr(_) => true,
        };
        if !valid {
            return false;
        }
        match statement {
            hir::Stmt::Let { local, .. } => self.record_binding(context.function, *local),
            hir::Stmt::LetTuple { locals, .. } => locals
                .iter()
                .flatten()
                .copied()
                .all(|local| self.record_binding(context.function, local)),
            _ => true,
        }
    }

    fn record_binding(&mut self, function: usize, local: hir::LocalId) -> bool {
        let count = self.binding_counts.entry((function, local)).or_default();
        *count = count.saturating_add(1);
        *count == 1
    }

    fn expression_envelope_ok(&self, expression: &hir::Expr, context: &BodyContext) -> bool {
        let envelope = match &expression.kind {
            hir::ExprKind::Unit
            | hir::ExprKind::Int(_)
            | hir::ExprKind::Float(_)
            | hir::ExprKind::Char(_)
            | hir::ExprKind::Str(_)
            | hir::ExprKind::Bool(_)
            | hir::ExprKind::Local(_)
            | hir::ExprKind::Unary { .. }
            | hir::ExprKind::Cast(_)
            | hir::ExprKind::Binary { .. }
            | hir::ExprKind::IntArith { .. }
            | hir::ExprKind::MathOp { .. }
            | hir::ExprKind::CallFnValue { .. }
            | hir::ExprKind::TaskGroup(_)
            | hir::ExprKind::ResultMapErr { .. }
            | hir::ExprKind::Spawn { .. }
            | hir::ExprKind::TaskGet(_)
            | hir::ExprKind::Wait
            | hir::ExprKind::If { .. }
            | hir::ExprKind::Block(_)
            | hir::ExprKind::OptionSome(_)
            | hir::ExprKind::OptionNone
            | hir::ExprKind::ElseUnwrap { .. }
            | hir::ExprKind::ResultOk(_)
            | hir::ExprKind::ResultErr(_)
            | hir::ExprKind::Try(_)
            | hir::ExprKind::Arena(_)
            | hir::ExprKind::Unsafe(_)
            | hir::ExprKind::RawAlloc(_)
            | hir::ExprKind::RawFree(_)
            | hir::ExprKind::RawLoad { .. }
            | hir::ExprKind::RawStore { .. }
            | hir::ExprKind::RawOffset { .. }
            | hir::ExprKind::HeapNew(_)
            | hir::ExprKind::BoxGet(_)
            | hir::ExprKind::BoxClone(_)
            | hir::ExprKind::StrClone(_)
            | hir::ExprKind::StrPredicate { .. }
            | hir::ExprKind::StrTrim { .. }
            | hir::ExprKind::StrBorrow(_)
            | hir::ExprKind::BuilderNew { .. }
            | hir::ExprKind::BuilderWrite { .. }
            | hir::ExprKind::BuilderToString(_) => true,
            hir::ExprKind::ArrayLit {
                elems,
                elem,
                pooled,
            } => {
                !elems.is_empty()
                    && u32::try_from(elems.len()).is_ok()
                    && self.body_ty_ok(*elem)
                    && (!*pooled || self.pooled_array_literal_ok(expression, context))
            }
            hir::ExprKind::ConstArray { elems, elem, len } => {
                u32::try_from(elems.len()).ok() == Some(*len)
                    && const_array_scalar_ok(*elem)
            }
            hir::ExprKind::ArrayZip { sources, tuple_id } => sources.len() >= 2
                && self
                    .program
                    .tuples
                    .get(*tuple_id as usize)
                    .is_some_and(|tuple| tuple.elems.len() == sources.len()),
            hir::ExprKind::Select { .. }
            | hir::ExprKind::VecSumWhere { .. }
            | hir::ExprKind::VecDot { .. }
            | hir::ExprKind::VecMinMax { .. }
            | hir::ExprKind::VecSum { .. } => true,
            hir::ExprKind::VecLoad { elem, n, .. }
            | hir::ExprKind::VecStore { elem, n, .. } => {
                valid_vector_scalar(*elem) && valid_vector_lanes(*n)
            }
            hir::ExprKind::VecLit { elems, elem } => {
                valid_vector_scalar(*elem)
                    && u32::try_from(elems.len())
                        .ok()
                        .is_some_and(valid_vector_lanes)
            }
            hir::ExprKind::ArraySum { stages, .. }
            | hir::ExprKind::ArrayCount { stages, .. }
            | hir::ExprKind::ArrayMinMax { stages, .. }
            | hir::ExprKind::ArrayReduce { stages, .. }
            | hir::ExprKind::ArrayScan { stages, .. }
            | hir::ExprKind::ArraySort { stages, .. }
            | hir::ExprKind::ArraySortBy { stages, .. }
            | hir::ExprKind::ArrayToArray { stages, .. }
            | hir::ExprKind::ArrayMapInto { stages, .. }
            | hir::ExprKind::ArrayPartition { stages, .. }
            | hir::ExprKind::ArrayParMap { stages, .. } => {
                self.pipeline_stages_envelope_ok(stages)
            }
            hir::ExprKind::ArrayAnyAll { stages, func, .. } => {
                valid_declaration_name(func) && self.pipeline_stages_envelope_ok(stages)
            }
            hir::ExprKind::ArrayDot { elem, .. } => self.body_ty_ok(*elem),
            hir::ExprKind::ArrayToSoa { struct_id, .. } => {
                self.program.structs.get(*struct_id as usize).is_some()
            }
            hir::ExprKind::ArrayChunks { elem, .. } => self.body_ty_ok(*elem),
            hir::ExprKind::ArrayToSlice(_)
            | hir::ExprKind::Len(_)
            | hir::ExprKind::Index { .. }
            | hir::ExprKind::SliceRange { .. } => true,
            hir::ExprKind::ElemField {
                path, struct_id, ..
            } => {
                !path.is_empty() && self.program.structs.get(*struct_id as usize).is_some()
            }
            hir::ExprKind::Template(parts) => {
                !parts.is_empty() && self.template_parts_envelope_ok(parts)
            }
            hir::ExprKind::JsonDecode { struct_id, .. }
            | hir::ExprKind::JsonDecodeStructArray { struct_id, .. }
            | hir::ExprKind::JsonScan { struct_id, .. } => {
                self.json_struct_descriptor_ok(*struct_id, false)
            }
            hir::ExprKind::JsonDecodeArray { elem, .. }
            | hir::ExprKind::JsonDecodeScalar { scalar: elem, .. } => {
                self.json_scalar_target_ok(*elem)
            }
            hir::ExprKind::JsonDecodeSoa { struct_id, .. } => {
                self.json_soa_struct_ok(*struct_id)
            }
            hir::ExprKind::JsonDecodeUnion { enum_id, .. } => {
                self.json_union_descriptor_ok(*enum_id, false)
            }
            hir::ExprKind::JsonDoc { .. }
            | hir::ExprKind::JsonDocKind { .. }
            | hir::ExprKind::JsonDocGet { .. }
            | hir::ExprKind::JsonDocAt { .. }
            | hir::ExprKind::JsonDocAsStr { .. }
            | hir::ExprKind::JsonDocLen { .. }
            | hir::ExprKind::JsonDocKey { .. }
            | hir::ExprKind::JsonDocElems { .. } => true,
            hir::ExprKind::JsonDocAsScalar { scalar, .. } => self.json_doc_scalar_ok(*scalar),
            hir::ExprKind::ArrayGroupAgg {
                base,
                struct_id,
                key_field,
                value_field,
                ..
            } => self.group_aggregate_envelope_ok(
                context,
                *base,
                *struct_id,
                *key_field,
                *value_field,
            ),
            hir::ExprKind::ArrayGroupAggMulti {
                base,
                struct_id,
                key_field,
                aggs,
                ..
            } => {
                !aggs.is_empty()
                    && self.group_aggregate_envelope_ok(
                        context,
                        *base,
                        *struct_id,
                        *key_field,
                        None,
                    )
                    && aggs.iter().all(|agg| self.group_aggregate_part_envelope_ok(agg))
            }
            hir::ExprKind::ArrayDictEncode {
                base,
                struct_id,
                key_field,
            } => self.dictionary_envelope_ok(context, *base, *struct_id, *key_field),
            hir::ExprKind::FnValue(name) => valid_declaration_name(name),
            hir::ExprKind::Closure { lifted, .. } => valid_declaration_name(lifted),
            hir::ExprKind::EnumValue {
                enum_id,
                variant,
                payload,
            } => self
                .program
                .enums
                .get(*enum_id as usize)
                .and_then(|definition| definition.variants.get(*variant as usize))
                .is_some_and(|definition| definition.payload.len() == payload.len()),
            hir::ExprKind::Match { arms, .. } => !arms.is_empty(),
            hir::ExprKind::Call {
                func,
                type_args,
                ..
            } => {
                valid_declaration_name(func)
                    && type_args.iter().all(|ty| self.body_ty_ok(*ty))
            }
            hir::ExprKind::StructLit { struct_id, fields } => self
                .program
                .structs
                .get(*struct_id as usize)
                .is_some_and(|definition| definition.fields.len() == fields.len()),
            hir::ExprKind::Field { root, path } => {
                self.local_ok(context, *root) && !path.is_empty()
            }
            hir::ExprKind::SoaColumn {
                base,
                struct_id,
                field,
            } => {
                self.local_ok(context, *base)
                    && self
                        .program
                        .structs
                        .get(*struct_id as usize)
                        .and_then(|definition| definition.fields.get(*field as usize))
                        .is_some()
            }
            hir::ExprKind::Tuple { tuple_id, elems } => self
                .program
                .tuples
                .get(*tuple_id as usize)
                .is_some_and(|tuple| tuple.elems.len() == elems.len()),
            hir::ExprKind::TupleIndex { .. } => true,
            hir::ExprKind::IndexField { base, path, .. } => {
                self.local_ok(context, *base) && !path.is_empty()
            }
            hir::ExprKind::Loop { body_locals, .. } => {
                let Some(function) = self.program.fns.get(context.function) else {
                    return false;
                };
                let Some(end) = usize::try_from(body_locals.end).ok() else {
                    return false;
                };
                let hir::ExprKind::Loop { body, .. } = &expression.kind else {
                    return false;
                };
                body_locals.start <= body_locals.end
                    && end <= function.locals.len()
                    && self.loop_body_locals_valid(body, body_locals, context)
            }
            _ => false,
        };
        envelope && valid_span(expression.span)
    }

    fn template_parts_envelope_ok(&self, parts: &[hir::TemplatePart]) -> bool {
        parts.iter().all(|part| match part {
            hir::TemplatePart::Text(_) | hir::TemplatePart::Hole(_) | hir::TemplatePart::JsonStr(_) => true,
            hir::TemplatePart::OptionField { name, .. } => valid_declaration_name(name),
            hir::TemplatePart::OptionStructField {
                name, struct_id, ..
            } => {
                valid_declaration_name(name)
                    && self.program.structs.get(*struct_id as usize).is_some()
                    && self.json_struct_descriptor_ok(*struct_id, true)
            }
            hir::TemplatePart::PopComma => true,
            hir::TemplatePart::StructArrayField { struct_id, .. } => {
                self.program.structs.get(*struct_id as usize).is_some()
                    && self.json_struct_descriptor_ok(*struct_id, true)
            }
            hir::TemplatePart::ScalarArrayField { elem, .. } => self.json_array_element_ok(*elem),
            hir::TemplatePart::UnionValue { enum_id, .. } => {
                self.program.enums.get(*enum_id as usize).is_some()
                    && self.json_union_descriptor_ok(*enum_id, true)
            }
        })
    }

    fn json_scalar_target_ok(&self, ty: Ty) -> bool {
        match ty {
            Ty::Int(integer) => valid_int(integer.bits),
            Ty::Float(float) => valid_float(float.bits),
            Ty::Bool => true,
            _ => false,
        }
    }

    fn json_doc_scalar_ok(&self, ty: Ty) -> bool {
        matches!(
            ty,
            Ty::Int(align_sema::IntTy {
                bits: 64,
                signed: true,
            }) | Ty::Float(align_sema::FloatTy { bits: 64 })
                | Ty::Bool
        )
    }

    fn json_array_element_ok(&self, scalar: Scalar) -> bool {
        match scalar {
            Scalar::Int(integer) => valid_int(integer.bits),
            Scalar::Float(float) => valid_float(float.bits),
            Scalar::Bool | Scalar::Str => true,
            _ => false,
        }
    }

    fn json_soa_struct_ok(&self, id: u32) -> bool {
        self.program.structs.get(id as usize).is_some_and(|definition| {
            !definition.fields.is_empty()
                && definition.fields.iter().all(|field| match field.ty {
                    Ty::Int(integer) => valid_int(integer.bits),
                    Ty::Float(float) => valid_float(float.bits),
                    Ty::Bool | Ty::Char | Ty::Str => true,
                    _ => false,
                })
        })
    }

    fn json_struct_descriptor_ok(&self, id: u32, encode: bool) -> bool {
        self.json_shape_ok(BodyJsonRoot::Struct(id), encode)
    }

    fn json_union_descriptor_ok(&self, id: u32, encode: bool) -> bool {
        self.json_shape_ok(BodyJsonRoot::Enum(id), encode)
    }

    fn json_shape_ok(&self, root: BodyJsonRoot, encode: bool) -> bool {
        let mut work = vec![BodyJsonWork::Start(root)];
        let mut values = Vec::new();
        let mut active_structs = HashSet::new();
        let mut active_enums = HashSet::new();
        while let Some(item) = work.pop() {
            match item {
                BodyJsonWork::Start(BodyJsonRoot::Struct(id)) => {
                    if self.program.structs.get(id as usize).is_none() {
                        values.push(false);
                        continue;
                    }
                    if !active_structs.insert(id) {
                        values.push(false);
                    } else {
                        work.push(BodyJsonWork::Struct { id, next: 0 });
                    }
                }
                BodyJsonWork::Start(BodyJsonRoot::Enum(id)) => {
                    let Some(definition) = self.program.enums.get(id as usize) else {
                        values.push(false);
                        continue;
                    };
                    if definition.variants.is_empty() || !active_enums.insert(id) {
                        values.push(false);
                    } else {
                        work.push(BodyJsonWork::Union(BodyJsonUnionFrame {
                            id,
                            next: 0,
                            class_seen: [false; align_sema::JSON_SHAPE_CLASSES],
                            ok: true,
                        }));
                    }
                }
                BodyJsonWork::Struct { id, next } => {
                    let Some(field) = self
                        .program
                        .structs
                        .get(id as usize)
                        .and_then(|definition| definition.fields.get(next))
                    else {
                        active_structs.remove(&id);
                        values.push(true);
                        continue;
                    };
                    let child = match field.ty {
                        Ty::Int(integer) => valid_int(integer.bits).then_some(None),
                        Ty::Float(float) => valid_float(float.bits).then_some(None),
                        Ty::Bool | Ty::Str => Some(None),
                        Ty::Struct(child) => Some(Some(BodyJsonRoot::Struct(child))),
                        Ty::Enum(child) => Some(Some(BodyJsonRoot::Enum(child))),
                        Ty::DynStructArray(child, Layout::Aos) => {
                            Some(Some(BodyJsonRoot::Struct(child)))
                        }
                        Ty::Option(payload) => match payload {
                            Scalar::Int(integer) => valid_int(integer.bits).then_some(None),
                            Scalar::Float(float) => valid_float(float.bits).then_some(None),
                            Scalar::Bool | Scalar::Str => Some(None),
                            Scalar::Struct(child) => Some(Some(BodyJsonRoot::Struct(child))),
                            Scalar::Enum(child) if encode => {
                                Some(Some(BodyJsonRoot::Enum(child)))
                            }
                            _ => None,
                        },
                        Ty::DynArray(element) => self.json_array_element_ok(element).then_some(None),
                        _ => None,
                    };
                    let Some(child) = child else {
                        active_structs.remove(&id);
                        values.push(false);
                        continue;
                    };
                    let next = next.saturating_add(1);
                    if let Some(child) = child {
                        work.push(BodyJsonWork::AfterStruct { id, next });
                        work.push(BodyJsonWork::Start(child));
                    } else {
                        work.push(BodyJsonWork::Struct { id, next });
                    }
                }
                BodyJsonWork::AfterStruct { id, next } => {
                    let Some(child_ok) = values.pop() else {
                        return false;
                    };
                    if child_ok {
                        work.push(BodyJsonWork::Struct { id, next });
                    } else {
                        active_structs.remove(&id);
                        values.push(false);
                    }
                }
                BodyJsonWork::Union(mut frame) => {
                    let Some(variant) = self
                        .program
                        .enums
                        .get(frame.id as usize)
                        .and_then(|definition| definition.variants.get(frame.next))
                    else {
                        active_enums.remove(&frame.id);
                        values.push(frame.ok);
                        continue;
                    };
                    frame.next = frame.next.saturating_add(1);
                    let Some(&payload) = variant.payload.first() else {
                        frame.ok = false;
                        work.push(BodyJsonWork::Union(frame));
                        continue;
                    };
                    if variant.payload.len() != 1
                        || !match payload {
                            Scalar::Int(integer) => valid_int(integer.bits),
                            Scalar::Float(float) => valid_float(float.bits),
                            Scalar::Bool | Scalar::Str => true,
                            Scalar::Struct(id) | Scalar::DynStructArray(id) => {
                                self.program.structs.get(id as usize).is_some()
                            }
                            _ => false,
                        }
                    {
                        frame.ok = false;
                        work.push(BodyJsonWork::Union(frame));
                        continue;
                    }
                    let Some(class) = align_sema::union_shape_class(payload).map(usize::from)
                    else {
                        frame.ok = false;
                        work.push(BodyJsonWork::Union(frame));
                        continue;
                    };
                    let Some(seen) = frame.class_seen.get_mut(class) else {
                        frame.ok = false;
                        work.push(BodyJsonWork::Union(frame));
                        continue;
                    };
                    if *seen {
                        frame.ok = false;
                        work.push(BodyJsonWork::Union(frame));
                        continue;
                    }
                    *seen = true;
                    let child = match payload {
                        Scalar::Struct(id) | Scalar::DynStructArray(id) => {
                            Some(BodyJsonRoot::Struct(id))
                        }
                        _ => None,
                    };
                    if let Some(child) = child {
                        work.push(BodyJsonWork::AfterUnion(frame));
                        work.push(BodyJsonWork::Start(child));
                    } else {
                        work.push(BodyJsonWork::Union(frame));
                    }
                }
                BodyJsonWork::AfterUnion(mut frame) => {
                    let Some(child_ok) = values.pop() else {
                        return false;
                    };
                    if !child_ok {
                        frame.ok = false;
                    }
                    work.push(BodyJsonWork::Union(frame));
                }
            }
        }
        match values.as_slice() {
            [value] => *value,
            _ => false,
        }
    }

    fn group_aggregate_envelope_ok(
        &self,
        context: &BodyContext,
        base: hir::LocalId,
        struct_id: u32,
        key_field: u32,
        value_field: Option<u32>,
    ) -> bool {
        self.local_ok(context, base)
            && self.program.structs.get(struct_id as usize).is_some_and(|definition| {
                definition.fields.get(key_field as usize).is_some()
                    && value_field.is_none_or(|field| definition.fields.get(field as usize).is_some())
            })
    }

    fn group_aggregate_part_envelope_ok(&self, aggregate: &hir::GroupAgg1) -> bool {
        matches!(
            aggregate.op,
            hir::GroupOp::Sum
                | hir::GroupOp::Min
                | hir::GroupOp::Max
                | hir::GroupOp::Count
        )
    }

    fn dictionary_envelope_ok(
        &self,
        context: &BodyContext,
        base: hir::LocalId,
        struct_id: u32,
        key_field: u32,
    ) -> bool {
        self.local_ok(context, base)
            && self
                .program
                .structs
                .get(struct_id as usize)
                .is_some_and(|definition| definition.fields.get(key_field as usize).is_some())
    }

    fn loop_body_locals_valid(
        &self,
        root: &'a hir::Block,
        range: &std::ops::Range<hir::LocalId>,
        context: &BodyContext,
    ) -> bool {
        let mut work = vec![BodyWork::EnterBlock(root, context.clone())];
        let mut actual = HashMap::<hir::LocalId, usize>::new();
        while let Some(item) = work.pop() {
            match item {
                BodyWork::EnterBlock(block, context) => {
                    if let Some(value) = block.value.as_deref() {
                        work.push(BodyWork::EnterExpr(value, context.clone()));
                    }
                    for statement in block.stmts.iter().rev() {
                        work.push(BodyWork::EnterStmt(statement, context.clone()));
                    }
                }
                BodyWork::EnterStmt(statement, context) => {
                    match statement {
                        hir::Stmt::Let { local, .. } => {
                            *actual.entry(*local).or_default() += 1;
                        }
                        hir::Stmt::LetTuple { locals, .. } => {
                            for local in locals.iter().flatten() {
                                *actual.entry(*local).or_default() += 1;
                            }
                        }
                        _ => {}
                    }
                    let mut children = statement_children(statement);
                    while let Some(child) = children.pop() {
                        work.push(BodyWork::EnterExpr(child, context.clone()));
                    }
                }
                BodyWork::EnterExpr(expression, context) => {
                    self.push_expression_children(expression, &context, &mut work);
                }
                BodyWork::EnterArm(arm, _, arm_context) => {
                    for local in &arm.bindings {
                        *actual.entry(*local).or_default() += 1;
                    }
                    work.push(BodyWork::EnterExpr(
                        &arm.body,
                        arm_context,
                    ));
                }
                BodyWork::ExitBlock(..)
                | BodyWork::ExitStmt(..)
                | BodyWork::ExitExpr(..)
                | BodyWork::ExitArm(..) => {}
            }
        }
        let expected = range.clone().collect::<Vec<_>>();
        let mut actual_ids = actual.keys().copied().collect::<Vec<_>>();
        actual_ids.sort_unstable();
        actual_ids == expected && actual.values().all(|count| *count == 1)
    }

    /// Recompute the producer's lexical task-group fallibility without descending into a nested
    /// group.  The HIR carries the `Spawn::fallible` discriminator but not a separate group
    /// record; the checker accumulates the same bit on the innermost active group while walking
    /// its block.  This explicit worklist keeps malformed deep bodies from consuming the Rust
    /// call stack and deliberately visits retained dead syntax as the producer does.
    fn task_group_is_fallible(&self, root: &'a hir::Block) -> bool {
        let context = BodyContext {
            function: 0,
            unsafe_depth: 0,
            arena_depth: 0,
            task_depth: 0,
            task_group_fallible: Vec::new(),
            loop_targets: Vec::new(),
            spawn: None,
            pooled_initializer: None,
        };
        let mut work = vec![BodyWork::EnterBlock(root, context)];
        while let Some(item) = work.pop() {
            match item {
                BodyWork::EnterBlock(block, context) => {
                    if let Some(value) = block.value.as_deref() {
                        work.push(BodyWork::EnterExpr(value, context.clone()));
                    }
                    for statement in block.stmts.iter().rev() {
                        work.push(BodyWork::EnterStmt(statement, context.clone()));
                    }
                }
                BodyWork::EnterStmt(statement, context) => {
                    let mut children = statement_children(statement);
                    while let Some(child) = children.pop() {
                        work.push(BodyWork::EnterExpr(child, context.clone()));
                    }
                }
                BodyWork::EnterExpr(expression, context) => {
                    if let hir::ExprKind::TaskGroup(_) = &expression.kind {
                        continue;
                    }
                    if let hir::ExprKind::Spawn { fallible: true, .. } = &expression.kind {
                        return true;
                    }
                    self.push_expression_children(expression, &context, &mut work);
                }
                BodyWork::EnterArm(arm, _, context) => {
                    work.push(BodyWork::EnterExpr(&arm.body, context));
                }
                BodyWork::ExitBlock(..)
                | BodyWork::ExitStmt(..)
                | BodyWork::ExitExpr(..)
                | BodyWork::ExitArm(..) => {}
            }
        }
        false
    }

    fn push_expression_children(
        &self,
        expression: &'a hir::Expr,
        context: &BodyContext,
        work: &mut Vec<BodyWork<'a>>,
    ) {
        macro_rules! push_expr {
            ($child:expr, $child_context:expr) => {{
                let mut child_context = $child_context;
                child_context.pooled_initializer = None;
                work.push(BodyWork::EnterExpr($child, child_context));
            }};
        }
        match &expression.kind {
            hir::ExprKind::Unary { expr, .. }
            | hir::ExprKind::Cast(expr)
            | hir::ExprKind::TaskGet(expr)
            | hir::ExprKind::OptionSome(expr)
            | hir::ExprKind::ResultOk(expr)
            | hir::ExprKind::ResultErr(expr)
            | hir::ExprKind::Try(expr)
            | hir::ExprKind::RawAlloc(expr)
            | hir::ExprKind::RawFree(expr)
            | hir::ExprKind::HeapNew(expr)
            | hir::ExprKind::BoxGet(expr)
            | hir::ExprKind::BoxClone(expr)
            | hir::ExprKind::StrClone(expr)
            | hir::ExprKind::StrTrim { recv: expr, .. }
            | hir::ExprKind::StrBorrow(expr)
            | hir::ExprKind::BuilderToString(expr) => {
                push_expr!(expr, context.clone());
            }
            hir::ExprKind::Binary { lhs, rhs, .. }
            | hir::ExprKind::IntArith { lhs, rhs, .. } => {
                push_expr!(rhs, context.clone());
                push_expr!(lhs, context.clone());
            }
            hir::ExprKind::MathOp { operands, .. }
            | hir::ExprKind::Closure { captures: operands, .. }
            | hir::ExprKind::EnumValue { payload: operands, .. }
            | hir::ExprKind::StructLit { fields: operands, .. }
            | hir::ExprKind::Tuple { elems: operands, .. } => {
                let child_context = if matches!(&expression.kind, hir::ExprKind::Closure { .. }) {
                    let mut child = context.clone();
                    child.spawn = None;
                    child
                } else {
                    context.clone()
                };
                for operand in operands.iter().rev() {
                    push_expr!(operand, child_context.clone());
                }
            }
            hir::ExprKind::CallFnValue { callee, args } => {
                for arg in args.iter().rev() {
                    push_expr!(arg, context.clone());
                }
                push_expr!(callee, context.clone());
            }
            hir::ExprKind::TaskGroup(block) => {
                let mut child = context.clone();
                child.task_depth = child.task_depth.saturating_add(1);
                child
                    .task_group_fallible
                    .push(self.task_group_is_fallible(block));
                child.pooled_initializer = None;
                work.push(BodyWork::EnterBlock(block, child));
            }
            hir::ExprKind::Match { scrutinee, arms } => {
                for arm in arms.iter().rev() {
                    let mut arm_context = context.clone();
                    arm_context.pooled_initializer = None;
                    work.push(BodyWork::EnterArm(arm, scrutinee, arm_context));
                }
                push_expr!(scrutinee, context.clone());
            }
            hir::ExprKind::ResultMapErr { result, f } => {
                push_expr!(f, context.clone());
                push_expr!(result, context.clone());
            }
            hir::ExprKind::Spawn { closure, .. } => {
                let mut child = context.clone();
                child.spawn = Some(SpawnContext {
                    fallible: matches!(&expression.kind, hir::ExprKind::Spawn { fallible: true, .. }),
                    ok: match expression.ty {
                        Ty::Task(scalar) => scalar,
                        _ => Scalar::Unit,
                    },
                });
                push_expr!(closure, child);
            }
            hir::ExprKind::Call { args, .. } => {
                for arg in args.iter().rev() {
                    push_expr!(arg, context.clone());
                }
            }
            hir::ExprKind::If { cond, then, els } => {
                let mut else_context = context.clone();
                else_context.pooled_initializer = None;
                work.push(BodyWork::EnterBlock(els, else_context));
                let mut then_context = context.clone();
                then_context.pooled_initializer = None;
                work.push(BodyWork::EnterBlock(then, then_context));
                push_expr!(cond, context.clone());
            }
            hir::ExprKind::TupleIndex { recv, .. } => push_expr!(recv, context.clone()),
            hir::ExprKind::Block(block)
            | hir::ExprKind::Arena(block)
            | hir::ExprKind::Unsafe(block) => {
                let mut child = context.clone();
                child.pooled_initializer = None;
                if matches!(&expression.kind, hir::ExprKind::Arena(_)) {
                    child.arena_depth = child.arena_depth.saturating_add(1);
                }
                if matches!(&expression.kind, hir::ExprKind::Unsafe(_)) {
                    child.unsafe_depth = child.unsafe_depth.saturating_add(1);
                }
                work.push(BodyWork::EnterBlock(block, child));
            }
            hir::ExprKind::Loop { body, .. } => {
                let mut child = context.clone();
                child.pooled_initializer = None;
                child.loop_targets.push(expression.ty);
                work.push(BodyWork::EnterBlock(body, child));
            }
            hir::ExprKind::ElseUnwrap { opt, fallback } => {
                push_expr!(fallback, context.clone());
                push_expr!(opt, context.clone());
            }
            hir::ExprKind::RawLoad { ptr, offset, .. } => {
                push_expr!(offset, context.clone());
                push_expr!(ptr, context.clone());
            }
            hir::ExprKind::RawOffset { ptr, offset } => {
                push_expr!(offset, context.clone());
                push_expr!(ptr, context.clone());
            }
            hir::ExprKind::RawStore { ptr, offset, value } => {
                push_expr!(value, context.clone());
                push_expr!(offset, context.clone());
                push_expr!(ptr, context.clone());
            }
            hir::ExprKind::StrPredicate {
                haystack, needle, ..
            } => {
                push_expr!(needle, context.clone());
                push_expr!(haystack, context.clone());
            }
            hir::ExprKind::BuilderNew {
                capacity: Some(capacity),
            } => push_expr!(capacity, context.clone()),
            hir::ExprKind::BuilderNew { capacity: None } => {}
            hir::ExprKind::BuilderWrite { builder, arg, .. } => {
                push_expr!(arg, context.clone());
                push_expr!(builder, context.clone());
            }
            hir::ExprKind::ArrayLit { elems, .. }
            | hir::ExprKind::ConstArray { elems, .. }
            | hir::ExprKind::VecLit { elems, .. } => {
                for element in elems.iter().rev() {
                    push_expr!(element, context.clone());
                }
            }
            hir::ExprKind::ArrayZip { sources, .. } => {
                for source in sources.iter().rev() {
                    push_expr!(source, context.clone());
                }
            }
            hir::ExprKind::Select { mask, a, b } => {
                push_expr!(b, context.clone());
                push_expr!(a, context.clone());
                push_expr!(mask, context.clone());
            }
            hir::ExprKind::VecSumWhere { vec, mask } => {
                push_expr!(mask, context.clone());
                push_expr!(vec, context.clone());
            }
            hir::ExprKind::VecDot { a, b } => {
                push_expr!(b, context.clone());
                push_expr!(a, context.clone());
            }
            hir::ExprKind::VecMinMax { vec, .. } | hir::ExprKind::VecSum { vec } => {
                push_expr!(vec, context.clone());
            }
            hir::ExprKind::VecLoad { src, index, .. } => {
                push_expr!(index, context.clone());
                push_expr!(src, context.clone());
            }
            hir::ExprKind::VecStore {
                dst,
                index,
                value,
                ..
            } => {
                push_expr!(value, context.clone());
                push_expr!(index, context.clone());
                push_expr!(dst, context.clone());
            }
            hir::ExprKind::ArraySum { source, stages }
            | hir::ExprKind::ArrayCount { source, stages }
            | hir::ExprKind::ArrayMinMax { source, stages, .. }
            | hir::ExprKind::ArraySort { source, stages, .. }
            | hir::ExprKind::ArrayToArray { source, stages, .. }
            | hir::ExprKind::ArrayParMap { source, stages, .. }
            | hir::ExprKind::ArrayPartition { source, stages, .. } => {
                self.push_pipeline_children(source, stages, &[], context, work);
            }
            hir::ExprKind::ArrayAnyAll {
                source,
                stages,
                captures,
                ..
            } => {
                self.push_pipeline_children(source, stages, captures, context, work);
            }
            hir::ExprKind::ArrayReduce {
                source,
                stages,
                captures,
                init,
                ..
            }
            | hir::ExprKind::ArrayScan {
                source,
                stages,
                captures,
                init,
                ..
            } => {
                self.push_pipeline_children_with_tail(
                    source,
                    stages,
                    &[init.as_ref()],
                    captures,
                    context,
                    work,
                );
            }
            hir::ExprKind::ArraySortBy {
                source,
                stages,
                captures,
                ..
            } => {
                self.push_pipeline_children(source, stages, captures, context, work);
            }
            hir::ExprKind::ArrayMapInto {
                source,
                stages,
                dst,
                ..
            } => {
                self.push_pipeline_children_with_tail(
                    source,
                    stages,
                    &[dst.as_ref()],
                    &[],
                    context,
                    work,
                );
            }
            hir::ExprKind::ArrayToSoa { source, .. } => {
                push_expr!(source, context.clone());
            }
            hir::ExprKind::ArrayDot { a, b, .. } => {
                push_expr!(b, context.clone());
                push_expr!(a, context.clone());
            }
            hir::ExprKind::ArrayChunks { source, n, .. } => {
                push_expr!(n, context.clone());
                push_expr!(source, context.clone());
            }
            hir::ExprKind::ArrayToSlice(source) | hir::ExprKind::Len(source) => {
                push_expr!(source, context.clone());
            }
            hir::ExprKind::Index { recv, index } => {
                push_expr!(index, context.clone());
                push_expr!(recv, context.clone());
            }
            hir::ExprKind::SliceRange { recv, start, end } => {
                if let Some(end) = end {
                    push_expr!(end, context.clone());
                }
                if let Some(start) = start {
                    push_expr!(start, context.clone());
                }
                push_expr!(recv, context.clone());
            }
            hir::ExprKind::ElemField { recv, index, .. } => {
                push_expr!(index, context.clone());
                push_expr!(recv, context.clone());
            }
            hir::ExprKind::Template(parts) => {
                for part in parts.iter().rev() {
                    match part {
                        hir::TemplatePart::Hole(expr)
                        | hir::TemplatePart::JsonStr(expr) => {
                            push_expr!(expr, context.clone());
                        }
                        hir::TemplatePart::OptionField { access, .. }
                        | hir::TemplatePart::OptionStructField { access, .. }
                        | hir::TemplatePart::StructArrayField { access, .. }
                        | hir::TemplatePart::ScalarArrayField { access, .. }
                        | hir::TemplatePart::UnionValue { access, .. } => {
                            push_expr!(access, context.clone());
                        }
                        hir::TemplatePart::Text(_) | hir::TemplatePart::PopComma => {}
                    }
                }
            }
            hir::ExprKind::JsonDecode { input, .. }
            | hir::ExprKind::JsonDecodeArray { input, .. }
            | hir::ExprKind::JsonDecodeScalar { input, .. }
            | hir::ExprKind::JsonDecodeStructArray { input, .. }
            | hir::ExprKind::JsonDecodeSoa { input, .. }
            | hir::ExprKind::JsonDecodeUnion { input, .. }
            | hir::ExprKind::JsonScan { input, .. }
            | hir::ExprKind::JsonDoc { input } => push_expr!(input, context.clone()),
            hir::ExprKind::JsonDocKind { doc }
            | hir::ExprKind::JsonDocAsStr { doc }
            | hir::ExprKind::JsonDocAsScalar { doc, .. }
            | hir::ExprKind::JsonDocLen { doc }
            | hir::ExprKind::JsonDocElems { doc } => push_expr!(doc, context.clone()),
            hir::ExprKind::JsonDocGet { doc, key } | hir::ExprKind::JsonDocAt { doc, index: key } | hir::ExprKind::JsonDocKey { doc, index: key } => {
                push_expr!(key, context.clone());
                push_expr!(doc, context.clone());
            }
            hir::ExprKind::ArrayGroupAgg { .. }
            | hir::ExprKind::ArrayGroupAggMulti { .. }
            | hir::ExprKind::ArrayDictEncode { .. } => {}
            hir::ExprKind::Unit
            | hir::ExprKind::Int(_)
            | hir::ExprKind::Float(_)
            | hir::ExprKind::Char(_)
            | hir::ExprKind::Str(_)
            | hir::ExprKind::Bool(_)
            | hir::ExprKind::Local(_)
            | hir::ExprKind::FnValue(_)
            | hir::ExprKind::Wait
            | hir::ExprKind::OptionNone
            | hir::ExprKind::SoaColumn { .. }
            | hir::ExprKind::Field { .. }
            | hir::ExprKind::IndexField { .. }
            => {}
            _ => {}
        }
    }

    fn push_pipeline_children(
        &self,
        source: &'a hir::Expr,
        stages: &'a [hir::Stage],
        terminal_captures: &'a [hir::Expr],
        context: &BodyContext,
        work: &mut Vec<BodyWork<'a>>,
    ) {
        self.push_pipeline_children_with_tail(
            source,
            stages,
            &[],
            terminal_captures,
            context,
            work,
        );
    }

    fn push_pipeline_children_with_tail(
        &self,
        source: &'a hir::Expr,
        stages: &'a [hir::Stage],
        terminal_args: &[&'a hir::Expr],
        terminal_captures: &'a [hir::Expr],
        context: &BodyContext,
        work: &mut Vec<BodyWork<'a>>,
    ) {
        macro_rules! push_expr {
            ($child:expr) => {{
                let mut child_context = context.clone();
                child_context.pooled_initializer = None;
                work.push(BodyWork::EnterExpr($child, child_context));
            }};
        }
        // The worklist is LIFO. Push terminal captures first, then stage children in reverse, and
        // source last so the observed order is source → stages → terminal operands/captures.
        for capture in terminal_captures.iter().rev() {
            push_expr!(capture);
        }
        for argument in terminal_args.iter().rev() {
            push_expr!(*argument);
        }
        for stage in stages.iter().rev() {
            match &stage.kind {
                hir::StageKind::Map { captures, .. }
                | hir::StageKind::Where { captures, .. } => {
                    for capture in captures.iter().rev() {
                        push_expr!(capture);
                    }
                }
                hir::StageKind::WhereStrContains { needle } => push_expr!(needle),
                hir::StageKind::WhereField { .. } | hir::StageKind::Project { .. } => {}
            }
        }
        push_expr!(source);
    }

    fn finish_expression(&mut self, expression: &hir::Expr, context: &BodyContext) -> bool {
        let Some((derived, falls, breaks)) = self.derive_expression(expression, context) else {
            return false;
        };
        let stored_type_matches = expression.ty == derived
            || context_polymorphic_expression(&expression.kind, falls);
        if !self.body_ty_ok(expression.ty) || !stored_type_matches {
            return false;
        }
        self.exprs.insert(
            ptr_key(expression),
            BodyFlow {
                ty: expression.ty,
                falls,
                breaks,
            },
        );
        true
    }

    fn derive_expression(
        &self,
        expression: &hir::Expr,
        context: &BodyContext,
    ) -> Option<(Ty, bool, Vec<Ty>)> {
        let kind = &expression.kind;
        match kind {
            hir::ExprKind::Unit => Some((Ty::Unit, true, Vec::new())),
            hir::ExprKind::Int(value) => {
                let Ty::Int(integer) = expression.ty else {
                    return None;
                };
                let (min, max) = int_range(integer)?;
                (*value >= min && *value <= max).then_some((expression.ty, true, Vec::new()))
            }
            hir::ExprKind::Float(_) => {
                matches!(expression.ty, Ty::Float(float) if valid_float(float.bits))
                    .then_some((expression.ty, true, Vec::new()))
            }
            hir::ExprKind::Char(value) => {
                (*value <= 0x10ffff && !(*value >= 0xd800 && *value <= 0xdfff))
                    .then_some((Ty::Char, true, Vec::new()))
            }
            hir::ExprKind::Str(_) => Some((Ty::Str, true, Vec::new())),
            hir::ExprKind::Bool(_) => Some((Ty::Bool, true, Vec::new())),
            hir::ExprKind::Local(id) => Some((self.local_type(context, *id)?, true, Vec::new())),
            hir::ExprKind::Unary { op, expr } => {
                let child = self.expr_flow(expr)?;
                let result = unary_result(*op, child.ty)?;
                Some((result, child.falls, child.breaks))
            }
            hir::ExprKind::Cast(inner) => {
                let child = self.expr_flow(inner)?;
                cast_result(child.ty, expression.ty).map(|_| (expression.ty, child.falls, child.breaks))
            }
            hir::ExprKind::Binary { op, lhs, rhs } => {
                let left = self.expr_flow(lhs)?;
                let right = self.expr_flow(rhs)?;
                let result = binary_result(*op, left.ty, right.ty)?;
                let mut breaks = left.breaks.clone();
                if left.falls {
                    breaks.extend(right.breaks.clone());
                }
                let falls = if matches!(op, align_ast::BinOp::And | align_ast::BinOp::Or) {
                    left.falls
                } else {
                    left.falls && right.falls
                };
                Some((result, falls, breaks))
            }
            hir::ExprKind::IntArith { op, mode, lhs, rhs } => {
                if !matches!(op, align_ast::BinOp::Add | align_ast::BinOp::Sub | align_ast::BinOp::Mul) {
                    return None;
                }
                let left = self.expr_flow(lhs)?;
                let right = self.expr_flow(rhs)?;
                let Ty::Int(integer) = left.ty else { return None };
                if left.ty != right.ty {
                    return None;
                }
                let result = match mode {
                    hir::ArithMode::Saturating => Ty::Int(integer),
                    hir::ArithMode::Checked => Ty::Option(Scalar::Int(integer)),
                };
                let (falls, breaks) = strict_flow(&[left, right]);
                Some((result, falls, breaks))
            }
            hir::ExprKind::MathOp { fn_, operands } => {
                let flows = self.expr_flows(operands)?;
                let result = math_result(*fn_, &flows)?;
                let (falls, breaks) = strict_flow(&flows);
                Some((result, falls, breaks))
            }
            hir::ExprKind::FnValue(name) => {
                let sig = self.resolve_signature(name)?;
                if sig.is_extern {
                    return None;
                }
                let Ty::Fn(fid) = expression.ty else { return None };
                if !self.fn_value_matches(fid, &sig, context.spawn) {
                    return None;
                }
                Some((Ty::Fn(fid), true, Vec::new()))
            }
            hir::ExprKind::Closure { lifted, captures } => {
                let sig = self.resolve_signature(lifted)?;
                let Ty::Fn(fid) = expression.ty else { return None };
                if !self.closure_matches(fid, &sig, captures, context.spawn, context) {
                    return None;
                }
                let flows = self.expr_flows(captures)?;
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::Fn(fid), falls, breaks))
            }
            hir::ExprKind::CallFnValue { callee, args } => {
                let callee_flow = self.expr_flow(callee)?;
                let Ty::Fn(fid) = callee_flow.ty else { return None };
                let function = self.program.fn_types.get(fid as usize)?;
                if function.params.len() != args.len()
                    || function.params.iter().any(|(mode, _)| {
                        matches!(mode, align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut)
                    })
                {
                    return None;
                }
                let arg_flows = self.expr_flows(args)?;
                for (index, ((mode, scalar), arg)) in function.params.iter().zip(&arg_flows).enumerate() {
                    let expected = align_sema::scalar_to_ty(*scalar);
                    if arg.ty != expected {
                        return None;
                    }
                    if *mode == align_ast::ParamMode::Out
                        && !self.out_arg_is_writable(context, args, index)
                    {
                        return None;
                    }
                }
                let mut all = vec![callee_flow];
                all.extend(arg_flows);
                let (falls, breaks) = strict_flow(&all);
                Some((function.ret, falls, breaks))
            }
            hir::ExprKind::TaskGroup(block) => {
                let flow = self.block_flow(block)?;
                Some((flow.ty, flow.falls, flow.breaks))
            }
            hir::ExprKind::EnumValue {
                enum_id,
                variant,
                payload,
            } => {
                let definition = self.program.enums.get(*enum_id as usize)?;
                let variant = definition.variants.get(*variant as usize)?;
                let flows = self.expr_flows(payload)?;
                if flows
                    .iter()
                    .zip(&variant.payload)
                    .any(|(flow, expected)| flow.ty != align_sema::scalar_to_ty(*expected))
                {
                    return None;
                }
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::Enum(*enum_id), falls, breaks))
            }
            hir::ExprKind::Match { scrutinee, arms } => {
                let scrutinee_flow = self.expr_flow(scrutinee)?;
                let payloads = self.sum_payloads(scrutinee_flow.ty)?;
                if arms.is_empty() {
                    return None;
                }
                let mut covered = HashSet::new();
                let mut wildcard = false;
                let mut result = None;
                let mut breaks = scrutinee_flow.breaks.clone();
                let mut any_arm_falls = false;
                for arm in arms {
                    if arm.variants.is_empty() {
                        if wildcard {
                            return None;
                        }
                        wildcard = true;
                    } else {
                        for &variant in &arm.variants {
                            if !covered.insert(variant) {
                                return None;
                            }
                        }
                    }
                    let flow = self.arms.get(&ptr_key(arm))?;
                    if scrutinee_flow.falls {
                        breaks.extend(flow.breaks.clone());
                    }
                    if scrutinee_flow.falls && flow.falls {
                        any_arm_falls = true;
                        if result.is_some_and(|ty| ty != flow.ty) {
                            return None;
                        }
                        result = Some(flow.ty);
                    }
                }
                let covered_all = wildcard || covered.len() == payloads.len();
                if !covered_all {
                    return None;
                }
                if !scrutinee_flow.falls || !any_arm_falls {
                    Some((expression.ty, false, breaks))
                } else {
                    Some((result?, true, breaks))
                }
            }
            hir::ExprKind::ResultMapErr { result, f } => {
                let result_flow = self.expr_flow(result)?;
                let function_flow = self.expr_flow(f)?;
                let Ty::Result(ok, err) = result_flow.ty else { return None };
                let Ty::Fn(fid) = function_flow.ty else { return None };
                let function = self.program.fn_types.get(fid as usize)?;
                let [(mode, parameter)] = function.params.as_slice() else { return None };
                if *mode != align_ast::ParamMode::ByValue
                    || *parameter != err
                    || !self.body_scalar_ok(align_sema::ty_to_scalar(function.ret)?)
                {
                    return None;
                }
                let error = align_sema::ty_to_scalar(function.ret)?;
                let (falls, breaks) = strict_flow(&[result_flow, function_flow]);
                Some((Ty::Result(ok, error), falls, breaks))
            }
            hir::ExprKind::Spawn { closure, fallible } => {
                if context.task_depth == 0 {
                    return None;
                }
                let closure_flow = self.expr_flow(closure)?;
                let Ty::Task(ok) = expression.ty else { return None };
                if !primitive_task_scalar(ok) || !matches!(closure_flow.ty, Ty::Fn(_)) {
                    return None;
                }
                let Ty::Fn(fid) = closure_flow.ty else { return None };
                let function = self.program.fn_types.get(fid as usize)?;
                if !function.params.is_empty() || function.ret != align_sema::scalar_to_ty(ok) {
                    return None;
                }
                let sig = self.resolve_lifted_signature(closure)?;
                let expected = if *fallible {
                    Ty::Result(ok, Scalar::Enum(self.error_id()?))
                } else {
                    align_sema::scalar_to_ty(ok)
                };
                if sig.ret != expected {
                    return None;
                }
                Some((Ty::Task(ok), closure_flow.falls, closure_flow.breaks))
            }
            hir::ExprKind::TaskGet(task) => {
                let flow = self.expr_flow(task)?;
                let Ty::Task(scalar) = flow.ty else { return None };
                if !primitive_task_scalar(scalar) {
                    return None;
                }
                Some((align_sema::scalar_to_ty(scalar), flow.falls, flow.breaks))
            }
            hir::ExprKind::Wait => {
                let &fallible = context.task_group_fallible.last()?;
                if context.task_depth == 0 || !wait_type_ok(self.program, expression.ty, fallible) {
                    return None;
                }
                Some((expression.ty, true, Vec::new()))
            }
            hir::ExprKind::Call {
                func,
                args,
                type_args,
            } => {
                let sig = self.resolve_signature(func)?;
                if sig.is_extern && context.unsafe_depth == 0 {
                    return None;
                }
                if type_args.is_empty() {
                    if matches!(sig.origin, Some(hir::FnOrigin::Monomorph)) {
                        return None;
                    }
                } else {
                    if !matches!(sig.origin, Some(hir::FnOrigin::Monomorph))
                        || !self.mangled_call_name_matches(func, type_args, &sig)
                    {
                        return None;
                    }
                }
                if sig.params.len() != args.len() || sig.modes.len() != args.len() {
                    return None;
                }
                let arg_flows = self.expr_flows(args)?;
                for (index, ((mode, expected), actual)) in sig
                    .modes
                    .iter()
                    .zip(&sig.params)
                    .zip(&arg_flows)
                    .enumerate()
                {
                    if actual.ty != *expected {
                        return None;
                    }
                    if matches!(mode, align_ast::ParamMode::Out | align_ast::ParamMode::BorrowMut)
                        && !self.out_arg_is_writable(context, args, index)
                    {
                        return None;
                    }
                }
                let all = arg_flows;
                let (falls, breaks) = strict_flow(&all);
                Some((sig.ret, falls, breaks))
            }
            hir::ExprKind::If { cond, then, els } => {
                let condition = self.expr_flow(cond)?;
                if condition.ty != Ty::Bool {
                    return None;
                }
                let then_flow = self.block_flow(then)?;
                let else_flow = self.block_flow(els)?;
                let mut breaks = condition.breaks.clone();
                if condition.falls {
                    breaks.extend(then_flow.breaks.clone());
                    breaks.extend(else_flow.breaks.clone());
                }
                let result = if !condition.falls {
                    expression.ty
                } else if then_flow.falls && else_flow.falls {
                    if then_flow.ty != else_flow.ty {
                        return None;
                    }
                    then_flow.ty
                } else if then_flow.falls {
                    then_flow.ty
                } else if else_flow.falls {
                    else_flow.ty
                } else {
                    expression.ty
                };
                Some((result, condition.falls && (then_flow.falls || else_flow.falls), breaks))
            }
            hir::ExprKind::StructLit { struct_id, fields } => {
                let definition = self.program.structs.get(*struct_id as usize)?;
                let flows = self.expr_flows(fields)?;
                if flows
                    .iter()
                    .zip(&definition.fields)
                    .any(|(flow, field)| flow.ty != field.ty)
                {
                    return None;
                }
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::Struct(*struct_id), falls, breaks))
            }
            hir::ExprKind::Field { root, path } => {
                let leaf = self.field_path_ty(self.local_type(context, *root), path)?;
                Some((leaf, true, Vec::new()))
            }
            hir::ExprKind::SoaColumn {
                base,
                struct_id,
                field,
            } => {
                if self.local_type(context, *base)? != Ty::Soa(*struct_id) {
                    return None;
                }
                let field_ty = self
                    .program
                    .structs
                    .get(*struct_id as usize)?
                    .fields
                    .get(*field as usize)
                    .map(|field| field.ty)?;
                let scalar = align_sema::ty_to_scalar(field_ty)?;
                if !matches!(scalar, Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char | Scalar::Str) {
                    return None;
                }
                Some((Ty::Slice(scalar), true, Vec::new()))
            }
            hir::ExprKind::Tuple { tuple_id, elems } => {
                let tuple = self.program.tuples.get(*tuple_id as usize)?;
                let flows = self.expr_flows(elems)?;
                if flows
                    .iter()
                    .zip(&tuple.elems)
                    .any(|(flow, expected)| flow.ty != align_sema::scalar_to_ty(*expected))
                {
                    return None;
                }
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::Tuple(*tuple_id), falls, breaks))
            }
            hir::ExprKind::ArrayLit {
                elems,
                elem,
                pooled: _,
            } => {
                let flows = self.expr_flows(elems)?;
                let length = u32::try_from(elems.len()).ok()?;
                if elems.is_empty()
                    || flows.iter().any(|flow| flow.ty != *elem)
                    || !self.array_literal_element_ok(*elem)
                {
                    return None;
                }
                let (falls, breaks) = strict_flow(&flows);
                match *elem {
                    Ty::Struct(id) => Some((Ty::StructArray(id, length), falls, breaks)),
                    element_ty => {
                        let scalar = align_sema::ty_to_scalar(element_ty)?;
                        if matches!(scalar, Scalar::Struct(_)) {
                            return None;
                        }
                        Some((Ty::Array(scalar, length), falls, breaks))
                    }
                }
            }
            hir::ExprKind::ConstArray { elems, elem, len } => {
                if !const_array_scalar_ok(*elem)
                    || u32::try_from(elems.len()).ok() != Some(*len)
                {
                    return None;
                }
                let expected = align_sema::scalar_to_ty(*elem);
                let flows = self.expr_flows(elems)?;
                if elems.iter().zip(&flows).any(|(child, flow)| {
                    flow.ty != expected
                        || !matches!(
                            child.kind,
                            hir::ExprKind::Int(_)
                                | hir::ExprKind::Float(_)
                                | hir::ExprKind::Bool(_)
                                | hir::ExprKind::Char(_)
                                | hir::ExprKind::Str(_)
                        )
                }) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::Slice(*elem), falls, breaks))
            }
            hir::ExprKind::ArrayZip { sources, tuple_id } => {
                let tuple = self.program.tuples.get(*tuple_id as usize)?;
                if sources.len() < 2 || tuple.elems.len() != sources.len() {
                    return None;
                }
                let flows = self.expr_flows(sources)?;
                let mut fixed_len = None;
                for ((source, flow), expected) in sources.iter().zip(&flows).zip(&tuple.elems) {
                    if !matches!(
                        source.kind,
                        hir::ExprKind::Local(_)
                            | hir::ExprKind::ArrayLit { .. }
                            | hir::ExprKind::SliceRange { .. }
                    ) {
                        return None;
                    }
                    let (scalar, length) = match flow.ty {
                        Ty::Array(scalar, length) => (scalar, Some(length)),
                        Ty::DynArray(scalar) | Ty::Slice(scalar) => (scalar, None),
                        _ => return None,
                    };
                    if !array_zip_scalar_ok(scalar)
                        || scalar != *expected
                        || !self.scalar_copy_ok(scalar)
                    {
                        return None;
                    }
                    if let Some(length) = length {
                        if fixed_len.is_some_and(|known| known != length) {
                            return None;
                        }
                        fixed_len = Some(length);
                    }
                }
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::Tuple(*tuple_id), falls, breaks))
            }
            hir::ExprKind::Select { mask, a, b } => {
                let mask = self.expr_flow(mask)?;
                let a = self.expr_flow(a)?;
                let b = self.expr_flow(b)?;
                let (scalar, lanes) = vector_numeric(a.ty)?;
                if mask.ty != Ty::Mask(scalar, lanes) || b.ty != Ty::Vec(scalar, lanes) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[mask, a, b]);
                Some((Ty::Vec(scalar, lanes), falls, breaks))
            }
            hir::ExprKind::VecSumWhere { vec, mask } => {
                let vector = self.expr_flow(vec)?;
                let mask = self.expr_flow(mask)?;
                let (scalar, lanes) = vector_numeric(vector.ty)?;
                if mask.ty != Ty::Mask(scalar, lanes) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[vector, mask]);
                Some((align_sema::scalar_to_ty(scalar), falls, breaks))
            }
            hir::ExprKind::VecDot { a, b } => {
                let a = self.expr_flow(a)?;
                let b = self.expr_flow(b)?;
                let (scalar, lanes) = vector_numeric(a.ty)?;
                if b.ty != Ty::Vec(scalar, lanes) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[a, b]);
                Some((align_sema::scalar_to_ty(scalar), falls, breaks))
            }
            hir::ExprKind::VecMinMax { vec, .. } | hir::ExprKind::VecSum { vec } => {
                let vector = self.expr_flow(vec)?;
                let (scalar, _) = vector_numeric(vector.ty)?;
                Some((
                    align_sema::scalar_to_ty(scalar),
                    vector.falls,
                    vector.breaks,
                ))
            }
            hir::ExprKind::VecLoad {
                src,
                index,
                elem,
                n,
            } => {
                if !valid_vector_scalar(*elem) || !valid_vector_lanes(*n) {
                    return None;
                }
                let src = self.expr_flow(src)?;
                let index = self.expr_flow(index)?;
                if src.ty != Ty::Slice(*elem) || index.ty != i64_ty() {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[src, index]);
                Some((Ty::Vec(*elem, *n), falls, breaks))
            }
            hir::ExprKind::VecStore {
                dst,
                index,
                value,
                elem,
                n,
            } => {
                if !valid_vector_scalar(*elem) || !valid_vector_lanes(*n) {
                    return None;
                }
                let dst_flow = self.expr_flow(dst)?;
                let index_flow = self.expr_flow(index)?;
                let value_flow = self.expr_flow(value)?;
                if dst_flow.ty != Ty::Slice(*elem)
                    || index_flow.ty != i64_ty()
                    || value_flow.ty != Ty::Vec(*elem, *n)
                    || !self.out_arg_is_writable(context, std::slice::from_ref(dst), 0)
                {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[dst_flow, index_flow, value_flow]);
                Some((Ty::Unit, falls, breaks))
            }
            hir::ExprKind::VecLit { elems, elem } => {
                let lanes = u32::try_from(elems.len()).ok()?;
                if !valid_vector_scalar(*elem) || !valid_vector_lanes(lanes) {
                    return None;
                }
                let expected = align_sema::scalar_to_ty(*elem);
                let flows = self.expr_flows(elems)?;
                if flows.iter().any(|flow| flow.ty != expected) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::Vec(*elem, lanes), falls, breaks))
            }
            hir::ExprKind::TupleIndex { recv, index } => {
                let flow = self.expr_flow(recv)?;
                let Ty::Tuple(tuple_id) = flow.ty else { return None };
                let element = *self.program.tuples.get(tuple_id as usize)?.elems.get(*index as usize)?;
                Some((align_sema::scalar_to_ty(element), flow.falls, flow.breaks))
            }
            hir::ExprKind::IndexField { base, index, path } => {
                let Ty::StructArray(struct_id, length) = self.local_type(context, *base)? else {
                    return None;
                };
                if *index >= length {
                    return None;
                }
                let leaf = self.field_path_ty(Some(Ty::Struct(struct_id)), path)?;
                Some((leaf, true, Vec::new()))
            }
            hir::ExprKind::Block(block) => {
                let flow = self.block_flow(block)?;
                Some((flow.ty, flow.falls, flow.breaks))
            }
            hir::ExprKind::OptionSome(value) => {
                let flow = self.expr_flow(value)?;
                let scalar = align_sema::ty_to_scalar(flow.ty)?;
                Some((Ty::Option(scalar), flow.falls, flow.breaks))
            }
            hir::ExprKind::OptionNone => {
                if let Ty::Option(payload) = expression.ty {
                    self.body_scalar_ok(payload).then_some((expression.ty, true, Vec::new()))
                } else {
                    None
                }
            }
            hir::ExprKind::ElseUnwrap { opt, fallback } => {
                let option = self.expr_flow(opt)?;
                let fallback = self.expr_flow(fallback)?;
                let payload = match option.ty {
                    Ty::Option(payload) => payload,
                    Ty::Result(ok, _) => ok,
                    _ => return None,
                };
                if fallback.falls && fallback.ty != align_sema::scalar_to_ty(payload) {
                    return None;
                }
                let mut breaks = option.breaks.clone();
                if option.falls {
                    breaks.extend(fallback.breaks.clone());
                }
                Some((align_sema::scalar_to_ty(payload), option.falls, breaks))
            }
            hir::ExprKind::ResultOk(value) => {
                let flow = self.expr_flow(value)?;
                let scalar = align_sema::ty_to_scalar(flow.ty)?;
                let Ty::Result(_, error) = expression.ty else {
                    return None;
                };
                if !self.body_scalar_ok(error) {
                    return None;
                }
                Some((Ty::Result(scalar, error), flow.falls, flow.breaks))
            }
            hir::ExprKind::ResultErr(value) => {
                let flow = self.expr_flow(value)?;
                let scalar = align_sema::ty_to_scalar(flow.ty)?;
                let Ty::Result(ok, error) = expression.ty else { return None };
                if error != scalar || !self.body_scalar_ok(ok) {
                    return None;
                }
                Some((Ty::Result(ok, scalar), flow.falls, flow.breaks))
            }
            hir::ExprKind::Try(value) => {
                let flow = self.expr_flow(value)?;
                let Ty::Result(ok, error) = flow.ty else { return None };
                let Ty::Result(_, enclosing_error) = self.program.fns.get(context.function)?.ret else {
                    return None;
                };
                if error != enclosing_error {
                    return None;
                }
                Some((align_sema::scalar_to_ty(ok), flow.falls, flow.breaks))
            }
            hir::ExprKind::Loop {
                body,
                diverges,
                ..
            } => {
                let flow = self.block_flow(body)?;
                if *diverges != flow.breaks.is_empty() {
                    return None;
                }
                if !flow.breaks.iter().all(|ty| *ty == expression.ty) {
                    return None;
                }
                Some((expression.ty, !*diverges, Vec::new()))
            }
            hir::ExprKind::Arena(block) => {
                let flow = self.block_flow(block)?;
                Some((flow.ty, flow.falls, flow.breaks))
            }
            hir::ExprKind::Unsafe(block) => {
                let flow = self.block_flow(block)?;
                Some((flow.ty, flow.falls, flow.breaks))
            }
            hir::ExprKind::RawAlloc(size) => {
                let flow = self.expr_flow(size)?;
                (context.unsafe_depth > 0 && flow.ty == i64_ty())
                    .then_some((Ty::Raw, flow.falls, flow.breaks))
            }
            hir::ExprKind::RawFree(ptr) => {
                let flow = self.expr_flow(ptr)?;
                (context.unsafe_depth > 0 && flow.ty == Ty::Raw)
                    .then_some((Ty::Unit, flow.falls, flow.breaks))
            }
            hir::ExprKind::RawLoad { ptr, offset, scalar } => {
                let pointer = self.expr_flow(ptr)?;
                let offset = self.expr_flow(offset)?;
                if context.unsafe_depth == 0 || pointer.ty != Ty::Raw || offset.ty != i64_ty() || !self.raw_scalar_ok(*scalar) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[pointer, offset]);
                Some((align_sema::scalar_to_ty(*scalar), falls, breaks))
            }
            hir::ExprKind::RawStore { ptr, offset, value } => {
                let pointer = self.expr_flow(ptr)?;
                let offset = self.expr_flow(offset)?;
                let value = self.expr_flow(value)?;
                if context.unsafe_depth == 0
                    || pointer.ty != Ty::Raw
                    || offset.ty != i64_ty()
                    || !self.raw_store_ty_ok(value.ty)
                {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[pointer, offset, value]);
                Some((Ty::Unit, falls, breaks))
            }
            hir::ExprKind::RawOffset { ptr, offset } => {
                let pointer = self.expr_flow(ptr)?;
                let offset = self.expr_flow(offset)?;
                if context.unsafe_depth == 0 || pointer.ty != Ty::Raw || offset.ty != i64_ty() {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[pointer, offset]);
                Some((Ty::Raw, falls, breaks))
            }
            hir::ExprKind::HeapNew(value) => {
                let flow = self.expr_flow(value)?;
                if context.arena_depth == 0 {
                    return None;
                }
                let scalar = align_sema::ty_to_scalar(flow.ty)?;
                if matches!(flow.ty, Ty::Slice(_)) || !self.placement.box_payload_ok(scalar) {
                    return None;
                }
                Some((Ty::Box(scalar), flow.falls, flow.breaks))
            }
            hir::ExprKind::BoxGet(value) => {
                let flow = self.expr_flow(value)?;
                let Ty::Box(scalar) = flow.ty else { return None };
                if !self.scalar_copy_ok(scalar) {
                    return None;
                }
                Some((align_sema::scalar_to_ty(scalar), flow.falls, flow.breaks))
            }
            hir::ExprKind::BoxClone(value) => {
                let flow = self.expr_flow(value)?;
                let Ty::Box(scalar) = flow.ty else { return None };
                if context.arena_depth == 0 || !self.scalar_copy_ok(scalar) {
                    return None;
                }
                Some((Ty::Box(scalar), flow.falls, flow.breaks))
            }
            hir::ExprKind::StrClone(value) => {
                let flow = self.expr_flow(value)?;
                (flow.ty == Ty::Str).then_some((Ty::String, flow.falls, flow.breaks))
            }
            hir::ExprKind::StrPredicate { kind, haystack, needle } => {
                let left = self.expr_flow(haystack)?;
                let right = self.expr_flow(needle)?;
                if left.ty != Ty::Str || right.ty != Ty::Str {
                    return None;
                }
                let result = match kind {
                    hir::StrPredKind::Find | hir::StrPredKind::Rfind => Ty::Option(Scalar::Int(align_sema::IntTy { bits: 64, signed: true })),
                    hir::StrPredKind::Contains
                    | hir::StrPredKind::StartsWith
                    | hir::StrPredKind::EndsWith
                    | hir::StrPredKind::EqIgnoreCase => Ty::Bool,
                };
                let (falls, breaks) = strict_flow(&[left, right]);
                Some((result, falls, breaks))
            }
            hir::ExprKind::StrTrim { recv, .. } => {
                let flow = self.expr_flow(recv)?;
                (flow.ty == Ty::Str).then_some((Ty::Str, flow.falls, flow.breaks))
            }
            hir::ExprKind::StrBorrow(value) => {
                let flow = self.expr_flow(value)?;
                (flow.ty == Ty::String).then_some((Ty::Str, flow.falls, flow.breaks))
            }
            hir::ExprKind::BuilderNew { capacity } => {
                if let Some(capacity) = capacity {
                    let flow = self.expr_flow(capacity)?;
                    (flow.ty == i64_ty()).then_some((Ty::Builder, flow.falls, flow.breaks))
                } else {
                    Some((Ty::Builder, true, Vec::new()))
                }
            }
            hir::ExprKind::BuilderWrite { builder, arg, kind } => {
                let builder = self.expr_flow(builder)?;
                let arg = self.expr_flow(arg)?;
                if builder.ty != Ty::Builder || !builder_write_ty_ok(*kind, arg.ty) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[builder, arg]);
                Some((Ty::Unit, falls, breaks))
            }
            hir::ExprKind::BuilderToString(builder) => {
                let flow = self.expr_flow(builder)?;
                (flow.ty == Ty::Builder).then_some((Ty::String, flow.falls, flow.breaks))
            }
            _ => self.derive_pipeline_expression(expression, context),
        }
    }

    fn pipeline_stages_envelope_ok(&self, stages: &[hir::Stage]) -> bool {
        stages.iter().all(|stage| {
            self.body_ty_ok(stage.out_ty)
                && match &stage.kind {
                    hir::StageKind::Map { func, .. }
                    | hir::StageKind::Where { func, .. } => valid_declaration_name(func),
                    hir::StageKind::WhereField { .. }
                    | hir::StageKind::WhereStrContains { .. }
                    | hir::StageKind::Project { .. } => true,
                }
        })
    }

    fn pipeline_source_element(
        &self,
        source: &hir::Expr,
        source_ty: Ty,
    ) -> Option<Ty> {
        match source_ty {
            Ty::Array(scalar, _)
            | Ty::Slice(scalar)
            | Ty::DynArray(scalar) => Some(align_sema::scalar_to_ty(scalar)),
            Ty::StructArray(id, _) | Ty::DynStructArray(id, Layout::Aos) | Ty::Soa(id) => {
                Some(Ty::Struct(id))
            }
            Ty::DynSliceArray(primitive) => {
                Some(Ty::Slice(align_sema::prim_to_scalar(primitive)))
            }
            Ty::JsonScanner(id) => Some(Ty::Struct(id)),
            Ty::Tuple(id) if matches!(source.kind, hir::ExprKind::ArrayZip { .. }) => {
                self.program.tuples.get(id as usize).and_then(|tuple| {
                    tuple
                        .elems
                        .iter()
                        .all(|scalar| self.scalar_copy_ok(*scalar))
                        .then_some(source_ty)
                })
            }
            Ty::Tuple(_) => None,
            // A scanner is owned by the later JSON slice. Keeping it out here also prevents the
            // array reducers from taking the Result<T, Error> scanner ABI by accident.
            Ty::DynStructArray(_, Layout::Soa)
            | Ty::DynResponseArray
            | Ty::Unit
            | Ty::Str
            | Ty::String
            | Ty::Struct(_)
            | Ty::Vec(_, _)
            | Ty::Mask(_, _)
            | Ty::Bool
            | Ty::Char
            | Ty::Float(_)
            | Ty::Int(_)
            | Ty::Raw
            | Ty::Option(_)
            | Ty::Result(_, _)
            | Ty::Tagged(_)
            | Ty::Box(_)
            | Ty::Fn(_)
            | Ty::Enum(_)
            | Ty::Task(_)
            | Ty::ArenaHandle
            | Ty::Builder
            | Ty::ArrayBuilder(_)
            | Ty::JsonDoc
            | Ty::DictEncoded(_, _)
            | Ty::Writer
            | Ty::Reader
            | Ty::Buffer
            | Ty::File
            | Ty::Rng
            | Ty::Regex
            | Ty::Captures
            | Ty::CliCommand
            | Ty::CliParsed
            | Ty::TcpConn
            | Ty::TcpListener
            | Ty::UdpSocket
            | Ty::Child
            | Ty::Command
            | Ty::RunOutput
            | Ty::HttpRequest
            | Ty::HttpResponse
            | Ty::HttpClient
            | Ty::HttpServer
            | Ty::HttpRequestCtx
            | Ty::HttpHeaders
            | Ty::ResponseBuilder
            | Ty::HttpStream
            | Ty::Param(_)
            | Ty::IntVar(_)
            | Ty::FloatVar(_)
            | Ty::StrFinder
            | Ty::Error => None,
        }
    }

    fn pipeline_terminal_result(&self, source_ty: Ty, payload: Ty) -> Option<Ty> {
        if matches!(source_ty, Ty::JsonScanner(_)) {
            let error = self.error_id()?;
            Some(Ty::Result(align_sema::ty_to_scalar(payload)?, Scalar::Enum(error)))
        } else {
            Some(payload)
        }
    }

    fn pipeline_callable_ok(
        &self,
        func: &str,
        captures: &[hir::Expr],
        capture_flows: &[BodyFlow],
        input: Ty,
        output: Ty,
        context: &BodyContext,
    ) -> bool {
        let Some(signature) = self.resolve_signature(func) else {
            return false;
        };
        if signature.is_extern && context.unsafe_depth == 0 {
            return false;
        }
        if signature.modes.len() != signature.params.len()
            || signature.modes.iter().any(|mode| *mode != align_ast::ParamMode::ByValue)
            || capture_flows.len() != captures.len()
            || capture_flows.iter().zip(captures).any(|(flow, capture)| {
                flow.ty != capture.ty || !self.ty_copy_ok(flow.ty, context)
            })
            || !self.ty_copy_ok(input, context)
            || !self.ty_copy_ok(output, context)
        {
            return false;
        }
        let mut expected = Vec::with_capacity(1 + captures.len());
        expected.push(input);
        expected.extend(capture_flows.iter().map(|flow| flow.ty));
        if signature.params != expected || signature.ret != output {
            return false;
        }
        match signature.origin {
            Some(hir::FnOrigin::Lifted { capture_count }) => {
                usize::try_from(capture_count).ok() == Some(captures.len())
            }
            Some(hir::FnOrigin::Source { .. }) | Some(hir::FnOrigin::Monomorph) | None => true,
        }
    }

    fn pipeline_prefix(
        &self,
        source: &hir::Expr,
        stages: &[hir::Stage],
        context: &BodyContext,
    ) -> Option<(Ty, Vec<BodyFlow>)> {
        let source_flow = self.expr_flow(source)?;
        let source_ty = source_flow.ty;
        let current_source = self.pipeline_source_element(source, source_ty)?;
        let needs_var = matches!(
            source_ty,
            Ty::Array(..) | Ty::StructArray(..) | Ty::DynStructArray(..) | Ty::Soa(_)
        );
        if needs_var
            && !matches!(
                source.kind,
                hir::ExprKind::Local(_) | hir::ExprKind::ArrayLit { .. }
            )
        {
            return None;
        }
        let slot_backed = matches!(
            source_ty,
            Ty::Array(..)
                | Ty::StructArray(..)
                | Ty::DynStructArray(..)
                | Ty::Soa(_)
                | Ty::JsonScanner(_)
        );
        let mut current = current_source;
        let mut mapped = false;
        let mut flows = vec![source_flow];
        for stage in stages {
            let stage_flows = match &stage.kind {
                hir::StageKind::Map { captures, .. }
                | hir::StageKind::Where { captures, .. } => self.expr_flows(captures)?,
                hir::StageKind::WhereStrContains { needle } => {
                    vec![self.expr_flow(needle)?]
                }
                hir::StageKind::WhereField { .. } | hir::StageKind::Project { .. } => Vec::new(),
            };
            flows.extend(stage_flows.iter().cloned());
            current = self.pipeline_stage_output(
                source_ty,
                current,
                slot_backed,
                &mut mapped,
                stage,
                &stage_flows,
                context,
            )?;
        }
        Some((current, flows))
    }

    #[allow(clippy::too_many_arguments)]
    fn pipeline_stage_output(
        &self,
        source_ty: Ty,
        current: Ty,
        slot_backed: bool,
        mapped: &mut bool,
        stage: &hir::Stage,
        capture_flows: &[BodyFlow],
        context: &BodyContext,
    ) -> Option<Ty> {
        match &stage.kind {
            hir::StageKind::Map { func, captures } => {
                if matches!(source_ty, Ty::Soa(_)) && matches!(current, Ty::Struct(_)) {
                    return None;
                }
                if !self.pipeline_callable_ok(
                    func,
                    captures,
                    capture_flows,
                    current,
                    stage.out_ty,
                    context,
                ) {
                    return None;
                }
                *mapped = true;
                Some(stage.out_ty)
            }
            hir::StageKind::Where { func, captures } => {
                if matches!(source_ty, Ty::Soa(_)) && matches!(current, Ty::Struct(_)) {
                    return None;
                }
                if stage.out_ty != current
                    || !self.pipeline_callable_ok(
                        func,
                        captures,
                        capture_flows,
                        current,
                        Ty::Bool,
                        context,
                    )
                {
                    return None;
                }
                Some(current)
            }
            hir::StageKind::WhereField { field } => {
                let Ty::Struct(id) = current else { return None };
                if *mapped || !slot_backed || stage.out_ty != current {
                    return None;
                }
                let field_ty = self
                    .program
                    .structs
                    .get(id as usize)?
                    .fields
                    .get(*field as usize)?
                    .ty;
                (field_ty == Ty::Bool).then_some(current)
            }
            hir::StageKind::WhereStrContains { needle } => {
                if current != Ty::Str || stage.out_ty != Ty::Str || capture_flows.len() != 1 {
                    return None;
                }
                (capture_flows[0].ty == needle.ty && needle.ty == Ty::Str).then_some(current)
            }
            hir::StageKind::Project { field } => {
                let Ty::Struct(id) = current else { return None };
                if *mapped || !slot_backed {
                    return None;
                }
                let field_ty = self
                    .program
                    .structs
                    .get(id as usize)?
                    .fields
                    .get(*field as usize)?
                    .ty;
                (stage.out_ty == field_ty).then_some(field_ty)
            }
        }
    }

    fn derive_pipeline_expression(
        &self,
        expression: &hir::Expr,
        context: &BodyContext,
    ) -> Option<(Ty, bool, Vec<Ty>)> {
        let kind = &expression.kind;
        match kind {
            hir::ExprKind::ArraySum { source, stages } => {
                let (elem, flows) = self.pipeline_prefix(source, stages, context)?;
                if !numeric_body_ty(elem) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&flows);
                Some((self.pipeline_terminal_result(source.ty, elem)?, falls, breaks))
            }
            hir::ExprKind::ArrayCount { source, stages } => {
                let (_, flows) = self.pipeline_prefix(source, stages, context)?;
                let (falls, breaks) = strict_flow(&flows);
                Some((
                    self.pipeline_terminal_result(source.ty, i64_ty())?,
                    falls,
                    breaks,
                ))
            }
            hir::ExprKind::ArrayAnyAll {
                source,
                stages,
                func,
                captures,
                ..
            } => {
                let (elem, mut flows) = self.pipeline_prefix(source, stages, context)?;
                if !self.ty_copy_ok(elem, context)
                    || align_sema::ty_to_scalar(elem).is_none()
                {
                    return None;
                }
                let capture_flows = self.expr_flows(captures)?;
                if !self.pipeline_callable_ok(
                    func,
                    captures,
                    &capture_flows,
                    elem,
                    Ty::Bool,
                    context,
                ) {
                    return None;
                }
                flows.extend(capture_flows);
                let (falls, breaks) = strict_flow(&flows);
                Some((
                    self.pipeline_terminal_result(source.ty, Ty::Bool)?,
                    falls,
                    breaks,
                ))
            }
            hir::ExprKind::ArrayMinMax { source, stages, .. } => {
                let (elem, flows) = self.pipeline_prefix(source, stages, context)?;
                if !numeric_body_ty(elem) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&flows);
                Some((self.pipeline_terminal_result(source.ty, elem)?, falls, breaks))
            }
            hir::ExprKind::ArrayReduce {
                source,
                stages,
                func,
                captures,
                init,
            } => {
                let (elem, mut flows) = self.pipeline_prefix(source, stages, context)?;
                let init_flow = self.expr_flow(init)?;
                if !self.ty_copy_ok(elem, context) || !self.ty_copy_ok(init_flow.ty, context) {
                    return None;
                }
                let capture_flows = self.expr_flows(captures)?;
                if !self.pipeline_reducer_callable_ok(
                    func,
                    captures,
                    &capture_flows,
                    init_flow.ty,
                    elem,
                    init_flow.ty,
                    context,
                ) {
                    return None;
                }
                flows.push(init_flow);
                flows.extend(capture_flows);
                let (falls, breaks) = strict_flow(&flows);
                Some((
                    self.pipeline_terminal_result(source.ty, init.ty)?,
                    falls,
                    breaks,
                ))
            }
            hir::ExprKind::ArrayScan {
                source,
                stages,
                func,
                captures,
                init,
                elem: output_elem,
            } => {
                let (input_elem, mut flows) = self.pipeline_prefix(source, stages, context)?;
                if matches!(source.ty, Ty::JsonScanner(_)) {
                    return None;
                }
                let init_flow = self.expr_flow(init)?;
                let output_scalar = align_sema::ty_to_scalar(*output_elem)
                    .filter(|scalar| align_sema::scalar_to_prim(*scalar).is_some());
                if init_flow.ty != *output_elem
                    || !self.ty_copy_ok(input_elem, context)
                    || !self.ty_copy_ok(*output_elem, context)
                    || output_scalar.is_none()
                {
                    return None;
                }
                let capture_flows = self.expr_flows(captures)?;
                if !self.pipeline_reducer_callable_ok(
                    func,
                    captures,
                    &capture_flows,
                    *output_elem,
                    input_elem,
                    *output_elem,
                    context,
                ) {
                    return None;
                }
                flows.push(init_flow);
                flows.extend(capture_flows);
                let (falls, breaks) = strict_flow(&flows);
                Some((
                    Ty::DynArray(output_scalar?),
                    falls,
                    breaks,
                ))
            }
            hir::ExprKind::ArrayDot { a, b, elem } => {
                let left = self.expr_flow(a)?;
                let right = self.expr_flow(b)?;
                let (left_scalar, left_len) = fixed_array_shape(a, left.ty)?;
                let (right_scalar, right_len) = fixed_array_shape(b, right.ty)?;
                if left_scalar != right_scalar
                    || left_len != right_len
                    || align_sema::scalar_to_ty(left_scalar) != *elem
                    || !numeric_body_ty(*elem)
                {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[left, right]);
                Some((*elem, falls, breaks))
            }
            hir::ExprKind::ArraySort { source, stages, elem } => {
                let (final_elem, flows) = self.pipeline_prefix(source, stages, context)?;
                if matches!(source.ty, Ty::JsonScanner(_)) {
                    return None;
                }
                let scalar = align_sema::ty_to_scalar(final_elem)?;
                if final_elem != *elem
                    || !numeric_body_ty(final_elem)
                    || !self.scalar_copy_ok(scalar)
                {
                    return None;
                }
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::DynArray(scalar), falls, breaks))
            }
            hir::ExprKind::ArraySortBy {
                source,
                stages,
                key_func,
                captures,
                key_ty,
                elem,
            } => {
                let (final_elem, mut flows) = self.pipeline_prefix(source, stages, context)?;
                if matches!(source.ty, Ty::JsonScanner(_)) {
                    return None;
                }
                let scalar = align_sema::ty_to_scalar(final_elem)?;
                if final_elem != *elem
                    || align_sema::scalar_to_prim(scalar).is_none()
                    || !self.scalar_copy_ok(scalar)
                    || !orderable_body_ty(*key_ty)
                {
                    return None;
                }
                let capture_flows = self.expr_flows(captures)?;
                if !self.pipeline_callable_ok(
                    key_func,
                    captures,
                    &capture_flows,
                    final_elem,
                    *key_ty,
                    context,
                ) {
                    return None;
                }
                flows.extend(capture_flows);
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::DynArray(scalar), falls, breaks))
            }
            hir::ExprKind::ArrayToArray { source, stages, elem } => {
                let (final_elem, flows) = self.pipeline_prefix(source, stages, context)?;
                if matches!(source.ty, Ty::JsonScanner(_)) {
                    return None;
                }
                if final_elem != *elem || !self.ty_copy_ok(final_elem, context) {
                    return None;
                }
                let result = self.array_to_array_result(final_elem)?;
                let (falls, breaks) = strict_flow(&flows);
                Some((result, falls, breaks))
            }
            hir::ExprKind::ArrayToSoa { source, struct_id } => {
                if context.arena_depth == 0 {
                    return None;
                }
                let source_flow = self.expr_flow(source)?;
                if !matches!(
                    source.kind,
                    hir::ExprKind::Local(_) | hir::ExprKind::ArrayLit { .. }
                ) {
                    return None;
                }
                let source_struct_id = match source_flow.ty {
                    Ty::StructArray(id, _)
                    | Ty::DynStructArray(id, Layout::Aos) => Some(id),
                    _ => None,
                };
                if source_struct_id != Some(*struct_id) {
                    return None;
                }
                if !self.soa_struct_ok(*struct_id) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[source_flow]);
                Some((Ty::Soa(*struct_id), falls, breaks))
            }
            hir::ExprKind::ArrayMapInto {
                source,
                stages,
                dst,
                elem,
            } => {
                let (final_elem, mut flows) = self.pipeline_prefix(source, stages, context)?;
                if matches!(source.ty, Ty::JsonScanner(_)) {
                    return None;
                }
                let dst_flow = self.expr_flow(dst)?;
                let scalar = align_sema::ty_to_scalar(final_elem)?;
                let Ty::Slice(dst_scalar) = dst_flow.ty else { return None };
                if final_elem != *elem
                    || scalar != dst_scalar
                    || !self.scalar_copy_ok(scalar)
                    || !stages.iter().all(|stage| {
                        matches!(
                            stage.kind,
                            hir::StageKind::Map { .. } | hir::StageKind::Project { .. }
                        )
                    })
                    || !self.out_arg_is_writable(context, std::slice::from_ref(dst), 0)
                {
                    return None;
                }
                flows.push(dst_flow);
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::Unit, falls, breaks))
            }
            hir::ExprKind::ArrayPartition {
                source,
                stages,
                func,
                captures,
                elem,
            } => {
                let (final_elem, mut flows) = self.pipeline_prefix(source, stages, context)?;
                if matches!(source.ty, Ty::JsonScanner(_)) {
                    return None;
                }
                let scalar = align_sema::ty_to_scalar(final_elem)?;
                let primitive = align_sema::scalar_to_prim(scalar)?;
                if final_elem != *elem || !self.scalar_copy_ok(scalar) {
                    return None;
                }
                let capture_flows = self.expr_flows(captures)?;
                if !self.pipeline_callable_ok(
                    func,
                    captures,
                    &capture_flows,
                    final_elem,
                    Ty::Bool,
                    context,
                ) {
                    return None;
                }
                let tuple = match expression.ty {
                    Ty::Tuple(id) => self.program.tuples.get(id as usize)?,
                    _ => return None,
                };
                let expected = [Scalar::DynArray(primitive), Scalar::DynArray(primitive)];
                if tuple.elems.as_slice() != expected.as_slice() {
                    return None;
                }
                flows.extend(capture_flows);
                let (falls, breaks) = strict_flow(&flows);
                Some((expression.ty, falls, breaks))
            }
            hir::ExprKind::ArrayParMap {
                source,
                stages,
                func,
                captures,
                elem,
            } => {
                let (input_elem, mut flows) = self.pipeline_prefix(source, stages, context)?;
                if matches!(source.ty, Ty::JsonScanner(_)) {
                    return None;
                }
                let output_scalar = align_sema::ty_to_scalar(*elem)?;
                if !matches!(
                    output_scalar,
                    Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char
                ) || !self.scalar_copy_ok(output_scalar)
                {
                    return None;
                }
                let capture_flows = self.expr_flows(captures)?;
                if !self.pipeline_callable_ok(
                    func,
                    captures,
                    &capture_flows,
                    input_elem,
                    *elem,
                    context,
                ) {
                    return None;
                }
                flows.extend(capture_flows);
                let (falls, breaks) = strict_flow(&flows);
                Some((Ty::DynArray(output_scalar), falls, breaks))
            }
            hir::ExprKind::ArrayChunks { source, n, elem } => {
                let source_flow = self.expr_flow(source)?;
                let n_flow = self.expr_flow(n)?;
                if n_flow.ty != i64_ty() {
                    return None;
                }
                let source_scalar = match source_flow.ty {
                    Ty::Array(scalar, _) | Ty::Slice(scalar) | Ty::DynArray(scalar) => scalar,
                    _ => return None,
                };
                let primitive = align_sema::scalar_to_prim(source_scalar)?;
                if !self.scalar_copy_ok(source_scalar)
                    || *elem != align_sema::scalar_to_ty(align_sema::prim_to_scalar(primitive))
                    || (matches!(source_flow.ty, Ty::Array(..))
                        && !matches!(
                            source.kind,
                            hir::ExprKind::Local(_) | hir::ExprKind::ArrayLit { .. }
                        ))
                {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[source_flow, n_flow]);
                Some((Ty::DynSliceArray(primitive), falls, breaks))
            }
            hir::ExprKind::ArrayToSlice(source) => {
                let flow = self.expr_flow(source)?;
                let result = match flow.ty {
                    Ty::Array(scalar, _) => {
                        if !matches!(
                            source.kind,
                            hir::ExprKind::Local(_) | hir::ExprKind::ArrayLit { .. }
                        ) {
                            return None;
                        }
                        Ty::Slice(scalar)
                    }
                    Ty::StructArray(id, _) => {
                        if !matches!(
                            source.kind,
                            hir::ExprKind::Local(_) | hir::ExprKind::ArrayLit { .. }
                        ) {
                            return None;
                        }
                        Ty::Slice(Scalar::Struct(id))
                    }
                    Ty::DynArray(scalar) => Ty::Slice(scalar),
                    Ty::DynStructArray(id, Layout::Aos) => Ty::Slice(Scalar::Struct(id)),
                    _ => return None,
                };
                let (falls, breaks) = strict_flow(&[flow]);
                Some((result, falls, breaks))
            }
            hir::ExprKind::Len(source) => {
                let flow = self.expr_flow(source)?;
                if !matches!(
                    flow.ty,
                    Ty::Str
                        | Ty::String
                        | Ty::Slice(_)
                        | Ty::DynArray(_)
                        | Ty::DynStructArray(_, _)
                        | Ty::DynSliceArray(_)
                        | Ty::DynResponseArray
                        | Ty::Soa(_)
                ) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[flow]);
                Some((i64_ty(), falls, breaks))
            }
            hir::ExprKind::Index { recv, index } => {
                let receiver = self.expr_flow(recv)?;
                let index_flow = self.expr_flow(index)?;
                if index_flow.ty != i64_ty() {
                    return None;
                }
                let result = match receiver.ty {
                    Ty::Vec(scalar, lanes) => {
                        let hir::ExprKind::Int(lane) = &index.kind else { return None };
                        if *lane < 0 || (*lane as u128) >= lanes as u128 {
                            return None;
                        }
                        align_sema::scalar_to_ty(scalar)
                    }
                    Ty::Array(scalar, _) | Ty::Slice(scalar) | Ty::DynArray(scalar) => {
                        align_sema::scalar_to_ty(scalar)
                    }
                    Ty::DynSliceArray(primitive) => {
                        Ty::Slice(align_sema::prim_to_scalar(primitive))
                    }
                    Ty::StructArray(id, _) | Ty::DynStructArray(id, Layout::Aos) | Ty::Soa(id) => {
                        Ty::Struct(id)
                    }
                    _ => return None,
                };
                if matches!(receiver.ty, Ty::Array(..) | Ty::StructArray(..))
                    && !matches!(
                        recv.kind,
                        hir::ExprKind::Local(_) | hir::ExprKind::ArrayLit { .. }
                    )
                {
                    return None;
                }
                if !self.ty_copy_ok(result, context) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[receiver, index_flow]);
                Some((result, falls, breaks))
            }
            hir::ExprKind::SliceRange { recv, start, end } => {
                let receiver = self.expr_flow(recv)?;
                let start_flow = match start.as_deref() {
                    Some(expr) => Some(self.expr_flow(expr)?),
                    None => None,
                };
                let end_flow = match end.as_deref() {
                    Some(expr) => Some(self.expr_flow(expr)?),
                    None => None,
                };
                if start_flow.as_ref().is_some_and(|flow| flow.ty != i64_ty())
                    || end_flow.as_ref().is_some_and(|flow| flow.ty != i64_ty())
                {
                    return None;
                }
                let result = match receiver.ty {
                    Ty::Str | Ty::String => Ty::Str,
                    Ty::Array(scalar, _) | Ty::Slice(scalar) | Ty::DynArray(scalar) => {
                        if !self.scalar_copy_ok(scalar) {
                            return None;
                        }
                        if matches!(receiver.ty, Ty::Array(..))
                            && !matches!(
                                recv.kind,
                                hir::ExprKind::Local(_) | hir::ExprKind::ArrayLit { .. }
                            )
                        {
                            return None;
                        }
                        Ty::Slice(scalar)
                    }
                    _ => return None,
                };
                let mut flows = vec![receiver];
                if let Some(flow) = start_flow {
                    flows.push(flow);
                }
                if let Some(flow) = end_flow {
                    flows.push(flow);
                }
                let (falls, breaks) = strict_flow(&flows);
                Some((result, falls, breaks))
            }
            hir::ExprKind::ElemField {
                recv,
                index,
                path,
                struct_id,
            } => {
                let receiver = self.expr_flow(recv)?;
                let index_flow = self.expr_flow(index)?;
                if index_flow.ty != i64_ty() || path.is_empty() {
                    return None;
                }
                let receiver_id = match receiver.ty {
                    Ty::StructArray(id, _) | Ty::DynStructArray(id, Layout::Aos) | Ty::Soa(id) => id,
                    _ => return None,
                };
                if receiver_id != *struct_id
                    || (matches!(receiver.ty, Ty::StructArray(..))
                        && !matches!(
                            recv.kind,
                            hir::ExprKind::Local(_) | hir::ExprKind::ArrayLit { .. }
                        ))
                {
                    return None;
                }
                let leaf = self.field_path_ty(Some(Ty::Struct(*struct_id)), path)?;
                if !self.ty_copy_ok(leaf, context) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[receiver, index_flow]);
                Some((leaf, falls, breaks))
            }
            hir::ExprKind::Template(parts) => {
                self.derive_template_expression(parts, context)
            }
            hir::ExprKind::JsonDecode { struct_id, input } => {
                self.derive_json_decode_struct(*struct_id, input, context, false)
            }
            hir::ExprKind::JsonDecodeArray { elem, input } => {
                self.derive_json_decode_array(*elem, input, context)
            }
            hir::ExprKind::JsonDecodeScalar { scalar, input } => {
                self.derive_json_decode_scalar(*scalar, input, context)
            }
            hir::ExprKind::JsonDecodeStructArray { struct_id, input } => {
                self.derive_json_decode_struct(*struct_id, input, context, true)
            }
            hir::ExprKind::JsonDecodeSoa { struct_id, input } => {
                let flow = self.expr_flow(input)?;
                if context.arena_depth == 0
                    || flow.ty != Ty::Str
                    || !self.json_soa_struct_ok(*struct_id)
                {
                    return None;
                }
                let error = self.error_id()?;
                let (falls, breaks) = strict_flow(&[flow]);
                Some((
                    Ty::Result(Scalar::Soa(*struct_id), Scalar::Enum(error)),
                    falls,
                    breaks,
                ))
            }
            hir::ExprKind::JsonDecodeUnion { enum_id, input } => {
                let flow = self.expr_flow(input)?;
                if flow.ty != Ty::Str || !self.json_union_descriptor_ok(*enum_id, false) {
                    return None;
                }
                let error = self.error_id()?;
                let (falls, breaks) = strict_flow(&[flow]);
                Some((
                    Ty::Result(Scalar::Enum(*enum_id), Scalar::Enum(error)),
                    falls,
                    breaks,
                ))
            }
            hir::ExprKind::JsonDoc { input } => {
                let flow = self.expr_flow(input)?;
                if context.arena_depth == 0 || flow.ty != Ty::Str {
                    return None;
                }
                let error = self.error_id()?;
                let (falls, breaks) = strict_flow(&[flow]);
                Some((
                    Ty::Result(Scalar::JsonDoc, Scalar::Enum(error)),
                    falls,
                    breaks,
                ))
            }
            hir::ExprKind::JsonDocKind { doc } => {
                let flow = self.expr_flow(doc)?;
                let kind = self.json_kind_id()?;
                if flow.ty != Ty::JsonDoc {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[flow]);
                Some((Ty::Enum(kind), falls, breaks))
            }
            hir::ExprKind::JsonDocGet { doc, key } => {
                let doc_flow = self.expr_flow(doc)?;
                let key_flow = self.expr_flow(key)?;
                if doc_flow.ty != Ty::JsonDoc || key_flow.ty != Ty::Str {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[doc_flow, key_flow]);
                Some((Ty::JsonDoc, falls, breaks))
            }
            hir::ExprKind::JsonDocAt { doc, index } => {
                let doc_flow = self.expr_flow(doc)?;
                let index_flow = self.expr_flow(index)?;
                if doc_flow.ty != Ty::JsonDoc || index_flow.ty != i64_ty() {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[doc_flow, index_flow]);
                Some((Ty::JsonDoc, falls, breaks))
            }
            hir::ExprKind::JsonDocAsStr { doc } => {
                let flow = self.expr_flow(doc)?;
                if flow.ty != Ty::JsonDoc {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[flow]);
                Some((Ty::Option(Scalar::Str), falls, breaks))
            }
            hir::ExprKind::JsonDocAsScalar { doc, scalar } => {
                let flow = self.expr_flow(doc)?;
                if flow.ty != Ty::JsonDoc || !self.json_doc_scalar_ok(*scalar) {
                    return None;
                }
                let payload = align_sema::ty_to_scalar(*scalar)?;
                let (falls, breaks) = strict_flow(&[flow]);
                Some((Ty::Option(payload), falls, breaks))
            }
            hir::ExprKind::JsonDocLen { doc } => {
                let flow = self.expr_flow(doc)?;
                if flow.ty != Ty::JsonDoc {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[flow]);
                Some((i64_ty(), falls, breaks))
            }
            hir::ExprKind::JsonDocKey { doc, index } => {
                let doc_flow = self.expr_flow(doc)?;
                let index_flow = self.expr_flow(index)?;
                if doc_flow.ty != Ty::JsonDoc || index_flow.ty != i64_ty() {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[doc_flow, index_flow]);
                Some((Ty::Option(Scalar::Str), falls, breaks))
            }
            hir::ExprKind::JsonDocElems { doc } => {
                let flow = self.expr_flow(doc)?;
                if context.arena_depth == 0 || flow.ty != Ty::JsonDoc {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[flow]);
                Some((Ty::Slice(Scalar::JsonDoc), falls, breaks))
            }
            hir::ExprKind::JsonScan { struct_id, input } => {
                let flow = self.expr_flow(input)?;
                if flow.ty != Ty::Str || !self.json_struct_descriptor_ok(*struct_id, false) {
                    return None;
                }
                let (falls, breaks) = strict_flow(&[flow]);
                Some((Ty::JsonScanner(*struct_id), falls, breaks))
            }
            hir::ExprKind::ArrayGroupAgg {
                base,
                struct_id,
                key_field,
                value_field,
                op,
                source,
            } => self.derive_group_aggregate(
                context,
                *base,
                *struct_id,
                *key_field,
                *value_field,
                *op,
                *source,
                expression.ty,
            ),
            hir::ExprKind::ArrayGroupAggMulti {
                base,
                struct_id,
                key_field,
                aggs,
                source,
            } => self.derive_group_aggregate_multi(
                context,
                *base,
                *struct_id,
                *key_field,
                aggs,
                *source,
                expression.ty,
            ),
            hir::ExprKind::ArrayDictEncode {
                base,
                struct_id,
                key_field,
            } => self.derive_dictionary(
                context,
                *base,
                *struct_id,
                *key_field,
            ),
            _ => None,
        }
    }

    fn derive_template_expression(
        &self,
        parts: &[hir::TemplatePart],
        _: &BodyContext,
    ) -> Option<(Ty, bool, Vec<Ty>)> {
        let mut flows = Vec::new();
        // `true` means that the current object has emitted at least one optional field since the
        // last `PopComma`.  The stack makes nested descriptor-driven objects independent.
        let mut object_has_option = Vec::new();
        for part in parts {
            match part {
                hir::TemplatePart::Text(text) => match text.as_str() {
                    "{" => object_has_option.push(false),
                    "}" => match object_has_option.pop() {
                        Some(false) => {}
                        Some(true) | None => return None,
                    },
                    _ => {}
                },
                hir::TemplatePart::Hole(expression) => {
                    let flow = self.expr_flow(expression)?;
                    let printable = match flow.ty {
                        Ty::Int(integer) => valid_int(integer.bits),
                        Ty::Float(float) => valid_float(float.bits),
                        Ty::Bool | Ty::Char | Ty::Str => true,
                        _ => false,
                    };
                    if !printable {
                        return None;
                    }
                    flows.push(flow);
                }
                hir::TemplatePart::JsonStr(expression) => {
                    let flow = self.expr_flow(expression)?;
                    if flow.ty != Ty::Str {
                        return None;
                    }
                    flows.push(flow);
                }
                hir::TemplatePart::OptionField { access, name } => {
                    if !valid_declaration_name(name)
                        || object_has_option.last().is_none()
                        || !matches!(
                            self.expr_flow(access)?.ty,
                            Ty::Option(payload) if self.json_array_element_ok(payload)
                        )
                    {
                        return None;
                    }
                    let flow = self.expr_flow(access)?;
                    if let Some(has_option) = object_has_option.last_mut() {
                        *has_option = true;
                    }
                    flows.push(flow);
                }
                hir::TemplatePart::OptionStructField {
                    access,
                    name,
                    struct_id,
                } => {
                    let flow = self.expr_flow(access)?;
                    if !valid_declaration_name(name)
                        || object_has_option.last().is_none()
                        || flow.ty != Ty::Option(Scalar::Struct(*struct_id))
                        || !self.json_struct_descriptor_ok(*struct_id, true)
                    {
                        return None;
                    }
                    if let Some(has_option) = object_has_option.last_mut() {
                        *has_option = true;
                    }
                    flows.push(flow);
                }
                hir::TemplatePart::PopComma => {
                    let has_option = object_has_option.last_mut()?;
                    if !*has_option {
                        return None;
                    }
                    *has_option = false;
                }
                hir::TemplatePart::StructArrayField { access, struct_id } => {
                    let flow = self.expr_flow(access)?;
                    if flow.ty != Ty::DynStructArray(*struct_id, align_sema::Layout::Aos)
                        || !self.json_struct_descriptor_ok(*struct_id, true)
                    {
                        return None;
                    }
                    flows.push(flow);
                }
                hir::TemplatePart::ScalarArrayField { access, elem } => {
                    let flow = self.expr_flow(access)?;
                    if flow.ty != Ty::DynArray(*elem) || !self.json_array_element_ok(*elem) {
                        return None;
                    }
                    flows.push(flow);
                }
                hir::TemplatePart::UnionValue { access, enum_id } => {
                    let flow = self.expr_flow(access)?;
                    if flow.ty != Ty::Enum(*enum_id)
                        || !self.json_union_descriptor_ok(*enum_id, true)
                    {
                        return None;
                    }
                    flows.push(flow);
                }
            }
        }
        if !object_has_option.is_empty() {
            return None;
        }
        let (falls, breaks) = strict_flow(&flows);
        Some((Ty::Str, falls, breaks))
    }

    fn derive_json_decode_struct(
        &self,
        struct_id: u32,
        input: &hir::Expr,
        _: &BodyContext,
        array: bool,
    ) -> Option<(Ty, bool, Vec<Ty>)> {
        let flow = self.expr_flow(input)?;
        if flow.ty != Ty::Str || !self.json_struct_descriptor_ok(struct_id, false) {
            return None;
        }
        let payload = if array {
            Ty::DynStructArray(struct_id, align_sema::Layout::Aos)
        } else {
            Ty::Struct(struct_id)
        };
        let error = self.error_id()?;
        let result = Ty::Result(align_sema::ty_to_scalar(payload)?, Scalar::Enum(error));
        let (falls, breaks) = strict_flow(&[flow]);
        Some((result, falls, breaks))
    }

    fn derive_json_decode_array(
        &self,
        elem: Ty,
        input: &hir::Expr,
        _: &BodyContext,
    ) -> Option<(Ty, bool, Vec<Ty>)> {
        let flow = self.expr_flow(input)?;
        if flow.ty != Ty::Str || !self.json_scalar_target_ok(elem) {
            return None;
        }
        let payload = Ty::DynArray(align_sema::ty_to_scalar(elem)?);
        let error = self.error_id()?;
        let (falls, breaks) = strict_flow(&[flow]);
        Some((Ty::Result(align_sema::ty_to_scalar(payload)?, Scalar::Enum(error)), falls, breaks))
    }

    fn derive_json_decode_scalar(
        &self,
        scalar: Ty,
        input: &hir::Expr,
        _: &BodyContext,
    ) -> Option<(Ty, bool, Vec<Ty>)> {
        let flow = self.expr_flow(input)?;
        if flow.ty != Ty::Str || !self.json_scalar_target_ok(scalar) {
            return None;
        }
        let error = self.error_id()?;
        let (falls, breaks) = strict_flow(&[flow]);
        Some((Ty::Result(align_sema::ty_to_scalar(scalar)?, Scalar::Enum(error)), falls, breaks))
    }

    fn json_kind_id(&self) -> Option<u32> {
        let names = ["Object", "Array", "Str", "Number", "Bool", "Null", "Missing"];
        let mut found = None;
        for (id, definition) in self.program.enums.iter().enumerate() {
            if definition.name != "json.kind"
                || definition.source_name != "json.kind"
                || definition.variants.len() != names.len()
                || !definition.variants.iter().zip(names).all(|(variant, name)| {
                    variant.name == name && variant.payload.is_empty() && variant.field_base == 1
                })
            {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = u32::try_from(id).ok();
        }
        found
    }

    #[allow(clippy::too_many_arguments)]
    fn derive_group_aggregate(
        &self,
        context: &BodyContext,
        base: hir::LocalId,
        struct_id: u32,
        key_field: u32,
        value_field: Option<u32>,
        op: hir::GroupOp,
        source: hir::GroupSource,
        result_ty: Ty,
    ) -> Option<(Ty, bool, Vec<Ty>)> {
        if !self.local_ok(context, base) {
            return None;
        }
        let base_ty = self.local_type(context, base)?;
        let definition = self.program.structs.get(struct_id as usize)?;
        let key_ty = definition.fields.get(key_field as usize)?.ty;
        let source_key_ty = match source {
            hir::GroupSource::SoaI64 => {
                if base_ty != Ty::Soa(struct_id) {
                    return None;
                }
                Ty::Int(align_sema::IntTy { bits: 64, signed: true })
            }
            hir::GroupSource::SoaStr => {
                if base_ty != Ty::Soa(struct_id) {
                    return None;
                }
                Ty::Str
            }
            hir::GroupSource::AosStr => {
                if base_ty != Ty::DynStructArray(struct_id, align_sema::Layout::Aos) {
                    return None;
                }
                Ty::Str
            }
            hir::GroupSource::Encoded => {
                let Ty::DictEncoded(encoded_id, encoded_field) = base_ty else {
                    return None;
                };
                if encoded_id != struct_id || encoded_field != key_field {
                    return None;
                }
                Ty::Str
            }
        };
        if key_ty != source_key_ty {
            return None;
        }
        let value_ty = match value_field {
            Some(field) => Some(definition.fields.get(field as usize)?.ty),
            None => None,
        };
        match op {
            hir::GroupOp::Count if value_field.is_some() => return None,
            hir::GroupOp::Count => {}
            hir::GroupOp::Sum | hir::GroupOp::Min | hir::GroupOp::Max => {
                if value_ty != Some(Ty::Int(align_sema::IntTy { bits: 64, signed: true })) {
                    return None;
                }
            }
        }
        let array_i64 = align_sema::ty_to_scalar(Ty::DynArray(Scalar::Int(
            align_sema::IntTy { bits: 64, signed: true },
        )))?;
        let array_key = align_sema::ty_to_scalar(Ty::DynArray(
            align_sema::ty_to_scalar(source_key_ty)?,
        ))?;
        let expected = [array_key, array_i64];
        let Ty::Tuple(tuple_id) = result_ty else {
            return None;
        };
        let tuple = self.program.tuples.get(tuple_id as usize)?;
        if tuple.elems.as_slice() != expected {
            return None;
        }
        Some((result_ty, true, Vec::new()))
    }

    #[allow(clippy::too_many_arguments)]
    fn derive_group_aggregate_multi(
        &self,
        context: &BodyContext,
        base: hir::LocalId,
        struct_id: u32,
        key_field: u32,
        aggregates: &[hir::GroupAgg1],
        source: hir::GroupSource,
        result_ty: Ty,
    ) -> Option<(Ty, bool, Vec<Ty>)> {
        if !self.local_ok(context, base)
            || source != hir::GroupSource::AosStr
            || aggregates.is_empty()
        {
            return None;
        }
        if self.local_type(context, base)?
            != Ty::DynStructArray(struct_id, align_sema::Layout::Aos)
        {
            return None;
        }
        let definition = self.program.structs.get(struct_id as usize)?;
        if definition.fields.get(key_field as usize)?.ty != Ty::Str {
            return None;
        }
        let i64_ty = Ty::Int(align_sema::IntTy { bits: 64, signed: true });
        for aggregate in aggregates {
            match aggregate.op {
                hir::GroupOp::Count if aggregate.value_field.is_some() => return None,
                hir::GroupOp::Count => {}
                hir::GroupOp::Sum | hir::GroupOp::Min | hir::GroupOp::Max => {
                    let field = aggregate.value_field?;
                    if definition.fields.get(field as usize)?.ty != i64_ty {
                        return None;
                    }
                }
            }
        }
        let array_str = align_sema::ty_to_scalar(Ty::DynArray(Scalar::Str))?;
        let array_i64 = align_sema::ty_to_scalar(Ty::DynArray(Scalar::Int(
            align_sema::IntTy { bits: 64, signed: true },
        )))?;
        let Ty::Tuple(tuple_id) = result_ty else {
            return None;
        };
        let tuple = self.program.tuples.get(tuple_id as usize)?;
        if tuple.elems.first().copied() != Some(array_str)
            || tuple.elems.len() != aggregates.len().saturating_add(1)
            || tuple.elems.iter().skip(1).any(|element| *element != array_i64)
        {
            return None;
        }
        Some((result_ty, true, Vec::new()))
    }

    fn derive_dictionary(
        &self,
        context: &BodyContext,
        base: hir::LocalId,
        struct_id: u32,
        key_field: u32,
    ) -> Option<(Ty, bool, Vec<Ty>)> {
        if !self.local_ok(context, base)
            || self.local_type(context, base)?
                != Ty::DynStructArray(struct_id, align_sema::Layout::Aos)
        {
            return None;
        }
        let definition = self.program.structs.get(struct_id as usize)?;
        if definition.fields.get(key_field as usize)?.ty != Ty::Str {
            return None;
        }
        Some((Ty::DictEncoded(struct_id, key_field), true, Vec::new()))
    }

    #[allow(clippy::too_many_arguments)]
    fn pipeline_reducer_callable_ok(
        &self,
        func: &str,
        captures: &[hir::Expr],
        capture_flows: &[BodyFlow],
        first: Ty,
        second: Ty,
        output: Ty,
        context: &BodyContext,
    ) -> bool {
        let Some(signature) = self.resolve_signature(func) else {
            return false;
        };
        if signature.is_extern && context.unsafe_depth == 0 {
            return false;
        }
        if signature.modes.len() != signature.params.len()
            || signature.modes.iter().any(|mode| *mode != align_ast::ParamMode::ByValue)
            || capture_flows.len() != captures.len()
            || capture_flows.iter().any(|flow| !self.ty_copy_ok(flow.ty, context))
            || !self.ty_copy_ok(first, context)
            || !self.ty_copy_ok(second, context)
            || !self.ty_copy_ok(output, context)
            || signature.ret != output
        {
            return false;
        }
        let mut expected = vec![first, second];
        expected.extend(capture_flows.iter().map(|flow| flow.ty));
        if signature.params != expected {
            return false;
        }
        match signature.origin {
            Some(hir::FnOrigin::Lifted { capture_count }) => {
                usize::try_from(capture_count).ok() == Some(captures.len())
            }
            Some(hir::FnOrigin::Source { .. }) | Some(hir::FnOrigin::Monomorph) | None => true,
        }
    }

    fn array_to_array_result(&self, elem: Ty) -> Option<Ty> {
        match elem {
            Ty::Struct(id) => {
                if self.program.structs.get(id as usize)?.align.is_some()
                    || !self.scalar_copy_ok(Scalar::Struct(id))
                {
                    return None;
                }
                Some(Ty::DynStructArray(id, Layout::Aos))
            }
            other => {
                let scalar = align_sema::ty_to_scalar(other)?;
                self.body_ty_ok(Ty::DynArray(scalar)).then_some(Ty::DynArray(scalar))
            }
        }
    }

    fn soa_struct_ok(&self, id: u32) -> bool {
        self.program
            .structs
            .get(id as usize)
            .is_some_and(|definition| {
                !definition.fields.is_empty()
                    && definition.fields.iter().all(|field| {
                        matches!(
                            field.ty,
                            Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str
                        )
                    })
            })
    }

    fn finish_block(&mut self, block: &hir::Block, _: &BodyContext) -> bool {
        let mut falls = true;
        let mut breaks = Vec::new();
        let mut result = Ty::Unit;
        for statement in &block.stmts {
            let Some(flow) = self.statements.get(&ptr_key(statement)).cloned() else {
                return false;
            };
            if falls {
                breaks.extend(flow.breaks);
                if flow.falls {
                    continue;
                }
                falls = false;
                result = flow.ty;
            }
        }
        if falls && let Some(value) = block.value.as_deref() {
            let Some(flow) = self.exprs.get(&ptr_key(value)).cloned() else {
                return false;
            };
            breaks.extend(flow.breaks);
            result = flow.ty;
            falls = flow.falls;
        }
        self.blocks.insert(
            ptr_key(block),
            BodyFlow {
                ty: result,
                falls,
                breaks,
            },
        );
        true
    }

    fn finish_statement(&mut self, statement: &hir::Stmt, context: &BodyContext) -> bool {
        let children = statement_children(statement);
        let flows = match children
            .iter()
            .map(|child| self.exprs.get(&ptr_key(*child)).cloned())
            .collect::<Option<Vec<_>>>()
        {
            Some(flows) => flows,
            None => return false,
        };
        let mut sequence_breaks = Vec::new();
        let mut children_fall = true;
        for flow in &flows {
            if children_fall {
                sequence_breaks.extend(flow.breaks.clone());
                children_fall = flow.falls;
            }
        }
        match statement {
            hir::Stmt::Let { local, .. } => {
                let Some(local_ty) = self.local_type(context, *local) else {
                    return false;
                };
                if flows.first().is_none_or(|flow| flow.ty != local_ty) {
                    return false;
                }
                self.store_statement(
                    statement,
                    Ty::Unit,
                    children_fall,
                    sequence_breaks,
                )
            }
            hir::Stmt::LetTuple { locals, tuple_id, .. } => {
                let Some(tuple) = self.program.tuples.get(*tuple_id as usize) else {
                    return false;
                };
                let Some(init) = flows.first() else {
                    return false;
                };
                if init.ty != Ty::Tuple(*tuple_id) {
                    return false;
                }
                for (local, expected) in locals.iter().zip(&tuple.elems) {
                    if let Some(local) = local
                        && self.local_type(context, *local)
                            != Some(align_sema::scalar_to_ty(*expected))
                    {
                        return false;
                    }
                }
                self.store_statement(statement, Ty::Unit, children_fall, sequence_breaks)
            }
            hir::Stmt::Assign { local, .. } => {
                let Some(local_ty) = self.local_type(context, *local) else {
                    return false;
                };
                if flows.first().is_none_or(|flow| flow.ty != local_ty) {
                    return false;
                }
                self.store_statement(statement, Ty::Unit, children_fall, sequence_breaks)
            }
            hir::Stmt::AssignIndex { base, .. } => {
                let Some(base_ty) = self.local_type(context, *base) else {
                    return false;
                };
                let Some(element_ty) = index_element_ty(base_ty) else {
                    return false;
                };
                let [index_flow, value_flow] = flows.as_slice() else {
                    return false;
                };
                if index_flow.ty != i64_ty()
                    || value_flow.ty != element_ty
                    || !self.primitive_store_ty_ok(element_ty)
                {
                    return false;
                }
                self.store_statement(statement, Ty::Unit, children_fall, sequence_breaks)
            }
            hir::Stmt::AssignVecLane { local, .. } => {
                let Some(Ty::Vec(scalar, _)) = self.local_type(context, *local) else {
                    return false;
                };
                if flows.first().is_none_or(|flow| flow.ty != align_sema::scalar_to_ty(scalar)) {
                    return false;
                }
                self.store_statement(statement, Ty::Unit, children_fall, sequence_breaks)
            }
            hir::Stmt::AssignField { root, path, .. } => {
                let Some(leaf) = self.field_path_ty(self.local_type(context, *root), path) else {
                    return false;
                };
                if flows.first().is_none_or(|flow| flow.ty != leaf) {
                    return false;
                }
                self.store_statement(statement, Ty::Unit, children_fall, sequence_breaks)
            }
            hir::Stmt::AssignElemField {
                struct_id,
                path,
                soa,
                ..
            } => {
                let Some(leaf) = self.field_path_ty(Some(Ty::Struct(*struct_id)), path) else {
                    return false;
                };
                if *soa && path.len() != 1 {
                    return false;
                }
                let [index_flow, value_flow] = flows.as_slice() else {
                    return false;
                };
                if index_flow.ty != i64_ty() || value_flow.ty != leaf {
                    return false;
                }
                self.store_statement(statement, Ty::Unit, children_fall, sequence_breaks)
            }
            hir::Stmt::AssignElem { struct_id, .. } => {
                let [index_flow, value_flow] = flows.as_slice() else {
                    return false;
                };
                if index_flow.ty != i64_ty() || value_flow.ty != Ty::Struct(*struct_id) {
                    return false;
                }
                self.store_statement(statement, Ty::Unit, children_fall, sequence_breaks)
            }
            hir::Stmt::Return(value) => {
                let Some(function_ret) = self.program.fns.get(context.function).map(|function| function.ret) else {
                    return false;
                };
                if value.is_some() {
                    if flows.first().is_none_or(|flow| flow.ty != function_ret) {
                        return false;
                    }
                } else if function_ret != Ty::Unit {
                    return false;
                }
                let breaks = if children_fall { Vec::new() } else { sequence_breaks };
                self.store_statement(statement, Ty::Unit, false, breaks)
            }
            hir::Stmt::Break { value, accepted } => {
                let value_ty = value
                    .as_ref()
                    .and_then(|_| flows.first().map(|flow| flow.ty))
                    .unwrap_or(Ty::Unit);
                if value.is_some() && flows.is_empty() {
                    return false;
                }
                if *accepted {
                    let Some(target) = context.loop_targets.last().copied() else {
                        return false;
                    };
                    if value_ty != target {
                        return false;
                    }
                }
                if !children_fall {
                    return self.store_statement(statement, Ty::Unit, false, sequence_breaks);
                }
                let mut breaks = sequence_breaks;
                if *accepted {
                    breaks.push(value_ty);
                }
                self.store_statement(statement, Ty::Unit, false, breaks)
            }
            hir::Stmt::Expr(_) => {
                let Some(flow) = flows.first() else {
                    return false;
                };
                if matches!(flow.ty, Ty::Result(..)) {
                    return false;
                }
                self.store_statement(statement, flow.ty, flow.falls, flow.breaks.clone())
            }
        }
    }

    fn store_statement(
        &mut self,
        statement: &hir::Stmt,
        ty: Ty,
        falls: bool,
        breaks: Vec<Ty>,
    ) -> bool {
        self.statements
            .insert(ptr_key(statement), BodyFlow { ty, falls, breaks });
        true
    }

    fn local_type(&self, context: &BodyContext, id: hir::LocalId) -> Option<Ty> {
        self.program
            .fns
            .get(context.function)
            .and_then(|function| function.locals.get(id as usize))
            .filter(|local| local.id == id)
            .map(|local| local.ty)
    }

    fn field_path_ty(&self, start: Option<Ty>, path: &[u32]) -> Option<Ty> {
        let mut current = start?;
        if path.is_empty() {
            return None;
        }
        for &field in path {
            let Ty::Struct(id) = current else {
                return None;
            };
            current = self
                .program
                .structs
                .get(id as usize)?
                .fields
                .get(field as usize)?
                .ty;
        }
        Some(current)
    }

    fn match_arm_envelope(
        &mut self,
        arm: &hir::MatchArm,
        scrutinee_ty: Ty,
        context: &BodyContext,
    ) -> bool {
        let Some(payloads) = self.sum_payloads(scrutinee_ty) else {
            return false;
        };
        if arm.variants.is_empty() {
            return arm.bindings.is_empty();
        }
        if arm
            .variants
            .iter()
            .any(|variant| (*variant as usize) >= payloads.len())
            || {
                let mut seen = HashSet::new();
                arm.variants.iter().any(|variant| !seen.insert(*variant))
            }
        {
            return false;
        }
        if arm.variants.len() != 1 {
            return arm.bindings.is_empty();
        }
        let Some(&variant) = arm.variants.first() else {
            return false;
        };
        let Some(expected) = payloads.get(variant as usize) else {
            return false;
        };
        if arm.bindings.len() != expected.len() {
            return false;
        }
        let mut seen = HashSet::new();
        for (&local, &scalar) in arm.bindings.iter().zip(expected) {
            if !seen.insert(local)
                || self.local_type(context, local) != Some(align_sema::scalar_to_ty(scalar))
            {
                return false;
            }
        }
        arm.bindings
            .iter()
            .copied()
            .all(|local| self.record_binding(context.function, local))
    }

    fn sum_payloads(&self, ty: Ty) -> Option<Vec<Vec<Scalar>>> {
        match ty {
            Ty::Enum(id) => self
                .program
                .enums
                .get(id as usize)
                .map(|definition| definition.variants.iter().map(|v| v.payload.clone()).collect()),
            Ty::Option(payload) => Some(vec![vec![payload], Vec::new()]),
            Ty::Result(ok, err) => Some(vec![vec![ok], vec![err]]),
            Ty::Tagged(id) => match self.program.tagged_types.get(id as usize).copied()? {
                hir::TaggedType::Option(payload) => Some(vec![vec![payload], Vec::new()]),
                hir::TaggedType::Result(ok, err) => Some(vec![vec![ok], vec![err]]),
            },
            _ => None,
        }
    }

    fn expr_flow(&self, expression: &hir::Expr) -> Option<BodyFlow> {
        self.exprs.get(&ptr_key(expression)).cloned()
    }

    fn block_flow(&self, block: &hir::Block) -> Option<BodyFlow> {
        self.blocks.get(&ptr_key(block)).cloned()
    }

    fn expr_flows(&self, expressions: &[hir::Expr]) -> Option<Vec<BodyFlow>> {
        expressions.iter().map(|expression| self.expr_flow(expression)).collect()
    }

    fn resolve_signature(&self, name: &str) -> Option<BodySignature> {
        let mut found = None;
        for (index, function) in self.program.fns.iter().enumerate() {
            if function.name == name {
                if found.is_some() {
                    return None;
                }
                if function.params.len() != function.param_modes.len() {
                    return None;
                }
                let params = function
                    .params
                    .iter()
                    .map(|&id| function.locals.get(id as usize).map(|local| local.ty))
                    .collect::<Option<Vec<_>>>()?;
                found = Some(BodySignature {
                    params,
                    modes: function.param_modes.clone(),
                    ret: function.ret,
                    origin: Some(function.origin),
                    is_extern: false,
                });
                let _ = index;
            }
        }
        for function in &self.program.imported_fns {
            if function.name == name {
                if found.is_some() {
                    return None;
                }
                if function.params.len() != function.param_modes.len() {
                    return None;
                }
                found = Some(BodySignature {
                    params: function.params.clone(),
                    modes: function.param_modes.clone(),
                    ret: function.ret,
                    origin: None,
                    is_extern: false,
                });
            }
        }
        for function in &self.program.externs {
            if function.name == name {
                if found.is_some() {
                    return None;
                }
                if function.params.len() != function.param_modes.len() {
                    return None;
                }
                found = Some(BodySignature {
                    params: function.params.clone(),
                    modes: function.param_modes.clone(),
                    ret: function.ret,
                    origin: None,
                    is_extern: true,
                });
            }
        }
        found
    }

    fn resolve_lifted_signature(&self, expression: &hir::Expr) -> Option<BodySignature> {
        let name = match &expression.kind {
            hir::ExprKind::FnValue(name) => name,
            hir::ExprKind::Closure { lifted, .. } => lifted,
            _ => return None,
        };
        self.resolve_signature(name)
    }

    fn fn_value_matches(
        &self,
        fid: u32,
        signature: &BodySignature,
        spawn: Option<SpawnContext>,
    ) -> bool {
        let Some(function) = self.program.fn_types.get(fid as usize) else {
            return false;
        };
        if signature.params.len() != signature.modes.len()
            || signature.params.len() != function.params.len()
        {
            return false;
        }
        let Some(origin) = signature.origin else {
            return spawn.is_none()
                && signature
                    .params
                    .iter()
                    .zip(&signature.modes)
                    .zip(&function.params)
                    .all(|((ty, mode), (function_mode, scalar))| {
                        mode == function_mode
                            && align_sema::fn_sig_scalar(*ty) == Some(*scalar)
                    })
                && signature.ret == function.ret;
        };
        if signature.is_extern {
            return false;
        }
        if let Some(spawn) = spawn {
            if !matches!(origin, hir::FnOrigin::Lifted { capture_count: 0 })
                || !signature.params.is_empty()
                || !signature.modes.is_empty()
                || function.ret != align_sema::scalar_to_ty(spawn.ok)
            {
                return false;
            }
            let target_ret = if spawn.fallible {
                let Some(error) = self.error_id() else {
                    return false;
                };
                Ty::Result(spawn.ok, Scalar::Enum(error))
            } else {
                align_sema::scalar_to_ty(spawn.ok)
            };
            return signature.ret == target_ret && function.params.is_empty();
        }
        if matches!(origin, hir::FnOrigin::Lifted { capture_count } if capture_count != 0) {
            return false;
        }
        signature.params.len() == function.params.len()
            && signature.ret == function.ret
            && signature
                .params
                .iter()
                .zip(&signature.modes)
                .zip(&function.params)
                .all(|((ty, signature_mode), (function_mode, scalar))| {
                    signature_mode == function_mode
                        && align_sema::fn_sig_scalar(*ty) == Some(*scalar)
                })
    }

    fn closure_matches(
        &self,
        fid: u32,
        signature: &BodySignature,
        captures: &[hir::Expr],
        spawn: Option<SpawnContext>,
        context: &BodyContext,
    ) -> bool {
        let Some(hir::FnOrigin::Lifted { capture_count }) = signature.origin else {
            return false;
        };
        let Ok(capture_count) = usize::try_from(capture_count) else {
            return false;
        };
        if capture_count == 0 || capture_count != captures.len() {
            return false;
        }
        let Some(function_type) = self.program.fn_types.get(fid as usize) else {
            return false;
        };
        if function_type.params.len() + capture_count != signature.params.len()
            || function_type.params.iter().any(|(mode, _)| *mode != align_ast::ParamMode::ByValue)
            || signature
                .modes
                .iter()
                .any(|mode| *mode != align_ast::ParamMode::ByValue)
            || signature.modes.len() != signature.params.len()
        {
            return false;
        }
        if let Some(spawn) = spawn {
            let Some(error) = self.error_id() else { return false };
            let expected = if spawn.fallible {
                Ty::Result(spawn.ok, Scalar::Enum(error))
            } else {
                align_sema::scalar_to_ty(spawn.ok)
            };
            if signature.ret != expected || function_type.ret != align_sema::scalar_to_ty(spawn.ok) {
                return false;
            }
        } else if signature.ret != function_type.ret {
            return false;
        }
        for ((mode, scalar), expected) in function_type.params.iter().zip(&signature.params) {
            if *mode != align_ast::ParamMode::ByValue
                || align_sema::scalar_to_ty(*scalar) != *expected
            {
                return false;
            }
        }
        for (expected, capture) in signature
            .params
            .iter()
            .skip(function_type.params.len())
            .zip(captures)
        {
            let Some(flow) = self.expr_flow(capture) else { return false };
            if flow.ty != *expected || !self.ty_copy_ok(flow.ty, context) {
                return false;
            }
        }
        true
    }

    fn scalar_copy_ok(&self, scalar: Scalar) -> bool {
        if scalar.is_move() {
            return false;
        }
        match scalar {
            Scalar::Struct(id) => !align_sema::struct_is_move(
                id,
                &self.program.structs,
                &self.program.enums,
                &self.program.tagged_types,
            ),
            Scalar::Enum(id) => !align_sema::enum_is_move(
                id,
                &self.program.structs,
                &self.program.enums,
                &self.program.tagged_types,
            ),
            Scalar::Tagged(id) => !align_sema::drop_plan(
                Ty::Tagged(id),
                &self.program.structs,
                &self.program.enums,
                &self.program.tagged_types,
            )
            .needs_drop(),
            _ => true,
        }
    }

    fn ty_copy_ok(&self, ty: Ty, _: &BodyContext) -> bool {
        match ty {
            Ty::Struct(id) => !align_sema::struct_is_move(
                id,
                &self.program.structs,
                &self.program.enums,
                &self.program.tagged_types,
            ),
            Ty::Enum(id) => !align_sema::enum_is_move(
                id,
                &self.program.structs,
                &self.program.enums,
                &self.program.tagged_types,
            ),
            Ty::Tuple(id) => self.program.tuples.get(id as usize).is_some_and(|tuple| {
                tuple.elems.iter().all(|scalar| self.scalar_copy_ok(*scalar))
            }),
            Ty::Box(_) | Ty::String | Ty::DynArray(_) | Ty::DynStructArray(..) => false,
            other => align_sema::ty_to_scalar(other).is_some_and(|scalar| self.scalar_copy_ok(scalar)),
        }
    }

    fn error_id(&self) -> Option<u32> {
        let mut found = None;
        for (id, definition) in self.program.enums.iter().enumerate() {
            if definition.name == "Error" && definition.source_name == "Error" && builtin_error_shape(definition) {
                if found.is_some() {
                    return None;
                }
                found = u32::try_from(id).ok();
            }
        }
        found
    }

    fn out_arg_is_writable(
        &self,
        context: &BodyContext,
        args: &[hir::Expr],
        index: usize,
    ) -> bool {
        let Some(hir::ExprKind::Local(id)) = args.get(index).map(|arg| &arg.kind) else {
            return false;
        };
        self.program
            .fns
            .get(context.function)
            .and_then(|function| function.locals.get(*id as usize))
            .is_some_and(|local| local.id == *id && local.is_mut)
    }

    fn raw_scalar_ok(&self, scalar: Scalar) -> bool {
        match scalar {
            Scalar::Int(integer) => valid_int(integer.bits),
            Scalar::Float(float) => valid_float(float.bits),
            Scalar::Bool | Scalar::Char => true,
            Scalar::Struct(id) => self
                .program
                .structs
                .get(id as usize)
                .is_some_and(|definition| definition.c_repr),
            _ => false,
        }
    }

    fn raw_store_ty_ok(&self, ty: Ty) -> bool {
        align_sema::ty_to_scalar(ty).is_some_and(|scalar| self.raw_scalar_ok(scalar))
    }

    fn mangled_call_name_matches(
        &self,
        name: &str,
        type_args: &[Ty],
        signature: &BodySignature,
    ) -> bool {
        let suffix = type_args
            .iter()
            .map(|ty| format!("${}", body_ty_mangle(*ty, self.program)))
            .collect::<String>();
        signature.origin.is_some_and(|origin| {
            matches!(origin, hir::FnOrigin::Monomorph)
                && name
                    .strip_suffix(&suffix)
                    .is_some_and(|base| !base.is_empty())
        })
    }
}

fn context_polymorphic_expression(kind: &hir::ExprKind, falls: bool) -> bool {
    !falls
        && matches!(
            kind,
            hir::ExprKind::TaskGroup(_)
                | hir::ExprKind::Match { .. }
                | hir::ExprKind::If { .. }
                | hir::ExprKind::Block(_)
                | hir::ExprKind::Loop { .. }
                | hir::ExprKind::Arena(_)
                | hir::ExprKind::Unsafe(_)
        )
}

fn int_range(integer: align_sema::IntTy) -> Option<(i128, i128)> {
    if !valid_int(integer.bits) {
        return None;
    }
    let bits = integer.bits as u32;
    Some(if integer.signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    })
}

fn strict_flow(flows: &[BodyFlow]) -> (bool, Vec<Ty>) {
    let mut falls = true;
    let mut breaks = Vec::new();
    for flow in flows {
        if falls {
            breaks.extend(flow.breaks.clone());
            falls = flow.falls;
        }
    }
    (falls, breaks)
}

fn numeric_body_ty(ty: Ty) -> bool {
    matches!(ty, Ty::Int(_) | Ty::Float(_))
}

fn orderable_body_ty(ty: Ty) -> bool {
    matches!(ty, Ty::Int(_) | Ty::Float(_) | Ty::Char | Ty::Str)
}

fn fixed_array_shape(expression: &hir::Expr, ty: Ty) -> Option<(Scalar, u32)> {
    if !matches!(
        expression.kind,
        hir::ExprKind::Local(_) | hir::ExprKind::ArrayLit { .. }
    ) {
        return None;
    }
    match ty {
        Ty::Array(scalar, length) => Some((scalar, length)),
        _ => None,
    }
}

fn primitive_task_scalar(scalar: Scalar) -> bool {
    matches!(
        scalar,
        Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char | Scalar::Unit
    )
}

fn wait_type_ok(program: &hir::Program, ty: Ty, fallible: bool) -> bool {
    if !fallible {
        return ty == Ty::Unit;
    }
    let Ty::Result(Scalar::Unit, Scalar::Enum(error)) = ty else {
        return false;
    };
    program
        .enums
        .get(error as usize)
        .is_some_and(|definition| {
            definition.name == "Error"
                && definition.source_name == "Error"
                && builtin_error_shape(definition)
        })
}

fn unary_result(op: align_ast::UnOp, ty: Ty) -> Option<Ty> {
    match op {
        align_ast::UnOp::Neg => match ty {
            Ty::Int(integer) if integer.signed => Some(ty),
            Ty::Float(float) if valid_float(float.bits) => Some(ty),
            _ => None,
        },
        align_ast::UnOp::Not if ty == Ty::Bool => Some(Ty::Bool),
        align_ast::UnOp::BitNot => matches!(ty, Ty::Int(integer) if valid_int(integer.bits)).then_some(ty),
        _ => None,
    }
}

fn cast_result(from: Ty, to: Ty) -> Option<()> {
    let valid = match (from, to) {
        (Ty::Int(a), Ty::Int(b)) => valid_int(a.bits) && valid_int(b.bits),
        (Ty::Int(a), Ty::Float(b)) => valid_int(a.bits) && valid_float(b.bits),
        (Ty::Int(a), Ty::Char) => valid_int(a.bits),
        (Ty::Float(a), Ty::Int(b)) => valid_float(a.bits) && valid_int(b.bits),
        (Ty::Float(a), Ty::Float(b)) => valid_float(a.bits) && valid_float(b.bits),
        (Ty::Char, Ty::Int(b)) => valid_int(b.bits),
        (Ty::Char, Ty::Char) => true,
        _ => false,
    };
    valid.then_some(())
}

fn scalar_numeric(ty: Ty) -> bool {
    matches!(ty, Ty::Int(integer) if valid_int(integer.bits))
        || matches!(ty, Ty::Float(float) if valid_float(float.bits))
}

fn valid_vector_lanes(lanes: u32) -> bool {
    matches!(lanes, 2 | 4 | 8 | 16)
}

const BODY_CONST_POOL_MIN_ELEMS: u32 = 32;

fn valid_vector_scalar(scalar: Scalar) -> bool {
    matches!(scalar, Scalar::Int(integer) if valid_int(integer.bits))
        || matches!(scalar, Scalar::Float(float) if valid_float(float.bits))
}

fn const_array_scalar_ok(scalar: Scalar) -> bool {
    matches!(scalar, Scalar::Int(integer) if valid_int(integer.bits))
        || matches!(scalar, Scalar::Float(float) if valid_float(float.bits))
        || matches!(scalar, Scalar::Bool | Scalar::Char | Scalar::Str)
}

fn array_zip_scalar_ok(scalar: Scalar) -> bool {
    matches!(scalar, Scalar::Int(integer) if valid_int(integer.bits))
        || matches!(scalar, Scalar::Float(float) if valid_float(float.bits))
        || matches!(scalar, Scalar::Bool | Scalar::Char)
}

fn pooled_scalar_literal_ok(expression: &hir::Expr) -> bool {
    match &expression.kind {
        hir::ExprKind::Int(_)
        | hir::ExprKind::Float(_)
        | hir::ExprKind::Bool(_)
        | hir::ExprKind::Char(_) => true,
        hir::ExprKind::Unary {
            op: align_ast::UnOp::Neg,
            expr,
        } => matches!(
            &expr.kind,
            hir::ExprKind::Int(_) | hir::ExprKind::Float(_)
        ),
        _ => false,
    }
}

fn vector_numeric(ty: Ty) -> Option<(Scalar, u32)> {
    match ty {
        Ty::Vec(scalar @ (Scalar::Int(_) | Scalar::Float(_)), lanes)
            if valid_vector_scalar(scalar) && valid_vector_lanes(lanes) => Some((scalar, lanes)),
        _ => None,
    }
}

fn numeric_pair_result(lhs: Ty, rhs: Ty, mask: bool) -> Option<Ty> {
    if scalar_numeric(lhs) && lhs == rhs {
        return Some(lhs);
    }
    if let (Some((left, lanes)), Some((right, other_lanes))) =
        (vector_numeric(lhs), vector_numeric(rhs))
        && left == right
        && lanes == other_lanes
    {
        return Some(if mask { Ty::Mask(left, lanes) } else { lhs });
    }
    if let Some((scalar, lanes)) = vector_numeric(lhs)
        && rhs == align_sema::scalar_to_ty(scalar)
    {
        return Some(if mask { Ty::Mask(scalar, lanes) } else { lhs });
    }
    if let Some((scalar, lanes)) = vector_numeric(rhs)
        && lhs == align_sema::scalar_to_ty(scalar)
    {
        return Some(if mask { Ty::Mask(scalar, lanes) } else { rhs });
    }
    None
}

fn binary_result(op: align_ast::BinOp, lhs: Ty, rhs: Ty) -> Option<Ty> {
    use align_ast::BinOp;
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
            numeric_pair_result(lhs, rhs, false)
        }
        BinOp::Eq | BinOp::Ne => {
            if lhs == rhs
                && matches!(lhs, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str)
            {
                Some(Ty::Bool)
            } else {
                numeric_pair_result(lhs, rhs, true)
            }
        }
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            if lhs == rhs && matches!(lhs, Ty::Int(_) | Ty::Float(_) | Ty::Char | Ty::Str) {
                Some(Ty::Bool)
            } else {
                numeric_pair_result(lhs, rhs, true)
            }
        }
        BinOp::And | BinOp::Or if lhs == Ty::Bool && rhs == Ty::Bool => Some(Ty::Bool),
        BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
            if lhs == rhs && matches!(lhs, Ty::Int(integer) if valid_int(integer.bits)) =>
        {
            Some(lhs)
        }
        _ => None,
    }
}

fn math_result(fn_: hir::MathFn, operands: &[BodyFlow]) -> Option<Ty> {
    let tys: Vec<Ty> = operands.iter().map(|flow| flow.ty).collect();
    let first = *tys.first()?;
    let numeric = scalar_numeric(first) || vector_numeric(first).is_some();
    let float = matches!(first, Ty::Float(_))
        || matches!(first, Ty::Vec(Scalar::Float(_), _));
    let exact = |count: usize| tys.len() == count && tys.iter().all(|ty| *ty == first);
    match fn_ {
        hir::MathFn::Abs | hir::MathFn::Sqrt | hir::MathFn::Floor | hir::MathFn::Ceil
        | hir::MathFn::Round | hir::MathFn::Trunc => {
            if !exact(1) || !numeric || (matches!(fn_, hir::MathFn::Sqrt | hir::MathFn::Floor | hir::MathFn::Ceil | hir::MathFn::Round | hir::MathFn::Trunc) && !float) {
                None
            } else {
                Some(first)
            }
        }
        hir::MathFn::Min | hir::MathFn::Max => exact(2).then_some(first).filter(|_| numeric),
        hir::MathFn::Pow => (exact(2) && matches!(first, Ty::Float(_))).then_some(first),
        hir::MathFn::Fma => (exact(3) && float).then_some(first),
    }
}

fn builder_write_ty_ok(kind: hir::BuilderWriteKind, ty: Ty) -> bool {
    match kind {
        hir::BuilderWriteKind::Str => ty == Ty::Str,
        hir::BuilderWriteKind::Int => matches!(ty, Ty::Int(integer) if valid_int(integer.bits)),
        hir::BuilderWriteKind::Float => matches!(ty, Ty::Float(float) if valid_float(float.bits)),
        hir::BuilderWriteKind::Bool => ty == Ty::Bool,
        hir::BuilderWriteKind::Char => ty == Ty::Char,
    }
}

pub(crate) fn body_ty_mangle(ty: Ty, program: &hir::Program) -> String {
    enum Work {
        Type(Ty),
        Text(String),
        ExitTagged(u32),
        ExitFn(u32),
    }

    fn push_sequence(work: &mut Vec<Work>, sequence: Vec<Work>) {
        work.extend(sequence.into_iter().rev());
    }

    let roots = |params: &[u32], captures: &[u32]| {
        format!(
            "p{}_c{}",
            params
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join("."),
            captures
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join("."),
        )
    };

    let mut work = vec![Work::Type(ty)];
    let mut output = String::new();
    let mut tagged_visiting = HashSet::new();
    let mut fn_visiting = HashSet::new();
    while let Some(item) = work.pop() {
        match item {
            Work::Text(text) => output.push_str(&text),
            Work::ExitTagged(id) => {
                tagged_visiting.remove(&id);
            }
            Work::ExitFn(id) => {
                fn_visiting.remove(&id);
            }
            Work::Type(ty) => match ty {
                Ty::Struct(id) => output.push_str(&program.structs.get(id as usize).map_or_else(
                    || "S_invalid".to_string(),
                    |definition| format!("S{}_{}", definition.name.len(), definition.name),
                )),
                Ty::Enum(id) => output.push_str(&program.enums.get(id as usize).map_or_else(
                    || "E_invalid".to_string(),
                    |definition| format!("E{}_{}", definition.name.len(), definition.name),
                )),
                Ty::Tagged(id) if tagged_visiting.insert(id) => {
                    let sequence = match program.tagged_types.get(id as usize).copied() {
                        Some(hir::TaggedType::Option(payload)) => vec![
                            Work::Text("O_".to_string()),
                            Work::Type(align_sema::scalar_to_ty(payload)),
                            Work::ExitTagged(id),
                        ],
                        Some(hir::TaggedType::Result(ok, err)) => vec![
                            Work::Text("R_".to_string()),
                            Work::Type(align_sema::scalar_to_ty(ok)),
                            Work::Text("_".to_string()),
                            Work::Type(align_sema::scalar_to_ty(err)),
                            Work::ExitTagged(id),
                        ],
                        None => {
                            tagged_visiting.remove(&id);
                            vec![Work::Text("T_invalid".to_string())]
                        }
                    };
                    push_sequence(&mut work, sequence);
                }
                Ty::Tagged(_) => output.push_str("T_cycle"),
                Ty::Option(payload) => push_sequence(
                    &mut work,
                    vec![
                        Work::Text("O_".to_string()),
                        Work::Type(align_sema::scalar_to_ty(payload)),
                    ],
                ),
                Ty::Result(ok, err) => push_sequence(
                    &mut work,
                    vec![
                        Work::Text("R_".to_string()),
                        Work::Type(align_sema::scalar_to_ty(ok)),
                        Work::Text("_".to_string()),
                        Work::Type(align_sema::scalar_to_ty(err)),
                    ],
                ),
                Ty::Box(payload) => push_sequence(
                    &mut work,
                    vec![
                        Work::Text("B_".to_string()),
                        Work::Type(align_sema::scalar_to_ty(payload)),
                    ],
                ),
                Ty::Array(payload, len) => push_sequence(
                    &mut work,
                    vec![
                        Work::Text(format!("A{len}_")),
                        Work::Type(align_sema::scalar_to_ty(payload)),
                    ],
                ),
                Ty::Slice(payload) => push_sequence(
                    &mut work,
                    vec![
                        Work::Text("V_".to_string()),
                        Work::Type(align_sema::scalar_to_ty(payload)),
                    ],
                ),
                Ty::DynArray(payload) => push_sequence(
                    &mut work,
                    vec![
                        Work::Text("D_".to_string()),
                        Work::Type(align_sema::scalar_to_ty(payload)),
                    ],
                ),
                Ty::Task(payload) => push_sequence(
                    &mut work,
                    vec![
                        Work::Text("K_".to_string()),
                        Work::Type(align_sema::scalar_to_ty(payload)),
                    ],
                ),
                Ty::StructArray(id, len) => push_sequence(
                    &mut work,
                    vec![
                        Work::Text(format!("A{len}_")),
                        Work::Type(Ty::Struct(id)),
                    ],
                ),
                Ty::DynStructArray(id, _) => push_sequence(
                    &mut work,
                    vec![
                        Work::Text("D_".to_string()),
                        Work::Type(Ty::Struct(id)),
                    ],
                ),
                Ty::Soa(id) => push_sequence(
                    &mut work,
                    vec![Work::Text("Q_".to_string()), Work::Type(Ty::Struct(id))],
                ),
                Ty::Tuple(id) => match program.tuples.get(id as usize) {
                    None => output.push_str("U_invalid"),
                    Some(tuple) => {
                        let mut sequence = vec![Work::Text(format!("U{}_", tuple.elems.len()))];
                        for (index, element) in tuple.elems.iter().enumerate() {
                            if index > 0 {
                                sequence.push(Work::Text("_".to_string()));
                            }
                            sequence.push(Work::Type(align_sema::scalar_to_ty(*element)));
                        }
                        push_sequence(&mut work, sequence);
                    }
                },
                Ty::Fn(id) if fn_visiting.insert(id) => match program.fn_types.get(id as usize) {
                    None => {
                        fn_visiting.remove(&id);
                        output.push_str("F_invalid");
                    }
                    Some(function) => {
                        let borrow = match &function.return_borrow {
                            hir::ReturnBorrowSummary::None => "n".to_string(),
                            hir::ReturnBorrowSummary::Roots { params, captures } => {
                                roots(params, captures)
                            }
                        };
                        let region = match &function.return_region {
                            hir::ReturnRegionSummary::None => "n".to_string(),
                            hir::ReturnRegionSummary::Roots { params, captures } => {
                                roots(params, captures)
                            }
                        };
                        let mut sequence = vec![Work::Text(format!("F{id}_"))];
                        for (index, (mode, scalar)) in function.params.iter().enumerate() {
                            if index > 0 {
                                sequence.push(Work::Text("_".to_string()));
                            }
                            sequence.push(Work::Text(
                                match mode {
                                    align_ast::ParamMode::ByValue => "v",
                                    align_ast::ParamMode::Out => "o",
                                    align_ast::ParamMode::Borrow => "b",
                                    align_ast::ParamMode::BorrowMut => "m",
                                }
                                .to_string(),
                            ));
                            sequence.push(Work::Type(align_sema::scalar_to_ty(*scalar)));
                        }
                        sequence.push(Work::Text("_".to_string()));
                        sequence.push(Work::Type(function.ret));
                        sequence.push(Work::Text(format!("_b{borrow}_r{region}")));
                        sequence.push(Work::ExitFn(id));
                        push_sequence(&mut work, sequence);
                    }
                },
                Ty::Fn(_) => output.push_str("F_cycle"),
                other => output.push_str(&body_simple_ty_name(other)),
            },
        }
    }
    output
        .chars()
        .map(|character| if character.is_alphanumeric() { character } else { '_' })
        .collect()
}

fn body_simple_ty_name(ty: Ty) -> String {
    match ty {
        Ty::Int(integer) => integer.name(),
        Ty::Float(float) => float.name(),
        Ty::Bool => "bool".to_string(),
        Ty::Char => "char".to_string(),
        Ty::Str => "str".to_string(),
        Ty::String => "string".to_string(),
        Ty::Unit => "()".to_string(),
        Ty::ArenaHandle => "arena".to_string(),
        Ty::Raw => "raw".to_string(),
        Ty::Builder => "builder".to_string(),
        Ty::Writer => "writer".to_string(),
        Ty::Reader => "reader".to_string(),
        Ty::Buffer => "buffer".to_string(),
        Ty::ArrayBuilder(scalar) => format!(
            "array_builder_{}",
            body_simple_ty_name(align_sema::scalar_to_ty(scalar))
        ),
        Ty::File => "file".to_string(),
        Ty::Rng => "rng".to_string(),
        Ty::Regex => "regex".to_string(),
        Ty::Captures => "captures".to_string(),
        Ty::CliCommand => "cli_command".to_string(),
        Ty::CliParsed => "cli_parsed".to_string(),
        Ty::TcpConn => "tcp_conn".to_string(),
        Ty::TcpListener => "tcp_listener".to_string(),
        Ty::UdpSocket => "udp_socket".to_string(),
        Ty::Child => "child".to_string(),
        Ty::Command => "command".to_string(),
        Ty::RunOutput => "run_output".to_string(),
        Ty::HttpRequest => "http_request".to_string(),
        Ty::HttpResponse => "http_response".to_string(),
        Ty::HttpClient => "http_client".to_string(),
        Ty::HttpServer => "http_server".to_string(),
        Ty::HttpRequestCtx => "http_request_ctx".to_string(),
        Ty::HttpHeaders => "http_headers".to_string(),
        Ty::ResponseBuilder => "response_builder".to_string(),
        Ty::HttpStream => "http_stream".to_string(),
        Ty::JsonDoc => "json_doc".to_string(),
        Ty::JsonScanner(id) => format!("json_scanner_struct_{id}"),
        Ty::DynSliceArray(primitive) => format!(
            "array_slice_{}",
            body_simple_ty_name(align_sema::scalar_to_ty(align_sema::prim_to_scalar(primitive)))
        ),
        Ty::DynResponseArray => "array_response".to_string(),
        Ty::Mask(scalar, lanes) => format!(
            "mask{lanes}_{}",
            body_simple_ty_name(align_sema::scalar_to_ty(scalar))
        ),
        Ty::Vec(scalar, lanes) => format!(
            "vec{lanes}_{}",
            body_simple_ty_name(align_sema::scalar_to_ty(scalar))
        ),
        Ty::DictEncoded(id, _) => format!("dict_encoded_struct_{id}"),
        Ty::Param(_) => "param".to_string(),
        Ty::IntVar(_) => "int_var".to_string(),
        Ty::FloatVar(_) => "float_var".to_string(),
        Ty::StrFinder => "str_finder".to_string(),
        Ty::Error => "error".to_string(),
        Ty::Struct(id) => format!("struct_{id}"),
        Ty::Enum(id) => format!("enum_{id}"),
        Ty::Tuple(id) => format!("tuple_{id}"),
        Ty::Fn(id) => format!("fn_{id}"),
        Ty::Task(scalar) => format!(
            "Task_{}",
            body_simple_ty_name(align_sema::scalar_to_ty(scalar))
        ),
        Ty::DynStructArray(_, Layout::Soa) => "array_struct_soa".to_string(),
        _ => "type".to_string(),
    }
}

fn ptr_key<T>(value: &T) -> usize {
    value as *const T as usize
}

fn statement_children(statement: &hir::Stmt) -> Vec<&hir::Expr> {
    match statement {
        hir::Stmt::Let { init, .. } | hir::Stmt::LetTuple { init, .. } => vec![init],
        hir::Stmt::Assign { value, .. }
        | hir::Stmt::AssignVecLane { value, .. }
        | hir::Stmt::AssignField { value, .. } => vec![value],
        hir::Stmt::AssignIndex { index, value, .. }
        | hir::Stmt::AssignElemField { index, value, .. }
        | hir::Stmt::AssignElem { index, value, .. } => vec![index, value],
        hir::Stmt::Return(value) | hir::Stmt::Break { value, .. } => {
            value.as_ref().into_iter().collect()
        }
        hir::Stmt::Expr(expression) => vec![expression],
    }
}

fn i64_ty() -> Ty {
    Ty::Int(align_sema::IntTy {
        bits: 64,
        signed: true,
    })
}

fn index_element_ty(ty: Ty) -> Option<Ty> {
    match ty {
        Ty::Array(element, _) | Ty::DynArray(element) | Ty::Slice(element) => {
            Some(align_sema::scalar_to_ty(element))
        }
        _ => None,
    }
}
}
