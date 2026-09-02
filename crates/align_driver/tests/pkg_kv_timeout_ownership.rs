//! `pkg.kv`'s source-reachable timeout row has an unsafe caller precondition: before a non-null
//! connection is reconfigured, no reader, writer, or logger-owned writer derived from that
//! connection may remain live.  This owner binds that rule to source type formation, the canonical
//! recursive `DropPlan`, and generated cleanup rather than maintaining a second recursive edge
//! list.

mod common;
use common::*;

use align_sema::{DropPlan, Ty, drop_plan, ty_is_move};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const CC_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PIPE_SETUP_TIMEOUT: Duration = Duration::from_secs(1);
const PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
const DRAIN_CANCEL_RESERVE: Duration = Duration::from_millis(100);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(5);
const LINK_CHILD_ENV: &str = "ALIGN_PKG_KV_TIMEOUT_OWNER_LINK_CHILD";
const LINK_EXE_ENV: &str = "ALIGN_PKG_KV_TIMEOUT_OWNER_LINK_EXE";
const LINK_OBJECT_COUNT_ENV: &str = "ALIGN_PKG_KV_TIMEOUT_OWNER_LINK_OBJECT_COUNT";
const LINK_LIBRARY_COUNT_ENV: &str = "ALIGN_PKG_KV_TIMEOUT_OWNER_LINK_LIBRARY_COUNT";

const FORMATION_SOURCE: &str = r#"
import std.log

ReaderHolder { value: reader, tag: i32 }
WriterHolder { value: writer }
LoggerHolder { value: log.logger }
ReaderPair { first: ReaderHolder, second: ReaderHolder }
ReaderTriple { first: ReaderHolder, second: ReaderHolder, third: ReaderHolder }
MixedReaders { target: ReaderHolder, other: ReaderHolder }
NestedReader { value: Option<Result<ReaderTriple, ReaderTriple>> }
FallibleReader { value: Option<Result<ReaderTriple, bool>> }
ReaderChoice { Active(ReaderTriple), Empty }
LoggerChoice { Active(log.logger), Empty }
SumChoice { Active(ReaderChoice), Empty }
TaggedChoice { Active(Option<Result<ReaderTriple, ReaderTriple>>), Empty }
ArrayField { values: array<ReaderHolder> }
ReaderChoiceArrayField { values: array<ReaderChoice> }
WriterArrayField { values: array<WriterHolder> }
LoggerArrayField { values: array<LoggerHolder> }
LoggerChoiceArrayField { values: array<LoggerChoice> }
SumChoiceArrayField { values: array<SumChoice> }
TaggedChoiceArrayField { values: array<TaggedChoice> }
DynamicClosure {
  readers: array<ReaderHolder>,
  reader_view: slice<ReaderHolder>,
  choices: array<ReaderChoice>,
  choice_view: slice<ReaderChoice>,
  reader_batches: array<array<ReaderHolder>>,
  reader_batch_view: slice<array<ReaderHolder>>,
  optional: Option<array<ReaderHolder>>,
  result: Result<array<ReaderHolder>, Error>,
  writers: array<WriterHolder>,
  writer_view: slice<WriterHolder>,
  writer_batches: array<array<WriterHolder>>,
  writer_batch_view: slice<array<WriterHolder>>,
  optional_writers: Option<array<WriterHolder>>,
  result_writers: Result<array<WriterHolder>, Error>,
  loggers: array<LoggerHolder>,
  logger_view: slice<LoggerHolder>,
  logger_batches: array<array<LoggerHolder>>,
  logger_batch_view: slice<array<LoggerHolder>>,
  optional_loggers: Option<array<LoggerHolder>>,
  result_loggers: Result<array<LoggerHolder>, Error>,
  logger_choices: array<LoggerChoice>,
  logger_choice_view: slice<LoggerChoice>,
  sum_choices: array<SumChoice>,
  sum_choice_view: slice<SumChoice>,
  tagged_choices: array<TaggedChoice>,
  tagged_choice_view: slice<TaggedChoice>,
}

fn nameable(
  readers: array<ReaderHolder>,
  reader_view: slice<ReaderHolder>,
  choices: array<ReaderChoice>,
  choice_view: slice<ReaderChoice>,
  reader_batches: array<array<ReaderHolder>>,
  reader_batch_view: slice<array<ReaderHolder>>,
  pair: (array<ReaderHolder>, i64),
  optional: Option<array<ReaderHolder>>,
  result: Result<array<ReaderHolder>, Error>,
  writer_pair: (array<WriterHolder>, i64),
  logger_pair: (array<LoggerHolder>, i64),
) -> i32 = 0

fn admitted_wrappers(
  tuple_rows: array<ReaderHolder>,
  optional_rows: array<ReaderHolder>,
  result_rows: array<ReaderHolder>,
  field_rows: array<ReaderHolder>,
  reader_choice_field_rows: array<ReaderChoice>,
  writer_tuple_rows: array<WriterHolder>,
  writer_optional_rows: array<WriterHolder>,
  writer_result_rows: array<WriterHolder>,
  writer_field_rows: array<WriterHolder>,
  logger_tuple_rows: array<LoggerHolder>,
  logger_optional_rows: array<LoggerHolder>,
  logger_result_rows: array<LoggerHolder>,
  logger_field_rows: array<LoggerHolder>,
  logger_choice_field_rows: array<LoggerChoice>,
  sum_choice_field_rows: array<SumChoice>,
  tagged_choice_field_rows: array<TaggedChoice>,
) {
  tuple_value := (tuple_rows, 1)
  optional_value: Option<array<ReaderHolder>> := Some(optional_rows)
  result_value: Result<array<ReaderHolder>, Error> := Ok(result_rows)
  field_value := ArrayField { values: field_rows }
  reader_choice_field_value := ReaderChoiceArrayField { values: reader_choice_field_rows }
  writer_tuple_value := (writer_tuple_rows, 2)
  writer_optional_value: Option<array<WriterHolder>> := Some(writer_optional_rows)
  writer_result_value: Result<array<WriterHolder>, Error> := Ok(writer_result_rows)
  writer_field_value := WriterArrayField { values: writer_field_rows }
  logger_tuple_value := (logger_tuple_rows, 3)
  logger_optional_value: Option<array<LoggerHolder>> := Some(logger_optional_rows)
  logger_result_value: Result<array<LoggerHolder>, Error> := Ok(logger_result_rows)
  logger_field_value := LoggerArrayField { values: logger_field_rows }
  logger_choice_field_value := LoggerChoiceArrayField { values: logger_choice_field_rows }
  sum_choice_field_value := SumChoiceArrayField { values: sum_choice_field_rows }
  tagged_choice_field_value := TaggedChoiceArrayField { values: tagged_choice_field_rows }
}

fn fixed_formation(first: reader, second: reader, third: reader) {
  values := [
    ReaderHolder { value: first, tag: 1 },
    ReaderHolder { value: second, tag: 2 },
    ReaderHolder { value: third, tag: 3 },
  ]
}

fn main() -> i32 = 0
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellKind {
    DirectReader,
    BufferedReader,
    DirectWriter,
    LoggerOwnedWriter,
}

