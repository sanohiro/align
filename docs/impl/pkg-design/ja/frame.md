# pkg — frame

> [English](../frame.md) · **日本語**
>
> **注意:** 英語版 (`../frame.md`) が正本。本書は同期ミラーである。
>
> **ステータス:** 設計候補。独立レビューが完了するまで公開契約は受理されない。

## 公開契約台帳

V1 は、結果を既存パイプラインへ渡す、上限付きの安定 inner equi-join ひとつだけである。
問い合わせ言語でも、第二の列・スキーマ体系でもない。

| 公開表面 | 入力・既定値・検証・評価 | 結果・エラー・順序・効果 | 所有権・寿命・割当・後始末 | owner・同一性・受理 owner |
|---|---|---|---|---|
| `pub RowPair { left: i64, right: i64 }` | `left`、`right` は 0 始まりの入力行 ordinal。負値や隠しフィールドは生成しない。物理・ソース順はこの順。 | Copy、Pure。レコード全体の新しい等値規則は加えない。 | 借用・割当・Drop を持たない。`array<RowPair>` は通常の owned 動的 AoS 配列。 | `pkg.frame` が nominal 定義を所有し、完全な定義グラフが interface/dependency/cache 同一性へ入る。既存配列と whole/per-unit owner が受理する。 |
| `pub JoinError { InvalidLimit, LimitExceeded }` | 閉じた payload なし sum。ordinal は `InvalidLimit=0`、`LimitExceeded=1`。 | 前者は `max_pairs < 0`。後者は right-build index が対象で表現不能、または結果が上限、i64 長、出力 byte 範囲を超えること。OOM は hard abort。 | 通常の `{ i32 tag }`。借用・割当・Drop なし。 | `pkg.frame` が定義と ordinal を所有する。通常の enum/Result と exact mapping owner が受理する。 |
| `pkg.frame.inner_join_i64(left: codec.i64_column, right: codec.i64_column, max_pairs: i64) -> Result<array<RowPair>, JoinError>` | 引数は左から 1 回ずつ評価。入力は生きた正規 `core.codec` i64 view。長さは異なってよく、空でも、同じ batch/storage generation でもよい。`max_pairs` に既定値はなく、範囲は正確に `0..=i64::MAX`。値は decode 後の符号付き i64 `==` で比較。 | 全一致対を left-row-major、同一 left 内は right ordinal 昇順で返す。重複は安定 Cartesian 積。負の上限は入力読取・割当前に `InvalidLimit`。right index 表現不能または次の 1 対が上限、i64 長、出力 byte 範囲を超える時は出力を公開せず `LimitExceeded`。Pure。 | 入力は呼出中だけ借用し保持しない。非空成功は通常の `array<RowPair>` 出力 1 個を所有し、空成功は `{null,0}` で出力割当なし。right hash/index scratch は処理所有で全 return 前に解放。error は出力なし。 | direct/imported/local/function-field/joined indirect の全 call が通常の public wrapper を実行し、その唯一の private root bridge call が専用 checked operation と inactive A121 候補を選ぶ。target-shape parity、join matrix、割当 parity、malformed-HIR、whole/per-unit/cache、ABI/export が owner。 |
| `pkg.frame.inner_join_str(left: codec.str_column, right: codec.str_column, max_pairs: i64) -> Result<array<RowPair>, JoinError>` | 評価・寿命・長さ・共有・上限規則は i64 と同じ。等値は byte-exact `str == str`。UTF-8 は `codec.open` が検証済み。NUL/LF は通常の byte で、正規化・locale・case fold・collation・key copy はない。 | 同じ安定 inner join と error。hash 一致後に必ず長さと byte を確認する。 | 文字列 byte も呼出中だけ借用。結果は ordinal だけを持ち両 batch を保持しない。 | inactive A122 候補を i64 行と同じ境界で有効化。owner は NUL/LF/multibyte/common-prefix/collision、別 batch の同一 byte、入力非保持も覆う。 |

## 決定と範囲

```text
検証済み typed codec column
  -> 上限付き安定 inner hash join
  -> 通常の array<RowPair>
  -> 既存 array/slice pipeline と明示的 typed codec access
```

`codec.batch` は既に動的列 metadata を検証して所有し、typed projection が kind 検査を明示する。
したがって別の `Frame` wrapper、文字列による列選択、schema 推論、joined column 実体化、query-plan
値は加えない。caller が既存の `find` と typed projection を行う。欠落列・wrong-kind policy は caller
側に残り、join 自体の失敗は可視の resource bound だけになる。

V1 は出荷済み SoA `group_by` と等値/hash 基盤を共有する i64 と str のみ。bool は実消費者がなく、
f64 は IEEE equality と整合する `-0.0`/NaN hash canonicalization の決定が必要なので入れない。

## 公開利用

```align
import core.codec
import pkg.frame

fn join_ids(
  left: codec.i64_column,
  right: codec.i64_column,
) -> Result<array<pkg.frame.RowPair>, pkg.frame.JoinError> =
  pkg.frame.inner_join_i64(left, right, 1000000)
```

