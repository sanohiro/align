# ジェネリクスとモジュール

> 🌐 [English](../09-generics-and-modules.md) · **日本語**

多くの言語では 2 章分にもなる仕組みですが、Align では意図的に小さく保っています。組み込みの境界が 3 つだけで完全な推論を持つジェネリクスと、ただのファイルであるモジュールです。

## ジェネリック関数

```align
fn max<T: Ord>(a: T, b: T) -> T = if a > b { a } else { b }
fn add<T: Num>(a: T, b: T) -> T = a + b
fn same<T: Eq>(a: T, b: T) -> bool = a == b
fn unwrap_or<T>(o: Option<T>, fallback: T) -> T = o else fallback

fn main() -> i32 {
    print(max(7, 12))       // 12   — T = i64, inferred
    print(max(1.5, 0.5))    // 1.5  — T = f64
    print(add(40, 2))       // 42
    print(same("a", "a"))   // true
    print(unwrap_or(Some(5), 0))    // 5
    return 0
}
```

型パラメータは**常に引数から推論されます**。turbofish もなければ、覚えたり読んだりすべき明示的なインスタンス化構文もありません。ジェネリクスはモノモーフィック化されます。使われたインスタンス化ごとに、まるで手で書いたかのように、専用のコードにコンパイルされます。実行時ディスパッチはゼロです。

## 境界: `Num` ⊃ `Ord` ⊃ `Eq`

境界のない `T` は不透明です。ムーブし、保存し、返すことはできますが、それ以外は何もできません。能力は、ちょうど 3 つの組み込み境界から来ます。

- `T: Eq` ― `==`、`!=`
- `T: Ord` ― 比較(`Eq` を含意)
- `T: Num` ― 算術(`Ord` を含意)

境界が許していない操作を使うと、**定義**がコンパイルに失敗します。呼び出し側でも、あとで、誰か別の人のビルドの中で失敗するのではありません。

これが制約システムのすべてで、それは意図的です。**ユーザー定義のトレイトやインターフェースはありません**(欠落ではなく決定です)。トレイト階層は、言語が第 2 の、チューリング完全な型レベル方言を育ててしまう場所です。人間はそれを流し読みし、AI はそれを幻覚します。Align の賭けは、3 つの境界と具体型があれば、データ指向プログラムに含まれる現実のジェネリックコードをカバーでき、それ以外はすべて素朴な関数で言い表したほうが良い、というものです。

## ジェネリック型

構造体と直和型も同じやり方でパラメータを取り、構築から推論されます。

```align
Pair<T> { a: T, b: T }

Opt<T> { Has(T), Empty }

fn sum_ints(p: Pair<i64>) -> i64 = p.a + p.b

fn main() -> i32 {
    p := Pair { a: 40, b: 2 }       // Pair<i64>, inferred
    q := Pair { a: 1.5, b: 2.5 }    // Pair<f64>
    print(sum_ints(p))              // 42
    print(q.a + q.b)                // 4.0
    o := Opt.Has(9)                 // Opt<i64>, inferred from the payload
    v := match o {
        Has(n) => n,
        Empty  => 0,
    }
    print(v)                        // 9
    return 0
}
```

`Option<T>` と `Result<T, E>` はまさにこの仕組みそのもので、言語に同梱されています。現時点の制限を正直に挙げておきます。ジェネリックな*構造体*に対するジェネリックな*関数*(`fn first<T>(p: Pair<T>) -> T`)は実装中で、ペイロードを持たないバリアントを単独で構築する(`Opt.Empty`)には `T` を確定するための文脈が必要です。

## モジュールはファイル

1 ファイル = 1 モジュールで、`module` 名はファイル名と一致しなければなりません。`import` は兄弟ファイルを取り込みます。`pub` で印を付けない限りすべて private で、モジュールをまたぐ参照は常に修飾されます。ヘッダーも、マニフェストも、検索パスの儀式もありません。

```align
// geom.align
module geom

pub Point { x: i64, y: i64 }
pub SCALE: i64 := 3
pub fn area(p: Point) -> i64 = p.x * p.y

fn hidden(x: i64) -> i64 = x        // private: invisible to importers
```

```align
// main.align
module main

import geom

fn main() -> i32 {
    p := geom.Point { x: 4, y: 5 }
    print(geom.area(p) * geom.SCALE)    // 60
    return 0
}
```

`alignc run main.align` はエントリファイルの隣にある `geom.align` を見つけます。`import util.math` は `util/math.align` に対応します。修飾の規則は絶対です。インポートした型は `geom.Point` であって、裸の `Point` にはなりません。インポートした直和型のバリアントは `geom.Color.Red` です。どのファイルのどの名前も、それがどこから来たのかを正確に教えてくれます。import リストを掘り返す考古学は不要です。エイリアス(`import x as y`)もなければ、グロブもありません。あるものを指す方法は 1 つです。

同じ `import` キーワードは、組み込みの機能モジュールも切り替えます ― `import std.fs`、`import core.json` のように。これは、ファイルが冒頭で「外の世界のどの部分に触れるか」を宣言する方法です。`std` の import が 1 つもないファイルは、I/O をいっさい行わないことが証明できます。[13](13-std-os.md) 章はこの上に築かれます。

## プログラムのかたち

小さなプログラムは 1 ファイルです。それが成長すると、継ぎ目はデータの境界になります。レコード型とそれらに対する関数がモジュール(`records.align`)へ移り、I/O の縁は `main.align` に残り、`pub` が意図的な表面に印を付けます。参照が修飾され、可視性が明示的なので、モジュールの本当のインターフェースは grep 可能です。`pub` の行が契約そのものです。
