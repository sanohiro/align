use std::collections::{HashMap, HashSet, VecDeque};

use align_ast::ParamMode;
use align_sema::{Scalar, Ty, hir};

use super::canonical_graph::Node;

pub(super) enum SourceShapeNode<'a> {
    Struct {
        source_name: &'a str,
        align: &'a Option<u32>,
        c_repr: &'a bool,
        fields: &'a [hir::FieldDef],
    },
    Enum {
        source_name: &'a str,
        variants: &'a [hir::EnumVariant],
    },
    Tuple {
        elems: &'a [Scalar],
    },
    Tagged(&'a hir::TaggedType),
    Function {
        params: &'a [(ParamMode, Scalar)],
        ret: &'a Ty,
        return_borrow: &'a hir::ReturnBorrowSummary,
        return_region: &'a hir::ReturnRegionSummary,
    },
}

pub(super) trait SourceShapeView {
    fn source_shape_node(&self, node: Node) -> Option<SourceShapeNode<'_>>;
}

impl SourceShapeView for hir::Program {
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

pub(super) fn source_shape_equal<V: SourceShapeView + ?Sized>(
    view: &V,
    left: Node,
    right: Node,
    known_shapes: &mut HashSet<(Node, Node)>,
) -> bool {
    let mut comparator = SourceShapeComparator {
        view,
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

struct SourceShapeComparator<'a, V: ?Sized> {
    view: &'a V,
    known_shapes: &'a HashSet<(Node, Node)>,
    root: (Node, Node),
    cache_enabled: bool,
    pending: VecDeque<(Node, Node)>,
    seen: HashSet<(Node, Node)>,
    left_to_right: HashMap<Node, Node>,
    right_to_left: HashMap<Node, Node>,
}

impl<V: SourceShapeView + ?Sized> SourceShapeComparator<'_, V> {
    fn run(&mut self) -> bool {
        loop {
            let mut restart = false;
            while let Some((left, right)) = self.pending.pop_front() {
                if !self.map_pair(left, right) {
                    return false;
                }
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
        match (left, right) {
            (
                SourceShapeNode::Struct {
                    source_name: left_name,
                    align: left_align,
                    c_repr: left_c_repr,
                    fields: left_fields,
                },
                SourceShapeNode::Struct {
                    source_name: right_name,
                    align: right_align,
                    c_repr: right_c_repr,
                    fields: right_fields,
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
                },
                SourceShapeNode::Function {
                    params: right_params,
                    ret: right_ret,
                    return_borrow: right_borrow,
                    return_region: right_region,
                },
            ) => {
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
            Scalar::Int(_)
            | Scalar::Float(_)
            | Scalar::DynArray(_)
            | Scalar::Slice(_)
            | Scalar::Param(_)
            | Scalar::Bool
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
            | Scalar::RunOutput => left == right,
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
            Ty::ArrayBuilder(left) => {
                same!(Ty::ArrayBuilder(right) => self.scalars_equal(left, right))
            }
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
            Ty::Int(_)
            | Ty::Float(_)
            | Ty::Param(_)
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
            | Ty::Buffer
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
            | Ty::HttpRequest
            | Ty::HttpResponse
            | Ty::HttpClient
            | Ty::HttpServer
            | Ty::HttpRequestCtx
            | Ty::ResponseBuilder
            | Ty::HttpStream
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

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::validate_hir_tests::baseline_program;
    use align_sema::{FloatTy, IntTy, Layout};
    use std::collections::HashSet;
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
            Ty::Param(0) => Ty::Param(1), Ty::IntVar(0) => Ty::IntVar(1),
            Ty::FloatVar(0) => Ty::FloatVar(1),
            Ty::Array(Scalar::Bool, 2) => Ty::Array(Scalar::Bool, 3),
            Ty::Vec(Scalar::Int(i(8)), 2) => Ty::Vec(Scalar::Int(i(8)), 4),
            Ty::DynStructArray(0, Layout::Aos) => Ty::DynStructArray(0, Layout::Soa),
            Ty::DictEncoded(0, 1) => Ty::DictEncoded(0, 2),
            Ty::Option(Scalar::Int(i(8))) => Ty::Option(Scalar::Int(i(16))),
            Ty::Option(Scalar::Param(0)) => Ty::Option(Scalar::Param(1)),
        );
        assert_ty_matrix(&[Ty::Param(0), Ty::IntVar(0), Ty::FloatVar(0), Ty::Error]);
        assert!(ty_equal(
            Ty::Option(Scalar::Param(0)),
            Ty::Option(Scalar::Param(0))
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
            ("HashSet<", 3),
            ("HashMap<", 2),
            ("VecDeque<", 1),
            ("HashSet::new", 1),
            ("HashMap::new", 2),
            ("VecDeque::from", 1),
        ] {
            assert_eq!(production.matches(needle).count(), count, "{needle}");
        }
        for absent in [
            "Observer",
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
}
