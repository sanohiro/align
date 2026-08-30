このディレクトリには `../std-design/` と同じ深さで `core` ライブラリ各領域の正式な
設計文書（シグネチャ、ownership/effect 分類、error policy、pitfall、test anchor）を置く。

# core.test — 言語内テスト

> [English](../test.md) · **日本語**

> **ステータス:** proposed design completed 2026-08-30、implementation pending。この文書だけでは
> parser、compiler、runner、assertion surface のいずれも出荷されない。

## Public-contract ledger

この ledger が最初の実装に対する authoritative record である。後続 prose は各 row を
説明できるが拡張してはならない。public change はまずこの表を更新し、その後に列挙した
全 source of truth へ一度に伝播する。

| Surface | Exact public record | Owner、artifact/cache identity、acceptance owner |
| --- | --- | --- |
| Declaration grammar | private top-level declaration は1つだけであり、`test`、通常の Align string token 1つ、block 1つの順で書く。`test` はこの item shape でだけ contextual になる。`pub`、parameter、type parameter、明示 return type、expression-body `=`、attribute、末尾 declaration name は reject する。declaration は callable/name binding を作らない。 | Lexer は `test` を identifier のまま保ち、AST/parser/formatter が contextual item を所有する。parser/formatter round-trip、recovery、全 reject near-shape を parameterized owner 1つで固定する。 |
| Name, identity, and catalog | decode 後の name は 1..=256 UTF-8 byte で Unicode C0/C1 control を含まない。U+0000..U+001F と U+007F..U+009F を rejectする。同一 module 内の duplicate name は reject。canonical public id は `<canonical-module-path>::<decoded-name>`、entry module path は `main`、完全な id は 1..=1,024 UTF-8 byte。explicit entry/import closure 全体で test は最大 65,535。catalog order は既存の dependency-first DFS unit order（direct import は source order）、次に各 unit の source declaration order。探索対象はその explicit closure だけであり、directory、filename、annotation、manifest discovery は追加しない。 | Sema が validation/canonical id、driver が ordered catalog を所有する。exact C0/C1 boundary neighbor、name/id/count limit、duplicate scope、diamond-import order、unimported file の無視を catalog owner が固定する。 |
| Body type and control | test は compiler-private zero-parameter `fn() -> Result<(), core.Error>` として check する。書かれた block が Unit で完了した後、construct が文書化された `Ok(())` tail 1つを補う。`?`、`return Err(...)`、`match`、`else`、arena、Drop、既存 control form は従来の semantics を保つ。明示 non-Unit tail または他 type の `return` は reject。通常の Ok/Err と assertion exit は ordinary function cleanup を実行し、hard error、`process.abort`、successful `process.exec` は従来の no-unwind behavior のまま。 | Sema/HIR/MIR が flagged private test function と implicit tail を所有し、public interface entry は出さない。control-flow、cleanup、Error variant、malformed checked-HIR owner が direct/per-unit lowering を覆う。 |
| Assertions | `import core.test` があるとき、exactly `test.expect(condition)` と `test.expect_eq(left, right)` が使える。lexical test body とその ordinary nested block 内の standalone statement に限り、function、lambda、constant、operand/tail position では reject。`expect` は exact `bool`。`expect_eq` は既存 `==` の type/admission rule を適用し、その comparison result が exact `bool` であることも要求し、left、right の順に exactly once 評価する。ordinary result が `maskN` になる vector/mask comparison は assertion-only reduction を加えず reject。success は Unit で allocation なし。failure は canonical test id と 1-based call line/column を含む bounded diagnostic 1つを書き、enclosing test から `Err(Error.Invalid)` を返す。`expect_eq` は operand value を inspect/format しない。最初の failed assertion でその test を終える。 | Parser は ordinary qualified call のまま保ち、sema が imported test-only builtin と source identity を認識し、MIR/runtime が diagnostic と early Err を所有する。positive/negative lexical context、scalar/string equality、vector/mask rejection、eager order、first failure、cleanup owner が必須。 |
| Production commands | `check` と `check-per-unit` は test declaration を parse/type-check する。`fmt` は format する。全 production source consumer、すなわち `build`、`run`、`size`、`emit-mir`、`emit-llvm`、`emit-obj`、`explain-opt`、`db prepare` は test を type-check するが test body/harness を lower、link、report、reachable にしない。`explain-opt` は located MIR/optimization remark から test を除外する。`db prepare` は test body の query 全てを `Checked.static_descriptors` と native preparation から除外する一方、test checking の ordinary diagnostic は保持する。test-only capability は production/native link に影響しない。`emit-interface` は test name/body/assertion/catalog/descriptor/capability を export しない。Align source を取らない `cache clear`、`--version`、`db migrate/status/check/repair` には test input がない。test を含む source も通常の main 必須 command では valid `main` を必要とする。 | mode は explicit compiler input。parameterized source-command owner が listed verb 全てを覆い、production/test MIR twin、located-MIR absence、database-descriptor absence、interface absence、unreachable test-only native library、production executable byte identity が command matrix を閉じる。 |
| Test command and options | `alignc test <entry.align>` は explicit closure を check し、host test executable 1つを build して全 catalog entry を sequential に実行する。user `main` は不要で、存在しても実行しない。accepted common option は `--target-cpu`、`--profile`、`--rt-lto`/`--no-rt-lto`、`--cache-stats`、`-j`/`--jobs` で、既存の spelling/placement/duplicate semantics を保つ。test default profile は `dev`。`--watch`、`--thin-lto`、PGO flags、`--export`、program arguments、unknown option は build 前に reject。test-only option はそれぞれ `--timeout-ns N` または `--timeout-ns=N`、`--max-output-bytes N` または `--max-output-bytes=N` を compiler arguments 中の任意位置で exactly one まで認め、`--` terminator は追加しない。timeout は 1..=900,000,000,000、default 60,000,000,000 per catalog row で harness launch/cleanup も含む。output は 0..=16,777,216、default 1,048,576 を stdout/stderr それぞれに適用。environment variable は値を変えない。flag 除去後の argv は command + entry path 1つ exactly。test 0件は artifact を allocateせず stderr へ exact `alignc: no tests found` + LF、stdout へ何も書かず exit 1。 | CLI/driver が artifact 作成前の parsing/validation を所有する。exact default/limit/rejected-next、conflicting/valueless/repeated option、no-main、user-main-not-run、zero-test byte owner が必須。 |
| Build and artifact | per-unit test-mode object graph 1つと compiler-generated harness 1つを private `ArtifactStage` executable へ一度だけ link。各 test function は harness が catalog ordinal で dispatch できる compiler-private hidden external symbol を持つが language export ではない。temporary executable は source stem へ publish せず、最後の direct-child reap 後かつ suite summary 前に除去。各 selected test は同じ immutable executable を起動し、per-test compilation は行わない。 | Driver/per-unit codegen/harness generation が artifact を所有する。whole/per-unit semantic parity、single-link/same-inode reuse、exact ordinal dispatch、hidden linkage、success/failure 後 summary 前 cleanup owner が必須。 |
| Signal controller | Artifact worker join 後、test child acquisition 前に driver は他の long-running mode と共有する process-global driver signal lease 1つを acquire。second lease は native side effect 前に fail。setup は current thread mask を読み SIGHUP/SIGINT/SIGQUIT/SIGTERM が unblock であることを要求し、SIGCHLD が `SA_NOCLDWAIT` なしの default disposition であることを要求して SIGCHLD は変更しない。4つの既存 graceful disposition を snapshotし、signals を block、nonblocking close-on-exec self-pipe 1組を作成、SIGHUP/SIGINT/SIGQUIT/SIGTERM 順に async-signal-safe handler を install、lease publish、original mask restore。handler は preallocated atomic に first signal だけを記録して1 byte writeし、EINTR retry、EAGAIN は successful coalescing。allocate/lock/format/close/reap/compiler state access はしない。setup failure はinstalled-handler restorationをreverse orderで全てattemptし、全handler restore後だけwrite/read close、lease clear、mask last restore。ordinary/error return は全 child/stage 除去後かつ summary 前に4 signal block、disposition reverse restore、write/read close、lease clear、original mask last restore。setup rollback/teardown restorationのいずれかがfailした場合はpipe/leaseをvalidのまま保持し、stage cleanupとsignal-handler infrastructure error後にexit 1を直接実行してkernel teardownにremaining handler/fd除去を任せ、closed/reused descriptorを指すhandlerを残してreturnしない。graceful signal pathもcleanup中controllerを保持してprocessを直接exit。既存 graceful handler は一時置換後にrestoreし chainしない。 | Parameterized signal owner 1つが default/ignored/custom prior disposition、initially blocked signal 各種、incompatible SIGCHLD、second lease、全 setup/rollback/teardown failpoint、pipe-full/EINTR、simultaneous signals、全 lifecycle state の signal をcross。 |
| Parent-to-harness launch ABI | 各 spawn 前に parent は `AF_UNIX` `SOCK_DGRAM` socketpair 1組を作る。child endpoint は意図して inherit する唯一の non-stdio descriptor で fd 3 へ map。stdin は `/dev/null`、stdout/stderr は capture pipe、argv は private stage path と等しい argv[0] だけ、inherited environment は変更しない。ordinal/control input は argv/environment を通らない。parent は spawn 直前に monotonic row start を sampleし timeout は spawn 内の時間も含む。spawn setup は exec 前に `setpgid(0, 0)` を要求し、parent は launch datagram 送信前に `getpgid(child_pid) == child_pid` を証明しなければならない。spawn は成功したが group establishment を証明できない場合、harness は user code 前でblockしたまま。parent は retained direct PID だけへ SIGKILL を送り、未検証のnegative PGIDへは決して送らず、row descriptorをdrain/closeし、そのchildをreapしてspawn infrastructure failureをreport。そのdirect-PID killのESRCHはnon-reaping observationが同じunreaped childのterminalを証明した後だけaccept。verified group establishment後、parentはexact 16-byte launch datagram 1つを送り、byte 0..7 は `41 4c 54 45 53 54 4c 01`、byte 8..11 は selected u32 ordinal little-endian、byte 12..15 は zero。harness は 17-byte receive capacity で datagram 1つを読み、short/long、wrong magic/version、nonzero reserved、linked catalog range 外 ordinal を rejectし、fd 3 を close-on-exec にして exact 16-byte acknowledgement を送る。ack byte 0..7 は `41 4c 54 45 53 54 41 01`、続いて same ordinal と zero 4 byte。valid acknowledgement 後だけ user test code を実行できる。ack 前の deadline expiry、per-stream output excess、descriptor mapping、launch send/read/validation/ack failure、child exit/signal、unexpected/repeated datagram、completion は runner infrastructure failure。invalid launch input 後に selected test は実行しない。specialized pre-ack diagnostic は下で固定。 | Driver spawn setup と generated harness が separate launch/ack codec を所有。semantic-to-byte / byte-to-semantic golden は ordinal 7 を `414c544553544c010700000000000000` と `414c5445535441010700000000000000` に固定。verification-state matrix が length、magic/version、reserved、ordinal range、order、repetition、group setup/proof failure、ack 前後の deadline/output、全 syscall/acquisition edge を覆う。 |
| Process isolation and completion record | acknowledgement 済みの各 test はverified new process groupで実行。selected test からの normal return だけが fd 3 へ exact 20-byte completion datagram 1つを送る。byte 0..7 は `41 4c 54 45 53 54 00 01`、byte 8 は outcome（`0` Ok、`1` Err）、byte 9 は Error tag（Ok は `255`、それ以外は `0=NotFound`, `1=Invalid`, `2=Denied`, `3=Timeout`, `4=Code`）、byte 10..11 は zero、byte 12..15 は signed i32 Code payload little-endian（tag 4 以外 zero）、byte 16..19 は selected u32 ordinal little-endian。その後 descriptor を close。valid ack 後の short/long、reserved nonzero、unknown outcome/tag、inconsistent tag/code、repeated、out-of-order、wrong ordinal datagram は test failure。Ok は completion Ok + exit 0 のときだけ pass。Err は completion Err + exit 1 のときだけ test failure として有効。他の completion/exit/signal product は fail closed し、`process.exit(0)`、abort、exec、crash は success を偽装できない。 | Generated harness と minimal runtime reporting ABI が completion record、parent driver が exact decoding を所有する。independent semantic-to-byte / byte-to-semantic golden が両方向と全 malformed product を覆う。 |
| Time, output, and child cleanup | Child pipe 作成/spawn 前に parent は selected bound exactly の fixed raw-byte backing store を stream ごとに1つ fallible allocateし、zero なら allocation なし。spawn 後は store を geometric grow、replace、duplicate しない。read は remaining range へ直接 fill し、full stream ごとに fixed one-byte probe 1つで rejected next byte を検出する。したがって retained capture payload は selected bound の2倍 + probe 2 byte + fixed pipe/control state exactly（allocator metadata/rounding を除く）で old/new-allocation transient はない。allocation failure は first store も free し user code 前の runner infrastructure failure。deadline は pre-spawn sample から acknowledgement、user execution、group quiescence、descriptor drain、direct-child reap まで継続。exact output fit success、first extra byte は ack 前なら infrastructure、ack 後なら test failure。successful spawn と verified group establishment 後は全 terminal path、すなわち pre-ack failure、Ok、Err、nonzero exit、signal、timeout、output excess、malformed/missing completion、infrastructure error、interrupt で leader PID を complete pinned process group の signal 前に reapしない。上記unverified-group failureだけはtrusted group targetがなくuser codeも未実行なのでdirect-PID cleanupを使う。ordinary/test/infrastructure path は SIGKILL。graceful SIGHUP/SIGINT/SIGQUIT/SIGTERM はまず同じ signal をforwardし exactly 250 ms 後に SIGKILL。direct-child terminal observation は `waitid(..., WNOWAIT)`。non-reaping observation を持たない release host は first spawn 前に reject。group kill の ESRCH は non-reaping observation が leader terminal を証明した後だけ success、それ以外 infrastructure failure。accepted prefix/control を drain、descriptor close後、EINTR retryで direct childだけを reap。descendant は signalするが runner は reapしない。全 kill/drain/close/wait/reap step を earlier error 後もfixed orderでattempt。cleanup failureはbest effort後suite停止、selected individual reasonがあれば bounded block を infrastructure diagnostic より先にemit。first graceful signal は simultaneous test/infrastructure outcome より優先し、cleanup後にnew diagnostic/summaryなしでそれぞれ129/130/131/143 exit。fully cleaned ordinary test resultだけがnext catalog rowを許す。 | Dedicated driver test-runner component だけが signal snapshot、allocation、descriptor、deadline、pinned PGID、non-reaping observation、stage cleanupを所有。Cartesian state/event ownerがpre/post-ack、completion absent/present、leader running/terminal、descendant absent/present、全 result class、4 graceful signals、全 boundaryのdeadline/output、各acquisition/cleanup failpoint、evidence preservation、reap前next-row禁止をcross。 |
| Reporting and exit | passing child stdout/stderr は emit しない。all-pass suite は runner stdout へ exact `test result: ok. <N> passed; 0 failed` + LF を書き exit 0。failure ごとに catalog order で runner stdout へ `FAIL <canonical-id>` + LF、fixed `reason: ...` line、続いて nonempty captured stdout/stderr を fixed `--- stdout ---` / `--- stderr ---` header 下へ byte decode/rewrite なしで出す。final LF がない capture の後は次 runner line 前に LF 1つを足す。最後に `test result: FAILED. <P> passed; <F> failed` + LF、exit 1。timeout、output-limit、signal/exit/record mismatch、returned Error、malformed-record detail は下の closed format を使う。assertion location は bounded child-stderr line。別 test が fail しても successful child output は suppressed。zero test は上の exact diagnostic。pre-ack timeout/output は下の exact infrastructure lines。その他 build/compiler/catalog diagnostic は compiler-owned format、runner infrastructure は下の closed stderr template。全 error は exit 1、complete-suite summary なし。graceful interrupt は added line/summary なしで prior failure block は残る。 | CLI reporter が deterministic bytes/terse-success behavior を所有。zero test、all-success、mixed result、arbitrary bytes、missing LF、assertion、全 termination reason、全 pre/post-ack product、graceful interrupt、selected result 前後 infrastructure の exact stdout/stderr golden が必須。 |
| Ownership, allocation, and effects | test declaration は runtime value を所有しない。test harness catalog/static assertion location は compiler-owned immutable data。test-mode object/harness allocation は driver-owned。parent は pre-spawn capture store 2つを exact ownership し、reporting のため captured byte を copyせず framing/stored range を直接 writeし、row 後に両 store を解放。test function/assertion は Impure で export しない。bounded process-signal lease/self-pipe は new process-global state 唯一で one `alignc test` runner の ownership 中だけ存在。test registry、reflection table、hidden source scan、persistent test history、retry、concurrency、shuffle、filter、fixture lifecycle、snapshot、coverage、benchmark、network policy は追加しない。 | Sema effect inference、codegen data ownership、signal-lease RAII、runner RAII が owner。allocation accounting/failpoint が exact requested live/transient capture byte を固定し、capability-link twin が production exclusion/test inclusion を証明。 |
| Cache and sources of truth | Test mode は distinct versioned cache domain。unit key は complete local ordered test catalog/body/assertion location/mode version/target/profile/codegen input/imported interface hash/runtime ABI fingerprint を含む。harness key は complete ordered canonical-id/symbol catalog と launch/acknowledgement/completion protocol version を含む。production interface/codegen key は test catalog/body semantics を除外する。既存 source-keyed frontend lookup は test-only edit 後 miss し得るが、生成する production MIR/object key/executable bytes は不変。まずこの file を更新し、その後 `draft.md`、`docs/language-spec.md`、`docs/design-notes.md`、`docs/open-questions.md`、`docs/impl/02-frontend.md`、`docs/impl/07-roadmap.md`、`docs/impl/19-hir-validation-ledger.md`、runtime ABI row が land するときの ledger、同期 Japanese mirror を更新する。 | Canonical key encoder と interface/implementation hash が invalidation を所有。named input を独立に mutate し、unchanged production artifact と必要箇所の changed test object/harness を pin。 |

