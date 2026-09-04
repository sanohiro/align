# JSON

> 🌐 [English](../08-json.md) · **日本語**

`core.json` は JSON テキストと型付きのレコードを相互に変換します。主に使う関数は `json.encode` と `json.decode` の2つです。

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

`json.encode(x)` は、構造体を JSON オブジェクトの文字列表現としてシリアライズします。文字列フィールドのエスケープ処理は自動的に行われます。内部的には [07](07-strings-and-text.md) 章で解説した文字列 `builder` が使用されており、実行時リフレクションや中間表現（DOM）は一切使用していません。

## デコード ― 型は注釈から来る

`json.decode(s)` は、代入先の変数が要求する型に合わせて JSON をパースします。パース処理は失敗する可能性があるため、戻り値は `Result` 型になります。

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

`json.decode<User>(...)` のような呼び出し形式は意図的に排除されています。Align では式の中で型引数を指定する構文をサポートしていないためです。変数への型注釈がパース対象の型を決定し、`?` 演算子と組み合わせることで自然なコードとして読み下せます。

不正な JSON フォーマット、必須フィールドの欠落、型の不一致、範囲外の数値など、これらはすべて `Err` として返されます。プログラムがパニック（クラッシュ）を起こすことも、気づかないうちに誤った値として処理されることもありません。

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

**Structure of Arrays（SoA）** 形式へ直接デコードすることもできます。

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
        print(s[0].name)                        // alice — clean 文字列は `data` を見る
    }
    return Ok(())
}
```

`soa<User>`（[11 章](11-data-oriented.md)）は、フィールドごとに連続した列を持ちます。デコーダは **JSON を読みながら列を構築する**ので、中間に構造体の配列を作ってから転置する必要がありません。エスケープのない文字列フィールドは入力テキストを借用し、選択されたエスケープ文字列は arena 内で一度だけデコードされます。列のライフタイムは arena と共通です。

## JSON プログラムのかたち

入力時に JSON をレコード型へデコードし、そのレコードをパイプラインで処理して、出力時にエンコードします。こうすると JSON の扱いを入出力部分にまとめられます。主な計算では `soa<User>` や `array<i64>` を使います。処理するデータに合わせてレコード型を宣言してください。
