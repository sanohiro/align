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
| Declaration grammar | private top-level declaration は1つだけであり、`test`、通常の Align string token 1つ、block 1つの順で書く。`test` は identifier のまま、item-position two-token lookahead が `test` + string のときだけこの contextual item にcommitする。bare `test {}` と `pub test {}` は keyword-less type declaration のまま。`pub test` + string lookahead は rejected visible-test form にcommit。commit後はparameter、type parameter、明示 return type、expression-body `=`、attribute、missing block、末尾 declaration nameをtest-specific recoveryでreject。declaration は callable/name binding を作らない。 | Lexer は `test` を identifier のまま保ち、AST/parser/formatter が contextual item を所有する。parser/formatter round-trip、`test`/`pub test` type twin、lookahead/recovery、全 reject near-shape を parameterized owner 1つで固定する。 |
| Name, identity, and catalog | decode 後の name は 1..=256 UTF-8 byte で Unicode C0/C1 control を含まない。U+0000..U+001F と U+007F..U+009F を rejectする。同一 module 内の duplicate name は reject。canonical public id は `<canonical-module-path>::<decoded-name>`。entry source は宣言した module path を使い、module declaration を省略した場合だけ `main` を使う。完全な id は 1..=1,024 UTF-8 byte。explicit entry/import closure 全体で test は最大 65,535。catalog order は既存の dependency-first DFS unit order（direct import は source order）、次に各 unit の source declaration order。探索対象はその explicit closure だけであり、directory、filename、annotation、manifest discovery は追加しない。 | Sema が validation/canonical id、driver が ordered catalog を所有する。declared/default entry path、exact C0/C1 boundary neighbor、name/id/count limit、duplicate scope、entry/imported `main` twin、diamond-import order、unimported file の無視を catalog owner が固定する。 |
| Body type and control | test は compiler-private zero-parameter `fn() -> Result<(), core.Error>` として check する。書かれた block が Unit で完了した後、construct が文書化された `Ok(())` tail 1つを補う。`?`、`return Err(...)`、`match`、`else`、arena、Drop、既存 control form は従来の semantics を保つ。明示 non-Unit tail または他 type の `return` は reject。通常の Ok/Err と assertion exit は ordinary function cleanup を実行し、hard error、`process.abort`、successful `process.exec` は従来の no-unwind behavior のまま。 | Sema/HIR/MIR が flagged private test function と implicit tail を所有し、public interface entry は出さない。control-flow、cleanup、Error variant、malformed checked-HIR owner が direct/per-unit lowering を覆う。 |
| Assertions | `import core.test` があるとき、exactly `test.expect(condition)` と `test.expect_eq(left, right)` が使える。lexical test body とその ordinary nested block 内の standalone statement に限る。ordinary parserはown terminated lineでも`}`直前のlast expressionを`Block::tail`にするため、semaはAST parentからchild check前にplacementを固定し、root test completionまたはenclosing block/control expression自体がcomplete statementのときだけ、そのsyntactic slotのexact assertionをfinal statementとしてconsumeする。全Value edgeはconsumerがUnitを期待してもreject。admitted assertionはUnit fallthroughを残してtestのimplicit Okへ進み、function、lambda、constantでもreject。`expect` は exact `bool`。`expect_eq` は既存 `==` の type/admission rule を適用し、その comparison result が exact `bool` であることも要求し、left、right の順に exactly once 評価する。ordinary result が `maskN` になる vector/mask comparison は assertion-only reduction を加えず reject。success は Unit で allocation なし。failure は canonical test id と 1-based call line/column を含む bounded diagnostic 1つを書き、enclosing test から `Err(Error.Invalid)` を返す。`expect_eq` は operand value を inspect/format しない。最初の failed assertion でその test を終える。 | Parser は ordinary qualified call/tail parsingを保ち、sema が imported test-only builtin/source identityを認識してadmitted syntactic assertion tailをchecked HIR前にnormalizeする。checked HIRはcomplete `Stmt::Expr`だけにdiscriminatorを認め、MIR/runtime が diagnostic と early Err を所有。root/nested final-statement twin、expected Unitを含む全consumed-tail negative family、scalar/string equality、vector/mask rejection、eager order、first failure、cleanup owner が必須。 |
| Production commands and modes | `check` / `check-per-unit` はtest declarationをparse/type-checkし、`fmt`はformatする。`build`、`run`、`size`、`emit-mir`、`emit-llvm`、`emit-obj`、`explain-opt`、`db prepare`はcomplete checked resultをvalidateするがfrozen production prefixだけを選ぶ。test root、lifted helper、test-only generic/type/resource monomorph、interned type、non-database static descriptor、capabilityはoverlayに残る。`explain-opt`はlocated MIR/remarkからoverlayを除外。DB Query/command constructorはordinary named top-level descriptor functionのcomplete bodyだけでlegalなのでprefix freeze前に形成され、test overlayからは作れない。handcrafted overlay DB descriptorはchecked-HIR validationでreject。`db prepare`は既存production descriptor setをprepareしtest flagを持たない。`alignc test`はそのmetadataをoffline consumeし、missing/stale `CheckedRequired`はartifact前にfailしてDBへ接続しない。`emit-interface`はtest surfaceを一切exportしない。sourceなしcommandにtest inputはない。main必須commandでは通常のvalid mainが必要。prefix selectionはone-shot build/run/size、`build --watch`初回/全rebuild、whole/per-unit、全profile/target/runtime-LTO、release/fast ThinLTO、release/fast PGO instrument/useへ独立に適用する。 | Explicit prefix/combined-view inputを使う。listed verb × admitted watch/profile/target/runtime-LTO/ThinLTO/PGO/jobs/cache-stats productをparameterized ownerで覆い、actual watch rebuild/ThinLTO/PGO artifact、production/test MIR twin、generated/type isolation、DB policy×driver、interface/native capability exclusion、byte-identical production artifactをownerにする。 |
| Test command and options | `alignc test <entry.align>`はexplicit closureをcheckしhost executable 1つをbuild、catalogをsequentialに実行。user `main`は不要で自動実行しない。common optionは既存spelling/placement/duplicate ruleの`--target-cpu`、`--profile`、`--rt-lto`/`--no-rt-lto`、`--cache-stats`、`-j`/`--jobs`。targetはdefault `baseline`、`native`または既存LLVM CPU spelling、last wins。profileはexact `dev|release|fast|small|tiny`、last wins、test default `dev`。runtime LTOはrelease/fastでdefault on、他off。explicit onはrelease/fastだけ、offは全profile、last wins。cache-stats repeatはidempotentでdiagnosticのみ。last positive jobs flag、なければ`ALIGNC_JOBS`、次にavailable parallelismでbuild worker数を決め、catalog並列性/artifact bytesは変えない。target/profile/LTOは全unit/harness objectとcache keyへ届く。timeout/outputはrunnerだけ。`--watch`、ThinLTO、PGO、export、program args、unknownはbuild前reject。test-only option spelling/bounds/default/repeat/no-`--`はEnglish ledgerとexactly同じで、environmentはtimeout/outputを変えない。flag除去後はcommand+entry 1 path exactly。zero testはartifactなし、exact diagnostic、exit 1。 | Accepted target×profile×resolved-LTO×cache-stats×jobs-source Cartesian ownerとterminal-consumer assertionを使う。target/profile/LTOはobject/harness、jobsはbuild scheduling、cache statsはdiagnostic、timeout/outputはrow stateだけへ届くこと、default/limit/reject、no-main/user-main-not-run/zero bytesを固定。 |
| Build, entry symbols, and artifact | Test loweringはimmutable prefix+validated overlayをper-unit test-mode object graph 1つとgenerated harness 1つへ落とし、一度だけprivate `ArtifactStage`へlinkする。harnessだけがexternal literal `i32 @main()`を所有し、test modeはordinary source-main wrapperを出さない。許可済み4形（`() -> i32`、`() -> Unit`、`() -> Result<Unit,Error>`、`(array<str>) -> Result<Unit,Error>`）は既存collision-free encoded symbolを使い、source spelling `main`はexact `align_fn$4$6d61696e`。ordinary internal/reachabilityに従い、明示reach時だけretain。global catalog ordinal `n`はexact hidden external `align_test$`+8 lowercase hex（7は`align_test$00000007`）へmapし、このfamilyはcompiler-reserved。overlay helperはordinary private symbol。executableはsource stemへpublishせず、last reap後summary前にremove。同じinodeを各rowでreuseしper-test compileなし。 | Driver/per-unit codegen/harnessとreserved-symbol collision checkがowner。全4 source-main ABI × absent/unreferenced/direct call、literal main重複なし、whole/per-unit parity、prefix/overlay closure、single-link/same-inode、ordinal/symbol dispatch、hidden linkage、cleanup-before-summaryを固定。 |
| Signal controller | Artifact worker join後、child acquisition前にshared process-global leaseをacquireし、既存のSIGHUP/SIGINT/SIGQUIT/SIGTERM disposition/maskとSIGCHLD compatibilityを検証する。handlerは`SA_RESTART`なしでinstallし、preallocated lock-free atomicのclosed state `Idle/Writing/Selected(signal)/WritingPending(signal)`をCASする。`Idle`ならfirst signalをselectし、`Writing`ならpendingにする。reporterは`Idle -> Writing`を取得したraw syscallだけを実行でき、結果後にpendingをSelectedへ、signalなしならIdleへ戻してからadvanceする。従ってSelected後にwriteは開始せず、既にpermit済みのsyscall prefixだけがselection前に残る。self-pipe、rollback、returning teardown、non-returning final guard、prior handler restore ruleはEnglish ledgerどおり。 | Parameterized ownerがprior disposition/mask、SIGCHLD、second lease、全setup/rollback failpoint、lock-free probe、全atomic transition、simultaneous signal、write前/中/commit後、final block/recheckをcrossする。 |
| Parent-to-harness launch ABI | Parentは各rowにfd 3用`SOCK_DGRAM` control pairとfd 4用containment-witness pipeを作る。全original endpointはCLOEXEC、parent control/witness readはspawn前nonblocking。child actionはsource-equals-targetを含めcontrolをfd 3、witness writerをfd 4へmapして、その2つだけをstdio以外でinheritする。verified row group後にexact launch recordを送る。harnessはordinal/rangeをvalidateし、fd 3をCLOEXEC、fd 4をinstall-once containment witnessとしてCLOEXECにしてからexact Ackを送る。fd 4 writerはbyteを書かず、parentはleader terminal後にexact EOFをobserveするまでquiesce/descriptor close/reap/classify/stage removalできない。premature EOF/data/hard errorとfd-3/fd-4 install failureはpre-ack infrastructure。 | Codec/witness matrixがcontrol/witness acquisition/flag、fd 0..4 remap、source-equals-3/4、Ack idle、witness empty/data/EOF/error、全record malformed/order、group proof、deadline/outputを覆う。 |
| Capture transport | fixed store 2つのallocation後、parentは`/dev/null` stdin sourceをopenしstdout/stderr pipeを作り、全original endpointをclose-on-exec、両parent read endpointをspawn前にnonblockingにする。child spawn actionは`/dev/null`だけをfd 0、child write endpointをfd 1/2へmapし、全source-equals-target caseでmappingのclose-on-execをclearしてunused original stdio endpointを全close。parentはspawn後に`/dev/null`/child write-end copyをcloseし、blocking capture readを行わない。selected readinessごとにそのstreamをstore/probeへ直接、`EAGAIN`、`EWOULDBLOCK`、EOF、rejected next byte、hard errorのいずれかまでdrainしてevent loopへ戻る。live childのshort writeはcontrol、other stream、status、signal、deadlineを止めない。successful spawn前のopen/pipe/flag/remap failureは全acquired endpointをclose、両storeをreleaseし、user codeなしinfrastructure failure。 | Runnerがstdio/capture flag/drain stateを所有。parameterized ownerがstdout/stderrを独立に、short-write-then-idle child、simultaneous pressure、`EAGAIN`/`EWOULDBLOCK`、EOF、exact/rejected-next bound、source fd 0/1/2 remap、全open/pipe/flag/remap failpoint、partial drain後のdeadline/signal progressを覆う。 |
| Process isolation and completion record | acknowledgement済みtestはverified process groupで実行。normal returnだけがfd 3へexact 20-byte completionを送り、envelope/outcome/tag/code/ordinal bytesはEnglish ledgerのexact record。harnessはOkなら0、Errなら1をreturnし、process terminationがfd 3のsole ordinary close owner。ack後のarrival-order/field-order/cardinality、`repetition`/`order`/`length`、Ok+exit0/Err+exit1だけのvalid productは従来rowどおり。exit/abort/exec/crashはsuccessを偽装できない。 | Generated harnessと下のexact compiler-private runtime ABIがchild receive/fd state/encoding/send、parent driverがindependent launch encoderとack/completion decoder/cardinalityを所有。semantic/byte goldenは両方向と全malformed productを固定。 |
| Child control runtime ABI | Test artifactだけが5つのunkeyed symbolをdeclareする: `align_rt_test_launch_recv_v1`、`align_rt_test_fd_cloexec_v1`、`align_rt_test_containment_install_v1`、`align_rt_test_ack_v1`、`align_rt_test_report_v1`。exact LLVM/Rust signatureはEnglish ledger/runtime ABI ledgerどおりで全て`nounwind`。containment installはopen fd 4だけを受け、CLOEXECを加え、runtime atomicをdisabledからborrowed descriptor identityへexactly once変更する。repeat/wrong/closedは`EINVAL`、ordinary pathはclose/replace/resetしない。他4関数のcodec/validation/errorは従来rowどおり。5関数ともallocationなしでfd 3/4をcloseせず、containment installだけchild-global stateを変更する。reserved exitは120..123でfd-3/fd-4 setup failureを121へmapする。 | Declaration/export/registry/count/collision、install state、codec、CLOEXEC/exec、reserved phase、whole/per-unit link ownerが必須。 |
| Time, output, and child cleanup | Exact 2-stream store/probe boundとdeadlineはfd-4 EOF/direct harness reapまで継続。Outer terminalはrow group、direct PIDの順にsignalし、control/capture barrier後もwitness EOF前にquiesceしない。untimed/unbounded commandはrow group内。timed/bounded commandはまずsentinelをforkし、sentinelはrow group外へ移ってfd 4/private abort pipe/private status pipeをretainした後、外部targetをdistinct group leaderとしてforkする。targetはgroup proof/private-fd close完了までpre-exec gateでblock。harness death/abort EOFではsentinelがtarget group、unreaped direct targetの順にSIGKILLし、targetをreap、group existenceが`ESRCH`になるまでpollしてからfd 4をclose/exit。normal terminalもsame cleanup後、English/std.process ledgerのexact 16-byte `ALTCMDS` v1 recordでpublic codeまたはraw runtime errorを返す。live harnessはcodec/EOFをvalidateしてsentinelをreapし、death後OS orphan reaperがownするのはsentinel statusだけ。targetはprivate fdをexec後inheritしない。deliberate `setsid` escapeは既存contract外。fd-4 EOFは全sentinelのtarget cleanup完了を証明し、status result 7/error 5 goldensと全malformed productをownerが固定し、outer classify/release/advance/stage cleanupはそれより後。 | Cartesian ownerがwitness state、全sentinel/target/gate/group-absence phase、nested descendant、harness return/exit/exec/abort/SIGKILL、全result/signal/failpoint、EOF前quiesce禁止をcrossする。 |
| Reporting and exit | Exact terse-success/failure bytes、quiesced-row consumption、sink error/final guardはEnglish ledgerどおり。全runner writeはSIGPIPEをblockし、signal arbitrationの`Idle -> Writing` permitをraw syscallごとに取得する。Selectedならbyteなし、syscall中のsignalはWritingPendingとなり、そのcomplete/partial prefix後にSelectedへcommitしてlater syscallを禁止する。write failureはevidence/controllerをnon-returning guardへtransferし、stage cleanupとdiagnostic attempt後direct exitする。 | Goldenがsignal before permit/during syscall/after commit、full/short/zero/EINTR/EPIPE、stdout/stderr、final block/recheck、quiet-wrapper success/failure/interruptを覆う。 |
| Ownership, allocation, and effects | Live rowはexact capture stores、child/control/capture/witness descriptorをownし、fd-4 EOF後だけchild/descriptorなしのquiesced rowへconsumeされる。Test childはinstall-once borrowed fd-4 identity、各bounded-command sentinelはprivate abort/status pipe/target group/direct childとfd 4をexitまでownする。runner signal lease/self-pipe/arbitration atomicとchild containment atomic以外のnew global stateなし。report/store/terminal guard/SIGPIPE mask ownershipはEnglish ledgerどおり。 | RAII/typestate ownerがfd-4 identity、sentinel lifecycle、EOF前quiesce/release禁止、last-write前release禁止と全allocation/descriptor failpointを固定する。 |
| Cache and sources of truth | Test modeはdistinct versioned cache domain。unit keyはcanonical production identity、overlay suffix、catalog/body/assertion/static descriptor、mode/target/profile/codegen/import/runtime ABIを含む。harness keyはordered catalog、sole-entry/source-main mapping、5 child-runtime ABI、launch/Ack/completion/containment-witness/command-sentinel-status/final-commit versionを含む。production HIR/descriptor span-free projectionとpublic-interface exclusionはEnglish ledgerどおり。更新順には`docs/impl/std-design/process.md`と同期Japanese mirrorも含む。 | Projection/registry/hash ownerが全semantic mutation、orphan key、overlay DB consumer、span shift、ABI/witness protocol mutationを固定する。 |
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

