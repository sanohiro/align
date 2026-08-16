# JSON escape benchmark evidence

> 🌐 **English** · [Japanese](./ja/json-escape-benchmark-evidence.md)

Status: proposed design for align-llm Request 7. This document defines the evidence boundary only.
It does not accept the JSON language change, select its immutable baseline, or make a performance
claim. The evidence implementation and benchmark-input prerequisite must merge before the Request
7 implementation branch exists.

## Purpose and threat model

Request 7 changes code that produces the values being measured. Its implementation therefore may
not choose the baseline, controller, workload, toolchain, host, sample order, parser, threshold, or
attestation that judges it. A benchmark script can execute candidate compiler/runtime code; it
cannot publish an accepted result by itself.

The trusted boundary is a root-owned launcher installed from a reviewed controller before baseline
selection, with the same controller bytes already present in the immutable baseline; one
digest-pinned Linux x86_64 OCI image containing the complete build toolchain and offline dependency
cache; one named, otherwise-idle native x86_64 evidence host; kernel-enforced read-only source,
controller, toolchain, and dependency mounts; distinct writable revision workspaces; and a host-held
Ed25519 signing key that is never mounted into a measurement container.

Candidate code, benchmark children, the ambient shell, mutable image tags, network, registry state,
and unsigned report text are untrusted. The evidence host administrator, installed container daemon,
host kernel, merged controller, pinned image, and private attestation key are trusted. Compromise of
those roots requires revoking the profile and re-running evidence under a new reviewed profile.

No provider API or provider credential is used by the controller. Normal GitHub publication and
merge remain governed by repository policy. The controller receives no GitHub token, SSH agent,
cloud or package credential, user home, or caller-selected executable.

## Authoritative contract ledger

This ledger is authoritative. Later prose and implementation may make a field more explicit but
must not broaden it independently.

| Field | Exact contract |
|---|---|
| Public producer | `/opt/align-evidence/v1/bin/align-json-escape-evidence run --repository REPO --baseline BASE --candidate CANDIDATE --review-log REVIEW_LOG --output-dir NEW_DIR`. The root-owned launcher and modules were installed from the merged evidence implementation, are immutable to the benchmark account, and have hashes embedded in the host profile. It reads the controller/profile/key blobs directly from verified `BASE` objects and requires byte/mode equality with its installed copies before other work. All paths are absolute. OIDs are lowercase 40-hex. `NEW_DIR` must not exist. No other arguments or overrides exist. The run path produces and publishes only the signed report pair; it has no PR body or post-PR review attestation input. |
| Public verifier | `/opt/align-evidence/v1/bin/align-json-escape-evidence verify --report REPORT --signature SIGNATURE --expected-baseline BASE --expected-candidate CANDIDATE --pr-body PR_BODY --review-attestation REVIEW_ATTESTATION`. It verifies bytes/signature and requires the report/review/preflight bindings to equal the explicit OIDs, PR body, and trusted review attestation below. Local use is diagnostic; acceptance runs this same installed verifier in trusted-base CI, with every expected input created from the GitHub event/API outside the checkout. It performs no build, checkout, benchmark, network, or repository mutation. |
| Public merge verifier | `/opt/align-evidence/v1/bin/align-json-escape-evidence verify-merge --repository REPO --report REPORT --signature SIGNATURE --merge MERGE --output-dir NEW_DIR`. After the provider returns and the exact object is fetched, it verifies the report again, reads `MERGE` through the isolated raw-object path, requires the local target ref to contain it on its first-parent chain, checks its two parents and tree against the signed expectations, and emits exactly `merge-verification.json` and `merge-verification.json.sig`. `NEW_DIR` must not exist; no override exists. |
| Trusted review adapter | Trusted-base CI alone queries the GitHub review API with its job token for the event repository/PR. It rejects the PR author, missing repository write role, dismissed/stale/duplicate reviews, wrong commit, and a body without the exact report `log_sha256`. It writes the canonical `REVIEW_ATTESTATION` outside the checkout and supplies it to the verifier. The controller, candidate, report, benchmark containers, and PR arguments never receive the token or raw API response. |
| Ambient state | There are no semantic defaults. The launcher enters an empty environment containing only fixed `PATH`, `LC_ALL=C`, `TZ=UTC`, empty `HOME`, `CARGO_NET_OFFLINE=true`, fixed `DOCKER_CONFIG=/etc/align-evidence/docker-empty`, fixed `DOCKER_HOST=unix:///var/run/docker.sock`, and controller-created descriptor/configuration values. Ambient Git, Cargo, Rust, Docker, locale, proxy, credential, target, and tuning variables are omitted. |
| Result | Success obtains the exclusive lock and immediately creates and fsyncs the profile-global publication reservation before repository, image, container, or child work. It then writes and fsyncs `report.json` and `report.json.sig` in private staging while holding the lock, releases the lock, atomically renames staging to `NEW_DIR`, fsyncs its parent, removes/fsyncs the reservation, prints the two absolute paths, and exits zero. The reservation makes every second invocation reject before repository/image/container work and survives a pre-publication crash for administrator recovery. A threshold failure publishes a valid signed `regression` report by the same sequence and exits 1. Every other failure removes private staging, output, and reservation where possible, emits no accepted path, and exits nonzero; uncertain cleanup leaves the reservation fail-closed. |
| Controller owner | A checked-in Python 3 controller, root-owned installed launcher, and fixture tests under `scripts/` and `tests/benchmark_evidence/`. The exact installed/source relationship, installer manifest, interpreter, Git, Docker client/daemon, `ssh-keygen`, kernel, OCI image, host profile, and executable SHA-256 identities are recorded. Candidate files are never executable evidence roots. |
| Persisted format | Canonical UTF-8 JSON schemas `align.json_escape_benchmark_evidence/v1` and `align.json_escape_benchmark_merge_verification/v1`, each signed byte-for-byte with the host Ed25519 key under its fixed namespace. Unknown, missing, duplicate, reordered, non-ASCII key, non-integer number, float, invalid UTF-8/escape, trailing byte, or noncanonical serialization rejects. |
| Ownership/allocation | The controller owns all temporary directories, pipes, children, containers, captures, report staging, and cleanup. Benchmark children receive only stdin `/dev/null`, private stdout, and private stderr. Captures have fixed profile ceilings. No child receives the report, signature, repository administration, controller, Docker socket, signing key, or another revision's writable directory. |
| Concurrency | One controller obtains the profile's host-global exclusive lock and installs the profile-global publication reservation before repository inspection or any mutable child work. It holds the lock through signing and cleanup, releases it only after the reservation is durable, and removes the reservation only after publication completes. Every later invocation rejects before repository/image/container work while either guard exists. Baseline and candidate never overlap. The fixed pair order is the only schedule. |
| Prerequisites | The benchmark-input slice, both Request 7 language prerequisites, this design, its evidence implementation, pinned image, host profile/public key, and controller adversarial owners merge before `BASE` is selected. `BASE` is the then-current target tip and exact parent of the first Request 7 implementation commit. |
| Acceptance | A valid signature, `pass` verdict, exact PR/preflight/trusted-review bindings, unchanged target-base binding at merge, a signed merge-verification artifact whose merge remains reachable from the final fetched target, identical protected inputs, all ten samples for all five fields, and every exact ratio at most 1.05 are all required. Correctness tests remain separate deterministic owners. |

`REVIEW_LOG` is the repository-standard pre-open review log outside the candidate repository. It
must bind the candidate directly with `CLEAN`, or bind one reviewed ancestor with `FINDINGS` and a
complete findings-fixed descendant chain ending at `CANDIDATE`. The controller records its exact
SHA-256 and parsed bindings but does not treat caller-owned prose as authentic by itself.

After the PR opens, the independent reviewer submits one native GitHub review whose body contains
exactly one line `ALIGN_REVIEW_LOG_SHA256=<report log_sha256>`. Trusted-base CI queries that review
directly and produces this single-line canonical JSON plus LF, with no whitespace outside strings:

```text
ReviewAttestation = {"repository":name,"pull_request":u64,"review_id":u64,
  "reviewer":name,"review_commit":hex40,"review_state":ReviewState,
  "review_log_sha256":hex64,"submitted_at":time}\n
```

For `clean`, the native review has GitHub state `APPROVED` on the candidate. For `fixed`, it has
GitHub state `COMMENTED` with verdict `FINDINGS` on `review_head`, and the PR carries the complete finding dispositions
and exact repair chain ending at the candidate. The trusted adapter requires repository identity,
event PR number/head/base, reviewer identity/role, review database ID, commit, state, body marker,
and report fields to agree. Missing, stale, author-owned, duplicate, edited-to-another-digest, or
API/file substitution rejects. Fixture API responses own every failure before the verifier reads
candidate-controlled content.

## Merged profile and fixed identities

The implementation installs the launcher under a root-owned `/opt/align-evidence/v1` tree with no
benchmark-account write permission. Its installation manifest fixes every relative path, mode,
owner, size, and SHA-256 and embeds the evidence implementation commit plus profile blob OID. The
launcher opens itself and every module before parsing caller input, checks the installed manifest,
then uses pinned Git to read the same paths from `BASE` and requires exact equality. The verifier is
the same installed program; it never imports a module from `REPO`, `BASE`, `CANDIDATE`, current
directory, `PYTHONPATH`, user site, or a caller-selected path. Installation/replacement is an
administrator-only qualification step before baseline selection.

The implementation adds canonical profile
`bench/json_escape/evidence/linux-x86_64-v1.json` containing exactly:

- schema/profile IDs; fixed local target ref `refs/heads/main`; evidence host ID; expected machine,
  kernel, CPU vendor/family/model/stepping,
  microcode, online and benchmark CPU sets, NUMA nodes, and minimum physical memory;
- host-global lock path and admissible pre/between/post load and resource observations;
- Docker client/daemon versions, client hash, daemon architecture, storage driver, cgroup version,
  and OCI runtime identity;
- image registry digest, local image ID/config digest, `linux/amd64` platform, and hashes/versions of
  Python, Git, Cargo, rustc, LLVM, C compiler, linker, and `ssh-keygen`;
- SHA-256 of the read-only Cargo home/cache manifest and fixed Cargo configuration;
- capture ceilings, phase timeouts, public Ed25519 key/fingerprint; and
- fixed threshold `105/100`, warm-up count `1`, pair count `10`, benchmark and field inventory.

