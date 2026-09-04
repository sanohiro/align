# std.xml — 公開契約と実装設計

> 🌐 [English](../xml.md) · **日本語**

> **状態:** IMPLEMENTED 2026-09-05。

## 権威ある公開契約 ledger

この表を最初の `std.xml` capability の authority とする。後続 prose と implementation は cell
を明確化してよいが、拡張してはならない。V1 は memory 上の UTF-8 XML 1.0 document を読む、
bounded な forward-only reader 1 個である。DOM、validating parser、namespace processor、query
language、serializer、file/network loader、general entity processor ではない。

| 公開 surface | 正確な input、default、validation、evaluation | 正確な result、error、effect | ownership、lifetime、allocation、cleanup | compiler/runtime owner、artifact/cache identity | prerequisite と acceptance owner |
|---|---|---|---|---|---|
| `xml.event { Start, End, Text }` | closed source/discriminator order は exact に `Start = 0`、`End = 1`、`Text = 2`。comment、declaration、processing instruction、attribute、EOF、error、unknown event tag はない。 | Copy、Pure。source-formed value に error/abort path はない。`next` の `None` だけが EOF。 | settled one-field enum aggregate `{ i32 tag }`。borrow、allocation、Drop、numeric conversion、retained state はない。 | `align_sema` が `import std.xml` 配下の unique builtin nominal definition と qualified type/variant resolution を所有。HIR/MIR は ordinary enum aggregate を保持し、interface/cache identity は既存 nominal named-type grammar を使う。 | exact tag/order、construction/match、qualified import、malformed checked-HIR/MIR、interface round trip、cache mutation。 |
| opaque Move `xml.reader` | successful `xml.parse` だけが reader を作る。bound/initialized/single-owner forward cursor である。public constructor、field、clone、reset、seek、raw input accessor、conversion、default はない。 | in-memory value として Pure。mutable cursor のため concurrent/shared `next` は不可。Send、Sync、print、equality、collection storage、parallel capture、global、native source compatibility はない。 | consumed input allocation と runtime shell 1 個を所有。Move は両方を transfer、Drop は両方を exactly once free。既存 region/Drop rule が許す ordinary handle carrier と scalar tagged payload を通って move できる。 | new `Ty::XmlReader` / `Scalar::XmlReader`、checked HIR/MIR ownership、canonical type-codec leaf `72` / `48`、runtime key selection、Drop key。whole/per-unit identity は imported module、used operation、compiler/runtime implementation、target、ordinary build input を含む。locale/environment/filesystem/network/MIME/namespace registry は含まない。 | move-in/out/nulling、replacement、direct/Result/user-sum return、`if`/`match`/`else`/`?`/`map_err`/loop/early exit、malformed cleanup、whole/per-unit/generic、exact type codec、one-shell/input-free。 |
| `xml.parse(input: string) -> Result<xml.reader, Error>` | `input` を exactly once evaluate/consume。`string` により既に valid UTF-8。empty representation は exact canonical `{null,0}` で allocation を所有せず、positive length は exactly one allocator-compatible owned allocation を指す。下記 exact profile を parse する。leading U+FEFF は optional で 1 個。XML declaration は optional だが先頭だけ、version は exact `1.0`、encoding は optional case-insensitive `UTF-8` だけ、standalone は declaration order の `'yes'|'no'` だけ。root は exactly one。element depth と one-element attributes の maximum は inclusive 256、次は invalid。option/ambient encoding/MIME default はない。 | complete-document validation 後だけ `Ok(reader)`。empty input、malformed XML、forbidden markup、unsupported declaration/encoding、invalid XML character/name/reference、duplicate attribute、mismatched nesting、second root、いずれかの bound は `Error.Invalid`。OOM と compiler-produced impossible status は hard abort。unsafe caller は下記の mechanically detectable pointer-shape/representation/supplied-range alias failure で `AL_INVALID`。shape check を通過してから必要な provenance/lifetime/accessibility/exclusivity を欠く pointer は unsafe contract 違反で safe abort を保証しない。Pure。I/O、lookup、logging、callback、entity fetch、partial publication はない。 | source operation は input を once consume。mechanical preflight 後に runtime は canonical empty の allocation なし、または positive-length allocation を受け入れる。public XML failure はその責任を exactly once release し、canonical empty は no-op。success は unchanged positive-length allocation を retain し fixed-size shell 1 個だけ allocate。mechanically rejected unsafe call は output untouched で input ownership を受けない。validation は fixed 256-entry stack scratch を使い owned allocation 0。input copy、event tape、tree、namespace/entity table、per-element allocation はない。 | checked `XmlParse` HIR/MIR。runtime `align_rt_xml_parse` は既存 A08 `i32(ptr, i64, ptr)`。status は `0 = success`、`-1 = public Error.Invalid`、positive `AL_INVALID = mechanically detectable private ABI rejection`。他は不可。XML を使う時だけ capability/runtime fingerprint が select する。 | shipped owned string/Result/Move pattern、W3C/cloud corpus、declaration/grammar/character/name/reference Cartesian matrix、exact limits、first-error/no-publication、allocation/free、direct/imported/function-value、whole/per-unit、cache、ABI、optimized/unoptimized。 |
| `r.next() -> Option<xml.event>` | `r` を mutably borrow し exactly once advance。下記 event stream を emit。empty element は `Start` の後に synthesized `End` 1 個。EOF 後は繰返し `None`。 | compiler-produced valid live reader には total、Pure、allocation-free。`Error` は返さず、detectable null/moved/malformed private state は input access 前 hard abort。raw caller の pointer/access/exclusivity 違反は unsafe ABI contract 外。 | call 中だけ reader borrow。外部 value を retain せず shell の cursor/current-event field だけ mutate。returned Copy enum は Static。 | checked `XmlNext`。`align_rt_xml_next` は A03 `i32(ptr)`、`0=None`、`1=Start`、`2=End`、`3=Text`。他の i32 は abort。 | exact event order、empty pair、skipped markup、repeated EOF、mutable receiver、no allocation、malformed state、direct/imported/generic/whole/per-unit。 |
| `r.name() -> str` | current event が `Start`/`End` の時だけ valid。UTF-8 decode 後の exact lexical element `Name` を original case/colon bytes のまま返す。empty element の synthesized `End` は start-tag name。first `next` 前、`Text`、EOF 後は programmer error abort。 | Pure、admitted state で total、allocation-free。namespace expansion/prefix resolution/Unicode normalization/case folding/entity decode はない。 | zero-copy view は reader-owned input を指し、reader current cursor state に region-tied。reader より長生きせず mutable `next` をまたげない。`.clone()` が explicit escape。 | checked `XmlName`。`align_rt_xml_name` は A19 `i32(ptr, ptr)`。complete pointer-shape/shell-field/internal-range/output-alias preflight failure は output untouched。その後 `{ptr,i64}` を zero。wrong-state failure は canonical zero、success は view を install。nonzero status は codegen abort。 | Start/explicit-End/synthesized-End、Unicode/colon、current-state negative、region escape/clone、move/Drop、raw view、ABI output。 |
| `r.attribute_count() -> i64` | `Start` の時だけ valid。exact source-order count `0..=256`。namespace declaration も ordinary attribute。wrong state は abort。 | Pure、admitted state で total、allocation-free。 | call 中だけ reader borrow。result は Copy/Static。 | checked `XmlAttributeCount`。`align_rt_xml_attribute_count` は A29 `i64(ptr)`。negative または 256 超は abort。 | zero/exact-limit、Start/self-closing、wrong/malformed state、no-allocation、ABI。 |
| `r.attribute_name(index: i64) -> str` | `Start` の時だけ valid。`index` は zero-based source order で `0..<attribute_count`。exact lexical XML `Name` を返す。negative/out-of-range/wrong state は abort。 | Pure、admitted state で total、allocation-free。namespace expansion/prefix resolution/Unicode normalization/case folding はない。 | zero-copy view は reader input を指し、`name` と同じ current-cursor region。`.clone()` なしに `next`/reader Drop をまたげない。 | checked `XmlAttributeName`。`align_rt_xml_attribute_name` は A20 `i32(ptr, ptr, i64)`。complete pointer-shape/shell-field/internal-range/output-alias preflight failure は output untouched。その後 out を zero。wrong-state/index failure は canonical zero、success は view を install。nonzero status は codegen abort。 | source order、Unicode/colon/`xmlns`、bound edge、region escape/clone、wrong state、malformed ABI、whole/per-unit。 |
| `r.attribute_value(index: i64) -> string` | state/index rule は `attribute_name` と同じ。complete XML 1.0 normalized value を返す。predefined/numeric reference を decode。literal tab/LF/CR は document line-end normalization 後に space。character reference が生む文字は再 normalization しない。quote は delimiter。 | Pure。invalid state/index は abort。valid call は fresh owned string 1 個（empty は canonical zero-allocation）。OOM は abort。error/fallback/lazy decode/retained cache はない。繰返し call は allocation/copy も繰返す。 | decode 中だけ reader borrow。nonempty result は right-sized allocation 1 個を所有し cursor/reader より長生きできる。empty は `{null,0}`。reader/input は不変。 | checked `XmlAttributeValue`。`align_rt_xml_attribute_value` は A20 `i32(ptr, ptr, i64)`。complete pointer-shape/shell-field/internal-range/output-alias preflight failure は output untouched。その後 owned out を zero。wrong-state/index failure は zero、success は final allocation fill 後だけ publish。 | quote/entity/character/line-end product、empty/nonempty allocation、repeat、state/index negatives、failure no-publication、Drop、ABI。 |
| `r.text() -> string` | `Text` の時だけ valid。ordinary text run は隣接する XML `CharData` と `Reference` production の maximal nonempty sequence 1 個。single/consecutive reference は run を分割せず、document line-end normalization 後に decode。comment、CDATA、child start/end tag、enclosing end tag が run を終了。各 nonempty CDATA は separate `Text` で line-end normalization 後の literal content を返し entity-looking bytes も literal。wrong state は abort。 | Pure。fresh owned string 1 個。empty は `Text` として emit しない。OOM abort。繰返し call は allocation/copy を繰返す。 | call 中だけ reader borrow。result は right-sized allocation 1 個を所有し cursor/reader より長生きできる。reader/input は不変。 | checked `XmlText`。`align_rt_xml_text` は A19 `i32(ptr, ptr)`。complete pointer-shape/shell-field/internal-range/output-alias preflight failure は output untouched。その後 owned out を zero。wrong-state failure は zero、success は final allocation fill 後だけ publish。 | exact `CharData`/single/consecutive-reference coalescing、comment/CDATA/child boundary、whitespace/line end、non-ASCII UTF-8 acceptance/preservation、parse 時 NUL rejection、repeat allocation、wrong state、Drop、malformed ABI、whole/per-unit。 |

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
  mut doc := xml.parse(body)?
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
borrowed byte view なので、consumer は UTF-8 validation と copy を明記する。

