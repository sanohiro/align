# std.xml — 公開契約と実装設計

> 🌐 [English](../xml.md) · **日本語**

> **状態:** DESIGNED 2026-09-05。実装は未着手。

## 権威ある公開契約 ledger

この表を最初の `std.xml` capability の authority とする。後続 prose と implementation は cell
を明確化してよいが、拡張してはならない。V1 は memory 上の UTF-8 XML 1.0 document を読む、
bounded な forward-only reader 1 個である。DOM、validating parser、namespace processor、query
language、serializer、file/network loader、general entity processor ではない。

| 公開 surface | 正確な input、default、validation、evaluation | 正確な result、error、effect | ownership、lifetime、allocation、cleanup | compiler/runtime owner、artifact/cache identity | prerequisite と acceptance owner |
|---|---|---|---|---|---|
| `xml.event { Start, End, Text }` | closed source/discriminator order は exact に `Start = 0`、`End = 1`、`Text = 2`。comment、declaration、processing instruction、attribute、EOF、error、unknown event tag はない。 | Copy、Pure。source-formed value に error/abort path はない。`next` の `None` だけが EOF。 | settled one-field enum aggregate `{ i32 tag }`。borrow、allocation、Drop、numeric conversion、retained state はない。 | `align_sema` が `import std.xml` 配下の unique builtin nominal definition と qualified type/variant resolution を所有。HIR/MIR は ordinary enum aggregate を保持し、interface/cache identity は既存 nominal named-type grammar を使う。 | exact tag/order、construction/match、qualified import、malformed checked-HIR/MIR、interface round trip、cache mutation。 |
| opaque Move `xml.reader` | successful `xml.parse` だけが reader を作る。bound/initialized/single-owner forward cursor である。public constructor、field、clone、reset、seek、raw input accessor、conversion、default はない。 | in-memory value として Pure。mutable cursor のため concurrent/shared `next` は不可。Send、Sync、print、equality、collection storage、parallel capture、global、native source compatibility はない。 | consumed input allocation と runtime shell 1 個を所有。Move は両方を transfer、Drop は両方を exactly once free。既存 region/Drop rule が許す ordinary handle carrier と scalar tagged payload を通って move できる。 | new `Ty::XmlReader` / `Scalar::XmlReader`、checked HIR/MIR ownership、canonical type-codec leaf `72` / `48`、runtime key selection、Drop key。whole/per-unit identity は imported module、used operation、compiler/runtime implementation、target、ordinary build input を含む。locale/environment/filesystem/network/MIME/namespace registry は含まない。 | move-in/out/nulling、replacement、direct/Result/user-sum return、`if`/`match`/`else`/`?`/`map_err`/loop/early exit、malformed cleanup、whole/per-unit/generic、exact type codec、one-shell/input-free。 |
| `xml.parse(input: string) -> Result<xml.reader, Error>` | `input` を exactly once evaluate/consume。`string` により既に valid UTF-8。下記 exact profile を parse する。leading U+FEFF は optional で 1 個。XML declaration は optional だが先頭だけ、version は exact `1.0`、encoding は optional case-insensitive `UTF-8` だけ、standalone は declaration order の `'yes'|'no'` だけ。root は exactly one。element depth と one-element attributes の maximum は inclusive 256、次は invalid。option/ambient encoding/MIME default はない。 | complete-document validation 後だけ `Ok(reader)`。empty input、malformed XML、forbidden markup、unsupported declaration/encoding、invalid XML character/name/reference、duplicate attribute、mismatched nesting、second root、いずれかの bound は `Error.Invalid`。OOM、impossible status、malformed compiler-private ABI は hard abort。Pure。I/O、lookup、logging、callback、entity fetch、partial publication はない。 | call entry で ownership transfer。failure は input を exactly once free し handle を返さない。success は元 input allocation をそのまま retain し、fixed-size reader shell 1 個だけ allocation。validation は fixed 256-entry stack scratch を使い owned allocation 0。input copy、event tape、tree、namespace/entity table、per-element allocation はない。 | checked `XmlParse` HIR/MIR。runtime `align_rt_xml_parse` は既存 A08 `i32(ptr, i64, ptr)`。status は `0 = success`、`-1 = public Error.Invalid`、positive `AL_INVALID = malformed private ABI`。他は不可。XML を使う時だけ capability/runtime fingerprint が select する。 | shipped owned string/Result/Move pattern、W3C/cloud corpus、declaration/grammar/character/name/reference Cartesian matrix、exact limits、first-error/no-publication、allocation/free、direct/imported/function-value、whole/per-unit、cache、ABI、optimized/unoptimized。 |
| `r.next() -> Option<xml.event>` | `r` を mutably borrow し exactly once advance。下記 event stream を emit。empty element は `Start` の後に synthesized `End` 1 個。EOF 後は繰返し `None`。 | valid live reader には total、Pure、allocation-free。null/moved/malformed/aliased private reader は input access 前 hard abort。 | call 中だけ reader borrow。外部 value を retain せず shell の cursor/current-event field だけ mutate。returned Copy enum は Static。 | checked `XmlNext`。`align_rt_xml_next` は A03 `i32(ptr)`、`0=None`、`1=Start`、`2=End`、`3=Text`。他の i32 は abort。 | exact event order、empty pair、skipped markup、repeated EOF、mutable receiver、no allocation、malformed state、direct/imported/generic/whole/per-unit。 |
| `r.name() -> str` | current event が `Start`/`End` の時だけ valid。UTF-8 decode 後の exact lexical element `Name` を original case/colon bytes のまま返す。empty element の synthesized `End` は start-tag name。first `next` 前、`Text`、EOF 後は programmer error abort。 | Pure、admitted state で total、allocation-free。namespace expansion/prefix resolution/Unicode normalization/case folding/entity decode はない。 | zero-copy view は reader-owned input を指し、reader current cursor state に region-tied。reader より長生きせず mutable `next` をまたげない。`.clone()` が explicit escape。 | checked `XmlName`。`align_rt_xml_name` は A19 `i32(ptr, ptr)`、status zero の時だけ `{ptr,i64}` str view を write。nonzero は abort。 | Start/explicit-End/synthesized-End、Unicode/colon、current-state negative、region escape/clone、move/Drop、raw view、ABI output。 |
| `r.attribute_count() -> i64` | `Start` の時だけ valid。exact source-order count `0..=256`。namespace declaration も ordinary attribute。wrong state は abort。 | Pure、admitted state で total、allocation-free。 | call 中だけ reader borrow。result は Copy/Static。 | checked `XmlAttributeCount`。`align_rt_xml_attribute_count` は A29 `i64(ptr)`。negative または 256 超は abort。 | zero/exact-limit、Start/self-closing、wrong/malformed state、no-allocation、ABI。 |
| `r.attribute_name(index: i64) -> str` | `Start` の時だけ valid。`index` は zero-based source order で `0..<attribute_count`。exact lexical XML `Name` を返す。negative/out-of-range/wrong state は abort。 | Pure、admitted state で total、allocation-free。namespace expansion/prefix resolution/Unicode normalization/case folding はない。 | zero-copy view は reader input を指し、`name` と同じ current-cursor region。`.clone()` なしに `next`/reader Drop をまたげない。 | checked `XmlAttributeName`。`align_rt_xml_attribute_name` は A20 `i32(ptr, ptr, i64)`、zero-only success、out は `{ptr,i64}` view。 | source order、Unicode/colon/`xmlns`、bound edge、region escape/clone、wrong state、malformed ABI、whole/per-unit。 |
| `r.attribute_value(index: i64) -> string` | state/index rule は `attribute_name` と同じ。complete XML 1.0 normalized value を返す。predefined/numeric reference を decode。literal tab/LF/CR は document line-end normalization 後に space。character reference が生む文字は再 normalization しない。quote は delimiter。 | Pure。invalid state/index は abort。valid call は fresh owned string 1 個（empty は canonical zero-allocation）。OOM は abort。error/fallback/lazy decode/retained cache はない。繰返し call は allocation/copy も繰返す。 | decode 中だけ reader borrow。nonempty result は right-sized allocation 1 個を所有し cursor/reader より長生きできる。empty は `{null,0}`。reader/input は不変。 | checked `XmlAttributeValue`。`align_rt_xml_attribute_value` は A20 `i32(ptr, ptr, i64)`。state/index/input access 前に owned-string out header を zero。nonzero は abort、string は publish しない。 | quote/entity/character/line-end product、empty/nonempty allocation、repeat、state/index negatives、failure no-publication、Drop、ABI。 |
| `r.text() -> string` | `Text` の時だけ valid。character data は document line-end normalization 後に predefined/numeric reference を decode。CDATA は line-end normalization 後の literal content、entity-looking bytes も literal。各 nonempty source character-data run と各 nonempty CDATA section は別 event。comment は run を分割して skip。wrong state は abort。 | Pure。fresh owned string 1 個。empty は `Text` として emit しない。OOM abort。繰返し call は allocation/copy を繰返す。 | call 中だけ reader borrow。result は right-sized allocation 1 個を所有し cursor/reader より長生きできる。reader/input は不変。 | checked `XmlText`。`align_rt_xml_text` は A19 `i32(ptr, ptr)`。state/input access 前に out を zero。nonzero は abort、string は publish しない。 | entity/CDATA/comment boundary、whitespace/line end、parse 時 non-ASCII/NUL rejection、repeat allocation、wrong state、Drop、malformed ABI、whole/per-unit。 |

