# S0A startup observation design

> **Status:** EXACT DESIGN ACCEPTED; implementation is next.
>
> **Authority:** This document is the exact S0A slice ledger required by
> [`31-execution-storage-startup-plan.md`](31-execution-storage-startup-plan.md).
> It owns only the local startup evidence harness. It adds no Align language,
> library, compiler CLI, runtime ABI, cache, artifact, or distribution contract.

## 1. Capability boundary

S0A adds one local benchmark family under `bench/startup/`. It builds ten fixed
Align fixtures with an explicitly identified compiler/runtime arm, then measures
each resulting executable from the parent immediately before `posix_spawn`
through direct-child `wait4` reap. A second candidate arm is optional; when it
is present, the parent counterbalances baseline/candidate order.

The harness observes existing programs. It does not change their MIR, generated
objects, link closure, runtime behavior, cache identity, or executable bytes. It
does not instrument, pause, preload, inspect, or export a probe from a timed
child. It introduces no performance threshold. Raw samples and a median are
evidence for later admission decisions, not a CI timing gate.

Implementation files are one useful failure domain:

```text
bench/startup/Cargo.toml
bench/startup/Cargo.lock
bench/startup/src/main.rs
bench/startup/tests/*.rs
bench/startup/run.sh
bench/startup/README.md
bench/startup/fixtures/*.align
```

The standalone Rust observer depends only on `libc`, `sha2`, and the repository's
small `align_hash` crate. The last supplies the one canonical streaming wyhash
implementation needed to validate existing cache CAS identity; no hash
algorithm is copied. It is not a workspace member and no compiler crate may
depend on it. The observer is built before a benchmark invocation with exact
repository command `scripts/cargo.sh build --release --locked --manifest-path
bench/startup/Cargo.toml --target-dir ABS`. That developer build is not part of
the evidence-producing entry point. Its complete output binary is instead an
explicit content-addressed input to `run.sh`: public options `--observer ABS`
and `--observer-sha256 SHA256` name the regular file and its expected digest. A
differently configured Cargo build therefore has a different identity and
cannot silently join the same evidence run; S0A makes no claim that distinct
Rust/Cargo configurations reproduce identical observer bytes.

`run.sh` is Bash-3.2-compatible. It validates the complete public option shape,
including observer path and digest, using shell built-ins before it creates a
directory, copies a file, or starts any child. It then copies the supplied
observer executable into one exclusively created mode-0700
`target/startup-observer/run.XXXXXX/` directory, fixes the copy to mode 0500,
and opens that private regular file on fixed descriptor 9. The private
directory contains exactly that one file. The wrapper nofollow-verifies the
directory/file identities, exact modes, effective-uid ownership, descriptor-to-
path identity, and link count one, then unlinks the file and removes the
now-empty directory. It uses Bash 3.2's `exec -c` to replace itself with an
empty environment and exact argv
`[/dev/fd/9, "--adopt-fd", "9", "--expected-sha256", SHA256,
ORIGINAL_PUBLIC_ARGS...]`. No loader,
allocator, locale, path, home, cache, or other caller variable reaches the new
image. At entry the
observer accepts exactly that internal prefix followed by the four public
options, and requires descriptor 9 to be open, inheritable,
regular, mode 0500, effective-uid-owned, and link count zero; it retains and
hashes that same descriptor and requires equality with the internal expected
digest. Before parsing public options it also requires
that native `environ` contain zero entries; a direct invocation or wrapper
drift exits 1. `OBSERVER_SHA256` therefore identifies the exact
unlinked inode selected by `exec`, rather than a later resolution of the
concurrently replaceable shared build path. A malformed internal prefix or
descriptor invariant exits 1 before any caller work-directory mutation.

There is no pre-validation side effect. After successful public-option
validation, a Bash job-control process group gives private copy and pre-exec
validation 20 seconds and reserves 5 seconds for TERM, group KILL, and wait;
the watchdog terminates `run.sh` with exit 1 at the 25-second terminal deadline
if the group has not closed. The
wrapper arms exact-path cleanup immediately after exclusive directory creation.
It disarms that cleanup only after unlink and directory removal, then disarms
and waits for the watchdog immediately before `exec`, so no wrapper process can
later signal the observer. An exec failure closes the inherited descriptor on
shell exit and leaves no pathname. Build stdout/stderr are inherited rather than
written under the evidence root. Together with the observer's 900-second global
bound, one entry invocation remains below the repository's 30-minute hard test
budget. The standalone Cargo target remains governed by the existing
Cargo/build gates, but no Cargo process is a child of the evidence-producing
entry point. S0A's 4-GiB evidence mount promise begins with benchmark validation
and covers every arm-produced path. `run.sh` is the only supported entry.

The implementation is expected to exceed roughly 1,000 hand-written lines
because the bounded unsafe lifecycle for every child phase, its injectable
owner, sealed inputs, the canonical record codec, and the real-child owner must
close together. A
separate record-only producer would have no stable consumer, while a
lifecycle-only slice would duplicate the same state/precedence proof when rows
landed. Keeping this one standalone capability boundary therefore removes
duplicated proof and has lower integration risk than splitting at an arbitrary
line count; fixture sources and golden data remain small.

## 2. Public-contract ledger

“Public” below means the repository-local benchmark interface and persisted
evidence, not an Align end-user surface.

| Surface | Exact S0A contract | Owner and acceptance |
|---|---|---|
| Entry | `bench/startup/run.sh --observer ABS --observer-sha256 SHA256 --work-dir ABS --provenance ABS` | The script accepts exactly the four named options once each, in any order. There are no defaults, environment fallbacks, short aliases, positional arguments, or additional modes. Unknown, repeated, missing, relative, empty, or malformed values exit 2 before filesystem mutation or child creation. Observer identity failure after private descriptor adoption exits 1. The wrapper-to-observer exec environment is empty and checked at observer entry. |
| Work directory | Existing, empty, absolute root of one caller-exclusive mounted filesystem outside the repository; raw Unix path bytes are accepted except embedded NUL | The observer nofollow-resolves the repository, mount record, and work root. The mounted filesystem must have total capacity at most 4,294,967,296 bytes and at least 1,073,741,824 bytes available at admission. It exclusively creates `build/`, `cache/`, `logs/`, `observer/`, `provenance.tsv`, and `results.tsv`, never deletes or unmounts the caller-owned root, and rejects a nonroot, nonempty, shared, oversized, undersupplied, or repository-contained mount before any benchmark tool or timed child. |
| Work filesystem | Producer-owned `WORK_FILESYSTEM_HEX` from §4 binds the admitted mount, filesystem type/identity, block geometry, total capacity, and initial availability | Every compiler-generated source, temporary, object, log, and executable path is below this mount. Its total capacity is the hard aggregate filesystem-growth ceiling even for a broken arm that fills its cwd or `TMPDIR`; every accepted executable is additionally at most 67,108,864 bytes. |
| Provenance input | Canonical `align-startup-provenance-v1` record in §3 | The complete file is read with a 64 KiB bound; every machine-bound host/libc/tool field is reproduced or rehashed before admission, while source/power/load annotations retain their explicit declared status. The validated bytes are copied only after validation, and their SHA-256 binds every run/artifact/sample row. |
| Arms | Required `baseline`; optional `candidate`; no other arm | One-arm mode measures baseline. Two-arm mode measures both with the fixed counterbalanced schedule in §7. Candidate fields are all absent or all present. |
| Build | Each fixture is built once per arm with that arm's private compiler/runtime copy, the fixed child-only 64-MiB file limit, an initially empty per-artifact `ALIGNC_CACHE=<work>/cache/<fixture>/<arm>`, configured immutable `PATH`, `TMPDIR=<work>/build/tmp`, `LANG=C`, `LC_ALL=C`, `TZ=UTC`, `ALIGNC_LINKER=system`, `--profile release`, `--target-cpu baseline`, `--rt-lto`; output remains unstripped | The private cache forces production, excludes every packaged/ambient cache, and yields the compiler-produced codegen manifest that binds the artifact to the mapped LLVM build. Exact launcher/argv/environment/stdio owners plus fake-compiler file-limit, cache-miss, manifest, and hidden-ambient tests own the boundary. No source-building, PGO, probe feature, packaged-cache lookup, or cache warming occurs. |
| Timed child | Absolute executable and fixed argv from §5; cwd `<work>/observer/cwd`; environment exactly `LANG=C`, `LC_ALL=C`, `TZ=UTC` | The parent supplies no `PATH`, HOME, cache, linker, allocator, worker-count, loader override, or measurement toggle. The cwd exists and is empty before measurement. |
| Inherited execution state | Producer-owned `EXECUTION_STATE_HEX` from §4 binds available CPUs/affinity, nice/scheduler/QoS, common resource limits, Linux NUMA policy, and Linux cgroup/cpuset controls | The observer accepts only the default inherited Linux NUMA policy, snapshots the complete record before its first child, and requires byte identity around every infrastructure child and before every timed start. It neither silently normalizes nor omits inherited scheduling/resource inputs. |
| Metrics | `wall_ns`, `user_ns`, `system_ns`, `minor_faults`, `major_faults`, `voluntary_context_switches`, `involuntary_context_switches`, `peak_rss_bytes`, each unsigned 64-bit or unavailable | §6 fixes producers, units, overflow, and availability. No metric is inferred from another or encoded as zero when unavailable. |
| Child result | Fixed exit/stdout/stderr byte oracle per fixture | Every sample retains status and bounded stream results. A failed warmup or measured sample fails that fixture and the complete run; it is never removed from a successful distribution. |
| Schedule | Two warmups and twenty measured samples per arm/fixture; one-arm serial or two-arm AB/BA blocks | §7 fixes every ordinal, position, median, fixture order, and failure truncation. There is no sample-count override. |
| Deadlines | Timed-child execution allowance 5,000,000,000 monotonic nanoseconds plus 1,000,000,000 cleanup nanoseconds; fixed bounded tool/build/inspection/loader phases and a 900,000,000,000-nanosecond observer-global deadline | §7–§8 fix every phase cutoff, capture cap, process-group/direct-pid signal order, reap, hard-abort boundary, and error precedence. There is no timeout override. |
| Capture | One pipe per stream, first 4,096 queued bytes retained per stream, fixed post-reap queued snapshots, overflow typed | Valid fixtures emit at most three bytes. A broken child that fills a pipe blocks and reaches the bounded timeout rather than growing a file. Captured storage is parent-owned and does not enter child RSS. |
| Persisted result | Canonical ASCII TSV `align-startup-result-v1` in §4 | Rows flush after completion. Missing terminal `end` is an incomplete failed record. No JSON, database, network upload, hidden sidecar, or automatic consumer is added. |
| Artifact companions | Actual executable digest and byte size; complete validated codegen-key bytes plus full/slot digests; ordered gated-request list; final dynamic dependencies and section report from the configured LLVM object inspector; producer-resolved loader/dependency closure; fixture/compiler/runtime digests; compiler-produced mapped-LLVM build id | Inspection and loader resolution are untimed and run on the exact executable later measured. The manifest/key fields and filenames are mutually authenticated; the report hashes the raw inspector output; `LOADER_CLOSURE_HEX` binds the actual Linux interpreter/resolved images or macOS dyld shared cache/libSystem image. Each compiler and executable image is independently validated as the architecture named by the target triple before timing. Mapped-image inspection is not applicable because S0A adds no mapped artifact. |
| Ownership/lifetime | The wrapper owns one exclusive private executable directory until pre-exec unlink/removal and transfers fixed descriptor 9 through `/dev/fd/9`; the script owns files below the caller root for the invocation; the observer owns that loaded-image descriptor plus one active direct child, process group, watchdog generation, pipe ends, captures, cache-manifest buffer/LLVM identity, and `wait4` result until its artifact or sample row flushes | No handle or borrowed bytes escape. The caller owns final evidence and later cleanup. Drop/exit cannot claim success before direct-child reap. |
| Allocation | Parent observer: at most ten 1 MiB fixture-input buffers during sealing, bounded phase captures, one at-most-64-MiB cache-manifest buffer during each serial artifact qualification, one fixed 64-KiB CAS/dependency hashing buffer, one at-most-1-MiB decoded/2-MiB encoded loader-closure record per artifact, at most two 1,024-entry macOS mount snapshots, two 4,096-byte timed-child buffers, fixed argv/environment storage, and at most 44 raw samples per fixture in two-arm mode; child allocation is solely the fixture/runtime behavior; the caller mount caps all work-output storage at 4 GiB | Formation, manifest, mount, and transient loader-resolution buffers are dropped before timing. Allocation is outside the timed child accounting except the existing child program itself. Output beyond a stream, manifest, loader-closure, executable, or mount-table cap terminates that phase without proportional allocation; arbitrary additional work-root files cannot exceed the admitted filesystem capacity. |
| Errors | CLI and deterministic input-contract rejection use stderr plus exit 2; observer identity, post-validation I/O, fixture build, inspection, loader-resolution, sample, and report failures use stderr plus exit 1; per-sample outcomes are §8 enums | Exact record/golden tests own machine output. Human OS error text is hex data only and never changes outcome precedence. |
| Cache/artifact identity | Every per-artifact cache begins empty and is retained only as producer evidence; a hit or a missing/malformed version-5 codegen manifest, mismatched full/slot filename, or mismatched CAS object rejects the artifact. SHA-256 is structural for provenance and fixture bundle, content identity for files, and never type/interface identity; `CODEGEN_KEY_HEX` preserves every decoded key input, while `CODEGEN_FULL_DIGEST`, `CODEGEN_SLOT_DIGEST`, and `LLVM_BUILD_ID` use existing nominal/cache identities | The complete reachable fixture graph is the ten source files; they import no package source. No report/source span enters compiler or executable identity. S0A consumes the current internal cache codec but adds or widens no cache contract. |
| Platform | Linux x86-64/aarch64 with unified cgroup v2 and a spawn close-from file action, and macOS aarch64 with spawn close-on-exec-default, `_dyld_get_shared_cache_uuid`, and `_dyld_shared_cache_contains_path`; both require executable `/dev/fd/3`, `/dev/fd/4`, and `/dev/fd/9`, `posix_spawn`, `waitid(..., WNOWAIT)`, `wait4`, process groups, and the named LLVM inspector | Missing compile-time/runtime capabilities, insufficient inherited `RLIMIT_FSIZE`, legacy/hybrid Linux cgroups, and other platforms reject before builds. A fixed no-descendant self-probe validates pgroup/descriptor/launcher behavior before any arm. macOS is development evidence; only a separately qualified Linux host may support a cold-cache claim. S0A performs no cache eviction and reports no cold-cache result. |
| Performance promise | None | CI and owner tests check structure, lifecycle, oracles, and codecs only. A later optimization supplies its own preregistered threshold and qualified host record. |
| Prerequisite | V0 artifact/host qualification and this reviewed ledger | Implementation may start only after independent design review closes every P0-P3 finding. S0B/C0/O0 are not prerequisites and are not bundled. |

