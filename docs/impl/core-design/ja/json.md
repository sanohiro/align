このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同等の粒度（シグネチャ、Move/effect の分類、エラー方針、落とし穴（Pitfalls）、テストアンカー）で記述された公式な設計ドキュメントを収めている。
執筆はメインループ（Fable）が担当している。

# core — json

> 🌐 [English](../json.md) · **日本語**

## Overview

テキスト境界を越える型付きレコードのシリアライズ・デシリアライズ（draft §14）。提供される関数は `encode` と `decode` の 2 つであり、ターゲットとなる型は **明示的な型引数ではなく型推論によって** 決定される（決定済: Align には式位置での型引数構文、つまり turbofish のような記法は存在しない）。使用には `import core.json` が必要である（capability-header のルールは `core.json` に対しても std モジュールと全く同様に適用される）。

## Signatures (verified)

```text
json.encode(x)   -> str                      // x: flat struct; str fields JSON-escaped
json.decode(s)   -> Result<T, Error>         // T from the binding/context: u: User := json.decode(s)?

// decode targets, all verified:
//   flat struct            (field order free; unknown keys ignored)
//   array<i64> / array<f64>
//   array<Struct>          (AoS; str fields = zero-copy views into the input)
//   soa<Struct>            (direct columnar decode — no AoS intermediate, no transpose;
//                           inside arena {}; str columns borrow the input text)
```

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

## 仕様先行(未実装)

- `json.scan`、`json.token`（ストリーミング/SAX 層）、`json.validate<T>`、`json.field_table<T>`（§18.1 のカタログ）— 現在ディスパッチ用のアームは未実装。`<T>` を明示するこれらの機能は、turbofish 構文を持たないという決定済みのルールによって *ブロックされている*。§18.1 でも既に「残るスキーマ選択のユースケースは… `decode` に統合される可能性がある」と記載されている。実装を進める前に、まずは `open-questions.md` でこの方針を決着させること。
- `json.decode<T>(...)` の呼び出し構文 — これは恒久的にサポートされない（決定済）。型は変数束縛や `?` 演算子の文脈から推論させる形が唯一の手段となる。
- ネストされた構造体 / Option フィールド / enum ペイロードをターゲットとするデコード — 現在は検証済みマトリクスに含まれていない。ターゲット文法の拡張は、コードを書く前に設計（フィールドテーブル、null の扱いなど）を行う必要がある。

## Pitfalls

- P1 — **デコードのターゲット文法はホワイトリスト制** であり、意味解析（sema）で強制される。ターゲットとなる型を追加するということは、既存の投機的パスやフォールバック機構（カウントパス、`FieldDst`、エラータグ）をすべて対応させることを意味する。特殊なデータ構造に対してパニックを引き起こすような不完全なサポートは、#295 で解決したバグクラスそのものである。その問題を再び引き起こしてはならない。
- P2 — 投機的（Mison PHF）パスとスローパスは、**外部から観測可能な挙動が完全に同一（observably identical）** に保たれなければならない（重複キーの扱い、エスケープ文字、数値の境界値など）。パーサーに変更を加えた場合は、必ず両方のパスに対して再度ファジング（`fuzz_differential` 方式のオラクルテストまたは m5 コーパス）を実行する必要がある。
- P3 — `encode` のエスケープ用テーブルは string builder のパスに組み込まれている。新しくエスケープが必要なフィールド型を追加する場合は、その場限りのエスケープ処理をインラインで書くのではなく、このテーブルの機能を拡張すること。
- P4 — soa デコードのパフォーマンス目標（100万行の処理において `serde` と同等レベル、`bench/json_soa`）は、パフォーマンス低下（リグレッション）を検知するための罠（tripwire）である。パーサーの変更をマージする前に、必ずこのベンチマークを再実行すること。

## Test anchors

`m5.rs`（デコードのマトリクステスト: 構造体/配列/str フィールド/順序/未知のキー/不正なデータ/数値の範囲 #295 #311、エンコード時のエスケープ、重複キー #306）、`soa.rs:317`（json から soa へのフィルタ済み集約）。例として `json.align`、`json_decode.align`、`soa_json_str.align`。ベンチマークとして `bench/json_decode`、`bench/json_soa`（計測モデルの詳細はそれぞれの README を参照）。
