//! D12 producer-owned static Query metadata thunk generation.

use crate::{
    BuiltStaticArtifact, GeneratedMetaDetail, GeneratedQueryMetaEntry, GeneratedQueryMetaRow,
    generated_query_meta_rows,
};
use align_ast::{BinOp, ParamMode};
use align_interface::{
    Driver, DriverRestriction, MetaNullability, MetaStatementClass, StaticArtifact,
    VerificationState,
};
use align_mir::{Block, Const, Function, Operand, Program, ProgramCall, Rvalue, Stmt, Term};
use align_sema::{IntTy, Scalar, Ty, hir};

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
    definitions
        .iter()
        .position(|definition| name(definition) == source_name)
        .and_then(|index| u32::try_from(index).ok())
        .ok_or_else(|| format!("generated QueryMeta type `{source_name}` is absent"))
}

fn enum_variant(program: &Program, enum_id: u32, name: &str) -> Result<u32, String> {
    program
        .enums
        .get(enum_id as usize)
        .and_then(|definition| {
            definition
                .variants
                .iter()
                .position(|variant| variant.name == name)
        })
        .and_then(|index| u32::try_from(index).ok())
        .ok_or_else(|| format!("generated QueryMeta enum variant `{name}` is absent"))
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

fn value_id(values: &mut Vec<Ty>, ty: Ty) -> Result<u32, String> {
    let id = u32::try_from(values.len())
        .map_err(|_| "generated QueryMeta thunk has too many values".to_string())?;
    values.push(ty);
    Ok(id)
}

struct MetaTypes {
    row: u32,
    driver: u32,
    restriction: u32,
    statement_class: u32,
    state: u32,
    entry: u32,
    nullable: u32,
}

impl MetaTypes {
    fn resolve(program: &Program) -> Result<Self, String> {
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
        let definition = &program.structs[row as usize];
        if definition.fields.len() != QUERY_META_FIELDS.len()
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

struct RowBlock<'a> {
    program: &'a Program,
    types: &'a MetaTypes,
    values: &'a mut Vec<Ty>,
    stmts: Vec<Stmt>,
}

impl RowBlock<'_> {
    fn value(&mut self, ty: Ty, rvalue: Rvalue) -> Result<Operand, String> {
        let id = value_id(self.values, ty)?;
        self.stmts.push(Stmt::Let(id, rvalue));
        Ok(Operand::Value(id))
    }

    fn store(&mut self, field: u32, value: Operand) {
        self.stmts.push(Stmt::StoreField(3, vec![field], value));
    }

    fn text(&mut self, field: u32, value: &str) -> Result<(), String> {
        let value = self.value(Ty::Str, Rvalue::StrLit(value.to_string()))?;
        self.store(field, value);
        Ok(())
    }

    fn option_text(&mut self, field: u32, value: Option<&str>) -> Result<(), String> {
        let ty = Ty::Option(Scalar::Str);
        let value = match value {
            Some(value) => {
                let text = self.value(Ty::Str, Rvalue::StrLit(value.to_string()))?;
                self.value(ty, Rvalue::OptionSome(text))?
            }
            None => self.value(ty, Rvalue::OptionNone)?,
        };
        self.store(field, value);
        Ok(())
    }

    fn option_i64(&mut self, field: u32, value: Option<i64>) -> Result<(), String> {
        let scalar = Scalar::Int(IntTy {
            bits: 64,
            signed: true,
        });
        let ty = Ty::Option(scalar);
        let value = match value {
            Some(value) => self.value(
                ty,
                Rvalue::OptionSome(Operand::Const(Const::Int(
                    i128::from(value),
                    Ty::Int(IntTy {
                        bits: 64,
                        signed: true,
                    }),
                ))),
            )?,
            None => self.value(ty, Rvalue::OptionNone)?,
        };
        self.store(field, value);
        Ok(())
    }

    fn enum_value(&mut self, field: u32, enum_id: u32, variant: &str) -> Result<(), String> {
        let variant = enum_variant(self.program, enum_id, variant)?;
        let value = self.value(
            Ty::Enum(enum_id),
            Rvalue::MakeEnum {
                enum_id,
                variant,
                payload: Vec::new(),
            },
        )?;
        self.store(field, value);
        Ok(())
    }

    fn row(mut self, row: &GeneratedQueryMetaRow) -> Result<Vec<Stmt>, String> {
        self.text(0, &row.query_id)?;
        self.enum_value(
            1,
            self.types.driver,
            match row.driver {
                Driver::SQLite => "SQLite",
                Driver::PostgreSQL => "PostgreSQL",
            },
        )?;
        self.enum_value(
            2,
            self.types.restriction,
            match row.driver_restriction {
                DriverRestriction::AnySupportedDriver => "AnySupportedDriver",
                DriverRestriction::SQLiteOnly => "SQLiteOnly",
                DriverRestriction::PostgreSQLOnly => "PostgreSQLOnly",
            },
        )?;
        self.enum_value(
            3,
            self.types.statement_class,
            match row.statement_class {
                MetaStatementClass::Select => "Select",
                MetaStatementClass::Dml => "Dml",
                MetaStatementClass::Ddl => "Ddl",
                MetaStatementClass::Native => "Native",
                MetaStatementClass::Unknown => "Unknown",
            },
        )?;
        self.text(4, &row.artifact_digest)?;
        self.enum_value(
            5,
            self.types.state,
            match row.state {
                VerificationState::Declared => "Declared",
                VerificationState::DatabaseChecked => "DatabaseChecked",
            },
        )?;
        self.option_text(6, row.metadata_fingerprint.as_deref())?;
        self.text(7, &row.source_sql_hash)?;
        self.text(8, &row.driver_wire_sql_hash)?;
        self.store(
            9,
            Operand::Const(Const::Int(
                i128::from(row.rewrite_format_version),
                Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                }),
            )),
        );
        self.option_text(10, row.prepare_identity.as_deref())?;
        self.option_text(11, row.schema_identity.as_deref())?;
        self.option_text(12, row.server_identity.as_deref())?;
        self.enum_value(
            13,
            self.types.entry,
            match row.entry {
                GeneratedQueryMetaEntry::Summary => "Summary",
                GeneratedQueryMetaEntry::Parameter => "Parameter",
                GeneratedQueryMetaEntry::Column => "Column",
            },
        )?;
        self.option_i64(14, row.ordinal)?;
        self.option_text(15, row.source_name.as_deref())?;
        self.option_text(16, row.source_alias.as_deref())?;
        self.option_text(17, row.logical_type.as_deref())?;
        self.option_text(18, row.native_type.as_deref())?;
        self.option_i64(19, row.native_type_id)?;
        self.option_text(20, row.origin_schema.as_deref())?;
        self.option_text(21, row.origin_table.as_deref())?;
        self.option_text(22, row.origin_column.as_deref())?;
        self.enum_value(
            23,
            self.types.nullable,
            match row.nullable {
                MetaNullability::Yes => "Yes",
                MetaNullability::No => "No",
                MetaNullability::Unknown => "Unknown",
            },
        )?;
        Ok(self.stmts)
    }
}

