# core.codec — カラムナバッチのワイヤ形式

> 🌐 [English](../codec.md) · **日本語**
>
> **状態:** 設計済み、実装待ち。

## 権威ある公開契約レジャー

この表が最初の `core.codec` capability の権威である。後続の本文と実装は各項目を明確化
してよいが、範囲を広げてはならない。この形式はデータバッチだけを運び、呼び出し、
サービス、実行計画、任意のユーザー型は運ばない。

| 公開面 | 正確な入力、検証、評価 | 正確な結果、エラー、effect | ownership、lifetime、allocation、cleanup | compiler/runtime owner と identity | acceptance owner |
|---|---|---|---|---|---|
| `codec.kind { I64, F64, Bool, Str }` | 閉じた builtin tag-only sum は1つだけ。source ordinal と wire tag は `I64=0`、`F64=1`、`Bool=2`、`Str=3`。数値変換、custom kind、nullable modifier、extension metadata、alias、ambient registry はない。 | Copy かつ Pure。equality は既存の closed-enum rule。source から作れる値では error も abort もない。 | 確定済みの一フィールド enum aggregate `{ i32 tag }`。borrow、allocation、Drop はない。 | `import core.codec` の背後にある唯一の builtin enum を `align_sema` が所有し、HIR/MIR は通常の enum aggregate を保持する。interface は nominal named definition を serialize する。ordinal は descriptor の `kind` byte と同一。 | import/type/variant positive、正確な ordinal、wrong module/type negative、checked-HIR enum identity/closed-tag validation、whole/per-unit identity。 |
| `codec.open(input: bytes) -> Result<codec.batch, Error>` | positional argument は1つだけ。`input` を borrow し、一度だけ評価する。成功する input は最大1024列のcanonical v1 envelope。先頭 address は任意alignment。owned/heap allocationなしで、fixed 4096-byte stack scratchと10-pass bottom-up merge sortによりname uniquenessを検証する。unknown tag/flag/version、overflow、over-limit、非canonical topology/padding、duplicate/invalid name、malformed buffer/UTF-8、trailing、positive-length nullを拒否する。 | Pure。成功時は validated batch view。malformed は `Err(Error.Invalid)` でowned allocation/mutation/partial batchなし。dangling pointerはsafe ABI precondition外。 | `codec.batch` は input のCopy opaque viewでgenerationを運び、`region_of(result)=region_of(input)`。derived view live中のinput owner move/replace/drop/mut borrowを拒否。 | Sema/HIR/MIR/runtimeの正確なResult/region/validator owner。capability/version/native/type leavesはfingerprintへ入る。 | 独立golden、precedence、truncation、overflow、1024/1025と4096-byte scratch、common-prefix work、base alignment 0..7、no-owned-allocation、region/control、whole/per-unit、malformed HIR/ABI。 |
| `b.rows() -> i64`; `b.columns() -> i64` | `b` は validated `codec.batch` で read-only borrow。 | Pure かつ total な Copy count。再検証しない。 | allocation、retained borrow、mutation はない。 | checked batch scalar field の direct load。 | empty/nonempty/max-admitted count、反復、imported/carrier-held batch。 |
| `b.name(i: i64) -> Option<str>`; `b.kind(i: i64) -> Option<codec.kind>`; `b.find(name: str|string) -> Option<i64>` | receiver、argument の順に一度だけ評価する。negative/out-of-range `i` は `None`。`find` は ordinal 順に byte-exact、case-sensitive 比較し、唯一の最初の一致を返す。なければ `None`。owned `string` は auto-borrow。 | Pure かつ total。`name` は zero-copy UTF-8 view、`kind` と ordinal は Copy。error/abort はない。 | `name` とその `Option` は batch/input region を運ぶ。`find` は何も保持しない。allocation はない。 | name/kind projection と `find` は validated envelope に対する optimizer-visible descriptor load と ordinal-order byte loop へ lower する。追加 native ABI row、source/artifact I/O、reflection はない。 | negative/in-range/upper-bound の積、empty/non-ASCII/NUL name、exact-case miss、ordered lookup、反復、region escape/invalidation、no revalidation/allocation。 |
| `b.i64s(i) -> Option<codec.i64_column>`; `b.f64s(i) -> Option<codec.f64_column>`; `b.bools(i) -> Option<codec.bool_column>`; `b.strs(i) -> Option<codec.str_column>` | receiver、ordinal を一度ずつ評価する。negative/out-of-range または異なる kind は `None`、一致時は全 row の view。再検証しない。4 view は全 input base alignment を許す。 | Pure かつ total。すべて zero-copy。`f64` は NaN/infinity を含む IEEE bit pattern をそのまま保つ。kind 間 coercion、alignment-dependent unavailable state はない。 | 返す view と `Option` はすべて Copy で batch input と generation に region-bound。allocation、ownership transfer、Drop はない。 | 4つの distinct checked HIR/MIR projection は4つの opaque `{ ptr, len }` class scalar を作る。typed pointer は形成しない。 | 4×4 kind/accessor、negative/out-of-range、全 base alignment、numeric/NaN bytes、region carrier/control/return/mutation rejection、element loop に runtime call がないこと。 |
| `codec.i64_column`: `c.len() -> i64`; `c.at(i: i64) -> Option<i64>` | `c` を borrow。`at` はindexを一度評価し、範囲外で`None`、範囲内でrowの8 little-endian bytesを読む。 | Pure、total、Copy、allocation-free。 | scalar resultにregionなし。column viewはinput-region-boundのまま。 | bounds comparison、alignment-1 i64 bit load、target-required byte swapへlowerする。bounds成功前にtyped pointerを作らない。 | empty、全base/element alignment、negative/upper-bound、signed extrema、optimized/unoptimized、little-/big-endian owner。 |
| `codec.f64_column`: `c.len() -> i64`; `c.at(i: i64) -> Option<f64>` | `c` を borrow。`at` はindexを一度評価し、範囲外で`None`、範囲内でrowの8 little-endian bytesを読む。 | Pure、total、Copy、allocation-free。全IEEE bit patternを正確に保持。 | scalar resultにregionなし。column viewはinput-region-boundのまま。 | bounds comparison、alignment-1 i64 bit load、target-required byte swap、f64 bitcastへlowerする。bounds成功前にtyped pointerを作らない。 | empty、全base/element alignment、negative/upper-bound、infinity/fixed NaN payload、optimized/unoptimized、little-/big-endian owner。 |
| `codec.bool_column`: `c.len() -> i64`; `c.at(i: i64) -> Option<bool>` | `c` を borrow。`at` はindexを一度評価し、範囲外で`None`、範囲内でArrow LSB-first bitを読む。 | Pure、total、Copy、allocation-free。 | scalar resultにregionなし。column viewはinput-region-boundのまま。 | bounds comparison、byte load、shift、maskへlowerする。bounds成功前にaddressを作らない。 | empty、全base alignment、byte boundary、bool tail、negative/upper-bound、optimized/unoptimized owner。 |
| `c.len() -> i64`; `c.at(i) -> Option<str>` for `codec.str_column` | `c` を borrow。index は一度だけ評価し、範囲外は `None`、範囲内では validated adjacent i32 offset から正確な UTF-8 cell を返す。empty string、embedded NUL/LF は通常データ。 | Pure、total、zero-copy、allocation-free。 | `str` と `Option` は元の batch/input region と generation を運ぶ。 | bounds comparison 1回、alignment-1 little-endian i32 load 2回と target-required byte swap、view construction へ lower。offset monotonicity/bounds/per-cell UTF-8 は `open` 済み。 | empty/non-ASCII/NUL/LF、repeated offset、全 base alignment、negative/upper-bound、return/carrier/control region、optimized/unoptimized、whole/per-unit。 |
| `codec.encoder(rows: i64) -> Result<codec.encoder, Error>` | positional argument は1つで一度だけ評価する。negative row は allocation 前に `Err(Error.Invalid)`。nonnegative row は zero-column encoder を作る。ambient schema/allocator setting/file/clock/target input はない。 | Pure。成功時は initialized encoder。OOM は language-wide hard abort。 | Move handle は encoder shell 1つと、後続で copy する column/name staging を所有する。argument は保持しない。Drop は staged byte を一度だけ解放し、output を出さない。 | nominal `Ty::CodecEncoder`/MIR scalar と keyed runtime constructor/drop pair。interface identity は named builtin type と既存 Move return rule。 | negative/zero/positive rows、no-allocation error、one-shell allocate/free、direct/imported/function-value return/Drop、malformed HIR/ABI。 |
| `e.put_i64(name, values: slice<i64>)`; `e.put_f64(name, values: slice<f64>)`; `e.put_bool(name, values: slice<bool>)`; `e.put_str(name, values: slice<str>)` — 各 `-> Result<(), Error>` | `e` は bound mutable encoder で非consuming。receiver、name、values の順に一度ずつ評価する。`name` は `str|string`、nonempty valid UTF-8、u32 length 内、成功済み列と byte-unique。成功列は最大1024。`values.len() == rows`。`put_str` は copied cell bytes 合計が signed i32 内。candidate name/count/length/kind size/final length/全 string cell を最初の mutation 前に検証する。 | Pure。成功時は列を正確に1つ append。invalidity/representability/format-limit failure は `Err(Error.Invalid)` で encoder bytes/count/order/future output を変えない。OOM は abort。numeric は little-endian bit を保持、bool は LSB-first pack、string は bytes と canonical i32 offsets を copy。 | name/values は call 中だけ borrow し encoder staging へ copy。input region を保持しない。明示的に構築した encoder 内で staging は grow できるが、allocation count、peak ratio、performance promise はない。 | 4つの checked operation/keyed runtime entry が pre-mutation validation/commit owner を共有する。`slice<str>` は settled header layout を使い extern view として渡さない。runtime は compiler-private valid-range precondition 下で header を読む。 | evaluation order、全 invalidity/precedence、failure-then-retry、1024th success/1025th no-op error、duplicate/case name、row、numeric/NaN、bool tail、UTF-8/NUL/LF、allocation/fatal、whole/per-unit/generic。 |
| `e.finish() -> buffer` | `e` は bound initialized encoder で、receiver check 後に一度だけ consume。nonnegative rows に zero successful columns も有効。 | Pure。下記 canonical v1 envelope を生成。source-dependent limit は transactional admission 済みなので recoverable error は返さず、OOM は abort。 | returned Move `buffer` が final contiguous bytes を所有する。finish は encoder shell/staging を consume/free し source を null にする。return point では output buffer だけが live。 | checked consuming operation 1つと keyed runtime finisher が既存 buffer representation/Drop を再利用。final byte range に exact allocation 1つを emit。staging allocation は promise しない。 | zero/one/four columns、order、failed put 後 finish、source nulling、early/control return、sole final owner、golden、allocation/fatal failpoint、whole/per-unit。 |

