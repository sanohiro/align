このディレクトリは `core` ライブラリの各領域の設計仕様をまとめる。
シグネチャ、所有権と effect の分類、エラー方針、注意点、関連テストを記載する。

# core — hash (+ scalar math)

> 🌐 [English](../hash.md) · **日本語**

## Overview

バイトビューに対する非暗号学的ハッシュ（draft §18.1）は、`align_hash` クレートの
wyhash を使う。呼び出し側がバイトビューを明示的に渡し、任意の値を対象とする
`Hash` トレイトは設けない。このファイルにはスカラー演算のメソッドも記載する。

## Signatures (verified)

```text
hash64(data)   -> u64            // data: str | string (auto-borrowed, not consumed) | slice<u8>
hash128(data)  -> (u64, u64)     // .0 == hash64(data); .1 = decorrelated second lane (no u128 type)

// scalar math — intrinsic methods on numeric values (no import, no core.math module):
x.abs()                          // signed int / float; identity on unsigned
a.min(b) / a.max(b)              // pairwise; int = llvm.{s,u}min/max, float = NaN-propagating minimum/maximum
x.sqrt() / .floor() / .ceil() / .round() / .round_even() / .trunc()   // float-only; round = ties away from zero
x.exp() / .exp2() / .log() / .log2() / .log10()     // float 専用; float vector にもレーン単位で適用
x.expm1() / .log1p() / .sin() / .cos(), y.atan2(x) // 同じ float scalar/vector 型; atan2 は y, x の順
b.pow(e)                         // float-only
fma(a, b, c)                     // free builtin; float scalar or float vector; one rounding
```

## Type & ownership classification

すべて Copy in、Copy out（値渡しによる入力と出力）となる。`hash*` はビューを借用する（所有権付きの `string` を渡しても消費されず、そのまま使い続けられる — `print` の前例と同様）。ペアワイズの `a.min(b)` は、配列の要素をリダクションする `arr.min()` とアリティ（引数の数）の違いによって共存する。

`round_even()` は `f32`・`f64` とそれらのベクトルに対する引数なしのメソッドで、
同じ型の整数値へ最近接丸めし、中間値は偶数側へ丸める。符号付きゼロ
（`-0.5 -> -0.0` を含む）、無限大、すでに整数である有限値を保持する。
NaN は NaN のままだが、ペイロード・符号・quiet 化は保証しない。
abort や割り当ては発生せず、ベクトルでは各レーンに独立に適用する。
従来の `round()` は中間値をゼロから遠い側へ丸める。

## Effects

Pure。ハッシュ結果は、与えられた入力に対して **1 つのビルド内では** 決定的（シード値固定）となるため、`par_map` でも安全に使用できる。

## Errors & aborts

なし。バイトビューではない `hash64(5)` の呼び出しやアリティの違いは型エラーとなる。int に対する `sqrt` は拒否される。unsigned に対する `.abs()` の呼び出しはエラーではなく、元の値をそのまま返す恒等関数（identity）として定義されている。

## Regions

なし — 値が入力され、値が出力されるのみ。

## 提供状況と制限

- **契約には含まれない事項:** *ビルド間やバージョン間* でのハッシュ出力の安定性 — 固定シードを持つ wyhash は現状たまたま安定しているが、§18.1 ではこれを明示的に「オンディスクやワイヤーフォーマットとして安定したものではない」と定めている。テストやユーザー側で、特定の正確なハッシュ値を API の仕様として固定（pin）することを許してはならない（ドライバのテストで固定するのは、決定性、レーンの等価性、ビューの受理といった *プロパティ* のみである）。
- DoS 耐性はなく、暗号用途でもない — 実装済みの [`std.crypto`](../../std-design/crypto.md) が暗号操作を提供する。セキュリティが要求される場面で「手軽に hash64 で済ませる」という方法は拒否すること。
- 初等関数 `exp`/`exp2`/`log`/`log2`/`log10` は plan 75 に従って実装済み。LLVM のスカラー／ベクタ intrinsic へ直接変換する。全入力に対する ULP 上限やターゲット間のビット一致は保証しない。命令選択前のスカラー化は診断に表示するが、ベクタ IR が残っていても後段でスカラー化され得る。最終的な SIMD 命令は出力オブジェクトを検査して確認する。
- `expm1`/`log1p` は各値の幅に一致するスカラー libm 関数を呼び、ベクタでは各レーンに適用する（[Plan234](../../234-cancellation-sensitive-math.md)）。`sin`/`cos`/`atan2` は LLVM のスカラー／ベクタ intrinsic を使う（[Plan235](../../235-trigonometric-math.md)）。すべて Pure で Align の領域を確保せず、精度とベクタ化には上記の制限がある。`atan2` の両辺は同じ型で、スカラーのブロードキャストは行わない。結果は [-pi, pi] のラジアン値になる。
- `core.bitset`（§18.1 に隣接する要素） — 未実装。ビット操作は整数演算子で行う。そのメモリレイアウトの問題は packed-bool な soa カラムと密接に関連しているため、一緒に決着させること。

## Pitfalls

- P1 — 内部で使われる wyhash（group_by の interning、dict_encode、JSON の PHF — #321 でこれらを `align_hash` に収束させた）と、ユーザー向けの `hash64` は **意図的に同じミキサー** に統一されている。構造的な振る舞い（例えば PHF のバイトマッチング）が、ユーザーが計算できるハッシュとは *異なる* ハッシュに依存してはならない。ミキサーを変更するなら一元的に変更すべきであり、部分的な変更は許されない。
- P2 — `hash128.0 == hash64` は固定されたプロパティである。もし将来のミキサーがこのレーン関係を壊すようなことがあれば、それは内部実装の変更ではなく API の破壊的変更（API break）を意味する。
- P3 — float の `min`/`max` は NaN を伝播する（`llvm.minimum/maximum` を使用）。ベクタの各レーンもスカラーと同じ規則に従い、差分ファザーで一致を検査する。

## Test anchors

`crates/align_driver/tests/hash.rs`（決定性、レーンの等価性、所有権付き string の借用、非ビューの拒否、アリティ）。`scalar_math.rs`（abs/min/max の int+float+unsigned における identity 挙動、ペアワイズとリダクションの共存、超越関数の値、int に対する sqrt の拒否）。#321（ハッシュの収束と group_by の高速化の固定）。