```align
response := http.parse(data)?
body := response.body()
text := body.as_str()?
doc := xml.parse(text.clone())?
```

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

comment と XML declaration は validate して skip し public event を作らない。ordinary `Text` は隣接する
XML `CharData` と `Reference` production の maximal nonempty sequence 1 個。reference は event を分割せず、
`<a>x&amp;y&#33;</a>` は `Text("x&y!")`、`<a>&amp;&lt;</a>` は 1 個の `Text("&<")` を emit。
comment、CDATA、child start/end tag、enclosing end tag が ordinary run を終了する。各 nonempty CDATA は
separate `Text`。empty ordinary run/CDATA は event なし。root 内 whitespace は text として preserve、
root 外は skip。

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
read せず、validation 中 callback/user code なし。全 DTD/entity/element/attribute-list/notation declaration と
全 processing instruction を publication 前 reject することで、XXE、DTD retrieval、PI-based XInclude、
billion-laughs expansion を configurable switch でなく構造的に閉じる。別に定義した conforming optional
initial XML declaration 1 個は引き続き accept/skip する。

## Validation と error precedence

source checking は exact import/receiver/argument/result、consuming input mode、`next` mutable receiver、
current-view region を先に証明。raw ABI entry は全て `unsafe`。runtime は pointer を dereference せず、その
integer value/length から forbidden null、misalignment、negative length、noncanonical empty、address-range
overflow、supplied-range alias を mechanically reject できる。caller はその exact malformed shape を渡して
specified rejection に依存できる。applicable shape check を通過して access 対象となる全 nonnull pointer
には、exact range の provenance、lifetime、dereferenceability、read/write accessibility が必要。output range
は call に表現されない access から exclusive。positive parse input は exact allocator-compatible range を
所有し、zero length は null の時だけ valid で allocation を所有しない。shape check を通る reader pointer
は shell 用 live allocation 1 個を指し、getter/count は shared access、`next` と nonnull `free` は exclusive
access。operation が shell 内の stored input pointer を追う場合、この runtime が publish した shell を
unchanged で使うのでなければ、その live allocation と exact readable range も caller precondition。
post-shape-check caller precondition 違反に behavior guarantee はなく、aligned nonnull integer address だけ
では valid pointer の証明にならない。

