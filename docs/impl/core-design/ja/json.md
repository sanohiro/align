このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同等の粒度（シグネチャ、Move/effect の分類、エラー方針、落とし穴（Pitfalls）、テストアンカー）で記述された公式な設計ドキュメントを収めている。
執筆はメインループ（Fable）が担当している。

# core — json

> 🌐 [English](../json.md) · **日本語**

## Overview

テキスト境界を越える型付きレコードのシリアライズ・デシリアライズ（draft §14）。提供される関数は `encode` と `decode` の 2 つであり、ターゲットとなる型は **明示的な型引数ではなく型推論によって** 決定される（決定済: Align には式位置での型引数構文、つまり turbofish のような記法は存在しない）。使用には `import core.json` が必要である（capability-header のルールは `core.json` に対しても std モジュールと全く同様に適用される）。

## Signatures (verified)

```text
json.encode(x)   -> str                      // x: struct (nested structs recurse); str fields JSON-escaped
json.decode(s)   -> Result<T, Error>         // T from the binding/context: u: User := json.decode(s)?

// decode targets, all verified:
//   struct                 (flat OR with nested-struct / Option<T> / array<Struct> fields; field order free; unknown keys ignored)
//   array<i64> / array<f64>
//   array<Struct>          (AoS; str fields = zero-copy views into the input; nested-struct + Option fields recurse)
//   soa<Struct>            (direct columnar decode — no AoS intermediate, no transpose;
//                           inside arena {}; str columns borrow the input text; primitive/str columns only,
//                           NO nested columns — the owned-columns deferral stands)
//   enum (union)           (shape-directed: a JSON oneOf → a sum type; the variant is selected by the
//                           value's shape class — str/number/bool/object; O(1) first-byte dispatch;
//                           str payloads borrow the input; array-payload variants = J2)
```

**Union（直和型）ターゲット（JSON completeness J1b）。** JSON `oneOf` は、値の **shape class** —
`Str`（`"`）/ `Number`（数字・`-`）/ `Bool`（`t`/`f`）/ `Object`（`{`）— で判別される直和型にマップされる
（先頭構造バイトでの O(1) ディスパッチ）。**コンパイル時検査（Align らしい設計）:** union-decodable な
enum は各バリアントがちょうど 1 つの payload を持ち、各 payload が 1 つの shape class にマップされ、
すべての class が **相互に排他** — `i64 | f64`（両方 Number）や 2 つの object payload は clash として
コンパイルエラー、tag-only や shape を持たない（`char`）payload も拒否。`null` は class ではない
（不在は `Option` の担当）。実行時に該当バリアントのない shape（配列や `null`）は decode `Err`。encode は
生きているバリアントの payload を **そのまま（ラッパーキー無し）** 出力するので、`decode(encode(x))` は
構成上ラウンドトリップする。ランタイム: `JsonUnion` descriptor（バリアントごとに 1 つの `JsonField`
payload arm ＋ shape-class→arm テーブル ＋ arm→enum-tag テーブル）。decode は先頭バイトを分類し、共有
`write_value` で payload を書き、tag を設定。encode は tag を読んで共有 `json_encode_value` で該当 arm を
出力。**v1 の境界:** payload は str / number / bool / object（所有 `array<Struct>` payload — OpenAI
マルチモーダル `content` union — は enum 所有 payload の drop が必要、J2）。union の `json.encode` は
ローカル束縛が必要（struct encode と同様）。**構造体フィールドとしての union**（`Message { content:
Content }`）は J1b-2b。

**`array<Struct>` フィールド（REST-gateway runway, Slice C）。** 構造体フィールドは所有の `array<Struct>`
であってよい — `messages: array<Message>` / `choices: array<Choice>` shape。フル OpenAI リクエスト/
レスポンスがラウンドトリップする。decode: descriptor kind 5（`sub` = 要素スキーマ）が
`decode_struct_array_value` を駆動し、JSON サブ配列を所有 AoS にパース（要素ごとに `parse_object`、
nested/`Option` 要素フィールドも再帰）して `{ptr,len}` をフィールドに書く。バッファは構造体の `Drop` で解放。
encode: `StructArrayField` ピースが runtime の descriptor 駆動エンコーダ（`json_encode_struct_array` →
`json_encode_object`、**decode descriptor を再利用** — 対称的で nested/Option/str/scalar を扱う）を呼ぶ。
**memory-safety:** array フィールド確保後に decode が `Err` になった場合、`drop_decoded_owned` が部分構造体の
AoS バッファを解放（codegen `drop_struct_fields` の runtime 双対）。**v1 要素制限:** 非所有（scalar /
`str` view / plain-data struct）— `array<string>` / `array<Move-struct>` は宣言時に拒否。**制約:** Move
構造体（array を所有）は関数境界を越える `Result`/`Option` Ok payload になれない — スコープ内で decode +
使用する。延期: `array<scalar>` フィールド decode、所有要素配列。

