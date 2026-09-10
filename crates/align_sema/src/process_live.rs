//! Producer-owned schemas for native process observations.
use crate::{IntTy, Scalar, Ty, hir};
pub const COPY_NAMES: &[&str] = &[
    "fs.memory_kind",
    "process.termination",
    "process.wait_result",
    "process.readiness",
    "process.signal",
    "process.signal_set",
    "process.snapshot",
];
pub fn termination_definition() -> hir::EnumDef {
    hir::EnumDef {
        name: "process.termination".into(),
        source_name: "process.termination".into(),
        variants: ["Exited", "Signaled"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| hir::EnumVariant {
                name: name.into(),
                payload: vec![Scalar::Int(IntTy {
                    bits: 64,
                    signed: true,
                })],
                field_base: if index == 0 { 1 } else { 2 },
            })
            .collect(),
    }
}
pub fn memory_kind_definition() -> hir::EnumDef {
    hir::EnumDef {
        name: "fs.memory_kind".into(),
        source_name: "fs.memory_kind".into(),
        variants: ["Data", "Executable"]
            .into_iter()
            .map(|name| hir::EnumVariant {
                name: name.into(),
                payload: Vec::new(),
                field_base: 1,
            })
            .collect(),
    }
}
pub fn signal_definition() -> hir::EnumDef {
    hir::EnumDef {
        name: "process.signal".into(),
        source_name: "process.signal".into(),
        variants: ["Hangup", "Interrupt", "Quit", "Terminate", "Kill"]
            .into_iter()
            .map(|name| hir::EnumVariant {
                name: name.into(),
                payload: Vec::new(),
                field_base: 1,
            })
            .collect(),
    }
}
pub fn record_definitions(termination: u32) -> Vec<hir::StructDef> {
    let integer = Ty::Int(IntTy {
        bits: 64,
        signed: true,
    });
    let optional = Ty::Option(Scalar::Int(IntTy {
        bits: 64,
        signed: true,
    }));
    [
        (
            "process.wait_result",
            vec![
                ("termination", Ty::Enum(termination)),
                ("max_rss_bytes", optional),
            ],
        ),
        (
            "process.readiness",
            vec![
                ("stdout", Ty::Bool),
                ("stderr", Ty::Bool),
                ("status", Ty::Bool),
            ],
        ),
        (
            "process.signal_set",
            vec![
                ("hangup", Ty::Bool),
                ("interrupt", Ty::Bool),
                ("quit", Ty::Bool),
                ("terminate", Ty::Bool),
            ],
        ),
        (
            "process.snapshot",
            vec![
                ("pid", integer),
                ("parent_pid", integer),
                ("rss_bytes", optional),
                ("cpu_ns", optional),
                ("threads", optional),
            ],
        ),
    ]
    .into_iter()
    .map(|(name, fields)| hir::StructDef {
        name: name.into(),
        source_name: name.into(),
        fields: fields
            .into_iter()
            .map(|(name, ty)| hir::FieldDef {
                name: name.into(),
                ty,
            })
            .collect(),
        align: None,
        c_repr: false,
    })
    .collect()
}
pub fn schemas_valid(structs: &[hir::StructDef], enums: &[hir::EnumDef]) -> bool {
    let mut termination_id = None;
    for expected in [
        termination_definition(),
        signal_definition(),
        memory_kind_definition(),
    ] {
        let mut found = false;
        for (id, actual) in enums.iter().enumerate() {
            if actual.name == expected.name || actual.source_name == expected.source_name {
                if found
                    || actual.name != expected.name
                    || actual.source_name != expected.source_name
                    || actual.variants.len() != expected.variants.len()
                    || !actual
                        .variants
                        .iter()
                        .zip(&expected.variants)
                        .all(|(a, b)| {
                            a.name == b.name
                                && a.payload == b.payload
                                && a.field_base == b.field_base
                        })
                {
                    return false;
                }
                found = true;
                if expected.name == "process.termination" {
                    termination_id = u32::try_from(id).ok();
                }
            }
        }
    }
    for expected in record_definitions(termination_id.unwrap_or(0)) {
        let mut found = false;
        for actual in structs {
            if actual.name == expected.name || actual.source_name == expected.source_name {
                if found
                    || (expected.name == "process.wait_result" && termination_id.is_none())
                    || actual.name != expected.name
                    || actual.source_name != expected.source_name
                    || actual.align.is_some()
                    || actual.c_repr
                    || actual.fields.len() != expected.fields.len()
                    || !actual
                        .fields
                        .iter()
                        .zip(&expected.fields)
                        .all(|(a, b)| a.name == b.name && a.ty == b.ty)
                {
                    return false;
                }
                found = true;
            }
        }
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub enum ProcessLiveKind {
    CommandNewSession,
    CommandStdoutTo,
    CommandStderrTo,
    CommandStart,
    ChildId,
    ChildStatus,
    ChildTryWait,
    ChildReadStdout,
    ChildReadStderr,
    ChildPoll,
    ChildKillGroup,
    ChildGroupMembers,
    RunOutputStatus,
    RunBytesStatus,
    SignalNumber,
    ProcessTable,
    SignalNew,
    SignalNext,
    SignalClose,
    MemoryNew,
    MemoryWrite,
    MemorySeal,
    SealedLen,
    SealedReadAt,
    Executable,
    ImageLen,
    ImageReadAt,
    CommandImage,
    CurrentImage,
    UserNamespace,
    InheritFile,
    InheritNamespace,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Owner(Ty),
    Integer,
    Bool,
    OutBytes,
    Readiness,
    Signal,
    SignalSet,
    MemoryKind,
    Bytes,
    Argv,
    Text,
}
impl ProcessLiveKind {
    pub fn local_receiver(self) -> bool {
        !matches!(self, Self::Executable | Self::CommandImage)
    }
    pub fn consumes(self, index: usize) -> bool {
        self == Self::MemorySeal && index == 0
    }
    pub fn module(self) -> &'static str {
        match self {
            Self::MemoryNew
            | Self::MemoryWrite
            | Self::MemorySeal
            | Self::SealedLen
            | Self::SealedReadAt => "std.fs",
            _ => "std.process",
        }
    }

    pub fn inputs(self) -> &'static [Input] {
        use Input::*;
        match self {
            Self::CommandNewSession => &[Owner(Ty::Command), Bool],
            Self::CommandStdoutTo | Self::CommandStderrTo => {
                &[Owner(Ty::Command), Owner(Ty::Writer)]
            }
            Self::CommandStart => &[Owner(Ty::Command)],
            Self::ChildId | Self::ChildStatus | Self::ChildTryWait => &[Owner(Ty::Child)],
            Self::ChildReadStdout | Self::ChildReadStderr => &[Owner(Ty::Child), OutBytes],
            Self::ChildPoll => &[Owner(Ty::Child), Readiness, Integer],
            Self::ChildKillGroup | Self::ChildGroupMembers => &[Owner(Ty::Child), Integer],
            Self::RunOutputStatus => &[Owner(Ty::RunOutput)],
            Self::RunBytesStatus => &[Owner(Ty::RunBytes)],
            Self::SignalNumber => &[Signal],
            Self::ProcessTable => &[Integer],
            Self::SignalNew => &[SignalSet],
            Self::MemoryNew => &[MemoryKind, Integer],
            Self::MemoryWrite => &[Owner(Ty::FsMemoryWriter), Bytes],
            Self::MemorySeal => &[Owner(Ty::FsMemoryWriter)],
            Self::SealedLen | Self::Executable => &[Owner(Ty::FsSealedFile)],
            Self::SealedReadAt => &[Owner(Ty::FsSealedFile), Integer, OutBytes],
            Self::ImageLen => &[Owner(Ty::ProcessImage)],
            Self::ImageReadAt => &[Owner(Ty::ProcessImage), Integer, OutBytes],
            Self::CommandImage => &[Owner(Ty::ProcessImage), Argv],
            Self::CurrentImage => &[],
            Self::UserNamespace => &[Text],
            Self::InheritFile => &[Owner(Ty::Command), Owner(Ty::FsSealedFile), Integer],
            Self::InheritNamespace => {
                &[Owner(Ty::Command), Owner(Ty::ProcessUserNamespace), Integer]
            }
            Self::SignalNext | Self::SignalClose => &[Owner(Ty::ProcessSignalSubscription)],
        }
    }
    pub fn pure(self) -> bool {
        matches!(
            self,
            Self::ChildId
                | Self::RunOutputStatus
                | Self::RunBytesStatus
                | Self::SignalNumber
                | Self::SealedLen
                | Self::ImageLen
        )
    }
    pub fn fallible(self) -> bool {
        !self.pure() && self != Self::CommandNewSession
    }
    pub fn exclusive(self) -> bool {
        matches!(
            self,
            Self::CommandNewSession
                | Self::CommandStdoutTo
                | Self::CommandStderrTo
                | Self::ChildStatus
                | Self::ChildTryWait
                | Self::ChildReadStdout
                | Self::ChildReadStderr
                | Self::ChildPoll
                | Self::MemoryWrite
                | Self::InheritFile
                | Self::InheritNamespace
                | Self::SignalNext
                | Self::SignalClose
        )
    }
    pub fn scratch(self) -> bool {
        !matches!(
            self,
            Self::CommandNewSession
                | Self::CommandStdoutTo
                | Self::CommandStderrTo
                | Self::ChildId
                | Self::ChildKillGroup
                | Self::SignalNumber
                | Self::SignalClose
                | Self::MemoryWrite
                | Self::InheritFile
                | Self::InheritNamespace
                | Self::SealedLen
                | Self::ImageLen
        )
    }
    pub fn from_method(receiver: Ty, name: &str) -> Option<Self> {
        Some(match (receiver, name) {
            (Ty::Command, "new_session") => Self::CommandNewSession,
            (Ty::Command, "stdout_to") => Self::CommandStdoutTo,
            (Ty::Command, "stderr_to") => Self::CommandStderrTo,
            (Ty::Command, "start") => Self::CommandStart,
            (Ty::Child, "id") => Self::ChildId,
            (Ty::Child, "status") => Self::ChildStatus,
            (Ty::Child, "try_wait") => Self::ChildTryWait,
            (Ty::Child, "read_stdout") => Self::ChildReadStdout,
            (Ty::Child, "read_stderr") => Self::ChildReadStderr,
            (Ty::Child, "poll") => Self::ChildPoll,
            (Ty::Child, "kill_group") => Self::ChildKillGroup,
            (Ty::Child, "group_members") => Self::ChildGroupMembers,
            (Ty::RunOutput, "status") => Self::RunOutputStatus,
            (Ty::RunBytes, "status") => Self::RunBytesStatus,
            (Ty::ProcessSignalSubscription, "next") => Self::SignalNext,
            (Ty::ProcessSignalSubscription, "close") => Self::SignalClose,
            (Ty::FsMemoryWriter, "write") => Self::MemoryWrite,
            (Ty::FsMemoryWriter, "seal") => Self::MemorySeal,
            (Ty::FsSealedFile, "len") => Self::SealedLen,
            (Ty::FsSealedFile, "read_at") => Self::SealedReadAt,
            (Ty::ProcessImage, "len") => Self::ImageLen,
            (Ty::ProcessImage, "read_at") => Self::ImageReadAt,
            (Ty::Command, "inherit_file") => Self::InheritFile,
            (Ty::Command, "inherit_namespace") => Self::InheritNamespace,
            _ => return None,
        })
    }
}
pub fn input_type(input: Input, structs: &[hir::StructDef], enums: &[hir::EnumDef]) -> Option<Ty> {
    Some(match input {
        Input::Owner(ty) => ty,
        Input::Integer => Ty::Int(IntTy {
            bits: 64,
            signed: true,
        }),
        Input::Bool => Ty::Bool,
        Input::Text | Input::Bytes => Ty::Str,
        Input::Argv => Ty::Slice(Scalar::Str),
        Input::MemoryKind => {
            Ty::Enum(u32::try_from(enums.iter().position(|e| e.name == "fs.memory_kind")?).ok()?)
        }
        Input::OutBytes => Ty::Slice(Scalar::Int(IntTy {
            bits: 8,
            signed: false,
        })),
        Input::Readiness => Ty::Struct(crate::fs_tree::record_id(structs, "process.readiness")?),
        Input::SignalSet => Ty::Struct(crate::fs_tree::record_id(structs, "process.signal_set")?),
        Input::Signal => {
            Ty::Enum(u32::try_from(enums.iter().position(|e| e.name == "process.signal")?).ok()?)
        }
    })
}
pub fn payload_type(
    kind: ProcessLiveKind,
    structs: &[hir::StructDef],
    enums: &[hir::EnumDef],
) -> Option<Ty> {
    use ProcessLiveKind::*;
    let integer = Scalar::Int(IntTy {
        bits: 64,
        signed: true,
    });
    Some(match kind {
        CommandNewSession | CommandStdoutTo | CommandStderrTo | ChildKillGroup => Ty::Unit,
        CommandStart => Ty::Child,
        SignalNew => Ty::ProcessSignalSubscription,
        MemoryNew => Ty::FsMemoryWriter,
        MemoryWrite | InheritFile | InheritNamespace => Ty::Unit,
        MemorySeal => Ty::FsSealedFile,
        Executable => Ty::ProcessImage,
        CommandImage => Ty::Command,
        CurrentImage => Ty::Reader,
        UserNamespace => Ty::ProcessUserNamespace,
        SealedLen | ImageLen | SealedReadAt | ImageReadAt => Ty::Int(IntTy {
            bits: 64,
            signed: true,
        }),
        SignalClose => Ty::Unit,
        SignalNext => Ty::Option(Scalar::Enum(
            u32::try_from(enums.iter().position(|e| e.name == "process.signal")?).ok()?,
        )),
        ChildId | SignalNumber => Ty::Int(IntTy {
            bits: 64,
            signed: true,
        }),
        ChildStatus => Ty::Option(Scalar::Enum(
            u32::try_from(enums.iter().position(|e| e.name == "process.termination")?).ok()?,
        )),
        ChildTryWait => Ty::Option(Scalar::Struct(crate::fs_tree::record_id(
            structs,
            "process.wait_result",
        )?)),
        ChildReadStdout | ChildReadStderr => Ty::Option(integer),
        ChildPoll => Ty::Struct(crate::fs_tree::record_id(structs, "process.readiness")?),
        ChildGroupMembers => Ty::DynArray(integer),
        ProcessTable => Ty::DynStructArray(
            crate::fs_tree::record_id(structs, "process.snapshot")?,
            crate::Layout::Aos,
        ),
        RunOutputStatus | RunBytesStatus => {
            Ty::Struct(crate::fs_tree::record_id(structs, "process.wait_result")?)
        }
    })
}

