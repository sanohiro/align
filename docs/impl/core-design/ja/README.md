# core — ライブラリ設計ドキュメント

> 🌐 [English](../README.md) · **日本語**

## このディレクトリが存在する理由

`core` ライブラリの現在のシグネチャ、所有権と effect の規則、エラー、関連テストを
領域ごとにまとめる。各ファイルでは、実装済みの操作と、文字列の `split` のように
仕様にはあるが未実装の操作を区別する。

言語の意味と設計上の決定は `draft.md` を正本とする。ここではライブラリの詳しい契約と
実装上の制限を記載する。公開契約が変わったときに該当する文書を更新し、内部実装だけの
変更では契約を再記述しない。

## ファイル一覧

- [option-result.md](option-result.md) — `Option<T>` / `Result<T, E>` / builtin `Error`: コンストラクタ、`?`、`else`、`map_err`、`main` の exit マッピング
- [array-slice-pipeline.md](array-slice-pipeline.md) — `array<T>` / `slice<T>` / range / `out`、そしてパイプライン語彙の全体 + 終端と fusion のルール
- [string.md](string.md) — `str` / `string` / `bytes` / `buffer` / `builder` / `template`: メソッド、連結の方針、UTF-8 の立場
- [json.md](json.md) — `json.encode` / `json.encode_bounded` / `json.decode`(struct / array / soa ターゲット)、`json.doc`、ストリーミング `json.scan`、エラー方針、ゼロコピービュー
- [soa-groupby.md](soa-groupby.md) — `soa<T>`、列操作、`group_by` の集約、`.agg(...)`、`dict_encode`
- [vec-mask.md](vec-mask.md) — `vecN<T>` / `maskN<T>`、レーン操作、`load`/`store`、`select`/`dot`/`fma`/`sum_where`、`align(N)`
- [arena-heap.md](arena-heap.md) — `arena {}` と `heap.new` / `box`: region、escape、drop
- [hash.md](hash.md) — `core.hash`(`hash64`/`hash128`): 状態と設計
- [test.md](test.md) — 実装済み `core.test` declaration/assertion、isolated runner protocol、terse-output policy
- [codec.md](codec.md) — 実装済み `core.codec` canonical columnar batch envelope、zero-copy view、encoder
- [json-escape-benchmark-evidence.md](json-escape-benchmark-evidence.md) — JSON escape benchmark の
  trusted native-host controller、isolation、report schema、acceptance boundary

各ファイルのテンプレート: **Overview → Signatures (明示的に pending のものを除き verified) → Type & ownership → Effects → Errors &
aborts → Regions → 仕様先行(未実装) → Pitfalls → Test anchors。**
