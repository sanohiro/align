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
| Public verifier | `/opt/align-evidence/v1/bin/align-json-escape-evidence verify --report REPORT --signature SIGNATURE --expected-baseline BASE --expected-candidate CANDIDATE --pr-body PR_BODY`。bytes/signature と report/review/preflight binding を明示OID/PR bodyへ照合する。local は diagnostic のみ。acceptance は trusted-base CI が GitHub event 由来の `BASE`/`CANDIDATE`/`PR_BODY` を渡す。同じ installed verifier は build/checkout/benchmark/network/repository mutation を行わない。 |
| Ambient state | semantic default なし。empty environment に fixed `PATH`, `LC_ALL=C`, `TZ=UTC`, empty `HOME`, `CARGO_NET_OFFLINE=true` と controller-created descriptor/config だけを置く。ambient Git/Cargo/Rust/Docker/locale/proxy/credential/target/tuning は渡さない。 |
| Result | success は `NEW_DIR` に `report.json`/`report.json.sig` のみを atomic create、両方と directory を fsync、exclusive lock を release、absolute path を出力し zero exit。threshold failure も同じ durability/lock-release 後に signed `regression` report を作り exit 1。他の construction/identity/isolation/execution/parsing/timeout/cleanup/signing/publication failure は staging を削除し `NEW_DIR` を残さず accepted path を出さない。 |
| Controller owner | checked-in Python 3 controller、root-owned installed launcher、`scripts/` と `tests/benchmark_evidence/` の fixture tests。installed/source relation、installer manifest、interpreter、Git、Docker client/daemon、`ssh-keygen`、kernel、OCI image、host profile、executable SHA-256 をすべて記録する。candidate file は evidence root にならない。 |
| Persisted format | canonical UTF-8 JSON `align.json_escape_benchmark_evidence/v1` を byte-for-byte、host Ed25519 key と `ssh-keygen -Y sign` namespace `align-json-escape-benchmark-evidence-v1` で署名。unknown/missing/duplicate/reordered/non-ASCII key、non-integer/float、invalid UTF-8/escape、trailing/noncanonical bytes は reject。 |
| Ownership/allocation | controller が temp dir、pipe、child、container、capture、report staging、cleanup を所有。benchmark child は stdin `/dev/null`、private stdout/stderr のみ。capture ceiling は profile 固定。report/signature/repository administration/controller/Docker socket/signing key/other revision writable dir は child に渡さない。 |
| Concurrency | repository inspection 前に profile の host-global exclusive lock を取得し、signing/cleanup まで保持。baseline/candidate は overlap しない。fixed pair order だけが schedule。 |
| Prerequisites | benchmark-input slice、Request 7 の両 language prerequisite、本 design、evidence implementation、pinned image、host profile/public key、adversarial owner が merge 後にのみ `BASE` を選ぶ。`BASE` は当時の target tip かつ最初の Request 7 commit の exact parent。 |
| Acceptance | valid signature、`pass` verdict、exact PR/preflight/review binding、merge 時の unchanged target base、identical protected input、5 field 各10 sample、全 exact ratio `<=1.05` が必要。correctness test は別の deterministic owner。 |

