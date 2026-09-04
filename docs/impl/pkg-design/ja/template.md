# pkg — template

> [English](../template.md) · **日本語**
>
> **注意:** 英語版 (`../template.md`) が正本。本書は同期ミラーである。
>
> **ステータス:** 提案中。design acceptance だけでは package source、compiler operation、runtime
> row、ABI shape を activate しない。

## 公開契約台帳

この台帳が first `pkg.template` capability の正本である。V1 は通常 write が既定で escape
される HTML text builder 一つであり、template parser、DOM、component、file loader、または
JavaScript/CSS/URL encoder ではない。

| 公開表面 | exact input・default・validation・evaluation | exact result・effect・error | ownership・lifetime・allocation・cleanup | compiler/runtime/package owner・artifact/cache identity | prerequisite・acceptance owner |
|---|---|---|---|---|---|
| qualified path segment `template` | `module`/`import`/type/value path で `template` token を `.` 直後の noninitial segment としてだけ認める。`pkg.template`、internal path、`pkg.template.html()` は parse。expression head の exact `template` + string token は shipped template expression のまま。bare declaration/reference `template`、`template.html()`、他 keyword segment/keyword-as-identifier は reject。 | compile-time only。runtime value/effect/allocation/error なし。formatter は canonical spelling を保持し qualified segment と template expression を曖昧性なく round-trip。 | ownership/lifetime effect なし。 | lexer token identity は不変。parser path-segment admission/formatter traversal が owner。module/import/interface/dependency/diagnostic は exact `pkg.template` bytes を保持。 | dotted module path/contextual template expression。parser/formatter positive-negative、span、round-trip、whole/per-unit resolution、no-other-keyword widening owner。 |
| `pub resource html_builder = pkg.template.internal.resource.drop_html_builder` | canonical `pkg.template.html()` だけが作る nominal arity-zero Move resource。non-Copy/non-comparable/non-printable で public raw/view conversion はない。shipped resource carrier grammar 全体、すなわち direct local/by-value parameter/return と recursively owning record/user sum/tuple/Option/Result を認め、`write`/`raw` は borrow-mut だけ。global/constant、fixed/dynamic collection element、owned value の box、capture/function/task/parallel、extern は既存規則で拒否。 | Pure owner。observable state は append 済み ordered byte prefix だけ。error/len/capacity/reset/clone/partial view なし。detectable malformed private state は dereference/mutation 前に abort。 | runtime builder shell と allocator-compatible grow buffer 一つを所有。move は complete source または selected aggregate path を null、recursive unfinished Drop は shell/payload を exactly once free、null Drop は no-op。call を越える borrow なし。 | nominal identity は `pkg.template`、construction/Drop は internal resource/descriptor owner。resource spelling/hook/wrapper/checked op/runtime key は whole/per-unit interface/dependency/object/link identity に入る。 | recursive resource move/borrow/Drop、canonical package、builder allocation。direct/record/sum/tuple/Option/Result、move/replacement/return/control/Drop、forbidden collection/capture/extern、malformed state、whole/per-unit owner。 |
| `pub fn html() -> html_builder` | 引数、overload、default、ambient arena/locale/document mode/allocator option なし。canonical wrapper を一回評価し exact internal descriptor だけを呼ぶ。 | Pure。empty live builder。OOM abort。impossible null runtime result は publication 前 abort。 | fixed shell 一つを allocate。初期 payload は canonical null/zero で payload allocation なし。 | checked `TemplateHtmlNew` HIR/MIR、runtime key `TemplateHtmlNew`、既存 pointer-return ABI shape。 | empty construction/Drop、alloc/free、null-result abort、direct/imported/return、whole/per-unit、cache、両 lowering。 |
| `pub fn write(borrow mut output: html_builder, value: str)` | `output`, `value` の順に exact once。`string` は通常規則で auto-borrow。全 byte を順に処理し、exact mapping は `& -> &amp;`, `< -> &lt;`, `> -> &gt;`, `\" -> &quot;`, `' -> &#39;`。他は NUL/CR/LF/non-ASCII UTF-8/existing entity を含め byte-identical copy。existing entity は再 escape。 | Pure exclusive mutation、Unit。HTML element text と、matching quote が既に開いた single/double quoted attribute の complete content に安全。surrounding markup、unquoted attribute、URL scheme、event handler、CSS/JS、comment、tag/attribute name、foreign-content grammar は保証しない。recoverable error なし。malformed owner/view/overflow は mutation 前 abort、OOM abort。 | call-only borrow、retain/temporary owned string/full-output copy なし。checked escaped length を mutation なしで計測し、必要なら一回 reserve 後 exact append。empty は no-op。 | checked `TemplateHtmlWrite` HIR/MIR。runtime key は `encoding.html_escape` owner の exact five-entity table を reuseし、第二 table を持たない。既存 `void @SYM(ptr, ptr, i64)`、empty attrs。 | 5 mapping/全順序/entity/empty/NUL/control/UTF-8、text/quoted-attribute parse、安全でない context control、overflow、allocation、state、whole/per-unit、codec differential。 |
| `pub fn raw(borrow mut output: html_builder, value: str)` | `output`, `value` を一回。string auto-borrow。entity recognition/escape/normalization/HTML parse/validation/trust marker なしで byte-exact append。 | Pure exclusive mutation、Unit。`html_builder` 唯一の public unescaped append。明示 trust boundary で malformed/unsafe HTML も出せる。malformed owner/view/overflow は mutation 前 abort、OOM abort。 | call-only borrow、retain/temporary allocation なし。checked length/reserve は first mutation 前。empty no-op。 | checked `TemplateHtmlRaw` HIR/MIR、runtime key `TemplateHtmlRaw`、既存 `void @SYM(ptr, ptr, i64)`、empty attrs。 | byte exact、mutation/overflow order、no-copy/no-retain、sole-bypass inventory、whole/per-unit、両 lowering。 |
| `pub fn to_string(output: html_builder) -> string` | output を一回 evaluate/consume。唯一の public finisher。`finish`/`build`/`as_str`/view/writer/implicit conversion なし。ordinary path-selected Move-call 規則で受ける initialized owner expression だけ。 | Pure。complete append bytes を owned valid-UTF-8 string で返す。detectable malformed state は publication 前 abort。 | allocator-compatible payload を allocation/copy なしで transfer し shell だけ free。cleanup 前に selected source path を null。empty は canonical null/zero owned string。resource Drop は no-op となり string Drop が payload を exactly once free。 | checked `TemplateHtmlToString` HIR/MIR、runtime key、既存 owned-string `{ptr,i64} @SYM(ptr)`。consume/result identity が interface/object/link に入る。 | empty/nonempty pointer identity、path-selected source null、string Drop、control/helper return、malformed state/double-use、whole/per-unit/cache。 |

