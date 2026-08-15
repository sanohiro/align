# JSON escape benchmark evidence

> 🌐 [English](../json-escape-benchmark-evidence.md) · **日本語**

Status: align-llm Request 7 の提案中 design。本書は evidence boundary だけを定義する。JSON
language change の受理、immutable baseline の選択、performance claim は行わない。evidence
implementation と benchmark-input prerequisite は Request 7 implementation branch より先に merge
されなければならない。

## 目的と threat model

Request 7 は計測値を生成する code 自体を変更する。その implementation が baseline、controller、
workload、toolchain、host、sample order、parser、threshold、attestation を選んではならない。
benchmark script は candidate compiler/runtime を実行できるが、自分だけで accepted result を発行できない。

trusted boundary は、baseline 選択前に reviewed controller から install された root-owned launcher、
immutable baseline 内の同一 controller bytes、complete build toolchain/offline dependency cache を持つ
digest-pinned Linux x86_64 OCI image、named otherwise-idle native x86_64 host、kernel-enforced read-only
source/controller/toolchain/dependency mounts、revision ごとの writable workspace、measurement container
に決して mount されない host-held Ed25519 signing key から成る。

candidate code、benchmark child、ambient shell、mutable image tag、network、registry state、unsigned
report text は untrusted。host administrator、container daemon、host kernel、merged controller、pinned
image、private attestation key は trusted root であり、侵害時は profile を revoke し、新しい reviewed
profile で evidence を再取得する。

controller は provider API/credential を使わない。通常の GitHub publication/merge は repository policy
に従う。GitHub token、SSH agent、cloud/package credential、user home、caller-selected executable は渡さない。

## Authoritative contract ledger

この ledger が authoritative である。後続の prose/implementation は field を具体化できるが独立に広げない。

| Field | Exact contract |
|---|---|
| Public producer | `/opt/align-evidence/v1/bin/align-json-escape-evidence run --repository REPO --baseline BASE --candidate CANDIDATE --review-log REVIEW_LOG --output-dir NEW_DIR`。root-owned launcher/module は merged evidence implementation から install され benchmark account から immutable、hash は host profile に埋め込む。verified `BASE` object から controller/profile/key blob を直接読み、installed copy と byte/mode equality を確認してから他の処理を行う。path は absolute、OID は lowercase 40-hex、`NEW_DIR` は不存在。override/追加引数なし。 |
| Public verifier | `/opt/align-evidence/v1/bin/align-json-escape-evidence verify --report REPORT --signature SIGNATURE --expected-baseline BASE --expected-candidate CANDIDATE --pr-body PR_BODY --review-attestation REVIEW_ATTESTATION`。bytes/signature/report/review/preflightをexplicit OID、PR body、trusted review attestationへ照合。localはdiagnostic、acceptanceはtrusted-base CIがGitHub event/APIからcheckout外で入力を作る。build/checkout/network/mutationなし。 |
| Public merge verifier | `/opt/align-evidence/v1/bin/align-json-escape-evidence verify-merge --repository REPO --report REPORT --signature SIGNATURE --merge MERGE --output-dir NEW_DIR`。provider response exact object fetch後、reportを再verify、raw-object pathで`MERGE`を読み、local target first-parent chain、parents/treeを照合しexact `merge-verification.json`/`merge-verification.json.sig`を出力。`NEW_DIR`不存在、overrideなし。 |
| Trusted review adapter | trusted-base CIだけがevent repo/PRのGitHub review APIをjob tokenでquery。PR author、write roleなし、dismissed/stale/duplicate/wrong commit、exact report `log_sha256`なしをreject。canonical `REVIEW_ATTESTATION`をcheckout外で作りverifierへ渡す。token/raw responseはcontroller/candidate/report/container/PR argumentへ渡さない。 |
| Ambient state | semantic default なし。empty environment に fixed `PATH`, `LC_ALL=C`, `TZ=UTC`, empty `HOME`, `CARGO_NET_OFFLINE=true` と controller-created descriptor/config だけを置く。ambient Git/Cargo/Rust/Docker/locale/proxy/credential/target/tuning は渡さない。 |
| Result | successはlock中にprivate report/signature/directoryをfsync、profile-global publication reservationをcreate/fsync、unlock、`NEW_DIR`へatomic rename、parent fsync、reservation remove/fsync、path出力。reservation中のsecond invocationはrepository/image/container前reject。threshold failureも同sequence。failureはstaging/output/reservationを削除しpathなし。crash/cleanup failureはreservationをfail-closedで残しadmin recovery。 |
| Controller owner | checked-in Python 3 controller、root-owned installed launcher、`scripts/` と `tests/benchmark_evidence/` の fixture tests。installed/source relation、installer manifest、interpreter、Git、Docker client/daemon、`ssh-keygen`、kernel、OCI image、host profile、executable SHA-256 をすべて記録する。candidate file は evidence root にならない。 |
| Persisted format | canonical UTF-8 JSON `align.json_escape_benchmark_evidence/v1` と `align.json_escape_benchmark_merge_verification/v1`。それぞれhost Ed25519 keyとfixed namespaceでbyte-for-byte署名。unknown/missing/duplicate/reordered/non-ASCII key、non-integer/float、invalid UTF-8/escape、trailing/noncanonical bytesはreject。 |
| Ownership/allocation | controller が temp dir、pipe、child、container、capture、report staging、cleanup を所有。benchmark child は stdin `/dev/null`、private stdout/stderr のみ。capture ceiling は profile 固定。report/signature/repository administration/controller/Docker socket/signing key/other revision writable dir は child に渡さない。 |
| Concurrency | repository inspection前からsigning/cleanupまでhost-global lock。unlock前にprofile-global publication reservationをinstallし、publication完了までlater invocationはrepository/image/container work前にreject。baseline/candidate overlapなし。 |
| Prerequisites | benchmark-input slice、Request 7 の両 language prerequisite、本 design、evidence implementation、pinned image、host profile/public key、adversarial owner が merge 後にのみ `BASE` を選ぶ。`BASE` は当時の target tip かつ最初の Request 7 commit の exact parent。 |
| Acceptance | valid signature、`pass`、exact PR/preflight/trusted-review binding、unchanged target base at merge、final fetched targetからmergeがreachableであるsigned merge artifact、identical protected input、5 field各10 sample、全ratio `<=1.05`。 |

