# JSON

> 🌐 [English](../08-json.md) · **日本語**

`core.json` は `json.encode` と `json.decode` で JSON テキストと型付きの値を相互に変換します。
スキーマが未知の場合は、arena を使うドキュメントビュー `json.doc` により、レコード型を
宣言せずに内容を調べたり、エンコードしたりできます。

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

型付きのエンコードは [07](07-strings-and-text.md) 章の文字列 `builder` に直接書き込み、実行時リフレクションや中間 DOM を使いません。

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

`json.decode<User>(...)` のように呼び出し側で型引数を書く構文はありません。束縛に対象型を注釈します。`?` を使う場合も、その注釈がデコードの期待型を与えます。

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

## スキーマが未知の場合

フィールドの形が決まっている場合は型付きデコードを使います。任意の JSON を扱う場合は、
arena 内で `json.doc` にパースし、必要な値を選びます。

```align
import core.json

fn forward_payload(input: str) -> Result<string, Error> {
    return arena {
        document := json.doc(input)?
        payload := document.get("payload")
        json.encode_bounded(payload, 4096)
    }
}

fn main() -> Result<(), Error> {
    output := forward_payload("{\"payload\": [1e999, null, \"hi\"]}")?
    print(output)    // [1e999,null,"hi"] — the parse arena is already gone
    return Ok(())
}
```

`get(name)` はオブジェクトのメンバー、`at(index)` は配列の要素を選びます。
存在しないメンバー、異なるコンテナ型、範囲外の添字は Missing ドキュメントとなり、
例外を起こさずに探索を続けられます。末端の `as_str()`、`as_i64()`、`as_f64()`、
`as_bool()` は `Option` を返します。たとえば
`payload.get("title").as_str() else "untitled"` はデフォルト値を与えます。
欠落、null、型の違いを区別する場合は `kind()` を使います。

ドキュメントやそのビューを使う間は、入力とパースした arena を生かしておきます。
オブジェクトの `keys()` と `elems()` は、対応するスライスを外側の arena に実体化します。
順序と重複キーは元のドキュメントどおりです。エンコードする部分は上の `payload` のように
名前を付けて束縛します。Missing のエンコードは `Error.Invalid` となりますが、JSON の
null は有効な値です。

ドキュメントのエンコード結果は、独立した所有文字列です。デフォルトでは構造上の空白を
取り除き、数値トークン、元の文字列エスケープ、メンバー順、重複を維持します。
そのため `1e999` も機械の浮動小数点数へ変換せずに転送できます。区切りの空白や文字列の
再エスケープを選ぶには、ドキュメントと同じ arena 内で明示的な最終引数を渡します。

```align
options := json.encode_options {
    space_after_comma: true,
    space_after_colon: true,
    reescape_strings: true,
}
output := json.encode_bounded(payload, 4096, options)?
```

3つのフィールドはすべて必須です。options 引数を省略するとすべて false になります。
再エスケープでは JSON エスケープをデコードし、通常の JSON 文字列エスケープと UTF-8 で
書き直します。Unicode 正規化や HTML エスケープは行いません。数値トークンは変わりません。
options はドキュメントのエンコードに使い、型付きエンコードの形式は従来どおりです。
空白やエスケープを含む全出力バイトが、指定値以下という上限に数えられます。負の上限や
上限超過は、部分出力を返さず `Error.Invalid` になります。この上限はパース用 arena や
ネイティブアロケータの追加領域を制限しません。

## JSON プログラムのかたち

形が既知の JSON は入力時にレコード型へデコードし、そのレコードをパイプラインで処理して、出力時にエンコードします。こうすると JSON の扱いを入出力部分にまとめられます。主な計算では `soa<User>` や `array<i64>` を使います。スキーマが未知の内容を調べたり転送したりする場合は、ドキュメントビューを使います。
