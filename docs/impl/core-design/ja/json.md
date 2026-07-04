このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同じ粒度(シグネチャ、
Move/effect の分類、エラー方針、pitfalls、テストアンカー)の正典となる設計ドキュメントを収めている。
執筆はメインループ (Fable) が担当している。

# core — json

> 🌐 [English](../json.md) · **日本語**

## Overview

テキスト境界を越える型付きレコード(draft §14)。関数は 2 つ — encode と decode — で、ターゲット型は
**書かれた型引数ではなく推論によって** 運ばれる(settled: Align には式位置の型引数構文が無い / turbofish
も無い)。`import core.json` が必要である(capability-header ルールは core.json にも std モジュールと全く
同じように適用される)。

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

- `encode` は string builder を通して組み立てる。結果は arena に region 付けされた `str` である。
- `array<T>`/`array<Struct>` への `decode` は所有権付きの Move 配列を生む(deep-drop される)。
- `soa<T>` への `decode` は列を外側の arena に確保する(`align_rt_json_decode_soa`、1 回の count パス +
  1 回の value-parse パスが `FieldDst` 経由で Mison speculation を共有する)。
- デコードされた `str` フィールド/列は **入力 `str` へのビュー** である — 入力はデコード済みの値より
  長生きしなければならず、region チェッカがそれを強制する。

## Effects

Pure(パースは計算であって I/O は無い — バイトのためには `std.fs`/`std.io` と組み合わせる)。

## Errors & aborts

不正なものはすべて `Err(Error)` になる — panic も、黙って誤った値になることも決してない: 構文エラー、
フィールドの欠落、型の不一致、**範囲外の整数**(符号を運ぶフィールドタグ、#295; `u64` フィールドは 1 つの
write dispatcher を通して `u64` の全域を受け入れる、#311)。投機パス上の重複キーは一貫して解決される
(slow パスと last-wins のパリティ、#306 — 新しい状態はゼロ、コストは未宣言のコロンを持つレコードに
限定される)。

## Regions

`region_of(decoded str view) = region_of(input)`; `region_of(soa columns) = enclosing arena`; owned
配列は自由に escape する。デコード済みビューをその入力を越えて escape させることは escape 地点で捕捉
される(保持したければ clone で外へ出す)。

## 仕様先行(未実装)

- `json.scan`、`json.token`(streaming/SAX ティア)、`json.validate<T>`、`json.field_table<T>`(§18.1
  カタログ) — ディスパッチのアーム無し。`<T>` を明示するペアは、settled な no-turbofish ルールによって
  *も* ブロックされている — §18.1 は既に「残余のスキーマ選択のケース…`decode` に畳み込まれるかもしれ
  ない」と記録している。ここで何かを実装する前に `open-questions.md` で決着させること。
- `json.decode<T>(...)` の呼び出し構文 — 恒久的に除外(settled)。注釈を `?` 越しに与える形が唯一の道で
  ある。
- ネスト struct / Option フィールド / enum ペイロードのデコードターゲット — 検証済みマトリクスには無い。
  ターゲット文法の拡張はコードの前に設計作業である(field table、null 方針)。

## Pitfalls

- P1 — **デコードのターゲット文法は whitelist** であり、sema で強制される。ターゲット型を追加すること
  は、同じ speculation/fallback の機構(count パス、`FieldDst`、error タグ)を一巡させることを意味する
  — 風変わりな形で panic する部分的サポートは #295 が閉じたバグクラスである。再び開けてはならない。
- P2 — 投機(Mison PHF)パスと slow パスは **観測上同一** に保たれなければならない(重複キー、escape、
  数値の端)。パーサーを変えたら両パスを再 fuzz する必要がある(`fuzz_differential` 風の oracle か m5
  コーパス)。
- P3 — encode の escape テーブルは builder パスに存在する — 新たに escape 可能なフィールド型は、その場
  限りの escape をインライン化するのではなく、それを拡張すること。
- P4 — soa decode の性能契約(1M 行で serde とほぼ同等、`bench/json_soa`)はリグレッションの tripwire
  である。パーサー変更を land する前に bench を再実行すること。

## Test anchors

`m5.rs`(デコードマトリクス: struct/arrays/str-fields/order/unknown-keys/malformed/range #295 #311;
encode の escape; 重複キー #306)、`soa.rs:317`(json→soa のフィルタ済み集約)、例 `json.align`、
`json_decode.align`、`soa_json_str.align`; ベンチ `bench/json_decode`、`bench/json_soa`(+ 計測モデルに
ついてはそれぞれの README)。
