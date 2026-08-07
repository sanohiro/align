//! Q1/D1 static Query/command artifact formation.

use crate::{ResolvedStaticInputs, StaticDescriptor, StaticDescriptorConsumer};
use align_interface::{
    static_artifact_digest, BindRetention, BindingEntry, CanonicalContract,
    CanonicalDefinitionBody, CanonicalType, CheckPolicy, CheckedMetadata, DeclaredColumnMeta,
    DeclaredParameterMeta, Driver, DriverEntry, Hash128, MetaStatementClass, ParameterOccurrence,
    QueryMetaPlan, RewriteEntry, Span, StaticArtifact, StaticArtifactError, StaticCommandArtifact,
    StaticOption, StaticOptionOwner, StaticOptionValue, StaticQueryArtifact, VerificationState,
    BINDER_ABI_VERSION, DECODER_ABI_VERSION, REWRITE_FORMAT_VERSION,
    STATIC_ARTIFACT_FORMAT_VERSION,
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
        if first_keyword.is_none() && identifier_start(byte) {
            let start = index;
            index += 1;
            while index < sql.len() && identifier_continue(sql[index]) {
                index += 1;
            }
            first_keyword = Some(String::from_utf8_lossy(&sql[start..index]).to_ascii_uppercase());
            has_statement_token = true;
            continue;
        }
        has_statement_token = true;
        index += 1;
    }
    if !has_statement_token {
        return Err("a static Query/command cannot be empty".to_string());
    }
    let statement_class = match first_keyword.as_deref() {
        Some("SELECT" | "WITH" | "VALUES") => MetaStatementClass::Select,
        Some("INSERT" | "UPDATE" | "DELETE" | "MERGE") => MetaStatementClass::Dml,
        Some("CREATE" | "ALTER" | "DROP" | "TRUNCATE") => MetaStatementClass::Ddl,
        Some("PRAGMA" | "EXPLAIN") => MetaStatementClass::Native,
        _ => MetaStatementClass::Unknown,
    };
    Ok(SqlScan {
        occurrences,
        statement_class,
    })
}

fn root_fields(contract: &CanonicalContract) -> Result<&[align_interface::CanonicalField], String> {
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

/// Build, validate, encode, and digest one artifact per resolved semantic descriptor.
pub fn build_static_artifacts(
    descriptors: &[StaticDescriptor],
    resolved: &ResolvedStaticInputs,
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
        let artifact = match descriptor.consumer {
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