## source surface

```align
import core.codec

fn encode(
  ids: slice<i64>,
  scores: slice<f64>,
  flags: slice<bool>,
  names: slice<str>,
) -> Result<buffer, Error> {
  mut out := codec.encoder(ids.len())?
  out.put_i64("id", ids)?
  out.put_f64("score", scores)?
  out.put_bool("active", flags)?
  out.put_str("name", names)?
  return Ok(out.finish())
}

fn first(input: bytes) -> Result<str, Error> {
  batch := codec.open(input)?
  index := batch.find("name") else { return Err(Error.Invalid) }
  names := batch.strs(index) else { return Err(Error.Invalid) }
  return Ok(names.at(0) else "")
}
```

declaration と positional call は別である。written type argument、named argument、schema
reflection、variadic column、map、macro、annotation、RPC method はない。column order は
insertion/wire order。name は normalization されない identity である。

## canonical v1 envelope

envelope と buffer の整数はすべて little-endian。各 buffer **offset** は8の倍数だが、
enclosing `bytes` address は任意alignmentでよく、accessor はalignment-1 loadを使う。
`total_len` は完全一致し、prefix/suffix framing と trailing byte は認めない。

### header — 32 bytes

| offset | width | field | canonical rule |
|---:|---:|---|---|
| 0 | 8 | magic | ASCII `ALNCOL01` (`41 4c 4e 43 4f 4c 30 31`) |
| 8 | 8 | `total_len` | exact envelope length、`32..=i64::MAX` |
| 16 | 8 | `row_count` | `0..=i64::MAX` |
| 24 | 4 | `column_count` | unsigned `0..=1024`。larger は descriptor access 前に reject |
| 28 | 4 | reserved | zero |

