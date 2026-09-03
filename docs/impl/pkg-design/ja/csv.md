# pkg — csv

> [English](../csv.md) · **日本語**
>
> **注意:** 英語版 (`../csv.md`) が正本。本書は同期ミラーである。
>
> **ステータス:** 設計済み（2026-09-03）。未実装。

## 公開契約台帳

V1 はメモリ上の UTF-8 CSV 1 文書を arena-backed `soa<R>` へ直接 decode する。後続本文は
この表を明確化できるが、ここにない公開表面、producer、error、allocation、input を加えてはならない。

| 公開表面 | exact input・default・意味 | error・effect・ownership・lifetime・allocation | compiler/runtime/package owner・artifact/cache identity | prerequisite・exact acceptance evidence |
|---|---|---|---|---|
| `pub Header { Present, Absent }` | source/discriminator 順は `Present=0`, `Absent=1`。`Present` は最初の logical record を header として消費し、`Absent` は physical column を `R` の宣言順へ写す。推論/default なし。 | Copy/Pure。borrow、allocation、Drop、ambient MIME、保持 state なし。 | nominal sum と tag/order は `pkg.csv` owner で、whole/per-unit interface/dependency/cache identity へ入る。 | 出荷済み closed sum。tag/order、construction/match、malformed checked-HIR、interface/cache owner。 |
| `pub LineEnding { CrLf, Lf }` | 順は `CrLf=0`, `Lf=1`。選んだ列だけが quote 外の record separator。最終 record は separator なしでもよい。`CrLf` は lone CR/LF、`Lf` は quote 外 CR を拒否。auto/mixed/lone-CR/platform default なし。 | Copy/Pure。quoted field 内の CR/LF/CRLF は byte-for-byte data。allocation/保持なし。 | nominal sum を `pkg.csv` が所有し、runtime へ checked i32 tag を渡す。 | exact tag/order、terminated/unterminated × mixed/lone、whole/per-unit、malformed-HIR owner。 |
| `pub DecodeOptions { header: Header, line_ending: LineEnding, max_rows: i64 }` | field/source 順は表示どおり。default なし。`max_rows` は nonnegative data-row inclusive bound。negative は `Invalid`、zero は header/empty を許すが data row は不可、exact は成功、次の完全な valid row は `LimitExceeded`。 | Copy/Pure。descriptor/input inspection と arena allocation より前に field 順で検証。保持・割当なし。 | nominal record と reachable graph は `pkg.csv` owner。全 field が interface/dependency/cache identity へ入る。 | record/i64 bounds。field/order/default、negative/zero/exact/next、direct/imported/generic、whole/per-unit/cache owner。 |
| `pub Error { Invalid, LimitExceeded }` | 順は `Invalid=0`, `LimitExceeded=1`。前者は options、grammar、line ending、header identity/uniqueness/completeness、width、selected conversion。後者は 1025 番目の physical header column、最初の bound 超過 row、または exact output/normalization layout の i64/target allocation-domain 表現不能。 | Copy/Pure。OOM と impossible compiler/runtime status は hard abort。message/path/position/partial output/fallback/retry/log/cleanup error なし。 | ordinary package sum。private status は `0=success,1=Invalid,2=LimitExceeded`、他は明示的 `std.process` dependency の `process.abort()`。 | Result/tag sum/abort。producer/status/tag/order、multi-invalid precedence、malformed ABI、whole/per-unit owner。 |
| `pub fn decode<R: SoaPlain>(input: str, out: region, options: DecodeOptions) -> Result<soa<R>, Error>` | 引数は左から 1 回ずつ。`R` は expected result だけから推論し、1..=1024 fields の nonempty natural-layout record。field は integer/float/`bool`/`char`/`str`。absolute byte 0 の UTF-8 BOM を 1 個だけ除去。Present header は 1..=1024 unique nonempty decoded names、全 declared field を byte-exact に 1 回 map、extra は grammar 検証のみ。Absent は declaration-order exact width。全 data record は physical width が一致。 | Pure。pass 1 は heap/arena allocation なしで完全な successful input、selected conversion、row bound、normalized bytes、layout を検証。pass 2 は exact aligned arena block 1 個だけを確保し、AoS/transpose なしで直接 column fill。unquoted と doubled quote なし quoted `str` は input span、`""` を含む quoted field だけ `"` へ collapse して同じ block へ copy。primitive-only result は `out`、`str` を含む result は `input+out` に依存。`soa<R>` は Copy/Drop なし。error は `out` を不変に保つ。 | canonical `pkg.csv` は public generic wrapper 1 個と private exact bridge 1 個を所有。sema は exact package/signature/schema の後だけ `CsvDecode` を形成。HIR/MIR は `R`, input, region, options, effect, region facts を保持。LLVM は immutable `CsvField` table と reserved A123 `align_rt_csv_decode_soa_v1` call を生成。package/schema/bridge/op/descriptor/runtime key/ABI/body は cache identity に入り、file/locale/MIME/CPU/env/allocator setting は入らない。 | shipped generics/`SoaPlain`/named region/SoA/package sealing/checked-HIR/arena/two-pass decode。formation、schema/header/grammar/value、BOM/bounds、zero-copy/copy、region/generation、no-allocation error、layout、ABI/status、whole/per-unit/generic/cache/lowering/work-count owner。 |

