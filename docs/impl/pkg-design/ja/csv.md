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
| `pub fn decode<R: SoaPlain>(input: str, out: region, options: DecodeOptions) -> Result<soa<R>, Error>` | 引数は左から 1 回ずつ。`R` は expected result だけから推論し、field が integer/float/`bool`/`char`/`str` の任意の nonempty record という既存 `SoaPlain` 全域。CSV 固有 schema-count/layout 制約はなく、explicit AoS layout/alignment は SoA column を制約しない。absolute byte 0 の UTF-8 BOM を 1 個だけ除去。Present header は 1..=1024 unique nonempty decoded names、全 declared field を byte-exact に 1 回 map、extra は grammar 検証のみ。1024 超の schema は Absent だけ成功可能。Present が 1024 以下なら missing coverage で `Invalid`、1025th physical header があれば先に `LimitExceeded`。Absent は declaration-order exact width。全 data record は physical width が一致。 | Pure。raw ABI は最初に allocation なしで UTF-8 を検証し invalid bytes は private `-1`。decode pass 1 は heap/arena allocation なしで完全な successful input、selected conversion、row bound、normalized bytes、layout を検証。`N>0` だけ decode pass 2 が exact aligned arena block 1 個を確保し、AoS/transpose なしで直接 column fill。`N==0` は allocation なしの `{null,0}`。unquoted と doubled quote なし quoted `str` は input span、`""` を含む quoted field だけ `"` へ collapse して同じ block へ copy。primitive-only result は `out`、`str` を含む result は `input+out` に依存。`soa<R>` は Copy/Drop なし。error は `out` を不変に保つ。 | canonical root `pkg.csv` は compiler-private spelling `pkg.csv.internal.descriptor.decode` を呼ぶ public generic wrapper 1 個を所有。internal module は source function を宣言せず application import は package `internal` rule が拒否。abstract template check は `row=Ty::Param(p)` と existing `SoaParam(p)` result の template-only `CsvDecode` を形成して破棄する。concrete monomorph の再チェックだけが `row=Ty::Struct(id)` の emitted operation を形成する。interface/frontend identity は root/internal source、generic body、nominal `R` graph、op/descriptor semantics。object/link key は既存 explicit target/CPU/features/profile/pipeline/runtime/link inputs も保持。ambient runtime CPU detection/file/locale/MIME/env/allocator setting は CSV semantics を変えない。 | shipped generics/`SoaPlain`/named region/SoA/package sealing/compiler-private internal descriptor/checked-HIR/arena/UTF-8 prevalidation/two decode-pass。abstract-template/concrete-monomorph formation、unbounded schema と bounded Present header、grammar/value、BOM/bounds、zero-copy/copy、region/generation、no-allocation error、layout、ABI/status、whole/per-unit/generic/cache/lowering/work-count owner。 |

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
import pkg.csv.internal.descriptor

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

