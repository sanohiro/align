このディレクトリには、ロードマップの本文ではカバーしきれない `std` モジュールについて、Opus がそのまま実装に着手できる粒度の設計仕様を収めている。執筆はメインループ（Fable）が担当しており、各モジュールの実装においてこれが信頼できる情報源（source of truth）となる。

# std.cli — implementation design (M10 Slice 3)

> 🌐 [English](../cli.md) · **日本語**

> **ステータス:** M10 で実装済みです。この文書は実装済みの契約（contract）と歴史的なスライス計画を併記しています。

## Overview

`main(args: array<str>)` が受け取る `array<str>`（draft §17）をパースするためのモジュールである。argv の入手経路はこれ一本に絞り、`env.args` のようなグローバル変数は用意しない。処理は言語内で完結する Pure なものであり、syscall は一切呼び出さない。
v1 では、フラグを明示的に登録していくビルダー方式を採用する。理想形である `struct-decode`（`json.decode` と同じ形式）は、`derive` 機能の実装を待ってからの対応となる。M10 の最後のスライスにあたる。

## Signatures

以下は `draft.md` §18.2 が定めるものであり、これが正典である:

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

- `command` と `parsed` は **Move 型** とし、内部に新たに `Ty::CliCommand` / `Ty::CliParsed` を設ける。どちらも内部にヒープバッファを所有する — 登録済みフラグのテーブルと、値が所有権を持つ（owned）`string` であるパース結果（name→value）マップである。したがって、Copy 可能な rng の系統ではなく、reader / writer / buffer と同じ Move の系統に属する。Drop 時にはこれら内部バッファ（所有権を持つ default / parsed 文字列を含むフラグテーブルのエントリ）を解放する。解放処理は `read_dir` の `array<string>`（#339）と同じ deep-drop であり、内部が所有する `string` を一つずつ解放していく（用語の確認: `str` は借用の read-only ビュー、`string` はヒープを所有する Move 型であり、これらのテーブルが所有するのは `string` の方である）。
- **`parse` は `c` を借用するだけで、消費（consume）しない。** `c.parse(args)` は `c` を可変借用で受け取り、フラグテーブルを読むだけでコマンド自体をムーブしない。そのため `parse` の後でも `c.usage()` を呼ぶことができる。これは `Err` パスでも同様であり、まさにヘルプメッセージを表示したい場面で有用である。仮に `parse` が `c` を消費してしまうと、パース失敗時に usage を描画できなくなる — ここでこの点を明記しているのはそのためである。
- どちらも array / slice / vec / box の要素型や Option / Result のペイロードとしては、`scalar_arg` という単一のチョークポイントで一律に拒否される — reader / writer と同じ扱いである（Slice 1 の前例）。ただし `parse` は `Result<parsed, Error>` を返すため、`parsed` は Result の `Ok` ペイロードとしては出現する。reader / writer が Option / Result のペイロード位置で例外的に許可されたのと同じ要領で、`parsed` も `Ok` 位置に限り許可される。buffer の `Scalar::Buffer`（#346）がそのテンプレートとなる。
- `p.get_*` の戻り値について。`bool` / `i64` は Copy である。`get_str` は **パース結果の構造体を指すビュー** として `str` を返し、`p`（パース結果）にリージョン束縛される — `json.decode` のフィールドビューがデコード済みの arena / value に束縛されるのと同じ関係である。したがって `region_of(CliGetStr) = region_of(p)` であり、`p` の Drop の寿命を越えて `str` を持ち出すことはコンパイルエラーとして捕捉・拒否される。取り出して保持したければ `.clone()` でコピーする。Static リージョンを返してはならない — それは #297 で問題になったワイルドカード・トラップである。`region_of` の分岐を明示的に追加すること。

## Effect classification

cli の操作はすべて **Pure** である（argv は `main(args)` の引数として捕捉済みなので、ここで syscall は発生しない）。もっとも `command` / `parsed` は Move なので、どのみち `par_map` のクロージャには乗せられない。それでも effect 推論を正しく機能させるために Pure と明示しておく。

## Error policy

- `c.parse(args)` の入力エラー — 未知のフラグ、str / i64 フラグの値の欠落、不正な i64 リテラル、フラグの種類の不一致 — はいずれも `Error.Invalid` を返す。これは固定のマッピングであり、errno 経由の syscall パスは通らない。エンコーディングの decode と同じく、Error を直接構築する。
- 一度も登録していない名前に対して、あるいはフラグの種類と食い違う型で `p.get_bool` / `get_str` / `get_i64` を呼んだ場合は **ランタイムの abort** とする。これは #345 のレビューで確定した方針である。Align には comptime がないため、`get_*` の呼び出しをビルダーが実行時に登録したフラグ集合と静的に突き合わせることはできない。そこで OOB インデックスや 0 除算と同様に abort する —「プログラマのバグは abort させ、決して黙って誤動作させない」という原則に従う。これはコンパイル時エラーでも Result によるハンドリング対象でもない。

