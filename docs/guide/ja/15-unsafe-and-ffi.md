# 境界: unsafe と C FFI

> 🌐 [English](../15-unsafe-and-ffi.md) · **日本語**

コンパイラはビューの有効期間、値のムーブ、純粋性を検査し、無効なビュー、二重解放、`par_map` でのデータ競合を防ぎます。一方、C ライブラリの内部や手動のポインタ操作までは検証できません。`unsafe {}` は、プログラマ自身が安全性を確認する箇所を示します。安全なラッパーを公開するなら、呼び出し側が渡せるすべての入力に対して、その保証を守る必要があります。

## `unsafe {}` と `raw.*`

`raw` は生ポインタを表す型です。これに触れる手段は6つの `raw.*` 操作に限られており、しかもそれらは **`unsafe` ブロックの内側でのみ** 呼び出しが許可されています。

```align
fn main() -> i32 {
    unsafe {
        p := raw.alloc(16)          // 16 raw bytes
        raw.store(p, 0, 42)         // write an i64 at byte offset 0
        raw.store(p, 8, 99)
        a: i64 := raw.load(p, 0)    // read back — type from the annotation
        b: i64 := raw.load(p, 8)
        raw.free(p)                 // yours to free — a raw is never dropped
        print(a + b)                // 141
        return 0
    }
}
```

操作は `null`、`alloc`、`free`、`load`、`store`、`offset` の6つです。ポインタ演算子やポインタ経由のキャスト構文はなく、操作名で検索できます。`resource` 型の内部には `resource.from_raw`、`.into_raw`、`.view_from_raw` などの専用操作もありますが、その型自身の `unsafe` コードでしか使えません。`raw.null()` はネイティブ ABI に渡す明示的な null ポインタであり、通常の Align 値に null を追加するものではありません。

`raw` 値を**保持して受け渡すこと**は安全です。`raw` は Copy 型なので、そのために `unsafe` は要りません。`raw.*` 操作を行うときにブロックが必要になります。

`load` と `store` は、プリミティブスカラー、`raw` ポインタ、および適格な `layout(C)` 構造体を扱えます。そのためネイティブラッパーは、アドレスを整数に変換することなく、パッケージ所有の状態ブロックに C ハンドルを保持できます。`raw.store(state, 0, handle)` で保存し、`handle: raw := raw.load(state, 0)` で読み戻します。スロットの大きさ、ポインタの有効性、および実効型は引き続きプログラマの責任です。

`unsafe` の中でも、アリーナからの持ち出し検査、ムーブ検査、通常の型の境界検査は有効です。すべての検査を無効にする指定ではありません。また、純粋性推論（第 [10](10-closures-and-parallelism.md) 章）では `unsafe` を含む関数は非純粋と判定されるため、`par_map` には渡せません。

## `extern "C"` — 外の世界を宣言する

```align
extern "C" {
    fn abs(x: i32) -> i32
    fn labs(x: i64) -> i64
}

fn main() -> i32 {
    unsafe {
        print(abs(-7))      // 7 — a real libc call
        print(labs(-40))    // 40
        return 0
    }
}
```

C 関数のシグネチャを宣言し、`unsafe` 内で呼び出します。宣言が正しく、呼び出しが C API の要件を満たすことを自分で確認してください。`libc` と `libm` は自動的にリンクされます。それ以外のライブラリは `link` で指定します。

```align
extern "C" link("m") {
    fn sqrt(x: f64) -> f64
    fn cbrt(x: f64) -> f64
}
```

## データを渡す

スカラーは C の型と直接対応します（`i32` ↔ `int32_t`、`f64` ↔ `double`）。Align のビュー（`str`、`slice<T>`、`bytes`）は、C への呼び出しでは**データポインタとして渡されます**。長さは渡されないので、別の引数として明示してください。

```align
extern "C" fn write(fd: i32, buf: str, count: i64) -> i64

fn main() -> i32 {
    msg := "written by libc\n"
    unsafe {
        n := write(1, msg, msg.len())   // fd 1 = stdout
        print(n)                        // 16
        return 0
    }
}
```

**Align の文字列は NUL 終端ではありません。** `write`、`memcmp`、`memcpy` など、長さを受け取る C API を使い、正しい長さを渡してください。`strlen` や `printf("%s")` は NUL 終端を前提とするため、ビューの末尾を超えて読むおそれがあります。

C に構造体を渡すときは、`layout(C)` で宣言順と C のアライメント規則を維持します。この指定がなければ、Align はメモリを詰めて配置するためにフィールドを並べ替える場合があります。

```align
layout(C) Point { x: i32, y: i32 }      // matches `struct { int32_t x, y; }`
```

`layout(C)` 構造体は `raw` ポインタ経由で渡せます。**値渡しに対応するのは、SysV ABI を使う x86-64 Linux だけです。** 構造体全体が引数または戻り値のレジスタに収まり、16バイト以下である必要があります。それより大きい構造体や、先行する引数でレジスタが足りなくなる宣言は拒否されます。Apple Silicon などの別ターゲットではポインタで渡してください。`layout(C)` を付けるだけでは、そこでの値渡しは可能になりません。

C が所有するメモリは `raw` として返されます。C のポインタには長さが含まれないためです。値を読むには `raw.load` を使います。ビューを作る場合は、別途長さを取得して検証してください。

## ネイティブ連携をまとめる

`extern` 宣言と `unsafe` ブロックは、小さくレビューしやすいモジュールにまとめます。その中でビューと長さを渡し、`raw` ポインタの処理と解放を行い、エラーを `Result` に変換します。公開 API は、呼び出し側に対して Align の安全性を保証する必要があります。`unsafe` の検索は、ラッパーが置いている前提も含め、人が確認すべきコードを探す手がかりになります。
