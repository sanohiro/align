# データをモデリングする: 構造体、直和型、match

> 🌐 [English](../03-modeling-data.md) · **日本語**

Align におけるデータモデリングの手法は、主に2つあります。構造体（「これらのフィールドの集まり」）と直和型（「これらのバリアントのいずれか」）です。これに加えて、型に名前を付けるほどではない場合のためにタプルが用意されています。型宣言に**キーワードは不要**で、波かっこ `{}` の中身の記述方法によって構造体か直和型かが決定されます。

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

`Name { field: Type, ... }` が宣言、`Name { field: value, ... }` が構築、`.field` が読み出しです。スカラー値のみからなる構造体は **Copy** 型になります。変数への代入や関数への引数渡しを行うと、整数と同じように値がコピーされます。構造体はネストでき、フィールドのパスはデータの深さのぶんだけ潜れます。

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

構造体は値渡し（pass-by-value）で受け取り、値で返します。

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

再帰的な構造体（例：`Node { next: Node }`）は定義できません。Align には null が存在しないため、再帰を終了させることができないからです。一方、所有権を持つフィールド（例えば `name: string`）を含む構造体は定義可能ですが、その場合、構造体全体が Move 型として扱われるようになります。詳細については [05](05-memory.md) 章で解説します。

## 直和型

直和型（Sum type）は、複数のバリアント（列挙子）を定義します。各バリアントはペイロード（付加データ）を持つことができます。

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

バリアントを生成する際は、型名で修飾します（例：`Shape.Rect(3, 4)` や `Shape.Dot`）。これにより、コードを読む人はそのバリアントがどの型に属しているかを常に把握できます。ペイロードは位置で指定し、スカラー値や素のデータを持つ構造体を格納できます。（現在のところ、`string` のような所有権を持つ型をペイロードにすることはできませんが、よくあるユースケースは `Option` や `Result` でカバーできます。）

## `match`

`match` は直和型を分解（パターンマッチ）するための構文であり、式として評価されます。

- 各アームは**修飾なし**のバリアント名を使います。`Shape.Circle(r)` ではなく `Circle(r)`。
- ペイロードは位置で束縛されます。`Rect(w, h) => w * h`。
- `A | B => ...` は、複数のバリアントを 1 つのアームでカバーします(何も束縛しません)。
- `_ => ...` は残りをカバーします。
- **網羅性は必須です**: バリアントの扱いが1つでも漏れているとコンパイルエラーになります。これは非常に重要な設計です。将来バリアントが追加されたとき、コンパイラが修正の必要なすべての `match` 箇所を的確に教えてくれるからです。

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

`match` には、パターンガード（例：`Circle(r) if r > 10`）やリテラルパターン（例：`match n { 0 => ... }`）が**意図的に用意されていません**。`match` はあくまで直和型を分解するための機能です。数値の条件分岐には `if` を使用してください。「1つの目的には1つの道具を使う」という原則に基づいています。

## タプル

わざわざ名前を付けた型を定義するほどではない「値のペア」を扱う際に使用します。

```align
fn divmod(a: i64, b: i64) -> (i64, i64) = (a / b, a % b)

fn main() -> i32 {
    (q, r) := divmod(17, 5)     // destructure; use _ to skip a slot
    print(q * 10 + r)           // 32
    return 0
}
```

`(a, b)` で構築し、`(q, r) :=` で分解、あるいは `t.0` や `t.1` のように位置でアクセスします。もしタプルを複数の関数間で引き回すようになれば、専用の型に名前を付けるべきタイミングです。構造体はたった1行で定義できます。

## 組み込みの `Error`

Align には言語組み込みの直和型が1つだけ存在します。それが `Error` 型であり、`Result<T, Error>` の標準的なエラーペイロードとして使用されます。バリアントは OS との境界で必要とされるカテゴリ（`NotFound`、`Invalid`、`Denied`、およびその他のエラーを表現する `Code(i64)`）です。`Error` 型を再定義することはできませんが、他の直和型とまったく同じように `match` で処理できます。

```align
fn describe(e: Error) -> i64 = match e {
    NotFound => 1,
    Invalid  => 2,
    _        => 99,
}
```

`Error` がプログラム内でどのように伝播していくのか（`?` 演算子、`main` の終了コード、独自のエラー型の定義など）については、次の章で詳しく解説します。