その precondition 内で runtime は Rust reference/slice を作る前に raw address arithmetic と scalar load
だけで mechanically detectable range/alias check を行う。parse boundary の exact order:

1. output header の nonnull/alignment/representable fixed-size range を確認。input length を先に検査し、zero
   は null、positive は nonnull と representable address range を要求。output header と input range の
   disjoint を確認。address value だけを検査する。failure は `AL_INVALID`、output untouched、input ownership
   を受けない。特に `(nonnull,0)` は reject して free しない。
2. post-shape-check pointer precondition の成立後、output に null を store。canonical `{null,0}` の allocation
   なし、または positive-length allocator-owned input を受け入れる。
3. empty input は public `Error.Invalid`。それ以外は optional BOM/declaration、その後 document を
   left-to-right validate。grammar/character/name/reference/uniqueness/nesting/depth/attribute failure は全て
   同じ public Invalid、input exactly once free、out null。diagnostic/subcode がないので二つの XML-invalid
   byte の優先は public observable ではない。test は earliest-boundary work counter と beyond-boundary
   read absence を pin。
4. complete validation 後だけ shell allocate/init、input ownership を transfer、pointer publish。OOM abort。

out を持つ getter は最初に output/reader pointer shape と output/fixed-shell disjoint を address value だけで
確認する。post-shape-check pointer precondition の成立後、raw scalar load で stored input を追わず shell
magic/field を検査し、stored shell/input internal disjoint と output/input disjoint を確認する。この complete
mechanical preflight の failure は output untouched。その後だけ `{ptr,i64}` output を zero し、current
state/index を検査して input view を形成する。state/index failure は borrowed/owned output とも canonical
zero のまま。owned getter は exact decoded length を検査し、once allocate/fill、completed
allocation だけ publish。`next` は genuine live shell の exclusive access を要求し、input を追う前に shell
を検査。`free` は null を受け入れ、nonnull は exclusively held genuine shell でなければならない。
detectable malformed genuine shell は invalid stored input range を追う前 abort、valid Drop は input/shell を
exactly once free。runtime は arbitrary dangling address の authentication を主張しない。

