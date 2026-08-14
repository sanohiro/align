# JSON escape benchmark evidence

Status: proposed design for align-llm Request 7. This document defines the evidence boundary only.
It does not accept the JSON language change, select its immutable baseline, or make a performance
claim. The evidence implementation and benchmark-input prerequisite must merge before the Request
7 implementation branch exists.

## Purpose and threat model

Request 7 changes code that produces the values being measured. Its implementation therefore may
not choose the baseline, controller, workload, toolchain, host, sample order, parser, threshold, or
attestation that judges it. A benchmark script can execute candidate compiler/runtime code; it
cannot publish an accepted result by itself.

The trusted boundary is a reviewed controller already present in the immutable baseline; one
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
| Public producer | `scripts/json-escape-benchmark-evidence run --repository REPO --baseline BASE --candidate CANDIDATE --review-log REVIEW_LOG --output-dir NEW_DIR` executed from a clean raw checkout of `BASE`. The launcher verifies its own script, modules, profile, and key bytes/modes against `BASE` before other work. All paths are absolute. OIDs are lowercase 40-hex. `NEW_DIR` must not exist. No other positional arguments, environment selectors, image/host overrides, sample counts, modes, or thresholds exist. |
| Public verifier | `scripts/json-escape-benchmark-evidence verify --report REPORT --signature SIGNATURE` checks an existing pair against the merged profile/public key. It performs no build, checkout, benchmark, network, or repository mutation. |
| Ambient state | There are no semantic defaults. The launcher enters an empty environment containing only fixed `PATH`, `LC_ALL=C`, `TZ=UTC`, empty `HOME`, `CARGO_NET_OFFLINE=true`, and controller-created descriptor/configuration values. Ambient Git, Cargo, Rust, Docker, locale, proxy, credential, target, and tuning variables are omitted. |
| Result | Success atomically creates exactly `report.json` and `report.json.sig` in `NEW_DIR`, fsyncs both and the directory, prints their absolute paths, and exits zero. A threshold failure creates a valid signed `regression` report and exits 1. Every other failure removes private staging, leaves `NEW_DIR` absent, emits no accepted path, and exits nonzero. |
| Controller owner | A checked-in Python 3 controller and fixture tests under `scripts/` and `tests/benchmark_evidence/`. The exact source, interpreter, Git, Docker client/daemon, `ssh-keygen`, kernel, OCI image, host profile, and allowed executable SHA-256 identities are recorded. |
| Persisted format | Canonical UTF-8 JSON schema `align.json_escape_benchmark_evidence/v1`, signed byte-for-byte with the host Ed25519 key using `ssh-keygen -Y sign` namespace `align-json-escape-benchmark-evidence-v1`. Unknown, missing, duplicate, reordered, non-ASCII key, non-integer number, float, invalid UTF-8/escape, trailing byte, or noncanonical serialization rejects. |
| Ownership/allocation | The controller owns all temporary directories, pipes, children, containers, captures, report staging, and cleanup. Benchmark children receive only stdin `/dev/null`, private stdout, and private stderr. Captures have fixed profile ceilings. No child receives the report, signature, repository administration, controller, Docker socket, signing key, or another revision's writable directory. |
| Concurrency | One controller obtains the profile's host-global exclusive lock before repository inspection and holds it through signing and cleanup. Baseline and candidate never overlap. The fixed pair order is the only schedule. |
| Prerequisites | The benchmark-input slice, both Request 7 language prerequisites, this design, its evidence implementation, pinned image, host profile/public key, and controller adversarial owners merge before `BASE` is selected. `BASE` is the then-current target tip and exact parent of the first Request 7 implementation commit. |
| Acceptance | A valid signature, `pass` verdict, exact PR/preflight/review bindings, unchanged target-base binding at merge, identical protected inputs, all ten samples for all five fields, and every exact ratio at most 1.05 are all required. Correctness tests remain separate deterministic owners. |

`REVIEW_LOG` is the repository-standard pre-open review log outside the candidate repository. It
must bind the candidate directly with `CLEAN`, or bind one reviewed ancestor with `FINDINGS` and a
complete findings-fixed descendant chain ending at `CANDIDATE`. The controller records the log
SHA-256 and bindings; it does not claim untrusted prose is independently authentic. Acceptance also
requires the trusted-base PR attestation to carry the identical candidate, merge base, review head,
reviewer, and clean/findings-fixed state. An edited log cannot create that required status.

## Merged profile and fixed identities

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

