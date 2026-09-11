//! Closed operation signatures for retained filesystem boundaries.
use crate::{IntTy, Scalar, Ty, hir};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub enum FsTreeKind {
    DirectoryOpen,
    DirectoryCursor,
    CursorNext,
    DirectoryMetadata,
    DirectoryMetadataAt,
    DirectoryOpenDir,
    DirectoryOpenRead,
    DirectoryOpenReadSingleLink,
    DirectoryCreateNew,
    DirectoryCreateDir,
    DirectoryRemoveFile,
    DirectoryRemoveDir,
    DirectorySetMode,
    ReaderMetadata,
    WriterMetadata,
    FileMetadata,
    ReaderSetMode,
    WriterSetMode,
    FileSetMode,
    DirectoryReadLink,
    DirectoryMetadataFollow,
    DirectoryAccess,
    DirectoryAccessAt,
    DirectoryCreateSymlink,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Owner(Ty),
    Text,
    Bytes,
    Mode,
    Bound,
    AccessMode,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Directory,
    Cursor,
    Reader,
    Writer,
    Metadata,
    EntryOption,
    Unit,
    OwnedBytes,
    Bool,
}

impl FsTreeKind {
    pub const ALL: [Self; 24] = [
        Self::DirectoryOpen,
        Self::DirectoryCursor,
        Self::CursorNext,
        Self::DirectoryMetadata,
        Self::DirectoryMetadataAt,
        Self::DirectoryOpenDir,
        Self::DirectoryOpenRead,
        Self::DirectoryOpenReadSingleLink,
        Self::DirectoryCreateNew,
        Self::DirectoryCreateDir,
        Self::DirectoryRemoveFile,
        Self::DirectoryRemoveDir,
        Self::DirectorySetMode,
        Self::ReaderMetadata,
        Self::WriterMetadata,
        Self::FileMetadata,
        Self::ReaderSetMode,
        Self::WriterSetMode,
        Self::FileSetMode,
        Self::DirectoryReadLink,
        Self::DirectoryMetadataFollow,
        Self::DirectoryAccess,
        Self::DirectoryAccessAt,
        Self::DirectoryCreateSymlink,
    ];
    pub fn inputs(self) -> &'static [Input] {
        use Input::*;
        match self {
            Self::DirectoryReadLink => &[Owner(Ty::FsDirectory), Bytes, Bound],
            Self::DirectoryMetadataFollow => &[Owner(Ty::FsDirectory), Bytes],
            Self::DirectoryAccess => &[Owner(Ty::FsDirectory), AccessMode],
            Self::DirectoryAccessAt => &[Owner(Ty::FsDirectory), Bytes, AccessMode],
            Self::DirectoryCreateSymlink => &[Owner(Ty::FsDirectory), Bytes, Bytes],
            Self::DirectoryOpen => &[Text],
            Self::DirectoryCursor | Self::DirectoryMetadata => &[Owner(Ty::FsDirectory)],
            Self::CursorNext => &[Owner(Ty::FsDirCursor)],
            Self::DirectoryMetadataAt
            | Self::DirectoryOpenDir
            | Self::DirectoryOpenRead
            | Self::DirectoryOpenReadSingleLink
            | Self::DirectoryCreateNew
            | Self::DirectoryRemoveFile
            | Self::DirectoryRemoveDir => &[Owner(Ty::FsDirectory), Bytes],
            Self::DirectoryCreateDir => &[Owner(Ty::FsDirectory), Bytes, Mode],
            Self::DirectorySetMode => &[Owner(Ty::FsDirectory), Mode],
            Self::ReaderMetadata => &[Owner(Ty::Reader)],
            Self::WriterMetadata => &[Owner(Ty::Writer)],
            Self::FileMetadata => &[Owner(Ty::File)],
            Self::ReaderSetMode => &[Owner(Ty::Reader), Mode],
            Self::WriterSetMode => &[Owner(Ty::Writer), Mode],
            Self::FileSetMode => &[Owner(Ty::File), Mode],
        }
    }
    pub fn output(self) -> Output {
        match self {
            Self::DirectoryReadLink => Output::OwnedBytes,
            Self::DirectoryMetadataFollow => Output::Metadata,
            Self::DirectoryAccess | Self::DirectoryAccessAt => Output::Bool,
            Self::DirectoryCreateSymlink => Output::Unit,
            Self::DirectoryOpen | Self::DirectoryOpenDir => Output::Directory,
            Self::DirectoryCursor => Output::Cursor,
            Self::CursorNext => Output::EntryOption,
            Self::DirectoryOpenRead | Self::DirectoryOpenReadSingleLink => Output::Reader,
            Self::DirectoryCreateNew => Output::Writer,
            Self::DirectoryMetadata
            | Self::DirectoryMetadataAt
            | Self::ReaderMetadata
            | Self::WriterMetadata
            | Self::FileMetadata => Output::Metadata,
            Self::DirectoryCreateDir
            | Self::DirectoryRemoveFile
            | Self::DirectoryRemoveDir
            | Self::DirectorySetMode
            | Self::ReaderSetMode
            | Self::WriterSetMode
            | Self::FileSetMode => Output::Unit,
        }
    }
    pub fn from_method(receiver: Ty, name: &str) -> Option<Self> {
        Some(match (receiver, name) {
            (Ty::FsDirectory, "read_link") => Self::DirectoryReadLink,
            (Ty::FsDirectory, "metadata_follow") => Self::DirectoryMetadataFollow,
            (Ty::FsDirectory, "access") => Self::DirectoryAccess,
            (Ty::FsDirectory, "access_at") => Self::DirectoryAccessAt,
            (Ty::FsDirectory, "create_symlink") => Self::DirectoryCreateSymlink,
            (Ty::FsDirectory, "cursor") => Self::DirectoryCursor,
            (Ty::FsDirectory, "metadata") => Self::DirectoryMetadata,
            (Ty::FsDirectory, "metadata_at") => Self::DirectoryMetadataAt,
            (Ty::FsDirectory, "open_dir") => Self::DirectoryOpenDir,
            (Ty::FsDirectory, "open_read") => Self::DirectoryOpenRead,
            (Ty::FsDirectory, "open_read_single_link") => Self::DirectoryOpenReadSingleLink,
            (Ty::FsDirectory, "create_new") => Self::DirectoryCreateNew,
            (Ty::FsDirectory, "create_dir") => Self::DirectoryCreateDir,
            (Ty::FsDirectory, "remove_file") => Self::DirectoryRemoveFile,
            (Ty::FsDirectory, "remove_dir") => Self::DirectoryRemoveDir,
            (Ty::FsDirectory, "set_mode") => Self::DirectorySetMode,
            (Ty::FsDirCursor, "next") => Self::CursorNext,
            (Ty::Reader, "metadata") => Self::ReaderMetadata,
            (Ty::Writer, "metadata") => Self::WriterMetadata,
            (Ty::File, "metadata") => Self::FileMetadata,
            (Ty::Reader, "set_mode") => Self::ReaderSetMode,
            (Ty::Writer, "set_mode") => Self::WriterSetMode,
            (Ty::File, "set_mode") => Self::FileSetMode,
            _ => return None,
        })
    }
    pub fn exclusive(self) -> bool {
        self == Self::CursorNext
    }
}

