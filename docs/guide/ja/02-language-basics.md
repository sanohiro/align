# 言語の基本

> 🌐 [English](../02-language-basics.md) · **日本語**

式指向のコアを、ひと息にまとめて。束縛、型、関数、制御フロー。この章を読めば、どんな Align の関数でも読めるようになります。

## 束縛

```align
fn main() -> i32 {
    x := 10             // introduce a binding (immutable)
    y: i64 := 20        // with an explicit type annotation
    mut total := 0      // mutable — only `mut` bindings may be reassigned
    total = x + y       // reassign with `=`, not `:=`
    print(total)        // 30
    return 0
}
```

`:=` は導入、`=` は再代入です。`mut` でない束縛への再代入はコンパイルエラーになります。可変性は常に宣言の時点で見える、というわけです。型は推論されます。推論とは別の型にしたいときや、ドキュメント目的のときに注釈を付けてください。

## 文と行

Align は Go スタイルです。改行が文を終わらせ、`;` は複数の文を 1 行に詰め込むためだけに存在します。ブロックは波かっこ `{}` で区切るので、インデントには意味がありません。`.` または二項演算子で*始まる*行は、前の行の続きになります。長いパイプラインを折り返すのは、この仕組みです。

```align
fn main() -> i32 {
    total := [1, 2, 3]
        .map(fn x { x * 2 })
        .sum()
    print(total)        // 12
    return 0
}
```

## 数値型

符号付きの `i8 i16 i32 i64`、符号なしの `u8 u16 u32 u64`、浮動小数点の `f32 f64`、それに `bool` と `char`(Unicode スカラー値、`'A'`、`'あ'`)があります。制約のない整数リテラルは既定で `i64`、制約のない浮動小数点リテラルは既定で `f64` になります。**暗黙の数値変換は一切ありません**。幅の異なる型を混ぜると型エラーになり、変換は `as` で明示します。

```align
fn main() -> i32 {
    x: i8 := 127
    y := x + 1          // i8 arithmetic
    print(y)            // -128 — overflow wraps, defined two's-complement
    big := 300
    b := big as i8      // explicit narrowing
    print(b)            // 44 (300 mod 256) — and the compiler warns: lossy conversion
    return 0
}
```

この例には、意図的な決定が 2 つ込められています。

- **整数のオーバーフローは wrap する。** これは定義された二の補数の挙動であり、未定義動作でも、隠れたトラップでもありません。checked/saturating な演算が*欲しい*ときのために、仕様は明示的な `checked_*` / `saturating_*` / `wrapping_*` の形を用意していて、意図がソースに現れるようになっています。
- **narrowing(縮小変換)は明示され、監査される。** `as` による切り捨ては定義された挙動です。そしてコンパイラは、損失のある `as` すべてに警告を出すので、黙った切り捨てが紛れ込むことはありません。

ゼロ除算(および `%` のゼロ)は、実行時のハードエラーです。プログラムは abort し、黙って誤った答えを返すことは決してありません。範囲外のリテラル(`x: i8 := 200`)はコンパイルエラーです。

## すべては式

`if`、`match`、ブロック —— これらはすべて値を生みます。ブロックの値は、その末尾の式です。

```align
fn main() -> i32 {
    limit := 100
    fee := if limit > 50 { 10 } else { 25 }   // if is an expression
    x := {
        a := 3
        a * 2                                  // trailing expression = block's value
    }
    print(fee + x)                             // 16
    return 0
}
```

だからこそ Align には三項演算子が要らず、`if` の文版・式版という区別も要りません。`if` はひとつだけで、その値を使えばそれが値になります。

## 関数

```align
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

// single-expression form: `=` instead of a block
fn square(x: i64) -> i64 = x * x

fn main() -> i32 {
    print(add(square(3), 1))    // 10
    return 0
}
```

本体の形は 2 つだけ、それ以外はありません。`return` を伴うブロックか、単一式の関数のための `= expr` か。引数は不変の値です。小さな値はコピーされます。所有権を持つ型で何が起きるかは [05](05-memory.md) 章で扱います。

## ループのキーワードはない

Align には `for` も `while` もありません。これは欠落ではなく、言語の重心そのものです。データに対する反復は**パイプライン**(`xs.map(f).where(p).sum()`、[06](06-pipelines.md) 章)であり、コンパイラはそれをベクトル化可能な単一ループへ融合します。本当に逐次的な処理がまれに必要になったときは、**再帰**を使います。

```align
fn sum_to(n: i64, acc: i64) -> i64 {
    if n == 0 { return acc }
    return sum_to(n - 1, acc + n)   // tail call — compiles to a jump, not a stack frame
}

fn main() -> i32 {
    print(sum_to(10, 0))    // 55
    return 0
}
```

ループを書きたくなったら、まず*変換*が何なのかを問うてください。十中八九、それはパイプラインです。残りの一回は、上のようにアキュムレータを使った再帰で書きます。

## 名前付き定数

```align
WIDTH: i32 := 6
HEIGHT: i32 := 7
AREA: i32 := WIDTH * HEIGHT     // folded at compile time

fn main() -> i32 = AREA         // exits 42
```

トップレベルの `NAME := expr` はコンパイル時定数です。Align の他のすべてと同じくキーワードなしで、不変であり、各使用箇所で畳み込まれて埋め込まれます。定義順は問いません(定数が、あとで定義される定数を参照してもかまいません)。注釈のない整数定数は `i64` です。

## `print` とテンプレート文字列

`print(x)` はプリミティブな値と改行を書き出します。整数、浮動小数点数(`1.0` は `1.0` と、最短の往復表現で表示されます)、`bool`(`true`/`false`)、`char`、そして文字列です。テキストを組み立てるには**テンプレート文字列**があります。

```align
fn main() -> i32 {
    name := "align"
    score := 40
    print(template "Hello {name}, score={score + 2}")   // Hello align, score=42
    return 0
}
```

穴の中には式をまるごと書けます。文字列の詳しい話 —— なぜ `builder` があるのか、`+` はいつ許されるのか —— は [07](07-strings-and-text.md) 章で扱います。

---

これでスカラーのコアはすべてです。次はデータの形作り —— 構造体、直和型、そして `match`。