## 決定と範囲

```text
メモリ上 UTF-8 CSV + 明示 dialect + 明示 destination region
  -> 検証済み selected columns
  -> arena-backed soa<R> 1 個
```

dynamic row/map、reflection、parser object、iterator、row-major result は加えない。expected
`soa<R>` annotation が schema。Present は wide input から narrow record への name projection、Absent は
exact declaration order/width だけを使う。

source declaration は次の exact 形である。

```align
module pkg.csv

import std.process

pub Header {
  Present
  Absent
}

pub LineEnding {
  CrLf
  Lf
}

pub DecodeOptions {
  header: Header,
  line_ending: LineEnding,
  max_rows: i64,
}

pub Error {
  Invalid
  LimitExceeded
}

fn decode_bridge<R: SoaPlain>(
  input: str,
  out: region,
  options: DecodeOptions,
) -> Result<soa<R>, Error> = process.abort()

pub fn decode<R: SoaPlain>(
  input: str,
  out: region,
  options: DecodeOptions,
) -> Result<soa<R>, Error> = decode_bridge(input, out, options)
```

`decode_bridge` は canonical `pkg.csv` 内のこの private declaration/body だけが compiler-recognized。
interface には export しない。同名 application function/extern/noncanonical package は ordinary declaration。
canonical validator は変更 bridge を placeholder 評価前に拒否する。

## 公開利用

```align
import pkg.csv

Trade {
  active: bool,
  amount: i64,
  symbol: str,
}

fn active_total(input: str) -> Result<i64, csv.Error> {
  options := csv.DecodeOptions {
    header: csv.Header.Present,
    line_ending: csv.LineEnding.Lf,
    max_rows: 1000000,
  }
  arena out {
    rows: soa<Trade> := csv.decode(input, out, options)?
    return Ok(rows.where(.active).amount.sum())
  }
}
```

owned `string` は通常規則で `str` へ auto-borrow。written type arguments、named/default arguments、
implicit arena、ambient MIME/platform newline はない。

## CSV grammar

常に valid UTF-8 の Align `str` 上で RFC 4180 の record/quote model を使う。元の printable ASCII
`TEXTDATA` を UTF-8 text へ広げ、LF compatibility は public enum で明示する。`EOL` は `CrLf` なら
exact CRLF、`Lf` なら exact LF。leading BOM を高々 1 個除いた grammar は次のとおり。

```text
file          = empty / record *(EOL record) [EOL]
record        = field *(COMMA field)
field         = unquoted / quoted
unquoted      = *UTF8-EXCEPT-COMMA-CR-LF-DQUOTE
quoted        = DQUOTE *(UTF8-EXCEPT-DQUOTE / COMMA / CR / LF / DQUOTE DQUOTE) DQUOTE
COMMA         = %x2C
DQUOTE        = %x22
BOM           = %xEF %xBB %xBF
```

- space/tab は data。trim なし。
- quote は field 先頭だけで quoted field を開始し、unquoted 内では禁止。quoted 内は `""` だけが `"`。
- closing quote 後は comma、selected `EOL`、EOF だけ。
- quoted 内の comma/CR/LF/CRLF/NUL/non-ASCII UTF-8 は非正規化 data。
- final `EOL` は空 record を増やさない。empty document は 0 records、blank record は empty field 1 個。
- trailing comma は empty final field。
- BOM は absolute byte 0 の 1 個だけ。以後の BOM は U+FEFF data。BOM-only は Absent で empty、Present で header 不在の Invalid。

## Header と projection 規則

Absent は physical width を `R` field 数に固定し、ordinal `i` を declaration field `i` へ decode。
Present は最初の logical record を、unterminated final record でも header として消費する。physical
header field は 1..=1024、decoded name は全て nonempty/byte-unique（undeclared も含む）。matching は
CSV quote decode 後の byte-exact/case-sensitive で Unicode normalization なし。全 declared name が exact
1 回必要。extra unique column は position/width と grammar を検証するが convert/copy しない。