```align
fn matched_left_rows(
  pairs: slice<pkg.frame.RowPair>,
) -> array<i64> = pairs.map(fn pair { pair.left }).to_array()
```

宣言と呼出式を分離して示している。named argument、method overload、暗黙 import、reflection、未実装構文は使わない。

## 正確な join 意味論

長さ `L` と `R` に対する意味結果は次のソース順積である。

```text
for left_ordinal in 0 .. L
  for right_ordinal in 0 .. R
    if left_key[left_ordinal] == right_key[right_ordinal]
      emit RowPair { left: left_ordinal, right: right_ordinal }
```

実装は hash join だが、この membership と順序に一致しなければならない。right を常に build side
とし、長さや profiling で切り替えない。right ordinal はソース昇順の collision-safe chain に入り、
left は count/bound と exact fill の 2 回 probe する。hash-table iteration order は観測不能。

i64 と str offset は alignment 1 の little-endian load で読む。str key は隣接 offset 間の検証済み
byte 範囲そのもの。既存 runtime byte hash を固定 seed で共有し、等値を必ず確認する。host endian、
pointer/allocator address、thread count、process configuration は結果を変えない。

上限は inclusive。ちょうど `max_pairs` は成功し、次の 1 対で `LimitExceeded`。残りを数えるために
走査しない。`count * 16` が i64 または対象 allocation size で表現不能な場合も割当前に同 error。

right 長を `R > 0` とし、`Q = R + ceil(R / 3)`、`C` を `max(8,Q)` 以上の最小 2 冪とする。
index の logical layout は i64 head table `C` 個、tail table `C` 個、next-link table `R` 個で、
right row は ordinal 昇順に bucket chain へ append する。`Q`、`C`、`16*C + 8*R` が i64 と対象
allocation-size domain に収まらなければ `LimitExceeded`。`R == 0` は index 不要。3 logical table
が 1 allocation を共有するかは公開 promise ではない。

## 検証とエラー優先順位

1. nonnull かつ正しく整列した writable output header を要求し、`{null,0}` に初期化する。
2. `max_pairs < 0` を両入力に触れる前に private invalid-limit status で拒否する。
3. left、right の順に private view を検証する。row 長は非負かつ対象で表現可能。正長なら必要な
   data/offset 範囲は nonnull。str は `(rows + 1) * 4` offset 範囲と、final offset が正なら data
   pointer を要求する。保護 arithmetic/pointer 検査前に slice/reference を作らない。
4. 上記の正確な `Q`、`C`、`16*C + 8*R` を scratch 割当前に検証する。対象で表現不能なら、
   意味結果が空でも割当前に `LimitExceeded`。
5. right index を ordinal 順に build し、left を ordinal 順に probe/count。caller limit を各
   would-be pair で先に検査し、i64/output-byte representability と同じ `LimitExceeded` に写す。
6. 非空出力を正確に 1 回割当、再 probe、正準順で fill 後に `{ptr,len}` を公開する。空は出力割当なし。
7. 全 return 前に scratch を解放し、error は入力も部分出力も保持しない。

private status `-1` は `InvalidLimit`、`-2` は `LimitExceeded`、0 は成功だけに写す。正の
`AL_INVALID` は malformed compiler-private ABI であり、producer-valid lowering では生じないため
hard abort する。direct-ABI multi-invalid owner が全境界を固定する。

## 所有権、region、割当

codec 引数は入力 buffer の region/storage-generation fact を持つ Copy view で、動的呼出期間だけ
借用する。move、null、mutation、store、return、結果への fact 付与は行わない。結果に残るのは
ordinal だけなので、呼出後に source owner を move/replacement できる。

`array<RowPair>` は `{ptr,len}` の通常の Move AoS 配列。要素 size は 16、alignment は
`{i64,i64}` の対象 ABI alignment、空は `{null,0}`、Drop は既存の null-safe array Drop。
非空成功は exact-size output allocation 1 個を公開する。scratch は right row 数で有界で全 return
前に解放する。OOM は即時 abort。

既存 Result/array owner が move-in/out、source nulling、destructure、`if`、`match`、`else`、`?`、
`map_err`、branch/loop join、replacement、early return、unused/result Drop を覆う。

## package、compiler、runtime、ABI 境界

vendor 可能な `pkg.frame` が 2 定義・2 public wrapper・2 private root bridge を所有する。direct、
imported、local/function-field、control-joined function value はすべて同じ通常 wrapper を実行し、
その本体の唯一の private bridge call だけを compiler が正準 package source/signature/definition
graph の検証後に認識する。他 module の同名関数や変更された package は通常の call。Sema は
bridge signature/evaluation/purity/region と 2 checked discriminator、checked
HIR は canonical package identity・input kind・result identity・fallthrough・非保持、MIR は status・
ownership/cleanup、LLVM は typed call/output reconstruction、runtime は hash engine・count/fill・
allocation/cleanup を所有する。

