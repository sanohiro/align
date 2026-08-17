# Recursive owned declared JSON plan

Status: **ACCEPTED DESIGN; IMPLEMENTATION PENDING**.

This document is the plan of record for align-llm Request 13. It replaces the
flat implementation boundary in `24-owned-json-plan.md` with one recursive,
view-free declared-record graph. The shipped Request 9 graph remains a strict
subset; there is no parallel flat/recursive API, descriptor, or runtime path.

The implementation starts only from a commit containing this reviewed design.
The public source form remains the inferred `core.json` surface. This is a
type-directed widening of one owned route, not a dynamic JSON value or a second
wire format.

## 1. Public-contract ledger

| Surface | Exact input and defaults | Result and errors | Ownership, lifetime, allocation | Compiler/runtime/package owner | Artifact and cache identity | Prerequisite, acceptance test, and metric |
| --- | --- | --- | --- | --- | --- | --- |
| `json.decode(input: str) -> Result<T, Error>` where `T` is an accepted recursive owned record | One positional `str`; no default, type argument, option object, environment input, or ambient allocator. Expected-result inference chooses one concrete `T`. | `Ok(T)` or existing `Error.Code(1)`. Capability, arity, inference, graph, cycle, layout, and mixed-view failures are compile-time diagnostics. UTF-8, string grammar, object syntax, duplicate, shape, range, missing-field, and trailing-input failures are recoverable. Capacity overflow and allocation failure retain the runtime-wide terminal-abort policy. | Every reachable `string`, owning option payload, dynamic-array spine, and owning array element is free-standing. The complete result is `Static`, independent of input and arena even inside `arena {}`. Decode publishes no partial value; recoverable failure drops the initialized recursive prefix exactly once. | `align_sema` selects and validates the graph; `align_interface` validates target-bound graph transport; checked HIR reconstructs the graph and route; MIR carries result ownership; LLVM emits one recursive descriptor table and canonical Drop; runtime parses, allocates, and cleans the recursive graph. No package owns semantics. | `OwnedJsonGraphDescV2` and `OwnedJsonInterfaceEnvelopeV2` below replace V1 for every owned record. Identity is structural over the complete reachable graph and target layout; the exported root association remains nominal within its unit. Interface format 8 and frontend/object keys consume the same transition. | Requests 7, 9, 12, and 15 are merged. The Align fixture owns the graph manifest and semantic/byte goldens; the implementation matrix in §9 owns compiler/runtime proof. Whole/per-unit allocation parity is a correctness measurement. No performance threshold is promised. |
| `json.encode(value: T) -> str` | One positional stable bound place of the same accepted `T`; no default or ambient input. The source is borrowed, never moved or mutated. | Infallible after checked graph/descriptor validation. | The existing template builder owns output exactly as before: arena-backed inside an arena, hidden scoped owner outside it. Persisting bytes still requires explicit `.clone()`. Every recursive source owner stays live and unchanged. | The shared semantic graph produces one checked recursive encode plan; HIR/MIR preserve the root borrow; LLVM calls the descriptor-driven builder; runtime walks the same field table used by decode. | Same V2 graph, target envelope, field order, type tags, and cache identity as decode. Canonical bytes use declaration order recursively. | Exact canonical vectors, source-nonmutation, round-trip, generic/import/cache, deep-shape, and concurrent-call owners in §9. No benchmark. |
| `json.encode_bounded(value: T, max_bytes: i64) -> Result<string, Error>` | Same stable bound place plus one positional exact `i64` inclusive limit; no default. Source graph validation precedes limit typing. | Existing Request 12 behavior: negative or first byte beyond the inclusive limit returns `Err(Error.Invalid)`; success returns `Ok(string)`; allocation failure is terminal. | Success owns one free-standing `string`; failure exposes no partial value. The source remains borrowed and unchanged. There is no unbounded-first pass or discarded public output. | Same graph and descriptor-driven writer as unbounded encode, using the existing bounded builder/finalizer. | Identical graph identity and canonical bytes to unbounded encode. Limit is an operation input, not schema/cache identity. | Exact-limit/next-byte rejection, byte parity, recursive failure, and source-liveness owners in §9. No performance threshold. |

All three operations share one classifier and one ordered graph. Acceptance by
only one operation is a compiler bug. There is no CLI/build input, persistent
configuration, option state, environment variable, process-global state,
connection-global state, reflection, artifact/source I/O, or overlap exclusion.
Concurrent calls use per-call parser, builder, destination, and cleanup state;
immutable descriptor tables may be shared.

## 2. Closed recursive graph

The root is one nonempty, natural-layout declared record. The operation selects
the owned route only when the complete reachable graph contains at least one
owned `string` leaf. With no owned leaf, the existing route owns validation and
is not narrowed. Once selected, the complete accepted grammar is:

```text
Record  := nonempty natural-layout declared record with Field+ and no cycle
Field   := Value
Value   := Int | Bool | string | Record | Option<Payload> | array<Element>
Payload := Int | Bool | string | Record | array<Element>
Element := Int | Bool | string | Record
Int     := i8 | i16 | i32 | i64 | u8 | u16 | u32 | u64
```

An `Option` may contain an accepted scalar, record, or dynamic array, but not
another `Option`: missing and JSON `null` are the same absence state, so nested
options could not round-trip. A dynamic array may contain a Copy integer/bool,
owned `string`, or an accepted record. Arrays of options or arrays and nested
dynamic arrays remain outside the language's element representation and are
rejected rather than encoded through a special
JSON-only type. Fixed arrays are not this runtime-sized C6 boundary.

Every reachable record must be nonempty, natural-layout, and acyclic after
generic substitution. `layout(C)`, `align(N)`, incomplete/unresolved types,
`str`, `array<str>`, floats, char, enums, `Result`, slices, fixed arrays, tuples,
boxes, resources, raw values, functions, builders, SoA, and every other type are
rejected before descriptor construction or runtime allocation. A `str` anywhere
inside a selected graph is the mixed-owned/borrowed diagnostic; no implicit
clone is inserted.

The C6 provenance is the clean align-llm commit
`871a0fad8284571b3500a964b45a40d20303c982`, whose
`docs/specs/c6-prompt-context-optimizer.md` Git blob is
`ed5d92b0ce6e445c931af0a600f3dec6b9c0995e` and whose exact file SHA-256 is
`a6e8d01e8298464921ea717c85f8fcb80d465fd95e5c1397e8e1298fd4b79e03`.
That sibling document is provenance only. The following complete Align-side
manifest is the authoritative Request 13 build input; no compiler test reads a
sibling checkout. A sibling change has no effect until this ledger, its checked-in
fixture, semantic goldens, and descriptor-byte goldens change together.