`REVIEW_LOG` は candidate repository 外の標準 pre-open review log。`CLEAN` なら candidate へ直接、
`FINDINGS` なら reviewed ancestor と `CANDIDATE` までの complete findings-fixed chain に bind する。
controllerはlog SHA-256/bindingを記録するがcaller proseをauthenticとしない。

PR open後、independent reviewerがbodyにexact one line
`ALIGN_REVIEW_LOG_SHA256=<report log_sha256>`を持つnative GitHub reviewをsubmit。trusted-base CIはAPIから
次のcanonical single-line JSON + LFを作る。

```text
ReviewAttestation = {"repository":name,"pull_request":u64,"review_id":u64,
  "reviewer":name,"review_commit":hex40,"review_state":ReviewState,
  "review_log_sha256":hex64,"submitted_at":time}\n
```

`clean`はGitHub `APPROVED` on candidate、`fixed`はGitHub `COMMENTED` + `FINDINGS` on `review_head`で、
PRはcomplete disposition/repair chainを持つ。adapterはrepository/event PR/head/base、reviewer role/ID、commit/
state/body digest/reportを照合。missing/stale/author-owned/duplicate/edited/substitutionはreject。fixture API responseが
candidate-controlled content前に全failureを所有。

## Merged profile と fixed identity

implementation は `/opt/align-evidence/v1` に root-owned launcher を install する。benchmark account は
write 不可。manifest は全 relative path/mode/owner/size/SHA-256 と evidence implementation commit/profile
blob OID を固定する。launcher は caller input より先に self/module を open・manifest check し、pinned Git
で `BASE` の同 path を読み exact equality を要求する。verifier も同じ program で、`REPO`/revision/CWD/
`PYTHONPATH`/user site から import しない。replacement は baseline 選択前の administrator qualification。

canonical profile `bench/json_escape/evidence/linux-x86_64-v1.json` は以下を exact に持つ。

- schema/profile ID、fixed local target ref `refs/heads/main`、host ID、machine/kernel、CPU
  vendor/family/model/stepping/microcode、online/benchmark CPU set、NUMA、minimum memory。
- host-global lock path と pre/between/post resource limit。
- Docker client/daemon version/hash/architecture/storage/cgroup/runtime identity。
- image registry digest、local image/config digest、`linux/amd64`、Python/Git/Cargo/rustc/LLVM/CC/linker/
  `ssh-keygen` version/hash。
- read-only Cargo home/cache manifest、fixed Cargo config、capture ceiling、phase timeout、public key/fingerprint。
- threshold `105/100`、warm-up `1`、pair `10`、benchmark/field inventory。

mutable tag、wildcard、caller override、optional identity、secret はない。変更は新しい reviewed profile ID。
controller は lock、native x86_64 host/daemon、CPU/microcode/memory/no quota、executable、runtime、local
`--pull=never` image と digest/config/platform、load、image self-inspection の tool identity を確認する。
ARM/Rosetta/QEMU/binfmt/cross-arch/mutable tag/change は reject。この lane は x86_64/ARM を emulate しない。

benchmark CPU は exclusive cpuset。root-owned pinned monitor が scheduler/cgroup/thermal-throttle/frequency/
pressure/memory/swap/container event source を build 前に開く。各 child の exact cgroup を bracket し、foreign
task schedule、migration、throttle/thermal/frequency limit、pressure/load、swap/memory、foreign container
transition を child reap 後まで latch する。counter delta により periodic sample 間だけの event も閉じる。
100 ms snapshot は load/pressure/frequency/temperature/free memory/container inventory を記録。overflow、lost
event、delay超過、death、counter reset、unattributed event は reject。boundary sample も保持し、全 observation
を report に含める。monitor control fd は container に渡さない。

## Revision/source construction

pinned `/usr/bin/git` のみを empty environment で用い、system/global/XDG config、hook、filter、fsmonitor、
commit graph、replacement、graft、lazy fetch、optional lock、prompt、alternate、network を無効化する。
`REPO` を no-follow open、common directory を一度解決し、symlink admin、alternate、promisor、shallow、
replace/graft/submodule/missing object を revision resolve 前に reject。

各 commit について raw commit SHA-256、tree OID/closure、parent、全 path の raw mode/type/OID/size/blob
SHA-256 を記録し、次を要求する。

- 両OIDはrun前から存在するcommit。
- first Request 7 commit の sole parent は `BASE`、以降first-parent descendant、終端`CANDIDATE`。
- review log は candidate または permitted findings-fixed chain を指す。
- merge/replacement/missing object なし。全 commit/path/mode inventory を出し、independent review/PR
  disposition が protected でない全 change の Request 7 scope 所属を attest。
- profile local target ref は run start で `BASE`。remote currency は publication/premerge で別確認。

verified raw blob から private source dir を作り、ambient worktree/index/hook/archive filter/candidate script は
使わない。`100644`/`100755`/review済みsymlinkをexact materialize。new/changed symlink、submodule、special、
hardlink/case-fold/normalized collision、absolute/`..`/NUL/non-UTF-8 path は reject。retained-fd walk で raw
object と byte/mode equality を確認し read-only mount、post-run manifest も同一にする。

