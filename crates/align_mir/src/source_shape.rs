use std::collections::{HashMap, HashSet, VecDeque};

use align_ast::ParamMode;
use align_sema::{AggregateArrayElem, Scalar, Ty, hir};

use super::canonical_graph::Node;

pub(super) enum SourceShapeNode<'a> {
    Struct {
        name: &'a str,
        source_name: &'a str,
        align: &'a Option<u32>,
        c_repr: &'a bool,
        fields: &'a [hir::FieldDef],
    },
    Enum {
        source_name: &'a str,
        variants: &'a [hir::EnumVariant],
    },
    Resource(&'a hir::ResourceDef),
    Tuple {
        elems: &'a [Scalar],
    },
    Tagged(&'a hir::TaggedType),
    Function {
        params: &'a [(ParamMode, Scalar)],
        ret: &'a Ty,
        return_borrow: &'a hir::ReturnBorrowSummary,
        return_region: &'a hir::ReturnRegionSummary,
        return_cleanup: hir::ReturnCleanupAbi,
    },
}

pub(super) trait SourceShapeView {
    fn source_shape_node(&self, node: Node) -> Option<SourceShapeNode<'_>>;
}

trait SourceShapeObserver {
    fn node(&mut self, node: Node, edges: usize);
    fn pair(&mut self, pass: usize, left: Node, right: Node);
    fn work(&mut self, units: usize);
}

impl SourceShapeObserver for () {
    #[inline]
    fn node(&mut self, _node: Node, _edges: usize) {}
    #[inline]
    fn pair(&mut self, _pass: usize, _left: Node, _right: Node) {}
    #[inline]
    fn work(&mut self, _units: usize) {}
}

impl SourceShapeView for hir::Program {
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
            Node::Resource(id) => self.resources.get(id as usize).map(SourceShapeNode::Resource),
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
        }
    }
}

pub(super) fn source_shape_equal<V: SourceShapeView + ?Sized>(
    view: &V,
    left: Node,
    right: Node,
    known_shapes: &mut HashSet<(Node, Node)>,
) -> bool {
    source_shape_equal_observed(view, left, right, known_shapes, &mut ())
}

