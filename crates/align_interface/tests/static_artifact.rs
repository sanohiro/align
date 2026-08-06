use align_interface::Hash128;
use align_interface::static_artifact::*;

fn decode_hex(value: &str) -> Vec<u8> {
    let bytes = value.trim().as_bytes();
    assert_eq!(bytes.len() % 2, 0);
    bytes
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).unwrap();
            let low = (pair[1] as char).to_digit(16).unwrap();
            ((high << 4) | low) as u8
        })
        .collect()
}

fn named(path: &str) -> CanonicalType {
    CanonicalType::Named {
        path: path.to_string(),
        args: Vec::new(),
    }
}

fn span_at(sql: &[u8], needle: &[u8]) -> Span {
    let start = sql
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("fixture placeholder") as u32;
    Span {
        start,
        end: start + u32::try_from(needle.len()).unwrap(),
    }
}

fn span_at_after(sql: &[u8], needle: &[u8], after: usize) -> Span {
    let start = sql[after..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| offset + after)
        .expect("fixture placeholder") as u32;
    Span {
        start,
        end: start + u32::try_from(needle.len()).unwrap(),
    }
}

fn params_contract(root: &str, fields: &[(&str, CanonicalType)]) -> CanonicalContract {
    CanonicalContract {
        root: named(root),
        definitions: vec![CanonicalDefinition {
            path: root.to_string(),
            args: Vec::new(),
            kind: CanonicalDefinitionBody::Struct {
                fields: fields
                    .iter()
                    .map(|(name, ty)| CanonicalField {
                        name: (*name).into(),
                        ty: ty.clone(),
                    })
                    .collect(),
            },
        }],
    }
}

fn leaf_contract(path: &str) -> CanonicalContract {
    CanonicalContract {
        root: named(path),
        definitions: Vec::new(),
    }
}