protected input はexactに以下。

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

presence/path/mode/type/blob/manifest equalityをcandidate実行前に要求し、script/kernel/harness/manifest/lock/
generator/output/timing/config、profile/public key、installed source/manifest、adversarial ownerすべてを含む。
変更はseparate reviewed profile implementation/requalificationが必要でRequest 7 candidateと併存不可。

## Container/process boundary

各build/benchmarkはtrusted controllerが新規containerで実行。profile image、`--pull=never`、
`--network=none`、read-only root、cap drop、`no-new-privileges`、fixed uid/gid/seccomp/LSM/CPU/NUMA/
memory/swap/pid/file/fd limit、minimum device、private namespace/tmp。Docker socket、host `/proc`、home、
agent/credential/repo admin/controller/report/key/other revision はない。

revision source はread-only、empty target/bench-work/tmpはrevision-private writable、Cargo home/toolchainは
read-only。`CARGO_TARGET_DIR`/`TMPDIR`/`ALIGN_BENCH_WORK_DIR`を固定。enabling implementationは両scriptを
absent/nonempty/unsafe work dirでrejectさせ、`kernel.o`等すべてをそこへ限定する。

benchmark-input sliceでは`ALIGN_BENCH_WORK_DIR`はrequiredで、absolute existing directoryを指しfinal componentは
symlink不可。physical pathは`/`、repository root、repository内を不可とし、hidden entryを含め開始時empty。
各scriptは`umask 077`でexact one private childを作り、root/detached Cargo target、`TMPDIR`、kernel object、
全configured build outputをchild内へ限定する。prepare successはsealed childをretainする。error/signal/interruptは
retained descriptor配下のnon-directory entryだけをrecursive unlinkし、directory-only skeletonはcontainer/process
teardown後にtrusted callerがremove。
relative/missing/non-directory/final-symlink/root/repository/in-repository/initially-nonempty/cleanup-failure/
foreign-residueはforeign entryを削除せずrejectし、script-level failure後はcaller-owned directoryとdirectory-only
owned treeを残す。outer controllerがcandidate container終了後にrace-freeでremoveする。
repeated trailing separatorと`/.`でfinal-component symlinkを隠せない。各build/compiler/harness commandは
own process groupで実行し、interrupt時はcomplete groupへbounded TERM/KILL escalationを行い、private file
remove前にdirect childをreapする。

scriptはuntrusted build開始前にprivate childをopenして保持する。accepted Linux pathではouter controllerの
read-only-root containerとcontroller-owned writable mountがarbitrary candidate build writeをconfineし、script単体を
sandboxとはしない。configured Cargo/compiler outputはretained child配下、trusted post-build copy/chmod/manifest/
cleanup mutationはすべてdescriptor-relativeであり、`prepared`のrename/replacementでcaller dataへpublicationを
redirectできない。controller-owned writable mount内のcandidate escapeはforeign residueとなりrejectする。
cleanupはretained descriptor配下のnon-directory entryだけをunlinkし、
candidate-side writerとraceし得る間はdirectory entryをdeleteしない。success時のempty build-directory skeletonも
prepared manifestに含め、measurementとcandidate teardown後にouter controllerがremoveする。macOSはnative ARM
development qualificationのままでaccepted adversarial evidenceには使わない。

baseline選択前に両protected scriptをclosed two-phase interfaceにする。`run.sh prepare native`が
root/detached Cargo buildと`alignc emit-obj`を行い、compiler/runtime/benchmark executable/kernel object/
effective configのcanonical SHA-256/mode manifestをrevision-private work dirに作る。`run.sh native`は
Cargo/compiler workを行わない。prepareはpublish前にprivate childのcaptured device/inode identityを再検証する。
nativeはfinal componentをfollowせずprepared rootをopenし、retained descriptorとcaptured device/inodeを
launcherへ渡す。launcherは一致を要求し、そのdescriptor配下でmanifestをverifyする。prepareはcanonical manifest
SHA-256もprintし、trusted controllerがcandidate-writable state外で保持して、later native invocationごとにrequired
`ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256`として渡す。launcherはcurrent self-consistent treeのmanifestがその
prepare-time digestと異なればartifact open前にrejectする。accepted Linux x86_64 pathは
executable/runtimeをhashしながらanonymous `memfd`へcopyし、source metadataをcopy前後で確認し、
Linux UAPI `MFD_EXEC` capabilityを要求して
`F_SEAL_WRITE|F_SEAL_GROW|F_SEAL_SHRINK|F_SEAL_SEAL`を適用しsealed descriptorだけをdirect exec/preloadする。
executable memfd非対応kernel（enforced `vm.memfd_noexec=2`を含む）はpath/unsealed fileへfallbackせず
qualificationをreject。macOS pathはaccepted evidenceではなくnative ARM development qualificationのままで、
repeated launchを安定させるため`DYLD_SHARED_REGION=private`を固定する。両platformともexec直前に
descriptor-bound prepared tree全体をretained manifest/digestに対して再検証する。
missing/extra/changed/wrong-mode/replaced/unsealable artifactとprepare-only selectorをreject。argv arrayでshell interpolationなし。prepare Cargoはすべて
`--locked --offline`。cache/source/config manifestをbefore/after比較しwriteをreject。benchmark-input
sliceは各scriptのroot build 2回とdetached `cargo run` 1回、合計current 6 Cargo invocationをlockする。
evidence implementationがbaseline前に2つの`cargo run`をprepare/direct-execへ置換する。

candidate-controlled Cargo/compiler work中はfinal artifactを作らない。全child process group終了後に
descriptor-relative helperが各fixed outputをno-follow/nonblocking openし、read前にnon-regular fileをreject、source
stabilityを確認してcomplete final artifact setを作る。buildはdeclared outputを生成できるが、manifest capture前の
published runtime/kernel/compiler/harnessを書き換えられない。