fd 3 datagram socketはapplication inputを運ばないcontrol channel。fd 4 pipeはwrite-only lifetime witnessで、
harnessと各active bounded-command sentinelがwriterをretainするがbyteを書かず、runnerがnonblocking readerを
ownする。childから見えるcompiler-private inputはこの2 descriptorだけ。harnessはAck前にfd 3をCLOEXEC、
fd 4をinstall-once containment witnessとしてCLOEXECにするため`process.exec`はどちらもinheritしない。
parent control/witness endpointはspawn前nonblockingでemptyならpoll/deadlineへ戻る。source-equals-targetを含め
fd 3/4 mappingだけがspawn時CLOEXECをclearし、harnessが再設定する。fixed 21-byte control capacityとexact
witness empty/EOF stateがmalformed inputをfail closedする。normal returnだけがcompletion producerで、
leader terminal後のwitness EOFは全sentinelがnon-liveであることを独立に証明する。

Test containmentはAck前にinstallされ、language-callable process APIを増やさない。untimed/unbounded commandは
row group内。timed/bounded commandはまずsentinelをforkし、sentinelはrow group外へ移りfd 4/private abort pipeを
retainしてから外部targetをdistinct group leaderとして作る。targetはgroup proofまでpre-exec gateでblock。
harness death/pipe EOFでsentinelはtarget group/direct targetをkill、target reap、exact group absence後だけfd 4を
closeする。normal completionもpublic codeを保ってsame residual cleanupを行い、live harnessはsentinelをreapして
`run`を返す。death後OS orphan reaperがownするのはsentinel statusだけ。fd-4 EOFはtarget cleanup完了を証明。
deliberate `setsid` escapeは既存contract外。

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
| Ready/acquire | first graceful signal; capture allocation; stdout/stderr pipe; control socket; witness pipe; flags; clock; spawn; group proof | 全acquired descriptor/storeをowner。verified leaderでAwaitAckへ。proof failureはdirect PIDだけcleanupし、witnessをdrain/closeしてreap、user codeなしで停止。 |
| AwaitAck | first signal; nonblocking control/witness/stdout/stderr; leader status; barrier; deadline | control/witnessはstreamより先にemptyまでdrain。terminal前witness data/EOFはlaunch infrastructure。valid Ackはfd-4 install proofでRunningへ。leader terminalはreap/classifyせずQuiescingへ入り、second control drainとwitness barrierを必須にする。 |
| Running | first signal; nonblocking stdout/stderr/control/witness; leader status; barrier; deadline | 全ready sourceはqueued stateだけをdrainしてpollへ戻る。live leader中のwitness data/EOFはinfrastructure。terminal status後はcontrol record productとwitness EOFを閉じてからprecedenceを決める。 |
| Quiescing | selected/mandatory signal; row group then direct PID; WNOWAIT; final control drain; witness EOF; streams; close; reap | row group/direct PIDをreap前にsignalし、controlを再drain、capture prefixをdrain、fd 4 exact EOFまでpollする。EOFがharness/全sentinel non-liveを証明した後だけdescriptor close/direct-child reap/quiesced row生成を許す。data/hard error/EOF前closeはinfrastructure。 |
| Reporting | first signal; pass discard/failure-block permit-aware write; report failure; release | passはsilent consume、failureはquiesced storeからdirect write。Selectedならnew syscallなし。complete report/discardだけstore release/Between。write failureはnon-returning guardへtransferしstage cleanup/diagnostic/direct exit、advance/early release/ordinary teardownなし。 |
| Between rows | first signal; next-row acquisition | live/waitable child、descriptor/witness writer、bounded-command sentinel、capture storeなし。reported/discarded resultだけadvance。 |
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
`control socket`、`descriptor flags`、`descriptor mapping`、`clock`、`control write`、`control read`、
`launch`、`spawn`、`process group`、`stdout read`、`stderr read`、`poll`、`close`、`wait`、`kill`、
`reap`、`report write`、`diagnostic write`のいずれか。mappingはclosed:

