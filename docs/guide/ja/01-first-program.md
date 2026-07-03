# はじめてのプログラム

> 🌐 [English](../01-first-program.md) · **日本語**

最小の Align プログラムはこれです。

```align
fn main() -> i32 {
    return 0
}
```

`i32` を返す `main` は C の entry point(エントリポイント)であり、その戻り値がプロセスの終了コードになります。

## 出力する

```align
fn main() -> i32 {
    print(42)
    return 0
}
```

`print` はビルトイン(組み込み関数)です。整数、浮動小数点数、`bool`、`char`、そして文字列といったプリミティブ型を扱えます。

## 値と型推論

```align
fn main() -> i32 {
    x := 10
    y := x + 5
    return y
}
```

`:=` は新しい値を束縛(bind)します。型は推論されます — ここでの `x` が `i32` になるのは、それが `i32` を返す return に流れ込むからです。制約のない整数リテラルは `i64` にデフォルトします。束縛はデフォルトで不変(immutable)であり、再代入したい場合は `mut` を付けます。

```align
fn main() -> i32 {
    mut total := 0
    total = total + 1
    return total
}
```

導入(新しい束縛を作る)には `:=`、再代入には `=` を使う点に注目してください。`mut` を付けずに再代入するとコンパイルエラーになります — ミュータビリティ(可変性)が目に見える形になっている、これも「やり方はひとつ」の現れです。

## エラーを値として扱う

失敗しうるプログラムは `Result` を返します。

```align
fn main() -> Result<(), Error> {
    n := parse_count()?
    print(n)
    return Ok(())
}
```

`?` 演算子は `Ok` を unwrap(中身を取り出す)するか、`Err` を早期リターンします — こちらが cold path(あまり通らない経路)です。例外は存在せず、エラーは `?` を通って戻ってくる普通の値です。`main` が `Result` を返す場合、`Err` からは自動的に非ゼロの終了コードが生成されます。

`Option<T>` は「値が存在しないかもしれない」ことを表す同じ考え方で、null を使いません。答えを持たないかもしれない関数は `Option` を返します。

```align
fn safe_div(a: i64, b: i64) -> Option<i64> {
    if b == 0 {
        return None
    }
    return Some(a / b)
}

// at the call site, unwrap with a default using `else`:
n := safe_div(10, 2) else 0     // n == 5
z := safe_div(10, 0) else 0     // z == 0  (None → the default)
```

## 関数

```align
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

// single-expression form
fn square(x: i64) -> i64 = x * x
```

ここまでが、書き始めるのに必要な表面積のすべてです。次は、ループを書くのをやめる番です。
