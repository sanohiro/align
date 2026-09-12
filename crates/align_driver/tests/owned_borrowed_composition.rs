//! Regression coverage for the R77/R79/R80/R81/R82/R83 composition boundary.
//!
//! These cases exercise the same checked-HIR, whole-program, per-unit, and native paths that
//! the external align-llm request batch consumes. R78 remains deferred under the friction ledger.

mod common;
use common::*;

#[test]
fn borrowed_config_forwarding_returns_independent_argv() {
    // R81's borrowed Config witness keeps both the string field and an array field as shared
    // call inputs. The returned argv owns cloned strings and remains valid after the caller's
    // Config is used again.
    let source = r#"module borrowed_config_forwarding
Config { workspace: string, readonly_roots: array<string>, target_argv: array<string> }

fn validate(borrow workspace: str, roots: slice<string>) -> Result<(), Error> {
  if workspace != "/workspace" || roots.len() != 1 || roots[0] != "/usr" { return Err(Error.Invalid) }
  return Ok(())
}

fn argv(borrow values: slice<string>, workspace: str) -> array<string> {
  mut output: array_builder<string> := array_builder()
  output.push("--workspace".clone())
  output.push(workspace.clone())
  mut index := 0
  loop {
    if index == values.len() { break }
    output.push(values[index].clone())
    index = index + 1
  }
  return output.build()
}

fn arguments(borrow config: Config) -> Result<array<string>, Error> {
  validate(config.workspace, config.readonly_roots)?
  return Ok(argv(config.target_argv, config.workspace))
}

fn main() -> i32 {
  mut roots: array_builder<string> := array_builder()
  roots.push("/usr".clone())
  mut target: array_builder<string> := array_builder()
  target.push("/bin/echo".clone())
  config := Config { workspace: "/workspace".clone(), readonly_roots: roots.build(), target_argv: target.build() }
  result := arguments(config)
  return match result {
    Err(_) => 0,
    Ok(values) => if values.len() == 3 && values[1] == "/workspace" && values[2] == "/bin/echo" &&
      config.workspace == "/workspace" && config.readonly_roots[0] == "/usr" { 42 } else { 0 },
  }
}
"#;
    assert_clean("r81-borrowed-config-forwarding", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r81-borrowed-config-forwarding", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r81-borrowed-config-forwarding-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn borrowed_task_source_expansion_keeps_owned_rows_and_source_reusable() {
    // R81's blocking source-collection shape: a borrowed TaskSource exposes both a manifest
    // record and an owned declaration array, an expansion returns fresh digest rows, and the
    // collector appends those rows into caller-owned output. The TaskSource remains readable
    // after the borrowed call completes.
    let source = r#"module borrowed_task_source_collection
ArtifactDigest { path: string, mode: string, byte_count: i64, sha256: string }
ArtifactExpectation { path: string, expected_sha256: string, kind: string }
Expansion { files: array<ArtifactDigest>, entry_count: i64, byte_count: i64 }
TaskSource { manifest: ArtifactDigest, declarations: array<ArtifactExpectation> }

fn expand(borrow declarations: slice<ArtifactExpectation>) -> Result<Expansion, Error> {
  mut files: array_builder<ArtifactDigest> := array_builder()
  mut index := 0
  loop {
    if index == declarations.len() { break }
    files.push(ArtifactDigest {
      path: declarations[index].path.clone(), mode: declarations[index].kind.clone(),
      byte_count: declarations[index].expected_sha256.len(),
      sha256: declarations[index].expected_sha256.clone(),
    })
    index = index + 1
  }
  return Ok(Expansion { files: files.build(), entry_count: declarations.len(), byte_count: 0 })
}

fn append(borrow row: ArtifactDigest, borrow mut output: array_builder<ArtifactDigest>) {
  output.push(ArtifactDigest {
    path: row.path.clone(), mode: row.mode.clone(), byte_count: row.byte_count,
    sha256: row.sha256.clone(),
  })
}

fn collect_task(borrow task: TaskSource, borrow mut output: array_builder<ArtifactDigest>) -> Result<(), Error> {
  append(task.manifest, output)
  expanded := expand(task.declarations)?
  rows: slice<ArtifactDigest> := expanded.files
  mut index := 0
  loop {
    if index == rows.len() { break }
    append(rows[index], output)
    index = index + 1
  }
  return Ok(())
}

fn main() -> i32 {
  mut declarations: array_builder<ArtifactExpectation> := array_builder()
  declarations.push(ArtifactExpectation {
    path: "src/a".clone(), expected_sha256: "aa".clone(), kind: "FILE".clone(),
  })
  declarations.push(ArtifactExpectation {
    path: "src/b".clone(), expected_sha256: "bbb".clone(), kind: "FILE".clone(),
  })
  task := TaskSource {
    manifest: ArtifactDigest { path: "manifest".clone(), mode: "M".clone(), byte_count: 1, sha256: "mm".clone() },
    declarations: declarations.build(),
  }
  mut output: array_builder<ArtifactDigest> := array_builder()
  result := collect_task(task, output)
  rows := output.build()
  return match result {
    Err(_) => 0,
    Ok(_) => if rows.len() == 3 && rows[1].path == "src/a" && rows[2].sha256 == "bbb" &&
      task.manifest.path == "manifest" && task.declarations[0].path == "src/a" { 42 } else { 0 },
  }
}
"#;
    assert_clean("r81-borrowed-task-source-collection", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r81-borrowed-task-source-collection", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r81-borrowed-task-source-collection-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn faithful_document_digest_and_template_return_path_is_admitted() {
    // R83's full evaluator witness keeps the resource-backed Document shape, nested optional
    // selectors, and the owned template's seven string leaves. It is a source/per-unit owner;
    // execution stays in the existing filesystem/native owners.
    let source = r#"module faithful_document_digest_template
import core.json
import std.crypto
import std.encoding
import std.fs
import std.time

Document { text: string, sha256: string, metadata: fs.metadata }
Profile { Diagnostics, EditSet, Policy }
Headers {
  STATUS: string, POLICY: Option<string>, EDITSET: Option<string>, SUMMARY: string,
  STDOUT: string, STDERR: string,
}
Template {
  schema_version: i64, artifact_kind: string, template_id: string, preamble_text: string,
  section_headers: Headers, closing_text: string, content_sha256: string,
}
Manifest { path: Option<string>, hash: Option<string>, kind: Option<string> }

fn same(before: fs.metadata, after: fs.metadata) -> bool {
  regular := match after.kind { Regular => true, _ => false }
  return regular && before.device == after.device && before.inode == after.inode &&
    after.links == 1 && before.mode == after.mode && before.size == after.size &&
    before.modified_seconds == after.modified_seconds &&
    before.modified_nanoseconds == after.modified_nanoseconds &&
    before.changed_seconds == after.changed_seconds &&
    before.changed_nanoseconds == after.changed_nanoseconds
}

fn read_document(borrow root: fs.directory, relative: slice<u8>, maximum: i64, deadline: i64) -> Result<Document, Error> {
  if maximum < 1 || maximum > 2097152 || time.instant() >= deadline { return Err(Error.Invalid) }
  input := root.open_read_single_link(relative)?
  before := input.metadata()?
  if before.links != 1 || before.size < 0 || before.size > maximum { return Err(Error.Invalid) }
  mut data := buffer(0)
  mut total := 0
  loop {
    if time.instant() >= deadline { return Err(Error.Invalid) }
    if total == maximum {
      mut probe := buffer(1)
      if input.read(probe)? != 0 { return Err(Error.Invalid) }
      break
    }
    remaining := maximum - total
    mut chunk := buffer(if remaining < 65536 { remaining } else { 65536 })
    count := input.read(chunk)?
    if count == 0 { break }
    data.append(chunk.bytes())
    total = total + count
  }
  after := input.metadata()?
  if time.instant() >= deadline || total != before.size || !same(before, after) { return Err(Error.Invalid) }
  text := data.bytes().as_str()?.clone()
  digest := crypto.sha256(text)
  return Ok(Document {
    text: text,
    sha256: encoding.hex_encode(digest[0..digest.len()]).clone(),
    metadata: after,
  })
}

fn decode_template(source: str, profile: Profile) -> Result<Template, Error> {
  return json.decode(source)
}

fn bound(borrow root: fs.directory, path: str, hash: str, deadline: i64, borrow mut remaining: i64) -> Result<Document, Error> {
  source := read_document(root, path.bytes(), remaining, deadline)?
  remaining = remaining - source.metadata.size
  if source.sha256 != hash { return Err(Error.Invalid) }
  return Ok(source)
}

fn wrap(borrow root: fs.directory, borrow manifest: Manifest, deadline: i64, borrow mut remaining: i64) -> Result<Option<Template>, Error> {
  return match manifest.path {
    None => Ok(None),
    Some(path) => {
      hash: str := match manifest.hash {
        None => { return Err(Error.Invalid) },
        Some(value) => value,
      }
      source := bound(root, path, hash, deadline, remaining)?
      profile := match manifest.kind {
        Some(kind) => if kind == "PROVIDER_EDIT" { Profile.Policy } else { Profile.Diagnostics },
        None => { return Err(Error.Invalid) },
      }
      value := decode_template(source.text, profile)?
      return Ok(Some(value))
    },
  }
}

fn main() {}
"#;
    assert_clean("r83-faithful-document-digest-template", source);
}

#[test]
fn owned_measurement_final_digest_field_is_certified_after_canonicalization() {
    // R83's second blocking consumer returns the 32-field TaskMeasurement after canonicalizing
    // it and assigning content_sha256 (the final StructField(31) in the provider record). Keep the
    // nested identity records and optional edit/completion leaves so this exercises the actual
    // owned-result shape rather than another short Template surrogate.
    let source = r#"module owned_measurement_final_digest
import core.json
import std.crypto
import std.encoding

GenerationRequestIdentity {
  schema_version: i64, artifact_kind: string, rendered_prompt_sha256: string,
  system_text_sha256: string, user_text_sha256: string, generation_policy_sha256: string,
  provider_control_sha256: string, environment_policy_sha256: string, max_tokens: i64,
  temperature_micros: i64, paired_seed: i64, provider_request_sha256: string,
  seed_attestation_sha256: string, content_sha256: string,
}
EnvironmentProbe {
  schema_version: i64, artifact_kind: string, producer: string, os: string,
  os_release: string, architecture: string, cpu: string, logical_cpu_count: Option<i64>,
  gpu: string, runtime_identity: string, content_sha256: string,
}
SeedCapabilityAttestation {
  schema_version: i64, artifact_kind: string, provider_kind: string, provider_model: string,
  requested_seed: i64, result: string, applied_seed: Option<i64>,
  provider_request_sha256: string, content_sha256: string,
}
EditSetBlock {
  schema_version: i64, artifact_kind: string, path: string, body_bytes: i64,
  body_sha256: string, body_text: Option<string>, content_sha256: string,
}
TaskMeasurement {
  schema_version: i64, artifact_kind: string, status: string, failure_kind: string,
  build_status: string, test_status: string, repair_loop_count: i64,
  unrelated_diff_count: i64, patch_size_bytes: i64, public_api_change_count: i64,
  policy_violation_count: i64, cleanup_passed: bool, containment_passed: bool,
  benchmark_regression_ppm: Option<i64>, generation_to_passing_patch_ns: Option<i64>,
  rendered_prompt_sha256: string, generation_request: GenerationRequestIdentity,
  environment_probe: EnvironmentProbe, seed_attestation: SeedCapabilityAttestation,
  diagnostic_summary: string, diagnostic_stdout: string, diagnostic_stderr: string,
  edit_set: Option<array<EditSetBlock>>, edit_set_total_bytes: Option<i64>,
  patch_sha256: Option<string>, base_adapter_runtime_identity: Option<string>,
  product_runtime: Option<string>, edit_refusal: Option<string>, completion_bytes: Option<i64>,
  completion_sha256: Option<string>, completion_text: Option<string>, content_sha256: string,
}

fn digest(value: str) -> string {
  bytes := crypto.sha256(value)
  return encoding.hex_encode(bytes[0..bytes.len()]).clone()
}

fn canonical(borrow value: TaskMeasurement) -> Result<string, Error> {
  return json.encode(value)
}

fn build() -> Result<TaskMeasurement, Error> {
  mut value := TaskMeasurement {
    schema_version: 4, artifact_kind: "TASK_MEASUREMENT".clone(), status: "PASS".clone(),
    failure_kind: "NONE".clone(), build_status: "PASS".clone(), test_status: "PASS".clone(),
    repair_loop_count: 0, unrelated_diff_count: 0, patch_size_bytes: 0,
    public_api_change_count: 0, policy_violation_count: 0, cleanup_passed: true,
    containment_passed: true, benchmark_regression_ppm: None,
    generation_to_passing_patch_ns: None, rendered_prompt_sha256: "r".clone(),
    generation_request: GenerationRequestIdentity {
      schema_version: 1, artifact_kind: "GENERATION_REQUEST_IDENTITY".clone(),
      rendered_prompt_sha256: "r".clone(), system_text_sha256: "s".clone(),
      user_text_sha256: "u".clone(), generation_policy_sha256: "g".clone(),
      provider_control_sha256: "p".clone(), environment_policy_sha256: "e".clone(),
      max_tokens: 1, temperature_micros: 0, paired_seed: 7,
      provider_request_sha256: "q".clone(), seed_attestation_sha256: "a".clone(),
      content_sha256: "c".clone(),
    },
    environment_probe: EnvironmentProbe {
      schema_version: 2, artifact_kind: "ENVIRONMENT_PROBE".clone(), producer: "ALIGN_TASK".clone(),
      os: "test".clone(), os_release: "test".clone(), architecture: "test".clone(),
      cpu: "test".clone(), logical_cpu_count: None, gpu: "none".clone(),
      runtime_identity: "ALIGN:test".clone(), content_sha256: "p".clone(),
    },
    seed_attestation: SeedCapabilityAttestation {
      schema_version: 1, artifact_kind: "SEED_CAPABILITY_ATTESTATION".clone(),
      provider_kind: "LOCAL".clone(), provider_model: "test".clone(), requested_seed: 7,
      result: "APPLIED".clone(), applied_seed: Some(7), provider_request_sha256: "q".clone(),
      content_sha256: "a".clone(),
    },
    diagnostic_summary: "".clone(), diagnostic_stdout: "".clone(), diagnostic_stderr: "".clone(),
    edit_set: None, edit_set_total_bytes: None, patch_sha256: None,
    base_adapter_runtime_identity: None, product_runtime: Some("ALIGN:test".clone()),
    edit_refusal: Some("NONE".clone()), completion_bytes: None, completion_sha256: None,
    completion_text: None, content_sha256: "".clone(),
  }
  canonical_text := canonical(value)?
  value.content_sha256 = digest(canonical_text)
  return Ok(value)
}

fn main() -> i32 {
  result := build()
  return match result {
    Err(_) => 0,
    Ok(value) => if value.schema_version == 4 && value.content_sha256.len() == 64 { 42 } else { 0 },
  }
}
"#;
    assert_clean("r83-owned-measurement-final-digest", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-owned-measurement-final-digest", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r83-owned-measurement-final-digest-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

fn assert_clean(name: &str, source: &str) {
    let checked = diff_check_multi(name, &[("main.align", source)], "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{name}\nwhole: {}\nper-unit: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
}

fn assert_rejected(name: &str, source: &str) {
    let checked = diff_check_multi(name, &[("main.align", source)], "main.align");
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "{name} unexpectedly passed\nwhole: {}\nper-unit: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
}

#[test]
fn borrowed_fixed_array_to_slice_materializes_a_descriptor() {
    let source = r#"module borrowed_fixed_array_slice
fn inspect(borrow values: slice<i64>) -> i64 = values[0]
fn main() -> i32 {
  mut values := [42]
  value := inspect(values)
  if value == 42 && values[0] == 42 { return 42 }
  return 0
}
"#;
    assert_clean("fixed-array-to-borrowed-slice", source);
    if backend_available() {
        assert_eq!(
            build_and_run("fixed-array-to-borrowed-slice", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("fixed-array-to-borrowed-slice-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn borrowed_string_root_view_retype_is_certified() {
    let source = r#"module borrowed_string_root_view
fn inspect(borrow value: str) -> i64 = value.len()
fn main() -> i32 {
  owner := "forty-two".clone()
  length := inspect(owner)
  if length == 9 && owner == "forty-two" { return 42 }
  return 0
}
"#;
    assert_clean("borrowed-string-root-view", source);
    if backend_available() {
        assert_eq!(
            build_and_run("borrowed-string-root-view", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("borrowed-string-root-view-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn mutable_owned_view_retypes_are_rejected_before_lowering() {
    let string_source = r#"module reject_mutable_string_view
fn replace(borrow mut value: str) { value = "new" }
fn main() -> i32 {
  mut owner := "old".clone()
  replace(owner)
  return 0
}
"#;
    let string_diagnostics = check_diagnostics("reject-mut-string-view", string_source);
    assert!(
        string_diagnostics.contains("cannot exclusively borrow owning storage through a view"),
        "an owned string must not be relabeled as a mutable str place:\n{string_diagnostics}"
    );
    assert_rejected("reject-mut-string-view", string_source);

    let array_source = r#"module reject_mutable_dynamic_view
fn touch(borrow mut values: slice<i64>) { values = [] }
fn main() -> i32 {
  mut owner := [1].to_array()
  touch(owner)
  return 0
}
"#;
    let array_diagnostics = check_diagnostics("reject-mut-dynamic-view", array_source);
    assert!(
        array_diagnostics.contains("cannot exclusively borrow owning storage through a view"),
        "an owned dynamic array must not be relabeled as a mutable slice place:\n{array_diagnostics}"
    );
    assert_rejected("reject-mut-dynamic-view", array_source);
}

#[test]
fn generic_borrow_accepts_an_indexed_move_field_chain() {
    let source = r#"module generic_indexed_move_field
Row { child: string }
fn observe<T>(borrow value: T) -> i64 = 42
fn main() -> i32 {
  rows := [Row { child: "answer".clone() }]
  result := observe(rows[0].child)
  if result == 42 { return 42 }
  return 0
}
"#;
    assert_clean("generic-indexed-move-field", source);
    if backend_available() {
        assert_eq!(
            build_and_run("generic-indexed-move-field", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("generic-indexed-move-field-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn fixed_record_field_borrow_keeps_the_physical_string_owner() {
    let source = r#"module borrowed_fixed_record_projection
Row { text: string, score: i64 }
fn size(borrow value: str) -> i64 = value.len()
fn main() -> i32 {
  rows := [Row { text: "fixed".clone(), score: 7 }]
  value := size(rows[0].text)
  if value == 5 && rows[0].score == 7 { return 42 }
  return 0
}

"#;
    assert_clean("r77-fixed-record-field", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r77-fixed-record-field", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r77-fixed-record-field-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn fixed_move_record_field_borrow_requires_an_integer_literal_index() {
    let source = r#"module fixed_move_record_projection
Policy { label: string }
Task { generation: Policy }
fn inspect(borrow policy: Policy) -> i64 = policy.label.len()
fn main() -> i32 {
  tasks := [Task { generation: Policy { label: "fixed".clone() } }]
  index := 0
  return inspect(tasks[index].generation) as i32
}
"#;
    assert_rejected("r77-fixed-runtime-index", source);
}

#[test]
fn fixed_move_record_field_borrow_accepts_an_integer_literal_index() {
    let source = r#"module fixed_move_record_literal_projection
Policy { label: string }
Task { generation: Policy }
fn inspect(borrow policy: Policy) -> i64 = policy.label.len()
fn main() -> i32 {
  tasks := [Task { generation: Policy { label: "fixed".clone() } }]
  return inspect(tasks[0].generation) as i32
}
"#;
    assert_clean("r77-fixed-literal-index", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r77-fixed-literal-index", source)
                .status
                .code(),
            Some(5)
        );
    }
}

#[test]
fn dynamic_record_field_borrow_keeps_the_physical_string_owner() {
    let source = r#"module borrowed_dynamic_record_projection
Record { body: string }
fn redact(borrow text: str) -> string = text.clone()
fn build(blocks: slice<Record>) -> string = redact(blocks[0].body)
fn main() -> i32 {
  value := build([Record { body: "dynamic".clone() }])
  return value.len() as i32
}
"#;
    assert_clean("r77-dynamic-record-field", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r77-dynamic-record-field", source)
                .status
                .code(),
            Some(7)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r77-dynamic-record-field-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(7)
        );
    }
}

#[test]
fn dynamic_move_record_field_borrow_is_read_only_in_place() {
    let source = r#"module borrowed_dynamic_move_record_projection
Policy { label: string }
Task { generation: Policy }
fn inspect(borrow policy: Policy) -> i64 = policy.label.len()
fn main() -> i32 {
  tasks := [Task { generation: Policy { label: "dynamic".clone() } }]
  return inspect(tasks[0].generation) as i32
}
"#;
    assert_clean("r83-dynamic-move-field-borrow", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-dynamic-move-field-borrow", source)
                .status
                .code(),
            Some(7)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r83-dynamic-move-field-borrow-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(7)
        );
    }
}

#[test]
fn dynamic_move_record_field_borrow_from_stable_field_keeps_the_root() {
    let source = r#"module borrowed_dynamic_move_record_field_base
Policy { label: string }
Task { generation: Policy }
Container { tasks: slice<Task> }
fn inspect(borrow policy: Policy) -> i64 = policy.label.len()
fn call(borrow container: Container) -> i64 {
  index := 0
  return inspect(container.tasks[index].generation)
}
fn main() -> i32 {
  tasks := [Task { generation: Policy { label: "dynamic".clone() } }]
  container := Container { tasks: tasks }
  return call(container) as i32
}
"#;
    assert_clean("r83-dynamic-move-field-base", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-dynamic-move-field-base", source)
                .status
                .code(),
            Some(7)
        );
    }
}

#[test]
fn mixed_owned_and_borrowed_record_leaves_keep_per_leaf_authority() {
    let source = r#"module mixed_owned_borrowed_record
Mixed { owned: string, view: str }
fn consume(value: Mixed) -> i64 = value.view.len()
fn main() -> i32 {
  source := "borrowed".clone()
  view: str := source
  value := Mixed { owned: "owned".clone(), view: view }
  result := consume(value)
  return result as i32
}
"#;
    assert_clean("r83-mixed-owned-borrowed-record", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-mixed-owned-borrowed-record", source)
                .status
                .code(),
            Some(8)
        );
    }
}

#[test]
fn nested_copy_process_signal_payload_is_read_without_transfer() {
    let source = r#"module fixture_setup_nested_signal
import std.process
Stop { Cancelled(process.signal) }
Report { stop: Stop, data: string }
Attempt { report: Option<Report> }
fn inspect(borrow attempt: Attempt) -> i64 {
  return match attempt.report {
    None => 0,
    Some(report) => match report.stop {
      Cancelled(signal) => process.signal_number(signal),
    },
  }
}
fn main() -> i32 {
  value := Attempt { report: Some(Report { stop: Stop.Cancelled(process.signal.Terminate), data: "".clone() }) }
  return inspect(value) as i32
}
"#;
    assert_clean("r79-nested-process-signal", source);
    if backend_available() {
        let output = build_and_run("r79-nested-process-signal", source);
        assert_eq!(output.status.code(), Some(15));
    }
}

#[test]
fn scalar_to_array_replacement_does_not_retain_the_loop_iteration_owner() {
    let source = r#"module scalar_copy_loop
Report { stdout: array<u8> }
fn report() -> Report {
  mut values: array_builder<u8> := array_builder()
  values.push(65 as u8)
  return Report { stdout: values.build() }
}
fn collect() -> array<u8> {
  empty: array_builder<u8> := array_builder()
  mut output := empty.build()
  mut index := 0
  loop {
    if index == 2 { break }
    value := report()
    bytes: slice<u8> := value.stdout
    output = bytes.to_array()
    index = index + 1
  }
  return output
}
fn main() -> i32 {
  result := collect()
  return result.len() as i32
}
"#;
    assert_clean("r80-scalar-to-array-loop", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r80-scalar-to-array-loop", source)
                .status
                .code(),
            Some(1)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r80-scalar-to-array-loop-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(1)
        );
    }
}

#[test]
fn borrowed_record_array_field_is_a_shared_view_in_both_build_modes() {
    let source = r#"module borrowed_array_return_test
Row { path: string }
Observation { tree: array<Row> }
fn changes(left: slice<Row>, right: slice<Row>) -> array<string> {
  mut result: array_builder<string> := array_builder()
  if left.len() != 0 { result.push(left[0].path.clone()) }
  if right.len() != 0 { result.push(right[0].path.clone()) }
  return result.build()
}
fn inspect(borrow value: Observation) -> array<string> = {
  mut rows: array_builder<Row> := array_builder()
  rows.push(Row { path: "b".clone() })
  right := rows.build()
  return changes(value.tree, right)
}
fn main() -> i32 {
  mut rows: array_builder<Row> := array_builder()
  rows.push(Row { path: "a".clone() })
  value := Observation { tree: rows.build() }
  direct := changes(value.tree, value.tree)
  actual := inspect(value)
  if direct.len() == 2 && direct[0] == "a" && direct[1] == "a" &&
  actual.len() == 2 && actual[0] == "a" && actual[1] == "b" &&
  value.tree[0].path == "a" { return 42 }
  return 0
}
"#;
    assert_clean("r81-borrowed-record-array", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r81-borrowed-record-array", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r81-borrowed-record-array-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn borrowed_process_namespace_payload_is_admitted_and_keeps_the_call_boundary() {
    let source = r#"module borrowed_namespace_command
import std.process
fn attach(borrow mut command: command, borrow namespace: Option<process.user_namespace>, slot: i64) -> Result<(), Error> {
  return match namespace {
    None => if slot == 0 { Ok(()) } else { Err(Error.Invalid) },
    Some(value) => command.inherit_namespace(value, slot),
  }
}
fn main() -> Result<(), Error> {
  mut command := process.command("/bin/echo", ["echo", "namespace absent"])
  absent: Option<process.user_namespace> := None
  attach(command, absent, 0)?
  return Ok(())
}
"#;
    assert_clean("r82-borrowed-process-namespace", source);
}

#[test]
fn nested_result_option_owned_return_preserves_the_view_read_and_owner() {
    let source = r#"module owned_document_return_test
Document { text: string }
Template { content: string }
Manifest { path: Option<string> }
fn bound(value: str) -> Result<Document, Error> {
  return Ok(Document { text: value.clone() })
}
fn decode(source: str) -> Result<Template, Error> {
  return Ok(Template { content: source.clone() })
}
fn wrap(borrow manifest: Manifest) -> Result<Option<Template>, Error> {
  return match manifest.path {
    None => Ok(None),
    Some(path) => {
      source := bound(path)?
      value := decode(source.text)?
      return Ok(Some(value))
    },
  }
}
fn main() -> i32 {
  manifest := Manifest { path: Some("payload".clone()) }
  result := wrap(manifest)
  length := match result {
    Err(_) => 0,
    Ok(option) => match option {
      None => 0,
      Some(value) => value.content.len() as i32,
    },
  }
  if length == 7 { return 42 }
  return 0
}
"#;
    assert_clean("r83-nested-owned-return", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-nested-owned-return", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r83-nested-owned-return-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn faithful_owned_document_return_keeps_all_result_leaves_certified() {
    // This retains the shape of the R83 provider reproduction: an optional borrowed manifest,
    // a local owned Document returned through Result, a field borrow into a decoder, and an owned
    // seven-field Template returned through Result<Option<_>>. It is deliberately broader than a
    // one-field Document control so a regression in a later owned leaf remains visible.
    let source = r#"module faithful_owned_document_return
Document { text: string, sha256: string, size: i64 }
Template { a: string, b: string, c: string, d: string, e: string, f: string, g: string }
Manifest { path: Option<string>, hash: Option<string> }
fn read_document(path: str) -> Result<Document, Error> = Ok(Document { text: path.clone(), sha256: "digest".clone(), size: path.len() })
fn bound(path: str, hash: str) -> Result<Document, Error> {
  source := read_document(path)?
  if source.sha256 != hash { return Err(Error.Invalid) }
  return Ok(source)
}
fn decode(source: str) -> Result<Template, Error> = Ok(Template {
  a: source.clone(), b: source.clone(), c: source.clone(), d: source.clone(),
  e: source.clone(), f: source.clone(), g: source.clone(),
})
fn wrap(borrow manifest: Manifest) -> Result<Option<Template>, Error> {
  return match manifest.path {
    None => Ok(None),
    Some(path) => {
      hash := match manifest.hash {
        None => { return Err(Error.Invalid) },
        Some(value) => { hash: str := value; hash },
      }
      source := bound(path, hash)?
      value := decode(source.text)?
      return Ok(Some(value))
    },
  }
}
fn main() -> i32 {
  manifest := Manifest { path: Some("payload".clone()), hash: Some("digest".clone()) }
  result := wrap(manifest)
  return match result {
    Err(_) => 0,
    Ok(option) => match option {
      None => 0,
      Some(value) => value.g.len() as i32,
    },
  }
}
"#;
    assert_clean("r83-faithful-owned-document-return", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-faithful-owned-document-return", source)
                .status
                .code(),
            Some(7)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r83-faithful-owned-document-return-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(7)
        );
    }
}

#[test]
fn owned_option_branch_replacement_keeps_nested_members() {
    // R80's non-loop witness: the successful Result arm installs an owned observation into an
    // Option, then the final owned result consumes that option. The source observation remains
    // usable for the scalar identity read and the nested member array must survive its expiry.
    let source = r#"module owned_option_branch_replacement
Member { path: string }
Observation { sha256: string, members: array<Member> }
Evidence { identity: string, file_set: Option<Observation> }
Readout { Ready(Evidence), Empty }

fn observe(ok: bool) -> Result<Observation, Error> {
  if !ok { return Err(Error.Invalid) }
  mut members: array_builder<Member> := array_builder()
  members.push(Member { path: "source".clone() })
  return Ok(Observation { sha256: "digest".clone(), members: members.build() })
}

fn identity(borrow files: Option<Observation>) -> string {
  return match files {
    None => "none".clone(),
    Some(value) => value.sha256.clone(),
  }
}

fn build(ok: bool) -> Result<Readout, Error> {
  mut retained: Option<Observation> := None
  result := observe(ok)
  match result {
    Err(_) => {},
    Ok(value) => {
      retained = Some(value)
    },
  }
  if !ok { return Ok(Readout.Empty) }
  name := identity(retained)
  return Ok(Readout.Ready(Evidence { identity: name, file_set: retained }))
}

fn main() -> i32 {
  result := build(true)
  return match result {
    Err(_) => 0,
    Ok(readout) => match readout {
      Empty => 0,
      Ready(value) => match value.file_set {
        None => 0,
        Some(files) => if value.identity == "digest" && files.members[0].path == "source" { 42 } else { 0 },
      },
    },
  }
}
"#;
    assert_clean("r80-owned-option-branch-replacement", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r80-owned-option-branch-replacement", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r80-owned-option-branch-replacement-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}