## Contract rationale

test block は ordinary checker、ownership model、error propagation を必要とするため language
declaration である。normal code に露出する第二の function syntax ではなく、parameter、callable
name、visibility、public interface entry を持たない。implicit success tail が construct の唯一の
wrap である。recoverable failure は既存 `Result<(), Error>`、hard error は process-fatal のまま
runner が isolate する。

`core.test` は assertion が punctuation ではなく library capability なので explicit import とする。
early-Err behavior を test block 内の standalone call に限定し、control edge を assertion site に
visible にして assertion value の無視/transport を不可能にする。helper function は既存の一方式、
すなわち Result を返し test 側で `?` を適用する。

entry/import closure だけが discovery root。`tests/` directory、filename suffix、annotation、manifest
は compilation/cache identity に hidden input を加える。既存 dependency-first unit order は
deterministic なので第二の order を作らず再利用する。

## Checking and lowering

全 compiler command が test を parse する。semantic checking は declaration body の間だけ test
context を install する。normal same-module private item と imported public item を resolve し、
`core.test` import がある場合だけ2つの assertion を認める。context は called function/lambda へ
flow しない。nested ordinary block、`if`、`match`、loop、arena、unsafe block は同じ test body。

checked record は canonical id、source ordinal、body、test discriminator 1つを持つ。production
lowering は record を validate して function を omit。test lowering は exact `Result<Unit, Error>`
ABI の private function を emit し assertion location を immutable data として emit。malformed
discriminator/name/ordinal/body result/assertion context/implicit-tail shape は MIR construction 前に reject。
validator は generated symbol spelling から test status を推論しない。

