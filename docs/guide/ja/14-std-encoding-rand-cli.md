# std: encoding、regex、rand、cli

> 🌐 [English](../14-std-encoding-rand-cli.md) · **日本語**

本章ではバイト列のエンコーディング、テキスト検索、乱数生成、コマンドライン引数の解析を扱います。第 [13](13-std-os.md) 章と同じく、インポートは明示し、失敗には `Result` と共通の errno 変換規則を使い、リソースを所有する値は Move 型とします。

## `std.encoding`

Base64(標準および URL セーフ)、hex、そして UTF-8 検証です。

```align
import std.encoding

pub fn main() -> Result<(), Error> {
    print(encoding.base64_encode("foobar"))     // Zm9vYmFy
    dec := encoding.base64_decode("Zm9vYmFy")?  // Result<buffer, Error>
    print(encoding.hex_encode(dec.bytes()))     // 666f6f626172
    print(encoding.utf8_valid(dec.bytes()))     // true
    match encoding.hex_decode("zz") {
        Ok(_)  => print("ok"),
        Err(_) => print("bad hex"),             // invalid input → Error.Invalid
    }
    return Ok(())
}
```

エンコードは失敗しないため、直接 `string` を返します。デコードは入力が不正な場合もあるため、`Result<buffer, Error>` を返します。成功時の `buffer` は生のバイト列で、UTF-8 として有効とは限りません。テキストとして扱うなら `utf8_valid` で検証し、バイナリのまま扱うならバイト列を受け取る API に渡します。`base64url_*` はパディングなしの URL セーフな文字集合を使い、hex のデコードは大文字と小文字の両方を受け付けます。

## `std.regex`

パターンを一度コンパイルし、その Move ハンドルをローカル変数へ束縛して再利用します。

```align
import std.regex

pub fn main() -> Result<(), Error> {
    re := regex.compile("[A-Za-z_][A-Za-z0-9_]*")?
    print(re.is_match("answer = 42"))
    match re.find("answer = 42") {
        Some(m) => print("answer = 42"[m.start..m.end]),
        None    => print("識別子なし"),
    }
    return Ok(())
}
```

`regex.compile(pattern: str) -> Result<regex, Error>` は所有権を持つコンパイル済みハンドルを返します。`is_match` は `bool`、`find` と `find_at` は `Option<regex_match>` を返します。`regex_match` は Copy 値です。`start` と `end` は UTF-8 のバイト位置で、どちらも文字境界にあり、`end` は範囲に含みません。そのため文字列のスライスにそのまま使えます。不正なパターンやリソース上限を超えるパターンは `Error.Invalid`、一致しなければ `None` です。範囲外や文字境界でない位置を `find_at` に渡すと、プログラミングエラーとして異常終了します。

先読み・後読みと後方参照には対応していません。正規表現リテラルや暗黙のグローバルキャッシュもありません。コンパイル済みの `regex` 値を自分で保持し、再利用します。

## `std.rand`

```align
import std.rand

pub fn main() -> i32 {
    mut a := rand.seed_with(42)     // deterministic — same seed, same sequence
    mut b := rand.seed_with(42)
    print(a.next() == b.next())     // true — reproducible by construction

    mut r := rand.seed_with(123)    // rand.seed() for an OS-seeded generator
    d6 := r.range(1, 7)             // uniform in [1, 7) — a die roll

    mut xs := [10, 20, 30, 40, 50][0..5]
    r.shuffle(xs)                   // in-place permutation
    print(xs.sum())                 // 150 — same elements, new order

    hand := r.sample([1, 2, 3, 4, 5, 6][0..6], 3)   // 3 distinct picks
    print(hand.count())             // 3
    return 0
}
```

乱数 API のルールは次のとおりです。

- **`rng` は乱数生成器の状態を持つ値**です。`rand.seed()` は OS からエントロピーを取得し、`rand.seed_with(s)` は同じシードから環境によらず同じ乱数列を生成します。状態を進めるため、各メソッドには `mut` なレシーバが必要です。
- 乱数の生成は状態を変更するので、rng を使うクロージャを `par_map` に渡すと**コンパイル時に拒否されます**。並列シミュレーションでは、`task_group` の各タスクに生成器を持たせるか、乱数列を先に生成してからパイプラインで処理します。
- `range` は偏りのない半開区間 `[lo, hi)` です。`range(1, 7)` なら6面のサイコロに相当します。`lo >= hi` や、`sample` の `k > len` はプログラミングエラーとして異常終了します。

## `std.cli`

位置引数が1つか2つで済まない場合は、コマンドにフラグを登録して `main(args: array<str>)` で受け取った引数を解析します。

```align
import std.cli

pub fn main(args: array<str>) -> Result<(), Error> {
    c := cli.command("tool")
    c.flag_bool("verbose")
    c.flag_str("input", "input.json")
    c.flag_i64("count", 1)

    p := c.parse(args)?
    if p.get_bool("verbose") { print(p.get_str("input")) }
    print(p.get_i64("count"))
    return Ok(())
}
```

`flag_bool` の既定値は `false` です。`flag_str` と `flag_i64` には既定値を明示します。形式は `--name value`、bool の場合は `--name` です。未知のフラグ、重複、不正な形式は `Error.Invalid` になります。解析に成功すれば、登録済みのフラグは getter で取得できます。未登録の名前や異なる型で取得しようとすると、プログラミングエラーとして異常終了します。`p.get_str` は `p` 内の文字列へのビューを返すので、`p` より長く保持する場合は `.clone()` します。

コマンドと解析結果はどちらも Move ハンドルです。メソッドを呼ぶ前にローカル変数へ束縛してください。名前のない所有レシーバは、まだ使えません。`c.usage()` は使用方法を説明する文字列を生成し、解析の成功・失敗にかかわらず呼べます。フラグは通常の関数呼び出しで登録するので、derive マクロや属性 DSL は不要です。

ネットワーク、HTTP/TLS、プロセス、圧縮、暗号は、第 [18](18-std-services.md) 章で紹介します。
