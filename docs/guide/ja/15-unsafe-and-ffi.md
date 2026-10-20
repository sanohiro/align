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

raw ポインタの操作は `null`、`alloc`、`free`、`load`、`store`、`offset` です。`resource` 型には `resource.from_raw`、`.into_raw`、`.view_from_raw` などの内部表現操作もあり、その型の権限を持つ `unsafe` コードに限定されます。`raw.null()` はネイティブ ABI の null ポインタを作ります。

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

フィールドには整数、浮動小数点数、`raw` ポインタ、および C の整数・浮動小数点型の
長さが正の固定配列（例: `dims: [i64; 8]`）を使えます。数値配列はインラインの Copy
データで、生メモリへの保存や対応 ABI の構造体値渡しでも追加の子領域を確保しません。
C フィールドの長さ 0、bool/char/raw の配列、入れ子の配列・構造体、所有権やビューを持つ要素は
対象外です。固定配列そのものを extern の引数や戻り値にはできません。
ポインタを含む構造体は Copy です。コピー・置換・返却によって参照先が複製・解放されたり、寿命が延びたりする
ことはありません。ポインタの有効性と参照先へのアクセスは、引き続き `unsafe` 内で
プログラマが管理します。通常の raw フィールドも同じ非所有の規則に従います。
ジェネリックな C 構造体はサポートしません。

`layout(C)` 構造体は `raw` ポインタ経由、または対応 ABI 上の値渡しで渡せます。
**LP64 x86-64 Linux（GNU/musl）、Apple Silicon、Linux ARM64** は、大きい構造体や
引数レジスタ不足にも対応します。x86-64 では必要な種類のレジスタにすべて収まらない
場合、その引数全体をスタックへ渡します。
コンパイラは各プラットフォームの ABI に従い、大きい引数にはアラインメントを保つ
一時的なスタックコピーを、戻り値には専用領域を使います。C が値引数を書き換えても
元の構造体は変化しません。内部のポインタは元の参照先を指したままです。
一時領域のためのヒープ確保はなく、そのアドレスを C 呼び出しの外へ保持しては
いけません。他のターゲットではポインタ経由で渡します。正確なプラットフォーム契約は
[plan138](../../impl/138-aarch64-c-record-values.md) と
[plan139](../../impl/139-sysv-memory-c-record-values.md) を参照してください。

C が所有するメモリは `raw` として返されます。C のポインタには長さが含まれないためです。値を読むには `raw.load` を使います。ビューを作る場合は、別途長さを取得して検証してください。

## ネイティブ連携をまとめる

`extern` 宣言と `unsafe` ブロックは、小さくレビューしやすいモジュールにまとめます。その中でビューと長さを渡し、`raw` ポインタの処理と解放を行い、エラーを `Result` に変換します。公開 API は、呼び出し側に対して Align の安全性を保証する必要があります。`unsafe` の検索は、ラッパーが置いている前提も含め、人が確認すべきコードを探す手がかりになります。

## 失敗を返す raw の確保

unsafe な `raw.try_alloc(size: i64) -> Result<raw, Error>` は、負のサイズや
対象環境で表現できないサイズを `Error.Invalid` とし、0 は確保せず
`Ok(raw.null())` を返します。正のサイズでは malloc と同じ系統で一度だけ
確保を試み、拒否されたら中断や再試行なしで `Error.Code(12)` を返します。
成功した領域は未初期化で、配置は `raw.alloc` と同じです（対応する64ビット環境では
最低8バイト境界）。`raw.free` で一度だけ解放するか、正のサイズの非null値を
対応する resource に移してください。常駐メモリ、過剰アラインメント、自動解放は
保証しません。既存の `raw.alloc` の動作は変わりません。

raw は Option、Result の両側、具象・ジェネリックな sum、フラットなタプル、通常の
構造体フィールドで受け渡せます。Copy/Static のままで、コピー・置換・破棄・返却が
参照先を解放したり、寿命・非null・別名・アクセス権限を与えたりしません。混在する
所有値や view は各自の解放と寿命の規則を維持します。raw 自体を要素とする collection、
box、task の結果、JSON、extern の carrier 型は今回の対象外です。

Drop hook に寿命の終わりを任せる場合は、正のサイズで確保できたポインタを
宣言済み resource に渡します。サイズ0のnull結果からresourceは構築できません。
