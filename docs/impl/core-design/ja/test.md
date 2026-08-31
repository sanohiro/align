このディレクトリには `../std-design/` と同じ深さで `core` ライブラリ各領域の正式な
設計文書（シグネチャ、ownership/effect 分類、error policy、pitfall、test anchor）を置く。

# core.test — 言語内テスト

> [English](../test.md) · **日本語**

> **ステータス:** 2026-08-31 implemented。parser、checked-HIR overlay、MIR/LLVM lowering、
> compiler-private runtime ABI、cache separation、bounded sequential runnerを一つの
> capabilityとして出荷する。

## Public-contract ledger

この ledger が最初の実装に対する authoritative record である。後続 prose は各 row を
説明できるが拡張してはならない。public change はまずこの表を更新し、その後に列挙した
全 source of truth へ一度に伝播する。

| Surface | Exact public record | Owner、artifact/cache identity、acceptance owner |
| --- | --- | --- |
| Declaration grammar | private top-level declaration は1つだけであり、`test`、通常の Align string token 1つ、block 1つの順で書く。`test` は identifier のまま、item-position two-token lookahead が `test` + string のときだけこの contextual item にcommitする。bare `test {}` と `pub test {}` は keyword-less type declaration のまま。`pub test` + string lookahead は rejected visible-test form にcommit。commit後はparameter、type parameter、明示 return type、expression-body `=`、attribute、missing block、末尾 declaration nameをtest-specific recoveryでreject。declaration は callable/name binding を作らない。 | Lexer は `test` を identifier のまま保ち、AST/parser/formatter が contextual item を所有する。parser/formatter round-trip、`test`/`pub test` type twin、lookahead/recovery、全 reject near-shape を parameterized owner 1つで固定する。 |
| Name, identity, and catalog | decode 後の name は 1..=256 UTF-8 byte で Unicode C0/C1 control を含まない。U+0000..U+001F と U+007F..U+009F を rejectする。同一 module 内の duplicate name は reject。canonical public id は `<canonical-module-path>::<decoded-name>`。entry source は宣言した module path を使い、module declaration を省略した場合だけ `main` を使う。完全な id は 1..=1,024 UTF-8 byte。entryがmodule declarationを省略し、import sourceが明示的に`module main`を宣言する場合はcatalog形成前に`default entry module 'main' conflicts with imported module 'main'; declare the entry module explicitly`でrejectする。explicit entry pathはordinary duplicate-module ruleに従う。explicit entry/import closure 全体で test は最大 65,535。catalog order は既存の dependency-first DFS unit order（direct import は source order）、次に各 unit の source declaration order。探索対象はその explicit closure だけであり、directory、filename、annotation、manifest discovery は追加しない。 | Sema が validation/canonical id、driver が ordered catalog を所有する。declared/default entry path、exact C0/C1 boundary neighbor、name/id/count limit、duplicate scope、rejected default-entry/imported-declared-`main` collision、diamond-import order、unimported file の無視を catalog owner が固定する。 |
| Body type and control | test は compiler-private zero-parameter `fn() -> Result<(), core.Error>` として check する。書かれた block が Unit で完了した後、construct が文書化された `Ok(())` tail 1つを補う。`?`、`return Err(...)`、`match`、`else`、arena、Drop、既存 control form は従来の semantics を保つ。明示 non-Unit tail または他 type の `return` は reject。通常の Ok/Err と assertion exit は ordinary function cleanup を実行し、hard error、`process.abort`、successful `process.exec` は従来の no-unwind behavior のまま。 | Sema/HIR/MIR が flagged private test function と implicit tail を所有し、public interface entry は出さない。control-flow、cleanup、Error variant、malformed checked-HIR owner が direct/per-unit lowering を覆う。 |
| Assertions | `import core.test` があるとき、exactly `test.expect(condition)` と `test.expect_eq(left, right)` が使える。lexical test body とその ordinary nested block 内の standalone statement に限る。ordinary parserはown terminated lineでも`}`直前のlast expressionを`Block::tail`にするため、semaはAST parentからchild check前にplacementを固定し、root test completionまたはenclosing block/control expression自体がcomplete statementのときだけ、そのsyntactic slotのexact assertionをfinal statementとしてconsumeする。全Value edgeはconsumerがUnitを期待してもreject。admitted assertionはUnit fallthroughを残してtestのimplicit Okへ進み、function、lambda、constantでもreject。`expect` は exact `bool`。`expect_eq` は既存 `==` の type/admission rule を適用し、その comparison result が exact `bool` であることも要求し、left、right の順に exactly once 評価する。ordinary result が `maskN` になる vector/mask comparison は assertion-only reduction を加えず reject。success は Unit で allocation なし。failure は canonical test id と 1-based call line/column を含む bounded diagnostic 1つを書き、enclosing test から `Err(Error.Invalid)` を返す。`expect_eq` は operand value を inspect/format しない。最初の failed assertion でその test を終える。 | Parser は ordinary qualified call/tail parsingを保ち、sema が imported test-only builtin/source identityを認識してadmitted syntactic assertion tailをchecked HIR前にnormalizeする。checked HIRはcomplete `Stmt::Expr`だけにdiscriminatorを認め、MIR/runtime が diagnostic と early Err を所有。root/nested final-statement twin、expected Unitを含む全consumed-tail negative family、scalar/string equality、vector/mask rejection、eager order、first failure、cleanup owner が必須。 |
| Production commands and modes | `check` / `check-per-unit` はtest declarationをparse/type-checkし、`fmt`はformatする。`align-repl`は同じcontextual itemをparseするが、test itemを含むsubmitted entry全体をreplacement resolution/session mutation/compile/execute前に`error: test declarations are not available in align-repl; use 'alignc test <entry.align>'`でrejectし、prior state/next ordinalを変えない。`build`、`run`、`size`、`emit-mir`、`emit-llvm`、`emit-obj`、`explain-opt`、`db prepare`はcomplete checked resultをvalidateするがfrozen production prefixだけを選ぶ。test root、lifted helper、test-only generic/type/resource monomorph、interned type、non-database static descriptor、capabilityはoverlayに残る。`explain-opt`はlocated MIR/remarkからoverlayを除外。DB Query/command constructorはordinary named top-level descriptor functionのcomplete bodyだけでlegalなのでprefix freeze前に形成され、test overlayからは作れない。handcrafted overlay DB descriptorはchecked-HIR validationでreject。`db prepare`は既存production descriptor setをprepareしtest flagを持たない。`alignc test`はそのmetadataをoffline consumeし、missing/stale `CheckedRequired`はartifact前にfailしてDBへ接続しない。`emit-interface`はtest surfaceを一切exportしない。sourceなしcommandにtest inputはない。main必須commandでは通常のvalid mainが必要。prefix selectionはone-shot build/run/size、`build --watch`初回/全rebuild、whole/per-unit、全profile/target/runtime-LTO、release/fast ThinLTO、release/fast PGO instrument/useへ独立に適用する。 | Explicit prefix/combined-view inputを使う。listed verb/transactional REPL rejection × admitted watch/profile/target/runtime-LTO/ThinLTO/PGO/jobs/cache-stats productをparameterized ownerで覆い、actual watch rebuild/ThinLTO/PGO artifact、production/test MIR twin、generated/type isolation、DB policy×driver、interface/native capability exclusion、byte-identical production artifactをownerにする。 |
| Test command and options | `alignc test <entry.align>`はexplicit closureをcheckしhost executable 1つをbuild、catalogをsequentialに実行。user `main`は不要で自動実行しない。common option、test-only option、bounds/default/repeat/no-`--`、artifact consumerはEnglish ledgerとexactly同じ。flag除去後はcommand+entry 1 path exactly。CLI validation直後、source I/O/cache/artifact前に`std::env::current_dir()`を一度だけnative `PathBuf`へsnapshotする。failureはstderrへ`alignc: test runner working directory failed (os error <signed-i32>)`+LF、artifactなしexit 1。cwdは全row共通runner inputだけでsource/object/harness/cache identityへ入らない。zero testはartifactなし、exact diagnostic、exit 1。 | Accepted option Cartesian ownerとterminal-consumer assertionに加え、current-directory failure、snapshot stability、runner-only identityを固定する。 |
| Build, entry symbols, and artifact | Test loweringはimmutable prefix+validated overlayをper-unit test-mode object graph 1つとgenerated harness 1つへ落とし、一度だけprivate `ArtifactStage`へlinkする。harnessだけがexternal literal `i32 @main()`を所有し、test modeはordinary source-main wrapperを出さない。許可済み4形は既存encoded symbolを使い、source spelling `main`はexact `align_fn$4$6d61696e`。global catalog ordinal `n`はexact hidden external `align_test$`+8 lowercase hex。link後signal-controller acquisition前にcatalog/cache report/limits/suite cwd/executable pathをimmutable化し、whole/per-unit `PipelinedPackageComplete`のprivate `object_stage`、generated-harness object stage、response/temp file、cache lease、joined worker/pipeline guardをnormal Dropで全consumeする。runner/final guardへ渡るbuild ownerはfinal executable `ArtifactStage`だけで、raw `_exit`へbuild-stage path/fdを残さない。executableはlast reap後summary前にremoveし、全rowでsame inodeをreuse。 | Entry/symbol、whole/per-unit parity、single link/inodeに加え、first spawn前の全build-stage owner absenceと全terminal cleanupを固定する。 |
| Test-callgraph process boundary | Cache lookup/native-capability collection/object/harness allocation/external side effect前に、`alignc test`はcatalog rootからvalidated combined call graphをwalkする。direct/imported call、function-value target、lifted callback/destructor、whole/per-unitのconcrete generic monomorphを含む。reachable `ExprKind::ProcessCommand`はexactly `process.command is not available from test code; run the external process in an owner test`でreject。catalog order、dependency-edge、structural HIR orderでfirst siteを固定しshared siteは一度だけ。unreachable production helperの`process.command`はprefixに残ってacceptされ、inert test object/ordinary runtime selectionへ残り得るがcatalog rootから実行できない。combined checked-HIR validatorもmalformed inputをrejectし、production behaviorは不変。`process.spawn`/`exec`/`exit`/`abort`はsettled row-group contractのまま。 | Driver/combined-view validatorがartifact前のreachability rejectionをown。direct/nested/imported/generic/function-value/lifted、shared/unreachable、precedence、malformed HIR、whole/per-unit、inert-prefix retention、production parityをparameterized ownerで閉じる。first capabilityのwideningにはseparately reviewed portable descendant-containment protocolが必要で、partial dynamic supervisorはshipしない。 |
| Signal controller | Artifact worker join後、child acquisition前にshared process-global leaseをacquireし、既存のSIGHUP/SIGINT/SIGQUIT/SIGTERM disposition/maskとSIGCHLD compatibilityを検証する。handlerは`SA_RESTART`なしでinstallし、preallocated lock-free atomicのclosed state `Idle/Writing/Selected(signal)/WritingPending(signal)`をCASする。handler entryでinterrupted threadの`errno`をsaveし、arbitration/self-pipe後の全exitでexact valueをrestoreする。EINTR retry、pipe-full EAGAIN、already-selectedも同じ。`Idle`ならfirst signalをselectし、`Writing`ならpendingにする。reporter permit、self-pipe、rollback、returning teardown、non-returning final guard、prior handler restore、numeric 129/130/131/143 `WIFEXITED` ruleはEnglish ledgerどおり。 | Parameterized ownerがprior state/failpoint/arbitration/wait statusに加え、representative nonzero `errno`をIdle/Writing/Selected、EINTR、pipe-full delivery後も不変と証明する。 |
| Parent-to-harness launch ABI | 各spawn前にparentはcontrol socketpair/capture endpointを準備し、parent endpointをnonblockingにする。child spawn actionは最初にone-invocation suite cwdを`posix_spawn_file_actions_addchdir_np`でinstallし、fd 0を`/dev/null`、fd 1/2をcapture、fd 3をcontrolへsource-equals-targetを含めmapしてfinal mappingのclose-on-execをclearし、全dup action後に`posix_spawn_file_actions_addclosefrom_np(4)`をappendする。childのfinal fd tableはexactly 0..3だけでambient/row-original fd 4以上はない。supported Linux/macOS release hostは両extensionを必須とし、unavailable/action-construction failureはspawn前にcwd=`working directory`、close-from/remap=`descriptor mapping`でreject。argv[0]はprivate stage pathのみ、environmentはinherit、cwdは全rowでsnapshot値のまま。残るnonblocking launch/ack、group proof、codec/validation ruleはEnglish ledgerどおり。 | Verification matrixがsuite-cwd snapshot/use/addchdir failpoint、fd 0/1/2/3全remap、closefrom order、ambient fd 4/soft-limit boundary、concurrent parent cwd/fd mutation、exact child fd inventory、全protocol/group edgeを覆う。 |
| Capture transport | fixed store 2つのallocation後、parentは`/dev/null` stdin sourceをopenしstdout/stderr pipeを作り、全original endpointをclose-on-exec、両parent read endpointをspawn前にnonblockingにする。child spawn actionは`/dev/null`だけをfd 0、child write endpointをfd 1/2へmapし、全source-equals-target caseでmappingのclose-on-execをclearしてunused original stdio endpointを全close。parentはspawn後に`/dev/null`/child write-end copyをcloseし、blocking capture readを行わない。selected readinessごとにそのstreamをstore/probeへ直接、`EAGAIN`、`EWOULDBLOCK`、EOF、rejected next byte、hard errorのいずれかまでdrainしてevent loopへ戻る。live childのshort writeはcontrol、other stream、status、signal、deadlineを止めない。successful spawn前のopen/pipe/flag/remap failureは全acquired endpointをclose、両storeをreleaseし、user codeなしinfrastructure failure。 | Runnerがstdio/capture flag/drain stateを所有。parameterized ownerがstdout/stderrを独立に、short-write-then-idle child、simultaneous pressure、`EAGAIN`/`EWOULDBLOCK`、EOF、exact/rejected-next bound、source fd 0/1/2 remap、全open/pipe/flag/remap failpoint、partial drain後のdeadline/signal progressを覆う。 |
| Process isolation and completion record | acknowledgement済みtestはverified process groupで実行。normal returnだけがfd 3へexact 20-byte completionを送り、envelope/outcome/tag/code/ordinal bytesはEnglish ledgerのexact record。harnessはOkなら0、Errなら1をreturnし、process terminationがfd 3のsole ordinary close owner。ack後のarrival-order/field-order/cardinality、`repetition`/`order`/`length`、Ok+exit0/Err+exit1だけのvalid productは従来rowどおり。exit/abort/exec/crashはsuccessを偽装できない。 | Generated harnessと下のexact compiler-private runtime ABIがchild receive/fd state/encoding/send、parent driverがindependent launch encoderとack/completion decoder/cardinalityを所有。semantic/byte goldenは両方向と全malformed productを固定。 |
| Child control runtime ABI | Test artifactだけが4つのunkeyed symbolをdeclareできる: `i32 @align_rt_test_launch_recv_v1(i32 fd, ptr out_ordinal)`、`i32 @align_rt_test_fd_cloexec_v1(i32 fd)`、`i32 @align_rt_test_ack_v1(i32 fd, i32 ordinal)`、`i32 @align_rt_test_report_v1(i32 fd, i8 outcome, i8 error_tag, i32 code, i32 ordinal)`、全て`nounwind`。Rustはordinal `u32`、outcome/tag `u8`、code/fd `i32`、output `*mut u32`。launch recvはnonnull/4-byte aligned outputをI/O前にzero、fixed 17-byte capacityでEINTR retryし、exact launch envelopeをvalidateしてsuccess時だけordinal publish。rangeはharness owner。fd helperは他flagを変えずCLOEXEC追加。ack/reportはclosed semantic productをvalidate、stack 16/20 bytesをencodeしdatagram send 1回、EINTR retry、short=`EIO`。return 0 success、positive raw OS code、invalid ABI=`EINVAL`、malformed launch=`EPROTO`。allocation/pointer・fd retain/close/process-global state changeなし。harnessはlaunch/range failure=exit120、CLOEXEC=121、ack=122、completion=123へmapし、parentはphaseに応じ`launch`/`descriptor flags`/`control write` code0へmap。reserved interpretationはphase-specificで、post-ack user statusは123以外ordinary missing-record exit。 | `align_codegen_llvm`/`align_runtime`/runtime ABI ledgerがdeclaration/definitionをatomic owner。language `RuntimeKey`なし。ABI/byte、null/alignment、invalid product、EINTR/short/error、fd/CLOEXEC/exec、reserved phase、collision/export parity、whole/per-unit ownerが必須。 |
| Time, output, and child cleanup | Child pipe 作成/spawn 前に parent は selected bound exactly の fixed raw-byte backing store を stream ごとに1つ fallible allocateし、zero なら allocation なし。spawn 後は store を geometric grow、replace、duplicate しない。read は remaining range へ直接 fill し、full stream ごとに fixed one-byte probe 1つで rejected next byte を検出する。したがって retained capture payload は selected bound の2倍 + probe 2 byte + fixed pipe/control state exactly（allocator metadata/rounding を除く）で old/new-allocation transient はない。allocation failure は first store も free し user code 前の runner infrastructure failure。deadline は pre-spawn sample から acknowledgement、user execution、target signalling、group quiescence、descriptor drain、direct-child reap まで継続。exact output fit success、first extra byte は ack 前なら infrastructure、ack 後なら test failure。各poll wakeはqueued control datagramをnonblockingで`EAGAIN`/`EWOULDBLOCK`まで全てdrain。`waitid(..., WNOWAIT)`がleader terminalをobserveした後、completion missingをclassifyする前にcontrolを同じboundaryまで再drainする。normal completion sendはleader exitより先なので、このterminal-observation barrierはqueued fast-test recordを失わない。successful spawn と verified group establishment 後の全terminal pathは、leader PIDをreapせずpinned process groupを先、direct PIDを次にsignalする。ack後leaderがverified groupを離れていてもdirect targetは必須。上記unverified-group failureだけはtrusted group targetがなくuser codeも未実行なのでdirect-PIDだけcleanup。ordinary/test/infrastructure path は両targetへ順にSIGKILL。graceful SIGHUP/SIGINT/SIGQUIT/SIGTERM は両targetへ同signalを順にforwardしexactly 250 ms後に両方へSIGKILL。non-reaping observationを持たないrelease hostはfirst spawn前にreject。group ESRCHはsame-child terminal observationまたはsubsequent direct-PID signal成功後にacceptし、direct-PID ESRCHはnon-reaping observationが同じunreaped child terminalを証明した後だけaccept。他target errorはinfrastructureだが後続signal/observe/drain/close/reap stepはfixed orderで全てattempt。parentはterminal control barrierを完了し、accepted stream prefixをdrain、descriptor close後、EINTR retryでdirect childだけreap。in-group descendantはpinned group経由でsignalするがreapせず、そこをescapedしたdescendantはcontract外。quiesced resultはdescriptor close/direct-child reap後もcapture store両方をretainし、complete failure-block reportingまたはsilent pass discard後だけrelease。cleanup failureはbest effort後suite停止、selected reasonがあればretained bounded blockをinfrastructure diagnosticより先にemit。first graceful signalがsimultaneous outcomeより優先しcleanup後new diagnostic/summaryなしでraw `_exit(128 + signal)`、すなわち129/130/131/143でexitし、observerは`WIFEXITED`で見て`WIFSIGNALED`では見ない。fully reported/discarded/released ordinary resultだけnext rowを許す。 | Dedicated driver test-runner componentだけがsignal snapshot、allocation、descriptor、deadline、pinned PGID/direct PID、non-reaping observation、quiesced evidence、stage cleanupを所有。Cartesian ownerがpre/post-ack、idle-child nonblocking drain、first drain/terminal observation前後のcompletion、leader group retained/moved、descendant absent/present/escaped、全result class、4 signal、全boundaryのdeadline/output、全failpoint、reporting完了までのevidence retention、report/release前next-row禁止をcross。 |
| Reporting and exit | Exact terse-success/failure bytes、quiesced-row consumption、sink error/final guardはEnglish ledgerどおり。全runner writeはSIGPIPEをblockし、signal arbitrationの`Idle -> Writing` permitをraw syscallごとに取得する。Selectedならbyteなし、syscall中のsignalはWritingPendingとなり、そのcomplete/partial prefix後にSelectedへcommitしてlater syscallを禁止する。write failureはevidence/controllerをnon-returning guardへtransferし、stage cleanupとdiagnostic attempt後direct exitする。 | Goldenがsignal before permit/during syscall/after commit、full/short/zero/EINTR/EPIPE、stdout/stderr、final block/recheck、quiet-wrapper success/failure/interruptを覆う。 |
| Ownership, allocation, and effects | test declarationはruntime valueを所有しない。prefix/overlay/catalogはcompiler-owned immutable data。test-mode object/harness allocationと全build-stage ownerはdriver-ownedでsignal-controller acquisition前にconsumeされる。suite cwdはartifact前にcompiler-owned `PathBuf`へ一度snapshotし、全spawn actionがborrowするためper-row path allocationなし。live row/quiesced row/reporting/terminal guard、signal lease/SIGPIPE ownershipはEnglish ledgerどおり。test registry、reflection、hidden scan、history、retry、concurrency、shuffle、filter、fixture、snapshot、coverage、benchmark、network policyは追加しない。 | Sema/checked-HIR/codegenに加え、build-stage consumption、suite-cwd ownership、signal-lease RAII、row typestate、terminal guards、runner RAIIがowner。 |
| Cache and sources of truth | Test modeはdistinct versioned cache domain。unit/harness key、production HIR/descriptor projection、interface/implementation identityはEnglish ledgerどおり。one-invocation suite cwdはrunner inputだけでsource/HIR/object/harness/link/cache identityへ入らない。test-only offset shiftはcurrent spansを変え得るがsemantic projections/object/link/executable bytesは不変。更新順はこのfile、`draft.md`、language/design/open questions、frontend/roadmap/cache plan/test policy/HIR ledger/runtime ABI ledger/`docs/impl/21-build-perf-plan.md`/`docs/impl/22-repl-plan.md`/pkg.db design、同期Japanese mirrors。 | Projection match、prefix/overlay encoders、runtime registry、interface/implementation hashがowner。new field/variant/side tableはcompile-time update。全ownership/descriptor semantic mutation、orphan key、overlay DB consumer rejection、descriptor span、earlier test width shift、cwd identity twinをownerが固定。 |
| Cache projection tags | Domainはexisting domain encoder下のliteral UTF-8 bytes `align-production-codegen-v1`。structurally visited expressionごとにexact u8 ownership tag 1つ、`00` absent、`01` arena（`false`）、`02` individual（`true`）をappendする。descriptor sourceごとにexact u8 tag 1つ、`00` File + canonical `Option<str>` path-literal encoding、または`01` Inline + canonical decoded-string encodingをappend。その他scalar/option/sequence/enum/string/type/contract fieldは既存fixed canonical encoderを使い、unknown tag/unencodable lengthはcache lookup/publication前にreject。 | Independently implemented semantic-to-byte/byte-to-semantic fragment goldenがnew tag 5つを全てpin。complete projection ownerがfield order、sequence length、malformed tag/length、changed-field/unchanged-diagnostic-span productをpin。 |

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