`REVIEW_LOG` は candidate repository 外の標準 pre-open review log。`CLEAN` なら candidate へ直接、
`FINDINGS` なら reviewed ancestor と `CANDIDATE` までの complete findings-fixed chain に bind する。
controller は log SHA-256/binding を記録するが prose の authenticity は主張しない。trusted-base PR
attestation も同じ candidate、merge base、review head/reviewer/state を持つ必要がある。

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
```

presence/path/mode/type/blob/manifest equalityをcandidate実行前に要求し、script/kernel/harness/manifest/lock/
generator/output/timing/configすべてを含む。

## Container/process boundary

各build/benchmarkはtrusted controllerが新規containerで実行。profile image、`--pull=never`、
`--network=none`、read-only root、cap drop、`no-new-privileges`、fixed uid/gid/seccomp/LSM/CPU/NUMA/
memory/swap/pid/file/fd limit、minimum device、private namespace/tmp。Docker socket、host `/proc`、home、
agent/credential/repo admin/controller/report/key/other revision はない。

revision source はread-only、empty target/bench-work/tmpはrevision-private writable、Cargo home/toolchainは
read-only。`CARGO_TARGET_DIR`/`TMPDIR`/`ALIGN_BENCH_WORK_DIR`を固定。enabling implementationは両scriptを
absent/nonempty/unsafe work dirでrejectさせ、`kernel.o`等すべてをそこへ限定する。

argv arrayでshell interpolationなし。root/detached Cargoはすべて`--locked --offline`、
`CARGO_NET_OFFLINE=true`はdefense in depth。cache/source/config manifestをbefore/after比較しwriteをreject。
targetはrevisionごとにempty startし、そのwarm-up/sample間だけ保持する。

child前にCLOEXEC pipeを作りfd enumerate、stdin/stdout/stderrだけを渡す。dup後番号確認、inherited range close、
entrypointも`/proc/self/fd`確認。collision/inheritance/missing CLOEXEC/mapping changeはreject。bounded outputは
SHA-256/escaped tailで記録しtimeout/truncation/nonzero後はparseしない。

failure/interrupt時はcontainer/process group kill/reap/remove、fd close、mount/private dir remove、残存なしを
確認。cleanup failureはvalid reportも抑止。partial evidenceはpublishしない。

## Workload/measurement

両revision build後、baseline選択前のevidence implementationが既存timed operation/round countを維持したまま
inner minimumをarithmetic medianに変更する。40/30のeven countはsorted nanosecondの中央2値の平均をchecked
integerで求め、既存3-decimal ms tokenへ1回だけ変換。low outlierを選ばずfloat medianなし。synthetic duration
ownerがextreme low outlier、middle pair、renderingを固定。timed invocationの追加/削除/reorderなし。`BASE`前に
identical protected inputとなる。

controllerは次だけを実行する。

```text
bench/json_decode/run.sh native
bench/json_soa/run.sh native
```

native target/profile CPU一致を要求し、downgrade/cross/emulation/effective config changeをreject。
benchmarkごとrevision各1 discarded warm-up、その後10 pair。odd B-C、even C-B、overlapなし。failed attemptを
置換せず、再試行はnew target/outputからrun全体をやり直す。

`ALIGN_BENCH_PROFILE`はunset。fixed `target: native`、title、header、ordered row `10000`,`100000`,`1000000`
のみを許す。headerはexact:

```text
json_decode: records, json KB, A-full, rs-full, full×, A-proj, rs-proj, proj×
json_soa:    records, json KB, soa ms, aos ms, proj ms, rust ms, soa/rust, aos/rust, proj/rustP
```

全column grammarを検証しmillion rowの`A-full`,`A-proj`,`soa ms`,`aos ms`,`proj ms`のみ保持。duplicate/
missing/extra/wrong order/whitespace/sign/exponent/nonfinite/ratio/profile outputはreject。tokenはinner medianの
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
（0以外leading zeroなし、overflow reject）、`bool`はlowercase、`hex40/64/128`はfixed lowercase hex、
`token=[0-9]+\.[0-9]{3}`、`time=YYYY-MM-DDTHH:MM:SS.NNNNNNNNNZ`、その他はclosed enumまたは
`name=[A-Za-z0-9._/:+=@-]{1,255}`。`hex40/64`はそれぞれ40/64桁の lowercase hex。arbitrary bytes/Git pathはlowercase even hex。quote/backslash/control/
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
  "changed_paths":[PathIdentity]}
CommitIdentity = {"oid":hex40,"raw_sha256":hex64,"tree_oid":hex40,"parents":[hex40]}
PathIdentity = {"path_hex":bytes,"mode":GitMode,"kind":PathKind,"oid":hex40,
  "size":u64,"sha256":hex64}
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
  "load_milli":u64,"cpu_pressure_total_us":u64,"memory_pressure_total_us":u64,
  "free_memory_bytes":u64,"swap_read_bytes":u64,"swap_write_bytes":u64,
  "throttle_events":u64,"thermal_events":u64,"foreign_schedule_events":u64,
  "foreign_container_events":u64,"monitor_lost_events":u64,
  "frequency_khz":u64,"temperature_millic":u64,"container_manifest_sha256":hex64}
BenchmarkEvidence = {"name":Benchmark,"argv":Argv,"warmups":[Run;2],"pairs":[Pair;10]}
Pair = {"ordinal":u32,"first":Run,"second":Run}
Run = {"revision":RevisionArm,"sequence":u32,"stdout_sha256":hex64,
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
ReviewState = "clean" | "findings-fixed"
Verdict = "pass" | "regression"
FieldOrEmpty = "" | Field
RevisionArm = "baseline" | "candidate"
Benchmark = "json_decode" | "json_soa"
Argv = "bench/json_decode/run.sh native" | "bench/json_soa/run.sh native"
Field = "A-full" | "A-proj" | "soa ms" | "aos ms" | "proj ms"
HostPhase = "pre-build" | "child-start" | "child-sample" | "child-end" |
  "between-children" | "post-run"
```

