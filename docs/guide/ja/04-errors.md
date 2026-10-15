# エラー: Option、Result、そして `?`

> 🌐 [English](../04-errors.md) · **日本語**

値を返さない場合がある計算は `Option<T>`、失敗する可能性がある計算は `Result<T, E>` を返します。Align には null や例外はありません。エラーは値として扱い、`?` で呼び出し元へ伝播します。

## `Option<T>` —— ないかもしれない

```align
fn half_if_even(n: i64) -> Option<i64> {
    if n % 2 == 0 {
        return Some(n / 2)
    }
    return None
}

fn main() -> i32 {
    a := half_if_even(8) else 0    // Some(4) → 4
    b := half_if_even(7) else 0    // None    → the default
    print(a + b)                  // 4
    return 0
}
```

`Some(x)` と `None` で値を構築し、**`else` によるアンラップ**で値を取り出します。`expr else default` は、ペイロードが存在すればそれを、なければデフォルト値を返します。`else` の右辺には発散する処理（早期の `return` や、プログラムを abort する関数の呼び出し）を書くこともでき、これにより「値を取り出すか、さもなくば関数から脱出する」という一般的なパターンを簡潔に記述できます。より複雑な処理が必要な場合は `match` を使って `Some(v) =>` と `None =>` に分岐します。他のすべての `match` と同様に、網羅的に記述する必要があります。

`half_if_even(0)` は `Some(0)`、`half_if_even(7)` は `None` を返します。どちらも `else 0` を付けると同じ数になるので、「値が0」と「値がない」を区別したい場合は `match` を使います。次に説明する `?` は `Result` 専用で、`Option` から値を取り出す用途には使えません。

`Option<T>` から値を取り出すには、`match` または `else` のフォールバックで `Some` と `None` を扱います。

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

`?` は `Ok` から値を取り出し、`Err` なら現在の関数から返します。エラーの内容を扱うには `match`、代わりの値を使うには `else` を選びます。`v := f() else fallback` は `Ok` の値を使うか、エラーを捨ててフォールバックを評価します。これらの形式で、どこでエラーを伝播・処理するかを決めます。

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
    if args.len() != 2 { return Err(Error.Invalid) }
    data := fs.read_file(args[1])?      // ENOENT becomes Err(NotFound)
    print(data.len())
    return Ok(())
}
```

`main` 関数から `Err` が返されると、プロセスは0以外の終了コード（非ゼロ）で終了します。各 `Error` カテゴリは小さな固定の終了コードに対応付けられています（`NotFound` → 1、`Invalid` → 2、`Denied` → 3、`Timeout` → 4 など）。また、`Error.Code(c)` の場合は `c` がそのまま終了コードになります。`error(c)` はこのエラーオブジェクトを生成するための短縮形です。例えば `return Err(error(7))` と書けば、プログラムは終了コード 7 で終了します。`main` の先頭にエラーを捕捉するための定型文（ボイラープレート）を書く必要はありません。このマッピングは言語の組み込み機能として提供されています。

コマンドライン引数は `main(args: array<str>)` で受け取ります。`args[1]` が最初のユーザー引数です。この例ではファイルのパスを一つ受け取り、引数の数が違えば `Error.Invalid` を返します。先に数を確かめるので、パスを渡し忘れても範囲外アクセスで停止しません。グローバルな引数リストや `env.args` はありません。

プログラム名を含む全引数の UTF-8 を検証してから、この `main` を実行します。空文字は有効です。
不正な文字列があれば `error: code 2` を報告し、本文を実行せず終了コード 2 で終了します。
引数を受け取らない `main` では、コマンドライン文字列を検証しません。

## 自作のエラー型

任意の直和型をエラーとして扱えます。呼び出す関数と呼び出し元でエラー型が同じなら、組み込みの `Error` でなくても `?` でそのまま伝播できます。エラー型が異なる場合だけ、`map_err` で明示的に変換します。次の例では、`Result<T, ParseErr>` を `Result<T, Error>` に合わせています。

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



## 身につけたい習慣

値がないこともある場合は `Option`、失敗する可能性がある場合は `Result`、失敗を報告する必要がない場合は通常の型 `T` を返します。呼び出し側では、`Result` のエラーを伝播させるなら `?`、代わりの値を使うなら `else`、内容を調べるなら `match` を使います。`main` が `Result<(), Error>` を返す場合、そこまで伝播したエラーは言語の規則に従ってプロセスの終了コードになります。
