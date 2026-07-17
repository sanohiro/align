# JSON

> 🌐 [English](../08-json.md) · **日本語**

JSON 処理機能は `core` パッケージに含まれています。これは Align が言語自体に巨大なフレームワークを抱え込みたいからではありません。「型付きのレコードを入力として受け取り、型付きのレコードを出力する」という処理が、ほぼすべてのデータ処理プログラムにおける境界（インターフェース）となるためです。データ指向言語である以上、その境界を高速かつ型安全に扱えるべきだと考えています。必要な `import` は1つ、主要な関数は2つだけです。

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

そしてここがデータ指向アプローチの真骨頂です。JSON の配列を**Structure of Arrays (SoA)** 形式へ直接デコードできるのです。

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

`soa<User>`（[11](11-data-oriented.md) 章参照）は、各フィールドをそれぞれ独立した連続した配列（列データ）としてメモリ上に格納します。この形式へデコードするよう指定すると、パーサは JSON を読み込みながら**直接これらの列データを組み立てていきます**。従来のような「構造体の配列」という中間データは生成されず、後からデータを転置（AoS から SoA への変換）する必要もありません。さらに、文字列の列は入力のテキストデータをそのまま借用するゼロコピーのビューとして構築されます。デコード処理が `arena` ブロックの中で行われているのはこのためです。生成された列データは arena のライフタイムを共有し、バッチ処理が終わるとまとめて破棄されます（コンパイラがこの安全性を保証します）。このたった1行のコードは、手作業でカリカリにチューニングされた大半のデコーダよりも高速に動作します（100万行の JSON パーシングにおいて、Rust の serde_json と同等のベンチマーク結果を出します）。これは「賢いループ最適化」によるものではなく、「データレイアウトを SoA にする」という判断自体が無駄な仕事を根こそぎ取り除いた結果なのです。

## JSON プログラムのかたち

システムの境界部分で JSON をパースして静的な型に変換し、システムの中核部分ではそれらの型に対してパイプライン処理を行い、反対側の境界で再び JSON にエンコードして出力する、というのが Align における基本形です。プログラムの中核部分は JSON というフォーマットを一切意識しません。そこにあるのは `soa<User>` や `array<i64>` といった、パイプライン処理や SIMD 命令が最も効率よく処理できるデータ構造だけです。もし「どんな構造の JSON でも受け取れる動的な JSON 値型」をプログラム内で引き回したくなったら、それは「適切なレコード型（構造体）を宣言すべきだ」という言語設計からのサインだと受け取ってください。