/// Build the one D12 QueryMeta materializer referenced by a Query descriptor header.
pub(crate) fn generate_query_meta_thunk(
    program: &Program,
    symbol: &str,
    artifact: &BuiltStaticArtifact,
) -> Result<(ProgramCall, Function), String> {
    let types = MetaTypes::resolve(program)?;
    let StaticArtifact::Query(query) = &artifact.artifact else {
        return Err("a command cannot generate a QueryMeta thunk".to_string());
    };
    let name = ProgramCall::try_from_logical(&format!("{symbol}$query_meta_v1"))
        .map_err(|_| "generated QueryMeta symbol is invalid".to_string())?;
    let u8_ty = Ty::Int(IntTy {
        bits: 8,
        signed: false,
    });
    let i64_ty = Ty::Int(IntTy {
        bits: 64,
        signed: true,
    });
    let row_ty = Ty::Struct(types.row);
    let option_ty = Ty::Option(Scalar::Struct(types.row));
    let mut candidates = Vec::new();
    for (driver_tag, driver) in [(0u8, Driver::SQLite), (1u8, Driver::PostgreSQL)] {
        if !query
            .driver_entries
            .iter()
            .any(|entry| entry.driver == driver)
        {
            continue;
        }
        for (detail_tag, detail) in [
            (0u8, GeneratedMetaDetail::Names),
            (1u8, GeneratedMetaDetail::Summary),
            (2u8, GeneratedMetaDetail::Full),
        ] {
            for (index, row) in
                generated_query_meta_rows(&artifact.artifact, artifact.digest, driver, detail)?
                    .into_iter()
                    .enumerate()
            {
                candidates.push((
                    driver_tag,
                    detail_tag,
                    i64::try_from(index).map_err(|_| {
                        "generated QueryMeta row index exceeds i64::MAX".to_string()
                    })?,
                    row,
                ));
            }
        }
    }

    let count = u32::try_from(candidates.len())
        .map_err(|_| "generated QueryMeta row count exceeds u32::MAX".to_string())?;
    let final_block = count
        .checked_mul(2)
        .ok_or_else(|| "generated QueryMeta block count exceeds u32::MAX".to_string())?;
    let mut values = Vec::new();
    let mut blocks = Vec::new();
    for (candidate, (driver, detail, index, _)) in candidates.iter().enumerate() {
        let candidate = u32::try_from(candidate)
            .map_err(|_| "generated QueryMeta block index exceeds u32::MAX".to_string())?;
        let mut stmts = Vec::new();
        let driver_eq = value_id(&mut values, Ty::Bool)?;
        stmts.push(Stmt::Let(
            driver_eq,
            Rvalue::Bin(
                BinOp::Eq,
                Operand::Arg(0),
                Operand::Const(Const::Int(i128::from(*driver), u8_ty)),
            ),
        ));
        let detail_eq = value_id(&mut values, Ty::Bool)?;
        stmts.push(Stmt::Let(
            detail_eq,
            Rvalue::Bin(
                BinOp::Eq,
                Operand::Arg(1),
                Operand::Const(Const::Int(i128::from(*detail), u8_ty)),
            ),
        ));
        let driver_detail = value_id(&mut values, Ty::Bool)?;
        stmts.push(Stmt::Let(
            driver_detail,
            Rvalue::Bin(
                BinOp::And,
                Operand::Value(driver_eq),
                Operand::Value(detail_eq),
            ),
        ));
        let index_eq = value_id(&mut values, Ty::Bool)?;
        stmts.push(Stmt::Let(
            index_eq,
            Rvalue::Bin(
                BinOp::Eq,
                Operand::Arg(2),
                Operand::Const(Const::Int(i128::from(*index), i64_ty)),
            ),
        ));
        let matched = value_id(&mut values, Ty::Bool)?;
        stmts.push(Stmt::Let(
            matched,
            Rvalue::Bin(
                BinOp::And,
                Operand::Value(driver_detail),
                Operand::Value(index_eq),
            ),
        ));
        blocks.push(Block {
            id: candidate,
            stmts,
            stmt_lines: Vec::new(),
            term: Term::Branch(
                Operand::Value(matched),
                count + candidate,
                if candidate + 1 == count {
                    final_block
                } else {
                    candidate + 1
                },
            ),
        });
    }
    for (candidate, (_, _, _, row)) in candidates.iter().enumerate() {
        let candidate = u32::try_from(candidate)
            .map_err(|_| "generated QueryMeta row block exceeds u32::MAX".to_string())?;
        let mut stmts = RowBlock {
            program,
            types: &types,
            values: &mut values,
            stmts: Vec::new(),
        }
        .row(row)?;
        let loaded = value_id(&mut values, row_ty)?;
        stmts.push(Stmt::Let(loaded, Rvalue::Load(3)));
        let wrapped = value_id(&mut values, option_ty)?;
        stmts.push(Stmt::Let(
            wrapped,
            Rvalue::OptionSome(Operand::Value(loaded)),
        ));
        blocks.push(Block {
            id: count + candidate,
            stmts,
            stmt_lines: Vec::new(),
            term: Term::Return(Some(Operand::Value(wrapped))),
        });
    }
    let none = value_id(&mut values, option_ty)?;
    blocks.push(Block {
        id: final_block,
        stmts: vec![Stmt::Let(none, Rvalue::OptionNone)],
        stmt_lines: Vec::new(),
        term: Term::Return(Some(Operand::Value(none))),
    });

    Ok((
        name.clone(),
        Function {
            name,
            params: vec![0, 1, 2],
            param_modes: vec![ParamMode::ByValue; 3],
            borrow_mut_cleanup_slots: vec![None; 3],
            ret: option_ty,
            return_borrow: hir::ReturnBorrowSummary::None,
            return_region: hir::ReturnRegionSummary::None,
            return_cleanup: hir::ReturnCleanupAbi::None,
            slots: vec![u8_ty, u8_ty, i64_ty, row_ty],
            slot_align: vec![None; 4],
            value_tys: values,
            blocks,
            entry: 0,
            exportable: false,
        },
    ))
}