No syntax, type, signature, ownership, allocation, error, FFI, native text/view,
runtime-global state, package surface, compiler option, or distribution promise
changes. `draft.md`, `docs/language-spec.md`, `docs/open-questions.md`, library
designs, and Japanese mirrors therefore do not change.

## 3. Provenance input

The provenance file is ASCII, LF-terminated, has no BOM, and is at most 64 KiB.
Every line is tab-separated. Decimal integers have no sign or leading zero
except the single byte `0`. Digests are exactly 64 lowercase hexadecimal bytes.
`HEX` is lowercase hexadecimal with exactly two digits per raw byte; it may be
empty only where stated. Decoded paths are raw Unix `OsStr` bytes, must be
absolute, and must not contain NUL. A path-list value uses an ordered sequence
of `u32` big-endian byte length followed by that many path bytes, then encodes
the concatenation as lowercase hex. It must contain at least one absolute,
canonical directory; an empty element, NUL, colon, non-directory, duplicate,
or length overflow is invalid. The build `PATH` joins those decoded elements
with literal colon bytes. Other decoded fields are evidence labels and may
contain arbitrary non-NUL bytes only where the field-specific rules below say
they are operator-supplied; they are never executed.

Rows occur exactly in this order:

```text
align-startup-provenance-v1
fixture_revision<TAB>LOWERCASE_40_HEX
host_os_hex<TAB>HEX
host_kernel_hex<TAB>HEX
host_arch_hex<TAB>HEX
cpu_identity_hex<TAB>HEX
logical_cpu_count<TAB>U32_NONZERO
memory_bytes<TAB>U64_NONZERO
power_condition_hex<TAB>HEX
load_condition_hex<TAB>HEX
target_triple_hex<TAB>HEX
target_cpu<TAB>baseline
profile<TAB>release
linker<TAB>system
libc_identity_hex<TAB>HEX
libc_link<TAB>dynamic
strip_policy<TAB>unstripped
runtime_lto<TAB>on
build_path_list_hex<TAB>NONEMPTY_LIST_HEX
cc_path_hex<TAB>NONEMPTY_HEX
cc_sha256<TAB>SHA256
cc_version_hex<TAB>HEX
ld_path_hex<TAB>NONEMPTY_HEX
ld_sha256<TAB>SHA256
ld_version_hex<TAB>HEX
object_inspector_path_hex<TAB>NONEMPTY_HEX
object_inspector_sha256<TAB>SHA256
object_inspector_version_hex<TAB>HEX
arm_count<TAB>1_OR_2
arm<TAB>baseline<TAB>SOURCE_REVISION<TAB>COMPILER_PATH_HEX<TAB>COMPILER_SHA256<TAB>RUNTIME_PATH_HEX<TAB>RUNTIME_SHA256<TAB>COMPILER_VERSION_HEX
[arm<TAB>candidate<TAB>... exactly when arm_count is 2]
```

The header is literal. `SOURCE_REVISION` is `LOWERCASE_40_HEX`.
`fixture_revision` and both arm source revisions are declared commit identities,
not structural fingerprints; the operator supplies them, while actual file and
bundle digests remain the machine-checked identities. The file digest is
content identity.

The machine-bound host fields are produced and must equal a fresh observation
before any child or work-root mutation. `host_os_hex` is the literal bytes
`linux` or `macos`. `host_arch_hex` is normalized to `x86_64` or `aarch64` and
must agree with both the compiled observer target and `uname.machine` (`arm64`
normalizes to `aarch64`). `host_kernel_hex` is the ordered big-endian-u32-
length-prefixed byte list of `uname.sysname`, `uname.release`, `uname.version`,
and `uname.machine`. `logical_cpu_count` is the checked
`_SC_NPROCESSORS_ONLN` result. `memory_bytes` is checked
`_SC_PHYS_PAGES * _SC_PAGESIZE` on Linux and `hw.memsize` from `sysctlbyname`
on macOS. `target_triple_hex` is exactly `x86_64-pc-linux-gnu`,
`aarch64-unknown-linux-gnu`, or `aarch64-apple-darwin` for the admitted platform
set.

`cpu_identity_hex` is the lowercase-hex encoding of a canonical ASCII record
whose first row is `align-startup-cpu-v1` and second row is
`platform_arch<TAB>linux-x86_64|linux-aarch64|macos-aarch64`. Linux x86-64 then
has rows `cpuid<TAB>LEAF_HEX<TAB>SUBLEAF_HEX<TAB>REGISTERS`, where leaf/subleaf
are eight lowercase hex digits and REGISTERS is the 32 lowercase hex digits for
EAX, EBX, ECX, and EDX in that order. Rows are fixed for leaves `0`, `1`,
`7:0`, `0x80000000`, `0x80000001`, and `0x80000002..0x80000004`; a leaf above
the maximum reported by leaf `0` or `0x80000000` uses the literal `unavailable`
instead of REGISTERS. `U64_HEX` below is exactly 16 lowercase hexadecimal
digits. Linux aarch64 instead records
`auxv_hwcap<TAB>U64_HEX`, `auxv_hwcap2<TAB>U64_HEX`,
`online_cpus<TAB>CPU_LIST`, using §4's canonical CPU-list grammar, then
`midr<TAB>CPU<TAB>U64_HEX` for every online CPU.
The online list is the retained nofollow parse of
`/sys/devices/system/cpu/online` and its cardinality must equal
`_SC_NPROCESSORS_ONLN`. Each MIDR comes from the retained nofollow hexadecimal
`midr_el1` value at
`/sys/devices/system/cpu/cpuN/regs/identification/midr_el1`; a missing online
CPU identity rejects the host. macOS aarch64 instead records
`hw_model_hex<TAB>NONEMPTY_HEX`, `hw_cputype<TAB>I32`,
`hw_cpusubtype<TAB>I32`, and `hw_cpufamily<TAB>U32` from those exact
`sysctlbyname` producers. Every record is LF-terminated and at most 64 KiB;
row order, exact/rejected-next bounds, and unknown-row rejection are fixed, and
semantic-to-byte and byte-to-semantic goldens cover all three host variants.

`libc_identity_hex` is likewise produced, not trusted as a label. It encodes an
LF-terminated ASCII record with rows `align-startup-libc-v1`,
`platform<TAB>linux_OR_macos`, `version_hex<TAB>HEX`,
`image_path_hex<TAB>NONEMPTY_HEX`,
`image_sha256<TAB>SHA256_OR_unavailable:macos`, and
`image_uuid<TAB>LOWERCASE_32_HEX_OR_unavailable:linux`. Linux requires glibc;
`gnu_get_libc_version` supplies version bytes, and `dladdr` on that function
selects the loader-owned image under the immutable-system-path rule. macOS uses
literal version bytes `libSystem`, walks the bounded `_dyld_image_count` table,
and selects exactly one native in-memory image whose bounded `LC_ID_DYLIB`
install name is `/usr/lib/libSystem.B.dylib`; it records
`_dyld_get_image_name` and reads `LC_UUID` directly from that dyld-owned Mach-O
header. Zero/multiple matches, malformed load commands, or an all-zero UUID
reject the host. Its file hash is the literal unavailable value because a
shared-cache image need not have a standalone file. The record is bounded at 64 KiB and
binds the actually loaded libc image. `libc_link` remains the literal `dynamic`. A host-field
mismatch rejects the provenance rather than preserving a stale or fabricated
label.

The result field `LLVM_BUILD_ID` is exactly 32 lowercase hexadecimal digits
rendering the existing `Hash128` as 16 digits of `lo` followed by 16 digits of
`hi`. It is not operator input. It is the
nominal identity of the mapped LLVM image that produced the arm's objects:
`Hash128::of(tag || raw-id)`, where tag 0 precedes the ELF GNU build-id note
bytes and tag 1 precedes the Mach-O `LC_UUID` bytes, exactly as specified by
[`21-build-perf-plan.md`](21-build-perf-plan.md). The first baseline artifact
establishes the run's value from that compiler invocation's freshly produced
cache manifest; every later baseline/candidate artifact must produce the same
value before any artifact can enter `results.tsv`.

The observer recomputes compiler/runtime, C compiler, system
linker, and inspector digests and validates every declared tool version before
creating any benchmark work output. Each
runtime must be the `libalign_runtime.a` adjacent to its compiler. The first
executable regular file named `cc` found through decoded
`build_path_list_hex`, after canonicalization, must be `cc_path_hex`. Exact
`[CC, "-print-prog-name=ld"]` must emit one LF-terminated path token with empty
stderr and exit 0. After removing its one final LF, an absolute token is
canonicalized directly, a slash-free token is resolved through the same path
list, and any other form is invalid; the result must be canonical
`ld_path_hex`. Exact `[CC, "--version"]` and
`[OBJECT_INSPECTOR, "--version"]` stdout must equal their provenance fields,
with empty stderr and exit 0. On Linux, exact `[LD, "--version"]` must have
stdout equal to `ld_version_hex`, empty stderr, and exit 0. On macOS, exact
`[LD, "-v"]` must have empty stdout, stderr equal to `ld_version_hex`, and exit
0. Every version field encodes its complete owning stream, including every
line and its final LF. The compiler/runtime digests, checked public compiler
version, and compiler-produced mapped-LLVM build id are the complete available
toolchain identity; S0A does not substitute a package or LLVM version label for
the loaded producer identity.

For each arm, exact `[COMPILER_FD, "--version"]` executes the already retained
digest-checked compiler descriptor through the common launcher before the work
root is mutated. It requires exactly the declared single LF-terminated stdout
line, empty stderr, and exit 0. Every validation probe in this paragraph uses
the one environment `ALIGNC_CACHE=off`, `ALIGNC_LINKER=system`, `LANG=C`,
`LC_ALL=C`, `PATH=BUILD_PATH`, `TZ=UTC` in that order, cwd equal to the
canonical repository root, and no other entry. The descriptor execution is
valid on macOS because the complete `--version` branch returns before
`current_exe`, runtime-archive, or cache discovery. A stale compiler version is
therefore a deterministic input rejection with exit 2 and no work-root side
effect.

The observer rejects effective uid 0. Every decoded build-`PATH` directory and
every canonical component and final file of the configured C compiler, linker,
inspector, and every Linux interpreter or loader-selected dependency file must
be owned by another uid and
deny write access to the observer's effective credentials; retained nofollow
descriptors, native ELF/Mach-O image admission, and repeated
device/inode/mode/owner checks prove the same immutable chain before and after
each use. Direct validation, inspection, and Linux loader resolution execute the retained tool descriptor
through its verified `/dev/fd/N` path while retaining the configured canonical
tool path as argv[0]. Fixture builds retain the immutable path list because the
private Align compiler launches the C driver and that driver
launches the linker; the chain checks make those resolutions nonreplaceable by
the benchmark user. A privileged system update during the invocation is outside
the supported host contract and causes a later identity mismatch whenever it is
observable. Thus the digest-bound executable selected for each accepted tool
invocation cannot be swapped and restored by a concurrent unprivileged process.

Fixture validation opens each fixed repository source without following a
symlink, rejects a non-regular file or a file larger than 1 MiB, and retains the
exact bounded bytes used for its digest and bundle digest. After complete
validation, the observer exclusively writes those retained bytes to
`build/input/<fixture>.align`, reopens and rehashes every sealed copy, and drops
all source buffers before timing. Every arm builds only the sealed path. The
observer likewise exclusively copies each compiler and runtime into a
mode-0700 per-arm tool directory, fixes the runtime mode to 0600, rehashes both
copies, requires the compiler copy to be a native ELF/Mach-O image, and builds
with those private absolute paths. This closes fixture and
tool source-path replacement during the run. Before the first private compiler
launch, the observer rereads its executable header directly from the retained
descriptor. Linux accepts only 64-bit little-endian ELF `ET_EXEC` or
`ET_DYN` with `e_machine` 62 for `x86_64-pc-linux-gnu` or 183 for
`aarch64-unknown-linux-gnu`. macOS accepts only a thin little-endian 64-bit
Mach-O `MH_EXECUTE` with `cputype` `CPU_TYPE_ARM64` and raw `cpusubtype` 0
(`CPU_SUBTYPE_ARM64_ALL`, no capability bits) for
`aarch64-apple-darwin`; fat/universal and translated x86-64 images are rejected.
After each build the observer applies the same target-specific header check to
the exact retained output descriptor before hashing or inspection. A compiler
or output architecture mismatch therefore fails before any timed sample, even
when an OS translation facility could execute the mismatched compiler.