test function は Impure。body が arithmetic だけでも `par_map`、task transfer、generic effect promise
に入れない。declaration は interface summary に含めない。imported unit の test function は driver
が explicit test mode を選んだときだけ compile する。

## Runner model

driver は immutable test executable 1つを link。generated entry は fixed compiler-private fd 3 で
launch datagram 1つを受け、selected catalog ordinal を validate/acknowledgeし、exact test function 1つを
dispatch、その function return 後に completion datagram を送って exit。user `main` は call せず、
test-only program は main 不要。parent は同じ executable を catalog row ごとに sequential 起動する。

fd 3 datagram socket は stdout/stderr と別で application input を運ばない。parent は compiler-private
argv/environment value を渡さず、child から見えるのは argv[0] の stage path、inherited environment、
`/dev/null` stdin、captured stdout/stderr だけ。harness は launch acknowledge 前に fd 3 を close-on-exec
にするため successful `process.exec` は completion authority を inherit しない。valid acknowledgement
により harness setup と user execution を区別し、normal test return だけが safe completion producer。
completion bytes がなければ exit zero も failure となり `process.exit(0)` bypass を閉じる。unsafe code
は他の safe invariant と同じく descriptor boundary を破り得るが malformed/self-inconsistent datagram
は fail closed。

v1 launch/acknowledgement golden vector:

