このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同等の粒度（シグネチャ、Move/effect の分類、エラー方針、落とし穴（Pitfalls）、テストアンカー）で記述された公式な設計ドキュメントを収めている。
執筆はメインループ（Fable）が担当している。

# core — array / slice / パイプライン

> 🌐 [English](../array-slice-pipeline.md) · **日本語**

## Overview

データ指向設計の中核（draft §7–§8）。3 つのコレクション形式と 1 つのデータ処理語彙からなる。
以下のステージはすべて単一のカウント付きループへと融合（fuse）される。パイプラインはリダクション（reduction）または実体化（materialization）によって **必ず終端しなければならない**（パイプラインの途中状態を保持しようとするとコンパイルエラーになる。つまり、遅延評価（lazy）された値がスコープ外へエスケープすることは決してない）。このファイルではその表面仕様と構造のルールについて扱う。列指向（columnar）レイヤについては [soa-groupby.md](soa-groupby.md) を参照。

## 3 つのコレクション形

```text
[a, b, c]        fixed array [T; N] — stack slot, compile-time length, Copy
array<T>         dynamic array — heap/arena {ptr,len}, Move (deep-dropped for str elements);
                 produced by .to_array(), chunks, json.decode, partition, sort
slice<T>         borrowed view {ptr,len}, Copy, region = the data it points into
```

`bytes` はドキュメントなどの文章表現における `slice<u8>` の略記であり、別個の型ではない。

## Signatures (verified)

```text
xs.len()   -> i64        // direct length: str/string, slice, array (fixed = const), soa, buffer
xs[i]                    // index (bounds-checked abort): scalar elem / chunk slice / struct gather / vec lane
xs[a..b]   -> slice<T>   // range view; scalar elements only; either bound omittable
xs[i] = v                // scalar element write — needs mut local or out slice param
arr[i] = structval       // whole-struct element write (POD; Move structs into FIXED arrays only)
arr[i].f = v             // element-field write, nested paths ok; dynamic arrays: primitive leaf only
fn f(out dst: slice<T>)  // writable-slice param; caller passes a mut binding; no-alias enforced

// stages                          // terminals
xs.map(f)                          xs.sum() / .count() / .min() / .max()
xs.where(p) / .where(.flag)        xs.any(p) / .all(p)
xs.field                           xs.reduce(init, f)      // init FIRST
xs.scan(init, f)                   xs.to_array()           // materialize -> array<T>
xs.chunks(n)                       xs.map_into(dst)        // write into caller slice
zip(a, b, ...)                     // lazy equal-length multi-source head (Copy scalars)
                                   xs.sort() / .sort_by_key(f)   // materializing
                                   (evens, odds) := xs.partition(p)
```

ステージへの関数引数は、名前付きの `fn`、ラムダ式 `fn x { … }` / `fn acc, x { … }`、または `.field` 射影の形式をとる。`reduce` や `scan` は **init-first（初期値が先）** である。末尾に初期値を置く古い形式は完全に廃止された（後方互換性を持たせないルールに従い、別名は一切残していない）。

## Type & ownership classification

- Fixed array は Copy 値である。**Move 要素** を持つ fixed array（所有権付きフィールドを持つ `[User{name}]` など）は、要素ごとの drop が実装されるまで拒否される。
- Dynamic `array<T>` は再帰的な Drop を持つ Move 型である（str 要素の配列は deep-free される。#339 の前例を参照）。
- Slice は Copy のビューである。`mut slice<T>` の束縛（または `out` 引数）が、唯一の書き込み可能なビュー形式となる。
- `.count()` は *パイプライン* の長さを表す（`where` と合成される）。一方 `.len()` は直接的な長さの読み取りである。これら 2 つは意図的に両方存在しており、統合してはならない。
- `zip(a, b, ...)` はパイプライン専用の遅延評価ソースである。インデックスごとに SSA タプルを 1 つだけ作成し、ループの前にランタイムの長さをすべて検査するため、タプルの配列用メモリは確保しない。v1 では 2 つ以上の名前付きの array/slice、fixed literal、または Copy 可能なプリミティブスカラー要素を持つ sub-slice を受け取る。

## Effects

ステージと終端（terminal）は、純粋な関数引数が与えられれば Pure となる。引数の純粋性は推論され、必要な箇所（`par_map` など）で要求される。また、パイプラインのラムダはアロケーションが漏れるような形式（`str + str`、`template` など）をコンパイルエラーとして拒否する（[string.md](string.md) を参照）。

## Errors & aborts