Each artifact build receives its own existing empty mode-0700 cache root under
`cache/<fixture>/<arm>` and no default/on cache mode. After successful group
closure, the observer relies on its retained root descriptor and recorded empty
enumeration immediately before spawn, then requires
the root to contain exactly one regular file below each of `actions/unit/`,
`index/unit/`, `actions/codegen/`, and `index/codegen/`, plus one regular CAS
object at `cas/HH/HASH`; the action/index pair in each namespace must be
byte-identical, every `HASH` is canonical 32-digit lowercase hex, `HH` is the
CAS hash's first two digits, and no other entry, symlink, special file, staging
file, or rejected marker is permitted. The empty exclusive root proves both
frontend and codegen were misses. It reads the codegen manifest with the
existing 64-MiB bound and decodes the complete current version-5 little-endian
codec: manifest version, full `CodegenKey`, and blob digest, rejecting unknown
versions/tags, invalid UTF-8, oversized sequence growth, truncation, or trailing
bytes. The complete decoded `CodegenKey` is validated, not merely parsed. Fixed
fields must be: cache-key version 5; frontend-schema version 9;
`located=false`; empty dependency-interface and export sequences for these
no-import whole programs; the provenance target triple; object format 0 on
Linux or 1 on macOS; resolved CPU `x86-64-v2` on x86-64 or `generic` on
aarch64; an empty resolved-feature string; profile `release`; pipeline
`default<O2>`; codegen optimization `default`; relocation model `PIC`; code
model `Default`; runtime LTO true with exactly one present digest; PGO tag
`Off`; and unit `main`. The decoded `compiler_build_id` must equal
`Hash128::of` over the exact private compiler bytes already retained for that
arm. `llvm_version` must be three canonical nonnegative decimal components
separated by dots, with no sign or leading zero except the single digit `0`,
and must match every artifact in both arms. The first decoded `llvm_build_id`
establishes `LLVM_BUILD_ID`;
its `lo` then `hi` little-endian words are rendered in the canonical result
form, and every later artifact in both arms must match it byte-for-byte. Each
arm's `rt_lto_digest` must also be identical across its ten artifacts; it is
producer-owned identity for bitcode embedded in that already hash-bound
compiler, not a digest inferred from the adjacent archive. The fixture-specific
`impl_hash` is producer-owned structural MIR identity and may differ between
fixtures or compiler arms.

`CODEGEN_KEY_HEX` is nonempty lowercase hex of the exact validated v5 full-key
wire bytes, beginning with cache-key version and ending with unit, excluding the
outer manifest version and blob digest. These fixed initial fixtures make it at
most 4 KiB. `CODEGEN_FULL_DIGEST` is the existing two-seed `Hash128` of those
bytes and renders as exactly 32 lowercase hexadecimal digits, `lo` then `hi`.
`CODEGEN_SLOT_DIGEST` has the same rendering and is `Hash128` of the exact slot
serialization: little-endian `u32` cache-key version, little-endian `lo`/`hi`
`compiler_build_id`, then little-endian `u32` byte length and UTF-8 bytes of
unit. The sole `actions/codegen/` filename must equal
`CODEGEN_FULL_DIGEST`; the sole `index/codegen/` filename must equal
`CODEGEN_SLOT_DIGEST`; and both files must contain the same decoded manifest.
Thus every decoded key component either has an independently checked expected
value or remains explicitly bound producer identity in the artifact row. Any
cache publication failure or mismatch rejects
the artifact even though ordinary compiler cache publication is best-effort.
The decoded blob digest must also name the sole CAS object: rendering its `lo`
then `hi` words as sixteen lowercase hexadecimal digits each must equal `HASH`,
and the first two rendered digits must equal `HH`. The observer opens that
object nofollow as the same retained regular descriptor used for metadata and
content, converts its nonnegative `st_size` to `u64`, requires it to be at most
67,108,864 bytes, and streams exactly that many bytes through two
`align_hash::WyHashStream` states
with seeds `0x9E3779B97F4A7C15` and `0xC2B2AE3D27D4EB4F`. It uses the fixed
64-KiB buffer, rejects short or excess input, reruns the descriptor
device/inode/kind/size check after EOF, and requires the finished `lo`/`hi`
digest to equal the manifest digest. This is the existing `Hash128` content
identity, not a new cache algorithm or collision-resistance promise.
The cache bytes are retained below the bounded work mount as evidence and are
never reused by another build. The configured inspector
remains an absolute digest-bound read-only tool. After the last build and
inspection and before the first timed sample, the observer rehashes the
configured C compiler, linker, and inspector and rejects any mismatch. No path
or tool version is inferred from ambient environment.

Artifact qualification has one failure order after a successful build phase:
retained output descriptor kind/identity/size, its fixed-size executable-header
parse and target match, private-cache tree topology, codegen-manifest fields in
wire order, complete key-field validation and action/slot-name derivation, CAS
name/content validation, first-or-equal LLVM identity, executable SHA-256,
inspector formation/execution/output parsing, then the platform
loader/dependency closure in §7. The first invalid item wins; later items are
not attempted. Thus a multi-invalid output/cache pair and every malformed
manifest combination have deterministic diagnostics and no timing side effect.

Decoded `power_condition_hex` and `load_condition_hex` are operator annotations
of at most 256 UTF-8 bytes with no ASCII control: either
`observed:NONEMPTY_TEXT` or `unavailable:NONEMPTY_REASON`. Absence and any other
prefix are invalid. They are bound annotations, not machine-produced host
identity or permission for a power/cold-cache claim; the harness does not invent
a power state from host APIs. Candidate fields have one topology: with
`arm_count=1` the candidate row is forbidden, and with `arm_count=2` it is
required. Extra rows, fields, tabs, CR, malformed hex, duplicates, unknown keys,
or trailing bytes are rejected.

The wrapper validates public option names, duplication, presence, absolute
paths, and SHA-256 grammar in argv order before private-copy setup; the first
bad option exits 2. No observer build can race or outrank that result because
the supported entry never invokes Cargo. After descriptor adoption, the
observer first validates its internal prefix, empty environment, descriptor
invariants, and expected observer digest. Provenance multi-invalid precedence
is then row order, then field order within the first bad row. Observer
validation completes in this order before work-directory mutation:

1. CLI shape and the two absolute argument paths;
2. input bound, BOM/CR/LF framing, header, row order, field counts;
3. integer, enum, digest, hex, decoded-NUL, and absolute-path validity;
4. arm topology and cross-field invariants;
5. canonical repository/work-mount resolution, outside-repository, exact
   mountpoint, empty-root, capacity, and stable filesystem-identity checks;
6. platform support and equality of every machine-produced host/CPU/libc field;
7. compiler/runtime/C-compiler/linker/inspector existence, regular-file kind,
   compiler target-header match, adjacency, canonical immutable-`PATH`
   resolution, and digest equality;
8. exact retained-descriptor tool-version equality; and
9. fixture inventory, kind/bound/content, retained-byte digests, and structural
   bundle digest formation.

No benchmark work output, artifact build/inspection/loader phase, or timed child
is touched before all applicable validation succeeds. The exact validation-only
compiler, C-compiler, linker, and inspector invocations in step 8 are the sole
child
processes during observer validation and create no work-root output. The
separate developer build of the explicitly supplied observer is outside the
supported entry and cannot affect its CLI/error precedence.

## 4. Persisted result grammar

`results.tsv` is canonical ASCII, LF-terminated, tab-separated, and contains no
quoting. The integer/digest/HEX rules in §3 apply. Enum tokens below are
lowercase ASCII. `-` is the sole unavailable numeric/flag token. Arbitrary
diagnostic bytes are lowercase HEX. A list is its ordered raw byte elements,
each preceded by a big-endian `u32` byte length, concatenated and encoded as
HEX. List length overflow is rejected before output.
Signed `I32` has one optional leading minus, no plus or leading zero, and no
negative zero.

All sample ordinals, indices, positions, arm/sample counts, exit values, stream
lengths, and completed-fixture counts are `u32`. Artifact byte size and every
metric are `u64`. The literals `1`, `2`, `20`, `5000000000`, `1000000000`, and
`900000000000` in `schema` and `run` are exact, not configurable integers. The
three deadline fields are respectively timed-child execution allowance,
timed-child cleanup reserve, and observer-global allowance. `WORK_DIR_HEX` is
the canonical work-root path's raw bytes. `CHILD_ARGV_LIST_HEX` contains the absolute
executable as argv[0] followed by the fixture arguments. `BUILD_ARGV_LIST_HEX`
contains the private compiler as argv[0] followed by its arguments. Environment
list elements are complete `KEY=VALUE` byte strings.

`WORK_FILESYSTEM_HEX` is the lowercase-hex encoding of this producer-owned
canonical ASCII record:

```text
align-startup-work-filesystem-v1
platform<TAB>linux_OR_macos
mount_id<TAB>U64_OR_unavailable:macos
mount_device_major<TAB>U32_OR_unavailable:macos
mount_device_minor<TAB>U32_OR_unavailable:macos
mount_root_hex<TAB>HEX_OR_unavailable:macos
fsid_word_0<TAB>I32
fsid_word_1<TAB>I32
filesystem_type_hex<TAB>NONEMPTY_HEX
mount_source_hex<TAB>NONEMPTY_HEX
mount_flags<TAB>U64
fragment_size<TAB>U64_NONZERO
block_count<TAB>U64_NONZERO
total_bytes<TAB>U64_NONZERO
available_blocks<TAB>U64
available_bytes<TAB>U64
```

On Linux, one at-most-1-MiB `/proc/self/mountinfo` parse accepts only canonical
decimal fields and the kernel escapes `\040`, `\011`, `\012`, and `\134`,
rejects any other backslash form, supplies the unique mount id, device
major/minor, and decoded mount-root/filesystem-type/source/mountpoint bytes, and
proves the canonical work root is the exact mount point. The selected
mount-root must be the single byte `/`, so a bind-mounted subtree cannot pose as
the complete capacity boundary; the device pair must equal `major(st_dev)` and
`minor(st_dev)` from the retained descriptor. `statvfs` supplies flags and
block geometry, and `fstatfs` supplies the two signed filesystem-id words. On
macOS, a null-buffer `getfsstat(..., MNT_NOWAIT)` count must be at most 1,024.
The observer allocates exactly that many entries, fills them with a second
call, repeats the count-and-fill sequence into a second independently bounded
snapshot, and requires both returned counts and every canonicalized entry byte
to agree. A count of 1,025, a fill count different from its preceding count,
truncation, or a changed second snapshot rejects the host. The stable snapshot
selects the one exact canonical mount point and supplies bounded NUL-terminated
type/source bytes, flags, and the two filesystem-id words; unterminated arrays
reject the host, mount id is the literal unavailable value, the three
Linux-only mount fields use their literal unavailable values, and `statvfs`
supplies block geometry. In both
cases a second descriptor observation must have the same device, inode,
filesystem id, type, source, flags, fragment size, block count, and available
block count.
Linux rejects every other row in the complete current mount namespace whose
device major/minor equals the selected row, regardless of that row's mount root
or mount point; macOS rejects every other mount-table entry with the same
filesystem id. This backing-filesystem comparison is the exact shared-alias
test. Checked
products require `total_bytes == fragment_size * block_count` and
`available_bytes == fragment_size * available_blocks`; total bytes must not
exceed 4,294,967,296, available bytes must be at least 1,073,741,824, and the
mount must permit execution. Any mismatch, overflow, shared mount-point alias,
or concurrent mount replacement fails before work-root mutation. The caller
must keep this otherwise-empty mount exclusive for the invocation; later
foreign mutation is outside the same explicit unsupported rule as work-root
mutation.

`EXECUTION_STATE_HEX` is a producer-owned, at-most-64-KiB canonical ASCII
record encoded as lowercase hex. `LIMIT` is canonical `u64` or
the literal `infinity`. `CPU_LIST` is a nonempty comma-separated sequence of
strictly increasing canonical `u32` CPU indices. Rows are exactly:

