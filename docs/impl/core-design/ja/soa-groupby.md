このディレクトリは `core` ライブラリの各領域の設計仕様をまとめる。
シグネチャ、所有権と effect の分類、エラー方針、注意点、関連テストを記載する。

# core — soa / group_by / dict_encode

> 🌐 [English](../soa-groupby.md) · **日本語**

## Overview

`soa<T>` は struct のフィールドごとに連続したカラムを格納する（draft §9）。
`group_by` はキーごとに集約し、`dict_encode` は `str` のキーカラムを一度だけ
整数 ID に変換して再利用する。カラムを選べば、行全体を読み込まずに必要なフィールドを
走査できる。配置や集約の実測値は末尾のベンチマークに記録している。

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

s.group_by(.k).sum(.v)           -> (keys, sums)     // i64 or str key on soa; str key on array<Struct>
s.group_by(.k).min(.v) / .max(.v) / .count()         // count takes NO field
xs.group_by(.name).agg(sum(.a), max(.b), count())    // fused multi-aggregate, ONE pass
xs.dict_encode(.name)            -> encoded          // intern str key column; reuse across group_bys
```

`group_by` の結果は、並列に並んだカラムのタプルとなる。`g.0` = 相異なるキー、`g.1..` = 各集約ごとのカラム（1 つの集約につき 1 本）であり、行は `g.0` と揃っている。

## Type & ownership classification

- `soa<T>` は **arena に常駐するビュー構造** である。カラムは外側の arena に確保される（`arena {}` の外で `to_soa` を呼び出すとコンパイルエラーとなる — 「'to_soa' allocates its column buffer in an arena」）。Move 型ではなく、region に束縛されたデータである。arena の寿命を越えたエスケープは、通常の region のルールによって拒否される。
- `T` は struct でなければならない。`str` フィールドを持つことは許容される（ビューのカラムとなる）。struct 以外を型引数とする `soa` は意味解析（sema）で拒否される。
- カラムの射影（`s.field`）は、元の soa の region を引き継ぐ単なるスライスである。
- `group_by` / `agg` の出力は所有権を持つ（owned な）結果カラム（配列のタプル）であり、パイプライン処理が完了した後でも使用できる。
- `dict_encode` の `encoded` 値はソース配列を借用する。後続の `group_by` で指定するキーは **エンコードしたキーと一致していなければならない**（不一致は sema で拒否される）。

## Effects

すべて純粋な計算であり、I/O もランダム（rng）も含まれない。`to_soa` / `group_by` / `agg` / `dict_encode` のノードは Pure であり、純粋性が要求されるどのコンテキストにも配置できる。ただし、arena 要件が制約しているのは *どこで* 実行可能か（スコープ）という点であり、effect のクラスではないことに注意。

## Errors & aborts

SoA の直接操作とグループ集約は `Result` を返さない。不正な形は **コンパイルエラー**
となる（誤ったソース型、i64 以外の集約対象、未知の集約名、空の `.agg()`、
集約のない `group_by`、`sum(.strfield)` など）。境界違反やメモリ確保の失敗は
通常の abort 経路を使う。集約の入力が空なら、キーと値のカラムも空になる。
`json.decode` は失敗し得るため、上記のとおり `Result` を返す。

## Regions

`region_of(soa) = 外側の arena`。`region_of(s.field) = region_of(s)`。`s[i]` は *値*（Copy 可能な struct）を gather する — そのため `T` が `str` フィールドを持たない限り region は発生しない。持つ場合、gather されたビューはカラムの region を引き継ぐ（#297 でのストレージと要素の region の区別が効果を発揮する。`str` カラムの *要素* は、例えば decode 元の JSON 入力のように長生きするテキストを指すことがあるが、*ストレージ* 自体は arena に束縛されている）。

## 仕様先行・保留(未実装)

- **soa をソースとする `.agg(...)`** — 初版では `str` キーを持つ AoS `array<Struct>` のみに対応している（`soa.rs` の `group_by_agg_soa_source_is_rejected` を参照）。`i64` キーの soa による多重集約は、フォローアップタスクとして記録済みである。
- `.agg` / `dict_encode` には **動的に確保された** `array<Struct>` ソースが必要である（固定長のスタック上のリテラル配列は拒否される。decode されるか、`array<T>` を引数として受け取る必要がある）。
- **所有権を持つ soa カラム**（arena の寿命を越えて存続する soa）、**`soa_slice<T>`**（ウィンドウとして切り出された soa ビュー — 内部表現は #330 で決定済みであり、新しい型ではなく統一的に扱う）、**packed-bool カラム** — これらはいずれも M6 以降のバックログであり、ロードマップおよび `open-questions.md` に記録済みである。
- 集約機能における `avg` / `median` / フィールドを引数に取る `count(.f)` — 設計上の判断により、初版では拒否される。`avg` はフォローアップの候補であり、`median` は別種のアルゴリズムクラスを必要とする。

## Pitfalls

- P1 — **arena 要件は重要な前提条件（load-bearing）である**: カラムはバンプアロケーションで確保されるバッチデータである。arena による束縛がない場合、カラムごとのバッファを破棄（drop）追跡するために Move 可能な soa が必要になってしまう（現在は保留中）。「裏でヒープに確保させる」といった手法で `to_soa` のコンパイルエラーを暗黙のうちに「修正」してはならない。
- P2 — **`agg` は AoS をストライド（strided）アクセスで読み取る**: fused されたパスは、行優先（row-major）のメモリ配置からフィールドを gather する。これは集約ごとに個別のパスを回すよりも約 3 倍高速だが、事前に転置された soa ソース（実装された場合）は密に（連続して）読み取ることができる — これら 2 つの形態を、あたかも等価であるかのようにベンチマークで単純比較してはならない。
- P3 — **`dict_encode` の再利用**: 同じカラムで再度呼ぶとハッシュ計算も繰り返す。エンコード結果を一度だけ束縛し、複数の `group_by` で使う。
- P4 — **`str` カラムは decode 入力のゼロコピービュー**: soa を使う間は参照先も生存している必要がある。region チェッカは、ビューが参照先の寿命を越えて持ち出される箇所でエラーを報告する。

## Test anchors

`crates/align_driver/tests/soa.rs`（カラム、インデックスアクセス/書き込み、`group_by` の各形式、`agg` の受理/拒否マトリクス、`dict_encode` の再利用とキー不一致による拒否）。`m5.rs` での json→soa の decode。ガイドの例 `examples/soa.align`、`examples/soa_json_str.align`。パフォーマンス検証用のピン: `bench/group_by_reuse/README.md`、`bench/json_soa/README.md`。