impl ShellKind {
    fn drop_ty(self) -> Ty {
        match self {
            Self::DirectReader | Self::BufferedReader => Ty::Reader,
            Self::DirectWriter => Ty::Writer,
            Self::LoggerOwnedWriter => Ty::Logger,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Provenance {
    Target,
    Other,
}

#[derive(Clone, Debug)]
enum LiveValue {
    Inert,
    MovedOut,
    Shell {
        kind: ShellKind,
        provenance: Provenance,
    },
    Struct(Vec<LiveValue>),
    Option(Option<Box<LiveValue>>),
    ResultOk(Box<LiveValue>),
    ResultErr(Box<LiveValue>),
    Sum {
        variant: usize,
        fields: Vec<LiveValue>,
    },
    FixedStructArray(Vec<LiveValue>),
}

fn shell(kind: ShellKind, provenance: Provenance) -> LiveValue {
    LiveValue::Shell { kind, provenance }
}

/// Count initialized leaves retaining `target` by walking the production cleanup graph.
///
/// This exhaustive match is deliberately only a cleanup-plan-variant tripwire. `align_sema`'s
/// compiler-owned `variant_sweep_tripwire` owns additions to the complete `Ty`/`Scalar` inventory,
/// while the positive and negative source fixtures below own the applicable formation edges. A
/// `DropPlan` match cannot honestly claim to exhaust private storage-header analysis: fixed struct
/// arrays compose their element plan outside `DropPlan` and are owned separately below.
fn target_retainers(plan: &DropPlan, value: &LiveValue) -> usize {
    if matches!(value, LiveValue::MovedOut) {
        return 0;
    }
    match plan {
        DropPlan::None => {
            assert!(matches!(value, LiveValue::Inert));
            0
        }
        DropPlan::Leaf(ty) => {
            let LiveValue::Shell { kind, provenance } = value else {
                panic!("Drop leaf {ty:?} did not receive a shell state: {value:?}");
            };
            assert_eq!(*ty, kind.drop_ty(), "shell kind drifted from its Drop leaf");
            usize::from(*provenance == Provenance::Target)
        }
        DropPlan::Struct { fields, .. } => {
            let LiveValue::Struct(values) = value else {
                panic!("struct Drop node did not receive struct state: {value:?}");
            };
            assert_eq!(fields.len(), values.len(), "struct field topology drifted");
            fields
                .iter()
                .zip(values)
                .map(|((_, child), value)| target_retainers(child, value))
                .sum()
        }
        DropPlan::Option(payload) => {
            let LiveValue::Option(value) = value else {
                panic!("Option Drop node did not receive tagged state: {value:?}");
            };
            value
                .as_deref()
                .map_or(0, |value| target_retainers(payload, value))
        }
        DropPlan::Result { ok, err } => match value {
            LiveValue::ResultOk(value) => target_retainers(ok, value),
            LiveValue::ResultErr(value) => target_retainers(err, value),
            _ => panic!("Result Drop node did not receive an active arm: {value:?}"),
        },
        DropPlan::Enum { variants, .. } => {
            let LiveValue::Sum { variant, fields } = value else {
                panic!("sum Drop node did not receive a selected variant: {value:?}");
            };
            let planned = variants
                .get(*variant)
                .unwrap_or_else(|| panic!("active sum variant {variant} is out of range"));
            assert_eq!(planned.len(), fields.len(), "sum payload topology drifted");
            planned
                .iter()
                .zip(fields)
                .map(|(child, value)| target_retainers(child, value))
                .sum()
        }
        DropPlan::Invalid => panic!("a compatible caller cannot classify a malformed Drop graph"),
    }
}

fn fixed_array_target_retainers(
    element: &DropPlan,
    expected_length: usize,
    value: &LiveValue,
) -> usize {
    if matches!(value, LiveValue::MovedOut) {
        return 0;
    }
    let LiveValue::FixedStructArray(elements) = value else {
        panic!("fixed struct-array plan did not receive array state: {value:?}");
    };
    assert_eq!(
        elements.len(),
        expected_length,
        "fixed struct-array length drifted",
    );
    elements
        .iter()
        .map(|value| target_retainers(element, value))
        .sum()
}

fn checked_formation() -> align_sema::hir::Program {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "pkg-kv-timeout-formation", FORMATION_SOURCE);
    assert!(
        !checked.diags.has_errors(),
        "accepted carrier grammar must type-check:\n{}",
        align_driver::format_diagnostics(&sources, &checked.diags),
    );
    checked.hir
}

fn struct_id(program: &align_sema::hir::Program, name: &str) -> u32 {
    program
        .structs
        .iter()
        .position(|definition| definition.source_name.rsplit('$').next() == Some(name))
        .unwrap_or_else(|| panic!("missing source-formed struct {name}")) as u32
}

fn enum_id(program: &align_sema::hir::Program, name: &str) -> u32 {
    program
        .enums
        .iter()
        .position(|definition| definition.source_name.rsplit('$').next() == Some(name))
        .unwrap_or_else(|| panic!("missing source-formed sum {name}")) as u32
}

fn struct_state(fields: impl IntoIterator<Item = LiveValue>) -> LiveValue {
    LiveValue::Struct(fields.into_iter().collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Carrier {
    DirectReader,
    BufferedReader,
    DirectWriter,
    LoggerOwnedWriter,
    ReaderStruct,
    WriterStruct,
    LoggerStruct,
    RecursiveReaderStruct,
    NestedOptionResult,
    NestedInactiveResult,
    StructRootedSum,
    LoggerRootedSum,
    SumRootedSum,
    TaggedRootedSum,
    FixedReaderStructArray,
}

impl Carrier {
    const ALL: [Self; 15] = [
        Self::DirectReader,
        Self::BufferedReader,
        Self::DirectWriter,
        Self::LoggerOwnedWriter,
        Self::ReaderStruct,
        Self::WriterStruct,
        Self::LoggerStruct,
        Self::RecursiveReaderStruct,
        Self::NestedOptionResult,
        Self::NestedInactiveResult,
        Self::StructRootedSum,
        Self::LoggerRootedSum,
        Self::SumRootedSum,
        Self::TaggedRootedSum,
        Self::FixedReaderStructArray,
    ];

    fn leaf_capacity(self) -> usize {
        match self {
            Self::DirectReader
            | Self::BufferedReader
            | Self::DirectWriter
            | Self::LoggerOwnedWriter
            | Self::ReaderStruct
            | Self::WriterStruct
            | Self::LoggerStruct
            | Self::LoggerRootedSum => 1,
            Self::RecursiveReaderStruct
            | Self::NestedOptionResult
            | Self::NestedInactiveResult
            | Self::StructRootedSum
            | Self::SumRootedSum
            | Self::TaggedRootedSum
            | Self::FixedReaderStructArray => 3,
        }
    }

    fn active_arms(self) -> &'static [ActiveArm] {
        match self {
            Self::NestedOptionResult | Self::TaggedRootedSum => {
                &[ActiveArm::ResultOk, ActiveArm::ResultErr]
            }
            Self::NestedInactiveResult => &[ActiveArm::ResultOk],
            _ => &[ActiveArm::Plain],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RootState {
    Active,
    Inactive,
    MovedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ActiveArm {
    Plain,
    ResultOk,
    ResultErr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ProvenanceClass {
    None,
    TargetOnly,
    OtherOnly,
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TargetCountClass {
    Zero,
    One,
    Multiple,
}

impl TargetCountClass {
    fn from_count(count: usize) -> Self {
        match count {
            0 => Self::Zero,
            1 => Self::One,
            _ => Self::Multiple,
        }
    }
}

#[derive(Clone, Debug)]
struct ActiveProfile {
    label: &'static str,
    provenance: ProvenanceClass,
    count: TargetCountClass,
    leaves: Vec<Option<Provenance>>,
}

fn active_profile(label: &'static str, leaves: Vec<Option<Provenance>>) -> ActiveProfile {
    let target = leaves
        .iter()
        .filter(|leaf| **leaf == Some(Provenance::Target))
        .count();
    let other = leaves
        .iter()
        .filter(|leaf| **leaf == Some(Provenance::Other))
        .count();
    let provenance = match (target > 0, other > 0) {
        (true, true) => ProvenanceClass::Mixed,
        (true, false) => ProvenanceClass::TargetOnly,
        (false, true) => ProvenanceClass::OtherOnly,
        (false, false) => ProvenanceClass::None,
    };
    ActiveProfile {
        label,
        provenance,
        count: TargetCountClass::from_count(target),
        leaves,
    }
}

fn active_profiles(capacity: usize) -> Vec<ActiveProfile> {
    match capacity {
        1 => vec![
            active_profile("other/zero", vec![Some(Provenance::Other)]),
            active_profile("target/one", vec![Some(Provenance::Target)]),
        ],
        3 => vec![
            active_profile(
                "other/zero",
                vec![
                    Some(Provenance::Other),
                    Some(Provenance::Other),
                    Some(Provenance::Other),
                ],
            ),
            active_profile("target/one", vec![Some(Provenance::Target), None, None]),
            active_profile(
                "target/multiple",
                vec![
                    Some(Provenance::Target),
                    Some(Provenance::Target),
                    Some(Provenance::Target),
                ],
            ),
            active_profile(
                "mixed/one",
                vec![
                    Some(Provenance::Target),
                    Some(Provenance::Other),
                    Some(Provenance::Other),
                ],
            ),
            active_profile(
                "mixed/multiple",
                vec![
                    Some(Provenance::Target),
                    Some(Provenance::Target),
                    Some(Provenance::Other),
                ],
            ),
        ],
        other => panic!("unsupported retainer capacity {other}"),
    }
}

#[derive(Clone, Debug)]
enum ClassifierPlan {
    Drop(DropPlan),
    FixedStructArray { element: DropPlan, length: usize },
}

impl ClassifierPlan {
    fn is_valid(&self) -> bool {
        match self {
            Self::Drop(plan) | Self::FixedStructArray { element: plan, .. } => plan.is_valid(),
        }
    }

    fn classify(&self, value: &LiveValue) -> usize {
        match self {
            Self::Drop(plan) => target_retainers(plan, value),
            Self::FixedStructArray { element, length } => {
                fixed_array_target_retainers(element, *length, value)
            }
        }
    }
}

struct Formation<'a> {
    program: &'a align_sema::hir::Program,
    reader_holder: u32,
    writer_holder: u32,
    logger_holder: u32,
    reader_triple: u32,
    nested_reader: u32,
    fallible_reader: u32,
    reader_choice: u32,
    logger_choice: u32,
    sum_choice: u32,
    tagged_choice: u32,
}

impl<'a> Formation<'a> {
    fn new(program: &'a align_sema::hir::Program) -> Self {
        Self {
            program,
            reader_holder: struct_id(program, "ReaderHolder"),
            writer_holder: struct_id(program, "WriterHolder"),
            logger_holder: struct_id(program, "LoggerHolder"),
            reader_triple: struct_id(program, "ReaderTriple"),
            nested_reader: struct_id(program, "NestedReader"),
            fallible_reader: struct_id(program, "FallibleReader"),
            reader_choice: enum_id(program, "ReaderChoice"),
            logger_choice: enum_id(program, "LoggerChoice"),
            sum_choice: enum_id(program, "SumChoice"),
            tagged_choice: enum_id(program, "TaggedChoice"),
        }
    }

    fn drop_plan(&self, ty: Ty) -> DropPlan {
        drop_plan(
            ty,
            &self.program.structs,
            &self.program.enums,
            &self.program.tagged_types,
        )
    }

    fn plan(&self, carrier: Carrier) -> ClassifierPlan {
        let ty = match carrier {
            Carrier::DirectReader | Carrier::BufferedReader => Ty::Reader,
            Carrier::DirectWriter => Ty::Writer,
            Carrier::LoggerOwnedWriter => Ty::Logger,
            Carrier::ReaderStruct => Ty::Struct(self.reader_holder),
            Carrier::WriterStruct => Ty::Struct(self.writer_holder),
            Carrier::LoggerStruct => Ty::Struct(self.logger_holder),
            Carrier::RecursiveReaderStruct => Ty::Struct(self.reader_triple),
            Carrier::NestedOptionResult => Ty::Struct(self.nested_reader),
            Carrier::NestedInactiveResult => Ty::Struct(self.fallible_reader),
            Carrier::StructRootedSum => Ty::Enum(self.reader_choice),
            Carrier::LoggerRootedSum => Ty::Enum(self.logger_choice),
            Carrier::SumRootedSum => Ty::Enum(self.sum_choice),
            Carrier::TaggedRootedSum => Ty::Enum(self.tagged_choice),
            Carrier::FixedReaderStructArray => {
                return ClassifierPlan::FixedStructArray {
                    element: self.drop_plan(Ty::Struct(self.reader_holder)),
                    length: 3,
                };
            }
        };
        ClassifierPlan::Drop(self.drop_plan(ty))
    }

    fn active_value(
        &self,
        carrier: Carrier,
        arm: ActiveArm,
        leaves: &[Option<Provenance>],
    ) -> LiveValue {
        assert_eq!(leaves.len(), carrier.leaf_capacity());
        let leaf = |index: usize, kind| reader_holder_value(kind, leaves[index]);
        let triple = || {
            struct_state([
                leaf(0, ShellKind::DirectReader),
                leaf(1, ShellKind::BufferedReader),
                leaf(2, ShellKind::DirectReader),
            ])
        };
        match carrier {
            Carrier::DirectReader => direct_value(ShellKind::DirectReader, leaves[0]),
            Carrier::BufferedReader => direct_value(ShellKind::BufferedReader, leaves[0]),
            Carrier::DirectWriter => direct_value(ShellKind::DirectWriter, leaves[0]),
            Carrier::LoggerOwnedWriter => direct_value(ShellKind::LoggerOwnedWriter, leaves[0]),
            Carrier::ReaderStruct => leaf(0, ShellKind::DirectReader),
            Carrier::WriterStruct => writer_holder_value(leaves[0]),
            Carrier::LoggerStruct => logger_holder_value(leaves[0]),
            Carrier::RecursiveReaderStruct => triple(),
            Carrier::NestedOptionResult => struct_state([LiveValue::Option(Some(Box::new(
                result_value(arm, triple()),
            )))]),
            Carrier::NestedInactiveResult => struct_state([LiveValue::Option(Some(Box::new(
                result_value(arm, triple()),
            )))]),
            Carrier::StructRootedSum => LiveValue::Sum {
                variant: 0,
                fields: vec![triple()],
            },
            Carrier::LoggerRootedSum => LiveValue::Sum {
                variant: 0,
                fields: vec![direct_value(ShellKind::LoggerOwnedWriter, leaves[0])],
            },
            Carrier::SumRootedSum => LiveValue::Sum {
                variant: 0,
                fields: vec![LiveValue::Sum {
                    variant: 0,
                    fields: vec![triple()],
                }],
            },
            Carrier::TaggedRootedSum => LiveValue::Sum {
                variant: 0,
                fields: vec![LiveValue::Option(Some(Box::new(result_value(
                    arm,
                    triple(),
                ))))],
            },
            Carrier::FixedReaderStructArray => LiveValue::FixedStructArray(vec![
                leaf(0, ShellKind::DirectReader),
                leaf(1, ShellKind::BufferedReader),
                leaf(2, ShellKind::DirectReader),
            ]),
        }
    }

    fn inactive_values(&self, carrier: Carrier) -> Vec<(&'static str, LiveValue)> {
        match carrier {
            Carrier::NestedOptionResult => {
                vec![("Option.None", struct_state([LiveValue::Option(None)]))]
            }
            Carrier::NestedInactiveResult => vec![
                ("Option.None", struct_state([LiveValue::Option(None)])),
                (
                    "Result.Err",
                    struct_state([LiveValue::Option(Some(Box::new(LiveValue::ResultErr(
                        Box::new(LiveValue::Inert),
                    ))))]),
                ),
            ],
            Carrier::StructRootedSum | Carrier::LoggerRootedSum => vec![(
                "sum.Empty",
                LiveValue::Sum {
                    variant: 1,
                    fields: vec![],
                },
            )],
            Carrier::SumRootedSum => vec![
                (
                    "outer.Empty",
                    LiveValue::Sum {
                        variant: 1,
                        fields: vec![],
                    },
                ),
                (
                    "inner.Empty",
                    LiveValue::Sum {
                        variant: 0,
                        fields: vec![LiveValue::Sum {
                            variant: 1,
                            fields: vec![],
                        }],
                    },
                ),
            ],
            Carrier::TaggedRootedSum => vec![
                (
                    "sum.Empty",
                    LiveValue::Sum {
                        variant: 1,
                        fields: vec![],
                    },
                ),
                (
                    "Option.None",
                    LiveValue::Sum {
                        variant: 0,
                        fields: vec![LiveValue::Option(None)],
                    },
                ),
            ],
            _ => vec![],
        }
    }
}

fn direct_value(kind: ShellKind, provenance: Option<Provenance>) -> LiveValue {
    provenance.map_or(LiveValue::MovedOut, |provenance| shell(kind, provenance))
}

fn reader_holder_value(kind: ShellKind, provenance: Option<Provenance>) -> LiveValue {
    provenance.map_or(LiveValue::MovedOut, |provenance| {
        struct_state([shell(kind, provenance), LiveValue::Inert])
    })
}

fn writer_holder_value(provenance: Option<Provenance>) -> LiveValue {
    provenance.map_or(LiveValue::MovedOut, |provenance| {
        struct_state([shell(ShellKind::DirectWriter, provenance)])
    })
}

fn logger_holder_value(provenance: Option<Provenance>) -> LiveValue {
    provenance.map_or(LiveValue::MovedOut, |provenance| {
        struct_state([shell(ShellKind::LoggerOwnedWriter, provenance)])
    })
}

fn result_value(arm: ActiveArm, payload: LiveValue) -> LiveValue {
    match arm {
        ActiveArm::ResultOk => LiveValue::ResultOk(Box::new(payload)),
        ActiveArm::ResultErr => LiveValue::ResultErr(Box::new(payload)),
        ActiveArm::Plain => panic!("nested Result carrier requires an active Result arm"),
    }
}

#[derive(Debug)]
struct MatrixCase {
    carrier: Carrier,
    state: RootState,
    arm: ActiveArm,
    provenance: ProvenanceClass,
    count: TargetCountClass,
    expected_target: usize,
    label: String,
    plan: ClassifierPlan,
    value: LiveValue,
}

fn ownership_matrix<'a>(formation: &Formation<'a>) -> Vec<MatrixCase> {
    let mut cases = Vec::new();
    for carrier in Carrier::ALL {
        let profiles = active_profiles(carrier.leaf_capacity());
        for &arm in carrier.active_arms() {
            for profile in &profiles {
                cases.push(MatrixCase {
                    carrier,
                    state: RootState::Active,
                    arm,
                    provenance: profile.provenance,
                    count: profile.count,
                    expected_target: profile
                        .leaves
                        .iter()
                        .filter(|leaf| **leaf == Some(Provenance::Target))
                        .count(),
                    label: format!("{carrier:?}/{arm:?}/{}", profile.label),
                    plan: formation.plan(carrier),
                    value: formation.active_value(carrier, arm, &profile.leaves),
                });
            }
        }
        for (inactive, value) in formation.inactive_values(carrier) {
            cases.push(MatrixCase {
                carrier,
                state: RootState::Inactive,
                arm: ActiveArm::Plain,
                provenance: ProvenanceClass::None,
                count: TargetCountClass::Zero,
                expected_target: 0,
                label: format!("{carrier:?}/inactive/{inactive}"),
                plan: formation.plan(carrier),
                value,
            });
        }
        cases.push(MatrixCase {
            carrier,
            state: RootState::MovedOut,
            arm: ActiveArm::Plain,
            provenance: ProvenanceClass::None,
            count: TargetCountClass::Zero,
            expected_target: 0,
            label: format!("{carrier:?}/moved-out"),
            plan: formation.plan(carrier),
            value: LiveValue::MovedOut,
        });
    }
    cases
}

#[test]
fn canonical_drop_graph_classifies_every_timeout_retainer_state() {
    let program = checked_formation();
    let formation = Formation::new(&program);
    let cases = ownership_matrix(&formation);
    let mut carriers = std::collections::BTreeSet::new();
    let mut states = std::collections::BTreeSet::new();
    let mut provenances = std::collections::BTreeSet::new();
    let mut counts = std::collections::BTreeSet::new();
    let mut active_cells = std::collections::BTreeSet::new();
    let mut moved_carriers = std::collections::BTreeSet::new();
    let mut inactive_carriers = std::collections::BTreeSet::new();

    for case in &cases {
        assert!(
            case.plan.is_valid(),
            "{}: malformed source-derived DropPlan",
            case.label,
        );
        let count = case.plan.classify(&case.value);
        assert_eq!(count, case.expected_target, "{}", case.label);
        assert_eq!(
            TargetCountClass::from_count(count),
            case.count,
            "{}",
            case.label,
        );
        assert_eq!(
            count == 0,
            case.count == TargetCountClass::Zero,
            "{}: exactly zero, and only zero, is compatible",
            case.label,
        );
        carriers.insert(case.carrier);
        states.insert(case.state);
        provenances.insert(case.provenance);
        counts.insert(case.count);
        match case.state {
            RootState::Active => {
                assert!(
                    active_cells.insert((case.carrier, case.arm, case.provenance, case.count,)),
                    "duplicate active matrix cell: {}",
                    case.label,
                );
            }
            RootState::Inactive => {
                inactive_carriers.insert(case.carrier);
            }
            RootState::MovedOut => {
                moved_carriers.insert(case.carrier);
            }
        }
    }

    assert_eq!(carriers, Carrier::ALL.into_iter().collect());
    assert_eq!(moved_carriers, Carrier::ALL.into_iter().collect());
    assert_eq!(
        states,
        [RootState::Active, RootState::Inactive, RootState::MovedOut]
            .into_iter()
            .collect(),
    );
    assert_eq!(
        provenances,
        [
            ProvenanceClass::None,
            ProvenanceClass::TargetOnly,
            ProvenanceClass::OtherOnly,
            ProvenanceClass::Mixed,
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(
        counts,
        [
            TargetCountClass::Zero,
            TargetCountClass::One,
            TargetCountClass::Multiple,
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(
        inactive_carriers,
        [
            Carrier::NestedOptionResult,
            Carrier::NestedInactiveResult,
            Carrier::StructRootedSum,
            Carrier::LoggerRootedSum,
            Carrier::SumRootedSum,
            Carrier::TaggedRootedSum,
        ]
        .into_iter()
        .collect(),
    );

    for carrier in Carrier::ALL {
        let expected_active =
            carrier.active_arms().len() * active_profiles(carrier.leaf_capacity()).len();
        assert_eq!(
            active_cells
                .iter()
                .filter(|(observed, ..)| *observed == carrier)
                .count(),
            expected_active,
            "{carrier:?}: incomplete applicable active product",
        );
    }
    // 8 singleton roots × 2 profiles, 7 three-leaf roots × 5 profiles, plus a second active
    // Result arm for each of the two retaining-Err carriers, 9 inactive paths, and 15 moved roots.
    assert_eq!(
        cases.len(),
        85,
        "the generated ownership matrix changed shape"
    );

    let fixed_ty = Ty::StructArray(formation.reader_holder, 3);
    let fixed_function = program
        .fns
        .iter()
        .find(|function| function.name.rsplit('$').next() == Some("fixed_formation"))
        .expect("source-formed fixed-array function");
    assert!(
        fixed_function
            .locals
            .iter()
            .any(|local| local.name == "values" && local.ty == fixed_ty),
        "the source fixture must form the exact fixed retaining-struct array classified below",
    );
    assert!(
        ty_is_move(
            fixed_ty,
            &program.structs,
            &program.tuples,
            &program.enums,
            &program.tagged_types,
        ),
        "fixed arrays of retaining structs must compose the recursive struct Drop",
    );
}

#[test]
fn formation_and_no_live_producer_edges_stay_closed() {
    // The accepted declarations include direct locals, recursive fields, nested Option/Result,
    // struct-backed sums, a direct logger sum, fixed Move-struct arrays, and every nameable
    // dynamic-array/slice wrapper.  Re-check here so this test owns formation independently of the
    // graph classifier above.
    let _ = checked_formation();

    for (name, ty, prelude) in [
        ("reader", "reader", ""),
        ("writer", "writer", ""),
        ("logger", "log.logger", "import std.log\n"),
    ] {
        for (container, declaration) in [
            ("array", format!("fn bad(value: array<{ty}>) -> i32 = 0")),
            ("slice", format!("fn bad(value: slice<{ty}>) -> i32 = 0")),
            (
                "fixed-array",
                format!("fn bad(value: {ty}) {{ values := [value] }}"),
            ),
            ("tuple", format!("fn bad(value: ({ty}, i64)) -> i32 = 0")),
            ("box", format!("fn bad(value: box<{ty}>) -> i32 = 0")),
        ] {
            let source = format!("{prelude}{declaration}\nfn main() -> i32 = 0\n");
            assert!(
                check_errs(
                    &format!("pkg-kv-timeout-formation-{name}-{container}"),
                    &source,
                ),
                "{name}/{container} must remain a formation negative",
            );
        }
    }
    for (name, ty) in [("reader", "reader"), ("writer", "writer")] {
        let source = format!("Bad {{ Active({ty}), Empty }}\nfn main() -> i32 = 0\n");
        assert!(
            check_errs(
                &format!("pkg-kv-timeout-formation-{name}-sum-payload"),
                &source,
            ),
            "{name}/user-sum payload must remain a formation negative",
        );
    }

    let prelude = r#"
import core.json
import std.log

ReaderHolder { value: reader, tag: i32 }
WriterHolder { value: writer }
LoggerHolder { value: log.logger }
ReaderTriple { first: ReaderHolder, second: ReaderHolder, third: ReaderHolder }
ReaderChoice { Active(ReaderTriple), Empty }
LoggerChoice { Active(log.logger), Empty }
SumChoice { Active(ReaderChoice), Empty }
TaggedChoice { Active(Option<Result<ReaderTriple, ReaderTriple>>), Empty }
ArrayField { values: array<ReaderHolder> }
ReaderChoiceArrayField { values: array<ReaderChoice> }
WriterArrayField { values: array<WriterHolder> }
LoggerArrayField { values: array<LoggerHolder> }
LoggerChoiceArrayField { values: array<LoggerChoice> }
SumChoiceArrayField { values: array<SumChoice> }
TaggedChoiceArrayField { values: array<TaggedChoice> }
"#;
    struct ProducerShape {
        label: &'static str,
        ty: &'static str,
        params: &'static str,
        value: &'static str,
        field: &'static str,
        struct_array_wrappers: bool,
    }
    let shapes = [
        ProducerShape {
            label: "reader-struct",
            ty: "ReaderHolder",
            params: "value: reader",
            value: "ReaderHolder { value: value, tag: 1 }",
            field: "ArrayField",
            struct_array_wrappers: true,
        },
        ProducerShape {
            label: "reader-sum",
            ty: "ReaderChoice",
            params: "first: reader, second: reader, third: reader",
            value: "ReaderChoice.Active(ReaderTriple { first: ReaderHolder { value: first, tag: 1 }, second: ReaderHolder { value: second, tag: 2 }, third: ReaderHolder { value: third, tag: 3 } })",
            field: "ReaderChoiceArrayField",
            struct_array_wrappers: false,
        },
        ProducerShape {
            label: "writer-struct",
            ty: "WriterHolder",
            params: "value: writer",
            value: "WriterHolder { value: value }",
            field: "WriterArrayField",
            struct_array_wrappers: true,
        },
        ProducerShape {
            label: "logger-struct",
            ty: "LoggerHolder",
            params: "value: writer",
            value: "LoggerHolder { value: log.new(value, log.level.Info) }",
            field: "LoggerArrayField",
            struct_array_wrappers: true,
        },
        ProducerShape {
            label: "logger-sum",
            ty: "LoggerChoice",
            params: "value: writer",
            value: "LoggerChoice.Active(log.new(value, log.level.Info))",
            field: "LoggerChoiceArrayField",
            struct_array_wrappers: false,
        },
        ProducerShape {
            label: "nested-sum",
            ty: "SumChoice",
            params: "first: reader, second: reader, third: reader",
            value: "SumChoice.Active(ReaderChoice.Active(ReaderTriple { first: ReaderHolder { value: first, tag: 1 }, second: ReaderHolder { value: second, tag: 2 }, third: ReaderHolder { value: third, tag: 3 } }))",
            field: "SumChoiceArrayField",
            struct_array_wrappers: false,
        },
        ProducerShape {
            label: "tagged-sum",
            ty: "TaggedChoice",
            params: "first: reader, second: reader, third: reader",
            value: "TaggedChoice.Active(Some(Ok(ReaderTriple { first: ReaderHolder { value: first, tag: 1 }, second: ReaderHolder { value: second, tag: 2 }, third: ReaderHolder { value: third, tag: 3 } })))",
            field: "TaggedChoiceArrayField",
            struct_array_wrappers: false,
        },
    ];
    for shape in shapes {
        let ProducerShape {
            label,
            ty,
            params,
            value,
            field,
            struct_array_wrappers,
        } = shape;
        let mut bodies = vec![
            (
                "materializer",
                format!("fn bad({params}) {{ rows := [{value}].to_array() }}"),
            ),
            (
                "builder",
                format!(
                    "fn bad({params}) {{ mut rows: array_builder<{ty}> := array_builder(); rows.push({value}) }}"
                ),
            ),
            (
                "region-builder",
                format!(
                    "fn bad({params}) -> i32 {{ arena out {{ mut rows: array_builder<{ty}> := array_builder(out); rows.push({value}); return 0 }} }}"
                ),
            ),
            (
                "move-slice",
                format!("fn bad({params}) {{ rows := [{value}]; view := rows[..] }}"),
            ),
            (
                "struct-field-wrapper-materializer",
                format!(
                    "fn bad({params}) {{ rows := [{value}].to_array(); wrapped := {field} {{ values: rows }} }}"
                ),
            ),
            (
                "decode",
                format!(
                    "fn bad() -> Result<(), Error> {{ rows: array<{ty}> := json.decode(\"[]\")?; return Ok(()) }}"
                ),
            ),
        ];
        if struct_array_wrappers {
            bodies.extend([
                (
                    "nested-dynamic-materializer",
                    format!("fn bad(rows: array<{ty}>) {{ batches := [rows].to_array() }}"),
                ),
                (
                    "nested-dynamic-slice",
                    format!(
                        "fn bad(rows: array<{ty}>) {{ batches := [rows]; view := batches[..] }}"
                    ),
                ),
                (
                    "tuple-wrapper-materializer",
                    format!(
                        "fn bad({params}) {{ rows := [{value}].to_array(); wrapped := (rows, 1) }}"
                    ),
                ),
                (
                    "option-wrapper-materializer",
                    format!(
                        "fn bad({params}) {{ rows := [{value}].to_array(); wrapped: Option<array<{ty}>> := Some(rows) }}"
                    ),
                ),
                (
                    "result-wrapper-materializer",
                    format!(
                        "fn bad({params}) {{ rows := [{value}].to_array(); wrapped: Result<array<{ty}>, Error> := Ok(rows) }}"
                    ),
                ),
            ]);
        }
        for (producer, body) in bodies {
            let source = format!("{prelude}{body}\nfn main() -> i32 = 0\n");
            assert!(
                check_errs(
                    &format!("pkg-kv-timeout-no-producer-{label}-{producer}"),
                    &source,
                ),
                "{label}/{producer} must not produce a live handle-retaining dynamic value",
            );
        }
    }
}

const LIFECYCLE_SOURCE_TEMPLATE: &str = r#"
import std.net
import std.log

extern "C" {
  fn align_rt_tcp_conn_set_io_timeout(connection: raw, timeout_ns: i64) -> i32
  fn align_kv_owner_reset()
  fn align_kv_owner_target_raw() -> raw
  fn align_kv_owner_target_live() -> i32
  fn align_kv_owner_total_live() -> i32
  fn align_kv_owner_mark_classification(target: i32, total: i32)
  fn align_kv_owner_protocol_errors() -> i32
fn align_kv_owner_final_ok() -> i32
}

ReaderHolder { value: reader, tag: i32 }
WriterHolder { value: writer }
LoggerHolder { value: log.logger }
ReaderTriple { first: ReaderHolder, second: ReaderHolder, third: ReaderHolder }
NestedReader { value: Option<Result<ReaderTriple, ReaderTriple>> }
FallibleReader { value: Option<Result<ReaderTriple, bool>> }
ReaderChoice { Active(ReaderTriple), Empty }
LoggerChoice { Active(log.logger), Empty }
SumChoice { Active(ReaderChoice), Empty }
TaggedChoice { Active(Option<Result<ReaderTriple, ReaderTriple>>), Empty }

fn observe(target: i32, total: i32) -> bool {
  unsafe {
    return align_kv_owner_target_live() == target
      && align_kv_owner_total_live() == total
      && align_kv_owner_protocol_errors() == 0
  }
}

fn probe(target: i32, total: i32) -> bool {
  unsafe {
    if !observe(target, total) { return false }
    align_kv_owner_mark_classification(target, total)
    status := align_rt_tcp_conn_set_io_timeout(align_kv_owner_target_raw(), 1234)
    if target == 0 {
      if status != 0 { return false }
    } else {
      if status != 2 { return false }
    }
    return observe(target, total)
  }
}

fn drop_reader(value: reader) {}
fn drop_writer(value: writer) {}
fn drop_logger(value: log.logger) {}
fn drop_holder(value: ReaderHolder) {}
fn drop_writer_holder(value: WriterHolder) {}
fn drop_logger_holder(value: LoggerHolder) {}
fn drop_triple(value: ReaderTriple) {}
fn drop_nested(value: NestedReader) {}
fn drop_fallible(value: FallibleReader) {}
fn drop_choice(value: ReaderChoice) {}
fn drop_logger_choice(value: LoggerChoice) {}
fn drop_sum_choice(value: SumChoice) {}
fn drop_tagged_choice(value: TaggedChoice) {}
fn returned_reader(borrow connection: tcp_conn) -> reader = connection.reader()

fn mixed_triple(borrow target: tcp_conn, borrow other: tcp_conn, tag: i32) -> ReaderTriple {
  return ReaderTriple {
    first: ReaderHolder { value: target.reader(), tag: tag },
    second: ReaderHolder { value: other.reader(), tag: tag + 1 },
    third: ReaderHolder { value: other.reader(), tag: tag + 2 },
  }
}

fn target_triple(borrow connection: tcp_conn, tag: i32) -> ReaderTriple {
  return ReaderTriple {
    first: ReaderHolder { value: connection.reader(), tag: tag },
    second: ReaderHolder { value: connection.reader(), tag: tag + 1 },
    third: ReaderHolder { value: connection.reader(), tag: tag + 2 },
  }
}

fn fixed_cycle(borrow target: tcp_conn, borrow other: tcp_conn) -> bool {
  fixed := [
    ReaderHolder { value: target.reader(), tag: 130 },
    ReaderHolder { value: target.reader(), tag: 131 },
    ReaderHolder { value: other.reader(), tag: 132 },
  ]
  return probe(__FIXED_MIXED_MULTIPLE__)
}

fn exercise() -> Result<i32, Error> {
  unsafe { align_kv_owner_reset() }
  target := tcp.connect("target", 1)?
  other := tcp.connect("other", 2)?
  if !probe(__ZERO__) { return Ok(10) }

  direct := target.reader()
  if !probe(__DIRECT_READER__) { return Ok(11) }
  drop_reader(direct)
  if !probe(__ZERO__) { return Ok(12) }

  base := target.reader()
  buffered := base.buffered()
  if !probe(__BUFFERED_READER__) { return Ok(13) }
  drop_reader(buffered)
  if !probe(__ZERO__) { return Ok(14) }

  output := target.writer()
  if !probe(__DIRECT_WRITER__) { return Ok(15) }
  drop_writer(output)
  if !probe(__ZERO__) { return Ok(16) }

  writer_holder := WriterHolder { value: target.writer() }
  if !probe(__WRITER_STRUCT__) { return Ok(151) }
  drop_writer_holder(writer_holder)
  if !probe(__ZERO__) { return Ok(152) }

  logger := log.new(target.writer(), log.level.Info)
  if !probe(__DIRECT_LOGGER__) { return Ok(17) }
  drop_logger(logger)
  if !probe(__ZERO__) { return Ok(18) }

  logger_holder := LoggerHolder { value: log.new(target.writer(), log.level.Info) }
  if !probe(__LOGGER_STRUCT__) { return Ok(171) }
  drop_logger_holder(logger_holder)
  if !probe(__ZERO__) { return Ok(172) }

  returned := returned_reader(target)
  if !probe(__RETURNED_READER__) { return Ok(19) }
  drop_reader(returned)
  if !probe(__ZERO__) { return Ok(20) }

  holder := ReaderHolder { value: target.reader(), tag: 7 }
  if !probe(__READER_STRUCT__) { return Ok(21) }
  moved := holder.value
  if holder.tag != 7 || !probe(__DIRECT_READER__) { return Ok(22) }
  drop_reader(moved)
  if !probe(__ZERO__) { return Ok(23) }

  recursive := target_triple(target, 30)
  if !probe(__RECURSIVE_TARGET_MULTIPLE__) { return Ok(24) }
  drop_triple(recursive)
  if !probe(__ZERO__) { return Ok(25) }

  nested := NestedReader { value: Some(Ok(mixed_triple(target, other, 40))) }
  if !probe(__NESTED_OK_MIXED_ONE__) { return Ok(26) }
  drop_nested(nested)
  if !probe(__ZERO__) { return Ok(27) }

  nested_err := NestedReader { value: Some(Err(mixed_triple(target, other, 50))) }
  if !probe(__NESTED_ERR_MIXED_ONE__) { return Ok(28) }
  drop_nested(nested_err)
  if !probe(__ZERO__) { return Ok(29) }

  fallible_active := FallibleReader { value: Some(Ok(mixed_triple(target, other, 55))) }
  if !probe(__FALLIBLE_OK_MIXED_ONE__) { return Ok(281) }
  drop_fallible(fallible_active)
  if !probe(__ZERO__) { return Ok(282) }

  choice := ReaderChoice.Active(mixed_triple(target, other, 60))
  if !probe(__STRUCT_SUM_MIXED_ONE__) { return Ok(30) }
  drop_choice(choice)
  if !probe(__ZERO__) { return Ok(31) }

  inactive := ReaderChoice.Empty
  if !probe(__INACTIVE_STRUCT_SUM__) { return Ok(32) }
  drop_choice(inactive)

  inactive_result: FallibleReader := FallibleReader { value: Some(Err(false)) }
  if !probe(__INACTIVE_RESULT__) { return Ok(321) }
  drop_fallible(inactive_result)

  moved_choice := ReaderChoice.Active(mixed_triple(target, other, 70))
  if !probe(__STRUCT_SUM_MIXED_ONE__) { return Ok(33) }
  match moved_choice {
    Active(held) => {
      if !probe(__STRUCT_SUM_MIXED_ONE__) { return Ok(34) }
      drop_triple(held)
      if !probe(__ZERO__) { return Ok(35) }
    }
    Empty => { return Ok(36) }
  }
  if !probe(__ZERO__) { return Ok(37) }

  logger_choice := LoggerChoice.Active(log.new(target.writer(), log.level.Info))
  if !probe(__LOGGER_SUM__) { return Ok(38) }
  drop_logger_choice(logger_choice)
  if !probe(__ZERO__) { return Ok(39) }

  sum_choice := SumChoice.Active(ReaderChoice.Active(mixed_triple(target, other, 80)))
  if !probe(__SUM_SUM_MIXED_ONE__) { return Ok(40) }
  drop_sum_choice(sum_choice)
  if !probe(__ZERO__) { return Ok(41) }

  tagged_ok := TaggedChoice.Active(Some(Ok(mixed_triple(target, other, 90))))
  if !probe(__TAGGED_SUM_OK_MIXED_ONE__) { return Ok(42) }
  drop_tagged_choice(tagged_ok)
  if !probe(__ZERO__) { return Ok(43) }

  tagged_err := TaggedChoice.Active(Some(Err(mixed_triple(target, other, 100))))
  if !probe(__TAGGED_SUM_ERR_MIXED_ONE__) { return Ok(44) }
  drop_tagged_choice(tagged_err)
  if !probe(__ZERO__) { return Ok(45) }

  other_reader := other.reader()
  if !probe(__OTHER_READER__) { return Ok(46) }
  drop_reader(other_reader)
  if !probe(__ZERO__) { return Ok(47) }

  mixed := mixed_triple(target, other, 110)
  if !probe(__RECURSIVE_MIXED_ONE__) { return Ok(48) }
  drop_triple(mixed)
  if !probe(__ZERO__) { return Ok(49) }

  if !fixed_cycle(target, other) { return Ok(50) }
  if !probe(__ZERO__) { return Ok(51) }

  maybe := Some(ReaderHolder { value: target.reader(), tag: 15 })
  if !probe(__READER_STRUCT__) { return Ok(52) }
  unwrapped := maybe else { return Ok(53) }
  extracted := unwrapped.value
  if !probe(__DIRECT_READER__) { return Ok(54) }
  drop_reader(extracted)
  if !probe(__ZERO__) { return Ok(55) }
  return Ok(0)
}

pub fn main() -> i32 {
  code := exercise() else { return 90 }
  unsafe {
    if code != 0 { return code }
    if align_kv_owner_final_ok() != 1 { return 91 }
  }
  return 0
}
"#;

fn replace_classified_probe(
    source: &mut String,
    token: &str,
    formation: &Formation<'_>,
    carrier: Carrier,
    arm: ActiveArm,
    leaves: &[Option<Provenance>],
) {
    let plan = formation.plan(carrier);
    let value = formation.active_value(carrier, arm, leaves);
    let target = plan.classify(&value);
    let total = leaves.iter().filter(|leaf| leaf.is_some()).count();
    assert!(
        source.contains(token),
        "lifecycle template omitted classifier token {token}",
    );
    *source = source.replace(token, &format!("{target}, {total}"));
}

fn replace_classified_state(
    source: &mut String,
    token: &str,
    formation: &Formation<'_>,
    carrier: Carrier,
    value: LiveValue,
    total: usize,
) {
    let target = formation.plan(carrier).classify(&value);
    assert!(
        source.contains(token),
        "lifecycle template omitted classifier token {token}",
    );
    *source = source.replace(token, &format!("{target}, {total}"));
}

fn lifecycle_source() -> String {
    let program = checked_formation();
    let formation = Formation::new(&program);
    let target = Some(Provenance::Target);
    let other = Some(Provenance::Other);
    let mut source = LIFECYCLE_SOURCE_TEMPLATE.to_owned();

    replace_classified_state(
        &mut source,
        "__ZERO__",
        &formation,
        Carrier::DirectReader,
        LiveValue::MovedOut,
        0,
    );
    for (token, carrier) in [
        ("__DIRECT_READER__", Carrier::DirectReader),
        ("__BUFFERED_READER__", Carrier::BufferedReader),
        ("__DIRECT_WRITER__", Carrier::DirectWriter),
        ("__WRITER_STRUCT__", Carrier::WriterStruct),
        ("__DIRECT_LOGGER__", Carrier::LoggerOwnedWriter),
        ("__LOGGER_STRUCT__", Carrier::LoggerStruct),
        ("__RETURNED_READER__", Carrier::DirectReader),
        ("__READER_STRUCT__", Carrier::ReaderStruct),
        ("__LOGGER_SUM__", Carrier::LoggerRootedSum),
    ] {
        replace_classified_probe(
            &mut source,
            token,
            &formation,
            carrier,
            ActiveArm::Plain,
            &[target],
        );
    }
    replace_classified_probe(
        &mut source,
        "__RECURSIVE_TARGET_MULTIPLE__",
        &formation,
        Carrier::RecursiveReaderStruct,
        ActiveArm::Plain,
        &[target, target, target],
    );
    for (token, carrier, arm) in [
        (
            "__NESTED_OK_MIXED_ONE__",
            Carrier::NestedOptionResult,
            ActiveArm::ResultOk,
        ),
        (
            "__NESTED_ERR_MIXED_ONE__",
            Carrier::NestedOptionResult,
            ActiveArm::ResultErr,
        ),
        (
            "__FALLIBLE_OK_MIXED_ONE__",
            Carrier::NestedInactiveResult,
            ActiveArm::ResultOk,
        ),
        (
            "__STRUCT_SUM_MIXED_ONE__",
            Carrier::StructRootedSum,
            ActiveArm::Plain,
        ),
        (
            "__SUM_SUM_MIXED_ONE__",
            Carrier::SumRootedSum,
            ActiveArm::Plain,
        ),
        (
            "__TAGGED_SUM_OK_MIXED_ONE__",
            Carrier::TaggedRootedSum,
            ActiveArm::ResultOk,
        ),
        (
            "__TAGGED_SUM_ERR_MIXED_ONE__",
            Carrier::TaggedRootedSum,
            ActiveArm::ResultErr,
        ),
        (
            "__RECURSIVE_MIXED_ONE__",
            Carrier::RecursiveReaderStruct,
            ActiveArm::Plain,
        ),
    ] {
        replace_classified_probe(
            &mut source,
            token,
            &formation,
            carrier,
            arm,
            &[target, other, other],
        );
    }
    replace_classified_probe(
        &mut source,
        "__OTHER_READER__",
        &formation,
        Carrier::DirectReader,
        ActiveArm::Plain,
        &[other],
    );
    replace_classified_probe(
        &mut source,
        "__FIXED_MIXED_MULTIPLE__",
        &formation,
        Carrier::FixedReaderStructArray,
        ActiveArm::Plain,
        &[target, target, other],
    );
    let inactive = formation
        .inactive_values(Carrier::StructRootedSum)
        .into_iter()
        .next()
        .expect("struct-rooted sum has an inactive variant")
        .1;
    replace_classified_state(
        &mut source,
        "__INACTIVE_STRUCT_SUM__",
        &formation,
        Carrier::StructRootedSum,
        inactive,
        0,
    );
    let inactive_result = formation
        .inactive_values(Carrier::NestedInactiveResult)
        .into_iter()
        .find(|(label, _)| *label == "Result.Err")
        .expect("fallible nested carrier has an inactive Result arm")
        .1;
    replace_classified_state(
        &mut source,
        "__INACTIVE_RESULT__",
        &formation,
        Carrier::NestedInactiveResult,
        inactive_result,
        0,
    );
    assert!(
        !source.contains("__"),
        "unexpanded classifier token in lifecycle source",
    );
    source
}

const LIFECYCLE_C: &str = r#"
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define CONN_MAGIC 0x434f4e4eu
#define SHELL_MAGIC 0x5348454cu
#define LOGGER_MAGIC 0x4c4f4747u
#define DIRECT_READER_KIND 1
#define WRITER_KIND 2
#define BUFFERED_READER_KIND 3
#define MAX_TOKENS 256
#define EXPECTED_READER_ALLOCS 39
#define EXPECTED_WRITER_ALLOCS 5
#define EXPECTED_DIRECT_READER_FREES 38
#define EXPECTED_BUFFERED_READER_FREES 1
#define EXPECTED_DIRECT_WRITER_FREES 2
#define EXPECTED_LOGGER_WRITER_FREES 3

typedef struct Conn {
    uint32_t magic;
    int32_t id;
    int32_t live;
} Conn;

typedef struct Shell {
    uint32_t magic;
    Conn *connection;
    int32_t kind;
    int32_t live;
} Shell;

typedef struct Logger {
    uint32_t magic;
    Shell *writer;
    int32_t live;
} Logger;

static Conn *target;
static Conn *connections[MAX_TOKENS];
static Shell *shells[MAX_TOKENS];
static Logger *loggers[MAX_TOKENS];
static int32_t connection_tokens;
static int32_t shell_tokens;
static int32_t logger_tokens;
static int32_t next_connection;
static int32_t connection_allocs;
static int32_t connection_frees;
static int32_t shell_allocs;
static int32_t shell_frees;
static int32_t reader_allocs;
static int32_t writer_allocs;
static int32_t buffered_reader_transitions;
static int32_t direct_reader_frees;
static int32_t buffered_reader_frees;
static int32_t direct_writer_frees;
static int32_t logger_writer_frees;
static int32_t logger_allocs;
static int32_t logger_frees;
static int32_t target_live;
static int32_t total_live;
static int32_t timeout_calls;
static int32_t classification_marks;
static int32_t admitted_timeout_calls;
static int32_t excluded_timeout_calls;
static int32_t pending_target;
static int32_t pending_total;
static int32_t protocol_errors;

static int conn_registered(Conn *connection) {
    if (connection == NULL) {
        return 0;
    }
    for (int32_t index = 0; index < connection_tokens; index += 1) {
        if (connections[index] == connection) {
            return 1;
        }
    }
    return 0;
}

static int conn_valid(Conn *connection) {
    return conn_registered(connection)
        && connection->live == 1
        && connection->magic == CONN_MAGIC;
}

static int shell_registered(Shell *shell) {
    if (shell == NULL) {
        return 0;
    }
    for (int32_t index = 0; index < shell_tokens; index += 1) {
        if (shells[index] == shell) {
            return 1;
        }
    }
    return 0;
}

static int logger_registered(Logger *logger) {
    if (logger == NULL) {
        return 0;
    }
    for (int32_t index = 0; index < logger_tokens; index += 1) {
        if (loggers[index] == logger) {
            return 1;
        }
    }
    return 0;
}

static Shell *new_shell(Conn *connection, int32_t kind) {
    if (!conn_valid(connection)
        || (kind != DIRECT_READER_KIND && kind != WRITER_KIND)) {
        protocol_errors += 1;
        return NULL;
    }
    Shell *shell = (Shell *)malloc(sizeof(Shell));
    if (shell == NULL) {
        protocol_errors += 1;
        return NULL;
    }
    if (shell_tokens >= MAX_TOKENS) {
        protocol_errors += 1;
        free(shell);
        return NULL;
    }
    shell->magic = SHELL_MAGIC;
    shell->connection = connection;
    shell->kind = kind;
    shell->live = 1;
    shells[shell_tokens++] = shell;
    shell_allocs += 1;
    if (kind == DIRECT_READER_KIND) {
        reader_allocs += 1;
    } else {
        writer_allocs += 1;
    }
    total_live += 1;
    if (connection == target) {
        target_live += 1;
    }
    return shell;
}

static int shell_valid(Shell *shell) {
    return shell_registered(shell)
        && shell->live == 1
        && shell->magic == SHELL_MAGIC
        && conn_valid(shell->connection);
}

static void retire_valid_shell(Shell *shell) {
    if (shell->connection == target) {
        target_live -= 1;
    }
    total_live -= 1;
    shell_frees += 1;
    shell->live = 0;
    shell->magic = 0;
}

static void free_reader_shell(Shell *reader) {
    if (reader == NULL) {
        return;
    }
    if (!shell_valid(reader)
        || (reader->kind != DIRECT_READER_KIND
            && reader->kind != BUFFERED_READER_KIND)) {
        protocol_errors += 1;
        return;
    }
    int32_t kind = reader->kind;
    retire_valid_shell(reader);
    if (kind == DIRECT_READER_KIND) {
        direct_reader_frees += 1;
    } else {
        buffered_reader_frees += 1;
    }
}

static void free_writer_shell(Shell *writer, int32_t from_logger) {
    if (writer == NULL) {
        return;
    }
    if (!shell_valid(writer) || writer->kind != WRITER_KIND
        || (from_logger != 0 && from_logger != 1)) {
        protocol_errors += 1;
        return;
    }
    retire_valid_shell(writer);
    if (from_logger == 0) {
        direct_writer_frees += 1;
    } else {
        logger_writer_frees += 1;
    }
}

void align_kv_owner_reset(void) {
    target = NULL;
    connection_tokens = 0;
    shell_tokens = 0;
    logger_tokens = 0;
    next_connection = 0;
    connection_allocs = 0;
    connection_frees = 0;
    shell_allocs = 0;
    shell_frees = 0;
    reader_allocs = 0;
    writer_allocs = 0;
    buffered_reader_transitions = 0;
    direct_reader_frees = 0;
    buffered_reader_frees = 0;
    direct_writer_frees = 0;
    logger_writer_frees = 0;
    logger_allocs = 0;
    logger_frees = 0;
    target_live = 0;
    total_live = 0;
    timeout_calls = 0;
    classification_marks = 0;
    admitted_timeout_calls = 0;
    excluded_timeout_calls = 0;
    pending_target = -1;
    pending_total = -1;
    protocol_errors = 0;
}

void *align_kv_owner_target_raw(void) { return target; }
int32_t align_kv_owner_target_live(void) { return target_live; }
int32_t align_kv_owner_total_live(void) { return total_live; }
int32_t align_kv_owner_protocol_errors(void) { return protocol_errors; }

void align_kv_owner_mark_classification(int32_t expected_target, int32_t expected_total) {
    classification_marks += 1;
    if (pending_target != -1 || pending_total != -1
        || expected_target < 0 || expected_total < expected_target
        || target_live != expected_target || total_live != expected_total) {
        protocol_errors += 1;
    }
    pending_target = expected_target;
    pending_total = expected_total;
}

int32_t align_kv_owner_tcp_connect(
    const uint8_t *host,
    int64_t host_len,
    int64_t port,
    int64_t timeout_ns,
    Conn **out
) {
    if (host == NULL || host_len <= 0 || port <= 0 || timeout_ns != 0 || out == NULL) {
        protocol_errors += 1;
        return 2;
    }
    Conn *connection = (Conn *)malloc(sizeof(Conn));
    if (connection == NULL) {
        protocol_errors += 1;
        return 2;
    }
    if (connection_tokens >= MAX_TOKENS) {
        protocol_errors += 1;
        free(connection);
        return 2;
    }
    connection->magic = CONN_MAGIC;
    connection->id = next_connection++;
    connection->live = 1;
    connections[connection_tokens++] = connection;
    connection_allocs += 1;
    if (target == NULL) {
        target = connection;
    }
    *out = connection;
    return 0;
}

void align_kv_owner_tcp_conn_free(Conn *connection) {
    if (!conn_valid(connection)) {
        protocol_errors += 1;
        return;
    }
    if (connection == target && target_live != 0) {
        protocol_errors += 1;
    }
    connection->live = 0;
    connection->magic = 0;
    connection_frees += 1;
}

Shell *align_kv_owner_tcp_conn_reader(Conn *connection) {
    return new_shell(connection, DIRECT_READER_KIND);
}

Shell *align_kv_owner_tcp_conn_writer(Conn *connection) {
    return new_shell(connection, WRITER_KIND);
}

Shell *align_kv_owner_io_reader_buffered(Shell *reader) {
    if (!shell_registered(reader)
        || reader->live != 1
        || reader->magic != SHELL_MAGIC
        || reader->kind != DIRECT_READER_KIND
        || !conn_valid(reader->connection)) {
        protocol_errors += 1;
        return NULL;
    }
    reader->kind = BUFFERED_READER_KIND;
    buffered_reader_transitions += 1;
    return reader;
}

void align_kv_owner_io_reader_free(Shell *reader) { free_reader_shell(reader); }
void align_kv_owner_io_writer_free(Shell *writer) { free_writer_shell(writer, 0); }

Logger *align_kv_owner_log_new(Shell *writer, int32_t minimum) {
    if (!shell_registered(writer)
        || writer->live != 1
        || writer->magic != SHELL_MAGIC
        || writer->kind != WRITER_KIND
        || !conn_valid(writer->connection)
        || minimum < 0
        || minimum > 4) {
        protocol_errors += 1;
        return NULL;
    }
    Logger *logger = (Logger *)malloc(sizeof(Logger));
    if (logger == NULL) {
        protocol_errors += 1;
        return NULL;
    }
    if (logger_tokens >= MAX_TOKENS) {
        protocol_errors += 1;
        free(logger);
        return NULL;
    }
    logger->magic = LOGGER_MAGIC;
    logger->writer = writer;
    logger->live = 1;
    loggers[logger_tokens++] = logger;
    logger_allocs += 1;
    return logger;
}

void align_kv_owner_log_free(Logger *logger) {
    if (logger == NULL) {
        return;
    }
    if (!logger_registered(logger)
        || logger->live != 1
        || logger->magic != LOGGER_MAGIC
        || !shell_valid(logger->writer)
        || logger->writer->kind != WRITER_KIND) {
        protocol_errors += 1;
        return;
    }
    logger->live = 0;
    logger->magic = 0;
    free_writer_shell(logger->writer, 1);
    logger_frees += 1;
}

int32_t align_kv_owner_set_io_timeout(Conn *connection, int64_t timeout_ns) {
    timeout_calls += 1;
    int32_t expected_target = pending_target;
    int32_t expected_total = pending_total;
    pending_target = -1;
    pending_total = -1;
    if (expected_target < 0 || expected_total < expected_target
        || target_live != expected_target || total_live != expected_total) {
        protocol_errors += 1;
        return 2;
    }
    if (!conn_valid(connection) || connection != target || timeout_ns != 1234) {
        protocol_errors += 1;
        return 2;
    }
    if (expected_target != 0) {
        excluded_timeout_calls += 1;
        return 2;
    }
    admitted_timeout_calls += 1;
    return 0;
}

int32_t align_kv_owner_final_ok(void) {
    return target_live == 0
        && total_live == 0
        && pending_target == -1
        && pending_total == -1
        && timeout_calls == classification_marks
        && admitted_timeout_calls > 0
        && excluded_timeout_calls > 0
        && protocol_errors == 0
        && shell_allocs == EXPECTED_READER_ALLOCS + EXPECTED_WRITER_ALLOCS
        && shell_frees == EXPECTED_READER_ALLOCS + EXPECTED_WRITER_ALLOCS
        && reader_allocs == EXPECTED_READER_ALLOCS
        && writer_allocs == EXPECTED_WRITER_ALLOCS
        && buffered_reader_transitions == EXPECTED_BUFFERED_READER_FREES
        && direct_reader_frees == EXPECTED_DIRECT_READER_FREES
        && buffered_reader_frees == EXPECTED_BUFFERED_READER_FREES
        && direct_writer_frees == EXPECTED_DIRECT_WRITER_FREES
        && logger_writer_frees == EXPECTED_LOGGER_WRITER_FREES
        && logger_allocs == EXPECTED_LOGGER_WRITER_FREES
        && logger_frees == EXPECTED_LOGGER_WRITER_FREES
        && connection_allocs == 2
        && connection_frees == 2;
}
"#;

const RENAMED_RUNTIME_SYMBOLS: &[(&str, &str)] = &[
    ("align_rt_tcp_connect", "align_kv_owner_tcp_connect"),
    ("align_rt_tcp_conn_free", "align_kv_owner_tcp_conn_free"),
    ("align_rt_tcp_conn_reader", "align_kv_owner_tcp_conn_reader"),
    ("align_rt_tcp_conn_writer", "align_kv_owner_tcp_conn_writer"),
    (
        "align_rt_tcp_conn_set_io_timeout",
        "align_kv_owner_set_io_timeout",
    ),
    (
        "align_rt_io_reader_buffered",
        "align_kv_owner_io_reader_buffered",
    ),
    ("align_rt_io_reader_free", "align_kv_owner_io_reader_free"),
    ("align_rt_io_writer_free", "align_kv_owner_io_writer_free"),
    ("align_rt_log_new", "align_kv_owner_log_new"),
    ("align_rt_log_free", "align_kv_owner_log_free"),
];

struct TempArtifacts(Vec<PathBuf>);

static ARTIFACT_NONCE: AtomicU64 = AtomicU64::new(0);

impl Drop for TempArtifacts {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Default)]
struct DrainedPipe {
    bytes: Vec<u8>,
    error: Option<String>,
    overflow: bool,
    eof: bool,
}

struct PipeDrainState {
    capture: Mutex<DrainedPipe>,
}

struct PipeDrain {
    label: &'static str,
    state: Arc<PipeDrainState>,
    handle: Option<std::thread::JoinHandle<()>>,
    capture: DrainedPipe,
}

#[cfg(unix)]
trait CapturedPipe: Read + Send + std::os::fd::AsRawFd {}

#[cfg(unix)]
impl<T: Read + Send + std::os::fd::AsRawFd> CapturedPipe for T {}

#[cfg(not(unix))]
trait CapturedPipe: Read + Send {}

#[cfg(not(unix))]
impl<T: Read + Send> CapturedPipe for T {}

fn sleep_until(deadline: Instant, interval: Duration) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        std::thread::sleep(remaining.min(interval));
    }
}

#[cfg(unix)]
fn set_pipe_nonblocking(
    pipe: &impl CapturedPipe,
    deadline: Instant,
    pipe_label: &str,
) -> Result<(), String> {
    let fd = pipe.as_raw_fd();
    let flags = loop {
        // SAFETY: `fd` belongs to the live child pipe, and F_GETFL does not use a third argument.
        let result = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if result >= 0 {
            break result;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
            continue;
        }
        return Err(format!(
            "make {pipe_label} nonblocking (get flags): {error}"
        ));
    };
    loop {
        // SAFETY: `fd` belongs to the live child pipe, and the existing flags remain valid with
        // O_NONBLOCK added.
        let result = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if result >= 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
            continue;
        }
        return Err(format!(
            "make {pipe_label} nonblocking (set flags): {error}"
        ));
    }
}

#[cfg(not(unix))]
fn set_pipe_nonblocking(
    _pipe: &impl CapturedPipe,
    _deadline: Instant,
    _pipe_label: &str,
) -> Result<(), String> {
    Ok(())
}

fn update_pipe_capture(state: &PipeDrainState, update: impl FnOnce(&mut DrainedPipe)) {
    let mut capture = state
        .capture
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    update(&mut capture);
}

fn start_pipe_drain(
    mut pipe: impl CapturedPipe + 'static,
    cancel: Arc<AtomicBool>,
    setup_deadline: Instant,
    pipe_label: &'static str,
) -> Result<PipeDrain, String> {
    set_pipe_nonblocking(&pipe, setup_deadline, pipe_label)?;
    let state = Arc::new(PipeDrainState {
        capture: Mutex::new(DrainedPipe::default()),
    });
    let worker_state = Arc::clone(&state);
    let handle = std::thread::Builder::new()
        .name(format!("align-{pipe_label}-drain"))
        .spawn(move || {
            let mut chunk = [0_u8; 4096];
            loop {
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                match pipe.read(&mut chunk) {
                    Ok(0) => {
                        update_pipe_capture(&worker_state, |capture| capture.eof = true);
                        break;
                    }
                    Ok(count) => update_pipe_capture(&worker_state, |capture| {
                        let retained =
                            count.min(MAX_CAPTURE_BYTES.saturating_sub(capture.bytes.len()));
                        capture.bytes.extend_from_slice(&chunk[..retained]);
                        capture.overflow |= retained != count;
                    }),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(CHILD_POLL_INTERVAL);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        update_pipe_capture(&worker_state, |capture| {
                            capture.error = Some(error.to_string());
                        });
                        break;
                    }
                }
            }
        })
        .map_err(|error| format!("spawn {pipe_label} drain: {error}"))?;
    Ok(PipeDrain {
        label: pipe_label,
        state,
        handle: Some(handle),
        capture: DrainedPipe::default(),
    })
}

fn join_finished_pipe_drains(drains: &mut [PipeDrain], issues: &mut Vec<String>) {
    for drain in drains {
        let finished = drain
            .handle
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished);
        if !finished {
            continue;
        }
        let Some(handle) = drain.handle.take() else {
            continue;
        };
        if handle.join().is_err() {
            issues.push(format!("{} drain thread panicked", drain.label));
        }
    }
}

fn wait_for_pipe_drains_until(
    drains: &mut [PipeDrain],
    deadline: Instant,
    issues: &mut Vec<String>,
) {
    loop {
        join_finished_pipe_drains(drains, issues);
        if drains.iter().all(|drain| drain.handle.is_none()) || Instant::now() >= deadline {
            return;
        }
        sleep_until(deadline, CHILD_POLL_INTERVAL);
    }
}

fn finish_pipe_drains(
    drains: &mut [PipeDrain],
    cancel: &AtomicBool,
    cleanup_deadline: Instant,
) -> Vec<String> {
    let mut issues = Vec::new();
    let now = Instant::now();
    let eof_deadline = cleanup_deadline
        .checked_sub(DRAIN_CANCEL_RESERVE)
        .unwrap_or(now)
        .max(now);
    wait_for_pipe_drains_until(drains, eof_deadline, &mut issues);
    cancel.store(true, Ordering::Release);
    wait_for_pipe_drains_until(drains, cleanup_deadline, &mut issues);
    join_finished_pipe_drains(drains, &mut issues);

    for drain in drains {
        if drain.handle.is_some() {
            issues.push(format!(
                "{} drain thread exceeded the cleanup deadline",
                drain.label
            ));
        }
        drain.capture = match drain.state.capture.try_lock() {
            Ok(capture) => capture.clone(),
            Err(TryLockError::Poisoned(poisoned)) => {
                issues.push(format!("{} capture mutex was poisoned", drain.label));
                poisoned.into_inner().clone()
            }
            Err(TryLockError::WouldBlock) => {
                issues.push(format!(
                    "{} capture unavailable without blocking at cleanup deadline",
                    drain.label
                ));
                DrainedPipe::default()
            }
        };
        if let Some(error) = &drain.capture.error {
            issues.push(format!("{} capture failed: {error}", drain.label));
        }
        if drain.capture.overflow {
            issues.push(format!(
                "{} capture exceeded its {MAX_CAPTURE_BYTES}-byte cap",
                drain.label
            ));
        }
        if !drain.capture.eof {
            issues.push(format!("{} drain did not reach EOF", drain.label));
        }
    }
    issues
}

fn isolate_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn kill_process_group_until(child: &std::process::Child, deadline: Instant) -> Option<String> {
    #[cfg(unix)]
    let result = match i32::try_from(child.id()) {
        Ok(pid) => loop {
            // SAFETY: the child was placed in a fresh process group whose id is its positive pid.
            let result = unsafe { libc::kill(-pid, libc::SIGKILL) };
            if result == 0 {
                break None;
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                break None;
            }
            if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
                continue;
            }
            break Some(error.to_string());
        },
        Err(error) => Some(format!("child pid is not representable as i32: {error}")),
    };
    #[cfg(not(unix))]
    let result = {
        let _ = (child, deadline);
        None
    };

    result
}

struct DirectKillFailure {
    message: String,
    exited_race: bool,
}

fn kill_direct_until(
    child: &mut std::process::Child,
    deadline: Instant,
) -> Option<DirectKillFailure> {
    loop {
        match child.kill() {
            Ok(()) => return None,
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) => {
                let exited_race = matches!(
                    error.kind(),
                    std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
                ) || {
                    #[cfg(unix)]
                    {
                        error.raw_os_error() == Some(libc::ESRCH)
                    }
                    #[cfg(not(unix))]
                    {
                        false
                    }
                };
                return Some(DirectKillFailure {
                    message: error.to_string(),
                    exited_race,
                });
            }
        }
    }
}

fn reap_child_until(
    child: &mut std::process::Child,
    deadline: Instant,
) -> Result<std::process::ExitStatus, String> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                return Err("reap exceeded the cleanup deadline".to_owned());
            }
            Ok(None) => sleep_until(deadline, CHILD_POLL_INTERVAL),
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) => return Err(format!("reap poll failed: {error}")),
        }
    }
}