```text
align-startup-execution-state-v1
online_cpu_count<TAB>U32_NONZERO
available_parallelism<TAB>U32_NONZERO
affinity<TAB>CPU_LIST_OR_unavailable:macos
numa_policy<TAB>default_OR_unavailable:macos
numa_nodes<TAB>none_OR_unavailable:macos
nice<TAB>I32
scheduler_policy<TAB>I32_OR_unavailable:macos
scheduler_priority<TAB>I32_OR_unavailable:macos
qos_class<TAB>QOS_OR_unavailable:linux
qos_relative_priority<TAB>I32_OR_unavailable:linux
linux_cap_inheritable<TAB>U64_HEX_OR_unavailable:macos
linux_cap_permitted<TAB>U64_HEX_OR_unavailable:macos
linux_cap_effective<TAB>U64_HEX_OR_unavailable:macos
linux_cap_bounding<TAB>U64_HEX_OR_unavailable:macos
linux_cap_ambient<TAB>U64_HEX_OR_unavailable:macos
linux_no_new_privs<TAB>0_OR_1_OR_unavailable:macos
rlimit<TAB>cpu<TAB>LIMIT<TAB>LIMIT
rlimit<TAB>fsize<TAB>LIMIT<TAB>LIMIT
rlimit<TAB>data<TAB>LIMIT<TAB>LIMIT
rlimit<TAB>stack<TAB>LIMIT<TAB>LIMIT
rlimit<TAB>core<TAB>LIMIT<TAB>LIMIT
rlimit<TAB>rss<TAB>LIMIT<TAB>LIMIT
rlimit<TAB>memlock<TAB>LIMIT<TAB>LIMIT
rlimit<TAB>nproc<TAB>LIMIT<TAB>LIMIT
rlimit<TAB>nofile<TAB>LIMIT<TAB>LIMIT
rlimit<TAB>as<TAB>LIMIT<TAB>LIMIT
proc_cgroup<TAB>HEX_OR_unavailable:macos
cpus_allowed_list<TAB>HEX_OR_unavailable:macos
mems_allowed_list<TAB>HEX_OR_unavailable:macos
cgroup_constraints<TAB>LIST_HEX_OR_unavailable:macos
```

On Linux, `online_cpu_count` comes from `_SC_NPROCESSORS_ONLN`,
`available_parallelism` from `std::thread::available_parallelism`, and
`affinity` from `sched_getaffinity(0)`. Scheduler fields come from `sched_getscheduler(0)` and
`sched_getparam(0)`, QoS fields are the literal unavailable value, and the
three `/proc` fields are bounded exact reads of `/proc/self/cgroup` plus the
trimmed `Cpus_allowed_list` and `Mems_allowed_list` values from
`/proc/self/status`; that status parse also supplies the five exact 16-digit
capability masks and `NoNewPrivs`. The four inheritable/permitted/effective/
ambient masks must be zero, so an unprivileged process cannot remount a
validated immutable tool chain; the bounding mask and no-new-privileges bit are
recorded. Those two proc files are individually bounded at 64 KiB.
Before those reads, `get_mempolicy` with no address and no policy-changing flag
must report `MPOL_DEFAULT` with an empty nodemask. S0A records the literal rows
`numa_policy<TAB>default` and `numa_nodes<TAB>none`; any explicit preferred,
bind, interleave, local, weighted-interleave, or unknown policy, any nonempty
nodemask, and an unavailable syscall reject the host before a child or work-root
mutation. The harness does not normalize or restore an inherited NUMA policy.
Linux requires `/sys/fs/cgroup` to be the nofollow cgroup-v2 mount and
`/proc/self/cgroup` to contain exactly one LF-terminated `0::PATH` row and no
other bytes. `PATH` is `/` or begins with `/` and has only nonempty components;
`.`, `..`, tab, CR, LF, NUL, and a trailing slash are forbidden. The observer
opens the mount and descends each PATH component from that retained descriptor
with nofollow directory opens and identity checks. `cgroup_constraints` is a
length-prefixed list for every directory from the current group through the
mount root, leaf first. For each directory the fixed constraint-key order is
`cgroup.type`, `cgroup.controllers`, `cgroup.subtree_control`,
`cgroup.max.depth`, `cgroup.max.descendants`, `cgroup.freeze`, `cpu.idle`,
`cpu.max`, `cpu.max.burst`, `cpu.weight`, `cpu.weight.nice`,
`cpu.uclamp.min`, `cpu.uclamp.max`, `cpuset.cpus`,
`cpuset.cpus.effective`, `cpuset.cpus.exclusive`,
`cpuset.cpus.exclusive.effective`, `cpuset.cpus.isolated`,
`cpuset.cpus.partition`, `cpuset.mems`, `cpuset.mems.effective`, `io.latency`,
`io.max`, `io.weight`, `io.prio.class`, `io.cost.model`, `io.cost.qos`, `memory.min`,
`memory.low`, `memory.high`, `memory.max`, `memory.oom.group`,
`memory.swap.high`, `memory.swap.max`, `memory.zswap.max`,
`memory.zswap.writeback`, `pids.max`, `rdma.max`, `misc.max`, `dmem.min`,
`dmem.low`, and `dmem.max`, followed by every present
`hugetlb.SIZE.max`/`hugetlb.SIZE.rsvd.max` pair in raw-byte SIZE order.
`RELATIVE_PATH_HEX` is the PATH without its leading slash, encoded as lowercase
hex; it is empty for the mount root. Each list element is ASCII
`RELATIVE_PATH_HEX<TAB>KEY<TAB>absent|CONTENT_HEX`; a present file must be a
nofollow regular kernel control with at most 4 KiB of exact bytes, either empty
or LF-terminated and containing no NUL or CR.

Every directory entry in the `cgroup.`, `cpu.`, `cpuset.`, `io.`, `memory.`,
`pids.`, `rdma.`, `hugetlb.`, `misc.`, or `dmem.` namespaces must be either one
of those recorded constraints or one of the fixed volatile/command families:
`*.current`, `*.events`, `*.events.local`, `*.peak`, `*.pressure`, `*.stat`,
`*.stat.local`, `*.numa_stat`, `cgroup.procs`, `cgroup.threads`, `cgroup.kill`,
or `memory.reclaim`. An unknown entry or a malformed hugepage SIZE rejects the
host rather than silently omitting a future constraint. Thus membership and
every admitted current/ancestor CPU, cpuset, I/O, memory, hugepage, and process
constraint are recorded; legacy or hybrid cgroups are rejected rather than
partly described.

On macOS, `online_cpu_count` has the same `_SC_NPROCESSORS_ONLN` producer and
`available_parallelism` uses the same Rust producer. Affinity/scheduler and
cgroup/Linux-capability/NUMA fields use their literal unavailable values, and QoS comes from
`pthread_get_qos_class_np` as one of `user-interactive`, `user-initiated`,
`default`, `utility`, `background`, or `unspecified` plus its relative
priority. Both platforms obtain `nice` from `getpriority(PRIO_PROCESS, 0)` and
the ten ordered soft/hard pairs from `getrlimit`. `online_cpu_count` must equal
provenance `logical_cpu_count`; on Linux the normalized `Cpus_allowed_list`
value must describe the same set as `affinity`.

The observer forms this snapshot twice consecutively before its first child and
accepts it only when both complete canonical byte records are identical. It
then compares one fresh complete record before and after every infrastructure
child and before every timed start. A mismatch or any unavailable producer
before `results.tsv` exists fails the run; the corresponding failure at a timed
start emits `formation-error` with operation `execution-state`. Concurrent
external mutation during a child remains unsupported, but no two accepted
phases can silently use different inherited state.

`LOADER_CLOSURE_HEX` is lowercase hex of one LF-terminated canonical ASCII
record of at most 1 MiB. Linux uses:

```text
align-startup-loader-closure-v1
platform<TAB>linux
loader_path_hex<TAB>NONEMPTY_HEX
loader_sha256<TAB>SHA256
vdso_name_hex<TAB>NONEMPTY_HEX
vdso_build_id_hex<TAB>NONEMPTY_HEX
entry_count<TAB>U32_NONZERO
entry<TAB>ORDINAL<TAB>SONAME_HEX<TAB>CANONICAL_PATH_HEX<TAB>SHA256
[entry rows through entry_count - 1]
```

The loader path is the artifact's sole `PT_INTERP`; its SHA-256 comes from the
retained immutable regular descriptor actually executed for resolution.
`vdso_name_hex` is the loader-emitted vDSO name. `vdso_build_id_hex` is the
nonempty GNU build-id note read by a bounded in-memory 64-bit native-endian ELF
parse starting at the observer's `getauxval(AT_SYSINFO_EHDR)` result; malformed,
wrong-architecture, duplicate, or absent notes reject the platform. Entry rows
preserve loader output order after removing the separately recorded vDSO and
loader-self rows. Each file entry records its emitted SONAME, nofollow
canonical absolute path, and SHA-256 of the retained other-owned,
observer-nonwritable regular native-ELF descriptor. Every direct SONAME in
`FINAL_DEPENDENCIES_LIST_HEX` must occur exactly once; transitive rows remain
visible rather than being discarded.

macOS uses the same header followed by:

```text
platform<TAB>macos
loader_path_hex<TAB>2f7573722f6c69622f64796c64
loader_shared_cache_uuid<TAB>LOWERCASE_32_HEX
vdso_name_hex<TAB>unavailable:macos
vdso_build_id_hex<TAB>unavailable:macos
entry_count<TAB>1
entry<TAB>0<TAB>2f7573722f6c69622f6c696253797374656d2e422e64796c6962<TAB>NONEMPTY_HEX<TAB>LOWERCASE_32_HEX
```

The retained artifact's bounded Mach-O load-command parse must contain exactly
one `LC_LOAD_DYLINKER` naming `/usr/lib/dyld`; the initial fixture set must have
exactly one final dependency, `/usr/lib/libSystem.B.dylib`. The producer calls
`_dyld_get_shared_cache_uuid`, rejects the all-zero UUID, requires
`_dyld_shared_cache_contains_path` for that dependency, and copies the path and
`LC_UUID` from the already loaded libSystem image selected for
`libc_identity_hex`. Its entry path/UUID must equal that libc record. Thus no
standalone shared-cache file is invented. On both platforms, after all
artifacts qualify, the observer reproduces every complete closure in artifact
order immediately before any timed sample and requires byte identity; a
mismatch leaves no timing row.

The complete row grammar is:

```text
schema<TAB>1
run<TAB>PROVENANCE_SHA256<TAB>FIXTURE_BUNDLE_SHA256<TAB>OBSERVER_SHA256<TAB>ARM_COUNT<TAB>2<TAB>20<TAB>5000000000<TAB>1000000000<TAB>900000000000<TAB>WORK_FILESYSTEM_HEX<TAB>EXECUTION_STATE_HEX<TAB>SCHEDULE<TAB>CHILD_ENV_LIST_HEX<TAB>WORK_DIR_HEX
artifact<TAB>FIXTURE<TAB>ARM<TAB>FIXTURE_SHA256<TAB>COMPILER_SHA256<TAB>RUNTIME_SHA256<TAB>LLVM_BUILD_ID<TAB>CODEGEN_KEY_HEX<TAB>CODEGEN_FULL_DIGEST<TAB>CODEGEN_SLOT_DIGEST<TAB>EXECUTABLE_SHA256<TAB>SIZE_BYTES<TAB>INSPECTION_SHA256<TAB>LOADER_CLOSURE_HEX<TAB>REQUESTED_GATED_LIBS_LIST_HEX<TAB>FINAL_DEPENDENCIES_LIST_HEX<TAB>BUILD_ARGV_LIST_HEX<TAB>BUILD_ENV_LIST_HEX<TAB>CHILD_ARGV_LIST_HEX
sample<TAB>FIXTURE<TAB>PHASE<TAB>SEQUENCE_ORDINAL<TAB>ARM<TAB>ARM_SAMPLE_INDEX<TAB>POSITION<TAB>OUTCOME<TAB>WALL_NS<TAB>USER_NS<TAB>SYSTEM_NS<TAB>MINOR_FAULTS<TAB>MAJOR_FAULTS<TAB>VOLUNTARY_SWITCHES<TAB>INVOLUNTARY_SWITCHES<TAB>PEAK_RSS_BYTES<TAB>EXIT_KIND<TAB>EXIT_VALUE<TAB>STDOUT_MATCH<TAB>STDERR_MATCH<TAB>STDOUT_LEN<TAB>STDERR_LEN<TAB>DETAIL_HEX
summary<TAB>FIXTURE<TAB>ARM<TAB>AVAILABILITY<TAB>SUCCESSFUL_MEASURED_SAMPLES<TAB>WALL_MEDIAN_NS<TAB>USER_MEDIAN_NS<TAB>SYSTEM_MEDIAN_NS<TAB>MINOR_FAULTS_MEDIAN<TAB>MAJOR_FAULTS_MEDIAN<TAB>VOLUNTARY_SWITCHES_MEDIAN<TAB>INVOLUNTARY_SWITCHES_MEDIAN<TAB>PEAK_RSS_MEDIAN_BYTES<TAB>REASON
end<TAB>RUN_OUTCOME<TAB>COMPLETED_FIXTURE_COUNT<TAB>FAILED_FIXTURE_LIST_HEX
```

There is one `schema`, one `run`, every `artifact`, every reached `sample`, one
`summary` per fixture/arm, and one `end`, in that order. Artifact order is §5
fixture order, baseline then candidate. Samples and summaries use that fixture
order. The `run` schedule is `single` for one arm and `abba` for two.
`CHILD_ENV_LIST_HEX` is exactly `LANG=C`, `LC_ALL=C`, `TZ=UTC` in that order.
Each artifact's build environment is exactly
`ALIGNC_CACHE=WORK_DIR/cache/FIXTURE/ARM`,
`ALIGNC_LINKER=system`, `LANG=C`, `LC_ALL=C`, `PATH=BUILD_PATH`,
`TMPDIR=WORK_DIR/build/tmp`, and `TZ=UTC` in that order. Neither list inherits
another variable.