The sibling specification uses `str` as wire-schema notation. This manifest maps
every persisted text, path, digest, status, and discriminator field to owned
`string`; `array<str>` to `array<string>`; `Option<str>` to
`Option<string>`; every schema version, count, index, byte size, duration, seed,
and ppm field to `i64`; and every predicate field to `bool`. Label alternatives
such as `PARENT | CANDIDATE` remain semantically validated strings, not Align
enums. The sibling document contains both the shipped four-field renderer-core
`ContextPolicy` and the deferred C6b-memory extension. Request 13 deliberately
pins the seven-field C6b-memory form below because the persisted C6 graph consumes
that final policy shape; the four-field form is not a second accepted declaration.

Fields in every record below are in exact source declaration order. These 50
nominal records are the complete persisted declaration graph. The fixture
syntax-checks each persisted envelope as a root and checks the complete reachable
definition graph for every root; supporting records are not silently replaced by
shape-equivalent declarations.

```text
PromptTextArtifact {
  schema_version: i64
  artifact_kind: string
  artifact_id: string
  text: string
  content_sha256: string
}

ArtifactReference {
  artifact_kind: string
  path: string
  artifact_id: string
  content_sha256: string
}

PromptVariant {
  schema_version: i64
  artifact_kind: string
  variant_id: string
  base_prompt: PromptTextArtifact
  repo_prompt: PromptTextArtifact
  learned_prompt_append: string
  context_policy: ContextPolicy
  candidate_id: string
  content_sha256: string
}

ContextSources {
  schema_version: i64
  artifact_kind: string
  task_id: string
  patch_evaluation: PromptTextArtifact
  failure_memory_jsonl: PromptTextArtifact
  diagnostic_stdout: PromptTextArtifact
  diagnostic_stderr: PromptTextArtifact
  content_sha256: string
}

RenderedPromptArtifact {
  schema_version: i64
  artifact_kind: string
  task_id: string
  variant_id: string
  variant_sha256: string
  task_prompt_sha256: string
  context_sources_sha256: string
  text: string
  content_sha256: string
}

PromptScope {
  schema_version: i64
  artifact_kind: string
  repo_id: string
  repo_profile_revision: string
  align_revision: string
  corpus_id: string
  corpus_revision: CorpusRevision
  evaluation_provider_kind: string
  evaluation_provider_model: string
  generation_policy_sha256: string
  acceptance_policy_sha256: string
  base_prompt_sha256: string
  repo_prompt_sha256: string
  content_sha256: string
}

CorpusRevision {
  schema_version: i64
  artifact_kind: string
  source_kind: string
  source_repository_id: string
  source_sha256: string
  content_sha256: string
}

ContextPolicy {
  include_patch_evaluation: bool
  include_failure_memory: bool
  include_diagnostics: bool
  max_patch_evaluation_bytes: i64
  max_failure_events: i64
  max_failure_context_bytes: i64
  max_diagnostic_bytes_per_stream: i64
}

ArtifactDigest {
  path: string
  mode: string
  byte_count: i64
  sha256: string
}

ArtifactExpectation {
  path: string
  kind: string
  expected_sha256: string
}

SnapshotRequest {
  schema_version: i64
  artifact_kind: string
  task_id: string
  project_root: string
  repo_path: string
  repo_revision: string
  require_clean_repo: bool
  static_expectations: array<ArtifactExpectation>
  additional_files: array<string>
  workspace_path: string
  allowed_workspace_entries: array<string>
  content_sha256: string
}

WorkspacePreflightRequest {
  schema_version: i64
  artifact_kind: string
  evaluation_id: string
  project_root: string
  workspace_path: string
  content_sha256: string
}

WorkspacePreflightResult {
  schema_version: i64
  artifact_kind: string
  evaluation_id: string
  status: string
  error_code: string
  error: string
  physical_project_root: string
  physical_workspace_path: string
  environment_probe: Option<EnvironmentProbe>
  content_sha256: string
}

SnapshotResult {
  schema_version: i64
  artifact_kind: string
  task_id: string
  status: string
  error_code: string
  error: string
  environment_probe: Option<EnvironmentProbe>
  artifact_digests: array<ArtifactDigest>
  content_sha256: string
}

TaskInputSnapshot {
  schema_version: i64
  artifact_kind: string
  task_id: string
  task_manifest_sha256: string
  artifact_digests: array<ArtifactDigest>
  environment_sha256: string
  content_sha256: string
}

RunSnapshotAttestation {
  schema_version: i64
  artifact_kind: string
  task_id: string
  sample_index: i64
  variant: string
  status: string
  error_code: string
  error: string
  snapshot_request_sha256: string
  before_snapshot_result_sha256: string
  after_snapshot_result_sha256: Option<string>
  before_input_snapshot_sha256: Option<string>
  after_input_snapshot_sha256: Option<string>
  content_sha256: string
}

GenerationPolicy {
  schema_version: i64
  artifact_kind: string
  generation_policy_id: string
  evaluation_provider_kind: string
  evaluation_provider_endpoint_id: string
  evaluation_provider_model: string
  provider_control_sha256: string
  provider_service_revision: string
  max_prompt_bytes: i64
  max_tokens: i64
  temperature_micros: i64
  seed_mode: string
  seed_base: i64
  content_sha256: string
}

EvaluationProviderControl {
  schema_version: i64
  artifact_kind: string
  provider_control_id: string
  provider_kind: string
  endpoint: string
  endpoint_id: string
  model: string
  api_key_env: Option<string>
  tokenize_endpoint: Option<string>
  timeout_ns: i64
  max_response_bytes: i64
  content_sha256: string
}

EnvironmentPolicy {
  schema_version: i64
  artifact_kind: string
  policy_id: string
  allowed_variables: array<EnvironmentVariable>
  executable_paths: array<string>
  locale: string
  content_sha256: string
}

EnvironmentVariable {
  name: string
  non_secret_value: string
  source: string
  precedence: i64
}

PromptSourceVerifierPolicy {
  schema_version: i64
  artifact_kind: string
  policy_id: string
  helper_path: string
  helper_sha256: string
  helper_runtime: string
  git_executable_sha256: string
  content_sha256: string
}

TaskAdapterRequest {
  schema_version: i64
  artifact_kind: string
  evaluation_id: string
  task_id: string
  sample_index: i64
  variant: string
  variant_path: string
  variant_sha256: string
  rendered_prompt_path: string
  rendered_prompt_sha256: string
  generation_policy_path: string
  generation_policy_sha256: string
  provider_control_path: string
  provider_control_sha256: string
  workspace_path: string
  result_path: string
  paired_seed: i64
  credential_env_name: Option<string>
  environment_policy_sha256: string
  content_sha256: string
}

EnvironmentProbe {
  schema_version: i64
  artifact_kind: string
  producer: string
  os: string
  os_release: string
  architecture: string
  cpu: string
  logical_cpu_count: Option<i64>
  gpu: string
  runtime_identity: string
  content_sha256: string
}

EnvironmentIdentityCore {
  schema_version: i64
  artifact_kind: string
  os: string
  os_release: string
  architecture: string
  cpu: string
  logical_cpu_count: Option<i64>
  gpu: string
  align_llm_commit: string
  align_revision: string
  measurement_adapter_runtime: string
  snapshot_helper_runtime: string
  source_verifier_runtime: string
  source_verifier_policy_sha256: string
  environment_policy_sha256: string
}

EnvironmentIdentity {
  schema_version: i64
  artifact_kind: string
  core: EnvironmentIdentityCore
  environment_id: string
  content_sha256: string
}

EvaluationInputIdentity {
  schema_version: i64
  artifact_kind: string
  task_id: string
  task_input_snapshot_sha256: string
  parent_variant_sha256: string
  candidate_variant_sha256: string
  task_prompt_sha256: string
  context_sources_sha256: string
  generation_policy_sha256: string
  generation_request_sha256: string
  adapter_request_sha256: string
  environment_policy_sha256: string
  environment_sha256: string
  sample_index: i64
  paired_seed: i64
  content_sha256: string
}

GenerationRequestIdentity {
  schema_version: i64
  artifact_kind: string
  rendered_prompt_sha256: string
  system_text_sha256: string
  user_text_sha256: string
  generation_policy_sha256: string
  provider_control_sha256: string
  environment_policy_sha256: string
  max_tokens: i64
  temperature_micros: i64
  paired_seed: i64
  provider_request_sha256: string
  seed_attestation_sha256: string
  content_sha256: string
}

SeedCapabilityAttestation {
  schema_version: i64
  artifact_kind: string
  provider_kind: string
  provider_model: string
  requested_seed: i64
  result: string
  applied_seed: Option<i64>
  provider_request_sha256: string
  content_sha256: string
}

CandidateProposal {
  schema_version: i64
  summary: string
  learned_prompt_append: string
  context_policy: ContextPolicy
}

PromptExperimentResult {
  schema_version: i64
  artifact_kind: string
  experiment_id: Option<string>
  status: string
  error_code: string
  error: string
  parent_activation: Option<ArtifactReference>
  scope: Option<PromptScope>
  opportunity: Option<ArtifactReference>
  proposal_provider_kind: Option<string>
  proposal_provider_endpoint_id: Option<string>
  proposal_provider_model: Option<string>
  proposal_elapsed_ns: i64
  proposal_status_code: Option<i64>
  proposal_summary: string
  candidate_variant: Option<PromptVariant>
  bounded_provider_output: string
  content_sha256: string
}

PromptSourceVerifierRequest {
  schema_version: i64
  artifact_kind: string
  mode: string
  align_llm_repository_path: string
  expected_align_llm_commit: string
  tested_align_llm_head: Option<string>
  align_repository_path: string
  expected_align_revision: string
  corpus_source_path: string
  corpus_source_kind: string
  corpus_file_set_manifest_path: Option<string>
  expected_corpus_source_repository_id: string
  expected_corpus_source_sha256: string
  git_executable_path: string
  git_executable_sha256: string
  content_sha256: string
}

PromptSourceVerifierResult {
  schema_version: i64
  artifact_kind: string
  status: string
  error_code: string
  error: string
  align_llm_reachability: string
  align_llm_observed_head: Option<string>
  align_reachability: string
  align_observed_revision: Option<string>
  corpus_reachability: string
  corpus_observed_source_sha256: Option<string>
  content_sha256: string
}

PromptEvaluationCorpus {
  schema_version: i64
  artifact_kind: string
  corpus_id: string
  corpus_revision: CorpusRevision
  task_files: array<string>
  content_sha256: string
}

PromptEvaluationTask {
  schema_version: i64
  artifact_kind: string
  task_id: string
  repo_id: string
  repo_revision: string
  repo_path: string
  require_clean_repo: bool
  cmd: string
  argv: array<string>
  snapshot_cmd: string
  snapshot_argv: array<string>
  measurement_adapter_runtime: string
  snapshot_helper_runtime: string
  cwd: string
  timeout_ns: i64
  task_prompt_path: string
  context_sources_path: string
  generation_policy_path: string
  provider_control_path: string
  environment_policy_path: string
  artifacts: array<ArtifactExpectation>
  regression_limits: RegressionLimits
  content_sha256: string
}

RegressionLimits {
  maximum_unrelated_diff_count: i64
  maximum_patch_size_bytes: i64
  maximum_public_api_change_count: i64
  maximum_repair_loops: i64
  maximum_benchmark_regression_ppm: Option<i64>
}

TaskMeasurement {
  schema_version: i64
  artifact_kind: string
  status: string
  failure_kind: string
  build_status: string
  test_status: string
  repair_loop_count: i64
  unrelated_diff_count: i64
  patch_size_bytes: i64
  public_api_change_count: i64
  policy_violation_count: i64
  cleanup_passed: bool
  containment_passed: bool
  benchmark_regression_ppm: Option<i64>
  generation_to_passing_patch_ns: Option<i64>
  rendered_prompt_sha256: string
  generation_request: GenerationRequestIdentity
  environment_probe: EnvironmentProbe
  seed_attestation: SeedCapabilityAttestation
  diagnostic_summary: string
  diagnostic_stdout: string
  diagnostic_stderr: string
  content_sha256: string
}

PromptTaskRow {
  schema_version: i64
  artifact_kind: string
  evaluation_id: string
  task_id: string
  sample_index: i64
  variant: string
  variant_id: string
  variant_sha256: string
  prompt_preparation_ns: i64
  time_to_passing_patch_ns: Option<i64>
  evaluation_input: EvaluationInputIdentity
  measurement: TaskMeasurement
  content_sha256: string
}

PromptTraceOverflow {
  schema_version: i64
  artifact_kind: string
  attempted_invocation_count: i64
  trace_record_count: i64
  trace_digest_sha256: string
  content_sha256: string
}

PromptEvaluationResult {
  schema_version: i64
  artifact_kind: string
  evaluation_id: Option<string>
  status: string
  error_code: string
  error: string
  experiment: Option<ArtifactReference>
  experiment_artifact: Option<PromptExperimentResult>
  parent_activation: Option<ArtifactReference>
  parent_activation_artifact: Option<PromptActivationResult>
  scope: Option<PromptScope>
  parent_variant: Option<PromptVariant>
  candidate_variant: Option<PromptVariant>
  corpus_source: Option<ArtifactReference>
  corpus: Option<PromptEvaluationCorpus>
  tasks: array<PromptEvaluationTask>
  acceptance_policy_source: Option<ArtifactReference>
  acceptance_policy: Option<PromptAcceptancePolicy>
  generation_policy_source: Option<ArtifactReference>
  generation_policy: Option<GenerationPolicy>
  provider_control_source: Option<ArtifactReference>
  provider_control: Option<EvaluationProviderControl>
  workspace_preflight_source: Option<ArtifactReference>
  workspace_preflight_request: Option<WorkspacePreflightRequest>
  workspace_preflight: Option<WorkspacePreflightResult>
  environment: Option<EnvironmentIdentity>
  snapshot_requests: array<SnapshotRequest>
  snapshot_results: array<SnapshotResult>
  input_snapshots: array<TaskInputSnapshot>
  snapshot_attestations: array<RunSnapshotAttestation>
  trace_failure: Option<PromptTraceOverflow>
  sample_count: i64
  gate_eligible: bool
  rows: array<PromptTaskRow>
  task_aggregates: array<TaskAggregate>
  corpus_aggregate: Option<CorpusAggregate>
  serious_regression_reasons: array<RegressionReason>
  content_sha256: string
}

PromptVerifierTrust {
  schema_version: i64
  artifact_kind: string
  expected_align_llm_commit: string
  expected_align_revision: string
  expected_corpus_source_kind: string
  expected_corpus_source_repository_id: string
  expected_corpus_source_sha256: string
  align_llm_reachability: string
  align_llm_observed_head: Option<string>
  align_reachability: string
  align_observed_revision: Option<string>
  corpus_reachability: string
  corpus_observed_source_sha256: Option<string>
  content_sha256: string
}

PromptExpectedInputDigest {
  schema_version: i64
  artifact_kind: string
  task_id: string
  sample_index: i64
  variant: string
  rendered_prompt_sha256: string
  context_sources_sha256: string
  generation_request_sha256: string
  adapter_request_sha256: string
  provider_request_sha256: string
  content_sha256: string
}

PromptEvaluationEvidence {
  schema_version: i64
  artifact_kind: string
  evaluation_id: string
  evaluation_result_sha256: string
  trust: PromptVerifierTrust
  expected_inputs: array<PromptExpectedInputDigest>
  content_sha256: string
}

PromptActivation {
  schema_version: i64
  artifact_kind: string
  activation_id: string
  operation: string
  scope: PromptScope
  parent_activation_id: string
  parent_activation_sha256: string
  effective_variant: PromptVariant
  accepted_evaluation_id: string
  accepted_evaluation_sha256: string
  rollback_target_activation_id: string
  rollback_target_activation_sha256: string
  decision_reason: string
  content_sha256: string
}

PromptActivationResult {
  schema_version: i64
  artifact_kind: string
  decision_id: Option<string>
  status: string
  error_code: string
  error: string
  activation: Option<PromptActivation>
  content_sha256: string
}

PromptAcceptancePolicy {
  schema_version: i64
  artifact_kind: string
  policy_id: string
  minimum_task_count: i64
  minimum_samples_per_variant: i64
  minimum_completion_gain_count: i64
  minimum_time_improvement_ppm: i64
  maximum_time_regression_ppm: i64
  maximum_repair_loop_regression_count: i64
  content_sha256: string
}

TaskAggregate {
  task_id: string
  parent_pass_count: i64
  candidate_pass_count: i64
  parent_repair_loop_count: i64
  candidate_repair_loop_count: i64
  paired_pass_count: i64
  parent_paired_median_time_ns: Option<i64>
  candidate_paired_median_time_ns: Option<i64>
  time_improvement_ppm: Option<i64>
  time_regression_ppm: Option<i64>
}

CorpusAggregate {
  task_count: i64
  sample_count: i64
  parent_pass_count: i64
  candidate_pass_count: i64
  parent_repair_loop_count: i64
  candidate_repair_loop_count: i64
  paired_pass_count: i64
  parent_paired_median_time_ns: Option<i64>
  candidate_paired_median_time_ns: Option<i64>
  completion_gain_count: i64
  time_improvement_ppm: Option<i64>
  time_regression_ppm: Option<i64>
  repair_loop_regression_count: i64
}

RegressionReason {
  task_id: string
  sample_index: i64
  code: string
  parent_value: string
  candidate_value: string
  limit: string
}

PromptGateManifest {
  schema_version: i64
  artifact_kind: string
  gate_id: string
  source_locator: PromptGateSourceLocator
  baseline_activation: ArtifactReference
  improved_evaluation: ArtifactReference
  improved_evaluation_evidence: ArtifactReference
  accepted_activation: ArtifactReference
  rollback_activation: ArtifactReference
  content_sha256: string
}

PromptGateSourceLocator {
  schema_version: i64
  artifact_kind: string
  source_bundle_id: string
  align_llm_source_relative_path: string
  align_source_relative_path: string
  corpus_source_relative_path: string
  corpus_file_set_manifest_relative_path: Option<string>
  source_verifier_policy_relative_path: string
  source_verifier_policy_sha256: string
  source_verifier_relative_path: string
  source_verifier_sha256: string
  source_verifier_runtime: string
  git_executable_sha256: string
  content_sha256: string
}
```