The profile has no mutable tag, wildcard version, caller override, optional identity, or secret.
Updating an item creates a new reviewed profile ID and cannot reinterpret an old report.

Before work, the controller verifies the profile, exclusive lock, native x86_64 host/daemon, CPU
topology and microcode, memory, absence of CPU quota, executable bytes, daemon/runtime identity,
locally present image with `--pull=never`, image digest/config/platform, and load limit. A fixed image
self-inspection with network disabled must reproduce all tool versions and hashes. ARM, Rosetta,
QEMU/binfmt, a cross-architecture image, mutable tag resolution, or changed tool rejects. This lane
never emulates x86_64 or ARM.

The named host is operationally reserved. Its benchmark CPUs are an exclusive cpuset from which the
host service manager excludes ordinary work. A root-owned, profile-pinned monitor opens scheduler,
cgroup, thermal-throttle, frequency, pressure, memory, swap, and container event sources before the
first build. For every child it brackets the exact child cgroup, continuously consumes events, and
latches any non-child task scheduled on the benchmark CPUs, CPU migration outside the set,
throttle/thermal/frequency-limit event, pressure/load-limit crossing, swap I/O, memory-limit event,
or foreign-container transition until after child reap. Monotonic counter deltas close events that
occur entirely between periodic samples. Fixed 100 ms snapshots additionally record load, pressure,
frequency, temperature, free memory, and container inventory during the child. An event-source
overflow, lost event, monitor delay above the profile ceiling, monitor death, counter reset, or
unattributed event rejects.

The controller also samples before build, between children, and after the last child. All latched
events and snapshots enter the report and any violation rejects the complete run instead of deleting
a slow sample. The monitor runs outside candidate containers, exposes no writable/control fd to
them, and is included in installed executable identities. It does not claim to detect a malicious
host administrator.

## Revision and source construction

The controller uses only pinned `/usr/bin/git` under an empty environment with system/global/XDG
configuration, hooks, filters, fsmonitor, commit graph, replacements, grafts, lazy fetch, optional
locks, prompts, alternates, and network disabled. It opens `REPO` with no-follow directory semantics,
resolves one common directory, and rejects symlinked administration, alternates, promisor config,
shallow history, replace refs, grafts, submodules, or missing objects before resolving revisions.

For each commit it records raw commit SHA-256, tree OID, raw tree closure, parent list, and every
path's raw mode, type, OID, size, and blob SHA-256. It requires:

- both OIDs are commits present before the run;
- the first Request 7 commit has sole parent `BASE`, later commits are first-parent descendants, and
  the final first-parent is `CANDIDATE`;
- the review log names that candidate or its permitted findings-fixed chain;
- enumeration contains no merge, replacement, or missing object; the controller records the
  complete commit/path/mode inventory, while the independent review and PR disposition attest that
  every non-protected change belongs to Request 7; and
- the profile's local target ref observed at run start is exactly `BASE`. Remote target currency is
  checked separately at publication and immediately before merge.

The controller copies verified raw blobs into new private source directories; it does not use an
ambient worktree, index, checkout hook, archive filter, or candidate script. Modes `100644`, `100755`,
and the repository's already-reviewed symlinks are exact. A new/changed symlink, submodule, special
file, hard-link collision, case-fold collision, duplicate normalized path, absolute path, `..`, NUL,
or non-UTF-8 path rejects. A retained-descriptor walk then compares every byte/mode to raw objects.
Sources are mounted read-only, and a post-run manifest must match.

The protected-input set is exactly:

```text
.cargo/**
Cargo.toml
Cargo.lock
rust-toolchain                 when present in either revision
rust-toolchain.toml            when present in either revision
bench/.cargo/**                when present in either revision
bench/json_decode/**
bench/json_soa/**
bench/json_escape/evidence/**
scripts/cargo.sh
scripts/dyld-env.sh
scripts/benchmark_evidence/**
tests/benchmark_evidence/**
```

Presence, path, mode, type, blob bytes, and structural manifest must match across revisions.
Optional-root presence mismatch rejects. This comparison covers scripts, kernels, harnesses,
manifests, lockfiles, generators, output format, timing loops, the profile/public key, every
installed controller/verifier/monitor source and installation manifest, their adversarial owners,
and nested configuration before any candidate code executes. Any change requires a separately
reviewed evidence-profile implementation and requalification before selecting a new baseline; it
cannot coexist with the measured Request 7 candidate.

## Container and process boundary

Every build or benchmark uses a fresh container created by the trusted controller. It uses the
profile image with `--pull=never`, `--network=none`, read-only root, all capabilities dropped,
`no-new-privileges`, fixed non-root uid/gid, fixed seccomp and LSM profile, CPU/NUMA set,
memory/swap/pid/file/fd limits, minimum devices, and private IPC/PID/mount/UTS/cgroup/tmp. There is no
Docker socket, host `/proc`, home, agent, credential, repository administration, controller, report,
signing key, or other revision mount.

One revision source is read-only. Its empty target, benchmark-work, and temporary directories are
writable and never shared. Cargo home/toolchain are read-only. `CARGO_TARGET_DIR`, `TMPDIR`, and one
fixed `ALIGN_BENCH_WORK_DIR` select those directories. The enabling implementation makes both
benchmark scripts reject an absent, nonempty, or unsafe work directory and confines `kernel.o` and
all generated files there.

For the benchmark-input slice, `ALIGN_BENCH_WORK_DIR` is required and names an absolute existing
directory whose final path component is not a symbolic link. Its physical path must not be `/`, the
repository root, or inside the repository, and it must contain no entry, including a hidden entry,
before the script starts. Each script enters `umask 077`, creates exactly one private child below
that directory, binds root and detached Cargo targets, `TMPDIR`, the kernel object, and every
configured build output below that child. Prepare success retains the sealed child. Error, signal, and
interrupt recursively unlink non-directory entries only below its retained descriptor and leave a
directory-only skeleton for the trusted caller to remove after container/process teardown. Relative,
final-symlink, root, repository, in-repository,
initially nonempty, cleanup-failure, and foreign-residue cases reject without deleting the foreign
entry. Repeated trailing separators and `/.` cannot disguise a final-component symbolic link. Each
build, compiler, and harness command runs in its own process group; interruption applies bounded
TERM/KILL escalation to the complete group and reaps the direct child before private files are
removed. The caller-owned directory and one directory-only owned tree remain after script-level
failure; the outer controller owns their race-free removal after the candidate container is gone.

The script opens and retains the private child before starting any untrusted build. On the accepted
Linux path, the outer controller's read-only-root container and controller-owned writable mounts
confine arbitrary candidate build writes; the script alone is not that sandbox. Configured
Cargo/compiler outputs stay below the retained child, and every trusted post-build copy, chmod,
manifest, and cleanup mutation is descriptor-relative, so renaming or replacing `prepared` cannot
redirect publication into caller data. Any candidate escape within a controller-owned writable
mount creates foreign residue and rejects.
Cleanup recursively unlinks non-directory entries only through the retained descriptor and never
deletes a directory entry while a candidate-side writer could race it. Empty build-directory
skeletons are included in the prepared manifest on success and removed by the outer controller only
after measurement and candidate teardown.
macOS remains native ARM development qualification and never supplies accepted adversarial
evidence.

Commands are argv arrays without shell interpolation. Before baseline selection, the evidence
implementation gives each protected script a closed two-phase interface. `run.sh prepare native`
performs every root and detached Cargo build plus `alignc emit-obj`, then writes a canonical
SHA-256/mode manifest for the compiler, runtime, detached benchmark executable, kernel object, and
effective configuration into the revision-private work directory. `run.sh native` accepts no Cargo
or compiler work. Prepare revalidates the captured device/inode identity before publishing the
private child. Native opens the prepared root without following its final component and passes both
the retained descriptor and captured device/inode to the launcher, which requires them to match and
verifies the manifest through that descriptor. Prepare also prints the canonical manifest SHA-256;
the trusted controller retains it outside candidate-writable state and supplies it as the required
`ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256` value to every later native invocation. The launcher rejects
any current self-consistent tree whose manifest does not equal that prepare-time digest. On the
accepted Linux x86_64 path it hashes the
executable and runtime while copying them to anonymous `memfd` objects, checks source metadata
before and after the copy, requests the Linux UAPI `MFD_EXEC` capability, applies
`F_SEAL_WRITE|F_SEAL_GROW|F_SEAL_SHRINK|F_SEAL_SEAL`, and directly execs/preloads only those sealed
descriptors. A kernel without executable memfds, including an enforced `vm.memfd_noexec=2` policy,
rejects qualification rather than falling back to a path or unsealed file. The macOS path remains
native ARM development qualification rather than accepted evidence and fixes
`DYLD_SHARED_REGION=private` for stable repeated launches. Immediately before either platform
executes, the launcher re-verifies the complete descriptor-bound prepared tree against the retained
manifest and digest. Missing, extra, changed,
wrong-mode, replaced, or unsealable artifacts and every prepare-only selector reject.

No final artifact exists while candidate-controlled Cargo/compiler work can still run. After every
child process group exits, the descriptor-relative helper opens each fixed output no-follow and
nonblocking, rejects non-regular files before reading, checks source stability, and creates the
complete final artifact set. Thus a build may produce its declared output, but it cannot rewrite an
already-published runtime, kernel, compiler, or harness before manifest capture.

All prepare-phase Cargo operations are visibly `--locked --offline`; `CARGO_NET_OFFLINE=true` is
defense in depth. Registry/cache/source manifests are compared before/after; a lockfile, index,
cache, source, or configuration write rejects. Each revision starts with an empty target and work
directory, is prepared exactly once per benchmark, and retains only its verified private artifacts
for warm-up and samples. The benchmark-input slice first locks all six current Cargo invocations:
two root builds and one detached `cargo run` in each script. The evidence implementation then
replaces the two measurement-time `cargo run` invocations with this protected prepare/direct-exec
interface before `BASE` can exist.

Before each child, the controller creates close-on-exec pipes, enumerates descriptors, and passes
exactly stdin/stdout/stderr. It validates descriptor numbers after duplication, closes the inherited
range, and the entrypoint independently enumerates `/proc/self/fd` before exec. Collision,
inheritance, missing CLOEXEC, or changed mapping rejects. Bounded stdout/stderr enter the report by
SHA-256 and escaped diagnostic tail and are never parsed after timeout, truncation, or nonzero exit.