struct ChildCleanup {
    status: Option<std::process::ExitStatus>,
    issues: Vec<String>,
}

fn cleanup_child_until(
    child: &mut std::process::Child,
    initial_status: Option<std::process::ExitStatus>,
    deadline: Instant,
) -> ChildCleanup {
    let group_error = kill_process_group_until(child, deadline);
    let direct_error = kill_direct_until(child, deadline);
    let mut issues = Vec::new();
    let status = match initial_status {
        Some(status) => Some(status),
        None => match reap_child_until(child, deadline) {
            Ok(status) => Some(status),
            Err(error) => {
                issues.push(error);
                None
            }
        },
    };
    if let Some(error) = group_error {
        issues.push(format!("process-group kill failed: {error}"));
    }
    if let Some(error) = direct_error
        && !(error.exited_race && status.is_some())
    {
        issues.push(format!("direct kill failed: {}", error.message));
    }
    ChildCleanup { status, issues }
}

enum ChildPrimary {
    Exited(std::process::ExitStatus),
    TimedOut,
    PollFailed(String),
}

fn poll_child(child: &mut std::process::Child, timeout: Duration) -> ChildPrimary {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return ChildPrimary::Exited(status),
            Ok(None) if Instant::now() >= deadline => return ChildPrimary::TimedOut,
            Ok(None) => sleep_until(deadline, CHILD_POLL_INTERVAL),
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) => return ChildPrimary::PollFailed(error.to_string()),
        }
    }
}