`PromptRender`, top-level command request records such as
`PromptEvaluateRequest`, and scorer-internal `Score*` records are not persisted
through this capability and are not roots. The content-bound helper and adapter
request records explicitly listed above are persisted and remain in the graph.
If a consumer later persists another record, it must be added here with exact
ordered fields and exercised without widening the accepted type grammar
implicitly.

The maximum reachable constructor depth is 128, measured with the root record
at depth one and incremented at every record, `Option`, or dynamic-array edge.
This matches the existing JSON parser's adversarial-input nesting bound and
keeps runtime descriptor walks
stack-safe without a hidden scratch allocation. Depth 128 is accepted and 129
is a compile-time graph error before layout or allocation. Shared DAG nodes count
at each path for depth but are serialized once. Record and field counts, names,
descriptor lengths, and offsets remain `u32`-bounded with checked arithmetic.

The declaration and positional calls remain separate:

```align
OwnedLeaf {
  ok: bool
  text: string
}

OwnedEnvelope {
  version: u16
  child: OwnedLeaf
  note: Option<string>
  items: array<OwnedLeaf>
}
```

```align
import core.json

fn decode_envelope(input: str) -> Result<OwnedEnvelope, Error> {
  return json.decode(input)
}

fn print_envelope(borrow value: OwnedEnvelope) {
  print(json.encode(value))
}

fn encode_envelope_bounded(
  borrow value: OwnedEnvelope,
  max_bytes: i64,
) -> Result<string, Error> {
  return json.encode_bounded(value, max_bytes)
}
```

