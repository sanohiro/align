このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同じ粒度(シグネチャ、
Move/effect の分類、エラー方針、pitfalls、テストアンカー)の正典となる設計ドキュメントを収めている。
執筆はメインループ (Fable) が担当している。

# core — hash (+ scalar math)

> 🌐 [English](../hash.md) · **日本語**

## Overview

byte ビューに対する非暗号ハッシュである(draft §18.1): 正典のミキサーは一つ(wyhash、`align_hash`
crate)、free-function の表面も一つ。**`Hash` トレイトは意図的に無い** — ハッシュするのは任意の値では
なく byte であり、値の byte ビューをどう作るかは呼び出し側の明示的な仕事である。このファイルは、スカラー
数学のメソッド表面も併せて記録する(独立したファイルを立てるほどの大きさではない)。

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

すべて Copy in、Copy out。`hash*` はビューを借用する(所有権付きの `string` はそのまま使い続けられる —
`print` の前例)。pairwise な `a.min(b)` は、配列 reduction の `arr.min()` と arity で共存する。

## Effects

Pure。ハッシュ結果は、与えられた入力に対して **1 つのビルド内では** 決定的(固定 seed)である — `par_map`
で安全に使える。

## Errors & aborts

なし。byte ビューでない `hash64(5)` や arity 違いは型エラー、int への `sqrt` は拒否、unsigned の `.abs()` は
エラーではなく定義された identity である。

## Regions

なし — 値が入り、値が出る。

## 仕様先行(未実装)

- **契約には含まれない:** *ビルド/バージョン* をまたいだハッシュ出力の安定性 — 固定 seed の wyhash は
  今日たまたま安定しているが、§18.1 はこれを「安定した on-disk/wire フォーマットではない」と明示的に
  scope している。テストやユーザーが正確なハッシュ値を API として pin することを許してはならない
  (ドライバのテストは *プロパティ* を pin する: 決定性、lane の等価性、ビューの受理)。
- DoS 耐性なし、暗号用途でもない — セキュリティ文脈の答えは `std.crypto`(M11、`../std-design/crypto.md`
  で設計済み)である。そこで「hash64 で済ませる」近道は拒否すること。
- 超越関数(`sin`/`cos`/`log`/`exp`)なし — `MathFn` enum は `pow`/`fma` で止まる。追加は機械的には
  小さいが、精度/libm 依存のスタンスは関数ごとではなく `open-questions.md` で一度決めること。
- `core.bitset`(§18.1 の隣人)— 未実装。ビット操作は整数演算子で行う。そのレイアウト問題は packed-bool
  soa カラムと結びついている。一緒に決着させること。

## Pitfalls

- P1 — 内部の wyhash(group_by の interning、dict_encode、JSON PHF — #321 でこれらを `align_hash` に
  収束させた)と、ユーザー向けの `hash64` は **意図的に同じミキサー** である。構造的な振る舞い(例えば PHF
  の byte-match)が、ユーザーの計算できるハッシュとは *別の* ハッシュに依存してはならない。ミキサーは
  一箇所で変えるか、変えないかのどちらかである。
- P2 — `hash128.0 == hash64` は pin されたプロパティである。将来のミキサーがこの lane 関係を壊すなら、
  それは内部の細部ではなく API break である。
- P3 — float の `min`/`max` は NaN-*伝播*(`llvm.minimum/maximum` であって `minnum/maxnum` ではない)で
  ある。vec lane の `min`/`max` はスカラーの選択と一致し続けなければならない — lane/scalar の乖離こそ、
  differential fuzzer が存在する理由であるバグクラスである。

## Test anchors

`crates/align_driver/tests/hash.rs`(決定性、lane の等価性、所有権付き string の借用、非ビューの拒否、
arity);`scalar_math.rs`(abs/min/max の int+float+unsigned-identity、pairwise と reduction の共存、
超越関数の値、int への sqrt の拒否);#321(ハッシュの収束 + group_by の高速化 pin)。
