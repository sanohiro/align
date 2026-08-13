//! Explicit Q3 checked-metadata regeneration.
//!
//! Normal compilation never enters this module. The public CLI first checks the reachable program,
//! resolves the same static SQL/artifact inputs in regeneration mode, selects one driver and exact
//! descriptor set, then hands those validated artifacts to a native describer. Only after every
//! description has been converted to canonical bytes may publication begin.

use crate::static_artifacts::{
    build_static_artifacts_for_regeneration, root_fields, static_statement_class,
};
use crate::static_inputs::{
    encode_checked_metadata, lock_metadata_publication_exclusive, lock_metadata_publication_shared,
    metadata_path, resolve_static_descriptors_for_regeneration, ParsedCheckedMetadata,
    ParsedMetadataColumn, ParsedMetadataExtension, ParsedMetadataParameter,
};
use crate::{try_lower_to_mir, BuiltStaticArtifact, Checked};
use align_interface::{
    static_options_hash, CheckedColumnMeta, CheckedParameterMeta, Driver, DriverEntry, Hash128,
    MetaNullability, StaticArtifact, StaticOptionValue,
};
use align_span::SourceMap;
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const METADATA_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeParameterDescription {
    pub ordinal: u32,
    pub source_name: Option<String>,
    pub native_type: Option<String>,
    pub native_type_id: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeColumnDescription {
    pub ordinal: u32,
    pub source_alias: String,
    pub native_type: Option<String>,
    pub native_type_id: Option<i64>,
    pub origin_schema: Option<String>,
    pub origin_table: Option<String>,
    pub origin_column: Option<String>,
    pub nullable: MetaNullability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeStatementDescription {
    pub parameters: Vec<NativeParameterDescription>,
    pub columns: Vec<NativeColumnDescription>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparationEnvironment {
    pub engine_version: String,
    pub driver_version: String,
    pub schema_fingerprint: Hash128,
    pub search_path: Vec<String>,
    pub extensions: Vec<PreparationExtension>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PreparationExtension {
    pub schema: String,
    pub name: String,
    pub version: Option<String>,
}

/// A driver is opened lazily by `environment`, after descriptor selection and all compiler-owned
/// validation have completed. Implementations must retain one native session until Drop so every
/// statement in the batch observes the same schema/server state.
pub trait MetadataDescriber {
    fn driver(&self) -> Driver;
    fn environment(&mut self) -> Result<PreparationEnvironment, PrepareError>;
    fn describe(
        &mut self,
        artifact: &StaticArtifact,
        entry: &DriverEntry,
    ) -> Result<NativeStatementDescription, PrepareError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedMetadataFile {
    pub descriptor_id: String,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedMetadataBatch {
    pub driver: Driver,
    pub project_root: PathBuf,
    pub environment: PreparationEnvironment,
    pub files: Vec<PreparedMetadataFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicationReport {
    pub selected: usize,
    pub changed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationEntry {
    pub version: u32,
    pub filename: String,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationCatalog {
    pub entries: Vec<MigrationEntry>,
    pub encoded: Vec<u8>,
    pub fingerprint: Hash128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepareError(pub String);

impl std::fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PrepareError {}

fn fail(reason: impl Into<String>) -> PrepareError {
    PrepareError(reason.into())
}

struct IdentityWriter {
    bytes: Vec<u8>,
}

impl IdentityWriter {
    fn new(magic: &[u8; 8]) -> Self {
        Self {
            bytes: magic.to_vec(),
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn hash(&mut self, value: Hash128) {
        self.bytes.extend_from_slice(&value.lo.to_le_bytes());
        self.bytes.extend_from_slice(&value.hi.to_le_bytes());
    }

    fn string(&mut self, value: &str) -> Result<(), PrepareError> {
        if value.as_bytes().contains(&0) {
            return Err(fail("database schema identity text contains U+0000"));
        }
        self.u32(
            u32::try_from(value.len()).map_err(|_| fail("schema identity text is too large"))?,
        );
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn extensions(&mut self, extensions: &[PreparationExtension]) -> Result<(), PrepareError> {
        self.u32(u32::try_from(extensions.len()).map_err(|_| fail("too many extensions"))?);
        let mut previous = None;
        for extension in extensions {
            if previous.is_some_and(|value: &PreparationExtension| value >= extension) {
                return Err(fail("extensions are not strictly sorted"));
            }
            self.string(&extension.schema)?;
            self.string(&extension.name)?;
            match &extension.version {
                Some(version) => {
                    self.u8(1);
                    self.string(version)?;
                }
                None => self.u8(0),
            }
            previous = Some(extension);
        }
        Ok(())
    }
}

fn migration_name(name: &str) -> Option<u32> {
    let bytes = name.as_bytes();
    if bytes.len() < 10 || &bytes[4..5] != b"_" || !name.ends_with(".sql") {
        return None;
    }
    if !bytes[..4].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let stem = &bytes[5..bytes.len() - 4];
    if stem.is_empty()
        || !stem[0].is_ascii_lowercase()
        || !stem[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        return None;
    }
    let version = name[..4].parse::<u32>().ok()?;
    (version != 0).then_some(version)
}

/// Read and validate the exact immediate-entry SQLite migration catalog before native work.
pub fn read_migration_catalog(directory: &Path) -> Result<MigrationCatalog, PrepareError> {
    let mut directory_entries = fs::read_dir(directory)
        .map_err(|error| {
            fail(format!(
                "cannot enumerate migration directory `{}`: {error}",
                directory.display()
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            fail(format!(
                "cannot enumerate migration directory `{}`: {error}",
                directory.display()
            ))
        })?;
    directory_entries.sort_by(|left, right| {
        left.file_name()
            .as_encoded_bytes()
            .cmp(right.file_name().as_encoded_bytes())
    });

    let mut selected = Vec::new();
    for entry in directory_entries {
        let raw_name = entry.file_name();
        let name = raw_name.to_str().ok_or_else(|| {
            fail(format!(
                "migration directory `{}` contains a non-UTF-8 entry name",
                directory.display()
            ))
        })?;
        let file_type = entry
            .file_type()
            .map_err(|error| fail(format!("cannot inspect migration entry `{name}`: {error}")))?;
        if file_type.is_symlink() {
            return Err(fail(format!("migration entry `{name}` is a symlink")));
        }
        if !file_type.is_file() {
            continue;
        }
        let Some(version) = migration_name(name) else {
            if name.ends_with(".sql") {
                return Err(fail(format!("migration filename `{name}` is invalid")));
            }
            continue;
        };
        selected.push((version, name.to_string(), entry.path()));
    }
    if selected.is_empty() {
        return Err(fail("migration catalog contains no migration files"));
    }
    selected.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.as_bytes().cmp(right.1.as_bytes()))
    });
    for (index, (version, name, _)) in selected.iter().enumerate() {
        let expected = u32::try_from(index + 1).map_err(|_| fail("migration count exceeds u32"))?;
        if *version != expected {
            return Err(fail(format!(
                "migration `{name}` has version {version:04}; expected {expected:04}"
            )));
        }
    }

    let mut entries = Vec::with_capacity(selected.len());
    for (version, filename, path) in selected {
        let bytes = fs::read(&path)
            .map_err(|error| fail(format!("cannot read migration `{filename}`: {error}")))?;
        std::str::from_utf8(&bytes)
            .map_err(|_| fail(format!("migration `{filename}` is not UTF-8")))?;
        if bytes.contains(&0) {
            return Err(fail(format!("migration `{filename}` contains U+0000")));
        }
        entries.push(MigrationEntry {
            version,
            filename,
            path,
            bytes,
        });
    }
    encode_migration_catalog(entries)
}

/// Encode the canonical `ALIGNMIG` stream. Exposed so independent fixtures can pin the empty form
/// even though `--migrations` itself requires at least one selected entry.
pub fn encode_migration_catalog(
    entries: Vec<MigrationEntry>,
) -> Result<MigrationCatalog, PrepareError> {
    let mut writer = IdentityWriter::new(b"ALIGNMIG");
    writer.u32(1);
    writer.u32(u32::try_from(entries.len()).map_err(|_| fail("migration count exceeds u32"))?);
    let mut expected = 1u32;
    for entry in &entries {
        if entry.version != expected {
            return Err(fail(
                "migration entries are not a contiguous sequence beginning at 0001",
            ));
        }
        if migration_name(&entry.filename) != Some(entry.version) {
            return Err(fail(format!(
                "migration filename `{}` is invalid",
                entry.filename
            )));
        }
        std::str::from_utf8(&entry.bytes)
            .map_err(|_| fail(format!("migration `{}` is not UTF-8", entry.filename)))?;
        if entry.bytes.contains(&0) {
            return Err(fail(format!(
                "migration `{}` contains U+0000",
                entry.filename
            )));
        }
        writer.u32(entry.version);
        writer.string(&entry.filename)?;
        writer.hash(Hash128::of(&entry.bytes));
        expected = expected
            .checked_add(1)
            .ok_or_else(|| fail("migration version overflow"))?;
    }
    let fingerprint = Hash128::of(&writer.bytes);
    Ok(MigrationCatalog {
        entries,
        encoded: writer.bytes,
        fingerprint,
    })
}

pub fn sqlite_database_schema_fingerprint(schema_id: &str) -> Result<Hash128, PrepareError> {
    if schema_id.is_empty() {
        return Err(fail("SQLite --schema-id must not be empty"));
    }
    let mut writer = IdentityWriter::new(b"ALIGNSID");
    writer.u32(1);
    writer.u8(Driver::SQLite as u8);
    writer.u8(1);
    writer.string(schema_id)?;
    Ok(Hash128::of(&writer.bytes))
}

pub fn sqlite_memory_schema_fingerprint(catalog_fingerprint: Option<Hash128>) -> Hash128 {
    let mut writer = IdentityWriter::new(b"ALIGNSID");
    writer.u32(1);
    writer.u8(Driver::SQLite as u8);
    writer.u8(0);
    match catalog_fingerprint {
        Some(fingerprint) => {
            writer.u8(1);
            writer.hash(fingerprint);
        }
        None => writer.u8(0),
    }
    Hash128::of(&writer.bytes)
}

pub fn postgres_schema_fingerprint(
    schema_id: &str,
    search_path: &[String],
    extensions: &[PreparationExtension],
) -> Result<Hash128, PrepareError> {
    if schema_id.is_empty() {
        return Err(fail("PostgreSQL --schema-id must not be empty"));
    }
    let mut writer = IdentityWriter::new(b"ALIGNSID");
    writer.u32(1);
    writer.u8(Driver::PostgreSQL as u8);
    writer.u8(2);
    writer.string(schema_id)?;
    writer.u32(u32::try_from(search_path.len()).map_err(|_| fail("too many search-path entries"))?);
    for entry in search_path {
        writer.string(entry)?;
    }
    writer.extensions(extensions)?;
    Ok(Hash128::of(&writer.bytes))
}

fn project_root(entry_path: &Path) -> Result<PathBuf, PrepareError> {
    let parent = entry_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let root = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| fail(format!("cannot resolve current directory: {error}")))?
            .join(parent)
    };
    if !root.is_dir() {
        return Err(fail(format!(
            "project root `{}` is not a directory",
            root.display()
        )));
    }
    // Preserve the lexical root spelling used by SourceMap paths. The static-input reader performs
    // its own canonical containment check, while this avoids `/var` versus `/private/var` aliasing
    // from making an in-root sibling SQL file appear outside the project on macOS.
    Ok(root)
}

fn select_artifacts<'a>(
    artifacts: &'a [BuiltStaticArtifact],
    driver: Driver,
    selected_ids: &[String],
) -> Result<Vec<&'a BuiltStaticArtifact>, PrepareError> {
    let mut by_id = artifacts
        .iter()
        .map(|artifact| (artifact.descriptor_id.as_str(), artifact))
        .collect::<HashMap<_, _>>();
    let mut selected = Vec::new();
    if selected_ids.is_empty() {
        selected.extend(
            artifacts
                .iter()
                .filter(|artifact| driver_entry(&artifact.artifact, driver).is_some()),
        );
    } else {
        let mut seen = HashSet::new();
        for id in selected_ids {
            if id.is_empty() || id.as_bytes().contains(&0) {
                return Err(fail("--query requires a non-empty NUL-free descriptor id"));
            }
            if !seen.insert(id.as_str()) {
                return Err(fail(format!("duplicate --query descriptor `{id}`")));
            }
            let Some(artifact) = by_id.remove(id.as_str()) else {
                return Err(fail(format!("unknown --query descriptor `{id}`")));
            };
            selected.push(artifact);
        }
    }
    selected.sort_by(|left, right| {
        left.descriptor_id
            .as_bytes()
            .cmp(right.descriptor_id.as_bytes())
    });
    let mut paths = HashMap::new();
    for artifact in &selected {
        if driver_entry(&artifact.artifact, driver).is_none() {
            return Err(fail(format!(
                "descriptor `{}` does not permit {}",
                artifact.descriptor_id,
                driver_name(driver)
            )));
        }
        let path_hash = Hash128::of(artifact.descriptor_id.as_bytes()).to_hex();
        if let Some(previous) = paths.insert(path_hash, artifact.descriptor_id.as_str()) {
            return Err(fail(format!(
                "descriptor IDs `{previous}` and `{}` collide in the metadata path",
                artifact.descriptor_id
            )));
        }
    }
    Ok(selected)
}

fn driver_name(driver: Driver) -> &'static str {
    match driver {
        Driver::SQLite => "SQLite",
        Driver::PostgreSQL => "PostgreSQL",
    }
}

fn driver_entry(artifact: &StaticArtifact, driver: Driver) -> Option<&DriverEntry> {
    match artifact {
        StaticArtifact::Query(query) => &query.driver_entries,
        StaticArtifact::Command(command) => &command.driver_entries,
    }
    .iter()
    .find(|entry| entry.driver == driver)
}

fn artifact_static_options(artifact: &StaticArtifact) -> &[align_interface::StaticOption] {
    match artifact {
        StaticArtifact::Query(query) => &query.static_options,
        StaticArtifact::Command(command) => &command.static_options,
    }
}

fn validate_preparation_options(
    selected: &[&BuiltStaticArtifact],
    driver: Driver,
) -> Result<(), PrepareError> {
    for built in selected {
        for option in artifact_static_options(&built.artifact) {
            match (&option.value, driver) {
                (StaticOptionValue::Check { .. }, _) => {}
                (StaticOptionValue::SQLiteRequireVersionAtLeast { .. }, Driver::SQLite) => {}
                (
                    StaticOptionValue::PostgreSQLParameterType {
                        parameter_name,
                        canonical_type_name,
                        ..
                    },
                    Driver::PostgreSQL,
                ) if canonical_type_name == "int8" => {
                    let contract = match &built.artifact {
                        StaticArtifact::Query(query) => &query.params_type,
                        StaticArtifact::Command(command) => &command.params_type,
                    };
                    let field = root_fields(contract)
                        .map_err(fail)?
                        .iter()
                        .find(|field| field.name == *parameter_name)
                        .ok_or_else(|| {
                            fail(format!(
                                "PostgreSQL parameter type names unknown Params field `{parameter_name}`"
                            ))
                        })?;
                    let logical_type = field.ty.spelling();
                    if logical_base(&logical_type) != "i64" {
                        return Err(fail(format!(
                            "PostgreSQL parameter type `int8` requires `{parameter_name}` to be i64 or Option<i64>"
                        )));
                    }
                }
                (
                    StaticOptionValue::PostgreSQLParameterType {
                        canonical_type_name,
                        ..
                    },
                    Driver::PostgreSQL,
                ) => {
                    return Err(fail(format!(
                        "unsupported PostgreSQL parameter type `{canonical_type_name}`"
                    )));
                }
                _ => {
                    return Err(fail(format!(
                        "descriptor `{}` has an option for a different database driver",
                        built.descriptor_id
                    )));
                }
            }
        }
    }
    Ok(())
}

fn logical_base(logical_type: &str) -> &str {
    logical_type
        .strip_prefix("Option<")
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(logical_type)
}

fn validate_native_type(
    driver: Driver,
    logical_type: &str,
    native_type: Option<&str>,
    native_type_id: Option<i64>,
) -> Result<(), PrepareError> {
    let logical = logical_base(logical_type);
    match driver {
        Driver::SQLite => {
            if native_type_id.is_some() {
                return Err(fail(
                    "SQLite metadata unexpectedly contains a native type ID",
                ));
            }
            let Some(native) = native_type else {
                // SQLite supplies no declaration for parameters and many expressions. Runtime
                // storage-class validation remains mandatory for this checked descriptor.
                return Ok(());
            };
            let upper = native.to_ascii_uppercase();
            let affinity = if upper.contains("INT") {
                "integer"
            } else if upper.contains("CHAR") || upper.contains("CLOB") || upper.contains("TEXT") {
                "text"
            } else if upper.is_empty() || upper.contains("BLOB") {
                "blob"
            } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
                "real"
            } else {
                "numeric"
            };
            let supported = match logical {
                "i8" | "i16" | "i32" | "i64" => affinity == "integer",
                "f64" => affinity == "real",
                "str" | "string" => affinity == "text",
                "slice<u8>" | "array<u8>" => affinity == "blob",
                _ => false,
            };
            if !supported {
                return Err(fail(format!(
                    "SQLite native type `{native}` does not support logical type `{logical_type}`"
                )));
            }
        }
        Driver::PostgreSQL => {
            let Some(oid) = native_type_id else {
                return Err(fail("PostgreSQL metadata is missing a native type OID"));
            };
            let Some(native) = native_type else {
                return Err(fail(
                    "PostgreSQL metadata is missing a canonical native type name",
                ));
            };
            let expected = match logical {
                "i16" => &[(21, "smallint")][..],
                "i32" => &[(23, "integer")][..],
                "i64" => &[(20, "bigint")][..],
                "f32" => &[(700, "real")][..],
                "f64" => &[(701, "double precision")][..],
                "bool" => &[(16, "boolean")][..],
                "str" | "string" => &[(25, "text"), (1043, "character varying"), (19, "name")][..],
                "slice<u8>" | "array<u8>" => &[(17, "bytea")][..],
                _ => &[][..],
            };
            let type_matches = u32::try_from(oid)
                .ok()
                .is_some_and(|oid| expected.contains(&(oid, native)));
            if !type_matches {
                return Err(fail(format!(
                    "PostgreSQL native type `{native}` (OID {oid}) does not support logical type `{logical_type}`"
                )));
            }
        }
    }
    Ok(())
}

fn parameter_records(
    artifact: &StaticArtifact,
    entry: &DriverEntry,
    description: &NativeStatementDescription,
) -> Result<Vec<ParsedMetadataParameter>, PrepareError> {
    let declared = match artifact {
        StaticArtifact::Query(query) => query
            .query_meta_plan
            .parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.ordinal,
                    parameter.source_name.as_str(),
                    parameter.logical_type.clone(),
                )
            })
            .collect::<Vec<_>>(),
        StaticArtifact::Command(command) => {
            let fields = root_fields(&command.params_type).map_err(fail)?;
            let mut bindings = entry.bindings.iter().collect::<Vec<_>>();
            bindings.sort_by_key(|binding| binding.protocol_ordinal);
            bindings
                .into_iter()
                .map(|binding| {
                    let field = fields
                        .get(binding.params_field_ordinal as usize)
                        .ok_or_else(|| fail("command Params field ordinal is out of range"))?;
                    Ok((
                        binding.protocol_ordinal,
                        binding.source_name.as_str(),
                        field.ty.spelling(),
                    ))
                })
                .collect::<Result<Vec<_>, PrepareError>>()?
        }
    };
    if declared.len() != description.parameters.len() {
        return Err(fail(
            "database parameter count does not match the static Params plan",
        ));
    }
    declared
        .into_iter()
        .zip(&description.parameters)
        .map(|((ordinal, source_name, logical_type), native)| {
            if ordinal != native.ordinal
                || native
                    .source_name
                    .as_deref()
                    .is_some_and(|actual| actual != source_name)
            {
                return Err(fail(
                    "database parameter order/name does not match the static Params plan",
                ));
            }
            validate_native_type(
                entry.driver,
                &logical_type,
                native.native_type.as_deref(),
                native.native_type_id,
            )?;
            Ok(ParsedMetadataParameter {
                source_name: source_name.to_string(),
                logical_type,
                checked: CheckedParameterMeta {
                    ordinal,
                    native_type: native.native_type.clone(),
                    native_type_id: native.native_type_id,
                },
            })
        })
        .collect()
}

fn column_records(
    artifact: &StaticArtifact,
    description: &NativeStatementDescription,
    driver: Driver,
) -> Result<Vec<ParsedMetadataColumn>, PrepareError> {
    let StaticArtifact::Query(query) = artifact else {
        if description.columns.is_empty() {
            return Ok(Vec::new());
        }
        return Err(fail(
            "database command unexpectedly describes result columns",
        ));
    };
    if query.query_meta_plan.columns.len() != description.columns.len() {
        return Err(fail(
            "database column count does not match the static Row plan",
        ));
    }
    query
        .query_meta_plan
        .columns
        .iter()
        .zip(&description.columns)
        .map(|(declared, native)| {
            if declared.ordinal != native.ordinal || declared.source_alias != native.source_alias {
                return Err(fail(
                    "database column order/name does not match the static Row plan",
                ));
            }
            validate_native_type(
                driver,
                &declared.logical_type,
                native.native_type.as_deref(),
                native.native_type_id,
            )?;
            Ok(ParsedMetadataColumn {
                source_alias: declared.source_alias.clone(),
                logical_type: declared.logical_type.clone(),
                checked: CheckedColumnMeta {
                    ordinal: declared.ordinal,
                    native_type: native.native_type.clone(),
                    native_type_id: native.native_type_id,
                    origin_schema: native.origin_schema.clone(),
                    origin_table: native.origin_table.clone(),
                    origin_column: native.origin_column.clone(),
                    nullable: native.nullable,
                },
            })
        })
        .collect()
}

fn record(
    artifact: &StaticArtifact,
    entry: &DriverEntry,
    environment: &PreparationEnvironment,
    description: &NativeStatementDescription,
) -> Result<(String, ParsedCheckedMetadata), PrepareError> {
    let (id, restriction, kind, class, source, source_hash, params_hash, row_hash, options) =
        match artifact {
            StaticArtifact::Query(query) => (
                query.query_id.as_str(),
                query.driver_restriction,
                crate::static_inputs::MetadataStatementKind::Query,
                query.query_meta_plan.statement_class,
                query.source_identity.clone(),
                query.source_sql_hash,
                query.params_fingerprint,
                Some(query.row_fingerprint),
                query.static_options.as_slice(),
            ),
            StaticArtifact::Command(command) => (
                command.command_id.as_str(),
                command.driver_restriction,
                crate::static_inputs::MetadataStatementKind::Command,
                static_statement_class(&command.source_sql).map_err(fail)?,
                command.source_identity.clone(),
                command.source_sql_hash,
                command.params_fingerprint,
                None,
                command.static_options.as_slice(),
            ),
        };
    Ok((
        id.to_string(),
        ParsedCheckedMetadata {
            format_version: METADATA_FORMAT_VERSION,
            metadata_digest: Hash128 { lo: 0, hi: 0 },
            driver: entry.driver,
            driver_restriction: restriction,
            statement_kind: kind,
            statement_class: class,
            source_identity: source,
            source_sql_hash: source_hash,
            wire_sql_hash: entry.wire_sql_hash,
            rewrite_format_version: entry.rewrite_format_version,
            static_options_hash: static_options_hash(options)
                .map_err(|error| fail(error.to_string()))?,
            params_fingerprint: params_hash,
            row_fingerprint: row_hash,
            schema_fingerprint: environment.schema_fingerprint,
            engine_version: environment.engine_version.clone(),
            driver_version: environment.driver_version.clone(),
            search_path: environment.search_path.clone(),
            extensions: environment
                .extensions
                .iter()
                .map(|extension| ParsedMetadataExtension {
                    schema: extension.schema.clone(),
                    name: extension.name.clone(),
                    version: extension.version.clone(),
                })
                .collect(),
            parameters: parameter_records(artifact, entry, description)?,
            columns: column_records(artifact, description, entry.driver)?,
        },
    ))
}

/// Build a complete in-memory Q3 metadata batch. This function performs no writes.
pub fn build_metadata_batch(
    source_map: &mut SourceMap,
    entry_path: &Path,
    checked: &Checked,
    selected_ids: &[String],
    describer: &mut dyn MetadataDescriber,
) -> Result<PreparedMetadataBatch, PrepareError> {
    if checked.diags.has_errors() {
        return Err(fail(
            "cannot prepare database metadata for a program with diagnostics",
        ));
    }
    let project_root = project_root(entry_path)?;
    let mir = try_lower_to_mir(&checked.hir)
        .map_err(|rejected| fail(format!("internal error: {rejected}")))?;
    let resolution_digest = align_interface::codegen_impl_hash(&mir);
    let resolved = resolve_static_descriptors_for_regeneration(
        &project_root,
        source_map,
        &checked.static_descriptors,
        resolution_digest,
    )
    .map_err(|error| fail(error.to_string()))?;
    let artifacts = build_static_artifacts_for_regeneration(&checked.static_descriptors, &resolved)
        .map_err(|error| fail(error.to_string()))?;
    let driver = describer.driver();
    let selected = select_artifacts(&artifacts, driver, selected_ids)?;
    if selected.is_empty() {
        return Err(fail(
            "the reachable program contains no selected static Query or command",
        ));
    }
    validate_preparation_options(&selected, driver)?;

    // The first potentially native operation happens only after the complete compiler-owned
    // inventory/selection pass above.
    let mut environment = describer.environment()?;
    environment.extensions.sort();
    if environment
        .extensions
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        return Err(fail("database extension inventory contains a duplicate"));
    }
    if driver == Driver::SQLite
        && (!environment.search_path.is_empty() || !environment.extensions.is_empty())
    {
        return Err(fail(
            "SQLite preparation environment must not expose search path or extensions",
        ));
    }

    let mut files = Vec::with_capacity(selected.len());
    for built in selected {
        let entry = driver_entry(&built.artifact, driver)
            .ok_or_else(|| fail("selected descriptor lost its driver entry"))?;
        let description = describer.describe(&built.artifact, entry)?;
        let (descriptor_id, record) = record(&built.artifact, entry, &environment, &description)?;
        let bytes = encode_checked_metadata(&descriptor_id, &record)
            .map_err(|error| fail(error.to_string()))?;
        files.push(PreparedMetadataFile {
            descriptor_id: descriptor_id.clone(),
            path: metadata_path(&project_root, &descriptor_id, driver)
                .map_err(|error| fail(error.to_string()))?,
            bytes,
        });
    }
    Ok(PreparedMetadataBatch {
        driver,
        project_root,
        environment,
        files,
    })
}

static PUBLICATION_NONCE: AtomicU64 = AtomicU64::new(0);

fn metadata_parent_is_safe(root: &Path, path: &Path) -> Result<(), PrepareError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        fail(format!(
            "metadata path `{}` escapes the project root",
            path.display()
        ))
    })?;
    let parent = relative
        .parent()
        .ok_or_else(|| fail("metadata path has no parent"))?;
    let mut current = root.to_path_buf();
    for component in parent.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(fail(format!(
                    "metadata parent `{}` is a symlink",
                    current.display()
                )));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(fail(format!(
                    "metadata parent `{}` is not a directory",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(fail(format!(
                    "cannot inspect metadata parent `{}`: {error}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn temporary_path(destination: &Path, purpose: &str) -> Result<PathBuf, PrepareError> {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            fail(format!(
                "metadata path `{}` has no UTF-8 filename",
                destination.display()
            ))
        })?;
    let nonce = PUBLICATION_NONCE.fetch_add(1, Ordering::Relaxed);
    Ok(destination.with_file_name(format!(
        ".{name}.{purpose}.{}.{}",
        std::process::id(),
        nonce
    )))
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), PrepareError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            fail(format!(
                "cannot create temporary metadata `{}`: {error}",
                path.display()
            ))
        })?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(fail(format!(
            "cannot write temporary metadata `{}`: {error}",
            path.display()
        )));
    }
    Ok(())
}