## 3. Allocation, transfer, and cleanup

The Request 9 ownership boundary remains unchanged and now applies to the
complete graph:

```text
borrowed JSON graph     input/arena-bounded views
fully owned JSON graph  free-standing owners at every reachable node
mixed graph             compile-time rejection
```

The root carries one path-local cleanup bit. A successful decode sets it only
after the complete graph is initialized. Every child allocation uses the same
free-standing mode; there is no arena fallback, per-member mode, or hidden clone.
The result therefore composes with the ordinary recursive Move carrier through
raw `Result`, parameter passing, return, replacement, `if`, `match`, `else`, `?`,
`map_err`, branch/loop joins, and early exits. Moving clears the complete source;
replacement drops the old graph before publishing the new graph.

Construction and failure use one deterministic transition ledger:

1. zero the complete root slot;
2. visit record fields in source declaration order;
3. construct a required nested record in its zeroed destination; recursive root
   cleanup may visit that partial record because every not-yet-live owner is null;
4. construct an option payload while its tag is `None`; if construction fails,
   clean that partial payload locally, and set `Some` only after success;
5. for an array, retain the spine owner, completed-element count, and one zeroed
   current element; publish the new length only after that element succeeds;
6. if an array's current element fails, clean that zeroed partial element
   locally; on the enclosing recoverable failure, walk root fields in source
   order. A required nested record follows its own source order, an owning
   option drops only its tagged `Some` payload, and an array drops its published
   completed elements in ascending index order, then the spine;