**`Option<T>` フィールド（REST-gateway runway, Slice B）。** 構造体フィールドは `Option<T>`（payload は
scalar / `str` / ネスト構造体）であってよい。**null ポリシー:** decode はキー欠落→`None`、JSON `null`→
`None`、型不一致→`Err`、必須（非 `Option`）フィールドは欠落で `Err`。**encode は `None` フィールドを
完全省略**（`"k":null` にしない）ので `decode(encode(x))` はラウンドトリップする。ランタイム: `JsonField`
に `opt_tag`（`-1`=必須、それ以外は `Option` の tag バイトオフセット）を追加。optional フィールドは
`all_required_seen` の対象外で、共有の `write_value` が payload スロットに書いてから `Some` tag を立てる。
encode は `Option` を含むオブジェクトを trailing-comma 方式に切替え、`}` の前で `align_rt_builder_pop_comma`
を 1 回呼ぶ（必須のみのオブジェクトは静的レイアウトを維持）。**v1 境界:** Option payload は **非所有**
（`Option<string>`/`Option<Move-struct>` は宣言時に拒否）。残る follow-up は `Option<struct>` の **encode**
（decode は対応済み）。

**ネストされた構造体フィールド（REST-gateway runway, Slice A）。** 構造体のフィールドはそれ自身が
`Struct` であってよい。`decode` はネストされたオブジェクトへ再帰し、`encode` はそれを再構築するため、
ネストされたレコードもラウンドトリップできる。ランタイム側ではフィールドディスクリプタが kind 4 と
`JsonSubTable` ポインタ（ネスト構造体自身のディスクリプタ + PHF + store size）を持ち、`parse_object` /
`write_field_indexed` が再帰する — したがってスローパスと Mison 投機パスの **両方** がネストを扱う
（ネストフィールドはレコードレベルのコロン 1 個で、その値をレコード分割器はより深いブラケット深度に
残す）。ネストされた `str` フィールドは入力へのゼロコピービューのままなので、値全体が再帰的に入力へ
region-tie される（`struct_has_str` が再帰する）。ここでの延期項目: `Option<T>` フィールド（Slice B）、
`array<T>` フィールド（Slice C）、enum ペイロードターゲット。

## Type & ownership classification

- `encode` は内部的に string builder を使用して文字列を構築する。戻り値は arena に region 付けされた `str` となる。
- `array<T>` / `array<Struct>` への `decode` は、所有権を持つ Move 配列を生成する（破棄時は deep-drop される）。
- `soa<T>` への `decode` は、外側の arena に列（カラム）を割り当てる（`align_rt_json_decode_soa` により、1 回のカウント用パスと 1 回の値パース用パスが `FieldDst` を介して Mison の投機的実行（speculation）パスを共有する）。
- デコードされた `str` フィールドや列は、**入力された `str` へのビュー（参照）** である。そのため、入力データはデコード結果よりも長生きしなければならず、これは region チェッカによって強制される。

## Effects

Pure（パース処理は純粋な計算であり、I/O は発生しない。バイトデータの入出力には `std.fs` や `std.io` を組み合わせる）。

## Errors & aborts

不正なデータはすべて `Err(Error)` として扱われ、パニックが発生したり、静かに誤った値が返されたりすることは決してない。これには構文エラー、フィールドの欠落、型の不一致、**範囲外の整数** が含まれる（符号を考慮するフィールドタグ、#295。`u64` フィールドは単一のディスパッチャを経由して `u64` の全範囲を受け入れる、#311）。投機的パス上で重複するキーが発見された場合も、一貫したルールで解決される（スローパスとの「後勝ち（last-wins）」挙動の同一性、#306 — 状態管理の追加オーバーヘッドはなく、コストは未宣言のコロンを持つレコードにのみ限定される）。

## Regions

`region_of(decoded str view) = region_of(input)`、`region_of(soa columns) = enclosing arena`。所有権を持つ（owned な）配列は自由にエスケープできる。デコード済みのビューを、その入力データの寿命を超えてエスケープさせようとした場合は、エスケープの時点でコンパイルエラーとして捕捉される（保持し続けたい場合は `.clone()` でコピーを取り出す必要がある）。

## 設計済み・未実装（JSON 完全対応設計、2026-07-18 決着）

完全な設計は `open-questions.md` →「JSON completeness — DESIGN SETTLED」（実装の source of
truth。spec 本文は draft §14 + §18.1）。残りスライスは J1–J6：