/// Compare or publish one complete in-memory metadata batch.
///
/// Check mode is strictly read-only. Write mode stages every changed record before the first
/// replacement and rolls already-replaced records back from their exact previous bytes if a later
/// rename fails. Temporary files always share the destination directory.
pub fn publish_metadata_batch(
    batch: &PreparedMetadataBatch,
    check_only: bool,
) -> Result<PublicationReport, PrepareError> {
    struct Change {
        destination: PathBuf,
        previous: Option<Vec<u8>>,
        staged: Option<PathBuf>,
    }

    let publication_lock = if check_only {
        lock_metadata_publication_shared(&batch.project_root)
    } else {
        lock_metadata_publication_exclusive(&batch.project_root)
    }
    .map_err(|error| fail(error.to_string()))?;

    let mut changes = Vec::new();
    for file in &batch.files {
        metadata_parent_is_safe(&batch.project_root, &file.path)?;
        let previous = match fs::symlink_metadata(&file.path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(fail(format!(
                    "metadata destination `{}` is a symlink",
                    file.path.display()
                )));
            }
            Ok(metadata) if metadata.is_file() => Some(fs::read(&file.path).map_err(|error| {
                fail(format!(
                    "cannot read metadata `{}`: {error}",
                    file.path.display()
                ))
            })?),
            Ok(_) => {
                return Err(fail(format!(
                    "metadata destination `{}` is not a regular file",
                    file.path.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(fail(format!(
                    "cannot inspect metadata `{}`: {error}",
                    file.path.display()
                )));
            }
        };
        if previous.as_deref() != Some(file.bytes.as_slice()) {
            changes.push(Change {
                destination: file.path.clone(),
                previous,
                staged: None,
            });
        }
    }
    if check_only {
        publication_lock
            .validate()
            .map_err(|error| fail(error.to_string()))?;
        if changes.is_empty() {
            return Ok(PublicationReport {
                selected: batch.files.len(),
                changed: 0,
            });
        }
        return Err(fail(format!(
            "{} checked metadata file(s) are missing or stale",
            changes.len()
        )));
    }

    for change in &mut changes {
        let parent = change
            .destination
            .parent()
            .ok_or_else(|| fail("metadata path has no parent"))?;
        fs::create_dir_all(parent).map_err(|error| {
            fail(format!(
                "cannot create metadata directory `{}`: {error}",
                parent.display()
            ))
        })?;
        metadata_parent_is_safe(&batch.project_root, &change.destination)?;
        let staged = temporary_path(&change.destination, "new")?;
        let bytes = batch
            .files
            .iter()
            .find(|file| file.path == change.destination)
            .map(|file| file.bytes.as_slice())
            .ok_or_else(|| fail("metadata publication lost a selected record"))?;
        if let Err(error) = write_new_file(&staged, bytes) {
            for earlier in &changes {
                if let Some(path) = &earlier.staged {
                    let _ = fs::remove_file(path);
                }
            }
            return Err(error);
        }
        change.staged = Some(staged);
    }

    let mut applied = 0usize;
    while applied < changes.len() {
        let change = changes
            .get(applied)
            .ok_or_else(|| fail("metadata publication index is out of range"))?;
        let staged = change
            .staged
            .as_ref()
            .ok_or_else(|| fail("metadata publication lost a staged file"))?;
        let destination = change.destination.clone();
        if let Err(error) = fs::rename(staged, &destination) {
            let mut rollback_error = None;
            for prior in changes[..applied].iter().rev() {
                let rollback = match &prior.previous {
                    Some(bytes) => {
                        temporary_path(&prior.destination, "rollback").and_then(|path| {
                            write_new_file(&path, bytes)?;
                            fs::rename(&path, &prior.destination).map_err(|rename_error| {
                                let _ = fs::remove_file(&path);
                                fail(format!(
                                    "cannot restore metadata `{}`: {rename_error}",
                                    prior.destination.display()
                                ))
                            })
                        })
                    }
                    None => fs::remove_file(&prior.destination).map_err(|remove_error| {
                        fail(format!(
                            "cannot remove newly published metadata `{}`: {remove_error}",
                            prior.destination.display()
                        ))
                    }),
                };
                if let Err(error) = rollback {
                    rollback_error = Some(error);
                    break;
                }
            }
            for pending in &changes[applied..] {
                if let Some(path) = &pending.staged {
                    let _ = fs::remove_file(path);
                }
            }
            if let Some(rollback) = rollback_error {
                return Err(fail(format!(
                    "cannot publish metadata `{}`: {error}; rollback also failed: {rollback}",
                    destination.display()
                )));
            }
            return Err(fail(format!(
                "cannot publish metadata `{}`: {error}",
                destination.display()
            )));
        }
        applied += 1;
    }
    Ok(PublicationReport {
        selected: batch.files.len(),
        changed: changes.len(),
    })
}
