このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同等の粒度（シグネチャ、Move/effect の分類、エラー方針、落とし穴（Pitfalls）、テストアンカー）で記述された公式な設計ドキュメントを収めている。
執筆はメインループ（Fable）が担当している。

# core — str / string / builder / template

> 🌐 [English](../string.md) · **日本語**

## Overview

テキスト処理（draft §12–§13） — 借用ビュー型、所有権を持つバッファ型、組み立て用の builder、そして 1 つの template 形式からなる。全体を通してバイト指向の UTF-8 として扱われる。検索系メソッドは memchr 系の SIMD スキャンレイヤの上に構築される（#310）。
ここでの中心的な方針は **「すべての文字列のアロケーション（メモリ確保）先が明確であり、目に見える形になっていること」** である — arena、所有者（owner）、または builder のいずれかに属する。パイプラインのラムダ内での隠れたアロケーションはコンパイルエラーとなる。

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
s.bytes()                  -> slice<u8>  // zero-copy byte view; UTF-8 義務なし
s.clone()                  -> string     // deep copy; the arena-escape hatch
a + b                      -> compile error; builder is the one concatenation path

b := builder()  /  builder(cap)
b.write(s: str|string)  /  b.write_int(i: i64)
b.to_string()              -> string     // the finisher (there is no finish()/build())

template "…{expr}…"        -> str        // holes: int, float, str, bool, char; full expressions
```

レシーバは自動で借用される — 上記のどのメソッドも `str` または `string` を受け取る（所有権を持つ `string` は消費されず、ビューとして扱われる）。`hash64` / `hash128` もこれらのビューを受け取る（[hash.md](hash.md) を参照）。

## Type & ownership classification

- `str` — Copy 可能なビュー `{ptr, len}` であり、region はそれが指し示すデータに依存する（リテラルの場合は region-0/static となる）。
- `string` — 所有権を持つ Move 型のヒープバッファ。破棄時（drop）に解放され、再代入されると古いデータは drop される。必要に応じて `str` に自動借用される。
- `builder` — 所有権を持つアキュムレータ。`to_string` の呼び出しによって終了する。隣接する `write` は MIR のピープホール（peephole）最適化によって融合（fuse）される（`fuse_builder_writes` において、`"lit" + int + "lit"` は 1 回のランタイム呼び出しに変換される） — 新しい形式の `write` を追加する場合は、このバッチ処理を迂回するのではなく拡張すること。
- `template` 式の結果は、arena 内では arena に region 付けされた `str` となる。arena 外で動的に生成された結果は、隠蔽されたスコープ付き `string` 所有者を参照する、フレーム境界（frame-bounded）ビューとなる。静的部分のみで構成されている場合は、プールされたリテラルに畳み込まれる（[audit 13](../../13-string-array-allocation-short-input-audit.md#33-fixed-2026-07-15--arena-free-template-and-jsonencode-have-scoped-owners)）。

## Effects

Pure（I/O なし）。*アロケーションの可視性* というルールはエフェクトシステムではなく構造的に強制される — `str + str` はあらゆる場所においてハードエラーとなる（決定済みの仕様）。arena 外での `template` 式の結果はパイプラインのラムダ内でローカルに消費できるが、そのフレーム境界ビューをラムダの戻り値として返すことはできない（`lambda.rs`）。チェッカーは一律の文字列連結ルールを強制しており、古い MIR の連結パスも削除された（audit 13 §3.2、2026-07-15 修正済み）。

## Errors & aborts

この領域では `Result` は使用されない。`s[a..b]` における範囲外アクセス（out of bounds）は abort を引き起こす。非 UTF-8 な *入力* のエラー処理は `std` 境界での関心事である（`fs.read_file` → `Error.Invalid`）。core の文字列操作は不変条件が満たされている前提でバイト指向を保つ。範囲の部分ビュー作成（range lowering）は、仕様どおり O(1) で両端の UTF-8 スカラー境界チェックを行い、違反時は abort する（audit 13 §3.1、2026-07-13 修正済み）。

## Regions

`region_of(trim*/s[a..b]/s.bytes()) = region_of(s)` — サブビューは元の region を継承する。`clone` は owned なデータを返すため region を持たない。`string` の struct フィールドの読み取りは、Frame region の `str` として借用される（所有権を持つ struct は移動する）。

## 仕様先行(未実装)

- **`split`** および **`find_any`**（§18.1 カタログ） — 現状ディスパッチ用のアームはない。特に `split` は大きな課題（大物）である — その戻り値の型（ビューの `array<str>` — つまり region 付きビュー要素を持つ Move 配列）を実現するには、Move 要素コレクションに関する対応が必要になる。owned-copies による妥協的な形態でリリースしてはならない（「理想形で出すか、さもなくば defer（延期）する」）。現状では `find` / `rfind` と `s[a..b]` を組み合わせて手動で split 処理を構築する。
- `s[i]` による直接のバイトアクセスはない — UTF-8 としての保証を外すことが呼び出し側で明確になるよう、`s.bytes()[i]` という明示的なバイトビューを使用する。
- §13 / §18.1 の template のバリアント（`html`、`raw`、json-template など） — 現在はプレーンな `template "…"` のみ存在する。エスケープバリアントの設計（文脈依存の autoescape など）はまだ未確定である。

## Pitfalls

- P1 — すべての検索や比較は **バイト指向** である。ユーザー向けの説明にはそれが文字（char）なのかバイト（byte）なのかを明記すること（find が返すのは *バイト* インデックスであり、`s[a..b]` への入力として有効な値である。文字数ではない）。
- P2 — `str + str` は単にパイプラインラムダ内の規則というわけではなく、どこでもハードエラーとなる決定済みの仕様である。連結には builder を使用すること。これを lint レベルに弱めたり、古い arena 用の連結実装を復活させたりしてはならない。
- P3 — `builder.to_string` が唯一の完了処理（finisher）である。`finish()` のような別名を追加することは One-way レビューの方針に反する。
- P4 — `eq_ignore_ascii_case` はその名前が示す通り、設計上 ASCII 専用である。Unicode の大文字小文字同一視（case-fold）はロケールに依存した（汚染された）別の機能である — non-goals に従い、スコープ外として拒否すること。

## Test anchors

`m5.rs`（find / rfind のペア、trim ファミリ、ゼロコピーの bytes ビュー、fuse を含む builder、template、エスケープ、UTF-8 のバイト長、print 型の網羅性チェックを含むメソッド）。`lambda.rs:271/280/287/294`（ラムダ内でのアロケーションの拒否 + ラムダ内での arena の許可）。`hash.rs`（ビューの受け入れ）。`fuzz_fmt.rs`（文字列を多用するソースの formatter 往復テスト）。例として `strings.align`、`template.align`。文字列連結の拒否は reducer、名前付き関数、ラムダの各コンテキストで一貫してカバーされている。SIMD スキャンの固定: #310 differential oracle。