正準 root module は `core.codec` と `std.process` を import し、英語正本に示す exact source shape
を持つ。private 名は `inner_join_i64_bridge` / `inner_join_str_bridge`、各 signature は対応 public
wrapper と同一で、body は `process.abort()` placeholder。public wrapper の完全な single-expression
body だけが対応 bridge を同じ 3 引数で 1 回呼ぶ。compiler はその位置だけを discriminator にし、
別位置、helper、変更 wrapper、private bridge の function value は package admission で拒否する。
public wrapper 自体は通常の callable value のままである。

設計中は次の行を inactive とする。

| 候補行 | symbol | ABI shape | Rust ABI |
|---|---|---|---|
| A121 | `align_rt_frame_inner_join_i64_v1` | `i32 @SYM(ptr, i64, ptr, i64, i64, ptr)` | `unsafe extern "C" fn(*const u8, i64, *const u8, i64, i64, *mut AlignStr) -> i32` |
| A122 | `align_rt_frame_inner_join_str_v1` | `i32 @SYM(ptr, ptr, i64, ptr, ptr, i64, i64, ptr)` | `unsafe extern "C" fn(*const u8, *const u8, i64, *const u8, *const u8, i64, i64, *mut AlignStr) -> i32` |

両方 C calling convention、`nounwind`。実装 body に対する証明前は curated attribute を持たない。
最終 pointer は aligned writable `AlignStr` header で、source `str` ではない。2 symbol、keyed row、
checked-HIR discriminator、package interface、owner は原子的に有効化する。それまでは registry、
collision reservation、total、fingerprint、capability identity に入らない。現在値からは
328/345/353 が 330/347/355 になり A123 が次だが、実装時に再計算する。

## 計算量と性能境界

通常の hash 分布では right build rows、left probes、確認 collision bytes、出力 pairs に比例する。
nested loop も出力 sort も行わない。公開 throughput/latency/allocation-count/peak ratio/SIMD/parallel
数値はない。`bench/frame_join` は one-to-one i64、duplicate fanout、equal-byte str、collision-heavy
str を測る非 gate のローカル証拠で、正しさ閾値ではない。

## V1 非目標

`Frame` wrapper、batch construction、schema reflection、name-based selection、materialization、filter、
sort、aggregate、group-by wrapper、query DSL/tree、lazy/optimizer/planner/SQL、file/mmap/stream、codec
output、mutable/nullable/composite/bool/f64 key、outer/semi/anti/cross/as-of join、predicate closure、
parallel/spill/distributed、automatic build-side choiceはない。将来能力は実消費者と新しい完全な台帳を必要とする。

## 実装 closure matrix

| 軸 | 必須 closure | owner 証拠 |
|---|---|---|
| 公開 formation/identity | 正準 vendored module、2 定義/2 wrapper/2 private bridge、同名非捕捉、ordinal/field order、whole/per-unit/generic reconstruction。全 direct/imported/local/function-field/joined-indirect target が wrapper から対応 bridge へ 1 回到達。 | import/name/signature positive と wrong-module/type negative、wrapper/bridge body、interface bytes/hash、parameterized target-shape parity、malformed definition/bridge。 |
| input region/evaluation | left/right/limit を順に 1 回、termination、全 carrier の generation、呼出後非保持。 | i64/str の direct/control/termination/source-invalidation matrix。 |
| join product | empty、unequal、no-match、duplicate、stable Cartesian、共有 batch、i64 edge、str byte class、collision。 | 独立 nested-loop oracle、order mutation、base alignment 0..7、endian twins。 |
| limit/precedence | negative/zero/exact/next、right-index load-factor/capacity/byte overflow、output overflow、固定 validation order、部分公開なし。 | direct ABI multi-invalid、empty-result representability twin、failpoint、全 error で null/zero header。 |
| output/control | exact array reconstruction、全 move/control/Drop path。 | Drop counter、driver control matrix、MIR cleanup/null assertion。 |
| ABI/allocation | A121/A122、status、pointer/length/alignment、LE read、provenance、nounwind、全 cleanup。 | registry/export/attribute mutation、malformed ABI、allocation parity、optimized/unoptimized/rt-LTO。 |
| hash engine | 固定 seed、stable chain、collision equality、key copy なし、2 probes 一致、bounded scratch。 | i64/str unit、forced collision、pass count、no-copy、count/fill parity。 |
| compatibility/cache | codec/group_by/pipeline 不変、package absence 不変、exact identity invalidation、ambient input なし。 | focused controls、add/remove/edit/revert cache twins、fingerprint と whole/per-unit link。 |

## 正典と author consistency pass

英語台帳、本書、`draft.md`、`docs/language-spec.md`、`docs/design-notes.md`、`docs/history.md`、
`docs/open-questions.md`、`docs/impl/07-roadmap.md`、`docs/impl/19-hir-validation-ledger.md`、
`docs/impl/20-runtime-abi-ledger.md`、`HANDOFF.md` は実装前に一致させる。

全引数・結果の型/評価/既定/ownership/lifetime/allocation/cleanup/error/effect、全 join/limit product、
UTF-8/NUL/byte equality、multi-invalid precedence、非 ambient 性、ABI scalar/pointer/status/allocator/
activation、producer-owned inspection、構文確認済み例、全 ledger invariant の acceptance owner を照合する。
benchmark は実装級の証拠であり correctness gate ではない。
