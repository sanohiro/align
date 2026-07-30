use std::collections::{HashMap, HashSet, VecDeque};

use align_sema::{PrimScalar, Scalar, Ty, hir};

/// Validate the program-global HIR type domain before MIR construction.
pub(crate) fn global_type_metadata_is_valid(program: &hir::Program) -> bool {
    Validator::new(program).validate()
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
