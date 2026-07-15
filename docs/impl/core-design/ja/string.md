このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同じ粒度(シグネチャ、
Move/effect の分類、エラー方針、pitfalls、テストアンカー)の正典となる設計ドキュメントを収めている。
執筆はメインループ (Fable) が担当している。

# core — str / string / builder / template

> 🌐 [English](../string.md) · **日本語**

## Overview

テキスト(draft §12–§13) — 借用ビュー型、所有バッファ型、組み立て用の builder、そして 1 つの template
形。全体を通してバイト指向の UTF-8 である。検索系メソッドは memchr 系の SIMD スキャンレイヤに乗る(#310)。
肝となる方針は **すべての文字列確保に目に見える住処がある** ということ — arena、所有者、または builder。
パイプラインのラムダ内での確保はコンパイルエラーである。

## Signatures and settled surface

```text
"lit"                      -> str        // single-line only; \n \t \" escapes; UTF-8
'A' / 'あ'                 -> char       // one Unicode scalar
s.len()                    -> i64        // BYTE length ("あ".len() == 3)
s.contains(n) / s.starts_with(n) / s.ends_with(n)      -> bool
s.eq_ignore_ascii_case(t)  -> bool       // ASCII fold only, not Unicode
s.find(n) / s.rfind(n)     -> Option<i64>   // byte index of first/last occurrence
s.trim() / s.trim_start() / s.trim_end()    -> str   // ASCII-whitespace; zero-copy sub-view
s[a..b]                    -> str        // range view; region-tied; NO s[i] byte indexing
s.clone()                  -> string     // deep copy; the arena-escape hatch
a + b                      -> compile error; builder is the one concatenation path

b := builder()  /  builder(cap)
b.write(s: str|string)  /  b.write_int(i: i64)
b.to_string()              -> string     // the finisher (there is no finish()/build())

template "…{expr}…"        -> str        // holes: int, float, str, bool, char; full expressions
```

レシーバは自動借用される — 上記のどのメソッドも `str` か `string` を取る(所有権付き `string` は消費
されず、ビューされる)。`hash64`/`hash128` もこれらのビューを受け取る([hash.md](hash.md))。

## Type & ownership classification

- `str` — Copy のビュー `{ptr, len}`、region = 指し示すデータ(リテラルは region-0/static)。
- `string` — 所有権付きの Move ヒープバッファ。drop で解放され、再代入は古い方を drop し、`str` へ自動
  借用される。
- `builder` — 所有権付きのアキュムレータ。`to_string` がそれを終える。隣接する write は MIR の peephole
  で fuse される(`fuse_builder_writes`、`"lit" + int + "lit"` → 1 回のランタイム呼び出し) — 新しい
  write の形は batcher を迂回するのではなく拡張すること。
- `template` の結果は arena 内では arena に region 付けされた `str` である。現在の arena 外 lowering は
  payload をプロセス寿命で leak する。これは性能契約ではなく、確認済みの ownership gap である
  ([audit 13](../../13-string-array-allocation-short-input-audit.md#33-confirmed-p0p1--arena-free-template-and-jsonencode-leak-forever))。

## Effects

Pure(I/O 無し)。*確保の可視性* のルールはエフェクトではなく構造的に強制される — `str + str` はすべての
場所で settled hard error である。自身の arena を持たないパイプラインラムダ内の `template` もハードエラー
になる(「黙って漏らすことになる」 — `lambda.rs`)。checker はこの一律の規則を強制し、古い MIR の
連結経路も削除された(audit 13 §3.2、2026-07-15 修正済み)。

## Errors & aborts

この領域に `Result` は無い。`s[a..b]` の out of bounds は abort する。非 UTF-8 の *入力* は `std` 境界の
関心事である(`fs.read_file` → `Error.Invalid`)。core の文字列操作は不変条件を前提としてバイト指向を
保つ。range lowering は両端で仕様どおりの O(1) UTF-8 scalar-boundary abort を行う
(audit 13 §3.1、2026-07-13 修正済み)。

## Regions

`region_of(trim*/s[a..b]) = region_of(s)` — サブビューは継承する。`clone` → owned、region 無し。
`string` の struct フィールド読み取りは Frame region の `str` として借用
される(所有権付き struct が動く)。

## 仕様先行(未実装)

- **`split`** と **`find_any`**(§18.1 カタログ) — ディスパッチのアーム無し。`split` が大物である —
  その戻り形(ビューの `array<str>` — region 付きビューの Move 配列)には Move 要素コレクションの作業が
  必要である。owned-copies の妥協形として出荷してはならない(「理想形か、さもなくば defer」)。今日は
  `find`/`rfind` + `s[a..b]` で手動の split を組み立てる。
- `s[i]` のバイトアクセスは無い — 今のところ意図的である(バイトアクセスは UTF-8 上でのインデックス
  バグを招く)。用途が要求するなら、`u8` を返すセマンティクスをまず `open-questions.md` で決めること。
- §13/§18.1 の template variant(`html`、`raw`、json-template) — プレーンな `template "…"` のみ存在
  する。escape variant の設計(文脈依存の autoescape)は未確定である。

## Pitfalls

- P1 — すべての検索/比較は **バイト指向** である。ユーザー向けのものには char か byte かを明記すること
  (find が返すのは *バイト* インデックスであり、`s[a..b]` に有効な入力であって char の個数ではない)。
- P2 — `str + str` は settled hard error であり、パイプラインラムダだけの規則ではない。builder を使うこと。
  lint へ弱めたり、古い arena concat 実装を復活させたりしてはならない。
- P3 — `builder.to_string` が唯一の finisher である。`finish()` の別名を追加するのは One-way に反する。
- P4 — `eq_ignore_ascii_case` は名前どおり、設計どおり ASCII 専用である。Unicode の case-fold は別の
  (ロケールに汚染された)機能である — non-goals に従い、スコープ外として拒否すること。

## Test anchors

`m5.rs`(find/rfind ペア、trim ファミリ、fuse を含む builder、template、escape、
UTF-8 のバイト長、print 型の網羅を含むメソッド); `lambda.rs:271/280/287/294`(ラムダの確保拒否 +
ラムダ内 arena の許容); `hash.rs`(ビュー受け入れ); `fuzz_fmt.rs`(文字列を多用するソースの formatter
往復); 例 `strings.align`、`template.align`。文字列 concat の拒否は reducer、名前付き関数、
ラムダの各コンテキストで一様にカバーされる。SIMD スキャンの固定: #310 differential oracle。