fn drain_text(drains: &[PipeDrain], label: &str) -> String {
    drains
        .iter()
        .find(|drain| drain.label == label)
        .map(|drain| String::from_utf8_lossy(&drain.capture.bytes).into_owned())
        .unwrap_or_default()
}

fn cleanup_setup_failure(
    child: &mut std::process::Child,
    drains: &mut [PipeDrain],
    cancel: &AtomicBool,
    command_label: &str,
    setup_error: &str,
) -> ! {
    let cleanup_deadline = Instant::now() + PROCESS_CLEANUP_TIMEOUT;
    let mut cleanup = cleanup_child_until(child, None, cleanup_deadline);
    cleanup
        .issues
        .extend(finish_pipe_drains(drains, cancel, cleanup_deadline));
    panic!(
        "{command_label}: {setup_error}; cleanup: {}; stdout: {}; stderr: {}",
        cleanup.issues.join("; "),
        drain_text(drains, "stdout"),
        drain_text(drains, "stderr"),
    );
}

fn run_command_bounded(command: &mut Command, timeout: Duration, command_label: &str) -> Output {
    isolate_process_group(command);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut drains = Vec::with_capacity(2);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("cannot spawn {command_label}: {error}"));
    let (stdout, stderr) = match (child.stdout.take(), child.stderr.take()) {
        (Some(stdout), Some(stderr)) => (stdout, stderr),
        (stdout, stderr) => {
            drop(stdout);
            drop(stderr);
            cleanup_setup_failure(
                &mut child,
                &mut drains,
                cancel.as_ref(),
                command_label,
                "configured pipe missing",
            );
        }
    };
    let setup_deadline = Instant::now() + PIPE_SETUP_TIMEOUT;
    match start_pipe_drain(stdout, Arc::clone(&cancel), setup_deadline, "stdout") {
        Ok(drain) => drains.push(drain),
        Err(error) => {
            drop(stderr);
            cleanup_setup_failure(
                &mut child,
                &mut drains,
                cancel.as_ref(),
                command_label,
                &error,
            );
        }
    }
    match start_pipe_drain(stderr, Arc::clone(&cancel), setup_deadline, "stderr") {
        Ok(drain) => drains.push(drain),
        Err(error) => cleanup_setup_failure(
            &mut child,
            &mut drains,
            cancel.as_ref(),
            command_label,
            &error,
        ),
    }

    let primary = poll_child(&mut child, timeout);
    let (initial_status, failure) = match primary {
        ChildPrimary::Exited(status) => (Some(status), None),
        ChildPrimary::TimedOut => (None, Some(format!("exceeded its {timeout:?} deadline"))),
        ChildPrimary::PollFailed(error) => (None, Some(format!("poll failed: {error}"))),
    };
    let cleanup_deadline = Instant::now() + PROCESS_CLEANUP_TIMEOUT;
    let mut cleanup = cleanup_child_until(&mut child, initial_status, cleanup_deadline);
    cleanup.issues.extend(finish_pipe_drains(
        &mut drains,
        cancel.as_ref(),
        cleanup_deadline,
    ));
    if cleanup.status.is_none() {
        cleanup
            .issues
            .push("child has no exit status after cleanup".to_owned());
    }
    let stdout = drain_text(&drains, "stdout");
    let stderr = drain_text(&drains, "stderr");
    if let Some(failure) = failure {
        panic!(
            "{command_label} {failure}; cleanup: {}; stdout: {stdout}; stderr: {stderr}",
            cleanup.issues.join("; "),
        );
    }
    assert!(
        cleanup.issues.is_empty(),
        "{command_label}: cleanup failed: {}; stdout: {stdout}; stderr: {stderr}",
        cleanup.issues.join("; "),
    );
    Output {
        status: cleanup
            .status
            .expect("successful bounded cleanup has an exit status"),
        stdout: drains[0].capture.bytes.clone(),
        stderr: drains[1].capture.bytes.clone(),
    }
}

