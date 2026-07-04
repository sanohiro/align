# 境界: unsafe と C FFI

> 🌐 [English](../15-unsafe-and-ffi.md) · **日本語**

ここまでのすべての保証(ダングリングビューがない、二重解放がない、`par_map` でデータ競合がない)は、コンパイラがすべてを見通せるからこそ成り立ちます。世界の端(C ライブラリ、手動管理のバッファ)では、それができません。Align の答えは標準的で、しかも小さく保たれています。`unsafe {}` ブロックが、保証を守るのがあなたの責任になる箇所を正確に印付けし、その外側からは何一つそれらを壊せません。

## `unsafe {}` と `raw.*`

`raw` は生ポインタ型です。5 つの `raw.*` 操作だけがそれに触れる唯一の手段で、しかも**`unsafe` の内側でのみ**合法です。

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

`alloc` / `free` / `load` / `store` / `offset`、これが unsafe の語彙のすべてです。ポインタ演算子もなく、ポインタ経由のキャスト方言もなく、grep できる 5 つの名前付き操作だけです。`raw` を*保持*すること自体は安全です(Copy 値なので、自由に受け渡せます)。ブロックが必要なのは、それを*操作*するときだけです。

`unsafe` が**しないこと**は、モード切り替えではなくマーカーだということです。アリーナのエスケープ検査、ムーブ検査、通常型への境界検査は、内側でもすべて有効です。そして純粋性推論(第 [10](10-closures-and-parallelism.md) 章)は `unsafe` を含む関数をすべて非純粋と印付けるので、生メモリのコードが `par_map` に乗ることは決してありません。

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

C のシグネチャを宣言し、`unsafe` の内側で呼び出します(コンパイラは C が何をするか検査できません。ブロックは*あなたが*確認したと告げるものです)。libc と libm は自動的に解決されます。それ以外は `link` でライブラリ名を指定します。

```align
extern "C" link("m") {
    fn sqrt(x: f64) -> f64
    fn cbrt(x: f64) -> f64
}
```

## データを渡す

スカラーは直接対応します(`i32`↔`int32_t`、`f64`↔`double`)。Align のビュー(`str`、`slice<T>`、`bytes`)はその**データポインタ**へと低下します。長さは自分で渡してください。

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

**どこかに刺青しておくべき唯一の FFI ルール:** Align の文字列は NUL 終端では*ありません*。長さを取る API(`write`、`memcmp`、`memcpy`)は安全です。`strlen`/`printf("%s")` は末尾を超えて読みます。C が構造体を欲しがるときは、`layout(C)` でレイアウトを固定します。宣言順、C のアラインメント規則、フィールド並べ替えなし(これがないと Align は密度のためにフィールドを並べ替えます)。

```align
layout(C) Point { x: i32, y: i32 }      // matches `struct { int32_t x, y; }`
```

`layout(C)` 構造体はポインタ経由(`raw` を通して)で、あるいは**値渡し**で境界を越えます(SysV x86-64 ABI、≤16 バイトのレジスタクラス構造体。clang と正確に一致します。それより大きい値渡しは実装中)。C が所有するメモリは `raw` として戻ってきます。C ポインタは長さを持たないので、何もビューを装いません。それを `raw.load` で読むか、正直な手段で得た長さでラップします。

## 規律

境界は薄く、監査可能に保ちます。1 つのモジュールが `extern` 宣言と `unsafe` ブロックを所有し、境界で変換し(ビュー + 長さを入れ、`raw` を扱って解放し、`Result` を出す)、完全に安全な API をエクスポートします。そのモジュールの呼び出し側は純粋な Align と同じ保証を得ます。`unsafe` ブロックの外側では、何一つ健全性を欠きえないからです。`grep unsafe` が監査面であり、健全なプログラムではそれは 1 ページであって、コードベース全体ではありません。
