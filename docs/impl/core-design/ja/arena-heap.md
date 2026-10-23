# core — arena / heap.new / box

> 🌐 [English](../arena-heap.md) · **日本語**

## Overview

`arena {}`、`heap.new` が確保するスカラーボックス、およびそれらの region と escape の
規則を扱う（draft §6）。arena は複数の確保領域を同じ寿命にまとめる。`heap.new` は
その arena 内にスカラーのボックスを1つ確保する。通常の所有する string、array、buffer
には、それぞれの確保と Drop の規則がある。

## Signatures (verified)

```text
arena { … }                 // expression block; release its allocations and mappings at }
heap.new(x)   -> box<T>     // ONE arg; must be inside an arena {}; T = primitive scalar only
b.get()       -> T          // copy the payload out
b.clone()     -> box<T>     // deep-copy the box; both remain valid
```

`out` はcontextualである。`out: region` はcapability parameter名、`out dst: slice<T>` は
out-mode parameterで、whitespaceではなくtoken lookaheadで区別する。

arena の終了時には、各 backing chunk と登録済みのファイルマッピングを解放する。
バンプ確保した値ごとに解放するわけではないが、処理量は chunk と mapping の数に応じて
増えるため、一般に O(1) ではない。

box には `.set()` メソッドも参照外し演算子もない。

名前付き region の形式（必須L4、**実装済み**）:

```text
arena out { … }             // 同じ arena を `out: region` として束縛する
fn f(out: region)           // 通常の呼び出しを通して同じ arena を伝播する
v.clone_in(out)             // view を名前付き arena へコピーする
```

名前付き arena form は
`box<T>` を拡張するものではない。`out` は通常のライブラリ関数へ渡せる、
スコープ限定のアロケーション先 capability である。匿名 arena と名前付き arena
は同じ begin/end と一括 cleanup を使う。完全な契約は
[`../../17-library-boundary-prerequisites.md`](../../17-library-boundary-prerequisites.md) §4
にある。

## Type & ownership classification

- `box<T>` は region 追跡される `Arena(depth)` データである。汎用のオーナー（所有者）では **ない**。関数の引数や戻り値型にはできず（「boxes are arena-local in M3」/「would escape its arena」）、array/slice の要素にも、Option/Result のペイロードにもできない。
- ペイロードのホワイトリスト: int/float/bool/char。それ以外は個別の診断メッセージを出して拒否する。所有権付きの Move 値（「an owned `…` cannot be boxed」）、struct、sum 型、`str` ビュー（`box<str>` は型解決の段階でも拒否）。
- `arena` は式である。その末尾値は region-free の場合（または clone によってスコープ外へ出された場合）に限り escape できる。

## Effects

Pure。arena の確保と解放はメモリ管理であり、言語上の I/O effect には含まれない。
ランタイムのアロケータが backing chunk を供給し、個々の確保は通常、その chunk 内の
バンプカーソルを進める。

## Errors & aborts

ランタイムでのエラーはない（アロケーションの失敗は、すべてのランタイムアロケーションで共有されるプロセスレベルの abort 経路である）。
それ以外はすべてコンパイルエラーとなる。arena の外での `heap.new`、arena に region 付けされた値の escape（`cannot return a value allocated in an arena`）、非スカラー値の boxing が該当する。

## Regions

region モデルの参照実装である: `region_of(box) = Arena(depth)`、`region_of(b.get()) = none`（スカラーコピー）。arena における二重解放（double-free）のクラスは 2026-07-02 の audit（#270–#277 の一連の対応）で解決済みであり、cleanup はどの exit 経路でも arena ごとにちょうど一度だけ実行される。

## 仕様先行(未実装)

- box における **struct / sum 型 / 所有権付きペイロード**、また box を param/return/field にすること。M3 でのスコープはスカラーかつ arena ローカルに限定されている。ペイロードの対象を広げるのは実際の設計作業であり（boxed な所有値の drop、Move との region 相互作用の考慮）、機械的な拡張では済まない。
- box の `.set()` や変更操作の表面 — 現在のところそれを要求するユースケースがない。今日の box は「一度計算してローカルに読む」ためのものである。これを汎用的なヒープのセルに拡張するには、すでにパターンをカバーしている `mut` ローカルや arena 値との競合を避けるための「One-way（一方向）」レビューが必要になる。
- escape する box（arena の寿命を越えて生き延びる box / グローバルな heap 層）— 意図的に提供していない。所有権モデルにおける「より長生きする値」への答えは Move 型であり、box のライフタイムで解決するべきではない。

（名前付き `region` はもうこのリストにはない: `arena out {}`、`out: region` parameter、通常の関数呼び出しを通した正確な arena の伝播、escape / storage / task / FFI の拒否、`clone_in(out)` はいずれも実装済みである — F-B の region materialization は L4 および L6 まで完了している。）

## Pitfalls

- P1 — **unnecessary-heap lint**（#323）は、`.get()` で読むだけで escape しない box を指摘し、スカラー値を直接使うよう勧める。box の lowering を変えるときも、この検出を維持する。
- P2 — `heap.new(x).get()` は arena に region 付けされたものであり、Move の合成的なオーナー（synthetic-owner）のケースではない（`v: i32 := heap.new(7).get()` は固定されたテストである）。一般的な束縛なしの Move のクリーンアップも現在では合成可能になっているが、それはパスローカルな所有権フラグを通じたものである。これら 2 つのライフタイム機構を混同してはならない。
- P3 — arena ブロックはネストする。escape チェックを駆動するのは region depth の比較であり、ブロックの同一性ではない。スコープを開く新しい構文（例えば将来の `soa` ビルダー）は、並行する新しい機構を追加するのではなく、depth の仕組みと統合しなければならない。
- P4 — 新しいライブラリのアロケーションを arena を迂回して行わないこと（ランタイムヘルパー内に隠れた `malloc` など）。ユーザーコードのために実行されるランタイムでのすべてのアロケーションは、arena、オーナー、またはビルダーのいずれかに属していなければならない。「何も隠されていない（nothing hidden）」という audit 表面はこれに依存している。

## Test anchors

`m3.rs`（box の construct/get/clone、annotation の伝播、arena 要件）。
`enum_match.rs:210`（`heap.new(C.R)` が panic ではなく拒否される）。`mmv2.rs` にまたがる escape/region のテスト、および #270–#277 の audit 修正スイート（arena double-free、exit 経路）。
`lint_unnecessary_heap.rs`。例 `arena.align`（42 で exit）、ガイド ch05。
