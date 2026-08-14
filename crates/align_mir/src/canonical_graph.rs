use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use align_ast::ParamMode;
use align_sema::{AggregateArrayElem, ArrayBuilderElem, Layout, PrimScalar, Scalar, Ty, hir};

use super::source_shape::{SourceShapeNode, SourceShapeView, source_shape_equal};
use super::{Program, function_embedded_types, remap_function_embedded_types};

#[derive(Clone, Debug)]
pub struct FunctionTypeDef {
    pub params: Vec<(ParamMode, Scalar)>,
    pub ret: Ty,
    pub return_borrow: hir::ReturnBorrowSummary,
    pub return_region: hir::ReturnRegionSummary,
    pub return_cleanup: hir::ReturnCleanupAbi,
}

#[derive(Clone, Debug)]
pub struct ProgramExtern {
    pub name: ProgramCall,
    pub params: Vec<Ty>,
    pub param_modes: Vec<ParamMode>,
    pub ret: Ty,
    pub return_borrow: hir::ReturnBorrowSummary,
    pub return_region: hir::ReturnRegionSummary,
    pub return_cleanup: hir::ReturnCleanupAbi,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProgramCall(Box<str>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramCallError {
    Empty,
    EmbeddedNul,
    TooLong,
}

impl ProgramCall {
    pub fn try_from_logical(value: &str) -> Result<Self, ProgramCallError> {
        if value.is_empty() {
            return Err(ProgramCallError::Empty);
        }
        if value.as_bytes().contains(&0) {
            return Err(ProgramCallError::EmbeddedNul);
        }
        if u32::try_from(value.len()).is_err() {
            return Err(ProgramCallError::TooLong);
        }
        Ok(Self(value.into()))
    }

    pub(super) fn from_validated(value: &str) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Display for ProgramCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalTy(Box<[u8]>);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalFnAbi(Box<[u8]>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalCodecError {
    Truncated,
    TrailingBytes,
    UnsupportedVersion,
    UnknownTag,
    InvalidBool,
    InvalidUtf8,
    EmbeddedNul,
    InvalidWidth,
    InvalidCount,
    MissingReference,
    DuplicateMember,
    NonCanonicalOrder,
    InvalidSummary,
    InvalidGraph,
}

impl From<CanonicalGraphError> for CanonicalCodecError {
    fn from(value: CanonicalGraphError) -> Self {
        match value {
            CanonicalGraphError::EmbeddedNul => Self::EmbeddedNul,
            CanonicalGraphError::InvalidWidth => Self::InvalidWidth,
            CanonicalGraphError::InvalidCount => Self::InvalidCount,
            CanonicalGraphError::MissingReference => Self::MissingReference,
            CanonicalGraphError::DuplicateMember => Self::DuplicateMember,
            CanonicalGraphError::InvalidSummary => Self::InvalidSummary,
            CanonicalGraphError::InvalidGraph => Self::InvalidGraph,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum CanonicalGraphError {
    EmbeddedNul,
    InvalidWidth,
    InvalidCount,
    MissingReference,
    DuplicateMember,
    InvalidSummary,
    InvalidGraph,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Node {
    Struct(u32),
    Enum(u32),
    Tuple(u32),
    Tagged(u32),
    Fn(u32),
    Resource(u32),
}

#[derive(Clone, Copy)]
struct CanonicalTypeView<'a> {
    structs: &'a [hir::StructDef],
    enums: &'a [hir::EnumDef],
    tuples: &'a [hir::TupleDef],
    tagged_types: &'a [hir::TaggedType],
    fn_types: &'a [FunctionTypeDef],
    resources: &'a [hir::ResourceDef],
}

impl SourceShapeView for CanonicalTypeView<'_> {
    fn source_shape_node(&self, node: Node) -> Option<SourceShapeNode<'_>> {
        match node {
            Node::Struct(id) => {
                self.structs
                    .get(id as usize)
                    .map(|definition| SourceShapeNode::Struct {
                        name: &definition.name,
                        source_name: &definition.source_name,
                        align: &definition.align,
                        c_repr: &definition.c_repr,
                        fields: &definition.fields,
                    })
            }
            Node::Enum(id) => self
                .enums
                .get(id as usize)
                .map(|definition| SourceShapeNode::Enum {
                    source_name: &definition.source_name,
                    variants: &definition.variants,
                }),
            Node::Tuple(id) => {
                self.tuples
                    .get(id as usize)
                    .map(|definition| SourceShapeNode::Tuple {
                        elems: &definition.elems,
                    })
            }
            Node::Tagged(id) => self
                .tagged_types
                .get(id as usize)
                .map(SourceShapeNode::Tagged),
            Node::Fn(id) => {
                self.fn_types
                    .get(id as usize)
                    .map(|definition| SourceShapeNode::Function {
                        params: &definition.params,
                        ret: &definition.ret,
                        return_borrow: &definition.return_borrow,
                        return_region: &definition.return_region,
                        return_cleanup: definition.return_cleanup,
                    })
            }
            Node::Resource(id) => self.resources.get(id as usize).map(SourceShapeNode::Resource),
        }
    }
}

struct ValidatedGraph<'a> {
    root: Ty,
    view: CanonicalTypeView<'a>,
    order: Vec<Node>,
}

impl<'a> ValidatedGraph<'a> {
    fn new(root: Ty, view: CanonicalTypeView<'a>) -> Result<Self, CanonicalGraphError> {
        Self::new_many(root, std::slice::from_ref(&root), view)
    }

    fn new_many(
        root: Ty,
        roots: &[Ty],
        view: CanonicalTypeView<'a>,
    ) -> Result<Self, CanonicalGraphError> {
        let mut validator = GraphValidator {
            view,
            pending: Vec::new(),
            seen: HashSet::new(),
            order: Vec::new(),
            candidates: Vec::new(),
            next_ordinal: 0,
            end_ordinals: HashMap::new(),
            inline_edges: Vec::new(),
        };
        let mut references = Vec::new();
        for &root in roots {
            validator.scan_ty(root, &mut references, None);
        }
        references.reverse();
        validator.pending.extend(references);
        while let Some(node) = validator.pending.pop() {
            validator.visit_node(node);
        }
        validator.collect_cross_node_candidates();
        validator.collect_inline_cycle_candidates();
        if let Some(candidate) = validator
            .candidates
            .iter()
            .min_by_key(|candidate| (candidate.ordinal, candidate.tie_rank))
        {
            return Err(candidate.error);
        }
        Ok(Self {
            root,
            view,
            order: validator.order,
        })
    }
}

struct GraphValidator<'a> {
    view: CanonicalTypeView<'a>,
    pending: Vec<Node>,
    seen: HashSet<Node>,
    order: Vec<Node>,
    candidates: Vec<ErrorCandidate>,
    next_ordinal: u64,
    end_ordinals: HashMap<Node, u64>,
    inline_edges: Vec<InlineEdge>,
}

#[derive(Clone, Copy)]
struct ErrorCandidate {
    ordinal: u64,
    tie_rank: u8,
    error: CanonicalGraphError,
}

#[derive(Clone, Copy)]
struct InlineEdge {
    from: Node,
    to: Node,
    ordinal: u64,
}

impl<'a> GraphValidator<'a> {
    fn visit_node(&mut self, node: Node) {
        if !self.seen.insert(node) {
            return;
        }
        let view = self.view;
        let Some(shape) = view.source_shape_node(node) else {
            let ordinal = self.field_ordinal();
            self.candidate(ordinal, CanonicalGraphError::MissingReference);
            return;
        };
        self.order.push(node);
        let mut references = Vec::new();
        match shape {
            SourceShapeNode::Struct {
                name,
                source_name,
                align,
                fields,
                c_repr,
            } => {
                let static_descriptor = (name.starts_with("pkg.db$query$")
                    || name.starts_with("pkg.db$command$"))
                    && matches!(fields, [hir::FieldDef { name, ty: Ty::Raw }]
                        if name == align_sema::STATIC_DESCRIPTOR_DATA_FIELD)
                    && align.is_none()
                    && !*c_repr;
                let sqlite_callback_descriptor = name == "pkg.db.sqlite$scalar_function"
                    && matches!(fields, [hir::FieldDef { name, ty: Ty::Raw }]
                        if name == align_sema::SQLITE_CALLBACK_DESCRIPTOR_DATA_FIELD)
                    && align.is_none()
                    && !*c_repr;
                let source_ordinal = self.field_ordinal();
                self.validate_source_name(source_name, source_ordinal);
                if let Some(align) = *align {
                    let ordinal = self.field_ordinal();
                    if align > (1 << 29) || !align.is_power_of_two() {
                        self.candidate(ordinal, CanonicalGraphError::InvalidGraph);
                    }
                } else {
                    self.field_ordinal();
                }
                self.field_ordinal(); // c_repr
                let count_ordinal = self.field_ordinal();
                self.validate_count(fields.len(), count_ordinal);
                let mut names = HashSet::new();
                for field in fields {
                    let name_ordinal = self.field_ordinal();
                    if !static_descriptor && !sqlite_callback_descriptor {
                        self.validate_identifier(&field.name, name_ordinal);
                    }
                    if !names.insert(field.name.as_str()) {
                        self.candidate(name_ordinal, CanonicalGraphError::DuplicateMember);
                    }
                    self.scan_ty(field.ty, &mut references, Some(node));
                }
            }
            SourceShapeNode::Enum {
                source_name,
                variants,
            } => {
                let source_ordinal = self.field_ordinal();
                self.validate_source_name(source_name, source_ordinal);
                let count_ordinal = self.field_ordinal();
                self.validate_count(variants.len(), count_ordinal);
                let mut names = HashSet::new();
                let mut expected_base = 1u32;
                for variant in variants {
                    let name_ordinal = self.field_ordinal();
                    self.validate_identifier(&variant.name, name_ordinal);
                    if !names.insert(variant.name.as_str()) {
                        self.candidate(name_ordinal, CanonicalGraphError::DuplicateMember);
                    }
                    let base_ordinal = self.field_ordinal();
                    if variant.field_base != expected_base {
                        self.candidate(base_ordinal, CanonicalGraphError::InvalidGraph);
                    }
                    let count_ordinal = self.field_ordinal();
                    self.validate_count(variant.payload.len(), count_ordinal);
                    match u32::try_from(variant.payload.len())
                        .ok()
                        .and_then(|len| expected_base.checked_add(len))
                    {
                        Some(next) => expected_base = next,
                        None => self.candidate(count_ordinal, CanonicalGraphError::InvalidCount),
                    }
                    for &value in &variant.payload {
                        self.scan_scalar(value, &mut references, Some(node));
                    }
                }
            }
            SourceShapeNode::Tuple { elems } => {
                let count_ordinal = self.field_ordinal();
                self.validate_count(elems.len(), count_ordinal);
                for &value in elems {
                    self.scan_scalar(value, &mut references, Some(node));
                }
            }
            SourceShapeNode::Tagged(value) => match value {
                hir::TaggedType::Option(value) => {
                    self.field_ordinal();
                    self.scan_scalar(*value, &mut references, Some(node));
                }
                hir::TaggedType::Result(ok, err) => {
                    self.field_ordinal();
                    self.scan_scalar(*ok, &mut references, Some(node));
                    self.scan_scalar(*err, &mut references, Some(node));
                }
            },
            SourceShapeNode::Function {
                params,
                ret,
                return_borrow,
                return_region,
                return_cleanup,
            } => {
                let count_ordinal = self.field_ordinal();
                self.validate_count(params.len(), count_ordinal);
                for &(_, value) in params {
                    self.field_ordinal();
                    self.scan_scalar(value, &mut references, None);
                }
                self.scan_ty(*ret, &mut references, None);
                self.scan_borrow_summary(return_borrow, params.len());
                let region_ordinal = self.scan_region_summary(return_region, params.len());
                if !summaries_agree(return_borrow, return_region) {
                    self.candidate(region_ordinal, CanonicalGraphError::InvalidSummary);
                }
                let expected_cleanup = if align_sema::needs_drop_flag(
                    *ret,
                    self.view.structs,
                    self.view.tuples,
                    self.view.enums,
                    self.view.tagged_types,
                ) {
                    hir::ReturnCleanupAbi::DynamicBit
                } else {
                    hir::ReturnCleanupAbi::None
                };
                let cleanup_ordinal = self.field_ordinal();
                if return_cleanup != expected_cleanup {
                    self.candidate(cleanup_ordinal, CanonicalGraphError::InvalidGraph);
                }
            }
            SourceShapeNode::Resource(resource) => {
                for value in [
                    resource.source_name.as_str(),
                    resource.name.as_str(),
                    resource.declaring_module.as_str(),
                    resource.drop_hook.as_str(),
                    resource.drop_thunk.as_str(),
                ] {
                    let ordinal = self.field_ordinal();
                    self.validate_source_name(value, ordinal);
                }
                if resource.representation_version != 1 {
                    let ordinal = self.field_ordinal();
                    self.candidate(ordinal, CanonicalGraphError::InvalidGraph);
                } else {
                    self.field_ordinal();
                }
                if resource.drop_abi_fingerprint != *b"align-res-drop-1" {
                    let ordinal = self.field_ordinal();
                    self.candidate(ordinal, CanonicalGraphError::InvalidGraph);
                } else {
                    self.field_ordinal();
                }
                self.field_ordinal(); // generic arity
            }
        }
        let end_ordinal = self.field_ordinal();
        self.end_ordinals.insert(node, end_ordinal);
        references.reverse();
        self.pending.extend(references);
    }

    fn collect_cross_node_candidates(&mut self) {
        let mut nominal_sources = HashMap::new();
        let mut tuples: HashMap<Vec<Scalar>, Node> = HashMap::new();
        let mut known_shapes = HashSet::new();
        let order = self.order.clone();
        let view = self.view;
        for node in order {
            let Some(&end_ordinal) = self.end_ordinals.get(&node) else {
                continue;
            };
            match node {
                Node::Struct(id) => {
                    if let Some(definition) = view.structs.get(id as usize)
                        && let Some(error) = Self::compare_nominal(
                            view,
                            node,
                            &definition.source_name,
                            &mut nominal_sources,
                            &mut known_shapes,
                        )
                    {
                        self.candidate(end_ordinal, error);
                    }
                }
                Node::Enum(id) => {
                    if let Some(definition) = view.enums.get(id as usize)
                        && let Some(error) = Self::compare_nominal(
                            view,
                            node,
                            &definition.source_name,
                            &mut nominal_sources,
                            &mut known_shapes,
                        )
                    {
                        self.candidate(end_ordinal, error);
                    }
                }
                Node::Resource(id) => {
                    if let Some(definition) = view.resources.get(id as usize)
                        && let Some(error) = Self::compare_nominal(
                            view,
                            node,
                            &definition.source_name,
                            &mut nominal_sources,
                            &mut known_shapes,
                        )
                    {
                        self.candidate(end_ordinal, error);
                    }
                }
                Node::Tuple(id) => {
                    if let Some(definition) = view.tuples.get(id as usize)
                        && tuples.insert(definition.elems.clone(), node).is_some()
                    {
                        self.candidate(end_ordinal, CanonicalGraphError::DuplicateMember);
                    }
                }
                Node::Tagged(_) | Node::Fn(_) => {}
            }
        }
    }

    fn collect_inline_cycle_candidates(&mut self) {
        let mut forward = HashMap::<Node, Vec<Node>>::new();
        let mut reverse = HashMap::<Node, Vec<Node>>::new();
        for edge in &self.inline_edges {
            if self.view.source_shape_node(edge.to).is_none() {
                continue;
            }
            forward.entry(edge.from).or_default().push(edge.to);
            reverse.entry(edge.to).or_default().push(edge.from);
        }

        let mut seen = HashSet::new();
        let mut finish = Vec::with_capacity(self.order.len());
        for &start in &self.order {
            if !seen.insert(start) {
                continue;
            }
            let mut work = vec![(start, false)];
            while let Some((node, exiting)) = work.pop() {
                if exiting {
                    finish.push(node);
                    continue;
                }
                work.push((node, true));
                if let Some(children) = forward.get(&node) {
                    for &child in children.iter().rev() {
                        if seen.insert(child) {
                            work.push((child, false));
                        }
                    }
                }
            }
        }

        let mut component_by_node = HashMap::new();
        let mut component_sizes = Vec::new();
        for start in finish.into_iter().rev() {
            if component_by_node.contains_key(&start) {
                continue;
            }
            let component = component_sizes.len();
            let mut size = 0usize;
            let mut work = vec![start];
            component_by_node.insert(start, component);
            while let Some(node) = work.pop() {
                size += 1;
                if let Some(parents) = reverse.get(&node) {
                    for &parent in parents {
                        if let Entry::Vacant(entry) = component_by_node.entry(parent) {
                            entry.insert(component);
                            work.push(parent);
                        }
                    }
                }
            }
            component_sizes.push(size);
        }

        let edges = self.inline_edges.clone();
        for edge in edges {
            let Some(&component) = component_by_node.get(&edge.from) else {
                continue;
            };
            if component_by_node.get(&edge.to) == Some(&component)
                && (edge.from == edge.to || component_sizes[component] > 1)
            {
                self.candidate(edge.ordinal, CanonicalGraphError::InvalidGraph);
            }
        }
    }

    fn compare_nominal(
        view: CanonicalTypeView<'a>,
        node: Node,
        source_name: &'a str,
        nominal_sources: &mut HashMap<&'a str, Node>,
        known_shapes: &mut HashSet<(Node, Node)>,
    ) -> Option<CanonicalGraphError> {
        if source_name.is_empty() || source_name.as_bytes().contains(&0) {
            return None;
        }
        let Some(&first) = nominal_sources.get(source_name) else {
            nominal_sources.insert(source_name, node);
            return None;
        };
        let same_kind = std::mem::discriminant(&first) == std::mem::discriminant(&node);
        let same_shape = same_kind && source_shape_equal(&view, first, node, known_shapes);
        (!same_shape).then_some(CanonicalGraphError::InvalidGraph)
    }

    fn scan_scalar(
        &mut self,
        value: Scalar,
        references: &mut Vec<Node>,
        inline_from: Option<Node>,
    ) {
        let ordinal = self.field_ordinal();
        match value {
            Scalar::Struct(id) | Scalar::DynStructArray(id) | Scalar::Soa(id) => {
                let node = Node::Struct(id);
                self.scan_reference(node, ordinal, references);
                if matches!(value, Scalar::Struct(_)) {
                    self.record_inline_edge(inline_from, node, ordinal);
                }
            }
            Scalar::Enum(id) => {
                let node = Node::Enum(id);
                self.scan_reference(node, ordinal, references);
                self.record_inline_edge(inline_from, node, ordinal);
            }
            Scalar::Tagged(id) => {
                let node = Node::Tagged(id);
                self.scan_reference(node, ordinal, references);
                self.record_inline_edge(inline_from, node, ordinal);
            }
            Scalar::Fn(id) => self.scan_reference(Node::Fn(id), ordinal, references),
            Scalar::Resource(id) | Scalar::ResourceRef(id) => {
                self.scan_reference(Node::Resource(id), ordinal, references)
            }
            Scalar::Int(value) if validate_int(value.signed, value.bits).is_err() => {
                self.candidate(ordinal, CanonicalGraphError::InvalidWidth)
            }
            Scalar::Float(value) if validate_float(value.bits).is_err() => {
                self.candidate(ordinal, CanonicalGraphError::InvalidWidth)
            }
            Scalar::DynArray(value) | Scalar::Slice(value) if validate_prim(value).is_err() => {
                self.candidate(ordinal, CanonicalGraphError::InvalidWidth)
            }
            Scalar::Param(_) => self.candidate(ordinal, CanonicalGraphError::InvalidGraph),
            _ => {}
        }
    }

    fn scan_aggregate_array_elem(
        &mut self,
        value: AggregateArrayElem,
        references: &mut Vec<Node>,
    ) {
        self.field_ordinal();
        match value {
            AggregateArrayElem::Vec(value, lanes)
            | AggregateArrayElem::Mask(value, lanes) => {
                let scalar_ordinal = self.next_ordinal;
                self.scan_scalar(value, references, None);
                let lanes_ordinal = self.field_ordinal();
                if !matches!(value, Scalar::Int(_) | Scalar::Float(_)) {
                    self.candidate(scalar_ordinal, CanonicalGraphError::InvalidWidth);
                }
                if !matches!(lanes, 2 | 4 | 8 | 16) {
                    self.candidate(lanes_ordinal, CanonicalGraphError::InvalidWidth);
                }
            }
            AggregateArrayElem::FixedArray(value, length) => {
                self.scan_scalar(value, references, None);
                let length_ordinal = self.field_ordinal();
                if length == 0 {
                    self.candidate(length_ordinal, CanonicalGraphError::InvalidCount);
                }
            }
            AggregateArrayElem::FixedStructArray(id, length) => {
                let id_ordinal = self.field_ordinal();
                self.scan_reference(Node::Struct(id), id_ordinal, references);
                let length_ordinal = self.field_ordinal();
                if length == 0 {
                    self.candidate(length_ordinal, CanonicalGraphError::InvalidCount);
                }
            }
        }
    }

    fn scan_array_builder_elem(
        &mut self,
        value: ArrayBuilderElem,
        references: &mut Vec<Node>,
    ) {
        self.field_ordinal();
        match value {
            ArrayBuilderElem::Scalar(value) => self.scan_scalar(value, references, None),
            ArrayBuilderElem::Aggregate(value) => {
                self.scan_aggregate_array_elem(value, references)
            }
        }
    }

    fn scan_ty(
        &mut self,
        value: Ty,
        references: &mut Vec<Node>,
        inline_from: Option<Node>,
    ) {
        let ordinal = self.field_ordinal();
        match value {
            Ty::Option(value) => self.scan_scalar(value, references, inline_from),
            Ty::Box(value)
            | Ty::Slice(value)
            | Ty::DynArray(value)
            | Ty::Task(value) => {
                self.scan_scalar(value, references, None)
            }
            Ty::ArrayBuilder(value) => {
                self.scan_array_builder_elem(ArrayBuilderElem::Scalar(value), references)
            }
            value @ (Ty::VecArrayBuilder(..)
            | Ty::MaskArrayBuilder(..)
            | Ty::FixedArrayBuilder(..)
            | Ty::FixedStructArrayBuilder(..)) => self.scan_array_builder_elem(
                value.array_builder_element().expect("matched aggregate builder"),
                references,
            ),
            value @ (Ty::DynVecArray(..)
            | Ty::DynMaskArray(..)
            | Ty::DynFixedArray(..)
            | Ty::DynFixedStructArray(..)) => self.scan_aggregate_array_elem(
                value.dyn_aggregate_array_element().expect("matched aggregate array"),
                references,
            ),
            Ty::Result(ok, err) => {
                self.scan_scalar(ok, references, inline_from);
                self.scan_scalar(err, references, inline_from);
            }
            Ty::Array(value, _) => {
                self.scan_scalar(value, references, inline_from);
                self.field_ordinal();
            }
            Ty::Vec(value, lanes) | Ty::Mask(value, lanes) => {
                let scalar_ordinal = self.next_ordinal;
                self.scan_scalar(value, references, None);
                let lanes_ordinal = self.field_ordinal();
                if !matches!(value, Scalar::Int(_) | Scalar::Float(_)) {
                    self.candidate(scalar_ordinal, CanonicalGraphError::InvalidWidth);
                }
                if !matches!(lanes, 2 | 4 | 8 | 16) {
                    self.candidate(lanes_ordinal, CanonicalGraphError::InvalidWidth);
                }
            }
            Ty::StructArray(id, _) => {
                let node = Node::Struct(id);
                self.scan_reference(node, ordinal, references);
                self.record_inline_edge(inline_from, node, ordinal);
                self.field_ordinal();
            }
            Ty::DictEncoded(id, field) => {
                self.scan_reference(Node::Struct(id), ordinal, references);
                let field_ordinal = self.field_ordinal();
                if self
                    .view
                    .structs
                    .get(id as usize)
                    .is_some_and(|definition| field as usize >= definition.fields.len())
                {
                    self.candidate(field_ordinal, CanonicalGraphError::InvalidGraph);
                }
            }
            Ty::DynStructArray(id, _) => {
                self.scan_reference(Node::Struct(id), ordinal, references);
                self.field_ordinal();
            }
            Ty::Tagged(id) => {
                let node = Node::Tagged(id);
                self.scan_reference(node, ordinal, references);
                self.record_inline_edge(inline_from, node, ordinal);
            }
            Ty::Soa(id) | Ty::JsonScanner(id) | Ty::Struct(id) => {
                let node = Node::Struct(id);
                self.scan_reference(node, ordinal, references);
                if matches!(value, Ty::Struct(_)) {
                    self.record_inline_edge(inline_from, node, ordinal);
                }
            }
            Ty::Tuple(id) => {
                let node = Node::Tuple(id);
                self.scan_reference(node, ordinal, references);
                self.record_inline_edge(inline_from, node, ordinal);
            }
            Ty::Fn(id) => self.scan_reference(Node::Fn(id), ordinal, references),
            Ty::Resource(id) | Ty::ResourceRef(id) => {
                self.scan_reference(Node::Resource(id), ordinal, references)
            }
            Ty::Enum(id) => {
                let node = Node::Enum(id);
                self.scan_reference(node, ordinal, references);
                self.record_inline_edge(inline_from, node, ordinal);
            }
            Ty::Int(value) if validate_int(value.signed, value.bits).is_err() => {
                self.candidate(ordinal, CanonicalGraphError::InvalidWidth)
            }
            Ty::Float(value) if validate_float(value.bits).is_err() => {
                self.candidate(ordinal, CanonicalGraphError::InvalidWidth)
            }
            Ty::DynSliceArray(value) if validate_prim(value).is_err() => {
                self.candidate(ordinal, CanonicalGraphError::InvalidWidth)
            }
            Ty::Param(_) | Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Error => {
                self.candidate(ordinal, CanonicalGraphError::InvalidGraph);
            }
            _ => {}
        }
    }

    fn scan_reference(&mut self, node: Node, ordinal: u64, references: &mut Vec<Node>) {
        if self.view.source_shape_node(node).is_some() {
            references.push(node);
        } else {
            self.candidate(ordinal, CanonicalGraphError::MissingReference);
        }
    }

    fn record_inline_edge(&mut self, from: Option<Node>, to: Node, ordinal: u64) {
        if let Some(from) = from {
            self.inline_edges.push(InlineEdge { from, to, ordinal });
        }
    }

    fn scan_borrow_summary(&mut self, summary: &hir::ReturnBorrowSummary, params: usize) -> u64 {
        match summary {
            hir::ReturnBorrowSummary::None => self.field_ordinal(),
            hir::ReturnBorrowSummary::Roots {
                params: roots,
                captures,
            } => self.scan_roots(roots, captures, params),
        }
    }

    fn scan_region_summary(&mut self, summary: &hir::ReturnRegionSummary, params: usize) -> u64 {
        match summary {
            hir::ReturnRegionSummary::None => self.field_ordinal(),
            hir::ReturnRegionSummary::Roots {
                params: roots,
                captures,
            } => self.scan_roots(roots, captures, params),
        }
    }

    fn scan_roots(&mut self, roots: &[u32], captures: &[u32], params: usize) -> u64 {
        let summary_ordinal = self.field_ordinal();
        let count_ordinal = self.field_ordinal();
        self.validate_count(roots.len(), count_ordinal);
        if roots.is_empty() && captures.is_empty() {
            self.candidate(count_ordinal, CanonicalGraphError::InvalidSummary);
        }
        let mut previous = None;
        for &root in roots {
            let ordinal = self.field_ordinal();
            if previous.is_some_and(|value| value >= root) || root as usize >= params {
                self.candidate(ordinal, CanonicalGraphError::InvalidSummary);
            }
            previous = Some(root);
        }
        let captures_count = self.field_ordinal();
        self.validate_count(captures.len(), captures_count);
        let mut previous = None;
        for &capture in captures {
            let ordinal = self.field_ordinal();
            if previous.is_some_and(|value| value >= capture) {
                self.candidate(ordinal, CanonicalGraphError::InvalidSummary);
            }
            previous = Some(capture);
        }
        summary_ordinal
    }

    fn validate_source_name(&mut self, value: &str, ordinal: u64) {
        if value.as_bytes().contains(&0) {
            self.candidate(ordinal, CanonicalGraphError::EmbeddedNul);
        }
        if value.is_empty() {
            self.candidate(ordinal, CanonicalGraphError::InvalidGraph);
        }
    }

    fn validate_identifier(&mut self, value: &str, ordinal: u64) {
        if value.as_bytes().contains(&0) {
            self.candidate(ordinal, CanonicalGraphError::EmbeddedNul);
        }
        if !identifier_is_valid(value) {
            self.candidate(ordinal, CanonicalGraphError::InvalidGraph);
        }
    }

    fn validate_count(&mut self, len: usize, ordinal: u64) {
        if u32::try_from(len).is_err() {
            self.candidate(ordinal, CanonicalGraphError::InvalidCount);
        }
    }

    fn field_ordinal(&mut self) -> u64 {
        let ordinal = self.next_ordinal;
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        ordinal
    }

    fn candidate(&mut self, ordinal: u64, error: CanonicalGraphError) {
        self.candidates.push(ErrorCandidate {
            ordinal,
            tie_rank: error_tie_rank(error),
            error,
        });
    }
}

fn identifier_is_valid(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn summaries_agree(borrow: &hir::ReturnBorrowSummary, region: &hir::ReturnRegionSummary) -> bool {
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
        ) => borrow_params == region_params && borrow_captures == region_captures,
        _ => false,
    }
}

fn error_tie_rank(error: CanonicalGraphError) -> u8 {
    match error {
        CanonicalGraphError::EmbeddedNul => 0,
        CanonicalGraphError::InvalidWidth | CanonicalGraphError::InvalidCount => 1,
        CanonicalGraphError::InvalidGraph => 2,
        CanonicalGraphError::DuplicateMember => 3,
        CanonicalGraphError::MissingReference => 4,
        CanonicalGraphError::InvalidSummary => 5,
    }
}

fn validate_int(_signed: bool, bits: u8) -> Result<(), CanonicalGraphError> {
    if matches!(bits, 8 | 16 | 32 | 64) {
        Ok(())
    } else {
        Err(CanonicalGraphError::InvalidWidth)
    }
}

fn validate_float(bits: u8) -> Result<(), CanonicalGraphError> {
    if matches!(bits, 32 | 64) {
        Ok(())
    } else {
        Err(CanonicalGraphError::InvalidWidth)
    }
}

fn validate_prim(value: PrimScalar) -> Result<(), CanonicalGraphError> {
    match value {
        PrimScalar::Int(value) => validate_int(value.signed, value.bits),
        PrimScalar::Float(value) => validate_float(value.bits),
        _ => Ok(()),
    }
}

fn canonical_type_bytes(graph: &ValidatedGraph<'_>) -> Result<Vec<u8>, CanonicalGraphError> {
    let classes = stable_classes(graph)?;
    canonical_type_bytes_with_classes(graph, graph.root, &classes)
}

fn canonical_type_bytes_with_classes(
    graph: &ValidatedGraph<'_>,
    root: Ty,
    classes: &HashMap<Node, u32>,
) -> Result<Vec<u8>, CanonicalGraphError> {
    let mut representative = HashMap::new();
    for &node in &graph.order {
        let class = classes
            .get(&node)
            .copied()
            .ok_or(CanonicalGraphError::MissingReference)?;
        representative.entry(class).or_insert(node);
    }

    let mut class_order = Vec::new();
    let mut class_ordinals = HashMap::new();
    let mut pending = type_nodes(root);
    pending.reverse();
    while let Some(node) = pending.pop() {
        let class = classes
            .get(&node)
            .copied()
            .ok_or(CanonicalGraphError::MissingReference)?;
        if class_ordinals.contains_key(&class) {
            continue;
        }
        class_ordinals.insert(
            class,
            u32::try_from(class_order.len()).map_err(|_| CanonicalGraphError::InvalidCount)?,
        );
        class_order.push(class);
        let node = representative
            .get(&class)
            .copied()
            .ok_or(CanonicalGraphError::MissingReference)?;
        let mut children = node_children(graph.view, node)?;
        children.reverse();
        pending.extend(children);
    }

    let mut out = Vec::new();
    out.push(3);
    out.extend(checked_count(class_order.len())?.to_le_bytes());
    let ordinal = |node: Node| {
        let class = classes
            .get(&node)
            .ok_or(CanonicalGraphError::MissingReference)?;
        class_ordinals
            .get(class)
            .copied()
            .ok_or(CanonicalGraphError::MissingReference)
    };
    for class in class_order {
        let node = representative
            .get(&class)
            .copied()
            .ok_or(CanonicalGraphError::MissingReference)?;
        encode_node(&mut out, graph.view, node, &ordinal)?;
    }
    ty(&mut out, root, &ordinal)?;
    Ok(out)
}

impl CanonicalTy {
    pub fn from_program(root: Ty, program: &Program) -> Result<Self, CanonicalCodecError> {
        let graph = ValidatedGraph::new(root, canonical_view(program))?;
        Ok(Self(canonical_type_bytes(&graph)?.into_boxed_slice()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalCodecError> {
        let consumed = canonical_type_record_len(bytes)?;
        if consumed != bytes.len() {
            return Err(CanonicalCodecError::TrailingBytes);
        }
        Ok(Self(bytes.into()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl CanonicalFnAbi {
    pub fn from_parts(
        params: &[(ParamMode, Ty)],
        ret: Ty,
        borrow: &hir::ReturnBorrowSummary,
        region: &hir::ReturnRegionSummary,
        cleanup: hir::ReturnCleanupAbi,
        program: &Program,
    ) -> Result<Self, CanonicalCodecError> {
        let count = checked_count(params.len())?;
        validate_function_summaries(borrow, region, params.len())?;
        let mut canonical_params = Vec::with_capacity(params.len());
        for &(mode, ty) in params {
            canonical_params.push((mode, CanonicalTy::from_program(ty, program)?));
        }
        let canonical_ret = CanonicalTy::from_program(ret, program)?;
        let expected_cleanup = if align_sema::needs_drop_flag(
            ret,
            &program.structs,
            &program.tuples,
            &program.enums,
            &program.tagged_types,
        ) {
            hir::ReturnCleanupAbi::DynamicBit
        } else {
            hir::ReturnCleanupAbi::None
        };
        if cleanup != expected_cleanup {
            return Err(CanonicalCodecError::InvalidGraph);
        }
        let mut out = Vec::new();
        out.push(1);
        out.extend(count.to_le_bytes());
        for (mode, ty) in canonical_params {
            encode_param_mode(&mut out, mode)?;
            out.extend(ty.as_bytes());
        }
        out.extend(canonical_ret.as_bytes());
        encode_borrow_summary(&mut out, borrow)?;
        encode_region_summary(&mut out, region)?;
        encode_return_cleanup(&mut out, cleanup);
        Ok(Self(out.into_boxed_slice()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalCodecError> {
        if canonical_fn_abi_record_len(bytes)? != bytes.len() {
            return Err(CanonicalCodecError::TrailingBytes);
        }
        Ok(Self(bytes.into()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

fn canonical_view(program: &Program) -> CanonicalTypeView<'_> {
    CanonicalTypeView {
        structs: &program.structs,
        enums: &program.enums,
        tuples: &program.tuples,
        tagged_types: &program.tagged_types,
        fn_types: &program.fn_types,
        resources: &program.resources,
    }
}

fn validate_function_summaries(
    borrow: &hir::ReturnBorrowSummary,
    region: &hir::ReturnRegionSummary,
    params: usize,
) -> Result<(), CanonicalCodecError> {
    fn valid(roots: &[u32], captures: &[u32], params: usize) -> bool {
        u32::try_from(roots.len()).is_ok()
            && u32::try_from(captures.len()).is_ok()
            && (!roots.is_empty() || !captures.is_empty())
            && roots.windows(2).all(|pair| pair[0] < pair[1])
            && captures.windows(2).all(|pair| pair[0] < pair[1])
            && roots.iter().all(|&root| (root as usize) < params)
    }

    let borrow_valid = match borrow {
        hir::ReturnBorrowSummary::None => true,
        hir::ReturnBorrowSummary::Roots {
            params: roots,
            captures,
        } => valid(roots, captures, params),
    };
    let region_valid = match region {
        hir::ReturnRegionSummary::None => true,
        hir::ReturnRegionSummary::Roots {
            params: roots,
            captures,
        } => valid(roots, captures, params),
    };
    if borrow_valid && region_valid && summaries_agree(borrow, region) {
        Ok(())
    } else {
        Err(CanonicalCodecError::InvalidSummary)
    }
}

struct DecodeCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> DecodeCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn byte(&mut self) -> Result<u8, CanonicalCodecError> {
        let value = self
            .bytes
            .get(self.offset)
            .copied()
            .ok_or(CanonicalCodecError::Truncated)?;
        self.offset += 1;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, CanonicalCodecError> {
        let end = self
            .offset
            .checked_add(4)
            .ok_or(CanonicalCodecError::Truncated)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(CanonicalCodecError::Truncated)?;
        self.offset = end;
        Ok(u32::from_le_bytes(
            bytes
                .try_into()
                .map_err(|_| CanonicalCodecError::Truncated)?,
        ))
    }

    fn boolean(&mut self) -> Result<bool, CanonicalCodecError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CanonicalCodecError::InvalidBool),
        }
    }

    fn count(&mut self, minimum_bytes: usize) -> Result<usize, CanonicalCodecError> {
        let count = self.u32()? as usize;
        if minimum_bytes != 0
            && count > self.bytes.len().saturating_sub(self.offset) / minimum_bytes
        {
            return Err(CanonicalCodecError::Truncated);
        }
        Ok(count)
    }

    fn text(&mut self) -> Result<String, CanonicalCodecError> {
        let len = self.count(1)?;
        let end = self
            .offset
            .checked_add(len)
            .ok_or(CanonicalCodecError::Truncated)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(CanonicalCodecError::Truncated)?;
        self.offset = end;
        let value = std::str::from_utf8(bytes).map_err(|_| CanonicalCodecError::InvalidUtf8)?;
        if value.as_bytes().contains(&0) {
            return Err(CanonicalCodecError::EmbeddedNul);
        }
        Ok(value.to_owned())
    }

    fn fixed16(&mut self) -> Result<[u8; 16], CanonicalCodecError> {
        let end = self.offset.checked_add(16).ok_or(CanonicalCodecError::Truncated)?;
        let bytes = self.bytes.get(self.offset..end).ok_or(CanonicalCodecError::Truncated)?;
        self.offset = end;
        bytes.try_into().map_err(|_| CanonicalCodecError::Truncated)
    }
}

enum DecodedNode {
    Struct(hir::StructDef),
    Enum(hir::EnumDef),
    Tuple(hir::TupleDef),
    Tagged(hir::TaggedType),
    Function(FunctionTypeDef),
    Resource(hir::ResourceDef),
}

pub(super) fn canonical_type_record_len(bytes: &[u8]) -> Result<usize, CanonicalCodecError> {
    let mut cursor = DecodeCursor::new(bytes);
    if cursor.byte()? != 3 {
        return Err(CanonicalCodecError::UnsupportedVersion);
    }
    let node_count = cursor.count(1)?;
    let mut nodes = Vec::with_capacity(node_count.min(1024));
    for _ in 0..node_count {
        nodes.push(decode_node(&mut cursor)?);
    }
    let mut root = decode_ty(&mut cursor)?;
    let consumed = cursor.offset;

    let mut struct_count = 0usize;
    let mut enum_count = 0usize;
    let mut tuple_count = 0usize;
    let mut tagged_count = 0usize;
    let mut function_count = 0usize;
    let mut resource_count = 0usize;
    let mut resolved = Vec::with_capacity(nodes.len());
    for node in &nodes {
        let (tag, local) = match node {
            DecodedNode::Struct(_) => {
                let local = struct_count;
                struct_count += 1;
                (0, local)
            }
            DecodedNode::Enum(_) => {
                let local = enum_count;
                enum_count += 1;
                (1, local)
            }
            DecodedNode::Tuple(_) => {
                let local = tuple_count;
                tuple_count += 1;
                (2, local)
            }
            DecodedNode::Tagged(_) => {
                let local = tagged_count;
                tagged_count += 1;
                (3, local)
            }
            DecodedNode::Function(_) => {
                let local = function_count;
                function_count += 1;
                (4, local)
            }
            DecodedNode::Resource(_) => {
                let local = resource_count;
                resource_count += 1;
                (5, local)
            }
        };
        resolved.push((
            tag,
            checked_count(local).map_err(CanonicalCodecError::from)?,
        ));
    }

    let mut structs = Vec::with_capacity(struct_count);
    let mut enums = Vec::with_capacity(enum_count);
    let mut tuples = Vec::with_capacity(tuple_count);
    let mut tagged_types = Vec::with_capacity(tagged_count);
    let mut fn_types = Vec::with_capacity(function_count);
    let mut resources = Vec::with_capacity(resource_count);
    for mut node in nodes {
        remap_decoded_node(&mut node, &resolved)?;
        match node {
            DecodedNode::Struct(value) => structs.push(value),
            DecodedNode::Enum(value) => enums.push(value),
            DecodedNode::Tuple(value) => tuples.push(value),
            DecodedNode::Tagged(value) => tagged_types.push(value),
            DecodedNode::Function(value) => fn_types.push(value),
            DecodedNode::Resource(value) => resources.push(value),
        }
    }
    remap_decoded_ty(&mut root, &resolved)?;

    let view = CanonicalTypeView {
        structs: &structs,
        enums: &enums,
        tuples: &tuples,
        tagged_types: &tagged_types,
        fn_types: &fn_types,
        resources: &resources,
    };
    let mut roots = Vec::with_capacity(resolved.len() + 1);
    for &(tag, local) in &resolved {
        roots.push(node_root_ty(tag, local)?);
    }
    roots.push(root);
    let graph = ValidatedGraph::new_many(root, &roots, view)?;
    let classes = stable_classes(&graph)?;
    let unique_classes: HashSet<u32> = classes.values().copied().collect();
    if unique_classes.len() != graph.order.len() {
        return Err(CanonicalCodecError::DuplicateMember);
    }
    let canonical = canonical_type_bytes_with_classes(&graph, root, &classes)?;
    if canonical.as_slice() != &bytes[..consumed] {
        return Err(CanonicalCodecError::NonCanonicalOrder);
    }
    Ok(consumed)
}

fn node_root_ty(tag: u8, id: u32) -> Result<Ty, CanonicalCodecError> {
    match tag {
        0 => Ok(Ty::Struct(id)),
        1 => Ok(Ty::Enum(id)),
        2 => Ok(Ty::Tuple(id)),
        3 => Ok(Ty::Tagged(id)),
        4 => Ok(Ty::Fn(id)),
        5 => Ok(Ty::Resource(id)),
        _ => Err(CanonicalCodecError::UnknownTag),
    }
}

fn decode_node(cursor: &mut DecodeCursor<'_>) -> Result<DecodedNode, CanonicalCodecError> {
    match cursor.byte()? {
        0 => {
            let source_name = cursor.text()?;
            let align = match cursor.byte()? {
                0 => None,
                1 => Some(cursor.u32()?),
                _ => return Err(CanonicalCodecError::UnknownTag),
            };
            let c_repr = cursor.boolean()?;
            let count = cursor.count(5)?;
            let mut fields = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                fields.push(hir::FieldDef {
                    name: cursor.text()?,
                    ty: decode_ty(cursor)?,
                });
            }
            Ok(DecodedNode::Struct(hir::StructDef {
                name: source_name.clone(),
                source_name,
                fields,
                align,
                c_repr,
            }))
        }
        1 => {
            let source_name = cursor.text()?;
            let count = cursor.count(12)?;
            let mut variants = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                let name = cursor.text()?;
                let field_base = cursor.u32()?;
                let payload_count = cursor.count(1)?;
                let mut payload = Vec::with_capacity(payload_count.min(1024));
                for _ in 0..payload_count {
                    payload.push(decode_scalar(cursor)?);
                }
                variants.push(hir::EnumVariant {
                    name,
                    payload,
                    field_base,
                });
            }
            Ok(DecodedNode::Enum(hir::EnumDef {
                name: source_name.clone(),
                source_name,
                variants,
            }))
        }
        2 => {
            let count = cursor.count(1)?;
            let mut elems = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                elems.push(decode_scalar(cursor)?);
            }
            Ok(DecodedNode::Tuple(hir::TupleDef { elems }))
        }
        3 => match cursor.byte()? {
            0 => Ok(DecodedNode::Tagged(hir::TaggedType::Option(decode_scalar(
                cursor,
            )?))),
            1 => Ok(DecodedNode::Tagged(hir::TaggedType::Result(
                decode_scalar(cursor)?,
                decode_scalar(cursor)?,
            ))),
            _ => Err(CanonicalCodecError::UnknownTag),
        },
        4 => {
            let count = cursor.count(2)?;
            let mut params = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                params.push((decode_param_mode(cursor)?, decode_scalar(cursor)?));
            }
            Ok(DecodedNode::Function(FunctionTypeDef {
                params,
                ret: decode_ty(cursor)?,
                return_borrow: decode_borrow_summary(cursor)?,
                return_region: decode_region_summary(cursor)?,
                return_cleanup: decode_return_cleanup(cursor)?,
            }))
        }
        5 => {
            let source_name = cursor.text()?;
            let name = cursor.text()?;
            let declaring_module = cursor.text()?;
            let drop_hook = cursor.text()?;
            let drop_thunk = cursor.text()?;
            let representation_version = cursor.u32()?;
            let drop_abi_fingerprint = cursor.fixed16()?;
            let generic_arity = cursor.u32()?;
            Ok(DecodedNode::Resource(hir::ResourceDef {
                name,
                source_name,
                declaring_module,
                generic_arity,
                drop_hook,
                drop_thunk,
                representation_version,
                drop_abi_fingerprint,
            }))
        }
        _ => Err(CanonicalCodecError::UnknownTag),
    }
}

fn decode_param_mode(cursor: &mut DecodeCursor<'_>) -> Result<ParamMode, CanonicalCodecError> {
    match cursor.byte()? {
        0 => Ok(ParamMode::ByValue),
        1 => Ok(ParamMode::Out),
        2 => Ok(ParamMode::Borrow),
        3 => Ok(ParamMode::BorrowMut),
        _ => Err(CanonicalCodecError::UnknownTag),
    }
}

fn decode_int(cursor: &mut DecodeCursor<'_>) -> Result<align_sema::IntTy, CanonicalCodecError> {
    let signed = cursor.boolean()?;
    let bits = cursor.byte()?;
    if !matches!(bits, 8 | 16 | 32 | 64) {
        return Err(CanonicalCodecError::InvalidWidth);
    }
    Ok(align_sema::IntTy { signed, bits })
}

fn decode_float(cursor: &mut DecodeCursor<'_>) -> Result<align_sema::FloatTy, CanonicalCodecError> {
    let bits = cursor.byte()?;
    if !matches!(bits, 32 | 64) {
        return Err(CanonicalCodecError::InvalidWidth);
    }
    Ok(align_sema::FloatTy { bits })
}

fn decode_prim(cursor: &mut DecodeCursor<'_>) -> Result<PrimScalar, CanonicalCodecError> {
    match cursor.byte()? {
        0 => Ok(PrimScalar::Int(decode_int(cursor)?)),
        1 => Ok(PrimScalar::Float(decode_float(cursor)?)),
        2 => Ok(PrimScalar::Bool),
        3 => Ok(PrimScalar::Char),
        4 => Ok(PrimScalar::Str),
        5 => Ok(PrimScalar::String),
        _ => Err(CanonicalCodecError::UnknownTag),
    }
}

fn decode_scalar(cursor: &mut DecodeCursor<'_>) -> Result<Scalar, CanonicalCodecError> {
    let node = |cursor: &mut DecodeCursor<'_>| cursor.u32();
    match cursor.byte()? {
        0 => Ok(Scalar::Int(decode_int(cursor)?)),
        1 => Ok(Scalar::Float(decode_float(cursor)?)),
        2 => Ok(Scalar::Bool),
        3 => Ok(Scalar::Char),
        4 => Ok(Scalar::Unit),
        5 => Ok(Scalar::Struct(node(cursor)?)),
        6 => Ok(Scalar::String),
        7 => Ok(Scalar::DynArray(decode_prim(cursor)?)),
        8 => Ok(Scalar::DynStructArray(node(cursor)?)),
        9 => Ok(Scalar::DynResponseArray),
        10 => Ok(Scalar::Str),
        11 => Ok(Scalar::Slice(decode_prim(cursor)?)),
        12 => Ok(Scalar::Enum(node(cursor)?)),
        13 => Ok(Scalar::Tagged(node(cursor)?)),
        14 => Ok(Scalar::Soa(node(cursor)?)),
        15 => Ok(Scalar::JsonDoc),
        16 => Ok(Scalar::Reader),
        17 => Ok(Scalar::Writer),
        18 => Ok(Scalar::Buffer),
        19 => Ok(Scalar::Regex),
        20 => Ok(Scalar::Captures),
        21 => Ok(Scalar::CliParsed),
        22 => Ok(Scalar::TcpConn),
        23 => Ok(Scalar::TcpListener),
        24 => Ok(Scalar::UdpSocket),
        25 => Ok(Scalar::Child),
        26 => Ok(Scalar::File),
        27 => Ok(Scalar::HttpResponse),
        28 => Ok(Scalar::HttpServer),
        29 => Ok(Scalar::HttpRequestCtx),
        30 => Ok(Scalar::ResponseBuilder),
        31 => Ok(Scalar::HttpStream),
        32 => Ok(Scalar::RunOutput),
        33 => Ok(Scalar::Fn(node(cursor)?)),
        34 => Ok(Scalar::Resource(node(cursor)?)),
        35 => Ok(Scalar::ResourceRef(node(cursor)?)),
        36 => Ok(Scalar::RunBytes),
        _ => Err(CanonicalCodecError::UnknownTag),
    }
}

fn decode_aggregate_array_elem(
    cursor: &mut DecodeCursor<'_>,
) -> Result<AggregateArrayElem, CanonicalCodecError> {
    let tag = cursor.byte()?;
    let elem = match tag {
        0 | 1 => {
            let scalar = decode_scalar(cursor)?;
            let lanes = cursor.u32()?;
            if !matches!(scalar, Scalar::Int(_) | Scalar::Float(_))
                || !matches!(lanes, 2 | 4 | 8 | 16)
            {
                return Err(CanonicalCodecError::InvalidWidth);
            }
            if tag == 0 {
                AggregateArrayElem::Vec(scalar, lanes)
            } else {
                AggregateArrayElem::Mask(scalar, lanes)
            }
        }
        2 => {
            let scalar = decode_scalar(cursor)?;
            let length = cursor.u32()?;
            if length == 0 {
                return Err(CanonicalCodecError::InvalidCount);
            }
            AggregateArrayElem::FixedArray(scalar, length)
        }
        3 => {
            let id = cursor.u32()?;
            let length = cursor.u32()?;
            if length == 0 {
                return Err(CanonicalCodecError::InvalidCount);
            }
            AggregateArrayElem::FixedStructArray(id, length)
        }
        _ => return Err(CanonicalCodecError::UnknownTag),
    };
    Ok(elem)
}

fn decode_array_builder_elem(
    cursor: &mut DecodeCursor<'_>,
) -> Result<ArrayBuilderElem, CanonicalCodecError> {
    match cursor.byte()? {
        0 => Ok(ArrayBuilderElem::Scalar(decode_scalar(cursor)?)),
        1 => Ok(ArrayBuilderElem::Aggregate(decode_aggregate_array_elem(
            cursor,
        )?)),
        _ => Err(CanonicalCodecError::UnknownTag),
    }
}

fn decode_ty(cursor: &mut DecodeCursor<'_>) -> Result<Ty, CanonicalCodecError> {
    let node = |cursor: &mut DecodeCursor<'_>| cursor.u32();
    let tag = cursor.byte()?;
    match tag {
        0 => Ok(Ty::Int(decode_int(cursor)?)),
        1 => Ok(Ty::Float(decode_float(cursor)?)),
        2 => Ok(Ty::Bool),
        3 => Ok(Ty::Char),
        4 => Ok(Ty::Option(decode_scalar(cursor)?)),
        5 => Ok(Ty::Result(decode_scalar(cursor)?, decode_scalar(cursor)?)),
        6 => Ok(Ty::Tagged(node(cursor)?)),
        7 => Ok(Ty::Box(decode_scalar(cursor)?)),
        8 => Ok(Ty::Array(decode_scalar(cursor)?, cursor.u32()?)),
        9 | 10 => {
            let scalar = decode_scalar(cursor)?;
            let lanes = cursor.u32()?;
            if !matches!(scalar, Scalar::Int(_) | Scalar::Float(_))
                || !matches!(lanes, 2 | 4 | 8 | 16)
            {
                return Err(CanonicalCodecError::InvalidWidth);
            }
            if tag == 9 {
                Ok(Ty::Vec(scalar, lanes))
            } else {
                Ok(Ty::Mask(scalar, lanes))
            }
        }
        11 => Ok(Ty::StructArray(node(cursor)?, cursor.u32()?)),
        12 => {
            let id = node(cursor)?;
            let layout = match cursor.byte()? {
                0 => Layout::Aos,
                1 => Layout::Soa,
                _ => return Err(CanonicalCodecError::UnknownTag),
            };
            Ok(Ty::DynStructArray(id, layout))
        }
        13 => Ok(Ty::Slice(decode_scalar(cursor)?)),
        14 => Ok(Ty::Soa(node(cursor)?)),
        15 => Ok(Ty::DynSliceArray(decode_prim(cursor)?)),
        16 => Ok(Ty::DynArray(decode_scalar(cursor)?)),
        17 => Ok(Ty::DynResponseArray),
        18 => Ok(Ty::Str),
        19 => Ok(Ty::String),
        20 => Ok(Ty::ArenaHandle),
        21 => Ok(Ty::Raw),
        22 => Ok(Ty::Builder),
        23 => Ok(Ty::Writer),
        24 => Ok(Ty::Reader),
        25 => Ok(Ty::Buffer),
        26 => Ok(Ty::array_builder(decode_array_builder_elem(cursor)?)),
        27 => Ok(Ty::StrFinder),
        28 => Ok(Ty::File),
        29 => Ok(Ty::Rng),
        30 => Ok(Ty::Regex),
        31 => Ok(Ty::Captures),
        32 => Ok(Ty::CliCommand),
        33 => Ok(Ty::CliParsed),
        34 => Ok(Ty::TcpConn),
        35 => Ok(Ty::TcpListener),
        36 => Ok(Ty::UdpSocket),
        37 => Ok(Ty::Child),
        38 => Ok(Ty::Command),
        39 => Ok(Ty::RunOutput),
        40 => Ok(Ty::HttpRequest),
        41 => Ok(Ty::HttpResponse),
        42 => Ok(Ty::HttpClient),
        43 => Ok(Ty::HttpServer),
        44 => Ok(Ty::HttpRequestCtx),
        45 => Ok(Ty::ResponseBuilder),
        46 => Ok(Ty::HttpStream),
        47 => Ok(Ty::HttpHeaders),
        48 => Ok(Ty::JsonDoc),
        49 => Ok(Ty::JsonScanner(node(cursor)?)),
        50 => Ok(Ty::Struct(node(cursor)?)),
        51 => Ok(Ty::Tuple(node(cursor)?)),
        52 => Ok(Ty::Fn(node(cursor)?)),
        53 => Ok(Ty::Enum(node(cursor)?)),
        54 => Ok(Ty::Task(decode_scalar(cursor)?)),
        55 => Ok(Ty::DictEncoded(node(cursor)?, cursor.u32()?)),
        56 => Ok(Ty::Unit),
        57 => Ok(Ty::Resource(node(cursor)?)),
        58 => Ok(Ty::ResourceRef(node(cursor)?)),
        59 => Ok(Ty::dyn_aggregate_array(decode_aggregate_array_elem(cursor)?)),
        60 => Ok(Ty::RunBytes),
        _ => Err(CanonicalCodecError::UnknownTag),
    }
}

fn decode_borrow_summary(
    cursor: &mut DecodeCursor<'_>,
) -> Result<hir::ReturnBorrowSummary, CanonicalCodecError> {
    match cursor.byte()? {
        0 => Ok(hir::ReturnBorrowSummary::None),
        1 => {
            let (params, captures) = decode_roots(cursor)?;
            Ok(hir::ReturnBorrowSummary::Roots { params, captures })
        }
        _ => Err(CanonicalCodecError::UnknownTag),
    }
}

fn decode_region_summary(
    cursor: &mut DecodeCursor<'_>,
) -> Result<hir::ReturnRegionSummary, CanonicalCodecError> {
    match cursor.byte()? {
        0 => Ok(hir::ReturnRegionSummary::None),
        1 => {
            let (params, captures) = decode_roots(cursor)?;
            Ok(hir::ReturnRegionSummary::Roots { params, captures })
        }
        _ => Err(CanonicalCodecError::UnknownTag),
    }
}

fn decode_roots(
    cursor: &mut DecodeCursor<'_>,
) -> Result<(Vec<u32>, Vec<u32>), CanonicalCodecError> {
    let param_count = cursor.count(4)?;
    let mut params = Vec::with_capacity(param_count.min(1024));
    for _ in 0..param_count {
        params.push(cursor.u32()?);
    }
    let capture_count = cursor.count(4)?;
    let mut captures = Vec::with_capacity(capture_count.min(1024));
    for _ in 0..capture_count {
        captures.push(cursor.u32()?);
    }
    Ok((params, captures))
}

fn resolve_decoded_node(
    global: u32,
    expected_tag: u8,
    resolved: &[(u8, u32)],
) -> Result<u32, CanonicalCodecError> {
    let &(tag, local) = resolved
        .get(global as usize)
        .ok_or(CanonicalCodecError::MissingReference)?;
    if tag == expected_tag {
        Ok(local)
    } else {
        Err(CanonicalCodecError::MissingReference)
    }
}

fn remap_decoded_scalar(
    value: &mut Scalar,
    resolved: &[(u8, u32)],
) -> Result<(), CanonicalCodecError> {
    let (id, tag) = match value {
        Scalar::Struct(id) | Scalar::DynStructArray(id) | Scalar::Soa(id) => (id, 0),
        Scalar::Enum(id) => (id, 1),
        Scalar::Tagged(id) => (id, 3),
        Scalar::Fn(id) => (id, 4),
        Scalar::Resource(id) | Scalar::ResourceRef(id) => (id, 5),
        _ => return Ok(()),
    };
    *id = resolve_decoded_node(*id, tag, resolved)?;
    Ok(())
}

fn remap_decoded_ty(value: &mut Ty, resolved: &[(u8, u32)]) -> Result<(), CanonicalCodecError> {
    match value {
        Ty::Option(value)
        | Ty::Box(value)
        | Ty::Slice(value)
        | Ty::DynArray(value)
        | Ty::Task(value)
        | Ty::Array(value, _)
        | Ty::Vec(value, _)
        | Ty::Mask(value, _) => remap_decoded_scalar(value, resolved),
        Ty::ArrayBuilder(value) => remap_decoded_scalar(value, resolved),
        Ty::VecArrayBuilder(value, _)
        | Ty::MaskArrayBuilder(value, _)
        | Ty::FixedArrayBuilder(value, _)
        | Ty::DynVecArray(value, _)
        | Ty::DynMaskArray(value, _)
        | Ty::DynFixedArray(value, _) => remap_decoded_scalar(value, resolved),
        Ty::FixedStructArrayBuilder(id, _) | Ty::DynFixedStructArray(id, _) => {
            *id = resolve_decoded_node(*id, 0, resolved)?;
            Ok(())
        }
        Ty::Result(ok, err) => {
            remap_decoded_scalar(ok, resolved)?;
            remap_decoded_scalar(err, resolved)
        }
        Ty::Tagged(id) => {
            *id = resolve_decoded_node(*id, 3, resolved)?;
            Ok(())
        }
        Ty::StructArray(id, _)
        | Ty::DynStructArray(id, _)
        | Ty::Soa(id)
        | Ty::JsonScanner(id)
        | Ty::DictEncoded(id, _)
        | Ty::Struct(id) => {
            *id = resolve_decoded_node(*id, 0, resolved)?;
            Ok(())
        }
        Ty::Tuple(id) => {
            *id = resolve_decoded_node(*id, 2, resolved)?;
            Ok(())
        }
        Ty::Fn(id) => {
            *id = resolve_decoded_node(*id, 4, resolved)?;
            Ok(())
        }
        Ty::Enum(id) => {
            *id = resolve_decoded_node(*id, 1, resolved)?;
            Ok(())
        }
        Ty::Resource(id) | Ty::ResourceRef(id) => {
            *id = resolve_decoded_node(*id, 5, resolved)?;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn remap_decoded_node(
    node: &mut DecodedNode,
    resolved: &[(u8, u32)],
) -> Result<(), CanonicalCodecError> {
    match node {
        DecodedNode::Struct(definition) => {
            for field in &mut definition.fields {
                remap_decoded_ty(&mut field.ty, resolved)?;
            }
        }
        DecodedNode::Enum(definition) => {
            for variant in &mut definition.variants {
                for scalar in &mut variant.payload {
                    remap_decoded_scalar(scalar, resolved)?;
                }
            }
        }
        DecodedNode::Tuple(definition) => {
            for scalar in &mut definition.elems {
                remap_decoded_scalar(scalar, resolved)?;
            }
        }
        DecodedNode::Tagged(definition) => match definition {
            hir::TaggedType::Option(value) => remap_decoded_scalar(value, resolved)?,
            hir::TaggedType::Result(ok, err) => {
                remap_decoded_scalar(ok, resolved)?;
                remap_decoded_scalar(err, resolved)?;
            }
        },
        DecodedNode::Function(definition) => {
            for (_, scalar) in &mut definition.params {
                remap_decoded_scalar(scalar, resolved)?;
            }
            remap_decoded_ty(&mut definition.ret, resolved)?;
        }
        DecodedNode::Resource(_) => {}
    }
    Ok(())
}

fn decode_nested_canonical_type(cursor: &mut DecodeCursor<'_>) -> Result<(), CanonicalCodecError> {
    let consumed = canonical_type_record_len(
        cursor
            .bytes
            .get(cursor.offset..)
            .ok_or(CanonicalCodecError::Truncated)?,
    )?;
    cursor.offset = cursor
        .offset
        .checked_add(consumed)
        .ok_or(CanonicalCodecError::Truncated)?;
    Ok(())
}

pub(super) fn canonical_fn_abi_record_len(bytes: &[u8]) -> Result<usize, CanonicalCodecError> {
    let mut cursor = DecodeCursor::new(bytes);
    if cursor.byte()? != 1 {
        return Err(CanonicalCodecError::UnsupportedVersion);
    }
    let count = cursor.count(7)?;
    for _ in 0..count {
        let _mode = decode_param_mode(&mut cursor)?;
        decode_nested_canonical_type(&mut cursor)?;
    }
    decode_nested_canonical_type(&mut cursor)?;
    let borrow = decode_borrow_summary(&mut cursor)?;
    let region = decode_region_summary(&mut cursor)?;
    decode_return_cleanup(&mut cursor)?;
    validate_function_summaries(&borrow, &region, count)?;
    Ok(cursor.offset)
}

fn stable_classes(graph: &ValidatedGraph<'_>) -> Result<HashMap<Node, u32>, CanonicalGraphError> {
    stable_classes_and_rounds(graph).map(|(classes, _)| classes)
}

fn stable_classes_and_rounds(
    graph: &ValidatedGraph<'_>,
) -> Result<(HashMap<Node, u32>, usize), CanonicalGraphError> {
    stable_classes_observed(graph, &mut ())
}

fn stable_classes_observed<O: RefinementObserver>(
    graph: &ValidatedGraph<'_>,
    observer: &mut O,
) -> Result<(HashMap<Node, u32>, usize), CanonicalGraphError> {
    let anonymous_nodes = graph
        .order
        .iter()
        .filter(|node| matches!(node, Node::Tuple(_) | Node::Tagged(_) | Node::Fn(_)))
        .count();
    let mut classes = assign_classes(
        graph,
        graph
            .order
            .iter()
            .map(|&node| initial_signature(graph.view, node))
            .collect::<Result<Vec<_>, _>>()?,
        observer,
    )?;
    for round in 1..=anonymous_nodes + 1 {
        let signatures = graph
            .order
            .iter()
            .map(|&node| {
                let mut bytes = Vec::new();
                encode_node(&mut bytes, graph.view, node, &|child| {
                    classes
                        .get(&child)
                        .copied()
                        .ok_or(CanonicalGraphError::MissingReference)
                })?;
                Ok(bytes)
            })
            .collect::<Result<Vec<_>, CanonicalGraphError>>()?;
        let next = assign_classes(graph, signatures, observer)?;
        if same_partition(&graph.order, &classes, &next) {
            return Ok((next, round));
        }
        classes = next;
    }
    Err(CanonicalGraphError::InvalidGraph)
}

fn assign_classes(
    graph: &ValidatedGraph<'_>,
    signatures: Vec<Vec<u8>>,
    observer: &mut impl RefinementObserver,
) -> Result<HashMap<Node, u32>, CanonicalGraphError> {
    if signatures.len() != graph.order.len() {
        return Err(CanonicalGraphError::InvalidGraph);
    }
    for signature in &signatures {
        observer.signature(signature.len());
    }
    let mut unique = signatures.clone();
    unique.sort_by(|left, right| observer.compare(left, right));
    unique.dedup();
    let mut class_by_signature = HashMap::new();
    for (class, signature) in unique.into_iter().enumerate() {
        class_by_signature.insert(signature, checked_count(class)?);
    }
    graph
        .order
        .iter()
        .copied()
        .zip(signatures)
        .map(|(node, signature)| {
            class_by_signature
                .get(&signature)
                .copied()
                .map(|class| (node, class))
                .ok_or(CanonicalGraphError::InvalidGraph)
        })
        .collect()
}

trait RefinementObserver {
    fn signature(&mut self, _bytes: usize) {}

    fn compare(&mut self, left: &[u8], right: &[u8]) -> std::cmp::Ordering {
        left.cmp(right)
    }
}

impl RefinementObserver for () {}

fn same_partition(order: &[Node], left: &HashMap<Node, u32>, right: &HashMap<Node, u32>) -> bool {
    let mut left_to_right = HashMap::new();
    let mut right_to_left = HashMap::new();
    order.iter().all(|node| {
        let (Some(&left), Some(&right)) = (left.get(node), right.get(node)) else {
            return false;
        };
        if left_to_right
            .get(&left)
            .is_some_and(|mapped| *mapped != right)
            || right_to_left
                .get(&right)
                .is_some_and(|mapped| *mapped != left)
        {
            return false;
        }
        left_to_right.insert(left, right);
        right_to_left.insert(right, left);
        true
    })
}

fn initial_signature(
    view: CanonicalTypeView<'_>,
    node: Node,
) -> Result<Vec<u8>, CanonicalGraphError> {
    let mut out = vec![match node {
        Node::Struct(_) => 0,
        Node::Enum(_) => 1,
        Node::Tuple(_) => 2,
        Node::Tagged(_) => 3,
        Node::Fn(_) => 4,
        Node::Resource(_) => 5,
    }];
    match node {
        Node::Struct(id) => text(
            &mut out,
            &view
                .structs
                .get(id as usize)
                .ok_or(CanonicalGraphError::MissingReference)?
                .source_name,
        )?,
        Node::Enum(id) => text(
            &mut out,
            &view
                .enums
                .get(id as usize)
                .ok_or(CanonicalGraphError::MissingReference)?
                .source_name,
        )?,
        Node::Resource(id) => text(
            &mut out,
            &view
                .resources
                .get(id as usize)
                .ok_or(CanonicalGraphError::MissingReference)?
                .source_name,
        )?,
        Node::Tuple(_) | Node::Tagged(_) | Node::Fn(_) => {}
    }
    Ok(out)
}

fn encode_node(
    out: &mut Vec<u8>,
    view: CanonicalTypeView<'_>,
    node: Node,
    ordinal: &impl Fn(Node) -> Result<u32, CanonicalGraphError>,
) -> Result<(), CanonicalGraphError> {
    append_transactional(out, |out| match node {
        Node::Struct(id) => {
            let definition = view
                .structs
                .get(id as usize)
                .ok_or(CanonicalGraphError::MissingReference)?;
            out.push(0);
            text(out, &definition.source_name)?;
            match definition.align {
                None => out.push(0),
                Some(align) => {
                    out.push(1);
                    out.extend(align.to_le_bytes());
                }
            }
            out.push(u8::from(definition.c_repr));
            out.extend(checked_count(definition.fields.len())?.to_le_bytes());
            for field in &definition.fields {
                text(out, &field.name)?;
                ty(out, field.ty, ordinal)?;
            }
            Ok(())
        }
        Node::Enum(id) => {
            let definition = view
                .enums
                .get(id as usize)
                .ok_or(CanonicalGraphError::MissingReference)?;
            out.push(1);
            text(out, &definition.source_name)?;
            out.extend(checked_count(definition.variants.len())?.to_le_bytes());
            for variant in &definition.variants {
                text(out, &variant.name)?;
                out.extend(variant.field_base.to_le_bytes());
                out.extend(checked_count(variant.payload.len())?.to_le_bytes());
                for &value in &variant.payload {
                    scalar(out, value, ordinal)?;
                }
            }
            Ok(())
        }
        Node::Tuple(id) => {
            let definition = view
                .tuples
                .get(id as usize)
                .ok_or(CanonicalGraphError::MissingReference)?;
            out.push(2);
            out.extend(checked_count(definition.elems.len())?.to_le_bytes());
            for &value in &definition.elems {
                scalar(out, value, ordinal)?;
            }
            Ok(())
        }
        Node::Tagged(id) => {
            let definition = view
                .tagged_types
                .get(id as usize)
                .ok_or(CanonicalGraphError::MissingReference)?;
            out.push(3);
            match definition {
                hir::TaggedType::Option(value) => {
                    out.push(0);
                    scalar(out, *value, ordinal)?;
                }
                hir::TaggedType::Result(ok, err) => {
                    out.push(1);
                    scalar(out, *ok, ordinal)?;
                    scalar(out, *err, ordinal)?;
                }
            }
            Ok(())
        }
        Node::Fn(id) => {
            let definition = view
                .fn_types
                .get(id as usize)
                .ok_or(CanonicalGraphError::MissingReference)?;
            out.push(4);
            out.extend(checked_count(definition.params.len())?.to_le_bytes());
            for &(mode, value) in &definition.params {
                encode_param_mode(out, mode)?;
                scalar(out, value, ordinal)?;
            }
            ty(out, definition.ret, ordinal)?;
            encode_borrow_summary(out, &definition.return_borrow)?;
            encode_region_summary(out, &definition.return_region)?;
            encode_return_cleanup(out, definition.return_cleanup);
            Ok(())
        }
        Node::Resource(id) => {
            let definition = view
                .resources
                .get(id as usize)
                .ok_or(CanonicalGraphError::MissingReference)?;
            out.push(5);
            text(out, &definition.source_name)?;
            text(out, &definition.name)?;
            text(out, &definition.declaring_module)?;
            text(out, &definition.drop_hook)?;
            text(out, &definition.drop_thunk)?;
            out.extend(definition.representation_version.to_le_bytes());
            out.extend(definition.drop_abi_fingerprint);
            out.extend(definition.generic_arity.to_le_bytes());
            Ok(())
        }
    })
}

fn encode_return_cleanup(out: &mut Vec<u8>, value: hir::ReturnCleanupAbi) {
    out.push(match value {
        hir::ReturnCleanupAbi::None => 0,
        hir::ReturnCleanupAbi::DynamicBit => 1,
    });
}

fn decode_return_cleanup(
    cursor: &mut DecodeCursor<'_>,
) -> Result<hir::ReturnCleanupAbi, CanonicalCodecError> {
    match cursor.byte()? {
        0 => Ok(hir::ReturnCleanupAbi::None),
        1 => Ok(hir::ReturnCleanupAbi::DynamicBit),
        _ => Err(CanonicalCodecError::UnknownTag),
    }
}

fn encode_borrow_summary(
    out: &mut Vec<u8>,
    value: &hir::ReturnBorrowSummary,
) -> Result<(), CanonicalGraphError> {
    match value {
        hir::ReturnBorrowSummary::None => out.push(0),
        hir::ReturnBorrowSummary::Roots { params, captures } => {
            out.push(1);
            encode_roots(out, params, captures)?;
        }
    }
    Ok(())
}

fn encode_region_summary(
    out: &mut Vec<u8>,
    value: &hir::ReturnRegionSummary,
) -> Result<(), CanonicalGraphError> {
    match value {
        hir::ReturnRegionSummary::None => out.push(0),
        hir::ReturnRegionSummary::Roots { params, captures } => {
            out.push(1);
            encode_roots(out, params, captures)?;
        }
    }
    Ok(())
}

fn encode_roots(
    out: &mut Vec<u8>,
    params: &[u32],
    captures: &[u32],
) -> Result<(), CanonicalGraphError> {
    out.extend(checked_count(params.len())?.to_le_bytes());
    for value in params {
        out.extend(value.to_le_bytes());
    }
    out.extend(checked_count(captures.len())?.to_le_bytes());
    for value in captures {
        out.extend(value.to_le_bytes());
    }
    Ok(())
}

fn node_children(
    view: CanonicalTypeView<'_>,
    node: Node,
) -> Result<Vec<Node>, CanonicalGraphError> {
    let shape = view
        .source_shape_node(node)
        .ok_or(CanonicalGraphError::MissingReference)?;
    let mut children = Vec::new();
    match shape {
        SourceShapeNode::Struct { fields, .. } => {
            for field in fields {
                children.extend(type_nodes(field.ty));
            }
        }
        SourceShapeNode::Enum { variants, .. } => {
            for variant in variants {
                for &value in &variant.payload {
                    children.extend(scalar_nodes(value));
                }
            }
        }
        SourceShapeNode::Tuple { elems } => {
            for &value in elems {
                children.extend(scalar_nodes(value));
            }
        }
        SourceShapeNode::Tagged(value) => match value {
            hir::TaggedType::Option(value) => children.extend(scalar_nodes(*value)),
            hir::TaggedType::Result(ok, err) => {
                children.extend(scalar_nodes(*ok));
                children.extend(scalar_nodes(*err));
            }
        },
        SourceShapeNode::Function { params, ret, .. } => {
            for &(_, value) in params {
                children.extend(scalar_nodes(value));
            }
            children.extend(type_nodes(*ret));
        }
        SourceShapeNode::Resource(_) => {}
    }
    Ok(children)
}

fn scalar_nodes(value: Scalar) -> Vec<Node> {
    match value {
        Scalar::Struct(id) | Scalar::DynStructArray(id) | Scalar::Soa(id) => {
            vec![Node::Struct(id)]
        }
        Scalar::Enum(id) => vec![Node::Enum(id)],
        Scalar::Tagged(id) => vec![Node::Tagged(id)],
        Scalar::Fn(id) => vec![Node::Fn(id)],
        Scalar::Resource(id) | Scalar::ResourceRef(id) => vec![Node::Resource(id)],
        _ => Vec::new(),
    }
}

fn type_nodes(value: Ty) -> Vec<Node> {
    match value {
        Ty::Option(value)
        | Ty::Box(value)
        | Ty::Slice(value)
        | Ty::DynArray(value)
        | Ty::Task(value)
        | Ty::Array(value, _)
        | Ty::Vec(value, _)
        | Ty::Mask(value, _) => scalar_nodes(value),
        Ty::ArrayBuilder(value) => scalar_nodes(value),
        Ty::VecArrayBuilder(value, _)
        | Ty::MaskArrayBuilder(value, _)
        | Ty::FixedArrayBuilder(value, _)
        | Ty::DynVecArray(value, _)
        | Ty::DynMaskArray(value, _)
        | Ty::DynFixedArray(value, _) => scalar_nodes(value),
        Ty::FixedStructArrayBuilder(id, _) | Ty::DynFixedStructArray(id, _) => {
            vec![Node::Struct(id)]
        }
        Ty::Result(ok, err) => {
            let mut nodes = scalar_nodes(ok);
            nodes.extend(scalar_nodes(err));
            nodes
        }
        Ty::Tagged(id) => vec![Node::Tagged(id)],
        Ty::StructArray(id, _)
        | Ty::DynStructArray(id, _)
        | Ty::Soa(id)
        | Ty::JsonScanner(id)
        | Ty::DictEncoded(id, _)
        | Ty::Struct(id) => vec![Node::Struct(id)],
        Ty::Tuple(id) => vec![Node::Tuple(id)],
        Ty::Fn(id) => vec![Node::Fn(id)],
        Ty::Resource(id) | Ty::ResourceRef(id) => vec![Node::Resource(id)],
        Ty::Enum(id) => vec![Node::Enum(id)],
        _ => Vec::new(),
    }
}

#[allow(dead_code)]
fn checked_count(len: usize) -> Result<u32, CanonicalGraphError> {
    u32::try_from(len).map_err(|_| CanonicalGraphError::InvalidCount)
}

#[allow(dead_code)]
fn text(out: &mut Vec<u8>, value: &str) -> Result<(), CanonicalGraphError> {
    if value.as_bytes().contains(&0) {
        return Err(CanonicalGraphError::EmbeddedNul);
    }
    out.extend(checked_count(value.len())?.to_le_bytes());
    out.extend(value.as_bytes());
    Ok(())
}

#[allow(dead_code)]
fn int(out: &mut Vec<u8>, signed: bool, bits: u8) -> Result<(), CanonicalGraphError> {
    if !matches!(bits, 8 | 16 | 32 | 64) {
        return Err(CanonicalGraphError::InvalidWidth);
    }
    out.extend([u8::from(signed), bits]);
    Ok(())
}

#[allow(dead_code)]
fn float(out: &mut Vec<u8>, bits: u8) -> Result<(), CanonicalGraphError> {
    if !matches!(bits, 32 | 64) {
        return Err(CanonicalGraphError::InvalidWidth);
    }
    out.push(bits);
    Ok(())
}

#[allow(dead_code)]
fn encode_param_mode(out: &mut Vec<u8>, mode: ParamMode) -> Result<(), CanonicalGraphError> {
    match mode {
        ParamMode::ByValue => out.push(0),
        ParamMode::Out => out.push(1),
        ParamMode::Borrow => out.push(2),
        ParamMode::BorrowMut => out.push(3),
    }
    Ok(())
}

#[allow(dead_code)]
fn append_transactional(
    out: &mut Vec<u8>,
    append: impl FnOnce(&mut Vec<u8>) -> Result<(), CanonicalGraphError>,
) -> Result<(), CanonicalGraphError> {
    let start = out.len();
    let result = append(out);
    if result.is_err() {
        out.truncate(start);
    }
    result
}

#[allow(dead_code)]
fn prim(out: &mut Vec<u8>, scalar: PrimScalar) -> Result<(), CanonicalGraphError> {
    append_transactional(out, |out| match scalar {
        PrimScalar::Int(ty) => {
            out.push(0);
            int(out, ty.signed, ty.bits)
        }
        PrimScalar::Float(ty) => {
            out.push(1);
            float(out, ty.bits)
        }
        PrimScalar::Bool => {
            out.push(2);
            Ok(())
        }
        PrimScalar::Char => {
            out.push(3);
            Ok(())
        }
        PrimScalar::Str => {
            out.push(4);
            Ok(())
        }
        PrimScalar::String => {
            out.push(5);
            Ok(())
        }
    })
}

#[allow(dead_code)]
fn scalar(
    out: &mut Vec<u8>,
    value: Scalar,
    ordinal: &impl Fn(Node) -> Result<u32, CanonicalGraphError>,
) -> Result<(), CanonicalGraphError> {
    append_transactional(out, |out| {
        macro_rules! leaf {
            ($tag:expr) => {{
                out.push($tag);
                Ok(())
            }};
        }
        macro_rules! node {
            ($tag:expr, $kind:ident, $id:expr) => {{
                out.push($tag);
                out.extend(ordinal(Node::$kind($id))?.to_le_bytes());
                Ok(())
            }};
        }
        match value {
            Scalar::Int(ty) => {
                out.push(0);
                int(out, ty.signed, ty.bits)
            }
            Scalar::Float(ty) => {
                out.push(1);
                float(out, ty.bits)
            }
            Scalar::Bool => leaf!(2),
            Scalar::Char => leaf!(3),
            Scalar::Unit => leaf!(4),
            Scalar::Struct(id) => node!(5, Struct, id),
            Scalar::String => leaf!(6),
            Scalar::DynArray(elem) => {
                out.push(7);
                prim(out, elem)
            }
            Scalar::DynStructArray(id) => node!(8, Struct, id),
            Scalar::DynResponseArray => leaf!(9),
            Scalar::Str => leaf!(10),
            Scalar::Slice(elem) => {
                out.push(11);
                prim(out, elem)
            }
            Scalar::Enum(id) => node!(12, Enum, id),
            Scalar::Tagged(id) => node!(13, Tagged, id),
            Scalar::Soa(id) => node!(14, Struct, id),
            Scalar::JsonDoc => leaf!(15),
            Scalar::Reader => leaf!(16),
            Scalar::Writer => leaf!(17),
            Scalar::Buffer => leaf!(18),
            Scalar::Regex => leaf!(19),
            Scalar::Captures => leaf!(20),
            Scalar::CliParsed => leaf!(21),
            Scalar::TcpConn => leaf!(22),
            Scalar::TcpListener => leaf!(23),
            Scalar::UdpSocket => leaf!(24),
            Scalar::Child => leaf!(25),
            Scalar::File => leaf!(26),
            Scalar::HttpResponse => leaf!(27),
            Scalar::HttpServer => leaf!(28),
            Scalar::HttpRequestCtx => leaf!(29),
            Scalar::ResponseBuilder => leaf!(30),
            Scalar::HttpStream => leaf!(31),
            Scalar::RunOutput => leaf!(32),
            Scalar::Fn(id) => node!(33, Fn, id),
            Scalar::Resource(id) => node!(34, Resource, id),
            Scalar::ResourceRef(id) => node!(35, Resource, id),
            Scalar::RunBytes => leaf!(36),
            Scalar::Param(_) | Scalar::SoaParam(_) => {
                Err(CanonicalGraphError::InvalidGraph)
            }
        }
    })
}

fn aggregate_array_elem(
    out: &mut Vec<u8>,
    value: AggregateArrayElem,
    ordinal: &impl Fn(Node) -> Result<u32, CanonicalGraphError>,
) -> Result<(), CanonicalGraphError> {
    append_transactional(out, |out| match value {
        AggregateArrayElem::Vec(elem, lanes) => {
            out.push(0);
            scalar(out, elem, ordinal)?;
            out.extend(lanes.to_le_bytes());
            Ok(())
        }
        AggregateArrayElem::Mask(elem, lanes) => {
            out.push(1);
            scalar(out, elem, ordinal)?;
            out.extend(lanes.to_le_bytes());
            Ok(())
        }
        AggregateArrayElem::FixedArray(elem, length) => {
            out.push(2);
            scalar(out, elem, ordinal)?;
            out.extend(length.to_le_bytes());
            Ok(())
        }
        AggregateArrayElem::FixedStructArray(id, length) => {
            out.push(3);
            out.extend(ordinal(Node::Struct(id))?.to_le_bytes());
            out.extend(length.to_le_bytes());
            Ok(())
        }
    })
}

fn array_builder_elem(
    out: &mut Vec<u8>,
    value: ArrayBuilderElem,
    ordinal: &impl Fn(Node) -> Result<u32, CanonicalGraphError>,
) -> Result<(), CanonicalGraphError> {
    append_transactional(out, |out| match value {
        ArrayBuilderElem::Scalar(value) => {
            out.push(0);
            scalar(out, value, ordinal)
        }
        ArrayBuilderElem::Aggregate(value) => {
            out.push(1);
            aggregate_array_elem(out, value, ordinal)
        }
    })
}

#[allow(dead_code)]
fn ty(
    out: &mut Vec<u8>,
    value: Ty,
    ordinal: &impl Fn(Node) -> Result<u32, CanonicalGraphError>,
) -> Result<(), CanonicalGraphError> {
    append_transactional(out, |out| {
        macro_rules! leaf {
            ($tag:expr) => {{
                out.push($tag);
                Ok(())
            }};
        }
        macro_rules! node {
            ($tag:expr, $kind:ident, $id:expr) => {{
                out.push($tag);
                out.extend(ordinal(Node::$kind($id))?.to_le_bytes());
                Ok(())
            }};
        }
        match value {
            Ty::Int(v) => {
                out.push(0);
                int(out, v.signed, v.bits)
            }
            Ty::Float(v) => {
                out.push(1);
                float(out, v.bits)
            }
            Ty::Bool => leaf!(2),
            Ty::Char => leaf!(3),
            Ty::Option(v) => {
                out.push(4);
                scalar(out, v, ordinal)
            }
            Ty::Result(a, b) => {
                out.push(5);
                scalar(out, a, ordinal)?;
                scalar(out, b, ordinal)
            }
            Ty::Tagged(id) => node!(6, Tagged, id),
            Ty::Box(v) => {
                out.push(7);
                scalar(out, v, ordinal)
            }
            Ty::Array(v, n) => {
                out.push(8);
                scalar(out, v, ordinal)?;
                out.extend(n.to_le_bytes());
                Ok(())
            }
            Ty::Vec(v, n) | Ty::Mask(v, n) => {
                out.push(if matches!(value, Ty::Vec(..)) { 9 } else { 10 });
                if !matches!(v, Scalar::Int(_) | Scalar::Float(_)) || !matches!(n, 2 | 4 | 8 | 16) {
                    return Err(CanonicalGraphError::InvalidWidth);
                }
                scalar(out, v, ordinal)?;
                out.extend(n.to_le_bytes());
                Ok(())
            }
            Ty::StructArray(id, n) => {
                out.push(11);
                out.extend(ordinal(Node::Struct(id))?.to_le_bytes());
                out.extend(n.to_le_bytes());
                Ok(())
            }
            Ty::DynStructArray(id, layout) => {
                out.push(12);
                out.extend(ordinal(Node::Struct(id))?.to_le_bytes());
                out.push(match layout {
                    Layout::Aos => 0,
                    Layout::Soa => 1,
                });
                Ok(())
            }
            Ty::Slice(v) => {
                out.push(13);
                scalar(out, v, ordinal)
            }
            Ty::Soa(id) => node!(14, Struct, id),
            Ty::DynSliceArray(v) => {
                out.push(15);
                prim(out, v)
            }
            Ty::DynArray(v) => {
                out.push(16);
                scalar(out, v, ordinal)
            }
            Ty::DynResponseArray => leaf!(17),
            Ty::Str => leaf!(18),
            Ty::String => leaf!(19),
            Ty::ArenaHandle => leaf!(20),
            Ty::Raw => leaf!(21),
            Ty::Builder => leaf!(22),
            Ty::Writer => leaf!(23),
            Ty::Reader => leaf!(24),
            Ty::Buffer => leaf!(25),
            Ty::ArrayBuilder(v) => {
                out.push(26);
                array_builder_elem(out, ArrayBuilderElem::Scalar(v), ordinal)
            }
            value @ (Ty::VecArrayBuilder(..)
            | Ty::MaskArrayBuilder(..)
            | Ty::FixedArrayBuilder(..)
            | Ty::FixedStructArrayBuilder(..)) => {
                out.push(26);
                array_builder_elem(
                    out,
                    value.array_builder_element().expect("matched aggregate builder"),
                    ordinal,
                )
            }
            Ty::StrFinder => leaf!(27),
            Ty::File => leaf!(28),
            Ty::Rng => leaf!(29),
            Ty::Regex => leaf!(30),
            Ty::Captures => leaf!(31),
            Ty::CliCommand => leaf!(32),
            Ty::CliParsed => leaf!(33),
            Ty::TcpConn => leaf!(34),
            Ty::TcpListener => leaf!(35),
            Ty::UdpSocket => leaf!(36),
            Ty::Child => leaf!(37),
            Ty::Command => leaf!(38),
            Ty::RunOutput => leaf!(39),
            Ty::HttpRequest => leaf!(40),
            Ty::HttpResponse => leaf!(41),
            Ty::HttpClient => leaf!(42),
            Ty::HttpServer => leaf!(43),
            Ty::HttpRequestCtx => leaf!(44),
            Ty::ResponseBuilder => leaf!(45),
            Ty::HttpStream => leaf!(46),
            Ty::HttpHeaders => leaf!(47),
            Ty::JsonDoc => leaf!(48),
            Ty::JsonScanner(id) => node!(49, Struct, id),
            Ty::Struct(id) => node!(50, Struct, id),
            Ty::Tuple(id) => node!(51, Tuple, id),
            Ty::Fn(id) => node!(52, Fn, id),
            Ty::Enum(id) => node!(53, Enum, id),
            Ty::Task(v) => {
                out.push(54);
                scalar(out, v, ordinal)
            }
            Ty::DictEncoded(id, field) => {
                out.push(55);
                out.extend(ordinal(Node::Struct(id))?.to_le_bytes());
                out.extend(field.to_le_bytes());
                Ok(())
            }
            Ty::Unit => leaf!(56),
            Ty::Resource(id) => node!(57, Resource, id),
            Ty::ResourceRef(id) => node!(58, Resource, id),
            value @ (Ty::DynVecArray(..)
            | Ty::DynMaskArray(..)
            | Ty::DynFixedArray(..)
            | Ty::DynFixedStructArray(..)) => {
                out.push(59);
                aggregate_array_elem(
                    out,
                    value.dyn_aggregate_array_element().expect("matched aggregate array"),
                    ordinal,
                )
            }
            Ty::RunBytes => leaf!(60),
            Ty::Param(_) | Ty::SoaParam(_) | Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Error => {
                Err(CanonicalGraphError::InvalidGraph)
            }
        }
    })
}

pub(super) fn canonicalize_function_types(
    program: &mut Program,
) -> Result<(), CanonicalGraphError> {
    let roots = program_type_roots(program);
    let view = CanonicalTypeView {
        structs: &program.structs,
        enums: &program.enums,
        tuples: &program.tuples,
        tagged_types: &program.tagged_types,
        fn_types: &program.fn_types,
        resources: &program.resources,
    };
    let graph = ValidatedGraph::new_many(Ty::Unit, &roots, view)?;
    let reachable: BTreeSet<u32> = graph
        .order
        .iter()
        .filter_map(|node| match node {
            Node::Fn(id) => Some(*id),
            _ => None,
        })
        .collect();
    let classes = stable_classes(&graph)?;

    let mut keyed = Vec::with_capacity(reachable.len());
    for old in reachable {
        keyed.push((
            canonical_type_bytes_with_classes(&graph, Ty::Fn(old), &classes)?,
            old,
        ));
    }
    keyed.sort();

    let mut remap = vec![None; program.fn_types.len()];
    let mut representatives = Vec::new();
    let mut previous: Option<Vec<u8>> = None;
    for (bytes, old) in keyed {
        let new_class = if previous.as_ref().is_some_and(|value| *value == bytes) {
            representatives
                .len()
                .checked_sub(1)
                .ok_or(CanonicalGraphError::InvalidGraph)?
        } else {
            previous = Some(bytes);
            representatives.push(old);
            representatives.len() - 1
        };
        let slot = remap
            .get_mut(old as usize)
            .ok_or(CanonicalGraphError::MissingReference)?;
        *slot = Some(checked_count(new_class)?);
    }

    let mut canonical = Vec::with_capacity(representatives.len());
    for old in representatives {
        let mut definition = program
            .fn_types
            .get(old as usize)
            .cloned()
            .ok_or(CanonicalGraphError::MissingReference)?;
        remap_ty_fn(&mut definition.ret, &remap);
        for (_, scalar) in &mut definition.params {
            remap_scalar_fn(scalar, &remap);
        }
        canonical.push(definition);
    }
    remap_program_function_types(program, &remap);
    program.fn_types = canonical;
    Ok(())
}

pub fn function_types_are_canonical(program: &Program) -> bool {
    let roots = program_type_roots(program);
    let definitions = function_type_facts(&program.fn_types);
    let mut canonical = program.clone();
    canonicalize_function_types(&mut canonical).is_ok()
        && roots == program_type_roots(&canonical)
        && definitions == function_type_facts(&canonical.fn_types)
}

type FunctionTypeFacts = (
    Vec<(ParamMode, Scalar)>,
    Ty,
    hir::ReturnBorrowSummary,
    hir::ReturnRegionSummary,
);

fn function_type_facts(definitions: &[FunctionTypeDef]) -> Vec<FunctionTypeFacts> {
    definitions
        .iter()
        .map(|definition| {
            (
                definition.params.clone(),
                definition.ret,
                definition.return_borrow.clone(),
                definition.return_region.clone(),
            )
        })
        .collect()
}

fn program_type_roots(program: &Program) -> Vec<Ty> {
    let mut roots = Vec::new();
    for definition in &program.structs {
        roots.extend(definition.fields.iter().map(|field| field.ty));
    }
    for definition in &program.enums {
        for variant in &definition.variants {
            roots.extend(
                variant
                    .payload
                    .iter()
                    .copied()
                    .map(align_sema::scalar_to_ty),
            );
        }
    }
    for definition in &program.tuples {
        roots.extend(
            definition
                .elems
                .iter()
                .copied()
                .map(align_sema::scalar_to_ty),
        );
    }
    for function in &program.fns {
        roots.push(function.ret);
        roots.extend(function.slots.iter().chain(&function.value_tys).copied());
        roots.extend(function_embedded_types(function));
    }
    for function in &program.externs {
        roots.push(function.ret);
        roots.extend(function.params.iter().copied());
    }
    for function in &program.imported_fns {
        roots.push(function.ret);
        roots.extend(function.params.iter().copied());
    }
    roots
}

fn remap_program_function_types(program: &mut Program, remap: &[Option<u32>]) {
    for definition in &mut program.structs {
        for field in &mut definition.fields {
            remap_ty_fn(&mut field.ty, remap);
        }
    }
    for definition in &mut program.enums {
        for variant in &mut definition.variants {
            for scalar in &mut variant.payload {
                remap_scalar_fn(scalar, remap);
            }
        }
    }
    for definition in &mut program.tuples {
        for scalar in &mut definition.elems {
            remap_scalar_fn(scalar, remap);
        }
    }
    for definition in &mut program.tagged_types {
        match definition {
            hir::TaggedType::Option(scalar) => remap_scalar_fn(scalar, remap),
            hir::TaggedType::Result(ok, err) => {
                remap_scalar_fn(ok, remap);
                remap_scalar_fn(err, remap);
            }
        }
    }
    for function in &mut program.fns {
        remap_ty_fn(&mut function.ret, remap);
        for ty in function.slots.iter_mut().chain(&mut function.value_tys) {
            remap_ty_fn(ty, remap);
        }
        remap_function_embedded_types(function, remap, remap_ty_fn);
    }
    for function in &mut program.externs {
        remap_ty_fn(&mut function.ret, remap);
        for ty in &mut function.params {
            remap_ty_fn(ty, remap);
        }
    }
    for function in &mut program.imported_fns {
        remap_ty_fn(&mut function.ret, remap);
        for ty in &mut function.params {
            remap_ty_fn(ty, remap);
        }
    }
}

fn remap_scalar_fn(value: &mut Scalar, remap: &[Option<u32>]) {
    if let Scalar::Fn(id) = value
        && let Some(Some(new)) = remap.get(*id as usize)
    {
        *id = *new;
    }
}

fn remap_ty_fn(value: &mut Ty, remap: &[Option<u32>]) {
    match value {
        Ty::Fn(id) => {
            if let Some(Some(new)) = remap.get(*id as usize) {
                *id = *new;
            }
        }
        Ty::Option(value)
        | Ty::Box(value)
        | Ty::Slice(value)
        | Ty::DynArray(value)
        | Ty::Task(value)
        | Ty::Array(value, _)
        | Ty::Vec(value, _)
        | Ty::Mask(value, _) => remap_scalar_fn(value, remap),
        Ty::ArrayBuilder(value) => remap_scalar_fn(value, remap),
        Ty::VecArrayBuilder(value, _)
        | Ty::MaskArrayBuilder(value, _)
        | Ty::FixedArrayBuilder(value, _)
        | Ty::DynVecArray(value, _)
        | Ty::DynMaskArray(value, _)
        | Ty::DynFixedArray(value, _) => remap_scalar_fn(value, remap),
        Ty::Result(ok, err) => {
            remap_scalar_fn(ok, remap);
            remap_scalar_fn(err, remap);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use align_sema::{FloatTy, IntTy};

    use super::*;
    use crate::validate_hir_tests::baseline_program;

    fn function_defs(program: &hir::Program) -> Vec<FunctionTypeDef> {
        program
            .fn_types
            .iter()
            .map(|definition| FunctionTypeDef {
                params: definition.params.clone(),
                ret: definition.ret,
                return_borrow: definition.return_borrow.clone(),
                return_region: definition.return_region.clone(),
                return_cleanup: definition.return_cleanup,
            })
            .collect()
    }

    fn validate(root: Ty, program: &hir::Program) -> Result<Vec<Node>, CanonicalGraphError> {
        let fn_types = function_defs(program);
        let graph = ValidatedGraph::new(
            root,
            CanonicalTypeView {
                structs: &program.structs,
                enums: &program.enums,
                tuples: &program.tuples,
                tagged_types: &program.tagged_types,
                fn_types: &fn_types,
                resources: &program.resources,
            },
        )?;
        assert_eq!(graph.root, root);
        assert_eq!(graph.view.structs.len(), program.structs.len());
        Ok(graph.order)
    }

    fn canonical(root: Ty, program: &hir::Program) -> Result<Vec<u8>, CanonicalGraphError> {
        let fn_types = function_defs(program);
        let graph = ValidatedGraph::new(
            root,
            CanonicalTypeView {
                structs: &program.structs,
                enums: &program.enums,
                tuples: &program.tuples,
                tagged_types: &program.tagged_types,
                fn_types: &fn_types,
                resources: &program.resources,
            },
        )?;
        canonical_type_bytes(&graph)
    }

    fn mir_program(program: &hir::Program) -> Program {
        Program {
            sqlite_callback_effects: std::collections::BTreeMap::new(),
            fns: Vec::new(),
            externs: Vec::new(),
            imported_fns: Vec::new(),
            link_libs: Vec::new(),
            structs: program.structs.clone(),
            enums: program.enums.clone(),
            resources: program.resources.clone(),
            tagged_types: program.tagged_types.clone(),
            fn_types: function_defs(program),
            tuples: program.tuples.clone(),
        }
    }

    #[derive(Default)]
    struct RefinementMetrics {
        signature_bytes: usize,
        comparisons: usize,
        compared_bytes: usize,
    }

    impl RefinementObserver for RefinementMetrics {
        fn signature(&mut self, bytes: usize) {
            self.signature_bytes += bytes;
        }

        fn compare(&mut self, left: &[u8], right: &[u8]) -> Ordering {
            self.comparisons += 1;
            let common = left
                .iter()
                .zip(right)
                .take_while(|(left, right)| left == right)
                .count();
            self.compared_bytes += common + usize::from(common < left.len().min(right.len()));
            left.cmp(right)
        }
    }

    fn observed_refinement(
        root: Ty,
        program: &hir::Program,
    ) -> Result<(usize, RefinementMetrics), CanonicalGraphError> {
        let fn_types = function_defs(program);
        let graph = ValidatedGraph::new(
            root,
            CanonicalTypeView {
                structs: &program.structs,
                enums: &program.enums,
                tuples: &program.tuples,
                tagged_types: &program.tagged_types,
                fn_types: &fn_types,
                resources: &program.resources,
            },
        )?;
        let mut metrics = RefinementMetrics::default();
        let (_, rounds) = stable_classes_observed(&graph, &mut metrics)?;
        Ok((rounds, metrics))
    }

    fn i(bits: u8) -> IntTy {
        IntTy { bits, signed: true }
    }

    fn f(bits: u8) -> FloatTy {
        FloatTy { bits }
    }

    fn ordinal(node: Node) -> Result<u32, CanonicalGraphError> {
        Ok(match node {
            Node::Struct(id) => 0x1000 + id,
            Node::Enum(id) => 0x2000 + id,
            Node::Tuple(id) => 0x3000 + id,
            Node::Tagged(id) => 0x4000 + id,
            Node::Fn(id) => 0x5000 + id,
            Node::Resource(id) => 0x6000 + id,
        })
    }

    fn appended(
        encode: impl FnOnce(&mut Vec<u8>) -> Result<(), CanonicalGraphError>,
    ) -> Result<Vec<u8>, CanonicalGraphError> {
        let mut out = vec![0xa5, 0x5a];
        encode(&mut out)?;
        assert_eq!(&out[..2], [0xa5, 0x5a]);
        Ok(out.split_off(2))
    }

    fn encoded_ty(value: Ty) -> Result<Vec<u8>, CanonicalGraphError> {
        appended(|out| ty(out, value, &ordinal))
    }

    fn encoded_scalar(value: Scalar) -> Result<Vec<u8>, CanonicalGraphError> {
        appended(|out| scalar(out, value, &ordinal))
    }

    fn encoded_prim(value: PrimScalar) -> Result<Vec<u8>, CanonicalGraphError> {
        appended(|out| prim(out, value))
    }

    macro_rules! cases {
        (encoded_ty; $($value:expr => $bytes:expr),+ $(,)?) => {{
            crate::source_shape::tests::assert_ty_matrix(&[$($value),+]); $(assert_eq!(encoded_ty($value).unwrap(), $bytes, "{:?}", $value);)+ }};
        (encoded_scalar; $($value:expr => $bytes:expr),+ $(,)?) => {{
            crate::source_shape::tests::assert_scalar_matrix(&[$($value),+]); $(assert_eq!(encoded_scalar($value).unwrap(), $bytes, "{:?}", $value);)+ }};
        ($encoder:ident; $($value:expr => $bytes:expr),+ $(,)?) => {
            $(assert_eq!($encoder($value).unwrap(), $bytes, "{:?}", $value);)+
        };
    }

    macro_rules! bytes {
        ($actual:expr, $expected:expr) => {
            assert_eq!($actual.unwrap(), $expected)
        };
    }

    macro_rules! error {
        ($actual:expr, $expected:expr) => {
            assert_eq!($actual, Err($expected))
        };
    }

    #[test]
    fn canonical_graph_validation() {
        let program = baseline_program();
        assert_eq!(
            validate(Ty::Struct(0), &program).unwrap(),
            [Node::Struct(0)]
        );
        assert_eq!(
            validate(Ty::Struct(u32::MAX), &program),
            Err(CanonicalGraphError::MissingReference)
        );

        let mut invalid = program.clone();
        invalid.structs[0].fields[0].name = "bad-name".into();
        assert_eq!(
            validate(Ty::Struct(0), &invalid),
            Err(CanonicalGraphError::InvalidGraph)
        );

        let mut duplicate = program.clone();
        let repeated = duplicate.structs[0].fields[0].clone();
        duplicate.structs[0].fields.push(repeated);
        assert_eq!(
            validate(Ty::Struct(0), &duplicate),
            Err(CanonicalGraphError::DuplicateMember)
        );

        let mut unreachable = program.clone();
        let mut bad = unreachable.structs[0].clone();
        bad.source_name.clear();
        unreachable.structs.push(bad);
        assert!(validate(Ty::Bool, &unreachable).unwrap().is_empty());

        let mut all_nodes = program.clone();
        all_nodes.enums[0].variants[0].field_base = 0;
        assert_eq!(
            validate(Ty::Enum(0), &all_nodes),
            Err(CanonicalGraphError::InvalidGraph)
        );
        assert_eq!(
            validate(Ty::Tuple(u32::MAX), &all_nodes),
            Err(CanonicalGraphError::MissingReference)
        );
        assert_eq!(
            validate(Ty::Tagged(u32::MAX), &all_nodes),
            Err(CanonicalGraphError::MissingReference)
        );
        assert_eq!(
            validate(Ty::Fn(u32::MAX), &all_nodes),
            Err(CanonicalGraphError::MissingReference)
        );
    }

    #[test]
    fn canonical_graph_rejects_inline_cycles_but_allows_header_cycles() {
        let mut direct = baseline_program();
        direct.structs[0].fields[0].ty = Ty::Struct(0);
        assert_eq!(
            validate(Ty::Struct(0), &direct),
            Err(CanonicalGraphError::InvalidGraph)
        );

        let mut cycle_before_missing = direct.clone();
        let mut later = cycle_before_missing.structs[0].fields[0].clone();
        later.name = "later".into();
        later.ty = Ty::Struct(u32::MAX);
        cycle_before_missing.structs[0].fields.push(later);
        assert_eq!(
            validate(Ty::Struct(0), &cycle_before_missing),
            Err(CanonicalGraphError::InvalidGraph)
        );

        let mut missing_before_cycle = direct.clone();
        let mut later = missing_before_cycle.structs[0].fields[0].clone();
        later.name = "later".into();
        missing_before_cycle.structs[0].fields[0].ty = Ty::Struct(u32::MAX);
        missing_before_cycle.structs[0].fields.push(later);
        assert_eq!(
            validate(Ty::Struct(0), &missing_before_cycle),
            Err(CanonicalGraphError::MissingReference)
        );

        let mut mutual = baseline_program();
        let mut child = mutual.structs[0].clone();
        child.source_name = "Child".into();
        child.fields[0].ty = Ty::Struct(0);
        mutual.structs[0].fields[0].ty = Ty::Struct(1);
        mutual.structs.push(child);
        assert_eq!(
            validate(Ty::Struct(0), &mutual),
            Err(CanonicalGraphError::InvalidGraph)
        );

        let mut boxed = baseline_program();
        boxed.structs[0].fields[0].ty = Ty::Box(Scalar::Struct(0));
        assert_eq!(
            validate(Ty::Struct(0), &boxed).unwrap(),
            [Node::Struct(0)]
        );
    }

    #[test]
    fn canonical_graph_validation_error_precedence() {
        let base = baseline_program();
        let make_struct = |source_name: &str, fields: Vec<(&str, Ty)>| {
            let mut definition = base.structs[0].clone();
            definition.source_name = source_name.into();
            definition.fields = fields
                .into_iter()
                .map(|(name, ty)| {
                    let mut field = base.structs[0].fields[0].clone();
                    field.name = name.into();
                    field.ty = ty;
                    field
                })
                .collect();
            definition
        };

        let mut program = base.clone();
        program.structs = vec![make_struct("bad\0source", vec![("value", Ty::Int(i(24)))])];
        assert_eq!(
            validate(Ty::Struct(0), &program),
            Err(CanonicalGraphError::EmbeddedNul)
        );

        program.structs = vec![
            make_struct(
                "Root",
                vec![("first", Ty::Int(i(24))), ("child", Ty::Struct(1))],
            ),
            make_struct("bad\0child", vec![("value", Ty::Bool)]),
        ];
        assert_eq!(
            validate(Ty::Struct(0), &program),
            Err(CanonicalGraphError::InvalidWidth)
        );

        program.structs = vec![make_struct(
            "Root",
            vec![("bad-name", Ty::Struct(u32::MAX))],
        )];
        assert_eq!(
            validate(Ty::Struct(0), &program),
            Err(CanonicalGraphError::InvalidGraph)
        );

        program.structs = vec![
            make_struct(
                "Root",
                vec![("first", Ty::Struct(1)), ("second", Ty::Struct(2))],
            ),
            make_struct("Alias", vec![("value", Ty::Bool)]),
            make_struct("Alias", vec![("value", Ty::Struct(u32::MAX))]),
        ];
        assert_eq!(
            validate(Ty::Struct(0), &program),
            Err(CanonicalGraphError::MissingReference)
        );

        program.structs = vec![
            make_struct(
                "Root",
                vec![
                    ("first", Ty::Struct(1)),
                    ("second", Ty::Struct(2)),
                    ("later", Ty::Struct(3)),
                ],
            ),
            make_struct("Alias", vec![("value", Ty::Bool)]),
            make_struct("Alias", vec![("value", Ty::Char)]),
            make_struct("Later", vec![("bad-name", Ty::Bool)]),
        ];
        assert_eq!(
            validate(Ty::Struct(0), &program),
            Err(CanonicalGraphError::InvalidGraph)
        );

        program.structs[2].fields[0].ty = Ty::Struct(3);
        program.structs[3].fields[0].name = "bad-name".into();
        assert_eq!(
            validate(Ty::Struct(0), &program),
            Err(CanonicalGraphError::InvalidGraph)
        );
    }

    #[test]
    fn canonical_graph_validation_rejects_raw_duplicates() {
        let mut nominal = baseline_program();
        let duplicate = nominal.structs[0].clone();
        let mut root = duplicate.clone();
        root.source_name = "Root".into();
        root.fields = vec![root.fields[0].clone()];
        let mut second = root.fields[0].clone();
        root.fields[0].name = "first".into();
        root.fields[0].ty = Ty::Struct(1);
        second.name = "second".into();
        second.ty = Ty::Struct(2);
        root.fields.push(second);
        nominal.structs = vec![root, duplicate.clone(), duplicate];
        assert_eq!(
            validate(Ty::Struct(0), &nominal).unwrap(),
            [Node::Struct(0), Node::Struct(1), Node::Struct(2)]
        );

        let mut tuple = baseline_program();
        tuple.tuples.push(tuple.tuples[0].clone());
        tuple.structs[0].fields[0].ty = Ty::Tuple(0);
        let mut second = tuple.structs[0].fields[0].clone();
        second.name = "second".into();
        second.ty = Ty::Tuple(1);
        tuple.structs[0].fields.push(second);
        assert_eq!(
            validate(Ty::Struct(0), &tuple),
            Err(CanonicalGraphError::DuplicateMember)
        );
    }

    #[test]
    fn canonical_graph_function_root_validation() {
        let mut program = baseline_program();
        program.fn_types[0].params = vec![(ParamMode::ByValue, Scalar::Struct(0))];
        program.fn_types[0].ret = Ty::Option(Scalar::Struct(0));
        assert_eq!(
            validate(Ty::Fn(0), &program).unwrap(),
            [Node::Fn(0), Node::Struct(0)]
        );

        program.fn_types[0].return_borrow = hir::ReturnBorrowSummary::Roots {
            params: vec![],
            captures: vec![],
        };
        assert_eq!(
            validate(Ty::Fn(0), &program),
            Err(CanonicalGraphError::InvalidSummary)
        );
    }

    #[test]
    fn canonical_graph_validation_raw_scan_is_linear() {
        let mut program = baseline_program();
        let mut leaf = program.structs[0].clone();
        leaf.source_name = "Leaf".into();
        program.structs.push(leaf);
        program.structs[0].fields[0].ty = Ty::Struct(1);
        let mut second = program.structs[0].fields[0].clone();
        second.name = "other".into();
        program.structs[0].fields.push(second);
        program.structs[1].fields[0].ty = Ty::Box(Scalar::Struct(1));
        let order = validate(Ty::Struct(0), &program).unwrap();
        assert_eq!(order, [Node::Struct(0), Node::Struct(1)]);
        let view = CanonicalTypeView {
            structs: &program.structs,
            enums: &program.enums,
            tuples: &program.tuples,
            tagged_types: &program.tagged_types,
            fn_types: &[],
            resources: &program.resources,
        };
        let edges: usize = order
            .iter()
            .map(|&node| node_children(view, node).unwrap().len())
            .sum();
        assert_eq!((order.len(), edges), (2, 3));
    }

    #[test]
    fn deep_canonical_graph_validation_is_stack_bounded() {
        let mut program = baseline_program();
        program.structs.clear();
        for id in 0..4096u32 {
            let mut definition = baseline_program().structs[0].clone();
            definition.source_name = format!("S{id}");
            definition.fields[0].ty = if id == 4095 {
                Ty::Bool
            } else {
                Ty::Struct(id + 1)
            };
            program.structs.push(definition);
        }
        let order = validate(Ty::Struct(0), &program).unwrap();
        assert_eq!(order.len(), 4096);
        assert_eq!(order.first(), Some(&Node::Struct(0)));
        assert_eq!(order.last(), Some(&Node::Struct(4095)));
    }

    #[test]
    fn canonical_graph_engine() {
        let program = baseline_program();
        assert_eq!(canonical(Ty::Unit, &program).unwrap(), [3, 0, 0, 0, 0, 56]);
        assert_eq!(canonical(Ty::Bool, &program).unwrap(), [3, 0, 0, 0, 0, 2]);
        assert_eq!(
            canonical(Ty::Int(i(64)), &program).unwrap(),
            [3, 0, 0, 0, 0, 0, 1, 64]
        );
        let bytes = canonical(Ty::Struct(0), &program).unwrap();
        assert_eq!(&bytes[..5], [3, 1, 0, 0, 0]);
        assert_eq!(bytes.last(), Some(&0));
    }

    #[test]
    fn canonical_type_codec() {
        let program = mir_program(&baseline_program());
        for (root, expected) in [
            (Ty::Unit, vec![3, 0, 0, 0, 0, 56]),
            (Ty::Bool, vec![3, 0, 0, 0, 0, 2]),
            (Ty::Int(i(64)), vec![3, 0, 0, 0, 0, 0, 1, 64]),
            (Ty::RunBytes, vec![3, 0, 0, 0, 0, 60]),
        ] {
            let encoded = CanonicalTy::from_program(root, &program).unwrap();
            assert_eq!(encoded.as_bytes(), expected);
            assert_eq!(CanonicalTy::decode(&expected).unwrap(), encoded);
        }
        for root in [
            Ty::Struct(0),
            Ty::Enum(0),
            Ty::Tuple(0),
            Ty::Tagged(0),
            Ty::Fn(0),
        ] {
            let encoded = CanonicalTy::from_program(root, &program).unwrap();
            assert_eq!(CanonicalTy::decode(encoded.as_bytes()).unwrap(), encoded);
        }

        assert_eq!(
            ProgramCall::try_from_logical("pkg$run").unwrap().as_bytes(),
            b"pkg$run"
        );
        assert_eq!(
            ProgramCall::try_from_logical(""),
            Err(ProgramCallError::Empty)
        );
        assert_eq!(
            ProgramCall::try_from_logical("bad\0name"),
            Err(ProgramCallError::EmbeddedNul)
        );

        let roots = [
            Ty::Int(i(8)),
            Ty::Float(f(32)),
            Ty::Bool,
            Ty::Char,
            Ty::Option(Scalar::Bool),
            Ty::Result(Scalar::Bool, Scalar::Char),
            Ty::Tagged(0),
            Ty::Box(Scalar::Bool),
            Ty::Array(Scalar::Bool, 2),
            Ty::Vec(Scalar::Int(i(8)), 2),
            Ty::Mask(Scalar::Float(f(32)), 2),
            Ty::StructArray(0, 2),
            Ty::DynStructArray(0, Layout::Aos),
            Ty::Slice(Scalar::Bool),
            Ty::Soa(0),
            Ty::DynSliceArray(PrimScalar::Bool),
            Ty::DynArray(Scalar::Bool),
            Ty::DynResponseArray,
            Ty::Str,
            Ty::String,
            Ty::ArenaHandle,
            Ty::Raw,
            Ty::Builder,
            Ty::Writer,
            Ty::Reader,
            Ty::Buffer,
            Ty::ArrayBuilder(Scalar::Bool),
            Ty::array_builder(ArrayBuilderElem::Aggregate(AggregateArrayElem::Vec(
                Scalar::Int(i(8)),
                2,
            ))),
            Ty::array_builder(ArrayBuilderElem::Aggregate(AggregateArrayElem::Mask(
                Scalar::Float(f(32)),
                4,
            ))),
            Ty::dyn_aggregate_array(AggregateArrayElem::FixedArray(Scalar::Bool, 2)),
            Ty::dyn_aggregate_array(AggregateArrayElem::FixedStructArray(0, 2)),
            Ty::StrFinder,
            Ty::File,
            Ty::Rng,
            Ty::Regex,
            Ty::Captures,
            Ty::CliCommand,
            Ty::CliParsed,
            Ty::TcpConn,
            Ty::TcpListener,
            Ty::UdpSocket,
            Ty::Child,
            Ty::Command,
            Ty::RunOutput,
            Ty::RunBytes,
            Ty::HttpRequest,
            Ty::HttpResponse,
            Ty::HttpClient,
            Ty::HttpServer,
            Ty::HttpRequestCtx,
            Ty::ResponseBuilder,
            Ty::HttpStream,
            Ty::HttpHeaders,
            Ty::JsonDoc,
            Ty::JsonScanner(0),
            Ty::Struct(0),
            Ty::Tuple(0),
            Ty::Fn(0),
            Ty::Enum(0),
            Ty::Task(Scalar::Bool),
            Ty::DictEncoded(0, 0),
            Ty::Unit,
        ];
        crate::source_shape::tests::assert_ty_matrix(&roots);
        for root in roots {
            let encoded = CanonicalTy::from_program(root, &program).unwrap();
            assert_eq!(CanonicalTy::decode(encoded.as_bytes()).unwrap(), encoded);
        }

        let scalars = [
            Scalar::Int(i(8)),
            Scalar::Float(f(32)),
            Scalar::Bool,
            Scalar::Char,
            Scalar::Unit,
            Scalar::Struct(0),
            Scalar::String,
            Scalar::DynArray(PrimScalar::Bool),
            Scalar::DynStructArray(0),
            Scalar::DynResponseArray,
            Scalar::Str,
            Scalar::Slice(PrimScalar::Char),
            Scalar::Enum(0),
            Scalar::Tagged(0),
            Scalar::Soa(0),
            Scalar::JsonDoc,
            Scalar::Reader,
            Scalar::Writer,
            Scalar::Buffer,
            Scalar::Regex,
            Scalar::Captures,
            Scalar::CliParsed,
            Scalar::TcpConn,
            Scalar::TcpListener,
            Scalar::UdpSocket,
            Scalar::Child,
            Scalar::File,
            Scalar::HttpResponse,
            Scalar::HttpServer,
            Scalar::HttpRequestCtx,
            Scalar::ResponseBuilder,
            Scalar::HttpStream,
            Scalar::RunOutput,
            Scalar::RunBytes,
            Scalar::Fn(0),
        ];
        crate::source_shape::tests::assert_scalar_matrix(&scalars);
        for scalar in scalars {
            let encoded = CanonicalTy::from_program(Ty::Option(scalar), &program).unwrap();
            assert_eq!(CanonicalTy::decode(encoded.as_bytes()).unwrap(), encoded);
        }
    }

    #[test]
    fn canonical_type_codec_function_root() {
        let mut hir = baseline_program();
        hir.fn_types[0].params = vec![(ParamMode::ByValue, Scalar::Fn(0))];
        hir.fn_types[0].ret = Ty::Unit;
        let program = mir_program(&hir);
        let ty = CanonicalTy::from_program(Ty::Fn(0), &program).unwrap();
        assert_eq!(CanonicalTy::decode(ty.as_bytes()).unwrap(), ty);

        let abi = CanonicalFnAbi::from_parts(
            &[],
            Ty::Unit,
            &hir::ReturnBorrowSummary::None,
            &hir::ReturnRegionSummary::None,
            hir::ReturnCleanupAbi::None,
            &program,
        )
        .unwrap();
        assert_eq!(abi.as_bytes(), [1, 0, 0, 0, 0, 3, 0, 0, 0, 0, 56, 0, 0, 0]);
        assert_eq!(CanonicalFnAbi::decode(abi.as_bytes()).unwrap(), abi);

        let params = [(ParamMode::ByValue, Ty::Fn(0))];
        let recursive = CanonicalFnAbi::from_parts(
            &params,
            Ty::Fn(0),
            &hir::ReturnBorrowSummary::Roots {
                params: vec![0],
                captures: vec![],
            },
            &hir::ReturnRegionSummary::Roots {
                params: vec![0],
                captures: vec![],
            },
            hir::ReturnCleanupAbi::None,
            &program,
        )
        .unwrap();
        assert_eq!(
            CanonicalFnAbi::decode(recursive.as_bytes()).unwrap(),
            recursive
        );
    }

    #[test]
    fn canonical_codec_error_precedence() {
        let error = |bytes: &[u8], expected| {
            assert_eq!(CanonicalTy::decode(bytes), Err(expected), "{bytes:02x?}");
        };
        error(&[], CanonicalCodecError::Truncated);
        error(&[2], CanonicalCodecError::UnsupportedVersion);
        error(&[3, 0, 0, 0, 0, 0xff], CanonicalCodecError::UnknownTag);
        error(&[3, 0, 0, 0, 0, 61], CanonicalCodecError::UnknownTag);
        error(&[3, 0, 0, 0, 0, 4, 37], CanonicalCodecError::UnknownTag);
        error(&[3, 0, 0, 0, 0], CanonicalCodecError::Truncated);
        error(&[3, 0, 0, 0, 0, 26, 2], CanonicalCodecError::UnknownTag);
        error(&[3, 0, 0, 0, 0, 26, 1, 4], CanonicalCodecError::UnknownTag);
        error(
            &[3, 0, 0, 0, 0, 26, 1, 0, 2, 4, 0, 0, 0],
            CanonicalCodecError::InvalidWidth,
        );
        error(
            &[3, 0, 0, 0, 0, 59, 2, 2, 0, 0, 0, 0],
            CanonicalCodecError::InvalidCount,
        );
        error(
            &[3, 0, 0, 0, 0, 59, 3, 0, 0, 0, 0, 0, 0, 0, 0],
            CanonicalCodecError::InvalidCount,
        );
        error(&[3, 0, 0, 0, 0, 0, 2, 64], CanonicalCodecError::InvalidBool);
        error(
            &[3, 0, 0, 0, 0, 0, 1, 24],
            CanonicalCodecError::InvalidWidth,
        );
        error(
            &[3, 0, 0, 0, 0, 50, 0xff, 0xff, 0xff, 0xff],
            CanonicalCodecError::MissingReference,
        );

        let mut trailing = vec![3, 0, 0, 0, 0, 56];
        trailing.push(0);
        error(&trailing, CanonicalCodecError::TrailingBytes);

        let invalid_utf8 = [
            3, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 50, 0, 0, 0, 0,
        ];
        error(&invalid_utf8, CanonicalCodecError::InvalidUtf8);
        let embedded_nul = [
            3, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 0, 0, 0, 0,
        ];
        error(&embedded_nul, CanonicalCodecError::EmbeddedNul);

        let invalid_align = [
            3, 1, 0, 0, 0, 0, 1, 0, 0, 0, b'S', 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 50, 0, 0, 0, 0,
        ];
        error(&invalid_align, CanonicalCodecError::InvalidGraph);

        let mut inline_cycle = baseline_program();
        let mut child = inline_cycle.structs[0].clone();
        child.source_name = "Child".into();
        child.fields[0].ty = Ty::Bool;
        inline_cycle.structs[0].fields[0].ty = Ty::Struct(1);
        inline_cycle.structs.push(child);
        let valid = CanonicalTy::from_program(
            Ty::Struct(0),
            &mir_program(&inline_cycle),
        )
        .unwrap();
        let mut recursive = valid.as_bytes().to_vec();
        let reference = recursive
            .windows(5)
            .position(|window| window == [50, 1, 0, 0, 0])
            .expect("fixture contains the root-to-child inline reference");
        recursive[reference + 1..reference + 5].copy_from_slice(&0u32.to_le_bytes());
        error(&recursive, CanonicalCodecError::InvalidGraph);

        let duplicate_function = [
            3, 2, 0, 0, 0, 4, 0, 0, 0, 0, 56, 0, 0, 0, 4, 0, 0, 0, 0, 56, 0, 0, 0, 52, 0, 0, 0, 0,
        ];
        error(&duplicate_function, CanonicalCodecError::DuplicateMember);

        let unreachable_function = [3, 1, 0, 0, 0, 4, 0, 0, 0, 0, 56, 0, 0, 0, 56];
        error(
            &unreachable_function,
            CanonicalCodecError::NonCanonicalOrder,
        );

        let invalid_summary = [
            3, 1, 0, 0, 0, 4, 0, 0, 0, 0, 56, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 52, 0, 0, 0, 0,
        ];
        error(&invalid_summary, CanonicalCodecError::InvalidSummary);

        let unit = [3, 0, 0, 0, 0, 56];
        let mut invalid_mode = vec![1, 1, 0, 0, 0, 4];
        invalid_mode.extend(unit);
        invalid_mode.extend(unit);
        invalid_mode.extend([0, 0]);
        assert_eq!(
            CanonicalFnAbi::decode(&invalid_mode),
            Err(CanonicalCodecError::UnknownTag)
        );
        let mut invalid_mode_then_truncated = vec![1, 1, 0, 0, 0, 4];
        invalid_mode_then_truncated.extend(unit);
        assert_eq!(
            CanonicalFnAbi::decode(&invalid_mode_then_truncated),
            Err(CanonicalCodecError::UnknownTag)
        );
        let mut abi_trailing = vec![1, 0, 0, 0, 0];
        abi_trailing.extend(unit);
        abi_trailing.extend([0, 0, 0, 0xff]);
        assert_eq!(
            CanonicalFnAbi::decode(&abi_trailing),
            Err(CanonicalCodecError::TrailingBytes)
        );
    }

    #[test]
    fn deep_canonical_type_codec_is_stack_bounded() {
        let mut hir = baseline_program();
        hir.structs.clear();
        for id in 0..4096u32 {
            let mut definition = baseline_program().structs[0].clone();
            definition.source_name = format!("Codec{id}");
            definition.fields[0].ty = if id == 4095 {
                Ty::Bool
            } else {
                Ty::Struct(id + 1)
            };
            hir.structs.push(definition);
        }
        let program = mir_program(&hir);
        let encoded = CanonicalTy::from_program(Ty::Struct(0), &program).unwrap();
        assert_eq!(CanonicalTy::decode(encoded.as_bytes()).unwrap(), encoded);
        assert_eq!(
            CanonicalTy::decode(&encoded.as_bytes()[..encoded.as_bytes().len() - 1]),
            Err(CanonicalCodecError::Truncated)
        );
    }

    #[test]
    fn canonical_graph_equivalence() {
        let mut program = baseline_program();
        program.fn_types[0].params = vec![(ParamMode::ByValue, Scalar::Bool)];
        program.fn_types[0].ret = Ty::Unit;
        program.fn_types.push(program.fn_types[0].clone());
        program.tuples[0].elems = vec![Scalar::Fn(0)];
        let mut equivalent = program.tuples[0].clone();
        equivalent.elems[0] = Scalar::Fn(1);
        program.tuples.push(equivalent);
        program.structs[0].fields.truncate(1);
        program.structs[0].fields[0].ty = Ty::Tuple(0);
        let mut second = program.structs[0].fields[0].clone();
        second.name = "other".into();
        second.ty = Ty::Tuple(1);
        program.structs[0].fields.push(second);
        let bytes = canonical(Ty::Struct(0), &program).unwrap();
        assert_eq!(&bytes[..5], [3, 3, 0, 0, 0]);

        let mut permuted = program.clone();
        permuted.tuples.swap(0, 1);
        permuted.structs[0].fields[0].ty = Ty::Tuple(1);
        permuted.structs[0].fields[1].ty = Ty::Tuple(0);
        assert_eq!(canonical(Ty::Struct(0), &permuted).unwrap(), bytes);

        permuted.fn_types[1].params[0].0 = ParamMode::Out;
        assert_ne!(canonical(Ty::Struct(0), &permuted).unwrap(), bytes);

        let mut cycles = baseline_program();
        cycles.fn_types[0].params = vec![(ParamMode::ByValue, Scalar::Fn(0))];
        cycles.fn_types[0].ret = Ty::Unit;
        let mut first = cycles.fn_types[0].clone();
        first.params[0].1 = Scalar::Fn(2);
        let mut second = cycles.fn_types[0].clone();
        second.params[0].1 = Scalar::Fn(1);
        cycles.fn_types.extend([first, second]);
        assert_eq!(
            canonical(Ty::Fn(0), &cycles).unwrap(),
            canonical(Ty::Fn(1), &cycles).unwrap()
        );
        cycles.fn_types[2].params[0].0 = ParamMode::Out;
        assert_ne!(
            canonical(Ty::Fn(0), &cycles).unwrap(),
            canonical(Ty::Fn(1), &cycles).unwrap()
        );
    }

    #[test]
    fn canonical_graph_refinement_round_bound() {
        const ANONYMOUS_NODES: usize = 128;
        let mut program = baseline_program();
        program.fn_types.clear();
        for id in 0..ANONYMOUS_NODES {
            let mut definition = baseline_program().fn_types[0].clone();
            definition.params = vec![(
                ParamMode::ByValue,
                if id + 1 == ANONYMOUS_NODES {
                    Scalar::Bool
                } else {
                    Scalar::Fn((id + 1) as u32)
                },
            )];
            definition.ret = Ty::Unit;
            definition.return_borrow = hir::ReturnBorrowSummary::None;
            definition.return_region = hir::ReturnRegionSummary::None;
            program.fn_types.push(definition);
        }
        let (rounds, _) = observed_refinement(Ty::Fn(0), &program).unwrap();
        assert!(rounds > 1);
        assert!(rounds <= ANONYMOUS_NODES + 1);
    }

    #[test]
    fn canonical_graph_signature_sort_bound() {
        const FUNCTIONS: usize = 96;
        const PARAMS: usize = 192;
        let mut program = baseline_program();
        program.fn_types.clear();
        program.tuples[0].elems = (0..FUNCTIONS).map(|id| Scalar::Fn(id as u32)).collect();
        for id in 0..FUNCTIONS {
            let mut definition = baseline_program().fn_types[0].clone();
            definition.params = vec![(ParamMode::ByValue, Scalar::Bool); PARAMS];
            definition.params.push((
                ParamMode::ByValue,
                if id % 2 == 0 {
                    Scalar::Char
                } else {
                    Scalar::Unit
                },
            ));
            definition.ret = Ty::Unit;
            definition.return_borrow = hir::ReturnBorrowSummary::None;
            definition.return_region = hir::ReturnRegionSummary::None;
            program.fn_types.push(definition);
        }
        let (_, metrics) = observed_refinement(Ty::Tuple(0), &program).unwrap();
        assert!(metrics.signature_bytes > FUNCTIONS * PARAMS);
        assert!(metrics.comparisons >= FUNCTIONS);
        assert!(metrics.compared_bytes > metrics.comparisons * PARAMS);
    }

    #[test]
    fn deep_canonical_graph_is_stack_bounded() {
        let mut program = baseline_program();
        program.structs.clear();
        for id in 0..4096u32 {
            let mut definition = baseline_program().structs[0].clone();
            definition.source_name = format!("S{id}");
            definition.fields[0].ty = if id == 4095 {
                Ty::Bool
            } else {
                Ty::Struct(id + 1)
            };
            program.structs.push(definition);
        }
        let bytes = canonical(Ty::Struct(0), &program).unwrap();
        assert_eq!(&bytes[..5], [3, 0, 16, 0, 0]);
    }

    #[test]
    fn canonical_graph_function_root() {
        let mut program = baseline_program();
        program.fn_types[0].params = vec![(ParamMode::ByValue, Scalar::Bool)];
        program.fn_types[0].ret = Ty::Unit;
        let first = canonical(Ty::Fn(0), &program).unwrap();
        program.fn_types.push(program.fn_types[0].clone());
        assert_eq!(canonical(Ty::Fn(1), &program).unwrap(), first);
        program.fn_types[1].params[0].0 = ParamMode::Out;
        assert_ne!(canonical(Ty::Fn(1), &program).unwrap(), first);
    }

    #[test]
    fn canonical_function_type_remap() {
        let hir = baseline_program();
        let mut first = function_defs(&hir)[0].clone();
        first.params = vec![(ParamMode::ByValue, Scalar::Bool)];
        let mut second = first.clone();
        second.params[0].1 = Scalar::Char;
        let duplicate = first.clone();
        let mut unreachable = first.clone();
        unreachable.params[0].1 = Scalar::Unit;

        let mut structs = hir.structs.clone();
        structs[0].fields[0].ty = Ty::Fn(2);
        structs[0].fields[1].ty = Ty::Tagged(0);
        let mut enums = hir.enums.clone();
        enums[0].variants[0].payload = vec![Scalar::Fn(1)];
        enums[0].variants[1].field_base = 2;
        let mut tuples = hir.tuples.clone();
        tuples[0].elems = vec![Scalar::Fn(2)];
        let mut program = Program {
            sqlite_callback_effects: std::collections::BTreeMap::new(),
            fns: Vec::new(),
            externs: Vec::new(),
            imported_fns: Vec::new(),
            link_libs: Vec::new(),
            structs,
            enums,
            resources: Vec::new(),
            tagged_types: vec![hir::TaggedType::Result(Scalar::Fn(0), Scalar::Fn(1))],
            fn_types: vec![first, second, duplicate, unreachable],
            tuples,
        };

        canonicalize_function_types(&mut program).unwrap();
        assert_eq!(program.fn_types.len(), 2);
        assert_eq!(program.structs[0].fields[0].ty, Ty::Fn(0));
        let Ty::Tagged(0) = program.structs[0].fields[1].ty else {
            panic!("tagged function root must remain reachable");
        };
        let hir::TaggedType::Result(Scalar::Fn(first), Scalar::Fn(second)) =
            program.tagged_types[0]
        else {
            panic!("tagged function references must be remapped");
        };
        assert_ne!(first, second);
        assert_eq!(program.tuples[0].elems, [Scalar::Fn(0)]);
        assert!(function_types_are_canonical(&program));

        let mut non_compact = program.clone();
        non_compact.fn_types.push(non_compact.fn_types[0].clone());
        assert!(!function_types_are_canonical(&non_compact));

        let mut missing = program.clone();
        missing.structs[0].fields[0].ty = Ty::Fn(u32::MAX);
        assert!(!function_types_are_canonical(&missing));
    }

    #[test]
    fn canonical_field_codec_covers_every_primitive_and_scalar_tag() {
        cases!(encoded_prim;
            PrimScalar::Int(i(8)) => [0, 1, 8], PrimScalar::Float(f(32)) => [1, 32],
            PrimScalar::Bool => [2], PrimScalar::Char => [3],
            PrimScalar::Str => [4], PrimScalar::String => [5],
        );
        cases!(encoded_scalar;
            Scalar::Int(i(8)) => [0, 1, 8], Scalar::Float(f(32)) => [1, 32],
            Scalar::Bool => [2], Scalar::Char => [3], Scalar::Unit => [4],
            Scalar::Struct(1) => [5, 1, 0x10, 0, 0], Scalar::String => [6],
            Scalar::DynArray(PrimScalar::Bool) => [7, 2],
            Scalar::DynStructArray(1) => [8, 1, 0x10, 0, 0],
            Scalar::DynResponseArray => [9], Scalar::Str => [10],
            Scalar::Slice(PrimScalar::Char) => [11, 3],
            Scalar::Enum(1) => [12, 1, 0x20, 0, 0],
            Scalar::Tagged(1) => [13, 1, 0x40, 0, 0],
            Scalar::Soa(1) => [14, 1, 0x10, 0, 0], Scalar::JsonDoc => [15],
            Scalar::Reader => [16], Scalar::Writer => [17], Scalar::Buffer => [18],
            Scalar::Regex => [19], Scalar::Captures => [20], Scalar::CliParsed => [21],
            Scalar::TcpConn => [22], Scalar::TcpListener => [23], Scalar::UdpSocket => [24],
            Scalar::Child => [25], Scalar::File => [26], Scalar::HttpResponse => [27],
            Scalar::HttpServer => [28], Scalar::HttpRequestCtx => [29],
            Scalar::ResponseBuilder => [30], Scalar::HttpStream => [31],
            Scalar::RunOutput => [32], Scalar::Fn(1) => [33, 1, 0x50, 0, 0],
            Scalar::Resource(1) => [34, 1, 0x60, 0, 0],
            Scalar::ResourceRef(1) => [35, 1, 0x60, 0, 0],
            Scalar::RunBytes => [36],
        );
    }

    #[test]
    fn canonical_field_codec_covers_every_root_tag() {
        cases!(encoded_ty;
            Ty::Int(i(8)) => [0, 1, 8], Ty::Float(f(32)) => [1, 32],
            Ty::Bool => [2], Ty::Char => [3], Ty::Option(Scalar::Bool) => [4, 2],
            Ty::Result(Scalar::Bool, Scalar::Char) => [5, 2, 3],
            Ty::Tagged(1) => [6, 1, 0x40, 0, 0], Ty::Box(Scalar::Bool) => [7, 2],
            Ty::Array(Scalar::Bool, 2) => [8, 2, 2, 0, 0, 0],
            Ty::Vec(Scalar::Int(i(8)), 2) => [9, 0, 1, 8, 2, 0, 0, 0],
            Ty::Mask(Scalar::Float(f(32)), 2) => [10, 1, 32, 2, 0, 0, 0],
            Ty::StructArray(1, 2) => [11, 1, 0x10, 0, 0, 2, 0, 0, 0],
            Ty::DynStructArray(1, Layout::Aos) => [12, 1, 0x10, 0, 0, 0],
            Ty::Slice(Scalar::Bool) => [13, 2], Ty::Soa(1) => [14, 1, 0x10, 0, 0],
            Ty::DynSliceArray(PrimScalar::Bool) => [15, 2],
            Ty::DynArray(Scalar::Bool) => [16, 2], Ty::DynResponseArray => [17],
            Ty::Str => [18], Ty::String => [19], Ty::ArenaHandle => [20], Ty::Raw => [21],
            Ty::Builder => [22], Ty::Writer => [23], Ty::Reader => [24], Ty::Buffer => [25],
            Ty::ArrayBuilder(Scalar::Bool) => [26, 0, 2],
            Ty::array_builder(ArrayBuilderElem::Aggregate(AggregateArrayElem::Vec(
                Scalar::Int(i(8)), 2,
            )))
                => [26, 1, 0, 0, 1, 8, 2, 0, 0, 0],
            Ty::StrFinder => [27],
            Ty::File => [28], Ty::Rng => [29], Ty::Regex => [30], Ty::Captures => [31],
            Ty::CliCommand => [32], Ty::CliParsed => [33], Ty::TcpConn => [34],
            Ty::TcpListener => [35], Ty::UdpSocket => [36], Ty::Child => [37],
            Ty::Command => [38], Ty::RunOutput => [39], Ty::HttpRequest => [40],
            Ty::HttpResponse => [41], Ty::HttpClient => [42], Ty::HttpServer => [43],
            Ty::HttpRequestCtx => [44], Ty::ResponseBuilder => [45], Ty::HttpStream => [46],
            Ty::HttpHeaders => [47], Ty::JsonDoc => [48],
            Ty::JsonScanner(1) => [49, 1, 0x10, 0, 0],
            Ty::Struct(1) => [50, 1, 0x10, 0, 0], Ty::Tuple(1) => [51, 1, 0x30, 0, 0],
            Ty::Fn(1) => [52, 1, 0x50, 0, 0], Ty::Enum(1) => [53, 1, 0x20, 0, 0],
            Ty::Task(Scalar::Bool) => [54, 2],
            Ty::DictEncoded(1, 2) => [55, 1, 0x10, 0, 0, 2, 0, 0, 0], Ty::Unit => [56],
            Ty::Resource(1) => [57, 1, 0x60, 0, 0],
            Ty::ResourceRef(1) => [58, 1, 0x60, 0, 0],
            Ty::dyn_aggregate_array(AggregateArrayElem::FixedArray(Scalar::Bool, 2))
                => [59, 2, 2, 2, 0, 0, 0],
            Ty::dyn_aggregate_array(AggregateArrayElem::Mask(Scalar::Float(f(32)), 4))
                => [59, 1, 1, 32, 4, 0, 0, 0],
            Ty::RunBytes => [60],
            Ty::dyn_aggregate_array(AggregateArrayElem::FixedStructArray(1, 2))
                => [59, 3, 1, 0x10, 0, 0, 2, 0, 0, 0],
        );
    }

    #[test]
    fn canonical_field_codec_encodes_payloads_and_modes_exactly() {
        let mut out = vec![0xa5];
        text(&mut out, "é").unwrap();
        assert_eq!(out, [0xa5, 2, 0, 0, 0, 0xc3, 0xa9]);

        bytes!(encoded_scalar(Scalar::Struct(7)), [5, 7, 0x10, 0, 0]);
        bytes!(
            encoded_scalar(Scalar::DynArray(PrimScalar::Int(i(16)))),
            [7, 0, 1, 16]
        );
        bytes!(
            encoded_ty(Ty::Result(Scalar::Bool, Scalar::Fn(3))),
            [5, 2, 33, 3, 0x50, 0, 0]
        );
        bytes!(
            encoded_ty(Ty::Array(Scalar::Char, 0x0102_0304)),
            [8, 3, 4, 3, 2, 1]
        );
        bytes!(
            encoded_ty(Ty::DynStructArray(5, Layout::Soa)),
            [12, 5, 0x10, 0, 0, 1]
        );
        bytes!(
            encoded_ty(Ty::DictEncoded(6, 0x0102_0304)),
            [55, 6, 0x10, 0, 0, 4, 3, 2, 1]
        );

        out.truncate(1);
        encode_param_mode(&mut out, ParamMode::ByValue).unwrap();
        encode_param_mode(&mut out, ParamMode::Out).unwrap();
        assert_eq!(out, [0xa5, 0, 1]);
    }

    #[test]
    fn canonical_field_codec_accepts_only_settled_widths_and_lanes() {
        for bits in u8::MIN..=u8::MAX {
            for signed in [false, true] {
                assert_eq!(
                    appended(|out| int(out, signed, bits)).is_ok(),
                    matches!(bits, 8 | 16 | 32 | 64)
                );
            }
            assert_eq!(
                appended(|out| float(out, bits)).is_ok(),
                matches!(bits, 32 | 64)
            );
        }
        for lanes in [2, 4, 8, 16] {
            encoded_ty(Ty::Vec(Scalar::Int(i(32)), lanes)).unwrap();
            encoded_ty(Ty::Vec(Scalar::Float(f(32)), lanes)).unwrap();
            encoded_ty(Ty::Mask(Scalar::Int(i(64)), lanes)).unwrap();
            encoded_ty(Ty::Mask(Scalar::Float(f(64)), lanes)).unwrap();
        }
        for lanes in [0, 1, 3, 5, 7, 9, 15, 17, u32::MAX] {
            for value in [
                Ty::Vec(Scalar::Int(i(32)), lanes),
                Ty::Mask(Scalar::Float(f(64)), lanes),
            ] {
                error!(encoded_ty(value), CanonicalGraphError::InvalidWidth);
            }
        }
        for value in [Ty::Vec(Scalar::Bool, 4), Ty::Mask(Scalar::Char, 4)] {
            error!(encoded_ty(value), CanonicalGraphError::InvalidWidth);
        }
    }

    #[test]
    fn canonical_field_codec_maps_typed_semantic_errors_exactly() {
        let mut out = vec![0xa5, 0x5a];
        error!(text(&mut out, "a\0b"), CanonicalGraphError::EmbeddedNul);
        assert_eq!(out, [0xa5, 0x5a]);
        error!(
            encoded_scalar(Scalar::Param(0)),
            CanonicalGraphError::InvalidGraph
        );
        error!(
            encoded_scalar(Scalar::SoaParam(0)),
            CanonicalGraphError::InvalidGraph
        );
        for value in [
            Ty::Param(0),
            Ty::SoaParam(0),
            Ty::IntVar(0),
            Ty::FloatVar(0),
            Ty::Error,
        ] {
            error!(encoded_ty(value), CanonicalGraphError::InvalidGraph);
        }
        let mut modes = Vec::new();
        encode_param_mode(&mut modes, ParamMode::Borrow).unwrap();
        encode_param_mode(&mut modes, ParamMode::BorrowMut).unwrap();
        assert_eq!(modes, [2, 3]);
        error!(
            prim(&mut out, PrimScalar::Int(i(24))),
            CanonicalGraphError::InvalidWidth
        );
        assert_eq!(out, [0xa5, 0x5a]);
        error!(
            scalar(&mut out, Scalar::Struct(0), &|_| Err(
                CanonicalGraphError::MissingReference
            )),
            CanonicalGraphError::MissingReference
        );
        assert_eq!(out, [0xa5, 0x5a]);
        error!(
            ty(
                &mut out,
                Ty::Result(Scalar::Bool, Scalar::Param(0)),
                &ordinal
            ),
            CanonicalGraphError::InvalidGraph
        );
        assert_eq!(out, [0xa5, 0x5a]);
    }

    #[test]
    fn canonical_field_codec_checks_counts_and_forms_function_records() {
        assert_eq!(checked_count(0), Ok(0));
        assert_eq!(checked_count(u32::MAX as usize), Ok(u32::MAX));
        #[cfg(target_pointer_width = "64")]
        error!(
            checked_count(u32::MAX as usize + 1),
            CanonicalGraphError::InvalidCount
        );

        let definition = FunctionTypeDef {
            params: vec![(ParamMode::Out, Scalar::Bool)],
            ret: Ty::Unit,
            return_borrow: hir::ReturnBorrowSummary::None,
            return_region: hir::ReturnRegionSummary::None,
            return_cleanup: hir::ReturnCleanupAbi::None,
        };
        assert_eq!(definition.params, [(ParamMode::Out, Scalar::Bool)]);
        assert_eq!(definition.ret, Ty::Unit);
        assert!(matches!(
            (definition.return_borrow, definition.return_region),
            (
                hir::ReturnBorrowSummary::None,
                hir::ReturnRegionSummary::None
            )
        ));
    }
}