## Source surface と use

```text
xml.event { Start, End, Text }
xml.reader

xml.parse(input: string) -> Result<xml.reader, Error>
r.next() -> Option<xml.event>
r.name() -> str
r.attribute_count() -> i64
r.attribute_name(index: i64) -> str
r.attribute_value(index: i64) -> string
r.text() -> string
```

`event`/`reader` は `import std.xml` 後の qualified `xml.event`/`xml.reader` だけ。既存 unqualified
I/O `reader` は不変。bare XML alias、overload、default option、declaration API、source-visible unsafe
entry はない。

```align
import std.xml

fn first_key(body: string) -> Result<string, Error> {
  doc := xml.parse(body)?
  loop {
    event := doc.next() else { break }
    match event {
      Start => {
        if doc.name() == "Key" {
          next := doc.next() else { return Err(Error.Invalid) }
          match next {
            Text => { return Ok(doc.text()) }
            Start => {}
            End => {}
          }
        }
      }
      End => {}
      Text => {}
    }
  }
  Err(Error.NotFound)
}
```

reader が input を retain するため input は `str` でなく owned `string`。HTTP response body は
borrowed view なので、consumer は `xml.parse(response.body().clone())` と copy を明記する。
owned string local は copy なしで transfer。parse path はこれ一つだけ。

