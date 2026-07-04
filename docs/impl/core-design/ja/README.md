このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同じ粒度(シグネチャ、
Move/effect の分類、エラー方針、pitfalls、テストアンカー)の正典となる設計ドキュメントを収めている。
執筆はメインループ (Fable) が担当している。

# core — ライブラリ設計ドキュメント

> 🌐 [English](../README.md) · **日本語**

## このディレクトリが存在する理由

`std` には、実装に先立って各モジュールの実装可能な設計仕様(`../std-design/`)が用意された。`core` は逆の
道をたどっている — マイルストーンを一つずつ(M0–M10)積み上げて出荷され、その規範的な表面は `draft.md`
(§5 Option/Result、§7 array/slice、§8 データ処理、§9 SIMD、§12 string、§13 template、§14 JSON、§18.1
カタログ)に散らばっており、§18.1 は薄い名前カタログに過ぎず、ところどころ実装より **先走って** いる
(例: `split`、`json.scan`)。これらのドキュメントはそのギャップを埋めるものである — core の領域ごとに 1
ファイルを設け、**実装済みでテストに固定された表面** を std-design と同じ粒度で記録し、加えて
*仕様先行(未実装)* のセクションを明示することで、drift を暗黙のうちに放置せず可視化する。

優先順位: `draft.md` は *セマンティクスと方向性* についての言語レベルの source of truth であり続ける。
これらのドキュメントは **現在のライブラリ表面** についての source of truth である — 正確なシグネチャ、
ownership/effect/region の分類、abort か `Result` かの方針、そして各挙動をどのテストが固定しているか。
core の領域を実装・変更するときは、同じ PR の中でここの該当ファイルを更新すること(std-design と同じ
ルール)。

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