| Fallible site | Exact operation |
| --- | --- |
| private stage acquire / final remove | `stage create` / `stage cleanup` |
| capture store allocation | `capture allocation` |
| signal lease/mask/disposition/self-pipe/rollback/teardown | `signal handler` |
| child stdin `/dev/null` acquire | `stdin open` |
| stdout/stderrまたはcontainment-witness pipe create | `pipe` |
| fd-3 socketpair | `control socket` |
| CLOEXEC/nonblocking/descriptor flag | `descriptor flags` |
| spawn action constructionまたはfd 0/1/2/3/4 remap plan | `descriptor mapping` |
| monotonic/deadline clock | `clock` |
| parent launch sendまたはreserved child ack/completion failure | `control write` |
| parent ack/completion receive/drainまたはwitness read | `control read` |
| protocol/ordinal/phase validation | `launch` |
| complete mapping後process create | `spawn` |
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
harness keyはcomplete ordered catalog、sole-entry/source-main/root symbol mapping、5 child-runtime ABI、
launch/Ack/completion/containment-witness/command-sentinel-status/terminal-commit versionを覆う。
import order changeはpublic module interfaceを変えずharness orderを変え得る。

executable は final witness EOF/direct child reap まで runner-retained `ArtifactStage` 以下にある。source-adjacent binary、
catalog file、history、snapshot、machine-readable public test artifact は生成しない。

