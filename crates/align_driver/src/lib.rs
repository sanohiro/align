//! Driver: connects the stages (`docs/impl/01-pipeline.md`).
//!
//! Exposes the `source.align` -> lexer -> parser -> sema -> MIR -> (codegen)
//! pipeline as library functions. Both the `alignc` binary (`main.rs`) and the
//! integration tests call this.

use align_diag::{Diagnostics, Severity};
use align_watch as watch_inputs;
use align_span::SourceMap;
use std::io::Read as _;
pub use align_codegen_llvm::{
    target_object_format, BuildTarget, DebugInfo, ObjectFormat, PartitionCodegenView,
    PartitionSharedCodegenView, Profile, SupportThunkOwner, SupportThunkRecord,
    ThinFunctionLinkage, ThinPeerDeclaration,
};
/// The lowered MIR program type (re-exported so callers can name it without depending on
/// `align_mir` directly).
pub use align_mir::Program as MirProgram;
/// M15 interface-summary types (re-exported so callers can name the [`check_per_unit`] result without
/// depending on `align_interface` directly).
pub use align_interface::{Hash128, InterfaceSummary};
pub use align_sema::{
    StaticDescriptor, StaticDescriptorConsumer, StaticDescriptorDriver, StaticDescriptorSource,
};

pub mod cache;
pub mod db_prepare;
pub mod db_prepare_native;
pub mod db_migrate;
pub mod db_migrate_native;
mod db_postgres_status;
pub mod explain;
pub mod memo;
mod query_meta_codegen;
pub mod static_artifacts;
pub mod static_inputs;
mod watch_link;

pub use align_watch::{
    BuildInput, BuildInputSet, BuildInputState, BuildInputTopologyError, BuildSourceError,
    FinalBuildInputSet, FinalizedWatchInputs, WatchRepairDependency, finalize_watch_inputs,
    merge_observed_build_inputs, snapshot_watch_repair,
};
pub use watch_link::{
    LinkOutputSink, LinkOutputStream, LinkStopSignal, link_objects_instrumented_with_output,
    link_objects_with_output,
};
pub mod static_runtime;
pub mod unit_cache;

pub use cache::{
    cas_blob_path, clear_cache, BackendKey, CacheContext, CacheLookup, CacheOutcome, CacheStage,
    CodegenKey, FirstDiff, ImportSourceDigest, InboundImport, PartitionKey, PgoKey, PrelinkKey,
    ThinPartitionSource, CACHE_KEY_FORMAT_VERSION,
};

pub use static_artifacts::{BuiltStaticArtifact, StaticArtifactBuildError, build_static_artifacts};
pub use static_inputs::{
    compose_codegen_impl_hash, metadata_logical_path, metadata_path, resolve_inline_static_input,
    resolve_static_descriptors, resolve_static_file, snapshot_checked_metadata,
    CheckedMetadataInput, MetadataState, ResolvedStaticInput, ResolvedStaticInputs,
    StaticConsumerKind, StaticDescriptorInputError, StaticDescriptorInputErrorCause, StaticInput,
    StaticInputError, StaticInputManifest, STATIC_INPUT_MANIFEST_FORMAT_VERSION,
    STATIC_INPUT_MANIFEST_MAGIC,
};
use static_inputs::lock_metadata_publication_shared;
pub use static_runtime::{
    FakeBoundValue, FakeCardinality, FakeDecodedField, FakeExecution, FakeExecutionError,
    FakeStatementKind, FakeValue, GeneratedBindField, GeneratedBindThunk, GeneratedCommandRuntime,
    GeneratedDecodeField, GeneratedDecodeThunk, GeneratedDriverRuntime, GeneratedMetaDetail,
    GeneratedQueryMetaEntry, GeneratedQueryMetaRow, GeneratedQueryMetaThunk, GeneratedQueryRuntime,
    GeneratedStaticRuntime, GeneratedValueKind, GeneratedValueShape, STATIC_RUNTIME_FORMAT_VERSION,
    execute_fake_static, generated_query_meta_rows,
};
// Keep the driver selector types alongside the resolver so callers do not need
// to depend on the interface crate just to construct a static input request.
pub use align_interface::{Driver, DriverRestriction};

fn current_owned_json_target() -> Result<align_interface::OwnedJsonTarget, String> {
    let object_format = match target_object_format()? {
        ObjectFormat::Elf => align_interface::OwnedJsonObjectFormat::Elf,
        ObjectFormat::MachO => align_interface::OwnedJsonObjectFormat::MachO,
    };
    Ok(align_interface::OwnedJsonTarget {
        triple: align_codegen_llvm::default_triple(),
        object_format,
    })
}

/// Whether a PostgreSQL native parameter type is the exact wire mapping for the generated Align
/// binder shape. Keep this decision on the closed generated tag: source `str`/`string` and
/// `slice<u8>`/`array<u8>` have already converged to their one runtime representation here, while
/// nullability remains an orthogonal `Option` bit.
fn postgres_parameter_type_matches(
    kind: GeneratedValueKind,
    canonical_type_name: &str,
) -> bool {
    match kind {
        GeneratedValueKind::Bool => canonical_type_name == "bool",
        GeneratedValueKind::I16 => canonical_type_name == "int2",
        GeneratedValueKind::I32 => canonical_type_name == "int4",
        GeneratedValueKind::I64 => canonical_type_name == "int8",
        GeneratedValueKind::F32 => canonical_type_name == "float4",
        GeneratedValueKind::F64 => canonical_type_name == "float8",
        GeneratedValueKind::Text => {
            matches!(canonical_type_name, "text" | "varchar" | "name")
        }
        GeneratedValueKind::Bytes => canonical_type_name == "bytea",
    }
}

/// Result of running the pipeline through sema.
pub struct Checked {
    pub hir: align_sema::Program,
    pub static_descriptors: Vec<StaticDescriptor>,
    /// The source-level module path that owns the entry function. Whole-program descriptor
    /// installation uses it to distinguish the entry's plain symbols from non-entry mangled ones.
    pub entry_unit: String,
    pub diags: Diagnostics,
}
fn static_interface_hash(
    base: Hash128,
    unit: &str,
    descriptors: &[StaticDescriptor],
) -> Result<Hash128, String> {
    fn bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<(), String> {
        let len =
            u32::try_from(value.len()).map_err(|_| "static interface field exceeds u32::MAX")?;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(value);
        Ok(())
    }
    let mut public = descriptors
        .iter()
        .filter(|descriptor| descriptor.is_public && descriptor.unit == unit)
        .collect::<Vec<_>>();
    public.sort_by(|left, right| {
        left.descriptor_id
            .as_bytes()
            .cmp(right.descriptor_id.as_bytes())
    });
    if public.is_empty() {
        return Ok(base);
    }
    let mut encoded = b"ALIGNSTI\0".to_vec();
    encoded.extend_from_slice(&base.lo.to_le_bytes());
    encoded.extend_from_slice(&base.hi.to_le_bytes());
    for descriptor in public {
        bytes(&mut encoded, descriptor.descriptor_id.as_bytes())?;
        encoded.push(match descriptor.consumer {
            StaticDescriptorConsumer::Query => 0,
            StaticDescriptorConsumer::Command => 1,
        });
        encoded.push(match descriptor.driver {
            StaticDescriptorDriver::AnySupportedDriver => 0,
            StaticDescriptorDriver::SQLiteOnly => 1,
            StaticDescriptorDriver::PostgreSQLOnly => 2,
        });
        let params = align_interface::CanonicalContract::try_from(&descriptor.params_contract)
            .map_err(|error| error.to_string())?;
        let params = params.fingerprint().map_err(|error| error.to_string())?;
        encoded.extend_from_slice(&params.lo.to_le_bytes());
        encoded.extend_from_slice(&params.hi.to_le_bytes());
        match &descriptor.row_contract {
            Some(row) => {
                encoded.push(1);
                let row = align_interface::CanonicalContract::try_from(row)
                    .map_err(|error| error.to_string())?;
                let row = row.fingerprint().map_err(|error| error.to_string())?;
                encoded.extend_from_slice(&row.lo.to_le_bytes());
                encoded.extend_from_slice(&row.hi.to_le_bytes());
            }
            None => encoded.push(0),
        }
        let has_check = descriptor
            .static_options
            .iter()
            .any(|option| matches!(option, align_sema::StaticDescriptorOption::Check(_)));
        let option_count = u32::try_from(descriptor.static_options.len() + usize::from(!has_check))
            .map_err(|_| "too many static descriptor options")?;
        encoded.extend_from_slice(&option_count.to_le_bytes());
        if !has_check {
            encoded.extend_from_slice(&[0, 0]);
        }
        for option in &descriptor.static_options {
            match option {
                align_sema::StaticDescriptorOption::Check(policy) => {
                    encoded.extend_from_slice(&[
                        0,
                        match policy {
                            align_sema::StaticCheckPolicy::DeclaredOnly => 0,
                            align_sema::StaticCheckPolicy::CheckedOptional => 1,
                            align_sema::StaticCheckPolicy::CheckedRequired => 2,
                        },
                    ]);
                }
                align_sema::StaticDescriptorOption::SQLiteRequireVersionAtLeast {
                    major,
                    minor,
                    patch,
                } => {
                    encoded.push(1);
                    encoded.extend_from_slice(&major.to_le_bytes());
                    encoded.extend_from_slice(&minor.to_le_bytes());
                    encoded.extend_from_slice(&patch.to_le_bytes());
                }
                align_sema::StaticDescriptorOption::PostgreSQLParameterType {
                    parameter_name,
                    canonical_type_name,
                } => {
                    encoded.push(2);
                    bytes(&mut encoded, parameter_name.as_bytes())?;
                    bytes(&mut encoded, canonical_type_name.as_bytes())?;
                }
            }
        }
    }
    Ok(Hash128::of(&encoded))
}

fn static_implementation_hash(
    base: Hash128,
    manifest: &StaticInputManifest,
    artifacts: &[BuiltStaticArtifact],
) -> Result<Hash128, String> {
    let manifest_digest = manifest.action_key().map_err(|error| error.to_string())?;
    let mut encoded = b"ALIGNSTP\0".to_vec();
    encoded.extend_from_slice(&manifest_digest.lo.to_le_bytes());
    encoded.extend_from_slice(&manifest_digest.hi.to_le_bytes());
    let mut ordered = artifacts.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.descriptor_id
            .as_bytes()
            .cmp(right.descriptor_id.as_bytes())
    });
    let count = u32::try_from(ordered.len()).map_err(|_| "too many static artifacts")?;
    encoded.extend_from_slice(&count.to_le_bytes());
    for artifact in ordered {
        let id = artifact.descriptor_id.as_bytes();
        let len = u32::try_from(id.len()).map_err(|_| "static descriptor id exceeds u32::MAX")?;
        encoded.extend_from_slice(&len.to_le_bytes());
        encoded.extend_from_slice(id);
        encoded.extend_from_slice(&artifact.digest.lo.to_le_bytes());
        encoded.extend_from_slice(&artifact.digest.hi.to_le_bytes());
    }
    Ok(compose_codegen_impl_hash(base, Hash128::of(&encoded)))
}

/// Replace each source-level static descriptor function with the producer-owned immutable data
/// constructor that D1 executes. The private descriptor field is a raw pointer to a canonical,
/// codegen-owned byte global; application source cannot spell or inspect that field.
fn rows_resource_names_for_descriptor(row: &align_sema::hir::StructDef) -> [String; 2] {
    db_resource_names_for_descriptor("rows", row)
}

fn batch_resource_names_for_descriptor(row: &align_sema::hir::StructDef) -> [String; 2] {
    db_resource_names_for_descriptor("batch", row)
}

fn db_resource_names_for_descriptor(
    resource: &str,
    row: &align_sema::hir::StructDef,
) -> [String; 2] {
    let direct = format!("pkg.db${resource}$S{}_{}", row.name.len(), row.name);
    // Per-unit generic-body reconstruction spells an application nominal through an ordinary
    // source identifier before substituting it into `rows<R>` (`app.users$Row` becomes
    // `app_users_Row`). Static-descriptor installation runs afterward against the producer's
    // canonical dotted/$ type table. Recognize both spellings so the generated streaming decoder
    // reuses the one semantic resource instance and its producer-owned Drop hook.
    let reconstructed_name = row
        .name
        .chars()
        .map(|character| match character {
            '.' | '$' => '_',
            other => other,
        })
        .collect::<String>();
    let reconstructed = format!(
        "pkg.db${resource}$S{}_{}",
        reconstructed_name.len(),
        reconstructed_name
    );
    [direct, reconstructed]
}

fn install_static_descriptor_data(
    mir: &mut align_mir::Program,
    entry_unit: Option<&str>,
    descriptors: &[StaticDescriptor],
    artifacts: &[BuiltStaticArtifact],
) -> Result<(), String> {
    use align_ast::{BinOp, ParamMode};
    use align_mir::{
        Block, ColumnBatchInput, Const, DirectCall, Function, ImportedFn, Operand, ProgramCall,
        Rvalue, StaticData, StaticDataRelocation, StaticDataTarget, Stmt, Term,
    };
    use align_sema::{IntTy, StaticDescriptorConsumer, StaticDescriptorOption, Ty, hir};

    if descriptors.len() != artifacts.len() {
        return Err("static descriptor/artifact count mismatch".to_string());
    }
    if descriptors.is_empty() {
        return Ok(());
    }
    let i32_ty = Ty::Int(IntTy { bits: 32, signed: true });
    let u32_ty = Ty::Int(IntTy { bits: 32, signed: false });
    let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
    let u8_ty = Ty::Int(IntTy { bits: 8, signed: false });
    let i16_ty = Ty::Int(IntTy { bits: 16, signed: true });
    let f32_ty = Ty::Float(align_sema::FloatTy { bits: 32 });
    let f64_ty = Ty::Float(align_sema::FloatTy { bits: 64 });
    let bytes_ty = Ty::Slice(align_sema::Scalar::Int(IntTy { bits: 8, signed: false }));
    let none_borrow = hir::ReturnBorrowSummary::None;
    let none_region = hir::ReturnRegionSummary::None;
    let no_cleanup = hir::ReturnCleanupAbi::None;
    let program_call = |name: &str| {
        ProgramCall::try_from_logical(name)
            .map_err(|_| format!("generated static descriptor symbol `{name}` is invalid"))
    };
    let callback = |name: &str| program_call(&format!("pkg.db.internal${name}"));
    let bind_callbacks = [
        callback("bind_bool_v2")?,
        callback("bind_i16_v2")?,
        callback("bind_i32_v2")?,
        callback("bind_i64_v2")?,
        callback("bind_f32_v2")?,
        callback("bind_f64_v2")?,
        callback("bind_text_v2")?,
        callback("bind_bytes_v2")?,
    ];
    let measure_callbacks = [
        callback("measure_bool_v1")?,
        callback("measure_i16_v1")?,
        callback("measure_i32_v1")?,
        callback("measure_i64_v1")?,
        callback("measure_f32_v1")?,
        callback("measure_f64_v1")?,
        callback("measure_text_v1")?,
        callback("measure_bytes_v1")?,
    ];
    let sqlite_version_callback = callback("require_sqlite_version_v1")?;
    let postgres_type_callback = callback("set_postgres_type_v2")?;
    let row_count_callback = callback("validate_row_count_v3")?;
    let validate_field_metadata_callback = callback("validate_field_metadata_v3")?;
    let validate_field_value_callback = callback("validate_field_value_v3")?;
    let read_callbacks = [
        callback("read_bool_v2")?,
        callback("read_i16_v2")?,
        callback("read_i32_v2")?,
        callback("read_i64_v2")?,
        callback("read_f32_v2")?,
        callback("read_f64_v2")?,
    ];
    let read_view_pointer_callback = callback("read_view_pointer_v2")?;
    let read_view_length_callback = callback("read_view_length_v2")?;
    let value_ty = |kind: GeneratedValueKind| match kind {
        GeneratedValueKind::Bool => Ty::Bool,
        GeneratedValueKind::I16 => i16_ty,
        GeneratedValueKind::I32 => i32_ty,
        GeneratedValueKind::I64 => i64_ty,
        GeneratedValueKind::F32 => f32_ty,
        GeneratedValueKind::F64 => f64_ty,
        GeneratedValueKind::Text => Ty::Str,
        GeneratedValueKind::Bytes => bytes_ty,
    };
    let value_tag = |kind: GeneratedValueKind| match kind {
        GeneratedValueKind::Bool => 0_i128,
        GeneratedValueKind::I16 => 1,
        GeneratedValueKind::I32 => 2,
        GeneratedValueKind::I64 => 3,
        GeneratedValueKind::F32 => 4,
        GeneratedValueKind::F64 => 5,
        GeneratedValueKind::Text => 6,
        GeneratedValueKind::Bytes => 7,
    };
    let option_ty = |ty: Ty| {
        align_sema::ty_to_scalar(ty)
            .map(Ty::Option)
            .ok_or_else(|| "generated database field is not an Option scalar".to_string())
    };

    let mut ensure_import = |name: ProgramCall, params: Vec<Ty>, ret: Ty| {
        if mir.fns.iter().any(|function| function.name == name)
            || mir.imported_fns.iter().any(|function| function.name == name)
        {
            return;
        }
        mir.imported_fns.push(ImportedFn {
            name,
            param_modes: vec![ParamMode::ByValue; params.len()],
            params,
            ret,
            return_borrow: none_borrow.clone(),
            return_region: none_region.clone(),
            return_cleanup: no_cleanup,
        });
    };
    for (kind, callback) in [
        GeneratedValueKind::Bool,
        GeneratedValueKind::I16,
        GeneratedValueKind::I32,
        GeneratedValueKind::I64,
        GeneratedValueKind::F32,
        GeneratedValueKind::F64,
        GeneratedValueKind::Text,
        GeneratedValueKind::Bytes,
    ]
    .into_iter()
    .zip(bind_callbacks.iter().cloned())
    {
        ensure_import(
            callback,
            vec![Ty::Raw, u32_ty, option_ty(value_ty(kind))?],
            i32_ty,
        );
    }
    for (kind, callback) in [
        GeneratedValueKind::Bool,
        GeneratedValueKind::I16,
        GeneratedValueKind::I32,
        GeneratedValueKind::I64,
        GeneratedValueKind::F32,
        GeneratedValueKind::F64,
        GeneratedValueKind::Text,
        GeneratedValueKind::Bytes,
    ]
    .into_iter()
    .zip(measure_callbacks.iter().cloned())
    {
        ensure_import(
            callback,
            vec![Ty::Raw, u32_ty, option_ty(value_ty(kind))?],
            i32_ty,
        );
    }
    ensure_import(
        sqlite_version_callback.clone(),
        vec![Ty::Raw, u32_ty, u32_ty, u32_ty],
        i32_ty,
    );
    ensure_import(
        postgres_type_callback.clone(),
        vec![Ty::Raw, u32_ty, Ty::Str],
        i32_ty,
    );
    ensure_import(row_count_callback.clone(), vec![Ty::Raw, u32_ty], i32_ty);
    ensure_import(
        validate_field_metadata_callback.clone(),
        vec![Ty::Raw, u32_ty, Ty::Str, u8_ty],
        i32_ty,
    );
    ensure_import(
        validate_field_value_callback.clone(),
        vec![Ty::Raw, u32_ty, u8_ty, Ty::Bool],
        i32_ty,
    );
    for (kind, callback) in [
        GeneratedValueKind::Bool,
        GeneratedValueKind::I16,
        GeneratedValueKind::I32,
        GeneratedValueKind::I64,
        GeneratedValueKind::F32,
        GeneratedValueKind::F64,
    ]
    .into_iter()
    .zip(read_callbacks.iter().cloned())
    {
        ensure_import(callback, vec![Ty::Raw, u32_ty], option_ty(value_ty(kind))?);
    }
    ensure_import(
        read_view_pointer_callback.clone(),
        vec![Ty::Raw, u32_ty],
        Ty::Raw,
    );
    ensure_import(
        read_view_length_callback.clone(),
        vec![Ty::Raw, u32_ty],
        i64_ty,
    );
    let static_constructor_monomorph = |name: &str| {
        [
            "pkg.db$query_file$",
            "pkg.db$query$",
            "pkg.db$command_file$",
            "pkg.db$command$",
            "pkg.db.sqlite$query_file$",
            "pkg.db.sqlite$query$",
            "pkg.db.sqlite$command_file$",
            "pkg.db.sqlite$command$",
            "pkg.db.postgres$query_file$",
            "pkg.db.postgres$query$",
            "pkg.db.postgres$command_file$",
            "pkg.db.postgres$command$",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
    };
    let mut generated_functions = Vec::new();
    for descriptor in descriptors {
        let artifact = artifacts
            .iter()
            .find(|artifact| artifact.descriptor_id == descriptor.descriptor_id)
            .ok_or_else(|| {
                format!(
                    "static descriptor `{}` has no generated artifact",
                    descriptor.descriptor_id
                )
            })?;
        // Whole-program MIR keeps entry-unit functions plain and prefixes every other module;
        // per-unit MIR follows the same rule for the unit being emitted. The explicit entry-unit
        // identity prevents a malformed/non-entry descriptor from binding to an unrelated plain
        // function with the same item name.
        let symbol = if entry_unit == Some(descriptor.unit.as_str()) {
            descriptor.item.clone()
        } else {
            format!("{}${}", descriptor.unit, descriptor.item)
        };
        let function_index = mir
            .fns
            .iter()
            .position(|function| function.name.as_str() == symbol)
            .ok_or_else(|| format!("static descriptor function `{symbol}` is absent from MIR"))?;
        let function = &mir.fns[function_index];
        if !function.params.is_empty() {
            return Err(format!(
                "static descriptor function `{symbol}` unexpectedly has parameters"
            ));
        }
        let Ty::Struct(struct_id) = function.ret else {
            return Err(format!(
                "static descriptor function `{symbol}` does not return a struct"
            ));
        };
        let definition = mir.structs.get(struct_id as usize).ok_or_else(|| {
            format!("static descriptor function `{symbol}` has an unknown return type")
        })?;
        if !align_sema::static_descriptor_struct_is_valid(definition) {
            return Err(format!(
                "static descriptor function `{symbol}` has an invalid runtime representation"
            ));
        }
        let binder_fields = match &artifact.runtime {
            GeneratedStaticRuntime::Query(runtime) => &runtime.drivers,
            GeneratedStaticRuntime::Command(runtime) => &runtime.drivers,
        };
        let first_binder = binder_fields
            .first()
            .ok_or_else(|| format!("static descriptor `{}` has no driver", descriptor.descriptor_id))?
            .binder
            .fields
            .clone();
        if binder_fields
            .iter()
            .any(|driver| driver.binder.fields != first_binder)
        {
            return Err(format!(
                "static descriptor `{}` has driver-dependent binder ordinals",
                descriptor.descriptor_id
            ));
        }
        let params_field_tys = match descriptor.params_ty {
            Ty::Struct(id) => mir
                .structs
                .get(id as usize)
                .map(|definition| {
                    definition.fields.iter().map(|field| field.ty).collect::<Vec<_>>()
                })
                .ok_or_else(|| "generated descriptor Params struct is absent".to_string())?,
            _ => return Err("generated descriptor Params contract is not a struct".to_string()),
        };
        let binder_supported = first_binder.iter().all(|field| {
            let Some(source_ty) = params_field_tys.get(field.params_field_ordinal as usize) else {
                return false;
            };
            let base = value_ty(field.shape.kind);
            let expected = if field.shape.nullable {
                option_ty(base).ok()
            } else {
                Some(base)
            };
            if expected == Some(*source_ty) {
                return true;
            }
            let owned = match field.shape.kind {
                GeneratedValueKind::Text => Ty::String,
                GeneratedValueKind::Bytes => Ty::DynArray(align_sema::Scalar::Int(IntTy {
                    bits: 8,
                    signed: false,
                })),
                _ => return false,
            };
            if field.shape.nullable {
                option_ty(owned).ok() == Some(*source_ty)
            } else {
                *source_ty == owned
            }
        });
        let row_supported = match &artifact.runtime {
            GeneratedStaticRuntime::Query(runtime) => runtime.decoder.fields.iter().all(|field| {
                matches!(
                    field.shape.kind,
                    GeneratedValueKind::Bool
                        | GeneratedValueKind::I16
                        | GeneratedValueKind::I32
                        | GeneratedValueKind::I64
                        | GeneratedValueKind::F32
                        | GeneratedValueKind::F64
                        | GeneratedValueKind::Text
                        | GeneratedValueKind::Bytes
                )
            }),
            GeneratedStaticRuntime::Command(_) => true,
        };
        let descriptor_supported = binder_supported && row_supported;
        let emitted_binder_fields = if binder_supported {
            first_binder.as_slice()
        } else {
            &[]
        };

        let binder_name = program_call(&format!("{symbol}$static_bind_v2"))?;
        let static_name = program_call(&format!("{symbol}$static_validate_v1"))?;
        let row_name = program_call(&format!("{symbol}$row_validate_v2"))?;
        let decode_name = program_call(&format!("{symbol}$decode_v1"))?;
        let stream_decode_name = program_call(&format!("{symbol}$stream_decode_v1"))?;
        let parameter_ordinal_name = program_call(&format!("{symbol}$parameter_ordinal_v1"))?;
        let parameter_count_name = program_call(&format!("{symbol}$parameter_count_v1"))?;
        let query_meta_name = if descriptor.consumer == StaticDescriptorConsumer::Query {
            let (name, function) =
                query_meta_codegen::generate_query_meta_thunk(mir, &symbol, artifact)?;
            generated_functions.push(function);
            Some(name)
        } else {
            None
        };

        let mut binder_blocks = Vec::new();
        let mut binder_value_tys = vec![Ty::Bool, Ty::Bool];
        let field_count = emitted_binder_fields.len() as u32;
        let measure_start = 2;
        let measure_success = measure_start + field_count;
        let measure_fail_start = measure_success + 1;
        let encode_start = measure_fail_start + field_count;
        let encode_success = encode_start + field_count;
        let encode_fail_start = encode_success + 1;
        let invalid_mode = encode_fail_start + field_count;
        binder_blocks.push(Block {
            id: 0,
            stmts: vec![Stmt::Let(
                0,
                Rvalue::Bin(
                    BinOp::Eq,
                    Operand::Arg(2),
                    Operand::Const(Const::Int(0, u8_ty)),
                ),
            )],
            stmt_lines: Vec::new(),
            term: Term::Branch(Operand::Value(0), measure_start, 1),
        });
        binder_blocks.push(Block {
            id: 1,
            stmts: vec![Stmt::Let(
                1,
                Rvalue::Bin(
                    BinOp::Eq,
                    Operand::Arg(2),
                    Operand::Const(Const::Int(1, u8_ty)),
                ),
            )],
            stmt_lines: Vec::new(),
            term: Term::Branch(Operand::Value(1), encode_start, invalid_mode),
        });
        for (index, field) in emitted_binder_fields.iter().enumerate() {
            let field_value = binder_value_tys.len() as u32;
            let normalized_value = field_value + 1;
            let option_value = field_value + 2;
            let status_value = field_value + 3;
            let success_value = field_value + 4;
            let next = measure_start + index as u32 + 1;
            let fail = measure_fail_start + index as u32;
            let source_ty = *params_field_tys
                .get(field.params_field_ordinal as usize)
                .ok_or_else(|| "generated binder field ordinal is out of range".to_string())?;
            let base_ty = value_ty(field.shape.kind);
            let normalized_ty = if field.shape.nullable {
                option_ty(base_ty)?
            } else {
                base_ty
            };
            let option_value_ty = option_ty(base_ty)?;
            let measure_callback = measure_callbacks
                .get(value_tag(field.shape.kind) as usize)
                .ok_or_else(|| "generated binder value kind is out of range".to_string())?
                .clone();
            binder_value_tys.extend([source_ty, normalized_ty, option_value_ty, i32_ty, Ty::Bool]);
            binder_blocks.push(Block {
                id: measure_start + index as u32,
                stmts: vec![
                    Stmt::Let(
                        field_value,
                        Rvalue::Field(1, vec![field.params_field_ordinal]),
                    ),
                    Stmt::Let(normalized_value, Rvalue::Use(Operand::Value(field_value))),
                    Stmt::Let(
                        option_value,
                        if field.shape.nullable {
                            Rvalue::Use(Operand::Value(normalized_value))
                        } else {
                            Rvalue::OptionSome(Operand::Value(normalized_value))
                        },
                    ),
                    Stmt::Let(
                        status_value,
                        Rvalue::Call(
                            DirectCall::Program(measure_callback),
                            vec![
                                Operand::Arg(0),
                                Operand::Const(Const::Int(
                                    i128::from(field.protocol_ordinal),
                                    u32_ty,
                                )),
                                Operand::Value(option_value),
                            ],
                        ),
                    ),
                    Stmt::Let(
                        success_value,
                        Rvalue::Bin(
                            BinOp::Eq,
                            Operand::Value(status_value),
                            Operand::Const(Const::Int(0, i32_ty)),
                        ),
                    ),
                ],
                stmt_lines: Vec::new(),
                term: Term::Branch(Operand::Value(success_value), next, fail),
            });
        }
        binder_blocks.push(Block {
            id: measure_success,
            stmts: Vec::new(),
            stmt_lines: Vec::new(),
            term: Term::Return(Some(Operand::Const(Const::Int(
                if binder_supported { 0 } else { -1 },
                i32_ty,
            )))),
        });
        for (index, _) in emitted_binder_fields.iter().enumerate() {
            binder_blocks.push(Block {
                id: measure_fail_start + index as u32,
                stmts: Vec::new(),
                stmt_lines: Vec::new(),
                term: Term::Return(Some(Operand::Const(Const::Int(1, i32_ty)))),
            });
        }
        for (index, field) in emitted_binder_fields.iter().enumerate() {
            let field_value = binder_value_tys.len() as u32;
            let normalized_value = field_value + 1;
            let option_value = field_value + 2;
            let status_value = field_value + 3;
            let success_value = field_value + 4;
            let next = encode_start + index as u32 + 1;
            let fail = encode_fail_start + index as u32;
            let source_ty = *params_field_tys
                .get(field.params_field_ordinal as usize)
                .ok_or_else(|| "generated binder field ordinal is out of range".to_string())?;
            let base_ty = value_ty(field.shape.kind);
            let normalized_ty = if field.shape.nullable {
                option_ty(base_ty)?
            } else {
                base_ty
            };
            let option_value_ty = option_ty(base_ty)?;
            let bind_callback = bind_callbacks[value_tag(field.shape.kind) as usize].clone();
            binder_value_tys.extend([source_ty, normalized_ty, option_value_ty, i32_ty, Ty::Bool]);
            binder_blocks.push(Block {
                id: encode_start + index as u32,
                stmts: vec![
                    Stmt::Let(
                        field_value,
                        Rvalue::Field(1, vec![field.params_field_ordinal]),
                    ),
                    Stmt::Let(normalized_value, Rvalue::Use(Operand::Value(field_value))),
                    Stmt::Let(
                        option_value,
                        if field.shape.nullable {
                            Rvalue::Use(Operand::Value(normalized_value))
                        } else {
                            Rvalue::OptionSome(Operand::Value(normalized_value))
                        },
                    ),
                    Stmt::Let(
                        status_value,
                        Rvalue::Call(
                            DirectCall::Program(bind_callback),
                            vec![
                                Operand::Arg(0),
                                Operand::Const(Const::Int(
                                    i128::from(field.protocol_ordinal),
                                    u32_ty,
                                )),
                                Operand::Value(option_value),
                            ],
                        ),
                    ),
                    Stmt::Let(
                        success_value,
                        Rvalue::Bin(
                            BinOp::Eq,
                            Operand::Value(status_value),
                            Operand::Const(Const::Int(0, i32_ty)),
                        ),
                    ),
                ],
                stmt_lines: Vec::new(),
                term: Term::Branch(Operand::Value(success_value), next, fail),
            });
        }
        binder_blocks.push(Block {
            id: encode_success,
            stmts: Vec::new(),
            stmt_lines: Vec::new(),
            term: Term::Return(Some(Operand::Const(Const::Int(
                if binder_supported { 0 } else { -1 },
                i32_ty,
            )))),
        });
        for (index, _) in emitted_binder_fields.iter().enumerate() {
            binder_blocks.push(Block {
                id: encode_fail_start + index as u32,
                stmts: Vec::new(),
                stmt_lines: Vec::new(),
                term: Term::Return(Some(Operand::Const(Const::Int(1, i32_ty)))),
            });
        }
        binder_blocks.push(Block {
            id: invalid_mode,
            stmts: Vec::new(),
            stmt_lines: Vec::new(),
            term: Term::Return(Some(Operand::Const(Const::Int(-1, i32_ty)))),
        });
        generated_functions.push(Function {
            name: binder_name.clone(),
            params: vec![0, 1, 2],
            param_modes: vec![ParamMode::ByValue, ParamMode::Borrow, ParamMode::ByValue],
            borrow_mut_cleanup_slots: vec![None, None, None],
            ret: i32_ty,
            return_borrow: none_borrow.clone(),
            return_region: none_region.clone(),
            return_cleanup: no_cleanup,
            slots: vec![Ty::Raw, descriptor.params_ty, u8_ty],
            slot_align: vec![None, None, None],
            value_tys: binder_value_tys,
            blocks: binder_blocks,
            entry: 0,
            exportable: false,
        });
        generated_functions.push(Function {
            name: parameter_count_name.clone(),
            params: Vec::new(),
            param_modes: Vec::new(),
            borrow_mut_cleanup_slots: Vec::new(),
            ret: u32_ty,
            return_borrow: none_borrow.clone(),
            return_region: none_region.clone(),
            return_cleanup: no_cleanup,
            slots: Vec::new(),
            slot_align: Vec::new(),
            value_tys: Vec::new(),
            blocks: vec![Block {
                id: 0,
                stmts: Vec::new(),
                stmt_lines: Vec::new(),
                term: Term::Return(Some(Operand::Const(Const::Int(
                    first_binder.len() as i128,
                    u32_ty,
                )))),
            }],
            entry: 0,
            exportable: false,
        });

        let align_sema::StaticContractType::Named { path, args } =
            &descriptor.params_contract.root
        else {
            return Err("generated parameter contract root is not nominal".to_string());
        };
        let parameter_fields = descriptor
            .params_contract
            .definitions
            .iter()
            .find(|definition| definition.path == *path && definition.args == *args)
            .and_then(|definition| match &definition.kind {
                align_sema::StaticContractDefinitionBody::Struct { fields } => {
                    Some(fields.as_slice())
                }
                align_sema::StaticContractDefinitionBody::Sum { .. } => None,
            })
            .ok_or_else(|| "generated parameter contract is not a struct".to_string())?;
        let mut ordinal_blocks = Vec::new();
        let mut ordinal_value_tys = Vec::new();
        let miss_block = first_binder.len() as u32;
        for (index, field) in first_binder.iter().enumerate() {
            let name = parameter_fields
                .get(field.params_field_ordinal as usize)
                .ok_or_else(|| "generated binder field ordinal is out of range".to_string())?
                .name
                .clone();
            let literal = (index * 2) as u32;
            let equal = literal + 1;
            ordinal_value_tys.extend([Ty::Str, Ty::Bool]);
            ordinal_blocks.push(Block {
                id: index as u32,
                stmts: vec![
                    Stmt::Let(literal, Rvalue::StrLit(name)),
                    Stmt::Let(
                        equal,
                        Rvalue::Bin(BinOp::Eq, Operand::Arg(0), Operand::Value(literal)),
                    ),
                ],
                stmt_lines: Vec::new(),
                term: Term::Branch(
                    Operand::Value(equal),
                    miss_block + 1 + index as u32,
                    index as u32 + 1,
                ),
            });
        }
        ordinal_blocks.push(Block {
            id: miss_block,
            stmts: Vec::new(),
            stmt_lines: Vec::new(),
            term: Term::Return(Some(Operand::Const(Const::Int(0, i32_ty)))),
        });
        for (index, field) in first_binder.iter().enumerate() {
            let mut ordinal = i32::try_from(field.protocol_ordinal)
                .map_err(|_| "generated parameter ordinal exceeds i32::MAX".to_string())?;
            let field_name = parameter_fields
                .get(field.params_field_ordinal as usize)
                .ok_or_else(|| "generated binder field ordinal is out of range".to_string())?
                .name
                .as_str();
            let binary_eligible = descriptor.static_options.iter().any(|option| {
                matches!(
                    option,
                    StaticDescriptorOption::PostgreSQLParameterType {
                        parameter_name,
                        canonical_type_name,
                    } if parameter_name == field_name
                        && postgres_parameter_type_matches(
                            field.shape.kind,
                            canonical_type_name,
                        )
                )
            });
            if binary_eligible {
                ordinal = -ordinal;
            }
            ordinal_blocks.push(Block {
                id: miss_block + 1 + index as u32,
                stmts: Vec::new(),
                stmt_lines: Vec::new(),
                term: Term::Return(Some(Operand::Const(Const::Int(
                    i128::from(ordinal),
                    i32_ty,
                )))),
            });
        }
        generated_functions.push(Function {
            name: parameter_ordinal_name.clone(),
            params: vec![0],
            param_modes: vec![ParamMode::ByValue],
            borrow_mut_cleanup_slots: vec![None],
            ret: i32_ty,
            return_borrow: none_borrow.clone(),
            return_region: none_region.clone(),
            return_cleanup: no_cleanup,
            slots: vec![Ty::Str],
            slot_align: vec![None],
            value_tys: ordinal_value_tys,
            blocks: ordinal_blocks,
            entry: 0,
            exportable: false,
        });

        let mut static_calls: Vec<(ProgramCall, Vec<Operand>, Option<String>)> = Vec::new();
        for option in descriptor
            .static_options
            .iter()
            .filter(|_| descriptor_supported)
        {
            match option {
                StaticDescriptorOption::Check(_) => {}
                StaticDescriptorOption::SQLiteRequireVersionAtLeast { major, minor, patch } => {
                    static_calls.push((
                        sqlite_version_callback.clone(),
                        vec![
                            Operand::Arg(0),
                            Operand::Const(Const::Int(i128::from(*major), u32_ty)),
                            Operand::Const(Const::Int(i128::from(*minor), u32_ty)),
                            Operand::Const(Const::Int(i128::from(*patch), u32_ty)),
                        ],
                        None,
                    ));
                }
                StaticDescriptorOption::PostgreSQLParameterType {
                    parameter_name,
                    canonical_type_name,
                } => {
                    let align_sema::StaticContractType::Named { path, args } =
                        &descriptor.params_contract.root
                    else {
                        return Err("generated parameter contract root is not nominal".to_string());
                    };
                    let fields = descriptor
                        .params_contract
                        .definitions
                        .iter()
                        .find(|definition| definition.path == *path && definition.args == *args)
                        .and_then(|definition| match &definition.kind {
                            align_sema::StaticContractDefinitionBody::Struct { fields } => {
                                Some(fields.as_slice())
                            }
                            align_sema::StaticContractDefinitionBody::Sum { .. } => None,
                        })
                        .ok_or_else(|| "generated parameter contract is not a struct".to_string())?;
                    let field = first_binder
                        .iter()
                        .find(|field| {
                            fields
                                .get(field.params_field_ordinal as usize)
                                .is_some_and(|field| field.name == *parameter_name)
                        })
                        .ok_or_else(|| {
                            format!(
                                "static descriptor `{}` has an unknown PostgreSQL parameter `{parameter_name}`",
                                descriptor.descriptor_id
                            )
                        })?;
                    if !postgres_parameter_type_matches(
                        field.shape.kind,
                        canonical_type_name,
                    ) {
                        return Err(format!(
                            "static descriptor `{}` maps PostgreSQL parameter `{parameter_name}` to incompatible native type `{canonical_type_name}`",
                            descriptor.descriptor_id,
                        ));
                    }
                    static_calls.push((
                        postgres_type_callback.clone(),
                        vec![
                            Operand::Arg(0),
                            Operand::Const(Const::Int(i128::from(field.protocol_ordinal), u32_ty)),
                            Operand::Value(0),
                        ],
                        Some(canonical_type_name.clone()),
                    ));
                }
            }
        }
        let mut static_blocks = Vec::new();
        let mut static_value_tys = Vec::new();
        let static_call_count = static_calls.len();
        for (index, (target, mut args, type_name)) in static_calls.into_iter().enumerate() {
            let mut stmts = Vec::new();
            if let Some(canonical_type_name) = type_name {
                stmts.push(Stmt::Let(
                    (index * 3) as u32,
                    Rvalue::StrLit(canonical_type_name),
                ));
                *args.last_mut().expect("PostgreSQL option has type name") =
                    Operand::Value((index * 3) as u32);
                static_value_tys.push(Ty::Str);
            } else {
                static_value_tys.push(Ty::Unit);
            }
            let status = (index * 3 + 1) as u32;
            let success = status + 1;
            stmts.push(Stmt::Let(status, Rvalue::Call(DirectCall::Program(target), args)));
            stmts.push(Stmt::Let(
                success,
                Rvalue::Bin(
                    BinOp::Eq,
                    Operand::Value(status),
                    Operand::Const(Const::Int(0, i32_ty)),
                ),
            ));
            static_value_tys.extend([i32_ty, Ty::Bool]);
            static_blocks.push(Block {
                id: index as u32,
                stmts,
                stmt_lines: Vec::new(),
                term: Term::Branch(
                    Operand::Value(success),
                    (index + 1) as u32,
                    (static_call_count + index + 1) as u32,
                ),
            });
        }
        let static_count = static_blocks.len();
        static_blocks.push(Block {
            id: static_count as u32,
            stmts: Vec::new(),
            stmt_lines: Vec::new(),
            term: Term::Return(Some(Operand::Const(Const::Int(
                if descriptor_supported { 0 } else { -1 },
                i32_ty,
            )))),
        });
        for index in 0..static_count {
            static_blocks.push(Block {
                id: (static_count + index + 1) as u32,
                stmts: Vec::new(),
                stmt_lines: Vec::new(),
                term: Term::Return(Some(Operand::Const(Const::Int(1, i32_ty)))),
            });
        }
        generated_functions.push(Function {
            name: static_name.clone(),
            params: vec![0],
            param_modes: vec![ParamMode::ByValue],
            borrow_mut_cleanup_slots: vec![None],
            ret: i32_ty,
            return_borrow: none_borrow.clone(),
            return_region: none_region.clone(),
            return_cleanup: no_cleanup,
            slots: vec![Ty::Raw],
            slot_align: vec![None],
            value_tys: static_value_tys,
            blocks: static_blocks,
            entry: 0,
            exportable: false,
        });

        let mut header = vec![0u8; 144];
        header[0..4].copy_from_slice(&6u32.to_le_bytes());
        header[4] = match descriptor.consumer {
            StaticDescriptorConsumer::Query => 0,
            StaticDescriptorConsumer::Command => 1,
        };
        let mut driver_mask = 0u8;
        let mut sqlite_sql = None;
        let mut postgres_sql = None;
        for driver in binder_fields {
            match driver.driver {
                Driver::SQLite => {
                    driver_mask |= 1;
                    sqlite_sql = Some(driver.wire_sql.clone());
                }
                Driver::PostgreSQL => {
                    driver_mask |= 2;
                    postgres_sql = Some(driver.wire_sql.clone());
                }
            }
        }
        header[5] = driver_mask;
        header[24..32].copy_from_slice(&(descriptor.descriptor_id.len() as i64).to_le_bytes());
        header[112..116].copy_from_slice(
            &u32::try_from(first_binder.len())
                .map_err(|_| "generated parameter count exceeds u32::MAX".to_string())?
                .to_le_bytes(),
        );
        if let Some(sql) = &sqlite_sql {
            header[40..48].copy_from_slice(&(sql.len() as i64).to_le_bytes());
        }
        if let Some(sql) = &postgres_sql {
            header[56..64].copy_from_slice(&(sql.len() as i64).to_le_bytes());
        }
        let mut relocations = vec![
            StaticDataRelocation {
                offset: 8,
                target: StaticDataTarget::Bytes {
                    bytes: artifact.runtime.bytes().to_vec(),
                    nul_terminated: false,
                },
            },
            StaticDataRelocation {
                offset: 16,
                target: StaticDataTarget::Bytes {
                    bytes: descriptor.descriptor_id.as_bytes().to_vec(),
                    nul_terminated: false,
                },
            },
            StaticDataRelocation {
                offset: 64,
                target: StaticDataTarget::Function(binder_name),
            },
            StaticDataRelocation {
                offset: 72,
                target: StaticDataTarget::Function(static_name),
            },
            StaticDataRelocation {
                offset: 104,
                target: StaticDataTarget::Function(parameter_ordinal_name),
            },
            StaticDataRelocation {
                offset: 136,
                target: StaticDataTarget::Function(parameter_count_name),
            },
        ];
        if let Some(sql) = sqlite_sql {
            relocations.push(StaticDataRelocation {
                offset: 32,
                target: StaticDataTarget::Bytes {
                    bytes: sql,
                    nul_terminated: true,
                },
            });
        }
        if let Some(sql) = postgres_sql {
            relocations.push(StaticDataRelocation {
                offset: 48,
                target: StaticDataTarget::Bytes {
                    bytes: sql,
                    nul_terminated: true,
                },
            });
        }
        if descriptor.consumer == StaticDescriptorConsumer::Query {
            let GeneratedStaticRuntime::Query(runtime) = &artifact.runtime else {
                return Err("query descriptor has command runtime data".to_string());
            };
            let mut metadata_calls = Vec::new();
            if row_supported {
                metadata_calls.push((
                    row_count_callback.clone(),
                    vec![
                        Operand::Arg(0),
                        Operand::Const(Const::Int(runtime.decoder.fields.len() as i128, u32_ty)),
                    ],
                    None,
                ));
            }
            for field in runtime.decoder.fields.iter().filter(|_| row_supported) {
                let expected_name = runtime
                    .query_meta
                    .plan
                    .columns
                    .get(field.row_field_ordinal as usize)
                    .ok_or_else(|| "generated row metadata ordinal is out of range".to_string())?
                    .source_alias
                    .clone();
                metadata_calls.push((
                    validate_field_metadata_callback.clone(),
                    vec![
                        Operand::Arg(0),
                        Operand::Const(Const::Int(i128::from(field.row_field_ordinal), u32_ty)),
                        Operand::Value(0),
                        Operand::Const(Const::Int(value_tag(field.shape.kind), u8_ty)),
                    ],
                    Some(expected_name),
                ));
            }
            let mut blocks = Vec::new();
            let mut values = vec![Ty::Bool, Ty::Bool];
            let metadata_count = metadata_calls.len() as u32;
            let value_count = if row_supported {
                runtime.decoder.fields.len() as u32
            } else {
                0
            };
            let metadata_start = 2;
            let metadata_success = metadata_start + metadata_count;
            let metadata_fail_start = metadata_success + 1;
            let value_start = metadata_fail_start + metadata_count;
            let value_success = value_start + value_count;
            let value_fail_start = value_success + 1;
            let invalid_mode = value_fail_start + value_count;
            blocks.push(Block {
                id: 0,
                stmts: vec![Stmt::Let(
                    0,
                    Rvalue::Bin(
                        BinOp::Eq,
                        Operand::Arg(1),
                        Operand::Const(Const::Int(0, u8_ty)),
                    ),
                )],
                stmt_lines: Vec::new(),
                term: Term::Branch(Operand::Value(0), metadata_start, 1),
            });
            blocks.push(Block {
                id: 1,
                stmts: vec![Stmt::Let(
                    1,
                    Rvalue::Bin(
                        BinOp::Eq,
                        Operand::Arg(1),
                        Operand::Const(Const::Int(1, u8_ty)),
                    ),
                )],
                stmt_lines: Vec::new(),
                term: Term::Branch(Operand::Value(1), value_start, invalid_mode),
            });
            for (index, (target, mut args, name)) in metadata_calls.into_iter().enumerate() {
                let base = values.len() as u32;
                let mut stmts = Vec::new();
                if let Some(name) = name {
                    stmts.push(Stmt::Let(base, Rvalue::StrLit(name)));
                    let Some(name_argument) = args.get_mut(2) else {
                        return Err("generated row field callback has no name argument".to_string());
                    };
                    *name_argument = Operand::Value(base);
                    values.push(Ty::Str);
                } else {
                    values.push(Ty::Unit);
                }
                stmts.push(Stmt::Let(
                    base + 1,
                    Rvalue::Call(DirectCall::Program(target), args),
                ));
                stmts.push(Stmt::Let(
                    base + 2,
                    Rvalue::Bin(
                        BinOp::Eq,
                        Operand::Value(base + 1),
                        Operand::Const(Const::Int(0, i32_ty)),
                    ),
                ));
                values.extend([i32_ty, Ty::Bool]);
                blocks.push(Block {
                    id: metadata_start + index as u32,
                    stmts,
                    stmt_lines: Vec::new(),
                    term: Term::Branch(
                        Operand::Value(base + 2),
                        metadata_start + index as u32 + 1,
                        metadata_fail_start + index as u32,
                    ),
                });
            }
            blocks.push(Block {
                id: metadata_success,
                stmts: Vec::new(),
                stmt_lines: Vec::new(),
                term: Term::Return(Some(Operand::Const(Const::Int(
                    if row_supported { 0 } else { -1 },
                    i32_ty,
                )))),
            });
            for index in 0..metadata_count {
                blocks.push(Block {
                    id: metadata_fail_start + index,
                    stmts: Vec::new(),
                    stmt_lines: Vec::new(),
                    term: Term::Return(Some(Operand::Const(Const::Int(1, i32_ty)))),
                });
            }
            for (index, field) in runtime
                .decoder
                .fields
                .iter()
                .filter(|_| row_supported)
                .enumerate()
            {
                let status = values.len() as u32;
                let success = status + 1;
                values.extend([i32_ty, Ty::Bool]);
                blocks.push(Block {
                    id: value_start + index as u32,
                    stmts: vec![
                        Stmt::Let(
                            status,
                            Rvalue::Call(
                                DirectCall::Program(validate_field_value_callback.clone()),
                                vec![
                                    Operand::Arg(0),
                                    Operand::Const(Const::Int(
                                        i128::from(field.row_field_ordinal),
                                        u32_ty,
                                    )),
                                    Operand::Const(Const::Int(
                                        value_tag(field.shape.kind),
                                        u8_ty,
                                    )),
                                    Operand::Const(Const::Bool(field.shape.nullable)),
                                ],
                            ),
                        ),
                        Stmt::Let(
                            success,
                            Rvalue::Bin(
                                BinOp::Eq,
                                Operand::Value(status),
                                Operand::Const(Const::Int(0, i32_ty)),
                            ),
                        ),
                    ],
                    stmt_lines: Vec::new(),
                    term: Term::Branch(
                        Operand::Value(success),
                        value_start + index as u32 + 1,
                        value_fail_start + index as u32,
                    ),
                });
            }
            blocks.push(Block {
                id: value_success,
                stmts: Vec::new(),
                stmt_lines: Vec::new(),
                term: Term::Return(Some(Operand::Const(Const::Int(
                    if row_supported { 0 } else { -1 },
                    i32_ty,
                )))),
            });
            for index in 0..value_count {
                blocks.push(Block {
                    id: value_fail_start + index,
                    stmts: Vec::new(),
                    stmt_lines: Vec::new(),
                    term: Term::Return(Some(Operand::Const(Const::Int(1, i32_ty)))),
                });
            }
            blocks.push(Block {
                id: invalid_mode,
                stmts: Vec::new(),
                stmt_lines: Vec::new(),
                term: Term::Return(Some(Operand::Const(Const::Int(-1, i32_ty)))),
            });
            generated_functions.push(Function {
                name: row_name.clone(),
                params: vec![0, 1],
                param_modes: vec![ParamMode::ByValue, ParamMode::ByValue],
                borrow_mut_cleanup_slots: vec![None, None],
                ret: i32_ty,
                return_borrow: none_borrow.clone(),
                return_region: none_region.clone(),
                return_cleanup: no_cleanup,
                slots: vec![Ty::Raw, u8_ty],
                slot_align: vec![None, None],
                value_tys: values,
                blocks,
                entry: 0,
                exportable: false,
            });

            let row_ty = descriptor
                .row_ty
                .ok_or_else(|| "query descriptor has no row type".to_string())?;
            let Ty::Struct(row_struct_id) = row_ty else {
                return Err("generated query Row contract is not a struct".to_string());
            };
            let row_definition = mir
                .structs
                .get(row_struct_id as usize)
                .ok_or_else(|| "generated query Row struct is absent".to_string())?;
            let rows_names = rows_resource_names_for_descriptor(row_definition);
            let rows_resource = if let Some(index) = mir
                .resources
                .iter()
                .position(|resource| resource.name == rows_names[0])
                .or_else(|| {
                    mir.resources
                        .iter()
                        .position(|resource| resource.name == rows_names[1])
                })
            {
                index as u32
            } else {
                let mut resource = mir
                    .resources
                    .iter()
                    .find(|resource| resource.name == "pkg.db$rows")
                    .cloned()
                    .unwrap_or_else(|| hir::ResourceDef {
                        name: "pkg.db$rows".to_string(),
                        source_name: "pkg.db$rows".to_string(),
                        declaring_module: "pkg.db".to_string(),
                        generic_arity: 1,
                        // The descriptor producer needs only the one-pointer ResourceRef type for
                        // current-row provenance. The canonical pkg.db unit owns the shared Drop
                        // thunk definition; keeping this hook unresolved makes this consumer-side
                        // record emit a declaration rather than a duplicate definition.
                        drop_hook: "__align_db_stream_owner_drop".to_string(),
                        drop_thunk: "__align_resource_drop$pkg.db$rows".to_string(),
                        representation_version: 1,
                        drop_abi_fingerprint: *b"align-res-drop-1",
                    });
                resource.name = rows_names[0].clone();
                resource.source_name = rows_names[0].clone();
                let id = mir.resources.len() as u32;
                mir.resources.push(resource);
                id
            };
            let row_borrows = align_sema::ty_may_borrow(
                row_ty,
                &mir.structs,
                &mir.tuples,
                &mir.enums,
                &mir.tagged_types,
            );
            let stream_return_borrow = if row_borrows {
                hir::ReturnBorrowSummary::Roots {
                    params: vec![1],
                    captures: Vec::new(),
                }
            } else {
                none_borrow.clone()
            };
            let stream_return_region = if row_borrows {
                hir::ReturnRegionSummary::Roots {
                    params: vec![1],
                    captures: Vec::new(),
                }
            } else {
                none_region.clone()
            };
            let materialized_supported = row_supported
                && runtime.decoder.fields.iter().all(|field| {
                    !matches!(
                        field.shape.kind,
                        GeneratedValueKind::Text | GeneratedValueKind::Bytes
                    )
                });
            let mut decode_stmts = Vec::new();
            let mut decode_values = Vec::new();
            for field in runtime
                .decoder
                .fields
                .iter()
                .filter(|_| materialized_supported)
            {
                let base_ty = value_ty(field.shape.kind);
                let option_value_ty = option_ty(base_ty)?;
                let option_value = decode_values.len() as u32;
                decode_values.push(option_value_ty);
                decode_stmts.push(Stmt::Let(
                    option_value,
                    Rvalue::Call(
                        DirectCall::Program(
                            read_callbacks[value_tag(field.shape.kind) as usize].clone(),
                        ),
                        vec![
                            Operand::Arg(0),
                            Operand::Const(Const::Int(i128::from(field.row_field_ordinal), u32_ty)),
                        ],
                    ),
                ));
                let field_value = if field.shape.nullable {
                    Operand::Value(option_value)
                } else {
                    let value = decode_values.len() as u32;
                    decode_values.push(base_ty);
                    decode_stmts.push(Stmt::Let(
                        value,
                        Rvalue::OptionUnwrap(Operand::Value(option_value)),
                    ));
                    Operand::Value(value)
                };
                decode_stmts.push(Stmt::StoreField(
                    1,
                    vec![field.row_field_ordinal],
                    field_value,
                ));
            }
            let result_value = decode_values.len() as u32;
            if materialized_supported {
                decode_values.push(row_ty);
                decode_stmts.push(Stmt::Let(result_value, Rvalue::Load(1)));
            }
            let mut stream_decode_stmts = Vec::new();
            let mut stream_decode_values = Vec::new();
            for field in runtime.decoder.fields.iter().filter(|_| row_supported) {
                let base_ty = value_ty(field.shape.kind);
                let option_value_ty = option_ty(base_ty)?;
                let option_value = if matches!(
                    field.shape.kind,
                    GeneratedValueKind::Text | GeneratedValueKind::Bytes
                ) {
                    let pointer = stream_decode_values.len() as u32;
                    stream_decode_values.push(Ty::Raw);
                    stream_decode_stmts.push(Stmt::Let(
                        pointer,
                        Rvalue::Call(
                            DirectCall::Program(read_view_pointer_callback.clone()),
                            vec![
                                Operand::Arg(0),
                                Operand::Const(Const::Int(
                                    i128::from(field.row_field_ordinal),
                                    u32_ty,
                                )),
                            ],
                        ),
                    ));
                    let length = stream_decode_values.len() as u32;
                    stream_decode_values.push(i64_ty);
                    stream_decode_stmts.push(Stmt::Let(
                        length,
                        Rvalue::Call(
                            DirectCall::Program(read_view_length_callback.clone()),
                            vec![
                                Operand::Arg(0),
                                Operand::Const(Const::Int(
                                    i128::from(field.row_field_ordinal),
                                    u32_ty,
                                )),
                            ],
                        ),
                    ));
                    let option_value = stream_decode_values.len() as u32;
                    stream_decode_values.push(option_value_ty);
                    let (view, check_utf8) = match field.shape.kind {
                        GeneratedValueKind::Text => (hir::ResourceViewKind::StrUtf8, true),
                        GeneratedValueKind::Bytes => {
                            (
                                hir::ResourceViewKind::Slice(align_sema::Scalar::Int(IntTy {
                                    bits: 8,
                                    signed: false,
                                })),
                                false,
                            )
                        }
                        _ => {
                            return Err(
                                "generated streaming view field has a non-view kind".to_string(),
                            );
                        }
                    };
                    stream_decode_stmts.push(Stmt::Let(
                        option_value,
                        Rvalue::ResourceViewFromRaw {
                            owner: Operand::Arg(1),
                            ptr: Operand::Value(pointer),
                            len: Operand::Value(length),
                            resource: rows_resource,
                            view,
                            allow_null_if_empty: true,
                            check_nonnegative_len: true,
                            check_alignment: 1,
                            check_utf8,
                        },
                    ));
                    option_value
                } else {
                    let option_value = stream_decode_values.len() as u32;
                    stream_decode_values.push(option_value_ty);
                    stream_decode_stmts.push(Stmt::Let(
                        option_value,
                        Rvalue::Call(
                            DirectCall::Program(
                                read_callbacks[value_tag(field.shape.kind) as usize].clone(),
                            ),
                            vec![
                                Operand::Arg(0),
                                Operand::Const(Const::Int(
                                    i128::from(field.row_field_ordinal),
                                    u32_ty,
                                )),
                            ],
                        ),
                    ));
                    option_value
                };
                let field_value = if field.shape.nullable {
                    Operand::Value(option_value)
                } else {
                    let value = stream_decode_values.len() as u32;
                    stream_decode_values.push(base_ty);
                    stream_decode_stmts.push(Stmt::Let(
                        value,
                        Rvalue::OptionUnwrap(Operand::Value(option_value)),
                    ));
                    Operand::Value(value)
                };
                stream_decode_stmts.push(Stmt::StoreField(
                    2,
                    vec![field.row_field_ordinal],
                    field_value,
                ));
            }
            let stream_result_value = stream_decode_values.len() as u32;
            if row_supported {
                stream_decode_values.push(row_ty);
                stream_decode_stmts.push(Stmt::Let(stream_result_value, Rvalue::Load(2)));
            }
            generated_functions.push(Function {
                name: decode_name.clone(),
                params: vec![0],
                param_modes: vec![ParamMode::ByValue],
                borrow_mut_cleanup_slots: vec![None],
                ret: row_ty,
                return_borrow: none_borrow.clone(),
                return_region: none_region.clone(),
                return_cleanup: no_cleanup,
                slots: vec![Ty::Raw, row_ty],
                slot_align: vec![None, None],
                value_tys: decode_values,
                blocks: vec![Block {
                    id: 0,
                    stmts: decode_stmts,
                    stmt_lines: Vec::new(),
                    term: if materialized_supported {
                        Term::Return(Some(Operand::Value(result_value)))
                    } else {
                        Term::Unreachable
                    },
                }],
                entry: 0,
                exportable: false,
            });
            generated_functions.push(Function {
                name: stream_decode_name.clone(),
                params: vec![0, 1],
                param_modes: vec![ParamMode::ByValue, ParamMode::ByValue],
                borrow_mut_cleanup_slots: vec![None, None],
                ret: row_ty,
                return_borrow: stream_return_borrow.clone(),
                return_region: stream_return_region.clone(),
                return_cleanup: no_cleanup,
                slots: vec![Ty::Raw, Ty::ResourceRef(rows_resource), row_ty],
                slot_align: vec![None, None, None],
                value_tys: stream_decode_values,
                blocks: vec![Block {
                    id: 0,
                    stmts: stream_decode_stmts,
                    stmt_lines: Vec::new(),
                    term: if row_supported {
                        Term::Return(Some(Operand::Value(stream_result_value)))
                    } else {
                        Term::Unreachable
                    },
                }],
                entry: 0,
                exportable: false,
            });

            let batch_names = batch_resource_names_for_descriptor(row_definition);
            let batch_resource = if let Some(index) = mir
                .resources
                .iter()
                .position(|resource| resource.name == batch_names[0])
                .or_else(|| {
                    mir.resources
                        .iter()
                        .position(|resource| resource.name == batch_names[1])
                })
            {
                index as u32
            } else {
                let mut resource = mir
                    .resources
                    .iter()
                    .find(|resource| resource.name == "pkg.db$batch")
                    .cloned()
                    .unwrap_or_else(|| hir::ResourceDef {
                        name: "pkg.db$batch".to_string(),
                        source_name: "pkg.db$batch".to_string(),
                        declaring_module: "pkg.db".to_string(),
                        generic_arity: 1,
                        drop_hook: "__align_db_batch_owner_drop".to_string(),
                        drop_thunk: "__align_resource_drop$pkg.db$batch".to_string(),
                        representation_version: 1,
                        drop_abi_fingerprint: *b"align-res-drop-1",
                    });
                resource.name = batch_names[0].clone();
                resource.source_name = batch_names[0].clone();
                let id = mir.resources.len() as u32;
                mir.resources.push(resource);
                id
            };
            let soa_plain = !row_definition.fields.is_empty()
                && row_definition.fields.iter().all(|field| {
                    matches!(field.ty, Ty::Bool | Ty::Char | Ty::Int(_) | Ty::Float(_) | Ty::Str)
                });
            let batch_create_name = program_call(&format!("{symbol}$batch_create_v1"))?;
            let batch_append_name = program_call(&format!("{symbol}$batch_append_v1"))?;
            let batch_finish_name = program_call(&format!("{symbol}$batch_finish_v1"))?;
            let batch_row_name = program_call(&format!("{symbol}$batch_row_v1"))?;
            let batch_soa_name = program_call(&format!("{symbol}$batch_soa_v1"))?;
            let batch_drop_name = program_call(&format!("{symbol}$batch_drop_v1"))?;
            generated_functions.push(Function {
                name: batch_create_name.clone(),
                params: vec![0],
                param_modes: vec![ParamMode::ByValue],
                borrow_mut_cleanup_slots: vec![None],
                ret: Ty::Raw,
                return_borrow: none_borrow.clone(),
                return_region: none_region.clone(),
                return_cleanup: no_cleanup,
                slots: vec![i64_ty],
                slot_align: vec![None],
                value_tys: vec![Ty::Raw],
                blocks: vec![Block {
                    id: 0,
                    stmts: vec![Stmt::Let(
                        0,
                        Rvalue::ColumnBatchCreate {
                            max_rows: Operand::Arg(0),
                            struct_id: row_struct_id,
                        },
                    )],
                    stmt_lines: Vec::new(),
                    term: Term::Return(Some(Operand::Value(0))),
                }],
                entry: 0,
                exportable: false,
            });
            let mut batch_append_stmts = Vec::new();
            let mut batch_append_values = Vec::new();
            let mut batch_append_inputs = Vec::new();
            for field in &runtime.decoder.fields {
                let base_ty = value_ty(field.shape.kind);
                if matches!(field.shape.kind, GeneratedValueKind::Text | GeneratedValueKind::Bytes)
                {
                    let pointer = batch_append_values.len() as u32;
                    batch_append_values.push(Ty::Raw);
                    batch_append_stmts.push(Stmt::Let(
                        pointer,
                        Rvalue::Call(
                            DirectCall::Program(read_view_pointer_callback.clone()),
                            vec![
                                Operand::Arg(1),
                                Operand::Const(Const::Int(
                                    i128::from(field.row_field_ordinal),
                                    u32_ty,
                                )),
                            ],
                        ),
                    ));
                    let length = batch_append_values.len() as u32;
                    batch_append_values.push(i64_ty);
                    batch_append_stmts.push(Stmt::Let(
                        length,
                        Rvalue::Call(
                            DirectCall::Program(read_view_length_callback.clone()),
                            vec![
                                Operand::Arg(1),
                                Operand::Const(Const::Int(
                                    i128::from(field.row_field_ordinal),
                                    u32_ty,
                                )),
                            ],
                        ),
                    ));
                    batch_append_inputs.push(ColumnBatchInput::View {
                        ptr: Operand::Value(pointer),
                        len: Operand::Value(length),
                    });
                } else {
                    let value = batch_append_values.len() as u32;
                    batch_append_values.push(option_ty(base_ty)?);
                    batch_append_stmts.push(Stmt::Let(
                        value,
                        Rvalue::Call(
                            DirectCall::Program(
                                read_callbacks[value_tag(field.shape.kind) as usize].clone(),
                            ),
                            vec![
                                Operand::Arg(1),
                                Operand::Const(Const::Int(
                                    i128::from(field.row_field_ordinal),
                                    u32_ty,
                                )),
                            ],
                        ),
                    ));
                    batch_append_inputs.push(ColumnBatchInput::Scalar(Operand::Value(value)));
                }
            }
            let batch_append_result = batch_append_values.len() as u32;
            batch_append_values.push(i32_ty);
            batch_append_stmts.push(Stmt::Let(
                batch_append_result,
                Rvalue::ColumnBatchAppend {
                    payload: Operand::Arg(0),
                    inputs: batch_append_inputs,
                    struct_id: row_struct_id,
                },
            ));
            generated_functions.push(Function {
                name: batch_append_name.clone(),
                params: vec![0, 1, 2],
                param_modes: vec![ParamMode::ByValue; 3],
                borrow_mut_cleanup_slots: vec![None; 3],
                ret: i32_ty,
                return_borrow: none_borrow.clone(),
                return_region: none_region.clone(),
                return_cleanup: no_cleanup,
                slots: vec![Ty::Raw, Ty::Raw, Ty::ResourceRef(rows_resource)],
                slot_align: vec![None; 3],
                value_tys: batch_append_values,
                blocks: vec![Block {
                    id: 0,
                    stmts: batch_append_stmts,
                    stmt_lines: Vec::new(),
                    term: Term::Return(Some(Operand::Value(batch_append_result))),
                }],
                entry: 0,
                exportable: false,
            });
            generated_functions.push(Function {
                name: batch_finish_name.clone(),
                params: vec![0, 1],
                param_modes: vec![ParamMode::ByValue; 2],
                borrow_mut_cleanup_slots: vec![None; 2],
                ret: Ty::Unit,
                return_borrow: none_borrow.clone(),
                return_region: none_region.clone(),
                return_cleanup: no_cleanup,
                slots: vec![Ty::Raw, i64_ty],
                slot_align: vec![None; 2],
                value_tys: Vec::new(),
                blocks: vec![Block {
                    id: 0,
                    stmts: vec![Stmt::ColumnBatchFinish {
                        payload: Operand::Arg(0),
                        struct_id: row_struct_id,
                    }],
                    stmt_lines: Vec::new(),
                    term: Term::Return(None),
                }],
                entry: 0,
                exportable: false,
            });
            generated_functions.push(Function {
                name: batch_row_name.clone(),
                params: vec![0, 1, 2],
                param_modes: vec![ParamMode::ByValue; 3],
                borrow_mut_cleanup_slots: vec![None; 3],
                ret: row_ty,
                return_borrow: stream_return_borrow.clone(),
                return_region: stream_return_region.clone(),
                return_cleanup: no_cleanup,
                slots: vec![Ty::Raw, Ty::ResourceRef(batch_resource), i64_ty],
                slot_align: vec![None; 3],
                value_tys: vec![row_ty],
                blocks: vec![Block {
                    id: 0,
                    stmts: vec![Stmt::Let(
                        0,
                        Rvalue::ColumnBatchRow {
                            payload: Operand::Arg(0),
                            owner: Operand::Arg(1),
                            index: Operand::Arg(2),
                            struct_id: row_struct_id,
                            resource: batch_resource,
                        },
                    )],
                    stmt_lines: Vec::new(),
                    term: Term::Return(Some(Operand::Value(0))),
                }],
                entry: 0,
                exportable: false,
            });
            if soa_plain {
                generated_functions.push(Function {
                    name: batch_soa_name.clone(),
                    params: vec![0, 1],
                    param_modes: vec![ParamMode::ByValue; 2],
                    borrow_mut_cleanup_slots: vec![None; 2],
                    ret: Ty::Soa(row_struct_id),
                    return_borrow: hir::ReturnBorrowSummary::Roots {
                        params: vec![1],
                        captures: Vec::new(),
                    },
                    return_region: hir::ReturnRegionSummary::Roots {
                        params: vec![1],
                        captures: Vec::new(),
                    },
                    return_cleanup: no_cleanup,
                    slots: vec![Ty::Raw, Ty::ResourceRef(batch_resource)],
                    slot_align: vec![None; 2],
                    value_tys: vec![Ty::Soa(row_struct_id)],
                    blocks: vec![Block {
                        id: 0,
                        stmts: vec![Stmt::Let(
                            0,
                            Rvalue::ColumnBatchSoa {
                                payload: Operand::Arg(0),
                                owner: Operand::Arg(1),
                                struct_id: row_struct_id,
                                resource: batch_resource,
                            },
                        )],
                        stmt_lines: Vec::new(),
                        term: Term::Return(Some(Operand::Value(0))),
                    }],
                    entry: 0,
                    exportable: false,
                });
            }
            generated_functions.push(Function {
                name: batch_drop_name.clone(),
                params: vec![0],
                param_modes: vec![ParamMode::ByValue],
                borrow_mut_cleanup_slots: vec![None],
                ret: Ty::Unit,
                return_borrow: none_borrow.clone(),
                return_region: none_region.clone(),
                return_cleanup: no_cleanup,
                slots: vec![Ty::Raw],
                slot_align: vec![None],
                value_tys: Vec::new(),
                blocks: vec![Block {
                    id: 0,
                    stmts: vec![Stmt::ColumnBatchDrop {
                        payload: Operand::Arg(0),
                        struct_id: row_struct_id,
                    }],
                    stmt_lines: Vec::new(),
                    term: Term::Return(None),
                }],
                entry: 0,
                exportable: false,
            });
            let mut batch_plan_bytes = vec![0u8; 72];
            batch_plan_bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
            batch_plan_bytes[4] = u8::from(soa_plain);
            batch_plan_bytes[8..12].copy_from_slice(
                &u32::try_from(row_definition.fields.len())
                    .map_err(|_| "generated batch field count exceeds u32::MAX".to_string())?
                    .to_le_bytes(),
            );
            let mut batch_plan_relocations = vec![
                StaticDataRelocation {
                    offset: 16,
                    target: StaticDataTarget::Function(batch_create_name),
                },
                StaticDataRelocation {
                    offset: 24,
                    target: StaticDataTarget::Function(batch_append_name),
                },
                StaticDataRelocation {
                    offset: 32,
                    target: StaticDataTarget::Function(batch_finish_name),
                },
                StaticDataRelocation {
                    offset: 40,
                    target: StaticDataTarget::Function(batch_row_name),
                },
                StaticDataRelocation {
                    offset: 56,
                    target: StaticDataTarget::Function(batch_drop_name),
                },
            ];
            if soa_plain {
                batch_plan_relocations.push(StaticDataRelocation {
                    offset: 48,
                    target: StaticDataTarget::Function(batch_soa_name),
                });
            }
            batch_plan_relocations.sort_by_key(|relocation| relocation.offset);
            relocations.push(StaticDataRelocation {
                offset: 128,
                target: StaticDataTarget::Record(Box::new(StaticData {
                    bytes: batch_plan_bytes,
                    align: 8,
                    relocations: batch_plan_relocations,
                })),
            });
            relocations.push(StaticDataRelocation {
                offset: 80,
                target: StaticDataTarget::Function(row_name),
            });
            relocations.push(StaticDataRelocation {
                offset: 88,
                target: StaticDataTarget::Function(decode_name),
            });
            relocations.push(StaticDataRelocation {
                offset: 120,
                target: StaticDataTarget::Function(stream_decode_name),
            });
        }
        if let Some(query_meta_name) = query_meta_name {
            relocations.push(StaticDataRelocation {
                offset: 96,
                target: StaticDataTarget::Function(query_meta_name),
            });
        }
        relocations.sort_by_key(|relocation| relocation.offset);

        let function = &mut mir.fns[function_index];
        function.params.clear();
        function.param_modes.clear();
        function.borrow_mut_cleanup_slots.clear();
        function.slots = vec![function.ret];
        function.slot_align = vec![None];
        function.value_tys = vec![Ty::Raw, function.ret];
        function.blocks = vec![Block {
            id: 0,
            stmts: vec![
                Stmt::Let(
                    0,
                    Rvalue::StaticData(Box::new(StaticData {
                        bytes: header,
                        align: 8,
                        relocations,
                    })),
                ),
                Stmt::StoreField(0, vec![0], Operand::Value(0)),
                Stmt::Let(1, Rvalue::Load(0)),
            ],
            stmt_lines: Vec::new(),
            term: Term::Return(Some(Operand::Value(1))),
        }];
        function.entry = 0;
    }
    mir.fns.extend(generated_functions);
    // The source constructor is legal only as the descriptor's complete body. Replacing every such
    // body therefore removes the only possible call to its consumer-side generic monomorph. Do not
    // emit a dormant `process.abort` constructor or make a consumer instantiate Query internals.
    mir.fns
        .retain(|function| !static_constructor_monomorph(function.name.as_str()));
    align_mir::recanonicalize_type_tables(mir).map_err(|reason| {
        format!("generated descriptor MIR has an invalid function-type graph: {reason}")
    })?;
    Ok(())
}

/// Lower a checked whole program and install its compiler-owned static descriptor data.
///
/// The normal CLI build uses the per-unit path, which installs descriptors while producing each
/// producer artifact. The whole-program path remains a supported inspection and differential-test
/// surface, so it needs the same post-lowering descriptor replacement before codegen.
pub fn lower_to_mir_with_static_descriptors(
    checked: &Checked,
    source_map: &mut SourceMap,
    project_root: &std::path::Path,
) -> Result<align_mir::Program, String> {
    let mut mir = try_lower_to_mir(&checked.hir)
        .map_err(|rejected| format!("internal error: {rejected}"))?;
    if checked.static_descriptors.is_empty() {
        return Ok(mir);
    }
    let resolution_digest = align_interface::codegen_impl_hash(&mir);
    let resolved = resolve_static_descriptors(
        project_root,
        source_map,
        &checked.static_descriptors,
        resolution_digest,
    )
    .map_err(|error| error.to_string())?;
    let artifacts = build_static_artifacts(&checked.static_descriptors, &resolved)
        .map_err(|error| error.to_string())?;
    install_static_descriptor_data(
        &mut mir,
        Some(checked.entry_unit.as_str()),
        &checked.static_descriptors,
        &artifacts,
    )?;
    Ok(mir)
}

/// lexer -> parser -> sema for the entry file plus its transitively-imported **user** modules
/// (multi-file, slice B1). User modules resolve by filename convention: `import geom` →
/// `<entry-dir>/geom.align`, which must declare `module geom`. Builtin imports (`core.*`/`std.*`)
/// are not files. Diagnostics are collected into `Checked.diags`.
/// A parsed source module (kept alive so `align_sema::Module` borrows into its `ast` are valid).
struct LoadedUnit {
    path: String,
    ast: align_ast::File,
    is_entry: bool,
    /// The module's full source text — retained for the M15 interface summary because generic
    /// template bodies and const values are recorded as source slices.
    src: String,
    /// The module's source file path on disk (the entry file's given `name`, or a resolved
    /// `<dir>/<seg>.align`). Carried so a per-unit consumer (`explain-opt`) can build that unit's own
    /// `DebugInfo` (its basename is what LLVM's remark strings — and thus the report — attribute to).
    file: String,
    /// Lossless filesystem spelling used by static-input resolution. `file` remains the diagnostic
    /// display name because `SourceMap` stores text, while this path preserves arbitrary Unix bytes.
    access_path: std::path::PathBuf,
    /// The unit source's id in the walk's `SourceMap`. The per-unit memo reattaches it to a replayed
    /// diagnostic, whose stored form deliberately drops the id (it is walk-local).
    fid: align_span::FileId,
}

// A user-module import is one whose first segment is neither `core` nor `std` (builtins).
fn user_import(p: &align_ast::Path) -> bool {
    p.segments.first().is_some_and(|s| s.name != "core" && s.name != "std")
}

/// The pkg-foundation import-edge rules (F0 of `impl/15-pkg-web-plan.md`; `open-questions.md`
/// "pkg-foundation" D7/D8). Checked per edge in [`load_units`], where the importer's module path
/// (`importer`) and the imported module's dotted segments (`imported`) are both known. `core`/`std`
/// imports never reach here (filtered by [`user_import`]), so both rules only ever see user modules.
///
/// **D7 — `internal` path rule.** An import whose path contains a segment `internal` is legal only
/// from within the subtree rooted at that `internal` segment's parent: `pkg.router.internal.pool`
/// is importable from `pkg.router` and `pkg.router.*` only. A project-root `internal` (no parent
/// prefix) is visible project-wide. Pure path rule (Go-proven), no package-boundary metadata.
///
/// **D8 — layering.** A module under `pkg/` may import only `core` / `std` / `pkg` modules; it may
/// not reach back into the consuming project's own modules (which would compile in one tree and
/// nowhere else, and inverts the dependency arrow). `core`/`std` are already allowed (filtered out
/// before this runs), so the only rejection here is a `pkg.*` module importing a project module.
fn check_pkg_import_edge(importer: &str, imported: &[&str], span: align_span::Span, diags: &mut Diagnostics) {
    let modpath = imported.join(".");
    // D7 — the `internal` path rule (first `internal` segment governs).
    if let Some(pos) = imported.iter().position(|s| *s == "internal") {
        let prefix = imported[..pos].join(".");
        let importer_ok =
            prefix.is_empty() || importer == prefix || importer.starts_with(&format!("{prefix}."));
        if !importer_ok {
            diags.error(
                format!(
                    "cannot import internal module `{modpath}` from `{importer}` — an `internal` module is importable only from within `{prefix}`"
                ),
                span,
            );
        }
    }
    // D8 — a module under `pkg/` may import only `core` / `std` / `pkg`.
    let importer_is_pkg = importer == "pkg" || importer.starts_with("pkg.");
    if importer_is_pkg && imported.first() != Some(&"pkg") {
        diags.error(
            format!(
                "a module under `pkg/` may import only `core` / `std` / `pkg` modules; `{importer}` cannot import project module `{modpath}` (layering: core -> std -> pkg -> project)"
            ),
            span,
        );
    }
}

/// lexer -> parser for the entry file plus its transitively-imported **user** modules, plus the
/// cyclic-import (DAG) check. The shared front half of [`check`] and [`build_interface_summaries`];
/// behavior-identical to the former inline loader.
fn load_units(
    source_map: &mut SourceMap,
    name: &str,
    src: &str,
    diags: &mut Diagnostics,
    entry_access_path: Option<&std::path::Path>,
) -> Vec<LoadedUnit> {
    let observed = entry_access_path.is_some();
    let entry_path_on_disk = entry_access_path
        .unwrap_or_else(|| std::path::Path::new(name))
        .to_path_buf();
    let entry_dir = entry_path_on_disk.parent().map(|path| path.to_path_buf());

    // The entry module's own name is its `module` decl, or `main` by default.
    let entry_fid = source_map.add_file(name, src);
    let entry_tokens = align_lexer::tokenize(entry_fid, src, diags);
    let entry_ast = align_parser::parse_file(entry_tokens, diags);
    let entry_path = entry_ast
        .module
        .as_ref()
        .and_then(|m| m.segments.last())
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "main".to_string());

    let mut loaded = vec![LoadedUnit {
        path: entry_path.clone(),
        ast: entry_ast,
        is_entry: true,
        src: src.to_string(),
        file: name.to_string(),
        access_path: entry_path_on_disk,
        fid: entry_fid,
    }];
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::from([entry_path.clone()]);

    // Edges of the module import graph (`importer path` -> `(imported modpath, import span)`),
    // collected for every `import` seen below regardless of the `seen` dedup — the dedup exists
    // only to avoid loading a shared module twice (the diamond case: `main` imports `b` and `c`,
    // both import `d`), not to license cycles. [`detect_import_cycles`] walks this graph
    // afterwards to tell that legal reconvergence apart from an actual cycle.
    let mut edges: std::collections::HashMap<String, Vec<(String, align_span::Span)>> =
        std::collections::HashMap::new();

    // Breadth-first over user-module imports, resolving each to `<entry-dir>/<name>.align`.
    let mut i = 0;
    while i < loaded.len() {
        let cur_path = loaded[i].path.clone();
        let imports: Vec<align_ast::Path> =
            loaded[i].ast.imports.iter().filter(|p| user_import(p)).cloned().collect();
        i += 1;
        for imp in imports {
            // The dotted module path (`util.math`) and the matching file path under the entry
            // directory (`util/math.align`): each segment is a directory, the last names the file.
            let segs: Vec<&str> = imp.segments.iter().map(|s| s.name.as_str()).collect();
            let modpath = segs.join(".");
            edges.entry(cur_path.clone()).or_default().push((modpath.clone(), imp.span));
            // pkg-foundation import-edge rules (F0): the `internal` path rule + pkg-layering. Checked
            // per edge (before the `seen` dedup) so an illegal importer is caught even when the target
            // module was already loaded via a legal edge.
            check_pkg_import_edge(&cur_path, &segs, imp.span, diags);
            if !seen.insert(modpath.clone()) {
                continue; // already loaded (shared / cyclic import)
            }
            let Some(dir) = &entry_dir else {
                diags.error(format!("cannot resolve `import {modpath}`: the entry file has no directory"), imp.span);
                continue;
            };
            let mut file_path = dir.clone();
            for seg in &imp.segments {
                file_path.push(&seg.name);
            }
            file_path.set_extension("align");
            let source_name = if observed {
                observed_source_name(&file_path)
            } else {
                file_path.display().to_string()
            };
            let msrc = match watch_inputs::observe_consumed_read(
                &file_path,
                |file| {
                    let mut file = file?;
                    let mut source = String::new();
                    file.read_to_string(&mut source)?;
                    Ok(source)
                },
                |result| result.as_ref().ok().map(String::as_bytes),
                || Err(std::io::Error::other("watch observation rejected path")),
            ) {
                Ok(s) => s,
                Err(e) => {
                    diags.error(
                        format!("cannot find module `{modpath}` (expected {source_name}): {e}"),
                        imp.span,
                    );
                    continue;
                }
            };
            let fid = source_map.add_file(source_name.clone(), msrc.clone());
            let toks = align_lexer::tokenize(fid, &msrc, diags);
            let mast = align_parser::parse_file(toks, diags);
            // The file must declare the full `module util.math` (path ↔ filename agreement).
            let declared = mast.module.as_ref().map(|m| m.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("."));
            if declared.as_deref() != Some(modpath.as_str()) {
                diags.error(
                    format!("module file `{source_name}` must declare `module {modpath}` (found {})",
                        declared.map(|d| format!("`module {d}`")).unwrap_or_else(|| "no module declaration".to_string())),
                    imp.span,
                );
            }
            loaded.push(LoadedUnit {
                path: modpath,
                ast: mast,
                is_entry: false,
                src: msrc,
                file: source_name,
                access_path: file_path,
                fid,
            });
        }
    }

    // The unit import graph must be a DAG (`draft.md` §17, M15 S0): a cycle of `import`s — direct,
    // transitive, or a module importing itself — is a compile error, not something the `seen`
    // dedup above should silently absorb. Sema still runs afterwards (this stage "continues as far
    // as possible on failure, accumulating diagnostics" — `align_diag`'s contract): a cyclic import
    // graph does not itself confuse per-module sema, and running it may surface further, genuinely
    // separate errors.
    detect_import_cycles(&entry_path, &edges, diags);

    loaded
}

/// The whole-program sema step, memoized in-process (`memo.rs`;
/// `docs/impl/10-cache-first-optimization.md` §6.6).
///
/// Sema reads exactly `modules`, which is built one-to-one from `loaded`, and each module's AST is a
/// pure function of its source text — so the ordered `(path, is_entry, src)` list is the complete
/// key. The retained HIR, descriptors, and diagnostics carry `FileId`s, so the memo is used only
/// under the CANONICAL assignment (unit `i` owns file `i`), which `load_units` produces whenever it
/// is handed a fresh `SourceMap`. Under any other assignment the step simply runs.
fn check_program_memoized(
    loaded: &[LoadedUnit],
    modules: &[align_sema::Module],
    diags: &mut Diagnostics,
) -> align_sema::CheckedProgram {
    let canonical = loaded
        .iter()
        .enumerate()
        .all(|(index, unit)| unit.fid as usize == index);
    let key = (canonical && memo::enabled()).then(|| {
        let units: Vec<(&str, bool, &str)> = loaded
            .iter()
            .map(|unit| (unit.path.as_str(), unit.is_entry, unit.src.as_str()))
            .collect();
        memo::program_key(&units, diags)
    });
    if let Some(key) = key
        && let Some(hit) = memo::program_lookup(key)
    {
        for diagnostic in hit.diagnostics {
            diags.push(diagnostic);
        }
        return align_sema::CheckedProgram {
            program: hit.program,
            static_descriptors: hit.static_descriptors,
        };
    }
    // Sema READS the sink it is given: `declaration_has_prior_error` suppresses static-descriptor
    // discovery for a function the loader already reported a lexer/parser error inside. It must
    // therefore see exactly what it saw before this memo existed — the loader's diagnostics. The sink
    // is seeded with them and only the TAIL past `seeded` is this step's own output, so nothing is
    // pushed back to the caller (or retained) twice. `memo::program_key` folds the seeded set for the
    // same reason: it is an input to the step being memoized.
    let seeded = diags.len();
    let mut sink = Diagnostics::new();
    for diagnostic in diags.iter() {
        sink.push(diagnostic.clone());
    }
    let checked = align_sema::check_program_with_static_descriptors(modules, &mut sink);
    let own: Vec<align_diag::Diagnostic> = sink.iter().skip(seeded).cloned().collect();
    // Retain only an error-free result whose every diagnostic points into one of the units the key
    // pins; a span outside that range names a file the replaying run may not have.
    let replayable = !sink.has_errors()
        && own
            .iter()
            .all(|diagnostic| diagnostic.span.is_some_and(|span| (span.file as usize) < loaded.len()));
    if let Some(key) = key
        && replayable
    {
        memo::program_store(
            key,
            memo::CachedProgram {
                program: checked.program.clone(),
                static_descriptors: checked.static_descriptors.clone(),
                diagnostics: own.clone(),
            },
        );
    }
    for diagnostic in own {
        diags.push(diagnostic);
    }
    checked
}

pub fn check(source_map: &mut SourceMap, name: &str, src: &str) -> Checked {
    let mut diags = Diagnostics::new();
    let loaded = load_units(source_map, name, src, &mut diags, None);
    let modules: Vec<align_sema::Module> = loaded
        .iter()
        .map(|l| align_sema::Module { path: l.path.clone(), file: &l.ast, is_entry: l.is_entry, interface_only: false })
        .collect();
    let checked = check_program_memoized(&loaded, &modules, &mut diags);

    Checked {
        hir: checked.program,
        static_descriptors: checked.static_descriptors,
        entry_unit: loaded
            .iter()
            .find(|unit| unit.is_entry)
            .map(|unit| unit.path.clone())
            .unwrap_or_else(|| "main".to_string()),
        diags,
    }
}

/// M15 S1a producer entry point: run the frontend for the entry file + its transitively-imported
/// user modules, and — if it type-checks cleanly — build one [`align_interface::InterfaceSummary`]
/// per unit (with its interface / impl hashes). Additive: it does not touch the build/run path. On
/// any frontend error, returns no summaries plus the diagnostics (a summary of an ill-typed program
/// would be meaningless).
pub fn build_interface_summaries(
    source_map: &mut SourceMap,
    name: &str,
    src: &str,
) -> (Vec<align_interface::InterfaceSummary>, Diagnostics) {
    let mut diags = Diagnostics::new();
    let loaded = load_units(source_map, name, src, &mut diags, None);
    let modules: Vec<align_sema::Module> = loaded
        .iter()
        .map(|l| align_sema::Module { path: l.path.clone(), file: &l.ast, is_entry: l.is_entry, interface_only: false })
        .collect();
    let checked = check_program_memoized(&loaded, &modules, &mut diags);
    if diags.has_errors() {
        return (Vec::new(), diags);
    }
    let hir = checked.program;
    // Summaries and impl hashes derived from the fail-closed empty program would publish wrong
    // cache identity silently, so this consumer reports the shared rejection instead.
    let mir = match try_lower_to_mir(&hir) {
        Ok(mir) => mir,
        Err(rejected) => {
            diags.error(
                vanished_lowering_message(name, rejected),
                align_span::Span::new(0, 0, 0),
            );
            return (Vec::new(), diags);
        }
    };
    let sources: std::collections::HashMap<String, String> = loaded
        .iter()
        .map(|l| (l.path.clone(), l.src.clone()))
        .collect();
    let target = match current_owned_json_target() {
        Ok(target) => target,
        Err(reason) => {
            diags.error(reason, align_span::Span::new(0, 0, 0));
            return (Vec::new(), diags);
        }
    };
    let mut summaries = match align_interface::build_summaries(
        &modules, &hir, &mir, &sources, &target,
    ) {
        Ok(summaries) => summaries,
        Err(reason) => {
            diags.error(
                format!("cannot form owned JSON interface descriptor: {reason}"),
                align_span::Span::new(0, 0, 0),
            );
            return (Vec::new(), diags);
        }
    };
    for summary in &mut summaries {
        match static_interface_hash(
            summary.interface_hash,
            &summary.unit,
            &checked.static_descriptors,
        ) {
            Ok(hash) => summary.interface_hash = hash,
            Err(reason) => diags.error(
                format!("cannot form static descriptor interface identity: {reason}"),
                align_span::Span::new(0, 0, 0),
            ),
        }
    }
    if diags.has_errors() {
        return (Vec::new(), diags);
    }
    (summaries, diags)
}

/// M15 S1b per-unit check result. `check_per_unit` walks the import DAG bottom-up, checking each
/// unit against the already-checked *interface summaries* of its (transitive) imports — never their
/// ASTs — and re-deriving each unit's own summary from that per-unit check.
pub struct PerUnitCheck {
    /// One interface summary per unit that checked cleanly, in bottom-up (dependency-first) order.
    /// A unit whose body fails to check contributes no summary (a summary of an ill-typed unit is
    /// meaningless), so dependents of a broken unit see it as an absent dependency.
    pub summaries: Vec<align_interface::InterfaceSummary>,
    /// For each unit (by module path, bottom-up), the TRANSITIVE set of imported units it depended
    /// on, each paired with that dependency's `interface_hash`. This is the S3 incremental-cache key
    /// input: a unit must be re-checked when any entry here changes. Foreign type references are
    /// by-name in the canonical surface, so the dependency is transitive, not just direct.
    pub dep_interface_hashes: Vec<(String, Vec<(String, align_interface::Hash128)>)>,
    /// The union of every unit's per-unit diagnostics (each unit's diagnostics are emitted once, when
    /// that unit is the unit-under-check; interface-only dependencies emit none).
    pub diags: Diagnostics,
}

struct PendingPerUnitArtifact {
    summary: align_interface::InterfaceSummary,
    body: UnitBody,
    is_entry: bool,
    static_descriptors: Vec<StaticDescriptor>,
    static_inputs: StaticInputManifest,
    static_artifacts: Vec<BuiltStaticArtifact>,
}

type ReadyUnitHook<'a> = dyn FnMut(
    usize,
    &str,
    &[(String, align_interface::Hash128)],
    &mut PendingPerUnitArtifact,
) + 'a;

fn store_ready_unit(
    mirs: &mut std::collections::HashMap<String, PendingPerUnitArtifact>,
    ready_index: &mut usize,
    unit: &str,
    dep_interface_hashes: &[(String, align_interface::Hash128)],
    mut pending: PendingPerUnitArtifact,
    on_ready: &mut Option<&mut ReadyUnitHook<'_>>,
) {
    if let Some(hook) = on_ready.as_deref_mut() {
        hook(*ready_index, unit, dep_interface_hashes, &mut pending);
    }
    *ready_index += 1;
    mirs.insert(unit.to_string(), pending);
}

/// M15 S1b: check every unit **per-unit**, each against only its own AST plus the interface summaries
/// of its (transitively-closed) imports — the literal reading of `draft.md` §17 ("each module is
/// checked against the already-checked interfaces of its imports"). This is an ADDITIVE capability
/// proving the separate-compilation seam; it does not replace the whole-program [`check`] build path
/// (S2 flips codegen). Units are processed bottom-up over the import DAG (S0 guarantees acyclicity);
/// each dependency's public surface is rendered back to source and re-parsed into an interface-only
/// module (one resolution path — the existing sema passes), and cross-unit effect bits are seeded
/// fail-closed.
/// Transitive dependency closure of `start` (excluding `start`), deterministic (import order).
fn transitive(
    start: &str,
    direct: &std::collections::HashMap<String, Vec<String>>,
) -> Vec<String> {
    fn go(
        node: &str,
        direct: &std::collections::HashMap<String, Vec<String>>,
        seen: &mut std::collections::HashSet<String>,
        order: &mut Vec<String>,
    ) {
        if let Some(deps) = direct.get(node) {
            for d in deps {
                if seen.insert(d.clone()) {
                    go(d, direct, seen, order);
                    order.push(d.clone());
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut order = Vec::new();
    go(start, direct, &mut seen, &mut order);
    order
}

/// [`transitive`], memoized per MODULE.
///
/// The shipped walk already computes each module's closure at most twice (once as a unit, once as a
/// render target), so this is not fixing an existing quadratic term — it is preventing the
/// persistent key from introducing one, since the key needs a closure for every closure MEMBER of
/// every unit. Returns an owned list so the memo is not borrowed across the caller's other
/// mutations.
fn closure_of(
    start: &str,
    direct: &std::collections::HashMap<String, Vec<String>>,
    memo: &mut std::collections::HashMap<String, Vec<String>>,
) -> Vec<String> {
    if let Some(hit) = memo.get(start) {
        return hit.clone();
    }
    let computed = transitive(start, direct);
    memo.insert(start.to_string(), computed.clone());
    computed
}

fn walk_per_unit(source_map: &mut SourceMap, name: &str, src: &str, located: bool) -> PerUnitWalk {
    walk_per_unit_at(source_map, name, src, located, None)
}

fn walk_per_unit_at(
    source_map: &mut SourceMap,
    name: &str,
    src: &str,
    located: bool,
    entry_access_path: Option<&std::path::Path>,
) -> PerUnitWalk {
    // `UnitReuse::Forbidden` makes the persistent unit cache inert, so this is byte-for-byte the
    // pre-cache walk: every unit is computed and every body is `Lowered`.
    let walk = walk_inner(
        source_map,
        name,
        src,
        located,
        &CacheContext::Disabled,
        UnitReuse::Forbidden,
        None,
        entry_access_path,
    );
    walk.into_per_unit()
}

/// The one bottom-up per-unit walk, shared by [`build_per_unit`] (reuse forbidden, MIR always
/// lowered here) and [`build_package`] (reuse allowed, a hit yields no MIR).
///
/// `cache` and `reuse` are the ONLY behavioral difference between the two. With
/// `UnitReuse::Forbidden` no unit-cache lookup or publish happens and no key is even built, so the
/// existing entry points keep their exact semantics and cost.
#[allow(clippy::too_many_arguments)]
fn walk_inner(
    source_map: &mut SourceMap,
    name: &str,
    src: &str,
    located: bool,
    cache: &CacheContext,
    reuse: UnitReuse,
    mut on_ready: Option<&mut ReadyUnitHook<'_>>,
    entry_access_path: Option<&std::path::Path>,
) -> PackageWalk {
    use std::collections::HashMap;
    let mut diags = Diagnostics::new();
    let loaded = load_units(source_map, name, src, &mut diags, entry_access_path);
    let lexical_project_root = entry_access_path
        .unwrap_or_else(|| std::path::Path::new(name))
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    // Preserve the entry path's lexical spelling (`/var` versus macOS's canonical `/private/var`,
    // and in-root source symlinks) while making a relative CLI root absolute. Static-input
    // resolution canonicalizes separately for containment checks.
    let project_root = if lexical_project_root.is_absolute() {
        lexical_project_root.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|current| current.join(lexical_project_root))
            .unwrap_or_else(|_| lexical_project_root.to_path_buf())
    };

    let by_path: HashMap<&str, &LoadedUnit> = loaded.iter().map(|l| (l.path.as_str(), l)).collect();
    let defining_paths: HashMap<align_span::FileId, std::path::PathBuf> = loaded
        .iter()
        .map(|unit| (unit.fid, unit.access_path.clone()))
        .collect();
    // Each unit's direct user-module dependencies, in import-declaration order (deterministic).
    let direct_deps: HashMap<String, Vec<String>> = loaded
        .iter()
        .map(|l| {
            let deps: Vec<String> = l
                .ast
                .imports
                .iter()
                .filter(|p| user_import(p))
                .map(|p| p.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("."))
                .filter(|d| by_path.contains_key(d.as_str()))
                .collect();
            (l.path.clone(), deps)
        })
        .collect();

    // Bottom-up (dependency-first) order: DFS post-order from the entry unit. All loaded units are
    // reachable from the entry (they were loaded by following its imports). A `visited` guard makes
    // this terminate even if S0's cycle check already flagged a cycle (a best-effort order then).
    let entry = loaded.iter().find(|l| l.is_entry).map(|l| l.path.clone()).unwrap_or_default();
    let mut order: Vec<String> = Vec::new();
    {
        let mut visited = std::collections::HashSet::new();
        fn post(
            node: &str,
            direct: &HashMap<String, Vec<String>>,
            visited: &mut std::collections::HashSet<String>,
            order: &mut Vec<String>,
        ) {
            if !visited.insert(node.to_string()) {
                return;
            }
            if let Some(deps) = direct.get(node) {
                for d in deps {
                    post(d, direct, visited, order);
                }
            }
            order.push(node.to_string());
        }
        post(&entry, &direct_deps, &mut visited, &mut order);
        // Include any unit not reachable from the entry (defensive; normally none).
        for l in &loaded {
            if !visited.contains(&l.path) {
                post(&l.path, &direct_deps, &mut visited, &mut order);
            }
        }
    }

    let mut summaries: HashMap<String, align_interface::InterfaceSummary> = HashMap::new();
    // Per-unit compilation artifacts keyed by unit path. Populated only for cleanly-checked units;
    // assembled bottom-up into `PerUnitArtifact`s at the end.
    let mut mirs: HashMap<String, PendingPerUnitArtifact> = HashMap::new();
    let mut dep_interface_hashes: Vec<(String, Vec<(String, align_interface::Hash128)>)> = Vec::new();
    // Cache of each dependency's synthesized interface AST, keyed by module path. Rendered and
    // parsed exactly once per dependency (not once per importer): `summary_to_source` is called
    // with the DEP'S OWN transitive closure, never the importer's, so the rendered source (and
    // therefore the parsed AST) is importer-independent and safe to share across every unit that
    // imports it. Without this, the bottom-up walk below would re-render and re-parse every
    // transitive dependency's summary once per importer — O(N^2) in the DAG's fan-in.
    // The rendered source is retained alongside the AST: it is the exact text sema consumes for that
    // dependency, and therefore the per-unit memo's dependency key material (`memo::unit_key`).
    let mut interface_ast_cache: HashMap<String, (String, align_ast::File)> = HashMap::new();
    let mut publication_lock = None;
    let mut publication_lock_attempted = false;
    let mut publication_lock_span = None;
    // The DIGEST path's per-MODULE memos (`docs/impl/10` §6.7 §2.2.4). Never per importer: the
    // shipped walk is already O(N) in renders and closures, and these must not introduce a
    // quadratic term. A summary is inserted into `summaries` once and never changes afterwards, so
    // caching its closure digest by module path is sound.
    let mut closures: HashMap<String, Vec<String>> = HashMap::new();
    let mut closure_digests: HashMap<String, align_interface::Hash128> = HashMap::new();
    // The persistent unit-frontend cache, active only when this is a package build against an
    // enabled cache. `located` opts out entirely: located MIR is a function of the walk's
    // `SourceMap`, which the key does not cover.
    let reuse_root: Option<std::path::PathBuf> = match (reuse, located) {
        (UnitReuse::Allowed, false) => cache.root().map(std::path::Path::to_path_buf),
        _ => None,
    };
    let unit_key_prefix = reuse_root.as_ref().and_then(|_| UnitKeyPrefix::current());
    // Per unit, in walk order: the frontend stage outcome, and the key it was looked up under
    // (retained so a later rehydration failure can unlink exactly that entry).
    let mut frontend_outcomes: HashMap<String, CacheOutcome> = HashMap::new();
    let mut unit_keys: HashMap<String, unit_cache::UnitKey> = HashMap::new();
    let mut replayed_diagnostics: HashMap<String, Vec<unit_cache::CachedDiagnostic>> = HashMap::new();
    let mut ready_index = 0usize;

    for unit_path in &order {
        let Some(u) = by_path.get(unit_path.as_str()).copied() else { continue };
        let tdeps = closure_of(unit_path, &direct_deps, &mut closures);

        // The S3 cache key input: this unit's transitive dependency interface hashes.
        let hset: Vec<(String, align_interface::Hash128)> = tdeps
            .iter()
            .filter_map(|d| summaries.get(d).map(|s| (d.clone(), s.interface_hash)))
            .collect();
        dep_interface_hashes.push((unit_path.clone(), hset));
        let hset = &dep_interface_hashes
            .last()
            .expect("the dependency-hash record was just appended")
            .1;

        // ---- DIGEST path -------------------------------------------------------------------
        // Build this unit's persistent key from digests alone: no interface is rendered and no
        // module is parsed, which is what makes an all-hit build cheap. Every closure member
        // appears exactly once, tagged Missing or Present, so "checked against an absent
        // dependency" stays a distinct input from "checked against a present one".
        let unit_key = unit_key_prefix.as_ref().map(|prefix| {
            let mut deps = Vec::with_capacity(tdeps.len());
            for module in &tdeps {
                let state = match summaries.get(module) {
                    None => unit_cache::DepState::Missing,
                    Some(summary) => {
                        // The dependency's INTERFACE hash, never a digest of its whole summary: the
                        // full encoding carries `impl_hash`, so keying on it would make a private
                        // body edit in a dependency invalidate every consumer.
                        let interface_hash = summary.interface_hash;
                        let closure_digest = match closure_digests.get(module) {
                            Some(digest) => *digest,
                            None => {
                                let closure = closure_of(module, &direct_deps, &mut closures);
                                let digest = unit_cache::closure_digest(&closure);
                                closure_digests.insert(module.clone(), digest);
                                digest
                            }
                        };
                        unit_cache::DepState::Present { interface_hash, closure_digest }
                    }
                };
                deps.push(unit_cache::UnitDep { module: module.clone(), state });
            }
            prefix.key(&u.path, u.is_entry, &u.src, deps)
        });

        // ---- persistent lookup -------------------------------------------------------------
        let mut frontend_reason: Option<FirstDiff> = None;
        if let (Some(_root), Some(key)) = (reuse_root.as_ref(), unit_key.as_ref()) {
            match cache.lookup_unit(key, u.src.len()) {
                unit_cache::UnitLookup::Hit(hit) => {
                    let hit = *hit;
                    // Replay the unit's diagnostics FIRST, in the position a recomputed unit would
                    // occupy, with each span reattached to this walk's own file id for the unit.
                    // Only a descriptor-free unit is ever stored, so nothing else has to be
                    // replayed: the publication lock, static-input resolution, and artifact build
                    // are all no-ops for an empty descriptor set.
                    for diagnostic in &hit.entry.diagnostics {
                        diags.push(align_diag::Diagnostic {
                            severity: diagnostic.severity,
                            message: diagnostic.message.clone(),
                            span: Some(align_span::Span::new(u.fid, diagnostic.lo, diagnostic.hi)),
                        });
                    }
                    // The manifest is a re-encoding of the summary for a descriptor-free unit:
                    // `resolution_digest` is `codegen_impl_hash` of the MIR before installation, and
                    // installation is a no-op with no descriptors, so it equals `impl_hash`.
                    let static_inputs = StaticInputManifest {
                        resolution_digest: hit.summary.impl_hash,
                        inputs: Vec::new(),
                    };
                    replayed_diagnostics.insert(u.path.clone(), hit.entry.diagnostics);
                    frontend_outcomes
                        .insert(u.path.clone(), unit_cache::outcome(&u.path, true, None));
                    unit_keys.insert(u.path.clone(), key.clone());
                    summaries.insert(u.path.clone(), hit.summary.clone());
                    let pending =
                        PendingPerUnitArtifact {
                            summary: hit.summary,
                            body: UnitBody::Reused { link_libs: hit.entry.link_libs },
                            is_entry: u.is_entry,
                            static_descriptors: Vec::new(),
                            static_inputs,
                            static_artifacts: Vec::new(),
                        };
                    store_ready_unit(
                        &mut mirs,
                        &mut ready_index,
                        &u.path,
                        hset,
                        pending,
                        &mut on_ready,
                    );
                    continue;
                }
                unit_cache::UnitLookup::Miss { reason } => frontend_reason = reason,
            }
        }
        if let Some(key) = unit_key.as_ref() {
            frontend_outcomes
                .insert(u.path.clone(), unit_cache::outcome(&u.path, false, frontend_reason));
            unit_keys.insert(u.path.clone(), key.clone());
        }
        // Whether every closure member with a summary rendered. A unit checked without a member
        // that should have been present must not be published: a later hit would skip the render
        // and therefore skip the error the cold path reports.
        let mut all_deps_rendered = true;

        // Reconstruct each transitive dependency as an interface-only module from its summary,
        // reusing (or populating) `interface_ast_cache` so each dependency is rendered and parsed
        // exactly once across the whole bottom-up walk, not once per importer.
        let mut external_effects: HashMap<String, align_sema::FnEffect> = HashMap::new();
        let mut external_return_provenance = align_sema::ExternalReturnProvenance::new();
        let mut external_resources = align_sema::ExternalResourceFacts::new();
        let mut external_resource_hooks = align_sema::ExternalResourceHookFacts::new();
        for d in &tdeps {
            let Some(dep_summary) = summaries.get(d) else { continue };
            if !interface_ast_cache.contains_key(d) {
                // Render using `d`'s OWN transitive closure (never the importer's `tdeps`): that
                // is what makes the rendered source, and therefore the parsed AST, independent of
                // which importer triggered the parse — and so safe to cache and share.
                let d_tdeps = closure_of(d, &direct_deps, &mut closures);
                let d_tdep_refs: Vec<&str> = d_tdeps.iter().map(|s| s.as_str()).collect();
                let source = match align_interface::summary_to_source(dep_summary, &d_tdep_refs) {
                    Ok(source) => source,
                    Err(error) => {
                        let fid =
                            source_map.add_file(format!("<interface:{d}>"), String::new());
                        diags.error(
                            format!("cannot import interface `{d}`: {error}"),
                            align_span::Span::new(fid, 0, 0),
                        );
                        all_deps_rendered = false;
                        continue;
                    }
                };
                // Parse the synthesized surface with the real parser (one resolution path). Synthesized
                // source is compiler-internal and always well-formed; its parse diagnostics are discarded.
                let mut sink = Diagnostics::new();
                let fid = source_map.add_file(format!("<interface:{d}>"), source.clone());
                let toks = align_lexer::tokenize(fid, &source, &mut sink);
                let ast = align_parser::parse_file(toks, &mut sink);
                interface_ast_cache.insert(d.clone(), (source, ast));
            }
            external_effects.extend(align_interface::summary_effects(dep_summary, false));
            external_return_provenance.extend(align_interface::summary_return_provenance(
                dep_summary,
                false,
            ));
            external_resources.extend(align_interface::summary_resource_facts(dep_summary));
            external_resource_hooks.extend(align_interface::summary_resource_hook_facts(
                dep_summary,
                false,
            ));
        }

        // Process-in-memory memoization of this unit's frontend result (`memo.rs`;
        // `docs/impl/10-cache-first-optimization.md` §6.6). The key is the exact sema input: the
        // unit's own path/entry-flag/source plus every interface-only dependency module in the order
        // they are passed below, keyed by the rendered source each was parsed from, plus the four
        // external fact maps seeded above. A hit therefore replays a result that this process
        // already computed from byte-identical inputs.
        let interfaces: Vec<(&str, &str)> = tdeps
            .iter()
            .filter_map(|d| {
                interface_ast_cache
                    .get(d)
                    .map(|(source, _)| (d.as_str(), source.as_str()))
            })
            .collect();
        // `located` lowering resolves every statement's (line, column) through THIS walk's
        // `SourceMap`, so its MIR is a function of the project's file paths and line tables as well
        // as of the sema input keyed here. That mode is the `explain-opt` reporting lens, not a
        // build path, so it opts out entirely rather than growing the key to cover a source map.
        let memo_keyed = (!located && memo::enabled()).then(|| {
            memo::unit_key(
                &u.path,
                u.is_entry,
                &u.src,
                &interfaces,
                memo::ExternalFacts {
                    effects: &external_effects,
                    return_provenance: &external_return_provenance,
                    resources: &external_resources,
                    resource_hooks: &external_resource_hooks,
                },
            )
        });
        if let Some((key, _)) = memo_keyed
            && let Some(hit) = memo::unit_lookup(key)
        {
            // Replay the unit's diagnostics first, in the same position in `diags` a recomputed unit
            // would occupy, with each span reattached to this walk's file id for the unit. Only a
            // unit that owns NO static descriptors is retained, so nothing else has to be replayed:
            // the publication lock, static-input resolution, and artifact build are all no-ops for
            // an empty descriptor set.
            for diagnostic in &hit.diagnostics {
                diags.push(align_diag::Diagnostic {
                    severity: diagnostic.severity,
                    message: diagnostic.message.clone(),
                    span: Some(align_span::Span::new(u.fid, diagnostic.lo, diagnostic.hi)),
                });
            }
            // A memo hit still PUBLISHES when the disk stage missed. The memo answers "this
            // process already computed this unit", which says nothing about whether the persistent
            // store has it: the earlier computation may have happened on a reuse-forbidden walk, or
            // against a different cache root. Without this, a long-lived process would compute a
            // unit once and never persist it. The retained result is sound to publish for exactly
            // the reason the memo could serve it — its key covers the same sema input, and the memo
            // only ever retains a descriptor-free unit whose diagnostics are replayable.
            // A retained memo entry is descriptor-free by construction (`memo::unit_store`'s
            // precondition), but the persistent publish must not depend on a comment for that: the
            // memo carries the fact, and an entry that ever disagreed would be excluded here rather
            // than silently persisted.
            let memo_hit_is_descriptor_free = hit.static_descriptors_were_empty;
            debug_assert!(
                memo_hit_is_descriptor_free,
                "the memo must only ever retain a descriptor-free unit"
            );
            if let (Some(root), Some(key)) = (reuse_root.as_ref(), unit_key.as_ref())
                && all_deps_rendered
                && memo_hit_is_descriptor_free
            {
                unit_cache::publish(
                    root,
                    key,
                    &unit_cache::UnitEntry {
                        summary_bytes: align_interface::serialize(&hit.summary),
                        diagnostics: hit
                            .diagnostics
                            .iter()
                            .map(|diagnostic| unit_cache::CachedDiagnostic {
                                severity: diagnostic.severity,
                                message: diagnostic.message.clone(),
                                lo: diagnostic.lo,
                                hi: diagnostic.hi,
                            })
                            .collect(),
                        link_libs: hit.mir.link_libs.clone(),
                    },
                );
            }
            summaries.insert(u.path.clone(), hit.summary.clone());
            let pending =
                PendingPerUnitArtifact {
                    summary: hit.summary,
                    body: UnitBody::Lowered(hit.mir),
                    is_entry: u.is_entry,
                    static_descriptors: Vec::new(),
                    static_inputs: hit.static_inputs,
                    static_artifacts: Vec::new(),
                };
            store_ready_unit(
                &mut mirs,
                &mut ready_index,
                &u.path,
                hset,
                pending,
                &mut on_ready,
            );
            continue;
        }

        let mut modules: Vec<align_sema::Module> = tdeps
            .iter()
            .filter_map(|d| {
                interface_ast_cache.get(d).map(|(_, ast)| align_sema::Module {
                    path: d.clone(),
                    file: ast,
                    is_entry: false,
                    interface_only: true,
                })
            })
            .collect();
        modules.push(align_sema::Module {
            path: u.path.clone(),
            file: &u.ast,
            is_entry: u.is_entry,
            interface_only: false,
        });

        let mut u_diags = Diagnostics::new();
        let checked = align_sema::check_program_with_all_interface_facts_and_static_descriptors(
            &modules,
            &external_effects,
            &external_return_provenance,
            &external_resources,
            &external_resource_hooks,
            &mut u_diags,
        );
        let program = checked.program;
        let static_descriptors = checked.static_descriptors;
        let had_errors = u_diags.has_errors();
        // The unit's diagnostics in replayable form, or `None` when one of them cannot be
        // reattached in another walk (see `memo::unit_diagnostics`). A clean unit routinely warns —
        // `pkg.db` emits sixteen `lossy conversion` warnings — so warnings are replayed rather than
        // used to disqualify the unit, which would exclude exactly the expensive modules.
        let replayable_diags = unit_cache::replayable_diagnostics(&u_diags, u.fid);
        for d in u_diags.iter() {
            diags.push(d.clone());
        }

        if !had_errors {
            // Re-derive THIS unit's own summary from its per-unit check (bottom-up: dependencies are
            // already summarized). Only the unit's real module is passed, so exactly one summary is
            // built. Its `interface_hash` folds cross-unit effect bits (`external_effects`).
            let unit_module = [align_sema::Module {
                path: u.path.clone(),
                file: &u.ast,
                is_entry: u.is_entry,
                interface_only: false,
            }];
            // Per-unit MIR (S2): the unit's own fns + in-consumer monomorphs, with the
            // separate-compilation visibility bits set (`pub` fns external, public declarations from
            // interface-only dependencies carried as external declares). The per-unit `impl_hash`
            // below fingerprints this exact codegen input, including its type tables, declarations,
            // linkage, and alignment. The legacy
            // whole-program summary producer still partitions function MIR for its multi-unit
            // inspection surface; only per-unit summaries feed the object cache.
            // MIR lowering fails closed on validator-rejected HIR; for a CHECKED unit that still
            // has functions this is a compiler defect, and shipping it silently produced an empty
            // object (`_main` undefined at link) with exit 0. Surface the shared rejection as a
            // loud internal error at the one walk every CLI verb shares.
            let lowered = if located {
                try_lower_to_mir_per_unit_located(&program, source_map)
            } else {
                try_lower_to_mir_per_unit(&program)
            };
            let mut mir = match lowered {
                Ok(mir) => mir,
                Err(rejected) => {
                    diags.error(
                        vanished_lowering_message(&u.path, rejected),
                        align_span::Span::new(0, 0, 0),
                    );
                    continue;
                }
            };
            let sources: HashMap<String, String> = HashMap::from([(u.path.clone(), u.src.clone())]);
            let target = match current_owned_json_target() {
                Ok(target) => target,
                Err(reason) => {
                    diags.error(reason, align_span::Span::new(0, 0, 0));
                    continue;
                }
            };
            let mut built = match align_interface::build_summaries_with_effects(
                &unit_module,
                &program,
                &mir,
                &sources,
                &external_effects,
                &target,
            ) {
                Ok(built) => built,
                Err(reason) => {
                    diags.error(
                        format!("cannot form owned JSON interface descriptor: {reason}"),
                        align_span::Span::new(0, 0, 0),
                    );
                    continue;
                }
            };
            if let Some(mut s) = built.pop() {
                // Static-input resolution is keyed by the producer before its descriptor bodies are
                // replaced. The final implementation hash below fingerprints the exact generated
                // MIR that codegen receives.
                let resolution_digest = align_interface::codegen_impl_hash(&mir);
                match static_interface_hash(s.interface_hash, &u.path, &static_descriptors) {
                    Ok(hash) => s.interface_hash = hash,
                    Err(reason) => {
                        diags.error(
                            format!("cannot form static descriptor interface identity: {reason}"),
                            static_descriptors.first().map_or_else(
                                || align_span::Span::new(0, 0, 0),
                                |descriptor| descriptor.constructor_span,
                            ),
                        );
                        continue;
                    }
                }
                if !static_descriptors.is_empty() && !publication_lock_attempted {
                    publication_lock_attempted = true;
                    publication_lock_span = static_descriptors
                        .first()
                        .map(|descriptor| descriptor.constructor_span);
                    match lock_metadata_publication_shared(&project_root) {
                        Ok(lock) => publication_lock = Some(lock),
                        Err(error) => {
                            let message = if entry_access_path.is_some() {
                                error.watch_message()
                            } else {
                                error.to_string()
                            };
                            diags.error(
                                message,
                                publication_lock_span
                                    .unwrap_or_else(|| align_span::Span::new(0, 0, 0)),
                            );
                            continue;
                        }
                    }
                }
                let resolved = match static_inputs::resolve_static_descriptors_at(
                    &project_root,
                    source_map,
                    &static_descriptors,
                    resolution_digest,
                    &defining_paths,
                ) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        let message = if entry_access_path.is_some() {
                            error.watch_message()
                        } else {
                            error.to_string()
                        };
                        diags.error(message, error.span);
                        continue;
                    }
                };
                let static_artifacts = match build_static_artifacts(&static_descriptors, &resolved)
                {
                    Ok(artifacts) => artifacts,
                    Err(error) => {
                        let span = static_descriptors
                            .iter()
                            .find(|descriptor| descriptor.descriptor_id == error.descriptor_id)
                            .map_or_else(
                                || align_span::Span::new(0, 0, 0),
                                |descriptor| descriptor.constructor_span,
                            );
                        diags.error(error.to_string(), span);
                        continue;
                    }
                };
                if let Err(reason) = install_static_descriptor_data(
                    &mut mir,
                    u.is_entry.then_some(u.path.as_str()),
                    &static_descriptors,
                    &static_artifacts,
                ) {
                    diags.error(
                        format!("cannot generate static descriptor runtime data: {reason}"),
                        static_descriptors.first().map_or_else(
                            || align_span::Span::new(0, 0, 0),
                            |descriptor| descriptor.constructor_span,
                        ),
                    );
                    continue;
                }
                // Cache soundness boundary: hash the exact structural MIR program that a miss hands
                // to codegen, including generated descriptor bodies and producer-owned data.
                s.impl_hash = align_interface::codegen_impl_hash(&mir);
                if !static_descriptors.is_empty() {
                    match static_implementation_hash(
                        s.impl_hash,
                        &resolved.manifest,
                        &static_artifacts,
                    ) {
                        Ok(hash) => s.impl_hash = hash,
                        Err(reason) => {
                            diags.error(
                                format!("cannot form static descriptor implementation identity: {reason}"),
                                static_descriptors
                                    .first()
                                    .map_or_else(|| align_span::Span::new(0, 0, 0), |descriptor| descriptor.constructor_span),
                            );
                            continue;
                        }
                    }
                }
                // Retain the frontend result for the rest of the process. A descriptor-owning unit is
                // excluded: its result depends on files under the project root and on a publication
                // lock, neither of which a replay re-establishes.
                if let Some((key, material_len)) = memo_keyed
                    && let Some(diagnostics) = replayable_diags.clone()
                    && static_descriptors.is_empty()
                {
                    memo::unit_store(
                        key,
                        memo::CachedUnit {
                            summary: s.clone(),
                            mir: mir.clone(),
                            static_inputs: resolved.manifest.clone(),
                            diagnostics,
                            static_descriptors_were_empty: true,
                        },
                        material_len,
                    );
                }
                // Persist the same result for the NEXT process. The exclusions mirror the memo's,
                // for the same reasons, plus the render-completeness rule above.
                if let (Some(root), Some(key)) = (reuse_root.as_ref(), unit_key.as_ref())
                    && static_descriptors.is_empty()
                    && all_deps_rendered
                    && let Some(diagnostics) = replayable_diags
                {
                    unit_cache::publish(
                        root,
                        key,
                        &unit_cache::UnitEntry {
                            summary_bytes: align_interface::serialize(&s),
                            diagnostics,
                            link_libs: mir.link_libs.clone(),
                        },
                    );
                }
                summaries.insert(u.path.clone(), s.clone());
                let pending =
                    PendingPerUnitArtifact {
                        summary: s,
                        body: UnitBody::Lowered(mir),
                        is_entry: u.is_entry,
                        static_descriptors,
                        static_inputs: resolved.manifest,
                        static_artifacts,
                    };
                store_ready_unit(
                    &mut mirs,
                    &mut ready_index,
                    &u.path,
                    hset,
                    pending,
                    &mut on_ready,
                );
            }
        }
    }

    if let Some(lock) = &publication_lock
        && let Err(error) = lock.validate()
    {
        diags.error(
            error.to_string(),
            publication_lock_span.unwrap_or_else(|| align_span::Span::new(0, 0, 0)),
        );
    }

    // Assemble one artifact per cleanly-checked unit, in bottom-up (dependency-first) order. A unit
    // that failed to check contributes none (its errors are in `diags`). `dep_interface_hashes` stays
    // the FULL per-order list (an entry for every unit, clean or not) — that is the S1b dev-verb
    // contract `check_per_unit` returns unchanged; each clean unit's artifact carries its own copy.
    let dep_hashes_by_unit: HashMap<&str, &Vec<(String, align_interface::Hash128)>> =
        dep_interface_hashes.iter().map(|(u, h)| (u.as_str(), h)).collect();
    let units: Vec<WalkUnit> = order
        .iter()
        .filter_map(|p| {
            let PendingPerUnitArtifact {
                summary,
                body,
                is_entry,
                static_descriptors,
                static_inputs,
                static_artifacts,
            } = mirs.remove(p)?;
            Some(WalkUnit {
                unit: p.clone(),
                is_entry,
                body,
                summary,
                dep_interface_hashes: dep_hashes_by_unit
                    .get(p.as_str())
                    .unwrap_or_else(|| panic!("missing dependency hashes for unit '{p}' — walk order must produce deps first"))
                    .to_vec(),
                file: by_path.get(p.as_str()).map(|u| u.file.clone()).unwrap_or_default(),
                static_descriptors,
                static_inputs,
                static_artifacts,
                frontend: frontend_outcomes.remove(p),
            })
        })
        .collect();

    // Everything a later single-unit rehydration needs, and nothing else. `scratch` is seeded with
    // the loaded units in FILE-ID order so ids `0..N` mean the same file there as in the caller's
    // map; only `<interface:…>` pseudo-files (ids >= N) can ever diverge, and nothing observable
    // references those. See `docs/impl/10-cache-first-optimization.md` §6.7 F-1..F-4.
    let mut scratch = SourceMap::new();
    let mut by_fid: Vec<&LoadedUnit> = loaded.iter().collect();
    by_fid.sort_by_key(|unit| unit.fid);
    for unit in &by_fid {
        let scratch_id = scratch.add_file(unit.file.clone(), unit.src.clone());
        // F-2: seeding must reproduce the caller's id assignment exactly, or a pseudo-file could
        // land on a real unit's id and the own-file diagnostic filter would accept it.
        debug_assert_eq!(scratch_id, unit.fid, "the rehydration map must mirror caller-space ids");
    }
    let rehydrate = RehydrateCtx {
        summaries,
        closures,
        rendered: interface_ast_cache,
        scratch,
        replayed: replayed_diagnostics,
        keys: unit_keys,
        root: reuse_root,
        located,
        loaded,
    };
    PackageWalk { units, dep_interface_hashes, diags, rehydrate }
}

/// One unit's per-unit compilation artifact (M15 S2): its own MIR (own fns + in-consumer monomorphs +
/// external declares for non-generic `pub` functions from interface-only dependencies), its
/// interface summary, and its transitive dependency interface-hash set (the S3 cache-key input).
/// Produced bottom-up by [`build_per_unit`].
pub struct PerUnitArtifact {
    pub unit: String,
    pub is_entry: bool,
    pub mir: MirProgram,
    pub summary: align_interface::InterfaceSummary,
    pub dep_interface_hashes: Vec<(String, align_interface::Hash128)>,
    /// The unit's source file path on disk — its basename is what `explain-opt`'s per-unit
    /// `DebugInfo` names, so LLVM's remarks attribute to the right file in the aggregated report.
    pub file: String,
    /// L5c descriptors owned by this real producer unit. Interface-only dependency bodies are not
    /// rediscovered in consumers.
    pub static_descriptors: Vec<StaticDescriptor>,
    /// Canonical static-source and checked-metadata dependency identity for descriptors owned by
    /// this producer. Empty for ordinary units.
    pub static_inputs: StaticInputManifest,
    /// Validated Query/command artifacts owned by this producer, sorted by descriptor identity.
    pub static_artifacts: Vec<BuiltStaticArtifact>,
}

/// The per-unit compilation result: one artifact per cleanly-checked unit (bottom-up), the FULL
/// per-order transitive dependency-hash list (an entry for every unit, whether or not it checked
/// cleanly — the S1b `check_per_unit` contract), and the union of all per-unit diagnostics. See
/// [`build_per_unit`].
pub struct PerUnitWalk {
    pub units: Vec<PerUnitArtifact>,
    pub dep_interface_hashes: Vec<(String, Vec<(String, align_interface::Hash128)>)>,
    pub diags: Diagnostics,
}

/// Whether a walk may serve a unit from the persistent unit-frontend cache.
///
/// `Forbidden` reproduces the pre-cache behavior exactly — no lookup, no publish, no key built — so
/// every existing entry point keeps its semantics and its cost. Only `build`/`run`/`size` opt in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum UnitReuse {
    Forbidden,
    Allowed,
}

/// A unit's body: lowered by this walk, or reused from the persistent cache.
///
/// Private: `PerUnitArtifact` keeps its `pub mir: MirProgram` field unchanged, so this type is only
/// ever observed through [`BuiltUnit`], which no pre-existing caller constructs or matches.
#[allow(clippy::large_enum_variant)]
enum UnitBody {
    /// Boxing the MIR would move a multi-megabyte program behind a pointer for the sole benefit of
    /// shrinking a `Reused` variant that only ever exists one-per-unit; the size difference is not
    /// worth an extra indirection on every codegen read.
    Lowered(MirProgram),
    /// Served from the cache. The MIR is absent; the link libraries are not, because the link never
    /// needs MIR and they cannot be re-derived from the summary's capability set.
    Reused { link_libs: Vec<String> },
    /// The pipelined package driver moved this unit's MIR into an owned codegen task. The link
    /// libraries remain because final link order is derived from the DAG record, never from worker
    /// completion order. This private state never escapes through `build_package`.
    Consumed { link_libs: Vec<String> },
}

impl UnitBody {
    fn link_libs(&self) -> &[String] {
        match self {
            UnitBody::Lowered(mir) => &mir.link_libs,
            UnitBody::Reused { link_libs } | UnitBody::Consumed { link_libs } => link_libs,
        }
    }

    fn take_lowered(&mut self) -> Option<MirProgram> {
        let link_libs = match self {
            UnitBody::Lowered(mir) => mir.link_libs.clone(),
            UnitBody::Reused { .. } | UnitBody::Consumed { .. } => return None,
        };
        match std::mem::replace(self, UnitBody::Consumed { link_libs }) {
            UnitBody::Lowered(mir) => Some(mir),
            UnitBody::Reused { .. } | UnitBody::Consumed { .. } => None,
        }
    }
}

/// One unit as the shared walk produces it, before it is projected into either public shape.
struct WalkUnit {
    unit: String,
    is_entry: bool,
    body: UnitBody,
    summary: align_interface::InterfaceSummary,
    dep_interface_hashes: Vec<(String, align_interface::Hash128)>,
    file: String,
    static_descriptors: Vec<StaticDescriptor>,
    static_inputs: StaticInputManifest,
    static_artifacts: Vec<BuiltStaticArtifact>,
    frontend: Option<CacheOutcome>,
}

/// The shared walk's raw result.
struct PackageWalk {
    units: Vec<WalkUnit>,
    dep_interface_hashes: Vec<(String, Vec<(String, align_interface::Hash128)>)>,
    diags: Diagnostics,
    rehydrate: RehydrateCtx,
}

impl PackageWalk {
    /// Project into the unchanged [`PerUnitWalk`]. Only reachable from a `UnitReuse::Forbidden`
    /// walk, where every body is `Lowered` by construction; a `Reused` body would mean the reuse
    /// policy leaked, so it is dropped with a loud internal error rather than faked into an empty
    /// program (which would link as an undefined `_main`).
    fn into_per_unit(self) -> PerUnitWalk {
        let PackageWalk { units, dep_interface_hashes, mut diags, .. } = self;
        let units = units
            .into_iter()
            .filter_map(|unit| match unit.body {
                UnitBody::Lowered(mir) => Some(PerUnitArtifact {
                    unit: unit.unit,
                    is_entry: unit.is_entry,
                    mir,
                    summary: unit.summary,
                    dep_interface_hashes: unit.dep_interface_hashes,
                    file: unit.file,
                    static_descriptors: unit.static_descriptors,
                    static_inputs: unit.static_inputs,
                    static_artifacts: unit.static_artifacts,
                }),
                UnitBody::Reused { .. } => {
                    diags.error(
                        format!(
                            "internal error: unit `{}` was served from the persistent cache on a \
                             walk that forbids reuse — report this",
                            unit.unit
                        ),
                        align_span::Span::new(0, 0, 0),
                    );
                    None
                }
                UnitBody::Consumed { .. } => {
                    diags.error(
                        format!(
                            "internal error: unit `{}` was consumed by package codegen on a walk \
                             that requires MIR — report this",
                            unit.unit
                        ),
                        align_span::Span::new(0, 0, 0),
                    );
                    None
                }
            })
            .collect();
        PerUnitWalk { units, dep_interface_hashes, diags }
    }

    fn into_package(self) -> PackageBuild {
        let PackageWalk { units, diags, rehydrate, .. } = self;
        let units = units
            .into_iter()
            .map(|unit| BuiltUnit {
                link_libs: unit.body.link_libs().to_vec(),
                unit: unit.unit,
                is_entry: unit.is_entry,
                summary: unit.summary,
                dep_interface_hashes: unit.dep_interface_hashes,
                static_inputs: unit.static_inputs,
                frontend: unit.frontend,
                body: unit.body,
            })
            .collect();
        PackageBuild { units, diags, rehydrate }
    }
}

/// One unit of a package build (`build`/`run`/`size`). Additive: nothing that existed before this
/// type constructs or matches it, which is what keeps `PerUnitArtifact`'s shape untouched.
pub struct BuiltUnit {
    pub unit: String,
    pub is_entry: bool,
    pub summary: align_interface::InterfaceSummary,
    pub dep_interface_hashes: Vec<(String, align_interface::Hash128)>,
    /// The unit's capability/FFI libraries, in MIR order. Present for both body shapes.
    pub link_libs: Vec<String>,
    /// The unit's static-input manifest — computed for a lowered unit, reconstructed from the
    /// summary for a reused (therefore descriptor-free) one.
    pub static_inputs: StaticInputManifest,
    /// This unit's frontend-stage cache outcome, or `None` when the stage was declined outright
    /// (cache disabled, reuse forbidden, or a located walk). A declined stage performs no lookup and
    /// is counted in neither hits nor misses, matching the memo's accounting rule.
    pub frontend: Option<CacheOutcome>,
    body: UnitBody,
}

impl BuiltUnit {
    /// Whether this unit came from the persistent cache and has not been rehydrated yet.
    pub fn is_reused(&self) -> bool {
        matches!(self.body, UnitBody::Reused { .. })
    }

    /// The unit's MIR if it is already materialized.
    pub fn mir(&self) -> Option<&MirProgram> {
        match &self.body {
            UnitBody::Lowered(mir) => Some(mir),
            UnitBody::Reused { .. } | UnitBody::Consumed { .. } => None,
        }
    }
}

/// Why a single-unit rehydration was rejected. Every variant means the persistent entry disagreed
/// with what recomputing the unit produced, i.e. the key was incomplete — a compiler defect. The
/// caller unlinks the entry and rebuilds the package once with reuse forbidden.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum RehydrateFailure {
    /// Recomputation reported errors, though the entry could only have been published by a clean
    /// check.
    Errors,
    /// `codegen_impl_hash` of the recomputed MIR differs from the entry's `impl_hash` — the exact
    /// component the codegen key is built from.
    ImplHash,
    /// The recomputed summary does not re-encode to the stored bytes.
    Summary,
    /// The recomputed MIR's link libraries differ from the stored ones.
    LinkLibs,
    /// The recomputed diagnostics differ from the replayed ones, or are not replayable at all
    /// (`None` from the filter is a mismatch, never "no diagnostics").
    Diagnostics,
    /// Recomputation discovered static descriptors, which a published unit never has. Defensive:
    /// discovery is a pure function of the AST and the seeded sink, both keyed.
    Descriptors,
}

impl std::fmt::Display for RehydrateFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self {
            RehydrateFailure::Errors => "recomputation reported errors",
            RehydrateFailure::ImplHash => "implementation hash",
            RehydrateFailure::Summary => "interface summary",
            RehydrateFailure::LinkLibs => "link libraries",
            RehydrateFailure::Diagnostics => "diagnostics",
            RehydrateFailure::Descriptors => "static descriptors",
        };
        write!(f, "cached unit disagreed with recomputation ({what})")
    }
}

/// Everything one unit needs to be re-checked later, and nothing else.
///
/// **`FileId` space (`docs/impl/10-cache-first-optimization.md` §6.7 F-1..F-4).** `load_units`
/// tokenizes every unit against the CALLER's `SourceMap` and registers all `N` of them before
/// anything else, so a retained AST's spans are caller-space and real units occupy exactly `0..N`
/// while every `<interface:…>` pseudo-file lands at `>= N`. `scratch` is therefore SEEDED with the
/// same `N` files in file-id order rather than started empty: a fresh map would hand the first
/// pseudo-file id `0`, which caller-side denotes unit 0, and the own-file diagnostic filter would
/// then accept a pseudo-file span as the unit's own. Divergence between the two maps is confined to
/// ids `>= N`, which no retained artifact, no published diagnostic, and no MIR references — the
/// last because non-located lowering consumes no `SourceMap` at all, which is exactly why a located
/// walk never reuses.
struct RehydrateCtx {
    loaded: Vec<LoadedUnit>,
    summaries: std::collections::HashMap<String, align_interface::InterfaceSummary>,
    closures: std::collections::HashMap<String, Vec<String>>,
    rendered: std::collections::HashMap<String, (String, align_ast::File)>,
    scratch: SourceMap,
    replayed: std::collections::HashMap<String, Vec<unit_cache::CachedDiagnostic>>,
    keys: std::collections::HashMap<String, unit_cache::UnitKey>,
    root: Option<std::path::PathBuf>,
    located: bool,
}

/// The key components that are identical for every unit of one walk.
struct UnitKeyPrefix {
    compiler_fingerprint: align_interface::Hash128,
    frontend_schema: u32,
    env_toggles: Vec<unit_cache::EnvToggle>,
    target_triple: String,
    object_format: u8,
}

impl UnitKeyPrefix {
    /// `None` when the host has no supported object format, which fails the whole namespace closed
    /// rather than keying entries under a guessed one. (An enabled `CacheContext` already
    /// guarantees the compiler fingerprint is a real identity.)
    fn current() -> Option<UnitKeyPrefix> {
        let object_format = match target_object_format().ok()? {
            ObjectFormat::Elf => 0u8,
            ObjectFormat::MachO => 1u8,
        };
        Some(UnitKeyPrefix {
            compiler_fingerprint: cache::compiler_build_id(),
            frontend_schema: align_interface::FORMAT_VERSION,
            env_toggles: unit_cache::UnitKey::current_env_toggles(),
            target_triple: align_codegen_llvm::default_triple(),
            object_format,
        })
    }

    fn key(
        &self,
        unit: &str,
        is_entry: bool,
        src: &str,
        deps: Vec<unit_cache::UnitDep>,
    ) -> unit_cache::UnitKey {
        unit_cache::UnitKey {
            key_format_version: unit_cache::UNIT_KEY_FORMAT_VERSION,
            compiler_fingerprint: self.compiler_fingerprint,
            frontend_schema: self.frontend_schema,
            env_toggles: self.env_toggles.clone(),
            target_triple: self.target_triple.clone(),
            object_format: self.object_format,
            unit: unit.to_string(),
            is_entry,
            source_digest: align_interface::Hash128::of(src.as_bytes()),
            deps,
        }
    }
}

/// Why [`codegen_package_parallel`] failed.
///
/// Typed rather than a formatted string so the one recoverable case — a cached unit that disagreed
/// with recomputation — is matched on its shape, not on the prefix of a message. A caller that
/// silently stopped recognizing that prefix would lose the retry and surface an internal
/// disagreement as a hard build failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum PackageCodegenError {
    /// A cached unit disagreed with recomputation. Its entry has already been unlinked, so
    /// rebuilding the package once with [`UnitReuse::Forbidden`] both succeeds and leaves the cache
    /// clean. The whole package, not just this unit: its dependents were checked against a summary
    /// that has just been shown untrustworthy.
    StaleCacheEntry { unit: String, failure: RehydrateFailure },
    /// Anything else — a codegen error, a PGO staging error, an LLVM init failure.
    Failed(String),
}

/// One attempt at the ordinary non-ThinLTO package pipeline.
///
/// Failure variants deliberately expose no object path: their private staging directory has
/// already been removed when the value is returned. Diagnostics always belong to the `SourceMap`
/// passed to this attempt.
pub enum PipelinedPackageBuild {
    FrontendFailed {
        diags: Diagnostics,
    },
    CodegenFailed {
        diags: Diagnostics,
        error: PackageCodegenError,
    },
    Complete(PipelinedPackageComplete),
}

/// One producer-observed ordinary package attempt for foreground compilation.
pub enum ObservedBuildAttempt {
    ObservationFailed { error: BuildInputTopologyError },
    SourceFailed { error: BuildSourceError, inputs: BuildInputSet },
    Pipeline { build: PipelinedPackageBuild, inputs: BuildInputSet },
}

/// The producer-observed ThinLTO front half for foreground compilation.
pub enum ObservedPerUnitBuild {
    ObservationFailed { error: BuildInputTopologyError },
    SourceFailed { error: BuildSourceError, inputs: BuildInputSet },
    Walk { walk: PerUnitWalk, inputs: BuildInputSet },
}

/// A complete pipelined package. The private stage owns every object lent by [`PipelinedBuiltUnit`]
/// and keeps it alive until the caller has finished linking.
pub struct PipelinedPackageComplete {
    pub units: Vec<PipelinedBuiltUnit>,
    pub diags: Diagnostics,
    pub codegen: UnitCodegen,
    // Ownership is the read: dropping this field removes the objects after link. It is deliberately
    // never otherwise inspected outside the in-module ownership tests.
    #[allow(dead_code)]
    object_stage: ArtifactStage,
}

/// The link-facing record for one complete unit, in bottom-up DAG order.
pub struct PipelinedBuiltUnit {
    pub unit: String,
    pub link_libs: Vec<String>,
    pub frontend: Option<CacheOutcome>,
    object: std::path::PathBuf,
}

impl PipelinedBuiltUnit {
    pub fn object(&self) -> &std::path::Path {
        &self.object
    }
}

impl std::fmt::Display for PackageCodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageCodegenError::StaleCacheEntry { unit, failure } => {
                write!(f, "cached unit `{unit}`: {failure}")
            }
            PackageCodegenError::Failed(message) => f.write_str(message),
        }
    }
}

/// A package build: the `build`/`run`/`size` result, which may contain units served from the
/// persistent cache and therefore carrying no MIR.
pub struct PackageBuild {
    pub units: Vec<BuiltUnit>,
    pub diags: Diagnostics,
    rehydrate: RehydrateCtx,
}

impl PackageBuild {
    /// Materialize `units[index]`'s MIR, rehydrating and VERIFYING it when the unit was reused.
    ///
    /// A hit hands out no MIR, so this is the one place a cached frontend result is re-derived
    /// rather than merely re-checked. Recomputation runs against exactly the dependency summaries
    /// the walk used, into a THROWAWAY diagnostic sink — the unit's diagnostics were already emitted
    /// when it was resolved, and appending them again would make a hit observable. The result is
    /// then compared against the entry component by component; any disagreement means the key was
    /// incomplete and is reported rather than used.
    pub fn materialize(&mut self, index: usize) -> Result<&MirProgram, RehydrateFailure> {
        let unit_name = match self.units.get(index) {
            // Already lowered — by this walk, or by an earlier `materialize`. Nothing to do.
            Some(BuiltUnit { body: UnitBody::Lowered(_), .. }) => {
                return match &self.units[index].body {
                    UnitBody::Lowered(mir) => Ok(mir),
                    // Cannot occur: the arm above matched `Lowered` on the same index, and nothing
                    // between the two reads mutates `self`. Reported rather than panicked so a
                    // future refactor that breaks the pairing fails the build, not the process.
                    UnitBody::Reused { .. } | UnitBody::Consumed { .. } => {
                        Err(RehydrateFailure::Errors)
                    }
                };
            }
            Some(BuiltUnit { body: UnitBody::Consumed { .. }, .. }) => {
                return Err(RehydrateFailure::Errors);
            }
            Some(unit) => unit.unit.clone(),
            // No such unit: nothing to materialize and nothing to invalidate.
            None => return Err(RehydrateFailure::Errors),
        };
        let recomputed = self.rehydrate.recompute(&unit_name)?;
        let stored = &self.units[index];
        if recomputed.summary.impl_hash != stored.summary.impl_hash {
            return self.reject(index, RehydrateFailure::ImplHash);
        }
        if align_interface::serialize(&recomputed.summary) != align_interface::serialize(&stored.summary) {
            return self.reject(index, RehydrateFailure::Summary);
        }
        if recomputed.mir.link_libs != stored.link_libs {
            return self.reject(index, RehydrateFailure::LinkLibs);
        }
        let replayed = self.rehydrate.replayed.get(&unit_name);
        if Some(&recomputed.diagnostics) != replayed {
            return self.reject(index, RehydrateFailure::Diagnostics);
        }
        // Verified. Promote the complete result into the in-process memo before adopting it: this
        // walk consulted the DISK stage first (the digest path is cheaper than rendering), so
        // nothing has populated the memo for this unit, and without this a `build_per_unit` or
        // `emit-mir` later in the same process would re-check a unit we just proved. `recompute`
        // returns the memo key it built on the render path, so this costs one insertion.
        if let Some((memo_key, material_len)) = recomputed.memo_key {
            memo::unit_store(
                memo_key,
                memo::CachedUnit {
                    summary: recomputed.summary,
                    mir: recomputed.mir.clone(),
                    // Reconstructed, exactly as `BuiltUnit::static_inputs` does: a rehydrated unit
                    // is descriptor-free by construction (`RehydrateFailure::Descriptors` rejects
                    // anything else), so the manifest is a re-encoding of the summary.
                    static_inputs: StaticInputManifest {
                        resolution_digest: recomputed.mir_impl_hash,
                        inputs: Vec::new(),
                    },
                    diagnostics: recomputed.diagnostics,
                    static_descriptors_were_empty: true,
                },
                material_len,
            );
        }
        self.units[index].body = UnitBody::Lowered(recomputed.mir);
        match &self.units[index].body {
            UnitBody::Lowered(mir) => Ok(mir),
            // Cannot occur: just assigned. Same fail-closed reasoning as above.
            UnitBody::Reused { .. } | UnitBody::Consumed { .. } => Err(RehydrateFailure::Errors),
        }
    }

    /// Unlink the offending entry and report. Split out so every rejection path unlinks exactly once.
    fn reject(
        &mut self,
        index: usize,
        failure: RehydrateFailure,
    ) -> Result<&MirProgram, RehydrateFailure> {
        let unit = &self.units[index].unit;
        if let (Some(root), Some(key)) = (self.rehydrate.root.as_ref(), self.rehydrate.keys.get(unit)) {
            unit_cache::reject(root, key);
            // The disagreement proves this key did not cover every semantic input. A packaged
            // action remains immutable, but publishing the recomputed value under the same rejected
            // key would merely replace one omitted-input state with another and authorize a false
            // hit later. The caller's one retry is uncached; later processes may safely repeat the
            // verification miss until the key contract is widened.
        }
        eprintln!("{}", cache::CORRUPT_NOTE);
        Err(failure)
    }
}

/// One recomputation of a reused unit, before verification.
struct Recomputed {
    mir: MirProgram,
    summary: align_interface::InterfaceSummary,
    diagnostics: Vec<unit_cache::CachedDiagnostic>,
    /// `codegen_impl_hash(&mir)`, which for a descriptor-free unit is also the value the
    /// reconstructed static-input manifest carries. Computed once, on the way to `summary`.
    mir_impl_hash: align_interface::Hash128,
    /// The in-process memo key the render path built for this unit, so a VERIFIED result can be
    /// promoted without rebuilding it. `None` when the memo is off.
    memo_key: Option<(align_interface::Hash128, u64)>,
}

impl RehydrateCtx {
    /// Re-run the compute path for ONE unit against exactly the dependency summaries the walk used.
    ///
    /// This mirrors the walk's compute path rather than sharing its body, because the walk's version
    /// is entangled with descriptor resolution, the publication lock, and the caller's `SourceMap` —
    /// none of which a descriptor-free reused unit needs. The risk that the two drift is not carried
    /// by inspection: [`PackageBuild::materialize`] compares the recomputed summary's full canonical
    /// encoding, its `impl_hash`, its link libraries, and its diagnostics against the stored entry,
    /// so any drift fails the build closed instead of miscompiling.
    fn recompute(&mut self, unit_name: &str) -> Result<Recomputed, RehydrateFailure> {
        use std::collections::HashMap;
        debug_assert!(!self.located, "a located walk never reuses, so it never rehydrates");
        let Some(position) = self.loaded.iter().position(|unit| unit.path == unit_name) else {
            return Err(RehydrateFailure::Errors);
        };
        let tdeps = self.closures.get(unit_name).cloned().unwrap_or_default();

        // RENDER path, memoized per module exactly as the walk memoizes it. Pseudo-files go into
        // `scratch`, whose ids `0..N` already mean the same files as the caller's map.
        let mut external_effects: HashMap<String, align_sema::FnEffect> = HashMap::new();
        let mut external_return_provenance = align_sema::ExternalReturnProvenance::new();
        let mut external_resources = align_sema::ExternalResourceFacts::new();
        let mut external_resource_hooks = align_sema::ExternalResourceHookFacts::new();
        for dep in &tdeps {
            let Some(dep_summary) = self.summaries.get(dep).cloned() else { continue };
            if !self.rendered.contains_key(dep) {
                let dep_closure = self.closures.get(dep).cloned().unwrap_or_default();
                let refs: Vec<&str> = dep_closure.iter().map(String::as_str).collect();
                let Ok(source) = align_interface::summary_to_source(&dep_summary, &refs) else {
                    // The walk refuses to publish a unit whose closure failed to render, so an entry
                    // whose render now fails is itself the disagreement.
                    return Err(RehydrateFailure::Summary);
                };
                let mut sink = Diagnostics::new();
                let fid = self.scratch.add_file(format!("<interface:{dep}>"), source.clone());
                let toks = align_lexer::tokenize(fid, &source, &mut sink);
                let ast = align_parser::parse_file(toks, &mut sink);
                self.rendered.insert(dep.clone(), (source, ast));
            }
            external_effects.extend(align_interface::summary_effects(&dep_summary, false));
            external_return_provenance
                .extend(align_interface::summary_return_provenance(&dep_summary, false));
            external_resources.extend(align_interface::summary_resource_facts(&dep_summary));
            external_resource_hooks
                .extend(align_interface::summary_resource_hook_facts(&dep_summary, false));
        }

        let unit = &self.loaded[position];
        // The memo key is keyed on the exact rendered source each dependency was parsed from, in
        // the order they are passed to sema — the same material `walk_inner` builds.
        let interfaces: Vec<(&str, &str)> = tdeps
            .iter()
            .filter_map(|dep| {
                self.rendered.get(dep).map(|(source, _)| (dep.as_str(), source.as_str()))
            })
            .collect();
        let memo_key = memo::enabled().then(|| {
            memo::unit_key(
                &unit.path,
                unit.is_entry,
                &unit.src,
                &interfaces,
                memo::ExternalFacts {
                    effects: &external_effects,
                    return_provenance: &external_return_provenance,
                    resources: &external_resources,
                    resource_hooks: &external_resource_hooks,
                },
            )
        });
        // If this process already computed this unit, take that result instead of re-running sema.
        // It is the same artifact the memo would have served the walk had the walk reached the
        // render path, and it is still put through the full verification below.
        if let Some((key, _)) = memo_key
            && let Some(hit) = memo::unit_lookup(key)
        {
            let mir_impl_hash = align_interface::codegen_impl_hash(&hit.mir);
            return Ok(Recomputed {
                mir: hit.mir,
                summary: hit.summary,
                diagnostics: hit.diagnostics,
                mir_impl_hash,
                memo_key: None, // already retained; re-storing would be a no-op insert
            });
        }
        let mut modules: Vec<align_sema::Module> = tdeps
            .iter()
            .filter_map(|dep| {
                self.rendered.get(dep).map(|(_, ast)| align_sema::Module {
                    path: dep.clone(),
                    file: ast,
                    is_entry: false,
                    interface_only: true,
                })
            })
            .collect();
        modules.push(align_sema::Module {
            path: unit.path.clone(),
            file: &unit.ast,
            is_entry: unit.is_entry,
            interface_only: false,
        });

        // A THROWAWAY sink: this unit's diagnostics were emitted when it was resolved, and appending
        // them again would make a hit observable. They are compared, never re-published.
        let mut u_diags = Diagnostics::new();
        let checked = align_sema::check_program_with_all_interface_facts_and_static_descriptors(
            &modules,
            &external_effects,
            &external_return_provenance,
            &external_resources,
            &external_resource_hooks,
            &mut u_diags,
        );
        if u_diags.has_errors() {
            return Err(RehydrateFailure::Errors);
        }
        if !checked.static_descriptors.is_empty() {
            return Err(RehydrateFailure::Descriptors);
        }
        let program = checked.program;
        let Ok(mir) = try_lower_to_mir_per_unit(&program) else {
            return Err(RehydrateFailure::Errors);
        };
        let unit_module = [align_sema::Module {
            path: unit.path.clone(),
            file: &unit.ast,
            is_entry: unit.is_entry,
            interface_only: false,
        }];
        let sources: HashMap<String, String> =
            HashMap::from([(unit.path.clone(), unit.src.clone())]);
        let target = current_owned_json_target().map_err(|_| RehydrateFailure::Summary)?;
        let mut built = align_interface::build_summaries_with_effects(
            &unit_module,
            &program,
            &mir,
            &sources,
            &external_effects,
            &target,
        )
        .map_err(|_| RehydrateFailure::Summary)?;
        let Some(mut summary) = built.pop() else {
            return Err(RehydrateFailure::Summary);
        };
        // With no descriptors this is the identity; called anyway so the two paths compose the
        // interface hash through the same function.
        match static_interface_hash(summary.interface_hash, &unit.path, &[]) {
            Ok(hash) => summary.interface_hash = hash,
            Err(_) => return Err(RehydrateFailure::Summary),
        }
        let mir_impl_hash = align_interface::codegen_impl_hash(&mir);
        summary.impl_hash = mir_impl_hash;
        // F-3: the filter target is always the CALLER-space id `load_units` assigned. `None` means
        // the recomputation produced a diagnostic that could not have been stored, which is a
        // mismatch, never "no diagnostics".
        let Some(diagnostics) = unit_cache::replayable_diagnostics(&u_diags, unit.fid) else {
            return Err(RehydrateFailure::Diagnostics);
        };
        Ok(Recomputed { mir, summary, diagnostics, mir_impl_hash, memo_key })
    }
}

/// M15 S2 per-unit build (library entry): walk the import DAG bottom-up, check each unit against its
/// imports' interface summaries, and lower each cleanly-checked unit to its OWN MIR under the
/// separate-compilation visibility model. Returns one [`PerUnitArtifact`] per unit (MIR + summary +
/// dependency hashes), ready for per-unit codegen + N-object link. Additive: the whole-program
/// [`check`]/build path is untouched. On any error, the affected unit contributes no artifact and the
/// error is in `diags` (the caller must not link a partial build).
pub fn build_per_unit(source_map: &mut SourceMap, name: &str, src: &str) -> PerUnitWalk {
    walk_per_unit(source_map, name, src, false)
}

fn observed_source(path: &std::path::Path) -> Result<String, BuildSourceError> {
    let bytes = watch_inputs::observe_consumed_read(
        path,
        |file| {
            let mut file = file?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(bytes)
        },
        |result| result.as_ref().ok().map(Vec::as_slice),
        || Err(std::io::Error::other("watch observation rejected path")),
    ).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            BuildSourceError::Missing
        } else if error.kind() == std::io::ErrorKind::InvalidInput
            || std::fs::metadata(path).is_ok_and(|metadata| !metadata.is_file())
        {
            BuildSourceError::NonRegular
        } else {
            BuildSourceError::Io { message: error.to_string() }
        }
    })?;
    String::from_utf8(bytes).map_err(|error| BuildSourceError::InvalidUtf8 {
        offset: u64::try_from(error.utf8_error().valid_up_to()).unwrap_or(u64::MAX),
    })
}

pub(crate) fn observed_source_name(path: &std::path::Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;

        let mut encoded = String::with_capacity(path.as_os_str().as_bytes().len());
        for byte in path.as_os_str().as_bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-') {
                encoded.push(char::from(*byte));
            } else {
                use std::fmt::Write as _;
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
        encoded
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().into_owned()
    }
}

/// Build one ordinary package while recording every compiler-observed input.
#[allow(clippy::too_many_arguments)]
pub fn build_path_pipelined_observed(
    source_map: &mut SourceMap,
    path: &std::path::Path,
    cache: CacheContext,
    reuse: UnitReuse,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
    pgo: &PgoMode,
) -> ObservedBuildAttempt {
    let (result, inputs) = watch_inputs::collect_observations(|| {
        let source = observed_source(path)?;
        let name = observed_source_name(path);
        Ok::<_, BuildSourceError>(build_package_pipelined_at(
            source_map,
            &name,
            &source,
            cache,
            reuse,
            target,
            profile,
            rt_lto,
            jobs,
            pgo,
            Some(path),
        ))
    });
    let inputs = match inputs {
        Ok(inputs) => inputs,
        Err(error) => return ObservedBuildAttempt::ObservationFailed { error },
    };
    match result {
        Ok(build) => ObservedBuildAttempt::Pipeline { build, inputs },
        Err(error) => ObservedBuildAttempt::SourceFailed { error, inputs },
    }
}

/// Run the existing per-unit front half while recording every observed input.
pub fn build_path_per_unit_observed(
    source_map: &mut SourceMap,
    path: &std::path::Path,
) -> ObservedPerUnitBuild {
    let (result, inputs) = watch_inputs::collect_observations(|| {
        let source = observed_source(path)?;
        let name = observed_source_name(path);
        Ok::<_, BuildSourceError>(walk_per_unit_at(
            source_map,
            &name,
            &source,
            false,
            Some(path),
        ))
    });
    let inputs = match inputs {
        Ok(inputs) => inputs,
        Err(error) => return ObservedPerUnitBuild::ObservationFailed { error },
    };
    match result {
        Ok(walk) => ObservedPerUnitBuild::Walk { walk, inputs },
        Err(error) => ObservedPerUnitBuild::SourceFailed { error, inputs },
    }
}

/// The `build`/`run`/`size` package build: the same bottom-up walk, but a unit whose frontend
/// result is already in the persistent cache is served from there and carries no MIR
/// (`docs/impl/10-cache-first-optimization.md` §6.7).
///
/// `UnitReuse::Forbidden` produces exactly what [`build_per_unit`] would, wrapped in the package
/// shape; `UnitReuse::Allowed` additionally consults and populates the unit-frontend namespace of
/// `cache`. A located walk never reuses (its MIR depends on the caller's `SourceMap`, which the key
/// does not cover), and neither does a descriptor-owning unit.
pub fn build_package(
    source_map: &mut SourceMap,
    name: &str,
    src: &str,
    cache: &CacheContext,
    reuse: UnitReuse,
) -> PackageBuild {
    walk_inner(source_map, name, src, false, cache, reuse, None, None).into_package()
}

/// M15 S2b per-unit build with **source locations** — like [`build_per_unit`], but each unit's MIR is
/// lowered with `Block::stmt_lines` populated ([`lower_to_mir_per_unit_located`]). Used by
/// `alignc explain-opt`, which compiles each unit in isolation and captures LLVM's optimization
/// remarks per unit (the remarks need the debug locations to attribute back to user source).
pub fn build_per_unit_located(source_map: &mut SourceMap, name: &str, src: &str) -> PerUnitWalk {
    walk_per_unit(source_map, name, src, true)
}

/// M15 S1b: check every unit **per-unit** (see [`build_per_unit`] for the shared walk). This is the
/// check-only projection: it discards the per-unit MIR and returns just the summaries + dependency
/// hashes + diagnostics (the S1b dev-verb contract).
pub fn check_per_unit(source_map: &mut SourceMap, name: &str, src: &str) -> PerUnitCheck {
    let walk = walk_per_unit(source_map, name, src, false);
    let summaries = walk.units.into_iter().map(|u| u.summary).collect();
    PerUnitCheck { summaries, dep_interface_hashes: walk.dep_interface_hashes, diags: walk.diags }
}

/// Reject a cyclic module import graph (`check`'s `edges` map: importer path -> `(imported
/// modpath, import span)`), M15 S0 — `draft.md` §17 requires the import graph to be a DAG. A
/// standard depth-first white/grey/black walk from `start` (the entry module): white = unvisited,
/// grey = open on the current DFS path, black = fully explored. A White target recurses; a Black
/// target is a **diamond** (already fully explored via an earlier sibling branch — `b` and `c` both
/// importing `d` is legal reconvergence, not a cycle) and is skipped; a Grey target means the edge
/// closes a cycle back to a module still open on the current path, direct (`a` -> `b` -> `a`),
/// transitive, or a self-import (`a` -> `a`) — reported once, at the closing edge's span, and the
/// walk stops (no cascading cyclic-import diagnostics for the same cycle).
fn detect_import_cycles(
    start: &str,
    edges: &std::collections::HashMap<String, Vec<(String, align_span::Span)>>,
    diags: &mut Diagnostics,
) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Color {
        White,
        Grey,
        Black,
    }

    fn visit<'a>(
        node: &'a str,
        edges: &'a std::collections::HashMap<String, Vec<(String, align_span::Span)>>,
        color: &mut std::collections::HashMap<&'a str, Color>,
        path: &mut Vec<&'a str>,
        diags: &mut Diagnostics,
    ) -> bool {
        color.insert(node, Color::Grey);
        path.push(node);
        if let Some(outs) = edges.get(node) {
            for (target, span) in outs {
                let target = target.as_str();
                match color.get(target).copied().unwrap_or(Color::White) {
                    Color::White => {
                        if visit(target, edges, color, path, diags) {
                            return true;
                        }
                    }
                    Color::Grey => {
                        // `target` is still open on the current DFS path: this edge is the back
                        // edge that closes the cycle. Render the path from `target`'s position on
                        // the stack through the current node, then back to `target`.
                        let start_ix = path.iter().position(|&p| p == target).unwrap_or(0);
                        let mut cycle = path[start_ix..].to_vec();
                        cycle.push(target);
                        diags.error(
                            format!(
                                "cyclic import: {} (the module import graph must be a DAG; merge \
                                 the modules or extract the shared part into a module both import)",
                                cycle.join(" -> ")
                            ),
                            *span,
                        );
                        return true;
                    }
                    Color::Black => {} // fully explored on an earlier branch: a diamond, not a cycle
                }
            }
        }
        path.pop();
        color.insert(node, Color::Black);
        false
    }

    let mut color = std::collections::HashMap::new();
    let mut path = Vec::new();
    visit(start, edges, &mut color, &mut path, diags);
}

/// Lower `hir` at the one fallible MIR boundary, reusing the MIR this process already lowered from
/// a byte-identical HIR (`memo.rs`; `docs/impl/10-cache-first-optimization.md` §6.6). Lowering is a
/// pure function of the HIR and the `variant` visibility model, so a hit is the same program.
///
/// A LOCATED request (`source_map` present) deliberately skips the cache: its MIR additionally
/// depends on the `SourceMap` it resolves line/column through, which the key does not cover. It
/// still routes through here so every driver entry point shares one vanished-program rule.
fn lower_memoized(
    hir: &align_sema::Program,
    variant: &str,
    per_unit: bool,
    source_map: Option<&SourceMap>,
) -> Result<align_mir::Program, align_mir::LoweringRejected> {
    // Only the unlocated variants are memoizable (see the note above); a located request skips the
    // cache entirely rather than keying MIR that also depends on the `SourceMap`.
    let keyed = (source_map.is_none() && memo::enabled()).then(|| memo::lowering_key(hir, variant));
    if let Some((key, _)) = keyed
        && let Some(hit) = memo::lowering_lookup(key)
    {
        return Ok(hit);
    }
    let mir = align_mir::lower_program_checked(hir, per_unit, source_map)?;
    if let Some((key, material_len)) = keyed {
        memo::lowering_store(key, &mir, material_len);
    }
    Ok(mir)
}

/// The internal-error text every CLI verb reports when a checked unit produces no MIR.
///
/// This is a compiler defect, not a user error, so it names the shape to report. Formatting it in
/// one place keeps the five former copies of this rule — the CLI walk, the interface-summary
/// producer, the whole-program static-descriptor surface, the unit-cache rehydration path, and
/// database metadata preparation — from drifting into five different messages for one condition.
fn vanished_lowering_message(unit: &str, rejected: align_mir::LoweringRejected) -> String {
    format!("internal error: unit `{unit}` {rejected}")
}

/// Lower the sema-checked HIR down to MIR, reporting a vanished checked program.
///
/// Every production path uses this form, because returning the empty program from checked input is
/// what silently shipped an object with an undefined `_main`. The infallible [`lower_to_mir`] stays
/// available for the inspection surface, which legitimately lowers non-producer HIR.
pub fn try_lower_to_mir(
    hir: &align_sema::Program,
) -> Result<align_mir::Program, align_mir::LoweringRejected> {
    lower_memoized(hir, "whole-program", false, None)
}

/// Lower the sema-checked HIR down to MIR, failing closed to the empty program.
///
/// This infallible form deliberately does NOT report a vanished program. It is the inspection
/// surface, and several owner tests reach it with HIR that no producer emits: HIR from a program
/// that failed checking (`m5`'s scanner-inference cases assert the rejected MIR is empty) and HIR
/// whose analysis facts a test overrode by hand (`owned_tagged_payloads`'s arena provenance case).
/// For those inputs the empty program is the correct answer, so the rule cannot live here — it
/// belongs where the caller has proven the input is producer-checked and error-free, which is
/// every production path, and every one of them uses [`try_lower_to_mir`].
pub fn lower_to_mir(hir: &align_sema::Program) -> align_mir::Program {
    // The reject path re-enters `align_mir::lower_program` rather than fabricating an empty
    // program here: that keeps ONE definition of "what a rejected program lowers to", and it is
    // the cold path, so the repeated validation costs nothing a caller notices.
    try_lower_to_mir(hir).unwrap_or_else(|_| align_mir::lower_program(hir))
}

/// M15 S2 per-unit lowering: lower ONE unit's checked HIR to MIR under the separate-compilation
/// visibility model — a non-entry `pub` function gets `external` linkage, and non-generic `pub`
/// declarations from interface-only dependencies become external declares
/// (`align_mir::lower_program_per_unit`). The whole-program [`lower_to_mir`] keeps every function
/// `internal` and drops declares, so the default object stays byte-identical.
pub fn try_lower_to_mir_per_unit(
    hir: &align_sema::Program,
) -> Result<align_mir::Program, align_mir::LoweringRejected> {
    lower_memoized(hir, "per-unit", true, None)
}

/// [`try_lower_to_mir_per_unit`] with the same fail-closed inspection contract as [`lower_to_mir`].
pub fn lower_to_mir_per_unit(hir: &align_sema::Program) -> align_mir::Program {
    // Cold reject path; see [`lower_to_mir`] for why it re-enters the library entry point.
    try_lower_to_mir_per_unit(hir).unwrap_or_else(|_| align_mir::lower_program_per_unit(hir))
}

/// M15 S2b per-unit lowering **with source locations** — [`lower_to_mir_per_unit`] plus populated
/// `Block::stmt_lines` (`align_mir::lower_program_per_unit_located`). Used by `explain-opt`, which
/// compiles each unit in isolation and needs the debug locations for LLVM's per-unit remarks.
pub fn try_lower_to_mir_per_unit_located(
    hir: &align_sema::Program,
    source_map: &SourceMap,
) -> Result<align_mir::Program, align_mir::LoweringRejected> {
    lower_memoized(hir, "per-unit-located", true, Some(source_map))
}

/// [`try_lower_to_mir_per_unit_located`] with the same fail-closed inspection contract as
/// [`lower_to_mir`].
pub fn lower_to_mir_per_unit_located(
    hir: &align_sema::Program,
    source_map: &SourceMap,
) -> align_mir::Program {
    // Cold reject path; see [`lower_to_mir`] for why it re-enters the library entry point.
    try_lower_to_mir_per_unit_located(hir, source_map)
        .unwrap_or_else(|_| align_mir::lower_program_per_unit_located(hir, source_map))
}


/// Whether the LLVM backend is available (codegen is wired up).
pub fn backend_available() -> bool {
    align_codegen_llvm::is_available()
}

/// Compile `mir` with debug locations, run `-O2`, and return LLVM's raw optimization-remark strings
/// (`"<file>:<line>:<col>: <message>"`). Process-global side effect — see
/// [`align_codegen_llvm::collect_opt_remarks`]. Used only by `explain-opt`.
pub fn collect_opt_remarks(
    mir: &align_mir::Program,
    target: BuildTarget,
    debug: &DebugInfo,
) -> Result<Vec<String>, String> {
    align_codegen_llvm::collect_opt_remarks(mir, &target, debug).map_err(|e| e.to_string())
}

/// Write MIR out to an object file (codegen). `target` selects the CPU baseline (portable default
/// vs. host-`native`); `profile` selects the middle-end pipeline (`default<O0|O2|O3|Os|Oz>`).
/// `exports` are the explicit export roots (`emit-obj --export`, M13 Codex-audit item 1): the
/// program-function names (matched against source-level `Function::name`, validate with
/// [`unknown_exports`] first) that keep `external` linkage instead of the default whole-program
/// `internal`. Empty for every caller except `emit-obj`/`emit-llvm`.
/// The fast-path string-primitive bitcode (`build.rs` → `str_prims.bc`), baked into `alignc`. Passed
/// to codegen as the `--rt-lto` artifact when `rt_lto` is set; parsing/linking it is codegen's job
/// (`link_in_rt_lto`), with a fail-loud fallback to the runtime staticlib on an unparseable artifact.
/// Baking dissolves the staleness question — the same `cargo build` regenerates it (M14 Slice 2).
const RT_LTO_BITCODE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/str_prims.bc"));

/// The baked `--rt-lto` bitcode when `rt_lto` is on, else `None` (the byte-identical flag-off path).
fn rt_lto_bytes(rt_lto: bool) -> Option<&'static [u8]> {
    rt_lto.then_some(RT_LTO_BITCODE)
}

/// The baked `--rt-lto` fast-path string-primitive bitcode (`build.rs` → `str_prims.bc`). Exposed
/// read-only for the M14 Slice-2 artifact gates (the symbol-set pin: `llvm-nm` must show the guarded
/// four as the only defined `align_rt_*` symbols) and any tooling that inspects the artifact.
pub fn rt_lto_bitcode() -> &'static [u8] {
    RT_LTO_BITCODE
}

/// The CLI's runtime-LTO default per optimization profile (settled 2026-08-09; the flip is
/// recorded in `docs/impl/07-roadmap.md` M14 Slice 2 and `docs/open-questions.md`): ON for the
/// optimizing `release`/`fast` profiles — bench/rt_lto measures 2.1x (aarch64) / 2.9x (x86-64) on
/// string-predicate kernels with a non-regressing numeric control and +1-2ms compile — and OFF for
/// `dev` (O0: nothing inlines) and `small`/`tiny` (the size sweeps conflict with fast-path
/// inlining). `--rt-lto` / `--no-rt-lto` override in either direction.
pub fn default_rt_lto(profile: Profile) -> bool {
    matches!(profile, Profile::Release | Profile::Fast)
}

pub fn emit_object_file(mir: &align_mir::Program, obj: &std::path::Path, target: BuildTarget, profile: Profile, exports: &[String], rt_lto: bool) -> Result<(), String> {
    // Process-in-memory memoization (`memo.rs`; `docs/impl/10-cache-first-optimization.md` §6.6).
    // `emit_object` is a pure function of the key material below, so replaying the retained bytes is
    // byte-identical to rerunning codegen. Only the failure MESSAGE of an unwritable output path
    // differs (an I/O error rather than an LLVM target error) — both fail, on the same input.
    let key = memo::enabled().then(|| memo::object_key(mir, &target, profile, exports, rt_lto));
    if let Some(key) = key
        && let Some(bytes) = memo::object_lookup(key)
    {
        return std::fs::write(obj, bytes.as_slice())
            .map_err(|e| format!("cannot write object file '{}': {e}", obj.display()));
    }
    // Retention reads the bytes BACK from the file codegen just wrote rather than having codegen hand
    // them over: `write_to_file` is the one seam every byte-identity gate is baselined on, and the
    // measured saving of switching to an in-memory buffer is one page-cache read (~0.6 MB, well under
    // a millisecond) after a stage that just spent seconds in LLVM. The read is only trustworthy if
    // no other thread wrote this same path meanwhile, which `EmitGuard` establishes.
    let emitting = key.map(|_| memo::begin_emit(obj));
    align_codegen_llvm::emit_object(mir, obj, &target, profile, exports, rt_lto_bytes(rt_lto)).map_err(|e| e.to_string())?;
    // A read-back failure is not a compilation failure: the object is already on disk and the build
    // proceeds exactly as it would without the memo.
    if let Some(key) = key
        && emitting.as_ref().is_some_and(memo::EmitGuard::exclusive)
        && let Ok(bytes) = std::fs::read(obj)
    {
        memo::object_store(key, bytes);
    }
    Ok(())
}

/// Build the S3 codegen cache key (`docs/impl/10-cache-first-optimization.md` §6.2) for one unit. The
/// target-dependent components come from [`align_codegen_llvm::resolve_target_identity`] and the exact
/// LLVM version from [`align_codegen_llvm::llvm_version`] — the SAME resolution codegen uses, so a
/// cache hit implies byte-identical object bytes. `impl_hash` + `dep_interface_hashes` are the unit's
/// `PerUnitArtifact` fields; `exports` is sorted+deduped and `dep_interface_hashes` sorted by unit
/// name so semantically equivalent inputs share a key.
#[allow(clippy::too_many_arguments)]
pub fn build_codegen_key(
    unit: &str,
    impl_hash: Hash128,
    dep_interface_hashes: &[(String, Hash128)],
    target: &BuildTarget,
    profile: Profile,
    exports: &[String],
    rt_lto: bool,
    pgo: cache::PgoKey,
) -> Result<CodegenKey, String> {
    let rt = align_codegen_llvm::resolve_target_identity(target).map_err(|e| e.to_string())?;
    let object_format = match target_object_format()? {
        ObjectFormat::Elf => 0u8,
        ObjectFormat::MachO => 1u8,
    };
    let mut dep_hashes = dep_interface_hashes.to_vec();
    dep_hashes.sort_by(|a, b| a.0.cmp(&b.0));
    let mut exp = exports.to_vec();
    exp.sort();
    exp.dedup();
    let rt_lto_digest = rt_lto.then(|| Hash128::of(rt_lto_bitcode()));
    let llvm_build_id = align_codegen_llvm::loaded_llvm_build_id()
        .ok_or_else(|| "cannot identify loaded LLVM build for codegen cache".to_string())?;
    Ok(CodegenKey {
        cache_format_version: cache::CACHE_KEY_FORMAT_VERSION,
        compiler_build_id: cache::compiler_build_id(),
        frontend_schema: align_interface::FORMAT_VERSION,
        located: false,
        impl_hash,
        dep_interface_hashes: dep_hashes,
        exports: exp,
        target_triple: rt.triple,
        object_format,
        resolved_cpu: rt.cpu,
        resolved_features: rt.features,
        profile_name: profile.name().to_string(),
        pipeline: profile.pipeline().to_string(),
        codegen_opt: profile.codegen_opt_name().to_string(),
        reloc_model: rt.reloc_model.to_string(),
        code_model: rt.code_model.to_string(),
        llvm_version: align_codegen_llvm::llvm_version(),
        llvm_build_id,
        rt_lto,
        rt_lto_digest,
        pgo_mode: pgo,
        unit: unit.to_string(),
    })
}

/// Build the same S3 codegen key while folding a validated L5 static-input manifest into the
/// producer implementation identity. A static-input edit therefore misses only the producer object;
/// callers keep the existing codegen cache and do not create a parallel Query cache.
#[allow(clippy::too_many_arguments)]
pub fn build_codegen_key_with_static_inputs(
    unit: &str,
    impl_hash: Hash128,
    dep_interface_hashes: &[(String, Hash128)],
    static_inputs: &StaticInputManifest,
    target: &BuildTarget,
    profile: Profile,
    exports: &[String],
    rt_lto: bool,
    pgo: cache::PgoKey,
) -> Result<CodegenKey, String> {
    let static_digest = static_inputs.action_key().map_err(|error| error.to_string())?;
    build_codegen_key(
        unit,
        compose_codegen_impl_hash(impl_hash, static_digest),
        dep_interface_hashes,
        target,
        profile,
        exports,
        rt_lto,
        pgo,
    )
}

/// Emit one unit's object **through the codegen cache** (`docs/impl/10-cache-first-optimization.md`).
/// On an enabled hit, the CAS blob is written verbatim to `obj` and no codegen runs; on a miss (or when
/// the cache is disabled) [`emit_object_file`] runs today's codegen verbatim into `obj` and — when
/// enabled — the object bytes are published to the CAS + index. Returns the structured
/// [`CacheOutcome`] (its `hit`/`miss_reason` the observability model the tests assert). When
/// `cache` is [`CacheContext::Disabled`] this is byte-for-byte the pre-S3a behavior.
#[allow(clippy::too_many_arguments)]
pub fn emit_object_cached(
    cache: &CacheContext,
    unit: &str,
    impl_hash: Hash128,
    dep_interface_hashes: &[(String, Hash128)],
    mir: &align_mir::Program,
    obj: &std::path::Path,
    target: BuildTarget,
    profile: Profile,
    exports: &[String],
    rt_lto: bool,
) -> Result<CacheOutcome, String> {
    // When the cache is disabled (the default), skip building the key entirely — the codegen-key
    // inputs (`compiler_build_id`'s one-time `alignc`-binary hash, `resolve_target_identity`,
    // `llvm_version`, `target_object_format`) are pure cache overhead a cache-off build must not pay.
    // This is the byte-identical, no-extra-I/O pre-S3a path (the same disabled miss `codegen` returns).
    if !cache.codegen_is_enabled() {
        emit_object_file(mir, obj, target, profile, exports, rt_lto)?;
        return Ok(cache::CacheOutcome {
            stage: cache::CacheStage::Codegen,
            unit: unit.to_string(),
            hit: false,
            miss_reason: None,
        });
    }
    // The `emit-obj` verb has no `--pgo-*` surface (PGO is `build`/`run`/`size` only), so this path is
    // always `PgoKey::Off` — a non-PGO object, keyed disjoint from any instrumented/use object.
    let key = build_codegen_key(unit, impl_hash, dep_interface_hashes, &target, profile, exports, rt_lto, cache::PgoKey::Off)?;
    cache.codegen(&key, obj, |out| emit_object_file(mir, out, target.clone(), profile, exports, rt_lto))
}

/// The aggregated result of [`codegen_units_parallel`]: the per-unit cache outcomes (DAG-ordered) plus,
/// for an instrument-PGO **use** build, every profile-use staleness warning captured across the units
/// that actually ran (cache MISSES) and the summed profile-match tally. On an all-HIT use build
/// `pgo_warnings` is empty and the tally is `0/0` by construction — no LLVM ran, so no diagnostics or
/// entry counts were produced; the staleness (if any) was already reported the first time each object was
/// built and is intrinsic to the cached bytes. Off/Instrument builds never warn and leave the tally `0/0`.
pub struct UnitCodegen {
    pub outcomes: Vec<CacheOutcome>,
    /// Profile-use warnings, flattened in DAG (unit) index order for a deterministic report.
    pub pgo_warnings: Vec<String>,
    /// `--pgo-use` only: how many of the rebuilt units' OPTIMIZED functions carried a PGO entry count
    /// (matched the profile), out of `pgo_total` defined functions. `pgo_matched == 0 && pgo_total > 0`
    /// is the "profile matched nothing" signal the caller surfaces as a prominent warning (not an error).
    pub pgo_matched: u32,
    pub pgo_total: u32,
}

/// M15 S3b: codegen every unit of a per-unit build into `obj_paths` (parallel over cache MISSES), the
/// `build`/`run`/`size` path. Two phases, per the settled S3 design:
///
/// 1. **Serial** cache lookups (they mutate no shared LLVM state and produce the ordering): for each
///    unit build its key and look it up; a HIT writes the object from the CAS immediately, a MISS is
///    queued for codegen. When the cache is disabled every unit is a miss and NO key work runs.
/// 2. **Parallel** codegen of the misses via `std::thread::scope` — `jobs` worker threads pull the
///    next miss through a shared atomic index; each runs [`emit_object_file`] (a fresh LLVM `Context`
///    per call) into its own `obj_paths[i]`, then publishes it to the CAS IMMEDIATELY. LLVM's native
///    target is initialized ONCE on this (main) thread before the scope, never racily inside a worker.
///
/// **Instrument-PGO (S2):** when `pgo` is active this is the SAME cached, parallel path — the only
/// PGO-specific bits are (a) the [`cache::PgoKey`] key component (built once here from the profile
/// content digest, so instrumented / profile-use / ordinary objects are structurally isolated and
/// never share a CAS blob), and (b) the per-unit emit swaps the stock opt run for the PGO pipeline
/// (`emit_object_pgo`; GEN inserts counters, USE attaches `!prof branch_weights`). A USE run's fail-loud
/// diagnostic handler turns a libLLVM-REJECTED profile (Error severity — e.g. an unsupported version)
/// into a hard error, and its staleness warnings + profile-match tally ride the return; both are
/// aggregated into `UnitCodegen` (see there). A profile that merely fails to MATCH (0% or partial) is
/// surfaced by the caller as a warning, never an error — there is no reliable 0%-match hard-error signal
/// (see the tally note in the body) and, as with clang, a mismatched profile is performance-only. An
/// all-HIT use build runs no LLVM and needs none. The instrumented link (profile runtime archive +
/// force-undefined symbol) is the caller's job.
///
/// Determinism: results return in DAG (unit) index order regardless of which worker finished first;
/// the caller iterates the returned outcomes / the units' capability libs in that same order. `-j 1`
/// is byte-identical to any `-j N` (each object is produced by an independent single-threaded codegen;
/// only *which thread* runs it differs). A codegen error is reported for the lowest failing DAG index.
///
/// `obj_paths.len()` must equal `units.len()`. `build`/`run`/`size` pass no export roots (a unit's
/// `pub` fns are already external; the entry's `main` is the only linker root).
#[allow(clippy::too_many_arguments)]
pub fn codegen_units_parallel(
    units: &[PerUnitArtifact],
    obj_paths: &[std::path::PathBuf],
    cache: &CacheContext,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
    pgo: &PgoMode,
) -> Result<UnitCodegen, String> {
    assert_eq!(units.len(), obj_paths.len(), "one object path per unit");
    let inputs: Vec<CodegenUnitInput<'_>> = units
        .iter()
        .map(|unit| CodegenUnitInput {
            unit: unit.unit.as_str(),
            impl_hash: unit.summary.impl_hash,
            dep_interface_hashes: &unit.dep_interface_hashes,
        })
        .collect();
    let staged = stage_pgo(pgo)?;
    let phase1 = codegen_lookup_phase(&inputs, obj_paths, cache, target, profile, rt_lto, staged.key)?;
    // Every MIR is already lowered on this path, so there is nothing to materialize between the
    // phases.
    let mirs: Vec<&align_mir::Program> = units.iter().map(|unit| &unit.mir).collect();
    codegen_produce_phase(&inputs, phase1, &mirs, obj_paths, cache, target, profile, rt_lto, jobs, &staged)
}

/// The `build`/`run`/`size` codegen driver over a [`PackageBuild`]: the same two phases as
/// [`codegen_units_parallel`], with ONE step between them — every unit that is both reused and a
/// codegen MISS is rehydrated and verified, serially on this thread, so the parallel phase still
/// receives a plain slice of already-lowered MIR.
///
/// Serial by design: sema is not audited for concurrent use, and on the common all-hit build there
/// is nothing to materialize at all. `-j 1` byte-identity, DAG ordering, immediate CAS publish, PGO
/// staging, and the lowest-index error rule are inherited unchanged.
#[allow(clippy::too_many_arguments)]
pub fn codegen_package_parallel(
    build: &mut PackageBuild,
    obj_paths: &[std::path::PathBuf],
    cache: &CacheContext,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
    pgo: &PgoMode,
) -> Result<UnitCodegen, PackageCodegenError> {
    assert_eq!(build.units.len(), obj_paths.len(), "one object path per unit");
    let staged = stage_pgo(pgo).map_err(PackageCodegenError::Failed)?;
    let phase1 = {
        let inputs: Vec<CodegenUnitInput<'_>> = build
            .units
            .iter()
            .map(|unit| CodegenUnitInput {
                unit: unit.unit.as_str(),
                impl_hash: unit.summary.impl_hash,
                dep_interface_hashes: &unit.dep_interface_hashes,
            })
            .collect();
        codegen_lookup_phase(&inputs, obj_paths, cache, target, profile, rt_lto, staged.key)
            .map_err(PackageCodegenError::Failed)?
    };
    // Materialize only what phase 2 will actually compile. A frontend hit whose object also hit
    // never needs its MIR, which is the whole point of storing the frontend without it.
    for &index in &phase1.misses {
        if build.units[index].is_reused() {
            let unit = build.units[index].unit.clone();
            build
                .materialize(index)
                .map_err(|failure| PackageCodegenError::StaleCacheEntry { unit, failure })?;
        }
    }
    let inputs: Vec<CodegenUnitInput<'_>> = build
        .units
        .iter()
        .map(|unit| CodegenUnitInput {
            unit: unit.unit.as_str(),
            impl_hash: unit.summary.impl_hash,
            dep_interface_hashes: &unit.dep_interface_hashes,
        })
        .collect();
    // A HIT unit is never compiled, so its absent MIR is never read; a placeholder keeps the slice
    // indexable without inventing a program. `codegen_produce_phase` only indexes `misses`.
    let placeholder = align_mir::Program::default();
    let mirs: Vec<&align_mir::Program> = build
        .units
        .iter()
        .map(|unit| unit.mir().unwrap_or(&placeholder))
        .collect();
    codegen_produce_phase(&inputs, phase1, &mirs, obj_paths, cache, target, profile, rt_lto, jobs, &staged)
        .map_err(PackageCodegenError::Failed)
}

/// One unit's contribution to a codegen key, borrowed from either unit shape.
struct CodegenUnitInput<'a> {
    unit: &'a str,
    impl_hash: Hash128,
    dep_interface_hashes: &'a [(String, Hash128)],
}

/// The serial lookup phase's result: per-unit keys and outcomes, plus the DAG indices still needing
/// codegen.
struct CodegenLookups {
    keys: Vec<Option<CodegenKey>>,
    outcomes: Vec<Option<CacheOutcome>>,
    misses: Vec<usize>,
}

/// The instrument-PGO snapshot for one invocation: the cache-key component plus the staged profile
/// libLLVM actually reads. Held by the caller so the temp file outlives the parallel scope.
struct StagedPgo {
    key: cache::PgoKey,
    effective: PgoMode,
    _guard: Option<StagedProfdata>,
}

struct PipelineTask {
    index: usize,
    unit: String,
    mir: MirProgram,
    object: std::path::PathBuf,
    #[cfg(test)]
    panic_before_emit: bool,
}

#[cfg(test)]
static PIPELINE_POOLS_STARTED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

struct PipelineQueueState {
    tasks: std::collections::VecDeque<PipelineTask>,
    results: Vec<(usize, Result<UnitPgoRun, String>)>,
    in_flight: usize,
    closed: bool,
    cancelled: bool,
}

struct PipelineQueue {
    state: std::sync::Mutex<PipelineQueueState>,
    ready: std::sync::Condvar,
}

#[derive(Clone)]
struct PipelineEmitConfig {
    target: BuildTarget,
    profile: Profile,
    rt_lto: bool,
    pgo: PgoMode,
}

struct PipelineClaimGuard {
    queue: std::sync::Arc<PipelineQueue>,
}

impl Drop for PipelineClaimGuard {
    fn drop(&mut self) {
        let mut state = self.queue.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        debug_assert!(state.in_flight > 0, "a claimed pipeline task must be in flight");
        if state.in_flight > 0 {
            state.in_flight -= 1;
        }
        if std::thread::panicking() {
            state.cancelled = true;
            state.tasks.clear();
        }
        self.queue.ready.notify_all();
    }
}

struct PipelineWorkers {
    queue: std::sync::Arc<PipelineQueue>,
    config: PipelineEmitConfig,
    handles: Vec<std::thread::JoinHandle<()>>,
    background_limit: usize,
}

impl PipelineWorkers {
    fn start(jobs: usize, config: PipelineEmitConfig) -> PipelineWorkers {
        #[cfg(test)]
        PIPELINE_POOLS_STARTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let queue = std::sync::Arc::new(PipelineQueue {
            state: std::sync::Mutex::new(PipelineQueueState {
                tasks: std::collections::VecDeque::new(),
                results: Vec::new(),
                in_flight: 0,
                closed: false,
                cancelled: false,
            }),
            ready: std::sync::Condvar::new(),
        });
        PipelineWorkers {
            queue,
            config,
            handles: Vec::new(),
            background_limit: jobs.saturating_sub(1),
        }
    }

    /// Grow only as outstanding work can use another background worker. `Builder::spawn` is
    /// fallible, so process thread exhaustion degrades to fewer workers and the coordinator still
    /// drains the queue instead of unwinding with already-created handles detached.
    fn ensure_background_workers(&mut self, available_work: usize) {
        let target = self.background_limit.min(available_work);
        while self.handles.len() < target {
            let worker_queue = std::sync::Arc::clone(&self.queue);
            let worker_config = self.config.clone();
            let worker_number = self.handles.len();
            let spawned = std::thread::Builder::new()
                .name(format!("align-codegen-{worker_number}"))
                .spawn(move || pipeline_worker(worker_queue, &worker_config));
            match spawned {
                Ok(handle) => self.handles.push(handle),
                Err(_) => break,
            }
        }
    }

    fn enqueue(&mut self, task: PipelineTask) {
        let mut state = self.queue.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.cancelled {
            state.tasks.push_back(task);
            self.queue.ready.notify_one();
            let available_work = state.tasks.len() + state.in_flight;
            drop(state);
            self.ensure_background_workers(available_work);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.queue
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cancelled
    }

    fn cancel(&self) {
        let mut state = self.queue.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.cancelled = true;
        state.tasks.clear();
        self.queue.ready.notify_all();
    }

    fn finish(mut self, coordinator_works: bool) -> Vec<(usize, Result<UnitPgoRun, String>)> {
        {
            let mut state = self.queue.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.closed = true;
            self.queue.ready.notify_all();
        }
        let coordinator_panic = if coordinator_works {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                pipeline_worker(std::sync::Arc::clone(&self.queue), &self.config)
            }))
            .err()
        } else {
            None
        };
        let mut worker_panic = None;
        for handle in self.handles.drain(..) {
            if let Err(payload) = handle.join()
                && worker_panic.is_none()
            {
                worker_panic = Some(payload);
            }
        }
        if let Some(payload) = coordinator_panic.or(worker_panic) {
            std::panic::resume_unwind(payload);
        }
        let mut state = self.queue.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut state.results)
    }
}

fn pipeline_worker(queue: std::sync::Arc<PipelineQueue>, config: &PipelineEmitConfig) {
    loop {
        let task = {
            let mut state = queue.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            loop {
                if state.cancelled {
                    return;
                }
                if let Some(task) = state.tasks.pop_front() {
                    state.in_flight += 1;
                    break task;
                }
                if state.closed {
                    return;
                }
                state = queue.ready.wait(state).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let _claim = PipelineClaimGuard { queue: std::sync::Arc::clone(&queue) };
        #[cfg(test)]
        if task.panic_before_emit {
            panic!("injected pipeline worker panic");
        }
        let result = emit_unit_object(
            &task.mir,
            &task.object,
            &config.target,
            config.profile,
            config.rt_lto,
            &config.pgo,
        )
        .map_err(|error| format!("codegen failed for unit `{}`: {error}", task.unit));
        let failed = result.is_err();
        let mut state = queue.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.results.push((task.index, result));
        if failed {
            state.cancelled = true;
            state.tasks.clear();
            queue.ready.notify_all();
        }
        drop(state);
        drop(_claim);
    }
}

/// Build one ordinary non-ThinLTO package while ready-unit codegen overlaps the remaining serial
/// frontend work. The CLI owns the one stale-entry retry so each attempt's diagnostics stay paired
/// with the exact `SourceMap` that allocated their file ids.
#[allow(clippy::too_many_arguments)]
pub fn build_package_pipelined(
    source_map: &mut SourceMap,
    name: &str,
    src: &str,
    cache: CacheContext,
    reuse: UnitReuse,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
    pgo: &PgoMode,
) -> PipelinedPackageBuild {
    build_package_pipelined_at(
        source_map, name, src, cache, reuse, target, profile, rt_lto, jobs, pgo, None,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_package_pipelined_at(
    source_map: &mut SourceMap,
    name: &str,
    src: &str,
    cache: CacheContext,
    reuse: UnitReuse,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
    pgo: &PgoMode,
    entry_access_path: Option<&std::path::Path>,
) -> PipelinedPackageBuild {
    let jobs = jobs.max(1);

    // Setup may execute before the walk to enable overlap, but its failure is retained until the
    // complete frontend verdict is known. Each later setup phase is attempted only when the prior
    // phase succeeded, preserving the public precedence and avoiding paths/workers without an owner.
    let object_stage = ArtifactStage::temp("align-per-unit-obj")
        .map_err(|error| format!("cannot create object staging directory: {error}"));
    let staged_pgo = object_stage.as_ref().ok().map(|_| stage_pgo_shallow(pgo));
    let target_init = staged_pgo
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .map(|_| align_codegen_llvm::ensure_target_initialized().map_err(|error| error.to_string()));
    let emit_config = target_init
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|_| staged_pgo.as_ref()?.as_ref().ok())
        .map(|staged| PipelineEmitConfig {
            target: target.clone(),
            profile,
            rt_lto,
            pgo: staged.effective.clone(),
        });

    let mut keys: Vec<Option<CodegenKey>> = Vec::new();
    let mut outcomes: Vec<Option<CacheOutcome>> = Vec::new();
    let mut objects: Vec<std::path::PathBuf> = Vec::new();
    let mut deferred: Vec<usize> = Vec::new();
    let mut key_error: Option<String> = None;
    let mut workers: Option<PipelineWorkers> = None;

    let walk = {
        let mut on_ready = |index: usize,
                            unit: &str,
                            dep_interface_hashes: &[(String, Hash128)],
                            pending: &mut PendingPerUnitArtifact| {
            debug_assert_eq!(
                index,
                objects.len(),
                "ready-unit indices must be dense and DAG ordered"
            );
            let object = object_stage
                .as_ref()
                .ok()
                .map(|stage| stage.path().join(format!("unit{index}.o")))
                .unwrap_or_default();
            objects.push(object.clone());
            keys.push(None);
            outcomes.push(None);

            // Stage/PGO setup failures preclude every cache operation. A first key error likewise
            // ends logical setup; the frontend itself continues so its diagnostics retain precedence.
            if object_stage.is_err()
                || staged_pgo.as_ref().is_some_and(Result::is_err)
                || key_error.is_some()
            {
                return;
            }
            let pgo_key = staged_pgo
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map(|staged| staged.key)
                .unwrap_or(cache::PgoKey::Off);
            let key = if cache.codegen_is_enabled() {
                match build_codegen_key(
                    unit,
                    pending.summary.impl_hash,
                    dep_interface_hashes,
                    target,
                    profile,
                    &[],
                    rt_lto,
                    pgo_key,
                ) {
                    Ok(key) => Some(key),
                    Err(error) => {
                        key_error = Some(error);
                        if let Some(pool) = workers.as_ref() {
                            pool.cancel();
                        }
                        return;
                    }
                }
            } else {
                None
            };

            let lookup = match key.as_ref() {
                Some(key) => cache.lookup(key, &object),
                None => CacheLookup::Miss { reason: None },
            };
            keys[index] = key;
            match lookup {
                CacheLookup::Hit(outcome) => {
                    outcomes[index] = Some(outcome);
                    // A hit needs no MIR. Destructive extraction proves the pipeline never retains
                    // a second owner while a miss task owns the same lowered program.
                    let _ = pending.body.take_lowered();
                }
                CacheLookup::Miss { reason } => {
                    outcomes[index] = Some(CacheOutcome {
                        stage: CacheStage::Codegen,
                        unit: unit.to_string(),
                        hit: false,
                        miss_reason: reason,
                    });
                    if matches!(pending.body, UnitBody::Reused { .. }) {
                        deferred.push(index);
                        return;
                    }
                    // A target-init error is selected only after stale-entry validation, but it
                    // must prevent workers from entering LLVM. Keep the MIR in the package until
                    // that error is returned.
                    let Some(config) = emit_config.as_ref() else {
                        return;
                    };
                    if workers.is_none() {
                        workers = Some(PipelineWorkers::start(jobs, config.clone()));
                    }
                    if workers.as_ref().is_some_and(PipelineWorkers::is_cancelled) {
                        let _ = pending.body.take_lowered();
                        return;
                    }
                    if let Some(mir) = pending.body.take_lowered() {
                        workers.as_mut().expect("pipeline workers just started").enqueue(
                            PipelineTask {
                                index,
                                unit: unit.to_string(),
                                mir,
                                object,
                                #[cfg(test)]
                                panic_before_emit: false,
                            },
                        );
                    }
                }
            }
        };
        walk_inner(
            source_map,
            name,
            src,
            false,
            &cache,
            reuse,
            Some(&mut on_ready),
            entry_access_path,
        )
    };
    let mut build = walk.into_package();

    if build.diags.has_errors() {
        if let Some(pool) = workers.take() {
            pool.cancel();
            let _ = pool.finish(false);
        }
        return PipelinedPackageBuild::FrontendFailed { diags: build.diags };
    }
    if build.units.is_empty() {
        if let Some(pool) = workers.take() {
            pool.cancel();
            let _ = pool.finish(false);
        }
        return PipelinedPackageBuild::CodegenFailed {
            diags: build.diags,
            error: PackageCodegenError::Failed("no units to build".to_string()),
        };
    }

    let stage = match object_stage {
        Ok(stage) => stage,
        Err(error) => {
            return PipelinedPackageBuild::CodegenFailed {
                diags: build.diags,
                error: PackageCodegenError::Failed(error),
            };
        }
    };
    let staged = match staged_pgo.expect("PGO setup exists when the object stage exists") {
        Ok(staged) => staged,
        Err(error) => {
            return PipelinedPackageBuild::CodegenFailed {
                diags: build.diags,
                error: PackageCodegenError::Failed(error),
            };
        }
    };
    if let Some(error) = key_error {
        if let Some(pool) = workers.take() {
            pool.cancel();
            let _ = pool.finish(false);
        }
        return PipelinedPackageBuild::CodegenFailed {
            diags: build.diags,
            error: PackageCodegenError::Failed(error),
        };
    }

    // Reused frontend misses are recomputed serially in DAG order. This remains mandatory after a
    // speculative worker error because a stale entry has higher observable precedence.
    for index in deferred {
        let unit = build.units[index].unit.clone();
        if let Err(failure) = build.materialize(index) {
            if let Some(pool) = workers.take() {
                pool.cancel();
                let _ = pool.finish(false);
            }
            return PipelinedPackageBuild::CodegenFailed {
                diags: build.diags,
                error: PackageCodegenError::StaleCacheEntry { unit, failure },
            };
        }
        if target_init.as_ref().is_some_and(Result::is_err) {
            continue;
        }
        if workers.as_ref().is_some_and(PipelineWorkers::is_cancelled) {
            let _ = build.units[index].body.take_lowered();
            continue;
        }
        if workers.is_none() {
            workers = Some(PipelineWorkers::start(
                jobs,
                PipelineEmitConfig {
                    target: target.clone(),
                    profile,
                    rt_lto,
                    pgo: staged.effective.clone(),
                },
            ));
        }
        if let Some(mir) = build.units[index].body.take_lowered() {
            workers.as_mut().expect("pipeline workers just started").enqueue(PipelineTask {
                index,
                unit,
                mir,
                object: objects[index].clone(),
                #[cfg(test)]
                panic_before_emit: false,
            });
        }
    }

    if let Some(Err(error)) = target_init {
        if let Some(pool) = workers.take() {
            pool.cancel();
            let _ = pool.finish(false);
        }
        return PipelinedPackageBuild::CodegenFailed {
            diags: build.diags,
            error: PackageCodegenError::Failed(error),
        };
    }

    let mut runs = workers.take().map(|pool| pool.finish(true)).unwrap_or_default();
    runs.sort_by_key(|(index, _)| *index);

    // The validation commit has passed. Publish every successful claimed miss, even when a sibling
    // failed, matching the existing two-phase driver's useful-sibling behavior.
    for (index, result) in &runs {
        if result.is_ok()
            && let Some(key) = &keys[*index]
        {
            cache.publish_after_miss(key, &objects[*index]);
        }
    }
    if let Some((_, Err(error))) = runs.iter().find(|(_, result)| result.is_err()) {
        return PipelinedPackageBuild::CodegenFailed {
            diags: build.diags,
            error: PackageCodegenError::Failed(error.clone()),
        };
    }

    let mut pgo_warnings = Vec::new();
    let mut pgo_matched = 0;
    let mut pgo_total = 0;
    if matches!(staged.effective, PgoMode::Use(_)) {
        for (_, result) in &runs {
            if let Ok(run) = result {
                pgo_warnings.extend(run.warnings.iter().cloned());
                pgo_matched += run.matched_fns;
                pgo_total += run.total_fns;
            }
        }
    }
    let codegen = UnitCodegen {
        outcomes: outcomes
            .into_iter()
            .map(|outcome| outcome.expect("every clean unit completes codegen lookup setup"))
            .collect(),
        pgo_warnings,
        pgo_matched,
        pgo_total,
    };
    let units = build
        .units
        .into_iter()
        .zip(objects)
        .map(|(unit, object)| PipelinedBuiltUnit {
            unit: unit.unit,
            link_libs: unit.link_libs,
            frontend: unit.frontend,
            object,
        })
        .collect();
    PipelinedPackageBuild::Complete(PipelinedPackageComplete {
        units,
        diags: build.diags,
        codegen,
        object_stage: stage,
    })
}

#[cfg(test)]
mod pipeline_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn config() -> PipelineEmitConfig {
        PipelineEmitConfig {
            target: BuildTarget::Baseline,
            profile: Profile::Dev,
            rt_lto: false,
            pgo: PgoMode::Off,
        }
    }

    #[test]
    fn job_budget_is_global() {
        let _serial = serial();
        for (jobs, background) in [(1usize, 0usize), (2, 1), (4, 3)] {
            let mut pool = PipelineWorkers::start(jobs, config());
            assert!(pool.handles.is_empty(), "workers are lazy until work exists");
            pool.ensure_background_workers(background);
            assert_eq!(pool.handles.len(), background, "coordinator occupies one of {jobs} jobs");
            pool.cancel();
            assert!(pool.finish(false).is_empty());
        }
    }

    #[test]
    fn oversized_job_count_is_capped_by_available_work() {
        let _serial = serial();
        let mut pool = PipelineWorkers::start(usize::MAX, config());
        pool.ensure_background_workers(1);
        assert_eq!(pool.handles.len(), 1, "one task can use at most one background worker");
        pool.cancel();
        assert!(pool.finish(false).is_empty());
    }

    #[test]
    fn bad_profdata_header_is_rejected_before_any_tail_read() {
        struct BadHeaderThenPanic {
            served_header: bool,
        }

        impl std::io::Read for BadHeaderThenPanic {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                if self.served_header {
                    panic!("a rejected header must prevent every tail read");
                }
                self.served_header = true;
                let n = out.len().min(8);
                out[..n].fill(0);
                Ok(n)
            }
        }

        let mut reader = BadHeaderThenPanic { served_header: false };
        let error = read_validated_profdata_bytes(
            std::path::Path::new("huge-garbage.profdata"),
            &mut reader,
            u64::MAX,
        )
        .expect_err("bad magic must be rejected");
        assert!(error.contains("bad magic"), "unexpected diagnostic: {error}");
    }

    #[test]
    fn panicking_worker_notifies_joins_then_resumes() {
        let _serial = serial();
        let stage = ArtifactStage::temp("align-pipeline-panic-test").expect("stage");
        let mut pool = PipelineWorkers::start(2, config());
        pool.enqueue(PipelineTask {
            index: 0,
            unit: "panic-owner".to_string(),
            mir: MirProgram::default(),
            object: stage.path().join("unit0.o"),
            panic_before_emit: true,
        });
        let resumed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pool.finish(true)));
        assert!(resumed.is_err(), "the original worker panic must resume after every join");
    }

    #[test]
    fn all_hit_starts_no_worker() {
        let _serial = serial();
        if !backend_available() {
            return;
        }
        static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "align-pipeline-all-hit-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("project dir");
        let entry = root.join("main.align");
        let source = "fn main() {\n  print(1)\n}\n";
        std::fs::write(&entry, source).expect("entry source");
        let cache_root = root.join("cache");
        let run = || {
            let mut source_map = SourceMap::new();
            build_package_pipelined(
                &mut source_map,
                entry.to_str().expect("utf-8 entry"),
                source,
                CacheContext::at(cache_root.clone()),
                UnitReuse::Allowed,
                &BuildTarget::Baseline,
                Profile::Dev,
                false,
                4,
                &PgoMode::Off,
            )
        };

        PIPELINE_POOLS_STARTED.store(0, Ordering::Relaxed);
        assert!(matches!(run(), PipelinedPackageBuild::Complete(_)));
        let after_cold = PIPELINE_POOLS_STARTED.load(Ordering::Relaxed);
        assert_eq!(after_cold, 1, "a cold object miss starts one pool");
        assert!(matches!(run(), PipelinedPackageBuild::Complete(_)));
        assert_eq!(
            PIPELINE_POOLS_STARTED.load(Ordering::Relaxed),
            after_cold,
            "an all-object-hit build must create no pool or worker"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ready_seam_preserves_per_unit_projection() {
        let _serial = serial();
        static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "align-ready-per-unit-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("project dir");
        std::fs::write(root.join("dep.align"), "module dep\npub fn value() -> i64 = 4\n")
            .expect("dependency");
        let entry = root.join("main.align");
        let source = "import dep\nfn main() {\n  print(dep.value())\n}\n";
        std::fs::write(&entry, source).expect("entry");
        let name = entry.to_str().expect("utf-8 entry");

        let mut expected_map = SourceMap::new();
        let expected = build_per_unit(&mut expected_map, name, source);
        let mut actual_map = SourceMap::new();
        let mut noop = |_: usize,
                        _: &str,
                        _: &[(String, Hash128)],
                        _: &mut PendingPerUnitArtifact| {};
        let actual = walk_inner(
            &mut actual_map,
            name,
            source,
            false,
            &CacheContext::Disabled,
            UnitReuse::Forbidden,
            Some(&mut noop),
            None,
        )
        .into_per_unit();
        assert_eq!(
            format_diagnostics(&expected_map, &expected.diags),
            format_diagnostics(&actual_map, &actual.diags)
        );
        assert_eq!(expected.dep_interface_hashes, actual.dep_interface_hashes);
        assert_eq!(expected.units.len(), actual.units.len());
        for (expected, actual) in expected.units.iter().zip(&actual.units) {
            assert_eq!(expected.unit, actual.unit);
            assert_eq!(expected.is_entry, actual.is_entry);
            assert_eq!(
                align_mir::print::program_to_string(&expected.mir),
                align_mir::print::program_to_string(&actual.mir)
            );
            assert_eq!(
                align_interface::serialize(&expected.summary),
                align_interface::serialize(&actual.summary)
            );
            assert_eq!(expected.dep_interface_hashes, actual.dep_interface_hashes);
            assert_eq!(expected.file, actual.file);
            assert_eq!(expected.static_descriptors, actual.static_descriptors);
            assert_eq!(expected.static_inputs, actual.static_inputs);
            assert_eq!(expected.static_artifacts, actual.static_artifacts);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ready_seam_preserves_package_projection() {
        let _serial = serial();
        static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "align-ready-package-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("project dir");
        std::fs::write(root.join("dep.align"), "module dep\npub fn value() -> i64 = 4\n")
            .expect("dependency");
        let entry = root.join("main.align");
        let source = "import dep\nfn main() {\n  print(dep.value())\n}\n";
        std::fs::write(&entry, source).expect("entry");
        let name = entry.to_str().expect("utf-8 entry");

        let mut expected_map = SourceMap::new();
        let expected = build_package(
            &mut expected_map,
            name,
            source,
            &CacheContext::Disabled,
            UnitReuse::Forbidden,
        );
        let mut actual_map = SourceMap::new();
        let mut noop = |_: usize,
                        _: &str,
                        _: &[(String, Hash128)],
                        _: &mut PendingPerUnitArtifact| {};
        let actual = walk_inner(
            &mut actual_map,
            name,
            source,
            false,
            &CacheContext::Disabled,
            UnitReuse::Forbidden,
            Some(&mut noop),
            None,
        )
        .into_package();
        assert_eq!(
            format_diagnostics(&expected_map, &expected.diags),
            format_diagnostics(&actual_map, &actual.diags)
        );
        assert_eq!(expected.units.len(), actual.units.len());
        for (expected, actual) in expected.units.iter().zip(&actual.units) {
            assert_eq!(expected.unit, actual.unit);
            assert_eq!(expected.is_entry, actual.is_entry);
            assert_eq!(
                align_interface::serialize(&expected.summary),
                align_interface::serialize(&actual.summary)
            );
            assert_eq!(expected.dep_interface_hashes, actual.dep_interface_hashes);
            assert_eq!(expected.link_libs, actual.link_libs);
            assert_eq!(expected.static_inputs, actual.static_inputs);
            assert_eq!(expected.frontend, actual.frontend);
            assert_eq!(expected.is_reused(), actual.is_reused());
            assert_eq!(
                expected.mir().map(align_mir::print::program_to_string),
                actual.mir().map(align_mir::print::program_to_string)
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }
}

/// Read, digest, and snapshot a `--pgo-use` profile — see the note in [`codegen_produce_phase`].
fn stage_pgo(pgo: &PgoMode) -> Result<StagedPgo, String> {
    match pgo {
        PgoMode::Off => Ok(StagedPgo { key: cache::PgoKey::Off, effective: PgoMode::Off, _guard: None }),
        PgoMode::Instrument => Ok(StagedPgo {
            key: cache::PgoKey::Instrument,
            effective: PgoMode::Instrument,
            _guard: None,
        }),
        PgoMode::Use(path) => {
            let bytes = watch_inputs::observe_consumed_read(
                path,
                |file| {
                    let mut file = file.map_err(|error| profdata_open_error(path, error))?;
                    let mut bytes = Vec::new();
                    file.read_to_end(&mut bytes).map_err(|error| {
                        format!(
                            "--pgo-use: cannot read profile data file '{}': {error}",
                            path.display()
                        )
                    })?;
                    Ok(bytes)
                },
                |result| result.as_ref().ok().map(Vec::as_slice),
                || Err("--pgo-use: watch observation rejected path".to_string()),
            )?;
            let digest = Hash128::of(&bytes);
            let staged = StagedProfdata::new(&bytes)?;
            let staged_path = staged.path().to_path_buf();
            Ok(StagedPgo {
                key: cache::PgoKey::Use(digest),
                effective: PgoMode::Use(staged_path),
                _guard: Some(staged),
            })
        }
    }
}

/// Pipelined-path PGO setup: shallow validation and the snapshot digest consume the same single
/// read, so a concurrent rewrite cannot separate the cache key from libLLVM's bytes.
fn stage_pgo_shallow(pgo: &PgoMode) -> Result<StagedPgo, String> {
    match pgo {
        PgoMode::Off => stage_pgo(pgo),
        PgoMode::Instrument => stage_pgo(pgo),
        PgoMode::Use(path) => {
            let bytes = read_and_validate_profdata(path)?;
            let digest = Hash128::of(&bytes);
            let staged = StagedProfdata::new(&bytes)?;
            let staged_path = staged.path().to_path_buf();
            Ok(StagedPgo {
                key: cache::PgoKey::Use(digest),
                effective: PgoMode::Use(staged_path),
                _guard: Some(staged),
            })
        }
    }
}

/// Phase 1 (serial): build each unit's codegen key and look it up. A HIT writes the object from the
/// CAS immediately; a MISS is queued. When the cache is disabled every unit is a miss and NO key
/// work runs — the codegen-key inputs are pure cache overhead a cache-off build must not pay.
#[allow(clippy::too_many_arguments)]
fn codegen_lookup_phase(
    inputs: &[CodegenUnitInput<'_>],
    obj_paths: &[std::path::PathBuf],
    cache: &CacheContext,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    pgo_key: cache::PgoKey,
) -> Result<CodegenLookups, String> {
    let n = inputs.len();
    let mut keys: Vec<Option<CodegenKey>> = (0..n).map(|_| None).collect();
    let mut outcomes: Vec<Option<CacheOutcome>> = (0..n).map(|_| None).collect();
    let mut misses: Vec<usize> = Vec::new();
    let enabled = cache.codegen_is_enabled();
    for (i, input) in inputs.iter().enumerate() {
        if enabled {
            let key = build_codegen_key(
                input.unit,
                input.impl_hash,
                input.dep_interface_hashes,
                target,
                profile,
                &[],
                rt_lto,
                pgo_key,
            )?;
            match cache.lookup(&key, &obj_paths[i]) {
                cache::CacheLookup::Hit(outcome) => outcomes[i] = Some(outcome),
                cache::CacheLookup::Miss { reason } => {
                    outcomes[i] = Some(CacheOutcome {
                        stage: CacheStage::Codegen,
                        unit: input.unit.to_string(),
                        hit: false,
                        miss_reason: reason,
                    });
                    misses.push(i);
                }
            }
            keys[i] = Some(key);
        } else {
            outcomes[i] = Some(CacheOutcome {
                stage: CacheStage::Codegen,
                unit: input.unit.to_string(),
                hit: false,
                miss_reason: None,
            });
            misses.push(i);
        }
    }
    Ok(CodegenLookups { keys, outcomes, misses })
}

/// Phase 2 (parallel): produce the misses. `jobs` workers pull the next miss through a shared atomic
/// index; each runs a fresh LLVM `Context` into its own `obj_paths[i]`, then publishes to the CAS
/// IMMEDIATELY on success.
///
/// Immediate publish is correct regardless of any PGO match ratio: the key already carries the
/// profile-content digest, so a published object is only ever served to a build with the identical
/// profile+source. Publishing in the worker (not deferred) also means a build where one unit fails
/// codegen still leaves its succeeded siblings published.
///
/// Results are DAG-ordered regardless of which worker finished first, and `-j 1` is byte-identical
/// to any `-j N`: only which thread runs an independent single-threaded codegen differs.
#[allow(clippy::too_many_arguments)]
fn codegen_produce_phase(
    inputs: &[CodegenUnitInput<'_>],
    lookups: CodegenLookups,
    mirs: &[&align_mir::Program],
    obj_paths: &[std::path::PathBuf],
    cache: &CacheContext,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
    staged: &StagedPgo,
) -> Result<UnitCodegen, String> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let CodegenLookups { keys, outcomes, misses } = lookups;
    // LLVM native-target init once, on the main thread, before any worker touches codegen.
    align_codegen_llvm::ensure_target_initialized().map_err(|e| e.to_string())?;
    let mut pgo_warnings: Vec<String> = Vec::new();
    let (mut pgo_matched, mut pgo_total): (u32, u32) = (0, 0);
    if !misses.is_empty() {
        let worker_count = jobs.max(1).min(misses.len());
        let next = AtomicUsize::new(0);
        let failed = AtomicBool::new(false);
        let errors = std::sync::Mutex::new(Vec::<(usize, String)>::new());
        let results = std::sync::Mutex::new(Vec::<(usize, UnitPgoRun)>::new());
        std::thread::scope(|scope| {
            for _ in 0..worker_count {
                scope.spawn(|| loop {
                    // Fail-fast: once any unit has errored, stop CLAIMING new work. Checked only
                    // between units — an in-progress emit is never interrupted. `Relaxed` is
                    // correct: the flag publishes no data (errors ride the Mutex, and the final
                    // read happens-after the scope join).
                    if failed.load(Ordering::Relaxed) {
                        break;
                    }
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    if k >= misses.len() {
                        break;
                    }
                    let i = misses[k];
                    match emit_unit_object(mirs[i], &obj_paths[i], target, profile, rt_lto, &staged.effective) {
                        Err(e) => {
                            errors.lock().expect("codegen error lock").push((i, e));
                            failed.store(true, Ordering::Relaxed);
                            continue;
                        }
                        Ok(run) => {
                            results.lock().expect("pgo result lock").push((i, run));
                        }
                    }
                    if let Some(key) = &keys[i] {
                        cache.publish_after_miss(key, &obj_paths[i]);
                    }
                });
            }
        });
        // Deterministic report: the lowest-DAG-index error among those collected. Fail-fast may
        // leave a higher-index unit unattempted, so with MULTIPLE independent failures the set
        // collected is timing-dependent; the reported error is still the lowest index present.
        let mut errs = errors.into_inner().expect("codegen error lock");
        if !errs.is_empty() {
            errs.sort_by_key(|(i, _)| *i);
            let (i, e) = &errs[0];
            return Err(format!("codegen failed for unit `{}`: {e}", inputs[*i].unit));
        }
        let mut runs = results.into_inner().expect("pgo result lock");
        runs.sort_by_key(|(i, _)| *i);
        if matches!(staged.effective, PgoMode::Use(_)) {
            pgo_matched = runs.iter().map(|(_, r)| r.matched_fns).sum();
            pgo_total = runs.iter().map(|(_, r)| r.total_fns).sum();
        }
        for (_, run) in runs {
            pgo_warnings.extend(run.warnings);
        }
    }
    Ok(UnitCodegen {
        outcomes: outcomes
            .into_iter()
            .map(|o| o.expect("every unit gets an outcome in phase 1"))
            .collect(),
        pgo_warnings,
        pgo_matched,
        pgo_total,
    })
}

/// One unit's PGO-pipeline result, returned by [`emit_unit_object`]: the profile-use staleness
/// warnings plus the profile-match tally (`matched`/`total` defined functions). Empty/zero for the
/// `Off` (stock) and `Instrument` (GEN) paths — only a `Use` run reads a profile.
struct UnitPgoRun {
    warnings: Vec<String>,
    matched_fns: u32,
    total_fns: u32,
}

/// Emit one per-unit object, swapping in the instrument-PGO pipeline when `pgo` is active. `Off` is the
/// stock, byte-identical [`emit_object_file`]; `Instrument`/`Use` route through
/// [`align_codegen_llvm::emit_object_pgo`] (GEN counters / USE `!prof` weights) and return that run's
/// profile-use staleness warnings + match tally (empty/zero for GEN). Called from the parallel miss
/// producer — each call builds its own LLVM `Context`, so the USE diagnostic handler is per-thread and
/// never races.
fn emit_unit_object(
    mir: &align_mir::Program,
    obj: &std::path::Path,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    pgo: &PgoMode,
) -> Result<UnitPgoRun, String> {
    let action = match pgo {
        PgoMode::Off => {
            emit_object_file(mir, obj, target.clone(), profile, &[], rt_lto)?;
            return Ok(UnitPgoRun { warnings: Vec::new(), matched_fns: 0, total_fns: 0 });
        }
        PgoMode::Instrument => align_codegen_llvm::pgo::PgoAction::Instrument,
        PgoMode::Use(p) => align_codegen_llvm::pgo::PgoAction::Use(p.as_path()),
    };
    let report = align_codegen_llvm::emit_object_pgo(
        mir, obj, target, profile, &[], rt_lto_bytes(rt_lto), action,
    )
    .map_err(|e| e.to_string())?;
    Ok(UnitPgoRun { warnings: report.warnings, matched_fns: report.matched_fns, total_fns: report.total_fns })
}

// ---- instrument-PGO surface (`--pgo-instrument` / `--pgo-use`) ----------------
// The driver-facing `PgoMode`, profdata validation + snapshotting, and profile-runtime resolution
// consumed by the S2 cached per-unit path (`codegen_units_parallel` above) and the instrumented link.

/// A private, per-invocation snapshot of the user's merged `.profdata`. The bytes read to compute the
/// `PgoKey::Use` digest are written here, and it is THIS path (never the user's live file) that
/// `emit_object_pgo` hands to libLLVM — so libLLVM provably reads the exact bytes that were digested
/// into the cache key, even if the user rewrites the original mid-build (a profile-iteration loop).
/// RAII: the staged file + its dir are removed on drop, after every emit has read it.
struct StagedProfdata {
    dir: std::path::PathBuf,
    file: std::path::PathBuf,
}

impl StagedProfdata {
    /// Write `bytes` to a fresh private temp file (unique dir: pid + monotonic nonce + wall stamp).
    fn new(bytes: &[u8]) -> Result<StagedProfdata, String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NONCE: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        for _ in 0..1024 {
            let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let dir = base.join(format!(".align-pgo-profdata-{}-{stamp}-{nonce}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => {
                    let file = dir.join("staged.profdata");
                    std::fs::write(&file, bytes)
                        .map_err(|e| format!("--pgo-use: cannot stage profile data snapshot: {e}"))?;
                    return Ok(StagedProfdata { dir, file });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("--pgo-use: cannot create profile snapshot dir: {e}")),
            }
        }
        Err("--pgo-use: could not create a unique profile snapshot directory".to_string())
    }

    fn path(&self) -> &std::path::Path {
        &self.file
    }
}

impl Drop for StagedProfdata {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The instrument-PGO build mode selected on the command line. `Off` is the byte-identical
/// default; `Instrument` builds a `-fprofile-generate`-equivalent binary; `Use` rebuilds
/// with `-fprofile-use` reading a merged `.profdata`. Mutually exclusive by construction.
#[derive(Clone, Debug)]
pub enum PgoMode {
    Off,
    Instrument,
    Use(std::path::PathBuf),
}

impl PgoMode {
    /// Whether any PGO mode is active (either flag present).
    pub fn is_on(&self) -> bool {
        !matches!(self, PgoMode::Off)
    }
}

/// Validate a merged `.profdata` before it is handed to the PGO USE pipeline (the S1 fail-loud
/// caveat, `docs/impl/07-roadmap.md`): the shim's return code CANNOT report a bad profile — libLLVM
/// diagnoses it on the context and, in the error case, exits the process — so existence /
/// readability / non-emptiness / a valid indexed-profdata magic are checked HERE, each a hard error
/// naming the path. The magic is the 8-byte indexed-profile header `llvm-profdata-22 merge` writes;
/// on the little-endian targets we support (x86-64 / arm64) the on-disk bytes are exactly
/// `ff 6c 70 72 6f 66 69 81` (verified against a real merged `.profdata`) — the 64-bit
/// `IndexedInstrProf::Magic` serialized LSB-first. A raw `.profraw` (different magic) is rejected,
/// guiding the user to run `merge` first.
pub fn validate_profdata(path: &std::path::Path) -> Result<(), String> {
    use std::io::Read;
    if !path.exists() {
        return Err(format!("--pgo-use: profile data file '{}' does not exist", path.display()));
    }
    if !path.is_file() {
        return Err(format!("--pgo-use: profile data path '{}' is not a regular file", path.display()));
    }
    let mut file = std::fs::File::open(path).map_err(|error| profdata_open_error(path, error))?;
    let mut head = [0u8; 8];
    let read = file
        .read(&mut head)
        .map_err(|error| format!("--pgo-use: cannot read profile data file '{}': {error}", path.display()))?;
    validate_profdata_header(path, &head[..read])
}

/// Read a profile exactly once after the public path-shape checks and validate only the indexed
/// profile header. Deeper format/version validation remains libLLVM's job on a cache miss.
fn read_and_validate_profdata(path: &std::path::Path) -> Result<Vec<u8>, String> {
    watch_inputs::observe_consumed_read(
        path,
        |file| read_and_validate_profdata_inner(path, file),
        |result| result.as_ref().ok().map(Vec::as_slice),
        || Err("--pgo-use: watch observation rejected path".to_string()),
    )
}

fn read_and_validate_profdata_inner(
    path: &std::path::Path,
    file: std::io::Result<std::fs::File>,
) -> Result<Vec<u8>, String> {
    let mut file = file.map_err(|error| profdata_open_error(path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("--pgo-use: cannot inspect profile data file '{}': {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("--pgo-use: profile data path '{}' is not a regular file", path.display()));
    }
    read_validated_profdata_bytes(path, &mut file, metadata.len())
}

fn profdata_open_error(path: &std::path::Path, error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        format!("--pgo-use: profile data file '{}' does not exist", path.display())
    } else {
        format!(
            "--pgo-use: cannot read profile data file '{}': {error}",
            path.display()
        )
    }
}

/// Validate the bounded header before allocating or reading the remaining snapshot. Kept as a
/// separate owner so a malformed first eight bytes can prove that no tail read is attempted.
fn read_validated_profdata_bytes(
    path: &std::path::Path,
    reader: &mut impl std::io::Read,
    expected_len: u64,
) -> Result<Vec<u8>, String> {
    use std::io::Read;

    let mut head = Vec::with_capacity(8);
    (&mut *reader)
        .take(8)
        .read_to_end(&mut head)
        .map_err(|error| format!("--pgo-use: cannot read profile data file '{}': {error}", path.display()))?;
    validate_profdata_header(path, &head)?;
    let mut bytes = Vec::with_capacity(expected_len.try_into().unwrap_or(8).max(8));
    bytes.extend_from_slice(&head);
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| format!("--pgo-use: cannot read profile data file '{}': {error}", path.display()))?;
    Ok(bytes)
}

fn validate_profdata_header(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    const MAGIC: [u8; 8] = [0xff, 0x6c, 0x70, 0x72, 0x6f, 0x66, 0x69, 0x81];
    const MAGIC_SWAPPED: [u8; 8] = [0x81, 0x69, 0x66, 0x6f, 0x72, 0x70, 0x6c, 0xff];
    if bytes.is_empty() {
        return Err(format!("--pgo-use: profile data file '{}' is empty", path.display()));
    }
    if bytes.len() < 8 || (bytes[..8] != MAGIC && bytes[..8] != MAGIC_SWAPPED) {
        return Err(format!(
            "--pgo-use: '{}' is not a valid LLVM indexed profile data file (bad magic); \
             merge your `.profraw` first: `llvm-profdata-22 merge -o out.profdata <raw>`",
            path.display()
        ));
    }
    Ok(())
}

/// Locate the clang profile runtime archive that defines the `__llvm_profile_runtime` anchor and the
/// atexit `.profraw` writer, resolved via `clang-22 -print-file-name` (which echoes the bare name
/// back verbatim when it cannot resolve it — treated as "not found" per probe).
///
/// clang ships this archive under two different layouts, so BOTH are probed, in order:
///   * the classic flat layout — `libclang_rt.profile-<arch>.a` (ELF) / `libclang_rt.profile_osx.a`
///     (Mach-O), the Debian/Ubuntu default; then
///   * the per-target-runtime layout (`LLVM_ENABLE_PER_TARGET_RUNTIME_DIR`, the Fedora/Arch default),
///     `lib/clang/<ver>/lib/<triple>/libclang_rt.profile.a` — no `-<arch>` suffix, which
///     `-print-file-name=libclang_rt.profile.a` resolves.
///
/// The architecture is derived from the build target's triple. A hard error (naming the probes) only
/// when clang is absent or NONE of the candidates resolve — an instrumented link without the archive
/// silently writes no profile.
pub fn profile_runtime_archive(target: &BuildTarget) -> Result<std::path::PathBuf, String> {
    let rt = align_codegen_llvm::resolve_target_identity(target).map_err(|e| e.to_string())?;
    let arch = rt.triple.split('-').next().unwrap_or("x86_64");
    let format = target_object_format()?;
    // Flat-layout name first, per-target-runtime name (`libclang_rt.profile.a`) second.
    let candidates: Vec<String> = match format {
        ObjectFormat::MachO => vec!["libclang_rt.profile_osx.a".to_string(), "libclang_rt.profile.a".to_string()],
        ObjectFormat::Elf => vec![format!("libclang_rt.profile-{arch}.a"), "libclang_rt.profile.a".to_string()],
    };
    let clang = llvm_tool("clang").ok_or_else(|| {
        "--pgo-instrument: clang (clang-22) not found on PATH — needed to locate the profile \
         runtime archive (libclang_rt.profile)"
            .to_string()
    })?;
    for name in &candidates {
        let out = std::process::Command::new(&clang)
            .arg(format!("-print-file-name={name}"))
            .output()
            .map_err(|e| format!("--pgo-instrument: cannot launch {}: {e}", clang.display()))?;
        let resolved = std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
        // A resolved absolute path that exists is a hit; the bare-name echo (relative, or the name
        // itself) means this candidate is not present — try the next.
        if resolved.is_absolute() && resolved.exists() {
            return Ok(resolved);
        }
    }
    Err(format!(
        "--pgo-instrument: profile runtime archive not found (tried {}); install the clang \
         compiler-rt profile library",
        candidates.join(", ")
    ))
}

/// The shared, per-build target/LLVM identity used by both ThinLTO cache keys — resolved once (the
/// same resolution `emit_prelink_bc` / `thinlto::backend` use, so a cache hit implies byte-identical
/// bytes).
struct ThinTargetIdentity {
    rt: align_codegen_llvm::ResolvedTarget,
    object_format: u8,
    llvm_version: String,
    llvm_build_id: Hash128,
}

fn resolve_thin_identity(target: &BuildTarget) -> Result<ThinTargetIdentity, String> {
    let rt = align_codegen_llvm::resolve_target_identity(target).map_err(|e| e.to_string())?;
    let object_format = match target_object_format()? {
        ObjectFormat::Elf => 0u8,
        ObjectFormat::MachO => 1u8,
    };
    let llvm_build_id = align_codegen_llvm::loaded_llvm_build_id()
        .ok_or_else(|| "cannot identify loaded LLVM build for codegen cache".to_string())?;
    Ok(ThinTargetIdentity {
        rt,
        object_format,
        llvm_version: align_codegen_llvm::llvm_version(),
        llvm_build_id,
    })
}

/// Build the phase-1 **prelink** cache key: today's codegen key minus the pure backend/target knobs
/// (cpu/features/reloc/code-model/machine-opt) — see [`cache::PrelinkKey`].
fn build_prelink_key(
    unit: &str,
    partition: cache::PartitionKey,
    impl_hash: Hash128,
    dep_interface_hashes: &[(String, Hash128)],
    exports: &[String],
    id: &ThinTargetIdentity,
    profile: Profile,
    rt_lto: bool,
) -> cache::PrelinkKey {
    let mut dep_hashes = dep_interface_hashes.to_vec();
    dep_hashes.sort_by(|a, b| a.0.cmp(&b.0));
    let mut exp = exports.to_vec();
    exp.sort();
    exp.dedup();
    cache::PrelinkKey {
        cache_format_version: cache::CACHE_KEY_FORMAT_VERSION,
        compiler_build_id: cache::compiler_build_id(),
        frontend_schema: align_interface::FORMAT_VERSION,
        located: false,
        impl_hash,
        dep_interface_hashes: dep_hashes,
        exports: exp,
        target_triple: id.rt.triple.clone(),
        object_format: id.object_format,
        profile_name: profile.name().to_string(),
        pipeline: profile.pipeline().to_string(),
        llvm_version: id.llvm_version.clone(),
        llvm_build_id: id.llvm_build_id,
        rt_lto,
        rt_lto_digest: rt_lto.then(|| Hash128::of(rt_lto_bitcode())),
        unit: unit.to_string(),
        partition,
    }
}

/// Build the phase-3 **backend** cache key — the precise cross-unit digest (see [`cache::BackendKey`]).
/// `inbound`, `outbound_exports`, and `import_source_digests` come from the thin-link plan; they are
/// sorted+deduped here so the key is order-independent.
#[allow(clippy::too_many_arguments)]
fn build_backend_key(
    unit: &str,
    partition: cache::PartitionKey,
    exports: &[String],
    id: &ThinTargetIdentity,
    profile: Profile,
    own_prelink_digest: Hash128,
    inbound: Vec<cache::InboundImport>,
    outbound_exports: Vec<u64>,
    import_source_digests: Vec<cache::ImportSourceDigest>,
) -> cache::BackendKey {
    let mut inbound = inbound;
    inbound.sort();
    inbound.dedup();
    let mut outbound_exports = outbound_exports;
    outbound_exports.sort_unstable();
    outbound_exports.dedup();
    let mut import_source_digests = import_source_digests;
    import_source_digests.sort_by(|left, right| left.source.cmp(&right.source));
    import_source_digests.dedup_by(|left, right| left.source == right.source);
    let mut exp = exports.to_vec();
    exp.sort();
    exp.dedup();
    cache::BackendKey {
        cache_format_version: cache::CACHE_KEY_FORMAT_VERSION,
        compiler_build_id: cache::compiler_build_id(),
        llvm_version: id.llvm_version.clone(),
        llvm_build_id: id.llvm_build_id,
        target_triple: id.rt.triple.clone(),
        object_format: id.object_format,
        resolved_cpu: id.rt.cpu.clone(),
        resolved_features: id.rt.features.clone(),
        reloc_model: id.rt.reloc_model.to_string(),
        code_model: id.rt.code_model.to_string(),
        profile_name: profile.name().to_string(),
        pipeline: profile.pipeline().to_string(),
        codegen_opt: profile.codegen_opt_name().to_string(),
        own_prelink_digest,
        inbound_imports: inbound,
        outbound_exports,
        import_source_digests,
        exports: exp,
        unit: unit.to_string(),
        partition,
    }
}

/// One validated function/support module and its exact structural identity. The borrowed view is
/// the sole input to both hashing and LLVM emission; derived strings remain owned so `exports` does
/// not participate in this record's lifetime.
pub struct ThinPartition<'a> {
    pub unit: &'a str,
    pub view: PartitionCodegenView<'a>,
    pub impl_hash: Hash128,
    pub preserve_symbols: Vec<String>,
}

impl ThinPartition<'_> {
    fn key(&self) -> PartitionKey {
        match &self.view {
            PartitionCodegenView::Function { selected, .. } => {
                PartitionKey::Function(selected.name.clone())
            }
            PartitionCodegenView::Support { .. } => PartitionKey::Support,
        }
    }

    fn source(&self) -> ThinPartitionSource {
        ThinPartitionSource {
            unit: self.unit.to_owned(),
            partition: self.key(),
        }
    }

    fn stable_id(&self) -> String {
        let unit_hex = thin_hex(self.unit.as_bytes());
        let prefix = format!("align-shard-v1${}${unit_hex}", self.unit.len());
        match &self.view {
            PartitionCodegenView::Support { .. } => format!("{prefix}$s"),
            PartitionCodegenView::Function { selected, .. } => {
                let function = selected.name.as_bytes();
                format!("{prefix}$f${}${}", function.len(), thin_hex(function))
            }
        }
    }

    fn label(&self) -> String {
        match &self.view {
            PartitionCodegenView::Support { .. } => format!("{}::support", self.unit),
            PartitionCodegenView::Function { selected, .. } => {
                format!("{}::{}", self.unit, selected.name)
            }
        }
    }
}

fn thin_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn thin_identity_error(prefix: &str, bytes: &[u8]) -> String {
    format!("{prefix}:{}:{}", bytes.len(), thin_hex(bytes))
}

fn thin_function_abi(
    program: &align_mir::Program,
    function: &align_mir::Function,
) -> Result<align_mir::CanonicalFnAbi, String> {
    let mut params = Vec::with_capacity(function.params.len());
    if function.params.len() != function.param_modes.len() {
        return Err(thin_identity_error(
            "ThinLTO function ABI invalid",
            function.name.as_bytes(),
        ));
    }
    for (&slot, &mode) in function.params.iter().zip(&function.param_modes) {
        let ty = function.slots.get(slot as usize).copied().ok_or_else(|| {
            thin_identity_error("ThinLTO function ABI invalid", function.name.as_bytes())
        })?;
        params.push((mode, ty));
    }
    align_mir::CanonicalFnAbi::from_parts(
        &params,
        function.ret,
        &function.return_borrow,
        &function.return_region,
        function.return_cleanup,
        program,
    )
    .map_err(|_| thin_identity_error("ThinLTO function ABI invalid", function.name.as_bytes()))
}

fn thin_peer(
    unit: &str,
    program: &align_mir::Program,
    function: &align_mir::Function,
    exports: &[String],
) -> Result<ThinPeerDeclaration, String> {
    let (symbol, linkage) =
        align_codegen_llvm::partition_function_symbol(unit, function, exports)?;
    Ok(ThinPeerDeclaration {
        logical: function.name.clone(),
        abi: thin_function_abi(program, function)?,
        symbol,
        linkage,
    })
}

fn thin_imported_peer(
    program: &align_mir::Program,
    function: &align_mir::ImportedFn,
) -> Result<ThinPeerDeclaration, String> {
    if function.params.len() != function.param_modes.len() {
        return Err(thin_identity_error(
            "ThinLTO imported function ABI invalid",
            function.name.as_bytes(),
        ));
    }
    let params = function
        .param_modes
        .iter()
        .copied()
        .zip(function.params.iter().copied())
        .collect::<Vec<_>>();
    let abi = align_mir::CanonicalFnAbi::from_parts(
        &params,
        function.ret,
        &function.return_borrow,
        &function.return_region,
        function.return_cleanup,
        program,
    )
    .map_err(|_| {
        thin_identity_error(
            "ThinLTO imported function ABI invalid",
            function.name.as_bytes(),
        )
    })?;
    Ok(ThinPeerDeclaration {
        logical: function.name.clone(),
        abi,
        symbol: align_codegen_llvm::imported_program_symbol(&function.name),
        linkage: ThinFunctionLinkage::Root,
    })
}

fn collect_static_function_targets(
    data: &align_mir::StaticData,
    targets: &mut std::collections::BTreeSet<align_mir::ProgramCall>,
) {
    for relocation in &data.relocations {
        match &relocation.target {
            align_mir::StaticDataTarget::Function(target) => {
                targets.insert(target.clone());
            }
            align_mir::StaticDataTarget::Record(record) => {
                collect_static_function_targets(record, targets);
            }
            align_mir::StaticDataTarget::Bytes { .. } => {}
        }
    }
}

fn referenced_program_calls(
    function: &align_mir::Function,
) -> std::collections::BTreeSet<align_mir::ProgramCall> {
    use align_mir::{DirectCall, Rvalue, Stmt};
    let mut targets = std::collections::BTreeSet::new();
    for block in &function.blocks {
        for statement in &block.stmts {
            let Stmt::Let(_, value) = statement else {
                continue;
            };
            match value {
                Rvalue::Call(DirectCall::Program(target), _) => {
                    targets.insert(target.clone());
                }
                Rvalue::CallWithCleanup(call) => {
                    targets.insert(call.target.clone());
                }
                Rvalue::SqliteCallbackDescriptor(descriptor) => {
                    targets.insert(descriptor.target.clone());
                }
                Rvalue::FnAddr { target, .. } => {
                    targets.insert(target.clone());
                }
                Rvalue::Closure { lifted, .. } => {
                    targets.insert(lifted.clone());
                }
                Rvalue::ParMapParallel { func, stages, .. } => {
                    targets.insert(func.clone());
                    targets.extend(stages.iter().filter_map(|stage| stage.func.clone()));
                }
                Rvalue::ParMapReduce { func, .. } => {
                    targets.insert(func.clone());
                }
                Rvalue::StaticData(data) => collect_static_function_targets(data, &mut targets),
                _ => {}
            }
        }
    }
    targets
}

fn thin_view_hash(view: &PartitionCodegenView<'_>) -> Hash128 {
    let rendered = format!("align-thin-partition-impl-v2\n{view:?}");
    Hash128::of(rendered.as_bytes())
}

/// Form every function/support partition in import-DAG order, validating the complete inventory
/// before any artifact stage, cache access, LLVM operation, or linker process can begin.
pub fn function_partitions<'a>(
    units: &'a [PerUnitArtifact],
    exports: &[String],
) -> Result<Vec<ThinPartition<'a>>, String> {
    if units.is_empty() {
        return Ok(Vec::new());
    }
    let entries = units
        .iter()
        .enumerate()
        .filter(|(_, unit)| unit.is_entry)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if entries.len() != 1 {
        return Err(format!("ThinLTO entry unit count invalid:{}", entries.len()));
    }
    let entry = entries[0];
    let mut seen_units = std::collections::BTreeSet::new();
    let mut per_unit_functions = Vec::with_capacity(units.len());

    // Function identity/ABI and complete callable validation precede export and support errors.
    for (index, unit) in units.iter().enumerate() {
        if unit.unit.is_empty() || unit.unit.as_bytes().contains(&0) {
            return Err(thin_identity_error(
                "ThinLTO unit identity invalid",
                unit.unit.as_bytes(),
            ));
        }
        if !seen_units.insert(unit.unit.as_str()) {
            return Err(thin_identity_error(
                "duplicate ThinLTO unit identity",
                unit.unit.as_bytes(),
            ));
        }
        let unit_exports = if index == entry { exports } else { &[] };
        let mut functions = unit.mir.fns.iter().collect::<Vec<_>>();
        functions.sort_by(|left, right| left.name.as_bytes().cmp(right.name.as_bytes()));
        for pair in functions.windows(2) {
            if pair[0].name == pair[1].name {
                return Err(thin_identity_error(
                    "duplicate ThinLTO function identity",
                    pair[0].name.as_bytes(),
                ));
            }
        }
        for function in &functions {
            let _ = thin_function_abi(&unit.mir, function)?;
        }
        align_codegen_llvm::validate_thin_partition_program(&unit.mir, unit_exports)
            .map_err(|error| format!("ThinLTO program validation failed for `{}`: {error}", unit.unit))?;
        per_unit_functions.push(functions);
    }

    let mut unknown = unknown_exports(&units[entry].mir, exports)
        .into_iter()
        .map(str::as_bytes)
        .collect::<Vec<_>>();
    unknown.sort_unstable();
    unknown.dedup();
    if !unknown.is_empty() {
        let mut message = format!("unknown ThinLTO export roots:{}", unknown.len());
        for name in unknown {
            message.push(':');
            message.push_str(&name.len().to_string());
            message.push(':');
            message.push_str(&thin_hex(name));
        }
        return Err(message);
    }

    let mut partitions = Vec::new();
    let mut root_symbols = std::collections::BTreeSet::new();
    for (index, unit) in units.iter().enumerate() {
        let unit_exports = if index == entry { exports } else { &[] };
        let functions = &per_unit_functions[index];
        let local = functions
            .iter()
            .map(|function| (function.name.clone(), *function))
            .collect::<std::collections::BTreeMap<_, _>>();

        let mut support_by_symbol = std::collections::BTreeMap::new();
        for resource in &unit.mir.resources {
            let owner = if resource.declaring_module == unit.unit {
                let hook = if let Some(function) = functions
                    .iter()
                    .copied()
                    .find(|function| function.name.as_str() == resource.drop_hook)
                {
                    thin_peer(&unit.unit, &unit.mir, function, unit_exports)?
                } else if let Some(function) = unit
                    .mir
                    .imported_fns
                    .iter()
                    .find(|function| function.name.as_str() == resource.drop_hook)
                {
                    thin_imported_peer(&unit.mir, function)?
                } else {
                    return Err(thin_identity_error(
                        "ThinLTO resource Drop hook missing",
                        resource.drop_hook.as_bytes(),
                    ));
                };
                SupportThunkOwner::Owned { hook }
            } else {
                // Consumer units need only the public Drop-thunk symbol; the private hook is
                // intentionally absent from their interface declarations.
                SupportThunkOwner::Imported
            };
            let record = SupportThunkRecord {
                drop_thunk: resource.drop_thunk.clone(),
                representation_version: resource.representation_version,
                drop_abi_fingerprint: resource.drop_abi_fingerprint,
                owner,
            };
            if let Some(previous) = support_by_symbol.insert(record.drop_thunk.clone(), record.clone())
                && previous != record
            {
                return Err(thin_identity_error(
                    "ThinLTO support thunk conflict",
                    record.drop_thunk.as_bytes(),
                ));
            }
        }
        let support = support_by_symbol.into_values().collect::<Vec<_>>();
        if support
            .iter()
            .any(|record| matches!(record.owner, SupportThunkOwner::Owned { .. }))
        {
            let view = PartitionCodegenView::Support { thunks: support };
            let impl_hash = thin_view_hash(&view);
            partitions.push(ThinPartition {
                unit: &unit.unit,
                view,
                impl_hash,
                preserve_symbols: Vec::new(),
            });
        }

        let shared = PartitionSharedCodegenView::from_program(&unit.mir);
        for selected in functions {
            let definition = thin_peer(&unit.unit, &unit.mir, selected, unit_exports)?;
            if definition.linkage == ThinFunctionLinkage::Root
                && !root_symbols.insert(definition.symbol.clone())
            {
                return Err(thin_identity_error(
                    "duplicate ThinLTO root symbol",
                    definition.symbol.as_bytes(),
                ));
            }
            let mut targets = referenced_program_calls(selected);
            targets.extend(unit.mir.sqlite_callback_effects.keys().cloned());
            targets.remove(&selected.name);
            let mut peers = Vec::new();
            let mut peer_functions = Vec::new();
            for target in targets {
                if let Some(function) = local.get(&target) {
                    peers.push(thin_peer(&unit.unit, &unit.mir, function, unit_exports)?);
                    peer_functions.push(*function);
                }
            }
            let mut preserve_symbols = Vec::new();
            if definition.linkage == ThinFunctionLinkage::Root {
                preserve_symbols.push(definition.symbol.clone());
            }
            if selected.name.as_str() == "main" {
                preserve_symbols.push("main".to_owned());
            }
            preserve_symbols.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
            preserve_symbols.dedup();
            let view = PartitionCodegenView::Function {
                selected,
                definition,
                peers,
                peer_functions,
                shared: shared.clone(),
            };
            let impl_hash = thin_view_hash(&view);
            partitions.push(ThinPartition {
                unit: &unit.unit,
                view,
                impl_hash,
                preserve_symbols,
            });
        }
    }
    Ok(partitions)
}

/// **ThinLTO S2 (`--thin-lto`): the cache-composing, parallel cross-unit-optimizing build.** Runs the
/// three phases for a multi-unit program (`units.len() >= 2`), producing one object per unit into
/// `obj_paths` (the caller links them exactly as the non-ThinLTO path does), and returns a
/// [`CacheOutcome`] per phase per unit (prelink outcomes first, then backend outcomes — the model the
/// `--cache-stats` gates assert).
///
/// Two cacheable phases + one serial global step (`docs/impl/07-roadmap.md` ThinLTO S2):
///  1. **prelink** (parallel, cacheable as [`CacheStage::ThinLtoPrelink`]) — per unit, look up
///     [`build_prelink_key`]; a hit materializes the summary-bearing `.bc` from the CAS, a miss runs
///     `emit_prelink_bc` (the shim's pre-link pipeline + summary) into `staging` and publishes it.
///     Every unit's `.bc` is on disk afterwards (thin-link + backends read the full set).
///  2. **thin-link** (serial, NEVER cached) — one shim call over all `.bc` computes the fresh
///     per-unit import edges + the global export set, so a dep private-body edit is always reflected.
///  3. **backend** (parallel, cacheable as [`CacheStage::ThinLtoBackend`]) — per unit, look up
///     [`build_backend_key`] (own prelink digest ⊕ inbound imports ⊕ outbound exports ⊕ import-source
///     prelink digests ⊕ backend/target bits); a hit materializes the object, a miss runs the shim's
///     import+optimize+emit and publishes.
///
/// Determinism: prelink `.bc` and backend objects are produced by independent single-threaded LLVM
/// operations (fresh `Context` / `TargetMachine` per unit), so `-j N` is byte-identical to `-j 1`;
/// the serial thin-link between the passes computes order-independent decisions (edges sorted).
/// Preserve set (fail-closed v1) = `{main}` ∪ every unit's exported symbols ∪ any `--export` roots.
///
/// Failure policy: any prelink/thin-link/backend shim failure → a loud `Err` naming the phase + unit;
/// the caller ABORTS (never a silent fallback, since the user asked for `--thin-lto`). A cache-WRITE
/// failure never fails the build (the artifact on disk is already correct).
///
/// **Structural invariant (do not reorder): the `--rt-lto` runtime-bitcode merge MUST happen inside
/// phase-1 prelink emission (`emit_prelink_bc` consumes `rt_lto_bytes(rt_lto)`), BEFORE the prelink
/// `.bc` is hashed/cached.** The backend key (phase 3) pins `--rt-lto` only transitively, via each
/// unit's `own_prelink_digest` (+ import-source prelink digests). Moving the merge after prelink
/// caching would let an rt-off and an rt-on build share a prelink digest → a stale backend hit could
/// serve an object built against the wrong runtime bodies. The prelink KEY also carries `rt_lto` +
/// `rt_lto_digest` explicitly, so the two never even share a prelink cache entry.
pub struct ThinLtoBuild {
    /// The two-phase per-unit cache outcomes (`ThinLtoPrelink` then `ThinLtoBackend`), one pair per
    /// unit — the same vector the pre-`ThinLtoBuild` API returned.
    pub outcomes: Vec<CacheOutcome>,
    /// The per-unit summary-bearing prelink `.bc` path this build wrote (in `staging`), indexed by DAG
    /// unit order. Exposed so callers (tests, tooling) do not re-derive the private staging naming.
    pub prelink_bc: Vec<std::path::PathBuf>,
}

#[allow(clippy::too_many_arguments)]
pub fn build_thin_lto(
    units: &[PerUnitArtifact],
    obj_paths: &[std::path::PathBuf],
    cache: &CacheContext,
    target: &BuildTarget,
    profile: Profile,
    exports: &[String],
    rt_lto: bool,
    staging: &std::path::Path,
    jobs: usize,
) -> Result<ThinLtoBuild, String> {
    assert_eq!(units.len(), obj_paths.len(), "one object path per unit");
    assert!(units.len() >= 2, "N=1 must skip ThinLTO entirely (caller's responsibility)");
    align_codegen_llvm::ensure_target_initialized().map_err(|e| e.to_string())?;

    let enabled = cache.codegen_is_enabled();
    let identity = if enabled { Some(resolve_thin_identity(target)?) } else { None };
    let n = units.len();
    let ids: Vec<String> = units.iter().map(|u| u.unit.clone()).collect();
    let unit_index: std::collections::HashMap<&str, usize> =
        ids.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
    let bc_paths: Vec<std::path::PathBuf> =
        (0..n).map(|i| staging.join(format!("unit{i}.prelink.bc"))).collect();
    let unit_exports = |i: usize| -> &[String] {
        if units[i].is_entry {
            exports
        } else {
            &[]
        }
    };

    // ---- Phase 1: prelink (parallel over misses) --------------------------------------------------
    let mut prelink_keys: Vec<Option<cache::PrelinkKey>> = (0..n).map(|_| None).collect();
    let mut prelink_outcomes: Vec<Option<CacheOutcome>> = (0..n).map(|_| None).collect();
    let mut prelink_misses: Vec<usize> = Vec::new();
    for i in 0..n {
        if let Some(id) = &identity {
            let key = build_prelink_key(
                &ids[i],
                cache::PartitionKey::WholeUnit,
                units[i].summary.impl_hash,
                &units[i].dep_interface_hashes,
                unit_exports(i),
                id,
                profile,
                rt_lto,
            );
            match cache.lookup_prelink(&key, &bc_paths[i]) {
                CacheLookup::Hit(o) => prelink_outcomes[i] = Some(o),
                CacheLookup::Miss { reason } => {
                    prelink_outcomes[i] = Some(CacheOutcome {
                        stage: CacheStage::ThinLtoPrelink,
                        unit: ids[i].clone(),
                        hit: false,
                        miss_reason: reason,
                    });
                    prelink_misses.push(i);
                }
            }
            prelink_keys[i] = Some(key);
        } else {
            prelink_outcomes[i] = Some(CacheOutcome {
                stage: CacheStage::ThinLtoPrelink,
                unit: ids[i].clone(),
                hit: false,
                miss_reason: None,
            });
            prelink_misses.push(i);
        }
    }
    run_thin_phase(&prelink_misses, jobs, |i| {
        align_codegen_llvm::emit_prelink_bc(
            &units[i].mir,
            &bc_paths[i],
            target,
            profile,
            unit_exports(i),
            rt_lto_bytes(rt_lto),
            &ids[i],
        )
        .map_err(|e| format!("ThinLTO prelink failed for unit `{}`: {e}", units[i].unit))?;
        if let Some(key) = &prelink_keys[i] {
            cache.publish_prelink(key, &bc_paths[i]);
        }
        Ok(())
    })?;

    // Every unit's prelink `.bc` is now on disk (hit or produced). Its content digest — hashed
    // uniformly from disk so a hit (cached blob) and a miss (fresh build) yield the same value —
    // feeds each importer's backend key.
    let mut prelink_digests: Vec<Hash128> = Vec::with_capacity(n);
    for path in &bc_paths {
        let bytes = std::fs::read(path)
            .map_err(|e| format!("ThinLTO: cannot read prelink bitcode {}: {e}", path.display()))?;
        prelink_digests.push(Hash128::of(&bytes));
    }

    // ---- Phase 2: thin-link (serial, never cached) ------------------------------------------------
    let mut preserve: Vec<String> = vec!["main".to_string()];
    for (i, unit) in units.iter().enumerate() {
        for s in align_codegen_llvm::thinlto::exported_symbols(&unit.mir, unit_exports(i)) {
            if !preserve.contains(&s) {
                preserve.push(s);
            }
        }
    }
    let plan = align_codegen_llvm::thinlto::thin_link(&bc_paths, &ids, &preserve)
        .map_err(|e| format!("ThinLTO thin-link failed: {e}"))?;

    // Per-unit slices of the thin-link decision set (kept for both the backend key and the shim call).
    let inbound: Vec<Vec<align_codegen_llvm::thinlto::ImportEdge>> =
        (0..n).map(|i| plan.imports.iter().filter(|e| e.dest == ids[i]).cloned().collect()).collect();

    // ---- Phase 3: backend (parallel over misses) --------------------------------------------------
    let mut backend_keys: Vec<Option<cache::BackendKey>> = (0..n).map(|_| None).collect();
    let mut backend_outcomes: Vec<Option<CacheOutcome>> = (0..n).map(|_| None).collect();
    let mut backend_misses: Vec<usize> = Vec::new();
    for i in 0..n {
        if let Some(id) = &identity {
            let inbound_key: Vec<cache::InboundImport> = inbound[i]
                .iter()
                .map(|edge| cache::InboundImport {
                    source: cache::ThinPartitionSource {
                        unit: edge.src.clone(),
                        partition: cache::PartitionKey::WholeUnit,
                    },
                    guid: edge.guid,
                    is_definition: edge.is_definition,
                })
                .collect();
            let outbound: Vec<u64> =
                plan.exports.iter().filter(|e| e.module == ids[i]).map(|e| e.guid).collect();
            let src_digests: Vec<cache::ImportSourceDigest> = {
                let mut srcs: Vec<&str> = inbound[i].iter().map(|e| e.src.as_str()).collect();
                srcs.sort_unstable();
                srcs.dedup();
                srcs.iter()
                    .filter_map(|source_unit| {
                        unit_index.get(source_unit).map(|&j| cache::ImportSourceDigest {
                            source: cache::ThinPartitionSource {
                                unit: (*source_unit).to_string(),
                                partition: cache::PartitionKey::WholeUnit,
                            },
                            prelink_digest: prelink_digests[j],
                        })
                    })
                    .collect()
            };
            let key = build_backend_key(
                &ids[i],
                cache::PartitionKey::WholeUnit,
                unit_exports(i),
                id,
                profile,
                prelink_digests[i],
                inbound_key,
                outbound,
                src_digests,
            );
            match cache.lookup_backend(&key, &obj_paths[i]) {
                CacheLookup::Hit(o) => backend_outcomes[i] = Some(o),
                CacheLookup::Miss { reason } => {
                    backend_outcomes[i] = Some(CacheOutcome {
                        stage: CacheStage::ThinLtoBackend,
                        unit: ids[i].clone(),
                        hit: false,
                        miss_reason: reason,
                    });
                    backend_misses.push(i);
                }
            }
            backend_keys[i] = Some(key);
        } else {
            backend_outcomes[i] = Some(CacheOutcome {
                stage: CacheStage::ThinLtoBackend,
                unit: ids[i].clone(),
                hit: false,
                miss_reason: None,
            });
            backend_misses.push(i);
        }
    }
    run_thin_phase(&backend_misses, jobs, |i| {
        align_codegen_llvm::thinlto::backend(
            &bc_paths,
            &ids,
            i,
            &preserve,
            &inbound[i],
            &plan.exports,
            target,
            profile,
            &obj_paths[i],
        )
        .map_err(|e| format!("ThinLTO backend failed for unit `{}`: {e}", units[i].unit))?;
        if let Some(key) = &backend_keys[i] {
            cache.publish_backend(key, &obj_paths[i]);
        }
        Ok(())
    })?;

    let mut outcomes: Vec<CacheOutcome> = Vec::with_capacity(2 * n);
    outcomes.extend(prelink_outcomes.into_iter().map(|o| o.expect("prelink outcome per unit")));
    outcomes.extend(backend_outcomes.into_iter().map(|o| o.expect("backend outcome per unit")));
    Ok(ThinLtoBuild { outcomes, prelink_bc: bc_paths })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FunctionThinLtoMode {
    WholeUnit,
    Partitioned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FunctionThinLtoObservation {
    WholeUnit {
        source: ThinPartitionSource,
        codegen: CacheOutcome,
    },
    Partitioned {
        source: ThinPartitionSource,
        prelink_digest: Hash128,
        prelink: CacheOutcome,
        backend: CacheOutcome,
    },
}

/// A sealed function-ThinLTO build. Artifact paths and link configuration remain private; the two
/// completion methods consume the only owner and therefore make publication single-use.
pub struct FunctionThinLtoBuild {
    mode: FunctionThinLtoMode,
    observations: Vec<FunctionThinLtoObservation>,
    prelink_bc: Vec<std::path::PathBuf>,
    objects: Vec<std::path::PathBuf>,
    profile: Profile,
    link_libs: Vec<String>,
    object_stage: ArtifactStage,
}

impl FunctionThinLtoBuild {
    pub fn mode(&self) -> FunctionThinLtoMode {
        self.mode
    }

    pub fn observations(&self) -> &[FunctionThinLtoObservation] {
        &self.observations
    }

    fn validate_topology(&self) -> Result<(), String> {
        if self.objects.is_empty() || self.prelink_bc.len() > self.objects.len() {
            return Err("ThinLTO result topology invalid".to_owned());
        }
        match self.mode {
            FunctionThinLtoMode::WholeUnit
                if matches!(
                    self.observations.as_slice(),
                    [FunctionThinLtoObservation::WholeUnit { source, codegen }]
                        if source.partition == PartitionKey::WholeUnit
                            && codegen.stage == CacheStage::Codegen
                            && codegen.unit == source.unit
                ) => {}
            FunctionThinLtoMode::Partitioned
                if self.observations.len() == self.objects.len()
                    && self.observations.iter().all(|observation| matches!(
                        observation,
                        FunctionThinLtoObservation::Partitioned {
                            source,
                            prelink,
                            backend,
                            ..
                        } if source.partition != PartitionKey::WholeUnit
                            && prelink.stage == CacheStage::ThinLtoPrelink
                            && backend.stage == CacheStage::ThinLtoBackend
                            && prelink.unit == source.unit
                            && backend.unit == source.unit
                    )) => {}
            _ => return Err("ThinLTO result topology invalid".to_owned()),
        }
        Ok(())
    }

    fn response_argument(&self) -> Result<std::path::PathBuf, String> {
        use std::io::Write as _;
        use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};

        self.validate_topology()?;
        let response = self.object_stage.path().join("objects.rsp");
        let mut file = std::fs::File::create(&response)
            .map_err(|error| format!("cannot create ThinLTO response file: {error}"))?;
        let encoded = encode_thin_response_records(&self.objects)?;
        file.write_all(&encoded)
            .map_err(|error| format!("cannot write ThinLTO response file: {error}"))?;
        file.flush()
            .map_err(|error| format!("cannot flush ThinLTO response file: {error}"))?;
        drop(file);

        let mut argument = Vec::with_capacity(1 + response.as_os_str().as_bytes().len());
        argument.push(b'@');
        argument.extend_from_slice(response.as_os_str().as_bytes());
        Ok(std::path::PathBuf::from(std::ffi::OsString::from_vec(argument)))
    }

    pub fn link_and_publish(self, exe: &std::path::Path) -> Result<(), String> {
        let response = self.response_argument()?;
        let parent = exe
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let publication = ArtifactStage::in_dir(parent, "align-publish")
            .map_err(|error| format!("cannot create executable staging directory: {error}"))?;
        let staged = publication.path().join(
            exe.file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("program")),
        );
        link_objects(&[response.as_path()], &staged, &self.link_libs, self.profile)?;
        std::fs::rename(&staged, exe)
            .map_err(|error| format!("cannot publish executable {}: {error}", exe.display()))
    }

    pub fn link_and_publish_with_output(
        self,
        exe: &std::path::Path,
        sink: &mut dyn LinkOutputSink,
    ) -> Result<(), String> {
        let response = self.response_argument()?;
        link_objects_with_output(
            &[response.as_path()],
            exe,
            &self.link_libs,
            self.profile,
            sink,
        )
    }
}

fn encode_thin_response_records(objects: &[std::path::PathBuf]) -> Result<Vec<u8>, String> {
    use std::os::unix::ffi::OsStrExt as _;
    let capacity = objects
        .iter()
        .map(|object| object.as_os_str().as_bytes().len().saturating_add(3))
        .sum();
    let mut encoded = Vec::with_capacity(capacity);
    for object in objects {
        let bytes = object.as_os_str().as_bytes();
        if bytes.iter().any(|byte| matches!(byte, 0 | b'\n' | b'\r')) {
            return Err(thin_identity_error(
                "cannot encode ThinLTO object path",
                bytes,
            ));
        }
        encoded.push(b'\"');
        for byte in bytes {
            if matches!(*byte, b'\"' | b'\\') {
                encoded.push(b'\\');
            }
            encoded.push(*byte);
        }
        encoded.extend_from_slice(b"\"\n");
    }
    Ok(encoded)
}

fn thin_link_lib_union(units: &[PerUnitArtifact]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut libraries = Vec::new();
    for unit in units {
        for library in &unit.mir.link_libs {
            if seen.insert(library.as_str()) {
                libraries.push(library.clone());
            }
        }
    }
    libraries
}

fn validate_thin_stage_parent() -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt as _;
    let temp = std::env::temp_dir();
    let bytes = temp.as_os_str().as_bytes();
    if bytes.iter().any(|byte| matches!(byte, 0 | b'\n' | b'\r')) {
        return Err(thin_identity_error(
            "cannot encode ThinLTO object path",
            bytes,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod function_thin_unit_tests {
    use super::{ArtifactStage, Profile, encode_thin_response_records, link_objects};
    use std::io::Write as _;
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::ffi::OsStringExt as _;
    use std::path::PathBuf;

    #[test]
    fn response_records_preserve_native_bytes_and_escape_only_quote_and_backslash() {
        let paths = vec![
            PathBuf::from(std::ffi::OsString::from_vec(b"plain".to_vec())),
            PathBuf::from(std::ffi::OsString::from_vec(
                b"a b\t\"\\\xff".to_vec(),
            )),
        ];
        assert_eq!(
            encode_thin_response_records(&paths).unwrap(),
            b"\"plain\"\n\"a b\t\\\"\\\\\xff\"\n"
        );
        assert_eq!(encode_thin_response_records(&[]).unwrap(), b"");
    }

    #[test]
    fn response_records_reject_line_and_nul_bytes_with_exact_identity() {
        for bytes in [b"a\nb".as_slice(), b"a\rb".as_slice(), b"a\0b".as_slice()] {
            let path = PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()));
            assert_eq!(
                encode_thin_response_records(&[path]).unwrap_err(),
                format!(
                    "cannot encode ThinLTO object path:{}:{}",
                    bytes.len(),
                    super::thin_hex(bytes)
                )
            );
        }
    }

    #[test]
    fn selected_cc_driver_round_trips_quoted_native_response_paths() {
        if std::process::Command::new("cc").arg("--version").output().is_err() {
            return;
        }
        let stage = ArtifactStage::temp("align-response-roundtrip").expect("response stage");
        let main_source = stage.path().join("main.c");
        let empty_source = stage.path().join("empty.c");
        let main_object = stage.path().join("main.o");
        let empty_object = stage.path().join("empty.o");
        std::fs::write(&main_source, b"int main(void) { return 0; }\n").unwrap();
        std::fs::write(&empty_source, b"static int unused;\n").unwrap();
        for (source, object) in [
            (&main_source, &main_object),
            (&empty_source, &empty_object),
        ] {
            let status = std::process::Command::new("cc")
                .arg("-c")
                .arg(source)
                .arg("-o")
                .arg(object)
                .status()
                .expect("launch cc compile");
            assert!(status.success(), "cc failed to compile response fixture");
        }

        let odd_names = [
            b"space name.o".as_slice(),
            b"tab\tname.o".as_slice(),
            b"quote\"name.o".as_slice(),
            b"back\\slash.o".as_slice(),
            b"invalid-\xff.o".as_slice(),
            b"@leading-at.o".as_slice(),
            b"-option-shaped.o".as_slice(),
        ];
        let mut objects = vec![main_object];
        for name in odd_names {
            let path = stage
                .path()
                .join(std::ffi::OsString::from_vec(name.to_vec()));
            std::fs::copy(&empty_object, &path).expect("copy response fixture");
            objects.push(path);
        }
        let response = stage.path().join("objects.rsp");
        let mut response_file = std::fs::File::create(&response).unwrap();
        response_file
            .write_all(&encode_thin_response_records(&objects).unwrap())
            .unwrap();
        response_file.flush().unwrap();
        drop(response_file);

        let mut argument = vec![b'@'];
        argument.extend_from_slice(response.as_os_str().as_bytes());
        let response_argument = PathBuf::from(std::ffi::OsString::from_vec(argument));
        let executable = stage.path().join("response-executable");
        link_objects(
            &[response_argument.as_path()],
            &executable,
            &[],
            Profile::Release,
        )
        .expect("selected cc/linker response round trip");
        assert!(
            std::process::Command::new(&executable)
                .status()
                .expect("run response executable")
                .success()
        );
    }
}

/// Build the settled function-partitioned ThinLTO path and retain sole ownership of every private
/// artifact until one consuming link/publication method completes.
#[allow(clippy::too_many_arguments)]
pub fn build_function_thin_lto(
    units: &[PerUnitArtifact],
    cache: &CacheContext,
    target: &BuildTarget,
    profile: Profile,
    exports: &[String],
    rt_lto: bool,
    jobs: usize,
) -> Result<FunctionThinLtoBuild, String> {
    let partitions = function_partitions(units, exports)?;
    if partitions.is_empty() {
        return Err("ThinLTO partition inventory is empty".to_owned());
    }
    validate_thin_stage_parent()?;
    for partition in &partitions {
        let id = partition.stable_id();
        if id.as_bytes().contains(&0) {
            return Err(thin_identity_error(
                "ThinLTO module identity invalid",
                id.as_bytes(),
            ));
        }
    }
    let object_stage = ArtifactStage::temp("align-function-thin")
        .map_err(|error| format!("cannot create object staging directory: {error}"))?;
    let link_libs = thin_link_lib_union(units);

    if units.len() == 1
        && partitions.len() == 1
        && matches!(partitions[0].key(), PartitionKey::Function(_))
    {
        let object = object_stage.path().join("partition0.o");
        let outcome = emit_object_cached(
            cache,
            &units[0].unit,
            units[0].summary.impl_hash,
            &units[0].dep_interface_hashes,
            &units[0].mir,
            &object,
            target.clone(),
            profile,
            exports,
            rt_lto,
        )?;
        let build = FunctionThinLtoBuild {
            mode: FunctionThinLtoMode::WholeUnit,
            observations: vec![FunctionThinLtoObservation::WholeUnit {
                source: ThinPartitionSource {
                    unit: units[0].unit.clone(),
                    partition: PartitionKey::WholeUnit,
                },
                codegen: outcome,
            }],
            prelink_bc: Vec::new(),
            objects: vec![object],
            profile,
            link_libs,
            object_stage,
        };
        build.validate_topology()?;
        return Ok(build);
    }

    align_codegen_llvm::ensure_target_initialized().map_err(|error| error.to_string())?;
    let enabled = cache.codegen_is_enabled();
    let identity = if enabled {
        Some(resolve_thin_identity(target)?)
    } else {
        None
    };
    let count = partitions.len();
    let ids = partitions
        .iter()
        .map(ThinPartition::stable_id)
        .collect::<Vec<_>>();
    let mut id_index = std::collections::BTreeMap::new();
    for (index, id) in ids.iter().enumerate() {
        if id_index.insert(id.as_str(), index).is_some() {
            return Err(thin_identity_error(
                "duplicate ThinLTO module identity",
                id.as_bytes(),
            ));
        }
    }
    let unit_index = units
        .iter()
        .map(|unit| (unit.unit.as_str(), unit))
        .collect::<std::collections::BTreeMap<_, _>>();
    let bc_paths = (0..count)
        .map(|index| object_stage.path().join(format!("partition{index}.prelink.bc")))
        .collect::<Vec<_>>();
    let objects = (0..count)
        .map(|index| object_stage.path().join(format!("partition{index}.o")))
        .collect::<Vec<_>>();

    let mut prelink_keys = (0..count).map(|_| None).collect::<Vec<_>>();
    let mut prelink_outcomes = (0..count).map(|_| None).collect::<Vec<_>>();
    let mut prelink_misses = Vec::new();
    for index in 0..count {
        let partition = &partitions[index];
        let unit = unit_index
            .get(partition.unit)
            .copied()
            .ok_or_else(|| thin_identity_error("ThinLTO partition unit missing", partition.unit.as_bytes()))?;
        let unit_exports = if unit.is_entry { exports } else { &[] };
        if let Some(identity) = &identity {
            let key = build_prelink_key(
                partition.unit,
                partition.key(),
                partition.impl_hash,
                &unit.dep_interface_hashes,
                unit_exports,
                identity,
                profile,
                rt_lto,
            );
            match cache.lookup_prelink(&key, &bc_paths[index]) {
                CacheLookup::Hit(outcome) => prelink_outcomes[index] = Some(outcome),
                CacheLookup::Miss { reason } => {
                    prelink_outcomes[index] = Some(CacheOutcome {
                        stage: CacheStage::ThinLtoPrelink,
                        unit: partition.unit.to_owned(),
                        hit: false,
                        miss_reason: reason,
                    });
                    prelink_misses.push(index);
                }
            }
            prelink_keys[index] = Some(key);
        } else {
            prelink_outcomes[index] = Some(CacheOutcome {
                stage: CacheStage::ThinLtoPrelink,
                unit: partition.unit.to_owned(),
                hit: false,
                miss_reason: None,
            });
            prelink_misses.push(index);
        }
    }
    run_thin_phase(&prelink_misses, jobs, |index| {
        let partition = &partitions[index];
        let result = match &partition.view {
            PartitionCodegenView::Function { .. } => align_codegen_llvm::emit_function_prelink_bc(
                &partition.view,
                &bc_paths[index],
                target,
                profile,
                rt_lto_bytes(rt_lto),
                &ids[index],
            ),
            PartitionCodegenView::Support { .. } => align_codegen_llvm::emit_support_prelink_bc(
                &partition.view,
                &bc_paths[index],
                target,
                profile,
                &ids[index],
            ),
        };
        result.map_err(|error| {
            format!("ThinLTO prelink failed for partition `{}`: {error}", partition.label())
        })?;
        if let Some(key) = &prelink_keys[index] {
            cache.publish_prelink(key, &bc_paths[index]);
        }
        Ok(())
    })?;

    let mut prelink_digests = Vec::with_capacity(count);
    for path in &bc_paths {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("ThinLTO: cannot read prelink bitcode: {error}"))?;
        prelink_digests.push(Hash128::of(&bytes));
    }

    let mut preserve = partitions
        .iter()
        .flat_map(|partition| partition.preserve_symbols.iter().cloned())
        .collect::<Vec<_>>();
    preserve.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    preserve.dedup();
    let plan = align_codegen_llvm::thinlto::thin_link(&bc_paths, &ids, &preserve)
        .map_err(|error| format!("ThinLTO thin-link failed: {error}"))?;

    let mut inbound = (0..count).map(|_| Vec::new()).collect::<Vec<_>>();
    let mut seen_edges = std::collections::BTreeSet::new();
    for edge in &plan.imports {
        let Some(&destination) = id_index.get(edge.dest.as_str()) else {
            return Err(thin_identity_error(
                "ThinLTO edge destination unknown",
                edge.dest.as_bytes(),
            ));
        };
        if !id_index.contains_key(edge.src.as_str()) {
            return Err(thin_identity_error(
                "ThinLTO edge source unknown",
                edge.src.as_bytes(),
            ));
        }
        if !seen_edges.insert((
            edge.src.as_str(),
            edge.dest.as_str(),
            edge.guid,
            edge.is_definition,
        )) {
            return Err(format!(
                "ThinLTO edge duplicated:{}:{}:{}:{}:{}:{}",
                edge.src.len(),
                thin_hex(edge.src.as_bytes()),
                edge.dest.len(),
                thin_hex(edge.dest.as_bytes()),
                edge.guid,
                u8::from(edge.is_definition),
            ));
        }
        inbound[destination].push(edge.clone());
    }
    for export in &plan.exports {
        if !id_index.contains_key(export.module.as_str()) {
            return Err(thin_identity_error(
                "ThinLTO edge source unknown",
                export.module.as_bytes(),
            ));
        }
    }

    let mut backend_keys = (0..count).map(|_| None).collect::<Vec<_>>();
    let mut backend_outcomes = (0..count).map(|_| None).collect::<Vec<_>>();
    let mut backend_misses = Vec::new();
    for index in 0..count {
        let partition = &partitions[index];
        let unit = unit_index
            .get(partition.unit)
            .copied()
            .ok_or_else(|| thin_identity_error("ThinLTO partition unit missing", partition.unit.as_bytes()))?;
        let unit_exports = if unit.is_entry { exports } else { &[] };
        if let Some(identity) = &identity {
            let inbound_key = inbound[index]
                .iter()
                .map(|edge| {
                    let source = id_index.get(edge.src.as_str()).copied().ok_or_else(|| {
                        thin_identity_error("ThinLTO edge source unknown", edge.src.as_bytes())
                    })?;
                    Ok(InboundImport {
                        source: partitions[source].source(),
                        guid: edge.guid,
                        is_definition: edge.is_definition,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let outbound = plan
                .exports
                .iter()
                .filter(|export| export.module == ids[index])
                .map(|export| export.guid)
                .collect::<Vec<_>>();
            let mut sources = inbound[index]
                .iter()
                .map(|edge| edge.src.as_str())
                .collect::<Vec<_>>();
            sources.sort_unstable();
            sources.dedup();
            let source_digests = sources
                .into_iter()
                .map(|source| {
                    let source_index = id_index.get(source).copied().ok_or_else(|| {
                        thin_identity_error("ThinLTO source digest missing", source.as_bytes())
                    })?;
                    Ok(ImportSourceDigest {
                        source: partitions[source_index].source(),
                        prelink_digest: prelink_digests[source_index],
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let key = build_backend_key(
                partition.unit,
                partition.key(),
                unit_exports,
                identity,
                profile,
                prelink_digests[index],
                inbound_key,
                outbound,
                source_digests,
            );
            match cache.lookup_backend(&key, &objects[index]) {
                CacheLookup::Hit(outcome) => backend_outcomes[index] = Some(outcome),
                CacheLookup::Miss { reason } => {
                    backend_outcomes[index] = Some(CacheOutcome {
                        stage: CacheStage::ThinLtoBackend,
                        unit: partition.unit.to_owned(),
                        hit: false,
                        miss_reason: reason,
                    });
                    backend_misses.push(index);
                }
            }
            backend_keys[index] = Some(key);
        } else {
            backend_outcomes[index] = Some(CacheOutcome {
                stage: CacheStage::ThinLtoBackend,
                unit: partition.unit.to_owned(),
                hit: false,
                miss_reason: None,
            });
            backend_misses.push(index);
        }
    }
    run_thin_phase(&backend_misses, jobs, |index| {
        align_codegen_llvm::thinlto::backend(
            &bc_paths,
            &ids,
            index,
            &preserve,
            &inbound[index],
            &plan.exports,
            target,
            profile,
            &objects[index],
        )
        .map_err(|error| {
            format!(
                "ThinLTO backend failed for partition `{}`: {error}",
                partitions[index].label()
            )
        })?;
        if let Some(key) = &backend_keys[index] {
            cache.publish_backend(key, &objects[index]);
        }
        Ok(())
    })?;

    let observations = (0..count)
        .map(|index| {
            Ok(FunctionThinLtoObservation::Partitioned {
                source: partitions[index].source(),
                prelink_digest: prelink_digests[index],
                prelink: prelink_outcomes[index].take().ok_or_else(|| {
                    format!(
                        "internal error: ThinLTO prelink outcome missing for partition `{}`",
                        partitions[index].label()
                    )
                })?,
                backend: backend_outcomes[index].take().ok_or_else(|| {
                    format!(
                        "internal error: ThinLTO backend outcome missing for partition `{}`",
                        partitions[index].label()
                    )
                })?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let build = FunctionThinLtoBuild {
        mode: FunctionThinLtoMode::Partitioned,
        observations,
        prelink_bc: bc_paths,
        objects,
        profile,
        link_libs,
        object_stage,
    };
    build.validate_topology()?;
    Ok(build)
}

/// Run a ThinLTO phase's MISSES in parallel via the shared atomic-claim pattern (mirrors
/// [`codegen_units_parallel`]): `jobs` workers pull the next miss index, run `produce`, and stop
/// claiming new work once any unit has errored (an in-progress `produce` is never interrupted). The
/// reported error is the lowest DAG index among those collected. Empty misses ⇒ a no-op.
fn run_thin_phase<F>(misses: &[usize], jobs: usize, produce: F) -> Result<(), String>
where
    F: Fn(usize) -> Result<(), String> + Sync,
{
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    if misses.is_empty() {
        return Ok(());
    }
    let worker_count = jobs.max(1).min(misses.len());
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let errors = std::sync::Mutex::new(Vec::<(usize, String)>::new());
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| loop {
                    if failed.load(Ordering::Relaxed) {
                        break;
                    }
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    if k >= misses.len() {
                        break;
                    }
                    let i = misses[k];
                    if let Err(e) = produce(i) {
                        errors.lock().expect("thin phase error lock").push((i, e));
                        failed.store(true, Ordering::Relaxed);
                    }
            });
        }
    });
    let mut errs = errors.into_inner().expect("thin phase error lock");
    if !errs.is_empty() {
        errs.sort_by_key(|(i, _)| *i);
        return Err(errs.remove(0).1);
    }
    Ok(())
}

/// MIR to LLVM IR text (`alignc emit-llvm`). `optimized` picks the lens: `false` (`--stage raw`)
/// prints what codegen emitted; `true` (`--stage optimized`) runs the `-O2` pipeline first, so the
/// output shows what LLVM actually did (inlined, fused, vectorized). `exports` is the same
/// export-roots list as [`emit_object_file`].
pub fn emit_llvm_ir(mir: &align_mir::Program, target: BuildTarget, optimized: bool, exports: &[String], rt_lto: bool) -> Result<String, String> {
    align_codegen_llvm::emit_llvm_ir(mir, &target, optimized, exports, rt_lto_bytes(rt_lto)).map_err(|e| e.to_string())
}

/// The names in `exports` that do not match any function in `mir` (by [`align_mir::Function::name`]).
/// Empty ⇒ every requested export root resolves. The fail-closed seam for `--export <name>`: an
/// unknown name must be a hard, listed error (`alignc: unknown export(s): …`), never a silent no-op
/// (a typo'd export name would otherwise compile a wrong object with no diagnostic at all).
pub fn unknown_exports<'a>(mir: &align_mir::Program, exports: &'a [String]) -> Vec<&'a str> {
    exports
        .iter()
        .filter(|name| !mir.fns.iter().any(|f| f.name.as_str() == name.as_str()))
        .map(String::as_str)
        .collect()
}

/// Link an object into an executable. Uses the system C compiler (`cc`); crt0 calls
/// the generated `main` as the entry point (`docs/impl/01-pipeline.md`: driver links).
///
/// The thin runtime (`libalign_runtime.a`, e.g. the builtin `print`) is linked in too. Being a
/// Rust staticlib, it needs the usual std support libraries (`pthread`/`dl`/`m` on ELF; on Mach-O
/// they are libSystem re-exports — see [`support_libs`]).
pub fn link_executable(obj: &std::path::Path, exe: &std::path::Path, link_libs: &[String], profile: Profile) -> Result<(), String> {
    link_objects(&[obj], exe, link_libs, profile)
}

/// Return the link-library list with an ordered supported libpq closure tail.
///
/// MIR and per-unit capability unions intentionally preserve first-seen order for deterministic
/// identity. That order can put a `crypto` request from an unrelated module before `pq`, however;
/// a static ELF linker has already scanned that archive by the time libpq introduces its symbols.
/// When `pq` is present, preserve the complete original list and append one dependent-first
/// closure tail. Repetition is intentional: a library after `pq` may itself introduce OpenSSL or
/// compression references, so the tail must be after every such suffix library. Programs without
/// `pq` retain their existing order and therefore keep the established `extern "C" link(...)`
/// behavior.
pub fn order_link_libs(link_libs: &[String]) -> Vec<String> {
    const LIBPQ_CLOSURE: [&str; 5] = ["pq", "ssl", "crypto", "zstd", "z"];
    if !link_libs.iter().any(|library| library == "pq") {
        return link_libs.to_vec();
    }
    let mut ordered = link_libs.to_vec();
    for library in LIBPQ_CLOSURE {
        if link_libs.iter().any(|candidate| candidate == library) {
            ordered.push(library.to_string());
        }
    }
    ordered
}

/// Link one or more object files (plus the Align runtime and the always-linked C libraries) into an
/// executable. The single-object [`link_executable`] is the common case; multiple objects are used
/// by the FFI tests that link an Align object against a compiled C-helper object (a by-value struct
/// callee), and by any future multi-translation-unit build.
pub fn link_objects(objs: &[&std::path::Path], exe: &std::path::Path, link_libs: &[String], profile: Profile) -> Result<(), String> {
    link_objects_inner(objs, exe, link_libs, profile, None)
}

/// [`link_objects`] for an instrument-PGO (`--pgo-instrument`) build: additionally links the clang
/// profile runtime archive (`profile_rt`, from [`profile_runtime_archive`]) and forces the
/// `__llvm_profile_runtime` anchor undefined so the archive's atexit `.profraw` writer is pulled in.
/// On ELF the instrumented object carries NO reference to the runtime (LLVM relies on the
/// `__start/__stop___llvm_prf_*` section brackets), so WITHOUT the forced-undefined symbol the link
/// succeeds silently and no profile is ever written — exactly the flag clang's own driver injects
/// (measured at PGO S0, `docs/impl/07-roadmap.md`).
pub fn link_objects_instrumented(
    objs: &[&std::path::Path],
    exe: &std::path::Path,
    link_libs: &[String],
    profile: Profile,
    profile_rt: &std::path::Path,
) -> Result<(), String> {
    link_objects_inner(objs, exe, link_libs, profile, Some(profile_rt))
}

/// Everything one link is made of — the complete input to [`link_command_args`].
///
/// A struct rather than a parameter list because the argv is order-sensitive in several
/// independent ways (objects before the runtime archive, the profile runtime after both, `-l`
/// last), and a named field at every call site is what keeps a caller from silently transposing
/// two `&Path`s that the type system cannot tell apart.
pub struct LinkPlan<'a> {
    /// The Align object files, in the order they are given to `cc`.
    pub objs: &'a [&'a std::path::Path],
    /// The executable to produce (`-o`).
    pub exe: &'a std::path::Path,
    /// `libalign_runtime.a`, linked after the objects that reference it.
    pub runtime: &'a std::path::Path,
    /// The capability + user libraries, already normalized by [`order_link_libs`].
    pub ordered_link_libs: &'a [String],
    /// Which flag dialect to speak.
    pub format: ObjectFormat,
    /// Decides the in-link strip on ELF.
    pub profile: Profile,
    /// The clang profile runtime archive, for `--pgo-instrument` only.
    pub profile_rt: Option<&'a std::path::Path>,
    /// Which linker `cc` should run.
    pub linker: &'a Linker,
}

/// The complete `cc` argument vector for one link, as a **pure function** of [`LinkPlan`].
///
/// Split out of [`link_objects_inner`] so what the driver asks the linker for is checkable without
/// running a linker, inspecting a produced image, or having any particular library installed. The
/// `capability_linking` owner asserts the gated `-l` set directly on this vector; the dynamic
/// dependency list of a real binary then remains a *corroborating* check rather than the only
/// proof, which matters because `--as-needed` precision differs between linkers and can make an
/// over-linked library invisible in `DT_NEEDED`.
pub fn link_command_args(plan: &LinkPlan<'_>) -> Vec<std::ffi::OsString> {
    let &LinkPlan { objs, exe, runtime, ordered_link_libs, format, profile, profile_rt, linker } = plan;
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    // Linker selection first: `-B`/`-fuse-ld=` are `cc` driver options, position-independent with
    // respect to the inputs, and reading them first makes the argv self-describing.
    args.extend(linker.cc_flags().into_iter().map(std::ffi::OsString::from));
    args.extend(objs.iter().map(|obj| obj.as_os_str().to_os_string()));
    args.push(runtime.as_os_str().to_os_string());
    args.push("-o".into());
    args.push(exe.as_os_str().to_os_string());
    // Link hygiene (M13 Slice 2), spelled per object format by `hygiene_flags`. Dead-code
    // removal (ELF `--gc-sections` / Mach-O `-dead_strip`) drops every unreferenced input
    // section from the final image; combined with the runtime's per-function sections (Rust's
    // default) this garbage-collects the `std.compress`/`std.crypto`/`std.http` code a program
    // does not use, eliminating its `libz`/`libzstd`/`libcrypto`/`libssl` references so those
    // libraries are not needed at all. Unused-dylib removal (ELF `--as-needed` / Mach-O
    // `-dead_strip_dylibs`) then records a dependency (`DT_NEEDED` / `LC_LOAD_DYLIB`) only for
    // libraries that actually satisfy a surviving reference. Both are correctness-neutral
    // hygiene, kept for EVERY profile (M13 Slice 4) — even `dev`: the potential link-speed
    // saving of dropping dead-code removal is not worth a second link-flag path, and a `dev`
    // binary that silently links dead `libssl` etc. would be a surprising difference from
    // `release`.
    args.extend(hygiene_flags(format).iter().map(std::ffi::OsString::from));
    // Per-profile strip (M13 Slice 4). The size profiles (`small`/`tiny`) drop the whole symbol
    // table; the speed profiles (`dev`/`release`/`fast`) keep symbols so a crash backtrace / `perf`
    // stays useful. The strip *decision* is owned by `Profile::strip` alone; only the *spelling*
    // is per-format: ELF strips in the link (`-Wl,--strip-all`), Mach-O has no ld64 equivalent and
    // runs the external `strip` after a successful link (in `link_objects_inner`).
    if profile.strip() && format == ObjectFormat::Elf {
        args.push("-Wl,--strip-all".into());
    }
    // The always-linked support libraries, per format (`support_libs`): on ELF,
    // `libpthread`/`libdl`/`libm` are Rust-std support libraries the runtime *core* may reference
    // (threads, dlopen, math) independent of any Align feature — NOT capability-gated. On Mach-O
    // all three are libSystem re-exports, so the list is empty.
    args.extend(support_libs(format).iter().map(std::ffi::OsString::from));
    // Instrument-PGO (`--pgo-instrument`): append the clang profile runtime archive and force the
    // `__llvm_profile_runtime` anchor undefined so its atexit `.profraw` writer is pulled from the
    // archive (see [`link_objects_instrumented`]). Placed AFTER the objects/archive that (indirectly)
    // need it — `-l`/archive resolution is left-to-right — and the forced-undefined symbol is spelled
    // per object format (ELF `--undefined=SYM`; Mach-O `-u,_SYM`, its symbols carry a leading `_`).
    if let Some(profile_rt) = profile_rt {
        args.push(profile_rt.as_os_str().to_os_string());
        args.push(
            match format {
                ObjectFormat::Elf => "-Wl,--undefined=__llvm_profile_runtime",
                ObjectFormat::MachO => "-Wl,-u,___llvm_profile_runtime",
            }
            .into(),
        );
    }
    // Capability + user libraries. `libz`/`libzstd`/`libcrypto`/`libssl` are NO LONGER linked
    // unconditionally: they now arrive through `link_libs`, which MIR populates from the builtins a
    // program actually uses (`align_mir::Capability`) plus any `extern "C" link("name")` the user
    // declared (validated in sema). All go AFTER the objects/archive that reference them (`-l`
    // resolves left-to-right against preceding inputs). The supported libpq closure is normalized
    // by `order_link_libs` before this call. Each name is a single `-l<name>` argv (no
    // shell/flag injection). A program using no gated feature links none of z/zstd/crypto/ssl.
    args.extend(ordered_link_libs.iter().map(|lib| std::ffi::OsString::from(format!("-l{lib}"))));
    args
}

fn link_objects_inner(objs: &[&std::path::Path], exe: &std::path::Path, link_libs: &[String], profile: Profile, profile_rt: Option<&std::path::Path>) -> Result<(), String> {
    let format = target_object_format()?;
    let runtime = runtime_archive()?;
    let ordered_link_libs = order_link_libs(link_libs);
    // Which linker `cc` drives (build-perf track item 2, `docs/impl/21-build-perf-plan.md`). ELF
    // only, and optimization-neutral: `ld.lld` produces an equally optimized image, just faster.
    // Resolved before any argv is built so a requested-but-missing lld fails before the link starts.
    let linker = select_linker(format)?;
    let mut cmd = std::process::Command::new("cc");
    cmd.args(link_command_args(&LinkPlan {
        objs,
        exe,
        runtime: &runtime,
        ordered_link_libs: &ordered_link_libs,
        format,
        profile,
        profile_rt,
        linker: &linker,
    }));
    let status = cmd
        .status()
        .map_err(|e| format!("cannot launch cc: {e}"))?;
    if !status.success() {
        return Err(link_failure_message(status.code(), &ordered_link_libs, &linker));
    }
    // Mach-O strip: ld64 has no `--strip-all`, so the size profiles run the external `strip` on the
    // linked image. `strip` ships with the same Xcode CLT as the `cc`/`ld` above (the existing
    // implicit toolchain dependency), and it re-signs the stripped binary ad hoc, so the result
    // stays runnable. A launch failure or nonzero exit is a hard error, same as a failed link — the
    // profile's contract (all symbols removed) must never be broken silently.
    if profile.strip() && format == ObjectFormat::MachO {
        let strip_status = std::process::Command::new("strip")
            .arg(exe)
            .status()
            .map_err(|e| format!("cannot launch strip: {e}"))?;
        if !strip_status.success() {
            return Err(format!("strip failed (exit code {:?})", strip_status.code()));
        }
    }
    Ok(())
}

/// The link-hygiene flags for `format` (see the call site in [`link_objects`] for what they do).
/// A data table, same shape as `Profile::pipeline` — the *meaning* is format-independent, only the
/// spelling differs.
fn hygiene_flags(format: ObjectFormat) -> &'static [&'static str] {
    match format {
        // Dead-section removal + record only the shared libraries that resolve a reference.
        ObjectFormat::Elf => &["-Wl,--gc-sections", "-Wl,--as-needed"],
        ObjectFormat::MachO => &["-Wl,-dead_strip", "-Wl,-dead_strip_dylibs"],
    }
}

/// The always-linked support libraries for `format` (see the call site in [`link_objects`]).
/// Mach-O has none: `pthread`/`dl`/`m` are all libSystem re-exports there, so naming them is noise.
fn support_libs(format: ObjectFormat) -> &'static [&'static str] {
    match format {
        ObjectFormat::Elf => &["-lpthread", "-ldl", "-lm"],
        ObjectFormat::MachO => &[],
    }
}

/// The environment override for linker selection (see [`select_linker`]). One variable, two values,
/// no other knob: `system` pins the C driver's own default linker, `lld` demands `ld.lld` and fails
/// the link when it cannot be found. Unset is the automatic policy. Named for the `ALIGNC_*` family
/// the other driver knobs use (`ALIGNC_CACHE`, `ALIGNC_JOBS`).
const LINKER_ENV: &str = "ALIGNC_LINKER";

/// The `-fuse-ld=` name and `-B` file name of LLVM's ELF linker. GCC's `collect2` and Clang both
/// resolve `-fuse-ld=lld` by looking for a program spelled exactly `ld.lld`; a version-suffixed
/// `ld.lld-22` is NOT usable through that mechanism, which is why discovery below looks for a
/// *directory containing this exact name* rather than for any lld binary.
const LLD_EXE: &str = "ld.lld";

/// Which linker the C driver (`cc`) is told to run for a link ([`link_command_args`]).
///
/// Build-perf track item 2 (`docs/impl/21-build-perf-plan.md`): `ld.lld` is substantially faster
/// than GNU `ld` on ELF and ships inside the LLVM 22 toolchain `alignc` already requires, so it
/// needs no new dependency. `mold` was rejected: it is an extra third-party install and has no
/// macOS support at all. The choice is **optimization-neutral** — the same objects, the same
/// hygiene flags, the same fully optimized code (the track's "output is always fully optimized"
/// principle is about optimization level, and the linker changes none of it). The image is not
/// byte-identical: two linkers lay out and prune differently, and lld's `--as-needed` is the more
/// precise of the two.
///
/// Public so the driver binary and the `capability_linking` owner can build a link argv for a
/// stated linker without consulting the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Linker {
    /// The C driver's own default: GNU `ld`/`ld.gold` on ELF, `ld64`/`ld-prime` on Mach-O. The
    /// behavior every Align build had before item 2, and still the behavior on macOS.
    System,
    /// LLVM's `ld.lld`, held as the *directory* that contains it (see [`Linker::cc_flags`]).
    Lld(std::path::PathBuf),
}

impl Linker {
    /// The `cc` driver flags that select this linker.
    ///
    /// `-fuse-ld=lld` alone is not enough and is actively unsafe: with GCC it makes `collect2`
    /// search `COMPILER_PATH` and `PATH` for `ld.lld`, and when only apt's version-suffixed
    /// `ld.lld-22` is installed the link does not fall back — it dies with
    /// `collect2: fatal error: cannot find 'ld'`. `-B<dir>` puts the resolved LLVM `bin` directory
    /// on `COMPILER_PATH`, so both GCC and Clang resolve the exact binary discovery picked.
    /// (`--ld-path=<abs>` would be more direct but is a Clang-only option; `cc` is GCC on the
    /// Debian/Ubuntu hosts this targets, and it rejects the flag outright.)
    ///
    /// `-B<dir>` also prepends `<dir>` to the startfile and library search paths. That is harmless
    /// *because discovery cannot name any other kind of directory*: both surviving search steps
    /// resolve an LLVM toolchain `bin` directory, which holds no `crt*.o` and no `lib*`. It stays
    /// visible in the link command rather than mutating the child's environment behind the
    /// caller's back (Nothing hidden).
    fn cc_flags(&self) -> Vec<String> {
        match self {
            Linker::System => Vec::new(),
            Linker::Lld(dir) => vec![format!("-B{}", dir.display()), "-fuse-ld=lld".to_string()],
        }
    }

    /// How this linker is named in a diagnostic ([`link_failure_message`]).
    fn describe(&self) -> String {
        match self {
            Linker::System => "system default".to_string(),
            Linker::Lld(dir) => format!("lld ({})", dir.join(LLD_EXE).display()),
        }
    }
}

/// Pick the linker for an object format, honoring the [`LINKER_ENV`] override.
///
/// Policy, in one place:
///
///  - **Mach-O is never touched.** Apple's linker (`ld-prime`, Xcode 15+) is already fast, and a
///    second linker on macOS would be a behavior difference for no measured gain. An explicit
///    `ALIGNC_LINKER=lld` there is a hard error, not a silent no-op — an explicit request must
///    never be quietly ignored.
///  - **ELF, unset:** use `ld.lld` when [`lld_bin_dir`] finds one, else the system linker. This
///    fail-open default is safe because both linkers produce a valid, equally optimized image: a
///    host without lld links exactly as it did before, at the old speed, with nothing to act on.
///  - **ELF, `lld`:** fail closed when no `ld.lld` is found. This is the knob CI sets so a missing
///    lld is a red build rather than a silent regression to the slow linker.
///  - **`system`:** the escape hatch, valid on every format.
///
/// Determinism: for one host and one `alignc` binary the answer is always the same — the search
/// order is total, depends on no ambient `PATH`, and is memoized per process.
fn select_linker(format: ObjectFormat) -> Result<Linker, String> {
    select_linker_with(format, std::env::var_os(LINKER_ENV).as_deref().map(|v| v.to_string_lossy().into_owned()), lld_bin_dir)
}

/// [`select_linker`] with the environment request and the discovery step as parameters (unit
/// testable without mutating process state or depending on what the host has installed).
///
/// The `format` arms are spelled out rather than defaulted, so adding an object format is a
/// compile error here instead of a silent inheritance of the Mach-O answer.
fn select_linker_with(
    format: ObjectFormat,
    request: Option<String>,
    discover: impl FnOnce() -> Option<std::path::PathBuf>,
) -> Result<Linker, String> {
    match (request.as_deref(), format) {
        (Some("system"), _) => Ok(Linker::System),
        (Some("lld"), ObjectFormat::Elf) => discover().map(Linker::Lld).ok_or_else(|| {
            format!(
                "{LINKER_ENV}=lld was requested but no `{LLD_EXE}` was found\n\
                 note: searched the compile-time $LLVM_SYS_221_PREFIX/bin, then `llvm-config --bindir`\n\
                 note: on Debian/Ubuntu install the lld-{} package, or unset {LINKER_ENV} to \
                 link with the system linker",
                align_codegen_llvm::LLVM_TOOL_VERSION
            )
        }),
        (Some("lld"), ObjectFormat::MachO) => Err(format!(
            "{LINKER_ENV}=lld is only supported for ELF targets; this build targets Mach-O, \
             whose linking always uses the Apple toolchain linker"
        )),
        (Some(other), _) => Err(format!("{LINKER_ENV} must be `system` or `lld` (got `{other}`)")),
        // Automatic: ELF prefers lld when present, Mach-O always keeps the system linker.
        (None, ObjectFormat::Elf) => Ok(discover().map_or(Linker::System, Linker::Lld)),
        (None, ObjectFormat::MachO) => Ok(Linker::System),
    }
}

/// The directory holding an `ld.lld` for this compiler's LLVM, memoized for the process.
///
/// Memoized because step 2 spawns `llvm-config`, while a multi-object or multi-run build asks
/// repeatedly and the answer cannot change mid-process. The *override* is deliberately NOT
/// memoized — only this filesystem probe is — so a caller that sets [`LINKER_ENV`] between links
/// still gets what it asked for.
///
/// **`PATH` is never searched**, deliberately. A `PATH` fallback would make the project's linker a
/// function of ambient environment (a conda/nix/toolbox shim, or a relative entry resolved against
/// whatever directory the build runs in) and would let `-B` name a directory that is not an LLVM
/// `bin` at all — exactly the assumption [`Linker::cc_flags`] relies on for that flag to be
/// harmless. Both steps here resolve an LLVM installation this compiler is version-matched to.
fn lld_bin_dir() -> Option<std::path::PathBuf> {
    static DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        resolve_lld_dir(lld_dir_from_prefix(option_env!("LLVM_SYS_221_PREFIX")), lld_dir_from_llvm_config)
    })
    .clone()
}

/// Compose the two discovery steps: the compile-time prefix wins outright, and `llvm-config` is
/// consulted only when it misses. Split out from [`lld_bin_dir`] so the *order* is testable without
/// depending on what the host has installed.
fn resolve_lld_dir(
    from_prefix: Option<std::path::PathBuf>,
    from_llvm_config: impl FnOnce() -> Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    from_prefix.or_else(from_llvm_config)
}

/// Search step 1: `<prefix>/bin/ld.lld` for the build-time LLVM prefix — the same compile-time
/// `LLVM_SYS_221_PREFIX` seam [`llvm_tool`] uses, so the linker version always matches the LLVM the
/// compiler was built against. A stale baked-in path (prefix moved since the build) falls through.
fn lld_dir_from_prefix(prefix: Option<&str>) -> Option<std::path::PathBuf> {
    let dir = std::path::Path::new(prefix?).join("bin");
    is_executable_file(&dir.join(LLD_EXE)).then_some(dir)
}

/// Search step 2: `llvm-config --bindir`. Covers an installed `alignc` whose baked prefix is gone,
/// and the apt.llvm.org layout where `ld.lld` lives in `/usr/lib/llvm-22/bin` and only the suffixed
/// `ld.lld-22` is on `PATH`. The versioned tool name is tried first so a host with several LLVMs
/// resolves the one this compiler matches.
fn lld_dir_from_llvm_config() -> Option<std::path::PathBuf> {
    let versioned = format!("llvm-config-{}", align_codegen_llvm::LLVM_TOOL_VERSION);
    for tool in [versioned.as_str(), "llvm-config"] {
        let Ok(out) = std::process::Command::new(tool).arg("--bindir").output() else { continue };
        if !out.status.success() {
            continue;
        }
        let bindir = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if bindir.is_empty() {
            continue;
        }
        let dir = std::path::PathBuf::from(bindir);
        if is_executable_file(&dir.join(LLD_EXE)) {
            return Some(dir);
        }
    }
    None
}

/// Whether `path` is a regular file the OS would actually run.
///
/// Mere existence is not enough: selecting a present-but-unrunnable `ld.lld` would make *every*
/// link on that host fail, turning the fail-open default into a fail-everything default. On Unix
/// this checks the file type plus any execute bit — the cheap `stat`-only approximation of `X_OK`;
/// it can still admit a file this user may not execute, which then fails the link loudly with the
/// linker named, but it rules out the realistic case of a non-executable leftover.
fn is_executable_file(path: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The gated capability libraries (`align_mir::Capability`): the ones a system commonly does NOT
/// ship in the default linker search path, so a link failure involving them gets a `LIBRARY_PATH`
/// hint appended ([`link_failure_message`]).
const GATED_LIBS: [&str; 4] = ["z", "zstd", "crypto", "ssl"];

/// The link-failure error. Always names the linker that ran: the selection is automatic, so a
/// failure that only a particular linker produces is otherwise invisible, and the reader needs to
/// know whether to retry with `ALIGNC_LINKER=system`. When the failed link involved a gated
/// capability library, append a note about non-default library prefixes: those libraries often live
/// outside the default search path (e.g. Homebrew keg-only OpenSSL on macOS), and the fix is the
/// standard `LIBRARY_PATH` mechanism. The driver never injects search paths itself — what is
/// linked, and from where, stays visible (Nothing hidden).
fn link_failure_message(code: Option<i32>, link_libs: &[String], linker: &Linker) -> String {
    let mut msg = format!("link failed (cc exit code {code:?}, linker: {})", linker.describe());
    if link_libs.iter().any(|l| GATED_LIBS.contains(&l.as_str())) {
        msg.push_str(
            "\nnote: libraries in a non-default prefix (e.g. Homebrew keg-only) are found via \
             LIBRARY_PATH, e.g. LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/openssl@3/lib",
        );
    }
    if matches!(linker, Linker::Lld(_)) {
        msg.push_str(&format!(
            "\nnote: this link used lld; set {LINKER_ENV}=system to link with the system linker instead"
        ));
    }
    msg
}

/// Locate the LLVM binutils replacement `name` (`llvm-readobj`, `llvm-nm`) with the version
/// matching the LLVM this compiler is built against. Used by `alignc size` and the link-inspection
/// tests. Search order:
///
///  1. `$LLVM_SYS_221_PREFIX/bin/<name>` — the build-time LLVM prefix (compile-time env, the same
///     variable llvm-sys builds from), so the tool version always matches the linked LLVM. A stale
///     baked-in path (prefix moved since the build) falls through.
///  2. `<name>-22` on `PATH` (apt.llvm.org naming; the suffix is
///     [`align_codegen_llvm::LLVM_TOOL_VERSION`]).
///  3. Plain `<name>` on `PATH`.
///
/// `None` when nothing is found — callers degrade the affected report section to a note.
pub fn llvm_tool(name: &str) -> Option<std::path::PathBuf> {
    llvm_tool_in(option_env!("LLVM_SYS_221_PREFIX"), name)
}

/// [`llvm_tool`] with the LLVM prefix as a parameter (unit-testable without rebaking the
/// compile-time env).
fn llvm_tool_in(prefix: Option<&str>, name: &str) -> Option<std::path::PathBuf> {
    if let Some(prefix) = prefix {
        let p = std::path::Path::new(prefix).join("bin").join(name);
        if p.exists() {
            return Some(p);
        }
    }
    let versioned = format!("{name}-{}", align_codegen_llvm::LLVM_TOOL_VERSION);
    for cand in [versioned.as_str(), name] {
        // Minimal PATH probe: does `<cand> --version` launch and exit 0?
        let found = std::process::Command::new(cand)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if found {
            return Some(std::path::PathBuf::from(cand));
        }
    }
    None
}

/// The in-tree `align_runtime` source directory, baked in at build time (relative to this
/// crate's manifest). Present only when `alignc` runs from inside the workspace; an installed
/// binary has no source tree, so the staleness check below simply no-ops there.
const RUNTIME_SRC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../align_runtime/src");

/// Locate `libalign_runtime.a`, built by `cargo build` alongside the `alignc` binary.
/// The integration tests run from `target/<profile>/deps/`, so the parent is checked too.
fn runtime_archive() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find current exe: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_string())?;
    for cand in [dir.join("libalign_runtime.a"), dir.join("../libalign_runtime.a")] {
        if cand.exists() {
            ensure_archive_fresh(&cand)?;
            return Ok(cand);
        }
    }
    Err(format!(
        "cannot find libalign_runtime.a near {}; run `cargo build` first",
        dir.display()
    ))
}

/// The shared seed for the runtime-source content digest (M15 S3b). MUST equal
/// `RUNTIME_SRC_DIGEST_SEED` in `build.rs` — the digest baked there is compared here, so the two
/// algorithms + seed must agree (pinned by [`tests::runtime_src_digest_matches_baked`]).
const RUNTIME_SRC_DIGEST_SEED: u64 = 0x616C_6967_6E5F_7274; // "align_rt"

/// The runtime-source content digest baked at build time (`build.rs` → `cargo:rustc-env`), the
/// staleness reference. Empty only if the source tree was absent when `alignc` was built.
const BAKED_RUNTIME_SRC_DIGEST: &str = env!("ALIGN_RUNTIME_SRC_DIGEST");

/// A deterministic, **mtime-independent** content digest of every `*.rs` file under `dir` (recursive):
/// relative paths sorted, then each `(rel_path, len, bytes)` folded into one buffer and wyhashed.
/// `None` if the tree is absent/unreadable. Mirrors the identical routine in `build.rs` (same seed +
/// canonical form), so a digest computed here at link time is comparable to the one baked at build.
pub fn runtime_src_digest(dir: &std::path::Path) -> Option<String> {
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).ok()?;
        for entry in entries.flatten() {
            // `file_type()` (from the iterator, no extra `stat`) does not follow symlinks, so a
            // symlinked dir is not traversed (no cycles / no escaping the tree).
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() && path.extension().is_some_and(|x| x == "rs") {
                let rel = path.strip_prefix(dir).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                files.push((rel, path));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut buf: Vec<u8> = Vec::new();
    for (rel, path) in &files {
        let bytes = std::fs::read(path).ok()?;
        buf.extend_from_slice(rel.as_bytes());
        buf.push(0);
        buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        buf.extend_from_slice(&bytes);
    }
    Some(format!("{:016x}", align_hash::wyhash(&buf, RUNTIME_SRC_DIGEST_SEED)))
}

/// The content digest of the runtime archive bytes (`docs/impl/10-cache-first-optimization.md` §6.3):
/// a future link-cache-key input, and the identity a shared/cross-host cache would key link actions on.
/// Not yet folded into any key (link caching is a later slice); exposed + tested now so the identity
/// exists. A `touch` of the archive leaves this unchanged (content-addressed); rebuilt bytes change it.
pub fn runtime_archive_digest() -> Result<Hash128, String> {
    let archive = runtime_archive()?;
    let bytes = std::fs::read(&archive).map_err(|e| format!("cannot read {}: {e}", archive.display()))?;
    Ok(Hash128::of(&bytes))
}

/// Fail loudly if `libalign_runtime.a` does not correspond to the current `align_runtime` source.
///
/// `align_driver` has no cargo dependency edge to the runtime *staticlib*, and a unit-test build
/// (`cargo test -p align_runtime`) recompiles only the test harness — neither refreshes the `.a`. So
/// editing the runtime and re-running the driver/tests without a full `cargo build` would silently
/// link a *stale* archive: wrong behavior and baffling test failures.
///
/// M15 S3b switched the check from **mtime** to a **content digest**: the current runtime-source digest
/// is compared to the one baked into this `alignc` at build time (`build.rs`). Since `alignc` and the
/// `.a` are produced by the same `cargo build`, a match means the `.a` is current — regardless of file
/// mtimes. This kills the false-stale papercut (a `git checkout`/`touch` bumps source mtimes without
/// changing content, which the old mtime check flagged as stale) while keeping the teeth: a real
/// source edit changes the digest and fails loud until `cargo build` refreshes both.
///
/// No-ops when the source tree is absent (an installed `alignc`), unreadable, or when no digest was
/// baked — it only ever turns a definitely-stale link into an error, never blocks a legitimate one.
fn ensure_archive_fresh(_archive: &std::path::Path) -> Result<(), String> {
    let src = std::path::Path::new(RUNTIME_SRC_DIR);
    if !src.is_dir() || BAKED_RUNTIME_SRC_DIGEST.is_empty() {
        return Ok(()); // installed binary / no baked reference: nothing to compare against
    }
    let Some(current) = runtime_src_digest(src) else {
        return Ok(()); // cannot read the source tree: do not block the build
    };
    if current != BAKED_RUNTIME_SRC_DIGEST {
        return Err(format!(
            "libalign_runtime.a is stale: the content of {} differs from what this `alignc` was \
             built against.\nThe driver has no cargo edge to the runtime staticlib, so run \
             `cargo build` to refresh the archive before linking.",
            src.display(),
        ));
    }
    Ok(())
}

/// Format diagnostics for humans (one per line, `file:line:col: severity: message`).
pub fn format_diagnostics(source_map: &SourceMap, diags: &Diagnostics) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for d in diags.iter() {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        if let Some(span) = d.span {
            let f = source_map.get(span.file);
            let (line, col) = f.line_col(span.lo);
            let _ = writeln!(out, "{}:{}:{}: {}: {}", f.name, line, col, sev, d.message);
        } else {
            let _ = writeln!(out, "{}: {}", sev, d.message);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_descriptor_set_installs_no_static_callback_imports() {
        // #731 regression owner: a unit with no static descriptors must not receive the
        // pkg.db.internal callback import set (pre-fix, every no-db unit's IR grew the full
        // extern family and broke CLI/library emit-llvm parity).
        let mut mir = align_mir::Program::default();
        install_static_descriptor_data(&mut mir, None, &[], &[]).expect("empty install");
        assert!(
            mir.imported_fns.is_empty(),
            "no-descriptor install must leave imported_fns untouched: {:?}",
            mir.imported_fns.iter().map(|f| &f.name).collect::<Vec<_>>(),
        );
        assert!(mir.fns.is_empty(), "no-descriptor install must add no functions");
    }

    #[test]
    fn descriptor_rows_resource_names_cover_whole_and_reconstructed_nominals() {
        let row = align_sema::hir::StructDef {
            name: "app.user_groups$Row".to_string(),
            source_name: "app.user_groups$Row".to_string(),
            fields: Vec::new(),
            align: None,
            c_repr: false,
        };
        assert_eq!(
            rows_resource_names_for_descriptor(&row),
            [
                "pkg.db$rows$S19_app.user_groups$Row",
                "pkg.db$rows$S19_app_user_groups_Row",
            ]
        );
    }

    #[test]
    fn postgres_parameter_types_match_only_the_exact_generated_shape() {
        let admitted = [
            (GeneratedValueKind::Bool, &["bool"][..]),
            (GeneratedValueKind::I16, &["int2"][..]),
            (GeneratedValueKind::I32, &["int4"][..]),
            (GeneratedValueKind::I64, &["int8"][..]),
            (GeneratedValueKind::F32, &["float4"][..]),
            (GeneratedValueKind::F64, &["float8"][..]),
            (GeneratedValueKind::Text, &["text", "varchar", "name"][..]),
            (GeneratedValueKind::Bytes, &["bytea"][..]),
        ];
        let canonical = [
            "bool", "int2", "int4", "int8", "float4", "float8", "text", "varchar", "name",
            "bytea",
        ];
        for (kind, accepted) in admitted {
            for candidate in canonical {
                assert_eq!(
                    postgres_parameter_type_matches(kind, candidate),
                    accepted.contains(&candidate),
                    "{kind:?} / {candidate} must follow the exact PostgreSQL mapping",
                );
            }
            assert!(
                !postgres_parameter_type_matches(kind, "numeric"),
                "deferred PostgreSQL types must stay unavailable for {kind:?}",
            );
        }
    }

    // ---- rt-LTO profile default (settled 2026-08-09) -----------------------------------------------
    // The CLI resolves an absent --rt-lto/--no-rt-lto flag through this exact mapping; pin all five
    // profiles so a new profile variant must decide its default here explicitly.
    #[test]
    fn rt_lto_defaults_on_for_optimizing_profiles_only() {
        assert!(default_rt_lto(Profile::Release));
        assert!(default_rt_lto(Profile::Fast));
        assert!(!default_rt_lto(Profile::Dev));
        assert!(!default_rt_lto(Profile::Small));
        assert!(!default_rt_lto(Profile::Tiny));
    }

    // ---- fix #1 mechanism gate: the profdata snapshot ----------------------------------------------
    // The cache-poisoning fix routes libLLVM to a private snapshot, not the user's live path. These
    // assert the plumbing directly (an integration test cannot time a mid-build file rewrite):
    //   * the staged path is NOT the user path (so a later user rewrite can't reach the emit), and
    //   * the staged bytes are IDENTICAL to what was digested (key↔content coupling), and
    //   * the snapshot is RAII-cleaned on drop.
    #[test]
    fn staged_profdata_is_a_distinct_copy_of_the_exact_bytes_and_self_cleans() {
        let bytes = b"MERGED-PROFDATA-BYTES-v1".to_vec();
        let user_path = std::env::temp_dir().join(format!("align-pgo-user-{}.profdata", std::process::id()));
        std::fs::write(&user_path, &bytes).unwrap();

        let staged_file;
        {
            let staged = StagedProfdata::new(&bytes).expect("stage");
            // Distinct from the user path — a rewrite of `user_path` can never reach libLLVM.
            assert_ne!(staged.path(), user_path.as_path(), "staged copy must not be the user path");
            // Byte-identical to the digested input — the exact bytes `PgoKey::Use(digest)` keys.
            assert_eq!(std::fs::read(staged.path()).unwrap(), bytes, "staged bytes must equal the digested bytes");
            assert_eq!(Hash128::of(&std::fs::read(staged.path()).unwrap()), Hash128::of(&bytes));
            staged_file = staged.path().to_path_buf();
            assert!(staged_file.exists());
        }
        // RAII: dropped snapshot leaves nothing behind.
        assert!(!staged_file.exists(), "the staged snapshot is removed on drop");

        // Two snapshots of the same bytes get distinct private paths (per-invocation uniqueness).
        let a = StagedProfdata::new(&bytes).expect("stage a");
        let b = StagedProfdata::new(&bytes).expect("stage b");
        assert_ne!(a.path(), b.path(), "each invocation stages to a unique path");

        let _ = std::fs::remove_file(&user_path);
    }

    #[test]
    fn runtime_src_digest_is_content_based_and_mtime_independent() {
        let root = std::env::temp_dir().join(format!("align-driver-srcdigest-{}-{:p}", std::process::id(), &0u8 as *const _));
        let sub = root.join("nested");
        std::fs::create_dir_all(&sub).expect("create temp tree");

        // Empty (no `.rs`) → a digest of the empty buffer (Some, deterministic). A non-`.rs` is ignored.
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        let empty = runtime_src_digest(&root);
        assert!(empty.is_some(), ".txt is not counted; the empty-`.rs` tree still digests");

        // Add `.rs` at two levels → a specific content digest.
        std::fs::write(root.join("a.rs"), b"fn a() {}").unwrap();
        std::fs::write(sub.join("b.rs"), b"fn b() {}").unwrap();
        let d1 = runtime_src_digest(&root).expect("digest");
        assert_ne!(Some(&d1), empty.as_ref(), "adding source changes the digest");

        // A pure `touch` (rewrite identical bytes, new mtime) leaves the digest UNCHANGED — the
        // papercut fix: content, not mtime, drives staleness.
        std::fs::write(root.join("a.rs"), b"fn a() {}").unwrap();
        assert_eq!(runtime_src_digest(&root).as_deref(), Some(d1.as_str()), "identical content → identical digest (mtime-independent)");

        // A content CHANGE flips the digest (keeps the teeth).
        std::fs::write(root.join("a.rs"), b"fn a() { let _ = 1; }").unwrap();
        assert_ne!(runtime_src_digest(&root).as_deref(), Some(d1.as_str()), "changed content → changed digest");

        // A missing directory yields None (read_dir fails; not a panic).
        assert_eq!(runtime_src_digest(&root.join("does-not-exist")), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn runtime_src_digest_matches_baked() {
        // Pins that `build.rs`'s baked digest and lib's recompute agree (same algorithm + seed). In a
        // dev tree the source is unchanged since the last build, so the two MUST be equal. Skips only
        // if `alignc` was built without a source tree (installed binary): baked digest empty.
        if BAKED_RUNTIME_SRC_DIGEST.is_empty() {
            return;
        }
        let src = std::path::Path::new(RUNTIME_SRC_DIR);
        if let Some(current) = runtime_src_digest(src) {
            assert_eq!(current, BAKED_RUNTIME_SRC_DIGEST, "lib recompute must equal build.rs's baked digest (algorithm/seed drift)");
        }
    }

    #[test]
    fn hygiene_flags_are_pinned_per_format() {
        // The flag tables ARE the linker policy — pin the exact spellings so a drive-by edit
        // cannot silently change what every `alignc build` passes to the linker.
        assert_eq!(hygiene_flags(ObjectFormat::Elf), ["-Wl,--gc-sections", "-Wl,--as-needed"]);
        assert_eq!(hygiene_flags(ObjectFormat::MachO), ["-Wl,-dead_strip", "-Wl,-dead_strip_dylibs"]);
    }

    #[test]
    fn support_libs_are_pinned_per_format() {
        assert_eq!(support_libs(ObjectFormat::Elf), ["-lpthread", "-ldl", "-lm"]);
        assert_eq!(support_libs(ObjectFormat::MachO), [] as [&str; 0]);
    }

    #[test]
    fn libpq_closure_is_ordered_after_unrelated_crypto_requests() {
        let input = ["crypto", "z", "zstd", "sqlite3", "pq", "ssl"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            order_link_libs(&input),
            [
                "crypto",
                "z",
                "zstd",
                "sqlite3",
                "pq",
                "ssl",
                "pq",
                "ssl",
                "crypto",
                "zstd",
                "z",
            ]
        );
    }

    #[test]
    fn link_order_without_libpq_is_unchanged() {
        let input = ["crypto", "z", "zstd"].into_iter().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(order_link_libs(&input), input);
    }

    #[test]
    fn libpq_closure_preserves_libraries_after_pq() {
        let input = ["pq", "ldap", "ssl", "crypto"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            order_link_libs(&input),
            ["pq", "ldap", "ssl", "crypto", "pq", "ssl", "crypto"]
        );
    }

    /// A uniquely named temp directory that removes itself on drop, so a failing assertion (which
    /// unwinds past any trailing cleanup line) cannot leave a tree behind. The name carries the pid
    /// and an address so two tests, and two concurrently running test binaries, never collide.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "align-driver-{tag}-{}-{:p}",
                std::process::id(),
                &0u8 as *const _
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create scratch dir");
            Scratch(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Give `path` the execute bits, so [`is_executable_file`] admits it. A no-op off Unix, where
    /// that predicate only checks the file type.
    fn make_executable(path: &std::path::Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path).expect("stat").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).expect("chmod");
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
    }

    #[test]
    fn link_failure_message_hints_library_path_only_for_gated_libs() {
        // No gated library involved → the plain error, no note.
        let plain = link_failure_message(Some(1), &["m".to_string()], &Linker::System);
        assert!(plain.starts_with("link failed"));
        assert!(!plain.contains("LIBRARY_PATH"), "no hint without a gated lib:\n{plain}");
        // A gated library (Homebrew keg-only class) → the LIBRARY_PATH hint is appended.
        let hinted = link_failure_message(Some(1), &["z".to_string(), "ssl".to_string()], &Linker::System);
        assert!(hinted.contains("LIBRARY_PATH"), "gated libs get the hint:\n{hinted}");
        assert!(hinted.contains("/opt/homebrew/opt/openssl@3/lib"), "hint shows an example path:\n{hinted}");
    }

    /// The linker that ran is always named, and an lld link additionally offers the one escape
    /// hatch — a failure the system linker would not have produced must be actionable.
    #[test]
    fn link_failure_message_names_the_linker() {
        let system = link_failure_message(Some(1), &[], &Linker::System);
        assert!(system.contains("linker: system default"), "{system}");
        assert!(!system.contains("ALIGNC_LINKER"), "no escape hatch to offer:\n{system}");

        let lld = link_failure_message(Some(1), &[], &Linker::Lld("/usr/lib/llvm-22/bin".into()));
        assert!(lld.starts_with("link failed"), "{lld}");
        assert!(lld.contains("linker: lld (/usr/lib/llvm-22/bin/ld.lld)"), "{lld}");
        assert!(lld.contains("ALIGNC_LINKER=system"), "the escape hatch is named:\n{lld}");
    }

    /// `-B<dir>` + `-fuse-ld=lld`, in that order, and nothing at all for the system linker — a
    /// system-linker build must be argv-identical to the pre-item-2 driver.
    #[test]
    fn linker_flags_are_pinned() {
        assert_eq!(Linker::System.cc_flags(), Vec::<String>::new());
        assert_eq!(
            Linker::Lld("/usr/lib/llvm-22/bin".into()).cc_flags(),
            ["-B/usr/lib/llvm-22/bin", "-fuse-ld=lld"]
        );
    }

    /// The full override × format × availability matrix for [`select_linker_with`].
    #[test]
    fn linker_selection_matrix() {
        let dir = std::path::PathBuf::from("/opt/llvm/bin");
        let found = || Some(dir.clone());
        let missing = || None;

        // Unset + ELF: lld when present, system when not (fail-open; both images are equally valid
        // and equally optimized, so there is nothing for the user to act on).
        assert_eq!(select_linker_with(ObjectFormat::Elf, None, found), Ok(Linker::Lld(dir.clone())));
        assert_eq!(select_linker_with(ObjectFormat::Elf, None, missing), Ok(Linker::System));
        // Unset + Mach-O: never lld, even when an `ld.lld` is installed. macOS is unchanged.
        assert_eq!(select_linker_with(ObjectFormat::MachO, None, found), Ok(Linker::System));

        // `system`: the system linker on every format.
        let system = Some("system".to_string());
        assert_eq!(select_linker_with(ObjectFormat::Elf, system.clone(), found), Ok(Linker::System));
        assert_eq!(select_linker_with(ObjectFormat::MachO, system, found), Ok(Linker::System));

        // `lld` + ELF: honored when found, a hard error when not (fail-closed, so a CI or
        // measurement run cannot silently fall back to the system linker).
        let lld = Some("lld".to_string());
        assert_eq!(select_linker_with(ObjectFormat::Elf, lld.clone(), found), Ok(Linker::Lld(dir.clone())));
        let err = select_linker_with(ObjectFormat::Elf, lld.clone(), missing).unwrap_err();
        assert!(err.contains("no `ld.lld` was found"), "{err}");
        assert!(err.contains("lld-22"), "the install hint names the versioned package:\n{err}");
        // The note must describe the search that actually ran: the COMPILE-TIME prefix and
        // llvm-config, and no `PATH` (which discovery deliberately never consults).
        assert!(err.contains("compile-time $LLVM_SYS_221_PREFIX/bin"), "{err}");
        assert!(err.contains("llvm-config --bindir"), "{err}");
        assert!(!err.contains("PATH"), "the note must not promise a PATH search:\n{err}");

        // `lld` + Mach-O: an explicit request is refused, never silently ignored.
        let err = select_linker_with(ObjectFormat::MachO, lld, found).unwrap_err();
        assert!(err.contains("only supported for ELF"), "{err}");

        // Anything else is a hard error on both formats: an unrecognized value must not degrade
        // into the default policy.
        for format in [ObjectFormat::Elf, ObjectFormat::MachO] {
            let err = select_linker_with(format, Some("mold".to_string()), found).unwrap_err();
            assert!(err.contains("must be `system` or `lld`"), "{err}");
            assert!(err.contains("got `mold`"), "{err}");
        }
    }

    /// Discovery step 1 in full: what does and does not count as a usable `ld.lld` under a prefix.
    /// Step 2 (`llvm-config --bindir`) spawns a process and depends on what the host installed, so
    /// only its *position* in the chain is pinned, by
    /// [`lld_discovery_prefers_the_baked_prefix_over_llvm_config`].
    #[test]
    fn lld_discovery_prefix_step() {
        let scratch = Scratch::new("lld-prefix");
        let bin = scratch.path().join("bin");
        std::fs::create_dir_all(&bin).expect("create temp prefix");
        let prefix = scratch.path().to_string_lossy().into_owned();

        // A prefix without `ld.lld` does not match, even though the prefix itself exists: a host
        // with LLVM but no lld package must fall through, not name a nonexistent linker.
        assert_eq!(lld_dir_from_prefix(Some(&prefix)), None);
        assert_eq!(lld_dir_from_prefix(None), None);

        // A NON-EXECUTABLE `ld.lld` is still not a match. Selecting one would make every link on
        // the host fail, turning the fail-open default into a fail-everything default.
        std::fs::write(bin.join(LLD_EXE), b"").unwrap();
        assert_eq!(lld_dir_from_prefix(Some(&prefix)), None, "a non-executable ld.lld must not be selected");

        // A directory named `ld.lld` is not a program either.
        let decoy = Scratch::new("lld-decoy");
        std::fs::create_dir_all(decoy.path().join("bin").join(LLD_EXE)).unwrap();
        assert_eq!(lld_dir_from_prefix(Some(&decoy.path().to_string_lossy())), None, "a directory is not a linker");

        // Only once it is executable does the prefix answer.
        make_executable(&bin.join(LLD_EXE));
        assert_eq!(lld_dir_from_prefix(Some(&prefix)), Some(bin));

        // A version-suffixed `ld.lld-22` never counts: `-fuse-ld=lld` can only address the exact
        // name, so admitting the suffixed one would produce a `-B` that resolves nothing.
        let suffixed = Scratch::new("lld-suffixed");
        let suffixed_bin = suffixed.path().join("bin");
        std::fs::create_dir_all(&suffixed_bin).unwrap();
        let alt = suffixed_bin.join(format!("{LLD_EXE}-{}", align_codegen_llvm::LLVM_TOOL_VERSION));
        std::fs::write(&alt, b"").unwrap();
        make_executable(&alt);
        assert_eq!(lld_dir_from_prefix(Some(&suffixed.path().to_string_lossy())), None);
    }

    /// The composition order of the two discovery steps, with no dependency on what the host has
    /// installed: the compile-time prefix wins outright and `llvm-config` is not even consulted.
    #[test]
    fn lld_discovery_prefers_the_baked_prefix_over_llvm_config() {
        use std::cell::Cell;

        let prefix_dir = std::path::PathBuf::from("/baked/llvm/bin");
        let config_dir = std::path::PathBuf::from("/queried/llvm/bin");

        let consulted = Cell::new(false);
        let config = || {
            consulted.set(true);
            Some(config_dir.clone())
        };
        assert_eq!(resolve_lld_dir(Some(prefix_dir.clone()), config), Some(prefix_dir));
        assert!(!consulted.get(), "a prefix hit must not spawn llvm-config");

        // A missed prefix falls through to `llvm-config`, and both missing yields None (the
        // fail-open answer the unset default turns into the system linker).
        let consulted = Cell::new(false);
        let config = || {
            consulted.set(true);
            Some(config_dir.clone())
        };
        assert_eq!(resolve_lld_dir(None, config), Some(config_dir));
        assert!(consulted.get(), "a missed prefix must consult llvm-config");
        assert_eq!(resolve_lld_dir(None, || None), None);
    }

    /// The exact `cc` argv for a link, as a pure function of its inputs. This is the seam the
    /// `capability_linking` owner asserts the gated `-l` set on, so its shape is pinned here.
    #[test]
    fn link_command_args_are_pinned_per_format() {
        let obj = std::path::Path::new("/tmp/prog.o");
        let objs = [obj];
        let libs = ["z".to_string()];
        let base = LinkPlan {
            objs: &objs,
            exe: std::path::Path::new("/tmp/prog"),
            runtime: std::path::Path::new("/tmp/libalign_runtime.a"),
            ordered_link_libs: &libs,
            format: ObjectFormat::Elf,
            profile: Profile::Release,
            profile_rt: None,
            linker: &Linker::System,
        };

        let elf = link_command_args(&base);
        assert_eq!(
            elf,
            [
                "/tmp/prog.o",
                "/tmp/libalign_runtime.a",
                "-o",
                "/tmp/prog",
                "-Wl,--gc-sections",
                "-Wl,--as-needed",
                "-lpthread",
                "-ldl",
                "-lm",
                "-lz",
            ]
        );

        // lld only prepends its two driver options; everything after is byte-identical.
        let lld_linker = Linker::Lld("/usr/lib/llvm-22/bin".into());
        let lld = link_command_args(&LinkPlan { linker: &lld_linker, ..base });
        assert_eq!(&lld[..2], ["-B/usr/lib/llvm-22/bin", "-fuse-ld=lld"]);
        assert_eq!(&lld[2..], &elf[..]);

        // Mach-O: the other hygiene spelling, no support libs, and no in-link strip even for a
        // stripping profile (an external `strip` runs after the link instead).
        let macho = link_command_args(&LinkPlan {
            ordered_link_libs: &[],
            format: ObjectFormat::MachO,
            profile: Profile::Tiny,
            ..base
        });
        assert_eq!(
            macho,
            ["/tmp/prog.o", "/tmp/libalign_runtime.a", "-o", "/tmp/prog", "-Wl,-dead_strip", "-Wl,-dead_strip_dylibs"]
        );

        // ELF stripping profile strips in the link; instrument-PGO appends the archive plus the
        // per-format forced-undefined anchor, after the objects that need it and before the `-l`s.
        let rt = std::path::Path::new("/tmp/libclang_rt.profile.a");
        let pgo = link_command_args(&LinkPlan { profile: Profile::Small, profile_rt: Some(rt), ..base });
        assert_eq!(
            pgo,
            [
                "/tmp/prog.o",
                "/tmp/libalign_runtime.a",
                "-o",
                "/tmp/prog",
                "-Wl,--gc-sections",
                "-Wl,--as-needed",
                "-Wl,--strip-all",
                "-lpthread",
                "-ldl",
                "-lm",
                "/tmp/libclang_rt.profile.a",
                "-Wl,--undefined=__llvm_profile_runtime",
                "-lz",
            ]
        );
    }

    #[test]
    fn llvm_tool_discovery_order() {
        // 1. A prefix that contains `bin/<name>` wins outright (mere existence, no launch).
        let root = std::env::temp_dir().join(format!(
            "align-driver-llvmtool-{}-{:p}",
            std::process::id(),
            &0u8 as *const _
        ));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("create temp prefix");
        std::fs::write(bin.join("llvm-sometool"), b"").unwrap();
        let prefix = root.to_string_lossy().into_owned();
        assert_eq!(
            llvm_tool_in(Some(&prefix), "llvm-sometool"),
            Some(bin.join("llvm-sometool")),
            "the build-time prefix hit is taken first"
        );

        // 2. A stale prefix (no such file) falls through to the PATH probe; a name that exists
        //    nowhere yields None (the caller degrades to a note).
        assert_eq!(llvm_tool_in(Some(&prefix), "llvm-definitely-not-a-tool"), None);
        assert_eq!(llvm_tool_in(None, "llvm-definitely-not-a-tool"), None);

        std::fs::remove_dir_all(&root).ok();
    }
}

#[cfg(test)]
mod walk_tests {
    use super::*;

    /// P32: the per-module closure memo. The key needs a closure for every closure MEMBER of every
    /// unit, so without memoization a fan-in DAG would recompute the same closure once per
    /// importer. `transitive` itself is unmemoized and stays the reference implementation.
    #[test]
    fn closure_of_computes_each_module_at_most_once() {
        use std::collections::HashMap;
        // A diamond: main -> {left, right} -> leaf. `leaf`'s closure is asked for by both sides.
        let direct: HashMap<String, Vec<String>> = HashMap::from([
            ("main".to_string(), vec!["left".to_string(), "right".to_string()]),
            ("left".to_string(), vec!["leaf".to_string()]),
            ("right".to_string(), vec!["leaf".to_string()]),
            ("leaf".to_string(), Vec::new()),
        ]);
        let mut memo = HashMap::new();
        for module in ["main", "left", "right", "leaf", "left", "right", "leaf"] {
            let memoized = closure_of(module, &direct, &mut memo);
            assert_eq!(memoized, transitive(module, &direct), "the memo must not change the answer");
        }
        assert_eq!(memo.len(), 4, "one entry per MODULE, never one per importer");
        assert_eq!(memo["main"], vec!["leaf", "left", "right"]);
        assert_eq!(memo["leaf"], Vec::<String>::new());
    }

    /// P33 / F-1, F-2, F-4: the `FileId` space.
    ///
    /// `load_units` registers every real unit against the CALLER's `SourceMap` before anything else
    /// is added, so real units occupy exactly `0..N` and every later `<interface:…>` pseudo-file
    /// lands at `>= N`. That partition is what lets the rehydration map diverge above `N` without
    /// any observable consequence — and what makes seeding it with the same `N` files mandatory.
    #[test]
    fn load_units_owns_the_low_file_id_space() {
        let dir = std::env::temp_dir().join(format!("align-fidspace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create");
        std::fs::write(dir.join("leaf.align"), "module leaf\n\npub fn one() -> i64 = 1\n").unwrap();
        std::fs::write(
            dir.join("mid.align"),
            "module mid\n\nimport leaf\n\npub fn two() -> i64 = leaf.one() + 1\n",
        )
        .unwrap();
        let entry_src = "import mid\n\nfn main() {\n    print(mid.two())\n}\n";
        let entry = dir.join("main.align");
        std::fs::write(&entry, entry_src).unwrap();

        let mut source_map = SourceMap::new();
        let mut diags = Diagnostics::new();
        let loaded = load_units(
            &mut source_map,
            &entry.display().to_string(),
            entry_src,
            &mut diags,
            None,
        );
        assert!(!diags.has_errors());
        let n = loaded.len();
        assert_eq!(n, 3);

        // F-1/F-2: the real units are exactly ids `0..N`, each used once.
        let mut ids: Vec<align_span::FileId> = loaded.iter().map(|unit| unit.fid).collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..n as align_span::FileId).collect::<Vec<_>>());

        // F-2: the next file registered — a pseudo-file — necessarily lands at `>= N`.
        let pseudo = source_map.add_file("<interface:leaf>".to_string(), String::new());
        assert!(
            pseudo >= n as align_span::FileId,
            "a pseudo-file must never collide with a real unit's id"
        );

        // F-2, negative: a FRESH map hands out id 0, which caller-side denotes unit 0. This is
        // precisely why `RehydrateCtx::scratch` is seeded with the N unit files instead of started
        // empty — an empty scratch map would make the own-file diagnostic filter accept a
        // pseudo-file span as the unit's own.
        let mut fresh = SourceMap::new();
        assert_eq!(fresh.add_file("<interface:leaf>".to_string(), String::new()), 0);

        // F-4: a seeded map agrees with the caller's map on `0..N` and only diverges above it.
        let mut seeded = SourceMap::new();
        let mut by_fid: Vec<&LoadedUnit> = loaded.iter().collect();
        by_fid.sort_by_key(|unit| unit.fid);
        for unit in &by_fid {
            seeded.add_file(unit.file.clone(), unit.src.clone());
        }
        for unit in &loaded {
            assert_eq!(
                seeded.get(unit.fid).src,
                source_map.get(unit.fid).src,
                "id {} must mean the same source in both maps",
                unit.fid
            );
        }
        assert!(seeded.add_file("<interface:mid>".to_string(), String::new()) >= n as align_span::FileId);

        std::fs::remove_dir_all(&dir).ok();
    }
}

/// A process-private staging directory, shared by every artifact producer in this workspace.
///
/// Lives in the library rather than in `alignc`'s binary crate because a library consumer needs it
/// too: `align-repl` stages one session's source, object, and executable exactly the way `alignc`
/// stages a build (`docs/impl/22-repl-plan.md` §11 L2). A second copy of this directory-race
/// protocol is a second place for it to drift.
static ARTIFACT_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A process-private staging directory. `create_dir` is the atomic claim: a stale or racing path is
/// skipped, and Drop removes only the directory this invocation successfully created.
pub struct ArtifactStage {
    dir: std::path::PathBuf,
    owned: bool,
}

impl ArtifactStage {
    pub fn in_dir(parent: &std::path::Path, label: &str) -> std::io::Result<Self> {
        let parent = std::fs::canonicalize(parent)?;
        for _ in 0..1024 {
            let nonce = ARTIFACT_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let dir = parent.join(format!(".{label}-{}-{stamp}-{nonce}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Self { dir, owned: true }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not create a unique artifact staging directory",
        ))
    }

    pub fn temp(label: &str) -> std::io::Result<Self> {
        Self::in_dir(&std::env::temp_dir(), label)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.dir
    }

    pub(crate) fn into_owned_dir(mut self) -> std::path::PathBuf {
        self.owned = false;
        std::mem::take(&mut self.dir)
    }
}

impl Drop for ArtifactStage {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

#[cfg(test)]
mod artifact_stage_tests {
    use super::ArtifactStage;

    #[test]
    fn concurrent_stages_are_distinct_and_remove_only_their_own_directory() {
        let left = std::thread::spawn(|| ArtifactStage::temp("align-stage-owner-test"));
        let right = std::thread::spawn(|| ArtifactStage::temp("align-stage-owner-test"));
        let left = left
            .join()
            .unwrap_or_else(|_| panic!("left stage thread panicked"))
            .unwrap_or_else(|error| panic!("create left stage: {error}"));
        let right = right
            .join()
            .unwrap_or_else(|_| panic!("right stage thread panicked"))
            .unwrap_or_else(|error| panic!("create right stage: {error}"));
        let left_path = left.path().to_path_buf();
        let right_path = right.path().to_path_buf();
        assert_ne!(left_path, right_path);
        assert!(left_path.is_dir());
        assert!(right_path.is_dir());

        drop(left);
        assert!(!left_path.exists());
        assert!(right_path.is_dir());
        drop(right);
        assert!(!right_path.exists());
    }
}
