//! Shared D12 QueryMeta layout authority for thunk production and native-view validation.

use crate::Program;
use align_sema::{IntTy, Scalar, Ty};

const QUERY_META_FIELDS: [&str; 24] = [
    "query_id",
    "driver",
    "driver_restriction",
    "statement_class",
    "artifact_digest",
    "state",
    "metadata_fingerprint",
    "source_sql_hash",
    "driver_wire_sql_hash",
    "rewrite_format_version",
    "prepare_identity",
    "schema_identity",
    "server_identity",
    "entry",
    "ordinal",
    "source_name",
    "source_alias",
    "logical_type",
    "native_type",
    "native_type_id",
    "origin_schema",
    "origin_table",
    "origin_column",
    "nullable",
];

fn nominal_id<T>(
    definitions: &[T],
    source_name: &str,
    name: impl Fn(&T) -> &str,
) -> Result<u32, String> {
    let mut matching = definitions
        .iter()
        .enumerate()
        .filter(|(_, definition)| name(definition) == source_name);
    let index = matching
        .next()
        .and_then(|(index, _)| u32::try_from(index).ok())
        .ok_or_else(|| format!("generated QueryMeta type `{source_name}` is absent"))?;
    if matching.next().is_some() {
        return Err(format!(
            "generated QueryMeta type `{source_name}` is ambiguous"
        ));
    }
    Ok(index)
}

fn require_unit_variants(
    program: &Program,
    enum_id: u32,
    source_name: &str,
    expected: &[&str],
) -> Result<(), String> {
    let definition = program
        .enums
        .get(enum_id as usize)
        .ok_or_else(|| format!("generated QueryMeta enum `{source_name}` is absent"))?;
    if definition.variants.len() != expected.len()
        || definition
            .variants
            .iter()
            .zip(expected)
            .any(|(variant, name)| variant.name != *name || !variant.payload.is_empty())
    {
        return Err(format!(
            "generated QueryMeta enum `{source_name}` differs from the D12 contract"
        ));
    }
    Ok(())
}

/// Exact nominal ids of the validated D12 metadata ABI.
pub struct QueryMetaTypes {
    pub row: u32,
    pub driver: u32,
    pub restriction: u32,
    pub statement_class: u32,
    pub state: u32,
    pub entry: u32,
    pub nullable: u32,
}

impl QueryMetaTypes {
    pub fn resolve(program: &Program) -> Result<Self, String> {
        let row = nominal_id(&program.structs, "pkg.db$QueryMeta", |value| {
            value.source_name.as_str()
        })?;
        let find_enum = |source_name| {
            nominal_id(&program.enums, source_name, |value| {
                value.source_name.as_str()
            })
        };
        let types = Self {
            row,
            driver: find_enum("pkg.db$Driver")?,
            restriction: find_enum("pkg.db$DriverRestriction")?,
            statement_class: find_enum("pkg.db$MetaStatementClass")?,
            state: find_enum("pkg.db$MetaQueryState")?,
            entry: find_enum("pkg.db$MetaQueryEntry")?,
            nullable: find_enum("pkg.db$MetaNullability")?,
        };
        for (id, source_name, variants) in [
            (types.driver, "pkg.db$Driver", &["SQLite", "PostgreSQL"][..]),
            (
                types.restriction,
                "pkg.db$DriverRestriction",
                &["AnySupportedDriver", "SQLiteOnly", "PostgreSQLOnly"][..],
            ),
            (
                types.statement_class,
                "pkg.db$MetaStatementClass",
                &["Select", "Dml", "Ddl", "Native", "Unknown"][..],
            ),
            (
                types.state,
                "pkg.db$MetaQueryState",
                &["Declared", "DatabaseChecked"][..],
            ),
            (
                types.entry,
                "pkg.db$MetaQueryEntry",
                &["Summary", "Parameter", "Column"][..],
            ),
            (
                types.nullable,
                "pkg.db$MetaNullability",
                &["Yes", "No", "Unknown"][..],
            ),
        ] {
            require_unit_variants(program, id, source_name, variants)?;
        }
        let i64_ty = Ty::Int(IntTy {
            bits: 64,
            signed: true,
        });
        let expected = [
            Ty::Str,
            Ty::Enum(types.driver),
            Ty::Enum(types.restriction),
            Ty::Enum(types.statement_class),
            Ty::Str,
            Ty::Enum(types.state),
            Ty::Option(Scalar::Str),
            Ty::Str,
            Ty::Str,
            i64_ty,
            Ty::Option(Scalar::Str),
            Ty::Option(Scalar::Str),
            Ty::Option(Scalar::Str),
            Ty::Enum(types.entry),
            Ty::Option(Scalar::Int(IntTy {
                bits: 64,
                signed: true,
            })),
            Ty::Option(Scalar::Str),
            Ty::Option(Scalar::Str),
            Ty::Option(Scalar::Str),
            Ty::Option(Scalar::Str),
            Ty::Option(Scalar::Int(IntTy {
                bits: 64,
                signed: true,
            })),
            Ty::Option(Scalar::Str),
            Ty::Option(Scalar::Str),
            Ty::Option(Scalar::Str),
            Ty::Enum(types.nullable),
        ];
        let definition = program
            .structs
            .get(row as usize)
            .ok_or_else(|| "generated QueryMeta row is absent".to_owned())?;
        if definition.align.is_some()
            || definition.c_repr
            || definition.fields.len() != QUERY_META_FIELDS.len()
            || definition
                .fields
                .iter()
                .zip(QUERY_META_FIELDS.into_iter().zip(expected))
                .any(|(field, (name, ty))| field.name != name || field.ty != ty)
        {
            return Err("pkg.db.QueryMeta fields differ from the D12 contract".to_string());
        }
        Ok(types)
    }
}