| Semantic value | Exact 16 bytes, hexadecimal |
| --- | --- |
| Launch ordinal 7 | `414c544553544c010700000000000000` |
| Acknowledge ordinal 7 | `414c5445535441010700000000000000` |

v1 completion-record golden vector:

| Semantic value | Exact 20 bytes, hexadecimal |
| --- | --- |
| Ok, ordinal 7 | `414c54455354000100ff00000000000007000000` |
| Err(Error.Invalid), ordinal 7 | `414c544553540001010100000000000007000000` |
| Err(Error.Code(-9)), ordinal 7 | `414c54455354000101040000f7ffffff07000000` |

encoding/decoding は別実装でこの vector に照合する。decoder は fixed envelope を byte order で読み、
reserved/conditional field を validate 後 selected ordinal/process status と比較する。untrusted byte array
を native record へ transmute しない。

## Assertions

`test.expect` は boolean を一度評価。`test.expect_eq` は left、right の順に評価し、その comparison
が exact `bool` を返す場合だけ ordinary source `==` と同じ比較を行う。ordinary vector/mask equality
は mask を返すため rejectし、implicit all-lanes reduction、assertion-only equality family、aggregate
debug formatter はない。failure は child stderr へ exact one line:

```text
assertion failed: <canonical-id>:<line>:<column>: expected true
assertion failed: <canonical-id>:<line>:<column>: expected equality
```