child前にCLOEXEC pipeを作りfd enumerate、stdin/stdout/stderrだけを渡す。dup後番号確認、inherited range close、
entrypointも`/proc/self/fd`確認。collision/inheritance/missing CLOEXEC/mapping changeはreject。bounded outputは
SHA-256/escaped tailで記録しtimeout/truncation/nonzero後はparseしない。

failure/interrupt時はcontainer/process group kill/reap/remove、fd close、mount/private dir remove、残存なしを
確認。cleanup failureはvalid reportも抑止。partial evidenceはpublishしない。

## Workload/measurement

controllerはdecodeのbaseline/candidate、次にsoaのbaseline/candidateに`run.sh prepare native`を実行し、
4 artifact manifestをwarm-up前にverify。prepare childはnon-overlap、measurementと別にmonitor/reportする。

baseline選択前のevidence implementationが既存timed operation/round countを維持しinner minimumを
arithmetic medianに変更。40/30はchecked integer nanosecondをsortしexact `middle_sum_ns`を保持。
output microsecondsはchecked `(middle_sum_ns + 1000) / 2000`（nearest microsecond、exact halfはup）。
`us / 1000`とzero-padded `us % 1000`でexisting ms tokenへし、float/二次roundingなし。synthetic ownerが
odd/even sum、half-unit tie、overflow、equal middle、low outlier、tokenを固定。timed invocation変更なし。

prepare後、warm-up/sample childは次だけを実行。

```text
bench/json_decode/run.sh native
bench/json_soa/run.sh native
```

prepared native target/profile CPU一致を要求し、downgrade/cross/emulation/effective config change、
measurement中build、artifact manifest changeをreject。
benchmarkごとrevision各1 discarded warm-up、その後10 pair。odd B-C、even C-B、overlapなし。failed attemptを
置換せず、再試行はnew target/outputからrun全体をやり直す。

`ALIGN_BENCH_PROFILE`はunset。fixed `target: native`、title、header、ordered row `10000`,`100000`,`1000000`
のみを許す。headerはexact:

```text
json_decode: records, json KB, A-full, rs-full, full×, A-proj, rs-proj, proj×
json_soa:    records, json KB, soa ms, aos ms, proj ms, rust ms, soa/rust, aos/rust, proj/rustP
```

全column grammarを検証しmillion rowの`A-full`,`A-proj`,`soa ms`,`aos ms`,`proj ms`のみ保持。duplicate/
missing/extra/wrong order/whitespace/sign/exponent/nonfinite/ratio/profile outputはreject。tokenはquantized inner medianの
positive ASCII 3-decimalで、roundingなしにinteger microsecondsへ変換。10値をsortしmedianをmiddle sum/2で保持。

```text
candidate_middle_sum * 100 <= baseline_middle_sum * 105
```

5 fieldすべてpass。zero baseline/overflow/missing/parser warningはreject。harness parity assertionはcorrectness
ownerの代替ではない。

## Canonical report/signature

complete fileは次のouter recordとLF exactly one。

```text
Report = {"body":Body,"body_sha256":hex64}\n
```

string外whitespaceなし。member orderは下記宣言順。arrayもorder/cardinality固定。`u32/u64`はunsigned decimal
（0以外leading zeroなし、overflow reject）、`bool`はlowercase、`hex40/64`はそれぞれ40/64桁の
lowercase hex。`token=[0-9]+\.[0-9]{3}`、`time=YYYY-MM-DDTHH:MM:SS.NNNNNNNNNZ`、その他はclosed
enumまたは`name=[A-Za-z0-9._/:+=@-]{1,255}`。arbitrary bytes/Git pathはlowercase even hex。quote/backslash/control/
non-ASCII/invalid UTF-8/overlong/unknown/missing/duplicate/reordered/wrong cardinality/float/negative/null/trailingをreject。

以下が全member名/order/type/presence。`[]`だけunbounded、他cardinality exact。

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
CommitIdentity = {"oid":hex40,"raw_sha256":hex64,"tree_oid":hex40,"parents":[hex40]}
PathIdentity = {"path_hex":bytes,"mode":GitMode,"kind":PathKind,"oid":hex40,
  "size":u64,"sha256":hex64}
PathChange = {"path_hex":bytes,"status":ChangeStatus,"old":PathSide,"new":PathSide}
PathSide = {"presence":Presence,"mode":GitModeOrEmpty,"kind":PathKindOrEmpty,
  "oid":OidOrEmpty,"size":u64,"sha256":DigestOrEmpty}
TargetBinding = {"local_ref":"refs/heads/main","run_oid":hex40,
  "expected_merge_base":hex40,"expected_merge_head":hex40,"expected_merge_tree":hex40}
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

baseline commit/path listはempty、candidate commitはbaseline後のnonempty first-parent順。`changed_paths`は
baseline/candidate treeのexact path-hex-ordered union diff。addedはabsent/present、deletedはpresent/absent、
modifiedはpresent/presentでmode/kind/OID/size/SHAの少なくとも1つが異なる。absent sideはempty
mode/kind/OID/digestとsize zero、present sideはemptyなし。protected entryはpath-hex順でmanifest equal。

HostObservationはdense。全`Preparation`/`Run`はglobally unique `child_id`とnonempty inclusive rangeを持ち、
first/lastは同IDのchild-start/end、interior child-sampleも同ID。rangeはglobal sequence順にstrictly
increasing/disjointで、nonempty-child observation全体のexact partition。reuse/orphanなし。non-child phaseはempty ID。
benchmark順はdecode/soa、各preparation B/C、warmup B/C、pair 1..10 balanced order。artifact manifestは
同benchmark/revisionの全runで再verify。sample fieldは2/3順、field resultは5順。original sampleはpair順、
sortedはnondecreasing exact permutation。ratio numerator/denominatorはcandidate/baseline middle sum。