pub fn decode<R: SoaPlain>(
  input: str,
  out: region,
  options: DecodeOptions,
) -> Result<soa<R>, Error> = pkg.csv.internal.descriptor.decode(input, out, options)
```

vendorable subtree は exact `pkg.csv.internal.descriptor` module も含み、import/source item はない。

```align
module pkg.csv.internal.descriptor
```

その `decode` spelling は private source declaration でなく、出荷済み `pkg.db.internal.descriptor` family と同じ
compiler-private descriptor operation。public generic body は interface から落ちる private item を参照せず、
importing unit は保持 body を vendored internal module identity に対して monomorphize できる。application import
は package `internal` rule が拒否。同名 function/extern、internal item 追加、wrapper 変更、noncanonical package
は `CsvDecode` を選べず package admission が body evaluation 前に拒否する。

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

physical header cap は schema cap ではない。1024 超の `SoaPlain` schema は Absent で動く。Present の
grammar-valid 1024 以下 header は必ず declared field を欠いて coverage phase の `Invalid`、1025th physical
field に達した input は coverage より先に `LimitExceeded`。

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

source は canonical package/signature と concrete `R` を先に検証し、`R` は nonempty/unique names の
existing `SoaPlain` 全域で CSV 固有 schema-count cap はない。explicit AoS layout/alignment は SoA column に影響しない。
input/out/options は written order で 1 回。terminating child は
以後を止める。private runtime 順序は exact に次である。

1. output header/live arena を slice formation、input load、allocation 前に検証し、output を `{null,0}`。失敗は abort。
2. `Header`, `LineEnding`, `max_rows >= 0` の順。negative row は input 前に `Invalid`。
3. descriptor count の positive/exact representability、table byte-size/alignment/nonnull guard の順。その後 declaration order で各 record の positive name length、nonnull/range arithmetic、exact source-identifier bytes、tag、zero reserved を順に検証。全 record valid 後、right ordinal 昇順・earlier left ordinal 昇順で最初の duplicate を拒否。失敗は private abort。output/arena が valid なら negative row は malformed descriptor より先。
4. input pointer/length を検証する。negative length と positive length の null は malformed private ABI。zero length は pointer を問わず dereference/slice formation なし。positive は nonnull/range-arithmetic guard を先に検証し、compiler/unsafe caller が complete call 中の exact readable range を保証する場合だけ byte slice を形成する。complete UTF-8 検証を行い、invalid は BOM/CSV/allocation 前に private `-1`。valid なら BOM を 1 個除き first-record path を選ぶ。
5. Present header を physical 順に grammar/EOL、1024 cap、nonempty、duplicate、required coverage で検証。1025th だけ `LimitExceeded`、それ以前の invalid が先。
6. data を source record/cell 順に grammar、selected conversion、width の順で検証。完全 otherwise-valid row が count を増やす直前に bound と比較し、次 row は `LimitExceeded`。後続は読まない。
7. EOF 成功後、normalized bytes、全 SoA offset/size/alignment、tail、total i64/target allocation size を検証。失敗は allocation 前 `LimitExceeded`。
8. `N==0` は allocation なしで `{null,0}`。`N>0` だけ `out` に exact aligned block 1 個を確保し再走査/direct fill。infallible pass mismatch は abort で partial result/error にしない。

observable precedence は malformed output/arena ABI abort、invalid options、malformed descriptor/input
ABI abort、header、earliest data grammar/conversion/width、row limit、representability、OOM/fill abort。
failure は heap allocation せず、recoverable failure は arena を進めない。

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
row:        template-only Ty::Param(p), or emitted Ty::Struct(id)
input:      exact str
arena:      exact region capability
options:    exact pkg.csv.DecodeOptions
result:     template-only Result<soa<Param(p)>, pkg.csv.Error>,
            or emitted Result<soa<id>, pkg.csv.Error>
effect:     Pure
```

exact wrapper の abstract body check は `p: SoaPlain` と matching `SoaParam(p)` result のときだけ
`row=Ty::Param(p)` を許す。concrete schema/descriptor/layout/provenance/MIR/native work は行わず、existing
generic-template path がこの HIR を破棄して source AST を interface monomorphization 用に保持する。
consumer instantiation は concrete substitution で body を再チェックし、そこでだけ `row=Ty::Struct(id)`
と `soa<id>` を形成して existing `soa_plain_ok` と package identity を schema-count 制約なしで適用する。outer generic forwarding も
concrete instantiation まで symbolic check を繰り返す。emitted validator は `Ty::Param`/`SoaParam`/non-Struct
row を children 前に拒否する。`options` は record child として 1 回評価し MIR が fields を 1 回 project。
全 traversal/replay/depth/effect/ownership/region/escape/type/storage/interface/monomorph/capability/
variant-tripwire は concrete expression を明示的に扱い、abstract form が到達しない owner も持つ。

`R` と全 `pkg.csv` declaration は nominal language/interface identity。同一 field spelling でも別 nominal
scope の record は別型。fingerprint は nominal identity と完全な ordered reachable definition graph を
encode し、`SoaPlain` の reachable leaf は各 primitive。runtime descriptor は structural execution
metadata に限られ、compiler/cache の nominal 区別を消さない。

frontend/interface key は canonical root/empty-internal source、generic body、checked op、nominal `R` graph。
object action key は既存 target triple/object format、resolved CPU/features、profile、pipeline、optimization/
relocation/code model、LLVM、runtime-LTO/digest、PGO、exports、dependency hashes も含む。final link は ordinary
ordered object/runtime/library input を含む。CSV は ambient runtime feature/data-dependent cache input を加えず、
既存 explicit build input を除かない。