## Accepted XML 1.0 profile

lexical authority は [XML 1.0 Fifth Edition](https://www.w3.org/TR/xml/) で、下記 profile を
明示的に適用する。ここで well formed とは forbidden construct を除いた後に applicable XML 1.0
production/WFC を満たす document entity であり、DTD validity や Namespaces in XML conformance
ではない。

consumer acceptance corpus は公開
[Amazon S3 ListObjectsV2](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListObjectsV2.html)
と [Azure List Blobs](https://learn.microsoft.com/en-us/rest/api/storageservices/list-blobs) XML
response を含む。provider status は parser truth ではない。S3 は HTTP 200 body が invalid XML の
場合を明記するため、package consumer は `xml.parse` failure と HTTP status success を分離する。

V1 が exact に accept するもの:

- optional leading U+FEFF BOM 1 個。
- BOM 後の absolute document start に optional XML declaration 1 個。
- root 前後の XML `S` whitespace と comment。
- exactly one properly nested root、ordinary start/end tag、empty-element tag、attribute、character
  data、CDATA、comment、5 predefined entity reference、decimal/hex numeric character reference。
- UTF-8 input のみ。declaration は absent または case-insensitive `UTF-8`。transport/caller encoding
  label で実際の `string` encoding を override できない。

reader publication 前に reject するもの:

- 全 `DOCTYPE`、internal/external DTD subset、entity/notation/element/attribute declaration、
  conditional section、parameter entity、declared general entity、external entity。
- initial XML declaration 以外の processing instruction。XML declaration は event でなく PI として
  扱わない。
- XML 1.1、exact `1.0` 以外の version、non-UTF-8 encoding declaration、text declaration、misplaced/
  repeated XML declaration。
- invalid XML character/name/reference、`--` を含むか `-` で終わる comment、ordinary character data
  の `]]>`、unquoted/duplicate attribute、mismatched tag、root 外の XML `S` 以外の text、missing/
  second root、depth 257、one element の attribute 257。

XML 1.0 Fifth Edition の complete `Char`、`NameStartChar`、`NameChar` range を accept する。name は
Unicode/case-sensitive、colon は ordinary name character。Namespaces in XML の QName/binding rule は
適用しない。`p:item` は exact `"p:item"`、`xmlns` は ordinary attribute。underlying XML `Name` が
well formed なら undeclared/rebound/multi-colon prefix も reject しない。duplicate attribute は expanded
namespace pair でなく exact lexical XML `Name` で比較する。

document entity の raw CRLF/raw CR は token interpretation 前に LF へ normalize し、CDATA/attribute
literal にも適用する。`&#13;` のような numeric reference はその phase 後に CR を生むので CR のまま。
attribute literal の tab/LF は space、numeric reference の該当文字は referenced character のまま。
named reference は `&amp;`、`&lt;`、`&gt;`、`&apos;`、`&quot;` だけ。expansion は一 scalar 単位で
recursive ではなく、XXE/exponential entity expansion は accepted grammar に表現できない。

comment と XML declaration は validate して skip し public event を作らない。各 nonempty ordinary
character-data run は `Text` 1 個、各 nonempty CDATA は separate `Text` 1 個。comment は隣接 run を
分離。empty run/CDATA は event なし。root 内 whitespace は text として preserve、root 外は skip。

## Event、cursor、view rule

```text
<a x="1">left<!-- gap --><![CDATA[<&]]><b/>right</a>
```

exact event sequence:

```text
Start(a), Text("left"), Text("<&"), Start(b), End(b), Text("right"), End(a), EOF
```

attribute は event ではない。`Start(a)` では `attribute_count() == 1`、
`attribute_name(0)` は view `"x"`、`attribute_value(0)` は fresh owned `"1"`。current event/getter
state は次の `next` まで不変。live `name`/`attribute_name` view は ordinary region rule により mutable
`next` を防ぐ。

wrong-event getter、invalid index、use-after-move、malformed private state は同じ programmer-state hard
abort であり、malformed input に偽装しない。document error は全て `parse` が発見し、`next` に
recoverable parse error はない。consumer の error boundary は一つで、invalid cloud response を partial
consume しない。

## Parser、resource、security contract

`parse` は iterative complete-document validation pass。open-element name span 用 fixed 256-entry stack
を使い、input depth 比例の recursive call はない。attribute uniqueness は fixed 256-entry span/hash
scratch で source order に検査し、hash match は byte confirmation。collision は acceptance を変えない。
limit は inclusive/deterministic。depth/attribute 256 は成功、next nesting/attribute admission は later byte
inspection 前に `Error.Invalid`。

success 時 runtime は fixed-size reader shell 1 個を allocation。shell は original string allocation、
cursor/current-event offset、pending empty-end record 1 個、fixed current-start attribute offset を所有。
tree/event tape/copied name/normalized value/entity table/namespace map/source location は保存しない。
`next` は complete traversal で各 source byte を bounded 回 scanし allocation 0。name/count/attribute_name
は current offset から O(1)。text/attribute_value は selected range を2回 scan（output length validation、
exact fill）、nonempty result を1回 allocationし incremental growth はしない。parse + full traversal は
O(input bytes)、resident memory は input allocation + constant-size shell。

XML operation は URL/path/environment/catalog/locale/timezone/MIME/network/process-global setting を
read せず、validation 中 callback/user code なし。全 declaration/PI を publication 前 reject することで、
XXE、DTD retrieval、PI-based XInclude、billion-laughs expansion を configurable switch でなく構造的に
閉じる。

## Validation と error precedence

source checking は exact import/receiver/argument/result、consuming input mode、`next` mutable receiver、
current-view region を先に証明。runtime parse boundary の exact order:

1. nonnull/correctly-aligned writable output header を要求し null を store。
2. Rust slice を形成せず input `{ptr,i64}` representation を検査。length は nonnegative/target-
   representable、positive length は nonnull readable range。source-valid consumed string はその exact
   allocator-compatible range を所有し、output header と disjoint。
3. valid input allocation の責任を operation へ transfer。empty input は public `Error.Invalid`、normal free。
4. optional BOM/declaration、その後 document を left-to-right validate。grammar/character/name/reference/
   uniqueness/nesting/depth/attribute failure は全て同じ public Invalid、input exactly once free、out null。
   diagnostic/subcode がないので二つの XML-invalid byte の優先は public observable ではない。test は
   earliest-boundary work counter と beyond-boundary read absence を pin。
5. complete validation 後だけ shell allocate/init、input ownership を transfer、pointer publish。OOM abort。

getter は out header があれば最初に validateし、owned out は zero。その後 reader null/alignment/private
invariant、current event、index を検査し、初めて input view を形成。owned getter は allocation 前に exact
output length を検査し fill 後だけ publish。Drop は null-safe。nonnull private shell を検証してから input/
shell を exactly once freeし、detectable malformed shell は untrusted input pointer を追う前 abort。

## Compiler、interface、runtime shape

Move type 1、ordinary enum 1、keyed runtime identity 8 個を追加する:

| Runtime key | exact symbol | existing ABI shape/declaration | exact Rust ABI/status |
|---|---|---|---|
| `XmlParse` | `align_rt_xml_parse` | A08: `i32 @SYM(ptr, i64, ptr)` | `unsafe extern "C" fn(*mut u8, i64, *mut *mut XmlReader) -> i32`; `0` success、`-1` public invalid、positive `AL_INVALID` malformed ABI |
| `XmlNext` | `align_rt_xml_next` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut XmlReader) -> i32`; `0..=3` は EOF/event table |
| `XmlName` | `align_rt_xml_name` | A19: `i32 @SYM(ptr, ptr)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr) -> i32`; zero-only success |
| `XmlAttributeCount` | `align_rt_xml_attribute_count` | A29: `i64 @SYM(ptr)` | `unsafe extern "C" fn(*const XmlReader) -> i64`; `0..=256` only |
| `XmlAttributeName` | `align_rt_xml_attribute_name` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr, i64) -> i32`; zero-only success |
| `XmlAttributeValue` | `align_rt_xml_attribute_value` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr, i64) -> i32`; zero-only success、out owns runtime allocation |
| `XmlText` | `align_rt_xml_text` | A19: `i32 @SYM(ptr, ptr)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr) -> i32`; zero-only success、out owns runtime allocation |
| `XmlFree` | `align_rt_xml_free` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut XmlReader)`、null-safe |