fn query_fixture() -> StaticQueryArtifact {
    let params_type = params_contract(
        "app.Params",
        &[("id", named("i64")), ("pattern", named("string"))],
    );
    let row_type = CanonicalContract {
        root: named("app.UserRow"),
        definitions: vec![CanonicalDefinition {
            path: "app.UserRow".into(),
            args: Vec::new(),
            kind: CanonicalDefinitionBody::Struct {
                fields: vec![
                    CanonicalField {
                        name: "id".into(),
                        ty: named("i64"),
                    },
                    CanonicalField {
                        name: "name".into(),
                        ty: named("str"),
                    },
                ],
            },
        }],
    };
    let source_sql = b"SELECT id, name FROM users WHERE id = :id OR name = :pattern\n".to_vec();
    let id_span = span_at(&source_sql, b":id");
    let pattern_span = span_at(&source_sql, b":pattern");
    let occurrences = vec![
        ParameterOccurrence {
            source_name: "id".into(),
            source_span: id_span,
            protocol_ordinal: 1,
        },
        ParameterOccurrence {
            source_name: "pattern".into(),
            source_span: pattern_span,
            protocol_ordinal: 2,
        },
    ];
    let pg_wire = b"SELECT id, name FROM users WHERE id = $1 OR name = $2\n".to_vec();
    let pg_id_span = span_at(&pg_wire, b"$1");
    let pg_pattern_span = span_at_after(&pg_wire, b"$2", pg_id_span.end as usize);
    let bindings = vec![
        BindingEntry {
            params_field_ordinal: 0,
            source_name: "id".into(),
            protocol_ordinal: 1,
            field_type_fingerprint: leaf_contract("i64").fingerprint().unwrap(),
            retention: BindRetention::BindValue,
        },
        BindingEntry {
            params_field_ordinal: 1,
            source_name: "pattern".into(),
            protocol_ordinal: 2,
            field_type_fingerprint: leaf_contract("string").fingerprint().unwrap(),
            retention: BindRetention::BindCopy,
        },
    ];
    let sqlite_rewrites = vec![
        RewriteEntry {
            source_span: id_span,
            wire_span: id_span,
        },
        RewriteEntry {
            source_span: pattern_span,
            wire_span: pattern_span,
        },
    ];
    let pg_rewrites = vec![
        RewriteEntry {
            source_span: id_span,
            wire_span: pg_id_span,
        },
        RewriteEntry {
            source_span: pattern_span,
            wire_span: pg_pattern_span,
        },
    ];
    let options = vec![StaticOption {
        owner: StaticOptionOwner::Common,
        value: StaticOptionValue::Check {
            policy: CheckPolicy::CheckedOptional,
        },
    }];
    let sqlite = DriverEntry {
        driver: Driver::SQLite,
        wire_sql: source_sql.clone(),
        wire_sql_hash: Hash128::of(&source_sql),
        rewrite_format_version: 1,
        rewrites: sqlite_rewrites,
        bindings: bindings.clone(),
        checked_metadata: CheckedMetadata {
            policy: CheckPolicy::CheckedOptional,
            state: VerificationState::Declared,
            metadata_format_version: None,
            metadata_digest: None,
            query_evidence: None,
        },
    };
    let postgres = DriverEntry {
        driver: Driver::PostgreSQL,
        wire_sql: pg_wire.clone(),
        wire_sql_hash: Hash128::of(&pg_wire),
        rewrite_format_version: 1,
        rewrites: pg_rewrites,
        bindings,
        checked_metadata: CheckedMetadata {
            policy: CheckPolicy::CheckedOptional,
            state: VerificationState::DatabaseChecked,
            metadata_format_version: Some(3),
            metadata_digest: Some(Hash128::of(b"metadata-query")),
            query_evidence: Some(CheckedQueryEvidence {
                prepare_identity: Hash128::of(b"prepare-query").to_hex(),
                schema_identity: Hash128::of(b"schema-v1").to_hex(),
                server_identity: Hash128::of(b"postgres-16").to_hex(),
                parameters: vec![
                    CheckedParameterMeta {
                        ordinal: 1,
                        native_type: Some("int8".into()),
                        native_type_id: Some(20),
                    },
                    CheckedParameterMeta {
                        ordinal: 2,
                        native_type: Some("text".into()),
                        native_type_id: Some(25),
                    },
                ],
                columns: vec![
                    CheckedColumnMeta {
                        ordinal: 0,
                        native_type: Some("int8".into()),
                        native_type_id: Some(20),
                        origin_schema: Some("public".into()),
                        origin_table: Some("users".into()),
                        origin_column: Some("id".into()),
                        nullable: MetaNullability::No,
                    },
                    CheckedColumnMeta {
                        ordinal: 1,
                        native_type: Some("text".into()),
                        native_type_id: Some(25),
                        origin_schema: Some("public".into()),
                        origin_table: Some("users".into()),
                        origin_column: Some("name".into()),
                        nullable: MetaNullability::Unknown,
                    },
                ],
            }),
        },
    };
    let split = u32::try_from(source_sql.len() - 1).unwrap();
    StaticQueryArtifact {
        format_version: STATIC_ARTIFACT_FORMAT_VERSION,
        unit: "app".into(),
        item: "users_by_id".into(),
        query_id: "app.users_by_id".into(),
        params_fingerprint: params_type.fingerprint().unwrap(),
        row_fingerprint: row_type.fingerprint().unwrap(),
        params_type,
        row_type,
        binder_abi_version: BINDER_ABI_VERSION,
        decoder_abi_version: DECODER_ABI_VERSION,
        driver_restriction: DriverRestriction::AnySupportedDriver,
        static_options: options,
        source_identity: SqlSourceIdentity::Inline {
            query_or_command_id: "app.users_by_id".into(),
        },
        source_sql: source_sql.clone(),
        source_sql_hash: Hash128::of(&source_sql),
        occurrences,
        driver_entries: vec![sqlite, postgres],
        decoded_span_map: vec![
            DecodedSpanEntry {
                decoded_span: Span {
                    start: 0,
                    end: split,
                },
                defining_file_span: Span {
                    start: 120,
                    end: 120 + split,
                },
            },
            DecodedSpanEntry {
                decoded_span: Span {
                    start: split,
                    end: split + 1,
                },
                defining_file_span: Span {
                    start: 240,
                    end: 242,
                },
            },
        ],
        query_meta_plan: QueryMetaPlan {
            statement_class: MetaStatementClass::Select,
            parameters: vec![
                DeclaredParameterMeta {
                    ordinal: 1,
                    source_name: "id".into(),
                    logical_type: "i64".into(),
                },
                DeclaredParameterMeta {
                    ordinal: 2,
                    source_name: "pattern".into(),
                    logical_type: "string".into(),
                },
            ],
            columns: vec![
                DeclaredColumnMeta {
                    ordinal: 0,
                    source_alias: "id".into(),
                    logical_type: "i64".into(),
                },
                DeclaredColumnMeta {
                    ordinal: 1,
                    source_alias: "name".into(),
                    logical_type: "str".into(),
                },
            ],
        },
    }
}