7. null each owner or tag as it is released so cleanup is idempotent and a later
   aggregate Drop observes no second owner.

Scalar arrays flat-free their spine. `array<string>` drops strings in ascending
index order then its spine. `array<Record>` recursively drops each completed
record in ascending index order then the spine. An `Option<array<Record>>`
applies the option rule outside the same array rule. Cleanup performs no
allocation. Capacity arithmetic and allocation failure remain terminal aborts
and make no cleanup-after-abort promise.

## 4. Wire, canonical bytes, and errors

Request 7 remains the sole JSON string grammar. Owned text accepts the RFC 8259
short escapes and valid UTF-16 surrogate pairs, decodes `\u0000` to embedded NUL,
and rejects malformed escapes, raw C0 bytes, invalid UTF-8, and invalid surrogate
sequences. Ignored keys and their complete values are validated with the same
grammar before being discarded. Repeated unknown keys remain ignored; a repeated
declared key is a recoverable error at the second occurrence.

Every source `str` is valid UTF-8 by type. A literal NUL byte is invalid JSON
syntax inside an unescaped string; `\u0000` is accepted and becomes an embedded
NUL in the owned value. Compiler-produced field names are nonempty ASCII
identifiers and therefore never contain NUL or need key escaping. Interface
target triples are nonempty ASCII without NUL. Graph/type/interface validation
finishes before a runtime call; runtime syntax errors may occur after earlier
internal allocations, but cleanup completes before the recoverable error is
published and there is no filesystem, process, network, or package side effect.

Every object is encoded in source declaration order recursively. Signed and
unsigned integers retain their exact width; range failures are recoverable and
full-range `u64` never passes through `i64`. Bool accepts only `true`/`false`.
Required fields reject missing and `null`. `Option<T>` maps missing or `null` to
`None`; encode omits `None`, while `Some` emits the payload. Arrays preserve
element order and reject `null` unless the element grammar itself gains an
option representation in a separately reviewed design.

The option/array product is exhaustive: each allowed option payload class has
missing, explicit `null`, and `Some` vectors; each allowed array element class
has empty, one-element, multi-element, and current-element-failure vectors;
record elements cross those states with their own nested options and arrays.
Row order is always declaration order for records and index order for arrays.
There is no unavailable-value sentinel other than `None`.

The normative semantic/byte vector is:

```text
input UTF-8 bytes:
{"version":1,"child":{"ok":true,"text":"nul:\u0000"},"note":null,"items":[{"ok":false,"text":"\u20ac"}],"ignored":{"deep":[1,true,"ok"]}}

decoded semantics:
version = 1
child = { ok = true, text = bytes "nul:" + NUL }
note = None
items = [{ ok = false, text = U+20AC }]

canonical output UTF-8 bytes:
{"version":1,"child":{"ok":true,"text":"nul:\u0000"},"items":[{"ok":false,"text":"€"}]}
```

The fixture decodes the input, drops its input owner, checks every retained
leaf, encodes to the exact output before any CLI newline, decodes that output,
and requires semantic stability. Independent vectors cover `None`, `Some` of
each accepted constructor, empty/nonempty arrays at every element class, exact
integer boundaries, embedded NUL, escaped and clean UTF-8, unknown fields, and
every recoverable failure after each live-owner prefix. Canonical declared-record
bytes are bytewise stable; noncanonical input with unknown fields is not.

Bounded encode evaluates the already-checked source plan, then `max_bytes`, then
initializes the builder. A negative limit creates the existing sticky-invalid
builder with zero payload allocation. Exact byte length succeeds; one byte less
returns `Error.Invalid`. Both results preserve the source and match the
unbounded canonical bytes on success.

## 5. Checked graph descriptor and interface envelope

`OwnedJsonGraphDescV2` replaces `OwnedJsonDescV1`. It is compiler-private,
canonical, target-local, and never serialized naked. The interface carries it
only inside `OwnedJsonInterfaceEnvelopeV2`. Neither is a public file format,
wire format, or reflection surface.

All integers below are little-endian. Records are numbered by root-first DFS,
following source-ordinal fields and assigning an ordinal on first encounter.
A repeated DAG record uses its first ordinal; an active-node encounter is the
cycle error. Record names are excluded from structural bytes. Field names are
included because they are JSON wire keys.

```text
descriptor := u8 schema_version (= 2)
              u8 layout_mode (= 0, natural only)
              u8 layout_algorithm (= 1, descending alignment; stable declaration ties)
              u32 record_count_le (non-zero)
              u32 root_record_ordinal_le (= 0)
              record[record_count]

record := u32 layout_size_le
          u32 layout_align_le
          u8 allocation_mode       // 0 Copy, 1 contains free-standing owner
          u8 drop_tag              // 0 Copy, 2 recursive record
          u32 field_count_le        // non-zero
          field[field_count]

field := u32 name_len_le
         u8[name_len] name_ascii
         type_node
         u32 physical_offset_le     // record-base-relative

type_node := u8 type_tag
             u32 layout_size_le
             u32 layout_align_le
             u8 allocation_mode     // 0 Copy, 1 free-standing owner
             u8 drop_tag
             type_payload

type 0x01 integer := u8 bits, u8 unsigned  // bits 8/16/32/64; unsigned 0/1
type 0x03 bool    := empty
type 0x10 string  := empty
type 0x20 record  := u32 record_ordinal_le
type 0x21 Option  := u32 tag_offset_le, u32 payload_offset_le, type_node payload
type 0x22 array   := u8 element_drop_plan_version (= 1), type_node element

drop 0x00 Copy
drop 0x01 string
drop 0x02 recursive record
drop 0x03 owning Option
drop 0x04 owning array
```

An option or record whose reachable payload is Copy uses allocation/drop
`0/0`; an owning option uses `1/3`, an owning record `1/2`, and every dynamic
array `1/4`. A record reference's size, alignment, allocation, and drop cells
must equal its referenced record. An option fixes tag/payload offsets and embeds
the complete payload node. An array embeds the complete element node; its
element cannot itself be option or array. Field offsets, sizes, alignments, and
record sizes must equal `TypeLayoutCache` and the canonical natural-layout
algorithm. Names are nonempty ASCII Align identifiers and unique per record.
Counts, nested node lengths, offsets, and total length use checked arithmetic;
unknown/reserved tags and trailing bytes reject.

Interface format changes atomically from 7 to exactly 8. Format 8 replaces
`owned_json_descriptors` with `owned_json_graphs` at the same canonical position
after exported struct definitions and before exported enums. Format 7 rejects as
`UnknownVersion(7)` before graph count or envelope bytes are read. There is no
V1/V2 compatibility decoder or old/new descriptor list.

The graph list is sorted by exported local root name:

```text
owned_json_graph_entry := u32 type_name_len_le
                          u8[type_name_len] type_name_ascii
                          u32 envelope_len_le
                          u8[envelope_len] envelope

envelope := u8 envelope_version (= 2)
            u32 target_triple_len_le
            u8[target_triple_len] llvm_canonical_target_triple_ascii
            u8 object_format       // 0 ELF, 1 Mach-O
            u8 endian              // 0 little
            u8 pointer_size        // 8
            u8 pointer_align       // 8
            u8 bool_size           // 1
            u8 bool_align          // 1
            u8 string_size         // 16
            u8 string_align        // 8
            u8 array_header_size   // 16
            u8 array_header_align  // 8
            u8 option_tag_size     // 1
            u8 option_tag_align    // 1
            u64 abi_hash_lo_le
            u64 abi_hash_hi_le
            u32 descriptor_len_le
            u8[descriptor_len] OwnedJsonGraphDescV2
```

The ABI hash is existing `Hash128::of` over bytes from envelope version through
option-tag alignment. It is a local consistency identity, not authenticity.
The enclosing interface hash covers root name, complete envelope, and graph.
Every accepted non-generic exported root receives one entry. Exported reachable
types are already required to be public and present in the summary. Concrete
generic monomorphs construct V2 in the consumer and fold it into structural MIR
implementation identity; private roots do the same locally. A private body edit
does not invalidate consumers. Root association is nominal within `summary.unit`;
all graph bytes are structural over the resolved reachable definitions.

The required baseline is x86_64 Ubuntu 24.04 with Rust 1.96 and LLVM 22. Release
acceptance also covers aarch64 Ubuntu 24.04 and aarch64 macOS 15 with those
compiler majors. All three are 64-bit little-endian and must match the encoded
cells; a future target with different cells rejects before graph offsets are
read. No 32-bit or big-endian contract is added.

For the required x86_64 Linux baseline, the exact envelope prefix and hash are:

```text
prefix 02 13 00 00 00 78 38 36 5f 36 34 2d 70 63 2d 6c
       69 6e 75 78 2d 67 6e 75 00 00 08 08 01 01 10 08
       10 08 01 01
hash   17 73 45 bb fc 42 7d 00 dc a3 b5 9c f9 79 f1 c8
```

For the `OwnedEnvelope`/`OwnedLeaf` declarations in §2, natural layouts are
`OwnedEnvelope { size 72, align 8, version@64, child@0, note@24, items@48 }`
and `OwnedLeaf { size 24, align 8, ok@16, text@0 }`. The exact 221 descriptor
bytes are:

```text
02 00 01 02 00 00 00 00 00 00 00 48 00 00 00 08
00 00 00 01 02 04 00 00 00 07 00 00 00 76 65 72
73 69 6f 6e 01 02 00 00 00 02 00 00 00 00 00 10
01 40 00 00 00 05 00 00 00 63 68 69 6c 64 20 18
00 00 00 08 00 00 00 01 02 01 00 00 00 00 00 00 00
04 00 00 00 6e 6f 74 65 21 18 00 00 00 08 00 00
00 01 03 00 00 00 00 08 00 00 00 10 10 00 00 00
08 00 00 00 01 01 18 00 00 00 05 00 00 00 69 74
65 6d 73 22 10 00 00 00 08 00 00 00 01 04 01 20
18 00 00 00 08 00 00 00 01 02 01 00 00 00 30 00
00 00 18 00 00 00 08 00 00 00 01 02 02 00 00 00
02 00 00 00 6f 6b 03 01 00 00 00 01 00 00 00 00
00 10 00 00 00 04 00 00 00 74 65 78 74 10 10 00
00 00 08 00 00 00 01 01 00 00 00 00
```

The independent golden owner constructs the semantic graph without calling the
production encoder, requires these descriptor/prefix/hash bytes, decodes them
back to the semantic graph, and checks every layout cell. Mutations cover every
header, count, ordinal, name, tag, nested node, layout, allocation/drop,
option/array payload, target/ABI/hash, truncation, overflow, duplicate/DAG/cycle,
cardinality, ordering, and trailing-byte boundary.

Interface and graph decoding report one deterministic first cause:

1. interface format version; format 7 stops before any V2 list byte;
2. list count, then each entry's name length/bound, ASCII identifier grammar,
   known exported non-generic root, uniqueness/order, envelope length, and exact
   envelope bound;
3. envelope version, target-triple length/bound/ASCII grammar, object format,
   endian, then the pointer/bool/string/array/option ABI cells in encoded order;
4. stored ABI hash, current target triple, current object format, and current ABI
   tuple, before descriptor length or an offset is trusted;
5. descriptor length/bound and exact envelope end;
6. descriptor version, layout mode, layout algorithm, nonzero record count, and
   root ordinal;
7. each encoded record in ordinal order: size, alignment, allocation/drop,
   nonzero field count, then each source-ordinal field's name length/bound,
   grammar, uniqueness/order, type tag, tag-specific nested payload, and physical
   offset. Nested type nodes check size, alignment, allocation/drop before their
   payload cells;
8. exact descriptor end, record-reference bounds, DFS order/DAG/cycle/depth,
   referenced-node equality, semantic graph/layout/Drop equality, then exact list
   cardinality with no missing/extra accepted root.

The first failing check is the only diagnostic. An imported malformed or
cross-target graph rejects before cache lookup, checked HIR, LLVM, or runtime;
it is never silently rebuilt. A well-formed object-cache identity difference is
an ordinary miss and rebuilds from current checked HIR.

## 6. Runtime table and routing

No runtime symbol, C signature, or public source operation is added. Owned decode
continues to call A103 `align_rt_json_decode` with a null arena. V2 produces one
immutable `JsonSubTable` per graph record and uses existing `JsonField.sub` edges.
Kinds `8` (owned string) and `9` (owned string array) remain the leaf allocation
kinds; nested records use kind 4, arrays of records kind 5, scalar arrays kind 7,
and options use the existing record-relative `opt_tag`. The implementation
extends the recursive table generator and failure cleanup, not the A103 ABI.
The strict fallback and indexed/Mison path consume the identical V2 tables and
must agree on key duplicates, escapes, range/shape errors, live-prefix cleanup,
and unknown-field validation at every nested record.

A80 `align_rt_json_encode_object` becomes the single V2 object writer for both
flat and recursive owned roots. Its existing kind dispatch admits kinds 8 and 9
as the read-only layout equivalents of string and string-array writers; nested
kind 4/5/7 tables recurse through the same validated graph. Unbounded and bounded
encode share this call and builder. The Request 9 direct owned template pieces
are removed rather than retained as a second flat path.

Routing remains operation-specific:

| Entry class | Rule |
| --- | --- |
| direct record decode/encode/bounded encode with a transitive owned string | V2 recursive owned route after complete graph validation. |
| direct record with no transitive owned string | Existing route, unchanged. |
| top-level scalar or scalar array | Existing scalar route, unchanged. |
| top-level `array<Record>` | Existing AoS route; it does not select V2 from its element. |
| fixed `StructArray`, union, SoA, scanner, or `json.doc` | Existing target-specific route, unchanged. |