全 Rust definition は C calling convention で、その境界を unwind してはならない。generated LLVM
declaration は existing shape の empty curated function/return/memory/parameter attribute set を不変に保つ。
A124 は next unreserved shape。implementation activation が eight
key/symbol/export/fingerprint rows を atomic に追加し、design だけでは active count を変えない。

`Ty::XmlReader=72`、`Scalar::XmlReader=48` は append-only canonical type-record-v3 leaf。exact byte は
`[3, 0, 0, 0, 0, 72]` と `Ty::Option(Scalar::XmlReader)` の
`[3, 0, 0, 0, 0, 4, 48]`。73/49 は later accepted capability まで unknown。`xml.event` は existing
nominal enum grammar を使い type-codec discriminator/interface format version 8 を変えない。unknown/
truncated/missing-payload/trailing record は cache publication 前 reject。

## Implementation closure matrix

implementation は one preflight review 前に author-side で全 cell を閉じる。1 parameterized owner で
multiple cell を閉じてよく、existing mutation-discriminating owner があれば new fixture は不要。

| axis | required implementation closure | exact regression owner |
|---|---|---|
| Type formation/import | `std.xml`、unique event、reader、exact method set、qualified-only spelling、consuming parse、mutable next、borrowed getter/effect を登録。unimported/bare/wrong receiver/arity/type/mode/collision/comparison/print/collection/parallel/native を HIR 前 reject。 | sema unit matrix、driver import/interface、builtin nominal tripwire。 |
| Parse construction/failure | selected string path を once move/null、typed access 前 raw auth、full validate、invalid 全 path で input free、success 後だけ shell publish、exact `-1` map。 | local/field/tagged/result/block/control matrix、invalid corpus、input/shell allocation/publication counter。 |
| Move/replacement/return/Drop | 全 Move/cleanup/resource predicate、control join、storage generation、interface mode、Drop map に XmlReader を sweep。Drop null-safe、malformed state は input access 前 abort、input→shell exactly once free。 | handle/Drop tripwire、move/use-after/replacement/return/control cleanup、malformed/allocation balance。 |
| Grammar/profile | W3C range/production と exact BOM/declaration/comment/CDATA/reference/DOCTYPE/PI/DTD/namespace profile。lenient library default 禁止、limits exact。 | independent W3C-style + S3/Azure corpus、forbidden opener、Unicode range edge、name/reference/comment/CDATA/tag/attribute products、255/256/257。 |
| Event/cursor | explicit/synthesized start/end、ordinary/CDATA text を exact order。declaration/comment/outside S skip。current state は next まで保持、EOF idempotent。 | nested/empty/comment/CDATA/whitespace/reference golden、repeated getter/EOF、cursor one-field mutation。 |
| View/value/region | name view に reader/current-state region、live 中 move/Drop/next 防止、clone escape。value/text は exact decode/normalize、once allocation、reader region なし。 | EscapeCheck call/control/carrier matrix、view pointer identity、owned allocation/Drop、entity/line Cartesian。 |
| HIR replay/validation | depth、clone/replay、effect、ownership、region、traversal、finalization、cache/semantic projection、checked-HIR、fail-closed switch に全 form。wildcard silent classification なし。 | variant tripwire、child/type/result/effect/mode/region/state mutation、replay/source-shape。 |
| MIR/runtime selection | typed op/result/event/region を保持、eight keyを追加、reachable row のみ select、wrong enum/type/status は LLVM 前 reject、whole/per-unit parity。 | MIR mutation、key inventory/bijection、unused import/no-selection、per-op selection。 |
| LLVM/native ABI | exact existing-shape declaration/status/out/enum/cleanup/destructor のみ。hand-written bypass/shape attribute mutation なし。 | IR golden、key/symbol reverse、base/export/collision、status/enum exhaustive、rt-LTO on/off。 |
| Interface/cache/generic | nominal event、72/48、mode/mutability/effect/view region/cleanup を encode。imported generic user ownershipは重複なし。used runtime implementation を object/link identity に含める。 | exact-byte/malformed codec、signature、generic whole/per-unit、two-build、surface/private edit-revert cache。 |
| Allocation/work | invalid parse は shell 0/input free 1。valid は shell 1/retained input/event/tree allocation 0。next/view/count 0。nonempty owned getter 1、empty attr 0。fixed scratch/bounded pass。 | counters、early-invalid read bound、depth/attribute RSS/stack、long traversal、repeat getter。timing benchmark なし。 |
| Diagnostics/docs | exact qualified spelling、bound/mutable receiver、consuming input、wrong state/index、unsupported profile。spec/design/history/roadmap/runtime/HIR ledger と JA mirror を同期。 | diagnostic、example syntax、doc/mirror/link/source-of-truth。 |

implementation は one capability boundary。reader type、full validator、cursor、view、Drop、HIR/MIR、eight
runtime row は strict producer-to-consumer chain で dormant subset に useful consumer はない。3 compiler
layer を越え、roughly 1,000 hand-written lines を超える可能性があるが、split は Move/region/ABI proof を
重複するか unusable handle を publish するため、この matrix が lower-risk boundary である。

## Deferred surface

`io.reader` streaming、incremental/fallible `next`、caller-buffer text decode、raw span/source location、
subtree skip、DOM/tree、XPath/CSS、namespace expansion/validation、DTD/schema validation、custom/external
entity、XInclude、PI delivery、comment event、canonicalization、mutation、writing/serialization は deferred。
追加には concrete consumer と new ledger が必要。S3/Azure Blob は accepted forward event stream だけを
必要とし、HTTP status/body bound と service-specific field semantics は package owner が持つ。