On timeout, signal, parser/threshold/controller error, or interruption, the controller kills the
complete container/process group, waits for removal, closes descriptors, removes private mounts and
directories, and verifies no owned container/child remains. Cleanup failure suppresses an otherwise
valid report. Signal handlers preserve the cause but never publish partial evidence.

## Workload and measurement

The controller runs `bench/json_decode/run.sh prepare native` for baseline and candidate, then does
the same for `json_soa`, and verifies all four artifact manifests before any warm-up. No prepare
child overlaps another, and every build/prepare child is monitored and reported separately from
measurement children.

Before baseline selection, the evidence implementation also changes both protected harnesses to
retain their existing timed operations and round counts but report the arithmetic median of every
inner timing for each field, not the minimum. For the even counts (40 and 30), it sorts checked
integer nanoseconds and retains the exact middle sum `middle_ns[0] + middle_ns[1]`. The output
microseconds are exactly `(middle_sum_ns + 1000) / 2000` using checked unsigned integer arithmetic:
nearest microsecond, with an exact half-microsecond rounded upward. The script formats that integer
as the existing three-decimal millisecond token using `us / 1000` and a zero-padded `us % 1000`,
without floating point or a second rounding. Deterministic owners feed odd/even nanosecond sums, half-unit ties, overflow
edges, equal middle values, and one extreme low outlier, and pin the middle pair, quantized
microseconds, and token. This does not add, remove, or reorder a timed kernel invocation. It is
identical protected input before `BASE` exists.

After preparation, each warm-up and sample child runs only:

```text
bench/json_decode/run.sh native
bench/json_soa/run.sh native
```

The prepared native target must match the profile CPU. Feature downgrade, cross compilation,
emulation, changed effective Cargo/rustc configuration, an attempted build, or an artifact-manifest
change rejects.

For each benchmark, one discarded warm-up per revision precedes ten measured pairs. Odd pairs run
baseline then candidate; even pairs candidate then baseline. Processes never overlap. A failure is
not replaced; a later attempt starts with new empty targets and a new output directory.

`ALIGN_BENCH_PROFILE` is absent. The parser accepts the fixed `target: native` line, fixed title, one
exact header, and exactly the three ordered rows `10000`, `100000`, and `1000000`; any other stdout
line rejects. Header fields are, in order:

```text
json_decode: records, json KB, A-full, rs-full, full×, A-proj, rs-proj, proj×
json_soa:    records, json KB, soa ms, aos ms, proj ms, rust ms, soa/rust, aos/rust, proj/rustP
```

The parser validates every column's fixed grammar and retains the million-row values for exactly:

```text
json_decode: A-full, A-proj
json_soa:    soa ms, aos ms, proj ms
```

Duplicate/missing lines, wrong row/order/field count, non-ASCII whitespace, sign, exponent,
nonfinite token, invalid ratio suffix, or profiling output rejects. Each retained token is the
harness's quantized inner median, is positive ASCII decimal with exactly three fractional digits, and converts without
floating point or rounding to integer microseconds. Original token and integer are reported. For
each field/revision, sort ten integers and define median `(sample[4] + sample[5]) / 2`. Store the
middle sum and denominator `2`, never a rounded value. Compare exactly, after checked-u64
multiplication of both products:

```text
candidate_middle_sum * 100 <= baseline_middle_sum * 105
```

All five must pass. Zero baseline, overflow, missing sample, parse error, or warning rejects. The
harness parity assertions are useful pre-timing guards but do not replace deterministic correctness
owners.

## Canonical report and signature

The complete file is exactly this outer record followed by one LF:

```text
Report = {"body":Body,"body_sha256":hex64}\n
```

There is no whitespace outside strings. Object members appear in the order declared below. Arrays
use the declared order and cardinality. `u32` and `u64` are unsigned ASCII decimal with no leading
zero except `0`; encoders use checked conversion and decoders reject overflow. `bool` is `true` or
`false`. `hex40` and `hex64` are lowercase hexadecimal of exactly 40 and 64 digits.
`token` is `[0-9]+\.[0-9]{3}`. `time` is exactly
`YYYY-MM-DDTHH:MM:SS.NNNNNNNNNZ`. Every other string belongs to a closed enum below or matches
`name = [A-Za-z0-9._/:+=@-]{1,255}`. Arbitrary bytes and Git paths are lowercase even-length hex strings,
so the format never needs a JSON escape. A quote, backslash, control byte, non-ASCII scalar, invalid
UTF-8, overlong string, unknown member, missing member, duplicate member, reordered member, wrong
array length/order, float, negative number, `null`, or trailing byte rejects.

The following type declarations fix every member name, order, type, and presence. `[]` is permitted
only where an unbounded sequence is shown. All other cardinalities are exact.

```text
Body = {
  "schema":"align.json_escape_benchmark_evidence/v1",
  "profile_id":name, "profile_sha256":hex64,
  "producer":ToolIdentity, "verifier":ToolIdentity, "monitor":ToolIdentity,
  "run_id":hex64, "started_at":time, "ended_at":time,
  "baseline":Revision, "candidate":Revision, "target":TargetBinding,
  "review":ReviewBinding, "protected_inputs":ProtectedInputs,
  "execution":ExecutionIdentity, "host_observations":[HostObservation],
  "benchmarks":[BenchmarkEvidence;2], "fields":[FieldResult;5],
  "cleanup":Cleanup, "verdict":Verdict, "first_failed_field":FieldOrEmpty
}

ToolIdentity = {"version":name,"source_commit":hex40,"source_manifest_blob":hex40,
  "source_manifest_sha256":hex64,"executable_sha256":hex64}
ExecutableIdentity = {"version":name,"executable_sha256":hex64}
Revision = {"commit_oid":hex40,"commit_sha256":hex64,"tree_oid":hex40,
  "tree_manifest_sha256":hex64,"parents":[hex40],"commits":[CommitIdentity],
  "changed_paths":[PathChange]}
CommitIdentity = {"oid":hex40,"raw_sha256":hex64,"tree_oid":hex40,
  "parents":[hex40]}
PathIdentity = {"path_hex":bytes,"mode":GitMode,"kind":PathKind,"oid":hex40,
  "size":u64,"sha256":hex64}
PathChange = {"path_hex":bytes,"status":ChangeStatus,"old":PathSide,"new":PathSide}
PathSide = {"presence":Presence,"mode":GitModeOrEmpty,"kind":PathKindOrEmpty,
  "oid":OidOrEmpty,"size":u64,"sha256":DigestOrEmpty}
TargetBinding = {"local_ref":"refs/heads/main","run_oid":hex40,
  "expected_merge_base":hex40,"expected_merge_head":hex40,
  "expected_merge_tree":hex40}
ReviewBinding = {"log_sha256":hex64,"review_head":hex40,"review_base":hex40,
  "state":ReviewState,"reviewer":name,"repair_commits":[hex40]}
ProtectedInputs = {"baseline_manifest_sha256":hex64,
  "candidate_manifest_sha256":hex64,"entries":[PathIdentity]}

ExecutionIdentity = {"host_id":name,"kernel":name,"cpu":name,"microcode":name,
  "cpu_set":name,"numa_set":name,"memory_bytes":u64,"docker_client":ExecutableIdentity,
  "docker_daemon":name,"oci_runtime":name,"image_digest":name,
  "image_id":hex64,"image_config":hex64,"cargo":ExecutableIdentity,"rustc":ExecutableIdentity,
  "llvm":ExecutableIdentity,"cc":ExecutableIdentity,"linker":ExecutableIdentity,
  "cargo_cache_manifest_sha256":hex64,"cargo_config_sha256":hex64,
  "environment_sha256":hex64,"mount_manifest_sha256":hex64,
  "limit_manifest_sha256":hex64,"descriptor_manifest_sha256":hex64}
HostObservation = {"ordinal":u32,"phase":HostPhase,"monotonic_ns":u64,
  "child_id":ChildOrEmpty,
  "load_milli":u64,"cpu_pressure_total_us":u64,"memory_pressure_total_us":u64,
  "free_memory_bytes":u64,"swap_read_bytes":u64,"swap_write_bytes":u64,
  "throttle_events":u64,"thermal_events":u64,"foreign_schedule_events":u64,
  "foreign_container_events":u64,"monitor_lost_events":u64,
  "frequency_khz":u64,"temperature_millic":u64,"container_manifest_sha256":hex64}

BenchmarkEvidence = {"name":Benchmark,"prepare_argv":PrepareArgv,"argv":Argv,
  "preparations":[Preparation;2],"warmups":[Run;2],"pairs":[Pair;10]}
Preparation = {"child_id":hex64,"revision":RevisionArm,"sequence":u32,
  "stdout_sha256":hex64,"stderr_sha256":hex64,"stderr_tail_hex":bytes,
  "exit_code":u32,"elapsed_ns":u64,"monitor_first":u32,"monitor_last":u32,
  "artifact_manifest_sha256":hex64}
Pair = {"ordinal":u32,"first":Run,"second":Run}
Run = {"child_id":hex64,"revision":RevisionArm,"sequence":u32,"stdout_sha256":hex64,
  "stderr_sha256":hex64,"stderr_tail_hex":bytes,"exit_code":u32,
  "elapsed_ns":u64,"monitor_first":u32,"monitor_last":u32,"samples":[Sample]}
Sample = {"field":Field,"token":token,"microseconds":u64}
FieldResult = {"field":Field,"baseline_tokens":[token;10],
  "candidate_tokens":[token;10],"baseline_samples_us":[u64;10],
  "candidate_samples_us":[u64;10],"baseline_sorted_us":[u64;10],
  "candidate_sorted_us":[u64;10],"baseline_middle_sum":u64,
  "candidate_middle_sum":u64,"median_denominator":2,"ratio_numerator":u64,
  "ratio_denominator":u64,"threshold_numerator":105,
  "threshold_denominator":100,"passed":bool}
Cleanup = {"children_remaining":u32,"containers_remaining":u32,
  "mounts_remaining":u32,"fds_remaining":u32,"private_dirs_remaining":u32,
  "host_lock_held_for_signing":bool,"source_manifests_unchanged":bool,
  "cache_manifests_unchanged":bool}
```

Closed enums are exact:

```text
PathKind = "blob" | "symlink"
GitMode = "100644" | "100755" | "120000"
ChangeStatus = "added" | "deleted" | "modified"
Presence = "absent" | "present"
GitModeOrEmpty = "" | GitMode
PathKindOrEmpty = "" | PathKind
OidOrEmpty = "" | hex40
DigestOrEmpty = "" | hex64
ReviewState = "clean" | "fixed"
Verdict = "pass" | "regression"
FieldOrEmpty = "" | Field
RevisionArm = "baseline" | "candidate"
Benchmark = "json_decode" | "json_soa"
PrepareArgv = "bench/json_decode/run.sh prepare native" |
  "bench/json_soa/run.sh prepare native"
Argv = "bench/json_decode/run.sh native" | "bench/json_soa/run.sh native"
Field = "A-full" | "A-proj" | "soa ms" | "aos ms" | "proj ms"
HostPhase = "pre-build" | "child-start" | "child-sample" | "child-end" |
  "between-children" | "post-run"
ChildOrEmpty = "" | hex64
```

`Revision.parents` has exactly the raw commit parents; baseline `commits` and `changed_paths` are
empty, candidate `commits` is the nonempty first-parent sequence after baseline, and candidate
`changed_paths` is the exact path-hex-ordered union diff of the baseline and candidate trees. An
`added` side is absent/present, a `deleted` side present/absent, and a `modified` side present/present
with at least one mode, kind, OID, size, or SHA-256 difference. An absent `PathSide` has empty
mode/kind/OID/digest and size zero; a present side has no empty value. Protected entries are
path-hex order and contain the common identity once; their two manifest hashes must be equal.

Host observations have dense ordinals. Every `Preparation` and `Run` has a globally unique
`child_id` and names an inclusive nonempty range whose first/last observations are respectively
`child-start`/`child-end` with that same ID; every interior `child-sample` carries that ID. In global
sequence order, those ranges are strictly increasing, disjoint, and are an exact partition of every
nonempty-child observation: no range or child observation is reused or orphaned. Non-child phases
carry the empty ID. Benchmarks are `json_decode`, then `json_soa`; each has preparations baseline
then candidate, followed by warm-ups baseline then candidate. Pair ordinals are 1 through 10 with
B/C arms in the specified balanced order. `Preparation.artifact_manifest_sha256` matches the
controller-owned schedule manifest for that benchmark/revision and the manifest reverified by
every later run of that benchmark/revision. `Run.samples` has fields in
benchmark order (two then three). Field results use the five field order above. Original samples
follow pair ordinal; sorted samples are nondecreasing and exact permutations. `ratio_numerator` is
candidate middle sum and `ratio_denominator` baseline middle sum.
All cleanup counts are zero and booleans true before either verdict can be signed. The host lock is
still held while the early profile-fixed root-owned publication-reservation file and both
private-staging files and that directory become durable. The reservation uses no-follow exclusive
creation, records the run/output identity, and fsyncs its directory before repository inspection or
child work. After signing, the producer releases the lock, atomically renames staging to `NEW_DIR`,
fsyncs the output parent, removes the reservation, fsyncs its directory, and only then prints
accepted paths. Any invocation acquiring the lock while the reservation exists rejects before
repository inspection. Unlock or publication failure removes private/output state where possible; a
surviving reservation marks any output unaccepted and blocks future runs until the administrator
validates and removes it. Reservation removal is a publication postcondition outside the
already-signed measurement cleanup; accepted paths do not exist until that postcondition succeeds.
`first_failed_field` is empty exactly for `pass`; for `regression` it is the first false field in
field order. The candidate `commit_sha256` equals the raw SHA-256 of the final candidate inventory
entry. A controller may publish a valid `regression` report only as a distinct non-accepted result
with exit status 1; a required-pass invocation rejects it before publication. There are no
conditional or omitted members.

For a `clean` review, `review_head` equals the candidate and `repair_commits` is empty. For a
`fixed` review, `review_head` is a reviewed ancestor strictly after the baseline and
`repair_commits` is the exact
nonempty first-parent sequence after it, ending at the candidate; every listed commit is also in
candidate `commits`, in the same order. `review_base` equals the reported baseline in either state.
These literals exactly equal the repository preflight stamp and PR marker; a review log verdict
`CLEAN` maps only to `clean`, while `FINDINGS` plus a nonempty accepted repair chain maps only to
`fixed`.
Each `ToolIdentity.source_manifest_blob` names the canonical installation-manifest blob in
`source_commit`; `source_manifest_sha256` hashes those blob bytes, and the manifest in turn fixes
every installed file used by that tool.

`body_sha256` is not recursive. Its exact preimage is the 45 ASCII bytes
`align-json-escape-benchmark-evidence-body-v1\0` followed by the canonical JSON encoding of `Body`
alone, with no LF. The digest is lowercase hex in the outer record. The signature covers the
complete canonical outer record including its final LF in namespace
`align-json-escape-benchmark-evidence-v1`.

Each detached `.sig` is the pinned OpenSSH `SSHSIG` version-1 ASCII armor, never raw Ed25519 bytes.
It is exactly `-----BEGIN SSH SIGNATURE-----\n`, RFC 4648 standard base64 of the binary SSHSIG record
wrapped at 70 ASCII characters per line (the final line has 1 through 70 characters, required `=`
padding retained), `\n` after every base64 line, and `-----END SSH SIGNATURE-----\n`. CR, spaces,
blank lines, noncanonical padding/wrapping, or trailing bytes reject. All SSH strings use unsigned
big-endian 32-bit length prefixes. The binary record is magic `SSHSIG`, version `1`, the exact
profile Ed25519 public-key blob, the exact namespace, empty reserved string, hash algorithm
`sha512`, and an SSH signature containing algorithm `ssh-ed25519` plus the 64-byte signature.

The Ed25519 signing preimage is exactly magic `SSHSIG`, SSH string namespace, empty SSH string
reserved, SSH string `sha512`, and SSH string containing the 64-byte SHA-512 of the complete
canonical message bytes. Report and merge-verification namespaces are respectively
`align-json-escape-benchmark-evidence-v1` and
`align-json-escape-benchmark-merge-verification-v1`. The producer/verifier decode the armor, enforce
every binary field and length, re-encode to identical armor bytes, and only then invoke the pinned
`ssh-keygen -Y sign/verify` implementation. Signature SHA-256 fields hash the complete canonical
armor including its final LF.

The producer encodes `Body`, computes that domain-separated digest, encodes `Report`, reparses with
duplicate rejection, re-encodes, and requires byte identity before signing. The run-side verifier
checks the report-only bindings and signature before publication. The post-PR verifier then repeats
schema/canonical validation, the body digest, every relationship, sample reconstruction, medians,
exact comparisons, explicit expected baseline/candidate/PR bindings, identities, key fingerprint,
and signature after it receives the trusted PR body and review attestation; it trusts no stored
derived value.

The implementation checks in complete minimal-pass and first-field-regression semantic fixtures,
their exact one-line report bytes, detached test signatures, and SHA-256 files. A structurally
independent reference encoder produces each golden without calling production encode/decode code;
production decode-to-semantics and re-encode must match byte-for-byte. The mutation matrix changes
every member, enum, scalar width/boundary, array cardinality/order, key order/presence/duplicate,
string grammar, LF, body preimage/domain/digest, signature namespace/key, and derived field.

Post-merge verification produces exactly this second canonical record followed by one LF:

```text
MergeVerification = {
  "schema":"align.json_escape_benchmark_merge_verification/v1",
  "profile_id":name,"profile_sha256":hex64,"verifier":ToolIdentity,
  "report_sha256":hex64,"report_signature_sha256":hex64,
  "target_ref":"refs/heads/main","target_oid":hex40,
  "merge_oid":hex40,"merge_sha256":hex64,"parents":[hex40;2],
  "tree_oid":hex40,"verified_at":time
}\n
```

It uses the same scalar, string, member-order, whitespace, UTF-8, and rejection grammar as `Report`.
The detached signature covers the complete bytes including LF under namespace
`align-json-escape-benchmark-merge-verification-v1`. `report_sha256` hashes the complete report
including LF; `report_signature_sha256` hashes its detached signature bytes. `merge_oid` equals the
supplied `MERGE`; `target_oid` is the locally fetched target tip and contains `merge_oid` on its
first-parent chain; `parents` are exactly baseline then candidate; and `tree_oid` equals the
report's expected candidate tree. The verifier records the raw merge-object
SHA-256 only after all relationships pass. Complete golden, mutation, wrong-parent/tree/ref, stale
report/signature, and raw-object swap owners cover this format. It obtains the same exclusive host
lock before inspection and uses the same private-stage, file/directory fsync, durable publication
reservation, unlock, atomic rename, parent fsync, reservation removal/directory fsync, then
path-publication sequence; a surviving reservation makes any output unaccepted and blocks later
work.

An accepted PR carries the complete report/signature as unmodified files in an immutable artifact
or base64-safe fenced attachment, plus verifier command/result. After merge, the trusted host stores
the complete merge-verification record/signature in an immutable artifact and links it from the PR
before Request 7 or any dependent lifecycle advances. Copying is allowed; changing one byte
invalidates the relevant signature. A record for another baseline, candidate, merge, profile, host,
toolchain, review, sample set, or target is stale. The private key opens only after measurement
containers are gone, through a non-inheritable descriptor. Signing failure leaves no output
directory.

## Review, base drift, and integration

Measurement begins after one comprehensive review yields a clean exact-candidate verdict or one
consolidated findings-fixed chain. Any later commit, amend, rebase, merge, or protected-input change
invalidates the report. Only raw commit OIDs/objects are identities.

At publication, preflight, trusted review attestation, and PR markers must match the report candidate, merge base, review
head, and state. Immediately before merge, target OID must still equal `BASE`; otherwise rebase from
the new tip, select that parent as a new baseline, and repeat review/evidence/preflight. Merge binds
the expected PR head. After the provider returns, the trusted host fetches that exact response OID,
runs `verify-merge`, and accepts the signed merge-verification artifact only when that merge remains
on the fetched target's first-parent chain, its first parent is `BASE`, its second parent is
`CANDIDATE`, and its tree is candidate-identical. After the artifact is durably stored and linked,
the trusted lifecycle adapter fetches the provider target once more and repeats raw-object
first-parent reachability before advancing Request 7. A normal later descendant preserves the
artifact; any force-push, replacement, or movement that removes `MERGE` invalidates it and blocks or
rolls back the lifecycle before dependent work. An unavailable object or failed final fetch is
unshipped and no lifecycle advances.