`PHASE` is `warmup` or `measure`. `OUTCOME` is `ok`, `formation-error`,
`spawn-error`, `cleanup-error`, `timeout`, `signal`, `exit`, `output-overflow`,
`metric-error`, `stdout-mismatch`, or `stderr-mismatch`. `EXIT_KIND` is `code`,
`signal`, or `none`; `EXIT_VALUE` is its nonnegative `u32` value or `-` with
`none`.
Match flags are `yes`, `no`, or `-`. A spawn error has child metrics, exit,
match, and stream lengths unavailable; `wall_ns` remains present when both
clock reads succeeded. A reaped post-spawn result retains every valid observed
field even when its outcome is not `ok`. The first 4,096 bytes of either
mismatching/overflowed stream, and every nonempty prefix obtained before a
stream read failure, are retained under `logs/`. `DETAIL_HEX` uses
the ordered length-prefixed list encoding with an explicit role prefix inside
every element: `operation<TAB>TOKEN` first, optional
`os-error<TAB>RAW_BYTES` second, then any
`stdout-path<TAB>RELATIVE_PATH` and `stderr-path<TAB>RELATIVE_PATH` elements in
that order. The first tab separates the fixed ASCII role from arbitrary bytes;
unknown, repeated, empty, missing, or reordered roles are invalid. The success
value is the empty-list encoding.
Stable operation tokens are the outcome name or the failing lifecycle operation
(`execution-state`, `cwd-check`, `clock-start`, `pipe-stdout`, `pipe-stderr`,
`devnull`,
`spawn-actions`, `spawn-attributes`, `spawn`, `group-check`, `group-kill`,
`direct-kill`, `waitid`, `group-absence`, `clock-end`, `fionread-stdout`, `fionread-stderr`,
`read-stdout`, `read-stderr`, `persist-stdout`, `persist-stderr`,
`close-stdout`, or `close-stderr`).
When multiple lifecycle operations fail, the earliest failure in this listed
order owns the operation and optional OS-error elements; later failures can
preserve the `cleanup-error` outcome but do not reorder detail elements. A
premature EOF has no `os-error` role, so a retained path can never be decoded as
native error bytes.

After successful reap, capture field presence is the Cartesian product of the
two independent stream states below. The observer attempts both snapshots in
stdout/stderr order, then stdout/stderr reads, stdout/stderr persistence, and
stdout/stderr closes in exactly that order; one stream failure does not suppress
the other stream's attempts.

| Per-stream terminal state | `*_LEN` | `*_MATCH` | detail path |
|---|---|---|---|
| `FIONREAD` failed or returned a negative count | `-` | `-` | absent |
| snapshot succeeded but exact read ended early or failed | snapshot `u32` | `-` | present only when a nonempty retained prefix was written completely |
| exact read completed | snapshot `u32` | `yes` or `no` | present only for a nonempty mismatching/overflowed capture written completely |
| retained-log persistence failed | as established above | as established above | absent |
| pipe close failed | unchanged | unchanged | unchanged |

A persistence, query, read, premature-EOF, or close failure selects
`cleanup-error`; a successful path element names only a completely written
retained file. A zero-byte mismatch therefore has no path. The other stream's
length, match, and path follow its own row independently.

Pre-reap field presence is fixed separately. A formation failure or its
resource-rollback cleanup failure before the first start-clock read has every
metric, exit, stream length, and match unavailable and no stream path; the
latter changes only `OUTCOME` to `cleanup-error` and records the earliest
failed close operation. If `posix_spawn` returns an error, the parent performs
stdout then stderr pipe close rollback in that order. A
rollback close failure similarly selects `cleanup-error`; `wall_ns` is present
exactly when both parent clock reads succeeded, while all seven child-produced
metrics, exit, stream lengths, matches, and paths remain unavailable. With no
rollback failure this same topology is `spawn-error`. Neither case attempts
capture. Once a child exists, no sample row may be emitted until one successful
direct-child reap: after that reap, even a later cleanup failure uses the full
metric/exit and two-stream Cartesian rules above. Failure to reap reaches the
hard-abort boundary and emits no row. Every emitted pre-reap failure retains the
reached sample ordinal and fails that fixture.

`AVAILABILITY` is `complete` or `failed`. It is `complete` only when all twenty
measured samples are `ok` and every metric is available. It is `failed` after
any failed warmup/measured sample. A failed summary reports every median as `-`;
it does not summarize the successful subset. `REASON` is respectively `none`
or `sample-failed`. `RUN_OUTCOME` is `ok` only when every summary is complete
and every fixture oracle succeeded; otherwise it is `failed`.

`SUCCESSFUL_MEASURED_SAMPLES` is 20 for complete summaries. For a
failed summary it is that arm's exact count of reached measured rows whose
outcome is `ok`, including zero and allowing one arm to be ahead by one within
the failed AB/BA block. A warmup never increments it.

`COMPLETED_FIXTURE_COUNT` counts fixtures whose scheduled samples all ran and
whose summaries are complete. `FAILED_FIXTURE_LIST_HEX` is the
ordered length-prefixed list of every other fixture identifier in §5 order; it
is the empty-list encoding on success. For a completed overflow snapshot, the
affected match is `no`, its length is present, and the other stream retains its
independently observed fields.

Rows are flushed after construction. A killed observer can leave a valid prefix
without `end`; every consumer must reject that as incomplete rather than infer
success. Flush means a complete `write_all` followed by a userspace writer
flush; S0A promises no `fsync`. A write or flush failure exits 1 and leaves an
incomplete record without a synthetic `end`. The first implementation includes
independent semantic-to-byte and byte-to-semantic golden vectors for all row
kinds, empty/nonempty byte lists, full/slot codegen-key derivations,
unavailable fields, both arm topologies, every outcome, maximum integers, both
platform loader-closure record topologies,
malformed/truncated rows, unknown enums, noncanonical decimals/hex, and missing,
repeated, reordered, or extra rows.

## 5. Fixture inventory and oracles

Fixture identifiers and construct order are fixed below. Every source is UTF-8
with LF endings. All import graphs are empty. The structural bundle fingerprint
is SHA-256 over, in this order, `u32-be identifier byte length`, identifier
bytes, `u64-be source byte length`, and source bytes for every fixture. It is
structural over the complete reachable source graph, not nominal by pathname.

| Ordinal | Identifier | Child argv after executable | Exit | stdout | stderr |
|---:|---|---|---:|---|---|
| 0 | `empty-i32` | none | 0 | empty | empty |
| 1 | `empty-result` | none | 0 | empty | empty |
| 2 | `argv` | `alpha`, UTF-8 `β` | 0 | empty | empty |
| 3 | `primitive-output` | none | 0 | `42\n` | empty |
| 4 | `arena-reset` | none | 0 | empty | empty |
| 5 | `par-map-below-floor` | none | 0 | empty | empty |
| 6 | `par-map-first-pool` | none | 0 | empty | empty |
| 7 | `par-map-reused-pool` | none | 0 | empty | empty |
| 8 | `task-group-single` | none | 0 | empty | empty |
| 9 | `readonly-first-touch` | `probe` | 0 | empty | empty |

The exact sources are the files under `bench/startup/fixtures/` in this order.
Their normative contents are:

```align
fn main() -> i32 = 0
```

```align
fn main() -> Result<(), Error> = Ok(())
```

```align
fn main(args: array<str>) -> Result<(), Error> {
  if args.len() == 3 && args[1] == "alpha" && args[2] == "β" {
    return Ok(())
  }
  return Err(error(73))
}
```

```align
fn main() -> i32 {
  print(42)
  return 0
}
```

```align
fn main() -> i32 {
  arena {
    xs := [1, 2, 3, 4].to_array()
    if xs.sum() == 10 {
      return 0
    }
    return 73
  }
}
```

The three parallel fixtures use the same builder and `add_one` body. The first
stops after 65,536 elements and validates indices 0 and 65,535. The second and
third stop after 65,537 elements and validate indices 0 and 65,536. The second
runs one `par_map`; the third feeds its first result to a second `par_map` and
validates 2 and 65,538. Their exact files are syntax-checked rather than
generated, so no unrecorded generator becomes source identity.

```align
fn add_one(x: i64) -> i64 = x + 1

fn main() -> i32 {
  mut builder: array_builder<i64> := array_builder()
  mut i := 0
  loop {
    builder.push(i)
    i = i + 1
    if i >= 65536 { break }
  }
  xs := builder.build()
  ys := xs.par_map(add_one)
  if ys[0] == 1 && ys[65535] == 65536 {
    return 0
  }
  return 73
}
```

```align
fn add_one(x: i64) -> i64 = x + 1

fn main() -> i32 {
  mut builder: array_builder<i64> := array_builder()
  mut i := 0
  loop {
    builder.push(i)
    i = i + 1
    if i >= 65537 { break }
  }
  xs := builder.build()
  ys := xs.par_map(add_one)
  if ys[0] == 1 && ys[65536] == 65537 {
    return 0
  }
  return 73
}
```

```align
fn add_one(x: i64) -> i64 = x + 1

fn main() -> i32 {
  mut builder: array_builder<i64> := array_builder()
  mut i := 0
  loop {
    builder.push(i)
    i = i + 1
    if i >= 65537 { break }
  }
  xs := builder.build()
  first := xs.par_map(add_one)
  second := first.par_map(add_one)
  if second[0] == 2 && second[65536] == 65538 {
    return 0
  }
  return 73
}
```

```align
fn main() -> i32 {
  task_group {
    task := spawn(fn { 21 + 21 })
    wait()
    if task.get() == 42 {
      return 0
    }
    return 73
  }
}
```

```align
TABLE := [3, 1, 4, 1, 5, 9, 2, 6]

fn main(args: array<str>) -> Result<(), Error> {
  if TABLE[args.len()] == 4 {
    return Ok(())
  }
  return Err(error(73))
}
```

All ten exact draft files were checked and executed successfully with the V0
v0.7.2 distribution before this ledger was submitted for review. The
implementation owner repeats syntax and oracle checks through the committed
fixtures; this author check is not acceptance evidence for uncommitted code.

The 65,536 row is the current caller-only boundary. The 65,537 row is the first
pool-eligible count on a host with more than one available worker. Host policy
remains authoritative: on a one-worker host the record is valid but its label
does not claim a pool initialized. The separate runtime structural owner, not a
probe in these binaries, checks pool initialization. The reused-pool row reports
the whole two-operation child and is not subtracted into a per-operation cost.

## 6. Metric semantics

The observer uses `clock_gettime(CLOCK_MONOTONIC)` before `posix_spawn` and
immediately after `wait4` returns the direct child. Seconds/nanoseconds are
validated and converted with checked unsigned arithmetic. `wall_ns` therefore
includes spawn, loader, relocation, CRT, Align entry wrapper, fixture work,
exit, terminal observation, and reap. It excludes fixture
build/inspection/loader resolution,
preopened pipe setup, post-reap stream draining/oracle comparison, row encoding,
and summary calculation.

When `posix_spawn` itself returns an error, the second clock read occurs
immediately after that return and `wall_ns` reports the failed launch interval.
A formation error occurs before the first clock read and has no wall value.

The same successful `wait4` returns one `rusage`:

- `ru_utime` and `ru_stime` become `user_ns` and `system_ns` after validating
  nonnegative seconds and microseconds in `0..1_000_000`;
- `ru_minflt`, `ru_majflt`, `ru_nvcsw`, and `ru_nivcsw` become the four named
  unsigned counters after nonnegative checked conversion; and
- `ru_maxrss` becomes bytes by checked multiplication by 1,024 on Linux and is
  already bytes on macOS.

These are direct-child process totals, including its threads. The fixed
fixtures spawn no descendant process; S0A makes no descendant-accounting claim.
A negative, malformed, or overflowing supplied value makes the affected field
unavailable and selects `metric-error`; it never wraps, saturates, or becomes
zero. All eight metrics are required on both supported platform families; a
platform ABI that lacks one rejects before builds. Failure to obtain the
terminal status/rusage record is a cleanup error, not mere metric
unavailability.

For each available summary field, sort the twenty unsigned sample values and
compute the median as `v[9] + (v[10] - v[9]) / 2`. This cannot overflow and
rounds a half unit down. Warmups never enter a summary. Raw measured rows are
the reported distribution; min/max, a minimum statistic, ratios, confidence
intervals, and inferred physical traffic are not reported by S0A.

## 7. Schedule and artifact formation

After the checked signal-disposition setup below, the main thread reads one
checked monotonic start and creates the persistent watchdog with a global
terminal deadline 900 seconds later. The
last five seconds are a global cleanup reserve: no child starts after the
895-second execution cutoff. Bounded parsing and hashing run on the main thread;
every child after observer descriptor adoption uses one common owned process-group,
deadline, capture, signal, and reap state machine. Phase deadlines are clamped
to the earlier global cutoffs.

| Child phase | Execution allowance | Cleanup reserve | stdout/stderr cap |
|---|---:|---:|---:|
| compiler/C compiler/linker/inspector validation probe | 4 s | 1 s | 64 KiB / 64 KiB |
| fixture build and producer-manifest publication | 55 s | 5 s | 1 MiB / 1 MiB |
| artifact inspection | 8 s | 2 s | 8 MiB / 64 KiB |
| Linux loader resolution | 8 s | 2 s | 1 MiB / 64 KiB |
| timed fixture | 5 s | 1 s | passive 4,096 B / passive 4,096 B |

