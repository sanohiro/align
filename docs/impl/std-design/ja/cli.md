このディレクトリには、ロードマップの本文を超えた std モジュールの Opus 実装可能な設計仕様が置かれている。
メインループ (Fable) が執筆したもので、各モジュールを実装する際の source of truth である。

# std.cli — implementation design (M10 Slice 3)

> 🌐 [English](../cli.md) · **日本語**

## Overview

`main(args: array<str>)` の `array<str>`(§17)を読むパーサ — argv の唯一のソースである(`env.args`
はない)。言語内で完結する Pure な処理で、syscall は発生しない。v1 は明示的なフラグ登録ビルダー方式であり、
struct-decode(`json.decode` 型の理想形)は derive を待つ。これが M10 の最終スライスである。

## Signatures

`draft.md` §18.2 が正典:

```text
c := cli.command(name: str)                    // builder; returns a cli command (Move)
c.flag_bool(name: str)                          // register a bool flag (default false)
c.flag_str(name: str, default: str)             // str flag with a default
c.flag_i64(name: str, default: i64)             // i64 flag with a default
c.parse(args: array<str>) -> Result<parsed, Error>
p.get_bool(name: str) -> bool                   // total after a successful parse
p.get_str(name: str) -> str
p.get_i64(name: str) -> i64
c.usage() -> string
```

## Type & ownership classification

要となる決定:

- `command` と `parsed` は **Move 型** である — 新設の `Ty::CliCommand` / `Ty::CliParsed`。内部にヒープ
  バッファを所有する(登録済みフラグのテーブル、および所有権付き `string` 値を持つ name→value のパース結
  果マップ)ため、Copy な rng 系ではなく reader/writer/buffer の Move 系の経路に従う。Drop は内部バッファ
  を解放する(フラグテーブルのエントリ、所有権付きの default/parsed 文字列を含む)。`read_dir` の
  `array<string>`(#339)と同様の deep-drop — 内部が所有する各 `string` を個別に解放する。(用語: `str` は
  借用された read-only ビュー、`string` はヒープ所有の Move 型 — これらのテーブルが所有する値は
  `string` である。)
- **`parse` は `c` を借用するだけで、消費(consume)しない。** `c.parse(args)` は `c` を可変借用として受け取
  る(フラグテーブルを読むだけでコマンドをムーブしない)。したがって `c.usage()` は parse **の後**でも呼び
  続けられる — `Err` パスでも同様であり、これはまさに usage を表示したい局面である。(もし parse が `c` を
  消費してしまうと、parse 失敗時に usage を描画できなくなる — これがこの仕様をわざわざ明記する理由であ
  る。)