cleanup countはzero、bool true。host lock保持中にsign/private staging fsync後、profile-fixed root-owned
publication reservationをno-follow exclusive create・fsync。unlock後atomic rename・output parent fsync・reservation
remove・reservation directory fsync後だけpathをprint。reservation存在中のlater invocationはrepository前reject。
failureはstateを削除し、surviving reservationはoutputをunacceptedとしadmin recoveryまでfuture runをblock。
reservation removalはalready-signed measurement cleanup外のpublication postconditionで、成功までaccepted pathなし。
`first_failed_field`はpassでempty、regressionで最初の
false field。conditional/omitted memberなし。

`clean` reviewは`review_head=candidate`、`repair_commits=[]`。`fixed`はreviewed ancestorを
`review_head`とし、その後candidateまでのnonempty exact first-parent順を`repair_commits`に記録する。
各commitはcandidate `commits`に同順で含まれる。どちらも`review_base=baseline`。
literalはrepository preflight stamp/PR markerとexact一致。log `CLEAN`は`clean`のみ、`FINDINGS`+
nonempty accepted repair chainは`fixed`のみにmap。
`ToolIdentity.source_manifest_blob`は`source_commit`内のcanonical installation manifest blob、
`source_manifest_sha256`はそのbytesのhashで、manifestがtoolの全installed fileを固定する。

`body_sha256`のpreimageはexact 45 ASCII bytes
`align-json-escape-benchmark-evidence-body-v1\0` + LFなしcanonical `Body`。recursiveではない。signatureは
final LF込みcomplete outer recordをnamespace `align-json-escape-benchmark-evidence-v1`で署名。

各`.sig`はpinned OpenSSH `SSHSIG` v1 ASCII armorでraw Ed25519ではない。exact header
`-----BEGIN SSH SIGNATURE-----\n`、binary SSHSIGのRFC 4648 standard base64（70 ASCII/line、最終は
1..70、required `=` padding保持）、各line LF、exact footer `-----END SSH SIGNATURE-----\n`。CR/space/
blank/noncanonical padding/wrap/trailingをreject。SSH stringはu32 big-endian length。binaryはmagic `SSHSIG`、
version 1、profile Ed25519 public-key blob、exact namespace、empty reserved、`sha512`、そして
`ssh-ed25519` + 64-byte signature。

signing preimageはexact magic `SSHSIG`、SSH string namespace、empty reserved、SSH string `sha512`、complete
canonical message bytesの64-byte SHA-512を持つSSH string。report/merge namespaceはそれぞれ
`align-json-escape-benchmark-evidence-v1` / `align-json-escape-benchmark-merge-verification-v1`。
armor/binary field/lengthをdecode・canonical re-encode equality後のみpinned `ssh-keygen -Y sign/verify`。
signature SHA-256はfinal LF込みcomplete armorをhash。

producerはBody encode→domain hash→Report encode→duplicate-reject parse/re-encode equality後にsign。verifierは
schema/canonical/body digest/relationship/sample/median/comparison/expected OID/PR/identity/key/signatureを再計算。
complete minimal-pass/first-regression semantic fixture、exact one-line bytes、test signature、SHA-256をcheck inし、
independent reference encoderを使う。全member/enum/width/cardinality/order/presence/duplicate/string/LF/domain/digest/
signature/derived field mutationを所有する。

post-merge verificationはexactに次のrecord + one LF。

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

Reportと同じscalar/string/order/whitespace/UTF-8/rejection grammar。detached signatureはfinal LF込みを
namespace `align-json-escape-benchmark-merge-verification-v1`で署名。report/signature hashはcomplete bytes。
`merge_oid=MERGE`、`target_oid`はfetched target tipでmergeをfirst-parent chainに含み、parentsはbaseline/
candidate、treeはreportのexpected tree。全relationship後に
raw merge-object SHA-256を記録。golden/mutation/wrong parent/tree/ref/stale/swap ownerを持つ。
同host lockをinspection前に取得し、private stage→file/dir fsync→durable publication reservation→unlock→
atomic rename→parent fsync→reservation remove/directory fsync→path publicationの同sequenceを使う。
surviving reservationがあればoutputはunacceptedでlater workをblock。

accepted PRはcomplete report/signatureをimmutable artifactまたはbase64-safe attachmentで保持しverifier結果を
記録。merge後、trusted hostがmerge-verification record/signatureをimmutable artifactに保存しPRからlink。
それまでlifecycle/dependent workを進めない。1 byte変更でinvalid。private keyはcontainer cleanup後
non-inheritable fdでのみopen。sign failureはoutputなし。

## Review/base drift/integration

coherent candidateのcomprehensive review（exact CLEANまたはone findings-fixed chain）後にmeasurement。以後の
commit/amend/rebase/merge/protected changeはreport無効。publication時preflight/trusted review/PR markerをcandidate/base/
reviewへ照合。merge直前target OIDは`BASE`必須。違えばnew tipへrebaseしnew baseline/review/evidence/preflight。
mergeはexpected headをbind。provider response exact OIDをfetchし`verify-merge`。mergeがfetched targetのfirst-parent
chain上、parents BASE/CANDIDATE、tree candidate-identicalのときだけsigned artifact。artifact store/link後、
trusted lifecycle adapterがtargetを再fetchしreachabilityを再verify後のみadvance。normal descendantはvalid、
MERGEを除くforce-push/replacement/movementはartifact/lifecycleをinvalidate。unavailable/fetch failureはunshipped。

