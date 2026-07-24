# std.regex 設計

> 🌐 [English](../regex.md) · **日本語**

状態: **COMPLETE 2026-07-24** — 最初のスライス（2026-07-23）に加え、第二の API 面
（find_all / split / replace / replace_all / captures / group_count / group_index / caps.group）
まで実装・ビルド・テスト・IR 検証を完了した。ランタイムアーカイブには `regex` 1.13.1 を含む。
`regex_tests` と `std_regex.rs` があらゆる操作を網羅する（範囲、Unicode オフセット、空一致での
終端、split の空フィールド、`$`-展開と名前付きグループ、非参加グループ、範囲外アボート、
`block`/`if`/`match`/loop をまたぐ Move/Drop の健全性）。`clippy -D warnings` はクリーン。
正規表現まわりで延期が残るのは以下の周辺拡張のみで（captures イテレータ、クロージャコールバック
置換、`rx"..."` リテラル）、いずれもまだ利用者がいない。

## 配置と目的

正規表現は、Align の言語構文ではなくアプリケーション向けライブラリの境界に置く。
新しい構文、型規則、コンパイラによる定数評価を増やす必要はない。最初のスライスでは、
明示的な import と、明示的にコンパイルするハンドルだけを追加する。

```align
import std.regex

re := regex.compile("^[a-z]+$")?
if re.is_match(input) { ... }
```

目標は、最悪時の検索計算量を予測可能にすること、割り当てと所有権を見える形にすること、
一度コンパイルした結果を再利用すること、隠れたキャッシュを持たないこと、そして新たな
文字列割り当てや新しいビューの借用を行わずに一致範囲を返すことである。

## API

```text
regex.compile(pattern: str) -> Result<regex, Error>
re.is_match(text: str) -> bool
re.find(text: str) -> Option<regex_match>
re.find_at(text: str, start: i64) -> Option<regex_match>

regex_match { start: i64, end: i64 }
```

`regex` は名前を記述できる不透明な Move 型である。コンパイルはエンジンプログラムを所有し、
Drop 時に `align_rt_regex_free` で解放する。各メソッドはハンドルと入力文字列を借用する。
現行の Align では所有権を持つ無名レシーバのクリーンアップに制限があるため、コンパイル結果は
メソッド呼び出しの前に先へ束縛しておく。

`regex_match` は常に登録される組み込み Copy 構造体で、2 個の `i64` フィールドを持つ。
範囲は半開区間で、UTF-8 のバイト単位で数える。両端は必ず文字境界なので、
`text[m.start..m.end]` は有効な `str` スライスになる。`str` そのものではなくオフセットを
返すことで、結果を Copy/Static のまま保ち、regex と入力の双方へのリージョン依存を避ける。

`find_at` は指定された UTF-8 バイト位置から検索を開始する。負数、末尾より後、または
コードポイントの途中を指す位置は、Align の範囲スライス検査モデルと同じくプログラマの誤りとして
アボートする。入力末尾ちょうどの位置は有効で、空一致が見つかる場合もある。一致しない
ことはエラーではなく `None` である。

**アンカーは `start` ではなく入力全体の真の先頭を基準に評価される。** アンカーを含むパターンでは
`find_at(text, k)` は `find(text[k..])` と等価ではない。`^`・`\A`・単語境界 `\b` は `text` 全体
（位置 0、`\b` では `k-1` のバイト）を基準に解決され、オフセットを基準にはしない。よって
`regex.compile("^a")?.find_at("aXa", 1)` は `None` になり（`^` が成立する位置は 0 のみで、
`k = 1` より前にある）、`\bword` は `k` の直前のバイトを境界文脈として見る。オフセット自体を
行頭・入力先頭として扱いたい場合は、先に入力をスライスする（`re.find(text[k..])`）。

## エンジンと資源上限

ランタイムは Rust の `regex` 1.13.1 を使用し、Unicode は既定で有効とする。このエンジンは
先読み・後読みと後方参照をサポートせず、バックトラッキング爆発の代わりにオートマトン方式の
予測可能な最悪時計算量を保証する。Align v1 はこの制限されたパターン言語を契約とする。