## Implementation closure matrix

| Axis | Required implementation closure | Acceptance owner |
| --- | --- | --- |
| Syntax and formatting | `test` + string contextual lookahead、`test {}` / `pub test {}` type twin、rejected `pub test` + string、attribute/signature near-shape、newline/braces、recovery、depth cap、format idempotence | parameterized lexer/parser/AST/formatter owner |
| Catalog and modules | independent HIR module/name/id correlation、declared/default entry path、entry/imported `main` twin、name/id/count bound、duplicate、module 間 same name、dependency-first diamond order、unimported exclusion、private/public access | whole/per-unit catalog golden、malformed-HIR identity matrix、sema owner |
| Body and control | implicit Ok、explicit Err、`?`、`match`、`else`、assertion early exit、cleanup-bearing control join、malformed HIR | sema + checked-HIR + MIR control/Drop owner |
| Assertion surface | import rule、lexical context、statement-only checked HIR、root/nested statement-placement final syntactic tail normalize、expected Unitを含む全Value-edge reject、bool equality family、vector/mask non-Bool rejection、left-to-right once、line/column、first failure | parameterized parser-shape/sema-context owner + MIR/runtime diagnostic golden |
| Checked artifact partition | test前production freeze、catalog/root closure、test helper/monomorph/type suffix、permitted non-DB descriptor/capability、overlay DB consumer rejection、prefix mutation/referenceなし | malformed prefix/overlay、whole/per-unit semantic twin、generated closure、DB policy×driver、descriptor/capability owners |
| Production isolation and mode product | 全source commandがprefix選択、located MIR overlayなし、全DB descriptor prefix-owned、overlay link/export/interface/cache influenceなし、ordinary main不変 | command×watch/profile/target/LTO/ThinLTO/PGO/jobs/stats matrix、actual watch/ThinLTO/PGO artifact、DB metadata、byte-identical production owners |
| Test options and artifact | accepted option productのterminal consumer、sole harness `main`、4 source-main ABI encoded/no wrapper、main absent/unreferenced/direct call、one link/inode、exact `align_test$<8hex>` | CLI Cartesian/terminal consumer、artifact、reserved-symbol、whole/per-unit owners |
| Launch protocol | source-equals-targetを含むfd 0..4 remap、all-original CLOEXEC、parent capture/control/witness spawn前nonblocking、launch/Ack、fd-4 install、witness phase product、group proof | driver/harness codec/containment matrix + acquisition/flag/remap failpoint |
| Child control runtime ABI | 5 exact symbol/signature、output init/alignment、fd-4 install exactly once、no allocation/descriptor close、launch/CLOEXEC/install/Ack/report全product、exit120..123 phase map | LLVM/Rust parity、install state、codec、registry/collision、whole/per-unit owner |
| Completion protocol | independent runtime encoder/driver decoder、3 golden、全malformed/order/cardinality/status product、exit/exec/abort/crash bypass | runtime/driver protocol matrix |
| Signal controller | shared lease、prior mask/disposition、SIGCHLD、setup/rollback、lock-free Idle/Writing/Selected/Pending arbitration、summary retention/final recheck、first-signal coalescing、signal before/during/after syscall | parameterized signal/writer/final-commit owner |
| Runner state machine | 全state×event、pre/post-ack deadline/output、nonblocking control/capture/witness barrier、row group/direct PID、mandatory witness EOF、evidence/report/final commit | deterministic Cartesian typestate/nonblocking/containment/final owner |
| Child lifecycle | preallocation/stdio/control/witness/failpoint、bounded/unbounded command、row外sentinel parent-before-target、exact status codec、全nested process/group-absence phase、harness return/exit/exec/abort/SIGKILL、terminal control+witness EOF、reap/evidence | allocation/I-O/diagnostic/nested-process/witness owner |
| Reporting | exact result bytes、failure-only evidence、全reason/signal/write product、Idle/Writing/Pending/Selected、partial-prefix-before-selection、terminal retention、quiet-wrapper | CLI/sink/writer-arbitration/final-exit/quiet owner |
| Cache identity | 全span-erased production/overlay/harness key fieldの独立mutation、per-expression `absent | arena | individual` ownership fact、orphan span-key rejection、全semantic descriptor field/diagnostic-only descriptor span、earlier variable-width test editでfrontend/located miss + production/descriptor span changeだがidentical semantic/descriptor projection/object hit、test descriptor/capability inclusion | cache hit/miss、ownership-stream/descriptor projection、prefix/suffix、canonical-key owner |

