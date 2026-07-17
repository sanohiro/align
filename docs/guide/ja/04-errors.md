# エラー: Option、Result、そして `?`

> 🌐 [English](../04-errors.md) · **日本語**

Align のエラーモデルは1つだけです。値が存在しないかもしれない計算は `Option<T>` を返し、*失敗*する可能性のある計算は `Result<T, E>` を返します。null も例外も存在しません。エラーは `?` 演算子を通じて伝播する、通常の値として扱われます。この章では、そのエラーモデルのすべてを解説します。

## `Option<T>` —— ないかもしれない

```align
fn find_even(xs: slice<i64>) -> Option<i64> {
    if xs.any(fn x { x % 2 == 0 }) {
        return Some(xs.where(fn x { x % 2 == 0 }).min())
    }
    return None
}

fn main() -> i32 {
    a := find_even([3, 8, 5, 4][0..4]) else 0    // Some(4) → 4
    b := find_even([3, 7, 5][0..3]) else 0       // None    → the default
    print(a + b)                                  // 4
    return 0
}
```

`Some(x)` と `None` で値を構築し、**`else` によるアンラップ**で値を取り出します。`expr else default` は、ペイロードが存在すればそれを、なければデフォルト値を返します。`else` の右辺には発散する処理（早期の `return` や、プログラムを abort する関数の呼び出し）を書くこともでき、これにより「値を取り出すか、さもなくば関数から脱出する」という一般的なパターンを簡潔に記述できます。より複雑な処理が必要な場合は `match` を使って `Some(v) =>` と `None =>` に分岐します。他のすべての `match` と同様に、網羅的に記述する必要があります。

言語仕様に null が存在しないため、「チェックを忘れる」といったミスは起こり得ません。値が存在しない場合の処理をプログラマが明示するまで、型システムは安全な値 `T` を渡してくれないからです。

## `Result<T, E>` —— 失敗しうる

```align
fn parse_positive(n: i64) -> Result<i64, Error> {
    if n <= 0 { return Err(Error.Invalid) }
    return Ok(n)
}

fn run(n: i64) -> Result<i64, Error> {
    v := parse_positive(n)?     // Ok(v) unwraps; Err returns early
    return Ok(v * 10)
}

fn report(r: Result<i64, Error>) -> i64 = match r {
    Ok(v)  => v,
    Err(_) => -1,
}

fn main() -> i32 {
    print(report(run(4)))       // 40
    print(report(run(-4)))      // -1
    return 0
}
```

エラー伝播の主役は `?` 演算子です。これは `Ok` であれば値を取り出し、`Err` であればそのエラーを即座に呼び出し元へ return します。これにより、正常系の処理（ハッピーパス）を上から下へ素直に読み下すことができます。エラー発生時の経路は実行頻度の低い（cold な）パスであり、コンパイラは実際にそれを cold ブランチとして最適化して配置します。エラーを伝播させず、最終的に `Result` を*消費*（処理）する地点に到達した場合、取り得る手段は2つです。エラーの内容に応じて処理を変えるなら `match` を使い、エラーの内容を気にしないのであれば `else` を使います。`v := f() else fallback` は、`Ok` の値を取り出すか、あるいはエラーを意図的に破棄してフォールバック用の値を評価します。目的ごとに構文が1つずつ決まっています。つまり、伝播には `?`、フォールバックには `else`、エラーの検査には `match` を使用します。

## エラーは黙って捨てられない

`Result` を捨てるのは **lint ではなくコンパイルエラー**です。

```align
import std.fs

fn main() -> Result<(), Error> {
    fs.write_file("out.txt", "hi")     // error: unhandled Result
    return Ok(())
}
```

取れる手は 3 つ、いずれもソース上に見える形で。

```align
fs.write_file("out.txt", "hi")?                  // propagate
ok := fs.write_file("out.txt", "hi")             // bind it (and deal with it)
match fs.write_file("out.txt", "hi") {           // decide per case
    Ok(_)  => print(1),
    Err(_) => print(0),
}
```

## `main` は `Result` を返す —— 終了コードはそれに従う

実行が失敗する可能性のあるプログラムでは、`main` 関数の戻り値として `Result<(), Error>` を指定します。

```align
import std.fs

pub fn main(args: array<str>) -> Result<(), Error> {
    data := fs.read_file(args[1])?      // ENOENT becomes Err(NotFound)
    print(data.len())
    return Ok(())
}
```

`main` 関数から `Err` が返されると、プロセスは0以外の終了コード（非ゼロ）で終了します。各 `Error` カテゴリは小さな固定の終了コードに対応付けられています（`NotFound` → 1、`Invalid` → 2、`Denied` → 3 など）。また、`Error.Code(c)` の場合は `c` がそのまま終了コードになります。`error(c)` はこのエラーオブジェクトを生成するための短縮形です。例えば `return Err(error(7))` と書けば、プログラムは終了コード 7 で終了します。`main` の先頭にエラーを捕捉するための定型文（ボイラープレート）を書く必要はありません。このマッピングは言語の組み込み機能として提供されています。

（この関数のシグネチャは、プログラムがコマンドライン引数をどのように受け取るかも示しています。`main(args: array<str>)` が唯一の引数受け取り口であり、`args[1]` がユーザーから渡された最初の引数となります。グローバル変数や `env.args` のような仕組みはありません。）

## 自作のエラー型

任意の直和型をエラーとして扱うことができます。ただし、`?` 演算子はエラー型を暗黙的に変換しません。つまり、`Result<T, MyErr>` は、`Result<T, Error>` を返す関数内では `?` を使ってそのまま伝播させることはできません。型を合わせるためには、`map_err` を使用してエラー型の変換を明示的に行う必要があります。

```align
ParseErr { Empty, BadChar }

fn to_error(e: ParseErr) -> Error = match e {
    Empty   => Error.Invalid,
    BadChar => Error.Invalid,
}

fn inner(n: i64) -> Result<i64, ParseErr> {
    if n == 0 { return Err(ParseErr.Empty) }
    return Ok(n)
}

fn outer(n: i64) -> Result<i64, Error> {
    v := inner(n).map_err(to_error)?    // the conversion is visible at the call
    return Ok(v + 1)
}

fn show(r: Result<i64, Error>) -> i64 = match r {
    Ok(v)  => v,
    Err(_) => -1,
}

fn main() -> i32 {
    print(show(outer(9)))       // 10
    return 0
}
```

これらのルールにより、エラーモデルはプログラムの隅々まで一貫性を保ちます。失敗する可能性のある処理はすべて型によって宣言されます。発生したエラーは必ず処理されるか、明示的に伝播されます。背後で勝手に型が変換されるようなことは一切ありません。

## 身につけたい習慣

呼び出し元が使い方を誤らないような関数設計を心がけましょう。値がないことが通常の状態であれば `Option` を返し、失敗が「例外的だが実際に起こり得る」ものであれば `Result` を返し、絶対に失敗しないのであればそのままの型 `T` を返します。呼び出し側ではそれらを `?` や `else` で適切に処理し、プロセス終了コードのルーティングは `main` のシグネチャに任せるのが Align 流の書き方です。