直後に `column_count` 個の48-byte descriptor が続く。

### column descriptor — 48 bytes

| relative offset | width | field | canonical rule |
|---:|---:|---|---|
| 0 | 8 | `name_offset` | packed name section の exact next byte |
| 8 | 4 | `name_len` | positive byte length |
| 12 | 1 | `kind` | `codec.kind` tag 0..3 |
| 13 | 1 | flags | zero。v1 は nullable/compression/dictionary なし |
| 14 | 2 | reserved | zero |
| 16 | 8 | `data_offset` | exact aligned next buffer offset |
| 24 | 8 | `data_len` | exact kind-derived data length |
| 32 | 8 | `aux_offset` | string-values buffer offset。それ以外は zero |
| 40 | 8 | `aux_len` | string-values length。それ以外は zero |

name section は `32 + 48 * column_count` から始まる。name は nonempty valid UTF-8、
byte-unique で、descriptor 順に separator/gap なしで pack する。末尾を zero byte で8に
pad する。その後の buffer は descriptor 順。data buffer は current aligned cursor から
始まり zero で8に pad する。`Str` の aux は offsets の直後に同じ規則で置く。最後の
padding が `total_len` で終わる。全 padding が zero なので1つの semantic batch に1つの
byte encoding しかない。

### kind buffer rule

