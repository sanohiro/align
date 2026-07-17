# std: encoding、rand、cli

> 🌐 [English](../14-std-encoding-rand-cli.md) · **日本語**

`std` ライブラリの機能紹介の第2波として、境界でのバイト列変換（エンコーディング）、乱数生成、そしてコマンドライン引数の解析について解説します。[13](13-std-os.md) 章で説明したのと同じ「3つの設計ルール」がここでも機能しています。すなわち、「明示的なインポートの要求」、「`Result` と一元化されたエラーテーブル」、そして「リソースを所有する箇所での Move セマンティクス」です。

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

「信頼境界（Trust Boundary）」は、関数の戻り値の型によって明確に語られます。**エンコード**処理は失敗する可能性がないため、直接 `string` を返します。一方、**デコード**処理は信頼できない外部からの入力を解析する操作であるため、`Result<buffer, Error>` を返します。成功時のペイロードである `buffer` は単なる生のバイト列です。デコードされた直後のデータは、それが正しい UTF-8 文字列であるという保証を持たないため、テキストとして扱う前には必ず `utf8_valid` で検証を行うか、バイナリセーフな処理にそのまま渡す必要があります。なお、`base64url_*` はパディング文字（`=`）を含まない URL セーフなアルファベットを使用し、16進数（hex）のデコードは大文字・小文字の両方を正常に受け付けます。

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

Align の乱数モジュールにおける設計上の判断（賭け）は以下の通りです。

- **`rng`（乱数生成器）は「値」であり**、隠れたグローバルな状態ではありません。`rand.seed()` を呼ぶと OS から安全なエントロピーを取得して初期化されます。`rand.seed_with(s)` は指定したシードに基づく決定的かつ環境非依存（ポータブル）な乱数列を生成し、テストやシミュレーションの正確な再現性を保証します。乱数を生成するすべてのメソッドは `mut` なレシーバを要求します。乱数生成器の状態を前に進めることは「ミューテーション（状態変更）」そのものであり、Align はミューテーションを暗黙のうちに隠すようなことはしないからです。
- 乱数を引く操作は明白に「非純粋（impure）」な副作用であるため、rng を使用するクロージャを `par_map` に渡そうとすると、コンパイル時に**拒否されます**。並列実行によって乱数の取得順序が変わり、シミュレーションの再現性が失われるという古典的なバグは、Align ではそもそもコードとして表現できないようになっています。（もしタスクごとに独立した乱数生成器を持たせたい場合は `task_group` を使用するか、あるいは事前に必要な乱数列をバッチ生成しておき、それをデータ並列のパイプラインに流し込むアプローチをとります。）
- `range` は上限を含まない半開区間 `[lo, hi)` をとり、統計的なバイアス（偏り）を生じさせません。例えば `range(1, 7)` は一般的な6面ダイス（サイコロ）の挙動になります。`lo >= hi` になっていたり、`sample` において取得する要素数 `k` が元の長さ `len` を超えているような無意味な引数が渡された場合、適当にそれらしい値を返してごまかすのではなく、大声で（明確に）プログラムをアボートして中断させます。

## `std.cli`

コマンドラインの「位置引数（Positional Arguments）」が1個か2個を超える規模になる場合は、`command` ビルダーにフラグを登録し、プログラムの唯一の引数入力である `main(args: array<str>)` を安全に解析します。

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

`flag_bool` で定義されたフラグのデフォルト値は `false` になり、`flag_str` や `flag_i64` ではデフォルト値を明示的に指定します。受け付けるコマンドラインの形式は `--name value`（bool 型の場合は単に `--name`）です。未知のフラグ、重複したフラグ、または不正な形式のフラグが渡された場合は `Error.Invalid` を返します。解析が成功した後に呼び出す getter メソッド（`get_bool` など）は全域関数（total function）として振る舞います。もし未登録のフラグ名を指定したり、登録時と異なる型を要求したりした場合は、プログラム上のロジックの誤りであるため即座にアボートします。`p.get_str` が返す文字列は、解析結果である `p` の内部データに対するビュー（参照）です。パース結果のハンドル `p` よりも長くその文字列を保持したい場合は `.clone()` でコピーを作成してください。

`command`（ビルダー）と解析結果（`parsed result`）は、どちらも Move セマンティクスを持つハンドルです。メソッドを呼び出す前に、必ずローカル変数へ束縛してください。[13](13-std-os.md) 章で触れたように、所有権を持つ無名のレシーバに対するメソッドチェーンの呼び出しは、v1 の現時点では制限として残っています。`c.usage()` は自動生成されたヘルプ（使用方法）の文字列を返し、パース処理の成否に関わらず呼び出すことができます。Align の CLI パーサーには、Rust のような `derive` マクロも属性ベースの DSL も存在しません。フラグの登録はすべて、呼び出し箇所で明白に読み取れる通常のコードとして記述されます。

標準ライブラリの次の波である、ネットワーク（TCP/UDP）、HTTP/TLS、サブプロセスの実行、データ圧縮、そして暗号化についてもすでに実装済みです。これらは第 [18](18-std-services.md) 章で紹介します。
