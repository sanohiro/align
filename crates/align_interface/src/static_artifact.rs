//! Versioned static Query/command artifacts (L5a).
//!
//! This module owns the producer-independent semantic record and the exact v1 byte boundary from
//! `docs/impl/17-library-boundary-prerequisites.md` §6.2.  Constructor discovery and cache
//! registration are deliberately outside this module; the next L5 checkpoint consumes these types.
//! The codec is fail-closed for bytes read from disk and validates the complete semantic record
//! before encoding, so a producer cannot accidentally publish a non-canonical artifact.

use crate::Hash128;
use std::collections::{HashMap, HashSet};

pub const STATIC_ARTIFACT_FORMAT_VERSION: u32 = 1;
pub const BINDER_ABI_VERSION: u32 = 1;
pub const DECODER_ABI_VERSION: u32 = 1;
pub const REWRITE_FORMAT_VERSION: u32 = 1;
const MAX_TYPE_DEPTH: usize = 256;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Driver {
    SQLite = 0,
    PostgreSQL = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DriverRestriction {
    AnySupportedDriver = 0,
    SQLiteOnly = 1,
    PostgreSQLOnly = 2,
}

impl DriverRestriction {
    pub fn drivers(self) -> &'static [Driver] {
        match self {
            Self::AnySupportedDriver => &[Driver::SQLite, Driver::PostgreSQL],
            Self::SQLiteOnly => &[Driver::SQLite],
            Self::PostgreSQLOnly => &[Driver::PostgreSQL],
        }
    }

    fn allows(self, driver: Driver) -> bool {
        self.drivers().contains(&driver)
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CheckPolicy {
    DeclaredOnly = 0,
    CheckedOptional = 1,
    CheckedRequired = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VerificationState {
    Declared = 0,
    DatabaseChecked = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindRetention {
    BindValue = 0,
    BindCopy = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StaticOptionOwner {
    Common = 0,
    SQLite = 1,
    PostgreSQL = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CanonicalDefinitionKind {
    Struct = 0,
    Sum = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetaStatementClass {
    Select = 0,
    Dml = 1,
    Ddl = 2,
    Native = 3,
    Unknown = 4,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetaNullability {
    Yes = 0,
    No = 1,
    Unknown = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlSourceIdentity {
    File { logical_path: String },
    Inline { query_or_command_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaticOptionValue {
    Check {
        policy: CheckPolicy,
    },
    SQLiteRequireVersionAtLeast {
        major: u32,
        minor: u32,
        patch: u32,
    },
    PostgreSQLParameterType {
        parameter_name: String,
        canonical_type_name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticOption {
    pub owner: StaticOptionOwner,
    pub value: StaticOptionValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalType {
    Named {
        path: String,
        args: Vec<CanonicalType>,
    },
    Tuple(Vec<CanonicalType>),
    Fn {
        params: Vec<CanonicalType>,
        result: Box<CanonicalType>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalField {
    pub name: String,
    pub ty: CanonicalType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalVariant {
    pub name: String,
    pub payload: Vec<CanonicalType>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalDefinitionBody {
    Struct { fields: Vec<CanonicalField> },
    Sum { variants: Vec<CanonicalVariant> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalDefinition {
    pub path: String,
    pub args: Vec<CanonicalType>,
    pub kind: CanonicalDefinitionBody,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalContract {
    pub root: CanonicalType,
    pub definitions: Vec<CanonicalDefinition>,
}

impl TryFrom<&align_sema::StaticContract> for CanonicalContract {
    type Error = StaticArtifactError;

    fn try_from(contract: &align_sema::StaticContract) -> Result<Self, Self::Error> {
        fn ty(
            value: &align_sema::StaticContractType,
        ) -> Result<CanonicalType, StaticArtifactError> {
            Ok(match value {
                align_sema::StaticContractType::Named { path, args } => CanonicalType::Named {
                    path: path.clone(),
                    args: args.iter().map(ty).collect::<Result<Vec<_>, _>>()?,
                },
                align_sema::StaticContractType::FixedArray { .. } => {
                    return Err(invalid(
                        "fixed arrays are not part of the static artifact v1 type contract",
                    ));
                }
                align_sema::StaticContractType::Tuple(elements) => {
                    CanonicalType::Tuple(elements.iter().map(ty).collect::<Result<Vec<_>, _>>()?)
                }
                align_sema::StaticContractType::Fn { params, result } => CanonicalType::Fn {
                    params: params.iter().map(ty).collect::<Result<Vec<_>, _>>()?,
                    result: Box::new(ty(result)?),
                },
            })
        }

        let mut definitions = contract
            .definitions
            .iter()
            .map(|definition| {
                Ok(CanonicalDefinition {
                    path: definition.path.clone(),
                    args: definition
                        .args
                        .iter()
                        .map(ty)
                        .collect::<Result<Vec<_>, StaticArtifactError>>()?,
                    kind: match &definition.kind {
                        align_sema::StaticContractDefinitionBody::Struct { fields } => {
                            CanonicalDefinitionBody::Struct {
                                fields: fields
                                    .iter()
                                    .map(|field| {
                                        Ok(CanonicalField {
                                            name: field.name.clone(),
                                            ty: ty(&field.ty)?,
                                        })
                                    })
                                    .collect::<Result<Vec<_>, StaticArtifactError>>()?,
                            }
                        }
                        align_sema::StaticContractDefinitionBody::Sum { variants } => {
                            CanonicalDefinitionBody::Sum {
                                variants: variants
                                    .iter()
                                    .map(|variant| {
                                        Ok(CanonicalVariant {
                                            name: variant.name.clone(),
                                            payload: variant
                                                .payload
                                                .iter()
                                                .map(ty)
                                                .collect::<Result<Vec<_>, StaticArtifactError>>()?,
                                        })
                                    })
                                    .collect::<Result<Vec<_>, StaticArtifactError>>()?,
                            }
                        }
                    },
                })
            })
            .collect::<Result<Vec<_>, StaticArtifactError>>()?;
        let mut keyed = definitions
            .drain(..)
            .map(|definition| Ok((write_definition_key(&definition)?, definition)))
            .collect::<Result<Vec<_>, StaticArtifactError>>()?;
        keyed.sort_by(|left, right| left.0.cmp(&right.0));
        let contract = CanonicalContract {
            root: ty(&contract.root)?,
            definitions: keyed
                .into_iter()
                .map(|(_, definition)| definition)
                .collect(),
        };
        contract.validate()?;
        Ok(contract)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterOccurrence {
    pub source_name: String,
    pub source_span: Span,
    pub protocol_ordinal: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewriteEntry {
    pub source_span: Span,
    pub wire_span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingEntry {
    pub params_field_ordinal: u32,
    pub source_name: String,
    pub protocol_ordinal: u32,
    pub field_type_fingerprint: Hash128,
    pub retention: BindRetention,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredParameterMeta {
    pub ordinal: u32,
    pub source_name: String,
    pub logical_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredColumnMeta {
    pub ordinal: u32,
    pub source_alias: String,
    pub logical_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedParameterMeta {
    pub ordinal: u32,
    pub native_type: Option<String>,
    pub native_type_id: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedColumnMeta {
    pub ordinal: u32,
    pub native_type: Option<String>,
    pub native_type_id: Option<i64>,
    pub origin_schema: Option<String>,
    pub origin_table: Option<String>,
    pub origin_column: Option<String>,
    pub nullable: MetaNullability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedQueryEvidence {
    pub prepare_identity: String,
    pub schema_identity: String,
    pub server_identity: String,
    pub parameters: Vec<CheckedParameterMeta>,
    pub columns: Vec<CheckedColumnMeta>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryMetaPlan {
    pub statement_class: MetaStatementClass,
    pub parameters: Vec<DeclaredParameterMeta>,
    pub columns: Vec<DeclaredColumnMeta>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedSpanEntry {
    pub decoded_span: Span,
    pub defining_file_span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedMetadata {
    pub policy: CheckPolicy,
    pub state: VerificationState,
    pub metadata_format_version: Option<u32>,
    pub metadata_digest: Option<Hash128>,
    pub query_evidence: Option<CheckedQueryEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriverEntry {
    pub driver: Driver,
    pub wire_sql: Vec<u8>,
    pub wire_sql_hash: Hash128,
    pub rewrite_format_version: u32,
    pub rewrites: Vec<RewriteEntry>,
    pub bindings: Vec<BindingEntry>,
    pub checked_metadata: CheckedMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticQueryArtifact {
    pub format_version: u32,
    pub unit: String,
    pub item: String,
    pub query_id: String,
    pub params_type: CanonicalContract,
    pub row_type: CanonicalContract,
    pub params_fingerprint: Hash128,
    pub row_fingerprint: Hash128,
    pub binder_abi_version: u32,
    pub decoder_abi_version: u32,
    pub driver_restriction: DriverRestriction,
    pub static_options: Vec<StaticOption>,
    pub source_identity: SqlSourceIdentity,
    pub source_sql: Vec<u8>,
    pub source_sql_hash: Hash128,
    pub occurrences: Vec<ParameterOccurrence>,
    pub driver_entries: Vec<DriverEntry>,
    pub decoded_span_map: Vec<DecodedSpanEntry>,
    pub query_meta_plan: QueryMetaPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticCommandArtifact {
    pub format_version: u32,
    pub unit: String,
    pub item: String,
    pub command_id: String,
    pub params_type: CanonicalContract,
    pub params_fingerprint: Hash128,
    pub binder_abi_version: u32,
    pub driver_restriction: DriverRestriction,
    pub static_options: Vec<StaticOption>,
    pub source_identity: SqlSourceIdentity,
    pub source_sql: Vec<u8>,
    pub source_sql_hash: Hash128,
    pub occurrences: Vec<ParameterOccurrence>,
    pub driver_entries: Vec<DriverEntry>,
    pub decoded_span_map: Vec<DecodedSpanEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaticArtifact {
    Query(StaticQueryArtifact),
    Command(StaticCommandArtifact),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaticArtifactError {
    UnknownVersion(u32),
    BadMagic,
    Truncated,
    BadTag { what: &'static str, tag: u8 },
    BadUtf8,
    TrailingBytes,
    Invalid(String),
}

impl std::fmt::Display for StaticArtifactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownVersion(v) => write!(f, "unknown static artifact format version {v}"),
            Self::BadMagic => write!(f, "static artifact has an invalid magic"),
            Self::Truncated => write!(f, "static artifact is truncated"),
            Self::BadTag { what, tag } => write!(f, "invalid {what} tag byte {tag}"),
            Self::BadUtf8 => write!(f, "static artifact contains invalid UTF-8"),
            Self::TrailingBytes => write!(f, "static artifact has trailing bytes"),
            Self::Invalid(reason) => write!(f, "invalid static artifact: {reason}"),
        }
    }
}

impl std::error::Error for StaticArtifactError {}

fn invalid(reason: impl Into<String>) -> StaticArtifactError {
    StaticArtifactError::Invalid(reason.into())
}

fn u32_len(n: usize) -> Result<u32, StaticArtifactError> {
    u32::try_from(n).map_err(|_| invalid("field exceeds u32::MAX"))
}

fn is_builtin_path(path: &str) -> bool {
    matches!(
        path,
        "()" | "bool"
            | "char"
            | "str"
            | "bytes"
            | "string"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "f32"
            | "f64"
            | "Option"
            | "Result"
            | "array"
            | "slice"
            | "tuple"
            | "region"
    )
}

fn write_type(w: &mut Writer, ty: &CanonicalType) -> Result<(), StaticArtifactError> {
    match ty {
        CanonicalType::Named { path, args } => {
            w.u8(0);
            w.str(path)?;
            w.seq(args, write_type)?;
        }
        CanonicalType::Tuple(elems) => {
            w.u8(1);
            w.seq(elems, write_type)?;
        }
        CanonicalType::Fn { params, result } => {
            w.u8(2);
            w.seq(params, write_type)?;
            write_type(w, result)?;
        }
    }
    Ok(())
}

fn validate_type(ty: &CanonicalType, depth: usize) -> Result<(), StaticArtifactError> {
    if depth > MAX_TYPE_DEPTH {
        return Err(invalid("canonical type nesting exceeds the format limit"));
    }
    match ty {
        CanonicalType::Named { path, args } => {
            if path.is_empty() || path.as_bytes().contains(&0) {
                return Err(invalid("canonical type path is empty or contains NUL"));
            }
            for arg in args {
                validate_type(arg, depth + 1)?;
            }
        }
        CanonicalType::Tuple(elems) => {
            for elem in elems {
                validate_type(elem, depth + 1)?;
            }
        }
        CanonicalType::Fn { params, result } => {
            for param in params {
                validate_type(param, depth + 1)?;
            }
            validate_type(result, depth + 1)?;
        }
    }
    Ok(())
}

fn write_definition_key(def: &CanonicalDefinition) -> Result<Vec<u8>, StaticArtifactError> {
    let mut w = Writer::new();
    w.str(&def.path)?;
    w.seq(&def.args, write_type)?;
    Ok(w.buf)
}

fn definition_types(def: &CanonicalDefinition) -> impl Iterator<Item = &CanonicalType> {
    let args = def.args.iter();
    let body = match &def.kind {
        CanonicalDefinitionBody::Struct { fields } => fields
            .iter()
            .flat_map(|field| std::iter::once(&field.ty))
            .collect::<Vec<_>>(),
        CanonicalDefinitionBody::Sum { variants } => variants
            .iter()
            .flat_map(|variant| variant.payload.iter())
            .collect::<Vec<_>>(),
    };
    args.chain(body)
}

fn lookup_definition<'a>(
    definitions: &'a [CanonicalDefinition],
    key: &[u8],
) -> Option<&'a CanonicalDefinition> {
    definitions
        .iter()
        .find(|def| write_definition_key(def).ok().as_deref() == Some(key))
}

impl CanonicalContract {
    pub fn fingerprint(&self) -> Result<Hash128, StaticArtifactError> {
        Ok(Hash128::of(&encode_contract(self)?))
    }

    pub fn validate(&self) -> Result<(), StaticArtifactError> {
        validate_type(&self.root, 0)?;
        let mut keys: Vec<Vec<u8>> = Vec::with_capacity(self.definitions.len());
        for (index, def) in self.definitions.iter().enumerate() {
            if def.path.is_empty() || is_builtin_path(&def.path) || def.path.as_bytes().contains(&0)
            {
                return Err(invalid(format!("definition {index} has an invalid path")));
            }
            for ty in definition_types(def) {
                validate_type(ty, 0)?;
            }
            match &def.kind {
                CanonicalDefinitionBody::Struct { fields } => {
                    for field in fields {
                        if field.name.is_empty() || field.name.as_bytes().contains(&0) {
                            return Err(invalid("struct field name is empty or contains NUL"));
                        }
                    }
                }
                CanonicalDefinitionBody::Sum { variants } => {
                    for variant in variants {
                        if variant.name.is_empty() || variant.name.as_bytes().contains(&0) {
                            return Err(invalid("sum variant name is empty or contains NUL"));
                        }
                    }
                }
            }
            let key = write_definition_key(def)?;
            if keys.last().is_some_and(|previous| previous >= &key) {
                return Err(invalid("canonical definitions are not strictly sorted"));
            }
            if keys.iter().any(|previous| previous == &key) {
                return Err(invalid("canonical definitions contain a duplicate key"));
            }
            keys.push(key);
        }

        // Reachability is checked with a borrowed, iterative traversal. Keeping references here
        // avoids cloning a complete graph and makes the definition lookup deterministic.
        let mut reachable = HashSet::new();
        let mut pending = vec![&self.root];
        while let Some(ty) = pending.pop() {
            match ty {
                CanonicalType::Named { path, args } => {
                    for arg in args {
                        pending.push(arg);
                    }
                    if !is_builtin_path(path) {
                        let mut key_writer = Writer::new();
                        key_writer.str(path)?;
                        key_writer.seq(args, write_type)?;
                        let key = key_writer.buf;
                        if !reachable.insert(key.clone()) {
                            continue;
                        }
                        let Some(def) = lookup_definition(&self.definitions, &key) else {
                            return Err(invalid(format!("missing reachable definition `{path}`")));
                        };
                        for child in definition_types(def) {
                            pending.push(child);
                        }
                    }
                }
                CanonicalType::Tuple(elems) => pending.extend(elems),
                CanonicalType::Fn { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
            }
        }
        for key in keys {
            if !reachable.contains(&key) {
                return Err(invalid(
                    "canonical definitions contain an unreachable entry",
                ));
            }
        }
        Ok(())
    }

    /// Project one field/root type into its exact reachable structural sub-contract.
    pub fn project(&self, root: &CanonicalType) -> Result<Self, StaticArtifactError> {
        project_contract(self, root)
    }
}

impl CanonicalType {
    pub fn spelling(&self) -> String {
        canonical_type_spelling(self)
    }
}

fn encode_contract(contract: &CanonicalContract) -> Result<Vec<u8>, StaticArtifactError> {
    contract.validate()?;
    let mut w = Writer::new();
    write_type(&mut w, &contract.root)?;
    w.seq(&contract.definitions, write_definition)?;
    Ok(w.buf)
}

fn write_definition(w: &mut Writer, def: &CanonicalDefinition) -> Result<(), StaticArtifactError> {
    w.str(&def.path)?;
    w.seq(&def.args, write_type)?;
    match &def.kind {
        CanonicalDefinitionBody::Struct { fields } => {
            w.u8(CanonicalDefinitionKind::Struct as u8);
            w.seq(fields, |w, field| {
                w.str(&field.name)?;
                write_type(w, &field.ty)
            })?;
        }
        CanonicalDefinitionBody::Sum { variants } => {
            w.u8(CanonicalDefinitionKind::Sum as u8);
            w.seq(variants, |w, variant| {
                w.str(&variant.name)?;
                w.seq(&variant.payload, write_type)
            })?;
        }
    }
    Ok(())
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

    fn u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    fn hash(&mut self, value: Hash128) {
        self.u64(value.lo);
        self.u64(value.hi);
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<(), StaticArtifactError> {
        self.u32(u32_len(bytes.len())?);
        self.buf.extend_from_slice(bytes);
        Ok(())
    }

    fn str(&mut self, value: &str) -> Result<(), StaticArtifactError> {
        self.bytes(value.as_bytes())
    }

    fn opt<T>(
        &mut self,
        value: Option<&T>,
        mut f: impl FnMut(&mut Self, &T) -> Result<(), StaticArtifactError>,
    ) -> Result<(), StaticArtifactError> {
        match value {
            Some(value) => {
                self.u8(1);
                f(self, value)
            }
            None => {
                self.u8(0);
                Ok(())
            }
        }
    }

    fn seq<T>(
        &mut self,
        values: &[T],
        mut f: impl FnMut(&mut Self, &T) -> Result<(), StaticArtifactError>,
    ) -> Result<(), StaticArtifactError> {
        self.u32(u32_len(values.len())?);
        for value in values {
            f(self, value)?;
        }
        Ok(())
    }
}

fn write_hash(w: &mut Writer, value: Hash128) {
    w.hash(value);
}

fn write_span(w: &mut Writer, span: Span) {
    w.u32(span.start);
    w.u32(span.end);
}

fn write_source_identity(
    w: &mut Writer,
    identity: &SqlSourceIdentity,
) -> Result<(), StaticArtifactError> {
    match identity {
        SqlSourceIdentity::File { logical_path } => {
            w.u8(0);
            w.str(logical_path)?;
        }
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => {
            w.u8(1);
            w.str(query_or_command_id)?;
        }
    }
    Ok(())
}

fn write_option(w: &mut Writer, option: &StaticOption) -> Result<(), StaticArtifactError> {
    w.u8(option.owner as u8);
    match (&option.owner, &option.value) {
        (StaticOptionOwner::Common, StaticOptionValue::Check { policy }) => {
            w.u8(0);
            w.u8(*policy as u8);
        }
        (
            StaticOptionOwner::SQLite,
            StaticOptionValue::SQLiteRequireVersionAtLeast {
                major,
                minor,
                patch,
            },
        ) => {
            w.u8(0);
            w.u32(*major);
            w.u32(*minor);
            w.u32(*patch);
        }
        (
            StaticOptionOwner::PostgreSQL,
            StaticOptionValue::PostgreSQLParameterType {
                parameter_name,
                canonical_type_name,
            },
        ) => {
            w.u8(0);
            w.str(parameter_name)?;
            w.str(canonical_type_name)?;
        }
        _ => return Err(invalid("static option owner/value mismatch")),
    }
    Ok(())
}

fn option_key(option: &StaticOption) -> Result<Vec<u8>, StaticArtifactError> {
    let mut w = Writer::new();
    write_option(&mut w, option)?;
    Ok(w.buf)
}

fn write_occurrence(
    w: &mut Writer,
    occurrence: &ParameterOccurrence,
) -> Result<(), StaticArtifactError> {
    w.str(&occurrence.source_name)?;
    write_span(w, occurrence.source_span);
    w.u32(occurrence.protocol_ordinal);
    Ok(())
}

fn write_rewrite(w: &mut Writer, rewrite: &RewriteEntry) {
    write_span(w, rewrite.source_span);
    write_span(w, rewrite.wire_span);
}

fn write_binding(w: &mut Writer, binding: &BindingEntry) -> Result<(), StaticArtifactError> {
    w.u32(binding.params_field_ordinal);
    w.str(&binding.source_name)?;
    w.u32(binding.protocol_ordinal);
    write_hash(w, binding.field_type_fingerprint);
    w.u8(binding.retention as u8);
    Ok(())
}

fn write_declared_parameter(
    w: &mut Writer,
    value: &DeclaredParameterMeta,
) -> Result<(), StaticArtifactError> {
    w.u32(value.ordinal);
    w.str(&value.source_name)?;
    w.str(&value.logical_type)?;
    Ok(())
}

fn write_declared_column(
    w: &mut Writer,
    value: &DeclaredColumnMeta,
) -> Result<(), StaticArtifactError> {
    w.u32(value.ordinal);
    w.str(&value.source_alias)?;
    w.str(&value.logical_type)?;
    Ok(())
}

fn write_checked_parameter(
    w: &mut Writer,
    value: &CheckedParameterMeta,
) -> Result<(), StaticArtifactError> {
    w.u32(value.ordinal);
    w.opt(value.native_type.as_ref(), |w, value| w.str(value))?;
    w.opt(value.native_type_id.as_ref(), |w, value| {
        w.i64(*value);
        Ok(())
    })?;
    Ok(())
}

fn write_checked_column(
    w: &mut Writer,
    value: &CheckedColumnMeta,
) -> Result<(), StaticArtifactError> {
    w.u32(value.ordinal);
    w.opt(value.native_type.as_ref(), |w, value| w.str(value))?;
    w.opt(value.native_type_id.as_ref(), |w, value| {
        w.i64(*value);
        Ok(())
    })?;
    w.opt(value.origin_schema.as_ref(), |w, value| w.str(value))?;
    w.opt(value.origin_table.as_ref(), |w, value| w.str(value))?;
    w.opt(value.origin_column.as_ref(), |w, value| w.str(value))?;
    w.u8(value.nullable as u8);
    Ok(())
}

fn write_evidence(w: &mut Writer, value: &CheckedQueryEvidence) -> Result<(), StaticArtifactError> {
    w.str(&value.prepare_identity)?;
    w.str(&value.schema_identity)?;
    w.str(&value.server_identity)?;
    w.seq(&value.parameters, write_checked_parameter)?;
    w.seq(&value.columns, write_checked_column)?;
    Ok(())
}

fn write_checked_metadata(
    w: &mut Writer,
    value: &CheckedMetadata,
    query: bool,
) -> Result<(), StaticArtifactError> {
    w.u8(value.policy as u8);
    w.u8(value.state as u8);
    match value.state {
        VerificationState::Declared => {}
        VerificationState::DatabaseChecked => {
            let format = value
                .metadata_format_version
                .ok_or_else(|| invalid("database-checked metadata has no format version"))?;
            let digest = value
                .metadata_digest
                .ok_or_else(|| invalid("database-checked metadata has no digest"))?;
            w.u32(format);
            w.hash(digest);
            if query {
                w.opt(value.query_evidence.as_ref(), write_evidence)?;
            } else if value.query_evidence.is_some() {
                return Err(invalid("command metadata cannot carry Query evidence"));
            }
        }
    }
    Ok(())
}

fn write_driver_entry(
    w: &mut Writer,
    value: &DriverEntry,
    query: bool,
) -> Result<(), StaticArtifactError> {
    w.u8(value.driver as u8);
    w.bytes(&value.wire_sql)?;
    w.hash(value.wire_sql_hash);
    w.u32(value.rewrite_format_version);
    w.seq(&value.rewrites, |w, value| {
        write_rewrite(w, value);
        Ok(())
    })?;
    w.seq(&value.bindings, write_binding)?;
    write_checked_metadata(w, &value.checked_metadata, query)?;
    Ok(())
}

fn write_decoded_span(w: &mut Writer, value: &DecodedSpanEntry) {
    write_span(w, value.decoded_span);
    write_span(w, value.defining_file_span);
}

fn write_meta_plan(w: &mut Writer, value: &QueryMetaPlan) -> Result<(), StaticArtifactError> {
    w.u8(value.statement_class as u8);
    w.seq(&value.parameters, write_declared_parameter)?;
    w.seq(&value.columns, write_declared_column)?;
    Ok(())
}

fn span_within(span: Span, len: usize, what: &str) -> Result<(), StaticArtifactError> {
    if span.start >= span.end || usize::try_from(span.end).ok().is_none_or(|end| end > len) {
        return Err(invalid(format!(
            "{what} span is empty or outside its byte field"
        )));
    }
    Ok(())
}

fn validate_utf8_sql(bytes: &[u8], what: &str) -> Result<(), StaticArtifactError> {
    if std::str::from_utf8(bytes).is_err() {
        return Err(invalid(format!("{what} is not UTF-8")));
    }
    if bytes.contains(&0) {
        return Err(invalid(format!("{what} contains U+0000")));
    }
    Ok(())
}

fn validate_source_identity(
    identity: &SqlSourceIdentity,
    id: &str,
) -> Result<(), StaticArtifactError> {
    match identity {
        SqlSourceIdentity::File { logical_path } => {
            if logical_path.is_empty()
                || logical_path.starts_with('/')
                || logical_path.contains('\\')
                || logical_path
                    .split('/')
                    .any(|part| part.is_empty() || part == "..")
                || logical_path.as_bytes().contains(&0)
            {
                return Err(invalid(
                    "file source identity is not a root-relative logical path",
                ));
            }
        }
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => {
            if query_or_command_id != id {
                return Err(invalid(
                    "inline source identity does not equal the artifact id",
                ));
            }
        }
    }
    Ok(())
}

fn root_definition(
    contract: &CanonicalContract,
) -> Result<Option<&CanonicalDefinition>, StaticArtifactError> {
    let CanonicalType::Named { path, args } = &contract.root else {
        return Ok(None);
    };
    if is_builtin_path(path) {
        return Ok(None);
    }
    let mut w = Writer::new();
    w.str(path)?;
    w.seq(args, write_type)?;
    lookup_definition(&contract.definitions, &w.buf)
        .map(Some)
        .ok_or_else(|| invalid("contract root definition is missing"))
}

fn project_contract(
    contract: &CanonicalContract,
    root: &CanonicalType,
) -> Result<CanonicalContract, StaticArtifactError> {
    let mut keys = HashSet::new();
    let mut pending = vec![root];
    while let Some(ty) = pending.pop() {
        match ty {
            CanonicalType::Named { path, args } => {
                pending.extend(args);
                if !is_builtin_path(path) {
                    let mut w = Writer::new();
                    w.str(path)?;
                    w.seq(args, write_type)?;
                    if keys.insert(w.buf.clone()) {
                        let Some(def) = lookup_definition(&contract.definitions, &w.buf) else {
                            return Err(invalid("field contract references a missing definition"));
                        };
                        pending.extend(definition_types(def));
                    }
                }
            }
            CanonicalType::Tuple(elems) => pending.extend(elems),
            CanonicalType::Fn { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
        }
    }
    let definitions = contract
        .definitions
        .iter()
        .filter(|def| {
            write_definition_key(def)
                .ok()
                .is_some_and(|key| keys.contains(&key))
        })
        .cloned()
        .collect();
    let projected = CanonicalContract {
        root: root.clone(),
        definitions,
    };
    projected.validate()?;
    Ok(projected)
}

fn validate_contract_fingerprint(
    contract: &CanonicalContract,
    stored: Hash128,
    what: &str,
) -> Result<(), StaticArtifactError> {
    contract.validate()?;
    if contract.fingerprint()? != stored {
        return Err(invalid(format!("{what} fingerprint mismatch")));
    }
    Ok(())
}

fn common_policy(options: &[StaticOption]) -> Result<CheckPolicy, StaticArtifactError> {
    let mut policy = None;
    for option in options {
        if let (StaticOptionOwner::Common, StaticOptionValue::Check { policy: value }) =
            (&option.owner, &option.value)
            && policy.replace(*value).is_some()
        {
            return Err(invalid("duplicate Common/Check option"));
        }
    }
    policy.ok_or_else(|| invalid("static options must carry the effective Common/Check option"))
}

fn validate_options(
    options: &[StaticOption],
    restriction: DriverRestriction,
    params: &CanonicalContract,
) -> Result<CheckPolicy, StaticArtifactError> {
    let mut previous = None;
    let mut sqlite_native = false;
    let mut postgres_native = false;
    let mut sqlite_version = None;
    let mut postgres_parameter_types: HashMap<&str, &str> = HashMap::new();
    for option in options {
        let key = option_key(option)?;
        if previous.as_ref().is_some_and(|old: &Vec<u8>| old >= &key) {
            return Err(invalid("static options are not strictly sorted"));
        }
        previous = Some(key);
        match (&option.owner, &option.value) {
            (StaticOptionOwner::Common, StaticOptionValue::Check { .. }) => {}
            (
                StaticOptionOwner::SQLite,
                StaticOptionValue::SQLiteRequireVersionAtLeast {
                    major,
                    minor,
                    patch,
                },
            ) => {
                sqlite_native = true;
                let version = (*major, *minor, *patch);
                if sqlite_version
                    .replace(version)
                    .is_some_and(|old| old != version)
                {
                    return Err(invalid("conflicting SQLite version options"));
                }
            }
            (
                StaticOptionOwner::PostgreSQL,
                StaticOptionValue::PostgreSQLParameterType {
                    parameter_name,
                    canonical_type_name,
                },
            ) => {
                if parameter_name.is_empty() || canonical_type_name.is_empty() {
                    return Err(invalid("PostgreSQL static option has an empty name/type"));
                }
                postgres_native = true;
                if postgres_parameter_types
                    .insert(parameter_name, canonical_type_name)
                    .is_some_and(|old| old != canonical_type_name)
                {
                    return Err(invalid("conflicting PostgreSQL parameter type options"));
                }
            }
            _ => return Err(invalid("static option owner/value mismatch")),
        }
    }
    if sqlite_native && restriction != DriverRestriction::SQLiteOnly {
        return Err(invalid(
            "SQLite native options require SQLiteOnly restriction",
        ));
    }
    if postgres_native && restriction != DriverRestriction::PostgreSQLOnly {
        return Err(invalid(
            "PostgreSQL native options require PostgreSQLOnly restriction",
        ));
    }
    if postgres_native {
        let Some(root) = root_definition(params)? else {
            return Err(invalid(
                "PostgreSQL parameter options require a named Params struct",
            ));
        };
        let CanonicalDefinitionBody::Struct { fields } = &root.kind else {
            return Err(invalid(
                "PostgreSQL parameter options require a struct Params root",
            ));
        };
        let field_names: HashSet<&str> = fields.iter().map(|field| field.name.as_str()).collect();
        for option in options {
            if let StaticOptionValue::PostgreSQLParameterType { parameter_name, .. } = &option.value
                && !field_names.contains(parameter_name.as_str())
            {
                return Err(invalid(format!(
                    "PostgreSQL parameter option names unknown Params field `{parameter_name}`"
                )));
            }
        }
    }
    common_policy(options)
}

fn occurrence_protocols(
    occurrences: &[ParameterOccurrence],
    source_len: usize,
) -> Result<(Vec<String>, HashMap<String, u32>), StaticArtifactError> {
    let mut names = Vec::new();
    let mut ordinals = HashMap::new();
    let mut previous_end = None;
    for occurrence in occurrences {
        if occurrence.source_name.is_empty() || occurrence.source_name.as_bytes().contains(&0) {
            return Err(invalid("parameter occurrence has an empty/NUL source name"));
        }
        span_within(occurrence.source_span, source_len, "parameter occurrence")?;
        if previous_end.is_some_and(|end| occurrence.source_span.start < end) {
            return Err(invalid(
                "parameter occurrences overlap or are not source ordered",
            ));
        }
        previous_end = Some(occurrence.source_span.end);
        let expected = match ordinals.get(&occurrence.source_name) {
            Some(value) => *value,
            None => {
                let ordinal =
                    u32::try_from(names.len() + 1).map_err(|_| invalid("too many parameters"))?;
                names.push(occurrence.source_name.clone());
                ordinals.insert(occurrence.source_name.clone(), ordinal);
                ordinal
            }
        };
        if occurrence.protocol_ordinal != expected {
            return Err(invalid(
                "parameter occurrence protocol ordinals are not canonical",
            ));
        }
    }
    Ok((names, ordinals))
}

fn validate_meta_plan(
    plan: &QueryMetaPlan,
    names: &[String],
    row: &CanonicalContract,
) -> Result<(), StaticArtifactError> {
    for (index, value) in plan.parameters.iter().enumerate() {
        let expected =
            u32::try_from(index + 1).map_err(|_| invalid("too many QueryMeta parameters"))?;
        if value.ordinal != expected
            || value.source_name != names.get(index).cloned().unwrap_or_default()
        {
            return Err(invalid(
                "QueryMeta parameter ordinals/names are not canonical",
            ));
        }
        if value.logical_type.is_empty() || value.logical_type.as_bytes().contains(&0) {
            return Err(invalid("QueryMeta parameter logical type is empty/NUL"));
        }
    }
    if plan.parameters.len() != names.len() {
        return Err(invalid(
            "QueryMeta parameter count does not match occurrences",
        ));
    }
    let Some(root) = root_definition(row)? else {
        return Err(invalid("Row contract root must be a named struct"));
    };
    let CanonicalDefinitionBody::Struct { fields } = &root.kind else {
        return Err(invalid("Row contract root must be a struct"));
    };
    if fields.len() != plan.columns.len() {
        return Err(invalid(
            "QueryMeta column count does not match the Row contract",
        ));
    }
    for (index, (value, field)) in plan.columns.iter().zip(fields).enumerate() {
        let expected = u32::try_from(index).map_err(|_| invalid("too many QueryMeta columns"))?;
        if value.ordinal != expected
            || value.source_alias != field.name
            || value.logical_type != canonical_type_spelling(&field.ty)
        {
            return Err(invalid("QueryMeta columns do not match the Row contract"));
        }
    }
    Ok(())
}

fn canonical_type_spelling(ty: &CanonicalType) -> String {
    match ty {
        CanonicalType::Named { path, args } if args.is_empty() => path.clone(),
        CanonicalType::Named { path, args } => format!(
            "{path}<{}>",
            args.iter()
                .map(canonical_type_spelling)
                .collect::<Vec<_>>()
                .join(",")
        ),
        CanonicalType::Tuple(elements) => {
            let contents = elements
                .iter()
                .map(canonical_type_spelling)
                .collect::<Vec<_>>()
                .join(",");
            if elements.len() == 1 {
                format!("({contents},)")
            } else {
                format!("({contents})")
            }
        }
        CanonicalType::Fn { params, result } => format!(
            "fn({}) -> {}",
            params
                .iter()
                .map(canonical_type_spelling)
                .collect::<Vec<_>>()
                .join(","),
            canonical_type_spelling(result)
        ),
    }
}

fn expected_wire(
    driver: Driver,
    source_sql: &[u8],
    occurrences: &[ParameterOccurrence],
) -> Result<(Vec<u8>, Vec<Span>), StaticArtifactError> {
    let mut wire_sql = Vec::with_capacity(source_sql.len());
    let mut wire_spans = Vec::with_capacity(occurrences.len());
    let mut source_cursor = 0usize;
    for occurrence in occurrences {
        let start = usize::try_from(occurrence.source_span.start)
            .map_err(|_| invalid("parameter occurrence offset does not fit usize"))?;
        let end = usize::try_from(occurrence.source_span.end)
            .map_err(|_| invalid("parameter occurrence offset does not fit usize"))?;
        if start < source_cursor || end < start || end > source_sql.len() {
            return Err(invalid("parameter occurrence spans are not source ordered"));
        }
        let placeholder = format!(":{}", occurrence.source_name);
        if source_sql.get(start..end) != Some(placeholder.as_bytes()) {
            return Err(invalid(
                "parameter occurrence span does not contain its named placeholder",
            ));
        }
        wire_sql.extend_from_slice(&source_sql[source_cursor..start]);
        let wire_start = u32_len(wire_sql.len())?;
        match driver {
            Driver::SQLite => {
                wire_sql.extend_from_slice(&source_sql[start..end]);
            }
            Driver::PostgreSQL => {
                wire_sql.extend_from_slice(format!("${}", occurrence.protocol_ordinal).as_bytes());
            }
        }
        let wire_end = u32_len(wire_sql.len())?;
        wire_spans.push(Span {
            start: wire_start,
            end: wire_end,
        });
        source_cursor = end;
    }
    wire_sql.extend_from_slice(&source_sql[source_cursor..]);
    Ok((wire_sql, wire_spans))
}

fn validate_identity_hash(value: &str, what: &str) -> Result<(), StaticArtifactError> {
    if value.len() != 32
        || !value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(invalid(format!(
            "checked Query {what} is not a Hash128 hex identity"
        )));
    }
    Ok(())
}

fn validate_evidence(
    evidence: &CheckedQueryEvidence,
    plan: &QueryMetaPlan,
    row: &CanonicalContract,
) -> Result<(), StaticArtifactError> {
    validate_identity_hash(&evidence.prepare_identity, "prepare identity")?;
    validate_identity_hash(&evidence.schema_identity, "schema identity")?;
    validate_identity_hash(&evidence.server_identity, "server identity")?;
    if evidence.parameters.len() != plan.parameters.len()
        || evidence.columns.len() != plan.columns.len()
    {
        return Err(invalid(
            "checked Query evidence count does not match the declared plan",
        ));
    }
    for (index, parameter) in evidence.parameters.iter().enumerate() {
        if parameter.ordinal
            != u32::try_from(index + 1).map_err(|_| invalid("too many checked parameters"))?
        {
            return Err(invalid("checked parameter ordinals are not dense"));
        }
    }
    let Some(root) = root_definition(row)? else {
        return Err(invalid("Row contract root must be a named struct"));
    };
    let CanonicalDefinitionBody::Struct { fields } = &root.kind else {
        return Err(invalid("Row contract root must be a struct"));
    };
    for (index, column) in evidence.columns.iter().enumerate() {
        if column.ordinal
            != u32::try_from(index).map_err(|_| invalid("too many checked columns"))?
        {
            return Err(invalid("checked column ordinals are not dense"));
        }
        let Some(field) = fields.get(index) else {
            return Err(invalid("checked column is outside the Row contract"));
        };
        let option = matches!(
            &field.ty,
            CanonicalType::Named { path, args } if path == "Option" && args.len() == 1
        );
        if (column.nullable == MetaNullability::Yes && !option)
            || (column.nullable == MetaNullability::No && option)
        {
            return Err(invalid(
                "checked column nullability conflicts with the Row contract",
            ));
        }
    }
    Ok(())
}

fn validate_bindings(
    params: &CanonicalContract,
    occurrences: &[ParameterOccurrence],
    bindings: &[BindingEntry],
) -> Result<(), StaticArtifactError> {
    let names = occurrence_protocols(occurrences, usize::MAX)?.0;
    let Some(root) = root_definition(params)? else {
        return Err(invalid("Params contract root must be a named struct"));
    };
    let CanonicalDefinitionBody::Struct { fields } = &root.kind else {
        return Err(invalid("Params contract root must be a struct"));
    };
    if bindings.len() != fields.len() {
        return Err(invalid("binding count does not match Params fields"));
    }
    let mut seen_names = HashSet::new();
    let mut seen_protocols = HashSet::new();
    for (index, binding) in bindings.iter().enumerate() {
        if binding.params_field_ordinal
            != u32::try_from(index).map_err(|_| invalid("too many Params fields"))?
        {
            return Err(invalid("Params binding field ordinals are not dense"));
        }
        let field = fields
            .get(index)
            .ok_or_else(|| invalid("binding field ordinal is out of range"))?;
        if binding.source_name != field.name || !seen_names.insert(binding.source_name.clone()) {
            return Err(invalid(
                "binding names are not unique or declaration ordered",
            ));
        }
        if !names.contains(&binding.source_name) || !seen_protocols.insert(binding.protocol_ordinal)
        {
            return Err(invalid("binding names do not match parameter occurrences"));
        }
        let expected = names
            .iter()
            .position(|name| name == &binding.source_name)
            .and_then(|position| u32::try_from(position + 1).ok())
            .ok_or_else(|| invalid("binding source name has no protocol ordinal"))?;
        if binding.protocol_ordinal != expected {
            return Err(invalid(
                "binding protocol ordinals are not first-occurrence ordered",
            ));
        }
        let field_contract = project_contract(params, &field.ty)?;
        if binding.field_type_fingerprint != field_contract.fingerprint()? {
            return Err(invalid("binding field type fingerprint mismatch"));
        }
    }
    if seen_names.len() != names.len() || seen_protocols.len() != names.len() {
        return Err(invalid("Params fields and parameter occurrences differ"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_driver_entry(
    entry: &DriverEntry,
    restriction: DriverRestriction,
    source_sql: &[u8],
    occurrences: &[ParameterOccurrence],
    params: &CanonicalContract,
    row: Option<&CanonicalContract>,
    policy: CheckPolicy,
    query: bool,
    query_meta: Option<&QueryMetaPlan>,
) -> Result<(), StaticArtifactError> {
    if !restriction.allows(entry.driver) {
        return Err(invalid("driver entry is not permitted by the restriction"));
    }
    if entry.rewrite_format_version != REWRITE_FORMAT_VERSION {
        return Err(invalid("unsupported rewrite format version"));
    }
    let (expected_wire, expected_wire_spans) =
        expected_wire(entry.driver, source_sql, occurrences)?;
    if entry.wire_sql != expected_wire {
        return Err(invalid("wire SQL does not match the source rewrite"));
    }
    validate_utf8_sql(&entry.wire_sql, "wire SQL")?;
    if Hash128::of(&entry.wire_sql) != entry.wire_sql_hash {
        return Err(invalid("wire SQL hash mismatch"));
    }
    if entry.rewrites.len() != occurrences.len() {
        return Err(invalid(
            "rewrite count does not match parameter occurrences",
        ));
    }
    let mut previous_source = None;
    let mut previous_wire = None;
    for (index, rewrite) in entry.rewrites.iter().enumerate() {
        span_within(rewrite.source_span, source_sql.len(), "rewrite source")?;
        span_within(rewrite.wire_span, entry.wire_sql.len(), "rewrite wire")?;
        if previous_source.is_some_and(|end| rewrite.source_span.start < end)
            || previous_wire.is_some_and(|end| rewrite.wire_span.start < end)
        {
            return Err(invalid(
                "rewrite spans are not monotone and non-overlapping",
            ));
        }
        previous_source = Some(rewrite.source_span.end);
        previous_wire = Some(rewrite.wire_span.end);
        if rewrite.source_span != occurrences[index].source_span {
            return Err(invalid("rewrite source span does not match its occurrence"));
        }
        if rewrite.wire_span != expected_wire_spans[index] {
            return Err(invalid(
                "rewrite wire span does not match the source rewrite",
            ));
        }
    }
    validate_bindings(params, occurrences, &entry.bindings)?;
    if entry.checked_metadata.policy != policy {
        return Err(invalid(
            "checked metadata policy differs from static options",
        ));
    }
    if (policy == CheckPolicy::DeclaredOnly
        && entry.checked_metadata.state != VerificationState::Declared)
        || (policy == CheckPolicy::CheckedRequired
            && entry.checked_metadata.state != VerificationState::DatabaseChecked)
    {
        return Err(invalid(
            "verification state conflicts with the static check policy",
        ));
    }
    match entry.checked_metadata.state {
        VerificationState::Declared => {
            if entry.checked_metadata.metadata_format_version.is_some()
                || entry.checked_metadata.metadata_digest.is_some()
                || entry.checked_metadata.query_evidence.is_some()
            {
                return Err(invalid("Declared metadata carries checked evidence"));
            }
        }
        VerificationState::DatabaseChecked => {
            if entry.checked_metadata.metadata_format_version.is_none()
                || entry.checked_metadata.metadata_digest.is_none()
            {
                return Err(invalid("DatabaseChecked metadata is incomplete"));
            }
            if query {
                let Some(evidence) = &entry.checked_metadata.query_evidence else {
                    return Err(invalid(
                        "DatabaseChecked Query metadata has no checked evidence",
                    ));
                };
                let Some(plan) = query_meta else {
                    return Err(invalid("DatabaseChecked Query has no QueryMeta plan"));
                };
                let Some(row) = row else {
                    return Err(invalid("DatabaseChecked Query has no Row contract"));
                };
                validate_evidence(evidence, plan, row)?;
            } else if entry.checked_metadata.query_evidence.is_some() {
                return Err(invalid("command metadata cannot carry Query evidence"));
            }
        }
    }
    Ok(())
}

fn validate_decoded_spans(
    identity: &SqlSourceIdentity,
    map: &[DecodedSpanEntry],
    source_len: usize,
) -> Result<(), StaticArtifactError> {
    match identity {
        SqlSourceIdentity::File { .. } => {
            if !map.is_empty() {
                return Err(invalid(
                    "file source identity cannot carry an inline span map",
                ));
            }
        }
        SqlSourceIdentity::Inline { .. } => {
            let mut cursor = 0u32;
            for entry in map {
                span_within(entry.decoded_span, source_len, "decoded SQL")?;
                if entry.decoded_span.start != cursor {
                    return Err(invalid(
                        "inline decoded span map does not cover SQL contiguously",
                    ));
                }
                if entry.defining_file_span.start >= entry.defining_file_span.end {
                    return Err(invalid("inline defining span is empty"));
                }
                cursor = entry.decoded_span.end;
            }
            if usize::try_from(cursor).ok() != Some(source_len) {
                return Err(invalid(
                    "inline decoded span map does not cover every SQL byte",
                ));
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_common_fields(
    format_version: u32,
    unit: &str,
    item: &str,
    id: &str,
    params: &CanonicalContract,
    params_fingerprint: Hash128,
    restriction: DriverRestriction,
    options: &[StaticOption],
    identity: &SqlSourceIdentity,
    source_sql: &[u8],
    source_sql_hash: Hash128,
    occurrences: &[ParameterOccurrence],
    drivers: &[DriverEntry],
    decoded_spans: &[DecodedSpanEntry],
    query: bool,
    row: Option<(&CanonicalContract, Hash128)>,
    query_meta: Option<&QueryMetaPlan>,
) -> Result<(), StaticArtifactError> {
    if format_version != STATIC_ARTIFACT_FORMAT_VERSION {
        return Err(invalid("unsupported producer format version"));
    }
    if unit.is_empty()
        || item.is_empty()
        || unit.as_bytes().contains(&0)
        || item.as_bytes().contains(&0)
    {
        return Err(invalid("artifact unit/item is empty or contains NUL"));
    }
    let expected_id = format!("{unit}.{item}");
    if id != expected_id {
        return Err(invalid("artifact id does not equal unit.item"));
    }
    validate_contract_fingerprint(params, params_fingerprint, "Params")?;
    if query {
        let (row_contract, row_fingerprint) =
            row.ok_or_else(|| invalid("Query artifact has no Row contract"))?;
        validate_contract_fingerprint(row_contract, row_fingerprint, "Row")?;
    } else if row.is_some() {
        return Err(invalid("command artifact carries a Row contract"));
    }
    let policy = validate_options(options, restriction, params)?;
    validate_source_identity(identity, id)?;
    validate_utf8_sql(source_sql, "source SQL")?;
    if Hash128::of(source_sql) != source_sql_hash {
        return Err(invalid("source SQL hash mismatch"));
    }
    let (names, _) = occurrence_protocols(occurrences, source_sql.len())?;
    if let Some(plan) = query_meta {
        let row_contract = row
            .map(|(contract, _)| contract)
            .ok_or_else(|| invalid("QueryMeta plan without Row"))?;
        validate_meta_plan(plan, &names, row_contract)?;
    } else if query {
        return Err(invalid("Query artifact has no QueryMeta plan"));
    }
    validate_decoded_spans(identity, decoded_spans, source_sql.len())?;
    if drivers.len() != restriction.drivers().len() {
        return Err(invalid("driver entry count does not match restriction"));
    }
    for (entry, expected_driver) in drivers.iter().zip(restriction.drivers()) {
        if entry.driver != *expected_driver {
            return Err(invalid(
                "driver entries are not in canonical permitted-driver order",
            ));
        }
        validate_driver_entry(
            entry,
            restriction,
            source_sql,
            occurrences,
            params,
            row.map(|(contract, _)| contract),
            policy,
            query,
            query_meta,
        )?;
    }
    Ok(())
}

impl StaticQueryArtifact {
    pub fn validate(&self) -> Result<(), StaticArtifactError> {
        validate_common_fields(
            self.format_version,
            &self.unit,
            &self.item,
            &self.query_id,
            &self.params_type,
            self.params_fingerprint,
            self.driver_restriction,
            &self.static_options,
            &self.source_identity,
            &self.source_sql,
            self.source_sql_hash,
            &self.occurrences,
            &self.driver_entries,
            &self.decoded_span_map,
            true,
            Some((&self.row_type, self.row_fingerprint)),
            Some(&self.query_meta_plan),
        )
    }

    pub fn encode(&self) -> Result<Vec<u8>, StaticArtifactError> {
        self.validate()?;
        let mut w = Writer::new();
        w.buf.extend_from_slice(b"ALIGNQRY");
        w.u32(self.format_version);
        w.str(&self.unit)?;
        w.str(&self.item)?;
        w.str(&self.query_id)?;
        write_contract(&mut w, &self.params_type)?;
        write_contract(&mut w, &self.row_type)?;
        w.hash(self.params_fingerprint);
        w.hash(self.row_fingerprint);
        w.u32(self.binder_abi_version);
        w.u32(self.decoder_abi_version);
        w.u8(self.driver_restriction as u8);
        w.seq(&self.static_options, write_option)?;
        write_source_identity(&mut w, &self.source_identity)?;
        w.bytes(&self.source_sql)?;
        w.hash(self.source_sql_hash);
        w.seq(&self.occurrences, write_occurrence)?;
        w.seq(&self.driver_entries, |w, entry| {
            write_driver_entry(w, entry, true)
        })?;
        w.seq(&self.decoded_span_map, |w, entry| {
            write_decoded_span(w, entry);
            Ok(())
        })?;
        write_meta_plan(&mut w, &self.query_meta_plan)?;
        Ok(w.buf)
    }

    pub fn digest(&self) -> Result<Hash128, StaticArtifactError> {
        Ok(Hash128::of(&self.encode()?))
    }
}

impl StaticCommandArtifact {
    pub fn validate(&self) -> Result<(), StaticArtifactError> {
        validate_common_fields(
            self.format_version,
            &self.unit,
            &self.item,
            &self.command_id,
            &self.params_type,
            self.params_fingerprint,
            self.driver_restriction,
            &self.static_options,
            &self.source_identity,
            &self.source_sql,
            self.source_sql_hash,
            &self.occurrences,
            &self.driver_entries,
            &self.decoded_span_map,
            false,
            None,
            None,
        )
    }

    pub fn encode(&self) -> Result<Vec<u8>, StaticArtifactError> {
        self.validate()?;
        let mut w = Writer::new();
        w.buf.extend_from_slice(b"ALIGNCMD");
        w.u32(self.format_version);
        w.str(&self.unit)?;
        w.str(&self.item)?;
        w.str(&self.command_id)?;
        write_contract(&mut w, &self.params_type)?;
        w.hash(self.params_fingerprint);
        w.u32(self.binder_abi_version);
        w.u8(self.driver_restriction as u8);
        w.seq(&self.static_options, write_option)?;
        write_source_identity(&mut w, &self.source_identity)?;
        w.bytes(&self.source_sql)?;
        w.hash(self.source_sql_hash);
        w.seq(&self.occurrences, write_occurrence)?;
        w.seq(&self.driver_entries, |w, entry| {
            write_driver_entry(w, entry, false)
        })?;
        w.seq(&self.decoded_span_map, |w, entry| {
            write_decoded_span(w, entry);
            Ok(())
        })?;
        Ok(w.buf)
    }

    pub fn digest(&self) -> Result<Hash128, StaticArtifactError> {
        Ok(Hash128::of(&self.encode()?))
    }
}

fn write_contract(w: &mut Writer, contract: &CanonicalContract) -> Result<(), StaticArtifactError> {
    contract.validate()?;
    write_type(w, &contract.root)?;
    w.seq(&contract.definitions, write_definition)?;
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], StaticArtifactError> {
        let end = self
            .pos
            .checked_add(count)
            .ok_or(StaticArtifactError::Truncated)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(StaticArtifactError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, StaticArtifactError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, StaticArtifactError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| StaticArtifactError::Truncated)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, StaticArtifactError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| StaticArtifactError::Truncated)?,
        ))
    }

    fn i64(&mut self) -> Result<i64, StaticArtifactError> {
        Ok(i64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| StaticArtifactError::Truncated)?,
        ))
    }

    fn hash(&mut self) -> Result<Hash128, StaticArtifactError> {
        Ok(Hash128 {
            lo: self.u64()?,
            hi: self.u64()?,
        })
    }

    fn bytes(&mut self) -> Result<Vec<u8>, StaticArtifactError> {
        let len = usize::try_from(self.u32()?).map_err(|_| StaticArtifactError::Truncated)?;
        Ok(self.take(len)?.to_vec())
    }

    fn str(&mut self) -> Result<String, StaticArtifactError> {
        let bytes = self.bytes()?;
        String::from_utf8(bytes).map_err(|_| StaticArtifactError::BadUtf8)
    }

    fn opt<T>(
        &mut self,
        mut f: impl FnMut(&mut Self) -> Result<T, StaticArtifactError>,
    ) -> Result<Option<T>, StaticArtifactError> {
        match self.u8()? {
            0 => Ok(None),
            1 => f(self).map(Some),
            tag => Err(StaticArtifactError::BadTag {
                what: "option",
                tag,
            }),
        }
    }

    fn seq<T>(
        &mut self,
        mut f: impl FnMut(&mut Self) -> Result<T, StaticArtifactError>,
    ) -> Result<Vec<T>, StaticArtifactError> {
        let count = usize::try_from(self.u32()?).map_err(|_| StaticArtifactError::Truncated)?;
        // Every supported sequence element consumes at least one byte. This prevents a malicious
        // count from reserving an unbounded Vec before the first truncation is observed.
        if count > self.bytes.len().saturating_sub(self.pos) {
            return Err(StaticArtifactError::Truncated);
        }
        let mut values = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            values.push(f(self)?);
        }
        Ok(values)
    }

    fn finish(self) -> Result<(), StaticArtifactError> {
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(StaticArtifactError::TrailingBytes)
        }
    }
}

fn read_type(reader: &mut Reader<'_>, depth: usize) -> Result<CanonicalType, StaticArtifactError> {
    if depth > MAX_TYPE_DEPTH {
        return Err(invalid("canonical type nesting exceeds the format limit"));
    }
    match reader.u8()? {
        0 => {
            let path = reader.str()?;
            let args = reader.seq(|reader| read_type(reader, depth + 1))?;
            Ok(CanonicalType::Named { path, args })
        }
        1 => Ok(CanonicalType::Tuple(
            reader.seq(|reader| read_type(reader, depth + 1))?,
        )),
        2 => {
            let params = reader.seq(|reader| read_type(reader, depth + 1))?;
            let result = Box::new(read_type(reader, depth + 1)?);
            Ok(CanonicalType::Fn { params, result })
        }
        tag => Err(StaticArtifactError::BadTag {
            what: "canonical type",
            tag,
        }),
    }
}

fn read_definition(reader: &mut Reader<'_>) -> Result<CanonicalDefinition, StaticArtifactError> {
    let path = reader.str()?;
    let args = reader.seq(|reader| read_type(reader, 0))?;
    let kind = match reader.u8()? {
        0 => CanonicalDefinitionBody::Struct {
            fields: reader.seq(|reader| {
                Ok(CanonicalField {
                    name: reader.str()?,
                    ty: read_type(reader, 0)?,
                })
            })?,
        },
        1 => CanonicalDefinitionBody::Sum {
            variants: reader.seq(|reader| {
                Ok(CanonicalVariant {
                    name: reader.str()?,
                    payload: reader.seq(|reader| read_type(reader, 0))?,
                })
            })?,
        },
        tag => {
            return Err(StaticArtifactError::BadTag {
                what: "canonical definition",
                tag,
            });
        }
    };
    Ok(CanonicalDefinition { path, args, kind })
}

fn read_contract(reader: &mut Reader<'_>) -> Result<CanonicalContract, StaticArtifactError> {
    let root = read_type(reader, 0)?;
    let definitions = reader.seq(read_definition)?;
    let contract = CanonicalContract { root, definitions };
    contract.validate()?;
    Ok(contract)
}

fn read_source_identity(reader: &mut Reader<'_>) -> Result<SqlSourceIdentity, StaticArtifactError> {
    match reader.u8()? {
        0 => Ok(SqlSourceIdentity::File {
            logical_path: reader.str()?,
        }),
        1 => Ok(SqlSourceIdentity::Inline {
            query_or_command_id: reader.str()?,
        }),
        tag => Err(StaticArtifactError::BadTag {
            what: "SQL source identity",
            tag,
        }),
    }
}

fn read_option(reader: &mut Reader<'_>) -> Result<StaticOption, StaticArtifactError> {
    let owner = match reader.u8()? {
        0 => StaticOptionOwner::Common,
        1 => StaticOptionOwner::SQLite,
        2 => StaticOptionOwner::PostgreSQL,
        tag => {
            return Err(StaticArtifactError::BadTag {
                what: "static option owner",
                tag,
            });
        }
    };
    let variant = reader.u8()?;
    let value = match (owner, variant) {
        (StaticOptionOwner::Common, 0) => StaticOptionValue::Check {
            policy: match reader.u8()? {
                0 => CheckPolicy::DeclaredOnly,
                1 => CheckPolicy::CheckedOptional,
                2 => CheckPolicy::CheckedRequired,
                tag => {
                    return Err(StaticArtifactError::BadTag {
                        what: "check policy",
                        tag,
                    });
                }
            },
        },
        (StaticOptionOwner::SQLite, 0) => StaticOptionValue::SQLiteRequireVersionAtLeast {
            major: reader.u32()?,
            minor: reader.u32()?,
            patch: reader.u32()?,
        },
        (StaticOptionOwner::PostgreSQL, 0) => StaticOptionValue::PostgreSQLParameterType {
            parameter_name: reader.str()?,
            canonical_type_name: reader.str()?,
        },
        _ => {
            return Err(StaticArtifactError::BadTag {
                what: "static option variant",
                tag: variant,
            });
        }
    };
    Ok(StaticOption { owner, value })
}

fn read_span(reader: &mut Reader<'_>) -> Result<Span, StaticArtifactError> {
    Ok(Span {
        start: reader.u32()?,
        end: reader.u32()?,
    })
}

fn read_occurrence(reader: &mut Reader<'_>) -> Result<ParameterOccurrence, StaticArtifactError> {
    Ok(ParameterOccurrence {
        source_name: reader.str()?,
        source_span: read_span(reader)?,
        protocol_ordinal: reader.u32()?,
    })
}

fn read_rewrite(reader: &mut Reader<'_>) -> Result<RewriteEntry, StaticArtifactError> {
    Ok(RewriteEntry {
        source_span: read_span(reader)?,
        wire_span: read_span(reader)?,
    })
}

fn read_binding(reader: &mut Reader<'_>) -> Result<BindingEntry, StaticArtifactError> {
    Ok(BindingEntry {
        params_field_ordinal: reader.u32()?,
        source_name: reader.str()?,
        protocol_ordinal: reader.u32()?,
        field_type_fingerprint: reader.hash()?,
        retention: match reader.u8()? {
            0 => BindRetention::BindValue,
            1 => BindRetention::BindCopy,
            tag => {
                return Err(StaticArtifactError::BadTag {
                    what: "bind retention",
                    tag,
                });
            }
        },
    })
}

fn read_declared_parameter(
    reader: &mut Reader<'_>,
) -> Result<DeclaredParameterMeta, StaticArtifactError> {
    Ok(DeclaredParameterMeta {
        ordinal: reader.u32()?,
        source_name: reader.str()?,
        logical_type: reader.str()?,
    })
}

fn read_declared_column(
    reader: &mut Reader<'_>,
) -> Result<DeclaredColumnMeta, StaticArtifactError> {
    Ok(DeclaredColumnMeta {
        ordinal: reader.u32()?,
        source_alias: reader.str()?,
        logical_type: reader.str()?,
    })
}

fn read_checked_parameter(
    reader: &mut Reader<'_>,
) -> Result<CheckedParameterMeta, StaticArtifactError> {
    Ok(CheckedParameterMeta {
        ordinal: reader.u32()?,
        native_type: reader.opt(|reader| reader.str())?,
        native_type_id: reader.opt(|reader| reader.i64())?,
    })
}

fn read_checked_column(reader: &mut Reader<'_>) -> Result<CheckedColumnMeta, StaticArtifactError> {
    Ok(CheckedColumnMeta {
        ordinal: reader.u32()?,
        native_type: reader.opt(|reader| reader.str())?,
        native_type_id: reader.opt(|reader| reader.i64())?,
        origin_schema: reader.opt(|reader| reader.str())?,
        origin_table: reader.opt(|reader| reader.str())?,
        origin_column: reader.opt(|reader| reader.str())?,
        nullable: match reader.u8()? {
            0 => MetaNullability::Yes,
            1 => MetaNullability::No,
            2 => MetaNullability::Unknown,
            tag => {
                return Err(StaticArtifactError::BadTag {
                    what: "metadata nullability",
                    tag,
                });
            }
        },
    })
}

fn read_evidence(reader: &mut Reader<'_>) -> Result<CheckedQueryEvidence, StaticArtifactError> {
    Ok(CheckedQueryEvidence {
        prepare_identity: reader.str()?,
        schema_identity: reader.str()?,
        server_identity: reader.str()?,
        parameters: reader.seq(read_checked_parameter)?,
        columns: reader.seq(read_checked_column)?,
    })
}

fn read_checked_metadata(
    reader: &mut Reader<'_>,
    query: bool,
) -> Result<CheckedMetadata, StaticArtifactError> {
    let policy = match reader.u8()? {
        0 => CheckPolicy::DeclaredOnly,
        1 => CheckPolicy::CheckedOptional,
        2 => CheckPolicy::CheckedRequired,
        tag => {
            return Err(StaticArtifactError::BadTag {
                what: "metadata check policy",
                tag,
            });
        }
    };
    let state = match reader.u8()? {
        0 => VerificationState::Declared,
        1 => VerificationState::DatabaseChecked,
        tag => {
            return Err(StaticArtifactError::BadTag {
                what: "verification state",
                tag,
            });
        }
    };
    match state {
        VerificationState::Declared => Ok(CheckedMetadata {
            policy,
            state,
            metadata_format_version: None,
            metadata_digest: None,
            query_evidence: None,
        }),
        VerificationState::DatabaseChecked => Ok(CheckedMetadata {
            policy,
            state,
            metadata_format_version: Some(reader.u32()?),
            metadata_digest: Some(reader.hash()?),
            query_evidence: if query {
                reader.opt(read_evidence)?
            } else {
                None
            },
        }),
    }
}

fn read_driver_entry(
    reader: &mut Reader<'_>,
    query: bool,
) -> Result<DriverEntry, StaticArtifactError> {
    let driver = match reader.u8()? {
        0 => Driver::SQLite,
        1 => Driver::PostgreSQL,
        tag => {
            return Err(StaticArtifactError::BadTag {
                what: "driver",
                tag,
            });
        }
    };
    Ok(DriverEntry {
        driver,
        wire_sql: reader.bytes()?,
        wire_sql_hash: reader.hash()?,
        rewrite_format_version: reader.u32()?,
        rewrites: reader.seq(read_rewrite)?,
        bindings: reader.seq(read_binding)?,
        checked_metadata: read_checked_metadata(reader, query)?,
    })
}

fn read_decoded_span(reader: &mut Reader<'_>) -> Result<DecodedSpanEntry, StaticArtifactError> {
    Ok(DecodedSpanEntry {
        decoded_span: read_span(reader)?,
        defining_file_span: read_span(reader)?,
    })
}

fn read_meta_plan(reader: &mut Reader<'_>) -> Result<QueryMetaPlan, StaticArtifactError> {
    Ok(QueryMetaPlan {
        statement_class: match reader.u8()? {
            0 => MetaStatementClass::Select,
            1 => MetaStatementClass::Dml,
            2 => MetaStatementClass::Ddl,
            3 => MetaStatementClass::Native,
            4 => MetaStatementClass::Unknown,
            tag => {
                return Err(StaticArtifactError::BadTag {
                    what: "metadata statement class",
                    tag,
                });
            }
        },
        parameters: reader.seq(read_declared_parameter)?,
        columns: reader.seq(read_declared_column)?,
    })
}

pub fn encode_static_query(artifact: &StaticQueryArtifact) -> Result<Vec<u8>, StaticArtifactError> {
    artifact.encode()
}

pub fn encode_static_command(
    artifact: &StaticCommandArtifact,
) -> Result<Vec<u8>, StaticArtifactError> {
    artifact.encode()
}

pub fn decode_static_query(bytes: &[u8]) -> Result<StaticQueryArtifact, StaticArtifactError> {
    let mut reader = Reader::new(bytes);
    if reader.take(8)? != b"ALIGNQRY" {
        return Err(StaticArtifactError::BadMagic);
    }
    let format_version = reader.u32()?;
    if format_version != STATIC_ARTIFACT_FORMAT_VERSION {
        return Err(StaticArtifactError::UnknownVersion(format_version));
    }
    let artifact = StaticQueryArtifact {
        format_version,
        unit: reader.str()?,
        item: reader.str()?,
        query_id: reader.str()?,
        params_type: read_contract(&mut reader)?,
        row_type: read_contract(&mut reader)?,
        params_fingerprint: reader.hash()?,
        row_fingerprint: reader.hash()?,
        binder_abi_version: reader.u32()?,
        decoder_abi_version: reader.u32()?,
        driver_restriction: match reader.u8()? {
            0 => DriverRestriction::AnySupportedDriver,
            1 => DriverRestriction::SQLiteOnly,
            2 => DriverRestriction::PostgreSQLOnly,
            tag => {
                return Err(StaticArtifactError::BadTag {
                    what: "driver restriction",
                    tag,
                });
            }
        },
        static_options: reader.seq(read_option)?,
        source_identity: read_source_identity(&mut reader)?,
        source_sql: reader.bytes()?,
        source_sql_hash: reader.hash()?,
        occurrences: reader.seq(read_occurrence)?,
        driver_entries: reader.seq(|reader| read_driver_entry(reader, true))?,
        decoded_span_map: reader.seq(read_decoded_span)?,
        query_meta_plan: read_meta_plan(&mut reader)?,
    };
    reader.finish()?;
    artifact.validate()?;
    Ok(artifact)
}

pub fn decode_static_command(bytes: &[u8]) -> Result<StaticCommandArtifact, StaticArtifactError> {
    let mut reader = Reader::new(bytes);
    if reader.take(8)? != b"ALIGNCMD" {
        return Err(StaticArtifactError::BadMagic);
    }
    let format_version = reader.u32()?;
    if format_version != STATIC_ARTIFACT_FORMAT_VERSION {
        return Err(StaticArtifactError::UnknownVersion(format_version));
    }
    let artifact = StaticCommandArtifact {
        format_version,
        unit: reader.str()?,
        item: reader.str()?,
        command_id: reader.str()?,
        params_type: read_contract(&mut reader)?,
        params_fingerprint: reader.hash()?,
        binder_abi_version: reader.u32()?,
        driver_restriction: match reader.u8()? {
            0 => DriverRestriction::AnySupportedDriver,
            1 => DriverRestriction::SQLiteOnly,
            2 => DriverRestriction::PostgreSQLOnly,
            tag => {
                return Err(StaticArtifactError::BadTag {
                    what: "driver restriction",
                    tag,
                });
            }
        },
        static_options: reader.seq(read_option)?,
        source_identity: read_source_identity(&mut reader)?,
        source_sql: reader.bytes()?,
        source_sql_hash: reader.hash()?,
        occurrences: reader.seq(read_occurrence)?,
        driver_entries: reader.seq(|reader| read_driver_entry(reader, false))?,
        decoded_span_map: reader.seq(read_decoded_span)?,
    };
    reader.finish()?;
    artifact.validate()?;
    Ok(artifact)
}

pub fn decode_static_artifact(bytes: &[u8]) -> Result<StaticArtifact, StaticArtifactError> {
    if bytes.len() < 8 {
        return Err(StaticArtifactError::Truncated);
    }
    match bytes.get(..8) {
        Some(b"ALIGNQRY") => decode_static_query(bytes).map(StaticArtifact::Query),
        Some(b"ALIGNCMD") => decode_static_command(bytes).map(StaticArtifact::Command),
        _ => Err(StaticArtifactError::BadMagic),
    }
}

pub fn static_artifact_digest(bytes: &[u8]) -> Result<Hash128, StaticArtifactError> {
    // Decode first so a caller cannot compute a cache identity for malformed bytes.
    let _ = decode_static_artifact(bytes)?;
    Ok(Hash128::of(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primitive(path: &str) -> CanonicalType {
        CanonicalType::Named {
            path: path.to_string(),
            args: Vec::new(),
        }
    }

    #[test]
    fn sema_contract_conversion_preserves_the_closed_v1_type_tags() {
        let contract = align_sema::StaticContract {
            root: align_sema::StaticContractType::Named {
                path: "app.Params".into(),
                args: Vec::new(),
            },
            definitions: vec![align_sema::StaticContractDefinition {
                path: "app.Params".into(),
                args: Vec::new(),
                kind: align_sema::StaticContractDefinitionBody::Struct {
                    fields: vec![align_sema::StaticContractField {
                        name: "digest".into(),
                        ty: align_sema::StaticContractType::FixedArray {
                            element: Box::new(align_sema::StaticContractType::Named {
                                path: "u8".into(),
                                args: Vec::new(),
                            }),
                            length: 4,
                        },
                    }],
                },
            }],
        };
        assert!(matches!(
            CanonicalContract::try_from(&contract),
            Err(StaticArtifactError::Invalid(reason))
                if reason.contains("not part of the static artifact v1 type contract")
        ));
    }

    #[test]
    fn contract_fingerprint_rejects_unreachable_definition() {
        let contract = CanonicalContract {
            root: primitive("app.Params"),
            definitions: vec![
                CanonicalDefinition {
                    path: "app.Params".into(),
                    args: Vec::new(),
                    kind: CanonicalDefinitionBody::Struct { fields: Vec::new() },
                },
                CanonicalDefinition {
                    path: "app.Unused".into(),
                    args: Vec::new(),
                    kind: CanonicalDefinitionBody::Struct { fields: Vec::new() },
                },
            ],
        };
        assert!(matches!(
            contract.validate(),
            Err(StaticArtifactError::Invalid(_))
        ));
    }

    #[test]
    fn malformed_magic_and_trailing_bytes_fail_closed() {
        assert_eq!(
            decode_static_artifact(b"bad"),
            Err(StaticArtifactError::Truncated)
        );
        let mut bytes = b"ALIGNQRY".to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            decode_static_artifact(&bytes),
            Err(StaticArtifactError::Truncated)
        );
    }

    #[test]
    fn nested_reader_tags_and_counts_fail_closed() {
        let mut option = Reader::new(&[3]);
        assert_eq!(
            read_option(&mut option),
            Err(StaticArtifactError::BadTag {
                what: "static option owner",
                tag: 3,
            })
        );
        let mut ty = Reader::new(&[9]);
        assert_eq!(
            read_type(&mut ty, 0),
            Err(StaticArtifactError::BadTag {
                what: "canonical type",
                tag: 9,
            })
        );
        let mut sequence = Reader::new(&[0xff, 0xff, 0xff, 0xff]);
        assert_eq!(
            sequence.seq(|reader| reader.u8()),
            Err(StaticArtifactError::Truncated)
        );
    }
}
