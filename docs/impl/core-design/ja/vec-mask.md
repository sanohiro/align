このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同じ粒度(シグネチャ、
Move/effect の分類、エラー方針、pitfalls、テストアンカー)の正典となる設計ドキュメントを収めている。
執筆はメインループ (Fable) が担当している。

# core — vecN / maskN / align(N)

> 🌐 [English](../vec-mask.md) · **日本語**

## Overview

明示的な固定幅 SIMD 層である(draft §9)。すべての API 選択を形作るので、まず方針から述べる。**pipeline が
幅に依存しないメインパス**(auto-vectorize され、スケーラブル ISA に対応)であり、`vecN<T>` / `maskN<T>` は
**固定幅カーネルの escape hatch** である。MIR が運ぶのは vectorization を可能にする *プロパティ* であって、
焼き込まれたベクタ幅ではない — ベクタ幅は恒久的にバックエンドの決定事項である(settled、2026-07-02 の内部
レビュー)。この領域のいかなるものも、幅の仮定をメインパスへ漏らしてはならない。

## Signatures (verified)

```text
v: vecN<T> := [a, b, ...]        // N ∈ {2,4,8,16}; T numeric; literal under annotation
v + w, v - w, v * w, v / w, v % w    // lane-wise, one instruction each
v + s / s + v                        // scalar literal broadcasts (either side)
v == w, v > w, v < w, ...        -> maskN<T>
v[i]                             -> T            // lane read, constant index
v[i] = x                                          // lane write (mut binding)
v.min() / v.max()                -> T            // horizontal reduce
a.min(b) / a.max(b)              -> vecN<T>      // element-wise
v.sqrt()/abs()/floor()/ceil()/round()/trunc()    // per-lane float math
dot(a, b)                        -> T
fma(a, b, c)                     -> vecN<T>      // one rounding
select(m, a, b)                  -> vecN<T>      // lane blend
v.sum_where(m)                   -> T            // masked reduction

s.load(i)                        -> vecN<T>      // N consecutive slice elems; bounds-checked
s.store(i, v)                                     // through an out/mut slice; bounds-checked

align(N) xs := [...]                              // over-align array storage (power of two)
align(N) Struct { ... }                           // over-align struct; stride padded to N
```

## Type & ownership classification

`vecN<T>` と `maskN<T>` は **Copy なスカラークラスの値**(レジスタサイズの集約)である。自由に渡し、返し、
格納してよい。move/drop/escape の経路には決して乗らない。`maskN<T>` は名前を付けられる(annotation、
param、return)。`align(N)` は型ではなく属性であり、`layout(C)` とはどちらの順でも合成できる。

## Effects

すべて Pure。vec カーネルは `par_map` 適格かつ pipeline-lambda 適格である。

## Errors & aborts

lane のセマンティクスは **スカラーのセマンティクスと同一** である — これは最適化の細部ではなく hard な
不変条件である。整数 lane はオーバーフローで wrap し、lane の除算 0 は **abort**(同じ `align_rt_div_fail`
ガードを lane 単位でチェック)、`INT_MIN / -1` は wrap、float lane は IEEE。`load`/`store` の範囲外は abort。
いかなる lane にも UB は決してない(#294/#318 で vec-div の残件を閉じた)。

## Regions

なし — Copy な値である。`load` は一瞬だけスライスを借用し、`store` は書き込み可能(`mut`/`out`)なスライス
を要求する。region が絡むのは境界のスライス経由のみである。

## 仕様先行(未実装)

- **`bitset`**(§18.1 カタログ)— 実装なし、テストなし。設計は未定: packed-bool soa カラム(post-M6
  backlog)との関係を一緒に決めるべきである。
- スカラー **変数** の broadcast は、仕様本文が示唆するより狭い。リテラルの broadcast(`v * 2`、`10 + v`)は
  検証済みだが、スカラーの *束縛* を lane 演算へ broadcast するのは拒否されるのを確認している
  (「type mismatch: f64 vs vec4<f64>」)— splat 形が実装されるまでは、明示的な splat ベクタかリテラルで
  書くこと。(ガイド ch12 は検証済みのサブセット内で書かれている。)
- 関数境界を越えた aligned-load の伝播(関数をまたいで証明可能に aligned なスライス)— 保留。今日は
  ローカルに証明可能な alignment だけが load を格上げする(#320)。

## Pitfalls

- P1 — **幅ジェネリックな `vec<T>` を追加しないこと**: 二層構成は settled。幅に依存しないものは pipeline に
  属し、そこでバックエンドが lane を選ぶ。
- P2 — **手で vectorize する前に audit せよ**: まず pipeline 版に `emit-llvm` をかける。たいていの場合、
  fused ループはすでに vectorize されている。カーネルはスライス境界の関数の後ろに置き
  (`fn kernel(src: slice<T>, out dst: slice<T>)`)、スカラーの端数は呼び出し側が `chunks(N)` でさばく。
- P3 — `align(N)` は常に *over*-align するだけであり、動的な `array<align(N) S>` は aligned な heap 確保が
  入るまで拒否のまま(#319)— この属性は汎用のアロケータ指示子ではない。
- P4 — mask の要素型は比較対象のベクタと一致していなければならない(`vec4<i32>` の比較からは
  `mask4<i32>`)。幅をまたぐ、あるいは型をまたぐ mask の再利用は存在しない。

## Test anchors

`examples/vec_simd.align`、`vec_mask.align`、`vec_mask_annot.align`、`vec_broadcast.align`、
`vec_sum_where.align`、`vec_minmax.align`、`vec_math.align`、`vec_fma.align`、`vec_dot.align`、
`vec_load_store.align`、`vec_lane_set.align`、`aligned_load.align`、`align_attr.align`;#318 まわりの vec
lane-`%`/div-guard テスト;differential fuzzer の lane-arith 拡張(#326)。M6 の完了 pin: 本物の
`<N x T>` IR + すべての reducer に対する branchless な `where`(#303, #327)。