| kind | `data` | `aux` | canonical rule |
|---|---|---|---|
| `I64` | `rows * 8` contiguous signed i64 | absent | Arrow fixed-width primitive、validity bitmap なし |
| `F64` | `rows * 8` contiguous IEEE-754 binary64 bits | absent | 全 bit pattern を許可、validity bitmap なし |
| `Bool` | `ceil(rows / 8)` bytes | absent | Arrow boolean bitmap。row `i` は `(byte[i/8] >> (i%8)) & 1`、unused tail bits は zero |
| `Str` | `(rows + 1) * 4` signed i32 offsets | concatenated UTF-8 bytes | Arrow Utf8 variable-binary、validity bitmap なし。first offset 0、monotonic/nonnegative、last は `aux_len <= i32::MAX`、各 adjacent range は valid UTF-8 |

これは Arrow と互換な **physical buffer layout** であり、Arrow IPC、FlatBuffers metadata、
Arrow C Data Interface、Parquet、compression、または Arrow 実装が `ALNCOL01` envelope を
直接読めるという約束ではない。v1 は non-null struct batch で、child buffer が Arrow の
non-null `Int64`、`Float64`、`Boolean`、32-bit-offset `Utf8` layout に一致する。absence は
batch 外で表し、nullable value と validity bitmap は一緒に defer する。

format と source surface は target-independent。accessor は little-endian bits を inline
decodeし、big-endian target は column copy/API changeなしで必要なbyte swapを加える。

## validation order と error precedence

malformed envelope はすべて同じ `Error.Invalid` だが、work と pre-side-effect behavior は
deterministic で、最初に失敗した step で止まる。

1. safe-view private precondition、minimum header、magic、header reserved、`total_len`、exact
   input length の順。base address alignment は validity condition ではない。
2. row/count exposure、`column_count <= 1024`、descriptor arithmetic、complete descriptor-table
   bounds を descriptor read 前に検証。
3. ordinal 順に kind、flags/reserved、scalar representability、kind-derived length。name/buffer
   はまだ読まない。
