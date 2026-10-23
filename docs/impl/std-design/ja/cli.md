このディレクトリは、ロードマップを補う `std` モジュールの詳細な設計仕様をまとめる。
各モジュールの実装は、この仕様に従う。

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
c.flag_f64(name: str, default: f64)             // f64 flag with a default
c.parse(args: array<str>) -> Result<parsed, Error> // also accepts slice<str>
p.get_bool(name: str) -> bool                   // total after a successful parse
p.get_str(name: str) -> str
p.get_i64(name: str) -> i64
p.get_f64(name: str) -> f64
p.occurrences(name: str) -> i64                 // explicit uses; zero when absent
c.usage() -> string
```

## Type & ownership classification

同じ `parse` はサブコマンド用の `slice<str>` も受け付ける。固定長配列は
明示的に `[..]` でスライスする。選択された範囲と各文字列の参照先は同期呼び出し中
だけ借用する。その範囲の先頭要素を名前として読み飛ばし、空または1要素なら
既定値だけをパースする。範囲検査はパースより先に行い、範囲外のトークンや
別の argv 入手経路は参照しない。スライスに伴う argv の割り当てや要素コピーはない。
結果は名前と値を独立して所有し、command・argv ヘッダー・元の文字列より長く
生存できる。`get_str` は引き続き parsed の寿命に従う。UTF-8 の各要素は長さ付きで
扱い、文字列値の埋め込み NUL も保持する。文法とエラーは配列の場合と同じである。
正確な受理条件と検証は [Plan206](../../206-cli-argument-slices.md) に記録する。

肝となる決定は次のとおり。

- `command` と `parsed` は **Move 型** とし、内部に新たに `Ty::CliCommand` / `Ty::CliParsed` を設ける。どちらも内部にヒープバッファを所有する — 登録済みフラグのテーブルと、値が所有権を持つ（owned）`string` であるパース結果（name→value）マップである。したがって、Copy 可能な rng の系統ではなく、reader / writer / buffer と同じ Move の系統に属する。Drop 時にはこれら内部バッファ（所有権を持つ default / parsed 文字列を含むフラグテーブルのエントリ）を解放する。解放処理は `read_dir` の `array<string>`（#339）と同じ deep-drop であり、内部が所有する `string` を一つずつ解放していく（用語の確認: `str` は借用の read-only ビュー、`string` はヒープを所有する Move 型であり、これらのテーブルが所有するのは `string` の方である）。
- `c.parse(args)` は共有借用でフラグテーブルを読む。パース後も `c.usage()` を呼べるため、`Err` の場合にもヘルプを表示できる。
- どちらも array / slice / vec / box の要素型や Option / Result のペイロードとしては、`scalar_arg` という単一のチョークポイントで一律に拒否される — reader / writer と同じ扱いである（Slice 1 の前例）。ただし `parse` は `Result<parsed, Error>` を返すため、`parsed` は Result の `Ok` ペイロードとしては出現する。reader / writer が Option / Result のペイロード位置で例外的に許可されたのと同じ要領で、`parsed` も `Ok` 位置に限り許可される。buffer の `Scalar::Buffer`（#346）がそのテンプレートとなる。
- `p.get_*` の戻り値について。`bool` / `i64` / `f64` と指定回数は Copy である。`get_str` は **パース結果の構造体を指すビュー** として `str` を返し、`p`（パース結果）にリージョン束縛される — `json.decode` のフィールドビューがデコード済みの arena / value に束縛されるのと同じ関係である。したがって `region_of(CliGetStr) = region_of(p)` であり、`p` の Drop の寿命を越えて `str` を持ち出すことはコンパイルエラーとして捕捉・拒否される。取り出して保持したければ `.clone()` でコピーする。Static リージョンを返してはならない — それは #297 で問題になったワイルドカード・トラップである。`region_of` の分岐を明示的に追加すること。

## Effect classification

cli の操作はすべて **Pure** である（argv は `main(args)` の引数として捕捉済みなので、ここで syscall は発生しない）。もっとも `command` / `parsed` は Move なので、どのみち `par_map` のクロージャには乗せられない。それでも effect 推論を正しく機能させるために Pure と明示しておく。

## Error policy

- `c.parse(args)` の入力エラー — 未知のフラグ、str / i64 / f64 フラグの値の欠落、不正な i64 / f64 リテラル、フラグの種類の不一致 — はいずれも `Error.Invalid` を返す。これは固定のマッピングであり、errno 経由の syscall パスは通らない。エンコーディングの decode と同じく、Error を直接構築する。
- 一度も登録していない名前に対して、あるいはフラグの種類と食い違う型で `p.get_bool` / `get_str` / `get_i64` / `get_f64` を呼んだ場合は **ランタイムの abort** とする。これは #345 のレビューで確定した方針である。Align には comptime がないため、`get_*` の呼び出しをビルダーが実行時に登録したフラグ集合と静的に突き合わせることはできない。そこで OOB インデックスや 0 除算と同様に abort する —「プログラマのバグは abort させ、決して黙って誤動作させない」という原則に従う。これはコンパイル時エラーでも Result によるハンドリング対象でもない。

## 有限の浮動小数点数と指定回数

`flag_f64` の既定値は厳密に `f64` 型とする。`parse` は argv の参照や
パース結果のエントリ割り当てより前に、登録されたすべての既定値の有限性を
検査する。重複登録や argv で上書きされる既定値も対象であり、非有限値は
`Error.Invalid` となる。argv の値は完全な ASCII JSON 数値トークン
`-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?` とする。前後の空白、先頭の
プラス、不要な先頭ゼロ、16進表記、NaN、無限大、埋め込み NUL、後続バイトは
拒否する。変換はホストの丸めモードに依存せず binary64 の最近接・偶数丸めを
使う。オーバーフローは拒否し、非正規化数、符号付きゼロへのアンダーフロー、
負のゼロは許可する。用途固有の範囲検査は呼び出し側が明示する。

`occurrences` は登録済みのすべての種類で明示的な指定回数を返す。
省略時の既定値は0回、空文字の明示指定は1回、bool の繰り返しも各1回と数える。
スペース形式と等号形式を合計し、値は従来どおり最後の指定を採用する。
別トークンの値はフラグに見えてもそのまま値として消費し、回数には含めない。
同名の重複登録は従来どおり最初の種類・既定値を採用する。
毎回のパースで回数はリセットされ、回数のオーバーフローは `Error.Invalid`。
未登録名は既存 getter と同様に abort し、`get_f64` は種類の不一致も abort する。

どちらの観測も束縛済みの parsed ローカルと名前を呼び出し中だけ借用し、
割り当てを行わず、独立した Copy スカラーを返す。文字列ビューの寿命は
変わらない。パース結果は文字列コピーを所有し、失敗時は途中のエントリを
解放する。command は再利用できる。usage は登録順に f64 の既定値を既存の
表示方式で出力し、パースで拒否される前の非有限既定値も表示できる。
正確な契約とコンパイラ・ランタイムの検証範囲は
[plan181](../../181-cli-float-occurrences.md) を参照する。

## New machinery required

新たに必要になるのは次のとおり。Move 型の `Ty` 2 種（CliCommand, CliParsed）と、それに対応するランタイム構造体および Drop 処理。ビルダーメソッド（flag_bool / str / i64 / f64）、parse、4 つの total な getter、指定回数の観測、usage を、`import std.cli` の下にある sema 組み込み（builtin）として実装する（シャドーイング対策の `name_in_scope` ガードには #340 のヘルパーを使う）。加えて `CliGetStr`（parsed を指すビュー）用の `region_of` 分岐を新設する。
新しい effect や新しい syscall の追加は不要である。

## Slice breakdown

スライスは 1 つだが、作業の順序は次のとおり。

1. 2 つの Move 型 + ランタイム構造体 + Drop（所有権を持つ文字列の deep-free）+ 全パスにわたる Gate-1 スイープ（reader / writer と `read_dir` の `array<string>` をテンプレートにする）。
2. ビルダー: `command` と `flag_bool` / `str` / `i64` / `f64`（名前・種類・デフォルト値をコマンドのテーブルに格納する）。
3. `parse`: argv をトークン化する（`--name`、`--name=value`、`--name value`、draft に従った `-` の慣習。v1 は最小限に絞り、bool は `--name` のみ、str / i64 / f64 は `--name value` と `--name=value` のみ受け付ける）。テーブルと突き合わせて検証し、入力エラーが一つでもあれば `Error.Invalid` を返し、なければ parsed を構築する。
4. getter 群（total であり、未登録や型不一致では abort する）と `usage`（テーブルからヘルプを描画する）。

## Pitfalls

- **P1 (Move sweep)**: 新設の 2 つの Move Ty は、reader / writer と全く同じように、すべての処理パスを漏れなく通す必要がある — `ty_is_move`、`tracks_region`、`null_moved_source`、drop 挿入、`MoveCheck`、`EscapeCheck`、`region_of`、finalize、MIR lowering、codegen、print。一箇所でも漏らせば double-free や use-after-move となる。最もリスクが高い部分である。
- **P2 (bound-receiver, #337/#338 の教訓)**: `command` / `parsed` は所有権を持つ Move なので、v1 では変数に束縛していない一時値をメソッドのレシーバにすることはできない（先に変数へ束縛する必要がある）。よって `cli.command("x").flag_bool("v")` のようなチェーン呼び出しや、`c.parse(args)?.get_bool("v")` は、Move 一時値の drop 対応が入るまで拒否される。代わりに `c := cli.command("x"); c.flag_bool("v"); ...` や `p := c.parse(args)?; p.get_bool("v")` と書かせる。bound-receiver のチェックゲートは最初から `check_cli_*` に組み込んでおくこと（`check_reader_method` / `check_writer_method` の前例に倣う）。
- **P3 (get_str のビューのリージョン, #297 のトラップ)**: `get_str` は parsed を指す str ビューを返す。そのリージョンは Static ではなく必ず `region_of(p)` でなければならない。`region_of` の分岐を明示的に加え、escape を適切に拒否するテストを用意すること。
- **P4 (get_* のランタイムテーブル参照)**: getter は名前をコンパイル時に静的に解決できない。codegen は parsed テーブルへのランタイム参照コードを出力し、見つからないときや型が食い違うときは abort するようにする。この abort が既存の abort 機構（OOB や 0 除算と同じ）を使い、黙ってデフォルト値を返すような実装になっていないことを確認する。
- **P5 (deep-drop)**: command のフラグテーブルと parsed の値マップは、所有権を持つ文字列（デフォルト値やパース済みの値）を抱えている。Drop はこれらを一つずつ確実に解放しなければならない。`read_dir` の `array<string>` における deep-free（#339）と同じである。浅く解放（shallow free）すればメモリリークし、二重に解放すればクラッシュする。

## Test checklist

- bool の有無（デフォルトは false）
- str / i64 / f64 のデフォルト値とコマンドライン引数による上書き
- `--name=value` と `--name value` の両形式のパース
- 未知のフラグ → `Error.Invalid`
- 値の欠落 → `Error.Invalid`
- 不正な i64 / f64、非有限な浮動小数点数 → `Error.Invalid`
- `get_*` に未登録の名前を指定 → abort
- `get_*` に誤った型を指定 → abort
- `get_str` のビューが `p` のスコープを越えて escape → コンパイルエラー
- `get_str` の `.clone()` は escape してよいことの確認
- `usage()` が登録された全フラグを描画すること
- command / parsed を array / box の要素にする → コンパイルエラーでの拒否
- 束縛していない一時値をレシーバにする → コンパイルエラーでの拒否（P2）
- deep-drop でリークも二重解放も起きないこと（valgrind 相当、または既存の RSS / drop テストパターンでの検証）
- モジュールの使用に import が必須であること