## 境界と exact source surface

```text
html() -> live html_builder
  write(text)  -> five-entity escaped append
  raw(markup)  -> trusted byte-exact append
  to_string()  -> consume + owned string transfer
```

vendorable topology と declaration は exact に次である。

```align
module pkg.template

import pkg.template.internal.descriptor
import pkg.template.internal.resource

pub resource html_builder = pkg.template.internal.resource.drop_html_builder

pub fn html() -> html_builder = pkg.template.internal.descriptor.html()

pub fn write(borrow mut output: html_builder, value: str) =
  pkg.template.internal.descriptor.write(output, value)

pub fn raw(borrow mut output: html_builder, value: str) =
  pkg.template.internal.descriptor.raw(output, value)

pub fn to_string(output: html_builder) -> string =
  pkg.template.internal.descriptor.to_string(output)
```

`pkg.template.internal.descriptor` は source declaration を持たず、4 spelling は上記 exact
canonical wrapper だけから admit される compiler-private operation。application は internal module
を import できない。同名 application module/function/extern、変更 wrapper、追加 internal item、
noncanonical path は operation を選べない。`pkg.template.internal.resource` は nominal resource が
必要とする exact public raw Drop hook だけを持ち、package 外からは使えず one runtime free row へ
delegate する。

```align
module pkg.template.internal.resource

extern "C" {
  fn align_rt_template_html_free_v1(state: raw)
}

pub fn drop_html_builder(state: raw) {
  unsafe { align_rt_template_html_free_v1(state) }
}
```