## Capability boundary and deferrals

implementation closure matrixはcomplete row-execution containment/output-commit product、すなわちouter
harness group、全safe `process.command` subgroup、witness ownership、leader/direct/sentinel cleanup、
signal selection対raw-write permit周囲でre-openした。preceding artifact mode/ABI closureとprior
statement-placement、terminal-target、capture/control liveness、semantic side-table closureは維持する。
accepted codeはtop-level strict proof domain 2つ。compiler formationはsyntaxからhidden harness/exact control
codecまでを所有。dedicated driver test-runner component 1つはimmutable artifact、ordered catalog、validated
limitだけをconsumeし、signal state、spawn/poll、deadline、process group、capture、reap、reportingをexclusive
ownership。どちらも他方のvalidation/lifecycle transitionを再実装しない。

compiler formation自体にもtyped seamがある。`CheckedProgram.production`はcomplete ordinary-source fixed
pointへ到達してfreezeした後に`TestOverlay`を形成する。overlayはprefix identityをread/reuseできるが、
新規test root、lifted/monomorph function、nominal/interned type suffix、static descriptor、capability consequenceを
全て所有する。production consumerはprefix viewだけ、test consumerはvalidated combined viewだけを取得できる。
prefix/suffix boundとcatalog reachabilityによりroot tagだけを見てtest helperをomitすることも、後でbodyを
discardしながらproduction idをperturbすることもできない。
DB Query/command descriptorはordinary named top-level constructorとしてprefix形成され、overlay consumerは
validatorがrejectし、testはordinary offline checked metadataをreuseする。

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
unit/harness objectへ届き、jobs/cache stats/timeout/outputは各scheduling/diagnostic/runner stateで終端する。
Production consumerはone-shot/watch、whole/per-unit、ThinLTO、PGOのadmitted product全体でprefix-only selectorを使う。

