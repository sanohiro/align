# ジェネリクスとモジュール

> 🌐 [English](../09-generics-and-modules.md) · **日本語**

この章では、ジェネリックな関数と型、型パラメータに指定する組み込みの制約、ファイル単位のモジュールを扱います。

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

型パラメータは、引数または期待される結果の型から推論します。引数だけでは決まらない場合は、第 [08](08-json.md) 章の `json.decode` と同じように、束縛に型注釈を付けます。呼び出しに `f<T>(x)` のような型引数は書きません。ジェネリクスは単相化され、使われる具体型ごとに専用のコードを生成するため、実行時のディスパッチはありません。

## 型制約: `Num` ⊃ `Ord` ⊃ `Eq`

制約のない `T` には、ムーブ、保存、戻り値として返すこと以外の操作はできません。次の組み込み制約を付けると、対応する演算を使えます。

- `T: Eq` ― `==`、`!=`
- `T: Ord` ― 比較(`Eq` を含意)
- `T: Num` ― 算術(`Ord` を含意)

制約で許されていない演算を使うと、**関数の定義を検査した時点で**コンパイルエラーになります。呼び出し側で具体的な型が決まるまで、検査が先送りされることはありません。

この3つの制約は、型パラメータに対して使える演算を定めます。構造に関する制約は別の用途に使います。例えば `RegionPlain` は、リージョン内で構築できるデータを、そのフィールドも含めて所有リソースを持たない型に制限します。算術演算や等価比較を許可する制約ではありません。**ユーザー定義のトレイトやインターフェースはありません。** それ以外の振る舞いは、具体的な型と関数で表します。

## ジェネリック型

構造体と直和型（Sum type）も同様に型パラメータを取ることができ、その型は値の構築時に推論されます。

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

`Option<T>` と `Result<T, E>` も同じ仕組みを使っています。ジェネリックな関数は、ジェネリックな構造体を受け取れます（`fn first<T>(p: Pair<T>) -> T`）。型パラメータは `array` や `slice`、別のトップレベルのジェネリック型の中にも使えます。`Opt.Empty` のようにペイロードのないバリアントを作る場合は、`T` を決めるために周囲の型情報が必要です。

## モジュールはファイル

1つのファイルが1つのモジュールに対応し、`module` 宣言の名前はファイル名と一致している必要があります。`import` 文は、同じディレクトリにある兄弟ファイルを読み込みます。`pub` キーワードを付けない限り、宣言はすべてプライベート（private）になり、モジュールをまたぐ参照は常にモジュール名で修飾する必要があります。ヘッダーファイルも、マニフェストも、複雑な検索パスの設定も不要です。

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

`alignc run main.align` は、エントリファイルと同じディレクトリの `geom.align` を読み込みます。`import util.math` は `util/math.align` に対応します。インポートした名前には必ずモジュール名を付け、型は `geom.Point`、直和型のバリアントは `geom.Color.Red` と書きます。使っている箇所で所属モジュールが分かります。インポートの別名（`import x as y`）やグロブインポートはありません。

`std.fs` や `core.json` などの組み込みモジュールも `import` で使えるようになります。インポートから分かるのは、そのファイルが直接使う API です。`std` のインポートがなくても、`print` やアプリケーション・パッケージの関数を通じて I/O を行うことはあります。純粋性の検査では、コンパイラが呼び出し先も含めて副作用を推論します（第 [10](10-closures-and-parallelism.md) 章）。

## プログラムのかたち

小さなプログラムは1つのファイルで完結します。プログラムが成長すると、データの境界がモジュールの分割線になります。データ構造（レコード型）とそれに紐づく処理は別のモジュール（例えば `records.align`）へ切り出され、I/O などの外界との境界部分は `main.align` に残ります。そして、`pub` キーワードによって公開インターフェースが明示されます。参照には常にモジュール名が修飾され、可視性も明示的であるため、モジュールの本当のインターフェースは単純な `grep` で簡単に把握できます。`pub` と書かれた行そのものが、モジュールの契約（API）となるのです。
