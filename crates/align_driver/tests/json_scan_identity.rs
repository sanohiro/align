//! Cross-compiler identity owner for the Request 6 Copy-row scanner contract.
//!
//! `scripts/compare-json-scan-identity.sh` copies this test and its fixture into two clean release
//! worktrees, then compares the exact files emitted here. Keep the output names and field spelling
//! stable: the shell owner deliberately uses `cmp` without normalization.

use std::path::{Path, PathBuf};

use align_driver::{
    BuildTarget, PgoKey, Profile, build_codegen_key, build_per_unit, emit_object_file,
};
use align_interface::serialize;
use align_mir::print::codegen_input_to_string;
use align_span::SourceMap;

fn output_dir() -> PathBuf {
    let raw = std::env::var_os("ALIGN_JSON_SCAN_IDENTITY_OUT")
        .expect("ALIGN_JSON_SCAN_IDENTITY_OUT must name an explicit output directory");
    let dir = PathBuf::from(raw);
    std::fs::create_dir_all(&dir).expect("create identity output directory");
    dir
}

fn write_bytes(dir: &Path, name: &str, bytes: impl AsRef<[u8]>) {
    std::fs::write(dir.join(name), bytes).unwrap_or_else(|error| panic!("write {name}: {error}"));
}

fn write_text(dir: &Path, name: &str, value: impl AsRef<str>) {
    write_bytes(dir, name, value.as_ref().as_bytes());
}

fn write_key_fields(dir: &Path, key: &align_driver::CodegenKey) {
    let fields = dir.join("key-fields");
    std::fs::create_dir_all(&fields).expect("create key-field directory");
    write_text(
        &fields,
        "cache_format_version",
        key.cache_format_version.to_string(),
    );
    write_text(&fields, "compiler_build_id", key.compiler_build_id.to_hex());
    write_text(&fields, "frontend_schema", key.frontend_schema.to_string());
    write_text(&fields, "located", key.located.to_string());
    write_text(&fields, "impl_hash", key.impl_hash.to_hex());
    let dep_interface_hashes = key
        .dep_interface_hashes
        .iter()
        .map(|(unit, hash)| format!("{unit}={}", hash.to_hex()))
        .collect::<Vec<_>>()
        .join("\n");
    write_text(&fields, "dep_interface_hashes", dep_interface_hashes);
    write_text(&fields, "exports", key.exports.join("\n"));
    write_text(&fields, "target_triple", &key.target_triple);
    write_text(&fields, "object_format", key.object_format.to_string());
    write_text(&fields, "resolved_cpu", &key.resolved_cpu);
    write_text(&fields, "resolved_features", &key.resolved_features);
    write_text(&fields, "profile_name", &key.profile_name);
    write_text(&fields, "pipeline", &key.pipeline);
    write_text(&fields, "codegen_opt", &key.codegen_opt);
    write_text(&fields, "reloc_model", &key.reloc_model);
    write_text(&fields, "code_model", &key.code_model);
    write_text(&fields, "llvm_version", &key.llvm_version);
    write_text(&fields, "rt_lto", key.rt_lto.to_string());
    write_text(
        &fields,
        "rt_lto_digest",
        key.rt_lto_digest
            .map_or_else(|| "none".to_string(), |hash| hash.to_hex()),
    );
    let pgo_mode = match key.pgo_mode {
        PgoKey::Off => "off".to_string(),
        PgoKey::Instrument => "instrument".to_string(),
        PgoKey::Use(hash) => format!("use:{}", hash.to_hex()),
    };
    write_text(&fields, "pgo_mode", pgo_mode);
    write_text(&fields, "unit", &key.unit);
    write_text(dir, "key-full-digest", key.full_digest().to_hex());
    write_text(dir, "key-slot-digest", key.slot_digest().to_hex());
}

#[test]
#[ignore = "run through scripts/compare-json-scan-identity.sh"]
fn json_scan_cross_compiler_identity() {
    let dir = output_dir();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json_scan_copy_identity.align");
    let source = std::fs::read_to_string(&fixture).expect("read scanner identity fixture");
    let mut source_map = SourceMap::new();
    let walk = build_per_unit(&mut source_map, &fixture.display().to_string(), &source);
    assert!(
        !walk.diags.has_errors(),
        "identity fixture must check cleanly:\n{}",
        align_driver::format_diagnostics(&source_map, &walk.diags)
    );
    let unit = walk
        .units
        .iter()
        .find(|unit| unit.unit == "main")
        .expect("identity fixture must produce the main unit");

    write_bytes(&dir, "interface.bin", serialize(&unit.summary));
    write_text(&dir, "interface-hash", unit.summary.interface_hash.to_hex());
    write_text(&dir, "mir.txt", codegen_input_to_string(&unit.mir));

    let target = BuildTarget::Baseline;
    let raw_llvm = align_driver::emit_llvm_ir(&unit.mir, target.clone(), false, &[], false)
        .expect("emit raw LLVM");
    write_text(&dir, "llvm.ll", raw_llvm);

    let object = dir.join("object.o");
    emit_object_file(
        &unit.mir,
        &object,
        target.clone(),
        Profile::Release,
        &[],
        false,
    )
    .expect("emit object");

    let key = build_codegen_key(
        &unit.unit,
        unit.summary.impl_hash,
        &unit.dep_interface_hashes,
        &target,
        Profile::Release,
        &[],
        false,
        PgoKey::Off,
    )
    .expect("build codegen key");
    write_key_fields(&dir, &key);
}