module declarationを省略したentryのdefault `main` identityは、import sourceが明示的に
`module main`を宣言しない場合だけ使える。このcollisionはcatalog形成前にexact diagnosticでrejectし、
test name/source ordinal/catalog order validation前にmodule identityをuniqueにする。explicit entry pathは
ordinary duplicate-module ruleに従う。

## Checking and lowering

全 compiler command が test を parse する。semantic checking は declaration body の間だけ test
context を install する。normal same-module private item と imported public item を resolve し、
`core.test` import がある場合だけ2つの assertion を認める。context は called function/lambda へ
flow しない。nested ordinary block、`if`、`match`、loop、arena、unsafe block は同じ test body。

parserのordinary block ruleはglobalに維持する。newline/semicolonが`End`を作っても`}`直前のlast expressionは
AST tailになる。semaはAST parentからchild check前にplacementをstructurally assignする。test contextだけで、
root completionまたはenclosing block/control expression自体がcomplete statementのとき、syntactic slotの
exact imported assertionをfinal statementへreclassifyする。全Value edgeはexpected Unitでもnormalize前にreject。
checked HIRはparser-only builtin shapeやassertion value pathを
増やさず、statement-only assertion 1方式を保つ。

checked formation はtest bodyをcheckする前に、全ordinary-source function、generated helper、
monomorph、interned/nominal type、analysis fact、native capability、static descriptorをcompleteしてfreezeする。
test rootとtest checkingだけで生じる全artifactはseparate overlayへappendし、そのindexはproduction tableを
immutable prefixとして使う。productionに同一monomorphがすでにあればreuseし、test checkingはprefixを
upgrade/reorder/mutateできない。production loweringは両partitionをvalidateしてprefixだけをconsumeし、
test loweringはprefix/overlayをcombineして各catalog rootをexact `Result<Unit, Error>` ABIでemitし、
assertion locationをimmutable dataとしてemitする。function、全type-table suffix、test static descriptorからなる
complete overlay artifact graphは、catalog rootから全checked-HIR reference/generation edge、すなわちdirect
call、function value、callback/destructor descriptor、lifted target、nominal/interned type member、transitive
function/type/resource monomorph demandを通じてreachableなclosureと一致する。malformed
partition/catalog back-reference/name/ordinal/body result/assertion context/implicit-tail shapeは
MIR construction前にrejectする。validatorはgenerated symbol spellingからtest statusを推論しない。

