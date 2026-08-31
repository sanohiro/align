# std.log — 公開契約と実装設計

> 🌐 [English](../log.md) · **Japanese**

> **状態:** 設計済み。実装は `core.test` 後の次の capability である。

## 権威ある公開契約台帳

この表が最初の `std.log` capability の権威である。後続の本文はこれを説明してよいが、
拡張してはならない。以下の `logger` はソース上の名前 `log.logger`、`level` は
`log.level` で記す。

| 公開面 | 正確な入力、デフォルト、検証、評価 | 正確な結果、エラー、effect | 所有権、ライフタイム、allocation、cleanup | compiler/runtime/package の owner と identity | acceptance owner |
|---|---|---|---|---|---|
| `log.level { Debug, Info, Warn, Error, Off }` | 閉じた組み込みの tag-only 直和型。正確な tag は `Debug=0`、`Info=1`、`Warn=2`、`Error=3`、`Off=4` であり、この順序が severity 順である。数値変換、custom level、名前 parser、default、alias、ambient override はない。record level が有効なのは、それが `Off` ではなく、その tag が logger の minimum tag 以上である場合に限る。minimum `Off` は全 record を無効にする。 | Copy かつ Pure。`log.level.Off` は無効化する値としてのみ有効であり、`enabled(Off)` は false、`line(Off, ...)` は何も出力しない。ソースから形成した値に対する level 操作は `Error` を返さず abort もしない。 | inline `i32`。borrow、allocation、Drop はない。 | `align_sema` が `import std.log` の背後にある一意な組み込み enum 定義と qualified type/variant 解決を所有し、HIR/MIR と native call は正確な `i32` tag を運ぶ。`align_interface` は既存の nominal named-type/enum 形式を serialize するため、interface encoding tag も format-version bump も不要である。 | import/type/variant の正例、ordinal と threshold の正確な Cartesian product、通常の enum 規則が要求する Copy/move/print/equality の負例、checked-HIR の wrong-enum-id/bad-variant 拒否。 |
| `log.new(output: writer, minimum: log.level) -> log.logger` | 正確に 2 個の positional argument を取り、default はない。argument は左から右へ評価する。`output` は完全な owned writer source でなければならず、両 argument の check 後にのみ consume される。成功 path では source slot を null にする。`minimum` は閉じた 5 tag のいずれかである。stdout/stderr/file constructor も environment/config lookup もない。 | 初期化済み logger を 1 個返す。Pure であり、write、flush、close、clock、process、thread、その他の externally visible I/O を行わない。ソースから形成した construction に recoverable error はなく、terminal OOM は言語全体の abort policy に従う。 | 返される Move handle は logger shell allocation 1 個と consume した writer handle を所有する。logger は writer allocation と optional 64 KiB buffer の唯一の owner になるが、descriptor provenance は変えない。owning writer は引き続き fd を close し、static standard-stream writer は process fd を borrow し、connection-derived writer は引き続き `tcp_conn` を borrow する。正確に `region_of(LogNew) = region_of(output)` で `Ty::Logger` は region-tracked であるため、borrowed-descriptor logger は local、carrier、field、return のいずれを通っても writer owner より長く生存できない。path/text view は保持しない。Logger Drop は underlying writer を通常の best-effort flush-then-close-if-owned path で free し、その後 logger shell を free する。 | `Ty::Logger`/`Scalar::Logger`、writer-derived region fact を持つ `ExprKind::LogNew`、MIR construction、keyed native constructor 1 本。runtime shell は private で `{ writer, minimum, first_error }` を持ち、第 2 の runtime owner ではなく compiler provenance が borrowed fd を live に保つ。公開型は structural ではなく nominal。`log.logger` は既存の `IType::Named` spelling として interface を越え、その return-region summary、reachability、`std.log` capability は既存の interface/implementation hash と compiler/runtime ABI fingerprint に入る。 | arity/type/import/shadowing、完全/不完全な static/owning/connection-derived writer source、local/function/direct-imported/function-value/branch/loop/`?`/`map_err` construction、region propagation、escape rejection、source nulling、tagged/field/return provenance、shell 1 個の allocation/free count、no-I/O constructor、malformed checked-HIR、whole-program/per-unit imported signature。 |
| `l.enabled(level: log.level) -> bool` | `l` は bound initialized logger local または許可された borrow receiver でなければならず、read-only borrow される。level argument は receiver の後に eager evaluation される。`Off`、minimum 未満の tag、または sink failure が一度でも latch された後は false、それ以外は true。environment を読まず、fd を probe しない。 | total、Copy、Pure。failure latch を clear/変更せず、`Error` を返さない。 | move、retained borrow、allocation、I/O、state mutation はない。bool は logger より長く生存できる。 | `ExprKind::LogEnabled` と private logger state に対する keyed native predicate。HIR validation は一意な `log.level` enum と正確な Bool result を要求する。 | failure 前の minimum 5 × record 5 matrix と failure 後の 5 record level、borrowed receiver、no-mutation/I/O/allocation、import と wrong-receiver/type の負例。 |
| `l.line(level: log.level, message: str|string|builder) -> ()` | `l` は bound initialized logger でなければならず、call 中 mutably borrowed される。consume はしない。receiver、level、message を eager に各 1 回だけ評価する。owned `string` は `str` として auto-borrow され、builder は borrow されて consume されない。gating は通常の argument evaluation 後に call 内で行う。skip される message construction 自体も避ける必要がある場合、caller は `if l.enabled(level) { ... }` を使わなければならない。disabled、`Off`、または既に failure 済みの call は sink I/O を行わず state を変更しない。enabled call では、最初の sink write より前に完全な text view を検証する。text は UTF-8 であり、embedded NUL と tab を許可する。byte transform は正確かつ allocation-free である。`\` は `\\`、LF は `\n`、CR は `\r` になり、その他の UTF-8 byte はすべて不変である。 | Unit かつ Impure。正確な record は level prefix（`[DEBUG] `、`[INFO] `、`[WARN] `、`[ERROR] `）、変換済み message byte、最後に LF byte 1 個である。最初の nonzero writer status を latch し、その record の残りは試行しない。call 自体は sink failure に対して `Error` を返さず、abort、record retry、byte rollback、fallback destination への write もしない。partial record はあり得る。後続 `line` は Drop まで sink I/O を行わず、`flush` が最初の failure を公開する。 | message は call 中だけ borrow する。`line` は owned allocation 0、O(message bytes) の scan 中に O(1) extra memory を使う。caller が作った `string`、builder、template allocation は可視のままで call 外にある。writer の既存 buffering と ownership を再利用する。syscall count、cross-thread/process atomicity、delivery、durability、異なる logger 間の ordering、terminal-safety は保証しない。 | str/string と builder の別 HIR form は keyed native text/builder entry に lower する。native layer は既存 writer mechanism を呼び、prefix/escape scan と first-status latch を所有する。null logger は dereference 前に `AL_INVALID` を返す。live logger では既存 latch を最初に返し、次に closed level を検証する。invalid level は earlier failure がない場合だけ `AL_INVALID` を latch/return する。次に gate し、suppressed call は message byte を inspect せず 0 を返す。enabled call だけが signed length、representability、nullness、UTF-8 の順で、すべて最初の write 前に検証する。length 0 は null を許し、positive length は許さない。検出可能な malformed enabled input は `AL_INVALID` を latch/return する。dangling non-null logger/text pointer または builder は compiler-private pointer-range precondition 違反である。 | 全 level、empty/non-ASCII/NUL/tab/backslash/LF/CR/mixed text の exact byte、str/string/builder/template parity、disabled/post-failure no-write、eager evaluation と guarded allocation の twin、first-piece/middle/newline/underlying-buffer failure injection、partial-output/no-fallback、zero-allocation owner、unbuffered/buffered writer、malformed ABI/checked-HIR、whole/per-unit codegen。 |
| `l.flush() -> Result<(), Error>` | `l` は bound initialized logger でなければならず、mutably borrowed されるが consume はしない。以前の first error が latch 済みなら、writer に触れる前にそれを選ぶ。それ以外は underlying writer を正確に 1 回 flush する。他の logger state や ambient input は関与しない。 | Impure。error が latch されておらず underlying flush が成功すれば `Ok(())`。以前または新たに返された nonzero status は 1 個の固定 std errno/status table を通して map し、その正確な first error で `Err` を返す。新しい flush error は return 前に latch する。failure 後の repeated flush は sink I/O なしに同じ error を返す。successful flush は logger を disable せず、後続 line を許可する。 | allocation、move、close、retained borrow はない。explicit flush が source-visible な唯一の error observation path である。Logger Drop は latch 後も underlying writer の best-effort flush/close-if-owned cleanup を呼ぶ。その最後の cleanup error は writer Drop と同様に観測不能である。 | `ExprKind::LogFlush`、既存の status-to-`Error` MIR helper、keyed native flush entry 1 本。HIR validation は `Result<(), builtin Error>` を固定する。 | empty/success/failure/repeated-failure/success-then-line、正確な固定 status mapping、latch replay の no-I/O、全 outcome 後の Drop、early return/`?`/`map_err`、malformed HIR/runtime null owner。 |

### 正確な level/prefix/gate 表

行は logger minimum、列は要求された record level である。`Y` は示した prefix を書き、
`-` は disabled である。latch 済み logger では全 `Y` が `-` になる。

| Minimum \ record | `Debug` (`[DEBUG] `) | `Info` (`[INFO] `) | `Warn` (`[WARN] `) | `Error` (`[ERROR] `) | `Off` |
|---|---:|---:|---:|---:|---:|
| `Debug` | Y | Y | Y | Y | - |
| `Info` | - | Y | Y | Y | - |
| `Warn` | - | - | Y | Y | - |
| `Error` | - | - | - | Y | - |
| `Off` | - | - | - | - | - |

timestamp、source location、target、process/thread id、structured-field map、JSON mode、multiline
mode、terminal escaping、rotation、file opening、dynamic minimum setter、asynchronous queue、fatal
level、global/default logger、logger 固有の formatting API はない。logging は通常の eager な
Align expression と shipped `template`/`builder` method（`write`、`write_int`、`write_bool`、
`write_char`、`write_float`）を使う。`write_hex` は shipped builder method ではなく、この
capability でも導入しない。

### placement と transfer の台帳

`log.logger` は第 2 の ownership model を定義せず、通常の bare Move-handle class に従う。

| position または transition | 契約 | 必須 owner |
|---|---|---|
| local、by-value/`borrow`/`borrow mut`/`out` parameter、direct function return | 既存 Move 規則の下で許可する。by-value transfer は完全な source を null にし、borrow は null にしない。region-bound writer から作った logger はすべての transfer でその region を保持し、descriptor owner から escape できない。 | type formation、call mode、MoveCheck、EscapeCheck、region/return summary、return cleanup、interface round trip。 |
| user struct field、direct builtin `Option`/`Result` または user-sum payload | 既存 single-owner handle grammar が `writer` を許可する位置で許可する。enclosing value は Move になり active logger を正確に 1 回 recursive Drop する。field move-in/move-out、replacement、consuming match、`else`、`?`、`map_err` は既存 aggregate/tagged ownership に従い、全 carrier が logger の writer-derived region を保持する。 | field/payload formation、DropPlan、active-tag cleanup、region propagation/escape、complete-source nulling、replacement と branch/loop join。 |
| array、slice、fixed array、tuple、`box`、builder element、parallel element/result、closure/task capture、global/constant、user native/extern ABI | MIR 前に拒否する。これらは必要な ownership proof なしに 1 個の opaque owner を copy、隠蔽、parallelize、externalize し得る。 | rejected edge ごとの diagnostic owner と fail-closed new-type tripwire。 |
| method receiver | bound initialized local または許可された borrow place のみ。`log.new(io.stderr, log.level.Info).line(...)` のような unbound owned temporary は拒否し、先に bind する。 | shared owned-handle receiver gate と exact diagnostic。 |
| `if`、`match`、`else`、`?`、`map_err`、block tail、loop-carried value、early return | 通常の complete-source と path-local ownership fact が全 join/exit で live owner 1 個を証明する場合だけ許可する。rejected path は runtime action を publish しない。 | parameterized Move/Drop/control-flow matrix。 |
| Drop、`process.exit`、abort、successful `process.exec` | ordinary scope exit と `process.exit` は logger cleanup を実行し、その内部で logger shell より先に writer を free する。既存の immediate abort と successful exec は cleanup を skip する。logging 固有 hook や global flush はない。 | 既存 lifecycle twin と logger fd/allocation probe。 |

### native ABI delta

実装は exactly 6 個の通常の keyed runtime record を追加する。正確な key、symbol、LLVM
declaration、Rust ABI は次のとおりである。

| Runtime key | Symbol | 正確な LLVM declaration | 正確な Rust ABI |
|---|---|---|---|
| `LogNew` | `align_rt_log_new` | A114: `ptr @SYM(ptr, i32)` | `unsafe extern "C" fn(*mut Writer, i32) -> *mut Logger` |
| `LogEnabled` | `align_rt_log_enabled` | A115: `i32 @SYM(ptr, i32)` | `unsafe extern "C" fn(*mut Logger, i32) -> i32` |
| `LogLine` | `align_rt_log_line` | A116: `i32 @SYM(ptr, i32, ptr, i64)` | `unsafe extern "C" fn(*mut Logger, i32, *const u8, i64) -> i32` |
| `LogLineBuilder` | `align_rt_log_line_builder` | A117: `i32 @SYM(ptr, i32, ptr)` | `unsafe extern "C" fn(*mut Logger, i32, *mut Builder) -> i32` |
| `LogFlush` | `align_rt_log_flush` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut Logger) -> i32` |
| `LogFree` | `align_rt_log_free` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut Logger)` |

これらの row には curated return/parameter/function attribute を約束しない。`LogFlush` と
`LogFree` は既存 ABI shape A03 と A62 を再利用する。shipped compiler-private `core.test`
row が A110 から A113 を所有し、logging の他の 4 個の exact declaration は新しい shape
A114 から A117 である。6 個すべての key、symbol、declaration、definition、collision
reservation、export-parity row、runtime ABI fingerprint、whole/per-unit selection、checked-in
declaration golden を atomic に有効化する。その実装は現在の exact inventory
314/331/339 keyed/base/maximum record を 320/337/345 に変更し、shape range を A117 まで
拡張する。optional feature または target-dependent row はない。

`LogNew` は唯一の live writer pointer と checked tag を受け取る。compiler-generated call は
常に initialized writer 由来の non-null provenance と `0..=4` を渡す。construction は pointer
を返される non-null logger へ transfer し、compiler は source slot を null にする。null writer
または invalid minimum は allocation/consumption なしで null を返す。dangling non-null pointer
または uniquely owned でない writer は、この compiler-private ownership ABI の外である。
terminal allocator failure は abort する。dereference 前に、`LogEnabled` は null logger または
invalid level に 0、`LogLine` と `LogLineBuilder` は null logger に `AL_INVALID`、`LogFlush` は
null logger に `AL_INVALID` を返し、`LogFree` は null-safe である。live logger に対する
`LogLine` と `LogLineBuilder` は successful/gated call 後に 0、既存 latch 後に stored
status、最初の failure では newly latched status を返す。MIR はその値を意図的に捨てる。
builder pointer は live で call 中 borrow されなければならない。検出可能な invalid text
shape は公開 row の validation/latch order に従う。
source checker と checked-HIR validator が runtime selection 前に bad tag/type/move を拒否する
ため、source program は foreign-precondition case を作れない。

private first-error field は正確な nonzero `i32` writer status を保存し、`Error` tag や
formatted text は保存しない。0 は no failure を意味する。runtime は status を返す前に検証し
latch する。MIR は public Unit のため `LogLine`/`LogLineBuilder` status を捨て、`LogFlush` は
既存の単一 status decoder を通して map する。新しい wire format、persisted record、
reflection table、environment input、package artifact は導入しない。

## 実装 closure matrix

実装は 1 回の preflight review 前にこの matrix を author-side で閉じなければならない。
1 個の parameterized owner が複数 cell を閉じてよい。既存 regression coverage がその defect
で failure しない場合にだけ、新しい test が必要である。

| axis | 必須の実装 closure | 正確な regression owner |
|---|---|---|
| Type formation と import | `log.level`、`log.logger`、`std.log`、qualified type/variant lookup、spelling、shadowing guard、builtin capability を登録する。unimported/wrong-arity/wrong-type/collision edge を HIR 前にすべて拒否する。 | sema unit matrix と driver import/interface test。 |
| Construction と move-in | argument を左から右へ check し、完全な writer source 1 個を要求し、`LogNew` を形成し、正確な writer region を logger に copy し、両方の check 後だけ transfer し、その source だけを null にして logger drop flag 1 個を初期化する。Static、owning-fd、connection-borrowed writer は provenance を消さず同じ operation を使う。 | direct local、param、function result、block、`if`/`match`、`else`、`?`、`map_err`、loop-carried writer constructor matrix と direct/imported/function-value connection-writer escape twin。 |
| Move-out、replacement、return | canonical Move/region/drop predicate と全 ownership fact に Logger を sweep する。local/field/tagged move-out、replacement cleanup、direct/Result/user-sum return、early return、branch/loop join、use-after-move を cover する。 | `MOVE_HANDLE_TYPES` tripwire、handle-free-key sweep、MoveCheck/EscapeCheck/return cleanup test、allocation/fd balance。 |
| Drop と terminal path | logger を `LogFree` に map し、shell より先に underlying writer を正確に 1 回 free する。ordinary scope、partial initialization、moved source、active/inactive tag、replacement、function return、`process.exit`、abort、exec、malformed cleanup fact を cover する。 | runtime allocation/fd probe、lifecycle twin、checked-HIR cleanup mutation matrix。 |
| `enabled` | unique enum id、eager single evaluation、pure borrow、complete threshold table、latch suppression、exact Bool result を replay/MIR/codegen 全体で保持する。 | threshold 25 cell と post-latch row、effect/no-I/O/no-allocation/wrong-HIR test。 |
| `line` string | receiver/level/message order を保持し、enabled text を I/O 前に検証し、prefix、escaped run、LF を emit し、最初の nonzero status で停止して latch し、public status を捨てる。 | runtime byte golden、全 validation fault の before-side-effect、piece ごとの injected failure、buffered/unbuffered driver program。 |
| `line` builder/template | builder を consume せず borrow し、同じ UTF-8 byte sequence を読み、正確に同じ transform/status machine を再利用し、すべての logger outcome で builder ownership を不変にする。 | str/builder/template parity、line 後の builder reuse、disabled guarded/eager allocation twin、failure injection。 |
| `flush` と error mapping | writer I/O より前に既存 latch を読み、それ以外は writer flush を 1 回呼んで failure を latch し、1 個の MIR helper で map し、Ok/Err/`?`/`map_err` で logger を保持する。 | status Cartesian table、repeated-flush no-I/O probe、success 後の continued logging、owner-control-flow test。 |
| HIR/replay/validation | depth、clone/replay、effect、ownership、borrow/region、finalization、traversal、cache/semantic projection、checked-HIR validation、malformed-input fail-closed switch に全 new expression を追加する。`tracks_region(Logger)` と `region_of(LogNew)` は local、carrier、field、direct/imported/function-value call、return 全体で正確な writer fact を consume しなければならない。wildcard が new form を silent classification してはならない。 | variant sweep tripwire、child/result/enum id/effect/ownership/region fact の one-field mutation、replay identity と borrowed-writer escape matrix。 |
| MIR/runtime selection | typed MIR form と全 6 `RuntimeKey` row を追加し、reachable operation だけを select し、whole-program/per-unit parity を保持し、malformed type/tag を LLVM 前に拒否する。 | MIR validation mutation test、runtime-key inventory/bijection、unused import/no-selection と operation ごとの selection test。 |
| LLVM/native ABI | exact declaration/call と opaque pointer を emit し、level/status に i32 を使い、6 row すべてを typed registry と base export に置く。hand-written declaration bypass はない。 | exact extern-type matrix、declaration golden、key/symbol reverse lookup、base/maximum export parity、rt-LTO on/off。 |
| Interface/cache/generics | `log.logger` と `log.level` を既存 nominal name で serialize し、parameter mode/effect/return-region/return cleanup を保持し、imported generic user を runtime ownership の重複なしに instantiate し、capability/runtime fingerprint を含める。encoding grammar が変わらないため FORMAT_VERSION は 8 のまま。 | producer/consumer signature/return-region golden、generic whole/per-unit borrowed/static parity、two-build determinism、surface use による interface hash change と private span-only edit での不変。 |
| Allocation/resource promise | successful `new` ごとに logger shell allocation 1 個、`enabled`/`line`/`flush` の owned allocation 0、line scratch O(1)、writer/logger free 各 1 回。caller の template/builder allocation は別に観測できる。 | str/builder、escaping/non-escaping/disabled path、Drop の `alloc-count` delta、long-message の bounded-RSS または allocation-count owner。timing benchmark ではない。 |
| Diagnostics と docs | exact source spelling `log.level`/`log.logger` を使い、bound receiver と `enabled` guard を説明し、English/Japanese design mirror と normative summary を同期する。実装 ship 前に end-user guide で API を教えない。 | diagnostic assertion、doc diff と mirror/anchor consistency、example syntax check。 |

実装は 1 capability boundary である。その type owner、logger runtime state、call、Drop は strict
producer-to-consumer chain であり、独立して有用な dormant midpoint がない。3 compiler layer
を越え、およそ 1,000 hand-written line を超える可能性があるが、分割すれば利用可能な
consumer を残さずに Move-handle、checked-HIR、ABI、allocation proof を重複させる。したがって
この matrix が必要な cross-cutting boundary の説明である。

### Design-review finding-to-fix ledger

| finding | ledger decision と closure |
|---|---|
| P1: connection-derived writer の consume で borrowed-fd lifetime が消える可能性があった | descriptor provenance を保存し、`tracks_region(Logger)` とともに `region_of(LogNew) = region_of(output)` を定義する。public ledger、placement rule、construction/HIR/interface matrix、specification summary、direct/imported/function-value と tagged/field/return owner がその region を保持して test する。 |
| P1: 提案した shape A110–A113 は既に `core.test` が使用していた | shipped child-control row を runtime ABI ledger で明示的に予約し、4 個の new logging declaration に A114–A117 を割り当てる。logging の exact range と inventory delta は registry と一致する。 |
| P2: malformed null logger の挙動が runtime acceptance owner と矛盾した | null を受ける全 entry に dereference 前の result を定義する。`LogEnabled` は 0、line/builder/flush は `AL_INVALID` を返し、free は null-safe のままである。`LogNew` も null writer または invalid minimum を allocation/consumption なしで拒否する。 |

## rationale と usage

logger は process-global policy ではなく明示的な data である。sink、minimum level、failure
checkpoint、lifetime は source に可視のままである。

```align
import std.io
import std.log

