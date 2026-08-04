このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同等の粒度（シグネチャ、Move/effect の分類、エラー方針、落とし穴（Pitfalls）、テストアンカー）で記述された公式な設計ドキュメントを収めている。
執筆はメインループ（Fable）が担当している。

# core — json

> 🌐 [English](../json.md) · **日本語**

## Overview

JSON を型付き境界とスキーマ未知境界の両方で扱う（draft §14）。表面は、型付きの `encode` / `decode`、遅延スキーマ未知ビューの `doc`、型付き行をストリーム処理する `scan` の4操作である。対象型と行型は **明示的な型引数ではなく型推論によって** 決定される（決定済: Align には式位置での型引数構文、つまり turbofish のような記法は存在しない）。使用には `import core.json` が必要である（capability-header のルールは `core.json` に対しても std モジュールと全く同様に適用される）。

## Signatures (verified)

```text
json.encode(x)   -> str                      // x: struct (nested structs recurse); str fields JSON-escaped
json.decode(s)   -> Result<T, Error>         // T from the binding/context: u: User := json.decode(s)?

// decode targets, all verified:
//   i64 / f64 / bool       (a BARE scalar — parses the whole input as one JSON number/bool; Copy → Static/returnable; T1b)
//   struct                 (flat OR with nested-struct / Option<T> / array<Struct> / array<scalar> fields; field order free; unknown keys ignored)
//   array<i64> / array<f64>
//   array<Struct>          (AoS; str fields = zero-copy views into the input; nested-struct + Option fields recurse)
//   soa<Struct>            (direct columnar decode — no AoS intermediate, no transpose;
//                           inside arena {}; str columns borrow the input text; primitive/str columns only,
//                           NO nested columns — the owned-columns deferral stands)
//   enum (union)           (shape-directed: a JSON oneOf → a sum type; the variant is selected by the
//                           value's shape class — str/number/bool/object/array; O(1) first-byte dispatch;
//                           str payloads borrow the input; an owned array<Struct> variant is J2b)
```

**Union（直和型）ターゲット（JSON completeness J1b）。** JSON `oneOf` は、値の **shape class** —
`Str`（`"`）/ `Number`（数字・`-`）/ `Bool`（`t`/`f`）/ `Object`（`{`）/ `Array`（`[`）— で判別される
直和型にマップされる（先頭構造バイトでの O(1) ディスパッチ）。**コンパイル時検査（Align らしい設計）:**
union-decodable な enum は各バリアントがちょうど 1 つの payload を持ち、各 payload が 1 つの shape class
にマップされ、すべての class が **相互に排他** — `i64 | f64`（両方 Number）・2 つの object payload・2 つの
array payload は clash としてコンパイルエラー、tag-only や shape を持たない（`char`）payload も拒否。`null`
は class ではない（不在は `Option` の担当）。実行時に該当バリアントのない shape（array バリアントを持たない
union への配列、や `null`）は decode `Err`。encode は生きているバリアントの payload を **そのまま
（ラッパーキー無し）** 出力するので、`decode(encode(x))` は構成上ラウンドトリップする。ランタイム:
`JsonUnion` descriptor（バリアントごとに 1 つの `JsonField` payload arm ＋ shape-class→arm テーブル ＋
arm→enum-tag テーブル）。decode は先頭バイトを分類し、共有 `write_value` で payload を書き、tag を設定。
encode は tag を読んで共有 `json_encode_value` で該当 arm を出力。**所有 `array<Struct>` payload（J2b,
SHIPPED — OpenAI マルチモーダル `content: str | array<Part>` union）:** `[` は Array-class アーム
（descriptor kind 5、要素構造体のサブスキーマ）にディスパッチし、enum の tag-switched `Drop` が解放する
所有 AoS に decode、encode は bare な JSON 配列として出力。完全な `Content { Text(str), Parts(array<Part>) }`
がラウンドトリップする。要素構造体は非所有でなければならない（Slice-C ルール。`array<string>` /
`array<Move-struct>` は延期）。`array<scalar>` union payload はまだ descriptor アームがない（J3）。
トップレベル union の `json.encode` はローカル束縛が必要（struct encode と同様）。**構造体フィールドとしての union（J1b-2b / J3a,
SHIPPED）:** 構造体フィールドは union であってよい（`Message { content: Content }`）— descriptor
**kind 6**（`sub` は `JsonUnion`、decode/encode で共有）。`field_width`/`write_value`（全 decode パス
= slow + Mison speculative + fallback）と `json_encode_value` に kind-6 アームが加わり、union
フィールドは nested struct・`Option` フィールド（trailing-comma layout）・`array<Struct>` フィールドと
合成される。**J3a** はこれを **Move** union フィールドへ拡張する — 完全なマルチモーダル
`content: str | array<Part>`（`Content { Text(str), Parts(array<Part>) }`）が `Message` に合成され、
両シェイプを decode/encode して byte-identical にラウンドトリップする。Move-enum フィールドは外側 struct を
**Move** にする: canonical な再帰的 `DropPlan` が enum payload を認識し、
`struct_is_move`/`enum_is_move` はそこから一貫して導出される。`drop_struct_fields` の `Ty::Enum` アームが
tag-switched な `drop_enum` で生きているバリアントを解放する。ランタイム `drop_decoded_owned` には
**kind-6** アーム（`→ drop_decoded_union`）が加わり、decode エラーパスで union の所有 payload を解放する。
`match m.content { … }` は所有 payload をムーブアウトしフィールドをゼロ化する（`NullStructField` が型対応
= `{tag,payloads}` 集約全体をゼロ化）ので、struct の `Drop` はそこで null を解放する（単一解放）。
union のバリアントは構造化 MIR の型テーブルに含まれるので、バリアント変更で decode/encode キャッシュが
無効化される。**境界:** Move struct は関数境界を越えて `Result`/`Option` の Ok
payload になれない（Slice-C 制約）ため `Message` の decode ターゲットは `?` で束縛する。top-level
Move union の decode も `?` で直接消費しなければならない。raw
`Result<MoveUnion, Error>` の保持・返却は、再帰的 tagged-payload Drop が未実装のため L1b 診断となる。続く J3b
スライスが所有要素の deep free を提供するため、`Message` が Move の場合も
`Chat { messages: array<Message> }` までラウンドトリップする。