structural shell-field validation は magic、valid event discriminator、representable/in-bounds
offset/count、consistent pending state を意味する。structurally valid だが selected getter が admit しない
event は output zero 後の later current-state check である。

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
| Parse construction/failure | selected owned string path を exactly once move し source を null。canonical `{null,0}` allocation なし、または positive allocator-owned range だけ accept。output null/misalignment/overflow、negative length、noncanonical `(nonnull,0)`、positive-null/overflow、output/input alias を address value だけで first rejectし、shape-passing pointer だけ provenance/access precondition 対象。preflight failure は output untouched、ownership なし。compiler-produced public invalid は accepted input responsibility を once release（empty は no-op）し output null。complete success 後だけ shell allocate/publish、exact `-1` map。 | local/field/tagged/result/block/`if`/`match`/`else`/`?`/`map_err`/loop/early-return matrix、invalid corpus、output null/misalignment/overflow × input negative/canonical-empty/noncanonical-empty/positive-null/positive-valid/overflow × alias owner、rejected `(nonnull,0)` no-free、input/shell allocation、untouched/zeroed output、publication counter。 |
| Move/replacement/return/Drop | 全 Move/cleanup/resource predicate、control join、storage generation、interface mode、Drop map に XmlReader を sweep。Drop null-safe、malformed state は input access 前 abort、input→shell exactly once free。 | handle/Drop tripwire、move/use-after/replacement/return/control cleanup、malformed/allocation balance。 |
| Grammar/profile | W3C range/production と exact BOM/declaration/comment/CDATA/reference/DOCTYPE/PI/DTD/namespace profile。lenient library default 禁止、limits exact。attribute uniqueness は hash candidate selection 後に exact byte confirmation。 | independent W3C-style + S3/Azure corpus、forbidden opener、Unicode range edge、name/reference/comment/CDATA/tag/attribute products、255/256/257。white-box unit は別々の valid name span に同じ stored candidate hash を与えて accept、同じ byte は reject。test-only injected hash は internal confirmation helper だけに届き runtime feature/export を追加しない。 |
| Event/cursor | explicit/synthesized start/end を exact order で emit。maximal adjacent `CharData` と single/consecutive `Reference` を ordinary text 1 個に coalesce。comment、CDATA、child、end-tag boundary で分割し、各 nonempty CDATA は separate。declaration/comment/outside S skip。current state は next まで保持、EOF idempotent。`XmlNext` は `Option<xml.event>` を返す one semantic MIR operation とし canonical enum id を持つ。LLVM lowering は exact `0=None`、`1=Start`、`2=End`、`3=Text`、other=abort mapping を one checked switch として所有する。generic compare/branch/`MakeEnum` node に identity を分割しない。 | exact `<a>x&amp;y&#33;</a>` と `<a>&amp;&lt;</a>` golden、nested/empty/comment/CDATA/child/whitespace document、repeated getter/EOF、cursor one-field mutation。MIR owner は result type と enum id を mutate。LLVM/codegen owner は4 valid status、3 event ordinal、invalid default-abort edge を区別し、valid event swap を不可能にする。 |
| View/value/region | name view に reader/current-state region、live 中 move/Drop/next 防止、clone escape。value/text は exact decode/normalize、once allocation、reader region なし。 | EscapeCheck call/control/carrier matrix、view pointer identity、owned allocation/Drop、entity/line Cartesian。 |
| HIR replay/validation | depth、clone/replay、effect、ownership、region、traversal、finalization、cache/semantic projection、checked-HIR、fail-closed switch に全 form。wildcard silent classification なし。 | variant tripwire、child/type/result/effect/mode/region/state mutation、replay/source-shape。 |
| MIR/runtime selection | typed op/result/event/region を保持、eight key を追加、reachable row のみ select、wrong operation/result/enum/provenance は LLVM 前 reject、whole/per-unit parity。seven callable XML operation は全て final source result を返す semantic MIR rvalue で、raw runtime status/native out slot を generic MIR control flow に出さない。resource selection/LLVM construction より前に one sealed producer preflight が exact MIR table から `ValidatedProducerGraph` を構築する。wildcard なしの exhaustive `Stmt`/`Rvalue` classification が intrinsic result、operand、slot/selected path、constructor、control input、callable、auxiliary result、runtime out-slot producer を検証し、新 MIR variant は classification 更新まで compile failure。XML preflight は independently trusted side-table type と access を結合せず、graph-issued inseparable `(exact selected type, presence, access class)` fact だけを消費する。function argument は `ByValue=Owned`、`Borrow=Shared`、`BorrowMut=Exclusive`、`Out=unreadable` で開始。fresh allocating/authenticated runtime out-slot producer は `Owned`、view は validated source access を保持。direct/indirect/imported call result は validated callable identity、return provenance、required dynamic-cleanup companion が transferred ownership を共同で証明するときだけ `Owned`。callable identity は `FnAddr`/`Closure`/`Use`/store/load/structural projection/control join を producer-derived に追跡する。indirect call は callee の exact `Ty::Fn(id)` を解決し、copied parameter mode/type、return、borrow、region、cleanup の全 field を `Program::fn_types[id]` と一致させ、raw/non-callable/mixed/missing/forged producer は ownership seed または LLVM call type 形成前に reject。admit する各 edge は access 伝播前に complete type relation を検証する：`Use` は operand/result equality、store/load は operand/result と resolved selected slot-path type の equality、field/tuple/variant/option/result construction/projection は canonical aggregate definition から selected child type を導出し、unrelated access fact を merge しなくても全 constructor arity/operand を検証、control selection は authenticated `bool` condition と equal alternative/result type を要求するが condition を provenance から除外、call は canonical parameter mode、全 argument、return type、selected return path、cleanup companion を検証する。cast、reinterpretation、`raw`、その他 unrelated producer は `string`/`xml.reader`/callable fact を確立できない。producer state は `Present(access)`、`MaybeAbsent(access)`、proven `Absent`、`Invalid`、unresolved を区別。他 option/result/enum alternative の constructor は `Absent`、`Absent` と one access の join はその access を持つ `MaybeAbsent`、all-absent selected path は valid のまま。whole-value call validation は absent self-owned leaf を許可し、present になり得る全 case を `Owned` 必須とする一方、mixed access、unknown tag、missing node、unresolved state は reject。provenance は selected aggregate path ごとに structural：field/tuple/variant/option/result projection は selected component だけを追跡し、`if`/`match`/loop join は selected value の alternative producer だけを merge、condition/unrelated sibling は merge しない。producer validity は selected dependency と validation-only dependency の両方に対する total fixed point。全 condition、fresh-producer input、call argument、unrelated constructor operand は valid typed producer に converge 必須だが、validation-only edge は access に寄与しない。seeded cycle は seed とともに converge、missing/duplicate definition、unseeded cycle、ill-typed node、unresolved check-only node は recursive descent/panic なしで `Invalid`。`SubSlice` と、`str`/`string`/`xml.reader`/callable/structural carrier を yield/feed し得る全 producer は exhaustive authority で explicit classify。`XmlParse` は present `Owned`、`XmlNext` は present `Owned`/`Exclusive`、immutable getter は present `Owned`/`Shared`/`Exclusive`、readable integer index は exact typed constant も許可。全 borrowed-place descriptor は noncanonical。wrong-mode/foreign-pointer argument は forged value/slot/wrapper/projection/call boundary 経由でも reject。semantic shape `XmlParse { input, error_enum, cleanup }` は primary `Result<xml.reader,error>` と distinct bool cleanup value を existing cleanup-returning call と同じ one atomic MIR definition にする。preflight は cleanup id の in-range、`bool`、primary id との distinct、他 definition からの absent、exactly-once definition を要求。lowering は Result に attach し、codegen だけが `0` を `(Ok(reader), true)`、`-1` を `(Error.Invalid, false)` に map。`XmlNext` は canonical event enum id と final `Option<xml.event>`、other op は exact final `str`/`string`/bounded `i64` を返す。 | compile-time variant sweep が exhaustive producer classification を所有。checked edge を feed し得る全 `Stmt`/`Rvalue` producer class、`ByValue`/`Borrow`/`BorrowMut`/`Out`、same-layout `raw`/other wrong type、fresh/runtime-out/direct/indirect/imported result、wrapper/projection/control path、typed/forged `Use`/store/load/field edge、join、constant、全 borrowed-place family を whole/per-unit preflight で交差。direct/indirect whole-value call owner は `None`/`Err`/non-owning enum variant を通し、present owned leaf は通すが present shared/mixed leaf は reject。callable owner は全 admitted fn-value producer で raw を laundering し、`Ty::Fn(id)` と copied mode/type/return/borrow/region/cleanup の各 field を whole/per-unit で独立 mutate。projection owner は local/cleanup-returning call result の両方で owned reader と unrelated shared tuple/enum sibling/shared-origin condition を区別し、producer-laundered または unresolved sibling/condition node は access を merge せず reject。`SubSlice`/trim/template/runtime-out-slot/call source は両 compilation mode で `StrClone` と `xml.parse` に到達。long acyclic chain、seeded cycle、unseeded selected cycle、unseeded check-only cycle、missing node、duplicate node が worklist termination/fail-closed behavior を所有。parse cleanup id の missing/duplicate/alias/out-of-range/wrong type、result type、cleanup attachment、error/event definition/id mutation は LLVM 前 reject。両 status-to-result/cleanup pairing と default abort は LLVM/codegen structural owner だけが証明。key inventory/bijection、unused import/no-selection、per-op selection は exact。 |
| LLVM/native ABI | exact existing-shape declaration/private out handling/aggregate/cleanup/destructor のみ。各 semantic XML rvalue が complete status contract を atomically lower：parse は `0` だけ cleanup true の `Ok(reader)`、`-1` だけ cleanup false の `Error.Invalid`、next は `0..=3` だけ fixed option/event table、name/attribute-name/value/text は zero only、attribute count は `0..=256` only、他は全 abort。generic MIR comparison/branch/enum/result wrapper/cleanup-bit producer は mapping に関与しない。address shape は dereference せず検査し、通過して access 対象となる pointer だけ provenance/access/exclusivity precondition を適用。getter の complete pointer-shape/shell-field/internal-range/output-alias preflight failure は untouched。preflight 後に zero し、later state/index failure は zero を保持。全 validation 前の typed input access なし。hand-written bypass/shape attribute mutation なし。 | exact declaration/call IR golden と per-operation codegen structural owner：parse は `0`/`-1`/default abort、exact Invalid ordinal、両 cleanup polarity、next は `0..=3`/全 event ordinal/default abort、zero-only getter は zero/default abort、count は両 boundary/default abort を区別。native owner は null/misalignment/noncanonical-empty/overflow/input-output-shell alias/malformed field、rejected-pointer no-dereference/no-free、sentinel untouched/canonical-zero、key/symbol reverse、base/export/collision、rt-LTO on/off を cover。 |
| Interface/cache/generic | nominal event、72/48、mode/mutability/effect/view region/cleanup を encode。imported generic user ownershipは重複なし。by-value parameter が return root になれるのは type 自体が borrow 可能な場合だけ。dynamic cleanup または non-cleanup ownership だけを持つ opaque/self-owned Move value は `borrow`/`borrow mut` 経由でのみ returned view の root になれる。direct/concrete-sum/generic carrier に再帰適用し、cleanup capability だけを external borrow provenance としない。used runtime implementation を object/link identity に含める。 | exact-byte/malformed codec、全 parameter mode を交差する direct/concrete/generic carrier summary、forged by-value XML root rejection と borrowed XML root acceptance、signature、generic whole/per-unit、two-build、surface/private edit-revert cache。 |
| Allocation/work | invalid positive-length parse は shell 0/input free 1。canonical empty invalid は input allocation/free 0。mechanical rejection は ownership/free 0。valid は shell 1/retained input/event/tree allocation 0。next/view/count 0。nonempty owned getter 1、empty attr 0。fixed scratch/bounded pass。 | canonical-empty/rejected noncanonical-empty/positive-invalid を含む counter、early-invalid read bound、depth/attribute RSS/stack、long traversal、repeat getter。timing benchmark なし。 |
| Diagnostics/docs | exact qualified spelling、bound/mutable receiver、consuming input、wrong state/index、unsupported profile。spec/design/history/roadmap/runtime/HIR ledger と JA mirror を同期。 | diagnostic、example syntax、doc/mirror/link/source-of-truth。 |