current hosting transactionはexpected base OIDをatomicに受けないため、implementationはdisposable remoteで
fail-closed merge/revert ownerを証明するか、endpoint/principal/repo/expected base+head/request/response/secretを
bindするprovider CASをreviewed amendmentで導入する。base ruleを弱めない。

## Delivery order

1. detached lock check-in、ignore削除、current 6 Cargo locked/offline、output confinement、missing/stale/cache/network/write/
   cleanup ownerのbenchmark-input sliceをmerge。
2. installed controller/verifier/monitor、profile/image recipe+digest/public key、inner-median harness、host guide、
   adversarial fixture、format golden、merge-race ownerをmerge。performance claimなし。
3. private key provision、image/cache/toolchain/profile independent verify、host self-qualification。
4. current target tipを`BASE`としてその直上にRequest 7 branchを作る。
5. correctness→review→evidence→PR→target freeze→merge object verify→lifecycle advance。

## Implementation closure matrix

最初のadversarial-owner implementationは、process boundary、exact schedule、
cleanup/publication ordering、exclusive-run reservationを1つのcapabilityに意図的にまとめる。
deterministic ownerを含むhand-written implementationは1,000行を少し超えるが、この4つのedgeは
1つのdormant producer-to-consumer chainを形成するためである。分割するとfailure-order fixtureが
重複し、controllerが未review boundaryをconsumeする。controller、verifier、merge-race behaviorは
このsliceに含めない。

| Cell | Owner / exact regression |
|---|---|
| Trusted bootstrap/CLI | installed producer/verifier/merge-verifier/monitorをmanifest/baseline blobへ照合。candidate/PATH/PYTHONPATH/CWD substitution、replacement、引数/path/OID/output/ambient異常をmutation前にreject。trusted CIがexpected OID/PRとfixture-owned GitHub API responseから作ったcanonical review attestationを供給し、wrong repository/PR/reviewer role/review ID/commit/state/body digest、author review、dismiss/stale/duplicate、API/file substitutionをcandidate content読取前にreject。`benchmark_evidence_bootstrap_cli_matrix`. |
| Raw identity/construction | clone/worktree、packed/loose、reviewed symlink、hostile Git state、missing/raw swap/path collision/mutation race。`benchmark_evidence_raw_object_matrix`. |
| Revision binding | exact parent chain/two-sided add-delete-modify diff、wrong target/ancestry/merge/ref/stale review/drift、missing deletion、wrong old/new mode/type、incomplete tree union。`benchmark_evidence_revision_binding_matrix`. |
| Protected input | required/optional config/manifest/lock/script/kernel/harness/generator/output/timing、evidence profile/public key、installed-source manifest、controller/verifier/monitor source、evidence owner testのpresence/path/type/mode/bytes mutationをcandidate前reject。`benchmark_evidence_protected_input_matrix`. |
| Toolchain/cache/offline | image/tool/cache/config、swap、lock/cache/network/write。`benchmark_evidence_toolchain_matrix`. |
| Native host/isolation | native x86_64 success、ARM/emulation/mismatch/quota/exposure reject。child内latched event/monitor loss、duplicate/reused/overlap range、wrong child ID、orphan observationをreject。`benchmark_evidence_host_isolation_matrix`. |
| Descriptor/environment | 全fd collision/CLOEXEC/mapping/env/truncation/capture。`benchmark_evidence_process_boundary_matrix`. |
| Schedule | 4 exact prepare + sealed artifactの後にwarmup/pair。measurement中build/Cargo/compiler、artifact mutation、overlap/reorder/retry/skip/crash/timeout/signal/nonzero。`benchmark_evidence_schedule_matrix`. |
| Inner/outer statistic | synthetic odd/even ns sum、half-us tie、middle/outlier/overflow、round-half-up quantization/rendering、10 exact token、1.05 boundary。`benchmark_evidence_statistic_matrix`. |
| Parser/arithmetic | exact line/row/fieldと全malformed。`benchmark_evidence_parser_ratio_matrix`. |
| Report/signature | report/merge-verification bidirectional goldenと全field/order/type/width/duplicate/escape/trailing/derived mutation、exact SSHSIG binary/preimage、armor header/footer/LF/base64/wrap/padding、wrong key/namespace/profile/stale。`benchmark_evidence_report_v1_matrix`. |
| Failure/cleanup | benchmark-input ownerはabsent/relative/missing/non-directory/final-symlink（repeated separatorと`/.` aliasを含む）/root/repository/in-repository/nonempty work root、foreign residue、error/signal後のdescriptor-relative file/symlink clearing、directory/caller entry非削除、TERM-ignoring descendant非残存をcover。script-level failureはdirectory-only owned treeのみを残し、trusted outer controllerがcontainer/process teardown後にremove。evidence ownerは全phase error/timeout/signal/disk/file+dir+parent+reservation fsync/unlock/rename/reservation remove/ordinary remove/signをcover。accepted pathなし。全resourceが消えるか、surviving fail-closed reservationがacceptとlater workを阻止。`benchmark_input_workdir_matrix`; `benchmark_evidence_cleanup_matrix`. |
| Concurrent | lockまたはpublication reservation中のsecond runはGit/image/container前fail。lockはmeasurement cleanupとdurable reservation後にreleaseし、accepted publishはreservationをremove+fsync。crash/restart/admin recoveryもfail-closed。`benchmark_evidence_exclusive_run`. |
| TOCTOU | untrusted work前にpreparation rootをretainし、accepted Linux writeはすべてdescriptor配下。manifest digestはtrusted controller stateでprepare/native phaseを越え、root descriptorはshell/launcher boundaryを越える。manifest traversalもその配下で、executable/runtimeはexec/preload前にfully write-sealed anonymous descriptorへcopy。cleanupのrecursive mutationもretained treeのみ。image/sourceはopened identityを使用またはprivilege boundaryで再検証。root rename/replacement、same-inode artifact write、daemon-image/source swapをreject。`benchmark_input_workdir_matrix`; `benchmark_evidence_bound_object_swap_matrix`. |
| Forged/stale | unsigned/edit/replay/truncate/concat/wrong namespace、PR/preflight/trusted-review mismatch。`CLEAN`は`clean`のみ、accepted `FINDINGS` + nonempty repair chainは`fixed`のみにmap。`benchmark_evidence_stale_forged_matrix`. |
| Base/integration race | target move、precheck race、unavailable/wrong response OID、local-target mismatch、wrong parent/tree、signed artifact mutation、normal later first-parent descendant、final trusted refetch前のforce-push/removal、revert failure、exact merge。final target first-parent chainにmergeが残らなければlifecycleを進めない。`benchmark_evidence_merge_race_matrix`. |