**`array<Struct>` フィールド（REST-gateway runway, Slice C）。** 構造体フィールドは所有の `array<Struct>`
であってよい — `messages: array<Message>` / `choices: array<Choice>` shape。フル OpenAI リクエスト/
レスポンスがラウンドトリップする。decode: descriptor kind 5（`sub` = 要素スキーマ）が
`decode_struct_array_value` を駆動し、JSON サブ配列を所有 AoS にパース（要素ごとに `parse_object`、
nested/`Option` 要素フィールドも再帰）して `{ptr,len}` をフィールドに書く。バッファは構造体の `Drop` で解放。
encode: `StructArrayField` ピースが runtime の descriptor 駆動エンコーダ（`json_encode_struct_array` →
`json_encode_object`、**decode descriptor を再利用** — 対称的で nested/Option/str/scalar を扱う）を呼ぶ。
**memory-safety:** array フィールド確保後に decode が `Err` になった場合、`drop_decoded_owned` が部分構造体の
AoS バッファを解放（codegen `drop_struct_fields` の runtime 双対）。**`array<Move-struct>` 要素（J3b,
SHIPPED）:** 要素自体が **Move** であってよい — `Chat { messages: array<Message> }` shape で各 `Message` が
Move-enum の `content` フィールドを所有する。drop は **deep** free: 共有の codegen `deep_free_struct_array`
ヘルパが `len` 要素をループして各要素を再帰的に `drop_struct_fields`（その `string`/所有 array/Move-enum
フィールドを解放）し、その後 AoS を解放する — 構造体フィールドの drop からも、スタンドアロンな
`array<Struct>` ローカルの `Stmt::Drop` からも呼ばれる。runtime のエラーパスも同様: `drop_decoded_owned`
の kind-5 アームが各要素を deep-free（`sub_owns_buffers` で判定）し、`decode_struct_array_value` は
mid-array パース失敗時に `buf[0..count]` の既 materialize 要素を解放する。**J3b で OpenAI chat ゲートウェイが
エンドツーエンドで閉じる**（`Chat` が byte-identical にラウンドトリップ）。**引き続き拒否:** `array<string>`
（bare-`string` 要素の array フィールド — 要素ごとの string free は別スライス、0b-2 で捕捉）。**制約:** Move
構造体（array/Move-enum を所有）は関数境界を越える `Result`/`Option` Ok payload になれない — スコープ内で
decode + 使用する。bare `array<Move-struct>` の `json.encode` とそのフィールド上の pipeline は制限される
（decode→encode パススルーは動作）。