Every infrastructure phase first spawns the same retained observer image as a
fixed exec launcher through descriptor 3, with the retained native ELF/Mach-O
tool image on descriptor 4 and a one-byte parent readiness pipe on descriptor
5. The launcher accepts internal argv
`[observer, "--exec-tool-fd", "4", "--argv0", TOOL_PATH, TOOL_ARGS...]` for
every Linux tool, every pre-mutation macOS compiler-version probe, and every
non-compiler macOS tool. A macOS private compiler build instead uses the sole alternate form
`[observer, "--exec-tool-path", PRIVATE_COMPILER, "--tool-fd", "4",
TOOL_ARGS...]`; it is accepted only when descriptor 4 and the nofollow canonical
private path still have the same device/inode/mode/owner, every ancestor is the
observer-owned mode-0700 work topology, and `PRIVATE_COMPILER` is the exact
arm path from §7. The launcher sets
child-only `RLIMIT_FSIZE` soft/hard to 67,108,864 bytes and `RLIMIT_CORE` to
zero, then blocks for exactly one byte `G`. Only after the guarded parent proves
`getpgid(pid) == pid` does it write that byte. Because the observer ignores
SIGPIPE, a closed readiness reader is the checked `EPIPE` write-error branch,
not process termination. Any EOF, other byte, extra byte,
read/write/close failure, or deadline fails while the fixed launcher still has
no descendant. The launcher then marks descriptors 3/4 close-on-exec, closes 5,
and immediately executes `/dev/fd/4` with `[TOOL_PATH, TOOL_ARGS...]`, except
that the admitted macOS compiler form executes the verified
`PRIVATE_COMPILER` pathname with that same path as argv[0]. The caller's
exclusive work root, its inaccessible ancestors, and the explicit unsupported
concurrent-work-root-mutation rule make that one pathname stable across the
final check and `execve`; descriptor 4 remains retained until the atomic exec
and closes in the new image. macOS `current_exe()` therefore returns the
resolvable private pathname, so adjacent `libalign_runtime.a` lookup and
compiler-byte cache identity both operate on the same admitted files.
Platform admission requires the observer's inherited hard file-size limit to
permit 67,108,864. Both tool
formats are validated as native images so no interpreter needs the closing
descriptor. Thus the tool remains the direct group leader and every regular
file it or an inherited helper creates has a kernel-enforced 64-MiB ceiling.

After launcher exec, every infrastructure tool receives `/dev/null` as stdin,
only descriptors 0/1/2, the §8 default signal mask/dispositions and owned
process group, and no ambient environment entry. Validation probes use cwd
equal to the canonical repository root and the exact validation environment in
§3. Fixture builds use the artifact build environment in §4; inspection and
Linux loader resolution use the timed-child environment. Launcher setup/exec
failure is the phase's formation/cleanup
failure under the same deadlines and ownership state; there is no direct
unbounded fallback launch.

Infrastructure phases drain both pipes without blocking while the child runs,
retain no more than the stated cap, and signal the owned group/direct child as
soon as another byte would exceed it. Probe captures remain in memory. Build
and inspection logs contain the complete streams on success or the exact capped
prefix on overflow. Overflow, timeout, incomplete capture, or a log write
failure fails the phase. The timed phase deliberately performs no concurrent
drain and follows §8 instead.

At a phase execution cutoff, the watchdog owns the generation and sends the
applicable group/direct SIGKILL sequence. All signal, status, `EINTR`, pipe, and
reap work must finish inside that phase's cleanup reserve. If the main thread
has not proved the direct child reaped and released the generation by the phase
terminal deadline, or if the global terminal deadline wins, the watchdog makes
one last applicable kill attempt and terminates the observer with `_exit(1)`.
No next phase, sample row, summary, or `end` is then permitted. This hard-abort
path claims no child cleanup success; the OS inherits any still-unreaped child.
Every complete report therefore comes only from the ordinary proved-reap path.

All provenance, fixtures, arm files, and tool availability validate first.
Then the retained fixture bytes and arm tools are sealed, and every fixture/arm
artifact builds and passes untimed inspection and loader resolution before the
first sample. A build, inspection, or loader-resolution failure prevents every
timing row; its bounded logs remain below
the work directory and the script exits 1. `results.tsv` is created exclusively
only after every artifact passes inspection and loader-closure revalidation, so
such a failure leaves no result
record to mistake for completed evidence.

Build/artifact order is fixture ordinal, then baseline, then candidate. Each
fixture builds in its own arm directory. The observer SHA-256 is inherited
descriptor 9, which is both the exact `/dev/fd/9` exec source and the retained
descriptor verified at process entry as specified in §1.
With the artifact directory as cwd, the
inspection report is raw stdout from exact argv
`[OBJECT_INSPECTOR, "--sections", "--needed-libs", "./FIXTURE"]` under the fixed
child locale, with empty stderr and exit 0. The stable relative operand keeps
the raw report independent of the work-root spelling. It describes sections and
dynamic dependencies on the exact executable; its content digest and parsed
dependency order enter `artifact`. The parser requires exactly one literal
`NeededLibraries [` line followed by zero or more lines beginning with exactly
two spaces and one closing `]` line. It removes those two spaces and retains
each remaining nonempty, NUL/CR/LF-free byte string in emitted order; malformed
or repeated blocks fail inspection.
All initial fixtures have the exact ordered requested gated-library list empty.
The existing `capability_linking` owner remains the producer for that compiler
decision; S0A does not reverse-engineer it from final dependencies.

On Linux the same bounded executable parse additionally requires exactly one
`PT_INTERP`, an absolute NUL-terminated path whose payload contains no embedded
NUL. `/etc/ld.so.preload` must be absent under a nofollow lookup. The interpreter
path is canonicalized, opened nofollow, and admitted by the immutable native-ELF
system-tool rules in §3. With the artifact directory as cwd, the observer runs
exact argv `[INTERPRETER, "--list", "./FIXTURE"]` through that retained
descriptor and the common launcher. Exit must be zero and stderr empty. Stdout
is ASCII and consists solely of: one `linux-vdso.so.1 (0xHEX)` row; zero or more
dependency rows `HTAB SONAME => ABSOLUTE_PATH (0xHEX)`; and one loader-self row
`HTAB ABSOLUTE_INTERPRETER (0xHEX)`, where `HEX` is nonempty lowercase
hexadecimal, each SONAME is nonempty and contains no slash or whitespace, and
each path token is nonempty and contains no whitespace. Duplicate vDSO,
loader-self, SONAME, or resolved path rows, an unrecognized row, `not found`, or
a path other than the retained interpreter in the self row rejects the
artifact. The observer resolves and hashes every dependency descriptor and
forms the Linux record in §4. `FINAL_DEPENDENCIES_LIST_HEX` is the ordered direct
SONAME subsequence of those entries; transitive entries retain loader order.

On macOS no artifact is launched for dependency resolution. The bounded Mach-O
load-command parse, dyld shared-cache APIs, and already loaded libSystem image
form the macOS record in §4. For both platforms, the observer repeats the full
parse/resolution/hash operation after all artifacts qualify, in artifact row
order immediately before timing, and requires each canonical record to be
byte-identical to its stored value. This second Linux resolution uses the same
bounded child phase and overwrites neither retained loader log; it compares its
captures in memory. A system producer change fails before `results.tsv` is
created.

The owned path topology is fixed:

```text
cache/<fixture>/<arm>/
build/tmp/
build/input/<fixture>.align
build/tool/<arm>/alignc
build/tool/<arm>/libalign_runtime.a
build/<fixture>/<arm>/<fixture>
logs/<fixture>/<arm>/build.stdout
logs/<fixture>/<arm>/build.stderr
logs/<fixture>/<arm>/inspection.stdout
logs/<fixture>/<arm>/inspection.stderr
logs/<fixture>/<arm>/loader.stdout
logs/<fixture>/<arm>/loader.stderr
logs/<fixture>/<phase>-<sequence>-<arm>-stdout.bin
logs/<fixture>/<phase>-<sequence>-<arm>-stderr.bin
observer/cwd/
results.tsv
provenance.tsv
```

All names shown are literal fixed ASCII or validated fixture/arm/phase/decimal
tokens. Build and inspection log pairs are required for every artifact; the
loader pair is required on Linux and forbidden on macOS; sample capture paths
exist only under §4's field rules. Each build cwd is
`build/<fixture>/<arm>`, and exact build argv is
`[PRIVATE_COMPILER, "build", SEALED_FIXTURE_SOURCE, "--profile", "release",
"--target-cpu", "baseline", "--rt-lto"]`. Before each build, its distinct
cache directory exists, is empty, and has never been supplied to another
process. Output stem `<fixture>` must be a
nofollow regular executable of at most 67,108,864 bytes before hashing or
inspection. The admitted mount's 4-GiB total capacity is the aggregate ceiling
for sealed tools, compiler temporaries, arbitrary additional arm output, logs,
and retained artifacts together; exhausting it fails the active phase, and
exact-size/rejected-next executable plus volume-fill owners pin both limits.
On successful group closure, each artifact cwd must contain exactly its one
named executable, `build/tmp` must be empty, and the private cache must pass the
exact manifest/tree validation in §3; an extra stable entry fails the phase.
Logs are created exclusively and never interpreted as authority. Any
collision after the root's empty check is an ownership loss and fails without
replacing the foreign entry.
The harness creates owned directories with mode 0700 and ordinary files with
mode 0600, independent of umask. Private compilers use mode 0700 and runtime
archives mode 0600. Build, inspection, and Linux loader-resolution stdin is
`/dev/null`; their stdout and stderr go only to the named logs.
`SEALED_FIXTURE_SOURCE`, `PRIVATE_COMPILER`,
and `OBJECT_INSPECTOR` are canonical absolute paths; inspection and loader
resolution use the fixed relative artifact operand above. Both receive
exactly the timed-child environment from §2. Concurrent caller mutation of the work root is
unsupported; every observable collision, symlink, kind, or identity change
fails closed.

The observer changes only its own process cwd. The main thread serially sets
each absolute build/inspection/loader cwd immediately before that child; the
watchdog never reads cwd or a relative path. After all inspection,
loader-closure revalidation, and tool revalidation,
the main thread changes once to `observer/cwd` and never changes cwd again.
Every subsequent observer file operation uses an absolute path. A second sample
cannot overlap, and no failure or Drop path restores a caller cwd because
`run.sh` was replaced by this dedicated process; its parent shell is unaffected.

Before it creates the watchdog or launches its first child, the main thread
uses checked `sigaction` calls to set the observer's own SIGCHLD disposition to
default and SIGPIPE disposition to ignore, both with empty masks and no flags.
Failure exits 1 before any child. The dedicated observer never installs a
handler and never restores process state that dies with it. Every spawned child
still receives the §8 explicit default dispositions. Consequently an
infrastructure launcher that exits before consuming readiness byte `G` makes
the parent write return `EPIPE`; it cannot terminate the observer or bypass the
owned group/direct cleanup path. Before every timed start it requires `observer/cwd` to
remain empty. A fixture-created or foreign entry fails that scheduled attempt
before the start timestamp rather than becoming ambient input to later samples.

One-arm execution per fixture is:

```text
warmup baseline[0]
warmup baseline[1]
measure baseline[0] ... baseline[19]
```

Two-arm execution consists of 22 blocks per fixture:

```text
warmup block 0: baseline(position 0), candidate(position 1)
warmup block 1: candidate(position 0), baseline(position 1)
measure even block: baseline(position 0), candidate(position 1)
measure odd block: candidate(position 0), baseline(position 1)
```

There are ten even and ten odd measured blocks. `SEQUENCE_ORDINAL` starts at 0
for each fixture and increments for every reached sample; `ARM_SAMPLE_INDEX`
starts at 0 separately for each phase and arm; `POSITION` is 0 in one-arm mode
and 0 or 1 inside a two-arm block. The complete Cartesian schedule is fixed by
arm count, phase, block parity, and fixture order.

After any failed sample, that fixture stops immediately, retains failed summary
state for every arm, and the harness continues at the next fixture so completed
evidence is preserved. It never fills missing ordinals with synthetic rows.
Only after every fixture's reached sample rows have flushed does it emit all
summary rows in fixture/arm order, followed by `end`. The final run still fails.

## 8. Process lifecycle and outcome precedence

Before the start timestamp the observer allocates captures, creates two
CLOEXEC pipes, opens `/dev/null` for stdin, and prepares fixed `posix_spawn`
file actions and attributes. A timed child inherits the already-fixed
`observer/cwd` and receives only descriptors 0, 1, and 2: after
stdin/stdout/stderr actions, Linux applies
`posix_spawn_file_actions_addclosefrom_np(..., 3)` and macOS uses
`POSIX_SPAWN_CLOEXEC_DEFAULT`. An infrastructure launch additionally duplicates
the retained observer/tool/readiness descriptors to 3/4/5, closes from 6 on
Linux or relies on the macOS default, and the §7 launcher closes 3/4/5 at tool
exec. The common attributes set an empty signal mask,
restore every catchable signal to its default disposition, and create a new
process group with pgroup 0. Parent read ends are nonblocking;
their distinct child write ends remain blocking. The child is therefore the
leader of the new group. Unsupported action/attribute formation fails before
the start timestamp.