baseline commit/path listはempty、candidate commitはbaseline後のnonempty first-parent順、path/protected entryは
path-hex順。protected manifestはequal。HostObservationはdenseでRunはchild-startからchild-endまでのnonempty
inclusive範囲。benchmark順はdecode/soa、warmup B/C、pair 1..10 balanced order、sample fieldは2/3の上記順、
field resultは5順。original sampleはpair順、sortedはnondecreasing exact permutation。ratio numerator/denominator
はcandidate/baseline middle sum。cleanup countはzero、bool trueで署名前もhost lock保持。durable output後にlockを
releaseし、その成功後だけpathをprintする。`first_failed_field`はpassでempty、regressionで最初のfalse field。
conditional/omitted memberなし。

`clean` reviewは`review_head=candidate`、`repair_commits=[]`。`findings-fixed`はreviewed ancestorを
`review_head`とし、その後candidateまでのnonempty exact first-parent順を`repair_commits`に記録する。
各commitはcandidate `commits`に同順で含まれる。どちらも`review_base=baseline`。
`ToolIdentity.source_manifest_blob`は`source_commit`内のcanonical installation manifest blob、
`source_manifest_sha256`はそのbytesのhashで、manifestがtoolの全installed fileを固定する。

`body_sha256`のpreimageはexact 45 ASCII bytes
`align-json-escape-benchmark-evidence-body-v1\0` + LFなしcanonical `Body`。recursiveではない。signatureは
final LF込みcomplete outer recordをnamespace `align-json-escape-benchmark-evidence-v1`で署名。

producerはBody encode→domain hash→Report encode→duplicate-reject parse/re-encode equality後にsign。verifierは
schema/canonical/body digest/relationship/sample/median/comparison/expected OID/PR/identity/key/signatureを再計算。
complete minimal-pass/first-regression semantic fixture、exact one-line bytes、test signature、SHA-256をcheck inし、
independent reference encoderを使う。全member/enum/width/cardinality/order/presence/duplicate/string/LF/domain/digest/
signature/derived field mutationを所有する。

accepted PRはcomplete report/signatureをimmutable artifactまたはbase64-safe attachmentで保持しverifier結果を
記録。1 byte変更でinvalid。private keyはcontainer cleanup後non-inheritable fdでのみopen。sign failureはoutputなし。

## Review/base drift/integration

coherent candidateのcomprehensive review（exact CLEANまたはone findings-fixed chain）後にmeasurement。以後の
commit/amend/rebase/merge/protected changeはreport無効。publication時preflight/PR attestationをcandidate/base/
reviewへ照合。merge直前target OIDは`BASE`必須。違えばnew tipへrebaseしnew baseline/review/evidence/preflight。
mergeはexpected headをbindし、returned commitのfirst parent BASE、second CANDIDATE、tree candidate-identicalを
raw-object verify。race結果はunshippedでdependent work前にrevert、lifecycleを進めない。

current hosting transactionはexpected base OIDをatomicに受けないため、implementationはdisposable remoteで
fail-closed merge/revert ownerを証明するか、endpoint/principal/repo/expected base+head/request/response/secretを
bindするprovider CASをreviewed amendmentで導入する。base ruleを弱めない。

## Delivery order