Test cache lookup/native-capability collection/artifact allocation前に、combined viewはcatalog rootをcatalog
order、次にdependency-edge/structural HIR orderでwalkする。direct/imported call、function-value target、lifted
callback/destructor、concrete monomorphを全てedgeとし、reachable `ExprKind::ProcessCommand`をexact diagnosticで
rejectする。shared siteはfirst rootで一度だけ。unreachable production helperはfrozen prefixのinert test
object/ordinary runtime selectionへ残り得るが、harness/catalog rootから実行できない。handcrafted combined
viewにもvalidatorが同じruleを適用し、production
validationとordinary `process.command` behaviorは変更しない。

DB descriptorはoverlay static formationの明示例外。package contractは`db.query`/command constructorを
ordinary named top-level descriptor functionのcomplete bodyだけに認めるため、test body前にproduction
prefixへcloseする。testはcallできるが新しいDB descriptorは作れず、combined validatorは
`test_static_descriptors`内のDB consumerをrejectする。`alignc test`はproductionと同じoffline metadata/
`CheckedRequired` failureを使いDBをopenしないので、`db prepare`にtest optionはない。

test function は Impure。body が arithmetic だけでも `par_map`、task transfer、generic effect promise
に入れない。declarationとoverlayはinterface summaryに含めない。imported unitのtest root/suffixはdriver
がcombined test viewをexplicitに選んだときだけcompileする。

