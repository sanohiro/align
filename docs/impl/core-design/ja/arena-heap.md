このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同等の粒度（シグネチャ、Move/effect の分類、エラー方針、落とし穴（Pitfalls）、テストアンカー）で記述された公式な設計ドキュメントを収めている。
執筆はメインループ（Fable）が担当している。

# core — arena / heap.new / box

> 🌐 [English](../arena-heap.md) · **日本語**

## Overview

ユーザーから見えるメモリアロケーションの場所は 2 つある（draft §6）。`arena {}`（バッチのライフタイム）と、単一の明示的なアロケーションで arena に常駐する `heap.new` である。メモリモデルのそれ以外の部分（Move 型、region、drop）は言語自体の機能であり、ライブラリではない。このファイルでは、ライブラリとして提供されている機能の表面と、現時点で意図的に厳しく制限されている仕様について扱う。

## Signatures (verified)

```text
arena { … }                 // expression block; all arena allocations freed at }, O(1)
heap.new(x)   -> box<T>     // ONE arg; must be inside an arena {}; T = primitive scalar only
b.get()       -> T          // copy the payload out
b.clone()     -> box<T>     // deep-copy the box; both remain valid
```

box の表面はこれで全部である — `.set()` も deref 演算子もない。

## Type & ownership classification

- `box<T>` は region 追跡される `Arena(depth)` データである。汎用のオーナー（所有者）では **ない**。関数の引数や戻り値型にはできず（「boxes are arena-local in M3」/「would escape its arena」）、array/slice の要素にも、Option/Result のペイロードにもできない。
- ペイロードのホワイトリスト: int/float/bool/char。それ以外は個別の診断メッセージを出して拒否する。所有権付きの Move 値（「an owned `…` cannot be boxed」）、struct、sum 型、`str` ビュー（`box<str>` は型解決の段階でも拒否）。
- `arena` は式である。その末尾値は region-free の場合（または clone によってスコープ外へ出された場合）に限り escape できる。

## Effects

Pure。arena の確保はバンプアロケーションであり、ここでは OS との直接的なやり取りは発生しない（arena のバッキングページは、検証済みの `noalias`/`nounwind` 属性を持つランタイムアロケータから提供される、#301）。

## Errors & aborts

ランタイムでのエラーはない（アロケーションの失敗は、すべてのランタイムアロケーションで共有されるプロセスレベルの abort 経路である）。
それ以外はすべてコンパイルエラーとなる。arena の外での `heap.new`、arena に region 付けされた値の escape（`cannot return a value allocated in an arena`）、非スカラー値の boxing が該当する。

## Regions

region モデルの参照実装である: `region_of(box) = Arena(depth)`、`region_of(b.get()) = none`（スカラーコピー）。arena における二重解放（double-free）のクラスは 2026-07-02 の audit（#270–#277 の一連の対応）で解決済みであり、cleanup はどの exit 経路でも arena ごとにちょうど一度だけ実行される。

## 仕様先行(未実装)

- box における **struct / sum 型 / 所有権付きペイロード**、また box を param/return/field にすること。M3 でのスコープはスカラーかつ arena ローカルに限定されている。ペイロードの対象を広げるのは実際の設計作業であり（boxed な所有値の drop、Move との region 相互作用の考慮）、機械的な拡張では済まない。
- box の `.set()` や変更操作の表面 — 現在のところそれを要求するユースケースがない。今日の box は「一度計算してローカルに読む」ためのものである。これを汎用的なヒープのセルに拡張するには、すでにパターンをカバーしている `mut` ローカルや arena 値との競合を避けるための「One-way（一方向）」レビューが必要になる。
- escape する box（arena の寿命を越えて生き延びる box / グローバルな heap 層）— 意図的に提供していない。所有権モデルにおける「より長生きする値」への答えは Move 型であり、box のライフタイムで解決するべきではない。

## Pitfalls

- P1 — **unnecessary-heap lint**（#323）は、`.get()` で読み取られるだけで一度も escape しない box に対して発火する。ガイドではこの lint を標準的なものとして教えている。もしある変更によってこの lint が idiomatic なコードに対して発火するようになったなら、間違っているのは lint ではなく変更のほうである。
- P2 — `heap.new(x).get()` は arena に region 付けされたものであり、Move の合成的なオーナー（synthetic-owner）のケースではない（`v: i32 := heap.new(7).get()` は固定されたテストである）。一般的な束縛なしの Move のクリーンアップも現在では合成可能になっているが、それはパスローカルな所有権フラグを通じたものである。これら 2 つのライフタイム機構を混同してはならない。
- P3 — arena ブロックはネストする。escape チェックを駆動するのは region depth の比較であり、ブロックの同一性ではない。スコープを開く新しい構文（例えば将来の `soa` ビルダー）は、並行する新しい機構を追加するのではなく、depth の仕組みと統合しなければならない。
- P4 — 新しいライブラリのアロケーションを arena を迂回して行わないこと（ランタイムヘルパー内に隠れた `malloc` など）。ユーザーコードのために実行されるランタイムでのすべてのアロケーションは、arena、オーナー、またはビルダーのいずれかに属していなければならない。「何も隠されていない（nothing hidden）」という audit 表面はこれに依存している。

## Test anchors

`m3.rs`（box の construct/get/clone、annotation の伝播、arena 要件）。
`enum_match.rs:210`（`heap.new(C.R)` が panic ではなく拒否される）。`mmv2.rs` にまたがる escape/region のテスト、および #270–#277 の audit 修正スイート（arena double-free、exit 経路）。
`lint_unnecessary_heap.rs`。例 `arena.align`（42 で exit）、ガイド ch05。