fn cc_available_bounded() -> bool {
    let mut command = Command::new("cc");
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    isolate_process_group(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(error) => panic!("cannot spawn timeout ownership C compiler probe: {error}"),
    };
    let primary = poll_child(&mut child, CC_PROBE_TIMEOUT);
    let (initial_status, failure) = match primary {
        ChildPrimary::Exited(status) => (Some(status), None),
        ChildPrimary::TimedOut => (
            None,
            Some(format!("exceeded its {CC_PROBE_TIMEOUT:?} deadline")),
        ),
        ChildPrimary::PollFailed(error) => (None, Some(format!("poll failed: {error}"))),
    };
    let cleanup_deadline = Instant::now() + PROCESS_CLEANUP_TIMEOUT;
    let cleanup = cleanup_child_until(&mut child, initial_status, cleanup_deadline);
    if failure.is_some() || !cleanup.issues.is_empty() || cleanup.status.is_none() {
        panic!(
            "timeout ownership C compiler probe {}; cleanup: {}",
            failure.unwrap_or_else(|| "cleanup failed".to_owned()),
            cleanup.issues.join("; "),
        );
    }
    let status = cleanup
        .status
        .expect("successful bounded compiler probe cleanup has an exit status");
    assert!(
        status.success(),
        "timeout ownership C compiler probe failed as {status}",
    );
    true
}