4. packed name topology、全nameのnonempty/bounds/UTF-8をordinal順に先に検証。2つのfixed
   `[u16; 1024]` stack array（4096 bytes）へordinalを入れ、byte-lexicographic name then ordinal
   でexact 10-pass stable bottom-up merge sortし、adjacent equalを拒否。最後にzero name
   padding。invalid later nameはduplicateより先で、最大9,217 lexicographic comparisons。
5. data/aux cursor を ordinal 順に再計算し、exact offset/length、8-byte bounds、absent-aux
   zeros、zero padding、final `total_len` equality。
6. ordinal 順に content。Bool unused tail bits、または Str offsets の first-to-last と各
   string cell UTF-8。numeric content validation はない。

各 arithmetic check は保護対象 memory read より前。length prefix から allocation しない。
multi-invalid fixture が全 precedence boundary を所有し、全 step 成功まで output scalar を
書かない。

encoder call は mutation 前に receiver validity、name length/nonempty/UTF-8、
admitted-column-count limit、row length、kind-specific representability/cell walk、duplicate
name、prospective complete-envelope arithmetic の順。OOM は terminal で `Error.Invalid` へ
変換しない。

## golden vector

production encoder と validation/view decoder は独立実装とし、expected bytes を相手側の
codec 呼び出しで作らない。checked-in golden は最低限次を含む。

| vector | semantic value | purpose |
|---|---|---|
| `empty-0x0` | 0 rows、0 columns | fixed header、zero-column/nonzero-row twin、exact length/trailing |
| `i64-two` | name `i`、`[-1, 2]` | signed little-endian と padding |
| `f64-bits` | name `f`、`1.5` と fixed quiet-NaN payload | exact IEEE bits |
| `bool-tail` | name `b`、9個の fixed bool | LSB order、byte boundary、zero tail |
| `str-mixed` | name `s`、`["", "a\0", "あ\n"]` | repeated i32 offset、NUL、multibyte UTF-8、LF |
| `all-four` | equal row count の4 named columns | descriptor/name/buffer order、cross-kind topology |

exact lowercase hexadecimal v1 bytes は英語 ledger と同じく次である。

```text
empty-0x0 (32 bytes)
414c4e434f4c3031200000000000000000000000000000000000000000000000

i64-two (104 bytes)
414c4e434f4c3031680000000000000002000000000000000100000000000000
5000000000000000010000000000000058000000000000001000000000000000
000000000000000000000000000000006900000000000000ffffffffffffffff
0200000000000000

f64-bits (104 bytes; 1.5 = 0x3ff8000000000000,
fixed quiet NaN = 0x7ff8000000000042)
414c4e434f4c3031680000000000000002000000000000000100000000000000
5000000000000000010000000100000058000000000000001000000000000000
000000000000000000000000000000006600000000000000000000000000f83f
420000000000f87f

bool-tail (96 bytes)
414c4e434f4c3031600000000000000009000000000000000100000000000000
5000000000000000010000000200000058000000000000000200000000000000
0000000000000000000000000000000062000000000000008d01000000000000

str-mixed (112 bytes)
414c4e434f4c3031700000000000000003000000000000000100000000000000
5000000000000000010000000300000058000000000000001000000000000000
6800000000000000060000000000000073000000000000000000000000000000
02000000060000006100e381820a0000

all-four (296 bytes; rows は i64 [-1,2]、f64 [1.5,-0.0]、
bool [true,false]、str ["x",""])
414c4e434f4c3031280100000000000002000000000000000400000000000000
e0000000000000000100000000000000e8000000000000001000000000000000
00000000000000000000000000000000e1000000000000000100000001000000
f800000000000000100000000000000000000000000000000000000000000000
e200000000000000010000000200000008010000000000000100000000000000
00000000000000000000000000000000e3000000000000000100000003000000
10010000000000000c0000000000000020010000000000000100000000000000
6966627300000000ffffffffffffffff0200000000000000000000000000f83f
0000000000000080010000000000000000000000010000000100000000000000
7800000000000000
```