The existing 17 operation variants and 153 unordered same-process pairs remain
the complete overlap product. Each owned variant now exercises flat, nested,
optional, string-array, scalar-array, and record-array graph shapes. No lock,
serialization, mutable descriptor state, or failed-second-operation behavior is
introduced.

## 7. Checked HIR and MIR boundary

The existing `JsonOwnedDecode`, `JsonOwnedEncode`, and
`JsonOwnedEncodeBounded` discriminants remain the dedicated route. Their flat
V1 plan is replaced atomically by `OwnedJsonGraphPlanV2 { root, graph }`.
`JsonOwnedEncode` and bounded encode each carry one stable root place plus that
checked plan; recursive field accesses are runtime descriptor walks rather than
an unrolled HIR template spine. This removes the direct owned template-part
variants and prevents graph depth from becoming expression depth.

The active checked-HIR gate independently reconstructs V2 from the root and
definitions, validates the exact graph/depth/layout/Drop/envelope, compares it
to the stored plan, and checks the operation envelope. Decode requires `str`
input and exact `Result<Struct, Error>` output with free-standing ownership at
every arena depth. Encode requires one stable bound source place and exact `str`
output; bounded encode additionally checks `max_bytes: i64` and exact
`Result<string, Error>`. All accesses borrow the root and no plan can move or
mutate it.

Whole-program, located, per-unit, and located-per-unit lowering run the same
active gate. Interface import validates V2 before cache lookup or HIR use.
Compiler graph discovery, descriptor encode/decode, validation, Drop-plan
comparison, structural hashing, clone/replay, and table construction use
explicit worklists with the 128-level rule; no new recursive Rust visitor or
wildcard enum arm may turn a deep/malformed graph into a compiler panic or a
skipped analysis. MIR owned encode/decode remain out-of-line-dispatched so deep
surrounding expression trees stay stack-safe.

## 8. Deterministic validation and error precedence

Route selection performs a non-diagnosing iterative scan for any reachable
owned `string`, skipping unsupported edges only for selection. If none exists,
the existing route owns every later result. Once selected, the only graph
diagnostic follows this order:

1. capability import, arity, and expected/source type inference;
2. root record identity, then root `layout(C)`, then root `align(N)`;
3. root-first DFS in source-field order. At each first record encounter: known
   complete definition, nonempty record, `layout(C)`, `align(N)`, depth, then
   fields. At each field: resolved completeness, outer constructor, exact
   integer width/sign or constructor payload. `str`/`array<str>` wins the mixed
   graph diagnostic; other excluded constructors use the unsupported graph
   diagnostic. An active record edge reports the cycle at that field. A completed
   shared DAG record is not revalidated;
4. canonical natural layout, recursive Drop plan, one free-standing allocation
   mode, V2 graph bytes, target envelope, and operation operand/result types;
5. for bounded encode only, limit expression checking and exact `i64` after the
   complete source plan.

At runtime, full input UTF-8/string-token validation precedes object parsing.
Object members are consumed in input order: syntax, declared-key duplicate,
value shape, integer range, nested record/option/array failure; missing required
fields are checked in declaration order at object close; trailing non-whitespace
follows. The first recoverable error is retained while cleanup runs and no
partial result is published. A parameterized multi-invalid owner permutes every
phase through sema, interface decode, checked-HIR replay, and all lowerers.

## 9. Implementation closure matrix

