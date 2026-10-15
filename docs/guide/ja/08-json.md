# JSON

> 🌐 [English](../08-json.md) · **日本語**

`core.json` は JSON テキストと型付きのレコードを相互に変換します。主に使う関数は `json.encode` と `json.decode` の2つです。

## エンコード

```align
import core.json

User { id: i64, name: str, active: bool }

fn main() -> Result<(), Error> {
    u := User { id: 7, name: "ada", active: true }
    print(json.encode(u)?)      // {"id":7,"name":"ada","active":true}
    return Ok(())
}
```

両 encode は `Result<string, Error>` を通じて所有する文字列を返します。
`json.encode(x)` は `Result<string, Error>` を返し、成功時は JSON テキストを所有します。
非有限 float は `Error.Invalid` となり、`?` で伝播できます。
`json.encode_bounded(x, max_bytes)` は負のバイト上限と超過も拒否します。
出力は入力と外側の arena より長く保持でき、文字列フィールドは自動で escape されます。
[詳細契約](../../impl/47-json-numeric-contract.md) を参照してください。

内部的には [07](07-strings-and-text.md) 章で解説した文字列 `builder` が使用されており、実行時リフレクションや中間表現（DOM）は一切使用していません。

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

入力の文法エラー、フィールドの欠落、型の不一致、数値の範囲超過は `Err` を返します。

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

## 借用テキストと所有テキストを選ぶ

デコード結果を入力より長く保持するなら、フィールドを `string` にします。所有レコードの配列や `array<string>` をルートとするデコードにも対応しています。

```align
import core.json

User { name: string, age: i64 }

fn load_users() -> Result<array<User>, Error> {
    input := "[{\"name\":\"Ada\",\"age\":37}]".clone()
    users: array<User> := json.decode(input)?
    return Ok(users)
}

fn main() -> Result<(), Error> {
    users := load_users()?
    print(users[0].name)    // Ada — input has already been dropped
    names: array<string> := json.decode("[\"Ada\",\"Lin\"]")?
    print(names[1])         // Lin — indexing borrows str from names
    return Ok(())
}
```

デコーダーは、arena 内でも各所有文字列と配列本体を独立して確保し、通常の再帰的な Drop で解放します。完全に所有された結果は、入力やその arena のライフタイムを保持しません。借用テキストをデコードしてから、残すフィールドを1つずつ clone する必要がなくなります。

対応済みの再帰的なレコード構造では、optional なテキストや配列を含めて `str` と `string` を混在できます。`string` はそれぞれのバイト列を所有します。エスケープのない `str` は入力を借用し、エスケープを含む `str` には明示的な外側の arena が必要です。arena がないと、選択されたエスケープ付き `str` のデコードは `Error.Code(1)` を返します。混在する結果は入力と外側の arena の両方に依存します。配列本体を所有していても、借用テキストを所有することにはなりません。どちらの形式も、エンコード結果は独立した所有文字列になります。対応する型とエラー規則の全体は [JSON の設計](../../impl/core-design/ja/json.md)を参照してください。

## JSON プログラムのかたち

入力時に JSON をレコード型へデコードし、そのレコードをパイプラインで処理して、出力時にエンコードします。こうすると JSON の扱いを入出力部分にまとめられます。主な計算では `soa<User>` や `array<i64>` を使います。処理するデータに合わせてレコード型を宣言してください。