testsはfixture executor/fake daemon/disposable repo+remote/test keyを使いperformance workload/wall-clock ratioを
ordinary testにしない。native host qualification/final measurementはmanual named evidence。

## Design-review finding closure

| Finding | Closure |
|---|---|
| candidate-selected producer/verifier | root-owned installed program、baseline blob equality、trusted CI expected binding。 |
| incomplete report v1 | complete ordered schema/scalar/cardinality/presence/golden/mutationを固定。 |
| recursive digest | Body外field、45-byte domain + canonical Bodyをexact preimage化。 |
| child中host event見逃し | continuous/latched root monitor + 100 ms snapshot、lossもreject。 |
| inner minimum bias | operation/round維持のchecked-integer inner medianとoutlier owner。 |
| mirror/index欠落 | 本mirrorと両indexを同じrepairに含め、Englishをauthoritativeとする。 |

## Final-review redesign closure

| Finding | Closure |
|---|---|
| inner medianはtokenにexact representableでない | exact middle ns sumからchecked round-half-upで1回だけinteger usへ変換後、3-decimal render。 |
| measured invocationが毎回rebuild | baseline前にfixed prepare/direct-exec interface。4 monitored prepareでartifact seal後、measurementはbuild/driftをreject。 |
| deletion/old-side pathが表現不能 | `PathChange`がexplicit presence/status/old/newを持つexact two-tree union diff。 |
| monitor rangeをreuse可能 | preparation/runごとunique child ID、全child observationのstrict disjoint ordered partition。 |
| verifierがactual mergeを観測不能 | installed `verify-merge`がfetched response OIDを読み、report/target/raw merge/parents/treeにbindしたsecond signed artifactを出す。 |
| unlock failureがaccepted-looking outputを残す | lock中はprivate staging、unlock後だけatomic publish、unlock/publication failureはunpublished stateを削除。 |
| Japaneseにundefined `hex128` | 両languageはused `hex40/64`のみでschemaも同一。 |

## Stable-candidate review closure

| Finding | Closure |
|---|---|
| caller-owned review logがtrusted reviewを偽造可能 | token-isolated trusted-base adapterがGitHub review APIをqueryし、repository/PR/reviewer role/review ID/commit/state/exact log digestにbindしたcanonical attestationを供給。fixture API substitutionで全rejectをowner。 |
| evidence profile/controller sourceがprotected input外 | profile/public key、installed-source manifest、controller/verifier/monitor source、adversarial ownerをprotected setへ追加。変更はbaseline選択前のseparate reviewed profile requalificationが必要。 |
| merge verify後にtargetが移動可能 | signed artifactはexact mergeをfirst-parent chainに含むfetched target tipを記録し、durable store後にtrusted lifecycle adapterがrefetchしてreachabilityを再確認。 |
| unlock-before-publicationでsecond runが開始可能 | lock中にdurable profile-global reservationを作りatomic publication完了まで保持。later invocationはrepository/image/container前rejectし、crash/cleanup failureもfail-closed。 |
| report review-state literalがrepository metadataと不一致 | canonical literalを`clean`/`fixed`に統一し、`CLEAN`とaccepted `FINDINGS` + nonempty repair chainからのone-way mappingを明記。 |
| detached signature bytesが曖昧 | exact OpenSSH SSHSIG v1 binary record、SHA-512 signing preimage、70-column canonical ASCII armor、final LF、byte-identical decode/re-encodeを固定。 |

## Integrated-candidate review closure

| Finding | Closure |
|---|---|
| retained prepared pathをverification後にreplace可能 | prepareはsuccess前にcaptured private-child device/inodeを再検証。nativeはcaptured identityをlauncherへ渡し、opened root descriptorとの一致を要求し、manifest traversalをそのdescriptor配下で実行。 |
| hash後のsame-inode writeでbytesを変更可能 | accepted Linux pathはexecutable/runtimeをhashしながらanonymous memfdへcopyし、source metadataをcopy前後で確認、4つのwrite/size/seal sealを適用後sealed copyだけをexec/preload。deterministic ownerはshell/launcher handoffでrootをreplaceし、source変更後もsealed copyが不変かつwrite不能と確認。 |

## Final-integration review closure

| Finding | Closure |
|---|---|
| phase間でself-consistent treeがprepared stateをreplace可能 | prepareがcanonical manifest SHA-256をprint。trusted controllerがcandidate-writable state外で保持し、全native invocationがrequired inputとして受け、descriptor-relative verificationがartifact open前にdifferent current manifestをreject。 |
| untrusted prepare childがlater path writeをredirect可能 | accepted controllerがread-only-root containerとcontroller-owned writable mountでarbitrary candidate writeをconfine。scriptはfirst build前にprivate-child descriptorをretainし、configured outputを配下に置き、全trusted post-build mutationをdescriptor-relativeにし、publicationはpublic pathのsame device/inodeを要求。script-only native ARM macOSはtrusted-checkout development qualificationのみ。 |
| cleanup check/path raceがreplacementをrecursive delete可能 | recursive cleanupはretained private-child descriptorだけをwalkし、non-directory entryだけをunlink。candidate-side concurrency中は全directory entryを残し、script-level failureのdirectory-only owned treeをteardown後のtrusted outer cleanupへ渡す。 |