fn command_fixture() -> StaticCommandArtifact {
    let params_type = params_contract(
        "app.InsertParams",
        &[("id", named("i64")), ("name", named("string"))],
    );
    let source_sql = b"INSERT INTO users(id,name) VALUES (:id,:name)".to_vec();
    let id_span = span_at(&source_sql, b":id");
    let name_span = span_at(&source_sql, b":name");
    let wire_sql = b"INSERT INTO users(id,name) VALUES ($1,$2)".to_vec();
    let bindings = vec![
        BindingEntry {
            params_field_ordinal: 0,
            source_name: "id".into(),
            protocol_ordinal: 1,
            field_type_fingerprint: leaf_contract("i64").fingerprint().unwrap(),
            retention: BindRetention::BindValue,
        },
        BindingEntry {
            params_field_ordinal: 1,
            source_name: "name".into(),
            protocol_ordinal: 2,
            field_type_fingerprint: leaf_contract("string").fingerprint().unwrap(),
            retention: BindRetention::BindCopy,
        },
    ];
    let entry = DriverEntry {
        driver: Driver::PostgreSQL,
        wire_sql: wire_sql.clone(),
        wire_sql_hash: Hash128::of(&wire_sql),
        rewrite_format_version: 1,
        rewrites: vec![
            RewriteEntry {
                source_span: id_span,
                wire_span: span_at(&wire_sql, b"$1"),
            },
            RewriteEntry {
                source_span: name_span,
                wire_span: span_at(&wire_sql, b"$2"),
            },
        ],
        bindings,
        checked_metadata: CheckedMetadata {
            policy: CheckPolicy::CheckedRequired,
            state: VerificationState::DatabaseChecked,
            metadata_format_version: Some(2),
            metadata_digest: Some(Hash128::of(b"metadata-command")),
            query_evidence: None,
        },
    };
    StaticCommandArtifact {
        format_version: STATIC_ARTIFACT_FORMAT_VERSION,
        unit: "app".into(),
        item: "insert_user".into(),
        command_id: "app.insert_user".into(),
        params_fingerprint: params_type.fingerprint().unwrap(),
        params_type,
        binder_abi_version: BINDER_ABI_VERSION,
        driver_restriction: DriverRestriction::PostgreSQLOnly,
        static_options: vec![
            StaticOption {
                owner: StaticOptionOwner::Common,
                value: StaticOptionValue::Check {
                    policy: CheckPolicy::CheckedRequired,
                },
            },
            StaticOption {
                owner: StaticOptionOwner::PostgreSQL,
                value: StaticOptionValue::PostgreSQLParameterType {
                    parameter_name: "id".into(),
                    canonical_type_name: "int8".into(),
                },
            },
        ],
        source_identity: SqlSourceIdentity::File {
            logical_path: "sql/insert_user.sql".into(),
        },
        source_sql: source_sql.clone(),
        source_sql_hash: Hash128::of(&source_sql),
        occurrences: vec![
            ParameterOccurrence {
                source_name: "id".into(),
                source_span: id_span,
                protocol_ordinal: 1,
            },
            ParameterOccurrence {
                source_name: "name".into(),
                source_span: name_span,
                protocol_ordinal: 2,
            },
        ],
        driver_entries: vec![entry],
        decoded_span_map: Vec::new(),
    }
}

