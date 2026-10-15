# CSVをJSONに集計するCLIを作る

> 🌐 [English](../practical-cli.md) · **日本語**

CSVファイルからスコアを読み、有効な行を数えてJSONで集計結果を出す小さな
プログラムを作ります。コンパイラのリポジトリとは別の場所にプロジェクトを作り、
パッケージを配置し、計算と入出力を分け、テストしてから実行ファイルを作るまでを
一通り試します。

このガイドは、現在のリポジトリのコンパイラと `pkg.csv` を使います。
[はじめる](01-getting-started.md)に従って `alignc` を導入するかビルドし、
コピーするパッケージと互換性のあるリビジョンのコンパイラを使ってください。
配布済みリリースには、リポジトリの最新機能が含まれない場合があります。
ここで使う操作は[用途別API索引](api-by-task.md)からも探せます。

## 1. プロジェクトを作る

シェルで `ALIGN_SOURCE` にAlignのチェックアウト先を指定し、アプリ用の新しい
ディレクトリを作ります。`alignc` には `PATH` が通っている必要があります。
ソースからビルドした場合は、そのチェックアウトの `target/debug` または
`target/release` を追加してください。

```bash
ALIGN_SOURCE=/path/to/align
mkdir score-report
cd score-report
cp -R "$ALIGN_SOURCE/apps/csv/pkg" ./pkg
```

`csv/internal/descriptor.align` を含めて、`pkg` 以下をすべてコピーします。
`import pkg.csv` は、このローカルのソースを参照します。パッケージのマニフェストや
ダウンロードコマンドはありません。詳しくは[パッケージ](23-packages.md)を参照してください。

次の3ファイルを作成します。完成したソースの配置は次のとおりです。

```text
score-report/
  main.align
  report.align
  scores.csv
  pkg/
    csv.align
    csv/
      internal/
        descriptor.align
```

次の入力を、改行コードをLFにして **`scores.csv`** に保存します。

```csv
active,score
true,10
false,999
true,32
```

## 2. 計算とテストをモジュールに置く

次のブロック全体を **`report.align`** に保存します。

```align
module report

import core.test
import pkg.csv

Score { active: bool, score: i32 }

pub Summary { rows: i64, active_rows: i64, score_sum: i64 }

fn csv_error(error: pkg.csv.Error) -> Error = match error {
    Invalid => Error.Invalid,
    LimitExceeded => Error.Code(-1),
}

pub fn summarize(input: str) -> Result<Summary, Error> {
    options := pkg.csv.DecodeOptions {
        header: pkg.csv.Header.Present,
        line_ending: pkg.csv.LineEnding.Lf,
        max_rows: 100000,
    }
    arena out {
        decoded: Result<soa<Score>, pkg.csv.Error> := pkg.csv.decode(input, out, options)
        rows := decoded.map_err(csv_error)?
        return Ok(Summary {
            rows: rows.len(),
            active_rows: rows.where(.active).count(),
            score_sum: rows.where(.active).score.map(fn score { score as i64 }).sum(),
        })
    }
}

test "only active scores contribute" {
    result := summarize("active,score\ntrue,10\nfalse,999\ntrue,32\n")?
    test.expect_eq(result.rows, 3)
    test.expect_eq(result.active_rows, 2)
    test.expect_eq(result.score_sum, 42)
}

test "a header without data has a zero summary" {
    result := summarize("active,score\n")?
    test.expect_eq(result.rows, 0)
    test.expect_eq(result.active_rows, 0)
    test.expect_eq(result.score_sum, 0)
}

test "invalid numeric input is an error" {
    rejected := match summarize("active,score\ntrue,oops\n") {
        Ok(_) => false,
        Err(error) => match error {
            Invalid => true,
            _ => false,
        },
    }
    test.expect(rejected)
}
```

期待される型 `soa<Score>` がCSVのスキーマになります。ヘッダー名で列を選び、
明示したオプションでLF区切りとデータ行数の上限100,000行を指定しています。
不正な構文やセルは `Error.Invalid` にし、パッケージの `LimitExceeded` は
このアプリで `Error.Code(-1)` に変換します。この対応はアプリ側の選択であり、
`pkg.csv` 自体の規則ではありません。

列は `out` に確保されます。`Summary` はCopyの整数だけを持つため、返した値に
入力やarenaへの参照は残りません。スコアは `i32` として読み、合計する前に
`i64` へ広げています。100,000件すべてが `i32` の極値でも、この合計は収まります。
フィルター後の件数と合計は、それぞれ別の集約で求めています。

