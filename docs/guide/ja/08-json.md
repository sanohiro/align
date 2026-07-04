# JSON

> 🌐 [English](../08-json.md) · **日本語**

JSON は `core` に入っています。Align が言語の中にフレームワークを抱え込みたいからではありません。「型付きレコードを入れ、型付きレコードを出す」ことが、ほぼすべてのデータプログラムの境界だからです。データ指向言語なら、その境界を速く、型付きで扱えるべきです。import は 1 つ、関数は 2 つです。

## エンコード

```align
import core.json

User { id: i64, name: str, active: bool }

fn main() -> i32 {
    u := User { id: 7, name: "ada", active: true }
    print(json.encode(u))       // {"id":7,"name":"ada","active":true}
    return 0
}
```

`json.encode(x)` は構造体を JSON オブジェクトの `str` として描き出します。文字列フィールドのエスケープは自動です。内部で使っているのは [07](07-strings-and-text.md) 章の文字列 builder であって、リフレクションも中間の DOM もありません。

## デコード ― 型は注釈から来る

`json.decode(s)` は、束縛が求める型が何であれ、それにパースします。入力は入力なので、返り値は `Result` です。

```align
import core.json

User { id: i64, active: bool }

fn parse(s: str) -> Result<User, Error> {
    u: User := json.decode(s)?      // target type = the annotation
    return Ok(u)
}

fn main() -> Result<(), Error> {
    u := parse("{\"active\": true, \"x\": 9, \"id\": 42}")?
    print(u.id)                     // 42 — field order free, unknown keys ignored
    return Ok(())
}
```

`json.decode<User>(...)` という呼び出し形式はまだありません(実装中)。今は注釈が型を運び、それが `?` を通して自然に読めます。

不正な入力、欠けたフィールド、型の不一致、範囲外の数値 ― これらはすべて `Err` になります。パニックにもならず、こっそり間違った値になることもありません。

```align
r: Result<User, Error> := json.decode("{\"id\": oops}")
match r {
    Ok(u)  => print(u.id),
    Err(_) => print("invalid json"),    // this one
}
```

## コレクションのデコード

配列は `array<T>` にデコードされます。要素はスカラーでも構造体でもかまいません。

```align
xs: array<i64> := json.decode("[3, 1, 4, 1, 5]")?
print(xs.sum())     // 14
```

そしてここがデータ指向の見返りです。**structure-of-arrays へ直接**デコードできます。

```align
import core.json

User { name: str, age: i64, active: bool }

fn main() -> Result<(), Error> {
    data := "[{\"name\":\"alice\",\"age\":30,\"active\":true},{\"name\":\"bob\",\"age\":25,\"active\":false},{\"name\":\"carol\",\"age\":41,\"active\":true}]"
    arena {
        s: soa<User> := json.decode(data)?      // parse directly into columns
        print(s.len())                          // 3
        print(s.age.sum())                      // 96
        print(s.where(.active).age.sum())       // 71
        print(s[0].name)                        // alice — a zero-copy view into `data`
    }
    return Ok(())
}
```

`soa<User>`([11](11-data-oriented.md) 章)は各フィールドをそれぞれ独立した連続した列として格納します。ここへデコードすると、パースしながら**そのまま列を組み立てます**。構造体の配列という中間物も、後からの転置もありません。しかも文字列の列は入力テキストを借用するゼロコピーのビューです。デコードが `arena` の中で行われるのはこのためです。列は arena のライフタイムを共有し、バッチ全体がまとめて寿命を終え、コンパイラがそれを守らせます。この 1 行は、たいていの手動チューニングされたデコーダを上回ります(100 万行で Rust の serde_json と互角のベンチマークが出ます)。賢い内側ループのおかげではなく、*レイアウトという判断*が仕事そのものを取り除いたからです。

## JSON プログラムのかたち

境界でパースして本物の型に変え、真ん中ではそれらの型に対するパイプラインで処理し、遠い側の境界でエンコードします。プログラムの真ん中は JSON をいっさい見ません。見えるのは `soa<User>` や `array<i64>`、つまりパイプラインや SIMD の仕組みが食べる形です。持ち回すための動的な「JSON 値」型が欲しくなったら、それは「レコード型を宣言せよ」という設計からの合図です。