// This encoder deliberately has no dependency on the production codec. It is a compact
// table-driven transcription of §6.2 used only to pin the reviewed byte order and the checked-in
// vectors. If the production writer changes accidentally, this reference remains independent.
struct RefWriter(Vec<u8>);

impl RefWriter {
    fn new() -> Self {
        Self(Vec::new())
    }
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn i64(&mut self, value: i64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn hash(&mut self, value: Hash128) {
        self.u64(value.lo);
        self.u64(value.hi);
    }
    fn bytes(&mut self, value: &[u8]) {
        self.u32(u32::try_from(value.len()).unwrap());
        self.0.extend_from_slice(value);
    }
    fn string(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }
    fn opt_string(&mut self, value: Option<&String>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.string(value);
            }
            None => self.u8(0),
        }
    }
}

fn ref_type(w: &mut RefWriter, value: &CanonicalType) {
    match value {
        CanonicalType::Named { path, args } => {
            w.u8(0);
            w.string(path);
            w.u32(u32::try_from(args.len()).unwrap());
            for arg in args {
                ref_type(w, arg);
            }
        }
        CanonicalType::Tuple(values) => {
            w.u8(1);
            w.u32(u32::try_from(values.len()).unwrap());
            for value in values {
                ref_type(w, value);
            }
        }
        CanonicalType::Fn { params, result } => {
            w.u8(2);
            w.u32(u32::try_from(params.len()).unwrap());
            for param in params {
                ref_type(w, param);
            }
            ref_type(w, result);
        }
    }
}

fn ref_contract(w: &mut RefWriter, value: &CanonicalContract) {
    ref_type(w, &value.root);
    w.u32(u32::try_from(value.definitions.len()).unwrap());
    for definition in &value.definitions {
        w.string(&definition.path);
        w.u32(u32::try_from(definition.args.len()).unwrap());
        for arg in &definition.args {
            ref_type(w, arg);
        }
        match &definition.kind {
            CanonicalDefinitionBody::Struct { fields } => {
                w.u8(0);
                w.u32(u32::try_from(fields.len()).unwrap());
                for field in fields {
                    w.string(&field.name);
                    ref_type(w, &field.ty);
                }
            }
            CanonicalDefinitionBody::Sum { variants } => {
                w.u8(1);
                w.u32(u32::try_from(variants.len()).unwrap());
                for variant in variants {
                    w.string(&variant.name);
                    w.u32(u32::try_from(variant.payload.len()).unwrap());
                    for ty in &variant.payload {
                        ref_type(w, ty);
                    }
                }
            }
        }
    }
}

fn ref_span(w: &mut RefWriter, value: Span) {
    w.u32(value.start);
    w.u32(value.end);
}

fn ref_identity(w: &mut RefWriter, value: &SqlSourceIdentity) {
    match value {
        SqlSourceIdentity::File { logical_path } => {
            w.u8(0);
            w.string(logical_path);
        }
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => {
            w.u8(1);
            w.string(query_or_command_id);
        }
    }
}

fn ref_option(w: &mut RefWriter, value: &StaticOption) {
    w.u8(value.owner as u8);
    match &value.value {
        StaticOptionValue::Check { policy } => {
            w.u8(0);
            w.u8(*policy as u8);
        }
        StaticOptionValue::SQLiteRequireVersionAtLeast {
            major,
            minor,
            patch,
        } => {
            w.u8(0);
            w.u32(*major);
            w.u32(*minor);
            w.u32(*patch);
        }
        StaticOptionValue::PostgreSQLParameterType {
            parameter_name,
            canonical_type_name,
        } => {
            w.u8(0);
            w.string(parameter_name);
            w.string(canonical_type_name);
        }
    }
}