MIR status は 0=`Ok`、1=`Err(Invalid)`、2=`Err(LimitExceeded)`。private `-1` と他 status は
`process.abort()`。runtime は malformed private ABI に `-1` だけを返し他 status は返さない。error edge は
output を公開せず Drop なし。semantics は MIR にあり LLVM は pure lowering。

## Runtime ABI reservation

design は current count を変えず keyed A123 を予約する。

| runtime key | exact symbol | exact LLVM declaration | exact Rust ABI |
|---|---|---|---|
| `CsvDecodeSoaV1` | `align_rt_csv_decode_soa_v1` | `i32 @SYM(ptr, i64, ptr, i64, ptr, i32, i32, i64, ptr)` | `unsafe extern "C" fn(*const u8, i64, *const CsvField, i64, *mut Arena, i32, i32, i64, *mut AlignStr) -> i32` |

parameter 順は input pointer/bytes、descriptor pointer/count、arena、header tag、line-ending tag、row
bound、writable output。C calling convention、`nounwind`、他 curated attributes なし。

call 全体で arena は nonnull、`align_of::<Arena>()` aligned、live/exclusive。output は nonnull、
`align_of::<AlignStr>()` aligned、writable/exclusive。両者は互いと immutable ranges から disjoint。
zero input length は null/non-null pointer の両方を許可し dereference なし。positive length は nonnull で
exact readable range。descriptor count は positive、exact `usize` 変換可能、CSV 固有 upper bound なし。その pointer は nonnull、
`align_of::<CsvField>()` aligned で immutable records 全体を指す。positive name length は nonnull exact
readable bytes。全 length/count/product/address addition は declared integer と target pointer-offset domain
内で、reference/slice/typed load より前に検証する。

input bytes、descriptor records、descriptor-name bytes は complete call 中 immutable。input は同じ arena の
prior live allocation 内でもよく、新 allocation はそれと overlap しない。runtime は mechanically detectable
negative/null/misaligned/overflowing representation を typed access 前に `-1` にする。guard 通過後の
dereferenceability/lifetime/provenance/immutability/overlap は exact-compatible unsafe caller が保証し、compiler
call は checked region と aligned static descriptor から満たす。guard は arbitrary nonnull address の backing
range を証明せず、otherwise-invalid unsafe call を defined にはしない。

`CsvField` は target-native `#[repr(C)]` / non-packed LLVM record:

```text
{ name_ptr: ptr, name_len: i64, tag: i32, reserved: i32 }
```

global は exact `repr(C)` size/alignment/field offsets 以上を使う。name は first byte ASCII `_`/letter、
remaining byte ASCII `_`/letter/digit の source identifier で、exact reserved tokens `fn`, `return`, `mut`,
`pub`, `module`, `import`, `if`, `else`, `true`, `false`, `arena`, `task_group`, `match`, `loop`, `break`,
`template`, `unsafe`, `extern`, `as` を除く。NUL/non-ASCII/invalid UTF-8/punctuation/keyword/他 spelling は
descriptor phase の private `-1`。name は pairwise byte-unique。`reserved=0`。
`tag=(signed<<16)|(kind<<8)|width`: integer kind 0 width 1/2/4/8、sign bit 16、bool
kind 1 width 1、float kind 2 width 4/8、str kind 3 width 16、char kind 4 width 4。他 bits は zero。
table 全体を declaration order で input/arena effect 前に検証する。

native order は output、arena、output zero、header tag、line-ending tag、row bound、positive representable
descriptor count/table/fields/names、input representation、complete UTF-8、CSV/header/data/layout、最後に nonempty allocation/fill。
malformed private ABI は `-1`、negative row は descriptor inspection 前に 1、public parse/conversion は 1、
public bound/layout は 2。この順序は前節の public precedence と同じ。

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