コンパイルには独立した 2 つの上限を設ける。

- UTF-8 パターンソース: 最大 64 KiB
- コンパイル済みエンジン: size limit 10 MiB

構文エラーまたはいずれかの上限超過は `Error.Invalid` を返す。v1 の `Error` には詳細メッセージを
格納しない。有効な UTF-8 の `str` と正しい開始境界を受け取る検索は全域的である。暗黙の
グローバルキャッシュやスレッドローカルキャッシュは持たない。所有ハンドルそのものがキャッシュで
あり、その寿命はコード上に明示される。

4 操作はいずれも意味論上 Pure である。メモリの割り当てや所有メモリの読み取りは外部効果
ではない。既存のクロージャ所有権規則がそのキャプチャを安全に表現できるようになれば、
コンパイル済みハンドルを Pure な逐次処理や並列処理で再利用できる。

## コンパイラとランタイムの形

- sema: 組み込み `std.regex`、`Ty::Regex` / `Scalar::Regex`、組み込み `regex_match`、
  束縛済みレシーバ検査、Move/Drop 分類
- HIR: `RegexCompile`、`RegexIsMatch`、`RegexFind`
- MIR: status と out-slot を使う compile、i32 フラグを返す検索、構造体へ書き込む find。
  所有ハンドルを `Result` に包む汎用の lowering は HTTP ハンドルと共有する
- LLVM: 不透明ポインタ ABI、`{ i64, i64 }` の match 出力スロット、デストラクタ分岐
- runtime: `align_rt_regex_compile`、`align_rt_regex_is_match`、`align_rt_regex_find`、
  `align_rt_regex_free`

追加の機能固有ネイティブシステムライブラリはリンクしない。Rust クレートは通常のランタイム
アーカイブにコンパイルして含める。

## 第二の API 面 — 反復・置換・split・キャプチャ（2026-07-24 に設計確定）

延期していた機能はいずれも既存の配線の上に理想形のまま載る。**新たな言語機能ゲートは不要**で、
新しい不透明 Move ハンドル型を追加するのは `captures` の 1 つだけである（機構的には `regex`
ハンドルそのものと同一）。

```text
re.find_all(text: str)               -> array<regex_match>     // owned Move; leftmost, non-overlapping
re.split(text: str)                  -> array<regex_match>     // owned Move; the between-match field spans
re.replace(text: str, repl: str)     -> string                // owned Move; first match only
re.replace_all(text: str, repl: str) -> string                // owned Move; every non-overlapping match
re.captures(text: str)               -> Option<captures>      // Move handle; None = no match at all
re.group_count()                     -> i64                   // total groups, incl. group 0
re.group_index(name: str)            -> Option<i64>           // name -> numbered index, on the pattern

caps.group(i: i64)                   -> Option<regex_match>   // absent group = None; out-of-range aborts
```

確定した設計判断:

- **すべてはバイト範囲である。** `find_all` も `split` もともに `array<regex_match>` を返し、
  API 全体を 1 つの表現で統一する。範囲は何も参照しない Copy な `i64` の組（純粋なオフセット）
  なので、この配列は**自由にエスケープでき**、regex にも入力にもリージョン依存しない。これは
  実装済みの `find` の契約と一致する。`split` はフィールド範囲（`text[p.start..p.end]`）を返し、
  部分文字列を割り当てない。所有 `array<str>` を返す案は却下した。N 個の部分文字列を深いコピー
  するか、断片のリージョンを `text` に縛るかのどちらかになり、いずれも範囲より劣るためである。
- **`replace` / `replace_all` は所有 `string` を返し**、Rust `regex` の置換契約を展開する
  （`$1`、`${name}`、リテラルの `$` は `$$`）。これは唯一の本当に有用な形で、完全に文書化
  されているので、隠さず仕様として明記する。結果は一致がなかった場合でも常に新しいバッファを
  所有する（`text` への借用には決してならない）。