## New machinery required

新たに必要になるのは次のとおり。Move 型の `Ty` 2 種（CliCommand, CliParsed）と、それに対応するランタイム構造体および Drop 処理。ビルダーメソッド（flag_bool / str / i64）、parse、3 つの total な getter、usage を、`import std.cli` の下にある sema 組み込み（builtin）として実装する（シャドーイング対策の `name_in_scope` ガードには #340 のヘルパーを使う）。加えて `CliGetStr`（parsed を指すビュー）用の `region_of` 分岐を新設する。
新しい effect や新しい syscall の追加は不要である。

## Slice breakdown

スライスは 1 つだが、作業の順序は次のとおり。

1. 2 つの Move 型 + ランタイム構造体 + Drop（所有権を持つ文字列の deep-free）+ 全パスにわたる Gate-1 スイープ（reader / writer と `read_dir` の `array<string>` をテンプレートにする）。
2. ビルダー: `command` と `flag_bool` / `str` / `i64`（名前・種類・デフォルト値をコマンドのテーブルに格納する）。
3. `parse`: argv をトークン化する（`--name`、`--name=value`、`--name value`、draft に従った `-` の慣習。v1 は最小限に絞り、bool は `--name` のみ、str / i64 は `--name value` と `--name=value` のみ受け付ける）。テーブルと突き合わせて検証し、入力エラーが一つでもあれば `Error.Invalid` を返し、なければ parsed を構築する。
4. getter 群（total であり、未登録や型不一致では abort する）と `usage`（テーブルからヘルプを描画する）。

## Pitfalls

- **P1 (Move sweep)**: 新設の 2 つの Move Ty は、reader / writer と全く同じように、すべての処理パスを漏れなく通す必要がある — `ty_is_move`、`tracks_region`、`null_moved_source`、drop 挿入、`MoveCheck`、`EscapeCheck`、`region_of`、finalize、MIR lowering、codegen、print。一箇所でも漏らせば double-free や use-after-move となる。最もリスクが高い部分である。
- **P2 (bound-receiver, #337/#338 の教訓)**: `command` / `parsed` は所有権を持つ Move なので、v1 では変数に束縛していない一時値をメソッドのレシーバにすることはできない（先に変数へ束縛する必要がある）。よって `cli.command("x").flag_bool("v")` のようなチェーン呼び出しや、`c.parse(args)?.get_bool("v")` は、Move 一時値の drop 対応が入るまで拒否される。代わりに `c := cli.command("x"); c.flag_bool("v"); ...` や `p := c.parse(args)?; p.get_bool("v")` と書かせる。bound-receiver のチェックゲートは最初から `check_cli_*` に組み込んでおくこと（`check_reader_method` / `check_writer_method` の前例に倣う）。
- **P3 (get_str のビューのリージョン, #297 のトラップ)**: `get_str` は parsed を指す str ビューを返す。そのリージョンは Static ではなく必ず `region_of(p)` でなければならない。`region_of` の分岐を明示的に加え、escape を適切に拒否するテストを用意すること。
- **P4 (get_* のランタイムテーブル参照)**: getter は名前をコンパイル時に静的に解決できない。codegen は parsed テーブルへのランタイム参照コードを出力し、見つからないときや型が食い違うときは abort するようにする。この abort が既存の abort 機構（OOB や 0 除算と同じ）を使い、黙ってデフォルト値を返すような実装になっていないことを確認する。
- **P5 (deep-drop)**: command のフラグテーブルと parsed の値マップは、所有権を持つ文字列（デフォルト値やパース済みの値）を抱えている。Drop はこれらを一つずつ確実に解放しなければならない。`read_dir` の `array<string>` における deep-free（#339）と同じである。浅く解放（shallow free）すればメモリリークし、二重に解放すればクラッシュする。

## Test checklist

- bool の有無（デフォルトは false）
- str / i64 のデフォルト値とコマンドライン引数による上書き
- `--name=value` と `--name value` の両形式のパース
- 未知のフラグ → `Error.Invalid`
- 値の欠落 → `Error.Invalid`
- 不正な i64 → `Error.Invalid`
- `get_*` に未登録の名前を指定 → abort
- `get_*` に誤った型を指定 → abort
- `get_str` のビューが `p` のスコープを越えて escape → コンパイルエラー
- `get_str` の `.clone()` は escape してよいことの確認
- `usage()` が登録された全フラグを描画すること
- command / parsed を array / box の要素にする → コンパイルエラーでの拒否
- 束縛していない一時値をレシーバにする → コンパイルエラーでの拒否（P2）
- deep-drop でリークも二重解放も起きないこと（valgrind 相当、または既存の RSS / drop テストパターンでの検証）
- モジュールの使用に import が必須であること
