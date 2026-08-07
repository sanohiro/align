//! D1 fake-driver execution over compiler-validated static statement artifacts.
//!
//! This is an owner-test consumer for the generated plans, not a database API. Artifact formation
//! resolves canonical type names once into closed value tags and field ordinals; execution accepts
//! only that producer-owned runtime plan, never opens SQL/artifact files, and performs no name
//! lookup or reflection.

use align_interface::{
    BindRetention, CanonicalDefinitionBody, CanonicalField, CanonicalType, CheckPolicy, Driver,
    Hash128, MetaStatementClass, QueryMetaPlan, StaticArtifact, StaticOption, StaticOptionOwner,
    StaticOptionValue,
};

pub const STATIC_RUNTIME_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedValueKind {
    Bool,
    I16,
    I32,
    I64,
    F32,
    F64,
    Text,
    Bytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedValueShape {
    pub kind: GeneratedValueKind,
    pub nullable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedBindField {
    pub params_field_ordinal: u32,
    pub protocol_ordinal: u32,
    pub retention: BindRetention,
    pub shape: GeneratedValueShape,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedBindThunk {
    pub fields: Vec<GeneratedBindField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedDecodeField {
    pub row_field_ordinal: u32,
    pub shape: GeneratedValueShape,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedDecodeThunk {
    pub fields: Vec<GeneratedDecodeField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedQueryMetaThunk {
    pub plan: QueryMetaPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedDriverRuntime {
    pub driver: Driver,
    pub wire_sql: Vec<u8>,
    pub binder: GeneratedBindThunk,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedQueryRuntime {
    pub query_id: String,
    pub artifact_digest: Hash128,
    pub static_options: Vec<StaticOption>,
    pub drivers: Vec<GeneratedDriverRuntime>,
    pub decoder: GeneratedDecodeThunk,
    pub query_meta: GeneratedQueryMetaThunk,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedCommandRuntime {
    pub command_id: String,
    pub artifact_digest: Hash128,
    pub static_options: Vec<StaticOption>,
    pub drivers: Vec<GeneratedDriverRuntime>,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeneratedStaticRuntime {
    Query(GeneratedQueryRuntime),
    Command(GeneratedCommandRuntime),
}

impl GeneratedStaticRuntime {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Query(runtime) => &runtime.bytes,
            Self::Command(runtime) => &runtime.bytes,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FakeValue {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(Vec<u8>),
    Bytes(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FakeCardinality {
    All,
    AtMostOne,
    ExactlyOne,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FakeStatementKind {
    Query,
    Command,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FakeBoundValue {
    pub params_field_ordinal: u32,
    pub protocol_ordinal: u32,
    pub retention: BindRetention,
    pub value: FakeValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FakeDecodedField {
    pub row_field_ordinal: u32,
    pub value: FakeValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FakeExecution {
    pub kind: FakeStatementKind,
    pub execution_count: u32,
    pub wire_sql: Vec<u8>,
    pub bound: Vec<FakeBoundValue>,
    pub rows: Vec<Vec<FakeDecodedField>>,
    pub query_meta: Option<QueryMetaPlan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FakeExecutionError {
    DriverNotPermitted,
    ParameterCount {
        expected: usize,
        actual: usize,
    },
    ParameterType {
        ordinal: u32,
        expected: String,
    },
    RowCount {
        expected: &'static str,
        actual: usize,
    },
    RowWidth {
        row: u32,
        expected: usize,
        actual: usize,
    },
    RowType {
        row: u32,
        ordinal: u32,
        expected: String,
    },
    CommandReturnedRows,
    InvalidArtifact(&'static str),
}

impl std::fmt::Display for FakeExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DriverNotPermitted => write!(f, "the selected driver is not permitted"),
            Self::ParameterCount { expected, actual } => {
                write!(f, "expected {expected} parameter values, got {actual}")
            }
            Self::ParameterType { ordinal, expected } => {
                write!(f, "parameter field {ordinal} is not `{expected}`")
            }
            Self::RowCount { expected, actual } => {
                write!(f, "expected {expected}, got {actual} rows")
            }
            Self::RowWidth {
                row,
                expected,
                actual,
            } => {
                write!(f, "row {row} has {actual} columns; expected {expected}")
            }
            Self::RowType {
                row,
                ordinal,
                expected,
            } => {
                write!(f, "row {row} column {ordinal} is not `{expected}`")
            }
            Self::CommandReturnedRows => write!(f, "a command cannot return rows"),
            Self::InvalidArtifact(reason) => write!(f, "invalid static runtime artifact: {reason}"),
        }
    }
}

impl std::error::Error for FakeExecutionError {}

fn root_fields(contract: &align_interface::CanonicalContract) -> Option<&[CanonicalField]> {
    let CanonicalType::Named { path, args } = &contract.root else {
        return None;
    };
    contract
        .definitions
        .iter()
        .find(|definition| definition.path == *path && definition.args == *args)
        .and_then(|definition| match &definition.kind {
            CanonicalDefinitionBody::Struct { fields } => Some(fields.as_slice()),
            CanonicalDefinitionBody::Sum { .. } => None,
        })
}

fn generated_shape(ty: &CanonicalType) -> Option<GeneratedValueShape> {
    let (ty, nullable) = match ty {
        CanonicalType::Named { path, args } if path == "Option" && args.len() == 1 => {
            (&args[0], true)
        }
        other => (other, false),
    };
    let kind = match ty {
        CanonicalType::Named { path, args } if args.is_empty() => match path.as_str() {
            "bool" => GeneratedValueKind::Bool,
            "i16" => GeneratedValueKind::I16,
            "i32" => GeneratedValueKind::I32,
            "i64" => GeneratedValueKind::I64,
            "f32" => GeneratedValueKind::F32,
            "f64" => GeneratedValueKind::F64,
            "str" | "string" => GeneratedValueKind::Text,
            _ => return None,
        },
        CanonicalType::Named { path, args }
            if matches!(path.as_str(), "slice" | "array")
                && matches!(args.as_slice(), [CanonicalType::Named { path, args }] if path == "u8" && args.is_empty()) =>
        {
            GeneratedValueKind::Bytes
        }
        _ => return None,
    };
    Some(GeneratedValueShape { kind, nullable })
}

struct RuntimeWriter {
    bytes: Vec<u8>,
}

impl RuntimeWriter {
    fn new(magic: &[u8; 8]) -> Self {
        let mut bytes = magic.to_vec();
        bytes.extend_from_slice(&STATIC_RUNTIME_FORMAT_VERSION.to_le_bytes());
        Self { bytes }
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

    fn field(&mut self, value: &[u8]) -> Result<(), &'static str> {
        let length = u32::try_from(value.len()).map_err(|_| "runtime field exceeds u32::MAX")?;
        self.u32(length);
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    fn text(&mut self, value: &str) -> Result<(), &'static str> {
        self.field(value.as_bytes())
    }

    fn count(&mut self, value: usize) -> Result<(), &'static str> {
        self.u32(u32::try_from(value).map_err(|_| "runtime sequence exceeds u32::MAX")?);
        Ok(())
    }

    fn shape(&mut self, shape: &GeneratedValueShape) {
        self.u8(match shape.kind {
            GeneratedValueKind::Bool => 0,
            GeneratedValueKind::I16 => 1,
            GeneratedValueKind::I32 => 2,
            GeneratedValueKind::I64 => 3,
            GeneratedValueKind::F32 => 4,
            GeneratedValueKind::F64 => 5,
            GeneratedValueKind::Text => 6,
            GeneratedValueKind::Bytes => 7,
        });
        self.u8(u8::from(shape.nullable));
    }

    fn option(&mut self, option: &StaticOption) -> Result<(), &'static str> {
        self.u8(match option.owner {
            StaticOptionOwner::Common => 0,
            StaticOptionOwner::SQLite => 1,
            StaticOptionOwner::PostgreSQL => 2,
        });
        match &option.value {
            StaticOptionValue::Check { policy } => {
                self.u8(0);
                self.u8(match policy {
                    CheckPolicy::DeclaredOnly => 0,
                    CheckPolicy::CheckedOptional => 1,
                    CheckPolicy::CheckedRequired => 2,
                });
            }
            StaticOptionValue::SQLiteRequireVersionAtLeast {
                major,
                minor,
                patch,
            } => {
                self.u8(1);
                self.u32(*major);
                self.u32(*minor);
                self.u32(*patch);
            }
            StaticOptionValue::PostgreSQLParameterType {
                parameter_name,
                canonical_type_name,
            } => {
                self.u8(2);
                self.text(parameter_name)?;
                self.text(canonical_type_name)?;
            }
        }
        Ok(())
    }

    fn options(&mut self, options: &[StaticOption]) -> Result<(), &'static str> {
        self.count(options.len())?;
        for option in options {
            self.option(option)?;
        }
        Ok(())
    }

    fn driver(&mut self, driver: &GeneratedDriverRuntime) -> Result<(), &'static str> {
        self.u8(match driver.driver {
            Driver::SQLite => 0,
            Driver::PostgreSQL => 1,
        });
        self.field(&driver.wire_sql)?;
        self.count(driver.binder.fields.len())?;
        for field in &driver.binder.fields {
            self.u32(field.params_field_ordinal);
            self.u32(field.protocol_ordinal);
            self.u8(match field.retention {
                BindRetention::BindValue => 0,
                BindRetention::BindCopy => 1,
            });
            self.shape(&field.shape);
        }
        Ok(())
    }
}

fn generated_drivers(
    params: &[CanonicalField],
    entries: &[align_interface::DriverEntry],
) -> Result<Vec<GeneratedDriverRuntime>, &'static str> {
    entries
        .iter()
        .map(|entry| {
            let fields = entry
                .bindings
                .iter()
                .map(|binding| {
                    let index = usize::try_from(binding.params_field_ordinal)
                        .map_err(|_| "parameter ordinal overflow")?;
                    let field = params
                        .get(index)
                        .ok_or("parameter ordinal is out of range")?;
                    Ok(GeneratedBindField {
                        params_field_ordinal: binding.params_field_ordinal,
                        protocol_ordinal: binding.protocol_ordinal,
                        retention: binding.retention,
                        shape: generated_shape(&field.ty)
                            .ok_or("unsupported generated binder field")?,
                    })
                })
                .collect::<Result<Vec<_>, &'static str>>()?;
            Ok(GeneratedDriverRuntime {
                driver: entry.driver,
                wire_sql: entry.wire_sql.clone(),
                binder: GeneratedBindThunk { fields },
            })
        })
        .collect()
}

/// Form the producer-owned, name-free runtime plan consumed by generated/fake driver thunks.
/// Canonical type names are resolved here once; runtime bind/decode uses only fixed ordinals and
/// closed value tags.
pub(crate) fn generate_static_runtime(
    artifact: &StaticArtifact,
    artifact_digest: Hash128,
) -> Result<GeneratedStaticRuntime, &'static str> {
    match artifact {
        StaticArtifact::Query(query) => {
            let params = root_fields(&query.params_type).ok_or("Params root is not a struct")?;
            let row = root_fields(&query.row_type).ok_or("Row root is not a struct")?;
            let drivers = generated_drivers(params, &query.driver_entries)?;
            let decoder = GeneratedDecodeThunk {
                fields: row
                    .iter()
                    .enumerate()
                    .map(|(ordinal, field)| {
                        Ok(GeneratedDecodeField {
                            row_field_ordinal: u32::try_from(ordinal)
                                .map_err(|_| "row ordinal overflow")?,
                            shape: generated_shape(&field.ty)
                                .ok_or("unsupported generated decoder field")?,
                        })
                    })
                    .collect::<Result<Vec<_>, &'static str>>()?,
            };
            let query_meta = GeneratedQueryMetaThunk {
                plan: query.query_meta_plan.clone(),
            };
            let mut writer = RuntimeWriter::new(b"ALIGNQST");
            writer.text(&query.query_id)?;
            writer.hash(artifact_digest);
            writer.options(&query.static_options)?;
            writer.count(drivers.len())?;
            for driver in &drivers {
                writer.driver(driver)?;
            }
            writer.count(decoder.fields.len())?;
            for field in &decoder.fields {
                writer.u32(field.row_field_ordinal);
                writer.shape(&field.shape);
            }
            writer.u8(match query_meta.plan.statement_class {
                MetaStatementClass::Select => 0,
                MetaStatementClass::Dml => 1,
                MetaStatementClass::Ddl => 2,
                MetaStatementClass::Native => 3,
                MetaStatementClass::Unknown => 4,
            });
            writer.count(query_meta.plan.parameters.len())?;
            for parameter in &query_meta.plan.parameters {
                writer.u32(parameter.ordinal);
                writer.text(&parameter.source_name)?;
                writer.text(&parameter.logical_type)?;
            }
            writer.count(query_meta.plan.columns.len())?;
            for column in &query_meta.plan.columns {
                writer.u32(column.ordinal);
                writer.text(&column.source_alias)?;
                writer.text(&column.logical_type)?;
            }
            Ok(GeneratedStaticRuntime::Query(GeneratedQueryRuntime {
                query_id: query.query_id.clone(),
                artifact_digest,
                static_options: query.static_options.clone(),
                drivers,
                decoder,
                query_meta,
                bytes: writer.bytes,
            }))
        }
        StaticArtifact::Command(command) => {
            let params = root_fields(&command.params_type).ok_or("Params root is not a struct")?;
            let drivers = generated_drivers(params, &command.driver_entries)?;
            let mut writer = RuntimeWriter::new(b"ALIGNCST");
            writer.text(&command.command_id)?;
            writer.hash(artifact_digest);
            writer.options(&command.static_options)?;
            writer.count(drivers.len())?;
            for driver in &drivers {
                writer.driver(driver)?;
            }
            Ok(GeneratedStaticRuntime::Command(GeneratedCommandRuntime {
                command_id: command.command_id.clone(),
                artifact_digest,
                static_options: command.static_options.clone(),
                drivers,
                bytes: writer.bytes,
            }))
        }
    }
}

fn value_matches(value: &FakeValue, shape: &GeneratedValueShape) -> bool {
    if matches!(value, FakeValue::Null) {
        return shape.nullable;
    }
    match shape.kind {
        GeneratedValueKind::Bool => matches!(value, FakeValue::Bool(_)),
        GeneratedValueKind::I16 => {
            matches!(value, FakeValue::Integer(number) if i16::try_from(*number).is_ok())
        }
        GeneratedValueKind::I32 => {
            matches!(value, FakeValue::Integer(number) if i32::try_from(*number).is_ok())
        }
        GeneratedValueKind::I64 => matches!(value, FakeValue::Integer(_)),
        GeneratedValueKind::F32 | GeneratedValueKind::F64 => {
            matches!(value, FakeValue::Float(_))
        }
        GeneratedValueKind::Text => matches!(value, FakeValue::Text(_)),
        GeneratedValueKind::Bytes => matches!(value, FakeValue::Bytes(_)),
    }
}

fn driver_entry(
    entries: &[GeneratedDriverRuntime],
    driver: Driver,
) -> Result<&GeneratedDriverRuntime, FakeExecutionError> {
    entries
        .iter()
        .find(|entry| entry.driver == driver)
        .ok_or(FakeExecutionError::DriverNotPermitted)
}

fn bind(
    thunk: &GeneratedBindThunk,
    params: &[FakeValue],
) -> Result<Vec<FakeBoundValue>, FakeExecutionError> {
    if params.len() != thunk.fields.len() {
        return Err(FakeExecutionError::ParameterCount {
            expected: thunk.fields.len(),
            actual: params.len(),
        });
    }
    let mut bound = Vec::with_capacity(thunk.fields.len());
    for (expected_ordinal, field) in thunk.fields.iter().enumerate() {
        if usize::try_from(field.params_field_ordinal).ok() != Some(expected_ordinal)
            || field.protocol_ordinal == 0
        {
            return Err(FakeExecutionError::InvalidArtifact(
                "generated binder fields are not dense or have an invalid protocol ordinal",
            ));
        }
        let index = usize::try_from(field.params_field_ordinal)
            .map_err(|_| FakeExecutionError::InvalidArtifact("parameter ordinal overflow"))?;
        let value = params
            .get(index)
            .ok_or(FakeExecutionError::InvalidArtifact(
                "parameter value is absent",
            ))?;
        if !value_matches(value, &field.shape) {
            return Err(FakeExecutionError::ParameterType {
                ordinal: field.params_field_ordinal,
                expected: format!("{:?}", field.shape),
            });
        }
        bound.push(FakeBoundValue {
            params_field_ordinal: field.params_field_ordinal,
            protocol_ordinal: field.protocol_ordinal,
            retention: field.retention,
            value: value.clone(),
        });
    }
    Ok(bound)
}

fn validate_cardinality(
    cardinality: FakeCardinality,
    rows: usize,
) -> Result<(), FakeExecutionError> {
    match cardinality {
        FakeCardinality::All => Ok(()),
        FakeCardinality::AtMostOne if rows <= 1 => Ok(()),
        FakeCardinality::ExactlyOne if rows == 1 => Ok(()),
        FakeCardinality::AtMostOne => Err(FakeExecutionError::RowCount {
            expected: "at most one row",
            actual: rows,
        }),
        FakeCardinality::ExactlyOne => Err(FakeExecutionError::RowCount {
            expected: "exactly one row",
            actual: rows,
        }),
    }
}

/// Execute one compiler-built static Query/command runtime plan over deterministic fake input.
///
/// `params` and each row are in declared field order. The generated binding/decoder plans already
/// own the ordinals, so execution performs no field-name lookup or reflection.
pub fn execute_fake_static(
    runtime: &GeneratedStaticRuntime,
    driver: Driver,
    params: &[FakeValue],
    rows: &[Vec<FakeValue>],
    cardinality: FakeCardinality,
) -> Result<FakeExecution, FakeExecutionError> {
    match runtime {
        GeneratedStaticRuntime::Query(query) => {
            let entry = driver_entry(&query.drivers, driver)?;
            let bound = bind(&entry.binder, params)?;
            validate_cardinality(cardinality, rows.len())?;
            let mut decoded = Vec::with_capacity(rows.len());
            for (row_index, row) in rows.iter().enumerate() {
                let row_ordinal = u32::try_from(row_index)
                    .map_err(|_| FakeExecutionError::InvalidArtifact("row ordinal overflow"))?;
                if row.len() != query.decoder.fields.len() {
                    return Err(FakeExecutionError::RowWidth {
                        row: row_ordinal,
                        expected: query.decoder.fields.len(),
                        actual: row.len(),
                    });
                }
                let mut fields = Vec::with_capacity(row.len());
                for (field_index, (value, field)) in
                    row.iter().zip(&query.decoder.fields).enumerate()
                {
                    let ordinal = u32::try_from(field_index).map_err(|_| {
                        FakeExecutionError::InvalidArtifact("row field ordinal overflow")
                    })?;
                    if field.row_field_ordinal != ordinal {
                        return Err(FakeExecutionError::InvalidArtifact(
                            "generated decoder fields are not dense",
                        ));
                    }
                    if !value_matches(value, &field.shape) {
                        return Err(FakeExecutionError::RowType {
                            row: row_ordinal,
                            ordinal,
                            expected: format!("{:?}", field.shape),
                        });
                    }
                    fields.push(FakeDecodedField {
                        row_field_ordinal: field.row_field_ordinal,
                        value: value.clone(),
                    });
                }
                decoded.push(fields);
            }
            Ok(FakeExecution {
                kind: FakeStatementKind::Query,
                execution_count: 1,
                wire_sql: entry.wire_sql.clone(),
                bound,
                rows: decoded,
                query_meta: Some(query.query_meta.plan.clone()),
            })
        }
        GeneratedStaticRuntime::Command(command) => {
            if !rows.is_empty() {
                return Err(FakeExecutionError::CommandReturnedRows);
            }
            let entry = driver_entry(&command.drivers, driver)?;
            let bound = bind(&entry.binder, params)?;
            Ok(FakeExecution {
                kind: FakeStatementKind::Command,
                execution_count: 1,
                wire_sql: entry.wire_sql.clone(),
                bound,
                rows: Vec::new(),
                query_meta: None,
            })
        }
    }
}