- **union（J1–J2）:** JSON の `oneOf` は sum type に写像し、**shape class**（Str/Number/Bool/
  Object/Array、pairwise 相異をコンパイル時強制、先頭バイト O(1) ディスパッチ）で判別。encode は
  生きている variant の payload を裸で書く。言語側の前提: enum の `str` payload（region 追跡）→
  所有 payload（`array<Struct>`、tag 分岐 drop）。
- **行列残り（J3）:** top-level scalar ターゲット、`array<scalar>` フィールド、`Option<struct>`
  encode、サポート済みコンストラクタの合成。
- **`json.doc`（J4）:** スキーマ未知の遅延ビュー — arena 常駐 tape。ナビゲーションは total かつ
  Missing 伝播（`get`/`at` は常に doc を返し、欠落は葉の `as_*` の `None` として一度だけ現れる）。
  キーがデータの object は順序付き `key(i)`+`at(i)` で吸収、`elems()` で 1 階層を materialize して
  pipeline に流す（map 型も serde 式 value 木も導入しない）。
- **`json.scan`（J5）:** 型付き行ストリーミング。binding annotation で型付け、v1 は pipeline
  source 専用。

決着済みの削除（未実装のまま残すのではなくカタログから削除）: `json.validate<T>`（decode して
捨てるのが validation）、`json.token`（doc + scan で覆う。consumer なし）、`json.field_table<T>`
（コンパイラ内部）。`json.decode<T>(...)` 呼び出し構文は恒久的に不採用（no turbofish）。

## Pitfalls

- P1 — **デコードのターゲット文法はホワイトリスト制** であり、意味解析（sema）で強制される。ターゲットとなる型を追加するということは、既存の投機的パスやフォールバック機構（カウントパス、`FieldDst`、エラータグ）をすべて対応させることを意味する。特殊なデータ構造に対してパニックを引き起こすような不完全なサポートは、#295 で解決したバグクラスそのものである。その問題を再び引き起こしてはならない。
- P2 — 投機的（Mison PHF）パスとスローパスは、**外部から観測可能な挙動が完全に同一（observably identical）** に保たれなければならない（重複キーの扱い、エスケープ文字、数値の境界値など）。パーサーに変更を加えた場合は、必ず両方のパスに対して再度ファジング（`fuzz_differential` 方式のオラクルテストまたは m5 コーパス）を実行する必要がある。
- P3 — `encode` のエスケープ用テーブルは string builder のパスに組み込まれている。新しくエスケープが必要なフィールド型を追加する場合は、その場限りのエスケープ処理をインラインで書くのではなく、このテーブルの機能を拡張すること。
- P4 — soa デコードのパフォーマンス目標（100万行の処理において `serde` と同等レベル、`bench/json_soa`）は、パフォーマンス低下（リグレッション）を検知するための罠（tripwire）である。パーサーの変更をマージする前に、必ずこのベンチマークを再実行すること。
- P5 — **デコードターゲットのフィールドスキーマは codegen のキャッシュキーに反映されなければならない。** デコードターゲット構造体のフィールド名/型は codegen のディスクリプタテーブルにのみ効き、周囲の MIR には現れない — 同一スロットでのフィールド名変更（RENAME）や、ネスト構造体のフィールド変更は、それ以外の MIR 文をバイト単位で不変にする。したがってスキーマフィンガープリントがなければユニットの `impl_hash` は変化せず、暖まったキャッシュが古いキーでデコードする **陳腐化した（STALE）** オブジェクトを提供してしまう（end-to-end で再現、#514/#517 の陳腐キャッシュクラス）。`JsonDecode*` MIR rvalue は再帰的な `json_schema_sig`（フィールド名 + 型 + `layout(C)`/`align`、ネスト展開）を埋め込み MIR に印字する — `cache_codegen.rs` の gate 2b で固定。スキーマを持つ新しいデコード面を追加する場合も同様にすること。

## Test anchors

`m5.rs`（デコードのマトリクステスト: 構造体/配列/str フィールド/順序/未知のキー/不正なデータ/数値の範囲 #295 #311、エンコード時のエスケープ、重複キー #306、**ネスト** の decode+encode ラウンドトリップ `json_decode_encode_nested_struct_roundtrip` と Mison パス `json_decode_nested_struct_array_mison`）、`soa.rs:317`（json から soa へのフィルタ済み集約）、`cache_codegen.rs` gate 2b（スキーマフィンガープリントによるキャッシュ無効化、flat + nested）、ランタイム `json_decode_nested_struct_single` / `..._array_mison`（ディスクリプタレベルのスロー + Mison 再帰）。例として `json.align`、`json_decode.align`、`json_nested.align`、`soa_json_str.align`。ベンチマークとして `bench/json_decode`、`bench/json_soa`（計測モデルの詳細はそれぞれの README を参照）。