This postcondition is cleanup for a hosting transaction that cannot currently atomically accept an
expected base OID. Before Request 7 starts, implementation must prove fail-closed merge/revert on a
disposable remote or replace it with a provider compare-and-swap that binds endpoint, principal,
repository, expected base/head, request bytes, response, and secret handling in a reviewed amendment.
It may not weaken the base rule.

## Delivery order

1. Merge the benchmark-input slice: check in detached locks, remove ignores, make all six current
   Cargo invocations locked/offline, confine generated outputs, and add deterministic missing/stale/cache,
   no-network, no-write, and cleanup owners.
2. Merge evidence implementation: installed controller/verifier/monitor, profile, pinned image
   recipe/digest, public key, inner-median harness update, host guide, adversarial fixtures, format
   goldens, and merge-race owner. It makes no Request 7 performance claim.
3. Provision the host private key, independently verify image/cache/toolchain/profile, and run host
   self-qualification.
4. Only then select current target tip as `BASE` and create the Request 7 branch directly on it.
5. Finish correctness owners, review final candidate, produce/verify evidence, publish, keep target
   at `BASE`, merge, verify the merge object, and advance lifecycle.

## Implementation closure matrix

The first adversarial-owner implementation deliberately groups the process boundary, exact schedule,
cleanup/publication ordering, and exclusive-run reservation into one capability even though its
hand-written implementation is just above 1,000 lines including deterministic owners. These four
edges form one dormant producer-to-consumer chain; splitting them would duplicate failure-order
fixtures and leave the controller consuming an unreviewed boundary. The slice does not include the
controller, verifier, or merge-race behavior.

| Cell | Owner and exact regression |
|---|---|
| Trusted bootstrap and CLI | Root-owned installed producer/verifier/merge-verifier/monitor bytes must match their manifest and verified baseline blobs. Candidate/PATH/PYTHONPATH/current-directory substitution, installed-file replacement, missing/extra/repeated options, relative paths, malformed/same OIDs, existing output, untrusted repository, and ambient selectors reject before mutation. Trusted CI supplies expected baseline/candidate/PR body and a canonical review attestation created from fixture-owned GitHub API responses; wrong repository/PR/reviewer role/review ID/commit/state/body digest, author review, dismissal, staleness, duplication, and API/file substitution reject before candidate-controlled content is read. `benchmark_evidence_bootstrap_cli_matrix`. |
| Raw identity/construction | Cover clone/worktree, packed/loose objects, reviewed symlinks, hostile config/includes/hooks/filters/fsmonitor/commit-graph/replacements/grafts/alternates/promisor/shallow/missing objects, raw swap, path/type/mode collisions, and mutation race. `benchmark_evidence_raw_object_matrix`. |
| Revision binding | Exact parent chain and two-sided added/deleted/modified path inventory succeed; wrong local target, unrelated ancestry, merge/side parent, ref movement, stale review, branch-after-drift, missing deletion, wrong old/new mode/type, and incomplete tree-union diff reject. `benchmark_evidence_revision_binding_matrix`. |
| Protected inputs | Mutate presence/path/type/mode/bytes of every required/optional config, manifest, lock, script, kernel, harness, generator, output, timing owner, evidence profile/public key, installed-source manifest, controller/verifier/monitor source, and evidence owner test; reject before candidate execution. `benchmark_evidence_protected_input_matrix`. |
| Toolchain/cache/offline | Image/tool/cache/config identity succeeds; tag/image/tool swap, missing/stale lock, incomplete cache, registry update, network, and source/cache/lock/target cross-write reject. `benchmark_evidence_toolchain_matrix`. |
| Native host/isolation | Cover x86_64 success; ARM/emulation, CPU/microcode/kernel/daemon/runtime/cgroup/image mismatch, quota/writable source/network/device/capability/credential/socket/cross-revision exposure reject. A latched foreign scheduler/container, throttle/thermal/frequency, pressure/load, swap/memory event during any child, monitor loss/overflow/delay/death, duplicate/reused/overlapping range, wrong child ID, or orphan child observation rejects. `benchmark_evidence_host_isolation_matrix`. |
| Descriptor/environment | Collide every inherited fd; cover missing CLOEXEC, unexpected fd, stdio swap, proxy/Cargo/Rust/Git/locale/home injection, truncation, and capture overflow. `benchmark_evidence_process_boundary_matrix`. |
| Schedule | Four exact prepare children complete and their artifacts are sealed before exact warm-ups and odd B-C/even C-B pairs. Build/Cargo/compiler use during measurement, artifact mutation, overlap, reorder, retry, skip, duplicate, crash, timeout, signal, and nonzero reject. `benchmark_evidence_schedule_matrix`. |
| Inner/outer statistics | Synthetic odd/even nanosecond sums, half-microsecond ties, equal middle values, overflow edges, and one arbitrarily low outlier pin the middle sum, round-half-up integer-microsecond quantization, and three-decimal rendering. Ten printed samples retain exact tokens; exact 1.05 passes and the next microsecond unit fails. `benchmark_evidence_statistic_matrix`. |
| Parser/arithmetic | Exact titles, headers, three rows, all columns, and five retained fields succeed; wrong/duplicate/missing/extra line, row, header, or field and whitespace/sign/exponent/nonfinite/precision/ratio/zero/overflow reject. `benchmark_evidence_parser_ratio_matrix`. |
| Report/signature | Bidirectional report and merge-verification goldens plus every field/order/type/width/duplicate/escape/trailing/derived mutation; exact SSHSIG binary fields and signing preimage; armor header/footer/LF/base64 alphabet/wrap/padding; and wrong key/namespace/signature/profile/stale candidate/report/merge reject. `benchmark_evidence_report_v1_matrix`. |
| Failure/cleanup | The benchmark-input owner first covers absent/relative/missing/non-directory/final-symlink (including repeated-separator and `/.` aliases)/root/repository/in-repository/nonempty work roots, foreign residue, and descriptor-relative file/symlink clearing after error/signal without deleting directory entries, caller entries, or leaving a TERM-ignoring descendant. Script-level failure leaves at most its directory-only owned tree; the trusted outer controller removes it only after container/process teardown. The evidence owner crosses each phase with error, timeout, signal, disk-full, file/directory/parent/reservation fsync, unlock, rename, reservation removal, ordinary removal, and signing failure. No accepted path remains; children, containers, mounts, fds, locks, staging, private dirs, output, and publication reservation are gone, or a surviving fail-closed reservation prevents acceptance and all later work. `benchmark_input_workdir_matrix`; `benchmark_evidence_cleanup_matrix`. |
| Concurrent runs | Second run fails before Git/image/container work while either the lock or publication reservation exists. Lock releases only after measurement cleanup and durable reservation creation; accepted publication removes and fsyncs the reservation. Crash/restart and administrator-recovery fixtures prove fail-closed behavior. `benchmark_evidence_exclusive_run`. |
| TOCTOU swap | The preparation root is retained before untrusted work and every accepted Linux write stays below its descriptor. Its manifest digest crosses the prepare/native phase boundary in trusted controller state; the root descriptor crosses the shell/launcher boundary; manifest traversal stays below it; and executable/runtime bytes are copied to fully write-sealed anonymous descriptors before exec/preload. Cleanup recursively mutates only the retained tree. Opened image/source identities are used or revalidated at their privilege boundary; root rename/replacement, same-inode artifact writes, daemon-image, and source swaps cannot substitute inspected bytes. `benchmark_input_workdir_matrix`; `benchmark_evidence_bound_object_swap_matrix`. |
| Forged/stale evidence | Unsigned, edited, replayed profile/target/review/candidate, truncated, concatenated, and valid-signature/wrong-namespace reports reject. PR/preflight/trusted-review mismatch blocks; `CLEAN` maps only to `clean`, and accepted `FINDINGS` with a nonempty repair chain maps only to `fixed`. `benchmark_evidence_stale_forged_matrix`. |
| Base/integration race | Disposable remote covers target movement before/after run, precheck-to-merge race, unavailable/wrong response OID, local-target mismatch, wrong merge parents/tree, signed merge-verification mutation, a normal later first-parent descendant, force-push/removal before the final trusted refetch, failed revert, and exact merge. The merge must remain on the final target first-parent chain; failures never advance lifecycle. `benchmark_evidence_merge_race_matrix`. |

The base/integration race capability is merged in PR #840. It consumes the already merged identity,
revision, report, signature, cleanup, and exclusive-run boundaries and proves the disposable
provider/lifecycle relationship without introducing provider credentials or a live service adapter.
Its completed closure matrix is:

| Axis | Closure and owner |
|---|---|
| Disposable remote | Validate the target ref, commit objects, parent order, candidate tree, and response identity before any transition; keep all state in a fixture-owned remote. `benchmark_evidence_merge_race_matrix`. |
| Base precheck | Bind the local target and remote target to `BASE`; a target move before the run or a local/remote mismatch rejects before merge and never advances lifecycle. `benchmark_evidence_merge_race_matrix`. |
| Merge response | Bind the provider response OID to a freshly fetched raw object; unavailable, unknown, or wrong response identity rejects and leaves the transaction unshipped. `benchmark_evidence_merge_race_matrix`. |
| Merge relationship | Require the fetched merge's exact two parents `(BASE, CANDIDATE)` and candidate tree; wrong parents or tree invoke fail-closed revert, and a failed revert leaves the remote blocked. `benchmark_evidence_merge_race_matrix`. |
| Signed artifact | Verify the signed merge-verification payload against the fetched raw merge, then detect a payload/signature mutation before acceptance. `benchmark_evidence_merge_race_matrix`. |
| Final refetch | After staging, accept a normal later first-parent descendant that still contains `MERGE`; a force-push, target removal, or unavailable final fetch rejects and never advances lifecycle. `benchmark_evidence_merge_race_matrix`. |
| Revert/lifecycle | Exact merge reaches `accepted` only after every relationship and final refetch passes. Every failure reaches `rejected`, `reverted`, or `blocked`; no failure advances lifecycle or emits an accepted artifact. `benchmark_evidence_merge_race_matrix`. |

The next capability is intentionally limited to the trusted controller/verifier orchestration core.
It consumes the merged bootstrap, CLI, profile, host/image/container, source, process, monitor,
schedule, cleanup, exclusive-run, report, signature, and merge-race boundaries. It provides the
phase ordering and trusted-artifact handoff as a deterministic fixture-owned consumer. It does not
inspect a real host, invoke Docker, query GitHub, receive provider credentials, run the performance
workload, perform cryptographic key management, or advance the post-merge lifecycle. Its
implementation closure matrix is:

Implementation maps every row to source and deterministic tests before review. Tests use fixture
gates, executors, artifact stores, and test keys; they do not inspect the host, contact a provider,
run Docker, run the performance workload, or assert wall-clock ratios. Native host qualification,
cryptographic key-process integration, provider/review API integration, and final Request 7
measurement remain named later evidence.

| Axis | Closure and owner |
|---|---|
| Trusted bootstrap and invocation | The run controller consumes only an already parsed `cli.RunInvocation`; the verifier port consumes the explicit bytes corresponding to a parsed `cli.VerifyInvocation`. Bind both to one immutable profile identity bundle covering profile ID, producer/verifier/monitor tool records, host, image, and complete execution identity; reject a mismatched installed manifest, profile digest, target ref, OID, path, or ambient selector before any gate or child. Path resolution remains a later trusted adapter. `benchmark_evidence_controller_verifier_matrix`. |
| Preflight gate ordering | Acquire the exclusive lock and durable profile-global reservation before immutable bootstrap, then run trusted host/image/source/review gates in the declared order and record each completed phase. A gate failure stops before the next gate and cannot create a child, report, signature, or staging output; known pre-child cleanup removes the unused reservation, while uncertain cleanup leaves it fail-closed. `benchmark_evidence_controller_verifier_matrix`. |
| Schedule/executor handoff | Drive the existing `ScheduleState` with an injected fixture executor. Every fixed preparation, warm-up, and alternating sample is started once, has a unique child ID, carries the sealed artifact digest, and is removed from the owned-resource ledger before the next child. Overlap, reorder, retry, build during measurement, artifact drift, timeout, signal, nonzero, and incomplete schedule reject. `benchmark_evidence_controller_verifier_matrix`. |
| Cleanup/publication ordering | Drive the existing `CleanupTransaction` and require zero children/containers/mounts/fds/private directories plus unchanged source/cache manifests before report staging. A trusted staging port writes and fsyncs the exact report/signature pair while the host lock is held; its idempotent discard port owns partial-stage cleanup. Durable reservation precedes lock release; publication precedes reservation removal; any publish, unlock, or cleanup failure emits no accepted result and leaves a fail-closed reservation when ownership is uncertain. `benchmark_evidence_controller_verifier_matrix`. |
| Report/signature handoff | Accept report bytes and signature bytes only from trusted fixture-owned producers after schedule and cleanup completion. The run-side verifier checks those bytes without PR inputs and returns the immutable produced record it actually checked; the lock-held staging port and later publisher receive that exact record through the result. The post-PR verifier separately consumes the explicit report/signature, PR body, and review attestation. The controller does not accept a candidate-provided report, opens no candidate module, and cannot publish different bytes. `benchmark_evidence_controller_verifier_matrix`. |
| Report semantic reconstruction | Reconstruct the fixed child schedule and monitor ranges, every sample/token, baseline/candidate arrays, sorted permutations, middle sums, exact threshold comparison, verdict, run ID, baseline raw parents with an empty candidate inventory (`commits` and `changed_paths`), candidate first-parent chain, candidate final raw SHA-256, and protected-input relationships; bind every preparation manifest to the controller-owned schedule map; reject any candidate changed path that is also protected; stored derived values are never trusted. A regression is published only as a distinct non-accepted result with exit status 1. `benchmark_evidence_controller_verifier_matrix`. |
| Verifier binding | Decode the canonical report, validate the full existing report schema and body digest, bind the complete profile identity bundle plus baseline/candidate/target/review fields to explicit trusted expectations, require `target.run_oid == BASE`, validate clean/fixed review-chain semantics (a fixed review head must be strictly after `BASE`) and every candidate commit parent, reject a candidate commit inventory containing `BASE`, reject overflow in both threshold cross-products, preserve the baseline's raw parent list while requiring only its candidate inventory to be empty, reject a changed/protected path overlap, parse exactly one value for every recognized PR-body preflight marker, and require an injected cryptographic signature check with the fixed report namespace/key. Wrong bytes, namespace, key, signature, attestation, repair chain, marker, identity, verdict, expected OID, baseline-rooted review, baseline pseudo-entry, overflow, overlap, or a malformed baseline inventory reject without repository, build, benchmark, network, or output mutation. `benchmark_evidence_controller_verifier_matrix`. |
| Failure and restart | Every exception has one terminal rejected or fail-closed result. The durable reservation is installed before any gate or child, remains while the lock is released and publication settles, and blocks a second invocation; known abort removes it only while the host lock is held and restores it if unlink or directory fsync fails. Final reservation removal reacquires the host lock and restores the reservation if unlink or directory fsync fails; `LOCK_UN` is the final guard transition, and descriptor close is best effort only after that transition. If reservation creation or restoration fails and durability is uncertain, the lease retains the host lock rather than releasing both guards. If abort itself fails, the returned cleanup evidence is conservatively fail-closed and reports the possible reservation. Accepted state is reachable only after the complete report/signature pair is durably published and the reservation is removed. Crash/restart and cleanup-failure fixtures never turn partial state into accepted evidence. `benchmark_evidence_controller_verifier_matrix`; `benchmark_evidence_exclusive_run`. |
| Explicit deferrals | Real host inspection, Docker daemon/image execution, GitHub review API/token isolation, `ssh-keygen`/Ed25519 signing, raw Git revision construction, merge verification against a provider, and post-merge lifecycle advancement remain later capabilities and are not faked by this core. `benchmark_evidence_controller_verifier_matrix`. |

The controller/verifier owner is an orchestration boundary, not accepted Request 7 evidence. It
proves that a producer can publish report/signature bytes without post-PR inputs, that the separate
verifier cannot consume them out of order, and that a verifier failure cannot cross the publication
boundary; later capability owners must replace each fixture gate with the corresponding privileged
adapter before host qualification or measurement.

This capability is expected to exceed roughly 1,000 hand-written lines when its controller,
verifier, and shared adversarial owner are counted. Keeping the strict dormant producer-to-consumer
chain together lets one report-only producer fixture and one explicit post-PR artifact fixture prove
both verifier bindings and the publication barrier; splitting the chain further would duplicate
those fixtures and leave the controller's byte handoff unreviewed.

## Native host qualification implementation closure

PR #842 merged the deterministic controller/verifier consumer. The next capability is the
privileged native host/daemon adapter. It reads the named Linux host and Docker observations,
constructs the existing canonical inspection records, and passes them through the already merged
`host.qualify` boundary. It does not widen the profile, inspect candidate code, execute the image,
start a benchmark, provision a signing key, or claim accepted Request 7 evidence by itself.

The adapter uses fixed absolute commands, an empty environment, bounded output, deterministic
parsers, and injected readers/runners in its owner. A real host run is an administrator
self-qualification operation; the deterministic owner must not depend on the current machine's
CPU, kernel, cgroup, Docker, or load state.

| Axis | Implementation and owner |
|---|---|
| Host identity | Read the fixed machine identity, kernel, architecture, CPU vendor/family/model/stepping, microcode, online CPU set, NUMA set, and physical memory through no-follow/fixed sources. The host ID and benchmark CPU set come only from root-owned, benchmark-account-unwritable `/etc/align-evidence/host-id` and `/etc/align-evidence/benchmark-cpus`; the kernel sources are the fixed `/proc` and `/sys` paths in `native_host.py`. Normalize only the profile-specified x86_64 spelling; reject missing, repeated, malformed, or cross-architecture values. `benchmark_evidence_native_host_matrix`. |
| CPU and quota boundary | Read the host cgroup quota and require the profile's unquotaed value. cgroup v2 accepts only `cpu.max=max <positive-period>` for unquotaed execution; the v1 fallback accepts only `cpu.cfs_quota_us=-1` with a positive period, and rejects malformed, zero, or positive quotas. Require the profile benchmark CPU set to be an online, canonical set and the NUMA set to be present; reject aliases, empty sets, quota, migration, and an ARM/emulation identity before Docker work. `benchmark_evidence_native_host_matrix`. |
| Docker daemon identity | Invoke only the pinned Docker client with fixed argv, empty environment, the root-owned empty `/etc/align-evidence/docker-empty` config directory, the explicit local `unix:///var/run/docker.sock` endpoint, bounded stdout/stderr, no shell, and a timeout. Hash the retained executable descriptor and compare it with the profile before either command executes; then execute both commands through that same descriptor. Parse client/daemon version, client bytes, daemon architecture, storage driver, cgroup version, and the daemon-reported `RuncCommit.ID` runtime identity in the existing canonical order; reject daemon unavailability, nonzero/truncated output, wrong architecture, alias-only runtime data, or profile mismatch. `benchmark_evidence_native_host_matrix`; `benchmark_evidence_process_boundary_matrix`. |
| Qualification snapshots | Capture exactly `pre`, `between`, and `post` snapshots for load, CPU/memory pressure, free memory, and swap counters; use integer parsing and fixed counter units, reject any pressure/swap counter reset, then call `host.qualify` so profile limits and order remain one validation boundary. A missing source, reset/overflow, invalid unit, or extra phase rejects. `benchmark_evidence_native_host_matrix`. |
| Ownership and failure | The adapter owns only opened descriptors and child processes it created; it closes/reaps them on every path, sends bounded TERM/KILL cleanup to the complete process group even after its leader exits, treats only `ESRCH` as an already-gone group, never mutates host configuration or repository state, and reports no qualified record after a timeout, close/reap uncertainty, parser error, or cleanup failure. `benchmark_evidence_native_host_matrix`. |
| Explicit deferrals | Image self-inspection/toolchain reproduction, the monitor's child event stream, cryptographic key-process integration, provider/review API integration, performance measurement, merge verification, and lifecycle advancement remain later capabilities. `benchmark_evidence_native_host_matrix`. |

## Native host review closure

The first independent review of the native acquisition candidate found five valid boundary gaps. The
fixes remain within this capability and do not widen the accepted evidence claim.