1. detached lock check-in、ignore削除、4 Cargo locked/offline、output confinement、missing/stale/cache/network/write/
   cleanup ownerのbenchmark-input sliceをmerge。
2. installed controller/verifier/monitor、profile/image recipe+digest/public key、inner-median harness、host guide、
   adversarial fixture、format golden、merge-race ownerをmerge。performance claimなし。
3. private key provision、image/cache/toolchain/profile independent verify、host self-qualification。
4. current target tipを`BASE`としてその直上にRequest 7 branchを作る。
5. correctness→review→evidence→PR→target freeze→merge object verify→lifecycle advance。

## Implementation closure matrix

| Cell | Owner / exact regression |
|---|---|
| Trusted bootstrap/CLI | installed producer/verifier/monitorをmanifest/baseline blobへ照合。candidate/PATH/PYTHONPATH/CWD substitution、replacement、引数/path/OID/output/ambient異常をmutation前にreject。CIがexpected OID/PRを供給。`benchmark_evidence_bootstrap_cli_matrix`. |
| Raw identity/construction | clone/worktree、packed/loose、reviewed symlink、hostile Git state、missing/raw swap/path collision/mutation race。`benchmark_evidence_raw_object_matrix`. |
| Revision binding | exact parent chain、wrong target/ancestry/merge/ref/stale review/drift。全path/modeをscope dispositionへ出す。`benchmark_evidence_revision_binding_matrix`. |
| Protected input | required/optional config/manifest/lock/script/kernel/harness/generator/output/timingのpresence/path/type/mode/bytes mutationをcandidate前reject。`benchmark_evidence_protected_input_matrix`. |
| Toolchain/cache/offline | image/tool/cache/config、swap、lock/cache/network/write。`benchmark_evidence_toolchain_matrix`. |
| Native host/isolation | native x86_64 success、ARM/emulation/mismatch/quota/exposure reject。child内latched foreign/throttle/thermal/frequency/pressure/swap/containerとmonitor lossをreject。`benchmark_evidence_host_isolation_matrix`. |
| Descriptor/environment | 全fd collision/CLOEXEC/mapping/env/truncation/capture。`benchmark_evidence_process_boundary_matrix`. |
| Schedule | exact warmup/pair、overlap/reorder/retry/skip/crash/timeout/signal/nonzero。`benchmark_evidence_schedule_matrix`. |
| Inner/outer statistic | synthetic middle/outlier/overflow/rendering、10 exact token、1.05 boundary。`benchmark_evidence_statistic_matrix`. |
| Parser/arithmetic | exact line/row/fieldと全malformed。`benchmark_evidence_parser_ratio_matrix`. |
| Report/signature | bidirectional goldenと全field/order/type/width/duplicate/escape/trailing/derived/key/namespace/stale。`benchmark_evidence_report_v1_matrix`. |
| Failure/cleanup | 全phase error/timeout/signal/disk/fsync/remove/sign。残存なし。`benchmark_evidence_cleanup_matrix`. |
| Concurrent | second runはGit/image前fail、完全cleanup後lock release。`benchmark_evidence_exclusive_run`. |
| TOCTOU | executable/image/source rename/replacement/swap。`benchmark_evidence_bound_object_swap_matrix`. |
| Forged/stale | unsigned/edit/replay/truncate/concat/wrong namespace、PR mismatch。`benchmark_evidence_stale_forged_matrix`. |
| Base/integration race | target move、precheck race、wrong parent/tree、revert failure、exact merge。`benchmark_evidence_merge_race_matrix`. |

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

## Author consistency pass

- ledger/threat/workload/report/delivery/matrixはone baseline/candidate/profile/controller/verifier/image/host/
  protected set/schedule/parser/threshold/failure ruleで一致。
- exchanged formatはcanonical report v1 + detached signatureのみ。field owner/malformed/identityがありfloat/
  ambient defaultなし。
- provider credentialはN/A。secretはcandidate container外のhost signing keyのみ。
- language/API/ABI/ownership changeはN/A。developer evidence toolingのみ。
- correctnessはRequest 7 compiler/runtime owner、controllerはrequired performance comparisonのみをaccept。