## Final-candidate review closure

| Finding | Closure |
|---|---|
| Linux proc-fd rootをpath-based manifest creationがreject | manifest moduleがalready-opened root descriptor配下でcanonical manifestをbuild/create/fsync/verify。prepareはdescriptor APIのみをcall。 |
| intermediate symlinkがtrusted post-build copyをredirect可能 | untrusted work前にrootと`artifacts`両descriptorをretain。dedicated helperがsource全componentをno-follow openし、destinationをretained artifacts descriptor相対でcreate、source stabilityを確認、build-treeのnon-directory entryをdescriptor-relative clearし、manifest前にpublic `artifacts` entryがretained objectと同一と検証。 |
| check後`rmdir`がempty replacementをremove可能 | script cleanupは`rmdir`を一切callしない。retained descriptor配下のnon-directory entryだけをunlinkし、success manifestにempty build-directory skeletonを含め、directory-only treeをcandidate teardown後のouter controllerへ渡す。 |
| direct executionがambient loader/timing stateをinherit | launcherはledgerのfixed base environmentをconstructし、platform-owned runtime bindingとmacOSのfixed `DYLD_SHARED_REGION=private` policyだけを追加。ownerはambient sentinelをinjectし、harnessが全fixed valueとsentinel absenceをassert。 |
| nested cleanupにもdirectory-entry raceが残存 | candidate-side prepare/cleanupは全directory entryをremoveしない。success/failure ownerがretained directory skeletonをrequireし、outer controllerだけがteardown後にremove。 |
| top-level benchmark workflowがone-phase executionのまま | `bench/README.md`はprepare-time digest captureからdirect native executionまでを示し、process/environment teardown後のwork-tree removalをcaller ownershipにする。 |
| build pathだけではarbitrary candidate writeをsandboxできない | script contractはcandidate code自体をsandboxすると主張しない。accepted evidenceはlater outer-controller container boundaryが必須。本sliceはconfigured outputと全trusted publication mutationをconfineし、script-only ARM qualificationはcheckoutをtrustする。 |
| candidate workがpublished final artifactを書換可能 | 全build/compiler child group終了後にだけfinal artifactを作る。helperがfixed compiler/runtime/kernel/harness outputをdescriptor-relative copyし、manifest captureする。 |
| FIFO outputがtrusted copyをblock可能 | sourceはnonblocking/no-follow descriptorでopenし、read前に全non-regular outputをreject。ownerはfixed runtime outputにFIFOを置きalarm内のrejectを要求する。 |
| candidate-controlled Cargo wrapper dependencyがprotectedでない | exact protected-input setに`scripts/cargo.sh`とsourceされる`scripts/dyld-env.sh`を含め、preparationがreviewed evidence profileと独立にCargo/LLVM/linker選択を変更できないようにする。 |
| manifest publicationがretained artifacts directoryを再bindしない | manifest publicationはrootの`artifacts` entryがretained descriptorと同一であることをmanifest create/verifyの前後で検証し、captured manifest subtreeをretained descriptorからのfresh walkとbyte-for-byte比較。persistent/transient replacementはsame retained artifactを記述しない限りreject。 |
| bound executionがFIFO replacementのopenでblock可能 | Linux sealed-copyとnative ARM qualificationの両openをnonblocking/no-followにし、read前にnon-regular descriptorをreject。 |
| bound executionが追加のself-consistent artifactをaccept | launcherはmanifestの`artifacts` subtreeをcompiler/runtime/kernel/selected harness/directory entryのexact setに限定し、extra entryをexecution前にreject。 |
| sealed Linux artifactが明示的にexecutableでない | launcherはsealingとともにstable Linux UAPI `MFD_EXEC` flagを常に要求。unsupported kernelとenforced `vm.memfd_noexec=2`等のpolicyはhost qualificationでfail-closedし、mutable path bytesへfallbackしない。 |
| initial verification後にnon-selected prepared fileを変更可能 | executable/runtime bind後、launcherがexec直前にdescriptor-bound prepared tree全体とretained manifest digestを再検証。deterministic ownerはbinding中にeffective configurationを変更し、executionへ到達しないことを証明。 |
| macOS qualificationにrepositoryのshared-cache isolationがない | native ARM launcherはambient loader stateをinheritせず`DYLD_SHARED_REGION=private`を固定し、executed harnessがexact valueをassert。 |
| macOS verificationがspecial fileのtype reject前にread | launcherはhash前にinitial descriptor modeを検査。FIFO ownerは`os.read`をrejecting sentinelへ置換し、両platform openerがreadなしでrejectすることを証明。 |

## Author consistency pass

- ledger/threat/workload/report/delivery/matrixはone baseline/candidate/profile/controller/verifier/image/host/
  protected set/schedule/parser/threshold/failure ruleで一致。
- exchanged formatはcanonical report v1とpost-merge verification v1、各detached signatureのみ。field owner/
  malformed/identityがありfloat/ambient defaultなし。
- provider credentialはN/A。secretはcandidate container外のhost signing keyのみ。
- language/API/ABI/ownership changeはN/A。developer evidence toolingのみ。
- correctnessはRequest 7 compiler/runtime owner、controllerはrequired performance comparisonのみをaccept。