| Finding | Ledger-first closure |
|---|---|
| Remote Docker context could be qualified | The fixed environment names the root-owned empty Docker config directory, both fixed command vectors pass the local Unix socket explicitly, and the owner verifies the exact vectors and environment. A user context, endpoint, credential, or proxy cannot redirect the daemon lookup. |
| Hashed Docker bytes could differ from executed bytes | The real Docker path opens one no-follow executable descriptor, hashes it, executes both fixed commands through `/proc/self/fd/<fd>` with that descriptor passed across exec, and closes it only after both commands finish. Path replacement cannot substitute a second client between hashing and execution. |
| Ineligible host state reached Docker | Native architecture and CPU quota are rejected immediately after their fixed sources are parsed, before the runner is called. The owner uses a rejecting runner to prove both paths are Docker-free. |
| Pressure or swap counters could reset between snapshots | The three constructed observations are compared in order for monotonic CPU-pressure, memory-pressure, and swap read/write totals before Docker output is accepted. A reset is a terminal native acquisition error. |
| A command leader could leave descendants | Cleanup always signals the owned process group, observes the direct child without reaping, escalates to `SIGKILL`, then reaps it. The owner starts a leader-exiting descendant and proves its delayed marker is never written. |

## Native host second-review redesign closure

The revised candidate review found one new P1 and three related process/configuration boundary gaps.
The closure matrix was reopened on the native-host process-trust axis before implementation. The
accepted evidence claim remains unchanged.

| Finding | Ledger-first redesign and owner |
|---|---|
| A mutable Docker executable could still change between hash and exec | `_open_executable` now requires a regular executable owned by root and unwritable by the benchmark account before the retained descriptor is hashed or executed. Root administration remains the explicit trust boundary; the owner covers non-root ownership and group/other-write metadata. `benchmark_evidence_native_host_matrix`. |
| The fixed Docker config directory was not itself trusted | The validator walks `/`, `etc`, `align-evidence`, and `docker-empty` through descriptor-relative `O_DIRECTORY|O_NOFOLLOW` opens, requires root ownership and no benchmark-account write permission at every component, and requires the final directory to be empty. Validation runs before either Docker child is spawned; the owner checks the no-follow flags, cleanup, ownership, permissions, and nonempty rejection. `benchmark_evidence_native_host_matrix`. |
| An exception after spawn could leak the child or its streams | Selector creation, stream setup, nonblocking configuration, and buffer construction are inside the outer cleanup guard. An injected setup failure starts a descendant and proves the complete owned group is torn down. `benchmark_evidence_process_boundary_matrix`; `benchmark_evidence_native_host_matrix`. |
| Reaping the leader before the final group signal could hit a reused PGID | The accepted Linux path observes direct-child exit with `waitid(WNOWAIT)`, sends `SIGTERM`, observes without reaping, sends `SIGKILL`, and only then calls `Popen.wait`. Unsupported non-reaping wait support fails closed before a successful command can cross the boundary. The owner asserts the TERM/observe/KILL/reap order. `benchmark_evidence_process_boundary_matrix`. |

## Native host identity and cleanup review closure

The next revised-candidate review found four actionable identity and fail-closed cleanup gaps. The
closure matrix was reopened on the native-host identity-and-cleanup axis before implementation; the
accepted evidence claim remains unchanged.

| Finding | Ledger-first redesign and owner |
|---|---|
| Docker runtime identity was only the `DefaultRuntime` alias | The adapter now requires the Docker info `RuncCommit` object and records its non-empty `ID`, which is the daemon-reported runtime version or commit identity. A name-only `DefaultRuntime` value is not accepted. `benchmark_evidence_native_host_matrix`. |
| An unprofiled Docker client could contact the daemon | The retained executable hash is compared with `profile.docker.client_sha256` after the no-follow/trust checks and before either Docker child is spawned. The owner proves a mismatch stops at config/open/hash with no runner event. `benchmark_evidence_native_host_matrix`. |
| Profile host files could be changed in place | The trusted source reader requires the fixed `host-id` and `benchmark-cpus` descriptors to be regular, root-owned, and unwritable by the benchmark account before reading them; injected metadata cases cover ownership and group/other write. `benchmark_evidence_native_host_matrix`. |
| Process-group signal failures were hidden | Cleanup ignores only `ESRCH`; every other TERM/KILL failure becomes a native acquisition error after best-effort cleanup, so no successful command or qualified record can cross uncertain cleanup. The owner injects `EPERM` and requires rejection. `benchmark_evidence_process_boundary_matrix`; `benchmark_evidence_native_host_matrix`. |

## Design-review finding closure

| Finding | Ledger-first closure |
|---|---|
| P1 candidate-selected producer/verifier could attest or replay itself | Both entry points are one root-owned installed program pinned by the host profile and independently matched to verified baseline blobs. Acceptance supplies expected OIDs and PR body from trusted-base CI, never candidate arguments alone. |
| P1 report v1 lacked complete nested bytes | The exact outer record, every ordered nested member/type/cardinality, scalar/string grammar, enum, presence rule, relationship, independent encoder, full pass/regression goldens, and mutation matrix are now normative. |
| P1 report digest had a recursive/ambiguous preimage | `body_sha256` is outside `Body` and hashes one exact 45-byte domain plus canonical `Body` without LF. Goldens and mutations own domain, boundary, and digest. |
| P1 host violations could occur wholly inside one child | A root-owned event monitor brackets every child, continuously consumes and latches scheduler/cgroup/thermal/frequency/pressure/memory/swap/container events, and pairs them with 100 ms snapshots. Loss, overflow, delay, or death rejects. |
| P1 inner minima selected unrelated low outliers | Both harnesses keep their operations/round counts but select checked-integer inner medians before the baseline exists; synthetic outlier owners pin the statistic. The controller retains balanced outer pairs and exact rational threshold arithmetic. |
| P2 core-design mirror/index absent | The synchronized Japanese mirror and both core-design indexes are part of this repair. English remains authoritative. |

## Final-review redesign closure

| Finding | Ledger-first closure |
|---|---|
| P1 inner median was not exactly representable by the output token | The harness retains the exact middle nanosecond sum and applies one checked round-half-up conversion to integer microseconds before exact three-decimal rendering. |
| P1 current scripts rebuilt inside every measured invocation | Protected scripts gain a fixed prepare/direct-exec interface before baseline selection. Four monitored preparation children seal artifacts before any warm-up; measurement rejects build activity or artifact drift. |
| P1 deleted and old-side paths were unrepresentable | `PathChange` is the exact two-tree union diff with explicit presence, status, and old/new identities. |
| P1 monitor ranges could be reused | Every preparation/run has a unique child ID, and its range participates in one strict disjoint ordered partition of all child observations. |
| P1 verifier could not observe the actual merge | The installed `verify-merge` mode consumes the fetched provider-response OID and emits a second host-signed canonical artifact bound to the report, target, raw merge object, parents, and tree. |
| P2 unlock failure left accepted-looking durable output | Report bytes remain private while the lock is held; unlock precedes atomic publication, and any unlock/publication failure removes unpublished state. |
| P2 Japanese mirror retained undefined `hex128` | Both languages now define only the used `hex40` and `hex64` grammars and carry the same schemas. |

## Stable-candidate review closure

| Finding | Ledger-first closure |
|---|---|
| P1 caller-owned review log could counterfeit trusted review | A token-isolated trusted-base adapter queries the GitHub review API and supplies one canonical attestation bound to repository, PR, reviewer role, review ID, commit, state, and exact log digest. Fixture API substitutions own every rejection. |
| P1 evidence profile and controller sources were not protected inputs | The protected set now includes the profile/public key, installed-source manifests, controller/verifier/monitor sources, and their adversarial owners. Changing any of them requires a separate reviewed profile requalification before baseline selection. |
| P1 target could move after merge verification | The signed artifact records the fetched target tip containing the exact merge on its first-parent chain; after durable storage the trusted lifecycle adapter refetches and requires that reachability again before advancing. |
| P2 unlock-before-publication admitted a concurrent second run | A durable profile-global reservation is created while locked and remains through atomic publication. Every later invocation rejects before repository/image/container work; crash and cleanup failure remain fail-closed. |
| P2 report review-state literal disagreed with repository metadata | The canonical literals are now exactly `clean` and `fixed`, with explicit one-way mappings from `CLEAN` and accepted `FINDINGS` plus its nonempty repair chain. |
| P2 detached signature bytes were ambiguous | Both signatures now use one exact OpenSSH SSHSIG v1 binary record, SHA-512 signing preimage, canonical 70-column ASCII armor, final LF, and byte-for-byte decode/re-encode validation. |

## Integrated-candidate review closure

| Finding | Ledger-first closure |
|---|---|
| P1 retained prepared path could be replaced after verification | Prepare revalidates its captured private-child device/inode before success. Native carries the captured identity into the launcher, which requires the opened root descriptor to match and performs all manifest traversal below that retained descriptor. |
| P1 same-inode writes could change bytes after hashing | The accepted Linux path hashes while copying executable/runtime bytes into anonymous memfds, checks source metadata around the copy, applies all four write/size/seal seals, and execs/preloads only the sealed copies. The deterministic owner replaces the root at the shell/launcher handoff and proves a sealed copy remains unchanged and unwritable after its source changes. |

## Revised-candidate review closure