positive-length 成功は complete UTF-8 prevalidation 1 回と decode pass 2 回の sequential input walk 3 回、
nonempty output の exact arena allocation 1 回、direct column write。
AoS/transpose/heap/per-row/per-field-owned-string/unselected-or-clean-text copy なし。header は bounded stack
hash+equality。SIMD を使う場合も x86/ARM64/scalar は one oracle と一致する。throughput/latency/SIMD width
は公開 promise でない。local non-gating `bench/csv_decode` は bytes/rows/physical-selected columns/
normalized bytes/UTF-8 and decode passes/arena allocations/conversions の producer counters を記録する。

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
| Public formation/identity | canonical root + empty internal descriptor、4 public types/1 generic wrapper/1 compiler-private spelling/no private source item、`p: SoaPlain` の abstract `Ty::Param(p)`+`SoaParam(p)` から concrete `Ty::Struct(id)` recheck、CSV schema-count cap なしで explicit layout/alignment を含む existing `SoaPlain` 全域、application import/interception なし、全 call shape | root/internal/interface hash、generic body private-item 非参照、abstract wrapper/forwarder、concrete substitution/recheck、Absent の 1/1024/1025/larger schema、Present missing coverage/1025th physical precedence、`layout(C)`/`align(N)` positive、wrong bound/abstract emission negative、schema/module/body/internal-item mutation、whole/per-unit/generic |
| Evaluation/HIR/MIR | children once/order/termination、exact type/id/effect/region、status/no-error-output、全 traversal | variant tripwire、one-field mutation、control、MIR status/abort |
| CSV lexical | BOM、quote/double、comma、space/NUL/UTF-8、EOL、EOF、blank/trailing | independent oracle、bounded mutation/fuzz、全 state transition |
| Header/projection | modes/order/extra/identity/1-1025/collision/width | generated duplicate/collision、skip counter、mapping oracle |
| Typed conversion | 全 integer/float/bool/char/empty/selected-extra | parameterized oracle、optimized/endian/whole-per-unit parity |
| Bounds/precedence | enum/options/rows/header/layout overflow、output/arena→options→descriptor/input→earliest data invalid | negative bound + malformed descriptor を含む multi-invalid matrix、representability twins、inspection/allocation counters |
| Allocation/atomicity | pass1 zero alloc、pass2 one block、no AoS、zero row、abort/error arena rule | arena/heap counters、failpoints、topology、cursor pre/post |
| String regions | clean input/escaped tail、type-level input+out、same-arena input、全 carriers/generations | pointer ranges/bytes、distinct/same-arena twins、escape/mutation negative、primitive release positive |
| SoA/pipeline | shared layout、alignment、projection/index/pipelines | independent layout oracle、current SoA bundle、residue/schema generation |
| Native ABI | A123 identity/attrs/export、input null-zero/positive-range、aligned live arena/output、`align_of::<CsvField>()` aligned positive representable uncapped table、options 後の source-identifier name validation、BOM/CSV 前 complete UTF-8、typed access 前 guard、output zero/no unwind | registry/golden/export/compat mutation、invalid address を dereference しない null-zero/null-positive/unaligned/zero-negative-overflow count/invalid start-continuation-nonASCII-NUL-duplicate name/invalid UTF-8 と negative-option+malformed descriptor-input matrix、compiler provenance/range/unsafe owner、rt-LTO |
| Cache/distribution | root/internal/body/schema/op/descriptor/runtime の exact invalidation、既存 explicit target/CPU/features/profile/pipeline/runtime/link input 維持、ship 時だけ inventory | whole/per-unit/target edit-revert/prebuilt/no-op/ambient-runtime/unrelated owner |
| Performance | UTF-8 prevalidation 1 + decode pass 2/direct selected fill/no clean copy/no heap/AoS/transpose、SIMD parity | producer counters、non-gating benchmark、scalar/x86/ARM64 equality |

## source of truth と author consistency pass

英語 ledger、本文書、`draft.md`、`docs/language-spec.md`、`docs/design-notes.md`、`docs/history.md`、
`docs/open-questions.md`、roadmap、HIR/ABI ledger、`HANDOFF.md` は implementation 前に一致する。guide と
vendorable/prebuilt inventory は source が ship するまで shipped package だけを記載する。