## Runner model

driverはimmutable test executable 1つをlink。generated `i32 @main()`だけがliteral entryを所有する。
test-mode codegenは全source-main shapeを既存`align_fn$4$6d61696e`へmapしordinary wrapperを出さず、
same-module testからexplicit callされた場合だけordinary functionとしてreach可能。entryはfd 3からlaunchを
受け、ordinalをvalidate/ackし、exact `align_test$<ordinal-as-eight-lowercase-hex>` rootをdispatch、return後
completionを送る。test-only programはmain不要。parentは同じexecutableをrowごとにsequential起動。

fd 3 datagram socket は stdout/stderr と別で application input を運ばない。parent は compiler-private
argv/environment value を渡さず、child から見えるのは argv[0] の stage path、inherited environment、
`/dev/null` stdin、captured stdout/stderr だけ。harness は launch acknowledge 前に fd 3 を close-on-exec
にするため successful `process.exec` は completion authority を inherit しない。parent endpointはspawn前に
nonblockingで、全parent receiveは`EAGAIN`/`EWOULDBLOCK`でpoll/deadline loopへ戻る。single launch recordを
待つchild endpointだけがblocking receiveを使う。child mappingはsource-is-3 caseを含めfd 3のclose-on-execを
明示的にclearし、harnessはlaunch後に再設定する。parentのfixed 21-byte receive capacityはvalid record 2種と
long datagramを区別する。valid acknowledgement
により harness setup と user execution を区別し、normal test return だけが safe completion producer。
completion bytes がなければ exit zero も failure となり `process.exit(0)` bypass を閉じる。unsafe code
は他の safe invariant と同じく descriptor boundary を破り得るが malformed/self-inconsistent datagram
は fail closed。

parent stdout/stderr read endpointもspawn前にnonblocking。readinessではcurrently queued bytesだけを
`EAGAIN`/`EWOULDBLOCK`までdrainしてpollへ戻る。live childの次byteを`read`内で待たない。
source-equals-target caseを含め、child mappingだけがfd 1/2のclose-on-execをclearする。parameterized
short-write-then-idle ownerが
capture streamごとにcontrol/status/signal/row deadlineのprogressを証明する。

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

Parent codecsとchild runtime codecsは別実装でvectorに照合する。child boundaryはexact
`align_rt_test_launch_recv_v1`、`align_rt_test_fd_cloexec_v1`、`align_rt_test_ack_v1`、
`align_rt_test_report_v1`だけ。decoderはfixed envelopeをbyte orderで読みreserved/conditional fieldを
validate後ordinal/statusと比較し、untrusted byte arrayをnative recordへtransmuteしない。

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
module-identity collisionはcatalog形成前、bad catalogはchecking後、reachable `ProcessCommand` walkはcatalog
validation後にrejectし、全てcache lookup/native capability/artifact allocation前に完了する。
CLI validation直後かつsource I/O前にsuite cwdを一度snapshotする。failureはexact `working directory`
infrastructure diagnosticでartifactなしとなり、source/catalog/cache/build/runner failureより先に確定する。

Accepted test option productのterminal consumer:

| Dimension | Admitted states | Sole terminal effect |
| --- | --- | --- |
| target CPU | default `baseline`、`native`、既存explicit LLVM CPU、last wins | 全unit/harness target machineとobject/harness key |
| profile | default/explicit `dev`、explicit `release|fast|small|tiny`、last wins | 全unit/harness optimizationとkey |
| runtime LTO | profile default、全profile explicit off、release/fastだけexplicit on、last wins | test artifact runtime-bitcode decisionとunit/link key |
| cache stats | absent/present、repeat idempotent | diagnosticだけ。artifact/order不変 |
| build jobs | last positive flag、次に`ALIGNC_JOBS`、次にavailable parallelism/1 | object worker boundだけ。catalog concurrency/artifact不変 |
| timeout/output | exact defaultまたは各1つのin-range value | row deadline/capture storeだけ。compiler/cache inputではない |