ここでテストを実行します。

```bash
alignc test report.align
```

最後に `test result: ok. 3 passed; 0 failed` と表示されます。`test` 宣言は
モジュール直下に置きます。`?` がエラーを受け取るとテストは失敗します。
レコードは `==` を使えないため、アサーションでは各フィールドを比較しています。

## 3. コマンドラインの入出力を加える

次のブロック全体を **`main.align`** に保存します。

```align
module main

import core.json
import std.cli
import std.fs
import std.io
import report

pub fn main(args: array<str>) -> Result<(), Error> {
    command := cli.command("score-report")
    command.flag_str("input", "scores.csv")
    command.flag_bool("help")
    parsed := command.parse(args)?
    output := io.stdout

    if parsed.get_bool("help") {
        usage := command.usage()
        output.write(usage)?
        output.write("\n")?
        return Ok(())
    }

    path := parsed.get_str("input")
    input := fs.read_file(path)?
    summary := report.summarize(input)?
    encoded := json.encode_bounded(summary, 256)?
    output.write(encoded)?
    output.write("\n")?
    return Ok(())
}
```

ファイル読み取り、引数解析、出力は `main` が担当します。フラグから得たパスの
ビューは `parsed` が生きている間に使います。`read_file` はUTF-8の所有文字列
`string` を返し、`summarize` の `str` 引数はそれを借用します。JSONのエンコードは
別の所有文字列を返し、明示した256バイトの上限を超えれば失敗します。
標準出力への書き込み失敗も含め、`?` でエラーを伝播します。

このアプリは、デコード前にファイル全体を読みます。行数の上限は、入力バイト数や
メモリ使用量の上限ではありません。小さなローカルファイル向けに使ってください。
入力バイト数にも上限が必要な場合は、既存の[CSVアプリ](../../../apps/csv/main.align)に
バッファへの読み取り量を制限する例があります。

## 4. 検査して実行ファイルを作る

`score-report/` から次を実行します。

```bash
alignc check main.align
alignc check-per-unit main.align
alignc test main.align
alignc run main.align -- --input scores.csv
```

`test main.align` は `import report` をたどって同じ3件のテストを実行し、
`main` は呼びません。テストは `tests/` ディレクトリの走査ではなく、インポートを
通して見つけます。2つの検査コマンドは異なるフロントエンドの経路を検査します。
ネイティブコードのコンパイルとリンクまで確かめるには、ビルドまたは実行が必要です。
詳しくは[ツールチェーン](16-toolchain.md)を参照してください。

実行すると、次のJSONが1行で出力されます。

```json
{"rows":3,"active_rows":2,"score_sum":42}
```

実行ファイルをビルドし、直接起動します。

```bash
alignc build main.align --profile release
./main --input scores.csv
./main --help
```

ビルドすると現在のディレクトリに `main` ができます。テスト本体は本番用の
実行ファイルには含まれません。`--` は `alignc run` のオプションとプログラムの
引数を分けるためのもので、実行ファイルを直接起動するときには不要です。

## 5. 失敗する場合も試す

次の内容を **`bad.csv`** に保存します。

```csv
active,score
true,oops
```

次を実行します。

```bash
./main --input bad.csv
./main --input missing.csv
```

最初のコマンドは数値変換に失敗します。2つ目は `missing.csv` が存在しなければ
失敗します。どちらも終了コードは非ゼロで、JSONの集計結果は出力されません。
入力の検証とJSONのエンコードは、集計結果を書き始める前に完了します。
ただし、出力エラーでは途中まで書かれる場合があります。
`./main --input scores.csv > summary.json` とリダイレクトしても、出力ファイルの
公開がアトミックになるわけではありません。アプリの起動前に、シェルがそのファイルを
作成したり、既存の内容を切り詰めたりする場合もあります。

編集したら `alignc fmt report.align` と `alignc fmt main.align` を使い、
`alignc test main.align` とサンプル入力での実行を繰り返します。純粋なデータ処理は
`report` に、ファイルやCLIの操作は `main` に置きます。関連する操作は
[用途別API索引](api-by-task.md)、[モジュール](09-generics-and-modules.md)、
[CSVの契約](../../impl/pkg-design/ja/csv.md)から探せます。