各 vector は fixed semantic-to-byte と byte-to-semantic assertion を持つ。one-byte mutation は
magic/version/tag/reserved/padding/offset/length/tail/UTF-8 class を覆い、truncation は全 byte
boundary を parameterize する。別 fixture はvalid vectorをbase residue 0..7へ置き同じsemantic
resultを証明し、`finish().bytes()`を直接reopenする。

## type、region、placement closure

compiler-private scalar layout は固定する。batch は `{ input_ptr, input_len }` で hidden
certificate pointer はない。I64/F64/Bool column は各 `{ bytes_ptr, row_len }`。string column は
`{ offsets_ptr, data_ptr, row_len }`。pointer は target width、length は i64。column pointer を
typed-aligned pointer に昇格しない。`codec.encoder` は live 中1つの nonnull runtime-owned
pointer。これらは source `layout(C)` record/extern parameter/return ではない。

`codec.batch`、4 typed column は opaque Copy view。各 view と
Option/Result、struct field、parameter、return、control carrier は canonical region-bearing
classifier を通じて正確な input region/storage generation を運ぶ。この capability では
user literal、cast、`raw`、extern return、constant/global、array element、parallel value、
closure/task capture から構築できない。numeric projection もopaque column view ruleに従う。

`codec.encoder` は `buffer`/`builder` の bare Move accumulator class に従う。local、by-value/
shared/mutable parameter、direct return、通常の Option/Result/user-sum/struct carrier は同じ
canonical single-owner classifier が許す場合だけ許可する。move-in/out、replacement、
consuming match、`else`、`?`、`map_err`、branch/loop join、early return、finish、Drop は one live
owner または none を保ち、consumed source を null にする。array/slice/fixed array/tuple/box、
parallel value、capture、global/constant、user native/extern ABI、unbound mutating receiver は
MIR 前に拒否する。

staging は成功した call 中に argument を copy するため region を蓄積しない。failed put は
commit 前 allocation failpoint も含め no-op。finish だけが staging ownership を returned
`buffer` へ移し、Drop without finish は何も publish しない。

## effect、allocation、performance boundary

source operation はすべて Pure。external I/O はない。allocation は `codec.encoder(...)`
constructor と returned Move storage に見える。`open` と全 accessor は zero owned/heap
allocationでfixed 4096-byte stack scratchを使う。1024列で10 merge pass、最大9,217
lexicographic comparisons。`find` O(columns)、access O(1)。encoderはsorted name indexを
binary searchし、encodingはO(output bytes + columns² fixed-index movement)。
throughput、syscall、exact staging allocation count、peak-memory ratio、SIMD width、compression
ratio は約束しないので benchmark は correctness gate ではない。

4 typed `at` は optimizer-visible で opaque per-element runtime helper を呼ばない。numeric
load はalignment 1を明示し、unaligned inputも安全でLLVMのunaligned-load vectorization対象。
`pkg.frame` がfuture virtual pipeline/fused scanを所有する。static-input artifactも measured
consumer と別ledgerが必要で、v1 bytes は変えない。

## capability boundary と defer

v1 には null/validity bitmap、unsigned/narrow int、f32、binary cell、nested list/struct/
dictionary/union、timestamp/decimal、mutable batch、row append、schema reflection、`soa<T>`
conversion、mmap/file、stream/fragment、compression、encryption/checksum、endianness switch、
Arrow IPC/Flight/C Data export、RPC、query plan、dataframe operation、stable cross-major promise は
ない。`pkg.frame` は最初の dynamic consumer だが別 capability。

v2 は別の8-byte magic と新しい complete ledger を使う。v1 decoder は future flag/tag を
受理せず、v1 encoder は emit しない。widening は one-way source surface を保つか release 前に
置換し、compatibility alias/permissive reader を推測で追加しない。

## implementation closure matrix

independent review がこの ledger を accept するまで implementation を始めない。1つの
implementation capability で各 applicable row を閉じる。