fn ref_occurrence(w: &mut RefWriter, value: &ParameterOccurrence) {
    w.string(&value.source_name);
    ref_span(w, value.source_span);
    w.u32(value.protocol_ordinal);
}

fn ref_driver(w: &mut RefWriter, value: &DriverEntry, query: bool) {
    w.u8(value.driver as u8);
    w.bytes(&value.wire_sql);
    w.hash(value.wire_sql_hash);
    w.u32(value.rewrite_format_version);
    w.u32(u32::try_from(value.rewrites.len()).unwrap());
    for rewrite in &value.rewrites {
        ref_span(w, rewrite.source_span);
        ref_span(w, rewrite.wire_span);
    }
    w.u32(u32::try_from(value.bindings.len()).unwrap());
    for binding in &value.bindings {
        w.u32(binding.params_field_ordinal);
        w.string(&binding.source_name);
        w.u32(binding.protocol_ordinal);
        w.hash(binding.field_type_fingerprint);
        w.u8(binding.retention as u8);
    }
    w.u8(value.checked_metadata.policy as u8);
    w.u8(value.checked_metadata.state as u8);
    if value.checked_metadata.state == VerificationState::DatabaseChecked {
        w.u32(value.checked_metadata.metadata_format_version.unwrap());
        w.hash(value.checked_metadata.metadata_digest.unwrap());
        if query {
            match &value.checked_metadata.query_evidence {
                Some(evidence) => {
                    w.u8(1);
                    w.string(&evidence.prepare_identity);
                    w.string(&evidence.schema_identity);
                    w.string(&evidence.server_identity);
                    w.u32(u32::try_from(evidence.parameters.len()).unwrap());
                    for parameter in &evidence.parameters {
                        w.u32(parameter.ordinal);
                        w.opt_string(parameter.native_type.as_ref());
                        match parameter.native_type_id {
                            Some(id) => {
                                w.u8(1);
                                w.i64(id);
                            }
                            None => w.u8(0),
                        }
                    }
                    w.u32(u32::try_from(evidence.columns.len()).unwrap());
                    for column in &evidence.columns {
                        w.u32(column.ordinal);
                        w.opt_string(column.native_type.as_ref());
                        match column.native_type_id {
                            Some(id) => {
                                w.u8(1);
                                w.i64(id);
                            }
                            None => w.u8(0),
                        }
                        w.opt_string(column.origin_schema.as_ref());
                        w.opt_string(column.origin_table.as_ref());
                        w.opt_string(column.origin_column.as_ref());
                        w.u8(column.nullable as u8);
                    }
                }
                None => w.u8(0),
            }
        }
    }
}

fn ref_decoded_span(w: &mut RefWriter, value: &DecodedSpanEntry) {
    ref_span(w, value.decoded_span);
    ref_span(w, value.defining_file_span);
}

fn ref_query(value: &StaticQueryArtifact) -> Vec<u8> {
    let mut w = RefWriter::new();
    w.0.extend_from_slice(b"ALIGNQRY");
    w.u32(value.format_version);
    w.string(&value.unit);
    w.string(&value.item);
    w.string(&value.query_id);
    ref_contract(&mut w, &value.params_type);
    ref_contract(&mut w, &value.row_type);
    w.hash(value.params_fingerprint);
    w.hash(value.row_fingerprint);
    w.u32(value.binder_abi_version);
    w.u32(value.decoder_abi_version);
    w.u8(value.driver_restriction as u8);
    w.u32(u32::try_from(value.static_options.len()).unwrap());
    for option in &value.static_options {
        ref_option(&mut w, option);
    }
    ref_identity(&mut w, &value.source_identity);
    w.bytes(&value.source_sql);
    w.hash(value.source_sql_hash);
    w.u32(u32::try_from(value.occurrences.len()).unwrap());
    for occurrence in &value.occurrences {
        ref_occurrence(&mut w, occurrence);
    }
    w.u32(u32::try_from(value.driver_entries.len()).unwrap());
    for entry in &value.driver_entries {
        ref_driver(&mut w, entry, true);
    }
    w.u32(u32::try_from(value.decoded_span_map.len()).unwrap());
    for entry in &value.decoded_span_map {
        ref_decoded_span(&mut w, entry);
    }
    w.u8(value.query_meta_plan.statement_class as u8);
    w.u32(u32::try_from(value.query_meta_plan.parameters.len()).unwrap());
    for parameter in &value.query_meta_plan.parameters {
        w.u32(parameter.ordinal);
        w.string(&parameter.source_name);
        w.string(&parameter.logical_type);
    }
    w.u32(u32::try_from(value.query_meta_plan.columns.len()).unwrap());
    for column in &value.query_meta_plan.columns {
        w.u32(column.ordinal);
        w.string(&column.source_alias);
        w.string(&column.logical_type);
    }
    w.0
}