- **`captures` は不透明な Move `captures` ハンドルを返す**（配列ではない）。
  `array<Option<regex_match>>` は表現できず（Option-of-struct を配列要素にできない）、
  そもそもハンドルが理想形である。オプショナルな Copy 範囲の固定バッファを所有し、何も借用せず、
  `CliParsed` と全く同じ形をとる（不透明 Move ハンドル、total-or-abort / `Option` なゲッター、
  解放関数による `Drop`）。`caps.group(i)` は `Option<regex_match>` を返す。グループ 0 は一致
  全体、参加しなかったグループは `None`、範囲外の `i` はプログラマの誤りとしてアボートする
  （`find_at` の境界モデル）。
- **名前付きグループは番号付きグループに帰着する。** 名前→インデックスの対応表は*パターン*の
  性質なので、コンパイル済み `regex` 側に置く。`re.group_index(name) -> Option<i64>`（未知の
  名前は `None`）で解決してから、通常の `caps.group(i)` を使う。これにより解決経路を 1 本に
  保ち（重複する `caps.name(...)` の機構を作らない）、「one way」に従う。
- 第二の API 面のすべての操作は **Pure** のままで、レシーバ束縛の規則（ハンドルを先に束縛する）
  を保つ。`caps` も同様に `.group` の前に束縛する。

**なお延期する機能（利用者がまだおらず、ブロッカーではない）:** 全一致にわたる captures
イテレータ（`array<captures>` = Move ハンドルの配列、`get_many` の `DynResponseArray`
パターン）、クロージャコールバック方式の置換（先にエスケープ可能な第一級クロージャが必要）、
言語リテラル構文（`rx"..."`）、コンパイル時検証、暗黙キャッシュ、互換目的の
バックトラッキングエンジン。

### スライス計画

1. **R1 `find_all`** — ランタイムが実体化する `array<regex_match>` を確立する
   （`lower_json_decode_struct_array` のテンプレートから `Result` を除いた形。out スロットが
   `{ptr, len}` を受け取り、`Load` して return）。ランタイム `align_rt_regex_find_all` は
   `find_iter` を新しい `align_rt_alloc` バッファへ収集する。空結果は `{null, 0}`（`Drop` は
   null 安全）。
2. **R2 `replace` / `replace_all`** — 独立。所有 `string` を `AlignStr` 経由で返す（`str_clone` /
   `PathJoin` の値返しの形）。常に所有バッファを実体化する（一致なしの `Cow::Borrowed` はクローン
   して返す）。
3. **R3 `split`** — R1 と同じ表現・配線。ランタイムが一致を走査し、一致間の範囲を出力する。
   先頭・末尾・内部の空フィールド、および空入力に対する 1 つの空フィールドも含む。
4. **R4 `captures` + `group_count` + `group_index` + `caps.group`** — `Ty::Captures` /
   `Scalar::Captures` を追加し（すべての Move/drop の `matches!` 一覧、codegen の ptr 型 +
   デストラクタのアーム、すべての網羅的な HIR/MIR ウォークに通す）、`align_rt_regex_captures*`
   ランタイムを追加する。

各スライス共通の健全性不変条件（`/align-self-review` に従う）: `{ptr,len}` のテキストビューを
検証し（`len < 0 || (len > 0 && ptr.is_null())`）、すべてのオフセットを `usize::try_from`、
戻すときは `i64::try_from`（`i64` を超えたら `find` と同様にアボート）、作業前に out スロットを
ゼロ初期化し、`align_rt_free` が期待する割り当てをそのまま返し、そして `find_all` / `split` /
`replace_all` では**空一致を 1 バイトではなく 1 コードポイント分進める**（無限ループを避け、
文字境界を保つため）。新しい HIR `ExprKind` / MIR `Rvalue` は、放置すると素通りしてしまう
escape/region/effect/`MoveCheck`/drop の各パスに必ず登録すること。

## 検証の引き継ぎ

このマシンではビルドもテストも行っていない。ビルド可能なマシンで `Cargo.lock` を更新し、
整形、ビルド、sema 診断と E2E ケース（Unicode 境界、空一致、不正構文、上限、Move/Drop/
エラー経路）、ランタイム単体テスト、workspace 全体のテストと clippy、生成 IR の ABI 確認、
Codex レビューを実施し、すべて通るまではスライスを shipped と記録してはならない。
