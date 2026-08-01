use std::collections::{HashMap, HashSet, VecDeque};

use align_sema::{Layout, PrimScalar, Scalar, Ty, hir};

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
                    if !valid {
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
        }
        let mut work = vec![Work::Enter(ty)];
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
