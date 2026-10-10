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

## レコード内の固定長配列

要素数がデータの形の一部なら `[T; N]` を使います。

```align
Table { weights: [i64; 4] }

fn main() -> i32 {
    mut table := Table { weights: [2, 3, 5, 7] }
    table.weights[1] = 11
    print(table.weights.sum())    // 25
    return 0
}
```

4つの要素は `Table` の内部に直接並び、配列のヘッダーもヒープ確保もありません。`N` は10進整数リテラルで指定し、変数や定数パラメータは使えません。初期値の要素数も一致させます。要素が Copy なら配列も Copy なので、大きな表を値渡しすると中身全体をコピーします。読むだけの関数には `borrow` やスライスを渡せます（[05 章](05-memory.md)）。`table.weights[1..3]` は表のストレージを借用します。関数から返された表の配列フィールドを添字で読む場合は、先に表を名前へ束縛します。

`[T; N]` の長さは固定です。動的な長さの所有コレクションには `array<T>` を使います。`[string; 4]` は 4 個の文字列を inline に所有し、添字アクセスは `str` を借用します。配列全体の move で所有権を移します。固定長配列の入れ子や、それ以外の独立した所有権を持つスカラー要素には対応していません。要素型の詳細は[仕様](../../../draft.md#array)を参照してください。

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

バリアントの生成には型名を付けます（`Shape.Rect(3, 4)`、`Shape.Dot`）。ペイロードは位置で指定し、スカラー、借用した `str`、構造体、直和型、所有する `string`、対応済みの所有配列を格納できます。所有権を持つペイロードがあると、直和型全体が Move 型になります。破棄の対象になるのは、その値が保持しているバリアントのペイロードだけで、入れ子の所有リソースも再帰的に破棄されます。1つのバリアントに複数の所有ペイロードがある場合、メモリの確保方式を揃える必要があります。`Result<Option<T>, E>` のように入れ子にした型にも、同じエラーモデルと所有権の規則が適用されます。

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

### 整数・文字・文字列のパターン

`match` では整数と `char` のリテラル、両端を含む範囲、文字列リテラルとの完全一致も使えます。

```align
fn digit(c: char) -> bool = match c {
    '0'..='9' => true,
    _         => false,
}

fn bucket(n: i64) -> i64 = match n {
    -1 | 0 => 0,
    1..=9  => 1,
    _      => 2,
}

fn command(name: str) -> i64 = match name {
    "build" | "check" => 1,
    "run"             => 2,
    _                 => 0,
}
```

整数の `match` は型の値域全体を覆うか、`_` を含めます。文字と文字列では必ず `_` が必要です。文字列はエスケープを解釈した後のバイト列で完全一致を調べ、埋め込み NUL も比較します。`str` と `string` は借用して調べるので、clone やメモリ確保はありません。重複するリテラル・範囲パターンはエラーです。ガード（`Circle(r) if r > 10`）、浮動小数点パターン、文字列の範囲には対応していません。述語を計算する条件には `if` を使います。

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

Align には言語組み込みの直和型が1つだけ存在します。それが `Error` 型であり、`Result<T, Error>` の標準的なエラーペイロードとして使用されます。バリアントは OS との境界で必要とされるカテゴリ（`NotFound`、`Invalid`、`Denied`、`Timeout`（実行・転送のデッドライン）、およびその他のエラーを表現する `Code(i32)`）です。`Error` 型を再定義することはできませんが、他の直和型とまったく同じように `match` で処理できます。

```align
fn describe(e: Error) -> i64 = match e {
    NotFound => 1,
    Invalid  => 2,
    _        => 99,
}
```

`Error` がプログラム内でどのように伝播していくのか（`?` 演算子、`main` の終了コード、独自のエラー型の定義など）については、次の章で詳しく解説します。
