このディレクトリは `core` ライブラリの各領域の設計仕様をまとめる。
シグネチャ、所有権と effect の分類、エラー方針、注意点、関連テストを記載する。

# core — vecN / maskN / align(N)

> 🌐 [English](../vec-mask.md) · **日本語**

## Overview

明示的な固定幅 SIMD 層である（draft §9）。ここでの方針はすべての API 選択の基盤となるため、まずはその方針から述べる。**パイプライン処理こそが幅に依存しないメインパス**（自動ベクタ化の対象であり、将来のスケーラブルな ISA にも幅を固定せず対応できる）であり、`vecN<T>` / `maskN<T>` は **固定幅カーネルの escape hatch（避難口）** である。MIR が保持するのは vectorization を可能にするための *プロパティ* であり、ハードコードされたベクタ幅ではない — ベクタ幅は最終的にバックエンドが決定する事項である（決定済、2026-07-02 の内部レビュー）。この領域のいかなる実装も、ベクタ幅に関する仮定をメインパスへと漏らしてはならない。

## Signatures (verified)

```text
v: vecN<T> := [a, b, ...]        // N ∈ {2,4,8,16}; T numeric; literal under annotation
v + w, v - w, v * w, v / w, v % w    // レーン単位; 命令選択はターゲット依存
v + s / s + v                        // scalar broadcasts (either side; literal or typed binding)
v == w, v > w, v < w, ...        -> maskN<T>
v[i]                             -> T            // lane read, constant index
v[i] = x                                          // lane write (mut binding)
v.min() / v.max()                -> T            // horizontal reduce
a.min(b) / a.max(b)              -> vecN<T>      // element-wise
v.sqrt()/abs()/floor()/ceil()/round()/trunc()    // per-lane float math
v.exp()/exp2()/log()/log2()/log10()             // レーン単位の float math; scalarize され得る
dot(a, b)                        -> T
fma(a, b, c)                     -> vecN<T>      // one rounding
select(m, a, b)                  -> vecN<T>      // lane blend; a and b are BOTH vectors (no broadcast)
                                                  // m: レーン数とレーン幅が一致する任意の mask
v.sum_where(m)                   -> T            // masked reduction; 同じ構造的 mask 規則

s.load(i)                        -> vecN<T>      // N consecutive slice elems; bounds-checked
s.store(i, v)                                     // through an out/mut slice; bounds-checked

align(N) xs := [...]                              // over-align array storage (power of two)
align(N) Struct { ... }                           // over-align struct; stride padded to N
```

## Type & ownership classification

`vecN<T>` と `maskN<T>` は **Copy 可能なスカラークラスの値**（レジスタサイズの集約データ）である。自由に渡し、返し、格納することができる。move / drop / escape の経路には決して乗らない。`maskN<T>` は型として名前を指定できる（アノテーション、引数、戻り値など）。mask の要素型は名前を付けるためのものであり、機械表現は `<N x i1>` である。`select` と `sum_where` は対象ベクタとレーン数・レーンのビット幅が一致する mask を受け取る。この判定は `align_sema::mask_gates_vector` に集約され、sema、checked-HIR record validator、MIR producer validator が共有する。

`align(N)` は型ではなく属性であり、`layout(C)` と組み合わせる場合どちらの順序でも合成可能である。

一つのベクタ演算が一つの機械命令になるとは限らない。LLVM の命令選択はレーン型・幅・
ターゲットに依存し、ベクタ命令、複数命令、スカラーコードのいずれにもなり得る。
上記の五つの初等関数は LLVM へ vector intrinsic として渡る。
`emit-llvm --stage optimized` と `explain-opt` で命令選択前の状態を確認する。
provider のない retained vector IR は後段でも scalarize され得るため、最終的な
machine SIMD は出力 object の検査で確かめる。[Plan 75](../../75-portable-math-plan.md) がこの境界を定める。

## Effects

すべて Pure。vec カーネルは `par_map` 適格であり、パイプラインラムダの適格性も満たす。

## Errors & aborts