canonical-id limit により line は bounded。line/column は qualified assertion call の decimal 1-based
source position。operand source text/value を retain/format/reflect/allocate しない。その後 assertion は
`Error.Invalid` で ordinary Err cleanup edge を通る。runner completion record が test return を独立証明。

## Resource and outcome precedence

CLI input は entry を read/compile する前に left-to-right で command shape、common option
spelling/value、test timeout、output bound、forbidden option combination、entry path の順に validate。
test-specific option の repeat は last-wins でなく error。bad catalog は checking 後 artifact allocation 前。

dedicated test-runner component 1つが complete state machine を所有し、CLI branch、harness codec、reporter
は独自に wait/signal/reap/catalog advance してはならない。state は次の通り:

| State | Ordered observable events | Required transition and invariant |
| --- | --- | --- |
| Ready/acquire | first graceful signal; capture allocation; stdout/stderr pipe; control socket; pre-spawn clock sample; spawn; process-group proof | signal は stage 除去後 conventional exit。spawn 前 failure は child なしの infrastructure。successful spawn と `getpgid(child_pid) == child_pid` proofでleader PIDをretainしAwaitAckへ。proof failureはそのPIDだけへSIGKILL、rowをdrain/close、そのchildをreapし、launchを送らずuser codeも実行せずspawn infrastructure failureで停止。 |
| AwaitAck | first graceful signal; expired deadline; control datagram; non-reaping leader status; stdout; stderr | deadline/first excess byteはlaunch infrastructure。controlをstreamより先に読みqueued valid ackがuser-code boundaryを確立後、causally later user outputを分類。valid exact ackでRunning。malformed/missing/out-of-order controlまたはleader terminationはlaunch infrastructure。terminal selectionはreapせずQuiescingへ。 |
| Running | first graceful signal; stdout; stderr; completion datagram; non-reaping leader status; deadline | stdout excessがstderrより先、両方completion/timeoutより先。同一wakeのcomplete valid completion/status productはdeadlineより先。completion without statusはpending。valid productなしterminal statusはclosed mismatch reason。selected outcomeはreapせずQuiescingへ。 |
| Quiescing | selected graceful signal or mandatory SIGKILL; remaining stdout/stderr/control; deadline; direct-child status; cleanup errors | pinned groupを`waitpid`前にsignal。accepted prefixをdrainしdirect childだけreap。pre-ack deadlineはlaunch infrastructure、post-ackはtest timeout。graceful signalが最優先。それ以外はcleanup failureがinfrastructureとなりselected test blockを保持。descriptor close/capture release前にresultをreportしない。 |
| Between rows | first graceful signal; selected result reporting; next-row acquisition | live/waitable direct childもopen row descriptorもない。cleaned test resultだけcatalog advance。infrastructureは停止、graceful signalはstage除去後exit。 |
| Finalize | first graceful signal; stage removal; ordinary signal-controller teardown; summary | stage removal/controller teardownはsummary前。failureはsummaryなしinfrastructure。graceful signalはnew line/summaryなしでcleanup後conventional exit。 |