| axis | required closure | owner evidence |
|---|---|---|
| type formation/identity | unique module/types/enum、imports、nominal/interface/generic/whole/per-unit、canonical type/capability/runtime fingerprint、new-type sweep | Sema、interface/type golden、checked-HIR sweep |
| batch validation | exact six stages、no pre-success output、arithmetic-before-read、zero owned/heap allocation、1024/1025、fixed 4096-byte name scratch、10 stable merge pass、canonical topology/content、independent decoder | runtime mutation/truncation/precedence/allocation matrix、limit/next、common-prefix distinct/duplicateと最大9,217 comparisons、driver owner |
| region/generation | direct/field/parameter/return、Option/Result/sum/struct、全 control join、mutation/replacement/drop rejection、malformed HIR | batch/name/4 typed column の parameterized provenance/borrow-liveness |
| projection/codegen | 4 kind product、total ordinal、4 inline element path、alignment-1 numeric/offset load、target byte order、no pre-bounds typed pointer/native call | base alignment 0..7 driver、LLVM structural、index differential、NaN/endian |
| encoder ownership | construction、4 put、failed-put retry/finish、move/null/Drop/replacement/return/carrier/control、全 allocation failpoint | driver ownership/control、runtime allocation ledger |
| native ABI | compiler declaration/Rust export parity、output init、null/alignment/length、no unwind、status、allocator provenance、whole/per-unit | ABI registry/attribute、malformed-ABI unit tests |
| canonical encoding | transactional check、exact order/padding、limits、sorted name index/binary search、1025th pre-mutation rejection、independent encoder、sole final buffer | 6 goldens both directions、mutation、1024th success/1025th no-op、common-prefix、retry、allocation parity |
| compatibility | existing binary ops/slices/arrays/JSON/SoA/cache/build/current little-endian unchanged。explicit byte-order loweringはbig-endianでもsource/wire変更なしに正しい | focused compatibility、whole/per-unit/cache、synthetic endian twins |

## design-review finding closure

| finding | ledger-first closure |
|---|---|
| P1 existing `buffer` の `Vec<u8>` がdecoder必須の8-byte base alignmentを保証せず、encoder outputを自分でopenできない | base alignmentをwire validityから除去し、standard numeric sliceをsymmetric opaque i64/f64 column viewへ置換。numeric/string offsetはalignment 1とexplicit little-endianでlowerし、全base alignmentと`finish().bytes()` roundtripをownする。Buffer cross-cutting changeは不要。 |
| P2 allocation-free duplicate-name checkがu32列までquadratic amplificationを許した | v1を1024列に固定し1025を事前拒否。decoderは2つのfixed `[u16;1024]` と10-pass merge sort（最大9,217 lexicographic comparisons）、encoderはsorted index/binary search。limit/next/common-prefix/scratch/precedence ownerを固定。 |

## source of truth と author consistency pass

この日本語 mirror、[英語 ledger](../codec.md)、`draft.md`、`docs/language-spec.md`、
`docs/design-notes.md`、`docs/history.md`、`docs/open-questions.md`、`docs/impl/07-roadmap.md`、
`docs/impl/19-hir-validation-ledger.md`、`docs/impl/20-runtime-abi-ledger.md` は implementation 前に
一致させる。implementation PR は public contract を reopen せず concrete variant/symbol row を
後二つへ追加できる。

design candidate の author-side pass は完了している。

- 全 argument/result に exact type、evaluation、ownership、region、allocation、error rule がある。
- kind/ordinal/presence の積に exhaustive field/unavailable rule がある。
- encoding、UTF-8、NUL、validation precedence、pre-side-effect behavior を固定した。
- 全 scalar width/tag/order/padding/malformed rule と independent golden を固定した。
- ambient config、reflection、source/artifact I/O、later milestone、RPC は入らない。
- runtime inspection は source/reflection でなく producer-validated envelope table を読む。
- example は declaration と positional call を分離し settled syntax を使う。
- acceptance owner が全 invariant を覆い、未宣言の performance benchmark は不要である。