pub fn input_matches(input: Input, ty: Ty, structs: &[hir::StructDef]) -> bool {
    match input {
        Input::Bound => ty == Ty::Int(IntTy { bits: 64, signed: true }),
        Input::AccessMode => matches!(ty, Ty::Struct(id) if structs.get(id as usize).is_some_and(crate::fs_access_mode_schema_valid)),
        Input::Owner(expected) => ty == expected,
        Input::Text => ty == Ty::Str,
        Input::Bytes => matches!(
            ty,
            Ty::Str
                | Ty::Slice(Scalar::Int(IntTy {
                    bits: 8,
                    signed: false
                }))
        ),
        Input::Mode => {
            ty == Ty::Int(IntTy {
                bits: 32,
                signed: false,
            })
        }
    }
}

pub fn record_id(structs: &[hir::StructDef], name: &str) -> Option<u32> {
    u32::try_from(
        structs
            .iter()
            .position(|definition| definition.name == name)?,
    )
    .ok()
}

pub fn result_type(
    kind: FsTreeKind,
    structs: &[hir::StructDef],
    enums: &[hir::EnumDef],
    tagged: &[hir::TaggedType],
) -> Option<Ty> {
    let error = u32::try_from(
        enums
            .iter()
            .position(|definition| definition.name == "Error")?,
    )
    .ok()?;
    let ok = match kind.output() {
        Output::Directory => Scalar::FsDirectory,
        Output::Cursor => Scalar::FsDirCursor,
        Output::Reader => Scalar::Reader,
        Output::Writer => Scalar::Writer,
        Output::Metadata => Scalar::Struct(record_id(structs, "fs.metadata")?),
        Output::EntryOption => {
            let entry = record_id(structs, "fs.dir_entry")?;
            Scalar::Tagged(
                u32::try_from(tagged.iter().position(|definition| {
                    *definition == hir::TaggedType::Option(Scalar::Struct(entry))
                })?)
                .ok()?,
            )
        }
        Output::OwnedBytes => Scalar::DynArray(crate::PrimScalar::Int(IntTy { bits: 8, signed: false })),
        Output::Bool => Scalar::Bool,
        Output::Unit => Scalar::Unit,
    };
    Some(Ty::Result(ok, Scalar::Enum(error)))
}