fn ref_command(value: &StaticCommandArtifact) -> Vec<u8> {
    let mut w = RefWriter::new();
    w.0.extend_from_slice(b"ALIGNCMD");
    w.u32(value.format_version);
    w.string(&value.unit);
    w.string(&value.item);
    w.string(&value.command_id);
    ref_contract(&mut w, &value.params_type);
    w.hash(value.params_fingerprint);
    w.u32(value.binder_abi_version);
    w.u8(value.driver_restriction as u8);
    w.u32(u32::try_from(value.static_options.len()).unwrap());
    for option in &value.static_options {
        ref_option(&mut w, option);
    }
    ref_identity(&mut w, &value.source_identity);
    w.bytes(&value.source_sql);
    w.hash(value.source_sql_hash);
    w.u32(u32::try_from(value.occurrences.len()).unwrap());
    for occurrence in &value.occurrences {
        ref_occurrence(&mut w, occurrence);
    }
    w.u32(u32::try_from(value.driver_entries.len()).unwrap());
    for entry in &value.driver_entries {
        ref_driver(&mut w, entry, false);
    }
    w.u32(u32::try_from(value.decoded_span_map.len()).unwrap());
    for entry in &value.decoded_span_map {
        ref_decoded_span(&mut w, entry);
    }
    w.0
}

#[test]
fn query_and_command_round_trip() {
    let query = query_fixture();
    let query_bytes = query.encode().expect("valid Query fixture");
    assert_eq!(
        decode_static_query(&query_bytes).expect("decode Query"),
        query
    );
    assert_eq!(ref_query(&query), query_bytes);
    assert_eq!(
        decode_hex(include_str!(
            "../../align_driver/tests/golden/static_query_v1.hex"
        )),
        query_bytes
    );
    assert_eq!(
        include_str!("../../align_driver/tests/golden/static_query_v1.digest").trim(),
        query.digest().unwrap().to_hex()
    );
    let command = command_fixture();
    let command_bytes = command.encode().expect("valid command fixture");
    assert_eq!(
        decode_static_command(&command_bytes).expect("decode command"),
        command
    );
    assert_eq!(ref_command(&command), command_bytes);
    assert_eq!(
        decode_hex(include_str!(
            "../../align_driver/tests/golden/static_command_v1.hex"
        )),
        command_bytes
    );
    assert_eq!(
        include_str!("../../align_driver/tests/golden/static_command_v1.digest").trim(),
        command.digest().unwrap().to_hex()
    );
    assert_ne!(query_bytes, command_bytes);
}

#[test]
fn mutation_of_hash_or_trailing_bytes_is_rejected() {
    let query = query_fixture();
    let mut bytes = query.encode().unwrap();
    let fingerprint = query.params_fingerprint.lo.to_le_bytes();
    let offset = bytes
        .windows(fingerprint.len())
        .position(|window| window == fingerprint)
        .expect("Params fingerprint in artifact");
    bytes[offset] ^= 1;
    assert!(matches!(
        decode_static_query(&bytes),
        Err(StaticArtifactError::Invalid(_))
    ));
    let mut bytes = query.encode().unwrap();
    bytes.push(0);
    assert_eq!(
        decode_static_query(&bytes),
        Err(StaticArtifactError::TrailingBytes)
    );
}