Child control seamはexact unkeyed function 5つだけ。runtimeがlaunch receive/fd-3 CLOEXEC/fd-4 install-once
containment/Ack/completion codec、harnessがcatalog range/dispatch/reserved status、driverがpeer codecとwitness
readerを所有する。ABI/witness/reserved mappingはharness/runtime cache identityへ同時に入る。

runner内では`LiveRow`がchild、descriptor、store、protocol/witness state、deadlineを所有。parent
capture/control/witness endpointはspawn前にnonblockingなので各drainはtyped
`Data | Empty(EAGAIN/EWOULDBLOCK) | Eof | Error`を返し、Emptyはpollへ
戻る。untimed/unbounded commandはrow group、timed/bounded commandはrow外fd-4-retaining sentinelをtarget
creation前にarmする。sentinelがtarget group/direct targetをkill/reapしgroup absenceを証明する。pinned-group then direct-PID、
non-reaping terminal observation、second control drain、exact witness EOF、descriptor closure/direct reapが
これを`QuiescedRow`へconsumeし、immutable outcomeと両capture
storeだけを所有。reporterだけが`QuiescedRow`を
consumeでき、complete failure-block writeまたはsilent pass discardがstoreをreleaseしてからcatalog
advance。terminal writer failureは`Reporting`をnon-returning guardへconsumeし、incomplete row/storeをdirect
exitまでretain。reportable rowを残したままstoreをreleaseするAPI、このterminal guardからreturnするAPI、
terminal barrier前にmissing completionをclassifyするAPIはない。fake-stage protocol/process-tree/
sink-failpoint ownerはAlign source compileなしでrunnerをvalidateし、whole/per-unit ownerは同じcodec boundaryに
対してproducerをvalidateする。