Parameterized ownerはprofile/LTO constraint適用後のCartesian product、default/explicit-equivalent twin、
各terminal consumerを観測し、CLI parse成功だけをcoverageとみなさない。

dedicated test-runner component 1つが complete state machine を所有し、CLI branch、harness codec、reporter
は独自に wait/signal/reap/catalog advance してはならない。state は次の通り:

| State | Ordered observable events | Required transition and invariant |
| --- | --- | --- |
| Ready/acquire | first graceful signal; capture allocation; pipe/socket/flags; suite-cwd/fd-close actions; clock; spawn; group proof | runnerはimmutable executable/catalog/limits/cwd/cache reportだけを所有し、全build ownerはnormal Drop済み。completed spawn planはcwd install、fd 0..3 replace、fd 4以上closeの順。cwd/remap/close-from failureはchild作成前infrastructure。残るsignal/group transitionはEnglish ledgerどおり。 |
| AwaitAck | first graceful signal; nonblocking control drain through EAGAIN/EWOULDBLOCK; nonblocking stdout; nonblocking stderr; non-reaping leader status; terminal control barrier; deadline | controlをstreamより先にdrainしqueued datagramなしならevent loopへ必ず戻る。各ready capture streamは`EAGAIN`/`EWOULDBLOCK`、EOF、excess、hard errorまでだけdrainし、live childのshort write後もevent loopへ戻る。valid ackで直ちにRunningへ入り、same drainをRunning ruleで継続するのでqueued ack+completionをone wakeでconsume。ack後otherwise idle childはstream/status/deadline処理をblockできない。pre-ack malformed/order error、deadline、first excessはlaunch infrastructure。leader terminal statusはreap/missing classifyせずrecordしQuiescingへ。post-terminal drainがrequired barrier。 |
| Running | first graceful signal; nonblocking stdout; nonblocking stderr; nonblocking control drain through EAGAIN/EWOULDBLOCK; non-reaping leader status; terminal control barrier; deadline | stdout excessがstderrより先、両方completion/timeoutより先。各ready stream/control socketはqueued dataだけを`EAGAIN`/`EWOULDBLOCK`、EOF、excess、hard errorまでdrainしてpollへ戻る。queued datagramをarrival orderで全consumeするためack/completion coalescingとrepetitionをstatus classification前にrecord。completion without statusはpending。empty queueは直ちにpollへ戻る。terminal statusはreap/missing reason選択なしでQuiescingへ。barrier後、same wake deadline前のcomplete valid completion/status productはdeadlineより先、それ以外はclosed precedenceに従う。 |
| Quiescing | selected graceful signal or mandatory signal; pinned-group then direct-PID targets; non-reaping terminal observation; final nonblocking control drain; remaining stdout/stderr; deadline; descriptor close; direct-child reap; cleanup errors | pinned groupとstill-unreaped direct PIDを順にreap前にsignalし、verified groupを離れたleaderもsecond targetで閉じる。terminal `WNOWAIT` statusを取得/保持し、controlを再度EAGAIN/EWOULDBLOCKまでdrain、accepted stdout then stderrをdrain、全row descriptor close後direct childだけreap。first drainがterminal observation直前にemptyでもsecond drain必須。pre-ack deadlineはlaunch infrastructure、post-ackはtest timeout。graceful signal最優先。それ以外のcleanup failureはselected outcome/storeを保持したinfrastructure。successful cleanupはchild/descriptorなし、両storeを所有したimmutable quiesced rowを生成。 |
| Reporting | first graceful signal; pass discard or failure-block write progress; report-write failure; store release | passはchild bytesを出さない。failureはquiesced rowのretained rangeからframingとbytesを直接write。complete reporting/pass discardで両storeをreleaseしBetweenへ。selected graceful signalはstore release、stage removal、controller retain後later syscallなしraw exit。既に`Writing`を持つ1 syscallのcomplete/partial prefixだけは残り得る。report failureはrow/storeをnon-returning writer guardへtransfer、live controllerをretainしたままstageをremove、stderr infrastructure line attempt後catalog advance/incomplete evidence release/ordinary controller teardownなしでdirect exit 1。 |
| Between rows | first graceful signal; next-row acquisition | live/waitable direct child、open row descriptor、capture storeはない。reported/discarded ordinary resultだけcatalog advance。infrastructureは停止、graceful signalはstage除去後exit。 |
| Finalize | first signal; stage removal; permit-aware summary; final mask/arbitration recheck; direct exit | stage cleanupがsummaryより先。raw summary syscallごとにWriting permitを取得し、pending signalはprefix後Selectedへcommitしてlater writeを禁止。complete summary後4 signalをblockしarbitration stateをrecheckしてdirect exit。 |

successful Quiescing後のordinary result orderはselected output/timeout、first arrival-ordered control
sequence/record error、record/status correlation、returned Error/Ok。first completion candidate内はlength、
magic/version、outcome、tag、reserved bytes、conditional code、ordinalの順。valid candidate後のlater
exact-length datagramがcompletion magic/versionを持てば`repetition`、その他は`order`。terminal barrier後の
completion candidate 0件は`length`。このstate-specific orderがscheduler timingをretry/error rewrite
inputにしない。

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

runner はacquisitionからquiescence/outcome consumptionまでbounded outputをretainしsuccess後はemitしない。repository validation
と同じ failure-only evidence policy であり、大きい passing suite も constant-size output、failure は
local evidence を保持。output は bytes のまま。failure 時 runner 自身の ASCII frame を書き nonempty
stream を unchanged replay。UTF-8 replacement、terminal escaping、line split、selected cap 未満の truncate
はしない。

output limit は silent truncation でなく correctness result。inclusive bound を超える first byte で fail/
terminate。zero bound は assertion diagnostic を含め empty output だけ許す。他 test が後で fail しても
successful output は suppressed のままなので1 failure が whole suite log を拡大しない。

Implementation owner commandとこのcapabilityを実行するCI phaseは`docs/impl/16-test-policy.md`の既存
`scripts/run-quiet.sh`/bounded-binary wrapperを使う。successはphase/aggregate summaryだけ、failureまたは
interruptはfailing unitのcomplete captured diagnostic logをreplayする。`ALIGN_QUIET_VERBOSE=1`だけが
investigation用escape hatch。wrapperはtest selection/concurrency/timeout/verdictを変えず、数千のpass caseでも
success logをphase数に比例させfailure evidenceを保つ。

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

`<record-detail>` は `order`、`repetition`、`length`、`magic/version`、`outcome`、`error tag`、
`reserved bytes`、`error code`、`ordinal` のいずれか exactly。first arrival-ordered protocol errorが勝ち、
そのcandidate内fieldは上のvalidation order。decimal number はleading plus/zero paddingなし。両streamの
output excessを同時検出した場合はevent-loop orderによりstdout。valid Err record + exit 1は
returned-Error line（assertionの`Error.Invalid`を含む）を選び、assertion locationはreplayed stderr bytesに残る。

runner infrastructure abort は stderr へ exactly
`alignc: test runner <operation> failed (os error <signed-i32>)` + LF。operation は
`stage create`、`stage cleanup`、`capture allocation`、`signal handler`、`stdin open`、`pipe`、
`control socket`、`descriptor flags`、`working directory`、`descriptor mapping`、`clock`、`control write`、`control read`、
`launch`、`spawn`、`process group`、`stdout read`、`stderr read`、`poll`、`close`、`wait`、`kill`、
`reap`、`report write`、`diagnostic write`のいずれか。mappingはclosed:

| Fallible site | Exact operation |
| --- | --- |
| private stage acquire / final remove | `stage create` / `stage cleanup` |
| capture store allocation | `capture allocation` |
| signal lease/mask/disposition/self-pipe/rollback/teardown | `signal handler` |
| child stdin `/dev/null` acquire | `stdin open` |
| stdout/stderr pipe create | `pipe` |
| fd-3 socketpair | `control socket` |
| CLOEXEC/nonblocking/descriptor flag | `descriptor flags` |
| suite `current_dir` snapshot、native-path conversion、cwd action construction | `working directory` |
| fd 0/1/2/3 remapまたはfd 4以上close-from action construction | `descriptor mapping` |
| monotonic/deadline clock | `clock` |
| parent launch sendまたはreserved child ack/completion failure | `control write` |
| parent ack/completion receive/drain | `control read` |
| protocol/ordinal/phase validation | `launch` |
| completed cwd/remap/close-from action planのprocess create/execute | `spawn` |
| setpgid/getpgid proof | `process group` |
| stream drain | `stdout read` / `stderr read` |
| readiness wait | `poll` |
| explicit descriptor close | `close` |
| non-reaping terminal observe | `wait` |
| group/direct-PID signal | `kill` |
| consuming child wait | `reap` |
| failure/summary sink | `report write` |
| infrastructure stderr sink | `diagnostic write` |

他operation wordを作ってはならない。numeric valueはraw OS code、allocation/validation、channel不能な
child-side control failure、zero-byte write、platformがcodeを出さないfailureではzero。earlier outcome後も
final suite summaryは出さず、既にemit済みまたはselectedされたfailure-block prefixは残る。
`diagnostic write` failureはfailed sink上でself-describeできないためpartial stderr prefixとincomplete row
evidenceを保持し、recursive lineなしでstage cleanupを実行、live controllerはvalidのままdirect exit 1。
compiler diagnostic/link errorは既存formatでsummaryなし。artifact-stage removalはall-pass/failed-suite
summaryより先なのでcleanup failureがpublished complete summary後にない。

reporterのprivate `write_no_sigpipe(fd, bytes)`はallocate/bufferしない。runner threadでoriginal maskをsnapshotし
SIGPIPEをblockした後、raw syscallごとにcontroller atomicの`Idle -> Writing` permitを取得する。Selectedなら
`write`を呼ばない。handlerはWriting中のsignalをWritingPendingにし、syscallのpositive/zero/error/EINTR結果後、
writerがcaller advance前にpendingをSelectedへ、signalなしならIdleへcommitする。EINTR retryも新しいpermitが必要。
従ってpermit前にsignalが勝てばbyteなし、syscall中ならそのcomplete/partial prefixがselection前に残り、その後の
syscallはない。complete rangeはSIGPIPE maskをrestore。zero/other errorはraw codeを持つnon-returning
`WriteFailureGuard`へevidence/controllerをtransferし、same permit protocolのdiagnostic attempt、stage cleanup、
direct exitまでretainする。platform ownerはfixed maskとatomic lock-freeをartifact前にprobe。ownerは全state transition、
blocked/unblocked/pending SIGPIPE、full/short/zero/EPIPE/EINTR、signal before/during/after、stdout/stderr/final recheckをcross。

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

production/test compilation は parsing/semantic rule を共有するが、semaはtest overlay形成前にproduction
prefixをclose/freezeする。test-only editはcomplete-source frontend lookupをinvalidateし得る。checked prefixは
current span/located metadataを保持するためearlier test editでshiftし得るが、production lowering/object
keyはcomplete semantic HIRのcanonical span-erased projectionとsemantic production-descriptor projectionを
encodeし、diagnostic span、located metadata、overlayを含めない。exhaustive HIR walkはspan-keyed map自体を
serialize/omitせず、structural orderで各expressionの`absent | arena | individual` ownership lookup resultを
emitする。descriptor walkはsemantic file-path literal/decoded inline SQLを含む全codegen-relevant fieldを
emitし、diagnostic locationだけをomit。これらprojection、MIR codegen graph、object key、link input、
executableはbyte-identicalでなければならない。versioned encoderはcurrent total-`Debug` lowering
fingerprintをfiltered renderingではなくreplaceし、exact cache-plan
transitionは`docs/impl/10-cache-first-optimization.md` §6.6が所有する。
test-mode keyはそのproduction identityに
全overlay suffix、local body、ordinal、canonical id、assertion location、permitted non-DB static descriptor、mode versionを加える。
harness keyはcomplete ordered catalog、sole-entry/source-main/root symbol mapping、4 child-runtime ABI、3つのcontrol protocol、terminal-commit versionを覆う。
import order changeはpublic module interfaceを変えずharness orderを変え得る。
one-invocation native suite cwdは全childへ使うrunner stateだけで、source/HIR/object/harness/link/cache
identityへ入らない。

Combined-view validatorはreachable `ProcessCommand`をunit/harness key lookup前にrejectするため、dynamic
command-supervisor/witness/status-codec cache inputはない。unreachable production command codeはordinary
production identity/runtime selectionを保ってfrozen-prefix test objectへinertに残り得るが、catalog-root
execution edgeを持たない。

executable は final child reap まで runner-retained `ArtifactStage` 以下にある。source-adjacent binary、
catalog file、history、snapshot、machine-readable public test artifact は生成しない。

## Implementation closure matrix

