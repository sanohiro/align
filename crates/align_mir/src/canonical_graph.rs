use std::collections::{HashMap, HashSet};

use align_ast::ParamMode;
use align_sema::{hir, Layout, PrimScalar, Scalar, Ty};

use super::source_shape::{source_shape_equal, SourceShapeNode, SourceShapeView};

#[derive(Clone, Debug)]
pub struct FunctionTypeDef {
    pub params: Vec<(ParamMode, Scalar)>,
    pub ret: Ty,
    pub return_borrow: hir::ReturnBorrowSummary,
    pub return_region: hir::ReturnRegionSummary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum CanonicalGraphError {
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
}

#[derive(Clone, Copy)]
struct CanonicalTypeView<'a> {
    structs: &'a [hir::StructDef],
    enums: &'a [hir::EnumDef],
    tuples: &'a [hir::TupleDef],
    tagged_types: &'a [hir::TaggedType],
    fn_types: &'a [FunctionTypeDef],
}

impl SourceShapeView for CanonicalTypeView<'_> {
    fn source_shape_node(&self, node: Node) -> Option<SourceShapeNode<'_>> {
        match node {
            Node::Struct(id) => {
                self.structs
                    .get(id as usize)
                    .map(|definition| SourceShapeNode::Struct {
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
                    })
            }
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
        let mut validator = GraphValidator {
            view,
            pending: Vec::new(),
            seen: HashSet::new(),
            order: Vec::new(),
            candidates: Vec::new(),
            next_ordinal: 0,
            end_ordinals: HashMap::new(),
        };
        let mut roots = Vec::new();
        validator.scan_ty(root, &mut roots);
        roots.reverse();
        validator.pending.extend(roots);
        while let Some(node) = validator.pending.pop() {
            validator.visit_node(node);
        }
        validator.collect_cross_node_candidates();
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
}