| Cell | Implementation owner | Required owner |
| --- | --- | --- |
| route selection, concrete generics, grammar, cycle/DAG/constructor-depth, explicit layout, mixed graph, and first-invalid order | one iterative `align_sema` V2 classifier shared with checked-HIR reconstruction | `recursive_owned_json_formation_matrix`, including every tag, depth 128/129, generic substitution, and multi-invalid permutation |
| exact C6 graph coverage and exclusions | checked-in Align declarations generated from the pinned 50-record, 543-field manifest, syntax checked separately from positional calls | `recursive_owned_json_c6_graph_manifest`, generated exact field/type/order/dependency assertions and per-root semantic/descriptor goldens, plus one exclusion per unsupported constructor |
| checked-HIR envelope, stored/rebuilt plan, malformed-node refusal, and enum-consumer sweep | existing `JsonOwned*` nodes with V2 plan; exhaustive visitors/hash/clone/replay/lowerers | parameterized `validate_hir` mutation sweep and `variant_sweep_tripwire` |
| root/nested construction, option None/Some, scalar/string/record arrays, empty/nonempty/DAG | V2 `JsonSubTable` graph and A103 null-arena decode | `recursive_owned_json_decode_states` |
| input/arena independence, move-in/out, raw Result, parameter/return/replacement/source nulling | ordinary recursive Move/Drop carrier with V2 allocation mode | `recursive_owned_json_transfer_matrix` and inside-arena/input-drop runtime owner |
| `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, early exits | sema flow and MIR cleanup CFG | `recursive_owned_json_control_flow_cleanup_matrix` |
| later-field recoverable failure at every nested live prefix | runtime transition ledger and deep decoded-owner cleanup | `recursive_owned_json_failure_prefix_matrix` |
| strict fallback versus indexed/Mison parity | one V2 table and shared nested value writers/cleanup | differential nested corpus covering field orders, duplicates, escapes, unknowns, malformed values, and allocation counts |
| array growth, completed/current element, option-wrapped array, reallocation, abandonment | runtime array staging plus canonical element Drop | `recursive_owned_json_array_transition_matrix` |
| overflow and allocator failure | checked counts/sizes/capacities and existing terminal policy | separate descriptor/decode/encode growth and allocator child-process owners |
| canonical bytes, unknown fields, escapes/NUL/UTF-8, integer width/sign, None omission, round trip | A80 recursive writer and Request 7/9/12 scalar/string writers | exact vectors in §4 plus flat Request 9 compatibility and bounded parity |
| source borrow/nonmutation and no unbounded-first pass | one root place, A80 builder, existing bounded finalizer | mutation-after-encode proof, allocation counters, exact/next limit |
| V2 descriptor/envelope golden, malformed bytes, target/layout provenance | `align_interface` V2 codec before descriptor use; LLVM independently checks TargetData | 221-byte descriptor and prefix/hash round trip, every single-field mutation, x86_64/aarch64 Linux and aarch64 Apple mismatch |
| format 7→8, interface/implementation split, whole/per-unit/generic/cache edit-revert | format constant, surface hash, K3, exported root list, consumer/private graph identities | stale-7 rejection, producer/consumer equality, cold/hot/edit/revert/two-process owners, private-body noninvalidation |
| runtime ABI and table domain | no new key/signature; kinds 8/9 admitted by A80 only from validated V2; A103/A80 tables agree | runtime registry counts unchanged, descriptor/table parity, decode/encode same-graph structural assertion |
| existing borrowed/AoS/SoA/union/scanner/fixed-array/document routes | unchanged operation predicates | parameterized compatibility suite proving no V2 table/call on each route |
| same-process and process concurrency | per-call state and immutable tables | 153-pair matrix expanded over recursive shapes plus two-process cache/runtime owner |
| stack and malformed-input safety | iterative compiler graph machinery, max type depth 128, existing input depth 128 | type-depth 128/129, unknown-value depth 128/129, deeply surrounding expression out-of-line dispatch, malformed descriptor no-panic sweep |

Implementation closure is owned by the checked-in tests named below. Rows that
cross several control-flow or constructor cells use one parameterized owner as
permitted by the repository test policy.

- Formation, routing, generic substitution, exclusions, and diagnostic order:
  `owned_json_formation_routing_and_multi_invalid_precedence_are_deterministic`
  plus `recursive_owned_json_constructor_depth_and_dag_are_exact`.
- The exact 50-record/543-field graph:
  `recursive_owned_json_c6_graph_manifest`; field names, resolved types, source
  order, and dependencies are pinned by one canonical graph hash and every root
  rebuilds a V2 plan.
- Checked-HIR and visitor closure:
  `owned_json_checked_hir_mutation_sweep_fails_closed_at_all_lowering_entrypoints`
  plus `owned_json_encoders_use_stack_safe_dispatch` and the existing variant
  sweep tripwire.
- Construction, canonical bytes, bounded parity, transfer, branch/error paths,
  and source lifetime:
  `recursive_owned_json_records_options_and_arrays_round_trip_canonically`,
  `owned_json_decode_encode_and_bounded_round_trip_without_an_arena`, and
  `owned_json_result_transfer_control_flow_replacement_and_drop_matrix`.
- Failure cleanup, array staging, completed/current elements, and terminal
  allocation policy: the `json_owned_*`, `json_nested_move_struct_array_*`, and
  `json_array_of_move_struct_*` allocation-count owners in `align_runtime`.
- Descriptor/envelope bytes and malformed input:
  `recursive_descriptor_and_envelope_match_normative_goldens` and
  `target_and_descriptor_mutations_fail_closed`, including every descriptor byte
  and envelope truncation point.
- Interface, cache, whole/per-unit, and concurrency parity:
  `exported_owned_json_records_carry_one_target_bound_format_8_graph`,
  `owned_json_descriptor_participates_in_cold_edit_revert_cache_identity`,
  `cross_module_owned_json_record_keeps_descriptor_and_ownership_parity`,
  `all_json_operation_variants_overlap_in_the_full_153_pair_matrix`, and
  `owned_json_calls_are_independent_in_two_concurrent_processes`.

This capability crosses type formation, checked HIR, MIR, interfaces/caches,
LLVM layout/table generation, runtime parse/encode/allocation, and recursive
cleanup. Splitting the producer-to-consumer chain would leave either a dormant
graph format or reachable owners without their cleanup consumer and duplicate
the same proof. One implementation PR is therefore expected to exceed roughly
1,000 hand-written lines; the single boundary lowers integration risk by landing
admission, construction, cleanup, ABI parity, and mutation owners atomically.

Before review, the author-side matrix-to-diff pass points every applicable cell
to implementation and a regression that fails on the pre-change compiler. A
review finding triggers a sweep of the entire root-cause class before the one
fix commit.

## 10. Documentation and lifecycle

This design updates `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`,
`docs/impl/08-memory-model-v2.md`, the English/Japanese JSON designs, the
native-boundary and interface/cache plans, checked-HIR and runtime ABI ledgers,
`docs/open-questions.md`, and `HANDOFF.md`. The implementation updates status
prose only where the shipped contract or capability state changes and adds the
Align-owned C6 syntax/golden fixtures.

Before implementation, the sibling request register mirrors this exact grammar,
depth rule, V2 descriptor/envelope, format-8 transition, validation order,
unchanged runtime ABI, C6 graph manifest, and closure matrix; that edit remains
uncommitted in the sibling repository. After implementation merges, update the
same register with the design and implementation PRs, leave it uncommitted, and
run exactly `cargo build --release --workspace`. align-llm pin/adoption and its
final `make ci` remain sibling-repository work.

## 11. Author-side ledger consistency pass

Completed before the initial independent review and repeated after closing the
complete-manifest and authoritative-ledger findings:

- the three public operations, exact positional inputs, inference/default rules,
  results, errors, ownership, lifetime, allocation, and owners appear in §1 and
  nowhere acquire an alternate spelling;
- the grammar in §2 closes every constructor, option state, array element class,
  cycle/DAG case, depth boundary, generic state, and C6 root; nested option and
  composite-array element states are explicitly unavailable rather than
  silently collapsed;
- the C6 provenance commit, Git blob, and file digest are pinned; all 50 nominal
  declarations and 543 ordered fields are materialized locally; every referenced
  record resolves inside the manifest; `PromptEvaluationTask` and
  `RegressionLimits` are in the `PromptEvaluationResult` closure; and the
  seven-field C6b-memory `ContextPolicy` is selected explicitly over the sibling
  document's incompatible four-field renderer-core form;
- UTF-8, embedded NUL, interface ASCII, validation-before-runtime, runtime error
  cleanup, and absence of external side effects are fixed in §4;
- the V2 format fixes every scalar width/tag, record/field/type order, target ABI
  cell, malformed-input precedence, and semantic-to-byte/byte-to-semantic golden;
- structural identity contains the complete reachable definition graph, while
  the exported root's ordinary nominal identity remains explicit;
- no runtime inspection, reflection, CLI/build option, ambient environment,
  global state, overlap exclusion, later milestone, or benchmark promise is
  introduced;
- the declarations and positional calls in §2 were parser/formatter checked
  against the merged compiler. Semantic acceptance is intentionally the
  implementation owner's regression because the shipped compiler still rejects
  this recursive graph; and
- `draft.md`, `docs/language-spec.md`, design notes, memory/cache/HIR/ABI ledgers,
  English/Japanese JSON designs, settled decisions, and `HANDOFF.md` point to
  this ledger without claiming the implementation has shipped.

## 12. Design-review finding-to-fix ledger

| Finding | Closure |
|---|---|
| P1: the C6 record-name list did not fix the complete field/type/order graph, omitted `PromptEvaluationTask` and `RegressionLimits`, and left two incompatible `ContextPolicy` forms | Pin the clean sibling commit, Git blob, and file digest; materialize the authoritative 50-record/543-field Align manifest; include every dependency; select the seven-field C6b-memory policy explicitly; and require generated field/type/order/dependency assertions and per-root goldens. |
| P2: the runtime ABI summary and machine owners retained the pre-Request-9 293/306/314 counts | Synchronize the current ledger to 294/307/315, the feature totals to 311/315, the runtime source-inventory assertion to 307, and the export/signature gate to the same exact counts. No ABI row or runtime behavior changes. |
| P2: the native-boundary plan still called Request 13 hypothetical and deferred its bounded parity owners | Link the accepted plan from the Request 12 ledger and matrix, preserve shipped flat behavior until implementation, and assign the atomic V2 `encode`/`encode_bounded` parity and limit owners to Request 13. |