| Axis | Required implementation closure | Acceptance owner |
| --- | --- | --- |
| Syntax and formatting | `test` + string contextual lookahead、`test {}` / `pub test {}` type twin、rejected `pub test` + string、attribute/signature near-shape、newline/braces、recovery、depth cap、format idempotence | parameterized lexer/parser/AST/formatter owner |
| Catalog and modules | independent HIR module/name/id correlation、external catalog-root/function exact back-reference、全assertion idとenclosing catalog rootの一致、declared/default entry path、rejected default-entry/imported-declared-`main` collision + ordinary explicit duplicate、name/id/count bound、duplicate、module 間 same name、dependency-first diamond order、unimported exclusion、private/public access | whole/per-unit catalog golden、malformed-HIR identity/back-reference matrix、sema owner |
| Body and control | implicit Ok、explicit Err、`?`、`match`、`else`、assertion early exit、cleanup-bearing control join、malformed HIR | sema + checked-HIR + MIR control/Drop owner |
| Assertion surface | import rule、lexical context、statement-only checked HIR、root/nested statement-placement final syntactic tail normalize、expected Unitを含む全Value-edge reject、bool equality family、vector/mask non-Bool rejection、left-to-right once、nested/multiline conditionと独立したcomplete qualified-call spanの保持、line/column、first failure | parameterized parser-shape/sema-context owner + MIR/runtime diagnostic golden |
| Checked artifact partition | test前production freeze、catalog/root closure、test helper/monomorph/type suffix、permitted non-DB descriptor/capability、overlay DB consumer rejection、prefix mutation/referenceなし | malformed prefix/overlay、whole/per-unit semantic twin、generated closure、DB policy×driver、descriptor/capability owners |
| Production isolation and mode product | `align-repl` test-bearing entry transactional reject、全source commandがprefix選択、located MIR overlayなし、全DB descriptor prefix-owned、overlay link/export/interface/cache influenceなし、ordinary main不変 | REPL rollback/ordinal owner + command×watch/profile/target/LTO/ThinLTO/PGO/jobs/stats matrix、actual watch/ThinLTO/PGO artifact、DB metadata、byte-identical production owners |
| Test options and artifact | accepted option productのterminal consumer、CLI後runner-only suite cwd、sole harness `main`、4 source-main ABI、one link/inode、全whole/per-unit/harness build ownerのrunner前normal Drop、exact `align_test$<8hex>` | CLI Cartesian/terminal consumer、cwd failure/stability、build-stage absence-before-spawn、artifact/symbol owners |
| Test-callgraph process boundary | catalog-root direct/imported/function-value/lifted/genericとrecursively embedded implicit resource-Drop-hook edgeのreachability、deterministic first site、reachable `ProcessCommand` rejection、shared de-dup、unreachable production acceptance、malformed HIR、publication lock取得・production static-input resolution・static-artifact formation前のno-test/count/process validation、inert frozen-prefix command/runtime retention、production parity | direct/nested resource Dropとno-test/process × missing production-static-input precedence productを含むwhole/per-unit reachability matrix、checked-HIR negative、inert-prefix twin、unchanged production artifact owner |
| Launch protocol | snapshotted cwdの`addchdir_np` install、source-equals-targetを含むfd 0..3 remap、最後の`addclosefrom_np(4)`、ambient fdなしexact inventory、parent capture/control nonblocking、concurrent parent cwd/fd mutation、argv/environment/stdin、group proof、launch/ack product | driver/harness protocol matrix + cwd/acquisition/flag/remap/close-from failpoint |
| Child control runtime ABI | 4 exact symbol/signature、output init/alignment、borrowed fd/pointer、no allocation/close/retain/global state、全receive/CLOEXEC/ack/report product、exit120..123 phase map | LLVM/Rust parity、independent codec、registry/collision、whole/per-unit owner |
| Completion protocol | independent runtime encoder/driver decoder、3 golden、全malformed/order/cardinality/status product、exit/exec/abort/crash bypass | runtime/driver protocol matrix |
| Signal controller | shared lease、prior mask/disposition、SIGCHLD、setup/rollback、lock-free arbitration、全handler pathのinterrupted-thread `errno` exact preservation、summary retention/final recheck、signal before/during/after syscall | parameterized signal/handler-errno/writer/final-commit/wait-status owner |
| Runner state machine | Ready/AwaitAck/Running/Quiescing/Reporting/Between/Finalize x 全event、pre-spawnで一度作ってsignal/quiescence/drain/reapへリセットなしで渡すterminal row deadline、その同一total budget内でcleanup timeを予約するderived work cutoff、pre/post-ack deadline/output、nonblocking drain-empty/terminal/drain barrier、全outcome pinned-group then direct-PID signal before reap、quiesced evidence through report/release、terminal commitまでcontroller-owned summary | deterministic Cartesian state/event/typestate/nonblocking/barrier/final-commit + short original-deadline owner |
| Child lifecycle | preallocation、`/dev/null`、cwd/action、spawn/pipe/socket/flag/remap/close-from/clock/poll failpoint、fd 0..3 inventoryとambient fd 4/soft-limit probe、first spawn前build-owner absence、closed operation mapping、group/direct lifecycle | allocation/I/O-liveness/build-owner/diagnostic-operation/process-tree owner |
| Reporting | exact result bytes、failure-only evidence、全reason/signal/write product、Idle/Writing/Pending/Selected、partial-prefix-before-selection、terminal retention、quiet-wrapper | CLI/sink/writer-arbitration/final-exit/quiet owner |
| Cache identity | 全span-erased production/overlay/harness key fieldの独立mutation、per-expression `absent | arena | individual` ownership fact、orphan span-key rejection、全semantic descriptor field/diagnostic-only descriptor span、earlier variable-width test editでfrontend/located miss + production/descriptor span changeだがidentical semantic/descriptor projection/object hit、test descriptor/capability inclusion | cache hit/miss、ownership-stream/descriptor projection、prefix/suffix、canonical-key owner |

## Capability boundary and deferrals

implementation closure matrixはimplicit-effect/identity/deadline axis、すなわちresource Drop hookからのreachable
process construction、external catalog-root/assertion identity、qualified-call source location、cleanupを同total
budget内に予約するsingle terminal deadline周囲でre-openした。preceding artifact-formation-to-execution mode/ABI closureと
prior statement-placement、terminal-target、capture/control liveness、semantic side-table closureは維持する。
final launch/terminal-owner passはraw exit前build-stage discharge、suite-cwd install、ambient fd exclusion、
handler `errno` preservationも閉じる。
accepted codeはtop-level strict proof domain 2つ。compiler formationはsyntaxからhidden harness/exact control
codecまでを所有。dedicated driver test-runner component 1つは全compiler/build ownerがnormal Drop済みの
immutable artifact、ordered catalog、validated limit、snapshotted suite cwd、cache reportだけをconsumeし、
signal state、spawn/poll、deadline、process group、capture、reap、reportingをexclusive
ownership。どちらも他方のvalidation/lifecycle transitionを再実装しない。

compiler formation自体にもtyped seamがある。`CheckedProgram.production`はcomplete ordinary-source fixed
pointへ到達してfreezeした後に`TestOverlay`を形成する。overlayはprefix identityをread/reuseできるが、
新規test root、lifted/monomorph function、nominal/interned type suffix、static descriptor、capability consequenceを
全て所有する。production consumerはprefix viewだけ、test consumerはvalidated combined viewだけを取得できる。
prefix/suffix boundとcatalog reachabilityによりroot tagだけを見てtest helperをomitすることも、後でbodyを
discardしながらproduction idをperturbすることもできない。
DB Query/command descriptorはordinary named top-level constructorとしてprefix形成され、overlay consumerは
validatorがrejectし、testはordinary offline checked metadataをreuseする。

同じcombined-view reachability walkはcache/capability/artifact前に全test-reachable `ProcessCommand`をrejectする。
dynamic command subtree supervisionは行わず、direct/imported/generic/function-value/lifted routeをclosed static
productとして扱い、unreachable production helperと全production commandは不変。`align-repl`は`Item::Test`を
含むsubmitted entry全体をreplacement resolution/session mutation前にrejectする。これは`std.process`内の
hidden runtime special caseではなくexplicit first-capability boundary。

compiler formationはexplicit projectionをさらに2つ所有する。test-context semaはAST parentからchild check前に
assignしたroot completionまたはstatement placementだけでsyntactic `Block::tail` assertionをconsumeし、
expected Unitを含む全Value edgeはrejectする。checked-HIR assertionはstatement-onlyのまま。production
codegen/cache identityはcomplete span-erased semantic HIRとdescriptor projection。structural expression
walkはcurrent span-keyed side tableからMIRがobserveするownership factをencodeする。descriptor walkは全
semantic fieldをencodeしてdiagnostic locationだけをdiscardする。current spanはdiagnostic/located output用に
checked prefixへ
残るがobject identityをperturbできず、different ownership factをsame memo keyの後ろへ隠せない。

Artifact formationはexact entry/symbol boundaryを1つ持つ。harnessだけがliteral `main`、source main全ABIは
`align_fn$4$6d61696e`、global ordinalはreserved hidden `align_test$<8hex>`。target/profile/runtime-LTOは
unit/harness objectへ届き、jobs/cache stats/timeout/output/suite cwdは各scheduling/diagnostic/runner stateで終端する。
link後にwhole/per-unit `PipelinedPackageComplete`のprivate object stage、harness object stage、response/temp、
cache lease、joined worker/pipeline guardをnormal Dropし、non-returning runnerへfinal executable stageだけを渡す。
Production consumerはone-shot/watch、whole/per-unit、ThinLTO、PGOのadmitted product全体でprefix-only selectorを使う。

Child control seamはexact unkeyed function 4つだけ。runtimeがlaunch receive/fd CLOEXEC/ack/completion codec/send、
harnessがcatalog range/dispatch/reserved status、driverがindependent peer codecを所有する。互いのcodecをduplicateせず、
ABI row/protocol/reserved mappingはharness/runtime cache identityへ同時に入る。

各spawnはone native suite-cwd snapshotを`posix_spawn_file_actions_addchdir_np`でinstallし、fd 0..3を
replaceした後`posix_spawn_file_actions_addclosefrom_np(4)`を最後にappendする。harnessのfdはexactly
`/dev/null`、stdout capture、stderr capture、fd-3 controlだけでambient parent fdはない。embedding threadが
parent cwd/fd tableを後で変えてもsnapshotとchild inventoryは不変。

