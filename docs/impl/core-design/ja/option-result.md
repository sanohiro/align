このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同等の粒度（シグネチャ、Move/effect の分類、エラー方針、落とし穴（Pitfalls）、テストアンカー）で記述された公式な設計ドキュメントを収めている。
執筆はメインループ（Fable）が担当している。

# core — Option / Result / Error

> 🌐 [English](../option-result.md) · **日本語**

## Overview

唯一の optional モデルと唯一のエラーモデルである（draft §5）。`Option<T>` = 値が存在しない可能性がある（それも正常な答えのひとつ）。`Result<T, E>` = 理由を伴って失敗した。言語内に null や例外（exception）は一切存在しない。エラーの伝播手段は `?` 演算子のみである。
インターフェースの表面は意図的に *狭く* 絞ってある — 分解の基本は `match` であり、そこにわずか 3 つのユーティリティ機能（`else`、`?`、`map_err`）を追加しているのみである。

## Signatures (verified)

```text
Some(x) / None                     // Option constructors — bare, not qualified
Ok(x)   / Err(e)                   // Result constructors — bare
v else fallback     -> T           // Option(Some/None) にも Result(Ok/Err) にも効くフォールバック付き
                                   //   アンラップ。Result では Ok の値を返し、(Copy の)エラーを捨てる
expr?                              // Try: unwrap Ok/Some, else early-return Err/None to the caller
r.map_err(f)        -> Result<T,F> // Result-only; f: fn(E) -> F; Ok passes through untouched
match v { Some(x) => …, None => … }        // exhaustive, payload binds positionally
match r { Ok(v) => …, Err(e) => … }

Error { NotFound, Invalid, Denied, Code(i64) }   // builtin; user redeclaration of `Error` rejected
error(c)                            // sugar: constructs the Code-carrying Error for Err(error(c))
```

## Type & ownership classification

`Option<T>` / `Result<T,E>` は通常のジェネリックな sum 型である（モノモルフィゼーションされる）。[`../../17-library-boundary-prerequisites.md`](../../17-library-boundary-prerequisites.md) の L1a/L1b は完了している。ひとつの再帰的なタグ付き `DropPlan` が有限かつ非再帰な Move payloadを受け入れ、tagged container自体をMoveとし、活性payloadだけをDropし、構築 / `match` / `else` / `?` でmove元を無効化する。recursive type、任意の新しいMove-element collection layout、L2のdynamic path-selected return cleanupは別の担当restrictionとして残る。新しいlibrary handleへcompiler-known exceptionを追加してはならない。

historical checkpoint境界は厳密だった。L1aがまず所有権付きstruct field leafとして
`Option<string>` だけを許可し、L1bがMove struct/sumをOption/Result/user sumのpayloadとして
許可してtagged control flowを完成させた。

## Effects

Pure な機構である。`?` は制御フローであってエフェクトではない。関数が impure になるのは、その関数が *呼び出す* 処理を通じてのみである。

## Errors & aborts

- **未処理の `Result` はハードなコンパイルエラー** となる（lint スイートの correctness スライス、#138）。処理されずに捨てられた `Result` の文は、`?` で伝播するか、`match` するか、変数に束縛しなければならない。
- `?` は **暗黙のエラー型変換を一切行わない** — `Result<T, MyErr>` は `Result<T, Error>` を要求する文脈を素通りできない。`.map_err(to_error)` を使って明示的に変換すること。
- `main() -> Result<(), Error>`: 外へエスケープした `Err` はプロセスの終了コード（exit code）へマッピングされる。カテゴリカルな variant は `tag + 1`（`NotFound`→1、`Invalid`→2、`Denied`→3）、`Code(c)` は `c` を exit code とする（#308 の対応により、`main` のエラー型を組み込みの `Error` に限定した。`main` にユーザー定義の `E` を指定することは拒否される）。

## Regions

自身の region は持たない。ペイロードとして保持されるビュー（`Ok` の中の `str` など）はそれ自身の region を保持する。

## 仕様先行(未実装)

- **コンビネータメソッドは存在しない**: `.map`、`.and_then`、`.unwrap_or`、`.ok()`、`.is_some` / `.is_none` / `.is_ok` / `.is_err` などのメソッドは存在しない — メソッドテーブルは `map_err` だけで打ち止めとなっている。これは現時点では実装漏れではなく、意図的な *設計上のスタンス* である。`match`、`else`、`?` が揃っていれば、コンビネータ風の第 2 の制御フロー言語（方言）を増やすことなく用途はまかなえる。これらのいずれかを追加する場合は設計上の判断（一方向の One-way レビュー）を伴うため、実装の前に `open-questions.md` で議論・記録すること。
- **再帰的なタグ付き Move の範囲**: L1b-a は、タグ付き arm で既存の `Scalar` で直接表現できる Move ペイロードを受け入れ、再帰的な `Drop`、`match`、`else`、`?`、`map_err` を実装する。L1b-b は複数 Move ペイロードの部分構築も実装し、それらに一様な所有モードを要求する。L1b-c は `Result<Option<T>, E>` のようなタグ付き型を入れ子にする表現を実装する。任意の新しい Move 要素レイアウトを持つ配列、再帰型、および L2 の動的なパス選択型 return cleanup は別の課題として残る。

## Pitfalls

- P1 — コンストラクタは **修飾なし（bare）** である（`Some` / `Ok`）。ユーザー定義の sum 型（`Type.Variant` のような形式）とは異なる。ドキュメントや診断メッセージで `Option.Some` のような形式を推奨してはならない。
- P2 — ペイロードを持たないジェネリックな variant を単独で使用した場合（ユーザーのジェネリクスにおける `Opt.None` など）、型引数 `T` を推論・固定できない。組み込みの `None` の型推論は文脈に依存している。そのため、修飾なしの `None` を構築するテストには、型の注釈または制御フローの文脈が必要になる。
- P3 — 用意されているシンタックスシュガーは `error(c)` のみである。variant ごとの専用コンストラクタや自動変換を追加してはならない — 境界における `map_err` の可視性こそが重要な設計の要点である。
- P4 — exit code のマッピング規則は言語の契約の一部である（ガイドの ch04 で説明されている）。`tag + 1` という方式を変更することは、実装の詳細の変更ではなく破壊的な仕様変更（API break）である。

## Test anchors

`crates/align_driver/tests/enum_match.rs`（Error の variant、`error(c)` → exit code、`map_err` の変換、no-implicit-`?`-coercion、網羅性）。`m1.rs` / `m2.rs` での Option / Result の基本および `?`。`generics.rs:229`（ジェネリックな関数内での `o else d`）。`else_result.rs`（`Result` への `else` — Ok の素通り / Err のフォールバック / ネストした連鎖 / Move-Ok の二重解放なし / Move Err の再帰 Drop）。`owned_tagged_payloads.rs`（Move struct/sum/string/handle、全制御経路、per-unit parity）。`lint_unhandled_result.rs`。#308 における main-error の制限テスト。
例として `option.align`、`result.align`、`match_option_result.align`、`error_enum.align`。