この領域では `Result` は使用されない。構造的な間違いはコンパイルエラーとなる（未終端のパイプライン、ステージのラムダの arity 不一致、Move 要素の slicing/indexing、`out` 引数の aliasing、`map_into` における source と dst の重複など）。
ランタイムでは、インデックスや範囲の境界外アクセス（out of bounds）、`map_into` や実行時の `zip` の長さ不一致が abort を引き起こす。
固定長 `zip` における長さの不一致はコンパイルエラーとなる。空の入力はエラーではなく、ひとつの結果として扱われる（`sum` は 0、`count` は 0、`any` は false、`all` は true）。証明可能に空なフィルタに対する `min` / `max` は番兵（sentinel）となる単位元を返す（分岐のない `where` リデューサ、#303）。

## Regions

`region_of(xs[a..b]) = region_of(xs)`、`region_of(chunks elem) = region_of(source storage)` — これは #297 で導入された storage と element の区別に基づく（str 配列の *要素* は配列の *storage* よりも長生きする可能性がある）。`to_array` / `sort` / `partition` の結果は owned となる（region なし）。`map_into` は呼び出し側の region を介して書き込みを行い、**no-alias を証明する**。呼び出し側の out-disjointness チェックは、#328 の call-laundered-aliasing 修正以降、意図的に保守的なものになっている。その敵対的なケースを再実行せずにチェックを緩めてはならない。
`zip(...).map_into(dst)` では、すべての source と `dst` が重複しないことが証明される。ランタイムの source 読み込みは 1 つの input-vs-output スコープを共有し、source 同士のエイリアスは許可されており、互いに disjoint であるとは宣言されない。

## 仕様先行(未実装)

- **Move 要素** のコレクションの slicing/indexing（「slicing a collection of the Move type … not supported yet」）。固定長の Move struct 配列と所有 struct-array フィールドには再帰的な要素 drop が実装済みである。残る問題はコレクションの破棄ではなく、読み出しを借用とするか所有権移動とするかという規則である。
- **非プリミティブな leaf**（str / owned / nested-Move）を持つ dynamic `array<Struct>` における要素フィールドの書き込み — `StoreElemFieldPtr` はプリミティブ leaf 専用である（#316）。
- ネストした要素書き込み `arr[i].a.x = v` は動作する。しかし、ネストした **soa** 列や、テスト済みの形式を超える chained projection 経由での要素書き込みは未対応 — `08-nested-structs.md` の deferred リストを参照。
- `soa` 列は汎用パス（generic path）経由では範囲スライスできない（列のウィンドウは実装済みの `s.field[a..b]` を経由する。未対応なのは汎用的な `check_slice_range` のアームのみである）。

## Pitfalls

- P1 — **終端のルールは言語の不変条件** であり、単なる style lint ではない。`xs.map(f)` の値を束縛すると、隠れた「実行待ちのループ（loop-in-waiting）」が生じてしまう。新しいステージは必ず終端するか、静的に terminal へ流れ込むよう要求されなければならない。
- P2 — **どこでも init-first（初期値が先）**: 新しい fold 系の API（`reduce`、`scan`、将来の `fold_*` など）は seed（初期値）を先に取る。規約が混在すると、AI が誤ったコードを生成する原因になる。
- P3 — `out` の no-alias チェックは、同じローカル変数の **サブスライス**（#302）と call-laundered なビュー（#328）の両方を考慮しなければならない — これらはどちらも実際に存在した soundness の穴だった。新しい書き込み可能なビューのインターフェースは、同じチェックを経由させる必要がある。
- P4 — fixed-array のインデックスアクセスは、レシーバがリテラルまたは変数であることを要求する（MIR がスロットをアドレス指定するため）。失敗する式レシーバのケースに対して、配列を暗黙にコピーして「修正」するようなことはしてはならない。
- P5 — 束縛されていない owned な一時値には 2026-07-15 以降、パスローカルな synthetic owner が存在し、ビューの生存期間やループの反復ごとの cleanup も追跡するようになった。`chunks(n)` は直接的な `.len()`、インデックスアクセス、パイプラインの consumer に渡す場合でも、依然としてヘッダの配列を実体化（materialize）する。この残存するコストについては、[audit 13 §8.2](../../13-string-array-allocation-short-input-audit.md#82-confirmed-p1--virtualize-chunks-for-direct-consumers) で追跡している。

## Test anchors

`m4.rs`（count/min/max/any/all）、`mmv2.rs`（scan/sort）、`lambda.rs`（ステージのラムダ、arity、純粋性の拒否）、`map_into.rs`（+#328 の aliasing ケース）、`out_params.rs`（no-alias、bounds）、`struct_index.rs`（要素/フィールド書き込み、ネストしたパス）、`tuples.rs`（partition の分解）、`zip_pipeline.rs`（fusion、SIMD、長さ/effect/trap/alias の契約）。
例として `pipeline.align`、`chunks.align`、`partition.align`、`sort_by_key.align`、`owned_array.align` がある。また、differential fuzzer が reducer の terminal を網羅している（#326）。
