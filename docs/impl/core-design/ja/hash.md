このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同等の粒度（シグネチャ、Move/effect の分類、エラー方針、落とし穴（Pitfalls）、テストアンカー）で記述された公式な設計ドキュメントを収めている。
執筆はメインループ（Fable）が担当している。

# core — hash (+ scalar math)

> 🌐 [English](../hash.md) · **日本語**

## Overview

バイトビュー（byte views）に対する非暗号学的ハッシュについて（draft §18.1）: 標準のミキサーは 1 つ（wyhash、`align_hash` クレート）であり、フリー関数として提供されるインターフェースも 1 つだけである。**`Hash` トレイトは意図的に存在しない** — ハッシュ化の対象は任意の値ではなくバイト列であり、ある値からどのようにバイトビューを生成するかは呼び出し側が明示的に管理すべき事柄である。
なお、このファイルにはスカラー演算（scalar math）メソッドのインターフェースも併記している（単独のファイルにするほどの規模ではないため）。

## Signatures (verified)

```text
hash64(data)   -> u64            // data: str | string (auto-borrowed, not consumed) | slice<u8>
hash128(data)  -> (u64, u64)     // .0 == hash64(data); .1 = decorrelated second lane (no u128 type)

// scalar math — intrinsic methods on numeric values (no import, no core.math module):
x.abs()                          // signed int / float; identity on unsigned
a.min(b) / a.max(b)              // pairwise; int = llvm.{s,u}min/max, float = NaN-propagating minimum/maximum
x.sqrt() / .floor() / .ceil() / .round() / .trunc()   // float-only; round = ties away from zero
b.pow(e)                         // float-only
fma(a, b, c)                     // free builtin; float scalar or float vector; one rounding
```

## Type & ownership classification

すべて Copy in、Copy out（値渡しによる入力と出力）となる。`hash*` はビューを借用する（所有権付きの `string` を渡しても消費されず、そのまま使い続けられる — `print` の前例と同様）。ペアワイズの `a.min(b)` は、配列の要素をリダクションする `arr.min()` とアリティ（引数の数）の違いによって共存する。

## Effects

Pure。ハッシュ結果は、与えられた入力に対して **1 つのビルド内では** 決定的（シード値固定）となるため、`par_map` でも安全に使用できる。

## Errors & aborts

なし。バイトビューではない `hash64(5)` の呼び出しやアリティの違いは型エラーとなる。int に対する `sqrt` は拒否される。unsigned に対する `.abs()` の呼び出しはエラーではなく、元の値をそのまま返す恒等関数（identity）として定義されている。

## Regions

なし — 値が入力され、値が出力されるのみ。

## 仕様先行(未実装)

- **契約には含まれない事項:** *ビルド間やバージョン間* でのハッシュ出力の安定性 — 固定シードを持つ wyhash は現状たまたま安定しているが、§18.1 ではこれを明示的に「オンディスクやワイヤーフォーマットとして安定したものではない」と定めている。テストやユーザー側で、特定の正確なハッシュ値を API の仕様として固定（pin）することを許してはならない（ドライバのテストで固定するのは、決定性、レーンの等価性、ビューの受理といった *プロパティ* のみである）。
- DoS 耐性はなく、暗号用途でもない — セキュリティが要求される文脈での正解は `std.crypto`（M11、`../std-design/crypto.md` で設計済み）である。そのような場面で「手軽に hash64 で済ませる」という方法は拒否すること。
- 超越関数（`sin`/`cos`/`log`/`exp`）は未提供 — `MathFn` enum は `pow`/`fma` で打ち止めとなっている。追加自体の作業コストは小さいが、精度や libm への依存に対する方針は、関数ごとではなく `open-questions.md` で一括して決めること。
- `core.bitset`（§18.1 に隣接する要素） — 未実装。ビット操作は整数演算子で行う。そのメモリレイアウトの問題は packed-bool な soa カラムと密接に関連しているため、一緒に決着させること。

## Pitfalls

- P1 — 内部で使われる wyhash（group_by の interning、dict_encode、JSON の PHF — #321 でこれらを `align_hash` に収束させた）と、ユーザー向けの `hash64` は **意図的に同じミキサー** に統一されている。構造的な振る舞い（例えば PHF のバイトマッチング）が、ユーザーが計算できるハッシュとは *異なる* ハッシュに依存してはならない。ミキサーを変更するなら一元的に変更すべきであり、部分的な変更は許されない。
- P2 — `hash128.0 == hash64` は固定されたプロパティである。もし将来のミキサーがこのレーン関係を壊すようなことがあれば、それは内部実装の変更ではなく API の破壊的変更（API break）を意味する。
- P3 — float の `min`/`max` は NaN-*伝播（propagating）*（`minnum/maxnum` ではなく `llvm.minimum/maximum` を使用）である。ベクタレーンの `min`/`max` もスカラーの挙動と一致し続けなければならない — レーンとスカラーでの挙動の乖離はまさに、differential fuzzer が検出を目的としているバグクラスである。

## Test anchors

`crates/align_driver/tests/hash.rs`（決定性、レーンの等価性、所有権付き string の借用、非ビューの拒否、アリティ）。`scalar_math.rs`（abs/min/max の int+float+unsigned における identity 挙動、ペアワイズとリダクションの共存、超越関数の値、int に対する sqrt の拒否）。#321（ハッシュの収束と group_by の高速化の固定）。
