このディレクトリには、ロードマップの本文だけでは足りない std モジュールについて、Opus がそのまま実装に
移せる粒度の設計仕様を収めている。執筆はメインループ (Fable) が担当しており、各モジュールを実装する際は
これが source of truth となる。

# std.cli — implementation design (M10 Slice 3)

> 🌐 [English](../cli.md) · **日本語**

> **ステータス:** M10 で実装済みです。この文書は実装済み contract と歴史的な slice plan を併記します。

## Overview

`main(args: array<str>)` が受け取る `array<str>`(§17)をパースするモジュールである。argv の入手経路は
これ一本に絞り、`env.args` は用意しない。処理は言語内で完結する Pure なもので、syscall は一切呼ばない。
v1 ではフラグを明示的に登録していくビルダー方式を採る。理想形である struct-decode(`json.decode` と同じ
形)は derive の実装を待つ。M10 の最後のスライスにあたる。

## Signatures

以下は `draft.md` §18.2 が定めるもので、これが正典である:

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

肝となる決定は次のとおり。

- `command` と `parsed` は **Move 型** とし、新たに `Ty::CliCommand` / `Ty::CliParsed` を設ける。どちらも
  内部にヒープバッファを所有する — 登録済みフラグのテーブルと、値が所有権付き `string` である name→value
  のパース結果マップである。したがって Copy な rng の系統ではなく、reader/writer/buffer と同じ Move の系統
  をたどる。Drop ではこれら内部バッファ(所有権付きの default/parsed 文字列を含むフラグテーブルのエント
  リ)を解放する。解放は `read_dir` の `array<string>`(#339)と同じ deep-drop で、内部が所有する
  `string` を一つずつ解放していく。(用語の確認: `str` は借用の read-only ビュー、`string` はヒープを所有
  する Move 型であり、これらのテーブルが所有するのは `string` の方である。)
- **`parse` は `c` を借用するだけで、消費(consume)しない。** `c.parse(args)` は `c` を可変借用で受け取
  り、フラグテーブルを読むだけでコマンドを move しない。おかげで parse の後でも `c.usage()` を呼べる。これ
  は `Err` パスでも同じで、まさにヘルプを出したい場面で使える。仮に parse が `c` を消費してしまうと、パース
  失敗時に usage を描画できなくなる — ここをわざわざ明記するのはそのためである。
- どちらも array/slice/vec/box の要素型や Option/Result のペイロードとしては、`scalar_arg` という単一の
  チョークポイントで拒否する — reader/writer と同じ扱いである(Slice 1 の前例)。ただし `parse` は
  `Result<parsed, Error>` を返すので、`parsed` は Result の Ok ペイロードとしては現れる。reader/writer が
  Option/Result のペイロード位置で許可されたのと同じ要領で、`parsed` も Ok 位置に限り許可する。buffer の
  `Scalar::Buffer`(#346)がそのテンプレートになる。
- `p.get_*` の戻り値について。`bool`/`i64` は Copy である。`get_str` は **パース結果の構造体を指すビュー**
  として `str` を返し、`p`(パース結果)にリージョン束縛される — `json.decode` のフィールドビューがデコー
  ド済みの arena/value に束縛されるのと同じ関係である。したがって `region_of(CliGetStr) = region_of(p)` で
  あり、`p` の Drop を越えて str を持ち出すことは拒否される。取り出したければ `.clone()` でコピーする。
  Static を返してはならない — それは #297 のワイルドカード・トラップである。`region_of` の分岐を明示的に
  追加すること。

## Effect classification

cli の操作はすべて **Pure** である(argv は `main(args)` で捕捉済みなので syscall は発生しない)。もっとも
`command`/`parsed` は Move なので、どのみち `par_map` のクロージャには乗らない。それでも effect 推論を
正しく働かせるために Pure と明示しておく。

## Error policy

- `c.parse(args)` の入力エラー — 未知のフラグ、str/i64 フラグの値の欠落、不正な i64 リテラル、種類の不一
  致 — はいずれも `Error.Invalid` を返す。これは固定のマッピングで、errno 経由の syscall パスは通らない。
  encoding の decode と同じく、Error を直接構築する。
- `p.get_bool/get_str/get_i64` を、一度も登録していない名前に対して、あるいはフラグの種類と食い違う型で
  呼んだ場合は **ランタイム abort** とする。これは #345 のレビューで確定した方針である。Align には
  comptime がないため、`get_*` の呼び出しをビルダーが実行時に登録したフラグ集合と静的に突き合わせること
  はできない。そこで OOB インデックスや division-by-zero と同様に abort する —「プログラマのミスは abort
  させ、決して黙って誤動作させない」という原則に従う。これはコンパイル時エラーでも Result でもない。

## New machinery required

新たに必要になるのは次のとおり。Move 型の `Ty` 2 種(CliCommand, CliParsed)と、そのランタイム構造体
および Drop。ビルダーメソッド(flag_bool/str/i64)、parse、3 つの total な getter、usage を、
`import std.cli` の下の sema builtin として実装する(シャドーイング対策の `name_in_scope` ガードには
#340 のヘルパーを使う)。加えて `CliGetStr`(parsed を指すビュー)用の `region_of` 分岐を新設する。
新しい effect も新しい syscall も要らない。

## Slice breakdown

スライスは 1 つだが、作業の順序は次のとおり。

1. 2 つの Move 型 + ランタイム構造体 + Drop(所有権付き文字列の deep-free)+ 全パスにわたる Gate-1
   スイープ(reader/writer と `read_dir` の `array<string>` をテンプレートにする)。
2. ビルダー: `command` と `flag_bool`/`str`/`i64`(名前・種類・デフォルト値をコマンドのテーブルに格納
   する)。
3. `parse`: argv をトークン化する(`--name`、`--name=value`、`--name value`、draft に従った `-` の慣習。
   v1 は最小限に絞り、bool は `--name`、str/i64 は `--name value` と `--name=value` のみ受ける)。テーブル
   と突き合わせて検証し、入力エラーが一つでもあれば `Error.Invalid`、なければ parsed を構築する。
4. getter 群(total で、未登録・型不一致では abort する)と `usage`(テーブルから描画する)。

## Pitfalls (implement carefully)

- **P1 (Move sweep)**: 新設の 2 つの Move Ty は、reader/writer と全く同じように、すべてのパスを漏れなく
  通す必要がある — `ty_is_move`、`tracks_region`、`null_moved_source`、drop 挿入、`MoveCheck`、
  `EscapeCheck`、`region_of`、finalize、MIR lower、codegen、print。一箇所でも漏らせば double-free か
  use-after-move になる。最もリスクが高い。
- **P2 (bound-receiver, #337/#338 の教訓)**: `command`/`parsed` は所有権付きの Move なので、v1 では束縛
  していない一時値をメソッドのレシーバにできない(先に変数へ束縛する)。よって `cli.command("x").flag_bool("v")`
  のようなチェーンや `c.parse(args)?.get_bool("v")` は、Move 一時値の drop 対応が入るまで拒否する。代わり
  に `c := cli.command("x"); c.flag_bool("v"); ...` や `p := c.parse(args)?; p.get_bool("v")` と書かせる。
  bound-receiver のゲートは最初から `check_cli_*` に組み込んでおくこと(`check_reader_method`/
  `check_writer_method` の前例に倣う)。
- **P3 (get_str のビューのリージョン, #297 のトラップ)**: `get_str` は parsed を指す str ビューを返す。
  そのリージョンは Static ではなく必ず `region_of(p)` でなければならない。`region_of` の分岐を明示的に
  加え、escape を拒否するテストを用意する。
- **P4 (get_* のランタイムテーブル参照)**: getter は名前を静的には解決できない。codegen は parsed テー
  ブルへのランタイム参照を出力し、見つからないときや型が食い違うときは abort する。この abort が既存の
  abort 機構(OOB や division-by-zero と同じ)を使い、黙ってデフォルト値を返す実装になっていないことを
  確認する。
- **P5 (deep-drop)**: command のフラグテーブルと parsed の値マップは所有権付き文字列(デフォルト値や
  パース済みの値)を抱えている。Drop はこれらを一つずつ解放しなければならない。`read_dir` の
  `array<string>` の deep-free(#339)と同じである。浅く解放すればリークし、二重に解放すればクラッシュ
  する。

## Test checklist

- bool の有無(デフォルトは false)
- str/i64 のデフォルト値と上書き
- `--name=value` と `--name value` の両形式
- 未知のフラグ → `Error.Invalid`
- 値の欠落 → `Error.Invalid`
- 不正な i64 → `Error.Invalid`
- `get_*` に未登録の名前 → abort
- `get_*` に誤った型 → abort
- `get_str` のビューが `p` を越えて escape → コンパイルエラー
- `get_str` の `.clone()` は escape してよい
- `usage()` が全フラグを描画する
- command/parsed を array/box の要素にする → 拒否
- 束縛していない一時値をレシーバにする → 拒否(P2)
- deep-drop でリークも二重解放も起きないこと(valgrind 相当、または既存の RSS/drop テストパターン)
- import が必須であること
