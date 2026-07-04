# エラー: Option、Result、そして `?`

> 🌐 [English](../04-errors.md) · **日本語**

Align のエラーモデルはひとつです。値を生まないかもしれない計算は `Option<T>` を返し、*失敗*しうる計算は `Result<T, E>` を返します。null はなく、例外もありません —— エラーは、`?` を通って戻ってくる普通の値です。この章がモデルのすべてです。

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

`Some(x)` と `None` が構築します。**`else` アンラップ**が取り出します。`expr else default` は、ペイロードか、なければ既定値を返します。`else` のアームは発散する(`return`、あるいは abort する呼び出し)こともできて、これが「アンラップするか脱出するか」の見た目になります。もっと込み入った処理なら `match` します —— `Some(v) =>` / `None =>`、あらゆる match と同じく網羅的に。

言語に null はないので、「チェックし忘れる」ということが起きません。値がないときにどうするかを言うまで、型システムは `T` を渡してくれないのです。

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

`?` が伝播のすべてです。`Ok` をアンラップするか、`Err` を即座に呼び出し元へ返すか。ハッピーパスは上から下へ読み下せます。エラーパスは冷たい(cold)縁であり、コンパイラは文字どおりそれを cold ブランチとして配置します。ついに `Result` を*消費する*地点まで来たら、それを `match` します。(`else` は Option 専用です。`Result` はあなたが見るか、あるいは受け渡すべきエラーを運んでいるのであって、既定値で塗りつぶすものではありません。)

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

失敗しうるプログラムは、`main` に `Result<(), Error>` の型を与えます。

```align
import std.fs

pub fn main(args: array<str>) -> Result<(), Error> {
    data := fs.read_file(args[1])?      // ENOENT becomes Err(NotFound)
    print(data.len())
    return Ok(())
}
```

`Err` が `main` の外へ伝播すると、プロセスは非ゼロで終了します。各 `Error` カテゴリは小さな固定コードに対応し(`NotFound` → 1、`Invalid` → 2、`Denied` → 3)、`Error.Code(c)` は `c` で終了します。`error(c)` はその運搬役を作る短縮形です。`return Err(error(7))` は 7 で終了します。`main` の先頭にハンドラの定型文を書く必要はありません。この対応づけは言語の一部です。

(このシグネチャは、プログラムが引数をどう受け取るかも示しています。`main(args: array<str>)` が唯一の argv です —— `args[1]` が最初のユーザー引数です。グローバル変数も `env.args` もありません。)

## 自作のエラー型

どんな直和型でもエラーになれます。ただし `?` はエラー型を暗黙には変換しません —— `Result<T, MyErr>` は、`Result<T, Error>` を返す関数を通して伝播しません。`map_err` で目に見える形で変換します。

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

ひとつの規則が、モデルを端から端まで誠実に保ちます。失敗しうるものはすべて、それを型で宣言する。あらゆる失敗は、扱われるか、目に見える形で伝播される。そして何も、あなたの背後で変換されない。

## 身につけたい習慣

呼び出し元が誤用できないように関数を設計しましょう。不在が普通なら `Option` を返し、失敗が「例外的だが現実にありうる」なら `Result` を返し、本当に失敗しえないなら素の `T` を返します。あとはすべてを `?` で呼び出し、終了コードの配管は `main` のシグネチャに任せます。
