//! Backend-independent validation of MIR producer contracts.
//!
//! Publication and native emission share the same typed producer and founded
//! initialization proof. Native lowering retains its separate target/ABI checks.

use align_ast::{BinOp, UnOp};
use std::collections::{HashMap, HashSet, VecDeque};
use crate::{Block, QueryMetaTypes, CanonicalTy, Const, ConstElem, DirectCall, Function,
    Operand, Program, ProgramCall, RuntimeKey, Rvalue, Slot, Stmt, Term, ValueId};
use align_sema::{ArrayBuilderElem, ERROR_VARIANT_CODE, FloatTy,
    IntTy, Layout, Scalar, StructDef, Ty, hir, scalar_to_ty};

/// A malformed MIR producer, reported before publication or native lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProducerError {
    Lowering(String),
}

impl std::fmt::Display for ProducerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lowering(message) => write!(formatter, "lowering failed: {message}"),
        }
    }
}
impl std::error::Error for ProducerError {}

pub fn lowercase_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}

pub fn builtin_error_enum_is_exact(program: &Program, id: u32) -> bool {
    let Some(definition) = program.enums.get(id as usize) else {
        return false;
    };
    if definition.name != "Error"
        || definition.source_name != "Error"
        || definition.variants.len() != 5
    {
        return false;
    }
    let tag_only = |index: usize, name: &str| {
        let variant = &definition.variants[index];
        variant.name == name && variant.payload.is_empty() && variant.field_base == 1
    };
    let code = &definition.variants[ERROR_VARIANT_CODE as usize];
    tag_only(0, "NotFound")
        && tag_only(1, "Invalid")
        && tag_only(2, "Denied")
        && tag_only(3, "Timeout")
        && code.name == "Code"
        && code.payload.as_slice()
            == [Scalar::Int(IntTy {
                bits: 32,
                signed: true,
            })]
        && code.field_base == 1
}

/// Recheck package-resource operations at the cached/hand-built MIR boundary. HIR validation
/// proves these facts for normal lowering, but resource ids and safety discriminators live directly
/// on MIR nodes and must fail closed before LLVM construction if an external producer corrupts them.
pub fn db_resource_matches_row(
    program: &Program,
    resource: u32,
    struct_id: u32,
    kind: &str,
) -> bool {
    let Some(row) = program.structs.get(struct_id as usize) else {
        return false;
    };
    let direct = format!("pkg.db${kind}$S{}_{}", row.name.len(), row.name);
    let reconstructed_row = row
        .name
        .chars()
        .map(|character| match character {
            '.' | '$' => '_',
            other => other,
        })
        .collect::<String>();
    let reconstructed = format!(
        "pkg.db${kind}$S{}_{}",
        reconstructed_row.len(),
        reconstructed_row
    );
    program.resources.get(resource as usize).is_some_and(|definition| {
        definition.declaring_module == "pkg.db"
            && definition.generic_arity == 1
            && matches!(
                definition.name.as_str(),
                name if name == direct || name == reconstructed
            )
            && definition.source_name == definition.name
    })
}

fn template_html_resource_matches(program: &Program, resource: u32) -> bool {
    program
        .resources
        .get(resource as usize)
        .is_some_and(|definition| {
            definition.source_name == "pkg.template$html_builder"
                && definition.name == "pkg.template$html_builder"
                && definition.declaring_module == "pkg.template"
                && definition.generic_arity == 0
                && definition.drop_hook == "pkg.template.internal.resource$drop_html_builder"
                && definition.drop_thunk == "__align_resource_drop$pkg.template$html_builder"
                && definition.representation_version == 1
                && definition.drop_abi_fingerprint == *b"align-res-drop-1"
        })
}

fn template_html_callable_resource_matches(program: &Program, resource: u32) -> bool {
    program
        .resources
        .get(resource as usize)
        .is_some_and(|definition| {
            definition.source_name == "pkg.template$html_builder"
                && definition.name == "pkg.template$html_builder"
                && definition.declaring_module == "pkg.template"
                && definition.generic_arity == 0
                && matches!(
                    definition.drop_hook.as_str(),
                    "pkg.template.internal.resource$drop_html_builder"
                        | "pkg.template.internal.resource.__align_interface_drop_html_builder"
                )
                && definition.drop_thunk == "__align_resource_drop$pkg.template$html_builder"
                && definition.representation_version == 1
                && definition.drop_abi_fingerprint == *b"align-res-drop-1"
        })
}

fn template_html_mir_signature(
    name: &str,
    resource: u32,
) -> Option<(Vec<Ty>, Vec<align_ast::ParamMode>, Ty, hir::ReturnCleanupAbi)> {
    use align_ast::ParamMode::{BorrowMut, ByValue};
    Some(match name {
        "pkg.template$html" => (
            Vec::new(),
            Vec::new(),
            Ty::Resource(resource),
            hir::ReturnCleanupAbi::DynamicBit,
        ),
        "pkg.template$write" | "pkg.template$raw" => (
            vec![Ty::Resource(resource), Ty::Str],
            vec![BorrowMut, ByValue],
            Ty::Unit,
            hir::ReturnCleanupAbi::None,
        ),
        "pkg.template$to_string" => (
            vec![Ty::Resource(resource)],
            vec![ByValue],
            Ty::String,
            hir::ReturnCleanupAbi::DynamicBit,
        ),
        _ => return None,
    })
}

fn template_html_mir_operation_name(rvalue: &Rvalue) -> Option<&'static str> {
    match rvalue {
        Rvalue::TemplateHtmlNew { .. } => Some("pkg.template$html"),
        Rvalue::TemplateHtmlWrite { .. } => Some("pkg.template$write"),
        Rvalue::TemplateHtmlRaw { .. } => Some("pkg.template$raw"),
        Rvalue::TemplateHtmlToString { .. } => Some("pkg.template$to_string"),
        _ => None,
    }
}

fn validate_template_html_mir_signatures(program: &Program) -> Result<(), ProducerError> {
    let has_surface = program
        .fns
        .iter()
        .any(|function| template_html_mir_signature(function.name.as_str(), 0).is_some())
        || program
            .imported_fns
            .iter()
            .any(|function| template_html_mir_signature(function.name.as_str(), 0).is_some());
    if !has_surface {
        return Ok(());
    }
    let Some(resource) = program
        .resources
        .iter()
        .enumerate()
        .find_map(|(resource, _)| {
            let resource = u32::try_from(resource).ok()?;
            template_html_callable_resource_matches(program, resource).then_some(resource)
        })
    else {
        return Err(ProducerError::Lowering(
            "pkg.template callable surface has no canonical resource".into(),
        ));
    };
    for function in &program.fns {
        let Some((params, modes, ret, cleanup)) =
            template_html_mir_signature(function.name.as_str(), resource)
        else {
            continue;
        };
        let actual = function
            .params
            .iter()
            .map(|slot| function.slots.get(*slot as usize).copied())
            .collect::<Option<Vec<_>>>();
        if actual.as_deref() != Some(params.as_slice())
            || function.param_modes != modes
            || function.ret != ret
            || function.return_borrow != hir::ReturnBorrowSummary::None
            || function.return_region != hir::ReturnRegionSummary::None
            || function.return_cleanup != cleanup
        {
            return Err(ProducerError::Lowering(format!(
                "pkg.template function '{}' has a malformed canonical signature",
                function.name
            )));
        }
        let operations = function
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .filter_map(|statement| match statement {
                Stmt::Let(_, rvalue) => template_html_mir_operation_name(rvalue),
                _ => None,
            })
            .collect::<Vec<_>>();
        if operations.as_slice() != [function.name.as_str()] {
            return Err(ProducerError::Lowering(format!(
                "pkg.template function '{}' does not contain exactly one matching operation",
                function.name
            )));
        }
    }
    for function in &program.imported_fns {
        let Some((params, modes, ret, cleanup)) =
            template_html_mir_signature(function.name.as_str(), resource)
        else {
            continue;
        };
        if function.params != params
            || function.param_modes != modes
            || function.ret != ret
            || function.return_borrow != hir::ReturnBorrowSummary::None
            || function.return_region != hir::ReturnRegionSummary::None
            || function.return_cleanup != cleanup
        {
            return Err(ProducerError::Lowering(format!(
                "imported pkg.template function '{}' has a malformed canonical signature",
                function.name
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XmlAccessProvenance {
    Owned,
    Shared,
    Exclusive,
    Unreadable,
    Mixed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XmlProducerState {
    Present(XmlAccessProvenance),
    MaybeAbsent(XmlAccessProvenance),
    Absent,
    Invalid,
}

impl XmlProducerState {
    fn owned_if_present(self) -> bool {
        matches!(
            self,
            Self::Present(XmlAccessProvenance::Owned)
                | Self::MaybeAbsent(XmlAccessProvenance::Owned)
                | Self::Absent
        )
    }
}

struct XmlSlotStores<'a> {
    roots: Vec<Vec<&'a Operand>>,
    fields: Vec<Vec<(&'a [u32], &'a Operand)>>,
    elements: Vec<Vec<(&'a Operand, &'a Operand)>>,
    element_fields: Vec<Vec<(&'a Operand, &'a [u32], &'a Operand)>>,
    constant_elements: Vec<Vec<(&'a [ConstElem], Ty)>>,
    producers: Vec<Vec<(ValueId, &'a Rvalue)>>,
}

struct ValidatedProducerGraph<'a> {
    function: &'a crate::Function,
    program: &'a Program,
    local_contracts: &'a HashSet<ProgramCall>,
    value_definitions: &'a [Option<&'a Rvalue>],
    auxiliary_value_definitions: &'a [Option<&'a Rvalue>],
    duplicate_values: &'a [bool],
    slot_stores: &'a XmlSlotStores<'a>,
    primary_definitions: &'a [u32],
    auxiliary_definitions: &'a [u32],
}

fn merge_xml_access(
    current: Option<XmlAccessProvenance>,
    next: XmlAccessProvenance,
) -> Option<XmlAccessProvenance> {
    Some(match (current, next) {
        (None, next) => next,
        (Some(XmlAccessProvenance::Owned), XmlAccessProvenance::Owned) => {
            XmlAccessProvenance::Owned
        }
        (Some(XmlAccessProvenance::Shared), XmlAccessProvenance::Shared) => {
            XmlAccessProvenance::Shared
        }
        (Some(XmlAccessProvenance::Exclusive), XmlAccessProvenance::Exclusive) => {
            XmlAccessProvenance::Exclusive
        }
        (Some(XmlAccessProvenance::Unreadable), XmlAccessProvenance::Unreadable) => {
            XmlAccessProvenance::Unreadable
        }
        (Some(XmlAccessProvenance::Mixed), XmlAccessProvenance::Mixed) => {
            XmlAccessProvenance::Mixed
        }
        (Some(XmlAccessProvenance::Mixed), _) | (_, XmlAccessProvenance::Mixed) => {
            XmlAccessProvenance::Mixed
        }
        (Some(XmlAccessProvenance::Owned), next)
        | (Some(next), XmlAccessProvenance::Owned) => next,
        (Some(XmlAccessProvenance::Shared), XmlAccessProvenance::Exclusive)
        | (Some(XmlAccessProvenance::Exclusive), XmlAccessProvenance::Shared) => {
            XmlAccessProvenance::Shared
        }
        (Some(XmlAccessProvenance::Shared), XmlAccessProvenance::Unreadable)
        | (Some(XmlAccessProvenance::Unreadable), XmlAccessProvenance::Shared) => {
            XmlAccessProvenance::Mixed
        }
        (Some(XmlAccessProvenance::Exclusive), XmlAccessProvenance::Unreadable)
        | (Some(XmlAccessProvenance::Unreadable), XmlAccessProvenance::Exclusive) => {
            XmlAccessProvenance::Unreadable
        }
    })
}

fn xml_event_definition_valid(program: &Program, event_enum: u32) -> bool {
    let Some(definition) = program.enums.get(event_enum as usize) else {
        return false;
    };
    const NAMES: [&str; 3] = ["Start", "End", "Text"];
    program
        .enums
        .iter()
        .filter(|candidate| {
            candidate.name == "xml.event" || candidate.source_name == "xml.event"
        })
        .count()
        == 1
        && definition.name == "xml.event"
        && definition.source_name == "xml.event"
        && definition.variants.len() == NAMES.len()
        && definition
            .variants
            .iter()
            .zip(NAMES)
            .all(|(variant, expected)| {
                variant.name == expected
                    && variant.payload.is_empty()
                    && variant.field_base == 1
            })
}

fn xml_argument_access(function: &crate::Function, index: u32) -> XmlAccessProvenance {
    match function.param_modes.get(index as usize) {
        Some(align_ast::ParamMode::ByValue) => XmlAccessProvenance::Owned,
        Some(align_ast::ParamMode::Borrow) => XmlAccessProvenance::Shared,
        Some(align_ast::ParamMode::BorrowMut) => XmlAccessProvenance::Exclusive,
        Some(align_ast::ParamMode::Out) => XmlAccessProvenance::Unreadable,
        None => XmlAccessProvenance::Mixed,
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum XmlAccessPathSegment {
    StructField(u32),
    TupleElement(u32),
    Element,
    EnumPayload { enum_id: u32, variant: u32, slot: u32 },
    OptionSome,
    ResultOk,
    ResultErr,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum XmlAccessNode {
    Value(ValueId, Vec<XmlAccessPathSegment>),
    Slot(Slot, Vec<XmlAccessPathSegment>),
    CaptureValue(ValueId, Vec<XmlAccessPathSegment>),
    CaptureSlot(Slot, Vec<XmlAccessPathSegment>),
    BufferValue(ValueId, Vec<XmlAccessPathSegment>),
    BufferSlot(Slot, Vec<XmlAccessPathSegment>),
}

#[derive(Clone, Debug)]
enum XmlAccessSource {
    Seed(XmlAccessProvenance),
    Node(XmlAccessNode),
    /// A borrowed place may be materialized only with shared authority.  Keep this distinct
    /// from an ordinary dependency so a later Store/Load cannot regain the owner's Move proof.
    ReadNode(XmlAccessNode),
    Invalid,
}

#[derive(Debug, Default)]
struct XmlAccessEquation {
    // Operation inputs must be initialized before the operation can found a result.
    requires_inputs: bool,
    // Storage joins can have several action-derived seeds. None also permits an
    // independent entry/ordinary-store seed; Some retains each action's own inputs.
    seed_inputs: Option<Vec<Vec<XmlAccessNode>>>,
    copied_scalar: bool,
    seed: Option<XmlAccessProvenance>,
    dependencies: Vec<XmlAccessNode>,
    read_dependencies: Vec<XmlAccessNode>,
    checks: Vec<(XmlAccessNode, OperandRequirement)>,
    invalid: bool,
    absent: bool,
    require_present: bool,
    guarded_absence: bool,
}

impl XmlAccessEquation {
    fn produced_access(&self, access: XmlAccessProvenance) -> XmlAccessProvenance {
        if self.copied_scalar && matches!(access,
            XmlAccessProvenance::Owned | XmlAccessProvenance::Shared | XmlAccessProvenance::Exclusive)
        {
            XmlAccessProvenance::Owned
        } else {
            access
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct OperandRequirement {
    read: bool,
    write: bool,
    exclusive: bool,
    move_value: bool,
    callable: bool,
}

impl OperandRequirement {
    const READ: Self = Self {
        read: true,
        write: false,
        exclusive: false,
        move_value: false,
        callable: false,
    };

    fn is_satisfied_by(self, state: XmlProducerState) -> bool {
        let access = match state {
            XmlProducerState::Present(access) | XmlProducerState::MaybeAbsent(access) => access,
            // A statically inactive option/result/enum leaf has no value on which any capability
            // could be exercised. Its requirement is therefore vacuous; active alternatives still
            // carry and must satisfy the full product.
            XmlProducerState::Absent => return true,
            XmlProducerState::Invalid => return false,
        };
        let (read, write, exclusive, move_value) = match access {
            XmlAccessProvenance::Owned => (true, true, true, true),
            XmlAccessProvenance::Shared => (true, false, false, false),
            XmlAccessProvenance::Exclusive => (true, true, true, false),
            XmlAccessProvenance::Unreadable => (false, true, true, false),
            XmlAccessProvenance::Mixed => {
                (false, false, false, false)
            }
        };
        (!self.read || read)
            && (!self.write || write)
            && (!self.exclusive || exclusive)
            && (!self.move_value || move_value)
            // Callable identity is certified by the node's semantic equation. It remains an
            // orthogonal requirement here so mode capabilities cannot replace that proof.
            && (!self.callable
                || !matches!(
                    access,
                    XmlAccessProvenance::Mixed
                ))
    }
}

fn solve_xml_access_equations(
    equations: &HashMap<XmlAccessNode, XmlAccessEquation>,
) -> (
    HashMap<XmlAccessNode, XmlProducerState>,
    HashSet<XmlAccessNode>,
) {
    fn read_only_access(access: XmlAccessProvenance) -> XmlAccessProvenance {
        match access {
            XmlAccessProvenance::Owned
            | XmlAccessProvenance::Shared
            | XmlAccessProvenance::Exclusive => XmlAccessProvenance::Shared,
            XmlAccessProvenance::Unreadable => XmlAccessProvenance::Unreadable,
            XmlAccessProvenance::Mixed => XmlAccessProvenance::Mixed,
        }
    }

    let mut validation_reverse = HashMap::<XmlAccessNode, Vec<XmlAccessNode>>::new();
    let mut reverse = HashMap::<XmlAccessNode, Vec<XmlAccessNode>>::new();
    for (node, equation) in equations {
        for dependency in equation
            .dependencies
            .iter()
            .chain(&equation.read_dependencies)
        {
            reverse
                .entry(dependency.clone())
                .or_default()
                .push(node.clone());
            validation_reverse
                .entry(dependency.clone())
                .or_default()
                .push(node.clone());
        }
        for (dependency, _) in &equation.checks {
            validation_reverse
                .entry(dependency.clone())
                .or_default()
                .push(node.clone());
        }
    }

    // Initialization is independent of access. In particular, a Shared call result
    // cannot provide the evidence for its own input or a closure's captured input.
    // Each node becomes ready once: operations require all checked inputs, while
    // storage joins keep independently founded entry/store alternatives available.
    let can_initialize = |equation: &XmlAccessEquation, ready: &HashSet<XmlAccessNode>| {
        !equation.invalid
            && (!equation.requires_inputs
                || equation
                    .checks
                    .iter()
                    .all(|(input, _)| ready.contains(input)))
            && (equation.absent
                || (equation.seed.is_some()
                    && equation.seed_inputs.as_ref().is_none_or(|alternatives| {
                        alternatives
                            .iter()
                            .any(|inputs| inputs.iter().all(|input| ready.contains(input)))
                    }))
                || equation
                    .dependencies
                    .iter()
                    .chain(&equation.read_dependencies)
                    .any(|input| ready.contains(input)))
    };
    let mut initialized = HashSet::new();
    let mut initialization_ready = VecDeque::new();
    for (node, equation) in equations {
        if can_initialize(equation, &initialized) {
            initialized.insert(node.clone());
            initialization_ready.push_back(node.clone());
        }
    }
    while let Some(changed) = initialization_ready.pop_front() {
        if let Some(parents) = validation_reverse.get(&changed) {
            for parent in parents {
                if !initialized.contains(parent)
                    && equations
                        .get(parent)
                        .is_some_and(|equation| can_initialize(equation, &initialized))
                {
                    initialized.insert(parent.clone());
                    initialization_ready.push_back(parent.clone());
                }
            }
        }
    }
    // Presence and access form one monotone finite-height lattice. Do not mix absence into this
    // worklist: an authenticated discriminator projects MaybeAbsent to Present, so feeding a
    // provisional absence through that projection made the former single-state solver oscillate.
    let mut present = HashMap::<XmlAccessNode, XmlAccessProvenance>::new();
    let mut present_ready = VecDeque::new();
    for (node, equation) in equations {
        if !equation.invalid && initialized.contains(node)
            && let Some(seed) = equation.seed
        {
            present.insert(node.clone(), equation.produced_access(seed));
            present_ready.push_back(node.clone());
        }
    }
    while let Some(changed) = present_ready.pop_front() {
        let Some(parents) = reverse.get(&changed) else {
            continue;
        };
        for parent in parents {
            let Some(equation) = equations.get(parent) else {
                continue;
            };
            if equation.invalid {
                continue;
            }
            let next = equation.dependencies.iter().fold(equation.seed, |current, dependency| {
                present
                    .get(dependency)
                    .copied()
                    .map_or(current, |access| merge_xml_access(current, access))
            });
            let next = equation
                .read_dependencies
                .iter()
                .fold(next, |current, dependency| {
                    present.get(dependency).copied().map_or(current, |access| {
                        merge_xml_access(current, read_only_access(access))
                    })
                });
            if let Some(next) = next.map(|access| equation.produced_access(access))
                && present.get(parent) != Some(&next)
            {
                present.insert(parent.clone(), next);
                present_ready.push_back(parent.clone());
            }
        }
    }

    // With present reachability fixed, absence is a separate monotone boolean fact. An exact
    // guard suppresses absence only when the selected present alternative is known to exist.
    let guarded_present = |node: &XmlAccessNode, equation: &XmlAccessEquation| {
        equation.require_present && equation.guarded_absence && present.contains_key(node)
    };
    let mut absent = HashSet::<XmlAccessNode>::new();
    let mut absent_ready = VecDeque::new();
    for (node, equation) in equations {
        if !equation.invalid && equation.absent && !guarded_present(node, equation) {
            absent.insert(node.clone());
            absent_ready.push_back(node.clone());
        }
    }
    while let Some(changed) = absent_ready.pop_front() {
        let Some(parents) = reverse.get(&changed) else {
            continue;
        };
        for parent in parents {
            let Some(equation) = equations.get(parent) else {
                continue;
            };
            if equation.invalid
                || guarded_present(parent, equation)
                || !absent.insert(parent.clone())
            {
                continue;
            }
            absent_ready.push_back(parent.clone());
        }
    }

    let mut values = HashMap::<XmlAccessNode, XmlProducerState>::new();
    for (node, equation) in equations {
        let state = if equation.invalid
            || (equation.require_present && !equation.guarded_absence && absent.contains(node))
        {
            Some(XmlProducerState::Invalid)
        } else {
            match (present.get(node).copied(), absent.contains(node)) {
                (Some(access), true) => Some(XmlProducerState::MaybeAbsent(access)),
                (Some(access), false) => Some(XmlProducerState::Present(access)),
                (None, true) => Some(XmlProducerState::Absent),
                (None, false) => None,
            }
        };
        if let Some(state) = state {
            values.insert(node.clone(), state);
        }
    }

    // Access propagation intentionally excludes validation-only dependencies. Once both raw
    // reachability phases converge, every such dependency must have produced a total typed state.
    // An unresolved check-only cycle is invalid, and invalidity flows through both edge classes.
    let mut invalid = equations
        .iter()
        .filter(|(node, equation)| {
            equation.invalid
                || !initialized.contains(*node)
                || !values.contains_key(*node)
                || values.get(*node) == Some(&XmlProducerState::Invalid)
                || (equation.copied_scalar && values.get(*node).copied()
                    .is_none_or(|state| !OperandRequirement::READ.is_satisfied_by(state)))
                || equation.checks.iter().any(|(dependency, requirement)| {
                    values
                        .get(dependency)
                        .copied()
                        .is_none_or(|state| !requirement.is_satisfied_by(state))
                })
        })
        .map(|(node, _)| node)
        .cloned()
        .collect::<HashSet<_>>();
    let mut invalid_ready = invalid.iter().cloned().collect::<VecDeque<_>>();
    while let Some(changed) = invalid_ready.pop_front() {
        let Some(parents) = validation_reverse.get(&changed) else {
            continue;
        };
        for parent in parents {
            if invalid.insert(parent.clone()) {
                invalid_ready.push_back(parent.clone());
            }
        }
    }
    (values, invalid)
}

fn xml_mode_requirement(
    program: &Program,
    selected: Ty,
    mode: align_ast::ParamMode,
) -> OperandRequirement {
    let move_value = align_sema::ty_is_move(
        selected,
        &program.structs,
        &program.tuples,
        &program.enums,
        &program.tagged_types,
    );
    let mut requirement = match mode {
        align_ast::ParamMode::ByValue => OperandRequirement {
            read: true,
            move_value,
            ..OperandRequirement::default()
        },
        align_ast::ParamMode::Borrow => OperandRequirement::READ,
        align_ast::ParamMode::BorrowMut => OperandRequirement {
            read: true,
            write: true,
            exclusive: true,
            ..OperandRequirement::default()
        },
        align_ast::ParamMode::Out => OperandRequirement {
            write: true,
            exclusive: true,
            ..OperandRequirement::default()
        },
    };
    requirement.callable = matches!(selected, Ty::Fn(_));
    requirement
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XmlCallFacts {
    pub params: Vec<Ty>,
    pub modes: Vec<align_ast::ParamMode>,
    pub ret: Ty,
    pub borrow: hir::ReturnBorrowSummary,
    pub region: hir::ReturnRegionSummary,
    pub cleanup: hir::ReturnCleanupAbi,
}

pub fn xml_fn_type_facts(program: &Program, id: u32) -> Option<XmlCallFacts> {
    let definition = program.fn_types.get(id as usize)?;
    Some(XmlCallFacts {
        params: definition
            .params
            .iter()
            .map(|(_, scalar)| scalar_to_ty(*scalar))
            .collect(),
        modes: definition.params.iter().map(|(mode, _)| *mode).collect(),
        ret: definition.ret,
        borrow: definition.return_borrow.clone(),
        region: definition.return_region.clone(),
        cleanup: definition.return_cleanup,
    })
}

fn xml_signature_matches_facts(
    signature: &crate::FnSignatureFacts,
    facts: &XmlCallFacts,
) -> bool {
    signature.param_modes == facts.modes
        && signature.return_borrow == facts.borrow
        && signature.return_region == facts.region
        && signature.return_cleanup == facts.cleanup
}

/// A closed by-value result domain whose protected leaves need only shared reads.
/// Do not use an empty ownership-leaf inventory as a proof: it also omits raw pointers
/// and mutable numeric slices. Nominal identity and nested callable ABI stay exact.
pub fn xml_shared_string_result(program: &Program, root: Ty) -> bool {
    let mut pending = vec![(root, false)];
    let mut active = HashSet::new();
    let mut complete = HashSet::new();
    let mut has_string = false;
    while let Some((ty, exit)) = pending.pop() {
        if exit {
            active.remove(&ty);
            complete.insert(ty);
            continue;
        }
        if complete.contains(&ty) {
            continue;
        }
        if !active.insert(ty) {
            return false;
        }
        pending.push((ty, true));
        match ty {
            Ty::Str => has_string = true,
            Ty::Unit | Ty::Bool | Ty::Char => {}
            Ty::Int(IntTy {
                bits: 8 | 16 | 32 | 64,
                ..
            })
            | Ty::Float(FloatTy { bits: 32 | 64 }) => {}
            Ty::Struct(id) => {
                let Some(definition) = program.structs.get(id as usize) else {
                    return false;
                };
                pending.extend(definition.fields.iter().map(|field| (field.ty, false)));
            }
            Ty::Tuple(id) => {
                let Some(definition) = program.tuples.get(id as usize) else {
                    return false;
                };
                pending.extend(definition.elems.iter().map(|ty| (scalar_to_ty(*ty), false)));
            }
            Ty::Enum(id) => {
                let Some(definition) = program.enums.get(id as usize) else {
                    return false;
                };
                pending.extend(definition.variants.iter().flat_map(|variant| {
                    variant.payload.iter().map(|ty| (scalar_to_ty(*ty), false))
                }));
            }
            Ty::Option(payload) => pending.push((scalar_to_ty(payload), false)),
            Ty::Result(ok, error) => {
                pending.push((scalar_to_ty(ok), false));
                pending.push((scalar_to_ty(error), false));
            }
            Ty::Tagged(id) => {
                let Some(definition) = program.tagged_types.get(id as usize) else {
                    return false;
                };
                match definition {
                    hir::TaggedType::Option(payload) => {
                        pending.push((scalar_to_ty(*payload), false))
                    }
                    hir::TaggedType::Result(ok, error) => {
                        pending.push((scalar_to_ty(*ok), false));
                        pending.push((scalar_to_ty(*error), false));
                    }
                }
            }
            _ => return false,
        }
    }
    has_string
}

/// Callable storage may join origins. Only shared string results may forget a root.
/// Concrete callable construction and canonical ABI identity remain exact.
pub fn xml_callable_flow_matches(program: &Program, actual: Ty, expected: Ty) -> bool {
    if actual == expected {
        return true;
    }
    let (Ty::Fn(actual), Ty::Fn(expected)) = (actual, expected) else {
        return false;
    };
    if canonical_ty(Ty::Fn(actual), program).is_err()
        || canonical_ty(Ty::Fn(expected), program).is_err()
    {
        return false;
    }
    let (Some(actual), Some(expected)) = (
        xml_fn_type_facts(program, actual),
        xml_fn_type_facts(program, expected),
    ) else {
        return false;
    };
    let roots_fit = |ap: &[u32], ac: &[u32], ep: &[u32], ec: &[u32]| {
        ap.iter().all(|root| ep.binary_search(root).is_ok())
            && ac.iter().all(|root| ec.binary_search(root).is_ok())
    };
    let shared_result = xml_shared_string_result(program, expected.ret);
    let borrow_fits = (shared_result && expected.borrow == hir::ReturnBorrowSummary::None)
        || match (&actual.borrow, &expected.borrow) {
            (hir::ReturnBorrowSummary::None, _) => true,
            (
                hir::ReturnBorrowSummary::Roots {
                    params: ap,
                    captures: ac,
                },
                hir::ReturnBorrowSummary::Roots {
                    params: ep,
                    captures: ec,
                },
            ) => roots_fit(ap, ac, ep, ec),
            _ => false,
        };
    let region_fits = (shared_result && expected.region == hir::ReturnRegionSummary::None)
        || match (&actual.region, &expected.region) {
            (hir::ReturnRegionSummary::None, _) => true,
            (
                hir::ReturnRegionSummary::Roots {
                    params: ap,
                    captures: ac,
                },
                hir::ReturnRegionSummary::Roots {
                    params: ep,
                    captures: ec,
                },
            ) => roots_fit(ap, ac, ep, ec),
            _ => false,
        };
    actual.modes == expected.modes
        && actual.cleanup == expected.cleanup
        && source_tys_match(&actual.params, &expected.params, program).unwrap_or(false)
        && source_ty_matches(actual.ret, expected.ret, program).unwrap_or(false)
        && borrow_fits
        && region_fits
}

pub fn xml_closure_borrow_summary(
    summary: &hir::ReturnBorrowSummary,
    explicit: u32,
    capture_count: u32,
) -> Option<hir::ReturnBorrowSummary> {
    match summary {
        hir::ReturnBorrowSummary::None => Some(hir::ReturnBorrowSummary::None),
        hir::ReturnBorrowSummary::Roots { params, captures } if captures.is_empty() => {
            let mut direct = Vec::new();
            let mut captured = Vec::new();
            for root in params {
                if *root < explicit {
                    direct.push(*root);
                } else {
                    let capture = root.checked_sub(explicit)?;
                    if capture >= capture_count {
                        return None;
                    }
                    captured.push(capture);
                }
            }
            Some(hir::ReturnBorrowSummary::Roots {
                params: direct,
                captures: captured,
            })
        }
        hir::ReturnBorrowSummary::Roots { .. } => None,
    }
}

pub fn xml_closure_region_summary(
    summary: &hir::ReturnRegionSummary,
    explicit: u32,
    capture_count: u32,
) -> Option<hir::ReturnRegionSummary> {
    match summary {
        hir::ReturnRegionSummary::None => Some(hir::ReturnRegionSummary::None),
        hir::ReturnRegionSummary::Roots { params, captures } if captures.is_empty() => {
            let mut direct = Vec::new();
            let mut captured = Vec::new();
            for root in params {
                if *root < explicit {
                    direct.push(*root);
                } else {
                    let capture = root.checked_sub(explicit)?;
                    if capture >= capture_count {
                        return None;
                    }
                    captured.push(capture);
                }
            }
            Some(hir::ReturnRegionSummary::Roots {
                params: direct,
                captures: captured,
            })
        }
        hir::ReturnRegionSummary::Roots { .. } => None,
    }
}

struct XmlCallResult<'a> {
    result: ValueId,
    result_ty: Ty,
    selected_ty: Ty,
    args: &'a [Operand],
    callee: Option<&'a Operand>,
    facts: &'a XmlCallFacts,
    cleanup: Option<ValueId>,
    modes_match: bool,
}

pub fn xml_operand_base_ty(function: &crate::Function, operand: &Operand) -> Option<Ty> {
    Some(match operand {
        Operand::Const(Const::Int(_, ty)) | Operand::Const(Const::Float(_, ty)) => *ty,
        Operand::Const(Const::Char(_)) => Ty::Char,
        Operand::Const(Const::Bool(_)) => Ty::Bool,
        Operand::Const(Const::Unit) => Ty::Unit,
        Operand::Value(value) => *function.value_tys.get(*value as usize)?,
        Operand::Arg(index) => {
            let slot = *function.params.get(*index as usize)?;
            *function.slots.get(slot as usize)?
        }
        Operand::BorrowedPlace(place) => place.ty,
        Operand::BorrowedElementPlace(place) => place.element_ty,
        Operand::BorrowedFixedElementPlace(place) => place.ty,
        Operand::BorrowedCleanupArg(_) => Ty::Bool,
    })
}

fn xml_numeric_scalar_ty(ty: Ty) -> bool {
    matches!(
        ty,
        Ty::Int(IntTy { bits: 8 | 16 | 32 | 64, .. })
            | Ty::Float(FloatTy { bits: 32 | 64 })
    )
}

fn xml_numeric_vector_shape(ty: Ty) -> Option<(Scalar, u32)> {
    let Ty::Vec(element, lanes @ (2 | 4 | 8 | 16)) = ty else {
        return None;
    };
    xml_numeric_scalar_ty(scalar_to_ty(element)).then_some((element, lanes))
}

fn xml_option_payload(program: &Program, ty: Ty) -> Option<Ty> {
    match ty {
        Ty::Option(payload) => Some(scalar_to_ty(payload)),
        Ty::Tagged(id) => match program.tagged_types.get(id as usize)? {
            hir::TaggedType::Option(payload) => Some(scalar_to_ty(*payload)),
            hir::TaggedType::Result(..) => None,
        },
        _ => None,
    }
}

fn xml_result_payload(program: &Program, ty: Ty, ok: bool) -> Option<Ty> {
    let (success, failure) = match ty {
        Ty::Result(success, failure) => (success, failure),
        Ty::Tagged(id) => match program.tagged_types.get(id as usize)? {
            hir::TaggedType::Result(success, failure) => (*success, *failure),
            hir::TaggedType::Option(..) => return None,
        },
        _ => return None,
    };
    Some(scalar_to_ty(if ok { success } else { failure }))
}

fn xml_ty_matches_tagged_body(program: &Program, actual: Ty, expected: Ty) -> bool {
    fn tagged_matches(program: &Program, id: u32, body: Ty) -> bool {
        match (program.tagged_types.get(id as usize), body) {
            (Some(hir::TaggedType::Option(payload)), Ty::Option(actual)) => *payload == actual,
            (Some(hir::TaggedType::Result(ok, error)), Ty::Result(actual_ok, actual_error)) => {
                *ok == actual_ok && *error == actual_error
            }
            _ => false,
        }
    }

    actual == expected
        || matches!(expected, Ty::Tagged(id) if tagged_matches(program, id, actual))
        || matches!(actual, Ty::Tagged(id) if tagged_matches(program, id, expected))
}

fn xml_value_flow_matches(program: &Program, actual: Ty, expected: Ty) -> bool {
    xml_ty_matches_tagged_body(program, actual, expected)
        || (matches!((actual, expected), (Ty::Fn(_), Ty::Fn(_)))
            && xml_callable_flow_matches(program, actual, expected))
}

pub fn xml_ty_is_view_retype(actual: Ty, expected: Ty) -> bool {
    let bytes = Scalar::Int(IntTy {
        bits: 8,
        signed: false,
    });
    match (actual, expected) {
        (Ty::Option(actual), Ty::Option(expected)) => {
            xml_ty_is_view_retype(scalar_to_ty(actual), scalar_to_ty(expected))
        }
        (Ty::String, Ty::Str) => true,
        (Ty::String | Ty::Str, Ty::Slice(element)) => element == bytes,
        (Ty::DynArray(actual), Ty::Slice(expected)) => actual == expected,
        // `ctx.headers()` is the request-context pointer retyped as a detached,
        // non-owning header-table view. MIR deliberately represents that zero-cost
        // conversion as `Use`, just like the owned collection-to-view cases above.
        (Ty::HttpRequestCtx, Ty::HttpHeaders) => true,
        (
            Ty::DynStructArray(actual, Layout::Aos),
            Ty::Slice(Scalar::Struct(expected)),
        ) => actual == expected,
        _ => false,
    }
}

pub fn xml_borrowed_place_ty_is_view_retype(actual: Ty, expected: Ty) -> bool {
    xml_ty_is_view_retype(actual, expected)
        || matches!(
            (actual, expected),
            (Ty::Array(element, _), Ty::Slice(view)) if element == view
        )
}

fn xml_dict_field_ty(program: &Program, id: u32, key: u32, index: u32) -> Option<Ty> {
    if program.structs.get(id as usize)?.fields.get(key as usize)?.ty != Ty::Str {
        return None;
    }
    match index {
        0 => Some(Ty::DynStructArray(id, Layout::Aos)),
        1 => Some(Ty::DynArray(Scalar::Int(IntTy { bits: 64, signed: true }))),
        2 => Some(Ty::DynArray(Scalar::Str)),
        _ => None,
    }
}

/// Physical inline field GEPs traverse ordinary records only; SoA fields are column views.
fn inline_struct_path_ty(program: &Program, id: u32, path: &[u32]) -> Option<Ty> {
    if path.is_empty() { return None; }
    let mut ty = Ty::Struct(id);
    for field in path {
        let Ty::Struct(id) = ty else { return None; };
        ty = program.structs.get(id as usize)?.fields.get(*field as usize)?.ty;
    }
    Some(ty)
}

fn xml_selected_ty(
    program: &Program,
    mut ty: Ty,
    path: &[XmlAccessPathSegment],
) -> Option<Ty> {
    for segment in path {
        ty = match *segment {
            XmlAccessPathSegment::StructField(field) => {
                let id = match ty {
                    Ty::Struct(id) | Ty::Soa(id) => id,
                    _ => return None,
                };
                program
                    .structs
                    .get(id as usize)?
                    .fields
                    .get(field as usize)?
                    .ty
            }
            XmlAccessPathSegment::TupleElement(index) => {
                if let Ty::DictEncoded(id, key) = ty {
                    ty = xml_dict_field_ty(program, id, key, index)?;
                    continue;
                }
                let Ty::Tuple(id) = ty else { return None };
                scalar_to_ty(*program.tuples.get(id as usize)?.elems.get(index as usize)?)
            }
            XmlAccessPathSegment::Element => match ty {
                Ty::DynResponseArray => Ty::HttpResponse,
                Ty::Box(payload)
                | Ty::Array(payload, _)
                | Ty::Slice(payload)
                | Ty::DynArray(payload)
                | Ty::Task(payload)
                | Ty::DynVecArray(payload, _)
                | Ty::DynMaskArray(payload, _)
                | Ty::DynFixedArray(payload, _) => scalar_to_ty(payload),
                Ty::DynSliceArray(payload) => {
                    scalar_to_ty(align_sema::prim_to_scalar(payload))
                }
                Ty::ArrayBuilder(payload) => scalar_to_ty(payload),
                Ty::VecArrayBuilder(payload, lanes) => Ty::Vec(payload, lanes),
                Ty::MaskArrayBuilder(payload, lanes) => Ty::Mask(payload, lanes),
                Ty::FixedArrayBuilder(payload, length) => Ty::Array(payload, length),
                Ty::FixedStructArrayBuilder(id, length) => Ty::StructArray(id, length),
                Ty::StructArray(id, _)
                | Ty::DynStructArray(id, _)
                | Ty::DynFixedStructArray(id, _) => Ty::Struct(id),
                _ => return None,
            },
            XmlAccessPathSegment::EnumPayload {
                enum_id,
                variant,
                slot,
            } => {
                if ty != Ty::Enum(enum_id) {
                    return None;
                }
                let payload = program
                    .enums
                    .get(enum_id as usize)?
                    .variants
                    .get(variant as usize)?
                    .payload
                    .get(slot as usize)?;
                scalar_to_ty(*payload)
            }
            XmlAccessPathSegment::OptionSome => xml_option_payload(program, ty)?,
            XmlAccessPathSegment::ResultOk => xml_result_payload(program, ty, true)?,
            XmlAccessPathSegment::ResultErr => xml_result_payload(program, ty, false)?,
        };
    }
    Some(ty)
}

fn xml_inline_array_element(program: &Program, ty: Ty) -> Option<Ty> {
    match ty {
        Ty::Array(element, _) => Some(scalar_to_ty(element)),
        Ty::StructArray(id, _) if program.structs.get(id as usize).is_some() => {
            Some(Ty::Struct(id))
        }
        _ => None,
    }
}

fn http_client_owner_leaf(ty: Ty) -> bool {
    matches!(ty, Ty::HttpClient | Ty::HttpRequest | Ty::HttpResponse | Ty::DynResponseArray)
}

fn xml_owned_leaf_paths(
    program: &Program,
    root: Ty,
) -> Option<Vec<(Ty, Vec<XmlAccessPathSegment>)>> {
    let mut leaves = Vec::new();
    let mut pending = vec![(root, Vec::new(), Vec::<Ty>::new())];
    while let Some((ty, path, mut ancestors)) = pending.pop() {
        if matches!(ty, Ty::Str | Ty::String | Ty::XmlReader | Ty::Fn(_)) || http_client_owner_leaf(ty) {
            leaves.push((ty, path));
            continue;
        }
        let aggregate = matches!(
            ty,
            Ty::Struct(_)
                | Ty::Tuple(_)
                | Ty::DictEncoded(..)
                | Ty::Enum(_)
                | Ty::Option(_)
                | Ty::Result(..)
                | Ty::Tagged(_)
                | Ty::Box(_)
                | Ty::Array(..)
                | Ty::StructArray(..)
                | Ty::DynStructArray(..)
                | Ty::Slice(_)
                | Ty::DynSliceArray(_)
                | Ty::DynArray(_)
                | Ty::DynVecArray(..)
                | Ty::DynMaskArray(..)
                | Ty::DynFixedArray(..)
                | Ty::DynFixedStructArray(..)
                | Ty::Soa(_)
                | Ty::Task(_)
                | Ty::ArrayBuilder(_)
                | Ty::VecArrayBuilder(..)
                | Ty::MaskArrayBuilder(..)
                | Ty::FixedArrayBuilder(..)
                | Ty::FixedStructArrayBuilder(..)
        );
        if aggregate {
            if ancestors.contains(&ty) {
                return None;
            }
            ancestors.push(ty);
        }
        match ty {
            Ty::Struct(id) => {
                let definition = program.structs.get(id as usize)?;
                for (field, definition) in definition.fields.iter().enumerate() {
                    let mut selected = path.clone();
                    selected.push(XmlAccessPathSegment::StructField(u32::try_from(field).ok()?));
                    pending.push((definition.ty, selected, ancestors.clone()));
                }
            }
            Ty::Tuple(id) => {
                let definition = program.tuples.get(id as usize)?;
                for (index, element) in definition.elems.iter().enumerate() {
                    let mut selected = path.clone();
                    selected.push(XmlAccessPathSegment::TupleElement(u32::try_from(index).ok()?));
                    pending.push((scalar_to_ty(*element), selected, ancestors.clone()));
                }
            }
            Ty::DictEncoded(id, key) => {
                for index in 0..3 {
                    let mut selected = path.clone();
                    selected.push(XmlAccessPathSegment::TupleElement(index));
                    pending.push((xml_dict_field_ty(program, id, key, index)?, selected, ancestors.clone()));
                }
            }
            Ty::Enum(enum_id) => {
                let definition = program.enums.get(enum_id as usize)?;
                for (variant, definition) in definition.variants.iter().enumerate() {
                    for (slot, payload) in definition.payload.iter().enumerate() {
                        let mut selected = path.clone();
                        selected.push(XmlAccessPathSegment::EnumPayload {
                            enum_id,
                            variant: u32::try_from(variant).ok()?,
                            slot: u32::try_from(slot).ok()?,
                        });
                        pending.push((scalar_to_ty(*payload), selected, ancestors.clone()));
                    }
                }
            }
            Ty::Option(payload) => {
                let mut selected = path;
                selected.push(XmlAccessPathSegment::OptionSome);
                pending.push((scalar_to_ty(payload), selected, ancestors));
            }
            Ty::Result(success, failure) => {
                let mut ok = path.clone();
                ok.push(XmlAccessPathSegment::ResultOk);
                pending.push((scalar_to_ty(success), ok, ancestors.clone()));
                let mut error = path;
                error.push(XmlAccessPathSegment::ResultErr);
                pending.push((scalar_to_ty(failure), error, ancestors));
            }
            Ty::Tagged(id) => match program.tagged_types.get(id as usize)? {
                hir::TaggedType::Option(payload) => {
                    let mut selected = path;
                    selected.push(XmlAccessPathSegment::OptionSome);
                    pending.push((scalar_to_ty(*payload), selected, ancestors));
                }
                hir::TaggedType::Result(success, failure) => {
                    let mut ok = path.clone();
                    ok.push(XmlAccessPathSegment::ResultOk);
                    pending.push((scalar_to_ty(*success), ok, ancestors.clone()));
                    let mut error = path;
                    error.push(XmlAccessPathSegment::ResultErr);
                    pending.push((scalar_to_ty(*failure), error, ancestors));
                }
            },
            Ty::Box(payload)
            | Ty::Array(payload, _)
            | Ty::Slice(payload)
            | Ty::DynArray(payload)
            | Ty::Task(payload)
            | Ty::DynVecArray(payload, _)
            | Ty::DynMaskArray(payload, _)
            | Ty::DynFixedArray(payload, _) => {
                let mut selected = path;
                selected.push(XmlAccessPathSegment::Element);
                pending.push((scalar_to_ty(payload), selected, ancestors));
            }
            Ty::DynSliceArray(payload) => {
                let mut selected = path;
                selected.push(XmlAccessPathSegment::Element);
                pending.push((
                    scalar_to_ty(align_sema::prim_to_scalar(payload)),
                    selected,
                    ancestors,
                ));
            }
            Ty::ArrayBuilder(payload) => {
                let mut selected = path;
                selected.push(XmlAccessPathSegment::Element);
                pending.push((scalar_to_ty(payload), selected, ancestors));
            }
            Ty::VecArrayBuilder(payload, lanes) => {
                let mut selected = path;
                selected.push(XmlAccessPathSegment::Element);
                pending.push((Ty::Vec(payload, lanes), selected, ancestors));
            }
            Ty::MaskArrayBuilder(payload, lanes) => {
                let mut selected = path;
                selected.push(XmlAccessPathSegment::Element);
                pending.push((Ty::Mask(payload, lanes), selected, ancestors));
            }
            Ty::FixedArrayBuilder(payload, length) => {
                let mut selected = path;
                selected.push(XmlAccessPathSegment::Element);
                pending.push((Ty::Array(payload, length), selected, ancestors));
            }
            Ty::FixedStructArrayBuilder(id, length) => {
                let mut selected = path;
                selected.push(XmlAccessPathSegment::Element);
                pending.push((Ty::StructArray(id, length), selected, ancestors));
            }
            Ty::StructArray(id, _)
            | Ty::DynStructArray(id, _)
            | Ty::DynFixedStructArray(id, _) => {
                let mut selected = path;
                selected.push(XmlAccessPathSegment::Element);
                pending.push((Ty::Struct(id), selected, ancestors));
            }
            Ty::Soa(id) => {
                let definition = program.structs.get(id as usize)?;
                for (field, definition) in definition.fields.iter().enumerate() {
                    let mut selected = path.clone();
                    selected.push(XmlAccessPathSegment::StructField(
                        u32::try_from(field).ok()?,
                    ));
                    pending.push((definition.ty, selected, ancestors.clone()));
                }
            }
            _ => {}
        }
    }
    Some(leaves)
}

fn xml_direct_call_facts(
    program: &Program,
    target: &ProgramCall,
    local_contracts: &HashSet<ProgramCall>,
) -> Option<XmlCallFacts> {
    let mut found = None;
    for function in &program.fns {
        if &function.name != target {
            continue;
        }
        if !local_contracts.contains(target) {
            return None;
        }
        let params = function
            .params
            .iter()
            .map(|slot| function.slots.get(*slot as usize).copied())
            .collect::<Option<Vec<_>>>()?;
        let facts = XmlCallFacts {
            params,
            modes: function.param_modes.clone(),
            ret: function.ret,
            borrow: function.return_borrow.clone(),
            region: function.return_region.clone(),
            cleanup: function.return_cleanup,
        };
        if found.replace(facts).is_some() {
            return None;
        }
    }
    for function in &program.imported_fns {
        if &function.name != target {
            continue;
        }
        if !function.producer_certified {
            return None;
        }
        let facts = XmlCallFacts {
            params: function.params.clone(),
            modes: function.param_modes.clone(),
            ret: function.ret,
            borrow: function.return_borrow.clone(),
            region: function.return_region.clone(),
            cleanup: function.return_cleanup,
        };
        if found.replace(facts).is_some() {
            return None;
        }
    }
    found
}


fn xml_const_element_matches_ty(element: &ConstElem, ty: Ty) -> bool {
    matches!(
        (element, ty),
        (ConstElem::Int(_), Ty::Int(_))
            | (ConstElem::Float(_), Ty::Float(_))
            | (ConstElem::Char(_), Ty::Char)
            | (ConstElem::Bool(_), Ty::Bool)
            | (ConstElem::Str(_), Ty::Str)
    )
}

/// Exact native-owner contracts, shared by value and out-slot producer checks.
pub struct NativeOwnerMirContract<'a> {
    pub result: Ty,
    pub outputs: Vec<(Slot, Ty)>,
    operands: Vec<(&'a Operand, Ty, OperandRequirement)>,
    writable_buffers: Vec<&'a Operand>,
    access: XmlAccessProvenance,
}

pub fn fs_tree_output_slots(output: crate::FsTreeOutput) -> Vec<Slot> {
    use crate::FsTreeOutput::*;
    match output {
        None => Vec::new(),
        Owner(slot) | Metadata(slot) | Bytes(slot) | Bool(slot) => vec![slot],
        CursorNext { entry, present } => vec![entry, present],
    }
}

pub fn native_owner_mir_contract<'a>(
    program: &Program,
    function: &Function,
    value: &'a Rvalue,
) -> Option<NativeOwnerMirContract<'a>> {
    let read = OperandRequirement::READ;
    let write = OperandRequirement {
        write: true,
        exclusive: true,
        ..read
    };
    let consume = OperandRequirement {
        move_value: true,
        ..read
    };
    let i32_ty = Ty::Int(IntTy {
        bits: 32,
        signed: true,
    });
    let i64_ty = Ty::Int(IntTy {
        bits: 64,
        signed: true,
    });
    let bytes = Ty::Slice(Scalar::Int(IntTy {
        bits: 8,
        signed: false,
    }));
    let byte_view = |operand| match xml_operand_base_ty(function, operand) {
        Some(ty @ (Ty::Str | Ty::String)) => ty,
        Some(ty) if ty == bytes => ty,
        _ => Ty::Error,
    };
    let mut contract = NativeOwnerMirContract {
        result: Ty::Unit,
        outputs: Vec::new(),
        operands: Vec::new(),
        writable_buffers: Vec::new(),
        access: XmlAccessProvenance::Owned,
    };
    match value {
        Rvalue::ProcessLive { kind, args, out } => {
            contract.result = if kind.fallible() { i32_ty } else if out.is_some() { Ty::Unit }
                else if matches!(kind,align_sema::process_live::ProcessLiveKind::ScopeId | align_sema::process_live::ProcessLiveKind::ScopeOwnerId | align_sema::process_live::ProcessLiveKind::ChildId | align_sema::process_live::ProcessLiveKind::SignalNumber | align_sema::process_live::ProcessLiveKind::SealedLen | align_sema::process_live::ProcessLiveKind::ImageLen) { i64_ty } else { Ty::Unit };
            for (index,(input,operand)) in kind.inputs().iter().zip(args).enumerate() {
                use align_sema::process_live::Input;
                if *input==Input::OutBytes {
                    // An Out buffer proves its writable backing without reading its elements.
                    // The separate shape gate still fixes the exact slice type.
                    contract.writable_buffers.push(operand);
                    continue;
                }
                let actual = xml_operand_base_ty(function,operand).unwrap_or(Ty::Error);
                let expected = match input { Input::Owner(ty) => *ty, Input::Integer => i64_ty,
                    Input::Bool => Ty::Bool, Input::OutBytes => bytes, Input::Readiness | Input::Signal | Input::SignalSet | Input::MemoryKind | Input::Bytes | Input::Argv => actual, Input::Text => Ty::Str };
                let requirement = if kind.consumes(index) { consume } else if index==0 && kind.exclusive() && matches!(input,Input::Owner(_)) { write } else { read };
                contract.operands.push((operand,expected,requirement));
            }
            if let Some(out) = out { contract.outputs.push((*out,function.slots.get(*out as usize).copied().unwrap_or(Ty::Error))); }
        }
        Rvalue::CommandCwd { command, dir } => {
            contract.operands=vec![(command,Ty::Command,write),(dir,Ty::Str,read)];
        }
        Rvalue::CommandTimeout { command, ns } | Rvalue::CommandMaxCapture { command, limit: ns } => {
            contract.operands=vec![(command,Ty::Command,write),(ns,i64_ty,read)];
        }
        Rvalue::CommandEnv { command, name, value } => {
            contract.operands=vec![(command,Ty::Command,write),(name,Ty::Str,read),(value,Ty::Str,read)];
        }
        Rvalue::CommandEnvClear { command } => contract.operands=vec![(command,Ty::Command,write)],
        Rvalue::CommandRun { command, out } | Rvalue::CommandRunBytes { command, out } => {
            contract.result=i32_ty;
            contract.operands=vec![(command,Ty::Command,read)];
            contract.outputs=vec![(*out,if matches!(value,Rvalue::CommandRun { .. }) { Ty::RunOutput } else { Ty::RunBytes })];
        }
        Rvalue::ChildWait { child, out } => {
            contract.result = i32_ty;
            contract.operands.push((child, Ty::Child, write));
            contract.outputs.push((*out, function.slots.get(*out as usize).copied().unwrap_or(Ty::Error)));
        }
        Rvalue::FsTree { kind, args, output } => {
            contract.result = i32_ty;
            for (input, operand) in kind.inputs().iter().zip(args) {
                let actual = xml_operand_base_ty(function, operand).unwrap_or(Ty::Error);
                let expected = if align_sema::fs_tree::input_matches(*input, actual, &program.structs) { actual } else { Ty::Error };
                let requirement = if kind.exclusive() && matches!(input, align_sema::fs_tree::Input::Owner(_)) { write } else { read };
                contract.operands.push((operand, expected, requirement));
            }
            contract.outputs = fs_tree_output_slots(*output).into_iter().map(|slot|
                (slot, function.slots.get(slot as usize).copied().unwrap_or(Ty::Error))).collect();
        }
        Rvalue::OsHost { out } | Rvalue::OsIdentity { out } => {
            // Exact nominal schema is independently certified by validate_host_mir.
            contract.result = i32_ty;
            contract.outputs = vec![(*out, function.slots.get(*out as usize).copied().unwrap_or(Ty::Error))];
        }
        Rvalue::FsCreateDir { path } => {
            contract.result = i32_ty;
            contract.operands = vec![(path, Ty::Str, read)];
        }
        Rvalue::FsIsDir { path, out } => {
            contract.result = i32_ty;
            contract.outputs = vec![(*out,Ty::Bool)];
            contract.operands = vec![(path, Ty::Str, read)];
        }
        Rvalue::CryptoDigestNew => contract.result = Ty::CryptoDigest,
        Rvalue::CryptoDigestUpdate { digest, data } => {
            let data_ty = match xml_operand_base_ty(function, data) {
                Some(Ty::Str) => Ty::Str,
                Some(ty) if ty == bytes => bytes,
                _ => Ty::Error,
            };
            contract.operands = vec![(digest, Ty::CryptoDigest, write), (data, data_ty, read)];
        }
        Rvalue::CryptoDigestFinish(digest) => {
            contract.result = Ty::DynArray(Scalar::Int(IntTy { bits: 8, signed: false }));
            contract.operands = vec![(digest, Ty::CryptoDigest, consume)];
        }
        Rvalue::HttpClient => contract.result = Ty::HttpClient,
        Rvalue::HttpRequest { method, url } => {
            contract.result = Ty::HttpRequest;
            contract.operands = vec![(method, Ty::Str, read), (url, Ty::Str, read)];
        }
        Rvalue::HttpHeader { req, name, value } => {
            contract.operands = vec![
                (req, Ty::HttpRequest, write),
                (name, Ty::Str, read),
                (value, Ty::Str, read),
            ];
        }
        Rvalue::HttpBody { req, data } => {
            contract.operands = vec![(req, Ty::HttpRequest, write), (data, byte_view(data), read)];
        }
        Rvalue::HttpRequestTimeout { req, ns: limit }
        | Rvalue::HttpRequestMaxResponseBodyBytes { req, limit } => {
            contract.operands = vec![(req, Ty::HttpRequest, write), (limit, i64_ty, read)];
        }
        Rvalue::HttpClientTimeout { client, ns: limit }
        | Rvalue::HttpClientMaxResponseBodyBytes { client, limit } => {
            contract.operands = vec![(client, Ty::HttpClient, write), (limit, i64_ty, read)];
        }
        Rvalue::HttpParse { data, out } => {
            contract.result = i32_ty;
            contract.outputs = vec![(*out, Ty::HttpResponse)];
            contract.operands = vec![(data, byte_view(data), read)];
        }
        Rvalue::HttpRespStatus { resp } => {
            contract.result = i64_ty;
            contract.operands = vec![(resp, Ty::HttpResponse, read)];
        }
        Rvalue::HttpRespBody { resp } => {
            contract.result = bytes;
            contract.operands = vec![(resp, Ty::HttpResponse, read)];
            contract.access = XmlAccessProvenance::Shared;
        }
        Rvalue::HttpRespHeader { resp, name, out } => {
            contract.result = i32_ty;
            contract.outputs = vec![(*out, Ty::Str)];
            contract.operands = vec![(resp, Ty::HttpResponse, read), (name, Ty::Str, read)];
            contract.access = XmlAccessProvenance::Shared;
        }
        Rvalue::HttpClientGet { client, url, out } => {
            contract.result = i32_ty;
            contract.outputs = vec![(*out, Ty::HttpResponse)];
            contract.operands = vec![(client, Ty::HttpClient, read), (url, Ty::Str, read)];
        }
        Rvalue::HttpClientPost {
            client,
            url,
            body,
            out,
        } => {
            contract.result = i32_ty;
            contract.outputs = vec![(*out, Ty::HttpResponse)];
            contract.operands = vec![
                (client, Ty::HttpClient, read),
                (url, Ty::Str, read),
                (body, byte_view(body), read),
            ];
        }
        Rvalue::HttpClientRequest { client, req, out }
        | Rvalue::HttpClientRequestStream { client, req, out } => {
            contract.result = i32_ty;
            contract.outputs = vec![(
                *out,
                if matches!(value, Rvalue::HttpClientRequestStream { .. }) {
                    Ty::HttpReadStream
                } else {
                    Ty::HttpResponse
                },
            )];
            contract.operands = vec![
                (client, Ty::HttpClient, read),
                (req, Ty::HttpRequest, consume),
            ];
        }
        Rvalue::HttpGetMany {
            client,
            urls,
            max_concurrency,
            out,
        } => {
            contract.result = i32_ty;
            contract.outputs = vec![(*out, Ty::DynResponseArray)];
            contract.operands = vec![
                (client, Ty::HttpClient, read),
                (urls, match xml_operand_base_ty(function, urls) {
                    Some(ty @ (Ty::Array(Scalar::Str, _) | Ty::Slice(Scalar::Str) | Ty::DynArray(Scalar::Str))) => ty,
                    _ => Ty::Error,
                }, read),
                (max_concurrency, i64_ty, read),
            ];
        }
        _ => return None,
    }
    Some(contract)
}

fn xml_written_slots(rvalue: &Rvalue) -> Vec<(Slot, XmlAccessProvenance)> {
    use XmlAccessProvenance::{Owned, Shared};

    let owned = |slot| vec![(slot, Owned)];
    match rvalue {
        Rvalue::ProcessLive { out, .. } => out.iter().map(|slot|(*slot,Owned)).collect(),
        Rvalue::ChildWait { out, .. } => vec![(*out, Owned)],
        Rvalue::FsTree { output, .. } => fs_tree_output_slots(*output).into_iter().map(|slot| (slot, Owned)).collect(),
        Rvalue::JsonDecode { out, arena, .. }
        | Rvalue::JsonDecodeStructArray { out, arena, .. }
        | Rvalue::JsonDecodeUnion { out, arena, .. } => {
            vec![(*out, if arena.is_some() { Shared } else { Owned })]
        }
        Rvalue::JsonDecodeSoa { out, .. } | Rvalue::CsvDecode { out, .. } => {
            vec![(*out, Shared)]
        }
        Rvalue::JsonDoc { out, .. }
        | Rvalue::JsonDocGet { out, .. }
        | Rvalue::JsonDocAt { out, .. }
        | Rvalue::JsonDocAsStr { out, .. }
        | Rvalue::JsonDocKey { out, .. }
        | Rvalue::JsonDocElems { out, .. }
        | Rvalue::BytesAsStr { out, .. }
        | Rvalue::FsReadFileView { out, .. }
        | Rvalue::FsReadBytesView { out, .. }
        | Rvalue::HttpRespHeader { out, .. }
        | Rvalue::HttpReadStreamHeader { out, .. }
        | Rvalue::HttpCtxHeader { out, .. } => vec![(*out, Shared)],
        Rvalue::JsonScanNext { cursor, row, .. } => {
            vec![(*cursor, Owned), (*row, Shared)]
        }
        Rvalue::HttpSseStreamNext {
            present,
            retry_present,
            retry_ms,
            event,
            data,
            last_event_id,
            ..
        } => vec![
            (*present, Owned),
            (*retry_present, Owned),
            (*retry_ms, Owned),
            (*event, Shared),
            (*data, Shared),
            (*last_event_id, Shared),
        ],
        Rvalue::CryptoArgon2(args) => owned(args.out),
        Rvalue::CryptoPublicKeyFromJwk(args) => owned(args.out),
        Rvalue::CryptoVerify(args) => owned(args.out),
        Rvalue::JsonEncode { out, .. }
        | Rvalue::JsonOwnedDecode { out, .. }
        | Rvalue::JsonDecodeArray { out, .. }
        | Rvalue::JsonDecodeScalar { out, .. }
        | Rvalue::JsonDocAsScalar { out, .. }
        | Rvalue::FsReadFile { out, .. }
        | Rvalue::FsCreatePrivateTempDir { out, .. }
        | Rvalue::ReaderOpen { out, .. }
        | Rvalue::ReaderOpenBeneath { out, .. }
        | Rvalue::ReaderOpenBeneathSingleLink { out, .. }
        | Rvalue::WriterCreate { out, .. }
        | Rvalue::WriterCreateExclusive { out, .. }
        | Rvalue::WriterCreateExclusiveBeneath { out, .. }
        | Rvalue::CodecEncoderNew { out, .. }
        | Rvalue::FsIsDir { out, .. }
        | Rvalue::OsHost { out }
        | Rvalue::OsIdentity { out }
        | Rvalue::FrameInnerJoin { out, .. }
        | Rvalue::FileCreateRw { out, .. }
        | Rvalue::FileOpenRw { out, .. }
        | Rvalue::FsReadDir { out, .. }
        | Rvalue::DnsResolve { out, .. }
        | Rvalue::TcpConnect { out, .. }
        | Rvalue::TcpListen { out, .. }
        | Rvalue::TcpAccept { out, .. }
        | Rvalue::UdpBind { out, .. }
        | Rvalue::ProcessSpawn { out, .. }
        | Rvalue::EnvGet { out, .. }
        | Rvalue::RegexCompile { out, .. }
        | Rvalue::RegexFind { out, .. }
        | Rvalue::RegexFindAll { out, .. }
        | Rvalue::RegexSplit { out, .. }
        | Rvalue::RegexCaptures { out, .. }
        | Rvalue::CapturesGroup { out, .. }
        | Rvalue::TimeFormat { out, .. }
        | Rvalue::TimeParse { out, .. }
        | Rvalue::EncodingDecode { out, .. }
        | Rvalue::CompressCompress { out, .. }
        | Rvalue::CompressDecompress { out, .. }
        | Rvalue::CryptoHkdf { out, .. }
        | Rvalue::CryptoAead { out, .. }
        | Rvalue::CryptoPrivateKeyFromPem { out, .. }
        | Rvalue::CryptoPublicKeyFromPem { out, .. }
        | Rvalue::CryptoSign { out, .. }
        | Rvalue::RandSeed { out, .. }
        | Rvalue::CliParse { out, .. }
        | Rvalue::CommandRun { out, .. }
        | Rvalue::CommandRunBytes { out, .. }
        | Rvalue::HttpParse { out, .. }
        | Rvalue::HttpClientGet { out, .. }
        | Rvalue::HttpClientPost { out, .. }
        | Rvalue::HttpClientRequest { out, .. }
        | Rvalue::HttpClientRequestStream { out, .. }
        | Rvalue::HttpReadStreamRead { out, .. }
        | Rvalue::HttpGetMany { out, .. }
        | Rvalue::HttpServe { out, .. }
        | Rvalue::HttpAccept { out, .. }
        | Rvalue::HttpRespondStream { out, .. }
        | Rvalue::HttpRespondUpgrade { out, .. } => owned(*out),
        _ => Vec::new(),
    }
}

fn xml_out_producer_result_ty(rvalue: &Rvalue) -> Option<Ty> {
    if xml_written_slots(rvalue).is_empty() {
        return None;
    }
    Some(match rvalue {
        Rvalue::JsonDocGet { .. }
        | Rvalue::JsonDocAt { .. }
        | Rvalue::JsonDocElems { .. }
        | Rvalue::RandSeed { .. } => Ty::Unit,
        _ => Ty::Int(IntTy {
            bits: 32,
            signed: true,
        }),
    })
}

fn xml_out_producer_operands(rvalue: &Rvalue) -> Vec<&Operand> {
    match rvalue {
        Rvalue::JsonEncode { max_bytes, .. } => max_bytes.iter().collect(),
        Rvalue::JsonDecode { input, arena, .. }
        | Rvalue::JsonDecodeStructArray { input, arena, .. }
        | Rvalue::JsonDecodeUnion { input, arena, .. } => {
            let mut operands = vec![input];
            operands.extend(arena.iter());
            operands
        }
        Rvalue::JsonOwnedDecode { input, .. }
        | Rvalue::JsonDecodeArray { input, .. }
        | Rvalue::JsonDecodeScalar { input, .. }
        | Rvalue::FsReadFile { path: input, .. }
        | Rvalue::FsCreatePrivateTempDir { prefix: input, .. }
        | Rvalue::ReaderOpen { path: input, .. }
        | Rvalue::WriterCreate { path: input, .. }
        | Rvalue::WriterCreateExclusive { path: input, .. }
        | Rvalue::FileCreateRw { path: input, .. }
        | Rvalue::FileOpenRw { path: input, .. }
        | Rvalue::FsReadDir { path: input, .. }
        | Rvalue::DnsResolve { host: input, .. }
        | Rvalue::BytesAsStr { bytes: input, .. }
        | Rvalue::EnvGet { name: input, .. }
        | Rvalue::RegexCompile { pattern: input, .. }
        | Rvalue::TimeFormat { ns: input, .. }
        | Rvalue::TimeParse { input, .. }
        | Rvalue::EncodingDecode { input, .. }
        | Rvalue::CompressDecompress { data: input, .. }
        | Rvalue::CryptoPrivateKeyFromPem { pem: input, .. }
        | Rvalue::CryptoPublicKeyFromPem { pem: input, .. }
        | Rvalue::CommandRun { command: input, .. }
        | Rvalue::CommandRunBytes { command: input, .. }
        | Rvalue::HttpParse { data: input, .. }
        | Rvalue::HttpAccept { server: input, .. } => vec![input],
        Rvalue::JsonDecodeSoa { input, arena, .. }
        | Rvalue::JsonDoc { input, arena, .. }
        | Rvalue::FsReadFileView {
            path: input, arena, ..
        }
        | Rvalue::FsReadBytesView {
            path: input, arena, ..
        } => vec![input, arena],
        Rvalue::CsvDecode {
            input,
            arena,
            options,
            ..
        } => vec![input, arena, options],
        Rvalue::JsonDocGet { doc, key, .. } => vec![doc, key],
        Rvalue::JsonDocAt { doc, index, .. } | Rvalue::JsonDocKey { doc, index, .. } => {
            vec![doc, index]
        }
        Rvalue::JsonDocAsStr { doc, .. } | Rvalue::JsonDocAsScalar { doc, .. } => vec![doc],
        Rvalue::JsonDocElems { doc, arena, .. } => vec![doc, arena],
        Rvalue::JsonScanNext { scanner, .. } => vec![scanner],
        Rvalue::ReaderOpenBeneath { root, relative, .. }
        | Rvalue::ReaderOpenBeneathSingleLink { root, relative, .. }
        | Rvalue::WriterCreateExclusiveBeneath { root, relative, .. } => vec![root, relative],
        Rvalue::CodecEncoderNew { rows, .. } => vec![rows],
        Rvalue::FrameInnerJoin {
            left,
            right,
            max_pairs,
            ..
        } => vec![left, right, max_pairs],
        Rvalue::TcpConnect {
            host,
            port,
            timeout_ns,
            ..
        } => vec![host, port, timeout_ns],
        Rvalue::TcpListen { host, port, .. }
        | Rvalue::UdpBind { host, port, .. } => vec![host, port],
        Rvalue::TcpAccept { listener, .. } => vec![listener],
        Rvalue::ProcessSpawn { cmd, args, .. } | Rvalue::CliParse { cmd, args, .. } => {
            vec![cmd, args]
        }
        Rvalue::RegexFind {
            regex,
            text,
            start,
            ..
        } => vec![regex, text, start],
        Rvalue::RegexFindAll { regex, text, .. }
        | Rvalue::RegexSplit { regex, text, .. }
        | Rvalue::RegexCaptures { regex, text, .. } => vec![regex, text],
        Rvalue::CapturesGroup { caps, index, .. } => vec![caps, index],
        Rvalue::CompressCompress { data, level, .. } => vec![data, level],
        Rvalue::CryptoHkdf {
            salt,
            ikm,
            info,
            len,
            ..
        } => vec![salt, ikm, info, len],
        Rvalue::CryptoAead {
            key,
            nonce,
            input,
            aad,
            ..
        } => vec![key, nonce, input, aad],
        Rvalue::CryptoArgon2(args) => vec![
            &args.password,
            &args.salt,
            &args.m_cost,
            &args.t_cost,
            &args.parallelism,
            &args.len,
        ],
        Rvalue::CryptoPublicKeyFromJwk(args) => {
            let mut operands = vec![&args.first];
            operands.extend(args.second.iter());
            operands
        }
        Rvalue::CryptoVerify(args) => vec![&args.key, &args.message, &args.signature],
        Rvalue::CryptoSign { key, message, .. } => vec![key, message],
        Rvalue::RandSeed { seed, .. } => seed.iter().collect(),
        Rvalue::HttpRespHeader { resp, name, .. } => vec![resp, name],
        Rvalue::HttpClientGet { client, url, .. } => vec![client, url],
        Rvalue::HttpClientPost {
            client,
            url,
            body,
            ..
        } => vec![client, url, body],
        Rvalue::HttpClientRequest { client, req, .. }
        | Rvalue::HttpClientRequestStream { client, req, .. } => vec![client, req],
        Rvalue::HttpReadStreamHeader { stream, name, .. } => vec![stream, name],
        Rvalue::HttpReadStreamRead { stream, buffer, .. }
        | Rvalue::HttpSseStreamNext { stream, buffer, .. } => vec![stream, buffer],
        Rvalue::HttpGetMany {
            client,
            urls,
            max_concurrency,
            ..
        } => vec![client, urls, max_concurrency],
        Rvalue::HttpServe { host, port, .. } => vec![host, port],
        Rvalue::HttpCtxHeader { ctx, name, .. } => vec![ctx, name],
        Rvalue::HttpRespondStream { ctx, rb, .. }
        | Rvalue::HttpRespondUpgrade { ctx, rb, .. } => vec![ctx, rb],
        _ => Vec::new(),
    }
}

fn xml_array_builder_output(builder: Ty) -> Option<Ty> {
    Some(match builder {
        Ty::ArrayBuilder(Scalar::Struct(id)) => Ty::DynStructArray(id, Layout::Aos),
        Ty::ArrayBuilder(element) => Ty::DynArray(element),
        Ty::VecArrayBuilder(element, lanes) => Ty::DynVecArray(element, lanes),
        Ty::MaskArrayBuilder(element, lanes) => Ty::DynMaskArray(element, lanes),
        Ty::FixedArrayBuilder(element, length) => Ty::DynFixedArray(element, length),
        Ty::FixedStructArrayBuilder(id, length) => Ty::DynFixedStructArray(id, length),
        _ => return None,
    })
}

fn assert_xml_stmt_variant_classified(statement: &Stmt) {
    match statement {
        Stmt::Let(..)
        | Stmt::Store(..)
        | Stmt::StoreField(..)
        | Stmt::StoreIndex(..)
        | Stmt::StoreConstArray { .. }
        | Stmt::PtrStore(..)
        | Stmt::PtrStoreNoalias { .. }
        | Stmt::VecStore { .. }
        | Stmt::StoreElemField(..)
        | Stmt::StoreElemFieldPtr { .. }
        | Stmt::StoreColumn { .. }
        | Stmt::ArenaEnd(..)
        | Stmt::RawFree(..)
        | Stmt::ColumnBatchFinish { .. }
        | Stmt::ColumnBatchDrop { .. }
        | Stmt::RawStore { .. }
        | Stmt::TgWait(..)
        | Stmt::TgEnd(..)
        | Stmt::DropFlagInit(..)
        | Stmt::NullTupleField(..)
        | Stmt::NullStructField(..)
        | Stmt::NullElemField(..)
        | Stmt::Drop(..)
        | Stmt::DropElem(..)
        | Stmt::DropElemField(..)
        | Stmt::BorrowedElementReservation { .. }
        | Stmt::DropValue(..) => {}
    }
}

struct XmlAccessAnalyzer<'a> {
    graph: &'a ValidatedProducerGraph<'a>,
    equations: HashMap<XmlAccessNode, XmlAccessEquation>,
    scheduled: HashSet<XmlAccessNode>,
    pending: VecDeque<XmlAccessNode>,
}

impl<'a> XmlAccessAnalyzer<'a> {
    fn new(graph: &'a ValidatedProducerGraph<'a>) -> Self {
        Self {
            graph,
            equations: HashMap::new(),
            scheduled: HashSet::new(),
            pending: VecDeque::new(),
        }
    }

    fn queue(&mut self, node: XmlAccessNode) -> XmlAccessSource {
        if self.scheduled.insert(node.clone()) {
            self.pending.push_back(node.clone());
        }
        XmlAccessSource::Node(node)
    }

    fn source(
        &mut self,
        operand: &Operand,
        expected: Ty,
        path: Vec<XmlAccessPathSegment>,
    ) -> XmlAccessSource {
        let Some(base) = xml_operand_base_ty(self.graph.function, operand) else {
            return XmlAccessSource::Invalid;
        };
        if !xml_selected_ty(self.graph.program, base, &path)
            .is_some_and(|actual| xml_callable_flow_matches(self.graph.program, actual, expected)) {
            return XmlAccessSource::Invalid;
        }
        match operand {
            Operand::Arg(index) => {
                XmlAccessSource::Seed(xml_argument_access(self.graph.function, *index))
            }
            Operand::Const(_) if path.is_empty() => {
                XmlAccessSource::Seed(XmlAccessProvenance::Owned)
            }
            Operand::Value(value) => self.queue(XmlAccessNode::Value(*value, path)),
            Operand::BorrowedCleanupArg(index)
                if path.is_empty()
                    && self
                        .graph
                        .function
                        .borrow_mut_cleanup_slots
                        .get(*index as usize)
                        .is_some_and(Option::is_some) =>
            {
                XmlAccessSource::Seed(XmlAccessProvenance::Shared)
            }
            Operand::BorrowedPlace(_)
            | Operand::BorrowedElementPlace(_)
            | Operand::BorrowedFixedElementPlace(_)
            | Operand::BorrowedCleanupArg(_)
            | Operand::Const(_) => XmlAccessSource::Invalid,
        }
    }

    /// Pointed-to buffer authority is distinct from both element access and header mutability.
    /// An owning array proves its buffer; a Copy view needs a separate grounded projection.
    fn buffer_node(&mut self, node: XmlAccessNode) -> XmlAccessSource {
        let (root, path) = match &node {
            XmlAccessNode::Value(value, path) => (self.graph.function.value_tys.get(*value as usize), path),
            XmlAccessNode::Slot(slot, path) => (self.graph.function.slots.get(*slot as usize), path),
            _ => return XmlAccessSource::Invalid,
        };
        match root.and_then(|root| xml_selected_ty(self.graph.program, *root, path)) {
            Some(Ty::Slice(_) | Ty::DynArray(_) | Ty::DynStructArray(_, Layout::Aos)) => match node {
                XmlAccessNode::Value(value, path) => self.queue(XmlAccessNode::BufferValue(value, path)),
                XmlAccessNode::Slot(slot, path) => self.queue(XmlAccessNode::BufferSlot(slot, path)),
                _ => XmlAccessSource::Invalid,
            },
            _ => XmlAccessSource::Invalid,
        }
    }

    fn buffer_parameter(&self, slot: Slot) -> Result<Option<usize>, ()> {
        let mut found = None;
        for (index, parameter) in self.graph.function.params.iter().enumerate() {
            if *parameter == slot && found.replace(index).is_some() { return Err(()); }
        }
        Ok(found)
    }

    fn buffer_source(&mut self, operand: &Operand, path: Vec<XmlAccessPathSegment>) -> XmlAccessSource {
        if let Operand::Arg(index) = operand {
            let Some(slot) = self.graph.function.params.get(*index as usize) else { return XmlAccessSource::Invalid; };
            if self.buffer_parameter(*slot) != Ok(Some(*index as usize)) { return XmlAccessSource::Invalid; }
        }
        let Some(selected) = xml_operand_base_ty(self.graph.function, operand)
            .and_then(|root| xml_selected_ty(self.graph.program, root, &path))
        else { return XmlAccessSource::Invalid; };
        match operand {
            Operand::Value(value) => self.buffer_node(XmlAccessNode::Value(*value, path)),
            Operand::Arg(index) if matches!(selected, Ty::Slice(_)) => {
                // BorrowMut of a Copy slice authenticates its header, not its backing allocation.
                let out = path.is_empty()
                    && self.graph.function.param_modes.get(*index as usize) == Some(&align_ast::ParamMode::Out);
                XmlAccessSource::Seed(if out { XmlAccessProvenance::Unreadable } else { XmlAccessProvenance::Shared })
            }
            Operand::Arg(index) if matches!(selected, Ty::DynArray(_) | Ty::DynStructArray(_, Layout::Aos)) => {
                XmlAccessSource::Seed(xml_argument_access(self.graph.function, *index))
            }
            _ => XmlAccessSource::Invalid,
        }
    }

    fn buffer_dependencies(&mut self, equation: &mut XmlAccessEquation) {
        // A seed lost its operand identity in the ordinary equation. It cannot certify a buffer.
        if equation.seed.is_some() { equation.seed = Some(XmlAccessProvenance::Shared); }
        for node in std::mem::take(&mut equation.dependencies) {
            let source = self.buffer_node(node);
            Self::add_source(equation, source);
        }
        for node in std::mem::take(&mut equation.read_dependencies) {
            let source = match self.buffer_node(node) {
                XmlAccessSource::Node(node) => XmlAccessSource::ReadNode(node),
                source => source,
            };
            Self::add_source(equation, source);
        }
    }

    fn buffer_value_equation(&mut self, value: ValueId, path: Vec<XmlAccessPathSegment>) -> XmlAccessEquation {
        let mut equation = self.value_equation(value, path.clone());
        if equation.invalid || equation.absent { return equation; }
        let selected = self.graph.function.value_tys.get(value as usize)
            .and_then(|root| xml_selected_ty(self.graph.program, *root, &path));
        if let Some(ty @ (Ty::DynArray(_) | Ty::DynStructArray(_, Layout::Aos))) = selected {
            let Some(leaves) = xml_owned_leaf_paths(self.graph.program, ty) else {
                equation.invalid = true;
                return equation;
            };
            for (leaf, tail) in leaves {
                let mut selected = path.clone();
                selected.extend(tail);
                let source = self.source(&Operand::Value(value), leaf, selected);
                Self::add_required_source(&mut equation, source, OperandRequirement::READ);
            }
            return equation;
        }
        let Some(Some(definition)) = self.graph.value_definitions.get(value as usize) else {
            equation.invalid = true;
            return equation;
        };
        match (*definition).clone() {
            Rvalue::MakeSlice(slot, _) if path.is_empty() => {
                // Keep the ordinary exact inline-layout and initialized-element checks.
                let Ok(parameter) = self.buffer_parameter(slot) else {
                    equation.invalid = true;
                    return equation;
                };
                equation.seed = Some(match parameter {
                    None => XmlAccessProvenance::Owned,
                    Some(index) => match self.graph.function.param_modes.get(index) {
                        Some(align_ast::ParamMode::ByValue) => XmlAccessProvenance::Owned,
                        Some(align_ast::ParamMode::BorrowMut) => XmlAccessProvenance::Exclusive,
                        Some(align_ast::ParamMode::Borrow) => XmlAccessProvenance::Shared,
                        _ => { equation.invalid = true; XmlAccessProvenance::Mixed }
                    },
                });
            }
            Rvalue::Use(operand) | Rvalue::SubSlice { base: operand, .. } => {
                equation.seed = None;
                equation.dependencies.clear();
                equation.read_dependencies.clear();
                let source = self.buffer_source(&operand, path);
                Self::add_source(&mut equation, source);
            }
            Rvalue::Select { a, b, .. } => {
                equation.seed = None;
                equation.dependencies.clear();
                equation.read_dependencies.clear();
                for operand in [&a, &b] {
                    let source = self.buffer_source(operand, path.clone());
                    Self::add_source(&mut equation, source);
                }
            }
            Rvalue::Load(_) | Rvalue::Field(..) | Rvalue::TupleIndex { .. }
            | Rvalue::MakeTuple { .. } | Rvalue::MakeEnum { .. } | Rvalue::EnumPayload { .. }
            | Rvalue::OptionSome(_) | Rvalue::OptionNone | Rvalue::OptionUnwrap(_)
            | Rvalue::ResultOk(_) | Rvalue::ResultErr(_)
            | Rvalue::ResultUnwrapOk(_) | Rvalue::ResultUnwrapErr(_) => {
                self.buffer_dependencies(&mut equation);
            }
            // A Copy return's lifetime roots do not exclude a static-return alternative.
            // Neither a call nor an unrelated producer may mint writable backing here.
            _ => equation.invalid = true,
        }
        equation
    }

    fn buffer_slot_equation(&mut self, slot: Slot, path: Vec<XmlAccessPathSegment>) -> XmlAccessEquation {
        let selected = self.graph.function.slots.get(slot as usize)
            .and_then(|root| xml_selected_ty(self.graph.program, *root, &path));
        if matches!(selected, Some(Ty::DynArray(_) | Ty::DynStructArray(_, Layout::Aos))) {
            return self.slot_equation(slot, path);
        }
        if !path.is_empty() {
            let mut equation = self.slot_equation(slot, path);
            self.buffer_dependencies(&mut equation);
            return equation;
        }
        let mut equation = XmlAccessEquation::default();
        let Some(ty @ Ty::Slice(_)) = self.graph.function.slots.get(slot as usize).copied() else {
            equation.invalid = true;
            return equation;
        };
        let stores = &self.graph.slot_stores;
        let Some(roots) = stores.roots.get(slot as usize) else {
            equation.invalid = true;
            return equation;
        };
        // A slice slot stores a header. Inline element/field writers cannot initialize it.
        let plain = stores.fields.get(slot as usize).is_some_and(Vec::is_empty)
            && stores.elements.get(slot as usize).is_some_and(Vec::is_empty)
            && stores.element_fields.get(slot as usize).is_some_and(Vec::is_empty)
            && stores.constant_elements.get(slot as usize).is_some_and(Vec::is_empty)
            && stores.producers.get(slot as usize).is_some_and(Vec::is_empty);
        if !plain { equation.invalid = true; }
        let roots = roots.iter().map(|operand| (*operand).clone()).collect::<Vec<_>>();
        let Ok(parameter) = self.buffer_parameter(slot) else {
            equation.invalid = true;
            return equation;
        };
        // Only borrowed slots alias incoming storage in emit_fn. An Out header
        // lives in an alloca and must be grounded by its explicit Arg store.
        if let Some(index) = parameter
            && matches!(self.graph.function.param_modes.get(index), Some(align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut))
            && let Ok(index) = u32::try_from(index)
        {
            let source = self.buffer_source(&Operand::Arg(index), Vec::new());
            Self::add_source(&mut equation, source);
        }
        for operand in roots {
            if xml_operand_base_ty(self.graph.function, &operand) != Some(ty) {
                equation.invalid = true;
                continue;
            }
            let source = self.buffer_source(&operand, Vec::new());
            Self::add_source(&mut equation, source);
        }
        if equation.seed.is_none()
            && equation.dependencies.is_empty()
            && equation.read_dependencies.is_empty()
        {
            equation.invalid = true;
        }
        equation
    }

    fn add_source(equation: &mut XmlAccessEquation, source: XmlAccessSource) {
        match source {
            XmlAccessSource::Seed(access) => {
                equation.seed = merge_xml_access(equation.seed, access);
            }
            XmlAccessSource::Node(node) => equation.dependencies.push(node),
            XmlAccessSource::ReadNode(node) => equation.read_dependencies.push(node),
            XmlAccessSource::Invalid => equation.invalid = true,
        }
    }

    fn add_required_source(
        equation: &mut XmlAccessEquation,
        source: XmlAccessSource,
        requirement: OperandRequirement,
    ) {
        match source {
            XmlAccessSource::Seed(access) => {
                if !requirement.is_satisfied_by(XmlProducerState::Present(access)) {
                    equation.invalid = true;
                }
            }
            XmlAccessSource::Node(node) | XmlAccessSource::ReadNode(node) => {
                equation.checks.push((node, requirement))
            }
            XmlAccessSource::Invalid => equation.invalid = true,
        }
    }

    fn check_source(&mut self, operand: &Operand, expected: Ty) -> XmlAccessSource {
        if xml_operand_base_ty(self.graph.function, operand) != Some(expected) {
            return XmlAccessSource::Invalid;
        }
        match operand {
            Operand::Value(value) => self.queue(XmlAccessNode::Value(*value, Vec::new())),
            Operand::Arg(index) => {
                XmlAccessSource::Seed(xml_argument_access(self.graph.function, *index))
            }
            Operand::Const(_) => XmlAccessSource::Seed(XmlAccessProvenance::Owned),
            Operand::BorrowedCleanupArg(index)
                if self
                    .graph
                    .function
                    .borrow_mut_cleanup_slots
                    .get(*index as usize)
                    .is_some_and(Option::is_some) =>
            {
                XmlAccessSource::Seed(XmlAccessProvenance::Shared)
            }
            Operand::BorrowedPlace(_)
            | Operand::BorrowedElementPlace(_)
            | Operand::BorrowedFixedElementPlace(_)
            | Operand::BorrowedCleanupArg(_) => XmlAccessSource::Invalid,
        }
    }

    fn add_operand(
        &mut self,
        equation: &mut XmlAccessEquation,
        operand: &Operand,
        expected: Ty,
        path: Vec<XmlAccessPathSegment>,
    ) {
        // A borrowed descriptor is a read-only place, not an SSA value.  Aggregate extraction
        // (enum/result/option payloads and struct fields) still needs to follow its exact storage
        // path; routing it through `source` would reject every nested Copy read as an invalid
        // transfer.  `read_source` retains the descriptor's cleanup/storage dependency while
        // keeping ownership authority out of the produced value.
        let source = if matches!(operand, Operand::BorrowedPlace(_)) {
            self.read_source(equation, operand, expected, path)
        } else {
            self.source(operand, expected, path)
        };
        Self::add_source(equation, source);
    }

    fn check_operand(
        &mut self,
        equation: &mut XmlAccessEquation,
        operand: &Operand,
        expected: Ty,
    ) {
        // Scalar and predicate operations read borrowed descriptors without transferring the
        // underlying owner. Keep the ordinary source path for SSA/argument values, but authenticate
        // a borrowed place through its storage projection and cleanup flag.
        let source = if matches!(operand, Operand::BorrowedPlace(_)) {
            self.read_source(equation, operand, expected, Vec::new())
        } else {
            self.check_source(operand, expected)
        };
        Self::add_required_source(equation, source, OperandRequirement::READ);
    }

    /// Authenticate a read without admitting borrowed descriptors as transferable SSA values.
    /// Selected storage remains a dependency, including later stores and its cleanup flag.
    fn read_source(
        &mut self,
        equation: &mut XmlAccessEquation,
        operand: &Operand,
        expected: Ty,
        path: Vec<XmlAccessPathSegment>,
    ) -> XmlAccessSource {
        let Operand::BorrowedPlace(place) = operand else {
            return self.source(operand, expected, path);
        };
        let projection = self.graph.function.slots.get(place.slot as usize)
            .and_then(|root| xml_borrowed_path(self.graph.program, *root, &place.path));
        let Some((stored, mut storage_path)) = projection else {
            return XmlAccessSource::Invalid;
        };
        let selected = xml_selected_ty(self.graph.program, stored, &path);
        if (stored != place.ty && !xml_borrowed_place_ty_is_view_retype(stored, place.ty))
            || xml_selected_ty(self.graph.program, place.ty, &path) != Some(expected)
            || !selected.is_some_and(|actual| {
                xml_callable_flow_matches(self.graph.program, actual, expected)
                    || xml_ty_is_view_retype(actual, expected)
            })
            || place.cleanup.is_some_and(|cleanup| {
                self.graph.function.slots.get(cleanup as usize) != Some(&Ty::Bool)
            })
        {
            return XmlAccessSource::Invalid;
        }
        if let Some(cleanup) = place.cleanup {
            let cleanup = self.queue(XmlAccessNode::Slot(cleanup, Vec::new()));
            Self::add_required_source(equation, cleanup, OperandRequirement::READ);
        }
        storage_path.extend(path);
        let node = XmlAccessNode::Slot(place.slot, storage_path);
        self.queue(node.clone());
        XmlAccessSource::ReadNode(node)
    }

    fn add_read_operand(
        &mut self,
        equation: &mut XmlAccessEquation,
        operand: &Operand,
        expected: Ty,
        path: Vec<XmlAccessPathSegment>,
    ) {
        let source = self.read_source(equation, operand, expected, path);
        Self::add_source(equation, source);
    }

    fn check_read_operand(
        &mut self,
        equation: &mut XmlAccessEquation,
        operand: &Operand,
        expected: Ty,
    ) {
        let source = self.read_source(equation, operand, expected, Vec::new());
        Self::add_required_source(equation, source, OperandRequirement::READ);
    }

    fn check_whole_read_operand(
        &mut self,
        equation: &mut XmlAccessEquation,
        operand: &Operand,
        expected: Ty,
    ) {
        let Some(leaves) = xml_owned_leaf_paths(self.graph.program, expected) else {
            equation.invalid = true;
            return;
        };
        if leaves.is_empty() {
            self.check_read_operand(equation, operand, expected);
        } else {
            for (selected, path) in leaves {
                let source = self.read_source(equation, operand, selected, path);
                Self::add_required_source(equation, source, OperandRequirement::READ);
            }
        }
    }

    fn check_whole_operand(
        &mut self,
        equation: &mut XmlAccessEquation,
        operand: &Operand,
        expected: Ty,
    ) {
        let Some(leaves) = xml_owned_leaf_paths(self.graph.program, expected) else {
            equation.invalid = true;
            return;
        };
        if leaves.is_empty() {
            self.check_operand(equation, operand, expected);
            return;
        }
        for (selected, path) in leaves {
            let source = self.source(operand, selected, path);
            Self::add_required_source(equation, source, OperandRequirement::READ);
        }
    }

    fn check_template_piece(
        &mut self,
        equation: &mut XmlAccessEquation,
        piece: &crate::TemplatePiece,
    ) -> bool {
        let operand_ty = |operand: &Operand| xml_operand_base_ty(self.graph.function, operand);
        if !template_piece_is_type_safe(self.graph.program, self.graph.function, piece) {
            return false;
        }
        let operand = match piece {
            crate::TemplatePiece::Static(_) | crate::TemplatePiece::PopComma => None,
            crate::TemplatePiece::IntHole(operand)
            | crate::TemplatePiece::StrHole(operand)
            | crate::TemplatePiece::JsonStrHole(operand)
            | crate::TemplatePiece::BoolHole(operand)
            | crate::TemplatePiece::CharHole(operand)
            | crate::TemplatePiece::FloatHole(operand)
            | crate::TemplatePiece::OptionField { opt: operand, .. }
            | crate::TemplatePiece::OptionStructField { opt: operand, .. }
            | crate::TemplatePiece::StructArrayField { array: operand, .. }
            | crate::TemplatePiece::ScalarArrayField { array: operand, .. }
            | crate::TemplatePiece::UnionValue { value: operand, .. }
            | crate::TemplatePiece::OwnedJsonObject { value: operand, .. } => Some(operand),
        };
        if let Some(operand) = operand {
            let Some(expected) = operand_ty(operand) else {
                return false;
            };
            self.check_operand(equation, operand, expected);
        }
        true
    }

    fn load_slot(&self, operand: &Operand) -> Option<Slot> {
        match operand {
            Operand::Arg(index) => self.graph.function.params.get(*index as usize).copied(),
            Operand::Value(value) => match self.graph.value_definitions.get(*value as usize)? {
                Some(Rvalue::Load(slot)) => Some(*slot),
                _ => None,
            },
            _ => None,
        }
    }

    fn batch_plan_guard_matches(&self, result: ValueId, callee: ValueId, plan: &Operand) -> bool {
        let Some(plan_slot) = self.load_slot(plan) else { return false; };
        let function = self.graph.function;
        let Some(block) = function.blocks.iter().find(|block| block.stmts.iter().any(
            |statement| matches!(statement, Stmt::Let(value, _) if *value == result)
        )) else { return false; };
        let position = |block: &Block, value: ValueId| block.stmts.iter().position(
            |statement| matches!(statement, Stmt::Let(id, _) if *id == value)
        );
        let Operand::Value(plan_value) = plan else { return false; };
        let (Some(plan_position), Some(pointer_position), Some(call_position)) =
            (position(block, *plan_value), position(block, callee), position(block, result))
        else { return false; };
        if !(plan_position < pointer_position && pointer_position < call_position) { return false; }
        if block.id == function.entry || block.stmts.iter().any(|statement| {
            matches!(statement, Stmt::Store(slot, _) if *slot == plan_slot)
        }) { return false; }
        let mut predecessors = function.blocks.iter().filter(|candidate| match &candidate.term {
            Term::Goto(target) => *target == block.id,
            Term::Branch(_, yes, no) => *yes == block.id || *no == block.id,
            _ => false,
        });
        let Some(predecessor) = predecessors.next() else { return false; };
        if predecessors.next().is_some() { return false; }
        let Term::Branch(Operand::Value(condition), yes, no) = &predecessor.term else { return false; };
        if *yes != block.id || yes == no { return false; }
        let Some(Some(Rvalue::Call(DirectCall::Program(target), arguments))) =
            self.graph.value_definitions.get(*condition as usize)
        else { return false; };
        let Some(condition_position) = predecessor.stmts.iter().position(
            |statement| matches!(statement, Stmt::Let(value, _) if value == condition)
        ) else { return false; };
        let Some(Operand::Value(guard_plan)) = arguments.first() else { return false; };
        let Some(guard_plan_position) = position(predecessor, *guard_plan) else { return false; };
        target.as_str() == "pkg.db.internal.resource$batch_plan_valid"
            && arguments.len() == 1
            && self.load_slot(&arguments[0]) == Some(plan_slot)
            && guard_plan_position < condition_position
            && !predecessor.stmts[guard_plan_position..].iter().any(|statement| {
                matches!(statement, Stmt::Store(slot, _) if *slot == plan_slot)
            })
            && function.blocks.iter().find(|candidate| candidate.id == *no).is_some_and(|failure| {
                matches!(failure.term, Term::Unreachable)
                    && failure.stmts.iter().any(|statement| matches!(statement,
                        Stmt::Let(_, Rvalue::Call(DirectCall::Runtime(RuntimeKey::ProcessAbort), args)) if args.is_empty()
                    ))
            })
    }

    fn extraction_guard_matches(&self, result: ValueId, operand: &Operand, selected: &XmlAccessPathSegment) -> bool {
        let (count, selected_tag) = match selected {
            XmlAccessPathSegment::OptionSome | XmlAccessPathSegment::ResultOk => (2, 1),
            XmlAccessPathSegment::ResultErr => (2, 0),
            XmlAccessPathSegment::EnumPayload { enum_id, variant, .. } => {
                let Some(count) = self.graph.program.enums.get(*enum_id as usize)
                    .and_then(|definition| u32::try_from(definition.variants.len()).ok())
                else { return false; };
                (count, *variant)
            }
            _ => return false,
        };
        let function = self.graph.function;
        let Some(block) = function.blocks.iter().find(|block| block.stmts.iter().any(
            |statement| matches!(statement, Stmt::Let(value, _) if *value == result)
        )) else { return false; };
        let same_operand = |candidate: &Operand| matches!((candidate, operand),
            (Operand::Value(a), Operand::Value(b)) | (Operand::Arg(a), Operand::Arg(b)) if a == b);
        let mut pending = vec![(block.id, (0..count).collect::<Vec<_>>() )];
        let mut visited = HashSet::new();
        let mut proved = false;
        while let Some((block, allowed)) = pending.pop() {
            if allowed.iter().all(|tag| *tag == selected_tag) {
                proved = true;
                continue;
            }
            if block == function.entry { return false; }
            if !visited.insert((block, allowed.clone())) { continue; }
            let mut has_predecessor = false;
            for predecessor in &function.blocks {
                let (condition, yes, no) = match &predecessor.term {
                    Term::Goto(target) if *target == block => (None, block, block),
                    Term::Branch(condition, yes, no) if *yes == block || *no == block => (Some(condition), *yes, *no),
                    _ => continue,
                };
                has_predecessor = true;
                let mut allowed = allowed.clone();
                if yes != no
                    && let Some(Operand::Value(condition)) = condition
                    && function.value_tys.get(*condition as usize) == Some(&Ty::Bool)
                    && predecessor.stmts.iter().any(|statement| matches!(statement, Stmt::Let(value, _) if value == condition))
                    && let Some(Some(predicate)) = self.graph.value_definitions.get(*condition as usize)
                {
                    let tested = match (predicate, selected) {
                        (Rvalue::OptionIsSome(value), XmlAccessPathSegment::OptionSome) if same_operand(value) => Some(1),
                        (Rvalue::ResultIsOk(value), XmlAccessPathSegment::ResultOk | XmlAccessPathSegment::ResultErr) if same_operand(value) => Some(1),
                        (Rvalue::EnumTagEq { enum_id, scrutinee, variant }, XmlAccessPathSegment::EnumPayload { enum_id: expected, .. })
                            if enum_id == expected && same_operand(scrutinee) && *variant < count => Some(*variant),
                        _ => None,
                    };
                    if let Some(tested) = tested {
                        allowed.retain(|tag| (*tag == tested) == (yes == block));
                    }
                }
                pending.push((predecessor.id, allowed));
            }
            if !has_predecessor { return false; }
        }
        proved
    }

    fn query_descriptor_row_matches(&self, descriptor: u32, row: u32) -> bool {
        let program = self.graph.program;
        let Some(definition) = program.structs.get(descriptor as usize) else { return false; };
        let Some(arguments) = definition.name.strip_prefix("pkg.db$query$S") else { return false; };
        let Some((length_text, rest)) = arguments.split_once('_') else { return false; };
        let Ok(length) = length_text.parse::<usize>() else { return false; };
        if length.to_string() != length_text {
            return false;
        }
        let Some(params) = rest.get(..length) else { return false; };
        let Some(row_name) = rest.get(length..).and_then(|tail| tail.strip_prefix('$')) else { return false; };
        let matches_name = |definition: &StructDef, encoded: &str| {
            let direct = format!("S{}_{}", definition.name.len(), definition.name);
            let reconstructed = definition.name.replace(['.', '$'], "_");
            encoded == direct || encoded == format!("S{}_{}", reconstructed.len(), reconstructed)
        };
        program.structs.iter().any(|definition| matches_name(definition, &format!("S{length}_{params}")))
            && program.structs.get(row as usize).is_some_and(|definition| matches_name(definition, row_name))
    }

    fn same_slot_observation(&self, first: &Operand, second: &Operand) -> bool {
        let Some(slot) = self.load_slot(first) else { return false; };
        if self.load_slot(second) != Some(slot) { return false; }
        let function = self.graph.function;
        if matches!(first, Operand::Arg(_)) || matches!(second, Operand::Arg(_)) {
            return function.blocks.iter().flat_map(|block| &block.stmts).all(|statement| {
                !matches!(statement, Stmt::Store(target, source) if *target == slot
                    && !matches!(source, Operand::Arg(index) if function.params.get(*index as usize) == Some(&slot)))
            });
        }
        let (Operand::Value(first), Operand::Value(second)) = (first, second) else { return false; };
        function.blocks.iter().any(|block| {
            let position = |value: ValueId| block.stmts.iter().position(
                |statement| matches!(statement, Stmt::Let(id, _) if *id == value)
            );
            let (Some(first), Some(second)) = (position(*first), position(*second)) else { return false; };
            !block.stmts[first.min(second)..=first.max(second)].iter().any(
                |statement| matches!(statement, Stmt::Store(target, _) if *target == slot)
            )
        })
    }

    // These are the existing checked-HIR native view bridges, not bodyless Align
    // certificates. Their unsafe native preconditions remain caller-owned; they may
    // publish shared Copy views, never an owned string, XML handle, or callable.
    fn native_view_call_matches(&self, result: ValueId, rvalue: &Rvalue) -> bool {
        let Rvalue::RawCall { callee, args, param_tys, ret_ty, signature } = rvalue else { return false; };
        if args.len() != param_tys.len()
            || signature.param_modes != vec![align_ast::ParamMode::ByValue; args.len()]
            || signature.return_cleanup != hir::ReturnCleanupAbi::None
            || args.iter().zip(param_tys).any(|(argument, expected)| {
                xml_operand_base_ty(self.graph.function, argument) != Some(*expected)
            })
            || align_sema::ty_is_move(*ret_ty, &self.graph.program.structs,
                &self.graph.program.tuples, &self.graph.program.enums, &self.graph.program.tagged_types)
            || !xml_owned_leaf_paths(self.graph.program, *ret_ty)
                .is_some_and(|leaves| leaves.iter().all(|(ty, _)| *ty == Ty::Str))
        { return false; }
        let Operand::Value(callee) = callee else { return false; };
        let Some(Some(Rvalue::RawPointerLoad { ptr, offset: Operand::Const(Const::Int(offset, offset_ty)) })) =
            self.graph.value_definitions.get(*callee as usize)
        else { return false; };
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        if *offset_ty != i64_ty || xml_operand_base_ty(self.graph.function, ptr) != Some(Ty::Raw) {
            return false;
        }
        let rooted = signature.return_borrow == hir::ReturnBorrowSummary::Roots { params: vec![1], captures: vec![] }
            && signature.return_region == hir::ReturnRegionSummary::Roots { params: vec![1], captures: vec![] };
        let unrooted = signature.return_borrow == hir::ReturnBorrowSummary::None
            && signature.return_region == hir::ReturnRegionSummary::None;
        if let Some(Ty::ResourceRef(resource)) = param_tys.get(1) {
            let (row, soa) = match ret_ty {
                Ty::Struct(row) => (*row, false),
                Ty::Soa(row) => (*row, true),
                _ => return false,
            };
            let row_borrows = soa || align_sema::ty_may_borrow(*ret_ty, &self.graph.program.structs,
                &self.graph.program.tuples, &self.graph.program.enums, &self.graph.program.tagged_types);
            if !(if row_borrows { rooted } else { unrooted }) { return false; }
            if db_resource_matches_row(self.graph.program, *resource, row, "batch") {
                let expected = if soa { vec![Ty::Raw, Ty::ResourceRef(*resource)] }
                    else { vec![Ty::Raw, Ty::ResourceRef(*resource), i64_ty] };
                return *param_tys == expected && *offset == if soa { 48 } else { 40 }
                    && self.batch_plan_guard_matches(result, *callee, ptr);
            }
            if !soa && *offset == 48 && param_tys == &[Ty::Raw, Ty::ResourceRef(*resource)]
                && db_resource_matches_row(self.graph.program, *resource, row, "rows")
                && let Operand::Value(pointer) = ptr
                && let Some(Some(Rvalue::ResourceRaw { reference, resource: owner })) = self.graph.value_definitions.get(*pointer as usize)
            {
                return *owner == *resource && self.same_slot_observation(reference, &args[1]);
            }
            return false;
        }
        let Operand::Value(pointer) = ptr else { return false; };
        let Some(Some(Rvalue::Field(slot, fields))) = self.graph.value_definitions.get(*pointer as usize) else { return false; };
        let Some(Ty::Struct(descriptor)) = self.graph.function.slots.get(*slot as usize) else { return false; };
        let descriptor_matches = fields.as_slice() == [0] && self.graph.program.structs.get(*descriptor as usize)
            .is_some_and(|definition| definition.name.starts_with("pkg.db$query$")
                && align_sema::static_descriptor_struct_is_valid(definition));
        descriptor_matches && unrooted && match *offset {
            88 => param_tys == &[Ty::Raw] && matches!(ret_ty, Ty::Struct(row)
                if self.query_descriptor_row_matches(*descriptor, *row)),
            96 => param_tys == &[Ty::Int(IntTy { bits: 8, signed: false }), Ty::Int(IntTy { bits: 8, signed: false }), i64_ty]
                && matches!(ret_ty, Ty::Option(Scalar::Struct(id)) if QueryMetaTypes::resolve(self.graph.program)
                    .is_ok_and(|types| types.row == *id)),
            _ => false,
        }
    }

    fn call_roots(
        borrow: &hir::ReturnBorrowSummary,
        region: &hir::ReturnRegionSummary,
    ) -> (Vec<u32>, Vec<u32>) {
        let mut roots = Vec::new();
        let mut captured = Vec::new();
        if let hir::ReturnBorrowSummary::Roots { params, captures } = borrow {
            roots.extend(params.iter().copied());
            captured.extend(captures.iter().copied());
        }
        if let hir::ReturnRegionSummary::Roots { params, captures } = region {
            roots.extend(params.iter().copied());
            captured.extend(captures.iter().copied());
        }
        roots.sort_unstable();
        roots.dedup();
        captured.sort_unstable();
        captured.dedup();
        (roots, captured)
    }

    /// Follow exactly the ordinary callable producer edges, with the same shape and sibling
    /// checks, then select returned captures from each producer's own validated environment.
    /// Keeping the projection on the worklist closes copied, stored, and control-joined callees
    /// without recursively walking cyclic callable/slot graphs.
    fn capture_equation(
        &mut self,
        node: XmlAccessNode,
    ) -> XmlAccessEquation {
        let (selected, captured, mut equation) = match &node {
            XmlAccessNode::Value(value, path) => {
                let selected = self.graph.function.value_tys.get(*value as usize)
                    .and_then(|ty| xml_selected_ty(self.graph.program, *ty, path));
                let captured = if path.is_empty() {
                    match self.graph.value_definitions.get(*value as usize) {
                        Some(Some(Rvalue::Closure { captures, capture_tys, signature, .. })) => {
                            let (_, roots) = Self::call_roots(&signature.return_borrow, &signature.return_region);
                            Some(roots.into_iter().map(|capture| {
                                captures.get(capture as usize).zip(capture_tys.get(capture as usize))
                                    .map(|(operand, ty)| (operand.clone(), *ty))
                            }).collect::<Option<Vec<_>>>())
                        }
                        Some(Some(Rvalue::FnAddr { .. })) => Some(Some(Vec::new())),
                        _ => None,
                    }
                } else { None };
                (selected, captured, self.value_equation(*value, path.clone()))
            }
            XmlAccessNode::Slot(slot, path) => {
                let selected = self.graph.function.slots.get(*slot as usize)
                    .and_then(|ty| xml_selected_ty(self.graph.program, *ty, path));
                (selected, None, self.slot_equation(*slot, path.clone()))
            }
            XmlAccessNode::CaptureValue(..) | XmlAccessNode::CaptureSlot(..)
                | XmlAccessNode::BufferValue(..) | XmlAccessNode::BufferSlot(..) => {
                return XmlAccessEquation { invalid: true, ..XmlAccessEquation::default() };
            }
        };
        if !matches!(selected, Some(Ty::Fn(_))) {
            equation.invalid = true;
            return equation;
        }
        let callable = self.queue(node);
        Self::add_required_source(&mut equation, callable, OperandRequirement {
            read: true,
            callable: true,
            ..OperandRequirement::default()
        });
        if let Some(captured) = captured {
            equation.seed = None;
            equation.dependencies.clear();
            equation.read_dependencies.clear();
            if let Some(captured) = captured {
                if captured.is_empty() { equation.seed = Some(XmlAccessProvenance::Owned); }
                for (operand, ty) in captured {
                    self.add_operand(&mut equation, &operand, ty, Vec::new());
                }
            } else {
                equation.invalid = true;
            }
            return equation;
        }
        // An opaque parameter capability does not authenticate an environment.
        // Every reaching producer must validate its own returned captures.
        if equation.seed.is_some()
            || (equation.dependencies.is_empty()
                && equation.read_dependencies.is_empty()
                && !equation.absent)
        {
            equation.invalid = true;
        }
        equation.seed = None;
        for dependency in std::mem::take(&mut equation.dependencies) {
            let source = match dependency {
                XmlAccessNode::Value(value, path) => {
                    self.queue(XmlAccessNode::CaptureValue(value, path))
                }
                XmlAccessNode::Slot(slot, path) => {
                    self.queue(XmlAccessNode::CaptureSlot(slot, path))
                }
                XmlAccessNode::CaptureValue(..) | XmlAccessNode::CaptureSlot(..)
                | XmlAccessNode::BufferValue(..) | XmlAccessNode::BufferSlot(..) => {
                    XmlAccessSource::Invalid
                }
            };
            Self::add_source(&mut equation, source);
        }
        for dependency in std::mem::take(&mut equation.read_dependencies) {
            let source = match dependency {
                XmlAccessNode::Value(value, path) => {
                    let node = XmlAccessNode::CaptureValue(value, path);
                    self.queue(node.clone());
                    XmlAccessSource::ReadNode(node)
                }
                XmlAccessNode::Slot(slot, path) => {
                    let node = XmlAccessNode::CaptureSlot(slot, path);
                    self.queue(node.clone());
                    XmlAccessSource::ReadNode(node)
                }
                XmlAccessNode::CaptureValue(..)
                | XmlAccessNode::CaptureSlot(..)
                | XmlAccessNode::BufferValue(..)
                | XmlAccessNode::BufferSlot(..) => XmlAccessSource::Invalid,
            };
            Self::add_source(&mut equation, source);
        }
        equation
    }

    /// Copy projections are value arguments, but their canonical descriptor is not an SSA value.
    /// Keep their complete storage proof in this worklist, including writes after parameter entry.
    fn check_copy_place(
        &mut self,
        equation: &mut XmlAccessEquation,
        place: &crate::BorrowedPlace,
        expected: Ty,
    ) -> Vec<XmlAccessNode> {
        let projection = self.graph.function.slots.get(place.slot as usize)
            .and_then(|root| xml_borrowed_path(self.graph.program, *root, &place.path));
        let Some((stored, storage_path)) = projection else {
            equation.invalid = true;
            return Vec::new();
        };
        let view_retype = xml_borrowed_place_ty_is_view_retype(stored, expected);
        if place.cleanup.is_some()
            || place.ty != expected
            || (stored != expected && !view_retype)
            || align_sema::ty_is_move(
                expected,
                &self.graph.program.structs,
                &self.graph.program.tuples,
                &self.graph.program.enums,
                &self.graph.program.tagged_types,
            )
        {
            equation.invalid = true;
            return Vec::new();
        }
        let Some(leaves) = xml_owned_leaf_paths(self.graph.program, stored) else {
            equation.invalid = true;
            return Vec::new();
        };
        let paths = if leaves.is_empty() {
            vec![Vec::new()]
        } else {
            leaves.into_iter().map(|(_, path)| path).collect()
        };
        let mut sources = Vec::with_capacity(paths.len());
        for path in paths {
            let mut selected = storage_path.clone();
            selected.extend(path);
            let node = XmlAccessNode::Slot(place.slot, selected);
            let source = self.queue(node.clone());
            Self::add_required_source(equation, source, OperandRequirement::READ);
            sources.push(node);
        }
        sources
    }

    fn add_call_result(
        &mut self,
        equation: &mut XmlAccessEquation,
        call: XmlCallResult<'_>,
    ) {
        let XmlCallResult {
            result,
            result_ty,
            selected_ty,
            args,
            callee,
            facts,
            cleanup,
            modes_match,
        } = call;
        let argument_types_match = args.len() == facts.params.len()
            && facts.modes.len() == facts.params.len()
            && args
                .iter()
                .zip(&facts.params)
                .all(|(operand, expected)| {
                    xml_operand_base_ty(self.graph.function, operand).is_some_and(|actual|
                        actual == *expected || xml_callable_flow_matches(self.graph.program, actual, *expected))
                });
        let cleanup_matches = match (cleanup, facts.cleanup) {
            (None, hir::ReturnCleanupAbi::None) => true,
            (Some(cleanup), hir::ReturnCleanupAbi::DynamicBit) => {
                cleanup != result
                    && self.graph.function.value_tys.get(cleanup as usize) == Some(&Ty::Bool)
                    && self.graph.primary_definitions.get(cleanup as usize) == Some(&0)
                    && self.graph.auxiliary_definitions.get(cleanup as usize) == Some(&1)
            }
            _ => false,
        };
        let (roots, captures) = Self::call_roots(&facts.borrow, &facts.region);
        if !argument_types_match
            || !modes_match
            || facts.ret != result_ty
            || !cleanup_matches
            || roots.iter().any(|root| *root as usize >= args.len())
            || (!captures.is_empty() && callee.is_none())
        {
            equation.invalid = true;
            return;
        }
        let mut copy_place_sources = HashMap::new();
        for (index, ((argument, expected), mode)) in args.iter().zip(&facts.params).zip(&facts.modes).enumerate() {
            if *mode == align_ast::ParamMode::Out
                && xml_owned_leaf_paths(self.graph.program, *expected).is_none_or(|leaves| !leaves.is_empty())
            {
                let source = self.buffer_source(argument, Vec::new());
                Self::add_required_source(equation, source, OperandRequirement {
                    write: true, exclusive: true, ..OperandRequirement::default()
                });
                continue;
            }
            if let (Operand::BorrowedPlace(place), align_ast::ParamMode::ByValue) = (argument, mode) {
                let sources = self.check_copy_place(equation, place, *expected);
                copy_place_sources.insert(index, sources);
                continue;
            }
            let canonical_borrow = matches!(
                argument,
                Operand::BorrowedPlace(_)
                    | Operand::BorrowedElementPlace(_)
                    | Operand::BorrowedFixedElementPlace(_)
            ) && matches!(
                mode,
                align_ast::ParamMode::Borrow
                    | align_ast::ParamMode::BorrowMut
                    | align_ast::ParamMode::Out
            );
            if !canonical_borrow {
                self.check_whole_operand(equation, argument, *expected);
            }
        }
        if callee.is_some() && selected_ty == Ty::Str
            && xml_shared_string_result(self.graph.program, facts.ret)
        {
            // An empty function-value summary can mean an opaque target. Even a
            // populated summary may contain an empty alternative. Authenticate the
            // invocation above, then publish only shared access without inspecting an
            // opaque capture environment. Writable backing has its own buffer proof.
            equation.seed = merge_xml_access(equation.seed, XmlAccessProvenance::Shared);
            return;
        }
        let mut captured_sources = Vec::new();
        if !captures.is_empty() {
            let source = match callee {
                Some(Operand::Value(value)) => {
                    self.queue(XmlAccessNode::CaptureValue(*value, Vec::new()))
                }
                _ => XmlAccessSource::Invalid,
            };
            Self::add_required_source(equation, source.clone(), OperandRequirement::READ);
            captured_sources.push(source);
        }
        if matches!(selected_ty, Ty::String | Ty::XmlReader) {
            if facts.cleanup != hir::ReturnCleanupAbi::DynamicBit {
                equation.invalid = true;
            } else {
                equation.seed = merge_xml_access(equation.seed, XmlAccessProvenance::Owned);
            }
            return;
        }
        // A returned Move carrier owns its shell even when that shell retains a region
        // dependency on an argument (for example, a DB cursor borrowing its connection).
        // Selected Copy views still follow their input roots below; lifetime dependence
        // must not downgrade the new owner's transfer or exclusive-borrow authority.
        if (roots.is_empty() && captured_sources.is_empty()) || align_sema::ty_is_move(
            selected_ty,
            &self.graph.program.structs,
            &self.graph.program.tuples,
            &self.graph.program.enums,
            &self.graph.program.tagged_types,
        ) {
            equation.seed = merge_xml_access(equation.seed, XmlAccessProvenance::Owned);
            return;
        }
        for source in captured_sources {
            Self::add_source(equation, source);
        }
        for root in roots {
            let Some((operand, expected)) = args.get(root as usize).zip(facts.params.get(root as usize)) else {
                equation.invalid = true;
                return;
            };
            match facts.modes.get(root as usize) {
                Some(align_ast::ParamMode::Borrow) => Self::add_source(
                    equation,
                    XmlAccessSource::Seed(XmlAccessProvenance::Shared),
                ),
                Some(align_ast::ParamMode::BorrowMut) => Self::add_source(
                    equation,
                    XmlAccessSource::Seed(XmlAccessProvenance::Exclusive),
                ),
                Some(align_ast::ParamMode::Out) => Self::add_source(
                    equation,
                    XmlAccessSource::Seed(XmlAccessProvenance::Unreadable),
                ),
                Some(align_ast::ParamMode::ByValue) => {
                    if matches!(operand, Operand::BorrowedPlace(_)) {
                        // A rooted view must have an independently founded source, exactly like
                        // an ordinary ByValue SSA argument. A check-only edge plus a new seed
                        // would let an uninitialized call/slot cycle authenticate itself.
                        if let Some(sources) = copy_place_sources.get(&(root as usize)) {
                            equation.dependencies.extend(sources.iter().cloned());
                        } else {
                            equation.invalid = true;
                        }
                    } else {
                        self.add_operand(equation, operand, *expected, Vec::new());
                    }
                }
                None => equation.invalid = true,
            }
        }
    }

    fn same_operand(left: &Operand, right: &Operand) -> bool {
        matches!((left, right),
            (Operand::Value(a), Operand::Value(b)) | (Operand::Arg(a), Operand::Arg(b)) if a == b)
            || matches!((left, right),
                (Operand::Const(Const::Int(a, at)), Operand::Const(Const::Int(b, bt)))
                    if a == b && at == bt)
    }

    /// Authenticate the allocation count behind a caller-owned runtime output buffer. Merely
    /// proving that the pointer has the expected scalar type does not prove that the runtime may
    /// write its declared bound through it.
    fn heap_buffer_capacity_matches(&self, output: &Operand, bound: &Operand) -> bool {
        let Operand::Value(output) = output else { return false; };
        matches!(
            self.graph.value_definitions.get(*output as usize),
            Some(Some(Rvalue::HeapAllocBuf { count, .. }))
                if Self::same_operand(count, bound)
        )
    }

    fn heap_buffer_capacity_matches_slice_len(
        &self,
        output: &Operand,
        source: &Operand,
    ) -> bool {
        let Operand::Value(output) = output else { return false; };
        let Some(Some(Rvalue::HeapAllocBuf { count: Operand::Value(count), .. })) =
            self.graph.value_definitions.get(*output as usize)
        else { return false; };
        matches!(
            self.graph.value_definitions.get(*count as usize),
            Some(Some(Rvalue::SliceLen(actual))) if self.same_runtime_row_count(actual, source)
        )
    }

    fn same_runtime_row_count(&self, left: &Operand, right: &Operand) -> bool {
        if Self::same_operand(left, right) {
            return true;
        }
        let (Operand::Value(left), Operand::Value(right)) = (left, right) else {
            return false;
        };
        let dictionary_field = |value: ValueId| match self
            .graph
            .value_definitions
            .get(value as usize)
        {
            Some(Some(Rvalue::DictField { base, idx })) => Some((*base, *idx)),
            _ => None,
        };
        matches!(
            (dictionary_field(*left), dictionary_field(*right)),
            (Some((left_base, left_field)), Some((right_base, right_field)))
                if left_base == right_base
                    && matches!((left_field, right_field), (0, 1) | (1, 0))
        )
    }

    fn heap_buffer_capacity_matches_slot_len(&self, output: &Operand, source: Slot) -> bool {
        let Operand::Value(output) = output else { return false; };
        let Some(Some(Rvalue::HeapAllocBuf { count: Operand::Value(count), .. })) =
            self.graph.value_definitions.get(*output as usize)
        else { return false; };
        let Some(Some(Rvalue::SliceLen(Operand::Value(array)))) =
            self.graph.value_definitions.get(*count as usize)
        else { return false; };
        matches!(
            self.graph.value_definitions.get(*array as usize),
            Some(Some(Rvalue::Load(actual))) if *actual == source
        )
    }

    fn add_column_buffer_writer(
        &mut self,
        equation: &mut XmlAccessEquation,
        writer: (ValueId, &Rvalue),
        output: (&Operand, &Operand, Ty, &[XmlAccessPathSegment]),
    ) -> bool {
        let (count, runtime) = writer;
        let (ptr, len, selected_ty, remaining) = output;
        let (keys, vals, out_keys, out_vals, op, key_scalar) = match runtime {
            Rvalue::GroupAgg { keys, vals, out_keys, out_vals, op } =>
                (keys, vals, out_keys, out_vals, *op,
                    Scalar::Int(IntTy { bits: 64, signed: true })),
            Rvalue::GroupAggStrCols { keys, vals, out_keys, out_vals, op } =>
                (keys, vals, out_keys, out_vals, *op, Scalar::Str),
            _ => return false,
        };
        let same = Self::same_operand;
        if !same(ptr, out_keys) && !same(ptr, out_vals) {
            return false;
        }
        let integer_scalar = Scalar::Int(IntTy { bits: 64, signed: true });
        let i64_ty = scalar_to_ty(integer_scalar);
        let keys_ty = xml_operand_base_ty(self.graph.function, keys);
        let vals_ty = xml_operand_base_ty(self.graph.function, vals);
        let valid_column = |ty, scalar| {
            matches!(ty, Some(Ty::Slice(actual) | Ty::DynArray(actual)) if actual == scalar)
        };
        let count_only = matches!(op, hir::GroupOp::Count);
        if self.graph.function.value_tys.get(count as usize) != Some(&i64_ty)
            || !same(len, &Operand::Value(count))
            || !valid_column(keys_ty, key_scalar)
            || (!count_only && !valid_column(vals_ty, integer_scalar))
            || xml_operand_base_ty(self.graph.function, out_keys) != Some(Ty::Box(key_scalar))
            || xml_operand_base_ty(self.graph.function, out_vals) != Some(Ty::Box(integer_scalar))
            || !self.heap_buffer_capacity_matches_slice_len(out_keys, keys)
            || !self.heap_buffer_capacity_matches_slice_len(out_vals, keys)
            || same(out_keys, out_vals)
            || selected_ty != if same(out_keys, ptr) { scalar_to_ty(key_scalar) } else { i64_ty }
            || !remaining.is_empty()
        {
            equation.invalid = true;
            return true;
        }
        if let Some(keys_ty) = keys_ty {
            self.check_whole_operand(equation, keys, keys_ty);
            let source = self.source(keys, scalar_to_ty(key_scalar),
                vec![XmlAccessPathSegment::Element]);
            Self::add_required_source(equation, source, OperandRequirement::READ);
        }
        if !count_only && let Some(vals_ty) = vals_ty {
            self.check_whole_operand(equation, vals, vals_ty);
            let source = self.source(vals, i64_ty, vec![XmlAccessPathSegment::Element]);
            Self::add_required_source(equation, source, OperandRequirement::READ);
        }
        self.check_operand(equation, out_keys, Ty::Box(key_scalar));
        self.check_operand(equation, out_vals, Ty::Box(integer_scalar));
        if same(out_keys, ptr) {
            self.add_operand(equation, keys, selected_ty, vec![XmlAccessPathSegment::Element]);
        } else {
            Self::add_source(equation, XmlAccessSource::Seed(XmlAccessProvenance::Owned));
        }
        true
    }

    /// Runtime AoS writers share the source row layout and output-buffer contract. Numeric
    /// outputs own their copied values; key views retain only the selected source key field.
    fn add_aos_buffer_writer(
        &mut self,
        equation: &mut XmlAccessEquation,
        writer: (ValueId, &Rvalue),
        output: (&Operand, &Operand, Ty, &[XmlAccessPathSegment]),
    ) -> bool {
        let (count, runtime) = writer;
        let (ptr, len, selected_ty, remaining) = output;
        let (base, struct_id, key_field, aggs, out_keys, out_vals, dictionary) = match runtime {
            Rvalue::GroupAggStr { base, struct_id, key_field, value_field, op, out_keys, out_vals } =>
                (*base, *struct_id, *key_field, vec![(*op, *value_field)], out_keys, vec![out_vals], false),
            Rvalue::GroupAggMultiStr { base, struct_id, key_field, aggs, out_keys, out_vals } =>
                (*base, *struct_id, *key_field, aggs.clone(), out_keys, out_vals.iter().collect(), false),
            Rvalue::DictEncode { base, struct_id, key_field, out_ids, out_dict } =>
                (*base, *struct_id, *key_field, Vec::new(), out_dict, vec![out_ids], true),
            _ => return false,
        };
        let same = Self::same_operand;
        let key_output = same(ptr, out_keys);
        if !key_output && !out_vals.iter().any(|out| same(ptr, out)) { return false; }
        let integer = Scalar::Int(IntTy { bits: 64, signed: true });
        let i64_ty = scalar_to_ty(integer);
        let base_ty = Ty::DynStructArray(struct_id, Layout::Aos);
        let field_ty = |field| self.graph.program.structs.get(struct_id as usize)
            .and_then(|record| record.fields.get(field as usize)).map(|field| field.ty);
        let valid_agg = |(op, field): &(hir::GroupOp, Option<u32>)| match (op, field) {
            (hir::GroupOp::Count, None) => true,
            (hir::GroupOp::Sum | hir::GroupOp::Min | hir::GroupOp::Max, Some(field)) =>
                field_ty(*field) == Some(i64_ty),
            _ => false,
        };
        if self.graph.function.slots.get(base as usize) != Some(&base_ty)
            || field_ty(key_field) != Some(Ty::Str)
            || self.graph.function.value_tys.get(count as usize) != Some(&i64_ty)
            || (!dictionary && (aggs.is_empty() || aggs.len() != out_vals.len() || !aggs.iter().all(valid_agg)))
            || ((!dictionary || key_output) && !same(len, &Operand::Value(count)))
            || selected_ty != if key_output { Ty::Str } else { i64_ty }
            || !remaining.is_empty()
        {
            equation.invalid = true;
            return true;
        }
        let mut outputs = vec![(out_keys, Scalar::Str)];
        outputs.extend(out_vals.iter().map(|out| (*out, integer)));
        for (index, (out, scalar)) in outputs.iter().enumerate() {
            if outputs[..index].iter().any(|(other, _)| same(out, other)) {
                equation.invalid = true;
            }
            if !self.heap_buffer_capacity_matches_slot_len(out, base) {
                equation.invalid = true;
            }
            self.check_operand(equation, out, Ty::Box(*scalar));
        }
        let source = self.queue(XmlAccessNode::Slot(base, Vec::new()));
        Self::add_required_source(equation, source, OperandRequirement::READ);
        if key_output {
            let source = self.queue(XmlAccessNode::Slot(base, vec![
                XmlAccessPathSegment::Element, XmlAccessPathSegment::StructField(key_field),
            ]));
            Self::add_source(equation, source);
        } else {
            Self::add_source(equation, XmlAccessSource::Seed(XmlAccessProvenance::Owned));
        }
        true
    }

    fn add_dictionary_buffer_writer(
        &mut self,
        equation: &mut XmlAccessEquation,
        writer: (ValueId, &Rvalue),
        output: (&Operand, &Operand, Ty, &[XmlAccessPathSegment]),
    ) -> bool {
        let (value, runtime) = writer;
        let (ptr, len, selected_ty, remaining) = output;
        let same = Self::same_operand;
        let integer = Scalar::Int(IntTy { bits: 64, signed: true });
        let i64_ty = scalar_to_ty(integer);
        match runtime {
            Rvalue::GatherColumnI64 { source, struct_id, field, out } if same(ptr, out) => {
                let base_ty = Ty::DynStructArray(*struct_id, Layout::Aos);
                if self.graph.function.value_tys.get(value as usize) != Some(&Ty::Unit)
                    || self.graph.program.structs.get(*struct_id as usize)
                        .and_then(|row| row.fields.get(*field as usize)).map(|field| field.ty) != Some(i64_ty)
                    || !self.heap_buffer_capacity_matches_slice_len(out, source)
                    || selected_ty != i64_ty || !remaining.is_empty()
                { equation.invalid = true; return true; }
                self.check_whole_operand(equation, source, base_ty);
                self.check_operand(equation, out, Ty::Box(integer));
                equation.seed = Some(XmlAccessProvenance::Owned);
                true
            }
            Rvalue::DictLookup { ids, n, dict, out } if same(ptr, out) => {
                if self.graph.function.value_tys.get(value as usize) != Some(&Ty::Unit)
                    || !same(len, n)
                    || !self.heap_buffer_capacity_matches(out, n)
                    || selected_ty != Ty::Str || !remaining.is_empty()
                { equation.invalid = true; return true; }
                self.check_operand(equation, ids, Ty::Box(integer));
                self.check_operand(equation, n, i64_ty);
                self.check_operand(equation, out, Ty::Box(Scalar::Str));
                self.check_whole_operand(equation, dict, Ty::DynArray(Scalar::Str));
                self.add_operand(equation, dict, Ty::Str, vec![XmlAccessPathSegment::Element]);
                true
            }
            _ => false,
        }
    }

    fn value_equation(
        &mut self,
        value: ValueId,
        path: Vec<XmlAccessPathSegment>,
    ) -> XmlAccessEquation {
        let mut equation = XmlAccessEquation::default();
        let Some(result_ty) = self.graph.function.value_tys.get(value as usize).copied() else {
            equation.invalid = true;
            return equation;
        };
        let Some(selected_ty) = xml_selected_ty(self.graph.program, result_ty, &path) else {
            equation.invalid = true;
            return equation;
        };
        if path.is_empty()
            && result_ty == Ty::Bool
            && self.graph.primary_definitions.get(value as usize) == Some(&0)
            && self.graph.auxiliary_definitions.get(value as usize) == Some(&1)
            && self
                .graph
                .auxiliary_value_definitions
                .get(value as usize)
                .is_some_and(|definition| {
                    definition.is_some_and(|definition| match definition {
                        Rvalue::CallWithCleanup(call) => call.cleanup == value,
                        Rvalue::CallIndirectWithCleanup(call) => call.cleanup == value,
                        Rvalue::XmlParse { cleanup, .. } => *cleanup == value,
                        _ => false,
                    })
                })
        {
            equation.seed = Some(XmlAccessProvenance::Owned);
            return equation;
        }
        let Some(Some(definition)) = self.graph.value_definitions.get(value as usize) else {
            equation.invalid = true;
            return equation;
        };
        if self.graph.duplicate_values.get(value as usize) != Some(&false) {
            equation.invalid = true;
            return equation;
        }
        let definition = (*definition).clone();
        if let Some(contract) = native_owner_mir_contract(self.graph.program, self.graph.function, &definition) {
            if result_ty != contract.result || !path.is_empty()
                || contract.outputs.iter().any(|&(slot, ty)| {
                    self.graph.function.slots.get(slot as usize) != Some(&ty)
                        || self.graph.function.params.contains(&slot)
                        || self.graph.slot_stores.roots.get(slot as usize).is_none_or(|stores| !stores.is_empty())
                        || self.graph.slot_stores.producers.get(slot as usize).is_none_or(|producers| producers.len() != 1 || producers[0].0 != value)
                })
            {
                equation.invalid = true;
            }
            for (operand, expected, requirement) in contract.operands {
                if requirement.move_value && !matches!(operand, Operand::Value(_) | Operand::Arg(_)) {
                    equation.invalid = true;
                }
                let source = self.read_source(&mut equation, operand, expected, Vec::new());
                Self::add_required_source(&mut equation, source, requirement);
            }
            for operand in contract.writable_buffers {
                let source = self.buffer_source(operand, Vec::new());
                Self::add_required_source(&mut equation, source, OperandRequirement {
                    write: true, exclusive: true, ..OperandRequirement::default()
                });
            }
            equation.seed = Some(if !contract.outputs.is_empty() { XmlAccessProvenance::Owned } else { contract.access });
            return equation;
        }
        let slice_index_noalias = matches!(&definition, Rvalue::SliceIndexNoalias { .. });
        match definition {
            Rvalue::Use(operand) => {
                if let Operand::BorrowedPlace(place)=&operand {
                    let physical=self.graph.function.slots.get(place.slot as usize)
                        .and_then(|root| xml_borrowed_path(self.graph.program,*root,&place.path))
                        .map(|(ty,_)| ty);
                    if result_ty == Ty::Str && place.ty == Ty::Str && path.is_empty()
                        && matches!(physical,Some(Ty::String | Ty::Str)) {
                        // Read the authenticated descriptor, retaining its founded slot/arm proof.
                        // A descriptor read grants shared access, never owner-transfer authority.
                        self.check_read_operand(&mut equation,&operand,Ty::Str);
                        equation.seed=Some(XmlAccessProvenance::Shared);
                    } else { equation.invalid=true; }
                    return equation;
                }

                let Some(source_ty) = xml_operand_base_ty(self.graph.function, &operand) else {
                    equation.invalid = true;
                    return equation;
                };
                if !xml_value_flow_matches(self.graph.program, source_ty, result_ty)
                    && !xml_ty_is_view_retype(source_ty, result_ty)
                {
                    equation.invalid = true;
                } else if path.is_empty() {
                    self.add_operand(&mut equation, &operand, source_ty, Vec::new());
                } else {
                    let Some(source_selected) =
                        xml_selected_ty(self.graph.program, source_ty, &path)
                    else {
                        equation.invalid = true;
                        return equation;
                    };
                    if !xml_value_flow_matches(
                        self.graph.program,
                        source_selected,
                        selected_ty,
                    ) && !xml_ty_is_view_retype(source_selected, selected_ty) {
                        equation.invalid = true;
                    } else {
                        self.add_operand(&mut equation, &operand, source_selected, path);
                    }
                }
            }
            ref native @ Rvalue::RawCall { ref callee, ref args, ref param_tys, ret_ty, .. } => {
                let unprotected = xml_owned_leaf_paths(self.graph.program, result_ty)
                    .is_some_and(|leaves| leaves.is_empty());
                if ret_ty != result_ty || (!unprotected && !self.native_view_call_matches(value, native)) {
                    equation.invalid = true;
                } else if unprotected {
                    equation.seed = Some(XmlAccessProvenance::Owned);
                } else {
                    self.check_operand(&mut equation, callee, Ty::Raw);
                    if let Operand::Value(callee) = callee
                        && let Some(Some(Rvalue::RawPointerLoad { ptr, .. })) = self.graph.value_definitions.get(*callee as usize)
                    {
                        self.check_operand(&mut equation, ptr, Ty::Raw);
                    }
                    for (argument, expected) in args.iter().zip(param_tys) {
                        self.check_whole_operand(&mut equation, argument, *expected);
                    }
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::ColumnBatchRow { payload, owner, index, struct_id, resource } => {
                let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
                if result_ty != Ty::Struct(struct_id)
                    || !db_resource_matches_row(self.graph.program, resource, struct_id, "batch")
                    || xml_operand_base_ty(self.graph.function, &payload) != Some(Ty::Raw)
                    || xml_operand_base_ty(self.graph.function, &owner) != Some(Ty::ResourceRef(resource))
                    || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &payload, Ty::Raw);
                    self.check_operand(&mut equation, &owner, Ty::ResourceRef(resource));
                    self.check_operand(&mut equation, &index, i64_ty);
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::ColumnBatchSoa { payload, owner, struct_id, resource } => {
                if result_ty != Ty::Soa(struct_id)
                    || !db_resource_matches_row(self.graph.program, resource, struct_id, "batch")
                    || xml_operand_base_ty(self.graph.function, &payload) != Some(Ty::Raw)
                    || xml_operand_base_ty(self.graph.function, &owner) != Some(Ty::ResourceRef(resource))
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &payload, Ty::Raw);
                    self.check_operand(&mut equation, &owner, Ty::ResourceRef(resource));
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::ResourceViewFromRaw {
                owner,
                ptr,
                len,
                resource,
                view,
                allow_null_if_empty,
                check_nonnegative_len,
                check_alignment,
                check_utf8,
            } => {
                let expected = match view {
                    hir::ResourceViewKind::StrUtf8 => Some((
                        Ty::Option(Scalar::Str),
                        Ty::Str,
                        1,
                        true,
                    )),
                    hir::ResourceViewKind::Slice(scalar) => {
                        let bits = match scalar {
                            Scalar::Int(integer) => integer.bits,
                            Scalar::Float(float) => float.bits,
                            _ => {
                                equation.invalid = true;
                                return equation;
                            }
                        };
                        align_sema::scalar_to_prim(scalar).map(|primitive| {
                            let payload = Ty::Slice(align_sema::prim_to_scalar(primitive));
                            (
                                Ty::Option(Scalar::Slice(primitive)),
                                payload,
                                u32::from(bits) / 8,
                                false,
                            )
                        })
                    }
                };
                let Some((expected_result, payload_ty, alignment, utf8)) = expected else {
                    equation.invalid = true;
                    return equation;
                };
                let path_valid = path.is_empty()
                    || path.as_slice() == [XmlAccessPathSegment::OptionSome]
                        && selected_ty == payload_ty;
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                if !xml_ty_matches_tagged_body(
                    self.graph.program,
                    result_ty,
                    expected_result,
                ) || !path_valid
                    || self
                        .graph
                        .program
                        .resources
                        .get(resource as usize)
                        .is_none()
                    || xml_operand_base_ty(self.graph.function, &owner)
                        != Some(Ty::ResourceRef(resource))
                    || xml_operand_base_ty(self.graph.function, &ptr) != Some(Ty::Raw)
                    || xml_operand_base_ty(self.graph.function, &len) != Some(i64_ty)
                    || !allow_null_if_empty
                    || !check_nonnegative_len
                    || check_alignment != alignment
                    || check_utf8 != utf8
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(
                        &mut equation,
                        &owner,
                        Ty::ResourceRef(resource),
                    );
                    self.check_operand(&mut equation, &ptr, Ty::Raw);
                    self.check_operand(&mut equation, &len, i64_ty);
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::Load(slot) => {
                if !self.graph.function.slots.get(slot as usize)
                    .is_some_and(|actual| xml_callable_flow_matches(self.graph.program, *actual, result_ty)) {
                    equation.invalid = true;
                } else {
                    Self::add_source(
                        &mut equation,
                        self.queue(XmlAccessNode::Slot(slot, path)),
                    );
                }
            }
            Rvalue::Index(slot, index) => {
                let Some(slot_ty) = self.graph.function.slots.get(slot as usize).copied() else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(element_ty) = xml_inline_array_element(self.graph.program, slot_ty) else {
                    equation.invalid = true;
                    return equation;
                };
                let mut source_path = vec![XmlAccessPathSegment::Element];
                source_path.extend(path);
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                if result_ty != element_ty
                    || xml_selected_ty(self.graph.program, slot_ty, &source_path)
                        != Some(selected_ty)
                    || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &index, i64_ty);
                    Self::add_source(
                        &mut equation,
                        self.queue(XmlAccessNode::Slot(slot, source_path)),
                    );
                }
            }
            Rvalue::IndexField(slot, index, fields) => {
                let Some(slot_ty) = self.graph.function.slots.get(slot as usize).copied() else {
                    equation.invalid = true;
                    return equation;
                };
                let Ty::StructArray(struct_id, _) = slot_ty else {
                    equation.invalid = true;
                    return equation;
                };
                let mut result_path = vec![XmlAccessPathSegment::Element];
                result_path.extend(
                    fields
                        .iter()
                        .copied()
                        .map(XmlAccessPathSegment::StructField),
                );
                let Some(expected_result) =
                    inline_struct_path_ty(self.graph.program, struct_id, &fields)
                else {
                    equation.invalid = true;
                    return equation;
                };
                let mut source_path = result_path;
                source_path.extend(path);
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                let result_matches = if expected_result == Ty::String {
                    result_ty == Ty::Str
                } else {
                    result_ty == expected_result && (matches!(expected_result, Ty::Resource(_)) || !align_sema::ty_is_move(
                        expected_result, &self.graph.program.structs, &self.graph.program.tuples,
                        &self.graph.program.enums, &self.graph.program.tagged_types,
                    ))
                };
                if fields.is_empty()
                    || !result_matches
                    || !xml_selected_ty(self.graph.program, slot_ty, &source_path).is_some_and(|source|
                        source == selected_ty || xml_ty_is_view_retype(source, selected_ty))
                    || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &index, i64_ty);
                    Self::add_source(
                        &mut equation,
                        self.queue(XmlAccessNode::Slot(slot, source_path)),
                    );
                }
            }
            Rvalue::MakeSlice(slot, length) => {
                let Some(slot_ty) = self.graph.function.slots.get(slot as usize).copied() else {
                    equation.invalid = true;
                    return equation;
                };
                let view_matches = match (slot_ty, result_ty) {
                    (Ty::Array(element, actual), Ty::Slice(view)) => {
                        element == view && u32::try_from(length) == Ok(actual)
                    }
                    (Ty::StructArray(id, actual), Ty::Slice(Scalar::Struct(view))) => {
                        id == view && u32::try_from(length) == Ok(actual)
                    }
                    _ => false,
                };
                if !view_matches {
                    equation.invalid = true;
                } else if length == 0 {
                    // Exact zero-length inline storage has no element producer to follow.
                    // Bounds validation keeps every element read unreachable; this is a
                    // readable empty view, not an uninitialized nonempty payload.
                    equation.seed = Some(XmlAccessProvenance::Shared);
                } else if path.is_empty() {
                    let source = self.queue(XmlAccessNode::Slot(
                        slot,
                        vec![XmlAccessPathSegment::Element],
                    ));
                    Self::add_required_source(
                        &mut equation,
                        source,
                        OperandRequirement::READ,
                    );
                    equation.seed = Some(XmlAccessProvenance::Shared);
                } else if let Some(remaining) =
                    path.strip_prefix(&[XmlAccessPathSegment::Element])
                {
                    let mut source_path = vec![XmlAccessPathSegment::Element];
                    source_path.extend_from_slice(remaining);
                    if xml_selected_ty(self.graph.program, slot_ty, &source_path)
                        != Some(selected_ty)
                    {
                        equation.invalid = true;
                        return equation;
                    }
                    Self::add_source(
                        &mut equation,
                        self.queue(XmlAccessNode::Slot(slot, source_path)),
                    );
                } else {
                    equation.invalid = true;
                }
            }
            Rvalue::ConstArray { elems, elem } => {
                let result_matches = align_sema::ty_to_scalar(elem)
                    .is_some_and(|element| result_ty == Ty::Slice(element));
                let path_matches = path.is_empty()
                    || path.as_slice() == [XmlAccessPathSegment::Element]
                        && selected_ty == elem;
                if !result_matches
                    || !path_matches
                    || elems
                        .iter()
                        .any(|element| !xml_const_element_matches_ty(element, elem))
                {
                    equation.invalid = true;
                } else {
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::SliceIndex(source, index)
            | Rvalue::SliceIndexNoalias {
                slice: source,
                index,
                ..
            } => {
                let Some(source_ty) = xml_operand_base_ty(self.graph.function, &source) else {
                    equation.invalid = true;
                    return equation;
                };
                let mut source_path = vec![XmlAccessPathSegment::Element];
                source_path.extend(path);
                let Some(source_selected) =
                    xml_selected_ty(self.graph.program, source_ty, &source_path)
                else {
                    equation.invalid = true;
                    return equation;
                };
                let selection_matches = {
                    source_selected == selected_ty
                        || (source_selected == Ty::String && selected_ty == Ty::Str)
                };
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                if !slice_index_result_matches(
                    self.graph.program,
                    source_ty,
                    result_ty,
                    slice_index_noalias,
                ) || !selection_matches
                    || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                {
                    equation.invalid = true;
                    return equation;
                }
                self.check_whole_read_operand(&mut equation, &source, source_ty);
                self.check_operand(&mut equation, &index, i64_ty);
                if source_ty == Ty::DynResponseArray {
                    // Indexing a batch borrows its response; it never transfers the owning leaf.
                    equation.seed = Some(XmlAccessProvenance::Shared);
                    return equation;
                }
                self.add_read_operand(
                    &mut equation,
                    &source,
                    source_selected,
                    source_path,
                );
            }
            Rvalue::SoaColumn {
                base,
                struct_id,
                field,
            } => {
                let Some(field_ty) = self
                    .graph
                    .program
                    .structs
                    .get(struct_id as usize)
                    .and_then(|definition| definition.fields.get(field as usize))
                    .map(|field| field.ty)
                else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(field_scalar) = align_sema::ty_to_scalar(field_ty) else {
                    equation.invalid = true;
                    return equation;
                };
                if self.graph.function.slots.get(base as usize) != Some(&Ty::Soa(struct_id))
                    || result_ty != Ty::Slice(field_scalar)
                {
                    equation.invalid = true;
                    return equation;
                }
                let mut source_path = vec![XmlAccessPathSegment::StructField(field)];
                if path.is_empty() {
                    let source = self.queue(XmlAccessNode::Slot(base, source_path));
                    Self::add_required_source(
                        &mut equation,
                        source,
                        OperandRequirement::READ,
                    );
                    equation.seed = Some(XmlAccessProvenance::Shared);
                } else if let Some(remaining) =
                    path.strip_prefix(&[XmlAccessPathSegment::Element])
                {
                    source_path.extend_from_slice(remaining);
                    if xml_selected_ty(self.graph.program, Ty::Soa(struct_id), &source_path)
                        != Some(selected_ty)
                    {
                        equation.invalid = true;
                        return equation;
                    }
                    Self::add_source(
                        &mut equation,
                        self.queue(XmlAccessNode::Slot(base, source_path)),
                    );
                } else {
                    equation.invalid = true;
                }
            }
            Rvalue::IndexFieldPtr {
                base,
                index,
                path: fields,
                struct_id,
            } => {
                let base_ty = match xml_operand_base_ty(self.graph.function, &base) {
                    Some(ty @ Ty::DynStructArray(id, Layout::Aos))
                    | Some(ty @ Ty::Slice(Scalar::Struct(id))) if id == struct_id => ty,
                    _ => { equation.invalid = true; return equation; }
                };
                if fields.is_empty() { equation.invalid = true; return equation; }
                let mut source_path = vec![XmlAccessPathSegment::Element];
                source_path.extend(fields.iter().copied().map(XmlAccessPathSegment::StructField));
                let Some(field_ty) = inline_struct_path_ty(self.graph.program, struct_id, &fields) else {
                    equation.invalid = true;
                    return equation;
                };
                source_path.extend(path);
                let Some(source_selected) =
                    xml_selected_ty(self.graph.program, base_ty, &source_path)
                else {
                    equation.invalid = true;
                    return equation;
                };
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                // Source indexing may read a Copy field as itself or borrow an owned `string`
                // field as `str`; it can never load a recursively Move field as a new owner.
                let result_matches = if field_ty == Ty::String {
                    result_ty == Ty::Str
                } else {
                    result_ty == field_ty
                        && !align_sema::ty_is_move(
                            field_ty,
                            &self.graph.program.structs,
                            &self.graph.program.tuples,
                            &self.graph.program.enums,
                            &self.graph.program.tagged_types,
                        )
                };
                if !result_matches
                    || xml_operand_base_ty(self.graph.function, &base) != Some(base_ty)
                    || (!xml_value_flow_matches(
                        self.graph.program,
                        source_selected,
                        selected_ty,
                    ) && !xml_ty_is_view_retype(source_selected, selected_ty))
                    || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                {
                    equation.invalid = true;
                    return equation;
                }
                self.check_operand(&mut equation, &index, i64_ty);
                self.add_read_operand(
                    &mut equation,
                    &base,
                    source_selected,
                    source_path,
                );
            }
            Rvalue::IndexColumn {
                base,
                index,
                field,
                struct_id,
            } => {
                let base_ty = Ty::Soa(struct_id);
                let Some(field_ty) = self
                    .graph
                    .program
                    .structs
                    .get(struct_id as usize)
                    .and_then(|record| record.fields.get(field as usize))
                    .map(|field| field.ty)
                else {
                    equation.invalid = true;
                    return equation;
                };
                let mut source_path = vec![XmlAccessPathSegment::StructField(field)];
                source_path.extend(path);
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                if result_ty != field_ty
                    || xml_operand_base_ty(self.graph.function, &base) != Some(base_ty)
                    || xml_selected_ty(self.graph.program, base_ty, &source_path)
                        != Some(selected_ty)
                    || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                {
                    equation.invalid = true;
                    return equation;
                }
                self.check_operand(&mut equation, &index, i64_ty);
                self.add_operand(
                    &mut equation,
                    &base,
                    selected_ty,
                    source_path,
                );
            }
            Rvalue::IndexPtr {
                base,
                index,
                struct_id,
            } => {
                let base_ty = Ty::DynStructArray(struct_id, Layout::Aos);
                let mut source_path = vec![XmlAccessPathSegment::Element];
                source_path.extend(path);
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                if self
                    .graph
                    .program
                    .structs
                    .get(struct_id as usize)
                    .is_none()
                    || result_ty != Ty::Struct(struct_id)
                    || xml_operand_base_ty(self.graph.function, &base) != Some(base_ty)
                    || xml_selected_ty(self.graph.program, base_ty, &source_path)
                        != Some(selected_ty)
                    || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                {
                    equation.invalid = true;
                    return equation;
                }
                self.check_operand(&mut equation, &index, i64_ty);
                self.add_operand(
                    &mut equation,
                    &base,
                    selected_ty,
                    source_path,
                );
            }
            Rvalue::SoaGather {
                base,
                index,
                struct_id,
            } => {
                let Some(definition) = self.graph.program.structs.get(struct_id as usize) else {
                    equation.invalid = true;
                    return equation;
                };
                let base_ty = Ty::Soa(struct_id);
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                if result_ty != Ty::Struct(struct_id)
                    || xml_operand_base_ty(self.graph.function, &base) != Some(base_ty)
                    || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                {
                    equation.invalid = true;
                    return equation;
                }
                self.check_operand(&mut equation, &index, i64_ty);
                if path.is_empty() {
                    for (field, field_definition) in definition.fields.iter().enumerate() {
                        let Ok(field) = u32::try_from(field) else {
                            equation.invalid = true;
                            return equation;
                        };
                        let source = self.source(
                            &base,
                            field_definition.ty,
                            vec![XmlAccessPathSegment::StructField(field)],
                        );
                        Self::add_required_source(
                            &mut equation,
                            source,
                            OperandRequirement::READ,
                        );
                    }
                    equation.seed = Some(XmlAccessProvenance::Shared);
                } else {
                    if xml_selected_ty(self.graph.program, base_ty, &path) != Some(selected_ty) {
                        equation.invalid = true;
                        return equation;
                    }
                    self.add_operand(&mut equation, &base, selected_ty, path);
                }
            }
            Rvalue::ArenaAlloc {
                handle,
                count,
                elem,
            } => {
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                let expected_result = align_sema::ty_to_scalar(elem).map(Ty::Box);
                if !path.is_empty()
                    || expected_result != Some(result_ty)
                    || xml_operand_base_ty(self.graph.function, &handle)
                        != Some(Ty::ArenaHandle)
                    || xml_operand_base_ty(self.graph.function, &count) != Some(i64_ty)
                {
                    equation.invalid = true;
                    return equation;
                }
                self.check_operand(&mut equation, &handle, Ty::ArenaHandle);
                self.check_operand(&mut equation, &count, i64_ty);
                equation.seed = Some(XmlAccessProvenance::Shared);
            }
            Rvalue::HeapAllocBuf { count, elem } => {
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                let expected_result = align_sema::ty_to_scalar(elem).map(Ty::Box);
                if !path.is_empty()
                    || expected_result != Some(result_ty)
                    || xml_operand_base_ty(self.graph.function, &count) != Some(i64_ty)
                {
                    equation.invalid = true;
                    return equation;
                }
                self.check_operand(&mut equation, &count, i64_ty);
                equation.seed = Some(XmlAccessProvenance::Owned);
            }
            Rvalue::SoaAlloc { handle, len, struct_id } => {
                let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
                let expected = self.graph.program.structs.get(struct_id as usize)
                    .and_then(|record| record.fields.first())
                    .and_then(|field| align_sema::ty_to_scalar(field.ty))
                    .map(Ty::Box);
                if !path.is_empty() || expected != Some(result_ty)
                    || xml_operand_base_ty(self.graph.function, &handle) != Some(Ty::ArenaHandle)
                    || xml_operand_base_ty(self.graph.function, &len) != Some(i64_ty)
                {
                    equation.invalid = true;
                    return equation;
                }
                self.check_operand(&mut equation, &handle, Ty::ArenaHandle);
                self.check_operand(&mut equation, &len, i64_ty);
                equation.seed = Some(XmlAccessProvenance::Shared);
            }
            Rvalue::MakeDynArray { ptr, len } => {
                let same_operand = |left: &Operand, right: &Operand| match (left, right) {
                    (Operand::Value(a), Operand::Value(b)) | (Operand::Arg(a), Operand::Arg(b)) => a == b,
                    (Operand::Const(Const::Int(a, at)), Operand::Const(Const::Int(b, bt))) => a == b && at == bt,
                    _ => false,
                };
                let Operand::Value(ptr_value) = ptr else {
                    equation.invalid = true;
                    return equation;
                };
                let ptr = Operand::Value(ptr_value);
                let Some(Ty::Box(element)) = xml_operand_base_ty(self.graph.function, &ptr) else {
                    equation.invalid = true;
                    return equation;
                };
                let element_ty = scalar_to_ty(element);
                let expected_result = match element {
                    Scalar::Struct(id) => Ty::DynStructArray(id, Layout::Aos),
                    _ => Ty::DynArray(element),
                };
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                if let Ty::Soa(struct_id) = result_ty {
                    let Some(Some(Rvalue::SoaAlloc { struct_id: allocated, len: allocated_len, .. })) =
                        self.graph.value_definitions.get(ptr_value as usize)
                    else {
                        equation.invalid = true;
                        return equation;
                    };
                    if *allocated != struct_id || !same_operand(allocated_len, &len)
                        || xml_operand_base_ty(self.graph.function, &len) != Some(i64_ty)
                    {
                        equation.invalid = true;
                        return equation;
                    }
                    self.check_operand(&mut equation, &ptr, Ty::Box(element));
                    self.check_operand(&mut equation, &len, i64_ty);
                    if path.is_empty() {
                        self.add_operand(&mut equation, &ptr, Ty::Box(element), Vec::new());
                        return equation;
                    }
                    let Some((XmlAccessPathSegment::StructField(selected_field), remaining)) = path.split_first() else {
                        equation.invalid = true;
                        return equation;
                    };
                    let mut found_store = false;
                    for statement in self.graph.function.blocks.iter().flat_map(|block| &block.stmts) {
                        let Stmt::StoreColumn { base, len: stored_len, index, field, struct_id: stored_id, value: stored } = statement else {
                            continue;
                        };
                        if !same_operand(base, &ptr) { continue; }
                        let field_ty = self.graph.program.structs.get(struct_id as usize)
                            .and_then(|record| record.fields.get(*field as usize))
                            .map(|field| field.ty);
                        if *stored_id != struct_id || !same_operand(stored_len, &len)
                            || xml_operand_base_ty(self.graph.function, index) != Some(i64_ty)
                            || field_ty.is_none()
                            || xml_operand_base_ty(self.graph.function, stored) != field_ty
                        {
                            equation.invalid = true;
                            continue;
                        }
                        self.check_operand(&mut equation, index, i64_ty);
                        if *field == *selected_field {
                            found_store = true;
                            self.add_operand(&mut equation, stored, selected_ty, remaining.to_vec());
                        }
                    }
                    if !found_store { equation.invalid = true; }
                    return equation;
                }
                if result_ty != expected_result
                    || xml_operand_base_ty(self.graph.function, &len) != Some(i64_ty)
                {
                    equation.invalid = true;
                    return equation;
                }
                self.check_operand(&mut equation, &ptr, Ty::Box(element));
                self.check_operand(&mut equation, &len, i64_ty);
                if path.is_empty() {
                    self.add_operand(
                        &mut equation,
                        &ptr,
                        Ty::Box(element),
                        Vec::new(),
                    );
                    return equation;
                }
                let Some(remaining) = path.strip_prefix(&[XmlAccessPathSegment::Element]) else {
                    equation.invalid = true;
                    return equation;
                };
                if xml_selected_ty(self.graph.program, element_ty, remaining)
                    != Some(selected_ty)
                {
                    equation.invalid = true;
                    return equation;
                }
                let mut found_store = false;
                for statement in self
                    .graph
                    .function
                    .blocks
                    .iter()
                    .flat_map(|block| &block.stmts)
                {
                    if let Stmt::Let(count, runtime) = statement {
                        let output = (&ptr, &len, selected_ty, remaining);
                        if self.add_column_buffer_writer(&mut equation, (*count, runtime), output)
                            || self.add_aos_buffer_writer(&mut equation, (*count, runtime), output)
                            || self.add_dictionary_buffer_writer(&mut equation, (*count, runtime), output)
                        {
                            found_store = true;
                            continue;
                        }
                    }
                    let (stored_ptr, index, stored) = match statement {
                        Stmt::PtrStore(stored_ptr, index, stored) => {
                            (stored_ptr, index, stored)
                        }
                        Stmt::PtrStoreNoalias {
                            ptr: stored_ptr,
                            index,
                            value: stored,
                            ..
                        } => (stored_ptr, index, stored),
                        _ => continue,
                    };
                    if !matches!(stored_ptr, Operand::Value(value) if *value == ptr_value) {
                        continue;
                    }
                    found_store = true;
                    if xml_operand_base_ty(self.graph.function, index) != Some(i64_ty)
                        || xml_operand_base_ty(self.graph.function, stored) != Some(element_ty)
                    {
                        equation.invalid = true;
                        continue;
                    }
                    self.check_operand(&mut equation, index, i64_ty);
                    self.check_whole_operand(&mut equation, stored, element_ty);
                    self.add_operand(
                        &mut equation,
                        stored,
                        selected_ty,
                        remaining.to_vec(),
                    );
                }
                if !found_store {
                    equation.invalid = true;
                }
            }
            runtime @ (Rvalue::GroupAgg { .. } | Rvalue::GroupAggStrCols { .. }) => {
                let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
                let (out, key) = match &runtime {
                    Rvalue::GroupAgg { out_keys, .. } => (out_keys, i64_ty),
                    Rvalue::GroupAggStrCols { out_keys, .. } => (out_keys, Ty::Str),
                    _ => { equation.invalid = true; return equation; }
                };
                let mut inputs = XmlAccessEquation::default();
                if result_ty != i64_ty || !path.is_empty()
                    || !self.add_column_buffer_writer(&mut inputs, (value, &runtime),
                        (out, &Operand::Value(value), key, &[]))
                { equation.invalid = true; return equation; }
                equation.invalid |= inputs.invalid;
                equation.checks.extend(inputs.checks);
                equation.checks.extend(inputs.dependencies.into_iter().map(|source| (source, OperandRequirement::READ)));
                equation.seed = Some(XmlAccessProvenance::Owned);
            }
            runtime @ (Rvalue::GroupAggStr { .. } | Rvalue::GroupAggMultiStr { .. } | Rvalue::DictEncode { .. }) => {
                let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
                let out = match &runtime {
                    Rvalue::GroupAggStr { out_keys, .. } | Rvalue::GroupAggMultiStr { out_keys, .. } => out_keys,
                    Rvalue::DictEncode { out_dict, .. } => out_dict,
                    _ => { equation.invalid = true; return equation; }
                };
                let mut inputs = XmlAccessEquation::default();
                if result_ty != i64_ty || !path.is_empty()
                    || !self.add_aos_buffer_writer(&mut inputs, (value, &runtime),
                        (out, &Operand::Value(value), Ty::Str, &[]))
                { equation.invalid = true; return equation; }
                equation.invalid |= inputs.invalid;
                equation.checks.extend(inputs.checks);
                equation.checks.extend(inputs.dependencies.into_iter().map(|source| (source, OperandRequirement::READ)));
                equation.seed = Some(XmlAccessProvenance::Owned);
            }
            runtime @ (Rvalue::GatherColumnI64 { .. } | Rvalue::DictLookup { .. }) => {
                let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
                let unused_length = Operand::Const(Const::Int(0, i64_ty));
                let (out, length, element) = match &runtime {
                    Rvalue::GatherColumnI64 { out, .. } => (out, &unused_length, i64_ty),
                    Rvalue::DictLookup { out, n, .. } => (out, n, Ty::Str),
                    _ => { equation.invalid = true; return equation; }
                };
                let mut inputs = XmlAccessEquation::default();
                if result_ty != Ty::Unit || !path.is_empty()
                    || !self.add_dictionary_buffer_writer(&mut inputs, (value, &runtime), (out, length, element, &[]))
                { equation.invalid = true; return equation; }
                equation.invalid |= inputs.invalid;
                equation.checks.extend(inputs.checks);
                equation.checks.extend(inputs.dependencies.into_iter().map(|source| (source, OperandRequirement::READ)));
                equation.seed = Some(XmlAccessProvenance::Owned);
            }
            Rvalue::MakeDictEncoded { source, ids, dict } => {
                let Ty::DictEncoded(id, key) = result_ty else {
                    equation.invalid = true;
                    return equation;
                };
                let operands = [&source, &ids, &dict];
                for (index, operand) in operands.iter().enumerate() {
                    let Some(expected) = u32::try_from(index).ok()
                        .and_then(|index| xml_dict_field_ty(self.graph.program, id, key, index))
                    else { equation.invalid = true; return equation; };
                    self.check_whole_operand(&mut equation, operand, expected);
                }
                if path.is_empty() {
                    equation.seed = Some(XmlAccessProvenance::Owned);
                } else if let Some((XmlAccessPathSegment::TupleElement(index), tail)) = path.split_first() {
                    if let Some(operand) = operands.get(*index as usize) {
                        self.add_operand(&mut equation, operand, selected_ty, tail.to_vec());
                    } else { equation.invalid = true; }
                } else { equation.invalid = true; }
            }
            Rvalue::DictField { base, idx } => {
                let Some(Ty::DictEncoded(id, key)) = self.graph.function.slots.get(base as usize).copied() else {
                    equation.invalid = true;
                    return equation;
                };
                if xml_dict_field_ty(self.graph.program, id, key, idx) != Some(result_ty) {
                    equation.invalid = true;
                } else {
                    let mut source_path = vec![XmlAccessPathSegment::TupleElement(idx)];
                    source_path.extend(path);
                    let source = self.queue(XmlAccessNode::Slot(base, source_path));
                    Self::add_source(&mut equation, source);
                }
            }
            Rvalue::Field(slot, fields) => {
                let Some(slot_ty) = self.graph.function.slots.get(slot as usize).copied() else {
                    equation.invalid = true;
                    return equation;
                };
                let mut selected_path = fields
                    .iter()
                    .copied()
                    .map(XmlAccessPathSegment::StructField)
                    .collect::<Vec<_>>();
                if fields.is_empty()
                    || xml_selected_ty(self.graph.program, slot_ty, &selected_path) != Some(result_ty)
                {
                    equation.invalid = true;
                } else {
                    selected_path.extend(path);
                    Self::add_source(
                        &mut equation,
                        self.queue(XmlAccessNode::Slot(slot, selected_path)),
                    );
                }
            }
            Rvalue::Select { cond, a, b } => {
                let condition_ty = xml_operand_base_ty(self.graph.function, &cond);
                let condition_matches = condition_ty == Some(Ty::Bool)
                    || xml_numeric_vector_shape(result_ty).is_some_and(|(element, lanes)| {
                        path.is_empty() && condition_ty == Some(Ty::Mask(element, lanes))
                    });
                if !condition_matches
                    || !xml_operand_base_ty(self.graph.function, &a)
                        .is_some_and(|actual| xml_callable_flow_matches(self.graph.program, actual, result_ty))
                    || !xml_operand_base_ty(self.graph.function, &b)
                        .is_some_and(|actual| xml_callable_flow_matches(self.graph.program, actual, result_ty))
                {
                    equation.invalid = true;
                } else {
                    // A condition is checked without contributing its access to either result
                    // arm, for both scalar branchless selection and lane-wise SIMD selection.
                    let Some(condition_ty) = condition_ty else {
                        equation.invalid = true;
                        return equation;
                    };
                    self.check_operand(&mut equation, &cond, condition_ty);
                    self.add_operand(&mut equation, &a, selected_ty, path.clone());
                    self.add_operand(&mut equation, &b, selected_ty, path);
                }
            }
            Rvalue::MakeTuple { tuple_id, elems } => {
                let Some(tuple) = self.graph.program.tuples.get(tuple_id as usize) else {
                    equation.invalid = true;
                    return equation;
                };
                let operand_tys = elems
                    .iter()
                    .map(|operand| xml_operand_base_ty(self.graph.function, operand))
                    .collect::<Option<Vec<_>>>();
                if result_ty != Ty::Tuple(tuple_id)
                    || elems.len() != tuple.elems.len()
                    || operand_tys.as_ref().is_none_or(|actual| {
                        actual.iter().zip(&tuple.elems).any(|(actual, expected)| {
                            !xml_value_flow_matches(
                                self.graph.program,
                                *actual,
                                scalar_to_ty(*expected),
                            )
                        })
                    })
                {
                    equation.invalid = true;
                    return equation;
                }
                let Some(operand_tys) = operand_tys else {
                    equation.invalid = true;
                    return equation;
                };
                for (operand, actual) in elems.iter().zip(&operand_tys) {
                    self.check_operand(&mut equation, operand, *actual);
                }
                if path.is_empty() {
                    if elems.is_empty() {
                        equation.seed = Some(XmlAccessProvenance::Owned);
                    } else {
                        for (operand, actual) in elems.iter().zip(&operand_tys) {
                            self.add_operand(
                                &mut equation,
                                operand,
                                *actual,
                                Vec::new(),
                            );
                        }
                    }
                    return equation;
                }
                let Some((XmlAccessPathSegment::TupleElement(index), rest)) = path.split_first()
                else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(operand) = elems.get(*index as usize) else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(actual) = operand_tys.get(*index as usize).copied() else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(actual_selected) =
                    xml_selected_ty(self.graph.program, actual, rest)
                else {
                    equation.invalid = true;
                    return equation;
                };
                if !xml_value_flow_matches(
                    self.graph.program,
                    actual_selected,
                    selected_ty,
                ) {
                    equation.invalid = true;
                } else {
                    self.add_operand(
                        &mut equation,
                        operand,
                        actual_selected,
                        rest.to_vec(),
                    );
                }
            }
            Rvalue::TupleIndex { tuple, index } => {
                let Some(tuple_ty) = xml_operand_base_ty(self.graph.function, &tuple) else {
                    equation.invalid = true;
                    return equation;
                };
                let Ty::Tuple(tuple_id) = tuple_ty else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(payload_ty) = self
                    .graph
                    .program
                    .tuples
                    .get(tuple_id as usize)
                    .and_then(|definition| definition.elems.get(index as usize))
                    .copied()
                    .map(scalar_to_ty)
                else {
                    equation.invalid = true;
                    return equation;
                };
                if !xml_value_flow_matches(self.graph.program, payload_ty, result_ty) {
                    equation.invalid = true;
                } else {
                    let mut selected_path = vec![XmlAccessPathSegment::TupleElement(index)];
                    selected_path.extend(path);
                    let Some(source_selected) =
                        xml_selected_ty(self.graph.program, tuple_ty, &selected_path)
                    else {
                        equation.invalid = true;
                        return equation;
                    };
                    if !xml_value_flow_matches(
                        self.graph.program,
                        source_selected,
                        selected_ty,
                    ) {
                        equation.invalid = true;
                    } else {
                        self.add_operand(
                            &mut equation,
                            &tuple,
                            source_selected,
                            selected_path,
                        );
                    }
                }
            }
            Rvalue::MakeEnum {
                enum_id,
                variant,
                payload,
            } => {
                let Some(definition) = self
                    .graph
                    .program
                    .enums
                    .get(enum_id as usize)
                    .and_then(|definition| definition.variants.get(variant as usize))
                else {
                    equation.invalid = true;
                    return equation;
                };
                let operand_tys = payload
                    .iter()
                    .map(|operand| xml_operand_base_ty(self.graph.function, operand))
                    .collect::<Option<Vec<_>>>();
                if result_ty != Ty::Enum(enum_id)
                    || payload.len() != definition.payload.len()
                    || operand_tys.as_ref().is_none_or(|actual| {
                        actual
                            .iter()
                            .zip(&definition.payload)
                            .any(|(actual, expected)| {
                                !xml_value_flow_matches(
                                    self.graph.program,
                                    *actual,
                                    scalar_to_ty(*expected),
                                )
                            })
                    })
                {
                    equation.invalid = true;
                    return equation;
                }
                let Some(operand_tys) = operand_tys else {
                    equation.invalid = true;
                    return equation;
                };
                if let Some(XmlAccessPathSegment::EnumPayload {
                    enum_id: path_enum,
                    variant: path_variant,
                    ..
                }) = path.first()
                    && (*path_enum != enum_id || *path_variant != variant)
                {
                    equation.absent = true;
                    return equation;
                }
                for (operand, actual) in payload.iter().zip(&operand_tys) {
                    self.check_operand(&mut equation, operand, *actual);
                }
                if path.is_empty() {
                    if payload.is_empty() {
                        equation.seed = Some(XmlAccessProvenance::Owned);
                    } else {
                        for (operand, actual) in payload.iter().zip(&operand_tys) {
                            self.add_operand(
                                &mut equation,
                                operand,
                                *actual,
                                Vec::new(),
                            );
                        }
                    }
                    return equation;
                }
                let Some((XmlAccessPathSegment::EnumPayload {
                    enum_id: path_enum,
                    variant: path_variant,
                    slot,
                }, rest)) = path.split_first()
                else {
                    equation.invalid = true;
                    return equation;
                };
                if *path_enum != enum_id || *path_variant != variant {
                    equation.invalid = true;
                    return equation;
                }
                let Some(operand) = payload.get(*slot as usize) else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(actual) = operand_tys.get(*slot as usize).copied() else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(actual_selected) =
                    xml_selected_ty(self.graph.program, actual, rest)
                else {
                    equation.invalid = true;
                    return equation;
                };
                if !xml_value_flow_matches(
                    self.graph.program,
                    actual_selected,
                    selected_ty,
                ) {
                    equation.invalid = true;
                } else {
                    self.add_operand(
                        &mut equation,
                        operand,
                        actual_selected,
                        rest.to_vec(),
                    );
                }
            }
            Rvalue::EnumPayload {
                enum_id,
                variant,
                slot,
                operand,
            } => {
                let Some(payload_ty) = self
                    .graph
                    .program
                    .enums
                    .get(enum_id as usize)
                    .and_then(|definition| definition.variants.get(variant as usize))
                    .and_then(|definition| definition.payload.get(slot as usize))
                    .copied()
                    .map(scalar_to_ty)
                else {
                    equation.invalid = true;
                    return equation;
                };
                if xml_operand_base_ty(self.graph.function, &operand) != Some(Ty::Enum(enum_id))
                    || !xml_value_flow_matches(
                        self.graph.program,
                        payload_ty,
                        result_ty,
                    )
                {
                    equation.invalid = true;
                } else {
                    equation.require_present = true;
                    equation.guarded_absence = self.extraction_guard_matches(value, &operand,
                        &XmlAccessPathSegment::EnumPayload { enum_id, variant, slot });
                    let mut selected_path = vec![XmlAccessPathSegment::EnumPayload {
                        enum_id,
                        variant,
                        slot,
                    }];
                    selected_path.extend(path);
                    let Some(source_selected) = xml_selected_ty(
                        self.graph.program,
                        Ty::Enum(enum_id),
                        &selected_path,
                    ) else {
                        equation.invalid = true;
                        return equation;
                    };
                    if !xml_value_flow_matches(
                        self.graph.program,
                        source_selected,
                        selected_ty,
                    ) {
                        equation.invalid = true;
                    } else {
                        self.add_operand(
                            &mut equation,
                            &operand,
                            source_selected,
                            selected_path,
                        );
                    }
                }
            }
            Rvalue::OptionSome(operand) => {
                let Some(payload_ty) = xml_option_payload(self.graph.program, result_ty) else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(actual_payload_ty) =
                    xml_operand_base_ty(self.graph.function, &operand)
                else {
                    equation.invalid = true;
                    return equation;
                };
                if !xml_value_flow_matches(
                    self.graph.program,
                    actual_payload_ty,
                    payload_ty,
                ) {
                    equation.invalid = true;
                    return equation;
                }
                self.check_operand(&mut equation, &operand, actual_payload_ty);
                if path.is_empty() {
                    self.add_operand(
                        &mut equation,
                        &operand,
                        actual_payload_ty,
                        Vec::new(),
                    );
                    return equation;
                }
                let Some((XmlAccessPathSegment::OptionSome, rest)) = path.split_first() else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(actual_selected) =
                    xml_selected_ty(self.graph.program, actual_payload_ty, rest)
                else {
                    equation.invalid = true;
                    return equation;
                };
                if !xml_value_flow_matches(
                    self.graph.program,
                    actual_selected,
                    selected_ty,
                ) {
                    equation.invalid = true;
                } else {
                    self.add_operand(
                        &mut equation,
                        &operand,
                        actual_selected,
                        rest.to_vec(),
                    );
                }
            }
            Rvalue::OptionNone => {
                if xml_option_payload(self.graph.program, result_ty).is_none() {
                    equation.invalid = true;
                } else if path.is_empty() {
                    equation.seed = Some(XmlAccessProvenance::Owned);
                } else if matches!(path.first(), Some(XmlAccessPathSegment::OptionSome)) {
                    equation.absent = true;
                } else {
                    equation.invalid = true;
                }
            }
            Rvalue::OptionUnwrap(operand) => {
                let Some(option_ty) = xml_operand_base_ty(self.graph.function, &operand) else {
                    equation.invalid = true;
                    return equation;
                };
                let payload_matches = xml_option_payload(self.graph.program, option_ty)
                    .is_some_and(|payload| {
                        xml_value_flow_matches(self.graph.program, payload, result_ty)
                    });
                if !payload_matches {
                    equation.invalid = true;
                } else {
                    equation.require_present = true;
                    equation.guarded_absence = self.extraction_guard_matches(value, &operand,
                        &XmlAccessPathSegment::OptionSome);
                    let mut selected_path = vec![XmlAccessPathSegment::OptionSome];
                    selected_path.extend(path);
                    let Some(source_selected) =
                        xml_selected_ty(self.graph.program, option_ty, &selected_path)
                    else {
                        equation.invalid = true;
                        return equation;
                    };
                    if !xml_value_flow_matches(
                        self.graph.program,
                        source_selected,
                        selected_ty,
                    ) {
                        equation.invalid = true;
                    } else {
                        self.add_operand(
                            &mut equation,
                            &operand,
                            source_selected,
                            selected_path,
                        );
                    }
                }
            }
            Rvalue::OptionIsSome(operand) => {
                let wrapper_ty = xml_operand_base_ty(self.graph.function, &operand);
                if result_ty != Ty::Bool
                    || !path.is_empty()
                    || !wrapper_ty.is_some_and(|ty| {
                        xml_option_payload(self.graph.program, ty).is_some()
                    })
                {
                    equation.invalid = true;
                } else {
                    let Some(wrapper_ty) = wrapper_ty else {
                        equation.invalid = true;
                        return equation;
                    };
                    self.check_operand(
                        &mut equation,
                        &operand,
                        wrapper_ty,
                    );
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::ResultOk(ref operand) | Rvalue::ResultErr(ref operand) => {
                let ok = matches!(&definition, Rvalue::ResultOk(_));
                let Some(payload_ty) = xml_result_payload(self.graph.program, result_ty, ok) else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(actual_payload_ty) =
                    xml_operand_base_ty(self.graph.function, operand)
                else {
                    equation.invalid = true;
                    return equation;
                };
                if !xml_value_flow_matches(
                    self.graph.program,
                    actual_payload_ty,
                    payload_ty,
                ) {
                    equation.invalid = true;
                    return equation;
                }
                let expected_segment = if ok {
                    XmlAccessPathSegment::ResultOk
                } else {
                    XmlAccessPathSegment::ResultErr
                };
                if let Some(segment) = path.first()
                    && matches!(segment, XmlAccessPathSegment::ResultOk | XmlAccessPathSegment::ResultErr)
                    && segment != &expected_segment
                {
                    equation.absent = true;
                    return equation;
                }
                self.check_operand(&mut equation, operand, actual_payload_ty);
                if path.is_empty() {
                    self.add_operand(
                        &mut equation,
                        operand,
                        actual_payload_ty,
                        Vec::new(),
                    );
                    return equation;
                }
                let Some((segment, rest)) = path.split_first() else {
                    equation.invalid = true;
                    return equation;
                };
                if segment != &expected_segment {
                    equation.invalid = true;
                } else {
                    let Some(actual_selected) =
                        xml_selected_ty(self.graph.program, actual_payload_ty, rest)
                    else {
                        equation.invalid = true;
                        return equation;
                    };
                    if !xml_value_flow_matches(
                        self.graph.program,
                        actual_selected,
                        selected_ty,
                    ) {
                        equation.invalid = true;
                    } else {
                        self.add_operand(
                            &mut equation,
                            operand,
                            actual_selected,
                            rest.to_vec(),
                        );
                    }
                }
            }
            Rvalue::ResultUnwrapOk(ref operand) | Rvalue::ResultUnwrapErr(ref operand) => {
                let ok = matches!(&definition, Rvalue::ResultUnwrapOk(_));
                let Some(wrapper_ty) = xml_operand_base_ty(self.graph.function, operand) else {
                    equation.invalid = true;
                    return equation;
                };
                let payload_matches = xml_result_payload(self.graph.program, wrapper_ty, ok)
                    .is_some_and(|payload| {
                        xml_value_flow_matches(self.graph.program, payload, result_ty)
                    });
                if !payload_matches {
                    equation.invalid = true;
                } else {
                    equation.require_present = true;
                    equation.guarded_absence = self.extraction_guard_matches(value, operand,
                        &if ok { XmlAccessPathSegment::ResultOk } else { XmlAccessPathSegment::ResultErr });
                    let mut selected_path = vec![if ok {
                        XmlAccessPathSegment::ResultOk
                    } else {
                        XmlAccessPathSegment::ResultErr
                    }];
                    selected_path.extend(path);
                    let Some(source_selected) =
                        xml_selected_ty(self.graph.program, wrapper_ty, &selected_path)
                    else {
                        equation.invalid = true;
                        return equation;
                    };
                    if !xml_value_flow_matches(
                        self.graph.program,
                        source_selected,
                        selected_ty,
                    ) {
                        equation.invalid = true;
                    } else {
                        self.add_operand(
                            &mut equation,
                            operand,
                            source_selected,
                            selected_path,
                        );
                    }
                }
            }
            Rvalue::ResultIsOk(operand) => {
                let wrapper_ty = xml_operand_base_ty(self.graph.function, &operand);
                if result_ty != Ty::Bool
                    || !path.is_empty()
                    || !wrapper_ty.is_some_and(|ty| {
                        xml_result_payload(self.graph.program, ty, true).is_some()
                            && xml_result_payload(self.graph.program, ty, false).is_some()
                    })
                {
                    equation.invalid = true;
                } else {
                    let Some(wrapper_ty) = wrapper_ty else {
                        equation.invalid = true;
                        return equation;
                    };
                    self.check_operand(
                        &mut equation,
                        &operand,
                        wrapper_ty,
                    );
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::EnumTagEq {
                enum_id,
                scrutinee,
                variant,
            } => {
                let variant_valid = self
                    .graph
                    .program
                    .enums
                    .get(enum_id as usize)
                    .is_some_and(|definition| (variant as usize) < definition.variants.len());
                if result_ty != Ty::Bool
                    || !path.is_empty()
                    || !variant_valid
                    || xml_operand_base_ty(self.graph.function, &scrutinee)
                        != Some(Ty::Enum(enum_id))
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &scrutinee, Ty::Enum(enum_id));
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::MakeError { enum_id, tag, code } => {
                let i32_ty = Ty::Int(IntTy {
                    bits: 32,
                    signed: true,
                });
                if result_ty != Ty::Enum(enum_id)
                    || !path.is_empty()
                    || !builtin_error_enum_is_exact(self.graph.program, enum_id)
                    || xml_operand_base_ty(self.graph.function, &tag) != Some(i32_ty)
                    || xml_operand_base_ty(self.graph.function, &code) != Some(i32_ty)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &tag, i32_ty);
                    self.check_operand(&mut equation, &code, i32_ty);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::FnAddr { target, signature } => {
                let Ty::Fn(id) = result_ty else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(canonical) = xml_fn_type_facts(self.graph.program, id) else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(target_facts) = xml_direct_call_facts(
                    self.graph.program,
                    &target,
                    self.graph.local_contracts,
                ) else {
                    equation.invalid = true;
                    return equation;
                };
                if !path.is_empty()
                    || canonical != target_facts
                    || !xml_signature_matches_facts(&signature, &canonical)
                {
                    equation.invalid = true;
                } else {
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::Closure {
                lifted,
                captures,
                capture_tys,
                signature,
            } => {
                let Ty::Fn(id) = result_ty else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(canonical) = xml_fn_type_facts(self.graph.program, id) else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(target) = self
                    .graph
                    .program
                    .fns
                    .iter()
                    .find(|function| function.name == lifted)
                else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(explicit) = target.params.len().checked_sub(capture_tys.len()) else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(explicit_param_slots) = target.params.get(..explicit) else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(target_capture_slots) = target.params.get(explicit..) else {
                    equation.invalid = true;
                    return equation;
                };
                let Some(explicit_modes) = target.param_modes.get(..explicit) else {
                    equation.invalid = true;
                    return equation;
                };
                let explicit_params = explicit_param_slots
                    .iter()
                    .map(|slot| target.slots.get(*slot as usize).copied())
                    .collect::<Option<Vec<_>>>();
                let target_capture_tys = target_capture_slots
                    .iter()
                    .map(|slot| target.slots.get(*slot as usize).copied())
                    .collect::<Option<Vec<_>>>();
                let explicit = u32::try_from(explicit).ok();
                let capture_count = u32::try_from(capture_tys.len()).ok();
                let facts = explicit_params
                    .zip(explicit)
                    .zip(capture_count)
                    .and_then(|((params, explicit), capture_count)| {
                        Some(XmlCallFacts {
                            params,
                            modes: explicit_modes.to_vec(),
                            ret: target.ret,
                            borrow: xml_closure_borrow_summary(
                                &target.return_borrow,
                                explicit,
                                capture_count,
                            )?,
                            region: xml_closure_region_summary(
                                &target.return_region,
                                explicit,
                                capture_count,
                            )?,
                            cleanup: target.return_cleanup,
                        })
                    });
                let captures_match = captures.len() == capture_tys.len()
                    && target_capture_tys.as_ref() == Some(&capture_tys)
                    && captures.iter().zip(&capture_tys).all(|(operand, expected)| {
                        xml_operand_base_ty(self.graph.function, operand) == Some(*expected)
                    });
                if !path.is_empty()
                    || facts.as_ref() != Some(&canonical)
                    || !xml_signature_matches_facts(&signature, &canonical)
                    || !captures_match
                {
                    equation.invalid = true;
                } else {
                    for (capture, expected) in captures.iter().zip(&capture_tys) {
                        self.check_operand(&mut equation, capture, *expected);
                    }
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::Call(DirectCall::Runtime(key), args) => {
                let Some(argument_types) = args
                    .iter()
                    .map(|operand| xml_operand_base_ty(self.graph.function, operand))
                    .collect::<Option<Vec<_>>>()
                else {
                    equation.invalid = true;
                    return equation;
                };
                let result_is_unprotected = xml_owned_leaf_paths(self.graph.program, result_ty)
                    .is_some_and(|leaves| leaves.is_empty());
                if !path.is_empty()
                    || !result_is_unprotected
                    || !direct_runtime_key_is_valid(
                        key,
                        &argument_types,
                        result_ty,
                        self.graph.program,
                    )
                {
                    equation.invalid = true;
                    return equation;
                }
                for (argument, expected) in args.iter().zip(argument_types) {
                    self.check_whole_operand(&mut equation, argument, expected);
                }
                equation.seed = Some(XmlAccessProvenance::Owned);
            }
            Rvalue::Call(DirectCall::Program(target), args) => {
                if let Some(facts) = xml_direct_call_facts(
                    self.graph.program,
                    &target,
                    self.graph.local_contracts,
                ) {
                    self.add_call_result(&mut equation, XmlCallResult {
                        result: value,
                        result_ty,
                        selected_ty,
                        args: &args,
                        callee: None,
                        facts: &facts,
                        cleanup: None,
                        modes_match: direct_operands_match_modes(
                            &target,
                            &args,
                            &facts.modes,
                            &facts.params,
                            self.graph.program,
                        ),
                    });
                    return equation;
                }
                let has_align_declaration = self
                    .graph
                    .program
                    .fns
                    .iter()
                    .any(|function| function.name == target)
                    || self
                        .graph
                        .program
                        .imported_fns
                        .iter()
                        .any(|function| function.name == target);
                let mut externs = self
                    .graph
                    .program
                    .externs
                    .iter()
                    .filter(|declaration| declaration.name == target);
                let Some(extern_) = externs.next() else {
                    equation.invalid = true;
                    return equation;
                };
                let result_is_unprotected = xml_owned_leaf_paths(self.graph.program, result_ty)
                    .is_some_and(|leaves| leaves.is_empty());
                if has_align_declaration
                    || externs.next().is_some()
                    || !path.is_empty()
                    || !result_is_unprotected
                    || extern_.ret != result_ty
                    || !direct_operands_match_modes(
                        &target,
                        &args,
                        &extern_.param_modes,
                        &extern_.params,
                        self.graph.program,
                    )
                    || args.len() != extern_.params.len()
                    || args.iter().zip(&extern_.params).any(|(argument, expected)| {
                        xml_operand_base_ty(self.graph.function, argument) != Some(*expected)
                    })
                {
                    equation.invalid = true;
                    return equation;
                }
                for (argument, expected) in args.iter().zip(&extern_.params) {
                    self.check_whole_operand(&mut equation, argument, *expected);
                }
                equation.seed = Some(XmlAccessProvenance::Owned);
            }
            Rvalue::CallWithCleanup(call) => {
                let Some(facts) = xml_direct_call_facts(
                    self.graph.program,
                    &call.target,
                    self.graph.local_contracts,
                ) else {
                    equation.invalid = true;
                    return equation;
                };
                self.add_call_result(&mut equation, XmlCallResult {
                    result: value,
                    result_ty,
                    selected_ty,
                    args: &call.args,
                    callee: None,
                    facts: &facts,
                    cleanup: Some(call.cleanup),
                    modes_match: direct_operands_match_modes(
                        &call.target,
                        &call.args,
                        &facts.modes,
                        &facts.params,
                        self.graph.program,
                    ),
                });
            }
            Rvalue::CallIndirect {
                callee,
                args,
                param_tys,
                ret_ty,
                signature,
            } => {
                let Some(Ty::Fn(id)) = xml_operand_base_ty(self.graph.function, &callee) else {
                    equation.invalid = true;
                    return equation;
                };
                let facts = XmlCallFacts {
                    params: param_tys,
                    modes: signature.param_modes,
                    ret: ret_ty,
                    borrow: signature.return_borrow,
                    region: signature.return_region,
                    cleanup: signature.return_cleanup,
                };
                if xml_fn_type_facts(self.graph.program, id).as_ref() != Some(&facts) {
                    equation.invalid = true;
                    return equation;
                }
                let callee_source = self.check_source(&callee, Ty::Fn(id));
                Self::add_required_source(
                    &mut equation,
                    callee_source,
                    OperandRequirement {
                        read: true,
                        callable: true,
                        ..OperandRequirement::default()
                    },
                );
                self.add_call_result(&mut equation, XmlCallResult {
                    result: value,
                    result_ty,
                    selected_ty,
                    args: &args,
                    callee: Some(&callee),
                    facts: &facts,
                    cleanup: None,
                    modes_match: operands_match_modes(
                        &args,
                        &facts.modes,
                        &facts.params,
                        self.graph.program,
                    ),
                });
            }
            Rvalue::CallIndirectWithCleanup(call) => {
                let Some(Ty::Fn(id)) = xml_operand_base_ty(self.graph.function, &call.callee) else {
                    equation.invalid = true;
                    return equation;
                };
                let facts = XmlCallFacts {
                    params: call.param_tys,
                    modes: call.signature.param_modes,
                    ret: call.ret_ty,
                    borrow: call.signature.return_borrow,
                    region: call.signature.return_region,
                    cleanup: call.signature.return_cleanup,
                };
                if xml_fn_type_facts(self.graph.program, id).as_ref() != Some(&facts) {
                    equation.invalid = true;
                    return equation;
                }
                let callee_source = self.check_source(&call.callee, Ty::Fn(id));
                Self::add_required_source(
                    &mut equation,
                    callee_source,
                    OperandRequirement {
                        read: true,
                        callable: true,
                        ..OperandRequirement::default()
                    },
                );
                self.add_call_result(&mut equation, XmlCallResult {
                    result: value,
                    result_ty,
                    selected_ty,
                    args: &call.args,
                    callee: Some(&call.callee),
                    facts: &facts,
                    cleanup: Some(call.cleanup),
                    modes_match: operands_match_modes(
                        &call.args,
                        &facts.modes,
                        &facts.params,
                        self.graph.program,
                    ),
                });
            }
            Rvalue::Chunks { src, n, elem } => {
                let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
                let Some(primitive) = align_sema::ty_to_scalar(elem)
                    .and_then(align_sema::scalar_to_prim)
                    .filter(|_| xml_numeric_scalar_ty(elem) || matches!(elem, Ty::Bool | Ty::Char | Ty::Str))
                else {
                    equation.invalid = true;
                    return equation;
                };
                let source_ty = Ty::Slice(align_sema::prim_to_scalar(primitive));
                if result_ty != Ty::DynSliceArray(primitive)
                    || xml_operand_base_ty(self.graph.function, &src) != Some(source_ty)
                    || xml_operand_base_ty(self.graph.function, &n) != Some(i64_ty)
                    || !(path.is_empty() || path.as_slice() == [XmlAccessPathSegment::Element])
                {
                    equation.invalid = true;
                    return equation;
                }
                self.check_operand(&mut equation, &n, i64_ty);
                self.check_whole_operand(&mut equation, &src, source_ty);
                if path.is_empty() {
                    // Only the new array of headers is owned. Its views retain their source.
                    equation.seed = Some(XmlAccessProvenance::Owned);
                } else {
                    self.add_operand(&mut equation, &src, elem, path);
                }
            }
            Rvalue::SubSlice {
                base,
                start,
                len,
                elem,
            } => {
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                let base_ty = xml_operand_base_ty(self.graph.function, &base);
                let relation_matches = match base_ty {
                    Some(Ty::Str | Ty::String) => {
                        result_ty == Ty::Str
                            && elem
                                == Ty::Int(IntTy {
                                    bits: 8,
                                    signed: false,
                                })
                    }
                    Some(Ty::Slice(element) | Ty::DynArray(element)) => {
                        result_ty == Ty::Slice(element) && elem == scalar_to_ty(element)
                    }
                    Some(Ty::Array(element, _)) => {
                        result_ty == Ty::Slice(element) && elem == scalar_to_ty(element)
                    }
                    _ => false,
                };
                let source_path = if path.is_empty() {
                    Some(Vec::new())
                } else if path.starts_with(&[XmlAccessPathSegment::Element]) {
                    Some(path.clone())
                } else {
                    None
                };
                if source_path.is_none()
                    || !relation_matches
                    || xml_operand_base_ty(self.graph.function, &start) != Some(i64_ty)
                    || xml_operand_base_ty(self.graph.function, &len) != Some(i64_ty)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &start, i64_ty);
                    self.check_operand(&mut equation, &len, i64_ty);
                    let Some(base_ty) = base_ty else {
                        equation.invalid = true;
                        return equation;
                    };
                    let source_path = source_path.unwrap_or_default();
                    let Some(source_selected) =
                        xml_selected_ty(self.graph.program, base_ty, &source_path)
                    else {
                        equation.invalid = true;
                        return equation;
                    };
                    if path.is_empty() || source_selected == selected_ty {
                        self.add_read_operand(
                            &mut equation,
                            &base,
                            source_selected,
                            source_path,
                        );
                    } else {
                        equation.invalid = true;
                    }
                }
            }
            Rvalue::SliceLen(input) => {
                let i64_ty = Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                });
                let input_ty = xml_operand_base_ty(self.graph.function, &input);
                let input_is_view = matches!(
                    input_ty,
                    Some(
                        Ty::Str
                            | Ty::String
                            | Ty::Slice(_)
                            | Ty::Array(_, _)
                            | Ty::DynArray(_)
                            | Ty::DynStructArray(_, _)
                            | Ty::DynSliceArray(_)
                            | Ty::Soa(_)
                    )
                );
                if result_ty != i64_ty || !path.is_empty() || !input_is_view {
                    equation.invalid = true;
                } else {
                    let Some(input_ty) = input_ty else {
                        equation.invalid = true;
                        return equation;
                    };
                    self.check_operand(
                        &mut equation,
                        &input,
                        input_ty,
                    );
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::FsReadFile { path: input, out }
            | Rvalue::FsCreatePrivateTempDir { prefix: input, out } => {
                let i32_ty = Ty::Int(IntTy {
                    bits: 32,
                    signed: true,
                });
                if result_ty != i32_ty
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &input) != Some(Ty::Str)
                    || self.graph.function.slots.get(out as usize) != Some(&Ty::String)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &input, Ty::Str);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::StrLit(_) => {
                if result_ty != Ty::Str || !path.is_empty() {
                    equation.invalid = true;
                } else {
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::Template(pieces, arena) => {
                if result_ty != Ty::Str || !path.is_empty() {
                    equation.invalid = true;
                } else {
                    for piece in &pieces {
                        if !self.check_template_piece(&mut equation, piece) {
                            equation.invalid = true;
                        }
                    }
                    if let Some(arena) = &arena {
                        if xml_operand_base_ty(self.graph.function, arena)
                            != Some(Ty::ArenaHandle)
                        {
                            equation.invalid = true;
                        } else {
                            self.check_operand(&mut equation, arena, Ty::ArenaHandle);
                        }
                    }
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::StrTrim { recv, .. } => {
                if result_ty != Ty::Str
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &recv) != Some(Ty::Str)
                {
                    equation.invalid = true;
                } else {
                    self.add_operand(&mut equation, &recv, Ty::Str, Vec::new());
                }
            }
            Rvalue::StrClone(operand) => {
                let source_ty = xml_operand_base_ty(self.graph.function, &operand);
                if result_ty != Ty::String
                    || !path.is_empty()
                    || !matches!(source_ty, Some(Ty::Str | Ty::String))
                {
                    equation.invalid = true;
                } else {
                    let Some(source_ty) = source_ty else {
                        equation.invalid = true;
                        return equation;
                    };
                    self.check_read_operand(&mut equation, &operand, source_ty);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::CloneIn { value, handle } => {
                let source_ty = xml_operand_base_ty(self.graph.function, &value);
                let byte_view = Ty::Slice(Scalar::Int(IntTy {
                    bits: 8,
                    signed: false,
                }));
                let cloneable = matches!(result_ty, Ty::Str) || result_ty == byte_view
                    || align_sema::region_plain_type_ok(
                        result_ty,
                        &self.graph.program.structs,
                        &self.graph.program.enums,
                        &self.graph.program.tagged_types,
                    );
                if source_ty != Some(result_ty)
                    || !cloneable
                    || xml_operand_base_ty(self.graph.function, &handle)
                        != Some(Ty::ArenaHandle)
                {
                    equation.invalid = true;
                } else {
                    self.check_whole_operand(&mut equation, &value, result_ty);
                    let source = self.source(&value, selected_ty, path);
                    Self::add_required_source(
                        &mut equation,
                        source,
                        OperandRequirement::READ,
                    );
                    self.check_operand(&mut equation, &handle, Ty::ArenaHandle);
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::ArrayBuilderNew { elem, region } => {
                let element = xml_selected_ty(
                    self.graph.program,
                    result_ty,
                    &[XmlAccessPathSegment::Element],
                );
                let builder_shape = xml_array_builder_output(result_ty).is_some();
                let region_valid = region.as_ref().is_none_or(|region| {
                    xml_operand_base_ty(self.graph.function, region) == Some(Ty::ArenaHandle)
                });
                if !builder_shape || element != Some(elem) || !region_valid {
                    equation.invalid = true;
                } else {
                    if let Some(region) = &region {
                        self.check_operand(&mut equation, region, Ty::ArenaHandle);
                    }
                    equation.seed = Some(if path.is_empty() || region.is_none() {
                        XmlAccessProvenance::Owned
                    } else {
                        XmlAccessProvenance::Shared
                    });
                }
            }
            Rvalue::ArrayBuilderBuild { builder } => {
                let Some(builder_ty) = xml_operand_base_ty(self.graph.function, &builder) else {
                    equation.invalid = true;
                    return equation;
                };
                if xml_array_builder_output(builder_ty) != Some(result_ty) {
                    equation.invalid = true;
                } else {
                    self.check_whole_operand(&mut equation, &builder, builder_ty);
                    if path.is_empty() {
                        self.add_operand(&mut equation, &builder, builder_ty, Vec::new());
                    } else {
                        self.add_operand(&mut equation, &builder, selected_ty, path);
                    }
                }
            }
            Rvalue::PathJoin { a, b } => {
                if result_ty != Ty::String
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &a) != Some(Ty::Str)
                    || xml_operand_base_ty(self.graph.function, &b) != Some(Ty::Str)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &a, Ty::Str);
                    self.check_operand(&mut equation, &b, Ty::Str);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::PathNormalize { path: source } => {
                if result_ty != Ty::String
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &source) != Some(Ty::Str)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &source, Ty::Str);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::EncodingEncode { data, .. } => {
                let data_ty = xml_operand_base_ty(self.graph.function, &data);
                let byte_view = matches!(
                    data_ty,
                    Some(Ty::Str | Ty::String | Ty::Slice(Scalar::Int(IntTy {
                        bits: 8,
                        signed: false,
                    })))
                );
                if result_ty != Ty::String || !path.is_empty() || !byte_view {
                    equation.invalid = true;
                } else {
                    let Some(data_ty) = data_ty else {
                        equation.invalid = true;
                        return equation;
                    };
                    self.check_operand(
                        &mut equation,
                        &data,
                        data_ty,
                    );
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::RegexReplace {
                regex, text, repl, ..
            } => {
                if result_ty != Ty::String
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &regex) != Some(Ty::Regex)
                    || xml_operand_base_ty(self.graph.function, &text) != Some(Ty::Str)
                    || xml_operand_base_ty(self.graph.function, &repl) != Some(Ty::Str)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &regex, Ty::Regex);
                    self.check_operand(&mut equation, &text, Ty::Str);
                    self.check_operand(&mut equation, &repl, Ty::Str);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::CliUsage { cmd } => {
                if result_ty != Ty::String
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &cmd) != Some(Ty::CliCommand)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &cmd, Ty::CliCommand);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::StaticData(_) => {
                if result_ty != Ty::Raw || !path.is_empty() {
                    equation.invalid = true;
                } else {
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::StaticDescriptorView { ptr, .. } => {
                if result_ty != Ty::Str
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &ptr) != Some(Ty::Raw)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &ptr, Ty::Raw);
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::PathComponent { path: source, .. } => {
                if result_ty != Ty::Str
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &source) != Some(Ty::Str)
                {
                    equation.invalid = true;
                } else {
                    self.add_operand(&mut equation, &source, Ty::Str, Vec::new());
                }
            }
            Rvalue::CliGetStr { parsed, name } => {
                if result_ty != Ty::Str
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &parsed) != Some(Ty::CliParsed)
                    || xml_operand_base_ty(self.graph.function, &name) != Some(Ty::Str)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &parsed, Ty::CliParsed);
                    self.check_operand(&mut equation, &name, Ty::Str);
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::RunOutputView { out, .. } => {
                if result_ty != Ty::Str
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &out) != Some(Ty::RunOutput)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &out, Ty::RunOutput);
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::HttpSseStreamLastEventId { stream } => {
                if result_ty != Ty::Str
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &stream)
                        != Some(Ty::HttpSseStream)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &stream, Ty::HttpSseStream);
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::HttpCtxMethod { ctx } | Rvalue::HttpCtxPath { ctx } => {
                if result_ty != Ty::Str
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &ctx)
                        != Some(Ty::HttpRequestCtx)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &ctx, Ty::HttpRequestCtx);
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::BuilderToString(builder) => {
                if result_ty != Ty::String
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &builder) != Some(Ty::Builder)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &builder, Ty::Builder);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::BuilderNew { capacity } => {
                let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
                if result_ty != Ty::Builder
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &capacity) != Some(i64_ty)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &capacity, i64_ty);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::TemplateHtmlToString { resource, output } => {
                if result_ty != Ty::String
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &output)
                        != Some(Ty::Resource(resource))
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(
                        &mut equation,
                        &output,
                        Ty::Resource(resource),
                    );
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::TemplateHtmlNew { resource } => {
                if result_ty != Ty::Resource(resource)
                    || !path.is_empty()
                    || self.graph.program.resources.get(resource as usize).is_none()
                {
                    equation.invalid = true;
                } else {
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::XmlParse {
                input,
                error_enum,
                cleanup,
            } => {
                let valid = result_ty
                    == Ty::Result(Scalar::XmlReader, Scalar::Enum(error_enum))
                    && xml_operand_base_ty(self.graph.function, &input) == Some(Ty::String)
                    && cleanup != value
                    && self.graph.function.value_tys.get(cleanup as usize) == Some(&Ty::Bool)
                    && self.graph.primary_definitions.get(cleanup as usize) == Some(&0)
                    && self.graph.auxiliary_definitions.get(cleanup as usize) == Some(&1);
                if !valid {
                    equation.invalid = true;
                    return equation;
                }
                let input = self.check_source(&input, Ty::String);
                Self::add_required_source(
                    &mut equation,
                    input,
                    OperandRequirement {
                        read: true,
                        move_value: true,
                        ..OperandRequirement::default()
                    },
                );
                let whole_result = path.is_empty() && selected_ty == result_ty;
                let reader_payload = path.as_slice() == [XmlAccessPathSegment::ResultOk]
                    && selected_ty == Ty::XmlReader;
                let error_payload = path.as_slice() == [XmlAccessPathSegment::ResultErr]
                    && selected_ty == Ty::Enum(error_enum);
                if whole_result || reader_payload || error_payload {
                    equation.seed = Some(XmlAccessProvenance::Owned);
                } else {
                    equation.invalid = true;
                }
            }
            Rvalue::XmlAttributeValue { reader, index } => {
                let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
                if result_ty != Ty::String
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &reader) != Some(Ty::XmlReader)
                    || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &reader, Ty::XmlReader);
                    self.check_operand(&mut equation, &index, i64_ty);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::XmlText { reader } => {
                if result_ty != Ty::String
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &reader) != Some(Ty::XmlReader)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &reader, Ty::XmlReader);
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::XmlAttributeCount(reader) => {
                let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
                if result_ty != i64_ty
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &reader) != Some(Ty::XmlReader)
                {
                    equation.invalid = true;
                } else {
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::XmlNext { reader, event_enum } => {
                if result_ty != Ty::Option(Scalar::Enum(event_enum))
                    || !xml_event_definition_valid(self.graph.program, event_enum)
                    || !(path.is_empty() || path.as_slice() == [XmlAccessPathSegment::OptionSome])
                {
                    equation.invalid = true;
                } else {
                    let source = self.check_source(&reader, Ty::XmlReader);
                    Self::add_required_source(&mut equation, source,
                        xml_mode_requirement(self.graph.program, Ty::XmlReader, align_ast::ParamMode::BorrowMut));
                    equation.seed = Some(XmlAccessProvenance::Owned);
                }
            }
            Rvalue::CodecBatchName(batch, index) => {
                if result_ty != Ty::Option(Scalar::Str)
                    || !(path.is_empty() || path.as_slice() == [XmlAccessPathSegment::OptionSome])
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &batch, Ty::CodecBatch);
                    self.check_operand(&mut equation, &index, Ty::Int(IntTy { bits: 64, signed: true }));
                    equation.seed = Some(XmlAccessProvenance::Shared);
                }
            }
            Rvalue::CodecColumnAt { column, index, kind } => {
                let (column_ty, scalar) = match kind {
                    hir::CodecPutKind::I64 => (Ty::CodecI64Column, Scalar::Int(IntTy { bits: 64, signed: true })),
                    hir::CodecPutKind::F64 => (Ty::CodecF64Column, Scalar::Float(FloatTy { bits: 64 })),
                    hir::CodecPutKind::Bool => (Ty::CodecBoolColumn, Scalar::Bool),
                    hir::CodecPutKind::Str => (Ty::CodecStrColumn, Scalar::Str),
                };
                if result_ty != Ty::Option(scalar)
                    || !(path.is_empty() || path.as_slice() == [XmlAccessPathSegment::OptionSome])
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &column, column_ty);
                    self.check_operand(&mut equation, &index, Ty::Int(IntTy { bits: 64, signed: true }));
                    equation.seed = Some(if scalar == Scalar::Str { XmlAccessProvenance::Shared } else { XmlAccessProvenance::Owned });
                }
            }
            Rvalue::XmlName { reader } => {
                if result_ty != Ty::Str
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &reader) != Some(Ty::XmlReader)
                {
                    equation.invalid = true;
                } else {
                    self.add_operand(
                        &mut equation,
                        &reader,
                        Ty::XmlReader,
                        Vec::new(),
                    );
                }
            }
            Rvalue::XmlAttributeName { reader, index } => {
                let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
                if result_ty != Ty::Str
                    || !path.is_empty()
                    || xml_operand_base_ty(self.graph.function, &reader) != Some(Ty::XmlReader)
                    || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                {
                    equation.invalid = true;
                } else {
                    self.check_operand(&mut equation, &index, i64_ty);
                    self.add_operand(
                        &mut equation,
                        &reader,
                        Ty::XmlReader,
                        Vec::new(),
                    );
                }
            }
            Rvalue::Un(operation, operand) => {
                let operation_matches = match operation {
                    UnOp::Neg => xml_numeric_scalar_ty(result_ty),
                    UnOp::Not => result_ty == Ty::Bool,
                    UnOp::BitNot => {
                        matches!(result_ty, Ty::Int(_)) && xml_numeric_scalar_ty(result_ty)
                    }
                };
                if !path.is_empty()
                    || !operation_matches
                    || xml_operand_base_ty(self.graph.function, &operand) != Some(result_ty)
                {
                    equation.invalid = true;
                } else {
                    self.add_operand(&mut equation, &operand, result_ty, Vec::new());
                }
            }
            Rvalue::Cast { operand, from, to } => {
                if !path.is_empty()
                    || result_ty != to
                    || xml_operand_base_ty(self.graph.function, &operand) != Some(from)
                    || align_sema::ty_is_move(result_ty, &self.graph.program.structs, &self.graph.program.tuples, &self.graph.program.enums, &self.graph.program.tagged_types)
                {
                    equation.invalid = true;
                } else {
                    self.add_operand(&mut equation, &operand, from, Vec::new());
                }
            }
            Rvalue::Bin(operation, left, right) => {
                let left_ty = xml_operand_base_ty(self.graph.function, &left);
                let right_ty = xml_operand_base_ty(self.graph.function, &right);
                // Sema and gen_bin retain the scalar operand for broadcasts in either order.
                // The vector fixes both the numeric element and lane count; masks are results
                // of comparisons only, never numeric operands or bitwise/logical vectors.
                if matches!(left_ty, Some(Ty::Vec(..)))
                    || matches!(right_ty, Some(Ty::Vec(..)))
                {
                    let vector = left_ty.and_then(xml_numeric_vector_shape)
                        .or_else(|| right_ty.and_then(xml_numeric_vector_shape));
                    let relation_matches = vector.is_some_and(|(element, lanes)| {
                        let vector_ty = Ty::Vec(element, lanes);
                        let scalar_ty = scalar_to_ty(element);
                        let operand_matches = |ty| ty == Some(vector_ty) || ty == Some(scalar_ty);
                        operand_matches(left_ty)
                            && operand_matches(right_ty)
                            && match operation {
                                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                                    result_ty == vector_ty
                                }
                                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                                    result_ty == Ty::Mask(element, lanes)
                                }
                                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor
                                | BinOp::Shl | BinOp::Shr | BinOp::And | BinOp::Or => false,
                            }
                    });
                    if !path.is_empty() || !relation_matches {
                        equation.invalid = true;
                    } else if let (Some(left_ty), Some(right_ty)) = (left_ty, right_ty) {
                        self.add_operand(&mut equation, &left, left_ty, Vec::new());
                        self.add_operand(&mut equation, &right, right_ty, Vec::new());
                    }
                    return equation;
                }
                let operand_ty = left_ty.filter(|left_ty| Some(*left_ty) == right_ty);
                let relation_matches = match operation {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                        operand_ty == Some(result_ty)
                            && xml_numeric_scalar_ty(result_ty)
                    }
                    BinOp::BitAnd
                    | BinOp::BitOr
                    | BinOp::BitXor
                    | BinOp::Shl
                    | BinOp::Shr => {
                        operand_ty == Some(result_ty)
                            && matches!(result_ty, Ty::Int(_))
                            && xml_numeric_scalar_ty(result_ty)
                    }
                    BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge => {
                        result_ty == Ty::Bool
                            && operand_ty.is_some_and(|ty| {
                                xml_numeric_scalar_ty(ty)
                                    || matches!(ty, Ty::Char | Ty::Str)
                                    || (ty == Ty::Bool && matches!(operation, BinOp::Eq | BinOp::Ne))
                            })
                    }
                    BinOp::And | BinOp::Or => {
                        result_ty == Ty::Bool && operand_ty == Some(Ty::Bool)
                    }
                };
                if !path.is_empty()
                    || !relation_matches
                {
                    equation.invalid = true;
                } else {
                    let Some(operand_ty) = operand_ty else {
                        equation.invalid = true;
                        return equation;
                    };
                    self.add_operand(&mut equation, &left, operand_ty, Vec::new());
                    self.add_operand(&mut equation, &right, operand_ty, Vec::new());
                }
            }
            Rvalue::IntArith {
                mode, int_ty, a, b, ..
            } => {
                let (payload_path, relation_matches) = match mode {
                    align_sema::ArithMode::Saturating => (Vec::new(), result_ty == int_ty),
                    align_sema::ArithMode::Checked => (
                        vec![XmlAccessPathSegment::OptionSome],
                        xml_option_payload(self.graph.program, result_ty) == Some(int_ty),
                    ),
                };
                if !relation_matches
                    || !(path.is_empty() || path == payload_path)
                    || xml_operand_base_ty(self.graph.function, &a) != Some(int_ty)
                    || xml_operand_base_ty(self.graph.function, &b) != Some(int_ty)
                {
                    equation.invalid = true;
                } else {
                    self.add_operand(&mut equation, &a, int_ty, Vec::new());
                    self.add_operand(&mut equation, &b, int_ty, Vec::new());
                }
            }
            Rvalue::MathOp { ty, operands, .. } => {
                if !path.is_empty()
                    || result_ty != ty
                    || operands.is_empty()
                    || operands
                        .iter()
                        .any(|operand| xml_operand_base_ty(self.graph.function, operand) != Some(ty))
                {
                    equation.invalid = true;
                } else {
                    for operand in &operands {
                        self.add_operand(&mut equation, operand, ty, Vec::new());
                    }
                }
            }
            // Exhaustive non-protected producers. Keeping this in the equation match
            // makes a missing semantic producer arm a compiler error, not a fallback.
            Rvalue::SqliteCallbackDescriptor(..)
            | Rvalue::ArenaBegin
            | Rvalue::TgBegin
            | Rvalue::SpawnTask { .. }
            | Rvalue::TgWaitResult { .. }
            | Rvalue::HeapAlloc(..)
            | Rvalue::RawAlloc(..)
            | Rvalue::ColumnBatchCreate { .. }
            | Rvalue::ColumnBatchAppend { .. }
            | Rvalue::RawNull
            | Rvalue::RawLoad { .. }
            | Rvalue::RawPointerLoad { .. }
            | Rvalue::RawOffset { .. }
            | Rvalue::RawIsNull(..)
            | Rvalue::ResourceFromRaw { .. }
            | Rvalue::ResourceBorrow { .. }
            | Rvalue::ResourceRaw { .. }
            | Rvalue::ResourceIntoRaw { .. }
            | Rvalue::BoxGet(..)
            | Rvalue::BoxClone(..)
            | Rvalue::MakeVec { .. }
            | Rvalue::VecExtract { .. }
            | Rvalue::VecInsert { .. }
            | Rvalue::VecSumWhere { .. }
            | Rvalue::VecDot { .. }
            | Rvalue::VecMinMax { .. }
            | Rvalue::VecSum { .. }
            | Rvalue::MaskAny { .. }
            | Rvalue::VecLoad { .. }
            | Rvalue::ParMapParallel { .. }
            | Rvalue::ParMapReduce { .. }
            | Rvalue::SlicePtr(..)
            | Rvalue::StrPredicate { .. }
            | Rvalue::StrFinderNew { .. }
            | Rvalue::StrFinderFind { .. }
            | Rvalue::BuilderWriteStr(..)
            | Rvalue::BuilderWriteInt(..)
            | Rvalue::BuilderWriteBool(..)
            | Rvalue::BuilderWriteChar(..)
            | Rvalue::BuilderWriteFloat(..)
            | Rvalue::BuilderWriteStrIntStr(..)
            | Rvalue::TemplateHtmlWrite { .. }
            | Rvalue::TemplateHtmlRaw { .. }
            | Rvalue::JsonEncode { .. }
            | Rvalue::JsonDecode { .. }
            | Rvalue::JsonOwnedDecode { .. }
            | Rvalue::JsonDecodeArray { .. }
            | Rvalue::JsonDecodeScalar { .. }
            | Rvalue::JsonDecodeStructArray { .. }
            | Rvalue::JsonDecodeSoa { .. }
            | Rvalue::CsvDecode { .. }
            | Rvalue::JsonDecodeUnion { .. }
            | Rvalue::JsonDoc { .. }
            | Rvalue::JsonDocKind { .. }
            | Rvalue::JsonDocGet { .. }
            | Rvalue::JsonDocAt { .. }
            | Rvalue::JsonDocAsStr { .. }
            | Rvalue::JsonDocAsScalar { .. }
            | Rvalue::JsonDocLen { .. }
            | Rvalue::JsonDocKey { .. }
            | Rvalue::JsonDocElems { .. }
            | Rvalue::JsonScanNew { .. }
            | Rvalue::JsonScanNext { .. }
            | Rvalue::ReaderOpen { .. }
            | Rvalue::ReaderOpenBeneath { .. }
            | Rvalue::ReaderOpenBeneathSingleLink { .. }
            | Rvalue::WriterCreate { .. }
            | Rvalue::WriterCreateExclusive { .. }
            | Rvalue::WriterCreateExclusiveBeneath { .. }
            | Rvalue::ReaderStdin
            | Rvalue::WriterStd { .. }
            | Rvalue::ReaderRead(..)
            | Rvalue::ReaderBuffered(..)
            | Rvalue::ReaderReadLine(..)
            | Rvalue::BytesAsStr { .. }
            | Rvalue::WriterWrite(..)
            | Rvalue::WriterWriteBuilder(..)
            | Rvalue::WriterFlush(..)
            | Rvalue::LogNew(..)
            | Rvalue::LogEnabled(..)
            | Rvalue::LogLine(..)
            | Rvalue::LogLineBuilder(..)
            | Rvalue::LogFlush(..)
            | Rvalue::CodecOpen(..)
            | Rvalue::CodecBatchRows(..)
            | Rvalue::CodecBatchColumns(..)
            | Rvalue::CodecBatchKind(..)
            | Rvalue::CodecBatchFind(..)
            | Rvalue::CodecBatchColumn { .. }
            | Rvalue::CodecColumnLen(..)
            | Rvalue::CodecEncoderNew { .. }
            | Rvalue::CodecEncoderPut { .. }
            | Rvalue::CodecEncoderFinish(..)
            | Rvalue::OsHost { .. }
            | Rvalue::OsIdentity { .. }
            | Rvalue::FrameInnerJoin { .. }
            | Rvalue::IoCopy(..)
            | Rvalue::FileCreateRw { .. }
            | Rvalue::FileOpenRw { .. }
            | Rvalue::FilePread { .. }
            | Rvalue::FilePwrite { .. }
            | Rvalue::FileLen { .. }
            | Rvalue::BufferNew(..)
            | Rvalue::BufferBytes(..)
            | Rvalue::BufferLen(..)
            | Rvalue::BufferCapacity(..)
            | Rvalue::BytesRead { .. }
            | Rvalue::BufferPut { .. }
            | Rvalue::BufferAppend { .. }
            | Rvalue::ArrayBuilderPush { .. }
            | Rvalue::ArrayBuilderPushStr { .. }
            | Rvalue::ArrayBuilderAppend { .. }
            | Rvalue::FsWriteFile { .. }
            | Rvalue::FsWriteFileBuilder { .. }
            | Rvalue::FsExists { .. }
            | Rvalue::FsRemove { .. }
            | Rvalue::FsCreateDir { .. } | Rvalue::FsIsDir { .. } | Rvalue::FsTree { .. } | Rvalue::ProcessLive { .. }
            | Rvalue::FsRemoveEmptyDir { .. }
            | Rvalue::RenameNoReplace { .. }
            | Rvalue::FsReadDir { .. }
            | Rvalue::DnsResolve { .. }
            | Rvalue::TcpConnect { .. }
            | Rvalue::ConnReader(..)
            | Rvalue::ConnWriter(..)
            | Rvalue::TcpReadTimeout { .. }
            | Rvalue::TcpWriteTimeout { .. }
            | Rvalue::TcpListen { .. }
            | Rvalue::TcpAccept { .. }
            | Rvalue::UdpBind { .. }
            | Rvalue::UdpSendTo { .. }
            | Rvalue::UdpRecvFrom { .. }
            | Rvalue::ProcessSpawn { .. }
            | Rvalue::ChildWait { .. }
            | Rvalue::ChildKill { .. }
            | Rvalue::ProcessExec { .. }
            | Rvalue::FsReadFileView { .. }
            | Rvalue::FsReadBytesView { .. }
            | Rvalue::EnvGet { .. }
            | Rvalue::EnvSet { .. }
            | Rvalue::TimeNow
            | Rvalue::ProcessCpuCount
            | Rvalue::TimeInstant
            | Rvalue::TimeSleep { .. }
            | Rvalue::RegexCompile { .. }
            | Rvalue::RegexIsMatch { .. }
            | Rvalue::RegexFind { .. }
            | Rvalue::RegexFindAll { .. }
            | Rvalue::RegexSplit { .. }
            | Rvalue::RegexCaptures { .. }
            | Rvalue::RegexGroupCount { .. }
            | Rvalue::RegexGroupIndex { .. }
            | Rvalue::CapturesGroup { .. }
            | Rvalue::TimeFormat { .. }
            | Rvalue::TimeParse { .. }
            | Rvalue::EncodingDecode { .. }
            | Rvalue::CompressCompress { .. }
            | Rvalue::CompressDecompress { .. }
            | Rvalue::Utf8Valid { .. }
            | Rvalue::CryptoCtEqual { .. }
            | Rvalue::CryptoRandom { .. }
            | Rvalue::CryptoDigestNew | Rvalue::CryptoDigestUpdate { .. } | Rvalue::CryptoDigestFinish(_)
            | Rvalue::CryptoHash { .. }
            | Rvalue::CryptoHmac { .. }
            | Rvalue::CryptoHkdf { .. }
            | Rvalue::CryptoAead { .. }
            | Rvalue::CryptoArgon2(..)
            | Rvalue::CryptoPrivateKeyFromPem { .. }
            | Rvalue::CryptoPublicKeyFromPem { .. }
            | Rvalue::CryptoPublicKeyFromJwk(..)
            | Rvalue::CryptoSign { .. }
            | Rvalue::CryptoVerify(..)
            | Rvalue::RandSeed { .. }
            | Rvalue::RandNext { .. }
            | Rvalue::RandRange { .. }
            | Rvalue::RandShuffle { .. }
            | Rvalue::RandSample { .. }
            | Rvalue::CliCommand { .. }
            | Rvalue::CliFlag { .. }
            | Rvalue::CliParse { .. }
            | Rvalue::CliGetBool { .. }
            | Rvalue::CliGetI64 { .. }
            | Rvalue::HttpRequest { .. }
            | Rvalue::HttpHeader { .. }
            | Rvalue::HttpBody { .. }
            | Rvalue::HttpRequestTimeout { .. }
            | Rvalue::HttpRequestMaxResponseBodyBytes { .. }
            | Rvalue::HttpClientTimeout { .. }
            | Rvalue::HttpClientMaxResponseBodyBytes { .. }
            | Rvalue::Command { .. }
            | Rvalue::CommandCwd { .. }
            | Rvalue::CommandTimeout { .. }
            | Rvalue::CommandMaxCapture { .. }
            | Rvalue::CommandEnv { .. }
            | Rvalue::CommandEnvClear { .. }
            | Rvalue::CommandRun { .. }
            | Rvalue::CommandRunBytes { .. }
            | Rvalue::RunBytesView { .. }
            | Rvalue::HttpParse { .. }
            | Rvalue::HttpRespStatus { .. }
            | Rvalue::HttpRespHeader { .. }
            | Rvalue::HttpRespBody { .. }
            | Rvalue::HttpClient
            | Rvalue::HttpClientGet { .. }
            | Rvalue::HttpClientPost { .. }
            | Rvalue::HttpClientRequest { .. }
            | Rvalue::HttpClientRequestStream { .. }
            | Rvalue::HttpReadStreamStatus { .. }
            | Rvalue::HttpReadStreamHeader { .. }
            | Rvalue::HttpReadStreamRead { .. }
            | Rvalue::HttpReadStreamSse { .. }
            | Rvalue::HttpSseStreamRetryMs { .. }
            | Rvalue::HttpSseStreamNext { .. }
            | Rvalue::HttpGetMany { .. }
            | Rvalue::HttpServe { .. }
            | Rvalue::HttpAccept { .. }
            | Rvalue::HttpCtxHeader { .. }
            | Rvalue::HttpHeadersCount { .. }
            | Rvalue::HttpHeadersTokensValid { .. }
            | Rvalue::HttpHeadersContainsToken { .. }
            | Rvalue::HttpCtxUpgradeReady { .. }
            | Rvalue::HttpCtxBody { .. }
            | Rvalue::HttpResponseBuilder { .. }
            | Rvalue::HttpRbHeader { .. }
            | Rvalue::HttpRbBody { .. }
            | Rvalue::HttpRespond { .. }
            | Rvalue::HttpRespondStream { .. }
            | Rvalue::HttpRespondUpgrade { .. }
            | Rvalue::HttpUpgradeReadExact { .. }
            | Rvalue::HttpUpgradeWrite { .. }
            | Rvalue::HttpUpgradeDeadline { .. }
            | Rvalue::HttpUpgradeShutdown { .. }
            | Rvalue::HttpStreamSend { .. }
            | Rvalue::HttpStreamFinish { .. }
            | Rvalue::HttpStreamReject { .. } => {
                if xml_owned_leaf_paths(self.graph.program, result_ty)
                    .is_some_and(|leaves| leaves.is_empty())
                {
                    equation.seed = Some(XmlAccessProvenance::Owned);
                } else {
                    equation.invalid = true;
                }
            }
        }
        equation
    }

    fn add_out_producer(
        &mut self,
        equation: &mut XmlAccessEquation,
        slot: Slot,
        slot_ty: Ty,
        selected_ty: Ty,
        path: &[XmlAccessPathSegment],
        producer: (ValueId, &Rvalue),
    ) {
        let (value, rvalue) = producer;
        let i64_ty = Ty::Int(IntTy {
            bits: 64,
            signed: true,
        });
        let bytes_ty = Ty::Slice(Scalar::Int(IntTy {
            bits: 8,
            signed: false,
        }));
        let native_contract = native_owner_mir_contract(self.graph.program, self.graph.function, rvalue);
        // Native value and out-slot queries must use the same instruction ABI. An
        // infallible scratch writer returns Unit, not an errno-status integer.
        let expected_result = native_contract
            .as_ref()
            .map(|contract| contract.result)
            .or_else(|| xml_out_producer_result_ty(rvalue));
        let result_is_unique = expected_result.is_some()
            && self.graph.function.value_tys.get(value as usize) == expected_result.as_ref()
            && self.graph.primary_definitions.get(value as usize) == Some(&1)
            && self.graph.auxiliary_definitions.get(value as usize) == Some(&0)
            && !self
                .graph
                .duplicate_values
                .get(value as usize)
                .copied()
                .unwrap_or(true)
            && self
                .graph
                .value_definitions
                .get(value as usize)
                .is_some_and(|definition| {
                    definition.is_some_and(|candidate| std::ptr::eq(candidate, rvalue))
                });
        let Some(recorded_access) = xml_written_slots(rvalue)
            .into_iter()
            .find_map(|(written, access)| (written == slot).then_some(access))
        else {
            equation.invalid = true;
            return;
        };
        if !result_is_unique
            || xml_selected_ty(self.graph.program, slot_ty, path) != Some(selected_ty)
        {
            equation.invalid = true;
            return;
        }

        if let Some(contract) = native_contract {
            // The exact output type and selected path were authenticated above.
            // Structural projections follow that type, not a second opcode list;
            // an opaque handle cannot acquire fields through this path.
            if !contract.outputs.contains(&(slot, slot_ty))
                || contract.access != recorded_access {
                equation.invalid = true;
            }
            for (operand, expected, requirement) in contract.operands {
                if requirement.move_value && !matches!(operand, Operand::Value(_) | Operand::Arg(_)) {
                    equation.invalid = true;
                }
                let source = self.read_source(equation, operand, expected, Vec::new());
                Self::add_required_source(equation, source, requirement);
            }
            for operand in contract.writable_buffers {
                let source = self.buffer_source(operand, Vec::new());
                Self::add_required_source(equation, source, OperandRequirement {
                    write: true, exclusive: true, ..OperandRequirement::default()
                });
            }
            equation.seed = merge_xml_access(equation.seed, contract.access);
            return;
        }

        // Only the authenticated client-family operations above can produce these owners.
        // A foreign native opcode cannot claim one merely by naming a pointer-sized out slot,
        // including an HTTP leaf selected inside an otherwise valid aggregate decoder result.
        if http_client_owner_leaf(selected_ty) {
            equation.invalid = true;
            return;
        }

        let (expected_out, access) = match rvalue {
            Rvalue::JsonEncode {
                pieces,
                max_bytes,
                out,
            } => {
                if slot_ty != Ty::String
                    || max_bytes.as_ref().is_some_and(|limit| xml_operand_base_ty(self.graph.function, limit) != Some(i64_ty))
                {
                    equation.invalid = true;
                    return;
                }
                if let Some(limit) = max_bytes { self.check_operand(equation, limit, i64_ty); }
                for piece in pieces {
                    if !self.check_template_piece(equation, piece) {
                        equation.invalid = true;
                    }
                }
                (*out, XmlAccessProvenance::Owned)
            }
            Rvalue::JsonDecode {
                struct_id,
                input,
                out,
                arena,
            } => {
                if slot_ty != Ty::Struct(*struct_id)
                    || self
                        .graph
                        .program
                        .structs
                        .get(*struct_id as usize)
                        .is_none()
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                    || arena.as_ref().is_some_and(|arena| {
                        xml_operand_base_ty(self.graph.function, arena) != Some(Ty::ArenaHandle)
                    })
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                if let Some(arena) = arena {
                    self.check_operand(equation, arena, Ty::ArenaHandle);
                }
                (
                    *out,
                    if arena.is_some() {
                        XmlAccessProvenance::Shared
                    } else {
                        XmlAccessProvenance::Owned
                    },
                )
            }
            Rvalue::JsonOwnedDecode { plan, input, out } => {
                let canonical = align_sema::owned_json_graph_plan_v3(
                        &self.graph.program.structs,
                        plan.root,
                    )
                    .is_ok_and(|canonical| canonical == *plan);
                if slot_ty != Ty::Struct(plan.root)
                    || !canonical
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                (*out, XmlAccessProvenance::Owned)
            }
            Rvalue::JsonDecodeArray { elem, input, out } => {
                let Some(scalar) = align_sema::ty_to_scalar(*elem)
                    .filter(|_| matches!(elem, Ty::Int(_) | Ty::Float(_) | Ty::Bool))
                else {
                    equation.invalid = true;
                    return;
                };
                if slot_ty != Ty::DynArray(scalar)
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                (*out, XmlAccessProvenance::Owned)
            }
            Rvalue::JsonDecodeScalar { scalar, input, out } => {
                if slot_ty != *scalar
                    || !matches!(scalar, Ty::Int(_) | Ty::Float(_) | Ty::Bool)
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                (*out, XmlAccessProvenance::Owned)
            }
            Rvalue::JsonDecodeStructArray {
                struct_id,
                input,
                out,
                arena,
            } => {
                if slot_ty != Ty::DynStructArray(*struct_id, align_sema::Layout::Aos)
                    || self
                        .graph
                        .program
                        .structs
                        .get(*struct_id as usize)
                        .is_none()
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                    || arena.as_ref().is_some_and(|arena| {
                        xml_operand_base_ty(self.graph.function, arena) != Some(Ty::ArenaHandle)
                    })
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                if let Some(arena) = arena {
                    self.check_operand(equation, arena, Ty::ArenaHandle);
                }
                (
                    *out,
                    if arena.is_some() {
                        XmlAccessProvenance::Shared
                    } else {
                        XmlAccessProvenance::Owned
                    },
                )
            }
            Rvalue::JsonDecodeSoa {
                struct_id,
                input,
                out,
                arena,
            } => {
                if slot_ty != Ty::Soa(*struct_id)
                    || self
                        .graph
                        .program
                        .structs
                        .get(*struct_id as usize)
                        .is_none()
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                    || xml_operand_base_ty(self.graph.function, arena) != Some(Ty::ArenaHandle)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                self.check_operand(equation, arena, Ty::ArenaHandle);
                (*out, XmlAccessProvenance::Shared)
            }
            Rvalue::CsvDecode {
                struct_id,
                options_struct_id,
                input,
                arena,
                options,
                out,
            } => {
                if slot_ty != Ty::Soa(*struct_id)
                    || self
                        .graph
                        .program
                        .structs
                        .get(*struct_id as usize)
                        .is_none()
                    || self
                        .graph
                        .program
                        .structs
                        .get(*options_struct_id as usize)
                        .is_none()
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                    || xml_operand_base_ty(self.graph.function, arena) != Some(Ty::ArenaHandle)
                    || xml_operand_base_ty(self.graph.function, options)
                        != Some(Ty::Struct(*options_struct_id))
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                self.check_operand(equation, arena, Ty::ArenaHandle);
                self.check_whole_operand(equation, options, Ty::Struct(*options_struct_id));
                (*out, XmlAccessProvenance::Shared)
            }
            Rvalue::JsonDecodeUnion {
                enum_id,
                input,
                out,
                arena,
            } => {
                if slot_ty != Ty::Enum(*enum_id)
                    || self.graph.program.enums.get(*enum_id as usize).is_none()
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                    || arena.as_ref().is_some_and(|arena| {
                        xml_operand_base_ty(self.graph.function, arena) != Some(Ty::ArenaHandle)
                    })
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                if let Some(arena) = arena {
                    self.check_operand(equation, arena, Ty::ArenaHandle);
                }
                (
                    *out,
                    if arena.is_some() {
                        XmlAccessProvenance::Shared
                    } else {
                        XmlAccessProvenance::Owned
                    },
                )
            }
            Rvalue::JsonScanNext {
                scanner,
                struct_id,
                cursor,
                row,
            } => {
                if slot != *row
                    || slot_ty != Ty::StructArray(*struct_id, 1)
                    || self
                        .graph
                        .program
                        .structs
                        .get(*struct_id as usize)
                        .is_none()
                    || xml_operand_base_ty(self.graph.function, scanner)
                        != Some(Ty::JsonScanner(*struct_id))
                    || self.graph.function.slots.get(*cursor as usize) != Some(&i64_ty)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, scanner, Ty::JsonScanner(*struct_id));
                (*row, XmlAccessProvenance::Shared)
            }
            Rvalue::FsReadFile { path: input, out }
            | Rvalue::FsCreatePrivateTempDir { prefix: input, out } => {
                if slot_ty != Ty::String
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                (*out, XmlAccessProvenance::Owned)
            }
            Rvalue::TimeFormat { ns, out, .. } => {
                if slot_ty != Ty::String || xml_operand_base_ty(self.graph.function, ns) != Some(i64_ty) {
                    equation.invalid = true;
                    return;
                }
                self.check_read_operand(equation, ns, i64_ty);
                (*out, XmlAccessProvenance::Owned)
            }
            Rvalue::TimeParse { input, out, .. } => {
                if slot_ty != i64_ty || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str) {
                    equation.invalid = true;
                    return;
                }
                self.check_read_operand(equation, input, Ty::Str);
                (*out, XmlAccessProvenance::Owned)
            }
            Rvalue::EnvGet { name, out } => {
                if slot_ty != Ty::String
                    || xml_operand_base_ty(self.graph.function, name) != Some(Ty::Str)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_read_operand(equation, name, Ty::Str);
                (*out, XmlAccessProvenance::Owned)
            }
            Rvalue::FsReadDir { path: input, out } | Rvalue::DnsResolve { host: input, out } => {
                if slot_ty != Ty::DynArray(Scalar::String)
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                (*out, XmlAccessProvenance::Owned)
            }
            Rvalue::JsonDocAsStr { doc, out } => {
                if slot_ty != Ty::Str
                    || xml_operand_base_ty(self.graph.function, doc) != Some(Ty::JsonDoc)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, doc, Ty::JsonDoc);
                (*out, XmlAccessProvenance::Shared)
            }
            Rvalue::JsonDocKey { doc, index, out } => {
                if slot_ty != Ty::Str
                    || xml_operand_base_ty(self.graph.function, doc) != Some(Ty::JsonDoc)
                    || xml_operand_base_ty(self.graph.function, index) != Some(i64_ty)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, doc, Ty::JsonDoc);
                self.check_operand(equation, index, i64_ty);
                (*out, XmlAccessProvenance::Shared)
            }
            Rvalue::BytesAsStr { bytes, out } => {
                if slot_ty != Ty::Str
                    || xml_operand_base_ty(self.graph.function, bytes) != Some(bytes_ty)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, bytes, bytes_ty);
                (*out, XmlAccessProvenance::Shared)
            }
            Rvalue::FsReadFileView {
                path: input,
                arena,
                out,
            } => {
                if slot_ty != Ty::Str
                    || xml_operand_base_ty(self.graph.function, input) != Some(Ty::Str)
                    || xml_operand_base_ty(self.graph.function, arena) != Some(Ty::ArenaHandle)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, input, Ty::Str);
                self.check_operand(equation, arena, Ty::ArenaHandle);
                (*out, XmlAccessProvenance::Shared)
            }
            Rvalue::HttpReadStreamHeader { stream, name, out } => {
                if slot_ty != Ty::Str
                    || xml_operand_base_ty(self.graph.function, stream)
                        != Some(Ty::HttpReadStream)
                    || xml_operand_base_ty(self.graph.function, name) != Some(Ty::Str)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, stream, Ty::HttpReadStream);
                self.check_operand(equation, name, Ty::Str);
                (*out, XmlAccessProvenance::Shared)
            }
            Rvalue::HttpCtxHeader { ctx, name, out } => {
                if slot_ty != Ty::Str
                    || xml_operand_base_ty(self.graph.function, ctx) != Some(Ty::HttpHeaders)
                    || xml_operand_base_ty(self.graph.function, name) != Some(Ty::Str)
                {
                    equation.invalid = true;
                    return;
                }
                self.check_operand(equation, ctx, Ty::HttpHeaders);
                self.check_operand(equation, name, Ty::Str);
                (*out, XmlAccessProvenance::Shared)
            }
            _ => {
                for operand in xml_out_producer_operands(rvalue) {
                    let Some(expected) = xml_operand_base_ty(self.graph.function, operand) else {
                        equation.invalid = true;
                        continue;
                    };
                    self.check_whole_operand(equation, operand, expected);
                }
                (slot, recorded_access)
            }
        };
        if expected_out != slot || access != recorded_access {
            equation.invalid = true;
        } else {
            equation.seed = merge_xml_access(equation.seed, access);
        }
    }

    fn slot_equation(
        &mut self,
        slot: Slot,
        path: Vec<XmlAccessPathSegment>,
    ) -> XmlAccessEquation {
        let mut equation = XmlAccessEquation::default();
        let Some(slot_ty) = self.graph.function.slots.get(slot as usize).copied() else {
            equation.invalid = true;
            return equation;
        };
        let Some(selected_ty) = xml_selected_ty(self.graph.program, slot_ty, &path) else {
            equation.invalid = true;
            return equation;
        };
        // Borrow parameters alias caller storage at function entry; unlike by-value
        // locals, LLVM does not wait for an explicit MIR Store to initialize them.
        // Keep every later store as a dependency so this seed cannot hide corruption.
        if let Some(parameter) = self.graph.function.params.iter().position(|candidate| *candidate == slot)
            && matches!(self.graph.function.param_modes.get(parameter),
                Some(align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut))
            && let Ok(parameter) = u32::try_from(parameter)
        {
            equation.seed = Some(xml_argument_access(self.graph.function, parameter));
        }
        let (Some(root_stores), Some(field_stores)) = (
            self.graph.slot_stores.roots.get(slot as usize),
            self.graph.slot_stores.fields.get(slot as usize),
        ) else {
            equation.invalid = true;
            return equation;
        };
        let root_stores = root_stores.iter().map(|operand| (*operand).clone()).collect::<Vec<_>>();
        let field_stores = field_stores
            .iter()
            .map(|(fields, operand)| ((*fields).to_vec(), (*operand).clone()))
            .collect::<Vec<_>>();
        let Some(element_stores) = self.graph.slot_stores.elements.get(slot as usize) else {
            equation.invalid = true;
            return equation;
        };
        let element_stores = element_stores
            .iter()
            .map(|(index, operand)| ((*index).clone(), (*operand).clone()))
            .collect::<Vec<_>>();
        let Some(element_field_stores) = self.graph.slot_stores.element_fields.get(slot as usize)
        else {
            equation.invalid = true;
            return equation;
        };
        let element_field_stores = element_field_stores
            .iter()
            .map(|(index, fields, operand)| {
                ((*index).clone(), (*fields).to_vec(), (*operand).clone())
            })
            .collect::<Vec<_>>();
        let whole_element_from_fields = if path.as_slice() == [XmlAccessPathSegment::Element] {
            match slot_ty {
                Ty::StructArray(id, _) => self
                    .graph
                    .program
                    .structs
                    .get(id as usize)
                    .is_some_and(|definition| {
                        !definition.fields.is_empty()
                            && definition.fields.iter().enumerate().all(|(field, _)| {
                                element_field_stores.iter().any(|(_, path, _)| {
                                    path.first().copied() == u32::try_from(field).ok()
                                })
                            })
                    }),
                _ => false,
            }
        } else {
            false
        };
        let Some(constant_stores) = self.graph.slot_stores.constant_elements.get(slot as usize)
        else {
            equation.invalid = true;
            return equation;
        };
        let constant_stores = constant_stores
            .iter()
            .map(|(elements, element)| ((*elements).to_vec(), *element))
            .collect::<Vec<_>>();
        let Some(producers) = self.graph.slot_stores.producers.get(slot as usize) else {
            equation.invalid = true;
            return equation;
        };
        let producers = producers.clone();
        for operand in root_stores {
            if !xml_operand_base_ty(self.graph.function, &operand)
                .is_some_and(|actual| xml_callable_flow_matches(self.graph.program, actual, slot_ty)) {
                equation.invalid = true;
            } else {
                if matches!(operand, Operand::BorrowedPlace(_)) {
                    // A match binding may materialize a physical Move leaf from a borrowed
                    // aggregate before it is retyped to a view (`string` -> `str`). It has read
                    // authority only; treating the descriptor as a transferable whole value
                    // would reject the valid borrowed source (and could later mint ownership).
                    self.check_whole_read_operand(&mut equation, &operand, slot_ty);
                    self.add_read_operand(&mut equation, &operand, selected_ty, path.clone());
                } else {
                    self.check_whole_operand(&mut equation, &operand, slot_ty);
                    self.add_operand(&mut equation, &operand, selected_ty, path.clone());
                }
            }
        }
        for (fields, operand) in field_stores {
            let stored_path = fields
                .iter()
                .copied()
                .map(XmlAccessPathSegment::StructField)
                .collect::<Vec<_>>();
            let Some(stored_ty) = xml_selected_ty(self.graph.program, slot_ty, &stored_path) else {
                equation.invalid = true;
                continue;
            };
            if fields.is_empty()
                || !xml_operand_base_ty(self.graph.function, &operand)
                .is_some_and(|actual| xml_callable_flow_matches(self.graph.program, actual, stored_ty))
            {
                equation.invalid = true;
                continue;
            }
            self.check_whole_operand(&mut equation, &operand, stored_ty);
            if path.is_empty() {
                self.add_operand(&mut equation, &operand, stored_ty, Vec::new());
            } else if path.starts_with(&stored_path) {
                self.add_operand(
                    &mut equation,
                    &operand,
                    selected_ty,
                    path[stored_path.len()..].to_vec(),
                );
            }
        }
        let element_path = path
            .strip_prefix(&[XmlAccessPathSegment::Element])
            .map(<[_]>::to_vec);
        for (index, operand) in element_stores {
            let i64_ty = Ty::Int(IntTy {
                bits: 64,
                signed: true,
            });
            let Some(element_ty) = xml_inline_array_element(self.graph.program, slot_ty) else {
                equation.invalid = true;
                continue;
            };
            if xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                || !xml_operand_base_ty(self.graph.function, &operand)
                .is_some_and(|actual| xml_callable_flow_matches(self.graph.program, actual, element_ty))
            {
                equation.invalid = true;
                continue;
            }
            self.check_operand(&mut equation, &index, i64_ty);
            self.check_whole_operand(&mut equation, &operand, element_ty);
            let Some(remaining) = element_path.as_ref() else {
                equation.invalid = true;
                continue;
            };
            if xml_selected_ty(self.graph.program, element_ty, remaining) != Some(selected_ty) {
                equation.invalid = true;
                continue;
            }
            self.add_operand(
                &mut equation,
                &operand,
                selected_ty,
                remaining.clone(),
            );
        }
        for (index, fields, operand) in element_field_stores {
            let i64_ty = Ty::Int(IntTy {
                bits: 64,
                signed: true,
            });
            let mut stored_path = vec![XmlAccessPathSegment::Element];
            stored_path.extend(
                fields
                    .iter()
                    .copied()
                    .map(XmlAccessPathSegment::StructField),
            );
            let Some(stored_ty) = xml_selected_ty(self.graph.program, slot_ty, &stored_path)
            else {
                equation.invalid = true;
                continue;
            };
            if fields.is_empty()
                || !matches!(slot_ty, Ty::StructArray(..))
                || xml_operand_base_ty(self.graph.function, &index) != Some(i64_ty)
                || !xml_operand_base_ty(self.graph.function, &operand)
                .is_some_and(|actual| xml_callable_flow_matches(self.graph.program, actual, stored_ty))
            {
                equation.invalid = true;
                continue;
            }
            self.check_operand(&mut equation, &index, i64_ty);
            self.check_whole_operand(&mut equation, &operand, stored_ty);
            if path.starts_with(&stored_path) {
                self.add_operand(
                    &mut equation,
                    &operand,
                    selected_ty,
                    path[stored_path.len()..].to_vec(),
                );
            }
        }
        let field_seed_inputs = whole_element_from_fields.then(||
            equation.checks.iter().map(|(input, _)| input.clone()).collect::<Vec<_>>());
        for (elements, element_ty) in constant_stores {
            let valid_shape = match slot_ty {
                Ty::Array(element, length) => {
                    scalar_to_ty(element) == element_ty
                        && usize::try_from(length) == Ok(elements.len())
                }
                _ => false,
            };
            if !valid_shape
                || elements
                    .iter()
                    .any(|element| !xml_const_element_matches_ty(element, element_ty))
                || element_path.as_deref() != Some(&[])
                || selected_ty != element_ty
            {
                equation.invalid = true;
                continue;
            }
            equation.seed = merge_xml_access(equation.seed, XmlAccessProvenance::Shared);
        }
        let independent_seed = equation.seed.is_some();
        let mut seed_inputs = Vec::new();
        if let Some(inputs) = field_seed_inputs {
            equation.seed = merge_xml_access(equation.seed, XmlAccessProvenance::Owned);
            seed_inputs.push(inputs);
        }
        for (value, producer) in producers {
            let start = equation.checks.len();
            self.add_out_producer(
                &mut equation,
                slot,
                slot_ty,
                selected_ty,
                &path,
                (value, producer),
            );
            seed_inputs.push(
                equation
                    .checks
                    .iter()
                    .skip(start)
                    .map(|(input, _)| input.clone())
                    .collect(),
            );
        }
        if !independent_seed && !seed_inputs.is_empty() {
            equation.seed_inputs = Some(seed_inputs);
        }
        if equation.seed.is_none()
            && equation.dependencies.is_empty()
            && equation.read_dependencies.is_empty()
            && !equation.invalid
        {
            equation.invalid = true;
        }
        equation
    }

    fn build(
        mut self,
        root: XmlAccessSource,
        cache: Option<&std::cell::RefCell<HashMap<XmlAccessNode, XmlAccessProvenance>>>,
    ) -> XmlProducerState {
        let root_node = match root {
            XmlAccessSource::Seed(access) => return XmlProducerState::Present(access),
            XmlAccessSource::Invalid => return XmlProducerState::Invalid,
            XmlAccessSource::Node(node) => node,
            XmlAccessSource::ReadNode(node) => node,
        };
        while let Some(node) = self.pending.pop_front() {
            if self.equations.contains_key(&node) {
                continue;
            }
            if let Some(access) = cache
                .and_then(|cache| cache.borrow().get(&node).copied())
            {
                self.equations.insert(
                    node,
                    XmlAccessEquation {
                        seed: Some(access),
                        ..XmlAccessEquation::default()
                    },
                );
                continue;
            }
            self.equations
                .insert(node.clone(), XmlAccessEquation::default());
            let mut equation = match &node {
                XmlAccessNode::Value(value, path) => {
                    let mut equation = self.value_equation(*value, path.clone());
                    // A scalar SSA result copies bits out of storage; it does not inherit the
                    // storage's borrow authority. Retain every grounding/check edge and absence
                    // fact, and never apply this transform to storage or aggregate projections.
                    equation.copied_scalar = path.is_empty()
                        && self.graph.function.value_tys.get(*value as usize).is_some_and(|ty| {
                            matches!(ty, Ty::Unit | Ty::Bool | Ty::Char)
                                || xml_numeric_scalar_ty(*ty)
                                || xml_numeric_vector_shape(*ty).is_some()
                                || matches!(*ty, Ty::Mask(element, lanes @ (2 | 4 | 8 | 16))
                                    if xml_numeric_vector_shape(Ty::Vec(element, lanes)).is_some())
                        });
                    equation
                },
                XmlAccessNode::Slot(slot, path) => self.slot_equation(*slot, path.clone()),
                XmlAccessNode::CaptureValue(value, path) => {
                    self.capture_equation(XmlAccessNode::Value(*value, path.clone()))
                }
                XmlAccessNode::CaptureSlot(slot, path) => {
                    self.capture_equation(XmlAccessNode::Slot(*slot, path.clone()))
                }
                XmlAccessNode::BufferValue(value, path) => self.buffer_value_equation(*value, path.clone()),
                XmlAccessNode::BufferSlot(slot, path) => self.buffer_slot_equation(*slot, path.clone()),
            };
            equation.requires_inputs = matches!(node,
                XmlAccessNode::Value(..) | XmlAccessNode::CaptureValue(..) | XmlAccessNode::BufferValue(..));
            self.equations.insert(node, equation);
        }

        let (values, invalid) = solve_xml_access_equations(&self.equations);
        if let Some(cache) = cache {
            let mut cache = cache.borrow_mut();
            for (node, state) in &values {
                if !invalid.contains(node)
                    && let XmlProducerState::Present(
                        access @ (XmlAccessProvenance::Owned
                        | XmlAccessProvenance::Shared
                        | XmlAccessProvenance::Exclusive
                        | XmlAccessProvenance::Unreadable),
                    ) = state
                {
                    cache.insert(node.clone(), *access);
                }
            }
        }
        if invalid.contains(&root_node) {
            return XmlProducerState::Invalid;
        }
        values
            .get(&root_node)
            .copied()
            .unwrap_or(XmlProducerState::Invalid)
    }
}

fn xml_operand_access(
    graph: &ValidatedProducerGraph<'_>,
    operand: &Operand,
    expected: Ty,
) -> XmlProducerState {
    let mut analysis = XmlAccessAnalyzer::new(graph);
    let root = analysis.source(operand, expected, Vec::new());
    analysis.build(root, None)
}

fn xml_operand_path_access(
    graph: &ValidatedProducerGraph<'_>,
    operand: &Operand,
    selected: Ty,
    path: Vec<XmlAccessPathSegment>,
    cache: &std::cell::RefCell<HashMap<XmlAccessNode, XmlAccessProvenance>>,
) -> XmlProducerState {
    let mut analysis = XmlAccessAnalyzer::new(graph);
    let root = analysis.source(operand, selected, path);
    analysis.build(root, Some(cache))
}

fn xml_slot_path_access(
    graph: &ValidatedProducerGraph<'_>,
    slot: Slot,
    selected: Ty,
    path: Vec<XmlAccessPathSegment>,
) -> XmlProducerState {
    if graph
        .function
        .slots
        .get(slot as usize)
        .copied()
        .and_then(|root| xml_selected_ty(graph.program, root, &path))
        != Some(selected)
    {
        return XmlProducerState::Invalid;
    }
    let mut analysis = XmlAccessAnalyzer::new(graph);
    let root = analysis.queue(XmlAccessNode::Slot(slot, path));
    analysis.build(root, None)
}

fn xml_borrowed_path(
    program: &Program,
    mut ty: Ty,
    path: &[hir::BorrowedPathSegment],
) -> Option<(Ty, Vec<XmlAccessPathSegment>)> {
    let mut selected = Vec::with_capacity(path.len());
    for segment in path {
        match *segment {
            hir::BorrowedPathSegment::RootSlot => return None,
            hir::BorrowedPathSegment::StructField(field) => {
                let Ty::Struct(id) = ty else { return None };
                ty = program
                    .structs
                    .get(id as usize)?
                    .fields
                    .get(field as usize)?
                    .ty;
                selected.push(XmlAccessPathSegment::StructField(field));
            }
            hir::BorrowedPathSegment::EnumPayload {
                variant,
                payload_ordinal,
            } => {
                let Ty::Enum(enum_id) = ty else { return None };
                ty = scalar_to_ty(
                    *program
                        .enums
                        .get(enum_id as usize)?
                        .variants
                        .get(variant as usize)?
                        .payload
                        .get(payload_ordinal as usize)?,
                );
                selected.push(XmlAccessPathSegment::EnumPayload {
                    enum_id,
                    variant,
                    slot: payload_ordinal,
                });
            }
            hir::BorrowedPathSegment::OptionSome => {
                ty = xml_option_payload(program, ty)?;
                selected.push(XmlAccessPathSegment::OptionSome);
            }
            hir::BorrowedPathSegment::ResultOk => {
                ty = xml_result_payload(program, ty, true)?;
                selected.push(XmlAccessPathSegment::ResultOk);
            }
            hir::BorrowedPathSegment::ResultErr => {
                ty = xml_result_payload(program, ty, false)?;
                selected.push(XmlAccessPathSegment::ResultErr);
            }
        }
    }
    Some((ty, selected))
}

fn xml_borrowed_access(
    graph: &ValidatedProducerGraph<'_>,
    operand: &Operand,
) -> XmlProducerState {
    let (slot, path, expected) = match operand {
        Operand::BorrowedPlace(place) => {
            let Some(root) = graph.function.slots.get(place.slot as usize).copied() else {
                return XmlProducerState::Invalid;
            };
            let Some((selected, path)) = xml_borrowed_path(graph.program, root, &place.path) else {
                return XmlProducerState::Invalid;
            };
            let view_retype = xml_borrowed_place_ty_is_view_retype(selected, place.ty);
            if selected != place.ty && !view_retype {
                return XmlProducerState::Invalid;
            }
            (place.slot, path, selected)
        }
        Operand::BorrowedElementPlace(place) => {
            let Some(root) = graph
                .function
                .slots
                .get(place.base.slot as usize)
                .copied()
            else {
                return XmlProducerState::Invalid;
            };
            let Some((base_selected, _)) =
                xml_borrowed_path(graph.program, root, &place.base.path)
            else {
                return XmlProducerState::Invalid;
            };
            let base_retype = xml_borrowed_place_ty_is_view_retype(base_selected, place.base.ty);
            if base_selected != place.base.ty && !base_retype {
                return XmlProducerState::Invalid;
            }
            let physical_element = match base_selected {
                Ty::Slice(element) | Ty::DynArray(element) => scalar_to_ty(element),
                Ty::DynStructArray(id, Layout::Aos) => Ty::Struct(id),
                _ => return XmlProducerState::Invalid,
            };
            let selected = if place.field_path.is_empty() {
                physical_element
            } else {
                let mut selected = physical_element;
                for (depth, field) in place.field_path.iter().copied().enumerate() {
                    let Ty::Struct(id) = selected else {
                        return XmlProducerState::Invalid;
                    };
                    let Some(field_ty) = graph
                        .program
                        .structs
                        .get(id as usize)
                        .and_then(|definition| definition.fields.get(field as usize))
                        .map(|field| field.ty)
                    else {
                        return XmlProducerState::Invalid;
                    };
                    if depth + 1 < place.field_path.len()
                        && !matches!(field_ty, Ty::Struct(_))
                    {
                        return XmlProducerState::Invalid;
                    }
                    selected = field_ty;
                }
                selected
            };
            let view_retype = xml_borrowed_place_ty_is_view_retype(selected, place.element_ty);
            if selected != place.element_ty && !view_retype {
                return XmlProducerState::Invalid;
            }
            return xml_borrowed_access(
                graph,
                &Operand::BorrowedPlace(Box::new(place.base.clone())),
            );
        }
        Operand::BorrowedFixedElementPlace(place) => {
            let Some(mut ty) = graph.function.slots.get(place.base as usize).copied() else {
                return XmlProducerState::Invalid;
            };
            let Ty::StructArray(struct_id, len) = ty else {
                return XmlProducerState::Invalid;
            };
            if place.index >= len {
                return XmlProducerState::Invalid;
            }
            ty = Ty::Struct(struct_id);
            let mut path = Vec::with_capacity(place.path.len());
            for field in &place.path {
                let Ty::Struct(id) = ty else {
                    return XmlProducerState::Invalid;
                };
                let Some(next) = graph
                    .program
                    .structs
                    .get(id as usize)
                    .and_then(|definition| definition.fields.get(*field as usize))
                    .map(|field| field.ty)
                else {
                    return XmlProducerState::Invalid;
                };
                ty = next;
                path.push(XmlAccessPathSegment::StructField(*field));
            }
            if ty != place.ty && !(ty == Ty::String && place.ty == Ty::Str) {
                return XmlProducerState::Invalid;
            }
            // This restricted resource-field call derives borrow authority from the owning
            // fixed local/parameter after the ordinary place validator authenticates the path.
            let access = graph
                .function
                .params
                .iter()
                .position(|candidate| *candidate == place.base)
                .map_or(XmlAccessProvenance::Owned, |index| {
                    xml_argument_access(graph.function, index as u32)
                });
            return XmlProducerState::Present(access);
        }
        _ => return XmlProducerState::Invalid,
    };
    if let Some(index) = graph
        .function
        .params
        .iter()
        .position(|candidate| *candidate == slot)
    {
        XmlProducerState::Present(xml_argument_access(graph.function, index as u32))
    } else {
        xml_slot_path_access(graph, slot, expected, path)
    }
}

fn xml_borrowed_storage_access(
    graph: &ValidatedProducerGraph<'_>,
    operand: &Operand,
) -> XmlProducerState {
    let slot = match operand {
        Operand::BorrowedPlace(place) => place.slot,
        Operand::BorrowedElementPlace(place) => place.base.slot,
        Operand::BorrowedFixedElementPlace(place) => place.base,
        _ => return XmlProducerState::Invalid,
    };
    if graph.function.slots.get(slot as usize).is_none() {
        return XmlProducerState::Invalid;
    }
    let access = graph
        .function
        .params
        .iter()
        .position(|candidate| *candidate == slot)
        .map_or(XmlAccessProvenance::Owned, |index| {
            xml_argument_access(graph.function, index as u32)
        });
    XmlProducerState::Present(access)
}

fn xml_borrowed_mode_satisfied(
    program: &Program,
    parameter: Ty,
    selected: Ty,
    mode: align_ast::ParamMode,
    payload: XmlProducerState,
    storage: XmlProducerState,
) -> bool {
    let mut readable_payload = OperandRequirement::READ;
    readable_payload.callable = matches!(selected, Ty::Fn(_));
    match mode {
        // A canonical borrowed descriptor is also the physical representation of a Copy view
        // passed by value. The call-shape check above already rejects a borrowed carrier for a
        // Move parameter; the remaining Copy case needs only readable payload provenance.
        align_ast::ParamMode::ByValue => readable_payload.is_satisfied_by(payload),
        align_ast::ParamMode::Borrow => readable_payload.is_satisfied_by(payload),
        align_ast::ParamMode::BorrowMut => {
            if align_sema::ty_is_move(
                parameter,
                &program.structs,
                &program.tuples,
                &program.enums,
                &program.tagged_types,
            ) {
                xml_mode_requirement(program, selected, mode)
                    .is_satisfied_by(payload)
            } else {
                readable_payload.is_satisfied_by(payload)
                    && OperandRequirement {
                        write: true,
                        exclusive: true,
                        ..OperandRequirement::default()
                    }
                    .is_satisfied_by(storage)
            }
        }
        align_ast::ParamMode::Out => OperandRequirement {
            write: true,
            exclusive: true,
            ..OperandRequirement::default()
        }
        .is_satisfied_by(storage),
    }
}

fn xml_borrowed_descriptor_path_valid(
    graph: &ValidatedProducerGraph<'_>,
    operand: &Operand,
) -> bool {
    let (place, element_base) = match operand {
        Operand::BorrowedPlace(place) => (place.as_ref(), false),
        Operand::BorrowedElementPlace(place) => (&place.base, true),
        Operand::BorrowedFixedElementPlace(place) => {
            return graph.function.slots.get(place.base as usize).is_some();
        }
        _ => return false,
    };
    let path = if element_base
        && matches!(place.path.first(), Some(hir::BorrowedPathSegment::RootSlot))
    {
        let Some(path) = place.path.get(1..) else {
            return false;
        };
        path
    } else {
        &place.path
    };
    let Some((selected, _)) = graph
        .function
        .slots
        .get(place.slot as usize)
        .copied()
        .and_then(|root| xml_borrowed_path(graph.program, root, path))
    else {
        return false;
    };
    // A traversable projection is canonical enough to defer its final retype check to callable
    // preflight, which owns the precise borrowed-place diagnostic. Root descriptors still need an
    // exact type here so a relabeled whole value cannot bypass producer authentication.
    !path.is_empty()
        || selected == place.ty
        || (!element_base && xml_borrowed_place_ty_is_view_retype(selected, place.ty))
}

fn xml_return_cleanup_companion(
    graph: &ValidatedProducerGraph<'_>,
    returned: &Operand,
) -> Option<Option<ValueId>> {
    let Operand::Value(initial) = returned else {
        return Some(None);
    };
    let mut value = *initial;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(value) || graph.duplicate_values.get(value as usize) != Some(&false) {
            return None;
        }
        let definition = graph.value_definitions.get(value as usize)?.as_ref()?;
        match definition {
            Rvalue::CallWithCleanup(call) => return Some(Some(call.cleanup)),
            Rvalue::CallIndirectWithCleanup(call) => return Some(Some(call.cleanup)),
            Rvalue::XmlParse { cleanup, .. } => return Some(Some(*cleanup)),
            Rvalue::Use(Operand::Value(next)) => value = *next,
            _ => return Some(None),
        }
    }
}

fn xml_local_call_components(program: &Program) -> Result<Vec<Vec<usize>>, ProducerError> {
    let mut indices = HashMap::new();
    for (index, function) in program.fns.iter().enumerate() {
        if indices.insert(function.name.clone(), index).is_some() {
            return Err(ProducerError::Lowering(
                "MIR producer certification contains duplicate local function names".to_owned(),
            ));
        }
    }

    let mut edges = vec![Vec::<usize>::new(); program.fns.len()];
    for (source, function) in program.fns.iter().enumerate() {
        for rvalue in function
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .filter_map(|statement| match statement {
                Stmt::Let(_, rvalue) => Some(rvalue),
                _ => None,
            })
        {
            let target = match rvalue {
                Rvalue::Call(DirectCall::Program(target), _) | Rvalue::FnAddr { target, .. } => {
                    Some(target)
                }
                Rvalue::CallWithCleanup(call) => Some(&call.target),
                Rvalue::Closure { lifted, .. } => Some(lifted),
                _ => None,
            };
            if let Some(target) = target.and_then(|target| indices.get(target).copied()) {
                edges[source].push(target);
            }
        }
        edges[source].sort_unstable();
        edges[source].dedup();
    }

    // Iterative Kosaraju keeps a malformed enormous call graph from consuming the native stack.
    let mut visited = vec![false; edges.len()];
    let mut finish = Vec::with_capacity(edges.len());
    for start in 0..edges.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut pending = vec![(start, 0usize)];
        while let Some((node, next)) = pending.last_mut() {
            if let Some(target) = edges[*node].get(*next).copied() {
                *next += 1;
                if !visited[target] {
                    visited[target] = true;
                    pending.push((target, 0));
                }
            } else {
                finish.push(*node);
                pending.pop();
            }
        }
    }
    let mut reverse = vec![Vec::<usize>::new(); edges.len()];
    for (source, targets) in edges.iter().enumerate() {
        for target in targets {
            reverse[*target].push(source);
        }
    }
    visited.fill(false);
    let mut components = Vec::new();
    for start in finish.into_iter().rev() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut component = Vec::new();
        let mut pending = vec![start];
        while let Some(node) = pending.pop() {
            component.push(node);
            for source in &reverse[node] {
                if !visited[*source] {
                    visited[*source] = true;
                    pending.push(*source);
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }

    let mut owner = vec![usize::MAX; edges.len()];
    for (component, members) in components.iter().enumerate() {
        for member in members {
            owner[*member] = component;
        }
    }
    let mut dependencies = vec![HashSet::<usize>::new(); components.len()];
    for (source, targets) in edges.iter().enumerate() {
        for target in targets {
            if owner[source] != owner[*target] {
                dependencies[owner[source]].insert(owner[*target]);
            }
        }
    }
    let mut ordered = Vec::with_capacity(components.len());
    let mut complete = HashSet::new();
    while ordered.len() != components.len() {
        let Some(next) = (0..components.len()).find(|candidate| {
            !complete.contains(candidate)
                && dependencies[*candidate]
                    .iter()
                    .all(|dependency| complete.contains(dependency))
        }) else {
            return Err(ProducerError::Lowering(
                "MIR producer call-graph condensation is cyclic".to_owned(),
            ));
        };
        complete.insert(next);
        ordered.push(components[next].clone());
    }
    Ok(ordered)
}

/// Validate the complete MIR producer graph and return the exact local bodies whose declared
/// contracts were certified together. Interface publication consumes this set; it must never infer
/// certification from a declaration alone.
pub fn validate_mir_producers(program: &Program) -> Result<HashSet<String>, ProducerError> {
    validate_tagged_program(program)?;
    validate_resource_program(program)?;
    validate_slice_index_rvalues(program)?;
    validate_fixed_element_nulling(program)?;
    // Publication certifies the typed producer graph, not final native codegen.
    // Generated callback/parallel-kernel preflight runs at emission, after the
    // consumer's interface checks and diagnostic precedence have completed.
    // Running it here both repeats canonical ABI construction for every imported
    // declaration and rejects dependency bodies before their consumer can report
    // the owning source-level error. Callable producers and copied call facts are
    // authenticated below by the same graph used at emission.
    validate_resource_rvalues(program)?;
    let certified = program
        .fns
        .iter()
        .map(|function| function.name.as_str().to_owned())
        .collect::<HashSet<_>>();
    if certified.len() != program.fns.len() {
        return Err(ProducerError::Lowering(
            "MIR producer certification contains duplicate local function names".to_owned(),
        ));
    }
    Ok(certified)
}

fn validate_host_mir(program: &Program) -> Result<(), ProducerError> {
    if !align_sema::process_live::schemas_valid(&program.structs, &program.enums) {
        return Err(ProducerError::Lowering("malformed process observation schema".to_string()));
    }
    for function in &program.fns {
        for statement in function.blocks.iter().flat_map(|block| &block.stmts) {
            if let Stmt::Let(value,Rvalue::ProcessLive { kind,args,out }) = statement {
                use align_sema::process_live::{input_type,payload_type};
                let payload = payload_type(*kind,&program.structs,&program.enums);
                let native_ty = if kind.fallible() { Some(Ty::Int(IntTy { bits:32,signed:true })) }
                    else if kind.scratch() { Some(Ty::Unit) } else { payload };
                let output_valid = match out { Some(slot) => kind.scratch() && function.slots.get(*slot as usize).copied()==payload,
                    None => !kind.scratch() };
                if payload.is_none() || !output_valid || function.value_tys.get(*value as usize).copied()!=native_ty
                    || args.len()!=kind.inputs().len() || kind.inputs().iter().zip(args).any(|(input,operand)|
                        xml_operand_base_ty(function,operand).is_none_or(|actual| !align_sema::process_live::view_input_matches(*input,actual).unwrap_or(Some(actual)==input_type(*input,&program.structs,&program.enums)))) {
                    return Err(ProducerError::Lowering("malformed live process producer".to_string()));
                }
            }
            if let Stmt::Let(value, Rvalue::ChildWait { child, out }) = statement {
                let result = align_sema::fs_tree::record_id(&program.structs, "process.wait_result").map(Ty::Struct);
                if result.is_none() || function.slots.get(*out as usize).copied() != result
                    || xml_operand_base_ty(function, child) != Some(Ty::Child)
                    || function.value_tys.get(*value as usize).copied() != Some(Ty::Int(IntTy { bits: 32, signed: true })) {
                    return Err(ProducerError::Lowering("malformed child wait producer".to_string()));
                }
            }
        }
    }

    if !align_sema::fs_tree_schemas_valid(&program.structs, &program.enums) {
        return Err(ProducerError::Lowering("malformed retained filesystem schema".to_string()));
    }
    for function in &program.fns {
        for statement in function.blocks.iter().flat_map(|block| &block.stmts) {
            let Stmt::Let(value, Rvalue::FsTree { kind, args, output }) = statement else { continue; };
            use align_sema::fs_tree::{Output, input_matches, record_id};
            use crate::FsTreeOutput;
            let slot_ty = |slot: Slot| function.slots.get(slot as usize).copied();
            let valid_output = match (kind.output(), *output) {
                (Output::Unit, FsTreeOutput::None) => true,
                (Output::Bool, FsTreeOutput::Bool(slot)) => slot_ty(slot) == Some(Ty::Bool),
                (Output::OwnedBytes, FsTreeOutput::Bytes(slot)) => slot_ty(slot) == Some(Ty::DynArray(Scalar::Int(IntTy { bits:8, signed:false }))),
                (Output::Directory, FsTreeOutput::Owner(slot)) => slot_ty(slot) == Some(Ty::FsDirectory),
                (Output::Cursor, FsTreeOutput::Owner(slot)) => slot_ty(slot) == Some(Ty::FsDirCursor),
                (Output::Reader, FsTreeOutput::Owner(slot)) => slot_ty(slot) == Some(Ty::Reader),
                (Output::Writer, FsTreeOutput::Owner(slot)) => slot_ty(slot) == Some(Ty::Writer),
                (Output::Metadata, FsTreeOutput::Metadata(slot)) => record_id(&program.structs, "fs.metadata")
                    .is_some_and(|id| slot_ty(slot) == Some(Ty::Struct(id))),
                (Output::EntryOption, FsTreeOutput::CursorNext { entry, present }) => entry != present
                    && slot_ty(present) == Some(Ty::Bool)
                    && record_id(&program.structs, "fs.dir_entry").is_some_and(|id| slot_ty(entry) == Some(Ty::Struct(id))),
                _ => false,
            };
            if !valid_output || args.len() != kind.inputs().len()
                || function.value_tys.get(*value as usize).copied() != Some(Ty::Int(IntTy { bits: 32, signed: true }))
                || kind.inputs().iter().zip(args).any(|(input, operand)|
                    xml_operand_base_ty(function, operand).is_none_or(|ty| !input_matches(*input, ty, &program.structs))) {
                return Err(ProducerError::Lowering("malformed retained filesystem producer".to_string()));
            }
        }
    }
    let mut host = None;
    for (id, definition) in program.structs.iter().enumerate() {
        if definition.name == "os.host_info" || definition.source_name == "os.host_info" {
            if host.is_some() || !align_sema::host_info_schema_valid(definition) {
                return Err(ProducerError::Lowering("malformed os.host_info schema".to_string()));
            }
            host = u32::try_from(id).ok();
        }
    }
    for function in &program.fns {
        for statement in function.blocks.iter().flat_map(|block| &block.stmts) {
            if let Stmt::Let(value, Rvalue::OsHost { out }) = statement
                && (host.is_none()
                    || function.slots.get(*out as usize).copied() != host.map(Ty::Struct)
                    || function.value_tys.get(*value as usize).copied() != Some(Ty::Int(IntTy { bits: 32, signed: true }))) {
                return Err(ProducerError::Lowering("malformed os.host producer".to_string()));
            }
        }
    }
    let mut identity = None;
    for (id, definition) in program.structs.iter().enumerate() {
        if definition.name == "os.identity_info" || definition.source_name == "os.identity_info" {
            if identity.is_some() || !align_sema::identity_info_schema_valid(definition) {
                return Err(ProducerError::Lowering("malformed os.identity_info schema".to_string()));
            }
            identity = u32::try_from(id).ok();
        }
    }
    for function in &program.fns {
        for statement in function.blocks.iter().flat_map(|block| &block.stmts) {
            if let Stmt::Let(value, Rvalue::OsIdentity { out }) = statement
                && (identity.is_none()
                    || function.slots.get(*out as usize).copied() != identity.map(Ty::Struct)
                    || function.value_tys.get(*value as usize).copied() != Some(Ty::Int(IntTy { bits: 32, signed: true }))) {
                return Err(ProducerError::Lowering("malformed os.identity producer".to_string()));
            }
        }
    }
    Ok(())
}

pub fn validate_resource_rvalues(program: &Program) -> Result<(), ProducerError> {
    validate_host_mir(program)?;
    validate_template_html_mir_signatures(program)?;
    let components = xml_local_call_components(program)?;
    let mut certified = HashSet::<ProgramCall>::new();
    for component in components {
        let mut provisional = certified.clone();
        provisional.extend(
            component
                .iter()
                .map(|member| program.fns[*member].name.clone()),
        );
        validate_resource_rvalues_component(program, &component, &provisional)?;
        certified = provisional;
    }
    Ok(())
}

fn validate_resource_rvalues_component(
    program: &Program,
    component: &[usize],
    local_contracts: &HashSet<ProgramCall>,
) -> Result<(), ProducerError> {
    fn operand_ty(function: &crate::Function, operand: &crate::Operand) -> Option<Ty> {
        Some(match operand {
            crate::Operand::Const(crate::Const::Int(_, ty))
            | crate::Operand::Const(crate::Const::Float(_, ty)) => *ty,
            crate::Operand::Const(crate::Const::Char(_)) => Ty::Char,
            crate::Operand::Const(crate::Const::Bool(_)) => Ty::Bool,
            crate::Operand::Const(crate::Const::Unit) => Ty::Unit,
            crate::Operand::Value(value) => *function.value_tys.get(*value as usize)?,
            crate::Operand::Arg(index) => {
                let slot = *function.params.get(*index as usize)?;
                *function.slots.get(slot as usize)?
            }
            crate::Operand::BorrowedPlace(place) => place.ty,
            crate::Operand::BorrowedElementPlace(place) => place.element_ty,
            crate::Operand::BorrowedFixedElementPlace(place) => place.ty,
            crate::Operand::BorrowedCleanupArg(_) => Ty::Bool,
        })
    }

    let fail = |function: &crate::Function, detail: &str| {
        ProducerError::Lowering(format!(
            "resource MIR in function '{}' is malformed: {detail}",
            function.name
        ))
    };
    let i32_ty = Ty::Int(IntTy {
        bits: 32,
        signed: true,
    });
    let i64_ty = Ty::Int(IntTy {
        bits: 64,
        signed: true,
    });
    let batch_fields = |struct_id: u32| {
        program.structs.get(struct_id as usize).and_then(|definition| {
            definition
                .fields
                .iter()
                .map(|field| {
                    let (base, nullable) = match field.ty {
                        Ty::Option(payload) => (scalar_to_ty(payload), true),
                        ty => (ty, false),
                    };
                    let view = matches!(
                        base,
                        Ty::Str
                            | Ty::Slice(Scalar::Int(IntTy {
                                bits: 8,
                                signed: false,
                            }))
                    );
                    matches!(
                        base,
                        Ty::Bool | Ty::Char | Ty::Int(_) | Ty::Float(_) | Ty::Str
                    )
                    .then_some((base, nullable, view))
                    .or_else(|| {
                        matches!(
                            base,
                            Ty::Slice(Scalar::Int(IntTy {
                                bits: 8,
                                signed: false,
                            }))
                        )
                        .then_some((base, nullable, view))
                    })
                })
                .collect::<Option<Vec<_>>>()
        })
    };
    for (function_index, function) in program.fns.iter().enumerate() {
        if component.binary_search(&function_index).is_err() {
            continue;
        }
        let mut primary_definitions = vec![0u32; function.value_tys.len()];
        let mut auxiliary_definitions = vec![0u32; function.value_tys.len()];
        let mut value_definitions = vec![None; function.value_tys.len()];
        let mut auxiliary_value_definitions = vec![None; function.value_tys.len()];
        let mut duplicate_values = vec![false; function.value_tys.len()];
        let mut slot_stores = XmlSlotStores {
            roots: vec![Vec::new(); function.slots.len()],
            fields: vec![Vec::new(); function.slots.len()],
            elements: vec![Vec::new(); function.slots.len()],
            element_fields: vec![Vec::new(); function.slots.len()],
            constant_elements: vec![Vec::new(); function.slots.len()],
            producers: vec![Vec::new(); function.slots.len()],
        };
        for block in &function.blocks {
            for statement in &block.stmts {
                assert_xml_stmt_variant_classified(statement);
                match statement {
                    Stmt::Let(value, rvalue) => {
                        if let Some(count) = primary_definitions.get_mut(*value as usize) {
                            *count = count.saturating_add(1);
                        }
                        if let Some(definition) = value_definitions.get_mut(*value as usize) {
                            if definition.is_some() {
                                if let Some(duplicate) =
                                    duplicate_values.get_mut(*value as usize)
                                {
                                    *duplicate = true;
                                }
                            } else {
                                *definition = Some(rvalue);
                            }
                        }
                        let auxiliary = match rvalue {
                            Rvalue::CallWithCleanup(call) => Some(call.cleanup),
                            Rvalue::CallIndirectWithCleanup(call) => Some(call.cleanup),
                            Rvalue::XmlParse { cleanup, .. } => Some(*cleanup),
                            _ => None,
                        };
                        if let Some(auxiliary) = auxiliary
                            && let Some(count) = auxiliary_definitions.get_mut(auxiliary as usize)
                        {
                            *count = count.saturating_add(1);
                            if let Some(definition) =
                                auxiliary_value_definitions.get_mut(auxiliary as usize)
                                && definition.is_none()
                            {
                                *definition = Some(rvalue);
                            }
                        }
                        for (out, _) in xml_written_slots(rvalue) {
                            if let Some(producers) = slot_stores.producers.get_mut(out as usize) {
                                producers.push((*value, rvalue));
                            }
                        }
                    }
                    Stmt::Store(slot, operand) => {
                        if let Some(stores) = slot_stores.roots.get_mut(*slot as usize) {
                            stores.push(operand);
                        }
                    }
                    Stmt::StoreField(slot, path, operand) => {
                        if let Some(stores) = slot_stores.fields.get_mut(*slot as usize) {
                            stores.push((path.as_slice(), operand));
                        }
                    }
                    Stmt::StoreIndex(slot, index, operand) => {
                        if let Some(stores) = slot_stores.elements.get_mut(*slot as usize) {
                            stores.push((index, operand));
                        }
                    }
                    Stmt::StoreElemField(slot, index, path, operand) => {
                        if let Some(stores) = slot_stores.element_fields.get_mut(*slot as usize) {
                            stores.push((index, path.as_slice(), operand));
                        }
                    }
                    Stmt::StoreConstArray { slot, elems, elem } => {
                        if let Some(stores) =
                            slot_stores.constant_elements.get_mut(*slot as usize)
                        {
                            stores.push((elems.as_slice(), *elem));
                        }
                    }
                    _ => {}
                }
            }
        }
        let access_graph = ValidatedProducerGraph {
            function,
            program,
            local_contracts,
            value_definitions: &value_definitions,
            auxiliary_value_definitions: &auxiliary_value_definitions,
            duplicate_values: &duplicate_values,
            slot_stores: &slot_stores,
            primary_definitions: &primary_definitions,
            auxiliary_definitions: &auxiliary_definitions,
        };
        // Every query below uses this immutable function graph. Reuse only nodes that a complete
        // fixed point proved Present; absent, conditional, unresolved, and invalid states must be
        // recomputed so a different selected root cannot erase their discriminator requirements.
        let access_node_cache =
            std::cell::RefCell::new(HashMap::<XmlAccessNode, XmlAccessProvenance>::new());
        let cached_path_access =
            |operand: &Operand, selected: Ty, path: Vec<XmlAccessPathSegment>| {
                xml_operand_path_access(
                    &access_graph,
                    operand,
                    selected,
                    path,
                    &access_node_cache,
                )
            };
        let xml_access = |operand: &Operand, expected: Ty| {
            cached_path_access(operand, expected, Vec::new())
        };
        let return_leaves = xml_owned_leaf_paths(program, function.ret)
            .ok_or_else(|| fail(function, "producer return type graph is malformed"))?;
        let return_leaf_valid = |operand: &Operand, selected: Ty, path: &[XmlAccessPathSegment]| {
            let state = if matches!(
                operand,
                Operand::BorrowedPlace(_)
                    | Operand::BorrowedElementPlace(_)
                    | Operand::BorrowedFixedElementPlace(_)
            ) {
                xml_borrowed_access(&access_graph, operand)
            } else {
                cached_path_access(operand, selected, path.to_vec())
            };
            if align_sema::ty_is_move(selected, &program.structs, &program.tuples, &program.enums, &program.tagged_types) {
                state.owned_if_present()
            } else {
                matches!(
                    state,
                    XmlProducerState::Present(
                        XmlAccessProvenance::Owned
                            | XmlAccessProvenance::Shared
                            | XmlAccessProvenance::Exclusive
                    )
                        | XmlProducerState::MaybeAbsent(
                            XmlAccessProvenance::Owned
                                | XmlAccessProvenance::Shared
                                | XmlAccessProvenance::Exclusive
                        )
                        | XmlProducerState::Absent
                )
            }
        };
        for block in &function.blocks {
            let (returned, cleanup) = match &block.term {
                Term::Return(Some(returned)) => (Some(returned), None),
                Term::ReturnWithCleanup(returned) => {
                    (Some(&returned.0), Some(&returned.1))
                }
                Term::Return(None) => {
                    if function.ret != Ty::Unit
                        || function.return_cleanup != hir::ReturnCleanupAbi::None
                    {
                        return Err(fail(
                            function,
                            "producer return omitted its declared value or cleanup bit",
                        ));
                    }
                    (None, None)
                }
                Term::Goto(_) | Term::Branch(..) | Term::Unreachable => (None, None),
            };
            if let Some(returned) = returned
            {
                let return_type_matches = match xml_operand_base_ty(function, returned) {
                    Some(actual) => source_ty_matches(actual, function.ret, program)?,
                    None => false,
                };
                if !return_type_matches {
                    return Err(fail(
                        function,
                        "producer return operand disagrees with its declared result type",
                    ));
                }
                if let Some((selected, path)) = return_leaves
                    .iter()
                    .find(|(selected, path)| !return_leaf_valid(returned, *selected, path))
                {
                    return Err(fail(
                        function,
                        &format!(
                            "producer return leaf {selected:?} at {path:?} is not certified by its body"
                        ),
                    ));
                }
                if cleanup.is_some()
                    != (function.return_cleanup == hir::ReturnCleanupAbi::DynamicBit)
                {
                    return Err(fail(
                        function,
                        "producer return cleanup mode is not certified by its body",
                    ));
                }
                let companion = xml_return_cleanup_companion(&access_graph, returned)
                    .ok_or_else(|| fail(function, "producer return cleanup source is malformed"))?;
                if let Some(expected) = companion
                    && !matches!(cleanup, Some(Operand::Value(actual)) if *actual == expected)
                {
                    return Err(fail(
                        function,
                        "producer return cleanup is detached from its value producer",
                    ));
                }
                if cleanup.is_some_and(|cleanup| {
                    !matches!(
                        xml_access(cleanup, Ty::Bool),
                        XmlProducerState::Present(
                            XmlAccessProvenance::Owned
                                | XmlAccessProvenance::Shared
                                | XmlAccessProvenance::Exclusive
                        )
                    )
                }) {
                    return Err(fail(
                        function,
                        "producer return cleanup operand is not certified by its body",
                    ));
                }
                if return_leaves.is_empty()
                    && align_sema::ty_is_move(
                        function.ret,
                        &program.structs,
                        &program.tuples,
                        &program.enums,
                        &program.tagged_types,
                    )
                    && !xml_mode_requirement(
                        program,
                        function.ret,
                        align_ast::ParamMode::ByValue,
                    )
                    .is_satisfied_by(xml_access(returned, function.ret))
                {
                    return Err(fail(
                        function,
                        "producer return does not transfer its declared Move value",
                    ));
                }
            }
        }
        let indirect_call_valid = |
            callee: &Operand,
            params: &[Ty],
            ret: Ty,
            signature: &crate::FnSignatureFacts,
        | {
            let Some(Ty::Fn(id)) = xml_operand_base_ty(function, callee) else {
                return false;
            };
            let copied = XmlCallFacts {
                params: params.to_vec(),
                modes: signature.param_modes.clone(),
                ret,
                borrow: signature.return_borrow.clone(),
                region: signature.return_region.clone(),
                cleanup: signature.return_cleanup,
            };
            xml_fn_type_facts(program, id).as_ref() == Some(&copied)
                && matches!(
                    xml_access(callee, Ty::Fn(id)),
                    XmlProducerState::Present(
                        XmlAccessProvenance::Owned
                            | XmlAccessProvenance::Shared
                            | XmlAccessProvenance::Exclusive
                    )
                )
        };
        let canonical_xml_value = |operand: &Operand| {
            matches!(operand, Operand::Value(_) | Operand::Arg(_))
        };
        let readable_xml_index = |operand: &Operand| {
            matches!(
                operand,
                Operand::Value(_) | Operand::Arg(_) | Operand::Const(Const::Int(_, _))
            ) && matches!(
                xml_access(operand, i64_ty),
                XmlProducerState::Present(
                    XmlAccessProvenance::Owned
                        | XmlAccessProvenance::Shared
                        | XmlAccessProvenance::Exclusive
                )
            )
        };
        let xml_call_arguments_valid =
            |args: &[Operand], params: &[Ty], modes: &[align_ast::ParamMode]| {
                args.len() == params.len()
                    && modes.len() == params.len()
                    && args
                        .iter()
                        .zip(params)
                        .zip(modes)
                        .all(|((operand, expected), mode)| {
                            let Some(leaves) = xml_owned_leaf_paths(program, *expected) else {
                                return false;
                            };
                            if *mode == align_ast::ParamMode::Out && !leaves.is_empty() {
                                if !matches!(expected, Ty::Slice(_))
                                    || xml_operand_base_ty(function, operand) != Some(*expected)
                                    || !matches!(operand, Operand::Value(_) | Operand::Arg(_))
                                { return false; }
                                let mut analysis = XmlAccessAnalyzer::new(&access_graph);
                                let source = analysis.buffer_source(operand, Vec::new());
                                return OperandRequirement {
                                    write: true, exclusive: true, ..OperandRequirement::default()
                                }.is_satisfied_by(analysis.build(source, None));
                            }
                            let canonical_borrow = matches!(
                                operand,
                                Operand::BorrowedPlace(_)
                                    | Operand::BorrowedElementPlace(_)
                                    | Operand::BorrowedFixedElementPlace(_)
                            );
                            let canonical_borrow_path = canonical_borrow
                                && xml_borrowed_descriptor_path_valid(&access_graph, operand);
                            let canonical_shape = !matches!(mode, align_ast::ParamMode::Out)
                                || canonical_borrow;
                            let parameter_moves = align_sema::ty_is_move(
                                *expected,
                                &program.structs,
                                &program.tuples,
                                &program.enums,
                                &program.tagged_types,
                            );
                            let by_value_shape = !matches!(mode, align_ast::ParamMode::ByValue)
                                || !parameter_moves
                                || matches!(operand, Operand::Value(_) | Operand::Arg(_));
                            if canonical_borrow {
                                if !canonical_borrow_path {
                                    return false;
                                }
                                let payload = xml_borrowed_access(&access_graph, operand);
                                let storage =
                                    xml_borrowed_storage_access(&access_graph, operand);
                                if leaves.is_empty() {
                                    return canonical_shape
                                        && by_value_shape
                                        && xml_borrowed_mode_satisfied(
                                            program,
                                            *expected,
                                            *expected,
                                            *mode,
                                            payload,
                                            storage,
                                        );
                                }
                                return canonical_shape
                                    && by_value_shape
                                    && leaves.iter().all(|(selected, _)| {
                                        xml_borrowed_mode_satisfied(
                                            program,
                                            *expected,
                                            *selected,
                                            *mode,
                                            payload,
                                            storage,
                                        )
                                    });
                            }
                            if leaves.is_empty() {
                                if !parameter_moves {
                                    // Copy-only carriers have no producer capability for this
                                    // validator to authenticate. Callable preflight owns their
                                    // exact Out/Borrow/BorrowMut destination and mode shape.
                                    return true;
                                }
                                let state = xml_operand_access(
                                    &access_graph,
                                    operand,
                                    *expected,
                                );
                                return canonical_shape
                                    && by_value_shape
                                    && xml_mode_requirement(
                                        program,
                                        *expected,
                                        *mode,
                                    )
                                    .is_satisfied_by(state);
                            }
                            canonical_shape
                                && by_value_shape
                                && leaves.iter().all(|(selected, path)| {
                                    let state =
                                        cached_path_access(operand, *selected, path.clone());
                                    // A Copy carrier such as `slice<Row>` exposes Move leaves
                                    // (`String`) without transferring those elements. For an
                                    // owning carrier, retain each selected leaf's own authority:
                                    // a nested `str` remains a read-only view even when its
                                    // sibling is an owning `string`. This keeps the requirement
                                    // per leaf while suppressing element transfer only for the
                                    // non-owning carrier.
                                    let requirement_ty =
                                        if parameter_moves { *selected } else { *expected };
                                    xml_mode_requirement(program, requirement_ty, *mode)
                                        .is_satisfied_by(state)
                                })
                        })
            };
        for block in &function.blocks {
            for statement in &block.stmts {
                match statement {
                    Stmt::ColumnBatchFinish { payload, struct_id }
                    | Stmt::ColumnBatchDrop { payload, struct_id }
                        if operand_ty(function, payload) != Some(Ty::Raw)
                            || batch_fields(*struct_id).is_none() =>
                    {
                        return Err(fail(function, "column-batch statement contract mismatch"));
                    }
                    _ => {}
                }
                let crate::Stmt::Let(value, rvalue) = statement else {
                    continue;
                };
                let result = function
                    .value_tys
                    .get(*value as usize)
                    .copied()
                    .ok_or_else(|| fail(function, "result value id is absent"))?;
                if native_owner_mir_contract(program, function, rvalue).is_some()
                    && !OperandRequirement::READ.is_satisfied_by(xml_access(&Operand::Value(*value), result))
                {
                    return Err(fail(function, &format!("native owner producer contract mismatch: {rvalue:?}")));
                }
                let protected_call_boundary = |args: &[Operand]| {
                    xml_owned_leaf_paths(program, result)
                        .is_none_or(|leaves| !leaves.is_empty())
                        || args.iter().any(|operand| {
                            xml_operand_base_ty(function, operand)
                                .and_then(|ty| xml_owned_leaf_paths(program, ty))
                                .is_none_or(|leaves| !leaves.is_empty())
                        })
                };
                let call_arguments_valid = match rvalue {
                    Rvalue::Call(DirectCall::Program(target), args) => {
                        xml_direct_call_facts(program, target, local_contracts).map_or_else(
                            || {
                                let has_align_declaration = program
                                    .fns
                                    .iter()
                                    .any(|function| &function.name == target)
                                    || program
                                        .imported_fns
                                        .iter()
                                        .any(|function| &function.name == target);
                                let matching = program
                                    .externs
                                    .iter()
                                    .filter(|declaration| &declaration.name == target)
                                    .collect::<Vec<_>>();
                                if !protected_call_boundary(args) {
                                    true
                                } else if !has_align_declaration
                                    && let [declaration] = matching.as_slice()
                                {
                                    direct_operands_match_modes(
                                        target,
                                        args,
                                        &declaration.param_modes,
                                        &declaration.params,
                                        program,
                                    ) && xml_call_arguments_valid(
                                        args,
                                        &declaration.params,
                                        &declaration.param_modes,
                                    )
                                } else {
                                    false
                                }
                            },
                            |facts| {
                                let deferred_borrowed = args.iter().any(|operand| {
                                    xml_borrowed_descriptor_path_valid(&access_graph, operand)
                                });
                                let mode_valid = deferred_borrowed || direct_operands_match_modes(
                                    target,
                                    args,
                                    &facts.modes,
                                    &facts.params,
                                    program,
                                );
                                let producer_valid =
                                    xml_call_arguments_valid(args, &facts.params, &facts.modes);
                                mode_valid && producer_valid
                            },
                        )
                    }
                    Rvalue::CallWithCleanup(call) => {
                        xml_direct_call_facts(program, &call.target, local_contracts).map_or_else(
                            || !protected_call_boundary(&call.args),
                            |facts| {
                                (call.args.iter().any(|operand| {
                                    xml_borrowed_descriptor_path_valid(&access_graph, operand)
                                }) || direct_operands_match_modes(
                                    &call.target,
                                    &call.args,
                                    &facts.modes,
                                    &facts.params,
                                    program,
                                )) && xml_call_arguments_valid(
                                    &call.args,
                                    &facts.params,
                                    &facts.modes,
                                )
                            },
                        )
                    }
                    Rvalue::CallIndirect {
                        callee,
                        args,
                        param_tys,
                        ret_ty,
                        signature,
                        ..
                    } => {
                        indirect_call_valid(callee, param_tys, *ret_ty, signature)
                            && operands_match_modes(
                                args,
                                &signature.param_modes,
                                param_tys,
                                program,
                            )
                            && xml_call_arguments_valid(
                                args,
                                param_tys,
                                &signature.param_modes,
                            )
                    }
                    Rvalue::CallIndirectWithCleanup(call) => {
                        indirect_call_valid(
                            &call.callee,
                            &call.param_tys,
                            call.ret_ty,
                            &call.signature,
                        ) && operands_match_modes(
                            &call.args,
                            &call.signature.param_modes,
                            &call.param_tys,
                            program,
                        ) && xml_call_arguments_valid(
                            &call.args,
                            &call.param_tys,
                            &call.signature.param_modes,
                        )
                    }
                    _ => true,
                };
                if !call_arguments_valid {
                    return Err(fail(
                        function,
                        "XML-capable call argument provenance mismatch",
                    ));
                }
                if matches!(
                    rvalue,
                    Rvalue::XmlParse { .. }
                        | Rvalue::XmlNext { .. }
                        | Rvalue::XmlName { .. }
                        | Rvalue::XmlAttributeCount(_)
                        | Rvalue::XmlAttributeName { .. }
                        | Rvalue::XmlAttributeValue { .. }
                        | Rvalue::XmlText { .. }
                ) && primary_definitions.get(*value as usize) != Some(&1)
                {
                    return Err(fail(function, "XML primary result definition is not unique"));
                }
                let valid = match rvalue {
                    Rvalue::TimeFormat { out, .. } | Rvalue::TimeParse { out, .. } => {
                        let output_ty = if matches!(rvalue, Rvalue::TimeFormat { .. }) { Ty::String } else { i64_ty };
                        // Scalar parser outputs also require the complete native producer proof:
                        // they do not otherwise reach the protected string/callable leaf walk.
                        result == i32_ty && matches!(
                            xml_slot_path_access(&access_graph, *out, output_ty, Vec::new()),
                            XmlProducerState::Present(XmlAccessProvenance::Owned)
                        )
                    }
                    Rvalue::RawIsNull(pointer) => {
                        operand_ty(function, pointer) == Some(Ty::Raw) && result == Ty::Bool
                    }
                    Rvalue::ResourceFromRaw {
                        raw,
                        resource,
                        parent,
                        abort_on_null,
                    } => {
                        program.resources.get(*resource as usize).is_some()
                            && operand_ty(function, raw) == Some(Ty::Raw)
                            && result == Ty::Resource(*resource)
                            && *abort_on_null
                            && parent.as_ref().is_none_or(|parent| {
                                matches!(operand_ty(function, parent), Some(Ty::ResourceRef(id))
                                    if program.resources.get(id as usize).is_some())
                            })
                    }
                    Rvalue::ResourceBorrow { owner, resource } => {
                        program.resources.get(*resource as usize).is_some()
                            && operand_ty(function, owner) == Some(Ty::Resource(*resource))
                            && result == Ty::ResourceRef(*resource)
                    }
                    Rvalue::ResourceRaw {
                        reference,
                        resource,
                    } => {
                        program.resources.get(*resource as usize).is_some()
                            && operand_ty(function, reference) == Some(Ty::ResourceRef(*resource))
                            && result == Ty::Raw
                    }
                    Rvalue::ResourceIntoRaw { owner, resource } => {
                        program.resources.get(*resource as usize).is_some()
                            && operand_ty(function, owner) == Some(Ty::Resource(*resource))
                            && result == Ty::Raw
                    }
                    Rvalue::XmlParse {
                        input,
                        error_enum,
                        cleanup,
                    } => {
                        let input_state = xml_access(input, Ty::String);
                        canonical_xml_value(input)
                            && operand_ty(function, input) == Some(Ty::String)
                            && input_state == XmlProducerState::Present(XmlAccessProvenance::Owned)
                            && result
                                == Ty::Result(Scalar::XmlReader, Scalar::Enum(*error_enum))
                            && builtin_error_enum_is_exact(program, *error_enum)
                            && *cleanup != *value
                            && function.value_tys.get(*cleanup as usize) == Some(&Ty::Bool)
                            && primary_definitions.get(*cleanup as usize) == Some(&0)
                            && auxiliary_definitions.get(*cleanup as usize) == Some(&1)
                    }
                    Rvalue::XmlNext { reader, event_enum } => {
                        canonical_xml_value(reader)
                            && operand_ty(function, reader) == Some(Ty::XmlReader)
                            && matches!(
                                xml_access(reader, Ty::XmlReader),
                                XmlProducerState::Present(
                                    XmlAccessProvenance::Owned
                                        | XmlAccessProvenance::Exclusive
                                )
                            )
                            && result == Ty::Option(Scalar::Enum(*event_enum))
                            && xml_event_definition_valid(program, *event_enum)
                    }
                    Rvalue::XmlAttributeCount(reader) => {
                        canonical_xml_value(reader)
                            && operand_ty(function, reader) == Some(Ty::XmlReader)
                            && matches!(
                                xml_access(reader, Ty::XmlReader),
                                XmlProducerState::Present(
                                    XmlAccessProvenance::Owned
                                        | XmlAccessProvenance::Shared
                                        | XmlAccessProvenance::Exclusive
                                )
                            )
                            && result == i64_ty
                    }
                    Rvalue::XmlName { reader } => {
                        canonical_xml_value(reader)
                            && operand_ty(function, reader) == Some(Ty::XmlReader)
                            && matches!(
                                xml_access(reader, Ty::XmlReader),
                                XmlProducerState::Present(
                                    XmlAccessProvenance::Owned
                                        | XmlAccessProvenance::Shared
                                        | XmlAccessProvenance::Exclusive
                                )
                            )
                            && result == Ty::Str
                    }
                    Rvalue::XmlAttributeName { reader, index } => {
                        canonical_xml_value(reader)
                            && operand_ty(function, reader) == Some(Ty::XmlReader)
                            && matches!(
                                xml_access(reader, Ty::XmlReader),
                                XmlProducerState::Present(
                                    XmlAccessProvenance::Owned
                                        | XmlAccessProvenance::Shared
                                        | XmlAccessProvenance::Exclusive
                                )
                            )
                            && operand_ty(function, index) == Some(i64_ty)
                            && readable_xml_index(index)
                            && result == Ty::Str
                    }
                    Rvalue::XmlAttributeValue { reader, index } => {
                        canonical_xml_value(reader)
                            && operand_ty(function, reader) == Some(Ty::XmlReader)
                            && matches!(
                                xml_access(reader, Ty::XmlReader),
                                XmlProducerState::Present(
                                    XmlAccessProvenance::Owned
                                        | XmlAccessProvenance::Shared
                                        | XmlAccessProvenance::Exclusive
                                )
                            )
                            && operand_ty(function, index) == Some(i64_ty)
                            && readable_xml_index(index)
                            && result == Ty::String
                    }
                    Rvalue::XmlText { reader } => {
                        canonical_xml_value(reader)
                            && operand_ty(function, reader) == Some(Ty::XmlReader)
                            && matches!(
                                xml_access(reader, Ty::XmlReader),
                                XmlProducerState::Present(
                                    XmlAccessProvenance::Owned
                                        | XmlAccessProvenance::Shared
                                        | XmlAccessProvenance::Exclusive
                                )
                            )
                            && result == Ty::String
                    }
                    Rvalue::GroupAgg { .. }
                    | Rvalue::GroupAggStrCols { .. }
                    | Rvalue::GroupAggStr { .. }
                    | Rvalue::GroupAggMultiStr { .. }
                    | Rvalue::DictEncode { .. }
                    | Rvalue::GatherColumnI64 { .. }
                    | Rvalue::DictLookup { .. } => matches!(
                        xml_access(&Operand::Value(*value), result),
                        XmlProducerState::Present(
                            XmlAccessProvenance::Owned
                                | XmlAccessProvenance::Shared
                                | XmlAccessProvenance::Exclusive
                        )
                    ),
                    Rvalue::ResourceViewFromRaw {
                        owner,
                        ptr,
                        len,
                        resource,
                        view,
                        allow_null_if_empty,
                        check_nonnegative_len,
                        check_alignment,
                        check_utf8,
                    } => {
                        let expected = match view {
                            hir::ResourceViewKind::StrUtf8 => Some((Ty::Option(Scalar::Str), 1, true)),
                            hir::ResourceViewKind::Slice(Scalar::Int(integer)) => {
                                align_sema::scalar_to_prim(Scalar::Int(*integer)).map(|primitive| {
                                    (
                                        Ty::Option(Scalar::Slice(primitive)),
                                        u32::from(integer.bits) / 8,
                                        false,
                                    )
                                })
                            }
                            hir::ResourceViewKind::Slice(Scalar::Float(float)) => {
                                align_sema::scalar_to_prim(Scalar::Float(*float)).map(|primitive| {
                                    (
                                        Ty::Option(Scalar::Slice(primitive)),
                                        u32::from(float.bits) / 8,
                                        false,
                                    )
                                })
                            }
                            hir::ResourceViewKind::Slice(_) => None,
                        };
                        expected.is_some_and(|(expected, alignment, utf8)| {
                            program.resources.get(*resource as usize).is_some()
                                && operand_ty(function, owner)
                                    == Some(Ty::ResourceRef(*resource))
                                && operand_ty(function, ptr) == Some(Ty::Raw)
                                && operand_ty(function, len)
                                    == Some(Ty::Int(IntTy {
                                        bits: 64,
                                        signed: true,
                                    }))
                                && result == expected
                                && *allow_null_if_empty
                                && *check_nonnegative_len
                                && *check_alignment == alignment
                                && *check_utf8 == utf8
                        })
                    }
                    Rvalue::ColumnBatchCreate {
                        max_rows,
                        struct_id,
                    } => {
                        batch_fields(*struct_id).is_some()
                            && operand_ty(function, max_rows) == Some(i64_ty)
                            && result == Ty::Raw
                    }
                    Rvalue::ColumnBatchAppend {
                        payload,
                        inputs,
                        struct_id,
                    } => batch_fields(*struct_id).is_some_and(|fields| {
                        operand_ty(function, payload) == Some(Ty::Raw)
                            && result == i32_ty
                            && inputs.len() == fields.len()
                            && inputs.iter().zip(fields).all(|(input, (base, _, view))| {
                                match input {
                                    crate::ColumnBatchInput::Scalar(value) => {
                                        !view
                                            && align_sema::ty_to_scalar(base).is_some_and(|base| {
                                                operand_ty(function, value)
                                                    == Some(Ty::Option(base))
                                            })
                                    }
                                    crate::ColumnBatchInput::View { ptr, len } => {
                                        view
                                            && operand_ty(function, ptr) == Some(Ty::Raw)
                                            && operand_ty(function, len) == Some(i64_ty)
                                    }
                                }
                            })
                    }),
                    Rvalue::ColumnBatchRow {
                        payload,
                        owner,
                        index,
                        struct_id,
                        resource,
                    } => {
                        batch_fields(*struct_id).is_some()
                            && db_resource_matches_row(program, *resource, *struct_id, "batch")
                            && operand_ty(function, payload) == Some(Ty::Raw)
                            && operand_ty(function, owner)
                                == Some(Ty::ResourceRef(*resource))
                            && operand_ty(function, index) == Some(i64_ty)
                            && result == Ty::Struct(*struct_id)
                    }
                    Rvalue::ColumnBatchSoa {
                        payload,
                        owner,
                        struct_id,
                        resource,
                    } => batch_fields(*struct_id).is_some()
                        && program.structs.get(*struct_id as usize).is_some_and(|definition| {
                            !definition.fields.is_empty()
                                && definition.fields.iter().all(|field| {
                                    matches!(
                                        field.ty,
                                        Ty::Bool
                                            | Ty::Char
                                            | Ty::Int(_)
                                            | Ty::Float(_)
                                            | Ty::Str
                                    )
                                })
                        })
                            && db_resource_matches_row(program, *resource, *struct_id, "batch")
                            && operand_ty(function, payload) == Some(Ty::Raw)
                            && operand_ty(function, owner)
                                == Some(Ty::ResourceRef(*resource))
                            && result == Ty::Soa(*struct_id)
                    ,
                    Rvalue::TemplateHtmlNew { resource } => {
                        function.name.as_str() == "pkg.template$html"
                            && template_html_resource_matches(program, *resource)
                            && result == Ty::Resource(*resource)
                    }
                    Rvalue::TemplateHtmlWrite {
                        resource,
                        output,
                        value,
                    } => {
                        function.name.as_str() == "pkg.template$write"
                            && template_html_resource_matches(program, *resource)
                            && operand_ty(function, output) == Some(Ty::Resource(*resource))
                            && operand_ty(function, value) == Some(Ty::Str)
                            && result == Ty::Unit
                    }
                    Rvalue::TemplateHtmlRaw {
                        resource,
                        output,
                        value,
                    } => {
                        function.name.as_str() == "pkg.template$raw"
                            && template_html_resource_matches(program, *resource)
                            && operand_ty(function, output) == Some(Ty::Resource(*resource))
                            && operand_ty(function, value) == Some(Ty::Str)
                            && result == Ty::Unit
                    }
                    Rvalue::TemplateHtmlToString { resource, output } => {
                        function.name.as_str() == "pkg.template$to_string"
                            && template_html_resource_matches(program, *resource)
                            && operand_ty(function, output) == Some(Ty::Resource(*resource))
                            && result == Ty::String
                    }
                    _ => continue,
                };
                if !valid {
                    let detail = if matches!(
                        rvalue,
                        Rvalue::XmlParse { .. }
                            | Rvalue::XmlNext { .. }
                            | Rvalue::XmlName { .. }
                            | Rvalue::XmlAttributeCount(_)
                            | Rvalue::XmlAttributeName { .. }
                            | Rvalue::XmlAttributeValue { .. }
                            | Rvalue::XmlText { .. }
                    ) {
                        "XML operation contract mismatch"
                    } else {
                        "resource operation contract mismatch"
                    };
                    return Err(fail(function, detail));
                }
            }
        }
    }
    Ok(())
}

/// Validate producer-owned resource records before LLVM symbols or types are created. Normal HIR
/// lowering constructs these fields canonically; cached or hand-built MIR must not be able to
/// redirect Drop to an arbitrary symbol, alias two incompatible resources onto one thunk, or call
/// a hook through a disagreeing ABI.
pub fn validate_resource_program(program: &Program) -> Result<(), ProducerError> {
    let fail = |detail: String| {
        ProducerError::Lowering(format!("resource metadata is malformed: {detail}"))
    };
    let valid_text = |value: &str| !value.is_empty() && !value.as_bytes().contains(&0);
    let mut names = std::collections::HashSet::new();
    let mut source_names = std::collections::HashSet::new();
    let mut thunks = std::collections::HashMap::new();

    for resource in &program.resources {
        for (field, value) in [
            ("name", resource.name.as_str()),
            ("source name", resource.source_name.as_str()),
            ("declaring module", resource.declaring_module.as_str()),
            ("Drop hook", resource.drop_hook.as_str()),
            ("Drop thunk", resource.drop_thunk.as_str()),
        ] {
            if !valid_text(value) {
                return Err(fail(format!("resource {field} is empty or contains NUL")));
            }
        }
        if !names.insert(resource.name.as_str()) {
            return Err(fail(format!(
                "duplicate concrete resource name '{}'",
                resource.name
            )));
        }
        if !source_names.insert(resource.source_name.as_str()) {
            return Err(fail(format!(
                "duplicate resource source identity '{}'",
                resource.source_name
            )));
        }
        if resource.representation_version != 1 {
            return Err(fail(format!(
                "resource '{}' uses representation version {}",
                resource.source_name, resource.representation_version
            )));
        }
        if resource.drop_abi_fingerprint != *b"align-res-drop-1" {
            return Err(fail(format!(
                "resource '{}' has an unsupported Drop ABI fingerprint",
                resource.source_name
            )));
        }
        if !resource.drop_thunk.starts_with("__align_resource_drop$") {
            return Err(fail(format!(
                "resource '{}' has a noncanonical Drop thunk",
                resource.source_name
            )));
        }
        if resource.generic_arity == 0
            && resource.drop_thunk
                != format!("__align_resource_drop${}", resource.source_name)
        {
            return Err(fail(format!(
                "resource '{}' Drop thunk disagrees with its nominal identity",
                resource.source_name
            )));
        }
        let thunk_contract = (
            resource.drop_hook.as_str(),
            resource.representation_version,
            resource.drop_abi_fingerprint,
        );
        if let Some(previous) = thunks.insert(resource.drop_thunk.as_str(), thunk_contract)
            && previous != thunk_contract
        {
            return Err(fail(format!(
                "Drop thunk '{}' is shared by incompatible resource records",
                resource.drop_thunk
            )));
        }

        if let Some(function) = program
            .fns
            .iter()
            .find(|function| function.name.as_str() == resource.drop_hook)
        {
            let params = function
                .params
                .iter()
                .filter_map(|slot| function.slots.get(*slot as usize))
                .copied()
                .collect::<Vec<_>>();
            if params != [Ty::Raw]
                || function.params.len() != 1
                || function.param_modes != [align_ast::ParamMode::ByValue]
                || function.ret != Ty::Unit
            {
                return Err(fail(format!(
                    "resource '{}' Drop hook has a disagreeing stored-function ABI",
                    resource.source_name
                )));
            }
        }
        if let Some(function) = program
            .imported_fns
            .iter()
            .find(|function| function.name.as_str() == resource.drop_hook)
            && (function.params != [Ty::Raw]
                || function.param_modes != [align_ast::ParamMode::ByValue]
                || function.ret != Ty::Unit)
        {
            return Err(fail(format!(
                "resource '{}' Drop hook has a disagreeing imported-function ABI",
                resource.source_name
            )));
        }
    }
    Ok(())
}

/// Validate the nested tagged table before any LLVM type lookup. This is the fail-closed seam for
/// malformed cached/interface-derived MIR: an absent id, abstract parameter, or inline cycle is a
/// `ProducerError`, never an out-of-bounds panic or the scalar `i32` fallback.
pub fn validate_tagged_program(program: &Program) -> Result<(), ProducerError> {
    validate_tagged_program_inner(program, true)
}

pub fn validate_partition_tagged_program(program: &Program) -> Result<(), ProducerError> {
    validate_tagged_program_inner(program, false)
}

fn validate_tagged_program_inner(
    program: &Program,
    require_compact_tables: bool,
) -> Result<(), ProducerError> {
    struct SignatureFacts<'a> {
        owner: &'a str,
        modes: &'a [align_ast::ParamMode],
        param_types: &'a [Ty],
        ret: Ty,
        borrow: &'a hir::ReturnBorrowSummary,
        region: &'a hir::ReturnRegionSummary,
        cleanup: hir::ReturnCleanupAbi,
        allow_out: bool,
        allow_return_roots: bool,
    }

    fn check_signature_facts(
        facts: SignatureFacts<'_>,
        program: &Program,
        type_graph: &mut TypeGraph<'_>,
    ) -> Result<(), ProducerError> {
        let SignatureFacts {
            owner,
            modes,
            param_types,
            ret,
            borrow,
            region,
            cleanup,
            allow_out,
            allow_return_roots,
        } = facts;
        if modes.len() != param_types.len() {
            return Err(ProducerError::Lowering(format!(
                "{owner} has {} parameter modes for {} parameters",
                modes.len(),
                param_types.len()
            )));
        }
        type_graph.check_ty(ret)?;
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
            return Err(ProducerError::Lowering(format!(
                "{owner} return cleanup ABI disagrees with its return type"
            )));
        }
        for &ty in param_types {
            type_graph.check_ty(ty)?;
        }
        for (&mode, &ty) in modes.iter().zip(param_types) {
            match mode {
                align_ast::ParamMode::ByValue => {}
                align_ast::ParamMode::Out if allow_out => {}
                align_ast::ParamMode::Out => {
                    return Err(ProducerError::Lowering(format!(
                        "{owner} uses `out` in a function-value ABI"
                    )));
                }
                align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut
                    if ty == Ty::ArenaHandle =>
                {
                    return Err(ProducerError::Lowering(format!(
                        "{owner} borrows a region capability instead of passing it by value"
                    )));
                }
                align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut => {}
            }
        }
        let validate_summary =
            |kind: &str, params: &[u32], captures: &[u32]| -> Result<(), ProducerError> {
                if params.is_empty() && captures.is_empty() {
                    return Err(ProducerError::Lowering(format!(
                        "{owner} has a non-canonical empty {kind} root set"
                    )));
                }
                if params.windows(2).any(|pair| pair[0] >= pair[1])
                    || captures.windows(2).any(|pair| pair[0] >= pair[1])
                {
                    return Err(ProducerError::Lowering(format!(
                        "{owner} has unsorted or duplicate {kind} roots"
                    )));
                }
                if params
                    .iter()
                    .any(|root| *root as usize >= param_types.len())
                {
                    return Err(ProducerError::Lowering(format!(
                        "{owner} has an out-of-range {kind} parameter root"
                    )));
                }
                Ok(())
            };
        if let hir::ReturnBorrowSummary::Roots { params, captures } = borrow {
            validate_summary("return-borrow", params, captures)?;
        }
        if let hir::ReturnRegionSummary::Roots { params, captures } = region {
            validate_summary("return-region", params, captures)?;
        }
        let summaries_agree = match (borrow, region) {
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
        };
        if !summaries_agree {
            return Err(ProducerError::Lowering(format!(
                "{owner} has disagreeing return-borrow and return-region roots in L2b-a1"
            )));
        }
        if !allow_return_roots
            && (!matches!(borrow, hir::ReturnBorrowSummary::None)
                || !matches!(region, hir::ReturnRegionSummary::None))
        {
            return Err(ProducerError::Lowering(format!(
                "{owner} cannot carry return provenance across an unanalyzed extern boundary"
            )));
        }
        if let hir::ReturnBorrowSummary::Roots { params, .. } = borrow {
            if !align_sema::ty_may_borrow(
                ret,
                &program.structs,
                &program.tuples,
                &program.enums,
                &program.tagged_types,
            ) {
                return Err(ProducerError::Lowering(format!(
                    "{owner} has return provenance on type {ret:?}, which cannot borrow"
                )));
            }
            for &root in params {
                let ty = param_types[root as usize];
                let borrowed_owner = matches!(
                    modes[root as usize],
                    align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut
                );
                if !borrowed_owner
                    && !align_sema::ty_may_borrow(
                        ty,
                        &program.structs,
                        &program.tuples,
                        &program.enums,
                        &program.tagged_types,
                    )
                {
                    return Err(ProducerError::Lowering(format!(
                        "{owner} return provenance root {root} has type {ty:?}, which cannot supply a borrow"
                    )));
                }
            }
        }
        Ok(())
    }

    fn check_mode_types(
        owner: &str,
        modes: &[align_ast::ParamMode],
        types: &[Ty],
    ) -> Result<(), ProducerError> {
        for (index, (mode, ty)) in modes.iter().zip(types).enumerate() {
            if *mode == align_ast::ParamMode::Out && !matches!(ty, Ty::Slice(_)) {
                return Err(ProducerError::Lowering(format!(
                    "{owner} parameter {index} has mode Out but type {ty:?}, not a slice"
                )));
            }
        }
        Ok(())
    }

    fn main_result_is_exact(program: &Program, ret: Ty) -> bool {
        matches!(
            ret,
            Ty::Result(Scalar::Unit, Scalar::Enum(id))
                if builtin_error_enum_is_exact(program, id)
        )
    }

    fn validate_main_abi(
        function: &Function,
        param_types: &[Ty],
        program: &Program,
    ) -> Result<(), ProducerError> {
        if function.name.as_str() != "main" {
            return Ok(());
        }
        if function.exportable {
            return Err(ProducerError::Lowering(
                "entry function `main` cannot be a per-unit export".to_string(),
            ));
        }
        let exact_i32 = Ty::Int(IntTy {
            bits: 32,
            signed: true,
        });
        let valid = match param_types {
            [] => {
                function.ret == Ty::Unit
                    || function.ret == exact_i32
                    || main_result_is_exact(program, function.ret)
            }
            [Ty::DynArray(Scalar::Str)] => {
                function.param_modes.as_slice() == [align_ast::ParamMode::ByValue]
                    && main_result_is_exact(program, function.ret)
            }
            _ => false,
        };
        if !valid {
            return Err(ProducerError::Lowering(format!(
                "entry function `main` has invalid C ABI: modes {:?}, parameters {param_types:?}, return {:?}",
                function.param_modes, function.ret
            )));
        }
        Ok(())
    }

    type NamedSignature<'a> = (
        Vec<Ty>,
        Ty,
        &'a [align_ast::ParamMode],
        &'a hir::ReturnBorrowSummary,
        &'a hir::ReturnRegionSummary,
        hir::ReturnCleanupAbi,
    );

    fn named_signature<'a>(
        program: &'a Program,
        name: &ProgramCall,
    ) -> Option<NamedSignature<'a>> {
        if let Some(function) = program.fns.iter().find(|function| &function.name == name) {
            return Some((
                function
                    .params
                    .iter()
                    .map(|slot| function.slots.get(*slot as usize).copied())
                    .collect::<Option<Vec<_>>>()?,
                function.ret,
                &function.param_modes,
                &function.return_borrow,
                &function.return_region,
                function.return_cleanup,
            ));
        }
        if let Some(function) = program
            .imported_fns
            .iter()
            .find(|function| &function.name == name)
        {
            return Some((
                function.params.clone(),
                function.ret,
                &function.param_modes,
                &function.return_borrow,
                &function.return_region,
                function.return_cleanup,
            ));
        }
        program
            .externs
            .iter()
            .find(|function| &function.name == name)
            .map(|function| {
                (
                    function.params.clone(),
                    function.ret,
                    function.param_modes.as_slice(),
                    &function.return_borrow,
                    &function.return_region,
                    function.return_cleanup,
                )
            })
    }

    /// Validate every MIR type reference and the complete inline layout graph before any semantic
    /// classifier or LLVM type constructor sees it. Pointer/header wrappers still validate their
    /// referenced ids, but they break the inline-cycle path; fixed arrays, tuples, tagged values,
    /// structs, and sums preserve it.
    enum TypeGraphWork {
        Ty(Ty),
        InlineScalar(Scalar),
        ExitStruct(u32),
        ExitEnum(u32),
        ExitTuple(u32),
        ExitTagged(u32),
    }

    struct TypeGraph<'a> {
        program: &'a Program,
        active_structs: HashSet<u32>,
        completed_structs: HashSet<u32>,
        active_enums: HashSet<u32>,
        completed_enums: HashSet<u32>,
        active_tuples: HashSet<u32>,
        completed_tuples: HashSet<u32>,
        active_tagged: HashSet<u32>,
        completed_tagged: HashSet<u32>,
    }

    impl<'a> TypeGraph<'a> {
        fn new(program: &'a Program) -> Self {
            Self {
                program,
                active_structs: HashSet::new(),
                completed_structs: HashSet::new(),
                active_enums: HashSet::new(),
                completed_enums: HashSet::new(),
                active_tuples: HashSet::new(),
                completed_tuples: HashSet::new(),
                active_tagged: HashSet::new(),
                completed_tagged: HashSet::new(),
            }
        }

        fn invalid(message: String) -> ProducerError {
            ProducerError::Lowering(message)
        }

        fn require_struct(&self, id: u32) -> Result<(), ProducerError> {
            self.program.structs.get(id as usize).map(|_| ()).ok_or_else(|| {
                Self::invalid(format!("tagged payload struct type id {id} is missing"))
            })
        }

        fn require_enum(&self, id: u32) -> Result<(), ProducerError> {
            self.program.enums.get(id as usize).map(|_| ()).ok_or_else(|| {
                Self::invalid(format!("tagged payload sum type id {id} is missing"))
            })
        }

        fn require_tuple(&self, id: u32) -> Result<(), ProducerError> {
            self.program.tuples.get(id as usize).map(|_| ()).ok_or_else(|| {
                Self::invalid(format!("tuple type id {id} is missing"))
            })
        }

        fn require_tagged(&self, id: u32) -> Result<(), ProducerError> {
            self.program
                .tagged_types
                .get(id as usize)
                .map(|_| ())
                .ok_or_else(|| Self::invalid(format!("nested tagged type id {id} is missing")))
        }

        fn require_resource(&self, id: u32) -> Result<(), ProducerError> {
            self.program
                .resources
                .get(id as usize)
                .map(|_| ())
                .ok_or_else(|| Self::invalid(format!("resource type id {id} is missing")))
        }

        fn enter_struct(
            &mut self,
            id: u32,
            work: &mut Vec<TypeGraphWork>,
        ) -> Result<(), ProducerError> {
            self.require_struct(id)?;
            if self.completed_structs.contains(&id) {
                return Ok(());
            }
            if !self.active_structs.insert(id) {
                return Err(Self::invalid(format!(
                    "struct type id {id} has a missing or recursive by-value definition"
                )));
            }
            work.push(TypeGraphWork::ExitStruct(id));
            for field in self.program.structs[id as usize].fields.iter().rev() {
                work.push(TypeGraphWork::Ty(field.ty));
            }
            Ok(())
        }

        fn enter_enum(
            &mut self,
            id: u32,
            work: &mut Vec<TypeGraphWork>,
        ) -> Result<(), ProducerError> {
            self.require_enum(id)?;
            if self.completed_enums.contains(&id) {
                return Ok(());
            }
            if !self.active_enums.insert(id) {
                return Err(Self::invalid(format!(
                    "sum type id {id} has a missing or recursive by-value definition"
                )));
            }
            work.push(TypeGraphWork::ExitEnum(id));
            for variant in self.program.enums[id as usize].variants.iter().rev() {
                for &payload in variant.payload.iter().rev() {
                    work.push(TypeGraphWork::InlineScalar(payload));
                }
            }
            Ok(())
        }

        fn enter_tuple(
            &mut self,
            id: u32,
            work: &mut Vec<TypeGraphWork>,
        ) -> Result<(), ProducerError> {
            self.require_tuple(id)?;
            if self.completed_tuples.contains(&id) {
                return Ok(());
            }
            if !self.active_tuples.insert(id) {
                return Err(Self::invalid(format!(
                    "tuple type id {id} has a missing or recursive by-value definition"
                )));
            }
            work.push(TypeGraphWork::ExitTuple(id));
            for &element in self.program.tuples[id as usize].elems.iter().rev() {
                work.push(TypeGraphWork::InlineScalar(element));
            }
            Ok(())
        }

        fn enter_tagged(
            &mut self,
            id: u32,
            work: &mut Vec<TypeGraphWork>,
        ) -> Result<(), ProducerError> {
            self.require_tagged(id)?;
            if self.completed_tagged.contains(&id) {
                return Ok(());
            }
            if !self.active_tagged.insert(id) {
                return Err(Self::invalid(format!(
                    "nested tagged type id {id} has a missing or recursive by-value definition"
                )));
            }
            work.push(TypeGraphWork::ExitTagged(id));
            match self.program.tagged_types[id as usize] {
                hir::TaggedType::Option(payload) => {
                    work.push(TypeGraphWork::InlineScalar(payload));
                }
                hir::TaggedType::Result(ok, err) => {
                    work.push(TypeGraphWork::InlineScalar(err));
                    work.push(TypeGraphWork::InlineScalar(ok));
                }
            }
            Ok(())
        }

        fn check_scalar_reference(&self, scalar: Scalar) -> Result<(), ProducerError> {
            match scalar {
                Scalar::Struct(id) | Scalar::DynStructArray(id) | Scalar::Soa(id) => {
                    self.require_struct(id)
                }
                Scalar::Enum(id) => self.require_enum(id),
                Scalar::Tagged(id) => self.require_tagged(id),
                Scalar::Resource(id) | Scalar::ResourceRef(id) => self.require_resource(id),
                Scalar::Param(id) => Err(Self::invalid(format!(
                    "abstract tagged payload parameter {id} survived into MIR"
                ))),
                Scalar::SoaParam(id) => Err(Self::invalid(format!(
                    "abstract soa payload parameter {id} survived into MIR"
                ))),
                Scalar::Int(_)
                | Scalar::Float(_)
                | Scalar::Bool
                | Scalar::Char
                | Scalar::Unit
                | Scalar::String
                | Scalar::DynArray(_)
                | Scalar::DynResponseArray
                | Scalar::Str
                | Scalar::Slice(_)
                | Scalar::JsonDoc
                | Scalar::Reader
                | Scalar::Writer
                | Scalar::Logger
                | Scalar::XmlReader
                | Scalar::Buffer
                | Scalar::CodecBatch
                | Scalar::CodecI64Column
                | Scalar::CodecF64Column
                | Scalar::CodecBoolColumn
                | Scalar::CodecStrColumn
                | Scalar::CryptoDigest
                | Scalar::FsDirectory
                | Scalar::FsDirCursor
                | Scalar::ProcessSignalSubscription | Scalar::ProcessChildScope | Scalar::ProcessMember | Scalar::FsMemoryWriter | Scalar::FsSealedFile | Scalar::ProcessImage | Scalar::ProcessUserNamespace | Scalar::Command
                | Scalar::CodecEncoder
                | Scalar::SignatureKey(_)
                | Scalar::Regex
                | Scalar::Captures
                | Scalar::CliParsed
                | Scalar::TcpConn
                | Scalar::TcpListener
                | Scalar::UdpSocket
                | Scalar::Child
                | Scalar::File
                | Scalar::HttpClient
                | Scalar::HttpRequest
                | Scalar::HttpResponse
                | Scalar::HttpServer
                | Scalar::HttpRequestCtx
                | Scalar::ResponseBuilder
                | Scalar::HttpStream
                | Scalar::HttpUpgrade
                | Scalar::HttpReadStream
                | Scalar::HttpSseStream
                | Scalar::RunOutput
                | Scalar::RunBytes
                // MIR carries no function-type table; the embedded signature facts are validated
                // at every function-value producer/consumer, and the physical closure ABI is
                // independent of this sema-local identity.
                | Scalar::Fn(_) => Ok(()),
            }
        }

        fn check_ty(&mut self, ty: Ty) -> Result<(), ProducerError> {
            let mut work = vec![TypeGraphWork::Ty(ty)];
            while let Some(next) = work.pop() {
                match next {
                    TypeGraphWork::Ty(ty) => match ty {
                        Ty::Param(id) => {
                            return Err(Self::invalid(format!(
                                "abstract type parameter {id} survived into MIR"
                            )));
                        }
                        Ty::SoaParam(id) => {
                            return Err(Self::invalid(format!(
                                "abstract soa type parameter {id} survived into MIR"
                            )));
                        }
                        Ty::IntVar(id) => {
                            return Err(Self::invalid(format!(
                                "unresolved integer type variable {id} survived into MIR"
                            )));
                        }
                        Ty::FloatVar(id) => {
                            return Err(Self::invalid(format!(
                                "unresolved float type variable {id} survived into MIR"
                            )));
                        }
                        Ty::Error => {
                            return Err(Self::invalid(
                                "error-sentinel type survived into MIR".to_string(),
                            ));
                        }
                        Ty::Struct(id) => self.enter_struct(id, &mut work)?,
                        Ty::Enum(id) => self.enter_enum(id, &mut work)?,
                        Ty::Tuple(id) => self.enter_tuple(id, &mut work)?,
                        Ty::Tagged(id) => self.enter_tagged(id, &mut work)?,
                        Ty::Resource(id) | Ty::ResourceRef(id) => self.require_resource(id)?,
                        Ty::Option(payload)
                        | Ty::Array(payload, _)
                        | Ty::Vec(payload, _)
                        | Ty::Mask(payload, _) => {
                            work.push(TypeGraphWork::InlineScalar(payload));
                        }
                        Ty::Result(ok, err) => {
                            work.push(TypeGraphWork::InlineScalar(err));
                            work.push(TypeGraphWork::InlineScalar(ok));
                        }
                        Ty::StructArray(id, _) => self.enter_struct(id, &mut work)?,
                        Ty::Box(payload)
                        | Ty::Slice(payload)
                        | Ty::DynArray(payload)
                        | Ty::Task(payload) => self.check_scalar_reference(payload)?,
                        Ty::ArrayBuilder(payload) => {
                            self.check_scalar_reference(payload)?
                        }
                        ty @ (Ty::VecArrayBuilder(..)
                        | Ty::MaskArrayBuilder(..)
                        | Ty::FixedArrayBuilder(..)
                        | Ty::FixedStructArrayBuilder(..)
                        | Ty::DynVecArray(..)
                        | Ty::DynMaskArray(..)
                        | Ty::DynFixedArray(..)
                        | Ty::DynFixedStructArray(..)) => {
                            let payload = ty
                                .array_builder_element()
                                .and_then(|element| match element {
                                    ArrayBuilderElem::Aggregate(element) => Some(element),
                                    ArrayBuilderElem::Scalar(_) => None,
                                })
                                .or_else(|| ty.dyn_aggregate_array_element())
                                .expect("matched aggregate type");
                            if !align_sema::region_plain_type_ok(
                                payload.ty(),
                                &self.program.structs,
                                &self.program.enums,
                                &self.program.tagged_types,
                            ) {
                                return Err(Self::invalid(format!(
                                    "aggregate array element {:?} is not RegionPlain",
                                    payload.ty()
                                )));
                            }
                            work.push(TypeGraphWork::Ty(payload.ty()));
                        }
                        Ty::DynStructArray(id, _)
                        | Ty::Soa(id)
                        | Ty::JsonScanner(id) => self.require_struct(id)?,
                        Ty::DictEncoded(id, field) => {
                            self.require_struct(id)?;
                            if field as usize >= self.program.structs[id as usize].fields.len() {
                                return Err(Self::invalid(format!(
                                    "dictionary-encoded type refers to missing field {field} on struct type id {id}"
                                )));
                            }
                        }
                        Ty::Int(_)
                        | Ty::Float(_)
                        | Ty::Bool
                        | Ty::Char
                        | Ty::DynSliceArray(_)
                        | Ty::DynResponseArray
                        | Ty::Str
                        | Ty::String
                        | Ty::ArenaHandle
                        | Ty::Raw
                        | Ty::Builder
                        | Ty::Writer
                        | Ty::Reader
                        | Ty::Logger
                        | Ty::XmlReader
                        | Ty::Buffer
                        | Ty::CodecBatch
                        | Ty::CodecI64Column
                        | Ty::CodecF64Column
                        | Ty::CodecBoolColumn
                        | Ty::CodecStrColumn
                        | Ty::CryptoDigest
                        | Ty::FsDirectory
                        | Ty::FsDirCursor
                        | Ty::ProcessSignalSubscription | Ty::ProcessChildScope | Ty::ProcessMember | Ty::FsMemoryWriter | Ty::FsSealedFile | Ty::ProcessImage | Ty::ProcessUserNamespace
                        | Ty::CodecEncoder
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
                        | Ty::HttpUpgrade
                        | Ty::HttpReadStream
                        | Ty::HttpSseStream
                        | Ty::HttpHeaders
                        | Ty::JsonDoc
                        | Ty::Fn(_)
                        | Ty::Unit => {}
                    },
                    TypeGraphWork::InlineScalar(scalar) => match scalar {
                        Scalar::Struct(id) => self.enter_struct(id, &mut work)?,
                        Scalar::Enum(id) => self.enter_enum(id, &mut work)?,
                        Scalar::Tagged(id) => self.enter_tagged(id, &mut work)?,
                        Scalar::DynStructArray(id) | Scalar::Soa(id) => {
                            self.require_struct(id)?;
                        }
                        other => self.check_scalar_reference(other)?,
                    },
                    TypeGraphWork::ExitStruct(id) => {
                        if !self.active_structs.remove(&id) {
                            return Err(Self::invalid(format!(
                                "struct type id {id} exited validation without an active entry"
                            )));
                        }
                        self.completed_structs.insert(id);
                    }
                    TypeGraphWork::ExitEnum(id) => {
                        if !self.active_enums.remove(&id) {
                            return Err(Self::invalid(format!(
                                "sum type id {id} exited validation without an active entry"
                            )));
                        }
                        self.completed_enums.insert(id);
                    }
                    TypeGraphWork::ExitTuple(id) => {
                        if !self.active_tuples.remove(&id) {
                            return Err(Self::invalid(format!(
                                "tuple type id {id} exited validation without an active entry"
                            )));
                        }
                        self.completed_tuples.insert(id);
                    }
                    TypeGraphWork::ExitTagged(id) => {
                        if !self.active_tagged.remove(&id) {
                            return Err(Self::invalid(format!(
                                "nested tagged type id {id} exited validation without an active entry"
                            )));
                        }
                        self.completed_tagged.insert(id);
                    }
                }
            }
            Ok(())
        }
    }

    fn check_ty(
        ty: Ty,
        type_graph: &mut TypeGraph<'_>,
    ) -> Result<(), ProducerError> {
        type_graph.check_ty(ty)
    }

    enum SourceAbiKeyWork {
        Ty(Ty),
        Text(&'static str),
        ExitTagged(u32),
    }

    fn source_abi_key(
        ty: Ty,
        program: &Program,
    ) -> Result<String, ProducerError> {
        let mut key = String::new();
        let mut active_tagged = HashSet::new();
        let mut work = vec![SourceAbiKeyWork::Ty(ty)];
        while let Some(next) = work.pop() {
            match next {
                SourceAbiKeyWork::Text(text) => key.push_str(text),
                SourceAbiKeyWork::ExitTagged(id) => {
                    if !active_tagged.remove(&id) {
                        return Err(ProducerError::Lowering(format!(
                            "nested tagged type id {id} exited source ABI key generation without an active entry"
                        )));
                    }
                }
                SourceAbiKeyWork::Ty(ty) => match ty {
                    Ty::Struct(id) => {
                        let definition = program.structs.get(id as usize).ok_or_else(|| {
                            ProducerError::Lowering(format!(
                                "source ABI key refers to missing struct type id {id}"
                            ))
                        })?;
                        key.push('S');
                        key.push_str(&definition.source_name);
                    }
                    Ty::Enum(id) => {
                        let definition = program.enums.get(id as usize).ok_or_else(|| {
                            ProducerError::Lowering(format!(
                                "source ABI key refers to missing sum type id {id}"
                            ))
                        })?;
                        key.push('E');
                        key.push_str(&definition.source_name);
                    }
                    Ty::Fn(_) => key.push_str("FnClosure"),
                    Ty::Tagged(id) => {
                        if !active_tagged.insert(id) {
                            return Err(ProducerError::Lowering(format!(
                                "recursive nested tagged type id {id} survived into a source ABI key"
                            )));
                        }
                        let tagged = program.tagged_types.get(id as usize).ok_or_else(|| {
                            ProducerError::Lowering(format!(
                                "source ABI key refers to missing nested tagged type id {id}"
                            ))
                        })?;
                        work.push(SourceAbiKeyWork::ExitTagged(id));
                        match *tagged {
                            hir::TaggedType::Option(payload) => {
                                key.push('O');
                                work.push(SourceAbiKeyWork::Ty(scalar_to_ty(payload)));
                            }
                            hir::TaggedType::Result(ok, err) => {
                                key.push('R');
                                work.push(SourceAbiKeyWork::Ty(scalar_to_ty(err)));
                                work.push(SourceAbiKeyWork::Text("_"));
                                work.push(SourceAbiKeyWork::Ty(scalar_to_ty(ok)));
                            }
                        }
                    }
                    Ty::Option(payload) => {
                        key.push('O');
                        work.push(SourceAbiKeyWork::Ty(scalar_to_ty(payload)));
                    }
                    Ty::Result(ok, err) => {
                        key.push('R');
                        work.push(SourceAbiKeyWork::Ty(scalar_to_ty(err)));
                        work.push(SourceAbiKeyWork::Text("_"));
                        work.push(SourceAbiKeyWork::Ty(scalar_to_ty(ok)));
                    }
                    Ty::Box(payload) => {
                        key.push('B');
                        work.push(SourceAbiKeyWork::Ty(scalar_to_ty(payload)));
                    }
                    Ty::Slice(payload) => {
                        key.push('V');
                        work.push(SourceAbiKeyWork::Ty(scalar_to_ty(payload)));
                    }
                    Ty::DynArray(payload) => {
                        key.push('D');
                        work.push(SourceAbiKeyWork::Ty(scalar_to_ty(payload)));
                    }
                    Ty::ArrayBuilder(element) => {
                        key.push_str("AB_");
                        work.push(SourceAbiKeyWork::Ty(scalar_to_ty(element)));
                    }
                    ty @ (Ty::VecArrayBuilder(..)
                    | Ty::MaskArrayBuilder(..)
                    | Ty::FixedArrayBuilder(..)
                    | Ty::FixedStructArrayBuilder(..)) => {
                        key.push_str("AB_");
                        work.push(SourceAbiKeyWork::Ty(
                            ty.array_builder_element().expect("matched aggregate builder").ty(),
                        ));
                    }
                    ty @ (Ty::DynVecArray(..)
                    | Ty::DynMaskArray(..)
                    | Ty::DynFixedArray(..)
                    | Ty::DynFixedStructArray(..)) => {
                        key.push_str("DA_");
                        work.push(SourceAbiKeyWork::Ty(
                            ty.dyn_aggregate_array_element().expect("matched aggregate array").ty(),
                        ));
                    }
                    Ty::Array(payload, count) => {
                        key.push_str(&format!("A{count}_"));
                        work.push(SourceAbiKeyWork::Ty(scalar_to_ty(payload)));
                    }
                    Ty::Task(payload) => {
                        key.push('K');
                        work.push(SourceAbiKeyWork::Ty(scalar_to_ty(payload)));
                    }
                    Ty::StructArray(id, count) => {
                        key.push_str(&format!("A{count}_"));
                        work.push(SourceAbiKeyWork::Ty(Ty::Struct(id)));
                    }
                    Ty::DynStructArray(id, layout) => {
                        key.push_str(&format!("D{layout:?}_"));
                        work.push(SourceAbiKeyWork::Ty(Ty::Struct(id)));
                    }
                    Ty::Soa(id) => {
                        key.push('Q');
                        work.push(SourceAbiKeyWork::Ty(Ty::Struct(id)));
                    }
                    Ty::Tuple(id) => {
                        let tuple = program.tuples.get(id as usize).ok_or_else(|| {
                            ProducerError::Lowering(format!(
                                "source ABI key refers to missing tuple type id {id}"
                            ))
                        })?;
                        key.push('T');
                        for &element in tuple.elems.iter().rev() {
                            work.push(SourceAbiKeyWork::Ty(scalar_to_ty(element)));
                            work.push(SourceAbiKeyWork::Text("_"));
                        }
                    }
                    other => key.push_str(&format!("{other:?}")),
                },
            }
        }
        Ok(key)
    }

    // Validate every retained entry, including an otherwise-unused corrupt one from a decoded
    // artifact. Production lowering removes unused valid entries, so rejecting extra invalid state
    // does not narrow a valid program. One graph instance retains completed nodes across every
    // root; each successful root leaves the active paths empty.
    let mut type_graph = TypeGraph::new(program);
    for id in 0..program.tagged_types.len() {
        check_ty(Ty::Tagged(id as u32), &mut type_graph)?;
    }
    for (id, def) in program.structs.iter().enumerate() {
        check_ty(Ty::Struct(id as u32), &mut type_graph)?;
        for field in &def.fields {
            check_ty(field.ty, &mut type_graph)?;
        }
    }
    for id in 0..program.enums.len() {
        check_ty(Ty::Enum(id as u32), &mut type_graph)?;
    }
    let mut struct_source_shapes = HashMap::new();
    for def in &program.structs {
        let mut shape = format!("align={:?};c_repr={};", def.align, def.c_repr);
        for field in &def.fields {
            shape.push_str(&field.name);
            shape.push(':');
            shape.push_str(&source_abi_key(field.ty, program)?);
            shape.push(';');
        }
        if let Some(previous) = struct_source_shapes.insert(def.source_name.as_str(), shape.clone())
            && previous != shape
        {
            return Err(ProducerError::Lowering(format!(
                "struct source identity `{}` has inconsistent origin-specific ABI shapes",
                def.source_name
            )));
        }
    }
    let mut enum_source_shapes = HashMap::new();
    for def in &program.enums {
        let mut shape = String::new();
        for variant in &def.variants {
            shape.push_str(&variant.name);
            shape.push('@');
            shape.push_str(&variant.field_base.to_string());
            for &payload in &variant.payload {
                shape.push(':');
                shape.push_str(&source_abi_key(scalar_to_ty(payload), program)?);
            }
            shape.push(';');
        }
        if let Some(previous) = enum_source_shapes.insert(def.source_name.as_str(), shape.clone())
            && previous != shape
        {
            return Err(ProducerError::Lowering(format!(
                "sum source identity `{}` has inconsistent origin-specific ABI shapes",
                def.source_name
            )));
        }
    }
    for id in 0..program.tuples.len() {
        check_ty(Ty::Tuple(id as u32), &mut type_graph)?;
    }
    for f in &program.fns {
        let param_types = f
            .params
            .iter()
            .map(|slot| {
                f.slots.get(*slot as usize).copied().ok_or_else(|| {
                    ProducerError::Lowering(format!(
                        "function `{}` parameter slot {slot} is missing",
                        f.name
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !f.borrow_mut_cleanup_slots.is_empty()
            && f.borrow_mut_cleanup_slots.len() != f.params.len()
        {
            return Err(ProducerError::Lowering(format!(
                "function `{}` has a malformed BorrowMut cleanup vector",
                f.name
            )));
        }
        for (index, (mode, ty)) in f
            .param_modes
            .iter()
            .zip(&param_types)
            .enumerate()
        {
            let expected = *mode == align_ast::ParamMode::BorrowMut
                && align_sema::needs_drop_flag(
                    *ty,
                    &program.structs,
                    &program.tuples,
                    &program.enums,
                    &program.tagged_types,
                );
            let cleanup = f
                .borrow_mut_cleanup_slots
                .get(index)
                .copied()
                .flatten();
            if cleanup.is_some() != expected
                || cleanup.is_some_and(|slot| f.slots.get(slot as usize) != Some(&Ty::Bool))
            {
                return Err(ProducerError::Lowering(format!(
                    "function `{}` parameter {index} has malformed BorrowMut cleanup storage",
                    f.name
                )));
            }
        }
        check_signature_facts(
            SignatureFacts {
                owner: &format!("function `{}`", f.name),
                modes: &f.param_modes,
                param_types: &param_types,
                ret: f.ret,
                borrow: &f.return_borrow,
                region: &f.return_region,
                cleanup: f.return_cleanup,
                allow_out: true,
                allow_return_roots: true,
            },
            program,
            &mut type_graph,
        )?;
        check_mode_types(&format!("function `{}`", f.name), &f.param_modes, &param_types)?;
        validate_main_abi(f, &param_types, program)?;
        check_ty(f.ret, &mut type_graph)?;
        for &ty in f.slots.iter().chain(&f.value_tys) {
            check_ty(ty, &mut type_graph)?;
        }
        for ty in crate::function_embedded_types(f) {
            check_ty(ty, &mut type_graph)?;
        }
        for block in &f.blocks {
            for statement in &block.stmts {
                let Stmt::Let(value, rvalue) = statement else {
                    continue;
                };
                match rvalue {
                    Rvalue::FnAddr { target, signature } => {
                        let Some((param_types, ret, modes, borrow, region, cleanup)) =
                            named_signature(program, target)
                        else {
                            return Err(callable_target_error(target));
                        };
                        check_signature_facts(
                            SignatureFacts {
                                owner: "function address",
                                modes: &signature.param_modes,
                                param_types: &param_types,
                                ret,
                                borrow: &signature.return_borrow,
                                region: &signature.return_region,
                                cleanup: signature.return_cleanup,
                                allow_out: false,
                                allow_return_roots: true,
                            },
                            program,
                            &mut type_graph,
                        )?;
                        if signature.param_modes != modes
                            || &signature.return_borrow != borrow
                            || &signature.return_region != region
                            || signature.return_cleanup != cleanup
                        {
                            return Err(callable_target_error(target));
                        }
                    }
                    Rvalue::Closure {
                        lifted,
                        captures,
                        capture_tys,
                        signature,
                        ..
                    } => {
                        let Some(target) =
                            program.fns.iter().find(|function| function.name == *lifted)
                        else {
                            return Err(callable_target_error(lifted));
                        };
                        let explicit = target.params.len().checked_sub(capture_tys.len()).ok_or_else(
                            || callable_target_error(lifted),
                        )?;
                        let target_modes = target
                            .param_modes
                            .get(..explicit)
                            .ok_or_else(|| callable_target_error(lifted))?;
                        let explicit_u32 = u32::try_from(explicit)
                            .map_err(|_| callable_target_error(lifted))?;
                        let capture_count = u32::try_from(capture_tys.len())
                            .map_err(|_| callable_target_error(lifted))?;
                        let target_borrow = xml_closure_borrow_summary(
                            &target.return_borrow,
                            explicit_u32,
                            capture_count,
                        )
                        .ok_or_else(|| callable_target_error(lifted))?;
                        let target_region = xml_closure_region_summary(
                            &target.return_region,
                            explicit_u32,
                            capture_count,
                        )
                        .ok_or_else(|| callable_target_error(lifted))?;
                        let capture_modes =
                            target.param_modes.get(explicit..).ok_or_else(|| callable_target_error(lifted))?;
                        if capture_modes
                            .iter()
                            .any(|mode| *mode != align_ast::ParamMode::ByValue)
                        {
                            return Err(callable_target_error(lifted));
                        }
                        if captures.len() != capture_tys.len() {
                            return Err(callable_target_error(lifted));
                        }
                        let target_capture_tys = target
                            .params
                            .get(explicit..)
                            .ok_or_else(|| callable_target_error(lifted))?
                            .iter()
                            .map(|slot| {
                                target
                                    .slots
                                    .get(*slot as usize)
                                    .copied()
                                    .ok_or_else(|| callable_target_error(lifted))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        let target_explicit_tys = target
                            .params
                            .get(..explicit)
                            .ok_or_else(|| callable_target_error(lifted))?
                            .iter()
                            .map(|slot| {
                                target
                                    .slots
                                    .get(*slot as usize)
                                    .copied()
                                    .ok_or_else(|| callable_target_error(lifted))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        if target_capture_tys != *capture_tys {
                            return Err(callable_target_error(lifted));
                        }
                        check_signature_facts(
                            SignatureFacts {
                                owner: "closure",
                                modes: &signature.param_modes,
                                param_types: &target_explicit_tys,
                                ret: target.ret,
                                borrow: &signature.return_borrow,
                                region: &signature.return_region,
                                cleanup: signature.return_cleanup,
                                allow_out: false,
                                allow_return_roots: true,
                            },
                            program,
                            &mut type_graph,
                        )?;
                        if signature.param_modes != target_modes
                            || signature.return_borrow != target_borrow
                            || signature.return_region != target_region
                            || signature.return_cleanup != target.return_cleanup
                        {
                            return Err(callable_target_error(lifted));
                        }
                    }
                    Rvalue::CallIndirect {
                        args,
                        param_tys,
                        ret_ty,
                        signature,
                        ..
                    } => {
                        if !operands_match_modes(args, &signature.param_modes, param_tys, program) {
                            return Err(callable_metadata_error());
                        }
                        check_signature_facts(SignatureFacts {
                            owner: "indirect call",
                            modes: &signature.param_modes,
                            param_types: param_tys,
                            ret: *ret_ty,
                            borrow: &signature.return_borrow,
                            region: &signature.return_region,
                            cleanup: signature.return_cleanup,
                            allow_out: false,
                            allow_return_roots: true,
                        }, program, &mut type_graph)?;
                    }
                    Rvalue::CallIndirectWithCleanup(call) => {
                        let crate::IndirectCallWithCleanup {
                            param_tys,
                            args,
                            ret_ty,
                            signature,
                            cleanup,
                            ..
                        } = call.as_ref();
                        if f.value_tys.get(*cleanup as usize) != Some(&Ty::Bool) {
                            return Err(callable_metadata_error());
                        }
                        if !operands_match_modes(args, &signature.param_modes, param_tys, program) {
                            return Err(callable_metadata_error());
                        }
                        check_signature_facts(
                            SignatureFacts {
                                owner: "indirect call",
                                modes: &signature.param_modes,
                                param_types: param_tys,
                                ret: *ret_ty,
                                borrow: &signature.return_borrow,
                                region: &signature.return_region,
                                cleanup: signature.return_cleanup,
                                allow_out: false,
                                allow_return_roots: true,
                            },
                            program,
                            &mut type_graph,
                        )?;
                    }
                    Rvalue::RawNull => {
                        if f.value_tys.get(*value as usize) != Some(&Ty::Raw) {
                            return Err(ProducerError::Lowering(
                                "raw null result metadata invalid".to_owned(),
                            ));
                        }
                    }
                    Rvalue::RawPointerLoad { ptr, offset } => {
                        let offset_ty = preflight_operand_ty(f, offset);
                        if f.value_tys.get(*value as usize) != Some(&Ty::Raw)
                            || preflight_operand_ty(f, ptr) != Some(Ty::Raw)
                            || !offset_ty.is_some_and(|ty| matches!(ty, Ty::Int(_)))
                        {
                            return Err(ProducerError::Lowering(
                                "raw pointer load metadata invalid".to_owned(),
                            ));
                        }
                    }
                    Rvalue::StaticDescriptorView { ptr, offset } => {
                        if f.value_tys.get(*value as usize) != Some(&Ty::Str)
                            || preflight_operand_ty(f, ptr) != Some(Ty::Raw)
                            || !matches!(offset, 16 | 32 | 48)
                        {
                            return Err(ProducerError::Lowering(
                                "static descriptor view metadata invalid".to_owned(),
                            ));
                        }
                    }
                    Rvalue::RawCall {
                        callee,
                        args,
                        param_tys,
                        ret_ty,
                        signature,
                    } => {
                        let argument_types = args
                            .iter()
                            .map(|operand| preflight_operand_ty(f, operand))
                            .collect::<Option<Vec<_>>>();
                        let types_match = match argument_types.as_deref() {
                            Some(actual) => source_tys_match(actual, param_tys, program)?,
                            None => false,
                        };
                        let result_matches = match f.value_tys.get(*value as usize).copied() {
                            Some(actual) => source_ty_matches(actual, *ret_ty, program)?,
                            None => false,
                        };
                        if preflight_operand_ty(f, callee) != Some(Ty::Raw)
                            || !types_match
                            || !result_matches
                            // A raw-call signature is an externally supplied bare-pointer ABI;
                            // it cannot reinterpret a typed borrowed-place operand as a by-value
                            // argument. Ordinary checked program calls may load a Copy projection,
                            // but raw calls must retain their explicit borrow mode.
                            || args.iter().zip(&signature.param_modes).any(|(operand, mode)| {
                                matches!(
                                    (operand, mode),
                                    (Operand::BorrowedPlace(_), align_ast::ParamMode::ByValue)
                                )
                            })
                            // Raw calls are not an admitted target for call-only indexed borrows.
                            // Reject the descriptor itself before LLVM call construction, even if
                            // a forged signature labels the operand as a shared borrow.
                            || args
                                .iter()
                                .any(|operand| matches!(operand, Operand::BorrowedElementPlace(_)))
                            || !operands_match_modes(
                                args,
                                &signature.param_modes,
                                param_tys,
                                program,
                            )
                            || signature.return_cleanup != hir::ReturnCleanupAbi::None
                        {
                            return Err(callable_metadata_error());
                        }
                        check_signature_facts(
                            SignatureFacts {
                                owner: "raw indirect call",
                                modes: &signature.param_modes,
                                param_types: param_tys,
                                ret: *ret_ty,
                                borrow: &signature.return_borrow,
                                region: &signature.return_region,
                                cleanup: signature.return_cleanup,
                                allow_out: false,
                                allow_return_roots: true,
                            },
                            program,
                            &mut type_graph,
                        )?;
                    }
                    _ => {}
                }
            }
        }
    }
    for ext in &program.externs {
        if ext
            .param_modes
            .iter()
            .any(|mode| *mode != align_ast::ParamMode::ByValue)
        {
            return Err(ProducerError::Lowering(format!(
                "extern function `{}` uses a non-by-value parameter mode at the C boundary",
                ext.name
            )));
        }
        check_signature_facts(
            SignatureFacts {
                owner: &format!("extern function `{}`", ext.name),
                modes: &ext.param_modes,
                param_types: &ext.params,
                ret: ext.ret,
                borrow: &ext.return_borrow,
                region: &ext.return_region,
                cleanup: ext.return_cleanup,
                allow_out: false,
                allow_return_roots: false,
            },
            program,
            &mut type_graph,
        )?;
        check_mode_types(&format!("extern function `{}`", ext.name), &ext.param_modes, &ext.params)?;
        check_ty(ext.ret, &mut type_graph)?;
        for &ty in &ext.params {
            check_ty(ty, &mut type_graph)?;
        }
    }
    for import in &program.imported_fns {
        check_signature_facts(
            SignatureFacts {
                owner: &format!("imported function `{}`", import.name),
                modes: &import.param_modes,
                param_types: &import.params,
                ret: import.ret,
                borrow: &import.return_borrow,
                region: &import.return_region,
                cleanup: import.return_cleanup,
                allow_out: true,
                allow_return_roots: true,
            },
            program,
            &mut type_graph,
        )?;
        check_mode_types(
            &format!("imported function `{}`", import.name),
            &import.param_modes,
            &import.params,
        )?;
        check_ty(import.ret, &mut type_graph)?;
        for &ty in &import.params {
            check_ty(ty, &mut type_graph)?;
        }
    }
    if require_compact_tables && !crate::tagged_types_are_canonical(program) {
        return Err(ProducerError::Lowering(
            "nested tagged type table is not compact, unique, and canonical".to_string(),
        ));
    }
    if require_compact_tables && !crate::function_types_are_canonical(program) {
        return Err(ProducerError::Lowering(
            "function type table is not compact, unique, and canonical".to_string(),
        ));
    }
    Ok(())
}

pub fn callable_hex(name: &ProgramCall) -> String {
    lowercase_hex(name.as_bytes())
}

pub fn callable_target_error(name: &ProgramCall) -> ProducerError {
    ProducerError::Lowering(format!("callable target invalid:{}", callable_hex(name)))
}

pub fn canonical_metadata<T>(result: Result<T, crate::CanonicalCodecError>) -> Result<T, ProducerError> {
    result.map_err(|error| ProducerError::Lowering(format!("callable metadata invalid:{error:?}")))
}

pub fn canonical_ty(ty: Ty, program: &Program) -> Result<CanonicalTy, ProducerError> {
    canonical_metadata(CanonicalTy::from_program(ty, program))
}

pub fn source_ty_matches(actual: Ty, expected: Ty, program: &Program) -> Result<bool, ProducerError> {
    if actual == expected {
        return Ok(true);
    }
    Ok(canonical_ty(actual, program)? == canonical_ty(expected, program)?)
}

fn source_tys_match(
    actual: &[Ty],
    expected: &[Ty],
    program: &Program,
) -> Result<bool, ProducerError> {
    if actual.len() != expected.len() {
        return Ok(false);
    }
    for (&actual, &expected) in actual.iter().zip(expected) {
        if !source_ty_matches(actual, expected, program)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn callable_metadata_error() -> ProducerError {
    ProducerError::Lowering("callable metadata invalid:InvalidGraph".to_owned())
}

pub fn preflight_operand_ty(function: &Function, operand: &Operand) -> Option<Ty> {
    match operand {
        Operand::Const(Const::Int(_, ty)) | Operand::Const(Const::Float(_, ty)) => Some(*ty),
        Operand::Const(Const::Char(_)) => Some(Ty::Char),
        Operand::Const(Const::Bool(_)) => Some(Ty::Bool),
        Operand::Const(Const::Unit) => Some(Ty::Unit),
        Operand::Value(value) => function.value_tys.get(*value as usize).copied(),
        Operand::Arg(index) => function
            .params
            .get(*index as usize)
            .and_then(|slot| function.slots.get(*slot as usize))
            .copied(),
        Operand::BorrowedPlace(place) => function
            .slots
            .get(place.slot as usize)
            .map(|_| place.ty),
        Operand::BorrowedElementPlace(place) => function
            .slots
            .get(place.base.slot as usize)
            .map(|_| place.element_ty),
        Operand::BorrowedFixedElementPlace(place) => {
            function.slots.get(place.base as usize).map(|_| place.ty)
        }
        Operand::BorrowedCleanupArg(_) => Some(Ty::Bool),
    }
}

/// Recover the physical element addressed by a pointer-backed MIR collection. This is deliberately
/// derived from the source operand rather than the result value: `array<string>[i]` is the one
/// legal case where the buffer physically stores `string` while the logical result is `str`.
pub fn slice_index_physical_element(source: Ty) -> Option<Ty> {
    let u8_ty = Ty::Int(IntTy {
        bits: 8,
        signed: false,
    });
    match source {
        Ty::Str => Some(u8_ty),
        Ty::Slice(element) | Ty::DynArray(element) => Some(scalar_to_ty(element)),
        Ty::DynSliceArray(element) => Some(Ty::Slice(align_sema::prim_to_scalar(element))),
        Ty::DynStructArray(id, Layout::Aos) => Some(Ty::Struct(id)),
        Ty::DynResponseArray => Some(Ty::HttpResponse),
        ty @ (Ty::DynVecArray(..)
        | Ty::DynMaskArray(..)
        | Ty::DynFixedArray(..)
        | Ty::DynFixedStructArray(..)) => ty.dyn_aggregate_array_element().map(|element| element.ty()),
        _ => None,
    }
}

pub fn slice_index_result_matches(program: &Program, source: Ty, result: Ty, noalias: bool) -> bool {
    let Some(physical) = slice_index_physical_element(source) else {
        return false;
    };
    if matches!(source, Ty::DynArray(Scalar::String) | Ty::Slice(Scalar::String)) {
        return !noalias && result == Ty::Str;
    }
    if source == Ty::DynResponseArray {
        return !noalias && result == Ty::HttpResponse;
    }
    physical == result
        && align_sema::collection_element_read_ok(
            physical,
            &program.structs,
            &program.tuples,
            &program.enums,
            &program.tagged_types,
        )
}

/// Reject malformed or cached MIR before LLVM constructs a GEP/load. Existing `SliceIndex`
/// producers require their exact physical result type; the sole representation-compatible
/// projection is ordinary `DynArray(String) -> Str` indexing. The vectorizer-only noalias form
/// remains exact.
pub fn validate_slice_index_rvalues(program: &Program) -> Result<(), ProducerError> {
    let i64_ty = Ty::Int(IntTy {
        bits: 64,
        signed: true,
    });
    for function in &program.fns {
        for block in &function.blocks {
            for statement in &block.stmts {
                let Stmt::Let(value, rvalue) = statement else {
                    continue;
                };
                // Plain scalar leaves do not necessarily enter the ownership graph. Certify
                // physical inline paths here for every result, before any LLVM GEP construction.
                let field_access = match rvalue {
                    Rvalue::IndexFieldPtr { base, index, path, struct_id } => {
                        let root_matches = matches!(preflight_operand_ty(function, base),
                            Some(Ty::DynStructArray(id, Layout::Aos) | Ty::Slice(Scalar::Struct(id))) if id == *struct_id);
                        Some((root_matches.then_some(*struct_id), index, path, false))
                    }
                    Rvalue::IndexField(slot, index, path) => {
                        let root = match function.slots.get(*slot as usize) {
                            Some(Ty::StructArray(id, _)) => Some(*id),
                            _ => None,
                        };
                        Some((root, index, path, true))
                    }
                    _ => None,
                };
                if let Some((root, index, path, fixed)) = field_access {
                    let leaf = root.and_then(|id| inline_struct_path_ty(program, id, path));
                    let result = function.value_tys.get(*value as usize).copied();
                    let valid_leaf = leaf.is_some_and(|leaf| {
                        if leaf == Ty::String { result == Some(Ty::Str) }
                        else {
                            result == Some(leaf) && ((fixed && matches!(leaf, Ty::Resource(_)))
                                || !align_sema::ty_is_move(leaf, &program.structs, &program.tuples,
                                    &program.enums, &program.tagged_types))
                        }
                    });
                    if !valid_leaf || preflight_operand_ty(function, index) != Some(i64_ty) {
                        return Err(ProducerError::Lowering(format!(
                            "indexed field MIR in function '{}' has an invalid inline path or leaf type: {:?}, {:?}, {:?}, {:?}",
                            function.name, root, leaf, result, preflight_operand_ty(function, index)
                        )));
                    }
                    continue;
                }
                let (source, index, noalias) = match rvalue {
                    Rvalue::SliceIndex(source, index) => (source, index, false),
                    Rvalue::SliceIndexNoalias { slice, index, .. } => (slice, index, true),
                    _ => continue,
                };
                let source_ty = preflight_operand_ty(function, source);
                let index_ty = preflight_operand_ty(function, index);
                let result_ty = function.value_tys.get(*value as usize).copied();
                if source_ty.is_none_or(|source_ty| {
                    result_ty.is_none_or(|result_ty| {
                        !slice_index_result_matches(program, source_ty, result_ty, noalias)
                    })
                }) || index_ty != Some(i64_ty)
                {
                    return Err(ProducerError::Lowering(format!(
                        "slice-index MIR in function '{}' has a physical source/result contract mismatch",
                        function.name
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Reject a malformed fixed-record resource nulling statement before LLVM indexes a slot, record,
/// or field table. This statement is emitted only after moving a canonical `pkg.template`
/// resource leaf out of a source-formed fixed record array.
pub fn validate_fixed_element_nulling(program: &Program) -> Result<(), ProducerError> {
    for function in &program.fns {
        for block in &function.blocks {
            for statement in &block.stmts {
                let Stmt::NullElemField(slot, index, path) = statement else {
                    continue;
                };
                let Some(Ty::StructArray(mut struct_id, len)) =
                    function.slots.get(*slot as usize).copied()
                else {
                    return Err(ProducerError::Lowering(format!(
                        "fixed element nulling in function '{}' has an invalid root slot",
                        function.name
                    )));
                };
                let Operand::Const(Const::Int(index, index_ty)) = index else {
                    return Err(ProducerError::Lowering(format!(
                        "fixed element nulling in function '{}' has a non-constant index",
                        function.name
                    )));
                };
                if !matches!(index_ty, Ty::Int(_))
                    || *index < 0
                    || u32::try_from(*index).ok().is_none_or(|index| index >= len)
                    || path.is_empty()
                {
                    return Err(ProducerError::Lowering(format!(
                        "fixed element nulling in function '{}' has an invalid index or path",
                        function.name
                    )));
                }
                let mut leaf = None;
                for (depth, field) in path.iter().copied().enumerate() {
                    let Some(ty) = program
                        .structs
                        .get(struct_id as usize)
                        .and_then(|record| record.fields.get(field as usize))
                        .map(|field| field.ty)
                    else {
                        return Err(ProducerError::Lowering(format!(
                            "fixed element nulling in function '{}' has an invalid field path",
                            function.name
                        )));
                    };
                    if depth + 1 == path.len() {
                        leaf = Some(ty);
                    } else if let Ty::Struct(next) = ty {
                        struct_id = next;
                    } else {
                        return Err(ProducerError::Lowering(format!(
                            "fixed element nulling in function '{}' crosses a non-record field",
                            function.name
                        )));
                    }
                }
                if !matches!(leaf, Some(Ty::Resource(resource)) if template_html_callable_resource_matches(program, resource))
                {
                    return Err(ProducerError::Lowering(format!(
                        "fixed element nulling in function '{}' does not select the canonical template resource",
                        function.name
                    )));
                }
            }
        }
    }
    Ok(())
}

pub fn template_piece_is_type_safe(
    program: &Program,
    function: &Function,
    piece: &crate::TemplatePiece,
) -> bool {
    let operand_ty = |operand| preflight_operand_ty(function, operand);
    match piece {
        crate::TemplatePiece::Static(_) | crate::TemplatePiece::PopComma => true,
        crate::TemplatePiece::IntHole(operand) => {
            matches!(operand_ty(operand), Some(Ty::Int(_)))
        }
        crate::TemplatePiece::StrHole(operand) => operand_ty(operand) == Some(Ty::Str),
        crate::TemplatePiece::CharHole(operand) => operand_ty(operand) == Some(Ty::Char),
        crate::TemplatePiece::JsonStrHole(operand) => operand_ty(operand) == Some(Ty::Str),
        crate::TemplatePiece::OwnedJsonObject { value, plan } => {
            operand_ty(value) == Some(Ty::Struct(plan.root))
                && align_sema::owned_json_graph_plan_v3(&program.structs, plan.root)
                    .is_ok_and(|rebuilt| rebuilt == *plan)
        }
        crate::TemplatePiece::BoolHole(operand) => operand_ty(operand) == Some(Ty::Bool),

        crate::TemplatePiece::FloatHole(operand) => {
            matches!(operand_ty(operand), Some(Ty::Float(_)))
        }
        crate::TemplatePiece::OptionField { opt, .. } => matches!(
            operand_ty(opt),
            Some(Ty::Option(payload))
                if matches!(
                    align_sema::scalar_to_ty(payload),
                    Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Str
                )
        ),
        crate::TemplatePiece::OptionStructField { opt, struct_id, .. } => {
            program.structs.get(*struct_id as usize).is_some()
                && operand_ty(opt) == Some(Ty::Option(Scalar::Struct(*struct_id)))
        }
        crate::TemplatePiece::StructArrayField { array, struct_id } => {
            program.structs.get(*struct_id as usize).is_some()
                && operand_ty(array) == Some(Ty::DynStructArray(*struct_id, align_sema::Layout::Aos))
        }
        crate::TemplatePiece::ScalarArrayField { array, elem } => {
            operand_ty(array) == Some(Ty::DynArray(*elem))
        }
        crate::TemplatePiece::UnionValue { value, enum_id } => {
            program.enums.get(*enum_id as usize).is_some()
                && operand_ty(value) == Some(Ty::Enum(*enum_id))
        }
    }
}

fn operands_match_modes(
    args: &[Operand],
    modes: &[align_ast::ParamMode],
    types: &[Ty],
    program: &Program,
) -> bool {
    args.len() == modes.len()
        && modes.len() == types.len()
        && args
            .iter()
            .zip(modes)
            .zip(types)
            .all(|((argument, mode), ty)| match (argument, mode) {
                (Operand::BorrowedPlace(place), align_ast::ParamMode::Borrow) => {
                    place.cleanup.is_none()
                }
                (Operand::BorrowedElementPlace(place), align_ast::ParamMode::Borrow) => {
                    place.base.cleanup.is_none() && place.element_ty == *ty
                }
                (Operand::BorrowedFixedElementPlace(place), align_ast::ParamMode::Borrow) => {
                    place.cleanup.is_none() && place.ty == *ty
                }
                (Operand::BorrowedPlace(place), align_ast::ParamMode::BorrowMut) => {
                    let move_pointee = align_sema::needs_drop_flag(
                        *ty,
                        &program.structs,
                        &program.tuples,
                        &program.enums,
                        &program.tagged_types,
                    );
                    place.cleanup.is_some() == move_pointee
                        && (!move_pointee || place.path.is_empty())
                }
                (Operand::BorrowedPlace(place), align_ast::ParamMode::ByValue)
                    if place.cleanup.is_none()
                        && !align_sema::needs_drop_flag(
                            *ty,
                            &program.structs,
                            &program.tuples,
                            &program.enums,
                            &program.tagged_types,
                        ) =>
                {
                    true
                }
                (
                    Operand::BorrowedPlace(_)
                    | Operand::BorrowedElementPlace(_)
                    | Operand::BorrowedFixedElementPlace(_),
                    _,
                ) => false,
                (_, align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut) => false,
                _ => true,
            })
}

pub fn direct_operands_match_modes(
    target: &ProgramCall,
    args: &[Operand],
    modes: &[align_ast::ParamMode],
    types: &[Ty],
    program: &Program,
) -> bool {
    if operands_match_modes(args, modes, types, program) {
        return true;
    }
    if !matches!(target.as_str(), "pkg.template$write" | "pkg.template$raw")
        || args.len() != 2
        || modes
            != [
                align_ast::ParamMode::BorrowMut,
                align_ast::ParamMode::ByValue,
            ]
    {
        return false;
    }
    let Some(Ty::Resource(resource)) = types.first().copied() else {
        return false;
    };
    let projected = match &args[0] {
        Operand::BorrowedPlace(place) => !place.path.is_empty() && place.cleanup.is_some(),
        Operand::BorrowedFixedElementPlace(place) => {
            !place.path.is_empty() && place.cleanup.is_some()
        }
        _ => false,
    };
    projected
        && template_html_callable_resource_matches(program, resource)
        && operands_match_modes(&args[1..], &modes[1..], &types[1..], program)
}

pub fn direct_runtime_key_is_valid(key: RuntimeKey, args: &[Ty], ret: Ty, program: &Program) -> bool {
    let i64_ty = Ty::Int(IntTy {
        bits: 64,
        signed: true,
    });
    match key {
        RuntimeKey::Print => args.len() == 1 && matches!(args[0], Ty::Int(_)) && ret == Ty::Unit,
        RuntimeKey::PrintStr => args == [Ty::Str] && ret == Ty::Unit,
        RuntimeKey::PrintBool => args == [Ty::Bool] && ret == Ty::Unit,
        RuntimeKey::PrintChar => args == [Ty::Char] && ret == Ty::Unit,
        RuntimeKey::PrintF32 => {
            args == [Ty::Float(FloatTy { bits: 32 })] && ret == Ty::Unit
        }
        RuntimeKey::PrintF64 => {
            args == [Ty::Float(FloatTy { bits: 64 })] && ret == Ty::Unit
        }
        RuntimeKey::Hash64 => {
            args.len() == 1
                && matches!(args[0], Ty::Str | Ty::Slice(_))
                && ret
                    == Ty::Int(IntTy {
                        bits: 64,
                        signed: false,
                    })
        }
        RuntimeKey::Hash128 => args.len() == 1
            && matches!(args[0], Ty::Str | Ty::Slice(_))
            && matches!(ret, Ty::Tuple(id)
                if program.tuples.get(id as usize).is_some_and(|tuple| {
                    let u64_scalar = Scalar::Int(IntTy { bits: 64, signed: false });
                    tuple.elems.as_slice() == [u64_scalar, u64_scalar]
                })),
        RuntimeKey::ProcessExit => args == [i64_ty] && ret == Ty::Unit,
        RuntimeKey::ProcessAbort | RuntimeKey::DivFail => args.is_empty() && ret == Ty::Unit,
        RuntimeKey::BoundsFail | RuntimeKey::Utf8BoundaryFail | RuntimeKey::LenMismatchFail => {
            args == [i64_ty, i64_ty] && ret == Ty::Unit
        }
        RuntimeKey::RangeFail => args == [i64_ty, i64_ty, i64_ty] && ret == Ty::Unit,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mir_publication_certifies_bodies_without_a_backend() {
        let mut diagnostics = align_diag::Diagnostics::new();
        let source = "fn identity(value: string) -> string = value\nfn main() -> i32 = 0\n";
        let tokens = align_lexer::tokenize(0, source, &mut diagnostics);
        let ast = align_parser::parse_file(tokens, &mut diagnostics);
        let hir = align_sema::check_file(&ast, &mut diagnostics);
        assert!(!diagnostics.has_errors());
        let program = crate::lower_program(&hir);
        let certified = validate_mir_producers(&program)
            .unwrap_or_else(|error| panic!("valid producer: {error}"));
        assert_eq!(certified, HashSet::from(["identity".to_owned(), "main".to_owned()]));

        let mut malformed = program.clone();
        let identity = malformed.fns.iter_mut().find(|function| function.name.as_str() == "identity")
            .unwrap_or_else(|| panic!("missing identity"));
        identity.ret = Ty::Raw;
        assert!(validate_mir_producers(&malformed).is_err(), "forged return contract");

        let mut duplicate = program.clone();
        duplicate.fns.push(program.fns[0].clone());
        assert!(validate_mir_producers(&duplicate).is_err(), "duplicate producer identity");
        assert_eq!(ProducerError::Lowering("invalid producer".to_owned()).to_string(),
            "lowering failed: invalid producer");
    }

    #[test]
    fn producer_copied_scalar_preserves_grounding_and_readability() {
        use XmlAccessProvenance::{Owned, Shared, Exclusive, Unreadable, Mixed};
        for seed in [None, Some(Owned), Some(Shared), Some(Exclusive), Some(Unreadable), Some(Mixed)] {
            for absent in [false, true] {
                let storage = XmlAccessNode::Slot(0, vec![]);
                let copied = XmlAccessNode::Value(0, vec![]);
                let mut equations = HashMap::from([
                    (storage.clone(), XmlAccessEquation { seed, dependencies: vec![copied.clone()],
                        ..XmlAccessEquation::default() }),
                    (copied.clone(), XmlAccessEquation { copied_scalar: true, absent,
                        dependencies: vec![storage.clone()], ..XmlAccessEquation::default() }),
                ]);
                let (values, invalid) = solve_xml_access_equations(&equations);
                let readable = matches!(seed, Some(Owned | Shared | Exclusive));
                // An explicitly absent alternative is vacuous; an unseeded present cycle is not.
                let valid = readable || (seed.is_none() && absent);
                assert_eq!(invalid.is_empty(), valid, "{seed:?}/{absent}: {invalid:?}");
                if readable {
                    let expected = if absent { XmlProducerState::MaybeAbsent(Owned) }
                        else { XmlProducerState::Present(Owned) };
                    assert_eq!(values.get(&copied), Some(&expected));
                }
                // A malformed validation dependency must poison the copied result and its cycle.
                let bad = XmlAccessNode::Value(1, vec![]);
                equations.insert(bad.clone(), XmlAccessEquation { invalid: true, ..XmlAccessEquation::default() });
                equations.get_mut(&copied).unwrap_or_else(|| panic!("missing copied equation")).checks.push((bad, OperandRequirement::READ));
                let (_, invalid) = solve_xml_access_equations(&equations);
                assert!(invalid.contains(&copied) && invalid.contains(&storage));
            }
        }
    }

    #[test]
    fn producer_borrowed_materialization_caps_transfer_authority() {
        let source = XmlAccessNode::Slot(0, Vec::new());
        let materialized = XmlAccessNode::Slot(1, Vec::new());
        let loaded = XmlAccessNode::Value(0, Vec::new());
        let consumed = XmlAccessNode::Value(1, Vec::new());
        let mut equations = HashMap::from([
            (
                source.clone(),
                XmlAccessEquation {
                    seed: Some(XmlAccessProvenance::Owned),
                    ..XmlAccessEquation::default()
                },
            ),
            (
                materialized.clone(),
                XmlAccessEquation {
                    read_dependencies: vec![source],
                    ..XmlAccessEquation::default()
                },
            ),
            (
                loaded.clone(),
                XmlAccessEquation {
                    dependencies: vec![materialized.clone()],
                    ..XmlAccessEquation::default()
                },
            ),
            (
                consumed.clone(),
                XmlAccessEquation {
                    dependencies: vec![loaded.clone()],
                    checks: vec![
                        (
                            loaded.clone(),
                            OperandRequirement {
                                read: true,
                                move_value: true,
                                ..OperandRequirement::default()
                            },
                        ),
                    ],
                    ..XmlAccessEquation::default()
                },
            ),
        ]);
        let (values, invalid) = solve_xml_access_equations(&equations);
        assert_eq!(
            values.get(&materialized),
            Some(&XmlProducerState::Present(XmlAccessProvenance::Shared))
        );
        assert_eq!(
            values.get(&loaded),
            Some(&XmlProducerState::Present(XmlAccessProvenance::Shared))
        );
        assert!(invalid.contains(&consumed), "borrowed owner reached a move use");

        // A read-only dependency still needs a founded source. Removing the owner seed must not
        // turn the borrowed Store/Load chain into an unconditional success.
        let source_equation = match equations.get_mut(&XmlAccessNode::Slot(0, Vec::new())) {
            Some(equation) => equation,
            None => panic!("source equation"),
        };
        source_equation.seed = None;
        let (_, invalid) = solve_xml_access_equations(&equations);
        assert!(invalid.contains(&XmlAccessNode::Slot(1, Vec::new())));
    }

    #[test]
    fn producer_json_borrowed_root_requires_initialized_exact_place() -> Result<(), &'static str> {
        let source = r#"import core.json
Record { text: string }
Carrier { record: Option<Record> }
fn encode(borrow input: Carrier) -> Result<string, Error> {
    match input.record { None => Err(Error.Invalid), Some(value) => json.encode(value) }
}
fn main() {}
"#;
        let mut diagnostics = align_diag::Diagnostics::new();
        let tokens = align_lexer::tokenize(0, source, &mut diagnostics);
        let ast = align_parser::parse_file(tokens, &mut diagnostics);
        let hir = align_sema::check_file(&ast, &mut diagnostics);
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.iter().collect::<Vec<_>>());
        let program = crate::lower_program(&hir);
        assert!(!program.fns.is_empty());
        validate_mir_producers(&program).map_err(|_| "valid projected JSON input")?;
        for mutation in 0..3 {
            let mut malformed = program.clone();
            let function = malformed.fns.iter_mut().find(|f| f.name.as_str() == "encode")
                .ok_or("encode function")?;
            let empty_slot = u32::try_from(function.slots.len()).map_err(|_| "slot index")?;
            let root_ty = function.slots[function.params[0] as usize];
            function.slots.push(root_ty);
            function.slot_align.push(None);
            let mut changed = false;
            for block in &mut function.blocks {
                for statement in &mut block.stmts {
                    if let Stmt::Let(_, Rvalue::JsonEncode { pieces, .. }) = statement {
                        for piece in pieces {
                            if let crate::TemplatePiece::OwnedJsonObject {
                                value: Operand::BorrowedPlace(place), ..
                            } = piece {
                                match mutation {
                                    0 => place.slot = empty_slot,
                                    1 => place.ty = Ty::Bool,
                                    _ => place.path.push(align_sema::hir::BorrowedPathSegment::StructField(u32::MAX)),
                                }
                                changed = true;
                            }
                        }
                    }
                }
            }
            assert!(changed, "owner must mutate the actual JSON projection");
            assert!(validate_mir_producers(&malformed).is_err(), "accepted mutation {mutation}");
        }
        Ok(())
    }

    #[test]
    fn producer_rejects_borrowed_store_load_before_owned_return() {
        let mut diagnostics = align_diag::Diagnostics::new();
        let source = "fn forward(value: string) -> string = value\nfn main() -> i32 = 0\n";
        let tokens = align_lexer::tokenize(0, source, &mut diagnostics);
        let ast = align_parser::parse_file(tokens, &mut diagnostics);
        let hir = align_sema::check_file(&ast, &mut diagnostics);
        assert!(!diagnostics.has_errors());
        let mut malformed = crate::lower_program(&hir);
        let function = match malformed
            .fns
            .iter_mut()
            .find(|function| function.name.as_str() == "forward")
        {
            Some(function) => function,
            None => panic!("forward MIR"),
        };
        let source_slot = function.params[0];
        let destination_slot = match u32::try_from(function.slots.len()) {
            Ok(slot) => slot,
            Err(_) => panic!("slot id"),
        };
        function.slots.push(Ty::String);
        function.slot_align.push(None);
        let loaded_value = match u32::try_from(function.value_tys.len()) {
            Ok(value) => value,
            Err(_) => panic!("value id"),
        };
        function.value_tys.push(Ty::String);
        let borrowed = Operand::BorrowedPlace(Box::new(crate::BorrowedPlace {
            slot: source_slot,
            path: Vec::new(),
            ty: Ty::String,
            cleanup: None,
        }));
        let entry = match function
            .blocks
            .iter_mut()
            .find(|block| block.id == function.entry)
        {
            Some(block) => block,
            None => panic!("forward entry block"),
        };
        entry.stmts.insert(1, Stmt::Store(destination_slot, borrowed));
        entry
            .stmts
            .insert(2, Stmt::Let(loaded_value, Rvalue::Load(destination_slot)));
        for block in &mut function.blocks {
            if let Term::ReturnWithCleanup(pair) = &mut block.term {
                pair.0 = Operand::Value(loaded_value);
            }
        }

        let error = match validate_mir_producers(&malformed) {
            Ok(_) => panic!("a borrowed Store/Load chain must not certify an owned return"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("producer return leaf")
                || error.to_string().contains("producer return does not transfer"),
            "unexpected malformed-store diagnostic: {error}"
        );
    }

    #[test]
    fn producer_fixed_point_preserves_guarded_absence_in_seeded_cycles() {
        let absent = XmlAccessNode::Value(0, Vec::new());
        let present = XmlAccessNode::Value(1, Vec::new());
        let joined = XmlAccessNode::Value(2, Vec::new());
        let guarded = XmlAccessNode::Value(3, Vec::new());
        let slot = XmlAccessNode::Slot(0, Vec::new());
        let loaded = XmlAccessNode::Value(4, Vec::new());
        let loop_join = XmlAccessNode::Value(5, Vec::new());
        let mut equations = HashMap::from([
            (
                absent.clone(),
                XmlAccessEquation {
                    absent: true,
                    ..XmlAccessEquation::default()
                },
            ),
            (
                present.clone(),
                XmlAccessEquation {
                    seed: Some(XmlAccessProvenance::Owned),
                    ..XmlAccessEquation::default()
                },
            ),
            (
                joined.clone(),
                XmlAccessEquation {
                    dependencies: vec![absent, present],
                    ..XmlAccessEquation::default()
                },
            ),
            (
                guarded.clone(),
                XmlAccessEquation {
                    dependencies: vec![joined.clone()],
                    require_present: true,
                    guarded_absence: true,
                    ..XmlAccessEquation::default()
                },
            ),
            (
                slot.clone(),
                XmlAccessEquation {
                    seed: Some(XmlAccessProvenance::Owned),
                    dependencies: vec![loop_join.clone()],
                    ..XmlAccessEquation::default()
                },
            ),
            (
                loaded.clone(),
                XmlAccessEquation {
                    dependencies: vec![slot.clone()],
                    ..XmlAccessEquation::default()
                },
            ),
            (
                loop_join.clone(),
                XmlAccessEquation {
                    dependencies: vec![loaded.clone(), guarded.clone()],
                    ..XmlAccessEquation::default()
                },
            ),
        ]);

        let (values, invalid) = solve_xml_access_equations(&equations);
        assert!(invalid.is_empty(), "guarded cycle was rejected: {invalid:?}");
        assert_eq!(
            values.get(&joined),
            Some(&XmlProducerState::MaybeAbsent(XmlAccessProvenance::Owned))
        );
        for node in [&guarded, &slot, &loaded, &loop_join] {
            assert_eq!(
                values.get(node),
                Some(&XmlProducerState::Present(XmlAccessProvenance::Owned)),
                "guarded present fact did not stabilize at {node:?}"
            );
        }

        equations.entry(guarded.clone()).or_default().guarded_absence = false;
        let (_, invalid) = solve_xml_access_equations(&equations);
        for node in [&guarded, &slot, &loaded, &loop_join] {
            assert!(
                invalid.contains(node),
                "unguarded absence did not poison {node:?}"
            );
        }
    }

    #[test]
    fn xml_access_join_is_the_capability_intersection() {
        use XmlAccessProvenance::{Exclusive, Mixed, Owned, Shared, Unreadable};

        let cases = [
            (Owned, Shared, Shared),
            (Owned, Exclusive, Exclusive),
            (Owned, Unreadable, Unreadable),
            (Shared, Exclusive, Shared),
            (Shared, Unreadable, Mixed),
            (Exclusive, Unreadable, Unreadable),
            (Owned, Mixed, Mixed),
            (Shared, Mixed, Mixed),
        ];
        for (left, right, expected) in cases {
            assert_eq!(merge_xml_access(Some(left), right), Some(expected));
            assert_eq!(merge_xml_access(Some(right), left), Some(expected));
        }

        let accesses = [Owned, Shared, Exclusive, Unreadable, Mixed];
        let join = |left, right| merge_xml_access(Some(left), right).unwrap_or(Mixed);
        for left in accesses {
            assert_eq!(join(left, left), left, "access join must be idempotent");
            for right in accesses {
                assert_eq!(
                    join(left, right),
                    join(right, left),
                    "access join must be commutative for {left:?} and {right:?}"
                );
                for third in accesses {
                    assert_eq!(
                        join(join(left, right), third),
                        join(left, join(right, third)),
                        "access join must be associative for {left:?}, {right:?}, and {third:?}"
                    );
                }
            }
        }
    }
}