header map は decoded hash/input span の fixed-capacity stack scratch。heap/arena allocation なし。
hash hit は decoded byte equality で必ず確認し、result column は input order に関係なく `R` declaration order。

## typed cell conversion

quote decode 後の logical cell 全体が grammar に一致する。prefix、trim、locale、thousands、hex、suffix、
default はない。

| target | exact spelling/result |
|---|---|
| signed `i8/i16/i32/i64` | `-?[0-9]+`。leading zero/`-0` を許可し、checked decimal accumulation が width 内。 |
| unsigned `u8/u16/u32/u64` | `[0-9]+`。leading zero を許可し、`u64::MAX` を含む target 全範囲。 |
| `f32/f64` | `-?[0-9]+(.[0-9]+)?([eE][+-]?[0-9]+)?`。dot/exponent は digit 必須。leading plus と `NaN`/`inf` を拒否。target IEEE width へ nearest-even、overflow は signed infinity、underflow は signed zero の場合あり。 |
| `bool` | exact lowercase ASCII `true` / `false`。 |
| `char` | decoded Unicode scalar ちょうど 1 個。 |
| `str` | empty/NUL/whitespace/comma/embedded line break を含む decoded field 全体。 |

float grammar の dot は literal ASCII `.`。empty は `str` だけ。selected invalid grammar/range は
`Invalid`、unselected extra は CSV grammar だけ。quoted scalar は direct parse し、`char` の `""""` は quote
1 byte。scalar conversion に normalized scratch は不要。

## Validation と error precedence

source は canonical package/signature と concrete `R` を先に検証し、`R` は unique names の
1..=1024-field natural-layout `SoaPlain`。input/out/options は written order で 1 回。terminating child は
以後を止める。private runtime 順序は exact に次である。

1. output header/live arena/descriptor count-table/name range/tag/reserved/uniqueness を slice formation、input load、allocation 前に検証し、output を `{null,0}`。失敗は abort。
2. `Header`, `LineEnding`, `max_rows >= 0` の順。negative row は input 前に `Invalid`。
3. input pointer/length を検証し、BOM を 1 個除き、empty/first-record path を選ぶ。
4. Present header を physical 順に grammar/EOL、1024 cap、nonempty、duplicate、required coverage で検証。1025th だけ `LimitExceeded`、それ以前の invalid が先。
5. data を source record/cell 順に grammar、selected conversion、width の順で検証。完全 otherwise-valid row が count を増やす直前に bound と比較し、次 row は `LimitExceeded`。後続は読まない。
6. EOF 成功後、normalized bytes、全 SoA offset/size/alignment、tail、total i64/target allocation size を検証。失敗は allocation 前 `LimitExceeded`。
7. `out` に exact aligned block 1 個を確保し、再走査して direct fill。infallible pass mismatch は abort で partial result/error にしない。

observable precedence は private abort、invalid options、header、earliest data grammar/conversion/width、
row limit、representability、OOM/fill abort。failure は heap allocation せず、recoverable failure は arena を進めない。

## Ownership・region・storage-generation closure

既存 target-specific SoA layout authority を共有し、sibling formula を持たない。`N` rows の各 field は
declaration-order contiguous column。normalized string tail は aligned column area 後。`N==0` は
`{null,0}` allocation なし。`N>0` は primitive、`str` header、padding、normalized bytes を 1 block に持つ。

unquoted `str` は input field 全体、doubled quote なし quoted は quotes 内部、doubled quote ありは arena
tail の exact decoded bytes を指す。全 output は `out` に依存。`R` が `str` を含むなら data に関係なく
input storage root/generation も型レベルで保持し、binding/field/Option/Result/control/call/projection/index/
pipeline を通る。primitive-only は call 後 input を保持しない。

`input` が既に `out` rooted view でもよく、alias rejection ではない。arena allocation は既存 live bytes を
relocate/overwrite せず append するため両方とも arena owner exit まで valid。compiler-produced layout の
new block は既存 live arena allocation の外にある。

`out` は non-owning capability で owner を transfer/store しない。lexical `arena out {}` が sole cleanup
owner。`soa<R>` は Copy で element/whole Drop なし。

## Compiler/package boundary と checked HIR

新 HIR expression と MIR rvalue は各 1 個、名前は `CsvDecode`。HIR record は exact に次。

