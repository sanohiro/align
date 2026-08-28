//! Deterministic compiler-registered static inputs (L5b).
//!
//! This module is the driver-side boundary between resolved static constructors and the L5a
//! artifact codec. It deliberately does not discover constructors or scan a directory. A future
//! frontend supplies the resolved descriptor identity, source literal, and source/import digest;
//! this module then resolves one exact file (or keeps decoded inline bytes), snapshots the exact
//! checked-metadata paths, and produces a fail-closed manifest/action digest.

use align_interface::{
    CheckedColumnMeta, CheckedParameterMeta, DecodedSpanEntry, Driver, DriverRestriction, Hash128,
    MetaNullability, MetaStatementClass, SqlSourceIdentity,
};
use align_sema::{
    StaticDescriptor, StaticDescriptorConsumer, StaticDescriptorDriver, StaticDescriptorSource,
};
use align_span::{FileId, SourceMap, Span};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const STATIC_INPUT_MANIFEST_FORMAT_VERSION: u32 = 1;
pub const STATIC_INPUT_MANIFEST_MAGIC: [u8; 8] = *b"ALIGNINP";
const MAX_FIELD_BYTES: usize = 16 * 1024 * 1024;
const MAX_SEQUENCE: usize = 1 << 16;
const METADATA_PUBLICATION_LOCK: &str = ".align-db/.publication.lock";

/// A cross-process metadata snapshot guard. Existing repositories without a lock file remain
/// readable, but the guard verifies that no first publisher appeared while that legacy snapshot
/// was being read. Once created, the lock file is stable operational state rather than a build
/// input; OS locks are released automatically if a compiler or preparation process exits.
pub(crate) struct MetadataPublicationLock {
    _file: Option<File>,
    absent_path: Option<PathBuf>,
}

impl MetadataPublicationLock {
    pub(crate) fn validate(&self) -> Result<(), StaticInputError> {
        let Some(path) = &self.absent_path else {
            return Ok(());
        };
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Ok(_) => Err(StaticInputError::Stale(
                "checked metadata publication overlapped static-input resolution".to_string(),
            )),
            Err(error) => Err(StaticInputError::Io {
                path: path.clone(),
                message: error.to_string(),
            }),
        }
    }
}

fn open_existing_publication_lock(
    path: &Path,
    write: bool,
) -> Result<File, StaticInputError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| StaticInputError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(StaticInputError::NotRegularFile(path.to_path_buf()));
    }
    OpenOptions::new()
        .read(true)
        .write(write)
        .open(path)
        .map_err(|error| StaticInputError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
}

pub(crate) fn lock_metadata_publication_shared(
    project_root: &Path,
) -> Result<MetadataPublicationLock, StaticInputError> {
    let path = project_root.join(METADATA_PUBLICATION_LOCK);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MetadataPublicationLock {
                _file: None,
                absent_path: Some(path),
            });
        }
        Err(error) => {
            return Err(StaticInputError::Io {
                path,
                message: error.to_string(),
            });
        }
        Ok(_) => {}
    }
    let file = open_existing_publication_lock(&path, false)?;
    file.lock_shared().map_err(|error| StaticInputError::Io {
        path: path.clone(),
        message: error.to_string(),
    })?;
    Ok(MetadataPublicationLock {
        _file: Some(file),
        absent_path: None,
    })
}

