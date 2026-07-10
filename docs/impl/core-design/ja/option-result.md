このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同じ粒度(シグネチャ、
Move/effect の分類、エラー方針、pitfalls、テストアンカー)の正典となる設計ドキュメントを収めている。
執筆はメインループ (Fable) が担当している。

# core — Option / Result / Error

> 🌐 [English](../option-result.md) · **日本語**

## Overview

唯一の optional モデルと唯一のエラーモデル(draft §5)。`Option<T>` = 値が無いかもしれない(それも正常な
答え)。`Result<T, E>` = 理由を伴って失敗した。言語のどこにも null は無く、例外も無い。伝播は `?` ただ一つ。
表面は意図的に *狭く* してある — 分解は `match`、それにちょうど 3 つの利便機能(`else`、`?`、`map_err`)
を加えるだけである。

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

`Option<T>`/`Result<T,E>` は普通の generic sum 型である(monomorphize される)。ペイロードは sum 型の
ペイロード規則に従う — スカラーとプレーンデータの struct。**所有権付き Move ペイロードは拒否** される。
拒否は `scalar_arg` というチョークポイントで行われる — ただし std の意図的な例外(std-design のドキュメ
ントに従い、`Ok` 位置の `reader`/`writer`/`buffer`/`parsed`)がある。`Option` ペイロードがスカラーのみで
あることは、niche 最適化を検討したうえで見送った理由でもある(#312 — 今日の言語に表現可能なターゲット
型が無い)。

## Effects

Pure な機構である。`?` は制御フローであってエフェクトではない。関数が impure になるのは、それが *呼ぶ*
ものを通じてのみである。

## Errors & aborts

- **未処理の `Result` はハードなコンパイルエラー** である(lint スイートの correctness スライス、#138)。
  捨てられた `Result` の文は、`?` で伝播するか、match するか、束縛しなければならない。
- `?` は **暗黙のエラー型変換を一切行わない** — `Result<T, MyErr>` は `Result<T, Error>` の文脈を素通り
  しない。`.map_err(to_error)` で目に見える形で変換すること。
- `main() -> Result<(), Error>`: escape する `Err` はプロセスの exit code へマップされる — カテゴリカルな
  variant は `tag + 1`(`NotFound`→1、`Invalid`→2、`Denied`→3)、`Code(c)` は `c` を exit する(#308 で
  `main` のエラー型を builtin の `Error` に限定した。`main` にユーザー定義の `E` を置くのは拒否される)。

## Regions

自身の region は持たない。ペイロードのビュー(`Ok` の中の `str`)はそれ自身の region を保持する。

## 仕様先行(未実装)

- **コンビネータメソッドは無い**: `.map`、`.and_then`、`.unwrap_or`、`.ok()`、`.is_some/.is_none/
  .is_ok/.is_err` は存在しない — メソッドテーブルは `map_err` で止まる。これは今のところ事故による欠落
  ではなく *立場* である。`match` + `else` + `?` があれば、コンビネータ風の第二の制御フロー方言を生や
  さずとも用途はまかなえる。これらのいずれかを追加するのは設計判断である(One-way レビュー) — 実装前
  に `open-questions.md` に記録すること。
- **Move エラーの `else`**:エラーが *Move* 型の `Result`(`Result<T, string>`)に対する `else` は今の
  ところ却下される —— 捨てられるバッファがリークするため(enum/Result の Move ペイロードにはまだ破棄時
  ドロップがない)。今日の `Result` のエラーはすべて Copy な enum(`Error` / ユーザー定義のエラー enum)
  なので通常のケースは完全にサポートされる。Move ペイロードが破棄時ドロップを得た時点でこの制限は外れる。

## Pitfalls

- P1 — コンストラクタは **bare** である(`Some`/`Ok`)。ユーザー定義の sum 型(`Type.Variant`)とは異な
  る。ドキュメントや診断メッセージが `Option.Some` を勧めてはならない。
- P2 — ペイロードの無い generic variant 単独(ユーザーの generics における `Opt.None` 風)では `T` を固定
  できない。builtin の `None` は文脈に頼る。bare な `None` を構築するテストには注釈かフロー文脈が必要である。
- P3 — `error(c)` が唯一の sugar である。variant ごとのコンストラクタや自動変換を追加してはならない —
  境界での `map_err` の可視性こそが要点である。
- P4 — exit code のマッピングは言語の契約の一部である(ガイド ch04 が教えている)。`tag + 1` の方式を
  変えるのは実装の詳細ではなく破壊的な仕様変更である。

## Test anchors

`crates/align_driver/tests/enum_match.rs`(Error の variant、`error(c)` → exit code、`map_err` の変換、
no-implicit-`?`-coercion、網羅性); `m1.rs`/`m2.rs` の Option/Result 基本 + `?`; `generics.rs:229`
(generic な関数内の `o else d`); `else_result.rs`(`Result` への `else` —— Ok の素通し / Err の
フォールバック / ネストした連鎖 / Move-Ok の二重解放なし / Move エラーの先送り); `lint_unhandled_result.rs`; #308 の main-error 制限テスト; 例
`option.align`、`result.align`、`match_option_result.align`、`error_enum.align`。