```text
struct_id: valid concrete SoaPlain record id with 1..=1024 fields
input:      exact str
arena:      exact region capability
options:    exact pkg.csv.DecodeOptions
result:     exact Result<soa<struct_id>, pkg.csv.Error>
effect:     Pure
```

validator は scalar id/non-child、3 children の source order、relational result の順。`options` は record
child として 1 回評価し、MIR が checked fields を 1 回 project。既存 `soa_plain_ok` と package/1024 rule を
共有する。mismatch は MIR/native/allocation/artifact/cache 前に拒否。全 traversal/replay/depth/effect/
ownership/region/escape/type/storage/interface/monomorph/capability/variant-tripwire が明示的に扱う。

`R` と全 `pkg.csv` declaration は nominal language/interface identity。同一 field spelling でも別 nominal
scope の record は別型。fingerprint は nominal identity と完全な ordered reachable definition graph を
encode し、`SoaPlain` の reachable leaf は各 primitive。runtime descriptor は structural execution
metadata に限られ、compiler/cache の nominal 区別を消さない。

MIR status は 0=`Ok`、1=`Err(Invalid)`、2=`Err(LimitExceeded)`、他は `process.abort()`。error edge は
output を公開せず Drop なし。semantics は MIR にあり LLVM は pure lowering。

## Runtime ABI reservation

design は current count を変えず keyed A123 を予約する。

| runtime key | exact symbol | exact LLVM declaration | exact Rust ABI |
|---|---|---|---|
| `CsvDecodeSoaV1` | `align_rt_csv_decode_soa_v1` | `i32 @SYM(ptr, i64, ptr, i64, ptr, i32, i32, i64, ptr)` | `unsafe extern "C" fn(*const u8, i64, *const CsvField, i64, *mut Arena, i32, i32, i64, *mut AlignStr) -> i32` |

parameter 順は input pointer/bytes、descriptor pointer/count、arena、header tag、line-ending tag、row
bound、writable output。C calling convention、`nounwind`、他 curated attributes なし。

call 全体で input bytes、descriptor records、descriptor-name bytes は immutable。live arena control object
と output header は別々に exclusive で、互いおよび全 immutable range と disjoint。input は同じ arena の
prior live allocation 内でもよく、新 allocation はそれと overlap しない。exact-compatible unsafe source
extern はこの provenance/overlap precondition を満たし、compiler-produced call は checked region と
compiler-owned static descriptor から lowering 前に満たす。

`CsvField` は target-native `#[repr(C)]` / non-packed LLVM record:

```text
{ name_ptr: ptr, name_len: i64, tag: i32, reserved: i32 }
```

`reserved=0`。`tag=(signed<<16)|(kind<<8)|width`: integer kind 0 width 1/2/4/8、sign bit 16、bool
kind 1 width 1、float kind 2 width 4/8、str kind 3 width 16、char kind 4 width 4。他 bits は zero。
static UTF-8 names は declaration order。table 全体を input/effect 前に検証する。

activation は key/symbol/declaration golden/definition/export/checked op/owners を atomic に追加し、
keyed/base/either-four-row-probe/maximum を 330/348/352/356 から 331/349/353/357 にして A124 を次にする。
source extern は row/checked op を activate できず、activation 後の exact compatible reuse だけ通常 registry
rule に従う。partial producer は不可。

## 決定的 example と golden vector

| vector | options/schema | exact result/error |
|---|---|---|
| empty | Absent、either EOL、`{ value: str }` | zero rows `{null,0}`、allocation なし |
| BOM-only | 同上 | zero rows。2 個目 BOM は `"\u{feff}"` 1 row |
| selected-wide | Present/Lf、`ignored,amount,active,symbol\n` | declared columns を reorder、unknown は skip、row order 維持 |
| quoted | Present/CrLf、quoted comma と doubled quote/CRLF | clean は interior borrow、escaped は exact decoded bytes in `out` |
| scalar edges | 全 integer min/max、`u64::MAX`、float edge、bool、Unicode char | exact bits/value、out-of-range/malformed twin は `Invalid` |
| row bound | zero/exact/next | zero/exact success、次の complete valid row は `LimitExceeded` |
| separators | terminated/unterminated と mixed/lone | selected spelling だけ成功、quoted bytes 不変 |
| headers | reorder/extra/missing/duplicate/empty/case/1024/1025 | mapping、invalid identity、1025th は `LimitExceeded` |

