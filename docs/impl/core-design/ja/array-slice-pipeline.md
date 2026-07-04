このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同じ粒度(シグネチャ、
Move/effect の分類、エラー方針、pitfalls、テストアンカー)の正典となる設計ドキュメントを収めている。
執筆はメインループ (Fable) が担当している。

# core — array / slice / パイプライン

> 🌐 [English](../array-slice-pipeline.md) · **日本語**

## Overview

データ指向の中心(draft §7–§8) — 3 つのコレクション形と 1 つの処理語彙。以下のステージはすべて単一の
カウント付きループへ fuse される。パイプラインは reduction か materialization で **必ず終端しなければ
ならない**(途中で保持された中間値はコンパイルエラー — lazy な値は決して escape しない)。このファイルは
表面 + 形のルールであり、列指向レイヤは [soa-groupby.md](soa-groupby.md) にある。

## 3 つのコレクション形

```text
[a, b, c]        fixed array [T; N] — stack slot, compile-time length, Copy
array<T>         dynamic array — heap/arena {ptr,len}, Move (deep-dropped for str elements);
                 produced by .to_array(), chunks, json.decode, partition, sort
slice<T>         borrowed view {ptr,len}, Copy, region = the data it points into
```

`bytes` は `slice<u8>` の散文上の略記であって、別個の型ではない。

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
                                   xs.sort() / .sort_by_key(f)   // materializing
                                   (evens, odds) := xs.partition(p)
```

ステージへの関数引数: 名前付き `fn`、ラムダ `fn x { … }` / `fn acc, x { … }`、または `.field` 射影の
形。`reduce`/`scan` は **init-first** である — 従来の末尾 init 順は完全に撤去した(no-backward-compat
ルールに従い、別名は一切残していない)。

## Type & ownership classification

- Fixed array は Copy 値である。**Move 要素** の fixed array(所有権付きフィールドを持つ `[User{name}]`)
  は、要素ごとの drop が入るまで拒否される。
- Dynamic `array<T>` は再帰的な Drop を持つ Move 型である(str 要素の配列は deep-free、#339 の前例)。
- Slice は Copy のビュー。`mut slice<T>` の束縛(または `out` 引数)が唯一の書き込み可能ビュー形である。
- `.count()` は *パイプライン* の長さ(`where` と合成される)。`.len()` は直接読み取りである。両方が
  意図的に存在する — 統合してはならない。

## Effects

ステージと terminal は、純粋な関数引数を与えれば Pure である。引数の純粋性は推論され、必要な箇所で要求
される(`par_map`; そしてパイプラインのラムダは、確保が漏れる形 — `str + str`、`template` — をコンパイル
エラーとして拒否する。[string.md](string.md) を参照)。

## Errors & aborts

この領域に `Result` は無い。形の間違いはコンパイルエラーである(未終端のパイプライン、ステージのラムダの
arity 不一致、Move 要素のスライス/インデックス、`out` 引数の aliasing、`map_into` の source/dst の重複)。
ランタイム abort: インデックス/範囲の out of bounds、`map_into` の長さ不一致。空入力はエラーではなく答え
である — `sum` は 0、`count` は 0、`any` は false、`all` は true。証明可能に空なフィルタに対する `min`/
`max` は sentinel の identity を返す(branchless な `where` reducer、#303)。

## Regions

`region_of(xs[a..b]) = region_of(xs)`; `region_of(chunks elem) = region_of(source storage)` — #297 の
storage-vs-element の区別である(str 配列の *要素* は配列の *storage* より長生きしうる)。`to_array`/
`sort`/`partition` の結果は owned である(region 無し)。`map_into` は呼び出し側の region を通して書き込み、
**no-alias を証明する** — 呼び出し側の out-disjointness チェックは #328 の call-laundered-aliasing 修正
以降あえて保守的にしてある。あの敵対的ケースを再実行せずに緩めてはならない。

## 仕様先行(未実装)

- **Move 要素** のコレクションのスライス/インデックス(「Move 型のコレクションのスライスは…まだ未対応」);
  Move struct の配列(要素ごとの drop が保留)。
- **非プリミティブ leaf**(str/owned/nested-Move)を持つ dynamic `array<Struct>` の要素フィールド書き込み
  — `StoreElemFieldPtr` はプリミティブ leaf 専用である(#316)。
- ネストした要素書き込み `arr[i].a.x = v` は動く。しかしネストした **soa** 列や、テスト済みの形を越える
  chained projection 経由の要素書き込みは別 — `08-nested-structs.md` の deferred リストを参照。
- `soa` 列は generic パス経由では範囲スライスできない(列の窓は `s.field[a..b]` を通り、こちらは実装済み
  — ギャップは generic な `check_slice_range` のアームだけである)。

## Pitfalls

- P1 — **終端ルールは言語不変条件** であって style lint ではない。束縛された `xs.map(f)` の値は隠れた
  loop-in-waiting になってしまう。新しいステージは、必ず終端するか、静的に terminal へ流れ込むことを要求
  されるかのどちらかでなければならない。
- P2 — **どこでも init-first**: 新しい fold 形の API(`reduce`、`scan`、将来の `fold_*`)は seed を先に取る。
  規約が混在すると、AI がコードを誤生成する原因になる。
- P3 — `out` の no-alias チェックは、同じローカルの **サブスライス**(#302)と call-laundered なビュー
  (#328)を考慮しなければならない — どちらも実在した soundness ホールだった。新しい書き込み可能ビューの
  表面は同じチェックを経由させること。
- P4 — fixed-array のインデックスはレシーバがリテラルか変数であることを要求する(MIR がスロットを
  アドレス指定する)。失敗する式レシーバのケースを、配列を黙ってコピーして「直す」ことをしてはならない。

## Test anchors

`m4.rs`(count/min/max/any/all)、`mmv2.rs`(scan/sort)、`lambda.rs`(ステージのラムダ、arity、純粋性の
拒否)、`map_into.rs`(+#328 の aliasing ケース)、`out_params.rs`(no-alias、bounds)、`struct_index.rs`
(要素/フィールド書き込み、ネストしたパス)、`tuples.rs`(partition の分解)、例 `pipeline.align`、
`chunks.align`、`partition.align`、`sort_by_key.align`、`owned_array.align`。differential fuzzer が
reducer の terminal を網羅する(#326)。
