//! Q1/D1 static Query/command artifact formation.

use crate::{
    ResolvedStaticInput, ResolvedStaticInputs, StaticDescriptor, StaticDescriptorConsumer,
    static_inputs::{MetadataState, MetadataStatementKind, ParsedCheckedMetadata},
};
use align_interface::{
    BINDER_ABI_VERSION, BindRetention, BindingEntry, CanonicalContract, CanonicalDefinitionBody,
    CanonicalType, CheckPolicy, CheckedColumnMeta, CheckedMetadata, CheckedParameterMeta,
    CheckedQueryEvidence, DECODER_ABI_VERSION, DeclaredColumnMeta, DeclaredParameterMeta, Driver,
    DriverEntry, Hash128, MetaNullability, MetaStatementClass, ParameterOccurrence, QueryMetaPlan,
    REWRITE_FORMAT_VERSION, RewriteEntry, STATIC_ARTIFACT_FORMAT_VERSION, Span, StaticArtifact,
    StaticArtifactError, StaticCommandArtifact, StaticOption, StaticOptionOwner, StaticOptionValue,
    StaticQueryArtifact, VerificationState, static_artifact_digest, static_options_hash,
};
use align_sema::{StaticCheckPolicy, StaticDescriptorOption};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltStaticArtifact {
    pub descriptor_id: String,
    pub artifact: StaticArtifact,
    pub bytes: Vec<u8>,
    pub digest: Hash128,
    pub runtime: crate::static_runtime::GeneratedStaticRuntime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticArtifactBuildError {
    pub descriptor_id: String,
    pub reason: String,
}

impl std::fmt::Display for StaticArtifactBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cannot build static artifact `{}`: {}",
            self.descriptor_id, self.reason
        )
    }
}

impl std::error::Error for StaticArtifactBuildError {}

fn fail(descriptor: &StaticDescriptor, reason: impl Into<String>) -> StaticArtifactBuildError {
    StaticArtifactBuildError {
        descriptor_id: descriptor.descriptor_id.clone(),
        reason: reason.into(),
    }
}

fn interface_error(
    descriptor: &StaticDescriptor,
    error: StaticArtifactError,
) -> StaticArtifactBuildError {
    fail(descriptor, error.to_string())
}

fn identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn identifier_continue(byte: u8) -> bool {
    identifier_start(byte) || byte.is_ascii_digit()
}

fn skip_quoted(sql: &[u8], mut index: usize, quote: u8) -> Result<usize, String> {
    index += 1;
    while index < sql.len() {
        if sql[index] == quote {
            if sql.get(index + 1) == Some(&quote) {
                index += 2;
                continue;
            }
            return Ok(index + 1);
        }
        index += 1;
    }
    Err("SQL contains an unterminated quoted token".to_string())
}

fn skip_escape_string(sql: &[u8], mut index: usize) -> Result<usize, String> {
    index += 1;
    while index < sql.len() {
        match sql[index] {
            b'\\' => {
                index = index
                    .checked_add(2)
                    .ok_or_else(|| "SQL escape-string offset overflow".to_string())?;
            }
            b'\'' if sql.get(index + 1) == Some(&b'\'') => index += 2,
            b'\'' => return Ok(index + 1),
            _ => index += 1,
        }
    }
    Err("SQL contains an unterminated escape string".to_string())
}

fn statement_class(keyword: Option<&str>) -> MetaStatementClass {
    match keyword {
        Some("SELECT" | "VALUES") => MetaStatementClass::Select,
        Some("INSERT" | "UPDATE" | "DELETE" | "MERGE") => MetaStatementClass::Dml,
        Some("CREATE" | "ALTER" | "DROP" | "TRUNCATE") => MetaStatementClass::Ddl,
        Some("PRAGMA" | "EXPLAIN") => MetaStatementClass::Native,
        _ => MetaStatementClass::Unknown,
    }
}

fn dollar_delimiter(sql: &[u8], start: usize) -> Option<&[u8]> {
    if sql.get(start) != Some(&b'$') {
        return None;
    }
    if sql.get(start + 1) == Some(&b'$') {
        return sql.get(start..=start + 1);
    }
    if !sql
        .get(start + 1)
        .is_some_and(|byte| identifier_start(*byte))
    {
        return None;
    }
    let mut end = start + 2;
    while end < sql.len() && identifier_continue(sql[end]) {
        end += 1;
    }
    (sql.get(end) == Some(&b'$')).then(|| &sql[start..=end])
}

#[derive(Debug)]
struct SqlScan {
    occurrences: Vec<ParameterOccurrence>,
    statement_class: MetaStatementClass,
}