runner内では`LiveRow`がchild、descriptor、store、protocol state、deadlineを所有。parent capture/control
endpointはspawn前にnonblockingなので各drainはtyped
`Data | Empty(EAGAIN/EWOULDBLOCK) | Eof | Error`を返し、Emptyはpollへ
戻る。pinned-group then direct-PID signalling、non-reaping terminal observation、required second control
drain、descriptor closure、direct-child reapがこれを`QuiescedRow`へconsumeし、immutable outcomeと両capture
storeだけを所有。reporterだけが`QuiescedRow`を
consumeでき、complete failure-block writeまたはsilent pass discardがstoreをreleaseしてからcatalog
advance。terminal writer failureは`Reporting`をnon-returning guardへconsumeし、incomplete row/storeをdirect
exitまでretain。reportable rowを残したままstoreをreleaseするAPI、このterminal guardからreturnするAPI、
terminal barrier前にmissing completionをclassifyするAPIはない。fake-stage protocol/process-tree/
sink-failpoint ownerはAlign source compileなしでrunnerをvalidateし、whole/per-unit ownerは同じcodec boundaryに
対してproducerをvalidateする。

`FinalExitGuard`はartifact stageとsignal controllerを最初にconsumeする。stage removal失敗は両方をterminal
diagnostic/direct-exit pathまで保持する。成功後はlast summary writeまでcontrollerを保持し、4 graceful
signalをblockする。各raw syscallはatomic `Writing` permitを先にownし、その間のsignalは
`WritingPending`からsyscall prefix後に`Selected`となってlater permitを禁止する。final recheck後はraw
`_exit(128 + signal)`または`_exit(suite_status)`し、selected signalもnumeric `WIFEXITED`で
`WIFSIGNALED`ではない。ordinary teardown/return edgeはない。
このterminal commitはrestored ignored/custom handlerがsummary behaviorを変えることを防ぎ、published
successful summary後にfallible controller cleanupが続くことも防ぐ。
installed handlerはarbitration/self-pipe前にinterrupted threadの`errno`をsaveし全exitでrestoreするため、
async deliveryはunrelated syscallのobservable error stateを壊さない。

この2 domainはdesign PR後のpublic capability 1つとしてlandする。runnerなしparser-to-harnessと
compiler-private symbolなしrunnerはいずれもdormantで、splitはuseful stable consumerをpublishせず
mode/cache/ABI integration proofを重複させる。このためexpected hand-written diffは1,000 lineを超え得る。
explicit internal boundary、state/event matrix、independent ownerによりunusable prefixをshipせずriskを下げる。

first capability は全test-reachable call graphの`process.command`、`align-repl`内test declaration、test filtering/listing、parallel/shuffle execution、retry、fixture、setup/teardown hook、
snapshot、coverage、benchmark、ignored test、expected failure、persistent history、hidden file discovery、
assertion formatting/reflection system を明示的に除外。`process.command` wideningにはseparately reviewed
portable descendant-containment protocolが必要。他はsequential error-model coreで不足するreal consumer後だけ検討。

## Design-review finding closure

| Finding | Ledger-first closure |
| --- | --- |
| P1 final assertions became block values | Test-context semaはroot completionまたはstructural statement placementだけでexact syntactic-tail assertionをnormalizeし、checked HIRはstatement-only、expected Unitを含む全Value edgeはreject。 |
| P1 a blocking parent control receive could stop deadlines | 両endpointをclose-on-exec、parent endpointをspawn前にnonblockingとし、全drainをEAGAIN/EWOULDBLOCKで終える。Ack-only idle-child ownerを持つ。 |
| P1 group-only signalling could leave the direct child alive | 全verified terminal pathがpinned group、unreaped direct PIDの順にsignalし、leader-moved-group ownerとtarget別ESRCH ruleを持つ。 |
| P2 raw HIR byte identity included shifting spans | Cache/codegen identityはcomplete span-erased semantic projectionを使い、earlier variable-width test editがcurrent span/located metadataだけを変えてobject key/artifact bytesを保つ。 |
| P1 blocking capture drains could stop deadlines | 両parent capture read endをspawn前にnonblockingとし、全readiness drainをempty/EOF/excess/errorで終える。stdout/stderr short-write-then-idle ownerを持つ。 |
| P1 omitting span-keyed semantic side tables could collide | Structural expression orderがMIRのexact ownership factをencodeし、orphan keyをreject、semantic descriptor fieldにexplicit span-free projectionを持つ。 |
| P2 the cache plan retained the total Debug key | Focused cache planがversioned production projection、exact retention charge、transition boundary、owner cellを記録し、このledgerのsource of truthにも含める。 |
| P1 harnessとsource `main`がentryを二重所有 | Harnessだけがliteral `main`。4 source-main ABIはtest modeで`align_fn$4$6d61696e`を使いwrapperなし。absent/unreferenced/direct-call ownerを持つ。 |
| P1 test-only checked DB descriptorにprepare pathなし | DB constructorはordinary top-level production declarationだけ。testはoffline prefix metadataをreuseしoverlay DB consumerはreject。policy×driver ownerを持ちnew flagなし。 |
| P1 child reporting bytesにnative ABIなし | Launch receive、fd CLOEXEC、ack、completionの4 exact unkeyed ABIがsignature/validation/ownership/allocation/error/close/reserved status/registry/cacheを固定。 |
| P1 build mode/test option stateが未閉包 | Complete production mode productと各test optionのsole terminal consumerをledger化し、Cartesian selectorとactual watch/ThinLTO/PGO artifact ownerを持つ。 |
| P2 `/dev/null`/poll failureのoperationなし | Closed failpoint tableに`stdin open`、`descriptor flags`、`descriptor mapping`、`clock`、`process group`、`poll`を含める。 |
| P1 supervisor crash後のwitness EOFがnested-command cleanupを証明しない | Dynamic witness/sentinel designを撤回。catalog-reachable `ProcessCommand`をcache/capability/artifact前にstatic rejectし、全direct/imported/indirect/generic routeとunreachable production controlをowner化。 |
| P1 supervisor status channelにnonblocking target/abort arbitrationなし | Supervisor/status channel自体をshipしない。同じstatic exclusionでsecond runtime state machineなしにfailure domainを閉じる。 |
| P1 `align-repl`にtest item policyなし | Test-bearing submitted entry全体をreplacement/session mutation/compile/execute前にexact diagnosticでtransactional rejectし、state/next ordinal不変。 |
| P2 child controlが4/5 ABIで不一致 | Discarded containment ABIなし。launch receive/fd CLOEXEC/Ack/completionのexact 4 functionだけ。 |
| P2 selected signalのobservable statusなし | Final guardはraw `_exit(128 + signal)`を使い129/130/131/143 numeric `WIFEXITED`、re-raise/`WIFSIGNALED`禁止。 |
| P2 implicit entry `main`とimported declared `main`がalias | Catalog形成前にexact diagnosticでrejectし、explicit entry pathはordinary duplicate-module validation。 |
| P2 raw terminal exitがper-unit build-stage cleanupをskipし得る | Link後controller acquisition前に全whole/per-unit/harness build ownerをnormal Dropし、final executable stageだけrunnerへ渡す。first spawn前object-stage absence ownerを持つ。 |
| P2 childがambient descriptorをinheritし得る | fd 0..3 replace後`posix_spawn_file_actions_addclosefrom_np(4)`を最後に実行し、fd 4/soft-limit probeとexact child inventoryでambient fd不在を証明。 |
| P2 child working directoryが未指定 | CLI直後native cwdを一度snapshotし、failure/actionをexact `working directory`へmap、`posix_spawn_file_actions_addchdir_np`で全rowへ固定する。 |
| P2 signal handlerがinterrupted-thread `errno`を壊し得る | handler entry/全exitで`errno`をsave/restoreし、Idle/Writing/Selected、EINTR、pipe-full ownerを持つ。 |
