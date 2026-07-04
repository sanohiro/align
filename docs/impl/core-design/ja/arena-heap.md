このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同じ粒度(シグネチャ、
Move/effect の分類、エラー方針、pitfalls、テストアンカー)の正典となる設計ドキュメントを収めている。
執筆はメインループ (Fable) が担当している。

# core — arena / heap.new / box

> 🌐 [English](../arena-heap.md) · **日本語**

## Overview

目に見える確保の住処は 2 つある(draft §6)。`arena {}` — バッチのライフタイム — と `heap.new` — 単一の
明示的な確保で arena に常駐する。メモリモデルのそれ以外(Move 型、region、drop)は言語であってライブラリ
ではない。このファイルが扱うのは、ライブラリの形をした表面と、現時点で意図的にきつく絞った制限である。

## Signatures (verified)

```text
arena { … }                 // expression block; all arena allocations freed at }, O(1)
heap.new(x)   -> box<T>     // ONE arg; must be inside an arena {}; T = primitive scalar only
b.get()       -> T          // copy the payload out
b.clone()     -> box<T>     // deep-copy the box; both remain valid
```

box の表面はこれで全部である — `.set()` も deref 演算子もない。

## Type & ownership classification

- `box<T>` は region 追跡される `Arena(depth)` データである。汎用のオーナーでは **ない**: 関数の引数にも
  戻り値型にもできず(「boxes are arena-local in M3」/「would escape its arena」)、array/slice の要素にも、
  Option/Result のペイロードにもできない。
- ペイロードのホワイトリスト: int/float/bool/char。それ以外は個別の診断で拒否する。所有権付きの Move 値
  (「an owned `…` cannot be boxed」)、struct、sum 型、`str` ビュー(`box<str>` は型解決の段階でも拒否)。
- `arena` は式である。その末尾値は region-free(あるいは clone で外へ出した)場合に限り escape する。

## Effects

Pure。arena の確保は bump であり、ここでは何も OS に触れない(arena のバッキングページは、検証済みの
`noalias`/`nounwind` 属性を持つランタイムアロケータから来る、#301)。

## Errors & aborts

ランタイムでは何もない(確保失敗は、あらゆるランタイム確保と共有するプロセスレベルの abort 経路である)。
それ以外はすべてコンパイルエラー: arena の外での `heap.new`、arena に region 付けされた値の escape
(`cannot return a value allocated in an arena`)、非スカラーの boxing。

## Regions

region モデルの参照実装である: `region_of(box) = Arena(depth)`;`region_of(b.get()) = none`(スカラー
コピー)。arena の double-free クラスは 2026-07-02 の audit(#270–#277 の一連)で閉じた — cleanup は
どの exit 経路でも arena ごとにちょうど一度だけ走る。

## 仕様先行(未実装)

- box における **struct / sum 型 / 所有権付きペイロード**;box を param/return/field にすること。M3 の切り
  出しはスカラー・arena ローカルである。ペイロード集合を広げるのは実際の設計作業であり(boxed な所有値の
  drop、Move との region 相互作用)、機械的な拡張ではない。
- box の `.set()` / 変更表面 — それを要求するユースケースがない。今日の box は「一度計算してローカルに
  読む」ものである。汎用の heap セルへ育てるなら、`mut` ローカルや arena 値との One-way レビューが要る —
  それらがすでにパターンをカバーしている。
- escape する box(arena を越えて生き延びる box / グローバルな heap 層)— 意図的に不在である。所有権
  モデルにおける「より長生き」への答えは、box のライフタイムではなく Move 型である。

## Pitfalls

- P1 — **unnecessary-heap lint**(#323)は、`.get()` で読まれるだけで一度も escape しない box に対して発火
  する。ガイドはこの lint を norm として教えている。ある変更でこの lint が idiomatic なコードに発火するよう
  になったなら、間違っているのは lint ではなく変更のほうである。
- P2 — `heap.new(x).get()` を束縛なしのチェーンとして書けるのは、annotation が通り抜けるからにすぎない
  (`v: i32 := heap.new(7).get()` は pin されたテスト)。ここから束縛なしの Move 一時値パターンを一般化して
  はならない — box は Move ではなく region 追跡型であり、*だからこそ* 合成できる。
- P3 — arena ブロックはネストする。escape チェックを駆動するのは region depth の比較であって、ブロックの
  同一性ではない。スコープを開く新しい構文(例えば将来の `soa` ビルダー)は、並列な機構を足すのではなく
  depth と統合しなければならない。
- P4 — 新しいライブラリの確保を arena の外へ回さないこと(ランタイムヘルパーに隠れた `malloc`)。ユーザー
  コードのための、ランタイムでのあらゆる確保は arena・オーナー・ビルダーのいずれかに属する —「nothing
  hidden」の audit 面がそれに依存している。

## Test anchors

`m3.rs`(box の construct/get/clone、annotation の通り抜け、arena 要件);
`enum_match.rs:210`(`heap.new(C.R)` が panic ではなく拒否される);`mmv2.rs` にまたがる escape/region
テスト + #270–#277 の audit 修正スイート(arena double-free、exit 経路);
`lint_unnecessary_heap.rs`;例 `arena.align`(42 で exit)、ガイド ch05。