pub(crate) fn lock_metadata_publication_exclusive(
    project_root: &Path,
) -> Result<MetadataPublicationLock, StaticInputError> {
    let directory = project_root.join(".align-db");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(StaticInputError::InvalidPath(format!(
                "metadata root `{}` is not a real directory",
                directory.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::create_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata = fs::symlink_metadata(&directory).map_err(|error| {
                        StaticInputError::Io {
                            path: directory.clone(),
                            message: error.to_string(),
                        }
                    })?;
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(StaticInputError::InvalidPath(format!(
                            "metadata root `{}` is not a real directory",
                            directory.display()
                        )));
                    }
                }
                Err(error) => {
                    return Err(StaticInputError::Io {
                        path: directory.clone(),
                        message: error.to_string(),
                    });
                }
            }
        }
        Err(error) => {
            return Err(StaticInputError::Io {
                path: directory,
                message: error.to_string(),
            });
        }
    }
    let path = project_root.join(METADATA_PUBLICATION_LOCK);
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            open_existing_publication_lock(&path, true)?
        }
        Err(error) => {
            return Err(StaticInputError::Io {
                path,
                message: error.to_string(),
            });
        }
    };
    file.lock().map_err(|error| StaticInputError::Io {
        path: path.clone(),
        message: error.to_string(),
    })?;
    Ok(MetadataPublicationLock {
        _file: Some(file),
        absent_path: None,
    })
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StaticConsumerKind {
    Query = 0,
    Command = 1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataState {
    Missing,
    Present {
        content_hash: Hash128,
        format_version: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedMetadataInput {
    pub driver: Driver,
    pub logical_path: String,
    pub state: MetadataState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticInput {
    pub descriptor_id: String,
    pub source: SqlSourceIdentity,
    pub content_hash: Hash128,
    pub consumer_kind: StaticConsumerKind,
    pub driver_restriction: DriverRestriction,
    pub checked_metadata: Vec<CheckedMetadataInput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedStaticInput {
    pub input: StaticInput,
    pub bytes: Vec<u8>,
    pub decoded_span_map: Vec<DecodedSpanEntry>,
    pub source_map_file: Option<FileId>,
    pub resolved_path: Option<PathBuf>,
    pub(crate) checked_metadata_records: Vec<ParsedCheckedMetadata>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedCheckedMetadata {
    pub format_version: u32,
    pub metadata_digest: Hash128,
    pub driver: Driver,
    pub driver_restriction: DriverRestriction,
    pub statement_kind: MetadataStatementKind,
    pub statement_class: MetaStatementClass,
    pub source_identity: SqlSourceIdentity,
    pub source_sql_hash: Hash128,
    pub wire_sql_hash: Hash128,
    pub rewrite_format_version: u32,
    pub static_options_hash: Hash128,
    pub params_fingerprint: Hash128,
    pub row_fingerprint: Option<Hash128>,
    pub schema_fingerprint: Hash128,
    pub engine_version: String,
    pub driver_version: String,
    pub search_path: Vec<String>,
    pub extensions: Vec<ParsedMetadataExtension>,
    pub parameters: Vec<ParsedMetadataParameter>,
    pub columns: Vec<ParsedMetadataColumn>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedMetadataExtension {
    pub schema: String,
    pub name: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedMetadataParameter {
    pub source_name: String,
    pub logical_type: String,
    pub checked: CheckedParameterMeta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedMetadataColumn {
    pub source_alias: String,
    pub logical_type: String,
    pub checked: CheckedColumnMeta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedStaticInputs {
    pub inputs: Vec<ResolvedStaticInput>,
    pub manifest: StaticInputManifest,
}

#[derive(Debug)]
pub struct StaticDescriptorInputError {
    pub descriptor_id: String,
    pub span: Span,
    pub cause: StaticDescriptorInputErrorCause,
}

#[derive(Debug)]
pub enum StaticDescriptorInputErrorCause {
    InvalidDefiningFile,
    Input(StaticInputError),
}

impl std::fmt::Display for StaticDescriptorInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cannot resolve static descriptor `{}`: ",
            self.descriptor_id
        )?;
        match &self.cause {
            StaticDescriptorInputErrorCause::InvalidDefiningFile => {
                write!(f, "its defining source file is not present in SourceMap")
            }
            StaticDescriptorInputErrorCause::Input(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for StaticDescriptorInputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.cause {
            StaticDescriptorInputErrorCause::InvalidDefiningFile => None,
            StaticDescriptorInputErrorCause::Input(error) => Some(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticInputManifest {
    pub resolution_digest: Hash128,
    pub inputs: Vec<StaticInput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaticInputError {
    InvalidDescriptorId,
    InvalidSource(String),
    InvalidPath(String),
    RootNotDirectory(PathBuf),
    OutsideProjectRoot(PathBuf),
    MissingFile(PathBuf),
    NotRegularFile(PathBuf),
    Io { path: PathBuf, message: String },
    InvalidUtf8 { logical_path: String },
    EmbeddedNul { logical_path: String, offset: u32 },
    HashMismatch { logical_path: String },
    Stale(String),
    MetadataMalformed { logical_path: String },
    BadMagic,
    UnknownVersion(u32),
    Truncated,
    BadTag { what: &'static str, tag: u8 },
    BadUtf8,
    TrailingBytes,
    NonCanonical(String),
}

impl std::fmt::Display for StaticInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDescriptorId => write!(f, "static descriptor id is empty or contains NUL"),
            Self::InvalidSource(reason) => write!(f, "invalid static source: {reason}"),
            Self::InvalidPath(reason) => write!(f, "invalid static input path: {reason}"),
            Self::RootNotDirectory(path) => {
                write!(f, "project root is not a directory: {}", path.display())
            }
            Self::OutsideProjectRoot(path) => {
                write!(
                    f,
                    "static input escapes the project root: {}",
                    path.display()
                )
            }
            Self::MissingFile(path) => {
                write!(f, "static input file does not exist: {}", path.display())
            }
            Self::NotRegularFile(path) => {
                write!(f, "static input is not a regular file: {}", path.display())
            }
            Self::Io { path, message } => {
                write!(f, "cannot read static input {}: {message}", path.display())
            }
            Self::InvalidUtf8 { logical_path } => {
                write!(f, "static input is not UTF-8: {logical_path}")
            }
            Self::EmbeddedNul {
                logical_path,
                offset,
            } => {
                write!(
                    f,
                    "static input contains U+0000 at byte {offset}: {logical_path}"
                )
            }
            Self::HashMismatch { logical_path } => {
                write!(f, "static input content changed: {logical_path}")
            }
            Self::Stale(reason) => write!(f, "static input manifest is stale: {reason}"),
            Self::MetadataMalformed { logical_path } => {
                write!(f, "checked metadata is malformed: {logical_path}")
            }
            Self::BadMagic => write!(f, "static input manifest has an invalid magic"),
            Self::UnknownVersion(version) => {
                write!(f, "unknown static input manifest version {version}")
            }
            Self::Truncated => write!(f, "static input manifest is truncated"),
            Self::BadTag { what, tag } => write!(f, "invalid {what} tag byte {tag}"),
            Self::BadUtf8 => write!(f, "static input manifest contains invalid UTF-8"),
            Self::TrailingBytes => write!(f, "static input manifest has trailing bytes"),
            Self::NonCanonical(reason) => {
                write!(f, "non-canonical static input manifest: {reason}")
            }
        }
    }
}

impl std::error::Error for StaticInputError {}

fn descriptor_input_error(
    descriptor: &StaticDescriptor,
    cause: StaticDescriptorInputErrorCause,
) -> StaticDescriptorInputError {
    StaticDescriptorInputError {
        descriptor_id: descriptor.descriptor_id.clone(),
        span: descriptor.constructor_span,
        cause,
    }
}

fn descriptor_consumer(consumer: StaticDescriptorConsumer) -> StaticConsumerKind {
    match consumer {
        StaticDescriptorConsumer::Query => StaticConsumerKind::Query,
        StaticDescriptorConsumer::Command => StaticConsumerKind::Command,
    }
}

fn descriptor_driver(driver: StaticDescriptorDriver) -> DriverRestriction {
    match driver {
        StaticDescriptorDriver::AnySupportedDriver => DriverRestriction::AnySupportedDriver,
        StaticDescriptorDriver::SQLiteOnly => DriverRestriction::SQLiteOnly,
        StaticDescriptorDriver::PostgreSQLOnly => DriverRestriction::PostgreSQLOnly,
    }
}

fn permitted_drivers(restriction: DriverRestriction) -> &'static [Driver] {
    match restriction {
        DriverRestriction::AnySupportedDriver => &[Driver::SQLite, Driver::PostgreSQL],
        DriverRestriction::SQLiteOnly => &[Driver::SQLite],
        DriverRestriction::PostgreSQLOnly => &[Driver::PostgreSQL],
    }
}

fn record_file_snapshot(
    resolved: &[ResolvedStaticInput],
    snapshots: &mut HashMap<String, usize>,
    input: &ResolvedStaticInput,
) -> Result<(), StaticInputError> {
    let SqlSourceIdentity::File { logical_path } = &input.input.source else {
        return Ok(());
    };
    let Some(first_index) = snapshots.get(logical_path).copied() else {
        snapshots.insert(logical_path.clone(), resolved.len());
        return Ok(());
    };
    match resolved.get(first_index) {
        Some(first) if first.bytes == input.bytes => Ok(()),
        Some(_) | None => Err(StaticInputError::HashMismatch {
            logical_path: logical_path.clone(),
        }),
    }
}

/// Resolve the complete L5c descriptor inventory through the L5b source/metadata boundary.
///
/// Resolution and canonical manifest formation finish before SQL sources are added to `source_map`,
/// so a late failure cannot publish a partial batch. Shared file identities receive one SourceMap
/// entry and every descriptor points at that same `FileId`.
pub fn resolve_static_descriptors(
    project_root: &Path,
    source_map: &mut SourceMap,
    descriptors: &[StaticDescriptor],
    resolution_digest: Hash128,
) -> Result<ResolvedStaticInputs, StaticDescriptorInputError> {
    resolve_static_descriptors_inner(
        project_root,
        source_map,
        descriptors,
        resolution_digest,
        true,
        None,
    )
}

pub(crate) fn resolve_static_descriptors_at(
    project_root: &Path,
    source_map: &mut SourceMap,
    descriptors: &[StaticDescriptor],
    resolution_digest: Hash128,
    defining_paths: &HashMap<align_span::FileId, PathBuf>,
) -> Result<ResolvedStaticInputs, StaticDescriptorInputError> {
    resolve_static_descriptors_inner(
        project_root,
        source_map,
        descriptors,
        resolution_digest,
        true,
        Some(defining_paths),
    )
}

/// Resolve descriptor/source inputs without reading the metadata files being regenerated.
pub(crate) fn resolve_static_descriptors_for_regeneration(
    project_root: &Path,
    source_map: &mut SourceMap,
    descriptors: &[StaticDescriptor],
    resolution_digest: Hash128,
) -> Result<ResolvedStaticInputs, StaticDescriptorInputError> {
    resolve_static_descriptors_inner(
        project_root,
        source_map,
        descriptors,
        resolution_digest,
        false,
        None,
    )
}

fn resolve_static_descriptors_inner(
    project_root: &Path,
    source_map: &mut SourceMap,
    descriptors: &[StaticDescriptor],
    resolution_digest: Hash128,
    load_checked_metadata: bool,
    defining_paths: Option<&HashMap<align_span::FileId, PathBuf>>,
) -> Result<ResolvedStaticInputs, StaticDescriptorInputError> {
    let mut descriptor_ids = HashSet::with_capacity(descriptors.len());
    for descriptor in descriptors {
        if let Err(error) = validate_descriptor_id(&descriptor.descriptor_id) {
            return Err(descriptor_input_error(
                descriptor,
                StaticDescriptorInputErrorCause::Input(error),
            ));
        }
        if !descriptor_ids.insert(descriptor.descriptor_id.as_str()) {
            return Err(descriptor_input_error(
                descriptor,
                StaticDescriptorInputErrorCause::Input(StaticInputError::NonCanonical(
                    "duplicate descriptor id".to_string(),
                )),
            ));
        }
    }
    let publication_lock = if load_checked_metadata {
        descriptors
            .first()
            .map(|descriptor| {
                lock_metadata_publication_shared(project_root).map_err(|error| {
                    descriptor_input_error(
                        descriptor,
                        StaticDescriptorInputErrorCause::Input(error),
                    )
                })
            })
            .transpose()?
    } else {
        None
    };
    let mut resolved = Vec::with_capacity(descriptors.len());
    let mut file_snapshots = HashMap::new();
    for descriptor in descriptors {
        let defining_source = source_map
            .files()
            .get(descriptor.constructor_span.file as usize)
            .map(|file| (file.name.clone(), file.src.clone()))
            .ok_or_else(|| {
                descriptor_input_error(
                    descriptor,
                    StaticDescriptorInputErrorCause::InvalidDefiningFile,
                )
            })?;
        let (defining_file, defining_source_text) = defining_source;
        let defining_path = defining_paths
            .and_then(|paths| paths.get(&descriptor.constructor_span.file))
            .cloned()
            .unwrap_or_else(|| PathBuf::from(&defining_file));
        let consumer = descriptor_consumer(descriptor.consumer);
        let restriction = descriptor_driver(descriptor.driver);
        let inline_mapping = match &descriptor.source {
            StaticDescriptorSource::Inline {
                decoded_sql,
                literal_span,
            } => {
                let Some((decoded, runs)) =
                    align_lexer::decoded_string_runs(&defining_source_text, *literal_span)
                else {
                    return Err(descriptor_input_error(
                        descriptor,
                        StaticDescriptorInputErrorCause::Input(StaticInputError::NonCanonical(
                            "cannot reconstruct the inline SQL literal span map".to_string(),
                        )),
                    ));
                };
                if decoded != *decoded_sql {
                    return Err(descriptor_input_error(
                        descriptor,
                        StaticDescriptorInputErrorCause::Input(StaticInputError::HashMismatch {
                            logical_path: descriptor.descriptor_id.clone(),
                        }),
                    ));
                }
                Some((decoded, runs, *literal_span))
            }
            StaticDescriptorSource::File { .. } => None,
        };
        let mut input = match &descriptor.source {
            StaticDescriptorSource::File { path_literal, .. } => resolve_static_file(
                project_root,
                &defining_path,
                path_literal.as_deref(),
                descriptor.descriptor_id.clone(),
                consumer,
                restriction,
                None,
            ),
            StaticDescriptorSource::Inline { decoded_sql, .. } => resolve_inline_static_input(
                descriptor.descriptor_id.clone(),
                decoded_sql,
                consumer,
                restriction,
            ),
        }
        .map_err(|error| {
            let span = match (&error, &inline_mapping) {
                (StaticInputError::EmbeddedNul { offset, .. }, Some((_, runs, literal_span))) => {
                    runs.iter()
                        .find(|run| run.decoded_start <= *offset && *offset < run.decoded_end)
                        .map(|run| {
                            let decoded_width = run.decoded_end - run.decoded_start;
                            let source_width = run.source_end - run.source_start;
                            if decoded_width == source_width {
                                let source_start = run
                                    .source_start
                                    .checked_add(*offset - run.decoded_start)
                                    .unwrap_or(run.source_start);
                                Span::new(
                                    literal_span.file,
                                    source_start,
                                    source_start.checked_add(1).unwrap_or(run.source_end),
                                )
                            } else {
                                Span::new(literal_span.file, run.source_start, run.source_end)
                            }
                        })
                        .unwrap_or(descriptor.constructor_span)
                }
                _ => descriptor.constructor_span,
            };
            StaticDescriptorInputError {
                descriptor_id: descriptor.descriptor_id.clone(),
                span,
                cause: StaticDescriptorInputErrorCause::Input(error),
            }
        })?;
        if let Some((decoded, runs, _)) = inline_mapping {
            if decoded.as_bytes() != input.bytes {
                return Err(descriptor_input_error(
                    descriptor,
                    StaticDescriptorInputErrorCause::Input(StaticInputError::HashMismatch {
                        logical_path: descriptor.descriptor_id.clone(),
                    }),
                ));
            }
            input.decoded_span_map = runs
                .into_iter()
                .map(|run| DecodedSpanEntry {
                    decoded_span: align_interface::Span {
                        start: run.decoded_start,
                        end: run.decoded_end,
                    },
                    defining_file_span: align_interface::Span {
                        start: run.source_start,
                        end: run.source_end,
                    },
                })
                .collect();
        }
        let metadata = if load_checked_metadata {
            permitted_drivers(restriction)
                .iter()
                .copied()
                .map(|driver| snapshot_checked_metadata_record(project_root, &descriptor.descriptor_id, driver))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    descriptor_input_error(descriptor, StaticDescriptorInputErrorCause::Input(error))
                })?
        } else {
            permitted_drivers(restriction)
                .iter()
                .copied()
                .map(|driver| {
                    Ok((CheckedMetadataInput {
                        driver,
                        logical_path: metadata_logical_path(&descriptor.descriptor_id, driver)?,
                        state: MetadataState::Missing,
                    }, None))
                })
                .collect::<Result<Vec<_>, StaticInputError>>()
                .map_err(|error| {
                    descriptor_input_error(descriptor, StaticDescriptorInputErrorCause::Input(error))
                })?
        };
        input.input.checked_metadata = metadata.iter().map(|(input, _)| input.clone()).collect();
        input.checked_metadata_records = metadata
            .into_iter()
            .filter_map(|(_, record)| record)
            .collect();
        record_file_snapshot(&resolved, &mut file_snapshots, &input).map_err(|error| {
            descriptor_input_error(descriptor, StaticDescriptorInputErrorCause::Input(error))
        })?;
        resolved.push(input);
    }

    if let (Some(lock), Some(descriptor)) = (&publication_lock, descriptors.first()) {
        lock.validate().map_err(|error| {
            descriptor_input_error(
                descriptor,
                StaticDescriptorInputErrorCause::Input(error),
            )
        })?;
    }

    let manifest = match StaticInputManifest::new(
        resolution_digest,
        resolved.iter().map(|input| input.input.clone()).collect(),
    ) {
        Ok(manifest) => manifest,
        Err(error) => {
            let Some(descriptor) = descriptors.first() else {
                return Ok(ResolvedStaticInputs {
                    inputs: Vec::new(),
                    manifest: StaticInputManifest::empty(resolution_digest),
                });
            };
            return Err(descriptor_input_error(
                descriptor,
                StaticDescriptorInputErrorCause::Input(error),
            ));
        }
    };

    let mut file_publications = Vec::<(String, String)>::new();
    let mut publication_index = HashSet::<String>::new();
    for (descriptor, input) in descriptors.iter().zip(&resolved) {
        let SqlSourceIdentity::File { logical_path } = &input.input.source else {
            continue;
        };
        if !publication_index.insert(logical_path.clone()) {
            continue;
        }
        let text = std::str::from_utf8(&input.bytes).map_err(|_| {
            descriptor_input_error(
                descriptor,
                StaticDescriptorInputErrorCause::Input(StaticInputError::InvalidUtf8 {
                    logical_path: logical_path.clone(),
                }),
            )
        })?;
        file_publications.push((logical_path.clone(), text.to_string()));
    }

    let mut registered = HashMap::<String, FileId>::new();
    for (logical_path, text) in file_publications {
        let file_id = source_map.add_file(logical_path.clone(), text);
        registered.insert(logical_path, file_id);
    }
    for input in &mut resolved {
        let SqlSourceIdentity::File { logical_path } = &input.input.source else {
            continue;
        };
        input.source_map_file = registered.get(logical_path).copied();
    }

    Ok(ResolvedStaticInputs {
        inputs: resolved,
        manifest,
    })
}

fn invalid_descriptor_id(id: &str) -> bool {
    id.is_empty() || id.as_bytes().contains(&0) || id.contains('\n') || id.contains('\r')
}

fn validate_descriptor_id(id: &str) -> Result<(), StaticInputError> {
    if invalid_descriptor_id(id) {
        Err(StaticInputError::InvalidDescriptorId)
    } else if id.len() > MAX_FIELD_BYTES {
        Err(StaticInputError::NonCanonical(
            "descriptor id exceeds the field limit".to_string(),
        ))
    } else {
        Ok(())
    }
}

fn validate_text_field(value: &str, what: &str) -> Result<(), StaticInputError> {
    if value.as_bytes().contains(&0) {
        return Err(StaticInputError::NonCanonical(format!(
            "{what} contains U+0000"
        )));
    }
    if value.len() > MAX_FIELD_BYTES {
        return Err(StaticInputError::NonCanonical(format!(
            "{what} exceeds the field limit"
        )));
    }
    Ok(())
}

fn source_sort_key(source: &SqlSourceIdentity) -> (u8, &[u8]) {
    match source {
        SqlSourceIdentity::File { logical_path } => (0, logical_path.as_bytes()),
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => (1, query_or_command_id.as_bytes()),
    }
}

fn source_identity_id(source: &SqlSourceIdentity) -> &str {
    match source {
        SqlSourceIdentity::File { logical_path } => logical_path,
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => query_or_command_id,
    }
}

fn source_identity_path(source: &SqlSourceIdentity) -> Option<&str> {
    match source {
        SqlSourceIdentity::File { logical_path } => Some(logical_path),
        SqlSourceIdentity::Inline { .. } => None,
    }
}

fn driver_dir(driver: Driver) -> &'static str {
    match driver {
        Driver::SQLite => "sqlite",
        Driver::PostgreSQL => "postgres",
    }
}

pub fn metadata_logical_path(
    descriptor_id: &str,
    driver: Driver,
) -> Result<String, StaticInputError> {
    validate_descriptor_id(descriptor_id)?;
    let digest = Hash128::of(descriptor_id.as_bytes()).to_hex();
    Ok(format!(".align-db/{}/{digest}.json", driver_dir(driver)))
}

pub fn metadata_path(
    project_root: &Path,
    descriptor_id: &str,
    driver: Driver,
) -> Result<PathBuf, StaticInputError> {
    let logical = metadata_logical_path(descriptor_id, driver)?;
    Ok(project_root.join(logical.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

fn canonical_root(project_root: &Path) -> Result<PathBuf, StaticInputError> {
    let root = fs::canonicalize(project_root).map_err(|e| StaticInputError::Io {
        path: project_root.to_path_buf(),
        message: e.to_string(),
    })?;
    if !root.is_dir() {
        return Err(StaticInputError::RootNotDirectory(root));
    }
    Ok(root)
}

fn lexical_absolute(path: &Path) -> Result<PathBuf, StaticInputError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .map_err(|error| StaticInputError::Io {
                path: path.to_path_buf(),
                message: error.to_string(),
            })
    }
}

fn ensure_inside(root: &Path, path: &Path) -> Result<(), StaticInputError> {
    if path.strip_prefix(root).is_err() {
        Err(StaticInputError::OutsideProjectRoot(path.to_path_buf()))
    } else {
        Ok(())
    }
}

fn canonical_defining_file(
    root: &Path,
    defining_align_file: &Path,
) -> Result<PathBuf, StaticInputError> {
    let candidate = if defining_align_file.is_absolute() {
        defining_align_file.to_path_buf()
    } else if root.is_absolute() && defining_align_file.exists() {
        // CLI entry paths are commonly relative to the current working directory while the
        // project root has already been canonicalized. Do not prepend that root a second time
        // (`apps/db` + `apps/db/app/q.align`). Containment is still checked on the canonical path.
        defining_align_file.to_path_buf()
    } else {
        root.join(defining_align_file)
    };
    let canonical = fs::canonicalize(&candidate).map_err(|e| StaticInputError::Io {
        path: candidate.clone(),
        message: e.to_string(),
    })?;
    ensure_inside(root, &canonical)?;
    if !canonical.is_file() {
        return Err(StaticInputError::NotRegularFile(canonical));
    }
    // Keep the lexical module path for sibling/explicit linkage. The canonical target above is
    // used only for containment and regular-file validation; an in-root symlink must not silently
    // change which sibling directory the source literal names.
    Ok(candidate)
}

fn validate_literal_path(raw: &str) -> Result<(), StaticInputError> {
    if raw.is_empty() {
        return Err(StaticInputError::InvalidPath(
            "explicit path is empty".to_string(),
        ));
    }
    if raw.as_bytes().contains(&0) {
        return Err(StaticInputError::InvalidPath(
            "path contains NUL".to_string(),
        ));
    }
    if raw.contains('\\') {
        return Err(StaticInputError::InvalidPath(
            "backslash is not a portable separator".to_string(),
        ));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(StaticInputError::InvalidPath(
            "absolute paths are not allowed".to_string(),
        ));
    }
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(StaticInputError::InvalidPath(
                "path escapes its defining directory".to_string(),
            ));
        }
    }
    Ok(())
}

fn logical_path(root: &Path, candidate: &Path) -> Result<String, StaticInputError> {
    let relative = candidate
        .strip_prefix(root)
        .map_err(|_| StaticInputError::OutsideProjectRoot(candidate.to_path_buf()))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    StaticInputError::InvalidPath("logical path is not UTF-8".to_string())
                })?;
                if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
                    return Err(StaticInputError::InvalidPath(
                        "logical path is not canonical".to_string(),
                    ));
                }
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(StaticInputError::InvalidPath(
                    "logical path is not relative".to_string(),
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(StaticInputError::InvalidPath(
            "logical path is empty".to_string(),
        ));
    }
    Ok(parts.join("/"))
}

fn read_static_bytes(
    root: &Path,
    candidate: &Path,
    logical: &str,
) -> Result<Vec<u8>, StaticInputError> {
    align_watch::observe_consumed_classification(
        candidate,
        || read_static_bytes_inner(root, candidate, logical),
        |result| Some(static_observation_state(result)),
        |_| None,
        || {
            Err(StaticInputError::InvalidPath(
                "watch observation rejected static path".to_string(),
            ))
        },
    )
}

fn read_static_bytes_inner(
    root: &Path,
    candidate: &Path,
    logical: &str,
) -> Result<Vec<u8>, StaticInputError> {
    let canonical = fs::canonicalize(candidate).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StaticInputError::MissingFile(candidate.to_path_buf())
        } else {
            StaticInputError::Io {
                path: candidate.to_path_buf(),
                message: e.to_string(),
            }
        }
    })?;
    ensure_inside(root, &canonical)?;
    if !canonical.is_file() {
        return Err(StaticInputError::NotRegularFile(canonical));
    }
    let bytes = read_bounded_file_inner(&canonical, fs::File::open(&canonical), &|| {
        StaticInputError::NonCanonical(format!("static input exceeds the field limit: {logical}"))
    })?;
    if std::str::from_utf8(&bytes).is_err() {
        return Err(StaticInputError::InvalidUtf8 {
            logical_path: logical.to_string(),
        });
    }
    if let Some(offset) = bytes.iter().position(|byte| *byte == 0) {
        let offset = u32::try_from(offset).map_err(|_| StaticInputError::EmbeddedNul {
            logical_path: logical.to_string(),
            offset: u32::MAX,
        })?;
        return Err(StaticInputError::EmbeddedNul {
            logical_path: logical.to_string(),
            offset,
        });
    }
    Ok(bytes)
}

fn static_observation_state(
    result: &Result<Vec<u8>, StaticInputError>,
) -> align_watch::BuildInputState {
    match result {
        Ok(bytes) => align_watch::BuildInputState::Regular {
            content_hash: Hash128::of(bytes),
            len: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        },
        Err(StaticInputError::MissingFile(_)) => align_watch::BuildInputState::Missing,
        Err(StaticInputError::NotRegularFile(_)) => align_watch::BuildInputState::NonRegular,
        Err(_) => align_watch::BuildInputState::Unreadable,
    }
}

fn make_input(
    descriptor_id: String,
    source: SqlSourceIdentity,
    bytes: Vec<u8>,
    consumer_kind: StaticConsumerKind,
    driver_restriction: DriverRestriction,
) -> Result<StaticInput, StaticInputError> {
    validate_descriptor_id(&descriptor_id)?;
    match &source {
        SqlSourceIdentity::File { logical_path } => {
            validate_text_field(logical_path, "file logical path")?;
            if logical_path.is_empty()
                || logical_path.starts_with('/')
                || logical_path.contains('\\')
            {
                return Err(StaticInputError::InvalidSource(
                    "file logical path is not root-relative".to_string(),
                ));
            }
            if logical_path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            {
                return Err(StaticInputError::InvalidSource(
                    "file logical path is not canonical".to_string(),
                ));
            }
        }
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => {
            if query_or_command_id != &descriptor_id {
                return Err(StaticInputError::InvalidSource(
                    "inline identity does not equal the descriptor id".to_string(),
                ));
            }
        }
    }
    if std::str::from_utf8(&bytes).is_err() {
        return Err(StaticInputError::InvalidUtf8 {
            logical_path: source_identity_id(&source).to_string(),
        });
    }
    if let Some(offset) = bytes.iter().position(|byte| *byte == 0) {
        let offset = u32::try_from(offset).unwrap_or(u32::MAX);
        return Err(StaticInputError::EmbeddedNul {
            logical_path: source_identity_id(&source).to_string(),
            offset,
        });
    }
    Ok(StaticInput {
        descriptor_id,
        source,
        content_hash: Hash128::of(&bytes),
        consumer_kind,
        driver_restriction,
        checked_metadata: Vec::new(),
    })
}

pub fn resolve_static_file(
    project_root: &Path,
    defining_align_file: &Path,
    path_literal: Option<&str>,
    descriptor_id: impl Into<String>,
    consumer_kind: StaticConsumerKind,
    driver_restriction: DriverRestriction,
    source_map: Option<&mut SourceMap>,
) -> Result<ResolvedStaticInput, StaticInputError> {
    let descriptor_id = descriptor_id.into();
    validate_descriptor_id(&descriptor_id)?;
    let lexical_root = lexical_absolute(project_root)?;
    let root = canonical_root(project_root)?;
    let lexical_defining = if defining_align_file.is_absolute() {
        defining_align_file.to_path_buf()
    } else if project_root.is_absolute() && defining_align_file.exists() {
        lexical_absolute(defining_align_file)?
    } else {
        lexical_root.join(defining_align_file)
    };
    let defining = canonical_defining_file(&root, &lexical_defining)?;
    let candidate = match path_literal {
        None => defining.with_extension("sql"),
        Some(raw) => {
            validate_literal_path(raw)?;
            defining
                .parent()
                .ok_or_else(|| {
                    StaticInputError::InvalidPath("defining file has no directory".to_string())
                })?
                .join(raw)
        }
    };
    let logical = logical_path(&lexical_root, &candidate)?;
    let bytes = read_static_bytes(&root, &candidate, &logical)?;
    let input = make_input(
        descriptor_id,
        SqlSourceIdentity::File {
            logical_path: logical.clone(),
        },
        bytes.clone(),
        consumer_kind,
        driver_restriction,
    )?;
    let text = std::str::from_utf8(&bytes).map_err(|_| StaticInputError::InvalidUtf8 {
        logical_path: logical.clone(),
    })?;
    let source_map_file = source_map.map(|map| map.add_file(logical.clone(), text.to_string()));
    Ok(ResolvedStaticInput {
        input,
        bytes,
        decoded_span_map: Vec::new(),
        source_map_file,
        resolved_path: Some(candidate),
        checked_metadata_records: Vec::new(),
    })
}

pub fn resolve_inline_static_input(
    descriptor_id: impl Into<String>,
    decoded_sql: &str,
    consumer_kind: StaticConsumerKind,
    driver_restriction: DriverRestriction,
) -> Result<ResolvedStaticInput, StaticInputError> {
    let descriptor_id = descriptor_id.into();
    validate_descriptor_id(&descriptor_id)?;
    // Apply the shared field bound before copying so oversized inline SQL is
    // rejected without allocating an unbounded source buffer.
    if decoded_sql.len() > MAX_FIELD_BYTES {
        return Err(StaticInputError::NonCanonical(
            "inline SQL exceeds the field limit".to_string(),
        ));
    }
    let bytes = decoded_sql.as_bytes().to_vec();
    let input = make_input(
        descriptor_id.clone(),
        SqlSourceIdentity::Inline {
            query_or_command_id: descriptor_id,
        },
        bytes.clone(),
        consumer_kind,
        driver_restriction,
    )?;
    Ok(ResolvedStaticInput {
        input,
        bytes,
        decoded_span_map: Vec::new(),
        source_map_file: None,
        resolved_path: None,
        checked_metadata_records: Vec::new(),
    })
}

pub fn snapshot_checked_metadata(
    project_root: &Path,
    descriptor_id: &str,
    driver: Driver,
) -> Result<CheckedMetadataInput, StaticInputError> {
    snapshot_checked_metadata_record(project_root, descriptor_id, driver).map(|(input, _)| input)
}

fn snapshot_checked_metadata_record(
    project_root: &Path,
    descriptor_id: &str,
    driver: Driver,
) -> Result<(CheckedMetadataInput, Option<ParsedCheckedMetadata>), StaticInputError> {
    let logical = metadata_logical_path(descriptor_id, driver)?;
    let root = canonical_root(project_root)?;
    let path = root.join(logical.replace('/', std::path::MAIN_SEPARATOR_STR));
    let metadata = align_watch::observe_consumed_classification(
        &path,
        || fs::symlink_metadata(&path),
        |result| match result {
            Ok(metadata) if metadata.is_file() => None,
            Ok(_) => Some(align_watch::BuildInputState::NonRegular),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Some(align_watch::BuildInputState::Missing)
            }
            Err(_) => Some(align_watch::BuildInputState::Unreadable),
        },
        |result| result.as_ref().ok(),
        || Err(std::io::Error::other("watch observation rejected metadata path")),
    );
    match metadata {
        Ok(metadata) if metadata.is_file() => {
            let bytes = read_metadata_bytes(&root, &path, &logical)?;
            let record = parse_checked_metadata(&bytes, &logical, descriptor_id, driver)?;
            Ok((CheckedMetadataInput {
                driver,
                logical_path: logical,
                state: MetadataState::Present {
                    content_hash: record.metadata_digest,
                    format_version: record.format_version,
                },
            }, Some(record)))
        }
        Ok(_) => Err(StaticInputError::NotRegularFile(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure_metadata_parent_inside(&root, &path)?;
            Ok((CheckedMetadataInput {
                driver,
                logical_path: logical,
                state: MetadataState::Missing,
            }, None))
        }
        Err(error) => Err(StaticInputError::Io {
            path,
            message: error.to_string(),
        }),
    }
}

fn parse_checked_metadata(
    bytes: &[u8],
    logical_path: &str,
    descriptor_id: &str,
    driver: Driver,
) -> Result<ParsedCheckedMetadata, StaticInputError> {
    if bytes.len() > MAX_FIELD_BYTES {
        return Err(StaticInputError::MetadataMalformed {
            logical_path: logical_path.to_string(),
        });
    }
    std::str::from_utf8(bytes).map_err(|_| StaticInputError::MetadataMalformed {
        logical_path: logical_path.to_string(),
    })?;
    if bytes.len() < 2 || bytes.last() != Some(&b'\n') || bytes[..bytes.len() - 1].contains(&b'\n')
    {
        return Err(StaticInputError::MetadataMalformed {
            logical_path: logical_path.to_string(),
        });
    }
    let mut parser = MetadataJsonParser::new(&bytes[..bytes.len() - 1]);
    let mut record = parser
        .parse_metadata_object(descriptor_id, driver)
        .map_err(|_| StaticInputError::MetadataMalformed {
            logical_path: logical_path.to_string(),
        })?;
    record.metadata_digest = Hash128::of(bytes);
    Ok(record)
}

/// Encode one Q3 checked-metadata record using the exact canonical v1 JSON contract.
///
/// The writer deliberately emits fields directly in contract order instead of routing through a
/// general JSON value/map. Before returning publishable bytes it feeds them through the production
/// fail-closed reader and requires semantic equality, which catches non-dense ordinals, invalid
/// driver/source combinations, unsorted extensions, and every other reader invariant before a
/// temporary file is created.
pub(crate) fn encode_checked_metadata(
    descriptor_id: &str,
    record: &ParsedCheckedMetadata,
) -> Result<Vec<u8>, StaticInputError> {
    fn string(output: &mut Vec<u8>, value: &str) -> Result<(), StaticInputError> {
        if value.len() > MAX_FIELD_BYTES || value.as_bytes().contains(&0) {
            return Err(StaticInputError::NonCanonical(
                "checked metadata text is too large or contains U+0000".to_string(),
            ));
        }
        output.push(b'"');
        for character in value.chars() {
            match character {
                '"' => output.extend_from_slice(br#"\""#),
                '\\' => output.extend_from_slice(br#"\\"#),
                '\u{0008}' => output.extend_from_slice(br#"\b"#),
                '\t' => output.extend_from_slice(br#"\t"#),
                '\n' => output.extend_from_slice(br#"\n"#),
                '\u{000c}' => output.extend_from_slice(br#"\f"#),
                '\r' => output.extend_from_slice(br#"\r"#),
                control if control <= '\u{001f}' => {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    let value = control as u32;
                    output.extend_from_slice(br#"\u00"#);
                    output.push(HEX[((value >> 4) & 0xf) as usize]);
                    output.push(HEX[(value & 0xf) as usize]);
                }
                scalar => {
                    let mut encoded = [0u8; 4];
                    output.extend_from_slice(scalar.encode_utf8(&mut encoded).as_bytes());
                }
            }
        }
        output.push(b'"');
        Ok(())
    }

    fn field(output: &mut Vec<u8>, first: bool, name: &str) {
        if !first {
            output.push(b',');
        }
        output.push(b'"');
        output.extend_from_slice(name.as_bytes());
        output.extend_from_slice(b"\":");
    }

    fn hash(output: &mut Vec<u8>, value: Hash128) {
        output.push(b'"');
        output.extend_from_slice(value.to_hex().as_bytes());
        output.push(b'"');
    }

    fn optional_string(output: &mut Vec<u8>, value: Option<&str>) -> Result<(), StaticInputError> {
        match value {
            Some(value) => string(output, value),
            None => {
                output.extend_from_slice(b"null");
                Ok(())
            }
        }
    }

    fn optional_i64(output: &mut Vec<u8>, value: Option<i64>) {
        match value {
            Some(value) => output.extend_from_slice(value.to_string().as_bytes()),
            None => output.extend_from_slice(b"null"),
        }
    }

    let mut output = Vec::new();
    output.push(b'{');
    field(&mut output, true, "format_version");
    output.extend_from_slice(record.format_version.to_string().as_bytes());
    field(&mut output, false, "descriptor_id");
    string(&mut output, descriptor_id)?;
    let (module, item) = descriptor_id.rsplit_once('.').ok_or_else(|| {
        StaticInputError::NonCanonical("checked metadata descriptor id has no module".to_string())
    })?;
    field(&mut output, false, "module");
    string(&mut output, module)?;
    field(&mut output, false, "item");
    string(&mut output, item)?;
    field(&mut output, false, "driver");
    string(&mut output, driver_dir(record.driver))?;
    field(&mut output, false, "driver_restriction");
    string(
        &mut output,
        match record.driver_restriction {
            DriverRestriction::AnySupportedDriver => "any_supported_driver",
            DriverRestriction::SQLiteOnly => "sqlite_only",
            DriverRestriction::PostgreSQLOnly => "postgres_only",
        },
    )?;
    field(&mut output, false, "statement_kind");
    string(
        &mut output,
        match record.statement_kind {
            MetadataStatementKind::Query => "query",
            MetadataStatementKind::Command => "command",
        },
    )?;
    field(&mut output, false, "statement_class");
    string(
        &mut output,
        match record.statement_class {
            MetaStatementClass::Select => "select",
            MetaStatementClass::Dml => "dml",
            MetaStatementClass::Ddl => "ddl",
            MetaStatementClass::Native => "native",
            MetaStatementClass::Unknown => "unknown",
        },
    )?;
    field(&mut output, false, "source_identity");
    output.push(b'{');
    field(&mut output, true, "kind");
    match &record.source_identity {
        SqlSourceIdentity::File { logical_path } => {
            string(&mut output, "file")?;
            field(&mut output, false, "logical_path");
            string(&mut output, logical_path)?;
        }
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => {
            string(&mut output, "inline")?;
            field(&mut output, false, "descriptor_id");
            string(&mut output, query_or_command_id)?;
        }
    }
    output.push(b'}');
    field(&mut output, false, "source_sql_hash");
    hash(&mut output, record.source_sql_hash);
    field(&mut output, false, "wire_sql_hash");
    hash(&mut output, record.wire_sql_hash);
    field(&mut output, false, "rewrite_format_version");
    output.extend_from_slice(record.rewrite_format_version.to_string().as_bytes());
    field(&mut output, false, "static_options_hash");
    hash(&mut output, record.static_options_hash);
    field(&mut output, false, "params_fingerprint");
    hash(&mut output, record.params_fingerprint);
    field(&mut output, false, "row_fingerprint");
    match record.row_fingerprint {
        Some(value) => hash(&mut output, value),
        None => output.extend_from_slice(b"null"),
    }
    field(&mut output, false, "schema_fingerprint");
    hash(&mut output, record.schema_fingerprint);
    field(&mut output, false, "engine_version");
    string(&mut output, &record.engine_version)?;
    field(&mut output, false, "driver_version");
    string(&mut output, &record.driver_version)?;
    field(&mut output, false, "search_path");
    output.push(b'[');
    for (index, path) in record.search_path.iter().enumerate() {
        if index != 0 {
            output.push(b',');
        }
        string(&mut output, path)?;
    }
    output.push(b']');
    field(&mut output, false, "extensions");
    output.push(b'[');
    for (index, extension) in record.extensions.iter().enumerate() {
        if index != 0 {
            output.push(b',');
        }
        output.push(b'{');
        field(&mut output, true, "schema");
        string(&mut output, &extension.schema)?;
        field(&mut output, false, "name");
        string(&mut output, &extension.name)?;
        field(&mut output, false, "version");
        optional_string(&mut output, extension.version.as_deref())?;
        output.push(b'}');
    }
    output.push(b']');
    field(&mut output, false, "parameters");
    output.push(b'[');
    for (index, parameter) in record.parameters.iter().enumerate() {
        if index != 0 {
            output.push(b',');
        }
        output.push(b'{');
        field(&mut output, true, "source_name");
        string(&mut output, &parameter.source_name)?;
        field(&mut output, false, "protocol_ordinal");
        output.extend_from_slice(parameter.checked.ordinal.to_string().as_bytes());
        field(&mut output, false, "logical_type");
        string(&mut output, &parameter.logical_type)?;
        field(&mut output, false, "native_type");
        optional_string(&mut output, parameter.checked.native_type.as_deref())?;
        field(&mut output, false, "native_type_id");
        optional_i64(&mut output, parameter.checked.native_type_id);
        output.push(b'}');
    }
    output.push(b']');
    field(&mut output, false, "columns");
    output.push(b'[');
    for (index, column) in record.columns.iter().enumerate() {
        if index != 0 {
            output.push(b',');
        }
        output.push(b'{');
        field(&mut output, true, "ordinal");
        output.extend_from_slice(column.checked.ordinal.to_string().as_bytes());
        field(&mut output, false, "source_alias");
        string(&mut output, &column.source_alias)?;
        field(&mut output, false, "logical_type");
        string(&mut output, &column.logical_type)?;
        field(&mut output, false, "native_type");
        optional_string(&mut output, column.checked.native_type.as_deref())?;
        field(&mut output, false, "native_type_id");
        optional_i64(&mut output, column.checked.native_type_id);
        field(&mut output, false, "nullable");
        string(
            &mut output,
            match column.checked.nullable {
                MetaNullability::Yes => "yes",
                MetaNullability::No => "no",
                MetaNullability::Unknown => "unknown",
            },
        )?;
        field(&mut output, false, "origin_schema");
        optional_string(&mut output, column.checked.origin_schema.as_deref())?;
        field(&mut output, false, "origin_table");
        optional_string(&mut output, column.checked.origin_table.as_deref())?;
        field(&mut output, false, "origin_column");
        optional_string(&mut output, column.checked.origin_column.as_deref())?;
        output.push(b'}');
    }
    output.extend_from_slice(b"]}\n");
    if output.len() > MAX_FIELD_BYTES {
        return Err(StaticInputError::NonCanonical(
            "checked metadata exceeds the field limit".to_string(),
        ));
    }

    let logical_path = metadata_logical_path(descriptor_id, record.driver)?;
    let parsed = parse_checked_metadata(&output, &logical_path, descriptor_id, record.driver)?;
    let mut expected = record.clone();
    expected.metadata_digest = Hash128::of(&output);
    if parsed != expected {
        return Err(StaticInputError::NonCanonical(
            "checked metadata writer failed semantic round trip".to_string(),
        ));
    }
    Ok(output)
}

struct MetadataJsonParser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> MetadataJsonParser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn parse_metadata_object(
        &mut self,
        descriptor_id: &str,
        driver: Driver,
    ) -> Result<ParsedCheckedMetadata, ()> {
        self.expect_byte(b'{')?;
        self.field(true, "format_version")?;
        let format_version = self.parse_u32()?;
        if format_version != 1 {
            return Err(());
        }

        self.field(false, "descriptor_id")?;
        let metadata_descriptor_id = self.parse_text()?;
        if metadata_descriptor_id != descriptor_id {
            return Err(());
        }

        self.field(false, "module")?;
        let module = self.parse_text()?;
        self.field(false, "item")?;
        let item = self.parse_text()?;
        let (expected_module, expected_item) = descriptor_id.rsplit_once('.').ok_or(())?;
        if expected_module.is_empty()
            || expected_item.is_empty()
            || module != expected_module
            || item != expected_item
        {
            return Err(());
        }

        self.field(false, "driver")?;
        if self.parse_text()?.as_str() != driver_dir(driver) {
            return Err(());
        }

        self.field(false, "driver_restriction")?;
        let driver_restriction = match self.parse_text()?.as_str() {
            "any_supported_driver" => DriverRestriction::AnySupportedDriver,
            "sqlite_only" => DriverRestriction::SQLiteOnly,
            "postgres_only" => DriverRestriction::PostgreSQLOnly,
            _ => return Err(()),
        };
        if !driver_restriction.drivers().contains(&driver) {
            return Err(());
        }

        self.field(false, "statement_kind")?;
        let statement_kind = match self.parse_text()?.as_str() {
            "query" => MetadataStatementKind::Query,
            "command" => MetadataStatementKind::Command,
            _ => return Err(()),
        };

        self.field(false, "statement_class")?;
        let statement_class = match self.parse_text()?.as_str() {
            "select" => MetaStatementClass::Select,
            "dml" => MetaStatementClass::Dml,
            "ddl" => MetaStatementClass::Ddl,
            "native" => MetaStatementClass::Native,
            "unknown" => MetaStatementClass::Unknown,
            _ => return Err(()),
        };

        self.field(false, "source_identity")?;
        let source_identity = self.parse_source_identity(descriptor_id)?;

        self.field(false, "source_sql_hash")?;
        let source_sql_hash = self.parse_hash()?;
        self.field(false, "wire_sql_hash")?;
        let wire_sql_hash = self.parse_hash()?;

        self.field(false, "rewrite_format_version")?;
        let rewrite_format_version = self.parse_u32()?;
        if rewrite_format_version != 1 {
            return Err(());
        }

        self.field(false, "static_options_hash")?;
        let static_options_hash = self.parse_hash()?;
        self.field(false, "params_fingerprint")?;
        let params_fingerprint = self.parse_hash()?;
        self.field(false, "row_fingerprint")?;
        let row_fingerprint = self.parse_optional_hash()?;
        self.field(false, "schema_fingerprint")?;
        let schema_fingerprint = self.parse_hash()?;

        self.field(false, "engine_version")?;
        let engine_version = self.parse_text()?;
        self.field(false, "driver_version")?;
        let driver_version = self.parse_text()?;

        self.field(false, "search_path")?;
        let search_path = self.parse_string_array()?;
        self.field(false, "extensions")?;
        let extensions = self.parse_extensions()?;
        self.field(false, "parameters")?;
        let parameters = self.parse_parameters()?;
        self.field(false, "columns")?;
        let columns = self.parse_columns()?;

        self.expect_byte(b'}')?;
        if self.position != self.bytes.len() {
            return Err(());
        }

        if (statement_kind == MetadataStatementKind::Query) != row_fingerprint.is_some() {
            return Err(());
        }
        if statement_kind == MetadataStatementKind::Command && !columns.is_empty() {
            return Err(());
        }
        if driver == Driver::SQLite && (!search_path.is_empty() || !extensions.is_empty()) {
            return Err(());
        }
        Ok(ParsedCheckedMetadata {
            format_version,
            metadata_digest: Hash128 { lo: 0, hi: 0 },
            driver,
            driver_restriction,
            statement_kind,
            statement_class,
            source_identity,
            source_sql_hash,
            wire_sql_hash,
            rewrite_format_version,
            static_options_hash,
            params_fingerprint,
            row_fingerprint,
            schema_fingerprint,
            engine_version,
            driver_version,
            search_path,
            extensions,
            parameters,
            columns,
        })
    }

    fn field(&mut self, first: bool, expected: &str) -> Result<(), ()> {
        if !first {
            self.expect_byte(b',')?;
        }
        if self.parse_string()? != expected.as_bytes() {
            return Err(());
        }
        self.expect_byte(b':')
    }

    fn parse_text(&mut self) -> Result<String, ()> {
        let value = decode_canonical_json_string(self.parse_string()?)?;
        if value.len() > MAX_FIELD_BYTES || value.as_bytes().contains(&0) {
            return Err(());
        }
        Ok(value)
    }

    fn parse_u32(&mut self) -> Result<u32, ()> {
        let number = self.parse_number()?;
        if number.first() == Some(&b'-') {
            return Err(());
        }
        let number = std::str::from_utf8(number).map_err(|_| ())?;
        number
            .parse::<u64>()
            .ok()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(())
    }

    fn parse_i64(&mut self) -> Result<i64, ()> {
        let number = std::str::from_utf8(self.parse_number()?).map_err(|_| ())?;
        number.parse::<i64>().map_err(|_| ())
    }

    fn parse_hash(&mut self) -> Result<Hash128, ()> {
        let value = self.parse_text()?;
        if value.len() != 32
            || !value
                .as_bytes()
                .iter()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(());
        }
        let lo = u64::from_str_radix(&value[..16], 16).map_err(|_| ())?;
        let hi = u64::from_str_radix(&value[16..], 16).map_err(|_| ())?;
        Ok(Hash128 { lo, hi })
    }

    fn parse_optional_hash(&mut self) -> Result<Option<Hash128>, ()> {
        if self.peek_byte() == Some(b'n') {
            self.expect_bytes(b"null")?;
            Ok(None)
        } else {
            Ok(Some(self.parse_hash()?))
        }
    }

    fn parse_optional_text(&mut self) -> Result<Option<String>, ()> {
        if self.peek_byte() == Some(b'n') {
            self.expect_bytes(b"null")?;
            Ok(None)
        } else {
            Ok(Some(self.parse_text()?))
        }
    }

    fn parse_optional_i64(&mut self) -> Result<Option<i64>, ()> {
        if self.peek_byte() == Some(b'n') {
            self.expect_bytes(b"null")?;
            Ok(None)
        } else {
            Ok(Some(self.parse_i64()?))
        }
    }

    fn parse_source_identity(&mut self, descriptor_id: &str) -> Result<SqlSourceIdentity, ()> {
        self.expect_byte(b'{')?;
        self.field(true, "kind")?;
        let kind = self.parse_text()?;
        let identity = match kind.as_str() {
            "file" => {
                self.field(false, "logical_path")?;
                let path = self.parse_text()?;
                if path.is_empty()
                    || path.starts_with('/')
                    || path.contains('\\')
                    || path
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == "..")
                {
                    return Err(());
                }
                SqlSourceIdentity::File { logical_path: path }
            }
            "inline" => {
                self.field(false, "descriptor_id")?;
                if self.parse_text()? != descriptor_id {
                    return Err(());
                }
                SqlSourceIdentity::Inline {
                    query_or_command_id: descriptor_id.to_string(),
                }
            }
            _ => return Err(()),
        };
        self.expect_byte(b'}')?;
        Ok(identity)
    }

    fn parse_string_array(&mut self) -> Result<Vec<String>, ()> {
        self.expect_byte(b'[')?;
        let mut values = Vec::new();
        if self.peek_byte() == Some(b']') {
            self.position += 1;
            return Ok(values);
        }
        loop {
            if values.len() >= MAX_SEQUENCE {
                return Err(());
            }
            values.push(self.parse_text()?);
            match self.peek_byte() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(values);
                }
                _ => return Err(()),
            }
        }
    }

    fn parse_extensions(&mut self) -> Result<Vec<ParsedMetadataExtension>, ()> {
        self.expect_byte(b'[')?;
        let mut previous: Option<(String, String, Option<String>)> = None;
        let mut values = Vec::new();
        if self.peek_byte() == Some(b']') {
            self.position += 1;
            return Ok(values);
        }
        loop {
            if values.len() >= MAX_SEQUENCE {
                return Err(());
            }
            self.expect_byte(b'{')?;
            self.field(true, "schema")?;
            let schema = self.parse_text()?;
            self.field(false, "name")?;
            let name = self.parse_text()?;
            self.field(false, "version")?;
            let version = self.parse_optional_text()?;
            self.expect_byte(b'}')?;
            let current = (schema, name, version);
            if let Some(previous) = previous.as_ref()
                && extension_cmp(previous, &current) != Ordering::Less
            {
                return Err(());
            }
            values.push(ParsedMetadataExtension {
                schema: current.0.clone(),
                name: current.1.clone(),
                version: current.2.clone(),
            });
            previous = Some(current);
            match self.peek_byte() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(values);
                }
                _ => return Err(()),
            }
        }
    }

    fn parse_parameters(&mut self) -> Result<Vec<ParsedMetadataParameter>, ()> {
        self.expect_byte(b'[')?;
        let mut ordinal = 1u32;
        let mut values = Vec::new();
        if self.peek_byte() == Some(b']') {
            self.position += 1;
            return Ok(values);
        }
        loop {
            if values.len() >= MAX_SEQUENCE {
                return Err(());
            }
            self.expect_byte(b'{')?;
            self.field(true, "source_name")?;
            let source_name = self.parse_text()?;
            self.field(false, "protocol_ordinal")?;
            if self.parse_u32()? != ordinal {
                return Err(());
            }
            self.field(false, "logical_type")?;
            let logical_type = self.parse_text()?;
            self.field(false, "native_type")?;
            let native_type = self.parse_optional_text()?;
            self.field(false, "native_type_id")?;
            let native_type_id = self.parse_optional_i64()?;
            self.expect_byte(b'}')?;
            values.push(ParsedMetadataParameter {
                source_name,
                logical_type,
                checked: CheckedParameterMeta {
                    ordinal,
                    native_type,
                    native_type_id,
                },
            });
            ordinal = ordinal.checked_add(1).ok_or(())?;
            match self.peek_byte() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(values);
                }
                _ => return Err(()),
            }
        }
    }

    fn parse_columns(&mut self) -> Result<Vec<ParsedMetadataColumn>, ()> {
        self.expect_byte(b'[')?;
        let mut ordinal = 0u32;
        let mut values = Vec::new();
        if self.peek_byte() == Some(b']') {
            self.position += 1;
            return Ok(values);
        }
        loop {
            if values.len() >= MAX_SEQUENCE {
                return Err(());
            }
            self.expect_byte(b'{')?;
            self.field(true, "ordinal")?;
            if self.parse_u32()? != ordinal {
                return Err(());
            }
            self.field(false, "source_alias")?;
            let source_alias = self.parse_text()?;
            self.field(false, "logical_type")?;
            let logical_type = self.parse_text()?;
            self.field(false, "native_type")?;
            let native_type = self.parse_optional_text()?;
            self.field(false, "native_type_id")?;
            let native_type_id = self.parse_optional_i64()?;
            self.field(false, "nullable")?;
            let nullable = match self.parse_text()?.as_str() {
                "yes" => MetaNullability::Yes,
                "no" => MetaNullability::No,
                "unknown" => MetaNullability::Unknown,
                _ => return Err(()),
            };
            self.field(false, "origin_schema")?;
            let origin_schema = self.parse_optional_text()?;
            self.field(false, "origin_table")?;
            let origin_table = self.parse_optional_text()?;
            self.field(false, "origin_column")?;
            let origin_column = self.parse_optional_text()?;
            self.expect_byte(b'}')?;
            values.push(ParsedMetadataColumn {
                source_alias,
                logical_type,
                checked: CheckedColumnMeta {
                    ordinal,
                    native_type,
                    native_type_id,
                    origin_schema,
                    origin_table,
                    origin_column,
                    nullable,
                },
            });
            ordinal = ordinal.checked_add(1).ok_or(())?;
            match self.peek_byte() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(values);
                }
                _ => return Err(()),
            }
        }
    }

    fn parse_string(&mut self) -> Result<&'a [u8], ()> {
        self.expect_byte(b'"')?;
        let start = self.position;
        loop {
            let byte = *self.bytes.get(self.position).ok_or(())?;
            match byte {
                b'"' => {
                    let end = self.position;
                    self.position += 1;
                    return Ok(&self.bytes[start..end]);
                }
                b'\\' => {
                    self.position += 1;
                    match self.bytes.get(self.position).copied() {
                        Some(b'"' | b'\\' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.position += 1;
                        }
                        Some(b'u') => {
                            let start_escape = self.position;
                            let digits = self
                                .bytes
                                .get(start_escape + 1..start_escape + 5)
                                .ok_or(())?;
                            if digits.len() != 4
                                || !digits.iter().all(|digit| digit.is_ascii_hexdigit())
                                || digits.iter().any(|digit| digit.is_ascii_uppercase())
                            {
                                return Err(());
                            }
                            let value = digits.iter().try_fold(0u32, |value, digit| {
                                Some(value * 16 + hex_value(*digit)?)
                            });
                            let value = value.ok_or(())?;
                            if value > 0x1f || matches!(value, 0x08 | 0x09 | 0x0a | 0x0c | 0x0d) {
                                return Err(());
                            }
                            self.position += 5;
                        }
                        _ => return Err(()),
                    }
                }
                0x00..=0x1f => return Err(()),
                _ => self.position += 1,
            }
        }
    }

    fn parse_number(&mut self) -> Result<&'a [u8], ()> {
        let start = self.position;
        if self.peek_byte() == Some(b'-') {
            self.position += 1;
        }
        match self.peek_byte() {
            Some(b'0') => {
                self.position += 1;
                if self.peek_byte().is_some_and(|byte| byte.is_ascii_digit()) {
                    return Err(());
                }
            }
            Some(b'1'..=b'9') => {
                self.position += 1;
                while self.peek_byte().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.position += 1;
                }
            }
            _ => return Err(()),
        }
        if self
            .peek_byte()
            .is_some_and(|byte| matches!(byte, b'.' | b'e' | b'E' | b'+'))
        {
            return Err(());
        }
        let value = &self.bytes[start..self.position];
        if value == b"-0" {
            return Err(());
        }
        Ok(value)
    }

    fn expect_byte(&mut self, expected: u8) -> Result<(), ()> {
        if self.peek_byte() == Some(expected) {
            self.position += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    fn expect_bytes(&mut self, expected: &[u8]) -> Result<(), ()> {
        if self
            .bytes
            .get(self.position..self.position + expected.len())
            == Some(expected)
        {
            self.position += expected.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetadataStatementKind {
    Query,
    Command,
}

fn extension_cmp(
    left: &(String, String, Option<String>),
    right: &(String, String, Option<String>),
) -> Ordering {
    left.0
        .as_bytes()
        .cmp(right.0.as_bytes())
        .then_with(|| left.1.as_bytes().cmp(right.1.as_bytes()))
        .then_with(|| match (&left.2, &right.2) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(left), Some(right)) => left.as_bytes().cmp(right.as_bytes()),
        })
}

fn decode_canonical_json_string(raw: &[u8]) -> Result<String, ()> {
    let mut decoded = Vec::with_capacity(raw.len());
    let mut position = 0;
    while position < raw.len() {
        if raw[position] != b'\\' {
            let byte = raw[position];
            if byte < 0x20 {
                return Err(());
            }
            decoded.push(byte);
            position += 1;
            continue;
        }
        position += 1;
        match raw.get(position).copied() {
            Some(b'"') => decoded.push(b'"'),
            Some(b'\\') => decoded.push(b'\\'),
            Some(b'b') => decoded.push(0x08),
            Some(b'f') => decoded.push(0x0c),
            Some(b'n') => decoded.push(b'\n'),
            Some(b'r') => decoded.push(b'\r'),
            Some(b't') => decoded.push(b'\t'),
            Some(b'u') => {
                let digits = raw.get(position + 1..position + 5).ok_or(())?;
                let value = digits
                    .iter()
                    .try_fold(0u32, |value, digit| Some(value * 16 + hex_value(*digit)?));
                decoded.push(u8::try_from(value.ok_or(())?).map_err(|_| ())?);
                position += 4;
            }
            _ => return Err(()),
        }
        position += 1;
    }
    String::from_utf8(decoded).map_err(|_| ())
}

fn hex_value(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some(u32::from(byte - b'0')),
        b'a'..=b'f' => Some(u32::from(byte - b'a' + 10)),
        _ => None,
    }
}

fn read_bounded_file_inner(
    path: &Path,
    file: std::io::Result<fs::File>,
    too_large: &impl Fn() -> StaticInputError,
) -> Result<Vec<u8>, StaticInputError> {
    let file = file.map_err(|e| StaticInputError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let length = file
        .metadata()
        .map_err(|e| StaticInputError::Io {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?
        .len();
    if length > MAX_FIELD_BYTES as u64 {
        return Err(too_large());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(length).unwrap_or(MAX_FIELD_BYTES));
    file.take(MAX_FIELD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| StaticInputError::Io {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    if bytes.len() > MAX_FIELD_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

fn read_metadata_bytes(
    root: &Path,
    path: &Path,
    logical_path: &str,
) -> Result<Vec<u8>, StaticInputError> {
    align_watch::observe_consumed_classification(
        path,
        || read_metadata_bytes_inner(root, path, logical_path),
        |result| Some(static_observation_state(result)),
        |_| None,
        || {
            Err(StaticInputError::InvalidPath(
                "watch observation rejected metadata path".to_string(),
            ))
        },
    )
}

fn read_metadata_bytes_inner(
    root: &Path,
    path: &Path,
    logical_path: &str,
) -> Result<Vec<u8>, StaticInputError> {
    let canonical = fs::canonicalize(path).map_err(|e| StaticInputError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    ensure_inside(root, &canonical)?;
    read_bounded_file_inner(&canonical, fs::File::open(&canonical), &|| {
        StaticInputError::MetadataMalformed {
            logical_path: logical_path.to_string(),
        }
    })
}

fn ensure_metadata_parent_inside(root: &Path, path: &Path) -> Result<(), StaticInputError> {
    let mut current = path.parent();
    while let Some(candidate) = current {
        match fs::symlink_metadata(candidate) {
            Ok(_) => {
                let canonical = fs::canonicalize(candidate).map_err(|e| StaticInputError::Io {
                    path: candidate.to_path_buf(),
                    message: e.to_string(),
                })?;
                return ensure_inside(root, &canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                current = candidate.parent();
            }
            Err(error) => {
                return Err(StaticInputError::Io {
                    path: candidate.to_path_buf(),
                    message: error.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn validate_metadata_entry(
    entry: &CheckedMetadataInput,
    descriptor_id: &str,
) -> Result<(), StaticInputError> {
    let expected = metadata_logical_path(descriptor_id, entry.driver)?;
    if entry.logical_path != expected {
        return Err(StaticInputError::NonCanonical(format!(
            "metadata path `{}` does not equal `{expected}`",
            entry.logical_path
        )));
    }
    if let MetadataState::Present { format_version, .. } = entry.state
        && format_version == 0
    {
        return Err(StaticInputError::NonCanonical(
            "metadata format version is zero".to_string(),
        ));
    }
    Ok(())
}

fn validate_input(input: &StaticInput) -> Result<(), StaticInputError> {
    validate_descriptor_id(&input.descriptor_id)?;
    match &input.source {
        SqlSourceIdentity::File { logical_path } => {
            validate_text_field(logical_path, "file logical path")?;
            if logical_path.is_empty()
                || logical_path.starts_with('/')
                || logical_path.contains('\\')
            {
                return Err(StaticInputError::NonCanonical(
                    "file source is not root-relative".to_string(),
                ));
            }
            if logical_path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            {
                return Err(StaticInputError::NonCanonical(
                    "file source is not canonical".to_string(),
                ));
            }
        }
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => {
            validate_text_field(query_or_command_id, "inline descriptor id")?;
            if query_or_command_id != &input.descriptor_id {
                return Err(StaticInputError::NonCanonical(
                    "inline source id mismatch".to_string(),
                ));
            }
        }
    }
    if input.checked_metadata.len() > MAX_SEQUENCE {
        return Err(StaticInputError::NonCanonical(
            "too many checked metadata entries".to_string(),
        ));
    }
    let expected_drivers = input.driver_restriction.drivers();
    if input.checked_metadata.len() != expected_drivers.len() {
        return Err(StaticInputError::NonCanonical(
            "checked metadata does not cover every permitted driver".to_string(),
        ));
    }
    let mut previous_driver = None;
    for (entry, expected_driver) in input.checked_metadata.iter().zip(expected_drivers) {
        validate_text_field(&entry.logical_path, "metadata logical path")?;
        if entry.driver != *expected_driver {
            return Err(StaticInputError::NonCanonical(
                "checked metadata is not in permitted-driver order".to_string(),
            ));
        }
        validate_metadata_entry(entry, &input.descriptor_id)?;
        if let Some(previous) = previous_driver
            && entry.driver <= previous
        {
            return Err(StaticInputError::NonCanonical(
                "checked metadata is not sorted or contains a duplicate driver".to_string(),
            ));
        }
        previous_driver = Some(entry.driver);
    }
    Ok(())
}

fn input_cmp(left: &StaticInput, right: &StaticInput) -> Ordering {
    source_sort_key(&left.source)
        .cmp(&source_sort_key(&right.source))
        .then_with(|| left.consumer_kind.cmp(&right.consumer_kind))
        .then_with(|| {
            left.descriptor_id
                .as_bytes()
                .cmp(right.descriptor_id.as_bytes())
        })
}

fn reject_duplicate_descriptor_ids(inputs: &[StaticInput]) -> Result<(), StaticInputError> {
    let mut descriptor_ids: Vec<&str> = inputs
        .iter()
        .map(|input| input.descriptor_id.as_str())
        .collect();
    descriptor_ids.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    if descriptor_ids
        .windows(2)
        .any(|pair| pair[0].as_bytes() == pair[1].as_bytes())
    {
        return Err(StaticInputError::NonCanonical(
            "duplicate descriptor id".to_string(),
        ));
    }
    Ok(())
}

impl StaticInputManifest {
    pub fn new(
        resolution_digest: Hash128,
        mut inputs: Vec<StaticInput>,
    ) -> Result<Self, StaticInputError> {
        for input in &inputs {
            validate_input(input)?;
        }
        reject_duplicate_descriptor_ids(&inputs)?;
        inputs.sort_by(input_cmp);
        for pair in inputs.windows(2) {
            if input_cmp(&pair[0], &pair[1]) == Ordering::Equal {
                return Err(StaticInputError::NonCanonical(
                    "duplicate static input".to_string(),
                ));
            }
        }
        Ok(Self {
            resolution_digest,
            inputs,
        })
    }

    pub fn empty(resolution_digest: Hash128) -> Self {
        Self {
            resolution_digest,
            inputs: Vec::new(),
        }
    }

    fn validate_canonical(&self) -> Result<(), StaticInputError> {
        if self.inputs.len() > MAX_SEQUENCE {
            return Err(StaticInputError::NonCanonical(
                "too many static inputs".to_string(),
            ));
        }
        for input in &self.inputs {
            validate_input(input)?;
        }
        reject_duplicate_descriptor_ids(&self.inputs)?;
        for pair in self.inputs.windows(2) {
            if input_cmp(&pair[0], &pair[1]) != Ordering::Less {
                return Err(StaticInputError::NonCanonical(
                    "static inputs are not canonically sorted".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, StaticInputError> {
        self.validate_canonical()?;
        let mut writer = Writer::new();
        writer.bytes(&STATIC_INPUT_MANIFEST_MAGIC);
        writer.u32(STATIC_INPUT_MANIFEST_FORMAT_VERSION);
        writer.h128(self.resolution_digest);
        writer.seq_len(self.inputs.len())?;
        for input in &self.inputs {
            write_input(&mut writer, input)?;
        }
        Ok(writer.buf)
    }

    pub fn action_key(&self) -> Result<Hash128, StaticInputError> {
        Ok(Hash128::of(&self.canonical_bytes()?))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, StaticInputError> {
        let mut reader = Reader::new(bytes);
        if reader.bytes_exact(STATIC_INPUT_MANIFEST_MAGIC.len())? != STATIC_INPUT_MANIFEST_MAGIC {
            return Err(StaticInputError::BadMagic);
        }
        let version = reader.u32()?;
        if version != STATIC_INPUT_MANIFEST_FORMAT_VERSION {
            return Err(StaticInputError::UnknownVersion(version));
        }
        let resolution_digest = reader.h128()?;
        let inputs = reader.seq(|reader| read_input(reader))?;
        reader.finish()?;
        let manifest = Self {
            resolution_digest,
            inputs,
        };
        manifest.validate_canonical()?;
        Ok(manifest)
    }

    pub fn revalidate(&self, project_root: &Path) -> Result<(), StaticInputError> {
        self.validate_canonical()?;
        let root = canonical_root(project_root)?;
        for input in &self.inputs {
            if let Some(logical) = source_identity_path(&input.source) {
                let candidate = root.join(logical.replace('/', std::path::MAIN_SEPARATOR_STR));
                let bytes = match read_static_bytes(&root, &candidate, logical) {
                    Ok(bytes) => bytes,
                    Err(StaticInputError::MissingFile(_)) => {
                        return Err(StaticInputError::Stale(format!(
                            "file `{logical}` was deleted"
                        )));
                    }
                    Err(StaticInputError::InvalidUtf8 { .. })
                    | Err(StaticInputError::EmbeddedNul { .. }) => {
                        return Err(StaticInputError::Stale(format!(
                            "file `{logical}` is no longer valid text"
                        )));
                    }
                    Err(error) => return Err(error),
                };
                if Hash128::of(&bytes) != input.content_hash {
                    return Err(StaticInputError::HashMismatch {
                        logical_path: logical.to_string(),
                    });
                }
            }
            for metadata in &input.checked_metadata {
                revalidate_metadata(&root, input, metadata)?;
            }
        }
        Ok(())
    }
}

fn revalidate_metadata(
    root: &Path,
    input: &StaticInput,
    expected: &CheckedMetadataInput,
) -> Result<(), StaticInputError> {
    let path = root.join(
        expected
            .logical_path
            .replace('/', std::path::MAIN_SEPARATOR_STR),
    );
    let current = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {
            let bytes = read_metadata_bytes(root, &path, &expected.logical_path)?;
            let record = parse_checked_metadata(
                &bytes,
                &expected.logical_path,
                &input.descriptor_id,
                expected.driver,
            )?;
            Some((record.metadata_digest, record.format_version))
        }
        Ok(_) => return Err(StaticInputError::NotRegularFile(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure_metadata_parent_inside(root, &path)?;
            None
        }
        Err(error) => {
            return Err(StaticInputError::Io {
                path,
                message: error.to_string(),
            });
        }
    };
    let matches = match (&expected.state, current) {
        (MetadataState::Missing, None) => true,
        (MetadataState::Missing, Some(_)) => false,
        (
            MetadataState::Present {
                content_hash,
                format_version,
            },
            Some((actual_hash, actual_version)),
        ) => *content_hash == actual_hash && *format_version == actual_version,
        (MetadataState::Present { .. }, None) => false,
    };
    if matches {
        Ok(())
    } else {
        Err(StaticInputError::Stale(format!(
            "metadata `{}` for `{}` changed",
            expected.logical_path, input.descriptor_id
        )))
    }
}

/// Compose the existing per-unit MIR identity with the canonical static-input digest. This is the
/// one cache-facing helper: static-input registration does not introduce a parallel Query cache.
pub fn compose_codegen_impl_hash(impl_hash: Hash128, static_inputs_digest: Hash128) -> Hash128 {
    let mut bytes = Vec::with_capacity(8 + 16 + 16);
    bytes.extend_from_slice(b"ALIGNIMP");
    bytes.extend_from_slice(&impl_hash.lo.to_le_bytes());
    bytes.extend_from_slice(&impl_hash.hi.to_le_bytes());
    bytes.extend_from_slice(&static_inputs_digest.lo.to_le_bytes());
    bytes.extend_from_slice(&static_inputs_digest.hi.to_le_bytes());
    Hash128::of(&bytes)
}

fn write_input(writer: &mut Writer, input: &StaticInput) -> Result<(), StaticInputError> {
    writer.str(&input.descriptor_id)?;
    match &input.source {
        SqlSourceIdentity::File { logical_path } => {
            writer.u8(0);
            writer.str(logical_path)?;
        }
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => {
            writer.u8(1);
            writer.str(query_or_command_id)?;
        }
    }
    writer.h128(input.content_hash);
    writer.u8(input.consumer_kind as u8);
    writer.u8(input.driver_restriction as u8);
    writer.seq_len(input.checked_metadata.len())?;
    for entry in &input.checked_metadata {
        writer.u8(entry.driver as u8);
        writer.str(&entry.logical_path)?;
        match entry.state {
            MetadataState::Missing => writer.u8(0),
            MetadataState::Present {
                content_hash,
                format_version,
            } => {
                writer.u8(1);
                writer.h128(content_hash);
                writer.u32(format_version);
            }
        }
    }
    Ok(())
}

fn read_input(reader: &mut Reader<'_>) -> Result<StaticInput, StaticInputError> {
    let descriptor_id = reader.str()?;
    let source = match reader.u8()? {
        0 => SqlSourceIdentity::File {
            logical_path: reader.str()?,
        },
        1 => SqlSourceIdentity::Inline {
            query_or_command_id: reader.str()?,
        },
        tag => {
            return Err(StaticInputError::BadTag {
                what: "source",
                tag,
            });
        }
    };
    let content_hash = reader.h128()?;
    let consumer_kind = match reader.u8()? {
        0 => StaticConsumerKind::Query,
        1 => StaticConsumerKind::Command,
        tag => {
            return Err(StaticInputError::BadTag {
                what: "consumer kind",
                tag,
            });
        }
    };
    let driver_restriction = match reader.u8()? {
        0 => DriverRestriction::AnySupportedDriver,
        1 => DriverRestriction::SQLiteOnly,
        2 => DriverRestriction::PostgreSQLOnly,
        tag => {
            return Err(StaticInputError::BadTag {
                what: "driver restriction",
                tag,
            });
        }
    };
    let checked_metadata = reader.seq(|reader| {
        let driver = match reader.u8()? {
            0 => Driver::SQLite,
            1 => Driver::PostgreSQL,
            tag => {
                return Err(StaticInputError::BadTag {
                    what: "driver",
                    tag,
                });
            }
        };
        let logical_path = reader.str()?;
        let state = match reader.u8()? {
            0 => MetadataState::Missing,
            1 => MetadataState::Present {
                content_hash: reader.h128()?,
                format_version: reader.u32()?,
            },
            tag => {
                return Err(StaticInputError::BadTag {
                    what: "metadata state",
                    tag,
                });
            }
        };
        Ok(CheckedMetadataInput {
            driver,
            logical_path,
            state,
        })
    })?;
    Ok(StaticInput {
        descriptor_id,
        source,
        content_hash,
        consumer_kind,
        driver_restriction,
        checked_metadata,
    })
}

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    fn u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    fn h128(&mut self, value: Hash128) {
        self.buf.extend_from_slice(&value.lo.to_le_bytes());
        self.buf.extend_from_slice(&value.hi.to_le_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    fn str(&mut self, value: &str) -> Result<(), StaticInputError> {
        let bytes = value.as_bytes();
        if bytes.len() > MAX_FIELD_BYTES {
            return Err(StaticInputError::NonCanonical(
                "field exceeds the writer limit".to_string(),
            ));
        }
        let length = u32::try_from(bytes.len())
            .map_err(|_| StaticInputError::NonCanonical("field exceeds u32::MAX".to_string()))?;
        self.u32(length);
        self.bytes(bytes);
        Ok(())
    }

    fn seq_len(&mut self, length: usize) -> Result<(), StaticInputError> {
        let length = u32::try_from(length)
            .map_err(|_| StaticInputError::NonCanonical("sequence exceeds u32::MAX".to_string()))?;
        self.u32(length);
        Ok(())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], StaticInputError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(StaticInputError::Truncated)?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or(StaticInputError::Truncated)?;
        self.offset = end;
        Ok(result)
    }

    fn bytes_exact(&mut self, length: usize) -> Result<&'a [u8], StaticInputError> {
        self.take(length)
    }

    fn u8(&mut self) -> Result<u8, StaticInputError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, StaticInputError> {
        let bytes = self.take(4)?;
        let bytes: [u8; 4] = bytes.try_into().map_err(|_| StaticInputError::Truncated)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn h128(&mut self) -> Result<Hash128, StaticInputError> {
        let lo = self.u64()?;
        let hi = self.u64()?;
        Ok(Hash128 { lo, hi })
    }

    fn u64(&mut self) -> Result<u64, StaticInputError> {
        let bytes = self.take(8)?;
        let bytes: [u8; 8] = bytes.try_into().map_err(|_| StaticInputError::Truncated)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn str(&mut self) -> Result<String, StaticInputError> {
        let length = usize::try_from(self.u32()?).map_err(|_| StaticInputError::Truncated)?;
        if length > MAX_FIELD_BYTES {
            return Err(StaticInputError::NonCanonical(
                "field exceeds the reader limit".to_string(),
            ));
        }
        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| StaticInputError::BadUtf8)
    }

    fn seq<T>(
        &mut self,
        mut read: impl FnMut(&mut Reader<'a>) -> Result<T, StaticInputError>,
    ) -> Result<Vec<T>, StaticInputError> {
        let length = usize::try_from(self.u32()?).map_err(|_| StaticInputError::Truncated)?;
        if length > MAX_SEQUENCE {
            return Err(StaticInputError::NonCanonical(
                "sequence exceeds the reader limit".to_string(),
            ));
        }
        let mut values = Vec::with_capacity(length.min(1024));
        for _ in 0..length {
            values.push(read(self)?);
        }
        Ok(values)
    }

    fn finish(self) -> Result<(), StaticInputError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(StaticInputError::TrailingBytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};
    use std::ops::Deref;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRoot(PathBuf);

    impl Deref for TempRoot {
        type Target = Path;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_root(label: &str) -> TempRoot {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "align-static-input-{label}-{}-{nonce}",
            std::process::id()
        ));
        create_dir_all(&root).expect("temporary test root");
        TempRoot(root)
    }

    fn metadata(id: &str, driver: Driver, state: MetadataState) -> CheckedMetadataInput {
        CheckedMetadataInput {
            driver,
            logical_path: metadata_logical_path(id, driver).expect("metadata path"),
            state,
        }
    }

    #[test]
    fn metadata_publication_lock_closes_first_publish_and_overlap_races() {
        let root = temp_root("publication-lock");
        let legacy_reader = lock_metadata_publication_shared(&root).expect("legacy read guard");
        let first_writer =
            lock_metadata_publication_exclusive(&root).expect("first publication lock");
        assert!(matches!(
            legacy_reader.validate(),
            Err(StaticInputError::Stale(_))
        ));
        drop(legacy_reader);
        drop(first_writer);

        let writer = lock_metadata_publication_exclusive(&root).expect("publication writer");
        let lock_path = root.join(METADATA_PUBLICATION_LOCK);
        let reader_file = open_existing_publication_lock(&lock_path, false).expect("reader file");
        let error = reader_file
            .try_lock_shared()
            .expect_err("exclusive writer must exclude a reader");
        assert!(matches!(error, std::fs::TryLockError::WouldBlock));
        drop(writer);
        reader_file
            .lock_shared()
            .expect("reader proceeds after writer release");
    }

    fn descriptor(
        file: FileId,
        id: &str,
        source: StaticDescriptorSource,
        driver: StaticDescriptorDriver,
    ) -> StaticDescriptor {
        let (unit, item) = id.rsplit_once('.').expect("descriptor id");
        StaticDescriptor {
            unit: unit.to_string(),
            item: item.to_string(),
            descriptor_id: id.to_string(),
            is_public: true,
            consumer: StaticDescriptorConsumer::Query,
            driver,
            source,
            constructor_span: Span::new(file, 0, 1),
            common_options_span: Span::new(file, 0, 1),
            native_options_span: None,
            params_ty: align_sema::Ty::Unit,
            row_ty: Some(align_sema::Ty::Unit),
            params_contract: align_sema::StaticContract {
                root: align_sema::StaticContractType::Named {
                    path: "()".to_string(),
                    args: Vec::new(),
                },
                definitions: Vec::new(),
            },
            row_contract: Some(align_sema::StaticContract {
                root: align_sema::StaticContractType::Named {
                    path: "()".to_string(),
                    args: Vec::new(),
                },
                definitions: Vec::new(),
            }),
            static_options: Vec::new(),
        }
    }

    #[test]
    fn descriptor_batch_resolves_files_inline_sql_metadata_and_shared_source_identity() {
        let root = temp_root("descriptor-batch");
        let defining = root.join("q.align");
        let defining_source = "module q\n\"select 3\"\n\"select 5\"\n";
        write(&defining, defining_source).expect("defining source");
        write(root.join("q.sql"), "select 1\n").expect("sibling SQL");
        create_dir_all(root.join("sql")).expect("SQL directory");
        write(root.join("sql/explicit.sql"), "select 4\n").expect("explicit SQL");
        let mut source_map = SourceMap::new();
        let file = source_map.add_file(defining.display().to_string(), defining_source.to_string());
        let descriptors = vec![
            descriptor(
                file,
                "q.first",
                StaticDescriptorSource::File {
                    path_literal: None,
                    path_span: None,
                },
                StaticDescriptorDriver::AnySupportedDriver,
            ),
            descriptor(
                file,
                "q.second",
                StaticDescriptorSource::File {
                    path_literal: None,
                    path_span: None,
                },
                StaticDescriptorDriver::AnySupportedDriver,
            ),
            descriptor(
                file,
                "q.third",
                StaticDescriptorSource::Inline {
                    decoded_sql: "select 3".to_string(),
                    literal_span: Span::new(file, 9, 19),
                },
                StaticDescriptorDriver::SQLiteOnly,
            ),
            descriptor(
                file,
                "q.fourth",
                StaticDescriptorSource::File {
                    path_literal: Some("sql/explicit.sql".to_string()),
                    path_span: Some(Span::new(file, 0, 1)),
                },
                StaticDescriptorDriver::SQLiteOnly,
            ),
            descriptor(
                file,
                "q.fifth",
                StaticDescriptorSource::Inline {
                    decoded_sql: "select 5".to_string(),
                    literal_span: Span::new(file, 20, 30),
                },
                StaticDescriptorDriver::PostgreSQLOnly,
            ),
        ];

        let resolved = resolve_static_descriptors(
            &root,
            &mut source_map,
            &descriptors,
            Hash128 { lo: 7, hi: 9 },
        )
        .expect("resolved descriptor batch");
        assert_eq!(resolved.inputs.len(), 5);
        assert_eq!(resolved.manifest.inputs.len(), 5);
        assert_eq!(
            source_map.files().len(),
            3,
            "the shared SQL file and explicit file are each registered once"
        );
        assert_eq!(
            resolved.inputs[0].source_map_file,
            resolved.inputs[1].source_map_file
        );
        assert!(resolved.inputs[0].source_map_file.is_some());
        assert_eq!(resolved.inputs[2].source_map_file, None);
        assert!(resolved.inputs[3].source_map_file.is_some());
        assert_eq!(resolved.inputs[4].source_map_file, None);
        assert_eq!(resolved.inputs[0].input.checked_metadata.len(), 2);
        assert_eq!(resolved.inputs[1].input.checked_metadata.len(), 2);
        assert_eq!(resolved.inputs[2].input.checked_metadata.len(), 1);
        assert_eq!(resolved.inputs[3].input.checked_metadata.len(), 1);
        assert_eq!(resolved.inputs[4].input.checked_metadata.len(), 1);
        assert_eq!(
            resolved.inputs[4].input.checked_metadata[0].driver,
            Driver::PostgreSQL
        );
        assert_eq!(
            resolved
                .manifest
                .inputs
                .iter()
                .map(|input| input.descriptor_id.as_str())
                .collect::<Vec<_>>(),
            ["q.first", "q.second", "q.fourth", "q.fifth", "q.third"]
        );
    }

    #[test]
    fn descriptor_batch_failure_does_not_publish_partial_source_map_entries() {
        let root = temp_root("descriptor-rollback");
        let valid = root.join("valid.align");
        write(&valid, "module valid\n").expect("valid source");
        write(root.join("valid.sql"), "select 1\n").expect("valid SQL");
        let missing = root.join("missing.align");
        let mut source_map = SourceMap::new();
        let valid_file = source_map.add_file(valid.display().to_string(), String::new());
        let missing_file = source_map.add_file(missing.display().to_string(), String::new());
        let descriptors = vec![
            descriptor(
                valid_file,
                "valid.query",
                StaticDescriptorSource::File {
                    path_literal: None,
                    path_span: None,
                },
                StaticDescriptorDriver::SQLiteOnly,
            ),
            descriptor(
                missing_file,
                "missing.query",
                StaticDescriptorSource::File {
                    path_literal: None,
                    path_span: None,
                },
                StaticDescriptorDriver::SQLiteOnly,
            ),
        ];
        let files_before = source_map.files().len();
        let error = resolve_static_descriptors(
            &root,
            &mut source_map,
            &descriptors,
            Hash128 { lo: 1, hi: 2 },
        )
        .expect_err("missing defining source must fail");
        assert_eq!(error.descriptor_id, "missing.query");
        assert_eq!(source_map.files().len(), files_before);

        let duplicate = descriptor(
            valid_file,
            "valid.query",
            StaticDescriptorSource::File {
                path_literal: None,
                path_span: None,
            },
            StaticDescriptorDriver::SQLiteOnly,
        );
        let duplicate_span = duplicate.constructor_span;
        let error = resolve_static_descriptors(
            &root,
            &mut source_map,
            &[duplicate.clone(), duplicate],
            Hash128 { lo: 1, hi: 2 },
        )
        .expect_err("duplicate descriptor ids must fail before publication");
        assert_eq!(error.descriptor_id, "valid.query");
        assert_eq!(error.span, duplicate_span);
        assert_eq!(source_map.files().len(), files_before);

        let invalid_id = descriptor(
            u32::MAX,
            "bad.\0query",
            StaticDescriptorSource::File {
                path_literal: None,
                path_span: None,
            },
            StaticDescriptorDriver::SQLiteOnly,
        );
        let invalid_span = invalid_id.constructor_span;
        let error = resolve_static_descriptors(
            &root,
            &mut source_map,
            &[invalid_id],
            Hash128 { lo: 1, hi: 2 },
        )
        .expect_err("descriptor identity is validated before SourceMap lookup");
        assert_eq!(error.descriptor_id, "bad.\0query");
        assert_eq!(error.span, invalid_span);
        assert!(matches!(
            error.cause,
            StaticDescriptorInputErrorCause::Input(StaticInputError::InvalidDescriptorId)
        ));
        assert_eq!(source_map.files().len(), files_before);
    }

    #[test]
    fn shared_file_snapshot_rejects_bytes_changed_between_descriptor_reads() {
        let root = temp_root("descriptor-snapshot-change");
        let defining = root.join("q.align");
        write(&defining, "module q\n").expect("defining source");
        write(root.join("q.sql"), "select 1\n").expect("first SQL snapshot");
        let first = resolve_static_file(
            &root,
            &defining,
            None,
            "q.first",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
            None,
        )
        .expect("first file snapshot");
        write(root.join("q.sql"), "select 2\n").expect("replace SQL snapshot");
        let second = resolve_static_file(
            &root,
            &defining,
            None,
            "q.second",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
            None,
        )
        .expect("second file snapshot");

        let mut resolved = Vec::new();
        let mut snapshots = HashMap::new();
        record_file_snapshot(&resolved, &mut snapshots, &first).expect("first snapshot owner");
        resolved.push(first);
        assert_eq!(
            record_file_snapshot(&resolved, &mut snapshots, &second),
            Err(StaticInputError::HashMismatch {
                logical_path: "q.sql".to_string(),
            })
        );
    }

    #[test]
    fn missing_static_source_retains_its_lexical_watch_identity() {
        let root = temp_root("missing-static-observation");
        let defining = root.join("q.align");
        write(&defining, "module q\n").expect("defining source");
        let candidate = root.join("q.sql");
        let (result, inputs) = align_watch::collect_observations(|| {
            resolve_static_file(
                &root,
                &defining,
                None,
                "q.missing",
                StaticConsumerKind::Query,
                DriverRestriction::SQLiteOnly,
                None,
            )
        });
        assert!(matches!(result, Err(StaticInputError::MissingFile(_))));
        let inputs = inputs.expect("watch observations");
        assert_eq!(inputs.inputs().len(), 1);
        assert_eq!(inputs.inputs()[0].path(), candidate);
        assert!(matches!(
            inputs.inputs()[0].state(),
            align_watch::BuildInputState::Missing
        ));
    }

    #[cfg(unix)]
    #[test]
    fn raw_defining_path_resolves_static_siblings_without_lossy_source_names() {
        use std::os::unix::ffi::OsStringExt;

        let outer = temp_root("raw-defining-path");
        let root = outer.join(std::ffi::OsString::from_vec(b"project-\xff".to_vec()));
        create_dir_all(&root).expect("raw project root");
        let defining = root.join("q.align");
        write(&defining, "module q\n").expect("defining source");
        write(root.join("q.sql"), "select 1\n").expect("static sibling");
        let mut source_map = SourceMap::new();
        let file = source_map.add_file(defining.to_string_lossy(), "module q\n".to_string());
        let descriptors = [descriptor(
            file,
            "q.raw",
            StaticDescriptorSource::File {
                path_literal: None,
                path_span: None,
            },
            StaticDescriptorDriver::SQLiteOnly,
        )];
        let defining_paths = HashMap::from([(file, defining)]);
        let resolved = resolve_static_descriptors_at(
            &root,
            &mut source_map,
            &descriptors,
            Hash128 { lo: 1, hi: 2 },
            &defining_paths,
        )
        .expect("resolve through raw defining path");
        assert_eq!(resolved.inputs[0].bytes, b"select 1\n");
    }

    #[test]
    fn whole_program_and_per_unit_checks_publish_the_same_producer_descriptor() {
        let root = temp_root("descriptor-parity");
        create_dir_all(root.join("pkg")).expect("pkg directory");
        write(
            root.join("pkg/db.align"),
            concat!(
                "module pkg.db\n",
                "import std.process\n",
                "pub query<P, R> {}\n",
                "pub Driver {\n  SQLite\n  PostgreSQL\n}\n",
                "pub DriverRestriction {\n  AnySupportedDriver\n  SQLiteOnly\n  PostgreSQLOnly\n}\n",
                "pub MetaNullability {\n  Yes\n  No\n  Unknown\n}\n",
                "pub MetaQueryState {\n  Declared\n  DatabaseChecked\n}\n",
                "pub MetaQueryEntry {\n  Summary\n  Parameter\n  Column\n}\n",
                "pub MetaStatementClass {\n  Select\n  Dml\n  Ddl\n  Native\n  Unknown\n}\n",
                "pub QueryMeta {\n",
                "  query_id: str,\n",
                "  driver: Driver,\n",
                "  driver_restriction: DriverRestriction,\n",
                "  statement_class: MetaStatementClass,\n",
                "  artifact_digest: str,\n",
                "  state: MetaQueryState,\n",
                "  metadata_fingerprint: Option<str>,\n",
                "  source_sql_hash: str,\n",
                "  driver_wire_sql_hash: str,\n",
                "  rewrite_format_version: i64,\n",
                "  prepare_identity: Option<str>,\n",
                "  schema_identity: Option<str>,\n",
                "  server_identity: Option<str>,\n",
                "  entry: MetaQueryEntry,\n",
                "  ordinal: Option<i64>,\n",
                "  source_name: Option<str>,\n",
                "  source_alias: Option<str>,\n",
                "  logical_type: Option<str>,\n",
                "  native_type: Option<str>,\n",
                "  native_type_id: Option<i64>,\n",
                "  origin_schema: Option<str>,\n",
                "  origin_table: Option<str>,\n",
                "  origin_column: Option<str>,\n",
                "  nullable: MetaNullability,\n",
                "}\n",
                "pub QueryOption { Check(i64) }\n",
                "pub fn query_file<P, R>(options: slice<QueryOption>) -> query<P, R> = process.abort()\n",
            ),
        )
        .expect("pkg.db source");
        write(
            root.join("queries.align"),
            concat!(
                "module queries\n",
                "import pkg.db\n",
                "pub Params { id: i64 }\n",
                "pub Row { name: str }\n",
                "pub fn users() -> pkg.db.query<Params, Row> = pkg.db.query_file([])\n",
            ),
        )
        .expect("query source");
        write(root.join("queries.sql"), "SELECT :id AS name\n").expect("query SQL");
        let entry = "module main\nimport queries\nfn main() -> i32 = 0\n";
        let entry_path = root.join("main.align");

        let mut whole_sources = SourceMap::new();
        let whole = crate::check(
            &mut whole_sources,
            entry_path.to_str().expect("entry path"),
            entry,
        );
        assert!(
            !whole.diags.has_errors(),
            "whole-program diagnostics: {:?}",
            whole.diags.iter().collect::<Vec<_>>()
        );
        assert_eq!(whole.static_descriptors.len(), 1);

        let mut per_unit_sources = SourceMap::new();
        let per_unit = crate::build_per_unit(
            &mut per_unit_sources,
            entry_path.to_str().expect("entry path"),
            entry,
        );
        assert!(
            !per_unit.diags.has_errors(),
            "per-unit diagnostics: {:?}",
            per_unit.diags.iter().collect::<Vec<_>>()
        );
        let query_unit = per_unit
            .units
            .iter()
            .find(|unit| unit.unit == "queries")
            .expect("query producer unit");
        assert_eq!(query_unit.static_descriptors.len(), 1);
        let whole_descriptor = &whole.static_descriptors[0];
        let per_unit_descriptor = &query_unit.static_descriptors[0];
        assert_eq!(
            per_unit_descriptor.descriptor_id,
            whole_descriptor.descriptor_id
        );
        assert_eq!(per_unit_descriptor.source, whole_descriptor.source);
        assert_eq!(per_unit_descriptor.driver, whole_descriptor.driver);
        assert!(
            per_unit
                .units
                .iter()
                .filter(|unit| unit.unit != "queries")
                .all(|unit| unit.static_descriptors.is_empty())
        );
    }

    fn metadata_json(format_version: u32, source_sql_hash: &str) -> Vec<u8> {
        let mut bytes = format!(
            "{{\"format_version\":{format_version},\"descriptor_id\":\"q.query\",\"module\":\"q\",\"item\":\"query\",\"driver\":\"sqlite\",\"driver_restriction\":\"sqlite_only\",\"statement_kind\":\"query\",\"statement_class\":\"select\",\"source_identity\":{{\"kind\":\"inline\",\"descriptor_id\":\"q.query\"}},\"source_sql_hash\":\"{source_sql_hash}\",\"wire_sql_hash\":\"00000000000000000000000000000000\",\"rewrite_format_version\":1,\"static_options_hash\":\"00000000000000000000000000000000\",\"params_fingerprint\":\"00000000000000000000000000000000\",\"row_fingerprint\":\"00000000000000000000000000000000\",\"schema_fingerprint\":\"00000000000000000000000000000000\",\"engine_version\":\"sqlite\",\"driver_version\":\"sqlite\",\"search_path\":[],\"extensions\":[],\"parameters\":[],\"columns\":[]}}"
        )
        .into_bytes();
        bytes.push(b'\n');
        bytes
    }

    fn replace_once(bytes: &mut Vec<u8>, old: &[u8], new: &[u8]) {
        let start = bytes
            .windows(old.len())
            .position(|window| window == old)
            .expect("metadata test marker");
        bytes.splice(start..start + old.len(), new.iter().copied());
    }

    fn reference_manifest_bytes(manifest: &StaticInputManifest) -> Vec<u8> {
        fn u8(bytes: &mut Vec<u8>, value: u8) {
            bytes.push(value);
        }
        fn u32(bytes: &mut Vec<u8>, value: u32) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        fn hash(bytes: &mut Vec<u8>, value: Hash128) {
            bytes.extend_from_slice(&value.lo.to_le_bytes());
            bytes.extend_from_slice(&value.hi.to_le_bytes());
        }
        fn string(bytes: &mut Vec<u8>, value: &str) {
            u32(
                bytes,
                u32::try_from(value.len()).expect("reference field length"),
            );
            bytes.extend_from_slice(value.as_bytes());
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&STATIC_INPUT_MANIFEST_MAGIC);
        u32(&mut bytes, STATIC_INPUT_MANIFEST_FORMAT_VERSION);
        hash(&mut bytes, manifest.resolution_digest);
        u32(
            &mut bytes,
            u32::try_from(manifest.inputs.len()).expect("reference input count"),
        );
        for input in &manifest.inputs {
            string(&mut bytes, &input.descriptor_id);
            match &input.source {
                SqlSourceIdentity::File { logical_path } => {
                    u8(&mut bytes, 0);
                    string(&mut bytes, logical_path);
                }
                SqlSourceIdentity::Inline {
                    query_or_command_id,
                } => {
                    u8(&mut bytes, 1);
                    string(&mut bytes, query_or_command_id);
                }
            }
            hash(&mut bytes, input.content_hash);
            u8(&mut bytes, input.consumer_kind as u8);
            u8(&mut bytes, input.driver_restriction as u8);
            u32(
                &mut bytes,
                u32::try_from(input.checked_metadata.len()).expect("reference metadata count"),
            );
            for entry in &input.checked_metadata {
                u8(&mut bytes, entry.driver as u8);
                string(&mut bytes, &entry.logical_path);
                match entry.state {
                    MetadataState::Missing => u8(&mut bytes, 0),
                    MetadataState::Present {
                        content_hash,
                        format_version,
                    } => {
                        u8(&mut bytes, 1);
                        hash(&mut bytes, content_hash);
                        u32(&mut bytes, format_version);
                    }
                }
            }
        }
        bytes
    }

    #[test]
    fn resolves_sibling_and_registers_root_relative_source() {
        let root = temp_root("sibling");
        let module = root.join("queries/user.align");
        create_dir_all(module.parent().expect("module parent")).expect("query directory");
        write(&module, "module queries.user\n").expect("align source");
        write(module.with_extension("sql"), "select $id\n").expect("sql source");
        let mut source_map = SourceMap::new();
        let resolved = resolve_static_file(
            &root,
            &module,
            None,
            "queries.user.query",
            StaticConsumerKind::Query,
            DriverRestriction::AnySupportedDriver,
            Some(&mut source_map),
        )
        .expect("sibling SQL");
        assert_eq!(
            resolved.input.source,
            SqlSourceIdentity::File {
                logical_path: "queries/user.sql".into()
            }
        );
        assert_eq!(resolved.input.content_hash, Hash128::of(b"select $id\n"));
        let file_id = resolved.source_map_file.expect("SourceMap registration");
        assert_eq!(source_map.get(file_id).name, "queries/user.sql");
        assert_eq!(source_map.get(file_id).src, "select $id\n");
    }

    #[cfg(unix)]
    #[test]
    fn in_root_defining_symlink_uses_lexical_module_sibling() {
        let root = temp_root("defining-link");
        let alias_dir = root.join("alias");
        let target_dir = root.join("src");
        create_dir_all(&alias_dir).expect("alias directory");
        create_dir_all(&target_dir).expect("target directory");
        let target_align = target_dir.join("query.align");
        let alias_align = alias_dir.join("query.align");
        write(&target_align, "module src.query\n").expect("target align source");
        write(alias_dir.join("query.sql"), "select lexical").expect("lexical SQL source");
        write(target_dir.join("query.sql"), "select target").expect("target SQL source");
        std::os::unix::fs::symlink(&target_align, &alias_align).expect("defining symlink");

        let resolved = resolve_static_file(
            &root,
            &alias_align,
            None,
            "src.query.query",
            StaticConsumerKind::Query,
            DriverRestriction::AnySupportedDriver,
            None,
        )
        .expect("lexical sibling SQL");
        assert_eq!(resolved.bytes, b"select lexical");
        assert_eq!(
            resolved.input.source,
            SqlSourceIdentity::File {
                logical_path: "alias/query.sql".into()
            }
        );
        assert_eq!(resolved.resolved_path, Some(alias_dir.join("query.sql")));
    }

    #[test]
    fn explicit_path_rejects_root_escape_and_symlink_escape() {
        let root = temp_root("escape");
        let module = root.join("queries/user.align");
        create_dir_all(module.parent().expect("module parent")).expect("query directory");
        write(&module, "module queries.user\n").expect("align source");
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("../secret.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::InvalidPath(_))
        ));
        let outside_root = temp_root("outside");
        let outside = outside_root.join("secret.sql");
        write(&outside, "secret").expect("outside SQL");
        let link = root.join("queries/link.sql");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");
        #[cfg(unix)]
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("link.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::OutsideProjectRoot(_))
        ));
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("/tmp/absolute.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::InvalidPath(_))
        ));
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("bad\0.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::InvalidPath(_))
        ));
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("bad\\name.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::InvalidPath(_))
        ));
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("missing.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::MissingFile(_))
        ));
        create_dir_all(root.join("queries/directory.sql")).expect("directory input");
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("directory.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::NotRegularFile(_))
        ));
    }

    #[test]
    fn invalid_text_reports_utf8_and_first_nul() {
        let root = temp_root("text");
        let module = root.join("q.align");
        write(&module, "module q\n").expect("align source");
        write(root.join("q.sql"), [0xff, 0xfe]).expect("invalid bytes");
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                None,
                "q.query",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::InvalidUtf8 { .. })
        ));
        write(root.join("q.sql"), b"ok\0bad").expect("NUL bytes");
        assert_eq!(
            resolve_static_file(
                &root,
                &module,
                None,
                "q.query",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::EmbeddedNul {
                logical_path: "q.sql".into(),
                offset: 2
            })
        );

        write(root.join("q.sql"), "select 1").expect("valid SQL bytes");
        let mut source_map = SourceMap::new();
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                None,
                "bad\0descriptor",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                Some(&mut source_map),
            ),
            Err(StaticInputError::InvalidDescriptorId)
        ));
        assert!(source_map.files().is_empty());
    }

    #[test]
    fn oversized_static_file_is_rejected_before_reading_contents() {
        let root = temp_root("oversized-sql");
        let module = root.join("q.align");
        write(&module, "module q\n").expect("align source");
        let sql = root.join("q.sql");
        let file = std::fs::File::create(&sql).expect("oversized SQL");
        file.set_len((MAX_FIELD_BYTES as u64) + 1)
            .expect("SQL length");
        drop(file);
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                None,
                "q.query",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None,
            ),
            Err(StaticInputError::NonCanonical(_))
        ));
    }

    #[test]
    fn oversized_inline_sql_is_rejected_before_buffering() {
        let sql = "x".repeat(MAX_FIELD_BYTES + 1);
        assert!(matches!(
            resolve_inline_static_input(
                "q.query",
                &sql,
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
            ),
            Err(StaticInputError::NonCanonical(_))
        ));
    }

    #[test]
    fn inline_does_not_resolve_a_file_and_identity_is_descriptor_bound() {
        let resolved = resolve_inline_static_input(
            "q.query",
            "select 1",
            StaticConsumerKind::Query,
            DriverRestriction::AnySupportedDriver,
        )
        .expect("inline source");
        assert_eq!(resolved.resolved_path, None);
        assert_eq!(resolved.source_map_file, None);
        assert_eq!(
            resolved.input.source,
            SqlSourceIdentity::Inline {
                query_or_command_id: "q.query".into()
            }
        );
        assert_eq!(resolved.input.content_hash, Hash128::of(b"select 1"));
        assert!(matches!(
            resolve_inline_static_input(
                "q.query",
                "bad\0sql",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
            ),
            Err(StaticInputError::EmbeddedNul { offset: 3, .. })
        ));
    }

    #[test]
    fn metadata_snapshot_and_revalidation_track_missing_present_and_change() {
        let root = temp_root("metadata");
        let id = "q.query";
        let input = resolve_inline_static_input(
            id,
            "select 1",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
        )
        .expect("inline input")
        .input;
        let missing = snapshot_checked_metadata(&root, id, Driver::SQLite).expect("missing state");
        assert_eq!(missing.state, MetadataState::Missing);
        let manifest = StaticInputManifest::new(
            Hash128 { lo: 1, hi: 2 },
            vec![StaticInput {
                checked_metadata: vec![missing],
                ..input.clone()
            }],
        )
        .expect("missing manifest");
        manifest.revalidate(&root).expect("missing remains valid");
        let path = metadata_path(&root, id, Driver::SQLite).expect("metadata path");
        create_dir_all(path.parent().expect("metadata directory")).expect("metadata directory");
        write(&path, metadata_json(1, "00000000000000000000000000000000")).expect("metadata");
        assert!(matches!(
            manifest.revalidate(&root),
            Err(StaticInputError::Stale(_))
        ));
        let present = snapshot_checked_metadata(&root, id, Driver::SQLite).expect("present state");
        let present_manifest = StaticInputManifest::new(
            Hash128 { lo: 1, hi: 2 },
            vec![StaticInput {
                checked_metadata: vec![present],
                ..input
            }],
        )
        .expect("present manifest");
        present_manifest
            .revalidate(&root)
            .expect("present remains valid");
        write(&path, metadata_json(2, "00000000000000000000000000000000")).expect("metadata edit");
        assert!(matches!(
            present_manifest.revalidate(&root),
            Err(StaticInputError::MetadataMalformed { .. })
        ));

        write(&path, metadata_json(1, "10000000000000000000000000000000"))
            .expect("changed metadata");
        assert!(matches!(
            present_manifest.revalidate(&root),
            Err(StaticInputError::Stale(_))
        ));
    }

    #[test]
    fn metadata_parser_consumes_complete_canonical_v1_record() {
        let root = temp_root("metadata-malformed");
        let id = "q.query";
        let path = metadata_path(&root, id, Driver::SQLite).expect("metadata path");
        create_dir_all(path.parent().expect("metadata directory")).expect("metadata directory");

        let mut malformed = metadata_json(1, "00000000000000000000000000000000");
        malformed.extend_from_slice(b"garbage");
        write(&path, malformed).expect("malformed metadata");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::MetadataMalformed { .. })
        ));

        write(&path, metadata_json(2, "00000000000000000000000000000000")).expect("v2 metadata");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::MetadataMalformed { .. })
        ));

        let mut mismatched_identity = metadata_json(1, "00000000000000000000000000000000");
        let marker = b"q.query";
        let offset = mismatched_identity
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("descriptor identity marker");
        mismatched_identity[offset] = b'x';
        write(&path, mismatched_identity).expect("mismatched metadata identity");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::MetadataMalformed { .. })
        ));

        let oversized = std::fs::File::create(&path).expect("oversized metadata");
        oversized
            .set_len((MAX_FIELD_BYTES as u64) + 1)
            .expect("metadata length");
        drop(oversized);
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::MetadataMalformed { .. })
        ));
    }

    #[test]
    fn metadata_parser_rejects_malformed_nested_records() {
        let root = temp_root("metadata-nested-malformed");
        let id = "q.query";
        let path = metadata_path(&root, id, Driver::SQLite).expect("metadata path");
        create_dir_all(path.parent().expect("metadata directory")).expect("metadata directory");

        let mut source_identity = metadata_json(1, "00000000000000000000000000000000");
        replace_once(
            &mut source_identity,
            b"\"source_identity\":{\"kind\":\"inline\",\"descriptor_id\":\"q.query\"}",
            b"\"source_identity\":{}",
        );
        write(&path, source_identity).expect("malformed source identity");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::MetadataMalformed { .. })
        ));

        let mut search_path = metadata_json(1, "00000000000000000000000000000000");
        replace_once(
            &mut search_path,
            b"\"search_path\":[]",
            b"\"search_path\":[0]",
        );
        write(&path, search_path).expect("malformed search path");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::MetadataMalformed { .. })
        ));

        let mut parameters = metadata_json(1, "00000000000000000000000000000000");
        replace_once(&mut parameters, b"\"parameters\":[]", b"\"parameters\":[0]");
        write(&path, parameters).expect("malformed parameters");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::MetadataMalformed { .. })
        ));

        let mut columns = metadata_json(1, "00000000000000000000000000000000");
        replace_once(&mut columns, b"\"columns\":[]", b"\"columns\":[0]");
        write(&path, columns).expect("malformed columns");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::MetadataMalformed { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn metadata_parent_symlink_cannot_escape_project_root() {
        let root = temp_root("metadata-link");
        let outside_root = temp_root("metadata-link-outside");
        let id = "q.query";
        let outside_path = metadata_path(&outside_root, id, Driver::SQLite).expect("metadata path");
        create_dir_all(outside_path.parent().expect("metadata directory"))
            .expect("metadata directory");
        write(
            &outside_path,
            metadata_json(1, "00000000000000000000000000000000"),
        )
        .expect("outside metadata");
        std::os::unix::fs::symlink(outside_root.join(".align-db"), root.join(".align-db"))
            .expect("metadata directory symlink");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::OutsideProjectRoot(_))
        ));

        let missing_root = temp_root("metadata-link-missing");
        let missing_outside = temp_root("metadata-link-missing-outside");
        create_dir_all(missing_outside.join(".align-db")).expect("outside metadata directory");
        std::os::unix::fs::symlink(
            missing_outside.join(".align-db"),
            missing_root.join(".align-db"),
        )
        .expect("missing metadata directory symlink");
        assert!(matches!(
            snapshot_checked_metadata(&missing_root, id, Driver::SQLite),
            Err(StaticInputError::OutsideProjectRoot(_))
        ));
        let input = resolve_inline_static_input(
            id,
            "select 1",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
        )
        .expect("missing metadata input")
        .input;
        let manifest = StaticInputManifest::new(
            Hash128 { lo: 3, hi: 4 },
            vec![StaticInput {
                checked_metadata: vec![metadata(id, Driver::SQLite, MetadataState::Missing)],
                ..input
            }],
        )
        .expect("missing metadata manifest");
        assert!(matches!(
            manifest.revalidate(&missing_root),
            Err(StaticInputError::OutsideProjectRoot(_))
        ));
    }

    #[test]
    fn manifest_codec_is_canonical_and_fail_closed() {
        let mut input = resolve_inline_static_input(
            "z.command",
            "delete",
            StaticConsumerKind::Command,
            DriverRestriction::SQLiteOnly,
        )
        .expect("command input")
        .input;
        input.checked_metadata = vec![metadata(
            "z.command",
            Driver::SQLite,
            MetadataState::Missing,
        )];
        let mut other = resolve_inline_static_input(
            "a.query",
            "select",
            StaticConsumerKind::Query,
            DriverRestriction::PostgreSQLOnly,
        )
        .expect("query input")
        .input;
        other.checked_metadata = vec![metadata(
            "a.query",
            Driver::PostgreSQL,
            MetadataState::Missing,
        )];
        let manifest = StaticInputManifest::new(Hash128 { lo: 7, hi: 8 }, vec![input, other])
            .expect("manifest");
        let bytes = manifest.canonical_bytes().expect("canonical bytes");
        let reference = reference_manifest_bytes(&manifest);
        assert_eq!(bytes, reference);
        assert_eq!(
            StaticInputManifest::decode(&bytes).expect("decode"),
            manifest
        );
        assert_eq!(
            StaticInputManifest::decode(&reference).expect("reference decode"),
            manifest
        );
        assert_eq!(manifest.action_key().expect("digest"), Hash128::of(&bytes));
        let mut bad_magic = bytes.clone();
        bad_magic[0] ^= 1;
        assert_eq!(
            StaticInputManifest::decode(&bad_magic),
            Err(StaticInputError::BadMagic)
        );
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(
            StaticInputManifest::decode(&trailing),
            Err(StaticInputError::TrailingBytes)
        );
        let truncated = &bytes[..bytes.len() - 1];
        assert_eq!(
            StaticInputManifest::decode(truncated),
            Err(StaticInputError::Truncated)
        );
        let mut huge_count = bytes.clone();
        huge_count[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            StaticInputManifest::decode(&huge_count),
            Err(StaticInputError::NonCanonical(_))
        ));
        let descriptor_len =
            u32::from_le_bytes(bytes[32..36].try_into().expect("descriptor length bytes"));
        let source_tag = 36 + usize::try_from(descriptor_len).expect("descriptor length");
        let mut bad_tag = bytes.clone();
        bad_tag[source_tag] = 9;
        assert!(matches!(
            StaticInputManifest::decode(&bad_tag),
            Err(StaticInputError::BadTag {
                what: "source",
                tag: 9
            })
        ));
        let mut bad_utf8 = bytes.clone();
        bad_utf8[source_tag + 5] = 0xff;
        assert_eq!(
            StaticInputManifest::decode(&bad_utf8),
            Err(StaticInputError::BadUtf8)
        );
        let mut duplicate = manifest.clone();
        duplicate.inputs = vec![manifest.inputs[0].clone(), manifest.inputs[0].clone()];
        assert!(matches!(
            StaticInputManifest::decode(&reference_manifest_bytes(&duplicate)),
            Err(StaticInputError::NonCanonical(_))
        ));
        let mut bad_metadata = manifest
            .inputs
            .iter()
            .find(|input| !input.checked_metadata.is_empty())
            .expect("metadata-bearing input")
            .clone();
        bad_metadata.checked_metadata[0].logical_path = ".align-db/sqlite/wrong.json".into();
        assert!(matches!(
            StaticInputManifest::new(Hash128 { lo: 7, hi: 8 }, vec![bad_metadata]),
            Err(StaticInputError::NonCanonical(_))
        ));
        let mut omitted_metadata = manifest
            .inputs
            .iter()
            .find(|input| !input.checked_metadata.is_empty())
            .expect("metadata-bearing input")
            .clone();
        omitted_metadata.checked_metadata.clear();
        assert!(matches!(
            StaticInputManifest::new(Hash128 { lo: 7, hi: 8 }, vec![omitted_metadata]),
            Err(StaticInputError::NonCanonical(_))
        ));
        let mut wrong_order = resolve_inline_static_input(
            "any.query",
            "select",
            StaticConsumerKind::Query,
            DriverRestriction::AnySupportedDriver,
        )
        .expect("any-driver input")
        .input;
        wrong_order.checked_metadata = vec![
            metadata("any.query", Driver::PostgreSQL, MetadataState::Missing),
            metadata("any.query", Driver::SQLite, MetadataState::Missing),
        ];
        assert!(matches!(
            StaticInputManifest::new(Hash128 { lo: 7, hi: 8 }, vec![wrong_order]),
            Err(StaticInputError::NonCanonical(_))
        ));
        let mut unsorted = manifest.clone();
        unsorted.inputs.swap(0, 1);
        assert!(matches!(
            unsorted.canonical_bytes(),
            Err(StaticInputError::NonCanonical(_))
        ));
    }

    #[test]
    fn manifest_rejects_nul_file_identity_and_independent_descriptor_duplicates() {
        let mut nul = resolve_inline_static_input(
            "nul.query",
            "select 1",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
        )
        .expect("nul test input")
        .input;
        nul.source = SqlSourceIdentity::File {
            logical_path: "queries/bad\0.sql".to_string(),
        };
        assert!(matches!(
            StaticInputManifest::new(Hash128 { lo: 9, hi: 10 }, vec![nul]),
            Err(StaticInputError::NonCanonical(_))
        ));

        let first = resolve_inline_static_input(
            "duplicate.query",
            "select 1",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
        )
        .expect("first duplicate input")
        .input;
        let mut second = first.clone();
        second.source = SqlSourceIdentity::File {
            logical_path: "queries/other.sql".to_string(),
        };
        second.consumer_kind = StaticConsumerKind::Command;
        assert!(matches!(
            StaticInputManifest::new(Hash128 { lo: 9, hi: 10 }, vec![first, second]),
            Err(StaticInputError::NonCanonical(_))
        ));
    }

    #[test]
    fn metadata_paths_are_exact_and_checkout_root_independent() {
        let id = "queries.user";
        let sqlite = metadata_logical_path(id, Driver::SQLite).expect("sqlite path");
        let postgres = metadata_logical_path(id, Driver::PostgreSQL).expect("postgres path");
        assert_eq!(
            sqlite,
            ".align-db/sqlite/".to_string() + &Hash128::of(id.as_bytes()).to_hex() + ".json"
        );
        assert_eq!(
            postgres,
            ".align-db/postgres/".to_string() + &Hash128::of(id.as_bytes()).to_hex() + ".json"
        );
    }

    #[test]
    fn file_deletion_is_a_manifest_stale_result() {
        let root = temp_root("deletion");
        let module = root.join("q.align");
        write(&module, "module q\n").expect("align source");
        let sql = root.join("q.sql");
        write(&sql, "select 1").expect("SQL source");
        let input = resolve_static_file(
            &root,
            &module,
            None,
            "q.query",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
            None,
        )
        .expect("file input")
        .input;
        let manifest = StaticInputManifest::new(
            Hash128 { lo: 1, hi: 2 },
            vec![StaticInput {
                checked_metadata: vec![metadata("q.query", Driver::SQLite, MetadataState::Missing)],
                ..input
            }],
        )
        .expect("manifest");
        std::fs::remove_file(sql).expect("delete SQL source");
        assert!(matches!(
            manifest.revalidate(&root),
            Err(StaticInputError::Stale(_))
        ));
    }

    #[test]
    fn codegen_identity_includes_static_inputs_without_path_or_mtime() {
        let base = Hash128 { lo: 10, hi: 11 };
        let first = StaticInputManifest::empty(Hash128 { lo: 1, hi: 2 })
            .action_key()
            .expect("digest");
        let second = StaticInputManifest::empty(Hash128 { lo: 3, hi: 4 })
            .action_key()
            .expect("digest");
        assert_ne!(
            compose_codegen_impl_hash(base, first),
            compose_codegen_impl_hash(base, second)
        );
        assert_eq!(
            compose_codegen_impl_hash(base, first),
            compose_codegen_impl_hash(base, first)
        );
    }

    #[test]
    fn equivalent_checkout_roots_have_identical_manifest_identity() {
        let first_root = temp_root("checkout-a");
        let second_root = temp_root("checkout-b");
        for root in [&first_root, &second_root] {
            let module = root.join("q.align");
            write(&module, "module q\n").expect("align source");
            write(root.join("q.sql"), "select 1").expect("SQL source");
        }
        let first = resolve_static_file(
            &first_root,
            &first_root.join("q.align"),
            None,
            "q.query",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
            None,
        )
        .expect("first checkout")
        .input;
        let second = resolve_static_file(
            &second_root,
            &second_root.join("q.align"),
            None,
            "q.query",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
            None,
        )
        .expect("second checkout")
        .input;
        let first = StaticInputManifest::new(
            Hash128 { lo: 1, hi: 2 },
            vec![StaticInput {
                checked_metadata: vec![metadata("q.query", Driver::SQLite, MetadataState::Missing)],
                ..first
            }],
        )
        .expect("first manifest");
        let second = StaticInputManifest::new(
            Hash128 { lo: 1, hi: 2 },
            vec![StaticInput {
                checked_metadata: vec![metadata("q.query", Driver::SQLite, MetadataState::Missing)],
                ..second
            }],
        )
        .expect("second manifest");
        assert_eq!(
            first.canonical_bytes().expect("first bytes"),
            second.canonical_bytes().expect("second bytes")
        );
        assert_eq!(
            first.action_key().expect("first digest"),
            second.action_key().expect("second digest")
        );
    }
}