fn scan_sql(sql: &[u8]) -> Result<SqlScan, String> {
    let mut index = 0usize;
    let mut occurrences = Vec::new();
    let mut ordinals = HashMap::<String, u32>::new();
    let mut first_keyword = None::<String>;
    let mut with_main_keyword = None::<String>;
    let mut parenthesis_depth = 0u32;
    let mut cte_group_closed = false;
    let mut statement_ended = false;
    let mut has_statement_token = false;
    while index < sql.len() {
        let byte = sql[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if sql.get(index..index + 2) == Some(b"--") {
            index += 2;
            while index < sql.len() && sql[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if sql.get(index..index + 2) == Some(b"/*") {
            let mut depth = 1u32;
            index += 2;
            while index < sql.len() && depth != 0 {
                if sql.get(index..index + 2) == Some(b"/*") {
                    depth = depth
                        .checked_add(1)
                        .ok_or_else(|| "SQL block-comment nesting overflow".to_string())?;
                    index += 2;
                } else if sql.get(index..index + 2) == Some(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            if depth != 0 {
                return Err("SQL contains an unterminated block comment".to_string());
            }
            continue;
        }
        if statement_ended {
            return Err("a static Query/command must contain exactly one statement".to_string());
        }
        if matches!(byte, b'E' | b'e')
            && sql.get(index + 1) == Some(&b'\'')
            && (index == 0 || !identifier_continue(sql[index - 1]))
        {
            has_statement_token = true;
            index = skip_escape_string(sql, index + 1)?;
            continue;
        }
        if matches!(byte, b'\'' | b'"' | b'`') {
            has_statement_token = true;
            index = skip_quoted(sql, index, byte)?;
            continue;
        }
        if byte == b'[' {
            has_statement_token = true;
            index = skip_quoted(sql, index, b']')?;
            continue;
        }
        if byte == b'$' {
            if let Some(delimiter) = dollar_delimiter(sql, index) {
                let content = index + delimiter.len();
                let Some(relative_end) = sql[content..]
                    .windows(delimiter.len())
                    .position(|window| window == delimiter)
                else {
                    return Err("SQL contains an unterminated dollar-quoted string".to_string());
                };
                has_statement_token = true;
                index = content + relative_end + delimiter.len();
                continue;
            }
            if sql.get(index + 1).is_some_and(u8::is_ascii_digit) {
                return Err("portable static SQL cannot use `$n` placeholders".to_string());
            }
        }
        if byte == b'?' {
            return Err("portable static SQL cannot use `?` placeholders".to_string());
        }
        if byte == b'@'
            && sql
                .get(index + 1)
                .is_some_and(|next| identifier_start(*next))
        {
            return Err("portable static SQL cannot use `@name` placeholders".to_string());
        }
        if byte == b':' {
            if sql.get(index + 1) == Some(&b':') {
                has_statement_token = true;
                index += 2;
                continue;
            }
            if sql
                .get(index + 1)
                .is_some_and(|next| identifier_start(*next))
            {
                let start = index;
                index += 2;
                while index < sql.len() && identifier_continue(sql[index]) {
                    index += 1;
                }
                let name = std::str::from_utf8(&sql[start + 1..index])
                    .map_err(|_| "placeholder name is not ASCII".to_string())?
                    .to_string();
                let ordinal = match ordinals.get(&name).copied() {
                    Some(ordinal) => ordinal,
                    None => {
                        let ordinal = u32::try_from(ordinals.len() + 1)
                            .map_err(|_| "too many SQL parameters".to_string())?;
                        ordinals.insert(name.clone(), ordinal);
                        ordinal
                    }
                };
                occurrences.push(ParameterOccurrence {
                    source_name: name,
                    source_span: Span {
                        start: u32::try_from(start)
                            .map_err(|_| "SQL source exceeds u32 offsets".to_string())?,
                        end: u32::try_from(index)
                            .map_err(|_| "SQL source exceeds u32 offsets".to_string())?,
                    },
                    protocol_ordinal: ordinal,
                });
                has_statement_token = true;
                continue;
            }
        }
        if byte == b';' {
            if !has_statement_token {
                return Err("a static Query/command cannot be empty".to_string());
            }
            statement_ended = true;
            index += 1;
            continue;
        }
        if identifier_start(byte) {
            let start = index;
            index += 1;
            while index < sql.len() && identifier_continue(sql[index]) {
                index += 1;
            }
            let keyword = String::from_utf8_lossy(&sql[start..index]).to_ascii_uppercase();
            if first_keyword.is_none() {
                first_keyword = Some(keyword);
            } else if parenthesis_depth == 0
                && first_keyword.as_deref() == Some("WITH")
                && cte_group_closed
                && statement_class(Some(&keyword)) != MetaStatementClass::Unknown
            {
                with_main_keyword = Some(keyword);
            }
            has_statement_token = true;
            continue;
        }
        if byte == b'(' {
            parenthesis_depth = parenthesis_depth
                .checked_add(1)
                .ok_or_else(|| "SQL parenthesis nesting overflow".to_string())?;
        } else if byte == b')' && parenthesis_depth != 0 {
            parenthesis_depth -= 1;
            if parenthesis_depth == 0 && first_keyword.as_deref() == Some("WITH") {
                cte_group_closed = true;
            }
        } else if byte == b','
            && parenthesis_depth == 0
            && first_keyword.as_deref() == Some("WITH")
            && with_main_keyword.is_none()
        {
            cte_group_closed = false;
        }
        has_statement_token = true;
        index += 1;
    }
    if !has_statement_token {
        return Err("a static Query/command cannot be empty".to_string());
    }
    let statement_class = if first_keyword.as_deref() == Some("WITH") {
        statement_class(with_main_keyword.as_deref())
    } else {
        statement_class(first_keyword.as_deref())
    };
    Ok(SqlScan {
        occurrences,
        statement_class,
    })
}

pub(crate) fn root_fields(
    contract: &CanonicalContract,
) -> Result<&[align_interface::CanonicalField], String> {
    let CanonicalType::Named { path, args } = &contract.root else {
        return Err("contract root is not a named struct".to_string());
    };
    let definition = contract
        .definitions
        .iter()
        .find(|definition| definition.path == *path && definition.args == *args)
        .ok_or_else(|| "contract root definition is missing".to_string())?;
    match &definition.kind {
        CanonicalDefinitionBody::Struct { fields } => Ok(fields),
        CanonicalDefinitionBody::Sum { .. } => Err("contract root is not a struct".to_string()),
    }
}

pub(crate) fn static_statement_class(sql: &[u8]) -> Result<MetaStatementClass, String> {
    scan_sql(sql).map(|scan| scan.statement_class)
}

fn bind_retention(ty: &CanonicalType) -> Result<BindRetention, String> {
    match ty {
        CanonicalType::Named { path, args } if path == "Option" && args.len() == 1 => {
            bind_retention(&args[0])
        }
        CanonicalType::Named { path, args }
            if args.is_empty()
                && matches!(
                    path.as_str(),
                    "bool" | "i16" | "i32" | "i64" | "f32" | "f64"
                ) =>
        {
            Ok(BindRetention::BindValue)
        }
        CanonicalType::Named { path, args }
            if args.is_empty() && matches!(path.as_str(), "str" | "string") =>
        {
            Ok(BindRetention::BindCopy)
        }
        CanonicalType::Named { path, args }
            if matches!(path.as_str(), "slice" | "array")
                && matches!(args.as_slice(), [CanonicalType::Named { path, args }] if path == "u8" && args.is_empty()) =>
        {
            Ok(BindRetention::BindCopy)
        }
        _ => Err(format!(
            "unsupported static Params field type `{}`",
            ty.spelling()
        )),
    }
}

fn validate_row_type(ty: &CanonicalType) -> Result<(), String> {
    match ty {
        CanonicalType::Named { path, args } if path == "Option" && args.len() == 1 => {
            validate_row_type(&args[0])
        }
        CanonicalType::Named { path, args }
            if args.is_empty()
                && matches!(
                    path.as_str(),
                    "bool" | "i16" | "i32" | "i64" | "f32" | "f64" | "str"
                ) =>
        {
            Ok(())
        }
        CanonicalType::Named { path, args }
            if path == "slice"
                && matches!(args.as_slice(), [CanonicalType::Named { path, args }] if path == "u8" && args.is_empty()) =>
        {
            Ok(())
        }
        _ => Err(format!(
            "unsupported static Row field type `{}`",
            ty.spelling()
        )),
    }
}

fn options(descriptor: &StaticDescriptor) -> Vec<StaticOption> {
    let mut options = descriptor
        .static_options
        .iter()
        .map(|option| match option {
            StaticDescriptorOption::Check(policy) => StaticOption {
                owner: StaticOptionOwner::Common,
                value: StaticOptionValue::Check {
                    policy: match policy {
                        StaticCheckPolicy::DeclaredOnly => CheckPolicy::DeclaredOnly,
                        StaticCheckPolicy::CheckedOptional => CheckPolicy::CheckedOptional,
                        StaticCheckPolicy::CheckedRequired => CheckPolicy::CheckedRequired,
                    },
                },
            },
            StaticDescriptorOption::SQLiteRequireVersionAtLeast {
                major,
                minor,
                patch,
            } => StaticOption {
                owner: StaticOptionOwner::SQLite,
                value: StaticOptionValue::SQLiteRequireVersionAtLeast {
                    major: *major,
                    minor: *minor,
                    patch: *patch,
                },
            },
            StaticDescriptorOption::PostgreSQLParameterType {
                parameter_name,
                canonical_type_name,
            } => StaticOption {
                owner: StaticOptionOwner::PostgreSQL,
                value: StaticOptionValue::PostgreSQLParameterType {
                    parameter_name: parameter_name.clone(),
                    canonical_type_name: canonical_type_name.clone(),
                },
            },
        })
        .collect::<Vec<_>>();
    if !options
        .iter()
        .any(|option| matches!(option.value, StaticOptionValue::Check { .. }))
    {
        options.insert(
            0,
            StaticOption {
                owner: StaticOptionOwner::Common,
                value: StaticOptionValue::Check {
                    policy: CheckPolicy::DeclaredOnly,
                },
            },
        );
    }
    options
}

fn policy(options: &[StaticOption]) -> CheckPolicy {
    options
        .iter()
        .find_map(|option| match option.value {
            StaticOptionValue::Check { policy } => Some(policy),
            _ => None,
        })
        .unwrap_or(CheckPolicy::DeclaredOnly)
}

fn driver_entries(
    descriptor: &StaticDescriptor,
    params: &CanonicalContract,
    source_sql: &[u8],
    occurrences: &[ParameterOccurrence],
    check_policy: CheckPolicy,
    drivers: &[Driver],
) -> Result<Vec<DriverEntry>, StaticArtifactBuildError> {
    let fields = root_fields(params).map_err(|reason| fail(descriptor, reason))?;
    let names = occurrences
        .iter()
        .map(|occurrence| occurrence.source_name.as_str())
        .collect::<HashSet<_>>();
    if fields
        .iter()
        .any(|field| !names.contains(field.name.as_str()))
        || names
            .iter()
            .any(|name| !fields.iter().any(|field| field.name == *name))
    {
        return Err(fail(
            descriptor,
            "SQL placeholders and Params fields must match exactly",
        ));
    }
    let mut bindings = Vec::with_capacity(fields.len());
    for (index, field) in fields.iter().enumerate() {
        let occurrence = occurrences
            .iter()
            .find(|occurrence| occurrence.source_name == field.name)
            .ok_or_else(|| {
                fail(
                    descriptor,
                    format!("Params field `{}` has no placeholder", field.name),
                )
            })?;
        bindings.push(BindingEntry {
            params_field_ordinal: u32::try_from(index)
                .map_err(|_| fail(descriptor, "too many Params fields"))?,
            source_name: field.name.clone(),
            protocol_ordinal: occurrence.protocol_ordinal,
            field_type_fingerprint: params
                .project(&field.ty)
                .and_then(|contract| contract.fingerprint())
                .map_err(|error| interface_error(descriptor, error))?,
            retention: bind_retention(&field.ty).map_err(|reason| fail(descriptor, reason))?,
        });
    }
    drivers
        .iter()
        .map(|&driver| {
            let mut wire_sql = Vec::with_capacity(source_sql.len());
            let mut rewrites = Vec::with_capacity(occurrences.len());
            let mut cursor = 0usize;
            for occurrence in occurrences {
                let start = occurrence.source_span.start as usize;
                let end = occurrence.source_span.end as usize;
                wire_sql.extend_from_slice(&source_sql[cursor..start]);
                let wire_start = u32::try_from(wire_sql.len())
                    .map_err(|_| fail(descriptor, "wire SQL exceeds u32 offsets"))?;
                match driver {
                    Driver::SQLite => wire_sql.extend_from_slice(&source_sql[start..end]),
                    Driver::PostgreSQL => wire_sql
                        .extend_from_slice(format!("${}", occurrence.protocol_ordinal).as_bytes()),
                }
                let wire_end = u32::try_from(wire_sql.len())
                    .map_err(|_| fail(descriptor, "wire SQL exceeds u32 offsets"))?;
                rewrites.push(RewriteEntry {
                    source_span: occurrence.source_span,
                    wire_span: Span {
                        start: wire_start,
                        end: wire_end,
                    },
                });
                cursor = end;
            }
            wire_sql.extend_from_slice(&source_sql[cursor..]);
            Ok(DriverEntry {
                driver,
                wire_sql_hash: Hash128::of(&wire_sql),
                wire_sql,
                rewrite_format_version: REWRITE_FORMAT_VERSION,
                rewrites,
                bindings: bindings.clone(),
                checked_metadata: CheckedMetadata {
                    policy: check_policy,
                    state: VerificationState::Declared,
                    metadata_format_version: None,
                    metadata_digest: None,
                    query_evidence: None,
                },
            })
        })
        .collect()
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

    fn string(&mut self, value: &str) -> Result<(), String> {
        self.u32(u32::try_from(value.len()).map_err(|_| "metadata identity field is too large")?);
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn hash(&mut self, value: Hash128) {
        self.bytes.extend_from_slice(&value.lo.to_le_bytes());
        self.bytes.extend_from_slice(&value.hi.to_le_bytes());
    }
}

fn driver_tag(driver: Driver) -> u8 {
    match driver {
        Driver::SQLite => 0,
        Driver::PostgreSQL => 1,
    }
}

fn checked_identities(
    descriptor_id: &str,
    metadata: &ParsedCheckedMetadata,
) -> Result<(String, String), String> {
    let mut server = IdentityWriter::new(b"ALIGNSRV");
    server.u32(1);
    server.u8(driver_tag(metadata.driver));
    server.string(&metadata.engine_version)?;
    server.string(&metadata.driver_version)?;
    server.u32(
        u32::try_from(metadata.search_path.len()).map_err(|_| "too many search-path entries")?,
    );
    for path in &metadata.search_path {
        server.string(path)?;
    }
    server.u32(u32::try_from(metadata.extensions.len()).map_err(|_| "too many extensions")?);
    for extension in &metadata.extensions {
        server.string(&extension.schema)?;
        server.string(&extension.name)?;
        match &extension.version {
            Some(version) => {
                server.u8(1);
                server.string(version)?;
            }
            None => server.u8(0),
        }
    }
    let server_identity = Hash128::of(&server.bytes);

    let mut prepare = IdentityWriter::new(b"ALIGNPRP");
    prepare.u32(1);
    prepare.string(descriptor_id)?;
    prepare.u8(driver_tag(metadata.driver));
    prepare.hash(metadata.metadata_digest);
    prepare.hash(server_identity);
    Ok((prepare_identity(&prepare), server_identity.to_hex()))
}

fn prepare_identity(writer: &IdentityWriter) -> String {
    Hash128::of(&writer.bytes).to_hex()
}

fn stale(reason: &'static str) -> Result<CheckedMetadata, String> {
    Err(reason.to_string())
}

fn checked_query_metadata(
    query: &StaticQueryArtifact,
    entry: &DriverEntry,
    metadata: &ParsedCheckedMetadata,
) -> Result<CheckedMetadata, String> {
    if metadata.driver_restriction != query.driver_restriction {
        return stale("driver restriction changed");
    }
    if metadata.statement_kind != MetadataStatementKind::Query {
        return stale("statement kind changed");
    }
    if metadata.statement_class != query.query_meta_plan.statement_class {
        return stale("statement class changed");
    }
    if metadata.source_identity != query.source_identity
        || metadata.source_sql_hash != query.source_sql_hash
        || metadata.wire_sql_hash != entry.wire_sql_hash
        || metadata.rewrite_format_version != entry.rewrite_format_version
        || metadata.static_options_hash
            != static_options_hash(&query.static_options).map_err(|e| e.to_string())?
        || metadata.params_fingerprint != query.params_fingerprint
        || metadata.row_fingerprint != Some(query.row_fingerprint)
    {
        return stale("artifact inputs changed");
    }
    if metadata.parameters.len() != query.query_meta_plan.parameters.len()
        || metadata
            .parameters
            .iter()
            .zip(&query.query_meta_plan.parameters)
            .any(|(actual, declared)| {
                actual.checked.ordinal != declared.ordinal
                    || actual.source_name != declared.source_name
                    || actual.logical_type != declared.logical_type
            })
    {
        return stale("parameter plan changed");
    }
    if metadata.columns.len() != query.query_meta_plan.columns.len()
        || metadata
            .columns
            .iter()
            .zip(&query.query_meta_plan.columns)
            .any(|(actual, declared)| {
                actual.checked.ordinal != declared.ordinal
                    || actual.source_alias != declared.source_alias
                    || actual.logical_type != declared.logical_type
            })
    {
        return stale("column plan changed");
    }
    let (prepare_identity, server_identity) = checked_identities(&query.query_id, metadata)?;
    Ok(CheckedMetadata {
        policy: entry.checked_metadata.policy,
        state: VerificationState::DatabaseChecked,
        metadata_format_version: Some(metadata.format_version),
        metadata_digest: Some(metadata.metadata_digest),
        query_evidence: Some(CheckedQueryEvidence {
            prepare_identity,
            schema_identity: metadata.schema_fingerprint.to_hex(),
            server_identity,
            parameters: metadata
                .parameters
                .iter()
                .map(|parameter| parameter.checked.clone())
                .collect(),
            columns: metadata
                .columns
                .iter()
                .map(|column| column.checked.clone())
                .collect(),
        }),
    })
}

fn checked_command_metadata(
    command: &StaticCommandArtifact,
    entry: &DriverEntry,
    metadata: &ParsedCheckedMetadata,
    statement_class: MetaStatementClass,
) -> Result<CheckedMetadata, String> {
    if metadata.driver_restriction != command.driver_restriction
        || metadata.statement_kind != MetadataStatementKind::Command
        || metadata.statement_class != statement_class
        || metadata.source_identity != command.source_identity
        || metadata.source_sql_hash != command.source_sql_hash
        || metadata.wire_sql_hash != entry.wire_sql_hash
        || metadata.rewrite_format_version != entry.rewrite_format_version
        || metadata.static_options_hash
            != static_options_hash(&command.static_options).map_err(|e| e.to_string())?
        || metadata.params_fingerprint != command.params_fingerprint
        || metadata.row_fingerprint.is_some()
        || !metadata.columns.is_empty()
    {
        return stale("command artifact inputs changed");
    }
    let fields = root_fields(&command.params_type)?;
    if metadata.parameters.len() != entry.bindings.len()
        || metadata.parameters.iter().any(|actual| {
            let Some(binding) = entry
                .bindings
                .iter()
                .find(|binding| binding.protocol_ordinal == actual.checked.ordinal)
            else {
                return true;
            };
            let Some(field) = fields.get(binding.params_field_ordinal as usize) else {
                return true;
            };
            actual.checked.ordinal != binding.protocol_ordinal
                || actual.source_name != binding.source_name
                || actual.logical_type != field.ty.spelling()
        })
    {
        return stale("parameter plan changed");
    }
    Ok(CheckedMetadata {
        policy: entry.checked_metadata.policy,
        state: VerificationState::DatabaseChecked,
        metadata_format_version: Some(metadata.format_version),
        metadata_digest: Some(metadata.metadata_digest),
        query_evidence: None,
    })
}

fn apply_checked_metadata(
    descriptor: &StaticDescriptor,
    input: &ResolvedStaticInput,
    artifact: &mut StaticArtifact,
    statement_class: MetaStatementClass,
) -> Result<(), StaticArtifactBuildError> {
    fn apply_entry(
        descriptor: &StaticDescriptor,
        input: &ResolvedStaticInput,
        entry: &mut DriverEntry,
        validate: impl FnOnce(&DriverEntry, &ParsedCheckedMetadata) -> Result<CheckedMetadata, String>,
    ) -> Result<(), StaticArtifactBuildError> {
        if entry.checked_metadata.policy == CheckPolicy::DeclaredOnly {
            return Ok(());
        }
        let snapshot = input
            .input
            .checked_metadata
            .iter()
            .find(|snapshot| snapshot.driver == entry.driver);
        let record = input
            .checked_metadata_records
            .iter()
            .find(|record| record.driver == entry.driver);
        let result = match (snapshot.map(|snapshot| &snapshot.state), record) {
            (Some(MetadataState::Present { .. }), Some(record)) => validate(entry, record),
            (Some(MetadataState::Missing), None) => Err("checked metadata is missing".to_string()),
            _ => Err("checked metadata snapshot is inconsistent".to_string()),
        };
        match result {
            Ok(metadata) => entry.checked_metadata = metadata,
            Err(_) if entry.checked_metadata.policy == CheckPolicy::CheckedOptional => {}
            Err(reason) => {
                return Err(fail(
                    descriptor,
                    format!("checked metadata for {:?} is stale: {reason}", entry.driver),
                ));
            }
        }
        Ok(())
    }

    match artifact {
        StaticArtifact::Query(query) => {
            let expected = query.clone();
            for entry in &mut query.driver_entries {
                apply_entry(descriptor, input, entry, |entry, metadata| {
                    checked_query_metadata(&expected, entry, metadata)
                })?;
            }
        }
        StaticArtifact::Command(command) => {
            let expected = command.clone();
            for entry in &mut command.driver_entries {
                apply_entry(descriptor, input, entry, |entry, metadata| {
                    checked_command_metadata(&expected, entry, metadata, statement_class)
                })?;
            }
        }
    }
    Ok(())
}

/// Build, validate, encode, and digest one artifact per resolved semantic descriptor.
pub fn build_static_artifacts(
    descriptors: &[StaticDescriptor],
    resolved: &ResolvedStaticInputs,
) -> Result<Vec<BuiltStaticArtifact>, StaticArtifactBuildError> {
    build_static_artifacts_inner(descriptors, resolved, true)
}

/// Build the same validated descriptor artifacts used by normal compilation while deliberately
/// leaving their per-driver verification state at `Declared`.
///
/// This is the Q3 regeneration boundary: `alignc db prepare` must be able to replace missing or
/// stale checked metadata, but it must not weaken any SQL, descriptor, type, option, or artifact
/// validation. Keeping the mode private to the driver prevents ordinary builds from bypassing a
/// `CheckedRequired` policy.
pub(crate) fn build_static_artifacts_for_regeneration(
    descriptors: &[StaticDescriptor],
    resolved: &ResolvedStaticInputs,
) -> Result<Vec<BuiltStaticArtifact>, StaticArtifactBuildError> {
    build_static_artifacts_inner(descriptors, resolved, false)
}

fn install_regeneration_placeholders(artifact: &mut StaticArtifact) {
    const ZERO_ID: &str = "00000000000000000000000000000000";
    let zero = Hash128 { lo: 0, hi: 0 };
    match artifact {
        StaticArtifact::Query(query) => {
            for entry in &mut query.driver_entries {
                if entry.checked_metadata.policy != CheckPolicy::CheckedRequired {
                    continue;
                }
                entry.checked_metadata = CheckedMetadata {
                    policy: CheckPolicy::CheckedRequired,
                    state: VerificationState::DatabaseChecked,
                    metadata_format_version: Some(1),
                    metadata_digest: Some(zero),
                    query_evidence: Some(CheckedQueryEvidence {
                        prepare_identity: ZERO_ID.to_string(),
                        schema_identity: ZERO_ID.to_string(),
                        server_identity: ZERO_ID.to_string(),
                        parameters: query
                            .query_meta_plan
                            .parameters
                            .iter()
                            .map(|parameter| CheckedParameterMeta {
                                ordinal: parameter.ordinal,
                                native_type: None,
                                native_type_id: None,
                            })
                            .collect(),
                        columns: query
                            .query_meta_plan
                            .columns
                            .iter()
                            .map(|column| CheckedColumnMeta {
                                ordinal: column.ordinal,
                                native_type: None,
                                native_type_id: None,
                                origin_schema: None,
                                origin_table: None,
                                origin_column: None,
                                nullable: MetaNullability::Unknown,
                            })
                            .collect(),
                    }),
                };
            }
        }
        StaticArtifact::Command(command) => {
            for entry in &mut command.driver_entries {
                if entry.checked_metadata.policy == CheckPolicy::CheckedRequired {
                    entry.checked_metadata = CheckedMetadata {
                        policy: CheckPolicy::CheckedRequired,
                        state: VerificationState::DatabaseChecked,
                        metadata_format_version: Some(1),
                        metadata_digest: Some(zero),
                        query_evidence: None,
                    };
                }
            }
        }
    }
}

fn build_static_artifacts_inner(
    descriptors: &[StaticDescriptor],
    resolved: &ResolvedStaticInputs,
    apply_metadata: bool,
) -> Result<Vec<BuiltStaticArtifact>, StaticArtifactBuildError> {
    let inputs = resolved
        .inputs
        .iter()
        .map(|input| (input.input.descriptor_id.as_str(), input))
        .collect::<HashMap<_, _>>();
    let mut built = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        let input = inputs
            .get(descriptor.descriptor_id.as_str())
            .ok_or_else(|| fail(descriptor, "resolved static input is missing"))?;
        if input.input.consumer_kind
            != match descriptor.consumer {
                StaticDescriptorConsumer::Query => crate::StaticConsumerKind::Query,
                StaticDescriptorConsumer::Command => crate::StaticConsumerKind::Command,
            }
        {
            return Err(fail(
                descriptor,
                "resolved input consumer kind disagrees with the descriptor",
            ));
        }
        let params = CanonicalContract::try_from(&descriptor.params_contract)
            .map_err(|error| interface_error(descriptor, error))?;
        let scan = scan_sql(&input.bytes).map_err(|reason| fail(descriptor, reason))?;
        let static_options = options(descriptor);
        let check_policy = policy(&static_options);
        let drivers = input.input.driver_restriction.drivers().to_vec();
        let entries = driver_entries(
            descriptor,
            &params,
            &input.bytes,
            &scan.occurrences,
            check_policy,
            &drivers,
        )?;
        let decoded_span_map = match &input.input.source {
            align_interface::SqlSourceIdentity::File { .. }
                if input.decoded_span_map.is_empty() =>
            {
                Vec::new()
            }
            align_interface::SqlSourceIdentity::File { .. } => {
                return Err(fail(
                    descriptor,
                    "file SQL unexpectedly carries an inline decoded span map",
                ));
            }
            align_interface::SqlSourceIdentity::Inline { .. } => input.decoded_span_map.clone(),
        };
        let source_hash = Hash128::of(&input.bytes);
        let mut artifact = match descriptor.consumer {
            StaticDescriptorConsumer::Query => {
                let row = descriptor
                    .row_contract
                    .as_ref()
                    .ok_or_else(|| fail(descriptor, "Query descriptor has no Row contract"))?;
                let row = CanonicalContract::try_from(row)
                    .map_err(|error| interface_error(descriptor, error))?;
                let row_fields = root_fields(&row).map_err(|reason| fail(descriptor, reason))?;
                for field in row_fields {
                    validate_row_type(&field.ty).map_err(|reason| fail(descriptor, reason))?;
                }
                let params_fields =
                    root_fields(&params).map_err(|reason| fail(descriptor, reason))?;
                let mut seen_parameters = HashSet::new();
                let parameter_names = scan
                    .occurrences
                    .iter()
                    .filter_map(|occurrence| {
                        seen_parameters
                            .insert(occurrence.source_name.as_str())
                            .then_some(occurrence.source_name.as_str())
                    })
                    .collect::<Vec<_>>();
                let query_meta_plan = QueryMetaPlan {
                    statement_class: scan.statement_class,
                    parameters: parameter_names
                        .iter()
                        .enumerate()
                        .map(|(index, name)| {
                            let field = params_fields
                                .iter()
                                .find(|field| field.name == **name)
                                .ok_or_else(|| {
                                    fail(
                                        descriptor,
                                        "QueryMeta parameter is absent from the Params contract",
                                    )
                                })?;
                            Ok(DeclaredParameterMeta {
                                ordinal: u32::try_from(index + 1).map_err(|_| {
                                    fail(descriptor, "too many QueryMeta parameters")
                                })?,
                                source_name: field.name.clone(),
                                logical_type: field.ty.spelling(),
                            })
                        })
                        .collect::<Result<Vec<_>, StaticArtifactBuildError>>()?,
                    columns: row_fields
                        .iter()
                        .enumerate()
                        .map(|(index, field)| {
                            Ok(DeclaredColumnMeta {
                                ordinal: u32::try_from(index)
                                    .map_err(|_| fail(descriptor, "too many QueryMeta columns"))?,
                                source_alias: field.name.clone(),
                                logical_type: field.ty.spelling(),
                            })
                        })
                        .collect::<Result<Vec<_>, StaticArtifactBuildError>>()?,
                };
                let artifact = StaticQueryArtifact {
                    format_version: STATIC_ARTIFACT_FORMAT_VERSION,
                    unit: descriptor.unit.clone(),
                    item: descriptor.item.clone(),
                    query_id: descriptor.descriptor_id.clone(),
                    params_fingerprint: params
                        .fingerprint()
                        .map_err(|error| interface_error(descriptor, error))?,
                    row_fingerprint: row
                        .fingerprint()
                        .map_err(|error| interface_error(descriptor, error))?,
                    params_type: params,
                    row_type: row,
                    binder_abi_version: BINDER_ABI_VERSION,
                    decoder_abi_version: DECODER_ABI_VERSION,
                    driver_restriction: input.input.driver_restriction,
                    static_options,
                    source_identity: input.input.source.clone(),
                    source_sql: input.bytes.clone(),
                    source_sql_hash: source_hash,
                    occurrences: scan.occurrences,
                    driver_entries: entries,
                    decoded_span_map,
                    query_meta_plan,
                };
                StaticArtifact::Query(artifact)
            }
            StaticDescriptorConsumer::Command => {
                if descriptor.row_contract.is_some() {
                    return Err(fail(
                        descriptor,
                        "command descriptor carries a Row contract",
                    ));
                }
                StaticArtifact::Command(StaticCommandArtifact {
                    format_version: STATIC_ARTIFACT_FORMAT_VERSION,
                    unit: descriptor.unit.clone(),
                    item: descriptor.item.clone(),
                    command_id: descriptor.descriptor_id.clone(),
                    params_fingerprint: params
                        .fingerprint()
                        .map_err(|error| interface_error(descriptor, error))?,
                    params_type: params,
                    binder_abi_version: BINDER_ABI_VERSION,
                    driver_restriction: input.input.driver_restriction,
                    static_options,
                    source_identity: input.input.source.clone(),
                    source_sql: input.bytes.clone(),
                    source_sql_hash: source_hash,
                    occurrences: scan.occurrences,
                    driver_entries: entries,
                    decoded_span_map,
                })
            }
        };
        if apply_metadata {
            apply_checked_metadata(descriptor, input, &mut artifact, scan.statement_class)?;
        } else {
            // Required+Declared is intentionally invalid in the normal artifact codec. Install
            // structurally valid private placeholders so regeneration still receives the codec's
            // complete semantic validation before native work. These values are never published,
            // cached, installed in MIR, or consulted by the metadata writer.
            install_regeneration_placeholders(&mut artifact);
        }
        let bytes = match &artifact {
            StaticArtifact::Query(query) => align_interface::encode_static_query(query),
            StaticArtifact::Command(command) => align_interface::encode_static_command(command),
        }
        .map_err(|error| interface_error(descriptor, error))?;
        let digest =
            static_artifact_digest(&bytes).map_err(|error| interface_error(descriptor, error))?;
        let runtime = crate::static_runtime::generate_static_runtime(&artifact, digest)
            .map_err(|reason| fail(descriptor, reason))?;
        built.push(BuiltStaticArtifact {
            descriptor_id: descriptor.descriptor_id.clone(),
            artifact,
            bytes,
            digest,
            runtime,
        });
    }
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanner_rewrites_only_live_named_placeholders() {
        let sql = b"SELECT ':skip', $$:skip$$, id FROM t -- :skip\nWHERE id = :id OR id = :id";
        let scan = scan_sql(sql).expect("valid portable SQL");
        assert_eq!(
            scan.occurrences
                .iter()
                .map(|occurrence| (occurrence.source_name.as_str(), occurrence.protocol_ordinal))
                .collect::<Vec<_>>(),
            [("id", 1), ("id", 1)]
        );
        assert_eq!(scan.statement_class, MetaStatementClass::Select);
    }

    #[test]
    fn scanner_keeps_postgres_escape_strings_opaque() {
        let sql = br"SELECT E'escaped\' :not_a_parameter', e'\\:also_not' WHERE id = :id";
        let scan = scan_sql(sql).expect("PostgreSQL escape strings");
        assert_eq!(
            scan.occurrences
                .iter()
                .map(|occurrence| occurrence.source_name.as_str())
                .collect::<Vec<_>>(),
            ["id"]
        );
    }

    #[test]
    fn scanner_classifies_the_main_statement_after_ctes() {
        for (sql, expected) in [
            (
                b"WITH current AS (SELECT 1) SELECT * FROM current".as_slice(),
                MetaStatementClass::Select,
            ),
            (
                b"WITH current AS (SELECT 1) UPDATE users SET active = 1 RETURNING id".as_slice(),
                MetaStatementClass::Dml,
            ),
            (
                b"WITH RECURSIVE a(x) AS (VALUES (1)), b AS (SELECT x FROM a) DELETE FROM users"
                    .as_slice(),
                MetaStatementClass::Dml,
            ),
        ] {
            assert_eq!(
                scan_sql(sql).expect("CTE statement").statement_class,
                expected
            );
        }
    }

    #[test]
    fn scanner_rejects_statement_and_placeholder_ambiguity() {
        for (sql, expected) in [
            (b"SELECT ?".as_slice(), "`?`"),
            (b"SELECT $1".as_slice(), "`$n`"),
            (b"SELECT $1$not_a_tag$1$".as_slice(), "`$n`"),
            (b"SELECT @id".as_slice(), "`@name`"),
            (b"SELECT 1; SELECT 2".as_slice(), "exactly one statement"),
            (b"/* open".as_slice(), "unterminated block comment"),
        ] {
            let error = scan_sql(sql).expect_err("malformed static SQL must fail");
            assert!(error.contains(expected), "{sql:?}: {error}");
        }
    }
}