**`array<scalar>` フィールド（JSON 完全対応 T1b + `array<str>`, align-llm Request 3）。** 構造体フィールドは
所有の `array<i64>` / `array<f64>` / `array<bool>`（align-LLM のデータシェイプ — embeddings、token ids）
**または `array<str>`**（argv リスト、`stop`/`tags`、tool 名リスト）であってよい。JSON descriptor **kind 7**:
フィールド自身の `{ptr,len}` スロットは幅 16（下位バイト）、要素スカラーの kind（0=int / 1=bool / 2=float /
**3=str**、bits 20-23）・width（bits **24-28** — 5 ビット。`str` 要素の `{ptr,len}` は幅 16 で元の 4 ビットに
収まらないため）・sign（bit 16）をタグ上位ビットに詰めるので、1 つのタグが両方を運ぶ。decode:
`decode_scalar_array_value` が共有の per-scalar `write_value` で JSON 配列を所有バッファにパースするので、scalar
*フィールド*と同じ範囲/符号/float 幅チェックが要素ごとに適用される。**`str` 要素（kind 3, 幅 16）は入力への
zero-copy な `{ptr,len}` ビューとして書き込まれる**（top-level `str` フィールドと同じルール）ので、所有スパインの
各エントリは入力を借用する — decode された構造体全体は `region_of(JsonDecode) = region_of(input)` により既に
入力 region に束縛されているため、新たな仕組みは不要。`.clone()` は入力を越えてコピーする。JSON エスケープを含む
要素（`\`）は escaped `str` フィールドと同じ既存の zero-copy 制限（`Err` に decode）。encode: `ScalarArrayField`
テンプレートピース → `json_encode_scalar_array` がバッファをループして `[e0,e1,…]` を出力（`str` 要素は quote +
`write_json_str_body` エスケープでレンダリング）。drop: 所有スパインを flat free（scalar / `str`-view 要素は何も
所有しない — ビューは誰も free せず入力を借用する）— 成功時は `drop_struct_fields` の `DynArray` アーム、decode
エラーパスは `drop_decoded_owned` kind-7（要素非依存の flat-free）。`sub_owns_buffers` に kind 7 があるので
`array<Move-struct>` 要素内の scalar/str-array フィールドも cleanup される。J3b と合成する
（`Table { rows: array<Row>, meta: array<i64> }`、`Row { vals: array<f64> }`）。構造化 MIR の指紋には
要素型も含まれるため、`array<i64>`→`array<f64>` の変更でキャッシュが無効化される。**なお延期:**
`array<char>`（JSON 形式なし）と、**top-level** の `array<str> := json.decode`
（構造体 FIELD は囲む構造体の入力 region 束縛に乗るが、top-level 配列の結果はその region を自身で運ぶ必要がある —
scalar の top-level 配列は意図的に `Static`/返却可能なので、top-level の `array<str>` は別途 region を運ぶ slice に
なる）。v1 制限: 所有 scalar-array フィールド上の `.sum()`/pipeline と bare `array<scalar>` の `json.encode` は
制限（decode + `.len()` + フィールドとしての encode は動作）。

**`Option<T>` フィールド（REST-gateway runway, Slice B）。** 構造体フィールドは `Option<T>`（payload は
scalar / `str` / ネスト構造体）であってよい。**null ポリシー:** decode はキー欠落→`None`、JSON `null`→
`None`、型不一致→`Err`、必須（非 `Option`）フィールドは欠落で `Err`。**encode は `None` フィールドを
完全省略**（`"k":null` にしない）ので `decode(encode(x))` はラウンドトリップする。ランタイム: `JsonField`
に `opt_tag`（`-1`=必須、それ以外は `Option` の tag バイトオフセット）を追加。optional フィールドは
`all_required_seen` の対象外で、共有の `write_value` が payload スロットに書いてから `Some` tag を立てる。
encode は `Option` を含むオブジェクトを trailing-comma 方式に切替え、`}` の前で `align_rt_builder_pop_comma`
を 1 回呼ぶ（必須のみのオブジェクトは静的レイアウトを維持）。**JSON の ownership 境界:** L1a 以降、
通常の言語構造体では所有する `Option<T>` field も許可されるが、各 JSON 経路はより狭い descriptor 契約を
持ち得る。現在の compiler の Decode schema は `Option<Move-struct>` shape を受理し、通常の decode、encode、
scope Drop もこの shape を維持する。既知の partial-error cleanup 欠陥（`Some` payload を decode した後、後続の
required sibling が失敗する場合）は別の ownership request が所有する。`Option<string>` は現在の JSON Decode
schema の範囲外である。以下の scanner 規則はこれらの通常 JSON の詳細によって弱められない。到達可能な型
グラフが `Drop` を必要とする row は `json.scan` が拒否する。**`Option<struct>` encode（T1b, SHIPPED）:**
`Some` は runtime の descriptor 駆動エンコーダ（新 `OptionStructField` テンプレートピース →
`align_rt_json_encode_object`、descriptor テーブルで単一 struct を出力）でネストオブジェクトを描画し、
`None` はフィールドを省略（同じ trailing-comma + `PopComma` 方式）。再帰的に合成する（ネスト plain struct と
ネストした `Option<str>` を持つ payload はその `None` も省略）。payload struct は encodable であることを
検証（`decode_struct_fields_ok`）し、現在受理される `Option<Move-struct>` shape も含む。scanner-only の
Copy 制約は通常の JSON decode、encode、scope Drop を狭めない。構造化 MIR の指紋には payload struct の定義も
含まれるため、`Option<struct>` payload のフィールド変更で decode/encode の両オブジェクトが無効化される。
JSON の MIR ノードは target id だけを持ち、手動で受け渡すスキーマ文字列は存在しない。

**ネストされた構造体フィールド（REST-gateway runway, Slice A）。** 構造体のフィールドはそれ自身が
`Struct` であってよい。`decode` はネストされたオブジェクトへ再帰し、`encode` はそれを再構築するため、
ネストされたレコードもラウンドトリップできる。ランタイム側ではフィールドディスクリプタが kind 4 と
`JsonSubTable` ポインタ（ネスト構造体自身のディスクリプタ + PHF + store size）を持ち、`parse_object` /
`write_field_indexed` が再帰する — したがってスローパスと Mison 投機パスの **両方** がネストを扱う
（ネストフィールドはレコードレベルのコロン 1 個で、その値をレコード分割器はより深いブラケット深度に
残す）。ネストされた `str` フィールドは入力へのゼロコピービューのままなので、値全体が再帰的に入力へ
region-tie される（`struct_has_str` が再帰する）。上で説明した後続の Option、array-field、union の
各スライスは、この再帰パスと合成済みである。

## Type & ownership classification

- `encode` は内部的に string builder を使用して文字列を構築する。戻り値は arena に region 付けされた `str` となる。
- `array<T>` / `array<Struct>` への `decode` は、所有権を持つ Move 配列を生成する（破棄時は deep-drop される）。
- `soa<T>` への `decode` は、外側の arena に列（カラム）を割り当てる（`align_rt_json_decode_soa` により、1 回のカウント用パスと 1 回の値パース用パスが `FieldDst` を介して Mison の投機的実行（speculation）パスを共有する）。
- デコードされた `str` フィールドや列は、**入力された `str` へのビュー（参照）** である。そのため、入力データはデコード結果よりも長生きしなければならず、これは region チェッカによって強制される。

## Effects

Pure（パース処理は純粋な計算であり、I/O は発生しない。バイトデータの入出力には `std.fs` や `std.io` を組み合わせる）。

## Errors & aborts

不正なデータはすべて `Err(Error)` として扱われ、パニックが発生したり、静かに誤った値が返されたりすることは決してない。これには構文エラー、フィールドの欠落、型の不一致、**範囲外の整数** が含まれる（符号を考慮するフィールドタグ、#295。`u64` フィールドは単一のディスパッチャを経由して `u64` の全範囲を受け入れる、#311）。宣言済みフィールドは正確に1回だけ現れなければならず、重複した宣言済みキーは strict path と speculative path の両方で `Err` になる。学習済みパターンが未照会位置とみなした場所に重複が現れた場合も同じであり、未宣言キーだけが読み飛ばされる。

## Regions

`region_of(decoded str view) = region_of(input)`、`region_of(soa columns) = enclosing arena`。所有権を持つ（owned な）配列は自由にエスケープできる。デコード済みのビューを、その入力データの寿命を超えてエスケープさせようとした場合は、エスケープの時点でコンパイルエラーとして捕捉される（保持し続けたい場合は `.clone()` でコピーを取り出す必要がある）。

## 完全対応の実装状況と残る境界

完全な設計は `open-questions.md` →「JSON completeness — DESIGN SETTLED」（実装履歴。spec 本文は
draft §14 + §18.1）。以下は出荷済みスライスと、現在も残る少数の境界をまとめた台帳である：

- **union（J1–J2）:** JSON の `oneOf` は sum type に写像し、**shape class**（Str/Number/Bool/
  Object/Array、pairwise 相異をコンパイル時強制、先頭バイト O(1) ディスパッチ）で判別。encode は
  生きている variant の payload を裸で書く。言語側の前提: enum の `str` payload（region 追跡）→
  所有 payload（`array<Struct>`、tag 分岐 drop）。**ここまで SHIPPED:** enum `str` payload + region
  追跡（J1a）、構造体フィールドとしての enum（J1b-1）、トップレベル union decode/encode（J1b-2a）、
  構造体フィールドとしての union（J1b-2b）、enum の所有 `array<Struct>` payload + tag 分岐 drop（J2a）、
  union の Array shape-class アーム（J2b）、**Move-enum 構造体フィールド**としてのマルチモーダル union
  （`Message { content: Content }`、J3a）、`array<Move-struct>` 構造体フィールド — 所有要素の deep
  free（J3b）で `Chat { messages: array<Message> }` を閉じる — いずれも上記で文書化。**OpenAI chat
  ゲートウェイはエンドツーエンドで閉じた。**
- **行列残り（J3/T1b）: 完了。** ~~top-level scalar/bool decode ターゲット~~（SHIPPED）、
  ~~`array<scalar>` フィールド~~（SHIPPED）、~~`Option<struct>` encode~~（SHIPPED）。`array<Option<T>>`
  は **延期** — composite 要素の所有配列は非再帰 `Scalar`/`PrimScalar` の型システムで表現不可（専用の
  composite-element 配列型が必要な言語型のギャップで、JSON matrix-fill ではない。価値も低い）。
  open-questions "T1b" 参照。
- **`json.doc`（J4）:** スキーマ未知の遅延ビュー — arena 常駐 tape。ナビゲーションは total かつ
  Missing 伝播（`get`/`at` は常に doc を返し、欠落は葉の `as_*` の `None` として一度だけ現れる）。
  キーがデータの object は順序付き `key(i)`+`at(i)` で吸収、`elems()` で 1 階層を materialize して
  pipeline に流す（map 型も serde 式 value 木も導入しない）。**Slice 1 SHIPPED:** `json.doc` 型 +
  `json.doc(s)?` パース（arena 常駐 tape、`Result<json.doc, Error>`）+ `kind()`（→ 組み込み
  `json.kind` 直和型）+ `get`/`at` ナビゲーション + 4 つの葉アクセサ `as_str`/`as_i64`/`as_f64`/
  `as_bool`（→ `Option`。`as_str` は入力へのゼロコピービュー、エスケープ文字列は arena に unescape）。
  数値は**形**でアクセサが決まる（`42.0` / `1e3` は整数値だが非整数形 → `as_i64` は `None`、`as_f64`
  は `Some`。simdjson on-demand と同じ）。**重複キー**の `get` は**最初**の出現を返す（遅延ビュー。
  型付き `decode` は重複した宣言済みキーを拒否するため、その規則とは意図的に異なる）。**Slice 2 SHIPPED:**
  `d.len()`（要素/メンバー数、非コンテナは 0）+ `d.key(i) -> Option<str>`（object の i 番目のキーを
  文書順で。順序付き object-as-data）。`at(i)` と併せて、doc 配列の反復を再帰で回せる（`loop` 不要）。
  型 `json.doc` / `json.kind` は**名前で書ける**ようになった（`fn f(d: json.doc)` ヘルパや
  `k: json.kind` 束縛が直接解決 — `core.json` の 2 つの組み込み型名）。**Slice 3 SHIPPED — J4 完了:**
  `d.elems() -> slice<json.doc>` は 1 階層（Array の各要素、または Object の各メンバー**値** — キーは
  `key(i)`）を arena 常駐の `slice<json.doc>` に**一度で** materialize する（O(n)、以降のインデックスは
  O(1)。`at(i)` の呼び出しごと O(i) 再走査に対して有利）。既存の `slice` 機構を再利用:
  `slice<json.doc>` = `Ty::Slice(Scalar::JsonDoc)`（既に表現可能 — 新配列型は不要）なので `.len()` と
  `xs[i] -> json.doc`（slice に region 拘束、Copy な 16 バイトハンドル → 二重解放なし）がそのまま動き、
  `slice<json.doc>` は引数型として名前で書けるので `fn f(xs: slice<json.doc>)` が再帰で 1 階層を回せる。
  slice バッファは外側 arena に bump-allocate（`arena {}` が必要）、min(input, arena) に region 拘束。
  `slice<json.doc>` 上の `.map`/`.where` **pipeline fusion**（json.doc を取る closure）は自然な次段だが
  必須ではない — index + len + 再帰で階層反復は今日すでにできる。
  **既知の systemic な緩さ（J4 の退行ではない — `decode` のスキャナと共有）:** 文字列内の生の C0 制御
  バイトと数値の先頭ゼロ（`007`）を現状は受理する。共有の `find_quote_or_escape` / `number_span` を
  RFC 8259 §7/§6 準拠に厳格化するのは `decode` と `doc` を**同時に**直す follow-up（片方だけ直すと
  同じ不正入力 `s` に対し `json.doc(s)` と `json.decode(s)` が食い違う）。
- **`json.scan`（J5）:** 型付き行ストリーミング。binding annotation で型付け、v1 は pipeline
  source 専用。**Slice 1 出荷済み:** `json.scanner<Row>` 型（Copy の `{ptr,len}` 入力ビュー。
  region-tracked — 入力を借用し、`array<Row>` を実体化しない）＋ `json.scan(view)`（行型は binding
  annotation `rows: json.scanner<Row> := json.scan(view)` から、`decode` と同じく。arena 不要 — 行は
  ステップごとのスタックスロットへ decode され、その `str` フィールドは入力を借用）＋ ストリーミング
  fused reducer `.sum()` / `.count()` → **`Result<T, Error>`**（不正な行は一度だけ `Err` として現れる。
  `?` で unwrap）。ステージ: `.field` 射影、`.where(.field)`、`.where(pred)`、`.map(f)` — 全ステージを
  行ごとに [`lower_json_scan_reduce`] が駆動（`lower_array_reduce` のカウントループではない）。1 つの
  scanner がトップレベル JSON 配列と NDJSON の**両方**を扱う（ランタイム `align_rt_json_scan_next` が
  先頭 `[`・値間 `,`・空白/改行を区切り、`]`/EOF を終端として扱い、行ごとに struct decode の descriptor を
  再利用）。ストリームに対する実体化終端（`.to_array()` / `.sort()` / `group_by`）は sema で拒否
  （誤 lowering ではなく明快な診断）。**Slice 2 出荷済み:** scanner に対する残りのストリーミング
  reducer 一式 — `.reduce(init, f)` / `.any(p)` / `.all(p)` / `.min()` / `.max()` — いずれも
  `Result<T, Error>`。`lower_json_scan_reduce` の guarded な行ごと fold を共有する。よって完全な
  ストリーミング reducer 集合は `sum` / `count` / `reduce` / `any` / `all` / `min` / `max`。
  実体化終端のみ対象外（設計どおり — ストリーミングを無効化するため）。

  **J5 の安全境界 — Request 6 設計ゲート（実装待ち）。** scanner は入力値ごとに 1 つの行
  スロットを再利用し、行ごとの arena や `Drop` 遷移を持たない。そのため行スキーマは再帰的に
  **Copy** でなければならない。`json.scan` は、到達可能な struct・option・union の完全な型
  グラフについて canonical な再帰 `DropPlan` が drop 不要と判定した場合だけ行を受理する。
  これは scanner 専用の規則であり、同じ宣言は通常の Align 型として有効なまま、各通常 JSON
  経路のスキーマ契約が許す限り通常の JSON 操作で利用できる。既存 JSON schema whitelist を
  通過した row について、配列の種類を個別列挙するのではなく、直接または推移的な
  `array<T>` / `array<Struct>`、所有する option/union payload を拒否する。JSON schema 自体が
  拒否する field shape（owned `string` や `array<string>` など）には既存の schema 診断を使い、
  下記の Copy 診断は schema-admitted な Move row の ownership error に限定する。

  意味解析の source check は既存の JSON decode schema whitelist の後、入力型検査、MIR 構築、descriptor 構築、
  runtime 呼び出しより前に行う。whole-program では active な `align_mir::hir_program_is_valid` pre-lowering gate が
  scanner の HIR envelope 全体を再検査し、同じ pure row predicate を適用する。active HIR の決定的な順序は、(1)
  `Expr.ty == Ty::JsonScanner(struct_id)`、(2) `struct_id` が既存 row 定義を指すこと、(3) `input.ty == Ty::Str`、
  (4) Decode schema、(5) canonical な再帰 Copy 検査である。envelope の各段階で失敗したら row graph の descriptor
  walker より前に拒否する。imported/per-unit では、まず interface/import reconstruction が checked HIR を構築し、その後
  active gate が同じ 5 段階の順序と row predicate を MIR lowering、descriptor 構築、runtime 呼び出しの前に再検査する。
  gate は source spelling を復元せず、dormant な `align_sema::checked_hir_body_facts_are_valid` の body replay は代替に
  ならない。schema-admitted な row の拒否時の診断は次の exact な source-level 形式とする。

  ```text
  `json.scan` row type '<row-type-source-spelling>' must be Copy; Move rows need per-row Drop before the scanner can reuse its row slot
  ```

  `<row-type-source-spelling>` は module qualifier と具体化済み generic 引数を含む公開表記であり、
  内部 `$` mangled 名や monomorph interner 名を出してはならない。`check_json_scan` は AST の
  spelling が `Ty` に消去される前に、producer-owned な source-type annotation または source-type
  formatter からこの表記を受け取らなければならない。formatter は expected row type を生成した
  module/type-resolution table を使って local/imported path と concrete generic argument を解決し、
  `ty_name`、`StructDef::name`、`StructDef::source_name`、内部 mangled/interner 名を使ってはならない。
  これは runtime reflection や cache/artifact read ではなく、diagnostic contract の producer-owned
  spelling である。受理されたプログラムでは既存の
  scanner handle、入力 region、framing、terminal の `Result`、HIR、MIR、codegen、runtime entrypoint、
  cache identity は変えない。これは行ごとの cleanup 実装ではなく、危険な既存 surface を compile-time
  で拒否する変更である。

  実装ゲートの契約 ledger は次のとおり。

  | Surface | 契約 |
  | --- | --- |
  | Public entrypoint | `rows: json.scanner<Row> := json.scan(view)`。row type は call-site の型引数ではなく expected scanner annotation から得る。scanner は pipeline source のみ。 |
  | Input と result | `view` は既存の `str` input（または既存の `string` からの明示的 borrow）。region は scanner を束縛する。対応する fused terminal は既存の `Result<T, Error>` scalar result を返し、malformed-row と exhaustion の挙動を保つ。 |
  | Compiler/runtime owner | `align_sema::Checker::check_json_scan` が source validation と source spelling を所有する。imported/per-unit では interface/import reconstruction が先に checked HIR を構築し、4 つの MIR lowerer が private な `align_mir::hir_program_is_valid(&hir::Program) -> bool` を呼ぶ。その active Request 6 exception は `validate_hir::json_scan_copy_rows_are_valid` を上記 5 段階の順序で MIR/runtime 構築より前に実行する。これは dormant な `align_sema::checked_hir_body_facts_are_valid` とは別であり、共有できるのは pure row-predicate helper だけである。既存の MIR `JsonScan` lowering、LLVM emission、`align_rt_json_scan_next` が受理時の実行を所有し、gate は runtime owner を追加しない。 |
  | 行の受理条件 | canonical `DropPlan` が有効で drop 不要な再帰的 non-owning row のみ `json.scan` が受理する。 |
  | 検査順序 | Source は capability import、arity、scanner annotation/inference、既存 JSON schema、再帰 Copy 検査、最後に入力 `str` 型と region。active HIR replay は exact に `Expr.ty == Ty::JsonScanner(struct_id)`、既存 row id、`input.ty == Ty::Str`、Decode schema、再帰 Copy 検査の順で、descriptor/MIR consumer より前に拒否する。 |
  | Ownership | 拒否行では scanner、descriptor、行スロット、allocation、runtime side effect を構築しない。受理行は既存の入力 borrow と Copy 行スロットを保持する。 |
  | 診断の identity | producer-owned な公開 local/imported/generic source spelling を使い、HIR mangling から復元しない。 |
  | ABI と永続化 | N/A。accepted program の source syntax、HIR/MIR node、descriptor、runtime ABI、wire format、cache identity は変更しない。 |
  | Runtime cleanup | accepted row の完全な型グラフは Drop 不要なので N/A。既存の scanner 入力と scalar accumulator の cleanup が引き続き所有する。 |
  | Compatibility prerequisite | implementation PR はこの design gate 後に作成し、既存 JSON schema と scanner terminal 契約を保持する。Request 6 の align-llm adoption は implementation release を pin した後の consumer gate である。 |
  | Acceptance と benchmark | 下記 owner test、`scripts/compare-json-scan-identity.sh` cross-compiler identity probe、`json_scan_copy_row_no_owned_alloc` allocation probe が契約を閉じる。性能主張はなく benchmark は N/A。 |
  | Source-of-truth map | この English design、本文書、`draft.md`、`docs/language-spec.md`、`docs/design-notes.md`、`docs/open-questions.md`、`docs/impl/17-library-boundary-prerequisites.md`、`docs/impl/19-hir-validation-ledger.md`、align-llm Request 6 register が一致しなければならない。 |
  | 並行 scanner | compile-time gate には N/A。accepted scanner は既存の独立した handle と slot を使う。 |
  | Performance | N/A。性能主張はせず、production MIR・codegen・runtime は変更しない。 |

  **Ownership closure matrix（implementation gate）。** 次の cell は implementation PR の開始前に閉じる。
  `N/A` は recursively Copy precondition の結果であり、決定の省略ではない。

  | Cell | Intended owner | Exact regression / benchmark |
  | --- | --- | --- |
  | Type formation、row validation、scanner construction | `align_sema::Checker::check_json_scan`。schema と Copy check を通るまで scanner node を生成しない。 | `m5::json_scan_copy_row_terminal_matrix`、`m5::json_scan_rejects_owned_row_fields` |
  | Move-in、move-out、source nulling、replacement、returned row ownership | accepted row では N/A。`DropPlan` が Move field なしを証明し、拒否経路は construction 前に戻る。 | `m5::json_scan_copy_row_error_matrix`、`json_scan_copy_row_no_owned_alloc` |
  | `if`、`match`、`else`、`?`、`map_err`、branch/loop join、early terminal return、malformed input | 既存 scanner MIR/runtime control flow。新しい ownership edge は Copy row invariant を越えて導入しない。 | `m5::json_scan_copy_row_terminal_matrix`、`m5::json_scan_copy_row_error_matrix` |
  | Direct、nested、optional、union、invalid/cyclic schema graph | canonical recursive `DropPlan` と JSON schema の producer table。missing/invalid graph node では fail closed。active gate は interface/import reconstruction 後も同じ pure predicate を適用し、scanner envelope の type/id mismatch と non-`str` input も fail closed にする。 | `m5::json_scan_rejects_transitive_owned_row_fields`、`m5::json_scan_row_schema_matrix`、`hir_body_validator_json_scan_copy_row`、`hir_program_json_scan_copy_row`、`hir_program_json_scan_envelope_mismatch` |
  | Generic monomorphization、return-context inference、imported source spelling | type-resolution producer が concrete public spelling を所有する。generic argument の検査前に concrete な expected `json.scanner<Row>` return context が substitution を seed し、substitute 済み parameter/return type が nested `json.scan` へ concrete spelling を渡す。解決済みの各 instantiation は既存 Decode schema と canonical `DropPlan` を再検査する。missing、ambiguous、conflicting substitution は既存の決定的 inference diagnostic を維持し、scanner HIR node と artifact を生成しない。 | `m5::json_scan_generic_row_ownership`、`m5::json_scan_generic_return_context_ownership`、`modules::json_scan_imported_row_ownership`、`modules::json_scan_imported_generic_return_context_ownership` |
  | Whole-program、per-unit、cold/hot cache、schema edit/revert | 既存 structural MIR/cache identity が owner。拒否 row は artifact を publish しない。per-unit fixture は interface reconstruction、accepted Copy row、rejected Move row を網羅する。 | `cache_codegen::json_scan_row_schema_rejection`、`cache_codegen::json_scan_per_unit_interface_row_ownership`、accepted Copy-row MIR/raw-LLVM identity comparison |
  | Interface serialization と persisted/wire identity | imported/per-unit の checked HIR には interface/import reconstruction が入力される。その reconstructed HIR の scanner envelope と row graph を active gate が MIR/runtime construction 前に検証し、accepted source identity は不変。 | `cargo test -p align_interface --test summary`、`modules::json_scan_imported_row_ownership`、`cache_codegen::json_scan_per_unit_interface_row_ownership` |
  | Runtime ownership provenance と allocation parity | 既存 scanner input/accumulator owner。composite fixture は `Option<i64>` の Some、明示的 `None`、欠落 field、non-owning union の全 arm、borrowed `str` field を nonempty と later-malformed stream で検証する。LLVM allocation oracle は `align_rt_json_scan_next` を要求し、`align_rt_alloc` と `align_rt_arena_alloc` の call を禁止する。 | `json_scan_copy_row_no_owned_alloc`、`json_scan_copy_row_copy_composites_no_owned_alloc`、`m5::json_scan_copy_composite_runtime_matrix` |
  | Exhaustion、empty input、malformed first/later row、`Result`/`?` cleanup | 既存 scanner input と accumulator cleanup。row-slot cleanup は invariant により N/A。Copy option/union row には nonempty と later-malformed stream を追加する。 | `m5::json_scan_copy_row_error_matrix`、`m5::json_scan_copy_row_terminal_matrix`、`m5::json_scan_copy_composite_runtime_matrix` |
  | Concurrent independent scanners | 新 gate には N/A。既存 independent handle、immutable descriptor、row slot の分離を保持する。 | 1 program 内の accepted scanner terminal 2 つと既存 nested-scanner rejection |
  | Performance | N/A。production performance claim はしない。 | N/A。implementation PR に理由を記録する。 |

  設計受入マトリクスは、直接・推移的な owned field、nested/optional struct、JSON の全 scalar
  width、borrowed `str`、Copy option/union、local/imported 型、resolved/unresolved/ambiguous/conflicting
  return-context inference を含む generic monomorph、MIR より前の semantic rejection、malformed scanner
  envelope、whole-program/per-unit interface reconstruction、cache の cold/hot/edit/revert、
  malformed/exhausted stream、通常の `json.decode` 互換性を網羅する。主な owner test は
  `m5::json_scan_copy_row_terminal_matrix`、`m5::json_scan_rejects_owned_row_fields`、
  `m5::json_scan_rejects_transitive_owned_row_fields`、`m5::json_scan_generic_row_ownership`、
  `m5::json_scan_generic_return_context_ownership`、`m5::json_scan_copy_composite_runtime_matrix`、
  `m5::json_scan_rejects_owned_composite_rows`、`hir_program_json_scan_envelope_mismatch`、
  `modules::json_scan_imported_row_ownership`、`modules::json_scan_imported_generic_return_context_ownership`、
  `cache_codegen::json_scan_row_schema_rejection`、`cache_codegen::json_scan_per_unit_interface_row_ownership`、
  runtime の allocation probe は `json_scan_copy_row_no_owned_alloc` と
  `json_scan_copy_row_copy_composites_no_owned_alloc` とする。名前付き identity probe
  `scripts/compare-json-scan-identity.sh` は checked-in fixture
  `crates/align_driver/tests/fixtures/json_scan_copy_identity.align`、baseline Align commit
  `576e57307fe4ef34e74566f5e389a2f0e2a04acd`、実装 PR と `HANDOFF.md` に記録した exact な
  implementation-head SHA を使う。2 つの clean release worktree で `cargo build --release --locked -p alignc`
  を実行し、required な `x86_64-unknown-linux-gnu` / LLVM 22 acceptance environment で `LC_ALL=C` を設定して、
  `alignc emit-interface`、`alignc emit-mir`、`alignc emit-llvm --stage raw`、`alignc emit-obj` の出力を
  `cmp` で byte-identical に比較する。single-file fixture は unit banner を持たず、output normalization は許可しない。
  Linux object comparison が失敗または利用不能なら gate は fail とし、optional な主張にはしない。align-llm Request 6 の
  adoption fixture が後の pin 変更を所有する。

  現在の compiler は一部の owning row をまだ受理する。この記述は reviewed target contract で
  あり、実装済みという主張ではない。実装 PR はこの設計ゲート後にのみ作成し、通常の decode、encode、
  scope Drop が維持する `Option<Move-struct>` JSON shape と、後続 sibling の decode error 後の cleanup 欠陥を
  別 request として明示する。

決着済みの削除（未実装のまま残すのではなくカタログから削除）: `json.validate<T>`（decode して
捨てるのが validation）、`json.token`（doc + scan で覆う。consumer なし）、`json.field_table<T>`
（コンパイラ内部）。`json.decode<T>(...)` 呼び出し構文は恒久的に不採用（no turbofish）。

## Pitfalls

- P1 — **デコードのターゲット文法はホワイトリスト制** であり、意味解析（sema）で強制される。ターゲットとなる型を追加するということは、既存の投機的パスやフォールバック機構（カウントパス、`FieldDst`、エラータグ）をすべて対応させることを意味する。特殊なデータ構造に対してパニックを引き起こすような不完全なサポートは、#295 で解決したバグクラスそのものである。その問題を再び引き起こしてはならない。
- P2 — 投機的（Mison PHF）パスとスローパスは、**外部から観測可能な挙動が完全に同一（observably identical）** に保たれなければならない（重複キーの扱い、エスケープ文字、数値の境界値など）。パーサーに変更を加えた場合は、必ず両方のパスに対して再度ファジング（`fuzz_differential` 方式のオラクルテストまたは m5 コーパス）を実行する必要がある。
- P3 — `encode` のエスケープ用テーブルは string builder のパスに組み込まれている。新しくエスケープが必要なフィールド型を追加する場合は、その場限りのエスケープ処理をインラインで書くのではなく、このテーブルの機能を拡張すること。
- P4 — soa デコードのパフォーマンス目標（100万行の処理において `serde` と同等レベル、`bench/json_soa`）は、パフォーマンス低下（リグレッション）を検知するための罠（tripwire）である。パーサーの変更をマージする前に、必ずこのベンチマークを再実行すること。
- P5 — **デコードターゲットのフィールドスキーマは codegen のキャッシュキーに反映されなければならない。** デコードターゲット構造体のフィールド名/型は周囲の文列ではなく codegen のディスクリプタテーブルに効く。そのため per-unit キーは、struct/enum テーブル、`layout(C)`、alignment を含む構造化 MIR Program 全体を指紋化する。`cache_codegen.rs` の gate 2/2b が flat、nested、型テーブルだけの変更を固定する。JSON の MIR ノードはコピーしたスキーマ文字列ではなく target id を持つ。新しいスキーマ面も全 backend 入力を構造化 Program に置き、人向け MIR printer にキャッシュ専用文字列を追加してはならない。

## Test anchors

`m5.rs`（デコードのマトリクステスト: 構造体/配列/str フィールド/順序/未知のキー/不正なデータ/数値の範囲 #295 #311、エンコード時のエスケープ、重複キー #306、**ネスト** の decode+encode ラウンドトリップ `json_decode_encode_nested_struct_roundtrip` と Mison パス `json_decode_nested_struct_array_mison`）、`soa.rs:317`（json から soa へのフィルタ済み集約）、`cache_codegen.rs` gate 2/2b（構造化 codegen 入力によるキャッシュ無効化、flat + nested）、ランタイム `json_decode_nested_struct_single` / `..._array_mison`（ディスクリプタレベルのスロー + Mison 再帰）。例として `json.align`、`json_decode.align`、`json_nested.align`、`soa_json_str.align`。ベンチマークとして `bench/json_decode`、`bench/json_soa`（計測モデルの詳細はそれぞれの README を参照）。