`FinalExitGuard`はartifact stageとsignal controllerを最初にconsumeする。stage removal失敗は両方をterminal
diagnostic/direct-exit pathまで保持する。成功後はlast summary raw syscallをlock-free arbitrationでsignal
selectionとserializeし、4 graceful signalをblock、stateをrecheckしてdirect exitする。ordinary
teardown/return edgeはない。
このterminal commitはrestored ignored/custom handlerがsummary behaviorを変えることを防ぎ、published
successful summary後にfallible controller cleanupが続くことも防ぐ。

この2 domainはdesign PR後のpublic capability 1つとしてlandする。runnerなしparser-to-harnessと
compiler-private symbolなしrunnerはいずれもdormantで、splitはuseful stable consumerをpublishせず
mode/cache/ABI integration proofを重複させる。このためexpected hand-written diffは1,000 lineを超え得る。
explicit internal boundary、state/event matrix、independent ownerによりunusable prefixをshipせずriskを下げる。

first capability は test filtering/listing、parallel/shuffle execution、retry、fixture、setup/teardown hook、
snapshot、coverage、benchmark、ignored test、expected failure、persistent history、hidden file discovery、
assertion formatting/reflection system を明示的に除外。sequential error-model core で不足する real consumer
が現れた後だけ additive に検討する。

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
| P1 child reporting bytesにnative ABIなし | Launch receive、fd CLOEXEC、containment install、Ack、completionの5 exact unkeyed ABIがsignature/validation/ownership/allocation/error/close/reserved status/registry/cacheを固定。 |
| P1 build mode/test option stateが未閉包 | Complete production mode productと各test optionのsole terminal consumerをledger化し、Cartesian selectorとactual watch/ThinLTO/PGO artifact ownerを持つ。 |
| P2 `/dev/null`/poll failureのoperationなし | Closed failpoint tableに`stdin open`、`descriptor flags`、`descriptor mapping`、`clock`、`process group`、`poll`を含める。 |
| P1 bounded `process.command` subgroupがrow cleanup後もlive | Row外fd-4-retaining sentinelをtarget creation前にarmし、harness death/normal completionでtarget group/direct targetをkill/reap、group absence後だけwitness close。runnerはaggregate EOF前にquiesceしない。 |
| P2 signal selectionとreport writeがrace | Lock-free atomicがIdleからWriting permitとSelectedを排他化し、syscall中signalはWritingPending、結果後Selectedへcommitしてlater syscallを禁止。 |