One persistent watchdog thread owns at most one generation. It uses the same
monotonic clock, a non-poisoning fixed mutex/condition record, and no
process-global signal handler. Before the start clock, the main thread reserves
one generation with no pid plus its execution/terminal allowances. A successful
start clock fixes both deadlines, then `posix_spawn` runs. On spawn success, the
very next ownership action constructs a nonallocating, infallible direct-pid
guard. The same nonfallible acquisition step records the `pgroup=0` spawn
attribute's guaranteed `pgid == pid` result and publishes
`(generation, pid, group-owned, deadlines)` under that mutex. No validation,
allocation, fd operation, callback, or other fallible step intervenes. Any
unwind after guard construction, including during publication, runs its shared
group/direct-kill-and-reap closure; once publication completes, the watchdog
also enforces both cutoffs.

Only after this ownership is live does the parent close its stdout/stderr write copies and
verify the platform guarantee with `getpgid(pid) == pid`. A contradictory live
value is an unsupported-platform cleanup failure; an infrastructure launcher
has not been released and therefore cannot have spawned a descendant. ESRCH for a fast child is
accepted only when an immediate nonreaping `waitid` proves that exact child
terminal while its unreaped leader still pins the expected group identity. The
infrastructure parent writes `G` and closes its readiness write end only after
the successful live group check; timed fixtures have no readiness channel and
retain their fixed no-descendant contract. The parent then blocks in
`waitid(P_PID, ..., WEXITED | WNOWAIT)` through EINTR. If `getpgid` reports
ESRCH without that proof, it is a group-check failure. Group-check EINTR retries
stop at the execution cutoff. `WNOWAIT` keeps the leader
unreaped and pins its pid/group while the parent and watchdog resolve the
deadline race.

At the execution cutoff, the watchdog locks the active generation. If it wins
before terminal observation disarms that generation, it marks timeout and sends
SIGKILL first to the negative process-group id and then to the direct pid,
retrying EINTR and accepting ESRCH. For a timed fixture, terminal observation
instead disarms before watchdog signaling and transitions directly to
`reaping`. For every infrastructure phase, terminal observation transitions to
`signaling` and sends SIGKILL to the owned negative group even after a successful
leader exit; this makes any still-running compiler, C-driver, linker, or
inspector helper unable to outlive the phase. The parent waits for claimed
signals, obtains one successful `wait4` through EINTR to reap and collect usage,
and reads the end timestamp immediately.

After an infrastructure leader is reaped, the parent never signals that numeric
group again. It probes `kill(-pgid, 0)` through EINTR until ESRCH or the terminal
deadline, using a checked monotonic clock and at most one 1-ms monotonic sleep
between successful probes; any initially surviving group makes the phase a
cleanup error even if it then disappears. Only proved ESRCH clears the pgid and permits final pipe
drain/capture. A timed fixture has the separately fixed no-descendant contract,
so it clears pid/pgid after direct reap and uses the passive §4 capture. The
generation releases only after the applicable capture path closes both pipes.
Every signal, reap, group-absence probe, final clock read, and capture operation
must complete before the terminal deadline; otherwise §7's hard abort wins. No
code uses the numeric pid after successful reap, and only the infrastructure
absence probe may use its pinned pgid afterward.

A spawn/group/status/capture setup error enters the cleanup path when a child
exists. Successful `pgroup=0` spawn formation owns the expected negative group;
a contradictory live `getpgid` result suppresses that unsafe group signal,
forces direct kill/reap, and hard-fails the unsupported platform before another
phase. A non-EINTR, non-ESRCH signal error is retained as cleanup failure, but
direct-pid SIGKILL and reap still complete before return. Infrastructure phases
prove their owned group absent; the fixed timed fixtures contain no
process-spawn capability and make no escaped-group descendant claim. An
unexpected external SIGCHLD handler or reaper is
unsupported and yields cleanup failure; the harness never changes the parent
shell's SIGCHLD disposition.

After reap the parent queries `FIONREAD` once for stdout and then stderr,
rejects a negative count, and snapshots each accepted queued byte count. It
reads each accepted quota through EINTR with a fixed 4,096-byte buffer, retains
the first 4,096, persists each §4-qualified capture, then closes both read ends
without waiting for EOF. Premature EOF or a zero read before a quota is a
cleanup error; the other stream still follows its independent attempt sequence.
Writes from an out-of-contract descendant after the snapshot
cannot extend the quota. A snapshot above 4,096 selects `output-overflow`;
`STDOUT_LEN` and `STDERR_LEN` are the snapshot counts. Valid fixtures fit in
the pipe. A broken producer that fills it blocks until the watchdog kills the
child, so capture storage and filesystem growth remain bounded.

Every fd close first nulls its owner, executes once, and never retries an EINTR
result because the platform may already have consumed the descriptor. No new fd
is allocated until that close set completes, so the numeric slot cannot be
mistaken for a later resource. The error is retained as `cleanup-error`; Drop
cannot close the nulled slot again. Reads, status/reap, and signals use their
operation-specific safe EINTR retry rules within the same terminal cutoff.

Validation and outcome precedence is deterministic:

1. `cleanup-error`, including formation cleanup, group mismatch, permanent
   signal/status, or pipe-query/read/persist/close failure;
2. `formation-error` before the start/spawn completes;
3. `spawn-error`;
4. `timeout` when the watchdog won the synchronized deadline race;
5. `signal` for a non-timeout signalled child;
6. `exit` for a nonzero/incorrect ordinary exit;
7. `output-overflow`;
8. `metric-error`, including invalid usage data or the post-reap clock read;
9. `stdout-mismatch`;
10. `stderr-mismatch`; and
11. `ok`.

Both stream match flags and all valid metrics remain in the row even when an
earlier outcome wins. Cleanup errors outrank the trigger because a return
without owned-child closure would invalidate all later evidence.

There is no shared mutable state between observer processes. Inside one
observer, the generation lock rejects a second active child phase as an
internal error before spawn. Watchdog exhaustion or generation overflow fails
before another phase; no generation is reused. Before the terminal cutoff,
Drop performs the same applicable group/direct kill and direct reap for an
armed child and cannot emit a successful row. At the terminal cutoff only the
watchdog's §7 hard-abort path is allowed.

## 9. Closure matrix and acceptance

| Axis | Exact closure | Owner evidence |
|---|---|---|
| Formation/validation | Four-option wrapper CLI before all effects; explicit observer path/digest; internal descriptor adoption; empty wrapper-to-observer environment; private loaded-image inode; all tool versions before work mutation; producer-bound host/work-filesystem/execution-state records including default Linux NUMA policy; compiler/output target architecture and subtype; complete loaded-LLVM/codegen-key identity and filename derivation; CAS content identity; platform loader closure; all field combinations; absolute/raw-byte paths; arm topology; retained fixture bytes; immutable tool chains; work root; digests | Parser tables plus semantic/byte golden vectors, malformed CLI with an unavailable observer and no side effects, inherited-fd/path exec and nonempty-environment rejection, observer digest mismatch, stale compiler version with empty work root, host-producer mutation, whole-backing-filesystem mount alias/capacity/replacement, compiler/artifact architecture/subtype mismatch, every v5 key-field/full-slot-name/CAS mutation, loader/shared-cache mutation, concurrent wrapper copy, inherited-state/NUMA mutation, source-mutation/sealed-copy, tool swap-and-restore, and multi-invalid precedence owners |
| Construction | Before watchdog/child construction the dedicated parent fixes checked SIGCHLD/SIGPIPE dispositions; every probe/build/inspection/loader/sample then uses the common bounded pipes, fixed actions/environment/argv, immediately owned group, monotonic execution/terminal deadlines, watchdog generation, terminal observation, and `wait4` owner; infrastructure launchers remain descendant-free behind a one-byte group-verification barrier; the macOS compiler alone retains descriptor identity while executing its stable private path | Signal-disposition failure owner, injectable syscall-state unit owner, plus barrier-controlled fd/path launcher and real helper-child integration owners for every phase class |
| Move-in/out and source nulling | Each loaded-image fd, fixture buffer/sealed copy, child fd/pid generation, capture, rusage, artifact, and report buffer has one owner; the direct guard is the first post-spawn action; generation crosses reserved/direct-owned/group-owned/signaling/reaping/capturing/released once | Failpoint sweep after every construction/transfer, spawn-return-to-guard structural assertion, cross-arm source mutation, and Drop barrier |
| Replacement/return | No active generation can be replaced; only the main thread changes cwd and the watchdog never resolves relative paths; timed cwd is immutable; sample row flush follows complete terminal classification; incomplete report lacks `end` | Overlap rejection, serialized cwd transition under a live quiescent watchdog, generation exhaustion, short-write, interrupt, and missing-end decoder owners |
| Drop/cleanup | Spawned direct child receives group/direct SIGKILL as applicable and is reaped before ordinary return; every infrastructure group is proved absent before phase release; pid is unused after reap and pgid only serves that absence probe; a missed terminal deadline hard-aborts without another phase or success claim; work root remains | Timeout/reserve/global-cutoff, signal, group mismatch, lingering helper with closed pipes, EINTR, ESRCH, permanent-error injection, escaped-private-group negative, hard-abort, and immediate pid/pgid-reuse model tests |
| Branch/early failure | Probe, build, cache publication/decoding/content hashing, inspection, loader resolution, launcher limit/handshake/exec, formation, spawn, pre-reap rollback, timeout, signal, exit, phase-output overflow, metric, every two-stream partial-capture product, and result-write failure follow exact precedence/topology | One parameterized phase/outcome/field-presence owner, including pre-clock and post-spawn-failure cleanup, reaped cleanup, readiness EOF/wrong/extra byte, readiness `EPIPE` with live observer cleanup, every launch transfer failure, two simultaneous invalid observations, and every hard-abort checkpoint |
| Generic/interface serialization | N/A: fixtures expose only executable main ABI and no imported Align units | Fixture inventory/import-graph assertion |
| Whole/per-unit/cache | Fixtures are whole programs. Every build uses a distinct empty explicit cache solely to force a production miss and obtain that invocation's complete existing version-5 codegen key; every fixed field, producer-owned field, action/slot digest, manifest copy, and CAS object is closed, and no entry is reused | Fake compiler argv/env owner, exact private-cache tree, every decoded component and presence invariant, full/slot derivation vectors, version/tag/truncation/trailing/field mutation goldens, CAS name/content/short-read/replacement goldens, and no-hit assertions |
| Runtime ABI/provenance | No ABI or runtime symbol changes; producer-observed host/libc identity, the inherited descriptor used as observer exec source, immutable C/linker/inspector/loader paths, private compiler/adjacent-runtime digests, compiler/output architecture, compiler-produced mapped-LLVM id, and platform loader/dependency closure bind every artifact | Every host producer variant, stale/fabricated field rejection, same-version/different-LLVM-build rejection, cross-arm LLVM mismatch, compiler/output architecture mismatch, Linux interpreter/vDSO/dependency and macOS dyld-cache/libSystem identity mutation, tool swap-and-restore, concurrent observer rebuild, wrong/closed/replaced descriptor rejection, distribution/source-arm adjacency, and mutation tests |
| Allocation/resource | Fixture sealing, every phase capture, loader closure, macOS mount-table snapshots, row storage, each executable, and aggregate work filesystem remain bounded; child usage comes only from fixture/runtime; overflow cannot grow evidence beyond the admitted mount | Every exact/rejected-next stream/closure/executable/filesystem cap, macOS 1,024/1,025 mount rows plus count/fill/snapshot churn, aggregate volume-fill failure, infrastructure/timed pipe-fill timeout, sample-count allocation, and no-timed-probe structural owners |
| Target/profile/link | Both arms use identical target, CPU, profile, runtime-LTO, mapped LLVM build, libc/link, strip, linker, and build environment shape; compiler and artifact headers must match that target/subtype; each artifact records its resolved loader closure; linker probe is Linux `--version` stdout or macOS `-v` stderr | Cross-arm mismatch rejection, ELF/Mach-O target-header table including raw subtype, both platform probe goldens, decoded compiler-produced codegen key, actual build argv, inspector report, and repeated platform-loader closure owners |
| Schedule/statistics | Every arm/phase/block/position/ordinal combination and median rule is exhaustive | One-/two-arm golden schedules, failed-prefix rows, median extremes, and checked conversion tests |
| Platform availability | Linux/macOS unit normalization; all eight fields are required; Linux requires default inherited NUMA policy, one root-of-filesystem mount with no same-device alias, classified complete cgroup-v2 constraint state, `PT_INTERP`, and in-memory vDSO build-id; macOS requires a stable at-most-1,024-entry mount table, raw arm64 subtype zero, and dyld shared-cache membership/UUID; unsupported host fails before build | `rusage` conversion tables, default/nondefault/unavailable NUMA tables, same-device/different-root mount table, bounded/changing macOS mount table, cgroup known/unknown path/ancestor/control mutation tables including `io.prio.class` and optional dynamic hugepages, Linux loader/vDSO and macOS dyld/subtype tables, invalid-value goldens, and platform gate |

The independent full-diff review of candidate `9480c714` produced five valid
findings. Their ledger-first closure is:

| Finding | Root class | Revised contract and owner |
|---|---|---|
| P1 macOS rejected by GNU linker probe | Platform-specific tool identity | §3 selects Linux stdout from `--version` or macOS stderr from `-v`; both probe goldens are required. |
| P1 unbounded child phases/cleanup and build logs | Child lifecycle/resource bound | §1 and §7 give the wrapper and every observer phase execution, cleanup, capture, and global cutoffs; real/injectable owners cover normal reap and hard abort. |
| P1 summaries could precede later samples | Canonical record topology | §7 retains summary state until all reached samples flush; failed multi-fixture goldens pin samples-before-summaries order. |
| P2 mutable fixtures could diverge across arms | Input identity/TOCTOU | §3 seals retained validated bytes once and every arm builds the owned copy; mutation owners prove both arm artifacts bind the recorded digest. |
| P2 partial capture fields were underspecified | Failure-state Cartesian product | §4 fixes both streams' length/match/path presence independently for every query/read/persist/close result; codec and failpoint owners cover the full product. |

The full redesign review of candidate `63081c97` found a new P1 and therefore
reopened the acquisition-to-guard and loaded execution-identity axes rather
than continuing a local patch loop:

| Finding | Root class | Redesigned contract and owner |
|---|---|---|
| P1 fallible group check preceded child guard | Acquisition-to-guard ordering | §8 reserves the generation before spawn, makes the direct-pid guard the first infallible post-success action, and publishes the `pgroup=0` owned group before validation; a structural assertion and failpoint owner pin the sequence. |
| P2 inherited affinity/nice/cgroup/rlimit state was unnamed | Ambient execution identity | §4 adds the producer-owned canonical execution-state record and requires exact checks around every infrastructure child and before each timed start. |
| P2 observer hash followed a replaceable shared path | Loaded producer identity | §1 executes the inherited descriptor of one exclusive private copy, removes its only path before exec, and hashes the same descriptor in the observer; concurrent wrapper builds cannot change it. |
| P2 spawn-error wall presence conflicted | Failure field topology | §4 keeps the parent wall interval when both clocks succeed and makes only child-produced fields unavailable. |
| P2 partial-read log retention conflicted | Failure field topology | §4 makes every nonempty failed-read prefix a retained log and preserves the same condition in its field matrix. |
| P2 watchdog/cwd ordering conflicted | Process-global state | §7 and the matrix assign cwd only to the main thread, prohibit watchdog-relative I/O, and make cwd immutable only for timed execution. |
| P3 acceptance bypassed Cargo wrapper | Supported build entry | §9 routes the standalone owner through `scripts/cargo.sh`. |

The next full review of candidate `20b49a38` found two new P1s, so this design
again reopened the infrastructure-group-quiescence and build-volume-bound axes
before implementation:

| Finding | Root class | Second redesigned contract and owner |
|---|---|---|
| P1 successful infrastructure leader could leave helpers | Whole-group lifecycle | §8 kills the owned group at every infrastructure terminal observation, reaps the direct leader, then requires a post-reap ESRCH absence proof before release; a closed-pipe lingering-helper owner pins the failure. |
| P1 compiler output could fill the host filesystem | Aggregate resource boundary | §2/§4 require an otherwise-empty caller-exclusive mount of at most 4 GiB, record its producer identity/geometry, and cap each executable at 64 MiB; exact/rejected-next and volume-fill owners close transient and retained growth. |
| P2 C/linker/inspector paths could swap and restore | Executed-tool identity | §3 requires every resolution directory and tool component to be other-owned/nonwritable, runs direct tools from retained descriptors, and rechecks the immutable chains around use. |
| P2 host provenance fields lacked producers | Host evidence identity | §3 derives OS/kernel/arch/CPU/count/memory/target/libc from exact native producers and rejects mismatched supplied values. |
| P2 cgroup allowlist omitted live constraints | Ambient execution identity | §4 records the expanded fixed/dynamic constraint set and rejects any controller entry not classified as a constraint or a fixed volatile/command field. |
| P2 optional detail elements were ambiguous | Canonical result decoding | §4 tags operation, OS-error, stdout-path, and stderr-path elements, fixes their order, and rejects malformed role topology. |

The full review of second-redesign candidate `d386424c` found three new P1s,
so the signal-disposition and producer-target-identity axes were reopened before
implementation; the same pass closed the related filesystem/cgroup discovery
class instead of patching only its named examples:

| Finding | Root class | Third redesigned contract and owner |
|---|---|---|
| P1 readiness write could terminate the observer with SIGPIPE | Parent signal disposition | §7 fixes SIGCHLD/SIGPIPE before watchdog or child creation, §8 resets child dispositions, and a closed-readiness `EPIPE` owner proves the live parent completes ordinary child cleanup. |
| P1 artifact omitted the compiler arm's loaded LLVM producer | Producer identity | §2–§4 add compiler-manifest-produced `LLVM_BUILD_ID`, require every artifact in both arms to match, and bind it into every artifact row; empty private-cache and version-5 codec owners prove it came from each producing invocation. |
| P1 translated compiler/output architecture could disagree with the host target | Target identity | §3 directly validates target-specific ELF/Mach-O headers for both private compiler and retained output before timing; native, wrong-architecture, translated, fat, and malformed-image owners cover the table. |
| P2 Linux alias check covered only the same mount root | Aggregate resource identity | §4 requires mount root `/`, matches the retained descriptor's device pair, and rejects every other current-namespace mount row with that pair regardless of root or mount point. |
| P2 current cgroup I/O priority constraint was omitted | Ambient execution identity | §4 adds `io.prio.class` in fixed order; the complete known/unknown controller-entry sweep retains fail-closed admission for later kernel constraints. |

The full review of third-redesign candidate `197a356e` found three new P1s, so
the resolvable-compiler-launch, pre-reap-result-topology, and
producer-loader-closure axes were reopened before implementation:

| Finding | Root class | Fourth redesigned contract and owner |
|---|---|---|
| P1 macOS fd launch made `current_exe` unresolvable | Loaded compiler identity | §7 gives only the private macOS compiler a retained-descriptor-verified private-path exec; its `--version` branch remains fd-launched before mutation, while real builds resolve the same admitted compiler path and adjacent runtime. |
| P1 compiler version was checked after work mutation | Validation side-effect order | §3 runs every retained compiler `--version` under one exact validation environment before any work-root output; a stale-version owner requires exit 2 and an empty root. |
| P1 pre-reap cleanup fields were not exhaustive | Failure result topology | §4 separates pre-clock rollback, failed-spawn rollback, successfully reaped cleanup, and unreaped hard abort, with exact wall/child/capture field presence for each. |
| P2 cache manifest did not authenticate its CAS bytes | Producer cache identity | §3 streams the sole bounded CAS object through the existing two-seed `Hash128` algorithm and binds its name, shard, metadata, bytes, and decoded manifest digest. |
| P2 Mach-O CPU subtype was unchecked | Target identity | §3 accepts only raw `CPU_SUBTYPE_ARM64_ALL` with no capability bits for compiler and output; subtype mutation owners cover both images. |
| P2 timed loader/dependencies were not producer-bound | Loader closure identity | §4/§7 record and repeat Linux interpreter/vDSO/resolved-image identities or macOS dyld-cache/libSystem identities before timing, without probing the timed child. |

The full review of fourth-redesign candidate `f9d643a2` found one new P1, so
the wrapper-to-observer-environment and complete-cache-key-derivation axes were
reopened before implementation:

| Finding | Root class | Fifth redesigned contract and owner |
|---|---|---|
| P1 caller loader variables could interpose the observer | Parent execution identity | §1 uses Bash `exec -c`, requires an empty native environment at observer entry, and supplies every later child environment explicitly; direct/nonempty-environment owners reject drift. |
| P2 decoded CodegenKey components and filenames were partly unchecked | Producer cache identity | §3 validates every fixed/current component, independently checks compiler identity, binds producer-variable key bytes into each artifact row, recomputes full/slot digests and filenames, and authenticates the manifest/CAS chain. |

The final full review of candidate `9267c675` found one P1 and three P2s. The
author reopened the observer-acquisition, validation-precedence,
inherited-NUMA, and macOS-mount-producer axes together and closed the complete
finding class in one pass:

| Finding | Root class | Final contract and owner |
|---|---|---|
| P1 observer build inherited ambient Cargo configuration | Observer acquisition identity | §1 removes Cargo from the evidence-producing entry. The caller supplies an absolute observer and expected SHA-256; the wrapper privately copies it and the adopted descriptor must hash to that value. Different builds are different explicit inputs, never silently equivalent evidence. |
| P2 malformed public options could lose to observer-build failure | Validation precedence | §1/§3 validate all four public options with shell built-ins before every mutation or child. With no in-entry build, a malformed option deterministically exits 2; a malformed-option-plus-missing-observer owner pins the order. |
| P2 inherited Linux NUMA policy was omitted | Ambient execution identity | §4 records and admits only `MPOL_DEFAULT` with an empty nodemask; every explicit/unknown policy and unavailable producer rejects before effects, and the execution-state sweep repeats the check around children. |
| P2 macOS mount-table allocation and snapshot were unbounded | Bounded producer state | §4 caps each of two `getfsstat` snapshots at 1,024 entries, rejects 1,025, truncation, count/fill disagreement, and inter-snapshot churn, and the allocation/platform matrix owns each boundary. |

These are closures against the already completed independent review, not a new
full-diff discovery round. Implementation must satisfy the revised matrix and
its exact owner tests; it must not reopen the discarded ambient-build path.

The author-side producer sibling sweep also replaces macOS `dladdr(malloc)`
with the bounded dyld image-table entry whose `LC_ID_DYLIB` is exactly
`/usr/lib/libSystem.B.dylib`; the loader-closure and libc records now select the
same in-memory producer rather than a libSystem re-export implementation.

The real helper-child owner covers exact success streams, wrong stdout and
stderr, ordinary nonzero exit, signal death, every infrastructure/timed deadline
and cap, pipe overflow, readiness `EPIPE` without parent death, a successful infrastructure leader with a pipe-closing
lingering group member, proved group absence, and ordinary group cleanup. The injectable
owner covers signal-disposition setup, syscall and clock failures that cannot be induced safely, every
EINTR retry, execution/reserve/global deadline races, hard abort, every capture
field-presence product, invalid metric data, and no-pid-use-after-reap except the
single pinned infrastructure pgid absence probe. It tests
the same state machine; it is not a second implementation.

The `run.sh` owner runs a private rewritten copy with shortened constants and a
fake supplied observer. It covers CLI rejection before copying, digest mismatch,
success disarm-before-exec, nonzero exit, execution
cutoff, TERM/KILL ordering, a descendant, and terminal-deadline wrapper exit
without exposing a production timeout override. It also covers `/dev/fd/9`
exec/adoption, every descriptor validation failure, and no pre-exec pathname
residue. Two concurrent wrapper runs must load distinct private inodes whose
retained-descriptor hashes match their own bytes even when the shared observer
source path is replaced between copies. Work-root owners cover mountpoint
alias/replacement, exact and rejected capacity, executable size, a fake compiler
that fills the volume, and cleanup that never unmounts or deletes the caller
root. Host/tool owners cover every platform producer, stale/fabricated fields,
default/nondefault/unavailable Linux NUMA policy, same-device/different-root
aliases, macOS mount counts 1,024/1,025 and two-snapshot churn, unknown cgroup controls including the
`io.prio.class` known case, target-header mismatches, compiler-produced LLVM
identity mismatches, and C/linker/inspector swap-and-restore attempts.
They also cover stale compiler versions before work mutation, CAS
name/content/replacement mismatches, both macOS private-launch forms, Linux
interpreter/vDSO/dependency closure changes, and macOS dyld-cache/libSystem
closure changes.
The wrapper owner also asserts that arbitrary loader/allocator/cache variables
visible to `run.sh` produce an empty observer environment, and direct observer
launch with any entry fails before public parsing. Cache owners mutate every
fixed and producer-owned CodegenKey component, optional tag, full/slot filename,
manifest copy, and CAS relationship independently.

The committed fixture owner independently runs `alignc check` on all ten
programs, then runs `run.sh`; every warmup and measured child enforces its fixed
oracle. Its assertions concern row topology and oracles only; no
elapsed/RSS/fault threshold enters CI. Existing `capability_linking` and
`par_map_cold_start` owners remain authoritative for dependency selection and
pool initialization respectively.

Implementation acceptance is:

```text
scripts/cargo.sh test --manifest-path bench/startup/Cargo.toml
scripts/cargo.sh build --release --locked --manifest-path bench/startup/Cargo.toml --target-dir ABS
bench/startup/run.sh --observer ABS --observer-sha256 SHA256 --work-dir ABS --provenance ABS
```

The second command is local qualification on the V0 host/artifact record and is
not added to `scripts/test-pr.sh`. The implementation PR selects the standalone
crate tests plus the smallest fixture/structure owner it adds, then follows the
ordinary code review and preflight gates for its classified tier.

## 10. Explicit exclusions

S0A does not provide cold-cache eviction, energy counters, hardware performance
counters, runtime tracing, child probes, mapped artifacts, a stable external
benchmark service, automatic uploads, a compiler command, JSON, a timing
threshold, a cross-language comparison, or a startup optimization. It does not
reuse the REPL's wall-only `:time` result as qualified evidence and does not
replace the general driver/test process runners. The dedicated mount bounds all
contracted cwd/`TMPDIR`/artifact output, but S0A is not a hostile-code sandbox:
its digest-bound Align/compiler and system-tool arms must not deliberately write
to unrelated absolute paths or escape the owned process group.

S0B remains the next independent observation design. C0 chunks consolidation
and any S1/S3 optimization remain unapproved until their own evidence and
closure gates are met.