successful Quiescing 後の ordinary result order は selected output/timeout、completion length、
magic/version、outcome、tag、reserved bytes、conditional code、ordinal、record/status correlation、
returned Error/Ok。このstate-specific orderがscheduler timingをretry/error rewrite inputにしない。

selected-size capture store 2つは pipe/socket/child acquisition 前に fallible allocateする。read は
in-place fillし、store が exact bound に達した後は fixed one byte で overflow を検出。spawn 後に
capture store を grow/replaceせず、reporting も copy せず stored range を直接 write。従って requested
live/transient capture payload は selected bound 2つ + probe 2 byte + fixed state exactly。allocator
metadata/size-class rounding は selected-byte promise の外。どちらかの allocation failure は両 store を
freeし child 作成前に infrastructure failure を reportする。

kill、descriptor close/drain、wait、reap、control cleanup は fixed best-effort sequence で実行する。
acquisition/cleanup operation の failure は output excess/timeout/他 test failure が先に selected でも
infrastructure failure として次 test を禁止する。その product では selected 済み bounded failure block
を保持してから infrastructure diagnostic を出し、stage を除去し、suite summary なしで停止する。

## Terse output policy

runner は test live 中だけ bounded output を retain し success 後は emit しない。repository validation
と同じ failure-only evidence policy であり、大きい passing suite も constant-size output、failure は
local evidence を保持。output は bytes のまま。failure 時 runner 自身の ASCII frame を書き nonempty
stream を unchanged replay。UTF-8 replacement、terminal escaping、line split、selected cap 未満の truncate
はしない。