- これらは、array/slice/vec/box の要素型として、また Option/Result のペイロードとして、単一の
  `scalar_arg` チョークポイントで拒否される — reader/writer と同じ扱いである(Slice 1 の前例)。
  (`parse` は `Result<parsed, Error>` を返すため、`parsed` は Result の Ok ペイロードとしては**登場す
  る** — reader/writer が Option/Result のペイロード位置で許可されたのとまったく同様に、`parsed` を Ok
  位置では許可すること。buffer の `Scalar::Buffer` の前例(#346)がそのテンプレートである。)
- `p.get_*` の戻り値: `bool`/`i64` は Copy である。`get_str` は **parse された構造体へのビュー**として
  `str` を返す — `json.decode` のフィールドビューがデコード済みの arena/value に束縛されるのと同様に、
  `p`(パース結果)にリージョン束縛される。つまり `region_of(CliGetStr) = region_of(p)` であり、`p` の
  Drop を越えて str が生き延びることは拒否され、`.clone()` はコピーして取り出す。(Static を返しては
  ならない — それは #297 のワイルドカード・トラップである。明示的な `region_of` の分岐を追加すること。)

## Effect classification

cli の演算はすべて **Pure** である(argv はすでに `main(args)` で捕捉済みなので syscall は発生しない)。
ただし `command`/`parsed` は Move なので、いずれにせよ `par_map` のクロージャに乗ることはない。それでも
効果推論の正しさのために Pure と明示しておく。

## Error policy

- `c.parse(args)` の入力エラー — 未知のフラグ、str/i64 フラグの値欠落、不正な i64 リテラル、種類の不一
  致 — は `Error.Invalid` を返す(固定のマッピングであり errno 経由の syscall パスは通らない。encoding の
  decode と同様に直接 Error を構築する)。
- `p.get_bool/get_str/get_i64` に、一度も登録されていない名前を渡した場合、または対応するフラグ種類と
  異なる型で呼んだ場合 — **ランタイム abort** とする(#345 のレビューで決定済み: Align には comptime が
  ないため、`get_*` 呼び出しをビルダーが実行時に登録したフラグ集合に対して静的検査することはできない。
  OOB インデックスや division-by-zero と同様に abort する ——「プログラマのミスは abort し、決して黙って
  誤動作しない」という方針)。これはコンパイル時エラーでも Result でもない。

## New machinery required

新設の Move `Ty` 2 種(CliCommand, CliParsed)とそのランタイム構造体・Drop。ビルダーのメソッド群
(flag_bool/str/i64)、parse、そして 3 つの total な getter と usage を、`import std.cli` 配下の sema
builtin として実装する(`name_in_scope` のシャドーイングガードは #340 のヘルパーを利用)。`CliGetStr`
(parsed へのビュー)用の新しい `region_of` 分岐が必要。新しい effect や新しい syscall はない。

## Slice breakdown

単一のスライスだが、順序は以下の通り:

1. 2 つの Move 型 + ランタイム構造体 + Drop(所有権付き文字列の deep-free)+ 全パスにわたる Gate-1
   スイープ(reader/writer と `read_dir` の `array<string>` のテンプレートに準拠)。
2. Builder: `command` + `flag_bool`/`str`/`i64`(名前・種類・デフォルト値をコマンドのテーブルに格納)。
3. `parse`: argv をトークン化する(`--name`、`--name=value`、`--name value`、draft に沿った `-` の慣習
   — v1 は最小限にとどめる: bool には `--name`、str/i64 には `--name value` と `--name=value`)。テーブ
   ルに対して検証し → 何らかの入力エラーがあれば `Error.Invalid`、なければ parsed を構築する。
4. Getter 群(total であり、未登録/型不一致では abort する)+ `usage`(テーブルからレンダリングする)。

## Pitfalls (implement carefully)

- **P1 (Move sweep)**: 新設の 2 つの Move Ty は、reader/writer とまったく同様に、すべてのパスを漏れなく
  通過しなければならない — `ty_is_move`、`tracks_region`、`null_moved_source`、Drop 挿入、
  `MoveCheck`、`EscapeCheck`、`region_of`、finalize、MIR lower、codegen、print。ここでの見落としは
  double-free または use-after-move につながる。最も高いリスク。
- **P2 (bound-receiver, #337/#338 lesson)**: `command`/`parsed` は所有権付きの Move であり、v1 では
  束縛されていない一時値をメソッドのレシーバにすることはできない(先に変数へ束縛する必要がある)。
  したがって `cli.command("x").flag_bool("v")` のようなチェーンや `c.parse(args)?.get_bool("v")` は、
  Move-temporary の drop 対応が実装されるまで拒否される — `c := cli.command("x"); c.flag_bool("v"); ...`
  および `p := c.parse(args)?; p.get_bool("v")` のように書くことを要求する。`check_cli_*` には最初から
  bound-receiver のゲートを組み込むこと(`check_reader_method`/`check_writer_method` の前例に倣う)。
- **P3 (get_str view region, #297 trap)**: `get_str` は parsed へのビューとして str を返す。その
  region は Static ではなく必ず `region_of(p)` でなければならない。明示的な `region_of` の分岐 +
  escape 拒否のテストが必要。
- **P4 (get_* runtime table lookup)**: getter は名前を静的には解決できない。codegen は parsed テーブル
  へのランタイム検索を発行し、見つからない場合・型不一致の場合は abort する。abort パスが既存の
  abort 機構(OOB/division-by-zero と同様)を使っており、黙ってデフォルト値を返すような実装になって
  いないことを確認する。
- **P5 (deep-drop)**: command のフラグテーブルと parsed の値マップは所有権付き文字列(デフォルト値、
  パース済みの値)を保持している — Drop は `read_dir` の `array<string>` の deep-free(#339)と同様に、
  それぞれを解放しなければならない。浅い解放はリークになり、二重解放はクラッシュする。

## Test checklist

- bool の有無(デフォルトは false)
- str/i64 でデフォルト値の場合と上書きされた場合
- `--name=value` と `--name value` の両形式
- 未知のフラグ → `Error.Invalid`
- 値の欠落 → `Error.Invalid`
- 不正な i64 → `Error.Invalid`
- `get_*` に未登録の名前 → abort
- `get_*` に誤った型 → abort
- `get_str` のビューが `p` を越えて escape → コンパイルエラー
- `get_str` の `.clone()` は escape してよい
- `usage()` が全フラグをレンダリングする
- command/parsed を array/box の要素にする → 拒否される
- 束縛されていない一時値をレシーバにする → 拒否される(P2)
- deep-drop によるリーク・二重解放がないこと(valgrind 相当、または既存の RSS/drop テストパターン)
- import が必須であること