レーンごとのセマンティクスは **スカラーのセマンティクスと完全に同一** である — これは単なる最適化の詳細ではなく、厳密な不変条件（hard invariant）である。整数レーンのオーバーフローはラップアラウンドし、レーン内での 0 除算は **abort** を引き起こす（スカラーと同じ `align_rt_div_fail` ガードをレーン単位でチェックする）。`INT_MIN / -1` はラップアラウンドし、float レーンは IEEE 標準に従う。`load` / `store` の範囲外アクセスは abort となる。いかなるレーンの操作にも未定義動作（UB）は存在しない（#294 および #318 で vec-div 関連の残件をクローズ済み）。

## Regions

なし — Copy な値であるため。`load` は一瞬だけスライスを借用し、`store` は書き込み可能（`mut` または `out`）なスライスを要求する。region が関与するのは境界のインターフェースとなるスライス経由のみである。

## 仕様先行(未実装)

- **`bitset`**（§18.1 カタログ） — 実装もテストもない。設計は未定であり、packed-bool な soa カラム（M6 以降のバックログ）との関係性を考慮して一緒に決定すべきである。
- **`select`** のオペランドへのスカラーのブロードキャスト（broadcast）。算術のブロードキャストは完成している — リテラル（`v * 2`、`10 + v`）も、型の付いたスカラー束縛（`s: f64 := 2.0; v * s` / `s * v`）も正しく lowering される。`select` はこれに参加しない。blend する2つのオペランドはどちらも `vecN<T>` でなければならず（`select(m, w, 0)` は「'select' vectors must have the same type, got vec4<i32> and int(undetermined)」となる）、splat 機能が実装されるまで定数側は明示的なベクタとして記述すること。
- 関数境界を越える aligned-load の伝播（関数をまたいでもアライメントが証明可能なスライスの引き渡し） — 保留中。現在はローカルで証明可能なアライメントのみが load 操作を最適化（格上げ）する（#320）。

## Pitfalls

- P1 — **幅ジェネリックな `vec<T>` を追加しないこと**: この二層構成は決定済み（settled）である。幅に依存しない処理はパイプラインに属し、そこでバックエンドが適切なレーン幅を選択する。
- P2 — **手動で vectorize する前に必ず audit すること**: まずパイプライン版のコードに対して `emit-llvm` を実行して確認する。多くの場合、融合（fuse）されたループはすでに自動的に vectorize されている。手動カーネルはスライス境界の関数の後ろに配置し（例: `fn kernel(src: slice<T>, out dst: slice<T>)`）、スカラーとして残る端数の処理は呼び出し側が `chunks(N)` などを用いて行う。
- P3 — `align(N)` は常に *over-align*（要求より厳しいアライメント制約）を指定するだけであり、動的な `array<align(N) S>` はアライメントを考慮した heap アロケーションがサポートされるまで拒否される（#319）。この属性は汎用的なアロケータへの指示子ではない。
- P4 — mask は**構造的に**ゲートする（要素型の一致ではない）: `select` / `sum_where` はレーン数とレーン幅が一致する任意の mask を受け取る（`mask4<f32>` は `vec4<i32>` / `vec4<u32>` / `vec4<f32>` をゲートできる）。これにより argmax / top-k / index compaction を厳密な整数インデックスで記述できる。レーン数またはレーン幅が異なる場合は従来どおり拒否される。比較が生成する mask の *型* は変わらないので、明示的な `maskN<T>` 注釈は比較対象の要素型を書く必要がある（#1083）。

## Test anchors

`examples/vec_simd.align`、`vec_mask.align`、`vec_mask_annot.align`、`vec_broadcast.align`、`vec_sum_where.align`、`vec_minmax.align`、`vec_math.align`、`vec_fma.align`、`vec_dot.align`、`vec_load_store.align`、`vec_lane_set.align`、`aligned_load.align`、`align_attr.align`。#318 周辺の vec lane-`%` / div-guard テスト。differential fuzzer の lane-arith 拡張（#326）。M6 完了時のピン: 本物の `<N x T>` IR + すべての reducer に対する分岐のない（branchless な）`where` の実現（#303, #327）。
