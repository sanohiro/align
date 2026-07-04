このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同じ粒度(シグネチャ、
Move/effect の分類、エラー方針、pitfalls、テストアンカー)の正典となる設計ドキュメントを収めている。
執筆はメインループ (Fable) が担当している。

# core — soa / group_by / dict_encode

> 🌐 [English](../soa-groupby.md) · **日本語**

## Overview

カラムナ層である。`soa<T>` は struct の各フィールドを 1 本の連続したカラムとして格納し(draft §9)、
`group_by` はグループ化された fold のプリミティブ、`dict_encode` は `str` のキーカラムを一度だけ intern
して再利用できるようにする。「decode → filter → aggregate」を、ごく普通のコードからカラムナデータベース
並みの速度で走らせるための層である。M6 のベンチでは、カラムに触れるワークロードで AoS スキャンに対して
~8–10× を出している。

## Signatures (verified)

```text
rows.to_soa()                    -> soa<T>       // AoS transpose; REQUIRES enclosing arena {}
json.decode(s)  (into soa<T>)    -> Result<soa<T>, Error>   // direct columnar decode, no transpose
s.len()                          -> i64
s.field                          -> slice<F>     // column projection; full pipeline applies
s.field[a..b]                    -> slice<F>     // column window
s[i]                             -> T            // gather one row (multi-column fetch)
s[i].field                       -> F            // one cell (IndexColumn)
s[i].field = v                                   // one-cell write (StoreColumn; needs mut binding)
s[i] = value                                     // whole-element scatter (gather+scatter)
s.where(.flag)                                   // column-predicate filter stage

s.group_by(.k).sum(.v)           -> (keys, sums)     // i64 key on soa, or str key on array<Struct>
s.group_by(.k).min(.v) / .max(.v) / .count()         // count takes NO field
xs.group_by(.name).agg(sum(.a), max(.b), count())    // fused multi-aggregate, ONE pass
xs.dict_encode(.name)            -> encoded          // intern str key column; reuse across group_bys
```

`group_by` の結果は、並列なカラムのタプルである。`g.0` = 相異なるキー、`g.1..` = 各集約につき 1 本ずつの
カラムで、`g.0` と行が揃っている。

## Type & ownership classification

- `soa<T>` は **arena に常駐するビュー構造** である。カラムは外側の arena に確保される(`arena {}` の外で
  `to_soa` すればコンパイルエラー — 「'to_soa' allocates its column buffer in an arena」)。Move 型では
  なく、region に束縛されたデータである。arena を越えた escape は通常の region ルールで拒否される。
- `T` は struct でなければならない。`str` フィールドは許される(ビューのカラムになる)。非 struct の
  `soa` は sema で拒否される。
- カラムの射影(`s.field`)は、soa の region を担うただのスライスである。
- `group_by`/`agg` の出力は所有権付きの結果カラム(配列のタプル)であり、パスの後でも使える。
- `dict_encode` の `encoded` 値はソース配列を借用する。続く `group_by` のキーは **encoded したキーと一致
  していなければならない**(不一致は sema で拒否)。

## Effects

すべて純粋な計算であり、I/O も rng もない。`to_soa`/`group_by`/`agg`/`dict_encode` のノードは Pure で、
純粋性が要求されるどこにでも置ける。ただし arena 要件が制約するのは *どこで* 走らせられるかであって、
effect クラスではないことに注意。

## Errors & aborts

この領域には `Result` が一切ない。形が不正なものは **コンパイルエラー**(誤ったソース型、i64 でない集約値、
未知の集約名、空の `.agg()`、集約のない裸の `group_by`、`sum(.strfield)`)であり、ランタイムでの失敗経路は
存在しない(ハッシュテーブルは grow するし、入力が空ならキー/値カラムも空になる)。

## Regions

`region_of(soa) = 外側の arena`。`region_of(s.field) = region_of(s)`。`s[i]` は *値*(Copy な struct)を
gather する — `T` が `str` フィールドを持たない限り region はない。持つ場合、gather したビューはカラムの
region を保つ(#297 のストレージ vs 要素リージョンの区別が効く。`str` カラムの *要素* は、より長生きする
テキスト、例えば decode 元の JSON 入力を指すことがあるが、*ストレージ* は arena 束縛である)。

## 仕様先行・保留(未実装)

- **soa をソースとする `.agg(...)`** — 初版は str キーの AoS `array<Struct>` のみ(`soa.rs`
  `group_by_agg_soa_source_is_rejected`)。i64 キーの soa 多重集約は記録済みのフォローアップである。
- `.agg` / `dict_encode` には **動的な** `array<Struct>` ソースが要る(固定長のスタックリテラル配列は拒否。
  decode するか、`array<T>` を引数で取る)。
- **所有権付きの soa カラム**(arena を越えて生き延びる soa)、**`soa_slice<T>`**(窓を切った soa ビュー —
  repr は #330 で決定済み、新しい型ではなく統一で扱う)、**packed-bool カラム** — いずれも post-M6 の
  backlog で、ロードマップと `open-questions.md` に記録済み。
- 集約内の `avg`/`median`/フィールドを取る `count(.f)` — 設計上、初版では拒否する。`avg` はフォローアップ
  候補、`median` は別種のアルゴリズムクラスを要する。

## Pitfalls

- P1 — **arena 要件は load-bearing**: カラムは bump 確保されるバッチデータである。arena 束縛が無いと、
  カラムごとのバッファの drop 追跡に Move な soa が必要になる(保留)。「heap に黙って確保させる」形で
  `to_soa` のコンパイルエラーを「修正」してはならない。
- P2 — **`agg` は AoS を strided に読む**: fused パスは行優先メモリからフィールドを gather する。集約ごとの
  パスに対して ~3× 速いが、事前転置した soa ソース(実装されたら)は密に読める — この 2 つの形を、
  等価であるかのようにベンチ比較してはならない。
- P3 — **dict_encode の再利用の作法**: 勝ち筋は hash-once である。同じカラムを 2 度 `dict_encode` すれば
  ハッシュ計算がもう一度かかる。encoded 値を一度だけ束縛し、すべての group_by をその束縛から走らせること。
- P4 — **str カラムは decode 入力へのゼロコピービュー** である。入力(または arena)を soa と同じだけ生か
  し続けること。region チェッカがこれを強制する。エラーは decode 時ではなく escape 地点で出ると心得よ。

## Test anchors

`crates/align_driver/tests/soa.rs`(カラム、index/write、group_by の各形、agg の accept/reject マトリクス、
dict_encode の再利用 + キー不一致の拒否);`m5.rs` の json→soa decode;ガイド例
`examples/soa.align`、`examples/soa_json_str.align`。Perf の pin: `bench/group_by_reuse/README.md`、
`bench/json_soa/README.md`。