#[derive(Clone, Copy)]
struct ErrorCandidate {
    ordinal: u64,
    tie_rank: u8,
    error: CanonicalGraphError,
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
                source_name,
                align,
                fields,
                ..
            } => {
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
                    self.validate_identifier(&field.name, name_ordinal);
                    if !names.insert(field.name.as_str()) {
                        self.candidate(name_ordinal, CanonicalGraphError::DuplicateMember);
                    }
                    self.scan_ty(field.ty, &mut references);
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
                        self.scan_scalar(value, &mut references);
                    }
                }
            }
            SourceShapeNode::Tuple { elems } => {
                let count_ordinal = self.field_ordinal();
                self.validate_count(elems.len(), count_ordinal);
                for &value in elems {
                    self.scan_scalar(value, &mut references);
                }
            }
            SourceShapeNode::Tagged(value) => match value {
                hir::TaggedType::Option(value) => {
                    self.field_ordinal();
                    self.scan_scalar(*value, &mut references);
                }
                hir::TaggedType::Result(ok, err) => {
                    self.field_ordinal();
                    self.scan_scalar(*ok, &mut references);
                    self.scan_scalar(*err, &mut references);
                }
            },
            SourceShapeNode::Function {
                params,
                ret,
                return_borrow,
                return_region,
            } => {
                let count_ordinal = self.field_ordinal();
                self.validate_count(params.len(), count_ordinal);
                for &(mode, value) in params {
                    let mode_ordinal = self.field_ordinal();
                    if !matches!(mode, ParamMode::ByValue | ParamMode::Out) {
                        self.candidate(mode_ordinal, CanonicalGraphError::InvalidGraph);
                    }
                    self.scan_scalar(value, &mut references);
                }
                self.scan_ty(*ret, &mut references);
                self.scan_borrow_summary(return_borrow, params.len());
                let region_ordinal = self.scan_region_summary(return_region, params.len());
                if !summaries_agree(return_borrow, return_region) {
                    self.candidate(region_ordinal, CanonicalGraphError::InvalidSummary);
                }
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
                    if let Some(definition) = view.structs.get(id as usize) {
                        if let Some(error) = Self::compare_nominal(
                            view,
                            node,
                            &definition.source_name,
                            &mut nominal_sources,
                            &mut known_shapes,
                        ) {
                            self.candidate(end_ordinal, error);
                        }
                    }
                }
                Node::Enum(id) => {
                    if let Some(definition) = view.enums.get(id as usize) {
                        if let Some(error) = Self::compare_nominal(
                            view,
                            node,
                            &definition.source_name,
                            &mut nominal_sources,
                            &mut known_shapes,
                        ) {
                            self.candidate(end_ordinal, error);
                        }
                    }
                }
                Node::Tuple(id) => {
                    if let Some(definition) = view.tuples.get(id as usize) {
                        if tuples.insert(definition.elems.clone(), node).is_some() {
                            self.candidate(end_ordinal, CanonicalGraphError::DuplicateMember);
                        }
                    }
                }
                Node::Tagged(_) | Node::Fn(_) => {}
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
        Some(if same_shape {
            CanonicalGraphError::DuplicateMember
        } else {
            CanonicalGraphError::InvalidGraph
        })
    }

    fn scan_scalar(&mut self, value: Scalar, references: &mut Vec<Node>) {
        let ordinal = self.field_ordinal();
        match value {
            Scalar::Struct(id) | Scalar::DynStructArray(id) | Scalar::Soa(id) => {
                self.scan_reference(Node::Struct(id), ordinal, references)
            }
            Scalar::Enum(id) => self.scan_reference(Node::Enum(id), ordinal, references),
            Scalar::Tagged(id) => self.scan_reference(Node::Tagged(id), ordinal, references),
            Scalar::Fn(id) => self.scan_reference(Node::Fn(id), ordinal, references),
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

    fn scan_ty(&mut self, value: Ty, references: &mut Vec<Node>) {
        let ordinal = self.field_ordinal();
        match value {
            Ty::Option(value)
            | Ty::Box(value)
            | Ty::Slice(value)
            | Ty::DynArray(value)
            | Ty::ArrayBuilder(value)
            | Ty::Task(value) => self.scan_scalar(value, references),
            Ty::Result(ok, err) => {
                self.scan_scalar(ok, references);
                self.scan_scalar(err, references);
            }
            Ty::Array(value, _) => {
                self.scan_scalar(value, references);
                self.field_ordinal();
            }
            Ty::Vec(value, lanes) | Ty::Mask(value, lanes) => {
                let scalar_ordinal = self.next_ordinal;
                self.scan_scalar(value, references);
                let lanes_ordinal = self.field_ordinal();
                if !matches!(value, Scalar::Int(_) | Scalar::Float(_)) {
                    self.candidate(scalar_ordinal, CanonicalGraphError::InvalidWidth);
                }
                if !matches!(lanes, 2 | 4 | 8 | 16) {
                    self.candidate(lanes_ordinal, CanonicalGraphError::InvalidWidth);
                }
            }
            Ty::StructArray(id, _) | Ty::DictEncoded(id, _) => {
                self.scan_reference(Node::Struct(id), ordinal, references);
                self.field_ordinal();
            }
            Ty::DynStructArray(id, _) => {
                self.scan_reference(Node::Struct(id), ordinal, references);
                self.field_ordinal();
            }
            Ty::Tagged(id) => self.scan_reference(Node::Tagged(id), ordinal, references),
            Ty::Soa(id) | Ty::JsonScanner(id) | Ty::Struct(id) => {
                self.scan_reference(Node::Struct(id), ordinal, references)
            }
            Ty::Tuple(id) => self.scan_reference(Node::Tuple(id), ordinal, references),
            Ty::Fn(id) => self.scan_reference(Node::Fn(id), ordinal, references),
            Ty::Enum(id) => self.scan_reference(Node::Enum(id), ordinal, references),
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
        if roots.is_empty() {
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
        if !captures.is_empty() {
            self.candidate(captures_count, CanonicalGraphError::InvalidSummary);
        }
        for _ in captures {
            self.field_ordinal();
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
    let mut pending = type_nodes(graph.root);
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
    out.push(1);
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
    ty(&mut out, graph.root, &ordinal)?;
    Ok(out)
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
            Ok(())
        }
    })
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
        _ => Vec::new(),
    }
}

fn type_nodes(value: Ty) -> Vec<Node> {
    match value {
        Ty::Option(value)
        | Ty::Box(value)
        | Ty::Slice(value)
        | Ty::DynArray(value)
        | Ty::ArrayBuilder(value)
        | Ty::Task(value)
        | Ty::Array(value, _)
        | Ty::Vec(value, _)
        | Ty::Mask(value, _) => scalar_nodes(value),
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
        ParamMode::Borrow | ParamMode::BorrowMut => {
            return Err(CanonicalGraphError::InvalidGraph);
        }
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
            Scalar::Param(_) => Err(CanonicalGraphError::InvalidGraph),
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
                scalar(out, v, ordinal)
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
            Ty::Param(_) | Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Error => {
                Err(CanonicalGraphError::InvalidGraph)
            }
        }
    })
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
            },
        )?;
        canonical_type_bytes(&graph)
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
            validate(Ty::Struct(0), &nominal),
            Err(CanonicalGraphError::DuplicateMember)
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
        program.structs[1].fields[0].ty = Ty::Struct(1);
        let order = validate(Ty::Struct(0), &program).unwrap();
        assert_eq!(order, [Node::Struct(0), Node::Struct(1)]);
        let view = CanonicalTypeView {
            structs: &program.structs,
            enums: &program.enums,
            tuples: &program.tuples,
            tagged_types: &program.tagged_types,
            fn_types: &[],
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
        assert_eq!(canonical(Ty::Unit, &program).unwrap(), [1, 0, 0, 0, 0, 56]);
        assert_eq!(canonical(Ty::Bool, &program).unwrap(), [1, 0, 0, 0, 0, 2]);
        assert_eq!(
            canonical(Ty::Int(i(64)), &program).unwrap(),
            [1, 0, 0, 0, 0, 0, 1, 64]
        );
        let bytes = canonical(Ty::Struct(0), &program).unwrap();
        assert_eq!(&bytes[..5], [1, 1, 0, 0, 0]);
        assert_eq!(bytes.last(), Some(&0));
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
        assert_eq!(&bytes[..5], [1, 3, 0, 0, 0]);

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
        assert_eq!(&bytes[..5], [1, 0, 16, 0, 0]);
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
            Ty::ArrayBuilder(Scalar::Bool) => [26, 2], Ty::StrFinder => [27],
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
        for value in [Ty::Param(0), Ty::IntVar(0), Ty::FloatVar(0), Ty::Error] {
            error!(encoded_ty(value), CanonicalGraphError::InvalidGraph);
        }
        for mode in [ParamMode::Borrow, ParamMode::BorrowMut] {
            error!(
                encode_param_mode(&mut out, mode),
                CanonicalGraphError::InvalidGraph
            );
            assert_eq!(out, [0xa5, 0x5a]);
        }
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