fn source_shape_equal_observed<V: SourceShapeView + ?Sized, O: SourceShapeObserver + ?Sized>(
    view: &V,
    left: Node,
    right: Node,
    known_shapes: &mut HashSet<(Node, Node)>,
    observer: &mut O,
) -> bool {
    let mut comparator = SourceShapeComparator {
        view,
        observer,
        known_shapes: &*known_shapes,
        root: (left, right),
        pass: 0,
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

struct SourceShapeComparator<'a, V: ?Sized, O: ?Sized> {
    view: &'a V,
    observer: &'a mut O,
    known_shapes: &'a HashSet<(Node, Node)>,
    root: (Node, Node),
    pass: usize,
    cache_enabled: bool,
    pending: VecDeque<(Node, Node)>,
    seen: HashSet<(Node, Node)>,
    left_to_right: HashMap<Node, Node>,
    right_to_left: HashMap<Node, Node>,
}

impl<V: SourceShapeView + ?Sized, O: SourceShapeObserver + ?Sized> SourceShapeComparator<'_, V, O> {
    fn run(&mut self) -> bool {
        loop {
            let mut restart = false;
            while let Some((left, right)) = self.pending.pop_front() {
                if !self.map_pair(left, right) {
                    return false;
                }
                self.observer.pair(self.pass, left, right);
                if self.cache_enabled && self.known_shapes.contains(&(left, right)) {
                    if !self.seen.is_empty() || !self.pending.is_empty() {
                        restart = true;
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
            if !restart {
                return true;
            }
            self.cache_enabled = false;
            self.pass += 1;
            self.pending.clear();
            self.pending.push_back(self.root);
            self.seen.clear();
            self.left_to_right.clear();
            self.right_to_left.clear();
        }
    }

    fn map_pair(&mut self, left: Node, right: Node) -> bool {
        if self
            .left_to_right
            .get(&left)
            .is_some_and(|mapped| *mapped != right)
            || self
                .right_to_left
                .get(&right)
                .is_some_and(|mapped| *mapped != left)
        {
            return false;
        }
        self.left_to_right.insert(left, right);
        self.right_to_left.insert(right, left);
        true
    }

    fn nodes_equal(&mut self, left_node: Node, right_node: Node) -> bool {
        let view = self.view;
        let Some(left) = view.source_shape_node(left_node) else {
            return false;
        };
        let Some(right) = view.source_shape_node(right_node) else {
            return false;
        };
        let (left_edges, work) = shape_cost(&left);
        let (right_edges, _) = shape_cost(&right);
        self.observer.node(left_node, left_edges);
        self.observer.node(right_node, right_edges);
        self.observer.work(work);
        match (left, right) {
            (
                SourceShapeNode::Struct {
                    source_name: left_name,
                    align: left_align,
                    c_repr: left_c_repr,
                    fields: left_fields,
                    ..
                },
                SourceShapeNode::Struct {
                    source_name: right_name,
                    align: right_align,
                    c_repr: right_c_repr,
                    fields: right_fields,
                    ..
                },
            ) => {
                if left_name != right_name
                    || left_align != right_align
                    || left_c_repr != right_c_repr
                    || left_fields.len() != right_fields.len()
                {
                    return false;
                }
                left_fields.iter().zip(right_fields).all(|(left, right)| {
                    left.name == right.name && self.types_equal(left.ty, right.ty)
                })
            }
            (SourceShapeNode::Resource(left), SourceShapeNode::Resource(right)) => left == right,
            (
                SourceShapeNode::Enum {
                    source_name: left_name,
                    variants: left_variants,
                },
                SourceShapeNode::Enum {
                    source_name: right_name,
                    variants: right_variants,
                },
            ) => {
                if left_name != right_name || left_variants.len() != right_variants.len() {
                    return false;
                }
                left_variants
                    .iter()
                    .zip(right_variants)
                    .all(|(left, right)| {
                        left.name == right.name
                            && left.field_base == right.field_base
                            && left.payload.len() == right.payload.len()
                            && left
                                .payload
                                .iter()
                                .zip(&right.payload)
                                .all(|(&left, &right)| self.scalars_equal(left, right))
                    })
            }
            (SourceShapeNode::Tuple { elems: left }, SourceShapeNode::Tuple { elems: right }) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(&left, &right)| self.scalars_equal(left, right))
            }
            (SourceShapeNode::Tagged(left), SourceShapeNode::Tagged(right)) => {
                match (*left, *right) {
                    (hir::TaggedType::Option(left), hir::TaggedType::Option(right)) => {
                        self.scalars_equal(left, right)
                    }
                    (
                        hir::TaggedType::Result(left_ok, left_err),
                        hir::TaggedType::Result(right_ok, right_err),
                    ) => {
                        self.scalars_equal(left_ok, right_ok)
                            && self.scalars_equal(left_err, right_err)
                    }
                    _ => false,
                }
            }
            (
                SourceShapeNode::Function {
                    params: left_params,
                    ret: left_ret,
                    return_borrow: left_borrow,
                    return_region: left_region,
                    return_cleanup: left_cleanup,
                },
                SourceShapeNode::Function {
                    params: right_params,
                    ret: right_ret,
                    return_borrow: right_borrow,
                    return_region: right_region,
                    return_cleanup: right_cleanup,
                },
            ) => {
                if left_cleanup != right_cleanup {
                    return false;
                }
                if left_params.len() != right_params.len()
                    || left_borrow != right_borrow
                    || left_region != right_region
                {
                    return false;
                }
                for ((left_mode, left), (right_mode, right)) in left_params.iter().zip(right_params)
                {
                    if left_mode != right_mode || !self.scalars_equal(*left, *right) {
                        return false;
                    }
                }
                self.types_equal(*left_ret, *right_ret)
            }
            _ => false,
        }
    }

    fn queue(&mut self, left: Node, right: Node) {
        self.pending.push_back((left, right));
    }

    fn scalars_equal(&mut self, left: Scalar, right: Scalar) -> bool {
        macro_rules! node {
            ($variant:ident, $kind:ident, $left:expr) => {
                match right {
                    Scalar::$variant(right) => {
                        self.queue_equal(Node::$kind($left), Node::$kind(right))
                    }
                    _ => false,
                }
            };
        }
        match left {
            Scalar::Struct(left) => node!(Struct, Struct, left),
            Scalar::DynStructArray(left) => node!(DynStructArray, Struct, left),
            Scalar::Soa(left) => node!(Soa, Struct, left),
            Scalar::Enum(left) => node!(Enum, Enum, left),
            Scalar::Tagged(left) => node!(Tagged, Tagged, left),
            Scalar::Fn(left) => node!(Fn, Fn, left),
            Scalar::Resource(left) => node!(Resource, Resource, left),
            Scalar::ResourceRef(left) => node!(ResourceRef, Resource, left),
            Scalar::Int(_)
            | Scalar::Float(_)
            | Scalar::DynArray(_)
            | Scalar::Slice(_)
            | Scalar::Param(_)
            | Scalar::SoaParam(_)
            | Scalar::Bool
            | Scalar::Char
            | Scalar::Unit
            | Scalar::String
            | Scalar::DynResponseArray
            | Scalar::Str
            | Scalar::JsonDoc
            | Scalar::Reader
            | Scalar::Writer
            | Scalar::Logger
            | Scalar::Buffer
            | Scalar::SignatureKey(_)
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
            | Scalar::HttpReadStream
            | Scalar::HttpSseStream
            | Scalar::RunOutput
            | Scalar::RunBytes => left == right,
        }
    }

    fn aggregate_array_elems_equal(
        &mut self,
        left: AggregateArrayElem,
        right: AggregateArrayElem,
    ) -> bool {
        match (left, right) {
            (AggregateArrayElem::Vec(left, a), AggregateArrayElem::Vec(right, b))
            | (AggregateArrayElem::Mask(left, a), AggregateArrayElem::Mask(right, b))
            | (AggregateArrayElem::FixedArray(left, a), AggregateArrayElem::FixedArray(right, b)) => {
                a == b && self.scalars_equal(left, right)
            }
            (
                AggregateArrayElem::FixedStructArray(left, a),
                AggregateArrayElem::FixedStructArray(right, b),
            ) => a == b && self.queue_equal(Node::Struct(left), Node::Struct(right)),
            _ => false,
        }
    }

    fn types_equal(&mut self, left: Ty, right: Ty) -> bool {
        macro_rules! same {
            ($pattern:pat => $body:expr) => {
                match right {
                    $pattern => $body,
                    _ => false,
                }
            };
        }
        macro_rules! node {
            ($variant:ident, $kind:ident, $left:expr) => {
                same!(Ty::$variant(right) => self.queue_equal(Node::$kind($left), Node::$kind(right)))
            };
        }
        match left {
            Ty::Option(left) => same!(Ty::Option(right) => self.scalars_equal(left, right)),
            Ty::Result(a, b) => {
                same!(Ty::Result(c, d) => self.scalars_equal(a, c) && self.scalars_equal(b, d))
            }
            Ty::Box(left) => same!(Ty::Box(right) => self.scalars_equal(left, right)),
            Ty::Array(left, a) => {
                same!(Ty::Array(right, b) => a == b && self.scalars_equal(left, right))
            }
            Ty::Vec(left, a) => {
                same!(Ty::Vec(right, b) => a == b && self.scalars_equal(left, right))
            }
            Ty::Mask(left, a) => {
                same!(Ty::Mask(right, b) => a == b && self.scalars_equal(left, right))
            }
            Ty::Slice(left) => same!(Ty::Slice(right) => self.scalars_equal(left, right)),
            Ty::DynArray(left) => same!(Ty::DynArray(right) => self.scalars_equal(left, right)),
            left @ (Ty::DynVecArray(..)
            | Ty::DynMaskArray(..)
            | Ty::DynFixedArray(..)
            | Ty::DynFixedStructArray(..)) => right
                .dyn_aggregate_array_element()
                .is_some_and(|right| {
                    self.aggregate_array_elems_equal(
                        left.dyn_aggregate_array_element().expect("matched aggregate array"),
                        right,
                    )
                }),
            Ty::ArrayBuilder(left) => {
                same!(Ty::ArrayBuilder(right) => self.scalars_equal(left, right))
            }
            left @ (Ty::VecArrayBuilder(..)
            | Ty::MaskArrayBuilder(..)
            | Ty::FixedArrayBuilder(..)
            | Ty::FixedStructArrayBuilder(..)) => right
                .array_builder_element()
                .and_then(|element| match element {
                    align_sema::ArrayBuilderElem::Aggregate(element) => Some(element),
                    align_sema::ArrayBuilderElem::Scalar(_) => None,
                })
                .is_some_and(|right| {
                    let align_sema::ArrayBuilderElem::Aggregate(left) =
                        left.array_builder_element().expect("matched aggregate builder")
                    else {
                        unreachable!()
                    };
                    self.aggregate_array_elems_equal(left, right)
                }),
            Ty::Task(left) => same!(Ty::Task(right) => self.scalars_equal(left, right)),
            Ty::Tagged(left) => node!(Tagged, Tagged, left),
            Ty::StructArray(left, a) => {
                same!(Ty::StructArray(right, b) => a == b && self.queue_equal(Node::Struct(left), Node::Struct(right)))
            }
            Ty::DynStructArray(left, a) => {
                same!(Ty::DynStructArray(right, b) => a == b && self.queue_equal(Node::Struct(left), Node::Struct(right)))
            }
            Ty::Soa(left) => node!(Soa, Struct, left),
            Ty::JsonScanner(left) => node!(JsonScanner, Struct, left),
            Ty::DictEncoded(left, a) => {
                same!(Ty::DictEncoded(right, b) => a == b && self.queue_equal(Node::Struct(left), Node::Struct(right)))
            }
            Ty::Struct(left) => node!(Struct, Struct, left),
            Ty::Tuple(left) => node!(Tuple, Tuple, left),
            Ty::Fn(left) => node!(Fn, Fn, left),
            Ty::Enum(left) => node!(Enum, Enum, left),
            Ty::Resource(left) => node!(Resource, Resource, left),
            Ty::ResourceRef(left) => node!(ResourceRef, Resource, left),
            Ty::Int(_)
            | Ty::Float(_)
            | Ty::Param(_)
            | Ty::SoaParam(_)
            | Ty::IntVar(_)
            | Ty::FloatVar(_)
            | Ty::DynSliceArray(_)
            | Ty::Bool
            | Ty::Char
            | Ty::DynResponseArray
            | Ty::Str
            | Ty::String
            | Ty::ArenaHandle
            | Ty::Raw
            | Ty::Builder
            | Ty::Writer
            | Ty::Reader
            | Ty::Logger
            | Ty::Buffer
            | Ty::SignatureKey(_)
            | Ty::StrFinder
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
            | Ty::RunBytes
            | Ty::HttpRequest
            | Ty::HttpResponse
            | Ty::HttpClient
            | Ty::HttpServer
            | Ty::HttpRequestCtx
            | Ty::ResponseBuilder
            | Ty::HttpStream
            | Ty::HttpReadStream
            | Ty::HttpSseStream
            | Ty::HttpHeaders
            | Ty::JsonDoc
            | Ty::Unit
            | Ty::Error => left == right,
        }
    }

    fn queue_equal(&mut self, left: Node, right: Node) -> bool {
        self.queue(left, right);
        true
    }
}

#[inline]
fn scalar_cost(value: Scalar) -> (usize, usize) {
    match value {
        Scalar::Struct(_)
        | Scalar::DynStructArray(_)
        | Scalar::Soa(_)
        | Scalar::Enum(_)
        | Scalar::Tagged(_)
        | Scalar::Fn(_)
        | Scalar::Resource(_)
        | Scalar::ResourceRef(_) => (1, 1),
        _ => (0, 1),
    }
}

#[inline]
fn ty_cost(value: Ty) -> (usize, usize) {
    let child = match value {
        Ty::Option(value)
        | Ty::Box(value)
        | Ty::Slice(value)
        | Ty::DynArray(value)
        | Ty::Task(value) => scalar_cost(value),
        Ty::ArrayBuilder(value) => scalar_cost(value),
        Ty::VecArrayBuilder(value, _)
        | Ty::MaskArrayBuilder(value, _)
        | Ty::FixedArrayBuilder(value, _)
        | Ty::DynVecArray(value, _)
        | Ty::DynMaskArray(value, _)
        | Ty::DynFixedArray(value, _) => scalar_cost(value),
        Ty::FixedStructArrayBuilder(..) | Ty::DynFixedStructArray(..) => (1, 1),
        Ty::Result(left, right) => {
            let left = scalar_cost(left);
            let right = scalar_cost(right);
            (left.0 + right.0, left.1 + right.1)
        }
        Ty::Array(value, _) | Ty::Vec(value, _) | Ty::Mask(value, _) => scalar_cost(value),
        Ty::Tagged(_)
        | Ty::StructArray(_, _)
        | Ty::DynStructArray(_, _)
        | Ty::Soa(_)
        | Ty::JsonScanner(_)
        | Ty::DictEncoded(_, _)
        | Ty::Struct(_)
        | Ty::Tuple(_)
        | Ty::Fn(_)
        | Ty::Enum(_)
        | Ty::Resource(_)
        | Ty::ResourceRef(_) => (1, 1),
        _ => (0, 0),
    };
    (child.0, child.1 + 1)
}

#[inline]
fn shape_cost(node: &SourceShapeNode<'_>) -> (usize, usize) {
    match node {
        SourceShapeNode::Struct {
            source_name,
            fields,
            ..
        } => fields
            .iter()
            .fold((0, 3 + source_name.len()), |(edges, work), field| {
                let cost = ty_cost(field.ty);
                (edges + cost.0, work + 1 + field.name.len() + cost.1)
            }),
        SourceShapeNode::Enum {
            source_name,
            variants,
        } => variants.iter().fold(
            (0, 2 + source_name.len()),
            |(mut edges, mut work), variant| {
                work += 2 + variant.name.len();
                for &value in &variant.payload {
                    let cost = scalar_cost(value);
                    edges += cost.0;
                    work += 1 + cost.1;
                }
                (edges, work)
            },
        ),
        SourceShapeNode::Tuple { elems } => elems.iter().fold((0, 1), |(edges, work), &value| {
            let cost = scalar_cost(value);
            (edges + cost.0, work + 1 + cost.1)
        }),
        SourceShapeNode::Tagged(value) => match value {
            hir::TaggedType::Option(value) => {
                let cost = scalar_cost(*value);
                (cost.0, 2 + cost.1)
            }
            hir::TaggedType::Result(ok, err) => {
                let ok = scalar_cost(*ok);
                let err = scalar_cost(*err);
                (ok.0 + err.0, 3 + ok.1 + err.1)
            }
        },
        SourceShapeNode::Function {
            params,
            ret,
            return_borrow,
            return_region,
            return_cleanup: _,
        } => {
            let mut cost = ty_cost(**ret);
            cost.1 += 4 + borrow_summary_work(return_borrow) + region_summary_work(return_region);
            for (_, value) in *params {
                let value = scalar_cost(*value);
                cost.0 += value.0;
                cost.1 += 2 + value.1;
            }
            cost
        }
        SourceShapeNode::Resource(resource) => (
            0,
            7 + resource.name.len()
                + resource.source_name.len()
                + resource.drop_hook.len()
                + resource.drop_thunk.len(),
        ),
    }
}

#[inline]
fn borrow_summary_work(summary: &hir::ReturnBorrowSummary) -> usize {
    match summary {
        hir::ReturnBorrowSummary::None => 1,
        hir::ReturnBorrowSummary::Roots { params, captures } => 3 + params.len() + captures.len(),
    }
}

#[inline]
fn region_summary_work(summary: &hir::ReturnRegionSummary) -> usize {
    match summary {
        hir::ReturnRegionSummary::None => 1,
        hir::ReturnRegionSummary::Roots { params, captures } => 3 + params.len() + captures.len(),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::validate_hir_tests::baseline_program;
    use align_sema::{FloatTy, IntTy, Layout};
    use std::collections::HashSet;

    #[derive(Default)]
    struct Metrics {
        nodes: HashMap<Node, usize>,
        pairs: HashSet<(usize, Node, Node)>,
        work: usize,
    }

    impl SourceShapeObserver for Metrics {
        fn node(&mut self, node: Node, edges: usize) {
            self.nodes.entry(node).or_insert(edges);
        }

        fn pair(&mut self, pass: usize, left: Node, right: Node) {
            self.pairs.insert((pass, left, right));
        }

        fn work(&mut self, units: usize) {
            self.work += units;
        }
    }

    impl Metrics {
        fn counts(&self) -> (usize, usize, usize, usize) {
            (
                self.nodes.len(),
                self.nodes.values().sum(),
                self.pairs.len(),
                self.work,
            )
        }
    }
    fn i(bits: u8) -> IntTy {
        IntTy { bits, signed: true }
    }
    fn twin_program() -> hir::Program {
        let mut program = baseline_program();
        program.fn_types[0].params = vec![(ParamMode::ByValue, Scalar::Bool)];
        program.structs.push(program.structs[0].clone());
        program.enums.push(program.enums[0].clone());
        program.tuples.push(program.tuples[0].clone());
        program.tagged_types.push(program.tagged_types[0]);
        program.fn_types.push(program.fn_types[0].clone());
        let resource = hir::ResourceDef {
            name: "pkg$Resource".into(),
            source_name: "Resource".into(),
            declaring_module: "pkg".into(),
            generic_arity: 0,
            drop_hook: "pkg$drop_resource".into(),
            drop_thunk: "pkg$drop_resource$thunk".into(),
            representation_version: 1,
            drop_abi_fingerprint: [7; 16],
        };
        program.resources.push(resource.clone());
        program.resources.push(resource);
        program
    }
    fn equal(view: &(impl SourceShapeView + ?Sized), left: Node, right: Node) -> bool {
        source_shape_equal(view, left, right, &mut HashSet::new())
    }
    fn ty_equal(left: Ty, right: Ty) -> bool {
        let mut program = twin_program();
        let id = program.structs.len() as u32;
        for (name, ty) in [("L", left), ("R", right)] {
            let mut root = program.structs[0].clone();
            root.name = name.into();
            root.source_name = "Root".into();
            root.fields[0].ty = ty;
            program.structs.push(root);
        }
        equal(&program, Node::Struct(id), Node::Struct(id + 1))
    }
    pub(crate) fn assert_ty_matrix(values: &[Ty]) {
        for (index, &value) in values.iter().enumerate() {
            assert!(ty_equal(value, value), "type {index}: {value:?}");
            assert!(!ty_equal(value, values[(index + 1) % values.len()]));
        }
    }
    pub(crate) fn assert_scalar_matrix(values: &[Scalar]) {
        for (index, &value) in values.iter().enumerate() {
            assert!(ty_equal(Ty::Option(value), Ty::Option(value)));
            assert!(!ty_equal(
                Ty::Option(value),
                Ty::Option(values[(index + 1) % values.len()])
            ));
        }
    }
    struct FixtureView(Vec<hir::StructDef>);
    impl SourceShapeView for FixtureView {
        fn source_shape_node(&self, node: Node) -> Option<SourceShapeNode<'_>> {
            let Node::Struct(id) = node else { return None };
            self.0
                .get(id as usize)
                .map(|value| SourceShapeNode::Struct {
                    name: &value.name,
                    source_name: &value.source_name,
                    align: &value.align,
                    c_repr: &value.c_repr,
                    fields: &value.fields,
                })
        }
    }
    #[test]
    fn canonical_source_shape_comparator() {
        macro_rules! unequal {
            ($($left:expr => $right:expr),+ $(,)?) => {$(assert!(!ty_equal($left, $right));)+};
        }
        macro_rules! rejects {
            ($left:expr, $right:expr; $($mutation:expr),+ $(,)?) => {$(
                let mut value = twin_program();
                $mutation(&mut value);
                assert!(!equal(&value, $left, $right));
            )+};
        }
        let program = twin_program();
        for (left, right) in [
            (Node::Struct(0), Node::Struct(1)),
            (Node::Enum(0), Node::Enum(1)),
            (Node::Tuple(0), Node::Tuple(1)),
            (Node::Tagged(0), Node::Tagged(1)),
            (Node::Fn(0), Node::Fn(1)),
        ] {
            assert!(equal(&program, left, right));
        }
        unequal!(
            Ty::Int(i(8)) => Ty::Int(IntTy { bits: 8, signed: false }),
            Ty::Float(FloatTy { bits: 32 }) => Ty::Float(FloatTy { bits: 64 }),
            Ty::Param(0) => Ty::Param(1), Ty::SoaParam(0) => Ty::SoaParam(1),
            Ty::IntVar(0) => Ty::IntVar(1),
            Ty::FloatVar(0) => Ty::FloatVar(1),
            Ty::Array(Scalar::Bool, 2) => Ty::Array(Scalar::Bool, 3),
            Ty::Vec(Scalar::Int(i(8)), 2) => Ty::Vec(Scalar::Int(i(8)), 4),
            Ty::DynStructArray(0, Layout::Aos) => Ty::DynStructArray(0, Layout::Soa),
            Ty::DictEncoded(0, 1) => Ty::DictEncoded(0, 2),
            Ty::Option(Scalar::Int(i(8))) => Ty::Option(Scalar::Int(i(16))),
            Ty::Option(Scalar::Param(0)) => Ty::Option(Scalar::Param(1)),
            Ty::Option(Scalar::SoaParam(0)) => Ty::Option(Scalar::SoaParam(1)),
        );
        assert_ty_matrix(&[
            Ty::Param(0),
            Ty::SoaParam(0),
            Ty::IntVar(0),
            Ty::FloatVar(0),
            Ty::Error,
        ]);
        assert!(ty_equal(
            Ty::Option(Scalar::Param(0)),
            Ty::Option(Scalar::Param(0))
        ));
        assert!(ty_equal(
            Ty::Option(Scalar::SoaParam(0)),
            Ty::Option(Scalar::SoaParam(0))
        ));
        unequal!(
            Ty::Tagged(0) => Ty::Tagged(99), Ty::StructArray(0, 1) => Ty::StructArray(99, 1),
            Ty::DynStructArray(0, Layout::Aos) => Ty::DynStructArray(99, Layout::Aos),
            Ty::Soa(0) => Ty::Soa(99), Ty::JsonScanner(0) => Ty::JsonScanner(99),
            Ty::DictEncoded(0, 0) => Ty::DictEncoded(99, 0), Ty::Struct(0) => Ty::Struct(99),
            Ty::Tuple(0) => Ty::Tuple(99), Ty::Fn(0) => Ty::Fn(99), Ty::Enum(0) => Ty::Enum(99),
            Ty::Option(Scalar::Struct(0)) => Ty::Option(Scalar::Struct(99)),
            Ty::Option(Scalar::DynStructArray(0)) => Ty::Option(Scalar::DynStructArray(99)),
            Ty::Option(Scalar::Soa(0)) => Ty::Option(Scalar::Soa(99)),
            Ty::Option(Scalar::Enum(0)) => Ty::Option(Scalar::Enum(99)),
            Ty::Option(Scalar::Tagged(0)) => Ty::Option(Scalar::Tagged(99)),
            Ty::Option(Scalar::Fn(0)) => Ty::Option(Scalar::Fn(99)),
        );
        rejects!(Node::Struct(0), Node::Struct(1);
            |p: &mut hir::Program| p.structs[1].source_name.push('x'),
            |p: &mut hir::Program| p.structs[1].align = Some(8),
            |p: &mut hir::Program| p.structs[1].c_repr = true,
            |p: &mut hir::Program| p.structs[1].fields[0].name.push('x'),
            |p: &mut hir::Program| p.structs[1].fields[0].ty = Ty::Bool,
        );
        rejects!(Node::Enum(0), Node::Enum(1);
            |p: &mut hir::Program| p.enums[1].source_name.push('x'),
            |p: &mut hir::Program| p.enums[1].variants[0].name.push('x'),
            |p: &mut hir::Program| p.enums[1].variants[1].field_base += 1,
            |p: &mut hir::Program| p.enums[1].variants[1].payload[0] = Scalar::Bool,
        );
        rejects!(Node::Tuple(0), Node::Tuple(1); |p: &mut hir::Program| p.tuples[1].elems[0] = Scalar::Bool);
        rejects!(Node::Tagged(0), Node::Tagged(1);
            |p: &mut hir::Program| p.tagged_types[1] = hir::TaggedType::Result(Scalar::Bool, Scalar::Char),
            |p: &mut hir::Program| p.tagged_types[1] = hir::TaggedType::Option(Scalar::Bool),
        );
        rejects!(Node::Fn(0), Node::Fn(1);
            |p: &mut hir::Program| p.fn_types[1].params.push((ParamMode::ByValue, Scalar::Bool)),
            |p: &mut hir::Program| p.fn_types[1].params[0].0 = ParamMode::Out,
            |p: &mut hir::Program| p.fn_types[1].params[0].1 = Scalar::Char,
            |p: &mut hir::Program| p.fn_types[1].ret = Ty::Bool,
            |p: &mut hir::Program| p.fn_types[1].return_borrow = hir::ReturnBorrowSummary::Roots { params: vec![0], captures: vec![] },
            |p: &mut hir::Program| p.fn_types[1].return_region = hir::ReturnRegionSummary::Roots { params: vec![0], captures: vec![] },
        );
        let fixture = FixtureView(program.structs.clone());
        assert!(equal(&fixture, Node::Struct(0), Node::Struct(1)));
        let mut unequal = fixture.0;
        unequal[1].fields[0].ty = Ty::Bool;
        let unequal = FixtureView(unequal);
        assert!(!equal(&unequal, Node::Struct(0), Node::Struct(1)));
        let mut known = HashSet::new();
        assert!(source_shape_equal(
            &program,
            Node::Struct(0),
            Node::Struct(1),
            &mut known
        ));
        assert!(known.contains(&(Node::Struct(0), Node::Struct(1))));
        let mut unequal = program.clone();
        unequal.structs[1].source_name.push('x');
        known.clear();
        assert!(!source_shape_equal(
            &unequal,
            Node::Struct(0),
            Node::Struct(1),
            &mut known
        ));
        assert!(known.is_empty());
        for node in [
            Node::Struct(99),
            Node::Enum(99),
            Node::Tuple(99),
            Node::Tagged(99),
            Node::Fn(99),
        ] {
            assert!(!equal(&program, node, node));
        }
        for node in [Node::Enum(0), Node::Tuple(0), Node::Tagged(0), Node::Fn(0)] {
            assert!(!equal(&program, Node::Struct(0), node));
        }
        let production = include_str!("source_shape.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for (needle, count) in [
            ("HashSet<", 4),
            ("HashMap<", 2),
            ("VecDeque<", 1),
            ("HashSet::new", 1),
            ("HashMap::new", 2),
            ("VecDeque::from", 1),
        ] {
            assert_eq!(production.matches(needle).count(), count, "{needle}");
        }
        assert!(
            production
                .contains("source_shape_equal_observed(view, left, right, known_shapes, &mut ())")
        );
        for absent in [
            "dyn SourceShapeObserver",
            "static mut",
            "CanonicalTypeView",
            "ValidatedGraph",
            "canonical_type_bytes",
        ] {
            assert!(!production.contains(absent), "{absent}");
        }
        assert_eq!(
            include_str!("validate_hir.rs")
                .matches("source_shape_equal(")
                .count(),
            1
        );
    }

    #[test]
    fn canonical_source_shape_complexity() {
        let program = twin_program();
        let mut known = HashSet::new();
        let mut metrics = Metrics::default();
        let pairs = [
            (Node::Struct(0), Node::Struct(1)),
            (Node::Enum(0), Node::Enum(1)),
            (Node::Tuple(0), Node::Tuple(1)),
            (Node::Tagged(0), Node::Tagged(1)),
            (Node::Fn(0), Node::Fn(1)),
        ];
        for (left, right) in pairs {
            assert!(source_shape_equal_observed(
                &program,
                left,
                right,
                &mut known,
                &mut metrics,
            ));
        }
        let counts = metrics.counts();
        eprintln!("V/E/P/Q = {counts:?}");
        assert_eq!(counts, (10, 0, 5, 63));

        let before = metrics.counts();
        assert!(source_shape_equal_observed(
            &program,
            Node::Struct(0),
            Node::Struct(1),
            &mut known,
            &mut metrics,
        ));
        assert_eq!(metrics.counts(), before, "a fresh cached root is free");
    }
}