underlying ordinary `builder` は公開しない。公開すると caller が `builder.write` で `raw` を名乗らず
escape を迂回できるためである。consume 前の borrowed view もない。

## 公開利用

```align
import pkg.template

fn page(name: str, count: i64) -> string {
  mut out := pkg.template.html()
  pkg.template.raw(out, "<p class=\"summary\">")
  pkg.template.write(out, name)
  pkg.template.raw(out, " has ")
  pkg.template.write(out, template "{count}")
  pkg.template.raw(out, " items</p>")
  return pkg.template.to_string(out)
}
```

markup trust は source に見え、dynamic text は default escaped path を使う。nested plain template
allocation は明示される。V1 は numeric overload や第二 formatter を加えず、scalar は shipped
`template`/`builder` で format して `write` へ渡す。固定 digit を trusted `raw` に渡すならその意図も
明示される。

## escape と context contract

mapping は shipped `encoding.html_escape` と exact 同一で同じ runtime helper を使う。entity decode/
canonicalization ではない。`&amp;` は `&amp;amp;`、`<script>` は `&lt;script&gt;` となり、両 quote を
escape するため single/double quoted attribute のどちらにも同じ result を使える。non-ASCII UTF-8
は byte-identical。

safety promise は trusted surrounding markup が一回の complete `write` result を element text または
既に開いた matching quote 内へ置く場合だけ。URL/event handler/style/script/comment/tag name/attribute
name や call 間で分割した grammar の semantic policy は保証しない。それらは dedicated encoder/
validation 後に `raw` が必要。five-entity escape を別言語 validator と偽らないため context tracker は
V1 にない。

## evaluation、state、failure order

public call は eager left-to-right。compiler は canonical module/wrapper と exact type を先に検証。
runtime operation は次の順である。

1. output pointer/alignment/version/live state/reserved bytes/builder pointer を builder dereference 前に
   validateし、malformed は abort。
2. write/raw は input slice formation 前に signed length、target-address representability、nullness を
   validate。zero は null 可、positive は不可。dangling nonnull は detectable ABI 外。
3. mutation 前に exact added/final length を checked compute。write の measurement scan は shared table
   を使い一回、raw は input length。
4. payload storage を reserve。allocation failure は recoverable partial result なしで hard abort。
5. order 通り append し new length を最後に commit。

`to_string` は complete state validation 後、wrapper を atomically spent にし、payload owner を外し、
shell を free、owned string を publish。Drop は nonnull state を cleanup 前に検証し payload ownership を
一回だけ外し、move/finish 後 null-safe。public fallible op がないため error sum/cleanup-error channel なし。

## compiler/runtime/ABI/cache closure

implementation は checked HIR/MIR operation 4 個、keyed runtime call 4 個、resource Drop call 1 個を
加える。全て既存 ABI shape を使い、新 shape は reserve せず A124 は next unused のまま。

| operation | runtime key / symbol | existing ABI shape |
|---|---|---|
| `TemplateHtmlNew` | `TemplateHtmlNew` / `align_rt_template_html_new_v1` | `ptr @SYM()` |
| `TemplateHtmlWrite` | `TemplateHtmlWrite` / `align_rt_template_html_write_v1` | `void @SYM(ptr, ptr, i64)` |
| `TemplateHtmlRaw` | `TemplateHtmlRaw` / `align_rt_template_html_raw_v1` | `void @SYM(ptr, ptr, i64)` |
| `TemplateHtmlToString` | `TemplateHtmlToString` / `align_rt_template_html_into_string_v1` | `{ ptr, i64 } @SYM(ptr)` |
| Drop hook | `TemplateHtmlFree` / `align_rt_template_html_free_v1` | `void @SYM(ptr)` |

supported 64-bit target の runtime shell は exact 32-byte、8-aligned private record。

```text
offset 0   u32 version = 1
offset 4   u8  lifecycle = 0 (live)
offset 5   u8  reserved = 0
offset 6   u16 reserved = 0
offset 8   ptr payload; capacity = 0 iff null
offset 16  u64 length
offset 24  u64 capacity
```