implementation は one capability boundary。reader type、full validator、cursor、view、Drop、HIR/MIR、eight
runtime row は strict producer-to-consumer chain で dormant subset に useful consumer はない。3 compiler
layer を越え、roughly 1,000 hand-written lines を超える可能性があるが、split は Move/region/ABI proof を
重複するか unusable handle を publish するため、この matrix が lower-risk boundary である。

### Design review finding-to-fix ledger

revised-diff review で first P1 correction が pointer shape と pointer safety をまだ混同し、canonical-empty
ownership を欠くと判明し、implementation re-review では3個の authenticated fact が permissive generic
machinery に分割されていた。post-redesign implementation review はさらに access と type が別々に
authenticate され、aggregate provenance が structural でないと指摘した。次の full implementation
review では presence、total producer validity、callable identity、exhaustive producer inventory も欠けると
判明した。このため matrix を **raw ABI representation phase**、**interface provenance**、**typed MIR
producer relation**、**structural presence**、**callable identity**、**total validity**、
**runtime-status/result identity** axis で再オープンする。shape-only rejection、post-shape caller
precondition、ownership acceptance は別 phase。cleanup capability は borrow provenance ではなく、XML MIR
は selected structural path ごとに one typed producer fact を認証する。各 semantic XML rvalue は one final
source result、LLVM lowering は complete checked status map を所有する。