output limit は silent truncation でなく correctness result。inclusive bound を超える first byte で fail/
terminate。zero bound は assertion diagnostic を含め empty output だけ許す。他 test が後で fail しても
successful output は suppressed のままなので1 failure が whole suite log を拡大しない。

`reason:` line は次の format のいずれか exactly:

```text
reason: returned Error.NotFound
reason: returned Error.Invalid
reason: returned Error.Denied
reason: returned Error.Timeout
reason: returned Error.Code(<signed-i32>)
reason: timed out after <timeout-ns> ns
reason: stdout exceeded <max-output-bytes> bytes
reason: stderr exceeded <max-output-bytes> bytes
reason: exited with status <signed-i32>; completion record: <record-detail>
reason: terminated by signal <positive-i32>; completion record: <record-detail>
reason: completion record <Ok|Err> mismatched exit status <signed-i32|signal positive-i32>
```

`<record-detail>` は `length`、`magic/version`、`outcome`、`error tag`、`reserved bytes`、
`error code`、`ordinal` のいずれか exactly で record-validation order により選ぶ。decimal number は
leading plus/zero padding なし。両 stream の output excess を同時検出した場合は event-loop order により
stdout。valid Err record + exit 1 は returned-Error line（assertion の `Error.Invalid` を含む）を選び、
assertion location は replayed stderr bytes に残る。

runner infrastructure abort は stderr へ exactly
`alignc: test runner <operation> failed (os error <signed-i32>)` + LF。operation は
`stage create`、`stage cleanup`、`capture allocation`、`signal handler`、`pipe`、`control socket`、
`control write`、`control read`、`launch`、`spawn`、`stdout read`、`stderr read`、`close`、`wait`、
`kill`、`reap` のいずれか。numeric value は raw OS code、allocation/validation または platform が code
を出さない failure では zero。earlier test outcome 後でも final suite summary は出さず、既に emit
済みまたは直前に selected された failure block は残る。compiler diagnostic/link error は既存 format
で summary なし。artifact-stage removal は all-pass/failed-suite summary より先なので cleanup failure が
published complete summary の後に起きることはない。

pre-ack resource failure 3種は代わりに次の specialized infrastructure line のいずれかexactlyをstderrへ
書き、captured setup bytesはdiscard:

```text
alignc: test runner launch timed out after <timeout-ns> ns
alignc: test runner launch stdout exceeded <max-output-bytes> bytes
alignc: test runner launch stderr exceeded <max-output-bytes> bytes
```

exit 1、stdout empty、full group/direct-child/stage cleanup、suite summaryなし。decimal formatは上の
result-line ruleと同じ。

## Cache and artifact identity

production/test compilation は parsing/semantic rule を共有するが codegen domain は disjoint。test-only
edit は source-keyed frontend lookup を invalidate し得るが、その後 identical production MIR/object key/
link input/executable bytes を再現しなければならない。test mode の local body/ordinal/canonical id/
assertion location/mode version は owning unit key、harness key は complete ordered catalog/symbol mapping/
3つの control-protocol version を覆う。import order change は public module interface を変えず harness
order を変え得る。

executable は final child reap まで runner-retained `ArtifactStage` 以下にある。source-adjacent binary、
catalog file、history、snapshot、machine-readable public test artifact は生成しない。

## Implementation closure matrix