fn run(count: i64) -> Result<(), Error> {
  out := io.stderr.buffered()
  l := log.new(out, log.level.Info)

  l.line(log.level.Info, "ready")
  if l.enabled(log.level.Debug) {
    l.line(log.level.Debug, template "items={count}")
  }

  l.flush()?
  return Ok(())
}
```

`enabled` guard は construction cost を制御するもので、正しさには不要である。guard がない
場合、通常の eager evaluation により `line` が Debug disabled と知る前に template が作られる。
conditional piece または shipped numeric formatter が必要なら、明示的に組み立てた builder を
同じ logger に渡せる。

CR/LF と escape marker 自体を escape することで、replacement string を allocate せず、成功した
各 record に正確に 1 個の physical LF delimiter を与える。3 byte-class transform を越えた
terminal-safe または reversible Unicode rendering は主張しない。tab、NUL、escape character、
bidi text、その他の Unicode control は caller data のままである。security-sensitive terminal
または machine protocol には、この primitive の上で明示的な encoder または structured package
が必要である。

best effort とは、通常の work が diagnostic write ごとに branch する必要がないことを意味する。
silent success を意味しない。delivery を重視する caller は `flush()?` で明示的な checkpoint を
選ぶ。最初の failure を保持することで checkpoint は deterministic になり、後続 write が元の
原因を隠すことを防ぐ。flush しない caller は通常の writer Drop と同じ unobservable error を
受け入れる。

formatting は 1 mechanism のままである。template と builder は allocation と conversion を既に
可視にするため、`std.log` は variadic argument も reflection-based field formatting も加えない。
logger は time と source metadata も意図的に省く。どちらも明示的な record に hidden effect または
compiler injection を追加するためである。