The named host is operationally reserved. The controller samples load, CPU pressure, throttling,
steal time, governor, temperature/throttle flags, free memory, swap activity, and running containers
before build, between every child, and after the last child. The profile fixes admissible values.
Any violation rejects the run instead of deleting a slow sample. Observations enter the report. The
controller does not claim to detect a malicious host administrator.

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
```

Presence, path, mode, type, blob bytes, and structural manifest must match across revisions.
Optional-root presence mismatch rejects. This comparison covers scripts, kernels, harnesses,
manifests, lockfiles, generators, output format, timing loops, and nested configuration before any
candidate code executes.

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

Commands are argv arrays without shell interpolation. All root and detached Cargo operations are
visibly `--locked --offline`; `CARGO_NET_OFFLINE=true` is defense in depth. Registry/cache/source
manifests are compared before/after; a lockfile, index, cache, source, or configuration write
rejects. Each build starts with an empty target; a revision keeps its private target for warm-up and
samples.

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

Builds finish for both revisions before warm-up. The controller then runs only:

```text
bench/json_decode/run.sh native
bench/json_soa/run.sh native
```

The emitted native target must match the profile CPU. Feature downgrade, cross compilation,
emulation, or changed effective Cargo/rustc configuration rejects.

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
nonfinite token, invalid ratio suffix, or profiling output rejects. Each retained token is positive
ASCII decimal with exactly three fractional digits and converts without
floating point or rounding to integer microseconds. Original token and integer are reported. For
each field/revision, sort ten integers and define median `(sample[4] + sample[5]) / 2`. Store the
middle sum and denominator `2`, never a rounded value. Compare exactly:

```text
candidate_middle_sum * 100 <= baseline_middle_sum * 105
```

All five must pass. Zero baseline, overflow, missing sample, parse error, or warning rejects. The
harness parity assertions are useful pre-timing guards but do not replace deterministic correctness
owners.

## Canonical report and signature

`report.json` uses fixed key order and ASCII keys. Strings are UTF-8; digests/OIDs are fixed-width
lowercase hex; measurements/counts are integers. Arrays preserve normative order. There are no JSON
floats, nulls, optional unknown fields, or timestamp identities.

The top-level record contains, in order:

1. schema/profile/controller/verifier versions and source/executable identities;
2. a 256-bit host-CSPRNG run ID, UTC start/end, and host lock identity;
3. baseline/candidate raw identities, parent chain, target-tip observation, changed-path inventory,
   and protected-input manifests;
4. review-log digest and parsed head/base/verdict/findings-fixed bindings;
5. host/kernel/CPU/memory/daemon/image/toolchain/cache/config/environment/mount/fd/limit observations;
6. ordered pre/between/post resource observations;
7. per benchmark: argv, warm-up capture identities, then ten ordered pairs with revision,
   stdout/stderr hashes, exit/duration, parsed token(s), and integer values;
8. per field/revision: original/sorted samples, middle sum and denominator; then exact ratio and
   threshold result;
9. cleanup inventory, verdict `pass` or `regression`, and first failed threshold; and
10. SHA-256 over the canonical unsigned semantic record.

The producer encodes, reparses with duplicate rejection, re-encodes, and requires byte identity
before signing. The verifier repeats schema/canonical validation, relationships, sample
reconstruction, medians, exact comparisons, identities, key fingerprint, and signature; it does not
trust stored derived values. Goldens cover both directions and one mutation of every scalar,
ordering, presence, duplicate, width, signature, and derived field.

An accepted PR carries the complete report/signature as unmodified files in an immutable artifact
or base64-safe fenced attachment, plus verifier command/result. Copying is allowed; changing one byte
invalidates the signature. A report for another baseline, candidate, profile, host, toolchain,
review, sample set, or target is stale. The private key opens only after measurement containers are
gone, through a non-inheritable descriptor. Signing failure leaves no output directory.

## Review, base drift, and integration

Measurement begins after one comprehensive review yields a clean exact-candidate verdict or one
consolidated findings-fixed chain. Any later commit, amend, rebase, merge, or protected-input change
invalidates the report. Only raw commit OIDs/objects are identities.

At publication, preflight and PR attestation must match the report candidate, merge base, review
head, and state. Immediately before merge, target OID must still equal `BASE`; otherwise rebase from
the new tip, select that parent as a new baseline, and repeat review/evidence/preflight. Merge binds
the expected PR head. Its returned commit must have first parent `BASE`, second parent `CANDIDATE`,
and candidate-identical tree. The merge verifier checks raw objects under the same Git isolation. A
race makes the result unshipped and it is reverted before dependent work; no lifecycle advances.

This postcondition is cleanup for a hosting transaction that cannot currently atomically accept an
expected base OID. Before Request 7 starts, implementation must prove fail-closed merge/revert on a
disposable remote or replace it with a provider compare-and-swap that binds endpoint, principal,
repository, expected base/head, request bytes, response, and secret handling in a reviewed amendment.
It may not weaken the base rule.

## Delivery order

1. Merge the benchmark-input slice: check in detached locks, remove ignores, make all four Cargo
   invocations locked/offline, confine generated outputs, and add deterministic missing/stale/cache,
   no-network, no-write, and cleanup owners.
2. Merge evidence implementation: controller, verifier, profile, pinned image recipe/digest, public
   key, host guide, adversarial fixtures, format goldens, and merge-race owner. It makes no Request 7
   performance claim.
3. Provision the host private key, independently verify image/cache/toolchain/profile, and run host
   self-qualification.
4. Only then select current target tip as `BASE` and create the Request 7 branch directly on it.
5. Finish correctness owners, review final candidate, produce/verify evidence, publish, keep target
   at `BASE`, merge, verify the merge object, and advance lifecycle.

## Implementation closure matrix

| Cell | Owner and exact regression |
|---|---|
| CLI construction | Reject missing/extra/repeated options, relative paths, malformed/same OIDs, existing output, untrusted controller checkout/repository, and ambient selectors before mutation. `benchmark_evidence_cli_matrix`. |
| Raw identity/construction | Cover clone/worktree, packed/loose objects, reviewed symlinks, hostile config/includes/hooks/filters/fsmonitor/commit-graph/replacements/grafts/alternates/promisor/shallow/missing objects, raw swap, path/type/mode collisions, and mutation race. `benchmark_evidence_raw_object_matrix`. |
| Revision binding | Exact parent chain succeeds; wrong local target, unrelated ancestry, merge/side parent, ref movement, stale review, and branch-after-drift reject. The report exposes every changed path/mode for review scope disposition. `benchmark_evidence_revision_binding_matrix`. |
| Protected inputs | Mutate presence/path/type/mode/bytes of every required/optional config, manifest, lock, script, kernel, harness, generator, output, and timing owner; reject before candidate execution. `benchmark_evidence_protected_input_matrix`. |
| Toolchain/cache/offline | Image/tool/cache/config identity succeeds; tag/image/tool swap, missing/stale lock, incomplete cache, registry update, network, and source/cache/lock/target cross-write reject. `benchmark_evidence_toolchain_matrix`. |
| Native host/isolation | Cover x86_64 success; ARM/emulation, CPU/microcode/kernel/daemon/runtime/cgroup/image mismatch, quota/load/throttle/swap, writable source, network/device/capability/credential/socket/cross-revision exposure reject. `benchmark_evidence_host_isolation_matrix`. |
| Descriptor/environment | Collide every inherited fd; cover missing CLOEXEC, unexpected fd, stdio swap, proxy/Cargo/Rust/Git/locale/home injection, truncation, and capture overflow. `benchmark_evidence_process_boundary_matrix`. |
| Schedule | Exact warm-ups and odd B-C/even C-B pairs succeed; overlap, reorder, retry, skip, duplicate, crash, timeout, signal, and nonzero reject. `benchmark_evidence_schedule_matrix`. |
| Parser/arithmetic | Exact titles, headers, three rows, all columns, and five retained fields succeed; wrong/duplicate/missing/extra line, row, header, or field and whitespace/sign/exponent/nonfinite/precision/ratio/zero/overflow reject. Exact 1.05 passes; next unit fails. `benchmark_evidence_parser_ratio_matrix`. |
| Report/signature | Bidirectional golden plus every field/order/type/width/duplicate/escape/trailing/derived mutation; wrong key/namespace/signature/profile/stale candidate reject. `benchmark_evidence_report_v1_matrix`. |
| Failure/cleanup | Cross each phase with error, timeout, signal, disk-full, fsync, removal, and signing failure. No accepted path remains; children, containers, mounts, fds, locks, and private dirs are gone. `benchmark_evidence_cleanup_matrix`. |
| Concurrent runs | Second run fails before Git/image/container work; lock releases only after cleanup, including signal/sign failure. `benchmark_evidence_exclusive_run`. |
| TOCTOU swap | Opened executable/image/source identities are used or revalidated at privilege boundary; rename, replacement, daemon-image, and source swaps cannot substitute inspected bytes. `benchmark_evidence_bound_object_swap_matrix`. |
| Forged/stale evidence | Unsigned, edited, replayed profile/target/review/candidate, truncated, concatenated, and valid-signature/wrong-namespace reports reject. PR/preflight mismatch blocks. `benchmark_evidence_stale_forged_matrix`. |
| Base/integration race | Disposable remote covers target movement before/after run, precheck-to-merge race, wrong merge parents/tree, failed revert, and exact merge. Failures never advance lifecycle. `benchmark_evidence_merge_race_matrix`. |

Implementation maps every row to source and deterministic tests before review. Tests use fixture
executors, a fake daemon, disposable repositories/remotes, and test keys; they do not run the
performance workload or assert wall-clock ratios. Native host qualification and final Request 7
measurement remain named manual evidence.

## Author consistency pass

- Ledger, threat model, workload, report, delivery, and matrix use one baseline/candidate, profile,
  controller/verifier, image/host, protected set, schedule/parser, exact threshold, and failure rule.
- Canonical report v1 plus detached signature is the sole exchanged format. Every field has an owner,
  malformed rule, and identity; there is no float or ambient default.
- Provider credentials are N/A. The sole secret is the host signing key outside all candidate
  containers and opened only after cleanup through a non-inheritable descriptor.
- Language/API/ABI/ownership changes are N/A: this adds developer evidence tooling only.
- Correctness remains in Request 7 compiler/runtime owners; the controller accepts only the required
  performance comparison.