fn link_env_count(name: &str) -> usize {
    std::env::var(name)
        .unwrap_or_else(|error| panic!("read timeout ownership link-child `{name}`: {error}"))
        .parse()
        .unwrap_or_else(|error| panic!("parse timeout ownership link-child `{name}`: {error}"))
}

#[test]
fn pkg_kv_timeout_ownership_link_child() {
    if std::env::var_os(LINK_CHILD_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let object_count = link_env_count(LINK_OBJECT_COUNT_ENV);
    let objects = (0..object_count)
        .map(|index| {
            PathBuf::from(
                std::env::var_os(format!("{LINK_CHILD_ENV}_OBJECT_{index}")).unwrap_or_else(|| {
                    panic!("missing timeout ownership link-child object {index}")
                }),
            )
        })
        .collect::<Vec<_>>();
    let object_refs = objects.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let library_count = link_env_count(LINK_LIBRARY_COUNT_ENV);
    let libraries = (0..library_count)
        .map(|index| {
            std::env::var(format!("{LINK_CHILD_ENV}_LIBRARY_{index}")).unwrap_or_else(|error| {
                panic!("read timeout ownership link-child library {index}: {error}")
            })
        })
        .collect::<Vec<_>>();
    let executable = PathBuf::from(
        std::env::var_os(LINK_EXE_ENV).expect("missing timeout ownership link-child executable"),
    );
    link_objects(&object_refs, &executable, &libraries, Profile::Release)
        .expect("link timeout ownership executable");
}

fn link_objects_bounded(objects: &[PathBuf], executable: &Path, libraries: &[String]) {
    let mut command =
        Command::new(std::env::current_exe().expect("resolve timeout ownership test executable"));
    command
        .args([
            "--exact",
            "pkg_kv_timeout_ownership_link_child",
            "--nocapture",
        ])
        .env(LINK_CHILD_ENV, "1")
        .env(LINK_EXE_ENV, executable)
        .env(LINK_OBJECT_COUNT_ENV, objects.len().to_string())
        .env(LINK_LIBRARY_COUNT_ENV, libraries.len().to_string());
    for (index, object) in objects.iter().enumerate() {
        command.env(format!("{LINK_CHILD_ENV}_OBJECT_{index}"), object);
    }
    for (index, library) in libraries.iter().enumerate() {
        command.env(format!("{LINK_CHILD_ENV}_LIBRARY_{index}"), library);
    }
    let linked = run_command_bounded(
        &mut command,
        PROCESS_TIMEOUT,
        "link timeout ownership executable",
    );
    assert!(
        linked.status.success(),
        "timeout ownership executable link failed as {}; stdout: {}; stderr: {}",
        linked.status,
        String::from_utf8_lossy(&linked.stdout),
        String::from_utf8_lossy(&linked.stderr),
    );
}

fn build_lifecycle_executable(name: &str) -> (PathBuf, TempArtifacts) {
    let lifecycle_source = lifecycle_source();
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, name, &lifecycle_source);
    assert!(
        !checked.diags.has_errors(),
        "lifecycle source rejected:\n{}",
        align_driver::format_diagnostics(&sources, &checked.diags),
    );
    assert!(
        !checked.hir.fns.is_empty(),
        "lifecycle checking produced no functions; externs: {:?}",
        checked
            .hir
            .externs
            .iter()
            .map(|function| function.name.as_str())
            .collect::<Vec<_>>(),
    );
    let mir = align_driver::try_lower_to_mir(&checked.hir)
        .unwrap_or_else(|error| panic!("checked lifecycle HIR failed MIR lowering: {error}"));
    let llvm = emit_llvm_ir(&mir, BuildTarget::Baseline, false, &[], false)
        .expect("emit timeout ownership LLVM");
    assert!(
        llvm.contains("call i32 @align_rt_tcp_conn_set_io_timeout(ptr"),
        "the generated compatible caller must reach the exact source ABI row",
    );
    let main_definition = llvm.lines().find(|line| {
        line.starts_with("define ") && (line.contains(" @main(") || line.contains(" @\"main\"("))
    });
    assert!(
        main_definition.is_some(),
        "lifecycle program omitted its native main entry; first definition:\n{}",
        llvm.find("define ").map_or_else(
            || format!(
                "<none>; MIR functions: {:?}",
                mir.fns
                    .iter()
                    .map(|function| &function.name)
                    .collect::<Vec<_>>()
            ),
            |start| llvm[start..llvm.len().min(start + 2_000)].to_owned(),
        ),
    );
    let dir = std::env::temp_dir();
    let nonce = ARTIFACT_NONCE.fetch_add(1, Ordering::Relaxed);
    let stem = format!(
        "align-pkg-kv-timeout-owner-{}-{nonce}-{name}",
        std::process::id(),
    );
    let align_object = dir.join(format!("{stem}.o"));
    let c_source = dir.join(format!("{stem}.c"));
    let c_object = dir.join(format!("{stem}-fixture.o"));
    let executable = dir.join(format!("{stem}{}", std::env::consts::EXE_SUFFIX));
    let artifacts = TempArtifacts(vec![
        align_object.clone(),
        c_source.clone(),
        c_object.clone(),
        executable.clone(),
    ]);

    emit_object_file(
        &mir,
        &align_object,
        BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
    )
    .expect("emit timeout ownership object");

    let objcopy = align_driver::llvm_tool("llvm-objcopy")
        .expect("the version-matched LLVM installation must provide llvm-objcopy");
    let mut rename = Command::new(objcopy);
    // This rewrite is the safety boundary for excluded probes: source and emitted LLVM must call
    // the exact production ABI name, but the executable redirects it to the rejecting oracle
    // below. A nonzero classified count therefore exercises call admission/status wiring without
    // ever entering the real row whose live-shell precondition it deliberately violates.
    for (original, replacement) in RENAMED_RUNTIME_SYMBOLS {
        rename.arg(format!("--redefine-sym={original}={replacement}"));
        // Mach-O symbol tables spell C externals with the platform underscore. Passing both forms
        // is harmless on ELF and keeps the fixture target-independent.
        rename.arg(format!("--redefine-sym=_{original}=_{replacement}"));
    }
    rename.arg(&align_object);
    let renamed = run_command_bounded(
        &mut rename,
        PROCESS_TIMEOUT,
        "timeout ownership llvm-objcopy",
    );
    assert!(
        renamed.status.success(),
        "renaming runtime fixture symbols failed: {}",
        String::from_utf8_lossy(&renamed.stderr),
    );

    std::fs::write(&c_source, LIFECYCLE_C).expect("write timeout ownership C fixture");
    let mut cc = Command::new("cc");
    cc.args(["-std=c11", "-c", "-O0"])
        .arg(&c_source)
        .arg("-o")
        .arg(&c_object);
    let compiled = run_command_bounded(&mut cc, PROCESS_TIMEOUT, "timeout ownership C compiler");
    assert!(
        compiled.status.success(),
        "C fixture compilation failed: {}",
        String::from_utf8_lossy(&compiled.stderr),
    );
    link_objects_bounded(
        &[align_object.clone(), c_object.clone()],
        &executable,
        &mir.link_libs,
    );
    (executable, artifacts)
}

#[test]
fn generated_cleanup_excludes_nonzero_retainers_then_reconfigures() {
    if !backend_available() || !cc_available_bounded() {
        return;
    }
    let (executable, _artifacts) = build_lifecycle_executable("lifecycle");
    let mut command = Command::new(&executable);
    let output = run_command_bounded(
        &mut command,
        Duration::from_secs(10),
        "timeout ownership executable",
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