base offset 0..7、endian twin、comma/quote/CR/LF/BOM/numeric/EOF mutation を owner が覆う。別 oracle が
production decoder を使わず exact SoA layout/projected values を検査する。test-only reference encoder は
semantic field vector を valid quoted/unquoted CSV bytes へ独立に写し decode と比較するが、public
`pkg.csv` encoder でも production parser code の source でもない。

## 複雑性と性能境界

成功は sequential input pass 2 回、nonempty output の exact arena allocation 1 回、direct column write。
AoS/transpose/heap/per-row/per-field-owned-string/unselected-or-clean-text copy なし。header は bounded stack
hash+equality。SIMD を使う場合も x86/ARM64/scalar は one oracle と一致する。throughput/latency/SIMD width
は公開 promise でない。local non-gating `bench/csv_decode` は bytes/rows/physical-selected columns/
normalized bytes/passes/arena allocations/conversions の producer counters を記録する。

## V1 non-goals と後続境界

encoder、reader/file/mmap/fragment、streaming、pipeline-source fusion、`array<R>`、owned/dynamic row/value/map、
reflection、inferred header/EOL、lone-CR/mixed、delimiter/quote/comment config、trim/blank skip、null/default/
missing/nullable、date/decimal/locale/normalization/case fold/alias/recovery/diagnostic payload、parallel decode、
external CSV library は V1 にない。`csv.scan` は chunk/view lifetime と Copy row rule を別途決める。encoding
も canonical EOL/quote/float/bound を別 ledger で決める。nullable は nullable SoA を待つ。

## 実装 closure matrix

この capability は hand-written 1,000 行を超える見込みだが、package/HIR/MIR/LLVM/runtime/owner の atomic
boundary が dormant split より proof duplication と integration risk を小さくする。

| axis | required closure | exact owner evidence |
|---|---|---|
| Public formation/identity | canonical package、4 public types/1 wrapper/1 bridge、inferred `R`、1..=1024、same-name interception なし、全 call shape | source/interface hash、schema/bridge mutation、call-target、whole/per-unit/generic |
| Evaluation/HIR/MIR | children once/order/termination、exact type/id/effect/region、status/no-error-output、全 traversal | variant tripwire、one-field mutation、control、MIR status/abort |
| CSV lexical | BOM、quote/double、comma、space/NUL/UTF-8、EOL、EOF、blank/trailing | independent oracle、bounded mutation/fuzz、全 state transition |
| Header/projection | modes/order/extra/identity/1-1025/collision/width | generated duplicate/collision、skip counter、mapping oracle |
| Typed conversion | 全 integer/float/bool/char/empty/selected-extra | parameterized oracle、optimized/endian/whole-per-unit parity |
| Bounds/precedence | enum/options/rows/header/layout overflow、earliest invalid | multi-invalid matrix、representability twins、inspection/allocation counters |
| Allocation/atomicity | pass1 zero alloc、pass2 one block、no AoS、zero row、abort/error arena rule | arena/heap counters、failpoints、topology、cursor pre/post |
| String regions | clean input/escaped tail、type-level input+out、same-arena input、全 carriers/generations | pointer ranges/bytes、distinct/same-arena twins、escape/mutation negative、primitive release positive |
| SoA/pipeline | shared layout、alignment、projection/index/pipelines | independent layout oracle、current SoA bundle、residue/schema generation |
| Native ABI | A123 identity/attrs/export、descriptor/full prevalidation/output zero/no unwind | registry/golden/export/compat mutation、malformed ABI、rt-LTO/provenance |
| Cache/distribution | package/schema/op/descriptor/runtime/body の exact invalidation、ship 時だけ inventory | whole/per-unit edit-revert/prebuilt/no-op/unrelated owner |
| Performance | two scans/direct selected fill/no clean copy/no heap/AoS/transpose、SIMD parity | producer counters、non-gating benchmark、scalar/x86/ARM64 equality |

## source of truth と author consistency pass

英語 ledger、本文書、`draft.md`、`docs/language-spec.md`、`docs/design-notes.md`、`docs/history.md`、
`docs/open-questions.md`、roadmap、HIR/ABI ledger、`HANDOFF.md` は implementation 前に一致する。guide と
vendorable/prebuilt inventory は source が ship するまで shipped package だけを記載する。

author pass は全 public field の type/order/default/effect/ownership/lifetime/allocation/owner/identity、全
Header×EOL×document×quote×selection×field-type×limit product、UTF-8/NUL/header equality、multi-invalid
precedence、two-pass/direct-column counters、A123 の全 width/tag/order/pointer/status/attribute/count、全
compiler stage/cache/distribution の atomicity、全 example syntax、later capability 非依存を閉じる。