| Finding | Ledger-first closure |
|---|---|
| P1 verifier trusted stored derived measurements | The verifier reconstructs the fixed schedule, monitor ranges, sample/token arrays, sorted permutations, middle sums, exact `105/100` comparison, and verdict before signature acceptance. The owner mutates a measured sample while leaving every stored derived field unchanged and requires rejection. |
| P1 pre-review candidate parents were unchecked | Every candidate inventory entry must have exactly one parent equal to the preceding OID from `BASE`; the final revision parent and tree must match the final inventory entry. The owner mutates the first entry's parent and requires rejection. |
| P1 profile and execution identities were partial | One immutable trusted identity bundle is bound at the controller configuration and verifier expectation boundary. Profile ID, complete producer/verifier/monitor records, complete execution record, host ID, and image digest must match exactly; the owner mutates the image identity and requires rejection. |
| P1 finalization could remove the only reservation before fsync failure | Finalization reacquires the host lock before removing the reservation. If unlink or parent fsync fails, the reservation is restored while that lock is held; the owner injects the post-unlink fsync failure and proves a second run remains blocked. |
| P1 regression verdict collapsed into accepted state | Keep `regression` as a distinct non-accepted controller result and publish it with exit status 1; a required-pass invocation rejects it before publication. The owner runs a valid regression artifact through publication and asserts `result.accepted` is false. |
| P1 preparation manifest was not bound to executed children | Compare every report preparation `(benchmark, revision)` manifest to the controller-owned `ScheduleState` map after verification and before staging. A semantically valid report with one altered preparation digest is rejected without publication. |
| P2 candidate revision digest was not bound to its final inventory entry | Require `Revision.commit_sha256` to equal the final `CommitIdentity.raw_sha256`; a candidate report with only that digest altered is rejected. |
| P1 fixed review rooted at the baseline was accepted | Require a fixed `review_head` to be a strict descendant of `BASE`; a fixed report whose review head is the baseline is rejected before accepting its repair suffix. |
| P2 threshold cross-products were not overflow-checked | Compute both `candidate_middle_sum * 100` and `baseline_middle_sum * 105` with checked-u64 multiplication and reject either overflow; the owner mutates valid middle sums to the overflow boundary. |
| P2 abort failure left cleanup evidence optimistic | If pre-publication abort or reservation removal fails, replace the normal cleanup result with conservative fail-closed evidence that reports a possible reservation; the owner injects lease-abort failure and checks state/evidence consistency. |
| P1 baseline raw parents were rejected as empty inventory | Preserve `Revision.parents` exactly for a normal non-root baseline and require only baseline `commits` and `changed_paths` to be empty; the owner uses a baseline with a raw parent and verifies it successfully. |

## Final-integration review closure

| Finding | Ledger-first closure |
|---|---|
| P1 a self-consistent tree could replace prepared state between phases | Prepare prints its canonical manifest SHA-256. The trusted controller retains that value outside candidate-writable state, every native invocation requires it, and descriptor-relative verification rejects a different current manifest before opening executable/runtime artifacts. |
| P1 an untrusted prepare child could redirect later path writes | The accepted controller confines arbitrary candidate writes with its read-only-root container and controller-owned writable mounts. The script retains the private-child descriptor before the first build; configured outputs stay below it, every trusted post-build mutation is descriptor-relative, and publication requires the public path to retain the same device/inode. Script-only native ARM macOS remains trusted-checkout development qualification. |
| P2 cleanup could recursively delete a replacement after a check/path race | Recursive cleanup walks only through the retained private-child descriptor, unlinks non-directory entries, and leaves all directory entries in place while candidate-side concurrency is possible. Script-level failure leaves its directory-only owned tree for trusted outer cleanup after teardown. |

## Final-candidate review closure

| Finding | Ledger-first closure |
|---|---|
| P1 the Linux proc-fd root was incompatible with path-based manifest creation | The manifest module now builds, creates, fsyncs, and verifies a canonical manifest directly below an already-opened root descriptor. Preparation calls only that descriptor API. |
| P1 intermediate symlinks could redirect trusted post-build copies | Preparation retains both root and `artifacts` descriptors before untrusted work. A dedicated helper opens every source component no-follow, creates each destination relative to the retained artifacts descriptor, checks source stability, clears build-tree non-directory entries descriptor-relatively, and verifies the public `artifacts` entry still names the retained object before manifest creation. |
| P2 check-then-`rmdir` could remove an empty replacement | Script cleanup never calls `rmdir`. It unlinks only non-directory entries below retained descriptors, includes empty build-directory skeletons in the success manifest, and leaves the directory-only tree for the outer controller after candidate teardown. |
| P1 direct execution inherited ambient loader/timing state | The launcher constructs the ledger's fixed base environment, then adds only the platform-owned runtime binding and the fixed macOS `DYLD_SHARED_REGION=private` policy. The owner injects an ambient sentinel and makes the harness assert every fixed value and the sentinel's absence. |
| P2 nested cleanup repeated the directory-entry race | Candidate-side preparation and cleanup never remove any directory entry. Both success and failure owners require the retained directory skeleton, and only the outer controller removes it after teardown. |
| P2 the top-level benchmark workflow still described one-phase execution | `bench/README.md` now shows prepare-time digest capture followed by direct native execution and assigns final work-tree removal to the caller after process/environment teardown. |
| P1 build paths alone did not sandbox arbitrary candidate writes | The script contract no longer claims to sandbox candidate code. Accepted evidence requires the later outer-controller container boundary; this slice confines configured outputs and all trusted publication mutations, while script-only ARM qualification trusts the checkout. |
| P1 candidate work could rewrite already-published final artifacts | Final artifacts are created only after every build/compiler child group exits. The helper then copies the fixed compiler/runtime/kernel/harness outputs descriptor-relatively before manifest capture. |
| P2 a FIFO output could block trusted copying | Source opens use nonblocking no-follow descriptors and reject every non-regular output before reading. The owner places a FIFO at the fixed runtime output and bounds rejection with an alarm. |
| P1 candidate-controlled Cargo wrapper dependencies were not protected | The exact protected-input set includes `scripts/cargo.sh` and its sourced `scripts/dyld-env.sh`, so preparation cannot change Cargo/LLVM/linker selection independently of the reviewed evidence profile. |
| P1 manifest publication did not rebind the retained artifacts directory | Manifest publication verifies that the root's `artifacts` entry names the retained descriptor before and after both manifest creation and verification, then compares the captured manifest subtree byte-for-byte with a fresh walk through the retained descriptor. Persistent and transient replacements therefore reject unless they describe the same retained artifacts. |
| P2 bound execution could block while opening a FIFO replacement | Both Linux sealed-copy and native ARM qualification opens are nonblocking/no-follow and reject non-regular descriptors before reading. |
| P2 bound execution accepted additional self-consistent artifacts | The launcher requires the manifest's `artifacts` subtree to be exactly the compiler, runtime, kernel, selected harness, and directory entry; every extra entry rejects before execution. |
| P1 sealed Linux artifacts were not explicitly executable | The launcher always requests the stable Linux UAPI `MFD_EXEC` flag together with sealing. Unsupported kernels and policies such as enforced `vm.memfd_noexec=2` fail closed during host qualification; execution never falls back to mutable path bytes. |
| P1 non-selected prepared files could change after initial verification | After binding the executable and runtime, the launcher re-verifies the complete descriptor-bound prepared tree and retained manifest digest immediately before `execve`. A deterministic owner mutates the effective configuration during binding and proves execution is never reached. |
| P2 macOS qualification omitted the repository's shared-cache isolation | The native ARM launcher fixes `DYLD_SHARED_REGION=private` instead of inheriting ambient loader state, and the executed harness asserts the exact value. |
| P2 macOS verification read a special file before rejecting its type | The launcher checks the initial descriptor mode before hashing. The FIFO owner replaces `os.read` with a rejecting sentinel and proves both platform openers reject without any read. |

## Post-final-review boundary closure

| Finding | Ledger-first closure |
|---|---|
| P1 verifier accepted an incomplete monitor lifecycle | Reconstruct every report observation as a `MonitorObservation`, require dense ordinals and phase/child identity consistency, and replay the complete `MonitorLifecycle`, including pre-build, every child boundary/sample, between-child, post-run, monotonic progression, counter monotonicity, delay ceiling, expected child order, and latched-event rejection. The owner mutates a valid report to omit lifecycle phases, break ordinals or phase/child identity, move time backwards, and reset a counter. |
| P1 known abort removed the reservation after unlocking | `ExclusiveRun.abort(remove_reservation=True)` removes and fsyncs the reservation while the host flock is held, restores it on any unlink/fsync failure, and only then performs the explicit unlock. The owner injects abort directory-fsync failure and proves a second acquisition remains blocked. |
| P1 producer `run` required post-PR inputs | Split `ProducedEvidence` from `EvidenceArtifact`: the run controller accepts only report/signature bytes and report-only expectations, while `verify_artifact` separately adds the explicit PR body and trusted review attestation. The owner asserts that the producer receives no post-PR fields and that both verifier boundaries preserve exact bytes. |
| P1 protected input could also be a candidate changed path | Report semantics now reject the set intersection between `candidate.changed_paths` and `protected_inputs.entries`; the valid fixture uses disjoint paths and the owner mutates it into an overlap. |
| P1 final lock close could lose the reservation | Finalization treats explicit `LOCK_UN` as the guard transition and performs descriptor close only as best-effort cleanup afterward. A failed `LOCK_UN` restores the reservation while the lock is still held; a real close failure after successful `LOCK_UN` cannot be reported as a failed finalization, and the owner proves the guard transition and actual close attempt separately. |
| P1 finalization recovery could release both guards | If reservation creation or finalization removes the reservation and restoration/durability fails, `ExclusiveRun.abort(remove_reservation=False)` refuses to release a held lock while the reservation is uncertain. The owner injects creation fsync failure, finalization restore failure with an absent path, and finalization parent-fsync failure with a present-but-not-durable path; it proves a second lease is blocked by the retained guard, then restores the reservation before releasing the fixture lease. |
| P1 replay normalized a child ID | Replay compares the complete lifecycle-produced observation tuple with the signed report tuple after enforcing dense ordinals, so a sample/end observation cannot silently inherit the active child ID. The owner mutates a valid interior child ID and requires rejection. |
| P1 replay omitted a report-visible swap event | Replay and production lifecycle bookkeeping treat an increase or reset of the report-visible swap totals as a latched `swap` event/counter violation. The owner mutates swap totals across the observation stream and requires rejection. |
| P1 unlock could precede durable report staging | Add explicit lock-held `stage_report` and idempotent `discard_staging` ports. The controller calls staging only after cleanup validation and before reservation/lock release, and calls discard on every later or partial-stage failure; the owner proves the order and failure cleanup. |
| P2 candidate inventory could contain a baseline pseudo-entry | Reject `BASE` in the candidate commit inventory before first-parent reconstruction. The owner prepends a self-parent baseline record and requires rejection. |

## Author consistency pass

- Ledger, threat model, workload, report, delivery, and matrix use one baseline/candidate, profile,
  controller/verifier, image/host, protected set, schedule/parser, exact threshold, and failure rule.
- Canonical report v1 and post-merge verification v1, each with its detached signature, are the sole
  exchanged formats. Every field has an owner, malformed rule, and identity; there is no float or
  ambient default.
- Provider credentials are N/A. The sole secret is the host signing key outside all candidate
  containers and opened only after cleanup through a non-inheritable descriptor.
- Language/API/ABI/ownership changes are N/A: this adds developer evidence tooling only.
- Correctness remains in Request 7 compiler/runtime owners; the controller accepts only the required
  performance comparison.