author pass は全 public field の type/order/default/effect/ownership/lifetime/allocation/owner/identity、全
Header×EOL×document×quote×selection×field-type×limit product、public UTF-8/NUL、raw invalid UTF-8、
descriptor source-identifier/NUL、header equality、multi-invalid
precedence、abstract/forwarding/concrete recheck/emitted symbolic rejection、UTF-8/two-decode-pass/direct-column counters、
A123 の全 width/tag/order/pointer/null/length/alignment/range/source-identifier/NUL/status/attribute/count、全
compiler stage/cache/distribution の atomicity、全 example syntax、later capability 非依存を閉じる。

## 独立 design review

candidate `5f9f978f` の fresh full-diff review は P1 1 件、P2 3 件。ledger を先に直し、同期 repair で
complete finding set を閉じる。

| finding | ledger-first repair |
|---|---|
| P1: public generic template は interface から落ちる private helper を参照できない | private bridge を exact empty internal module の compiler-private `pkg.csv.internal.descriptor.decode` spelling へ置換。internal import sealing、root-only formation、retained generic body/interface、whole/per-unit monomorph owner を追加。 |
| P2: options ledger と native descriptor の precedence が逆 | safe output/arena check と descriptor check を分離し、header/line-ending/row bound を complete descriptor より先に検証。negative bound + malformed descriptor owner を固定。 |
| P2: cache 文が required CPU features を除外 | 全 ordinary explicit target/CPU/features/profile/pipeline/runtime/link input を維持し、ambient runtime detection/data-dependent CSV state だけを除外。explicit-target edit/revert owner を追加。 |
| P2: summary が zero-row success にも allocation を約束 | `N==0` は `{null,0}`/zero allocation、one-block は `N>0` だけと ledger と全 normative summary で明記。 |

candidate `b4d15acd` の required strategy re-review は P1 3 件、P2 2 件。2 回目の新 P1 のため closure
matrix を `generic-ffi-schema` 軸で再オープンし、line-local patch でなく boundary を再設計した。

| finding | reopened-matrix repair |
|---|---|
| P1: concrete-only `struct_id` では generic wrapper の abstract check が形成不能 | exact `row: Ty` に変更。discarded template HIR は bound-matching `Ty::Param(p)`+`SoaParam(p)` だけ、retained AST の monomorph recheck は `Ty::Struct(id)`、emitted HIR は symbolic form 全拒否。wrapper/forwarding/substitution/emission owner を追加。 |
| P1: empty input の valid `{null,0}` が ABI 未定義 | zero length は pointer を問わず dereference/slice なし、positive は nonnull complete readable range。null-zero/null-positive runtime twins を追加。 |
| P1: typed descriptor table の alignment precondition 欠落 | `align_of::<CsvField>()` を要求して typed access 前に guard、compiler global も align。arena/output も同じ class で監査。 |
| P2: package-only natural-layout rule が existing `SoaPlain` bound を狭める | extra rule を削除。SoA layout は AoS record layout を無視し、`layout(C)`/`align(N)` を positive owner にする。 |
| P2: precedence summary が全 private abort を options より前に置く | malformed output/arena と malformed descriptor/input を option phase の前後に分け、numbered order と一致。 |

次の candidate `5b1b6aaf` の fresh review は P1 1 件、P2 2 件。P1 は reopened bound matrix が layout は
閉じたが cardinality を閉じていないことを示したため、`bound-capacity-raw-text` で再度 open した。

| finding | reopened-matrix repair |
|---|---|
| P1: 1024 schema-field cap がなお `SoaPlain` を狭める | sema/HIR/descriptor count/全 summary から schema cap を削除。1024 は physical Present-header cap だけ。Absent は 1025-field 以上も許可し、Present は missing coverage で `Invalid`、1025th physical header がある場合だけ先に `LimitExceeded`。 |
| P2: raw input ABI の invalid UTF-8 semantics 欠落 | pointer/descriptor phase 後、BOM/CSV 前に allocation-free complete UTF-8 prevalidation。invalid は private `-1`。成功は UTF-8 walk 1 + decode pass 2。 |
| P2: descriptor name の embedded-NUL semantics 欠落 | descriptor phase で exact ASCII source-identifier grammar と reserved-token exclusion を検証。NUL と全 non-source spelling は input 前 private `-1`。 |