#[test]
fn semantic_validation_rejects_identity_driver_and_policy_drift() {
    let mut artifact = query_fixture();
    artifact.query_id = "app.other".into();
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));

    let mut artifact = query_fixture();
    artifact.driver_entries.swap(0, 1);
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));

    let mut artifact = query_fixture();
    artifact.driver_entries[0].checked_metadata.policy = CheckPolicy::DeclaredOnly;
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));

    let mut artifact = query_fixture();
    artifact.source_sql.push(0);
    artifact.source_sql_hash = Hash128::of(&artifact.source_sql);
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));
}

#[test]
fn review_findings_are_closed_at_the_artifact_boundary() {
    let mut artifact = query_fixture();
    artifact.driver_entries[1].checked_metadata.query_evidence = None;
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));

    let mut artifact = query_fixture();
    artifact.driver_entries[1].wire_sql =
        b"SELECT id, name FROM users WHERE id = $9 OR name = $2\n".to_vec();
    artifact.driver_entries[1].wire_sql_hash = Hash128::of(&artifact.driver_entries[1].wire_sql);
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));

    let mut artifact = query_fixture();
    artifact.query_meta_plan.columns[0].source_alias = "wrong".into();
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));

    let mut artifact = query_fixture();
    artifact.driver_entries[1]
        .checked_metadata
        .query_evidence
        .as_mut()
        .unwrap()
        .columns[0]
        .nullable = MetaNullability::Yes;
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));

    let mut artifact = command_fixture();
    artifact.static_options.push(StaticOption {
        owner: StaticOptionOwner::PostgreSQL,
        value: StaticOptionValue::PostgreSQLParameterType {
            parameter_name: "missing".into(),
            canonical_type_name: "text".into(),
        },
    });
    artifact
        .static_options
        .sort_by_key(|option| match &option.value {
            StaticOptionValue::Check { .. } => (0u8, String::new()),
            StaticOptionValue::SQLiteRequireVersionAtLeast { .. } => (1u8, String::new()),
            StaticOptionValue::PostgreSQLParameterType { parameter_name, .. } => {
                (2u8, parameter_name.clone())
            }
        });
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));

    let mut artifact = query_fixture();
    artifact.driver_entries[1]
        .checked_metadata
        .query_evidence
        .as_mut()
        .unwrap()
        .prepare_identity = "not-a-hash".into();
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));
}

#[test]
fn decoder_rejects_unknown_version_and_truncated_prefixes() {
    let query = query_fixture().encode().unwrap();
    let mut bad_version = query.clone();
    bad_version[8..12].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        decode_static_query(&bad_version),
        Err(StaticArtifactError::UnknownVersion(2))
    );
    for length in [0, 1, 7, 8, 9, 16, query.len() / 2] {
        assert!(decode_static_query(&query[..length]).is_err());
    }
}

#[test]
fn conflicting_native_options_are_rejected() {
    let mut artifact = command_fixture();
    artifact.static_options.push(StaticOption {
        owner: StaticOptionOwner::PostgreSQL,
        value: StaticOptionValue::PostgreSQLParameterType {
            parameter_name: "id".into(),
            canonical_type_name: "int4".into(),
        },
    });
    artifact
        .static_options
        .sort_by_key(|option| match &option.value {
            StaticOptionValue::Check { .. } => (0u8, String::new()),
            StaticOptionValue::PostgreSQLParameterType { parameter_name, .. } => {
                (2u8, parameter_name.clone())
            }
            StaticOptionValue::SQLiteRequireVersionAtLeast { .. } => (1u8, String::new()),
        });
    assert!(matches!(
        artifact.validate(),
        Err(StaticArtifactError::Invalid(_))
    ));
}
