# データをモデリングする: 構造体、直和型、match

> 🌐 [English](../03-modeling-data.md) · **日本語**

Align で型を組み立てる方法は、ちょうど 2 つ —— 構造体(「これらのフィールドすべて」)と直和型(「これらのバリアントのどれか」) —— に、名前を付けたくない場合のためのタプルを加えたものです。型宣言は**キーワードなし**です。波かっこの中身が、どちらを書いたかを決めます。

## 構造体

```align
Point { x: i64, y: i64 }

fn main() -> i32 {
    mut p := Point { x: 3, y: 4 }
    p.y = 10                        // field write needs a `mut` binding
    print(p.x + p.y)                // 13
    return 0
}
```

`Name { field: Type, ... }` が宣言、`Name { field: value, ... }` が構築、`.field` が読み出しです。素のデータからなる構造体は **Copy** 値です。代入や引き渡しをするとコピーされます —— 整数と同じです。構造体はネストでき、フィールドのパスはデータの深さのぶんだけ潜れます。

```align
Point { x: i64, y: i64 }
Line  { a: Point, b: Point }

fn main() -> i32 {
    mut l := Line { a: Point{x: 1, y: 2}, b: Point{x: 3, y: 4} }
    l.a.x = 100                     // deep write
    l.b = Point { x: 30, y: 40 }    // replace a whole nested struct
    print(l.a.x + l.b.y)            // 140
    return 0
}
```

構造体は値渡し・値返しです。

```align
Point { x: i64, y: i64 }

fn sum(p: Point) -> i64 = p.x + p.y
fn flip(p: Point) -> Point = Point { x: p.y, y: p.x }

fn main() -> i32 {
    p := Point { x: 1, y: 9 }
    print(sum(flip(p)))     // 10
    return 0
}
```

再帰的な構造体(`Node { next: Node }`)は拒否されます —— それを終端させる null が存在しないからです。所有権を持つフィールド(たとえば `name: string`)を含む構造体は正当で、その構造体全体を Move 型に変えます。その話は [05](05-memory.md) 章で扱います。

## 直和型

直和型はバリアントを並べます。バリアントはペイロードを持てます。

```align
Shape { Circle(i64), Rect(i64, i64), Dot }

fn area(s: Shape) -> i64 = match s {
    Circle(r)  => 3 * r * r,
    Rect(w, h) => w * h,
    Dot        => 0,
}

fn main() -> i32 {
    print(area(Shape.Rect(3, 4)))   // 12
    print(area(Shape.Dot))          // 0
    return 0
}
```

構築は修飾付きです —— `Shape.Rect(3, 4)`、`Shape.Dot` —— なので、そのバリアントがどの型に属するかを読み手は常に知ることができます。ペイロードは位置指定です。スカラーか、素のデータからなる構造体を置けます。(`string` のような所有権を持つペイロードは今のところ拒否されます。よくあるケースは `Option`/`Result` でカバーできます。)

## `match`

`match` は直和型を分解する手段であり、式です。

- 各アームは**修飾なし**のバリアント名を使います。`Shape.Circle(r)` ではなく `Circle(r)`。
- ペイロードは位置で束縛されます。`Rect(w, h) => w * h`。
- `A | B => ...` は、複数のバリアントを 1 つのアームでカバーします(何も束縛しません)。
- `_ => ...` は残りをカバーします。
- **網羅は必須です。** バリアントを 1 つでも忘れると、プログラムはコンパイルできません。これこそが要点です。来月バリアントを追加すれば、コンパイラは判断を要するすべての `match` を列挙してくれます。

```align
Signal { Red, Yellow, Green, Off }

fn go(s: Signal) -> i64 = match s {
    Red | Yellow => 0,
    Green        => 1,
    _            => 0,      // Off
}

fn main() -> i32 {
    print(go(Signal.Green))     // 1
    return 0
}
```

`match` が意図的に**持たない**もの。ガード(`Circle(r) if r > 10`)とリテラルパターン(`match n { 0 => ... }`)です。`match` は直和型のためのものです。数値には `if` を書きます。ひとつの道具はひとつの仕事に。

## タプル

名前付きの型に値しない「値のペア」のために。

```align
fn divmod(a: i64, b: i64) -> (i64, i64) = (a / b, a % b)

fn main() -> i32 {
    (q, r) := divmod(17, 5)     // destructure; use _ to skip a slot
    print(q * 10 + r)           // 32
    return 0
}
```

`(a, b)` で構築、`(q, r) :=` で分解、あるいは位置でインデックスします。`t.0`、`t.1`。もしタプルを 2 つ以上の関数に通し回しているなら、名前を付けましょう。構造体はたった 1 行です。

## 組み込みの `Error`

言語に同梱される直和型がひとつあります。`Error`、`Result<T, Error>` の標準エラーペイロードです。そのバリアントは OS 境界が必要とするカテゴリ —— `NotFound`、`Invalid`、`Denied`、そしてそれ以外すべてのための `Code(i64)` —— です。`Error` を再宣言することはできませんが、他のどの直和型とも同じように match できます。

```align
fn describe(e: Error) -> i64 = match e {
    NotFound => 1,
    Invalid  => 2,
    _        => 99,
}
```

`Error` がプログラムをどう流れるか —— `?`、`main` の終了コード、あなた自身のエラー型 —— は次の章で扱います。