pub fn result_type(
    kind: ProcessLiveKind,
    structs: &[hir::StructDef],
    enums: &[hir::EnumDef],
    tagged: &[hir::TaggedType],
) -> Option<Ty> {
    let payload = payload_type(kind, structs, enums)?;
    if !kind.fallible() {
        return Some(payload);
    }
    let scalar = match payload {
        Ty::Option(inner) => Scalar::Tagged(
            u32::try_from(
                tagged
                    .iter()
                    .position(|t| *t == hir::TaggedType::Option(inner))?,
            )
            .ok()?,
        ),
        _ => crate::ty_to_scalar(payload)?,
    };
    let error = u32::try_from(enums.iter().position(|e| e.name == "Error")?).ok()?;
    Some(Ty::Result(scalar, Scalar::Enum(error)))
}

// Variable byte/argv view forms share one native descriptor layout.
pub fn view_input_matches(input: Input, ty: Ty) -> Option<bool> {
    match input {
        Input::Bytes => Some(
            ty == Ty::Str
                || ty
                    == Ty::Slice(Scalar::Int(IntTy {
                        bits: 8,
                        signed: false,
                    })),
        ),
        Input::Argv => Some(matches!(
            ty,
            Ty::Slice(Scalar::Str) | Ty::DynArray(Scalar::Str)
        )),
        _ => None,
    }
}