| finding | class-wide resolution | closure evidence |
|---|---|---|
| P1: raw ABI pointer provenance、dereferenceability、exclusivity、alias ordering、failure-output ownership が不完全で内部不整合だった。 | public ledger、validation order、ABI ledger、closure matrix は exact-compatible unsafe caller precondition と mechanically detectable rejection を分離。parse/getter は mutation/typed access 前に raw range/alias preflight。preflight failure は output untouched、その後の getter failure は全て canonical zero。parse ownership は preflight 後だけ transfer。 | Parse/LLVM matrix row が full alias product、malformed shell field、untouched/zero output、allocation/publication counter を要求。English mirror と runtime ABI ledger も同じ rule。 |
| P2: XML `Reference` production をまたぐ ordinary `Text` segmentation が未指定だった。 | ordinary event は maximal adjacent `CharData`/`Reference` sequence。single/consecutive reference は coalesce、comment、CDATA、child tag、enclosing end tag は split。 | exact mixed/consecutive-reference golden を event/cursor matrix に明記し English mirror と同期。 |
| Revised P1: zero-length/non-null input が ownership acceptance に到達し、null/alignment は caller precondition と mechanical rejection の両方だった。 | empty input は allocation なしの exact `{null,0}`。address-shape check は dereference なしで exact malformed shape を rejectし、provenance/access precondition は通過後だけ開始。`(nonnull,0)` は output mutation/ownership 前に rejectし free しない。 | reopened Parse/LLVM row が address-shape Cartesian product、rejected-pointer no-dereference/no-free、canonical-empty public-invalid release を列挙。runtime ABI と English mirror も同じ phase split。 |
| P2: attribute-name hash collision 後の byte confirmation に discriminating owner がなかった。 | collision rule は hash candidate selection 後の byte equality。test-only stored-hash injection は internal confirmation helper だけに届く。 | white-box owner が distinct valid name の hash を強制一致して acceptance、equal byte で duplicate rejection を要求。 |
| P3: text owner が non-ASCII と NUL rejection を誤って一括した。 | owner は non-ASCII UTF-8 acceptance/preservation と NUL rejection を独立に要求。 | public ledger と English mirror が両 polarity を明記。 |
| P3: security summary が optional XML declaration も含め全 declaration reject と書いた。 | rejection は DTD/entity/element/attribute-list/notation declaration に限定し、conforming initial XML declaration 1 個は accept/skip。 | English/Japanese security prose と roadmap を qualified set に同期。 |
| P2: getter row と detailed order が malformed shell/internal-range failure の output を untouched/zero で食い違えた。 | complete mechanical preflight failure（pointer shape、shell field、internal range、output alias）は全て output untouched。getter は preflight 後だけ zero し、later state/index failure は zero を保持。aliased output が rejection 前に shell/input を壊さない。 | 全 public getter row、detailed validation order、runtime ABI ledger、English mirror、sentinel-output owner が同じ2 postcondition。 |
| Implementation re-review P2: interface root validation が dynamic cleanup を external borrow provenance と扱い、by-value `xml.reader` parameter の forged root を許可した。 | parameter mode と type capability を一緒に検査。by-value root は intrinsically borrow-bearing type を要求し、opaque/self-owned Move leaf と concrete/generic carrier は cleanup shape に関係なく `borrow`/`borrow mut` を要求。 | direct/nested carrier summary を ByValue/Borrow/BorrowMut と交差し、malformed by-value XML root は reject、compiler-produced borrowed-root summary は round trip。 |
| Implementation re-review P2: XML MIR operand check が forged borrowed-place descriptor 内の declared type を信用した。 | XML preflight は全 borrowed-place family を rejectするが、shape filter 自体は authority ではない。producer-derived lattice が argument mode を canonical store/load/move/join 経由で追跡し、parse は owned consumable input、next は owned/exclusive access、getter は shared access を許可、`Out`/mixed/unknown origin は reject。 | 全 operation で全 argument mode と raw/load/store/join form を whole/per-unit validation で交差。forbidden origin 由来の same-typed value は raw forged argument と同様に reject。 |
| Implementation re-review P2: `XmlNext` status identity が generic comparison/branch/enum construction に分割され、valid-typed Start/End swap が可能だった。 | seven callable XML operation を全て final source result を返す semantic MIR rvalue に変更。parse、next、全 getter、count の raw status/native out slot は LLVM lowering 内に閉じ、generic MIR node が status/result/abort graph を rewrite できない。 | MIR mutation は operand/provenance/result type/error-event enum id を所有。exact LLVM/codegen structural owner は全 documented valid status/result ordinal と invalid default-abort edge を区別し、whole/per-unit output identical。 |
| Redesign review P1: typed `Arg` を許可しても parameter mode を検証せず、forbidden borrowed origin を same-typed `Value` 経由で laundering できた。 | canonical XML access は producer-derived。initial argument mode が ownership/access を定め、admitted producer が伝播し、unknown/mixed join は reject。operation-specific requirement が consuming parse、mutating next、immutable getter を区別。 | provenance mutation matrix は全 operation/compilation mode について `ByValue`/`Borrow`/`BorrowMut`/`Out` を store/load/join 前後で交差。 |
| Redesign review P2: `XmlNext` だけの atomization では parse と getter/count の status graph が generic MIR から変更可能だった。 | semantic boundary を全 callable XML rvalue に拡張し、final source result だけを返す。LLVM lowering が各 exact runtime-status table と default abort を privately ownership。 | per-operation MIR graph assertion は generic status consumer を禁止し、exact emitted-IR owner が parse/next/zero-only getter/bounded count を cover。 |
| Redesign review P2: MIR に status constant/abort edge がなくなるのに MIR owner がそれらを mutate するとしていた。 | evidence を owning layer で分割：MIR validation は operand/provenance/result type/enum id、LLVM/codegen inspection は runtime constant/ordinal/invalid edge を所有。 | codegen owner は全 XML status contract の全 valid case と default abort を独立に区別。 |
| Focused redesign re-review P1: final parse Result と dynamic cleanup bit が one authenticated semantic result ではなかった。 | `XmlParse { input, error_enum, cleanup }` が primary Result と distinct bool cleanup id を atomically define。preflight は range/type/uniqueness/single definition/attachment を証明し、codegen だけが `0`→`(Ok(reader), true)`、`-1`→`(Error.Invalid, false)`、other→abort を map。 | whole/per-unit MIR owner は cleanup-id range/bool type/distinctness/single definition/Result attachment だけを mutateし、exact emitted IR だけが両 status pair/default edge を証明。 |
| Focused redesign re-review P2: fresh/callable result に provenance seed がなく、admitted function-result path と矛盾した。 | fresh allocation は owned。direct/indirect/imported result は validated signature/return provenance/cleanup-transfer fact から access を導出し、wrapper/projection/control producer は cleanup capability を borrow provenance とせずその class を伝播。 | direct/imported/function-value と wrapper/control case を全 access class と交差し、unknown/mixed root は reject。 |
| Post-redesign implementation review P1: access propagation が forged value/slot type table を信用し、`raw` を `Use` または store/load 経由で `xml.reader` にできた。 | access を standalone fact にしない。admit する各 graph edge は exact source/result type と access を一緒に導出・検証してから伝播し、mismatch は LLVM/native call 前の preflight で terminate。 | whole/per-unit mutation は same-layout `raw` と wrong aggregate component を全 admitted value/store/load/field/wrapper/projection/call edge 経由で launder し、`CodegenError` を要求。 |
| Post-redesign implementation review P2: whole-aggregate taint が unrelated sibling と `Select` condition を merge し、valid reader projection を reject した。 | provenance は selected structural path ごとに記録。tuple/enum/option/result/field projection は selected component だけを追跡し、control condition は type-check するが result provenance に加えず、同じ selected value の alternative producer だけを join。 | positive tuple/multi-payload-sum carrier は owned reader と unrelated shared sibling を組み合わせ、shared-origin condition が owned reader を select。selected reader は owned のまま、owned/shared alternative join は mixed reject。 |
| Typed-provenance re-review P1: whole-value call validation が inactive Option/Result/enum owned leaf を unresolved と扱い、valid non-owning alternative を reject した。 | presence を validity/access から分離。proven inactive path は valid `Absent`、present/absent join は `MaybeAbsent(access)` obligation を保持し、present になり得る case だけ call mode を満たす必要がある。 | direct/indirect と whole/per-unit call owner は `None`/`Err`/non-owning enum variant を通し、present shared/mixed self-owned leaf は reject。 |
| Typed-provenance re-review P1: indirect call が copied signature field と arbitrary declared `Ty::Fn` を信用し、forged callee/ABI fact が ownership seed と wrong LLVM call type を作れた。 | validated producer graph が全 fn-value producer 経由で exact callable identity を導出。producer が one canonical `Ty::Fn(id)` に解決し、copied field が全て `Program::fn_types[id]` と一致する場合だけ indirect call を admit。 | whole/per-unit mutation は raw/non-callable callee producer、fn-value store/projection/join、type id、各 parameter mode/type、return、borrow、region、cleanup field を cover。 |
| Typed-provenance re-review P1: validation-only dependency が explicit invalidity だけを伝播し、unresolved node を伝えないため unseeded condition/input cycle と laundered unrelated sibling が通った。 | one total validity fixed point が selected/validation-only edge を共に cover。convergence 後の unresolved は invalid。condition、fresh input、call argument、全 constructor operand は valid typed producer 必須だが、その access は selected-path merge から除外。 | unseeded check-only cycle と raw-laundered sibling/condition owner は rejectし、対応する valid shared sibling/condition は accept。 |
| Typed-provenance re-review P1: producer coverage が `SubSlice` を欠き、valid substring clone の `xml.parse` input を reject した。 | one exhaustive no-wildcard producer authority が全 MIR variant を classify し、exact result/dependency relation または explicit irrelevant class を供給。new variant は classify まで compile failure。 | compile-time sweep と whole/per-unit substring/trim/template/runtime-out-slot/call-result XML input が coverage と valid-source acceptance を close。 |
| Typed-provenance re-review P2: documented HTTP integration が `slice<u8>` を直接 clone したが、その copy は `str`/`string` だけで定義される。 | HTTP body はまず `as_str()?` で explicit UTF-8 validation、その後 validated `str` を clone して owned parse input にする。 | English/Japanese example を一致させ、syntax owner が complete `http.parse` → `body` → `as_str()?` → `clone` → `xml.parse` sequence を検査。 |

## Deferred surface

`io.reader` streaming、incremental/fallible `next`、caller-buffer text decode、raw span/source location、
subtree skip、DOM/tree、XPath/CSS、namespace expansion/validation、DTD/schema validation、custom/external
entity、XInclude、PI delivery、comment event、canonicalization、mutation、writing/serialization は deferred。
追加には concrete consumer と new ledger が必要。S3/Azure Blob は accepted forward event stream だけを
必要とし、HTTP status/body bound と service-specific field semantics は package owner が持つ。