| Axis | Required implementation closure | Acceptance owner |
| --- | --- | --- |
| Syntax and formatting | contextual item、`pub`/attribute/signature near-shape、newline/braces、recovery、depth cap、format idempotence | parameterized lexer/parser/AST/formatter owner |
| Catalog and modules | name/id/count bound、duplicate、module 間 same name、dependency-first diamond order、unimported exclusion、private/public access | whole/per-unit catalog golden と sema owner |
| Body and control | implicit Ok、explicit Err、`?`、`match`、`else`、assertion early exit、cleanup-bearing control join、malformed HIR | sema + checked-HIR + MIR control/Drop owner |
| Assertion surface | import rule、lexical context、statement-only placement、bool equality family、vector/mask non-Bool rejection、left-to-right once、line/column、first failure | parameterized positive/negative sema owner + MIR/runtime diagnostic golden |
| Production isolation | 全 source command は check するが test を omit、located MIR/remark と `db prepare` static descriptor も omit、test capability/link/export/interface/cache influence なし、ordinary main 不変 | parameterized command matrix、production/test twin、database descriptor owner、byte-identical executable owner |
| Test artifact | no-main test program、user main not called、one link、one immutable inode、hidden symbol、exact ordinal dispatch | driver artifact + whole/per-unit execution owner |
| Launch protocol | fd 3 datagram mapping、argv/environment/stdin shape、pre-exec group setup/proof、launch/ack codec/golden、malformed/order/ordinal product、pre/post-ack exit distinction | driver/harness protocol matrix + acquisition failpoint |
| Completion protocol | independent codec 2つ、semantic golden 3つ、全 malformed field、wrong ordinal、exit/record Cartesian product、exit/exec/abort/crash bypass | runtime/driver protocol matrix |
| Signal controller | shared lease、prior mask/disposition、SIGCHLD compatibility、setup/rollback/teardown、HUP/INT/QUIT/TERM order、first-signal coalescing、全 runner stateのsignal | parameterized process-global signal owner |
| Runner state machine | Ready/AwaitAck/Running/Quiescing/Between/Finalize x 全event、pre/post-ack deadline/output、completion/status timing、全 outcomeのgroup signal before direct reap | deterministic Cartesian state/event/barrier owner |
| Child lifecycle | exact preallocation/failure、spawn/pipe/control acquisition failure、unverified-group direct-PID cleanup、concurrent drain、exact/rejected-next bound、verified-groupの全terminal result x descendant absent/present、WNOWAIT pin、group-kill-before-reap、cleanup-error override + evidence preservation、interrupted wait/read | deterministic allocation/failpoint/process-tree owner |
| Reporting | exact zero-test/pre-ack infrastructure bytes、all pass one line、mixed failure、returned Error variant、assertion、exit/signal/timeout/output/malformed reason、4 graceful signals、raw bytes/missing LF | exact CLI stdout/stderr golden |
| Cache identity | unit/harness key field の独立 mutation、production frontend miss + object hit、test-only capability inclusion | cache hit/miss + canonical-key owner |

## Capability boundary and deferrals

implementation boundary はrunner lifecycle周囲でre-openした。accepted codeはstrict internal proof domain
2つを持つ。compiler formationはsyntaxからhidden harness/exact control codecまでを所有。dedicated driver
test-runner component 1つはimmutable artifact、ordered catalog、validated limitだけをconsumeし、signal
state、spawn/poll、deadline、process group、capture、reap、reportingをexclusive ownership。どちらも他方の
validation/lifecycle transitionを再実装しない。fake-stage protocol/process-tree ownerはAlign source compile
なしでrunnerをvalidateし、whole/per-unit ownerは同じcodec boundaryに対してproducerをvalidateする。

この2 domainはdesign PR後のpublic capability 1つとしてlandする。runnerなしparser-to-harnessと
compiler-private symbolなしrunnerはいずれもdormantで、splitはuseful stable consumerをpublishせず
mode/cache/ABI integration proofを重複させる。このためexpected hand-written diffは1,000 lineを超え得る。
explicit internal boundary、state/event matrix、independent ownerによりunusable prefixをshipせずriskを下げる。

first capability は test filtering/listing、parallel/shuffle execution、retry、fixture、setup/teardown hook、
snapshot、coverage、benchmark、ignored test、expected failure、persistent history、hidden file discovery、
assertion formatting/reflection system を明示的に除外。sequential error-model core で不足する real consumer
が現れた後だけ additive に検討する。
