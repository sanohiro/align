このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同等の粒度（シグネチャ、Move/effect の分類、エラー方針、落とし穴（Pitfalls）、テストアンカー）で記述された公式な設計ドキュメントを収めている。
執筆はメインループ（Fable）が担当している。

# core — ライブラリ設計ドキュメント

> 🌐 [English](../README.md) · **日本語**

## このディレクトリが存在する理由

`std` モジュールには、実装に先立って各モジュールの設計仕様（`../std-design/`）が用意されていた。一方、`core` は逆の道をたどってきた。つまり、マイルストーン（M0–M10）ごとに徐々に実装・リリースされ、その仕様は `draft.md` （§5 Option/Result、§7 array/slice、§8 データ処理、§9 SIMD、§12 string、§13 template、§14 JSON、§18.1 カタログ）に散在している。しかも、§18.1 は単なる名前のカタログに過ぎず、ところどころ実装に **先行** している（例: `split`、`json.scan`）。
このディレクトリのドキュメントは、そのギャップを埋めるためのものである。core の領域ごとに 1 つのファイルを作成し、**実装済みでテストによって担保された仕様** を std-design と同等の粒度で記録する。加えて、*仕様先行（未実装）* のセクションを明示することで、仕様と実装の乖離（drift）を暗黙のままにせず可視化する。

位置づけとしては、`draft.md` が *セマンティクスと方向性* に関する言語レベルの「信頼できる情報源（source of truth）」であり続けるのに対し、ここのドキュメントは **現在のライブラリの実装状況** に関する「信頼できる情報源」となる。正確なシグネチャ、ownership/effect/region の分類、abort か `Result` かのエラー方針、そして各挙動をどのテストが担保しているかを定義する。
core の領域を実装・変更する際は、同じ PR 内でここの該当ファイルも更新すること（これは std-design と同じルールである）。

## ファイル一覧

- [option-result.md](option-result.md) — `Option<T>` / `Result<T, E>` / builtin `Error`: コンストラクタ、`?`、`else`、`map_err`、`main` の exit マッピング
- [array-slice-pipeline.md](array-slice-pipeline.md) — `array<T>` / `slice<T>` / range / `out`、そしてパイプライン語彙の全体 + 終端と fusion のルール
- [string.md](string.md) — `str` / `string` / `bytes` / `buffer` / `builder` / `template`: メソッド、連結の方針、UTF-8 の立場
- [json.md](json.md) — `json.encode` / `json.decode`(struct / array / soa ターゲット)、エラー方針、ゼロコピービュー
- [soa-groupby.md](soa-groupby.md) — `soa<T>`、列操作、`group_by` の集約、`.agg(...)`、`dict_encode`
- [vec-mask.md](vec-mask.md) — `vecN<T>` / `maskN<T>`、レーン操作、`load`/`store`、`select`/`dot`/`fma`/`sum_where`、`align(N)`
- [arena-heap.md](arena-heap.md) — `arena {}` と `heap.new` / `box`: region、escape、drop
- [hash.md](hash.md) — `core.hash`(`hash64`/`hash128`): 状態と設計

各ファイルのテンプレート: **Overview → Signatures (verified) → Type & ownership → Effects → Errors &
aborts → Regions → 仕様先行(未実装) → Pitfalls → Test anchors。**