complete live product は version 1/lifecycle 0/reserved zero、
`0 <= length <= capacity <= isize::MAX`、payload null iff capacity zero。spent shell は publish せず、
consume finish は payload を外して shell を free、compiler は source slot を null。embedded grow buffer は
ordinary builder の allocation/five-entity helper を再利用する。layout/size assert と malformed-product
sweep が drift を fail closed。checked HIR は operation kind、resource
identity、input/result、effect、borrow/consume mode を検証し、MIR が再検証、LLVM preflight が key/
signature を照合する。allocation/mutation/abort/ownership transfer があるため curated LLVM attrs は
empty のまま。

root/internal source、hook、signature、operation kind、runtime key、shared table semantics は既存
interface/frontend/object/link cache identity に入る。input/output bytes は入らない。whole/per-unit は
同じ operation/runtime inventory。design acceptance だけでは active inventory を変えない。

## implementation closure matrix

| closure cell | required evidence | exact owner |
|---|---|---|
| contextual path/canonical formation | noninitial qualified `template` segment だけを parse、template expression は不変。exact topology/wrapper だけが4 opを形成し、bare/other-keyword/same-name/body/signature/extra-item/application-import twins は MIR 前 reject。 | parser/formatter + package admission |
| construction/move | new、direct/imported/by-value/return/reassignment/branch/match/loop が source null と final Drop 一回を保持。 | lifecycle owner |
| mutable borrow | bound initialized direct/path-selected aggregate owner の borrow-mut を認め、all-peer exclusivity と containing owner を保持。shared/moved/unbound temp/retention/collection access/capture/task/parallel/extern reject。 | resource-carrier sweep |
| escaped append | 全 entity/byte class/split/repeat/existing entity/empty が shared encoder と同一、measure/reserve は mutation 前。 | runtime differential + integration |
| raw append | exact bytes、唯一の reachable unescaped op、underlying builder 非公開。 | inventory + golden |
| finish/return | empty/nonempty pointer transfer no-copy、source null、shell once、payload string Drop once、全 return/control path。 | allocation/pointer lifecycle |
| early exit/replacement | fallthrough/return/`?`/`map_err`/`else`/branch/match/loop/replacement/malformed downstream/enclosing Drop。 | parameterized cleanup |
| malformed pipeline/native | wrong type/resource/effect/borrow/consume/key/signature/state/version/reserved/pointer/length/alignment は unsafe formation/mutation/allocation/publication 前 fail。 | HIR/MIR sweep + abort subprocess |
| allocation parity | shell、empty zero payload、escaped temporaryなし、no-growth/growth/overflow-before-mutation/OOM/zero-copy/free count。 | failpoint/resource owner |
| interface/ABI/cache | semantic-byte round trip、declaration/export、whole/per-unit、edit/revert、両 lowering、A124 unused。 | interface/ABI/cache owners |
| docs/examples | English、JA、roadmap/handoff/open-question、syntax-checked declaration/call example 一致。 | docs/package syntax owner |

implementation は one capability PR。producer op と唯一の consumer を分けると独立価値のない privileged
operation が休眠し、carrier/ABI/cleanup proof を重複させる。hand-written diff が約1000行を超える見込み
なら、この単一 lifecycle/shared-table boundary が integration risk を下げる理由をPRに記録する。

## deferred surface

`html "..."` syntax、contextual parser/interpolation AST、component/slot/layout、condition/loop/include DSL、
reflection/dynamic value、file/cache/hot reload、streaming writer、arena/borrowed result、capacity/reset/clone、
escape-disable flag、HTML parser/unescape/sanitizer/DOM、URL/CSS/JavaScript encoder、URL policy、CSP/nonce、
tag/attribute name construction、framework integration は全てV1外である。

plain language `template "..."` は唯一の scalar formatter のままで escape を得ない。
`encoding.html_escape` は allocate-and-return codec のまま。`pkg.template.write` は exact table の first
builder-sink consumer であり、第二 table や codec-result alias ではない。

## design-review record

この台帳を accept した independent review はまだない。public surface を変える finding は ledger を先に
更新し、その一決定を本文/mirrorへ一回で伝播する。
