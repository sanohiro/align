# std: encoding、rand、cli

> 🌐 [English](../14-std-encoding-rand-cli.md) · **日本語**

`std` の第 2 波は、境界でのバイト列変換、乱数、そしてコマンドライン解析です。第 [13](13-std-os.md) 章と同じ 3 つのルールが働きます。明示的なインポート、`Result` と唯一の errno テーブル、そしてリソースを所有する箇所での Move です。

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

信頼境界は型が語ります。**エンコード**は失敗しえない → `string` を直接返します。**デコード**は信頼できない入力の解析です → `Result<buffer, Error>` を返し、そのペイロードは `buffer`(生のバイト列)です。デコードされたデータは UTF-8 の保証を持たないため、テキストとして扱う前に `utf8_valid` を走らせる(あるいはバイナリセーフな処理に渡す)必要があります。`base64url_*` はパディングなしの URL セーフなアルファベットを使い、hex デコードは大文字小文字の両方を受け付けます。

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

設計上の賭けは次の通りです。

- **`rng` は値である**、隠れたグローバルではありません。`rand.seed()` は OS にエントロピーを求めます。`rand.seed_with(s)` は決定的かつポータブルで、テストやシミュレーションを正確に再現します。すべてのメソッドは `mut` レシーバを必要とします。状態を進めることは*まさに*ミューテーションであり、Align はミューテーションを隠さないからです。
- 数を引くことは目に見えて非純粋なので、rng を使うクロージャは `par_map` からコンパイル時に**拒否されます**。並列シミュレーションが再現しなくなるという古典的なバグは、そもそも表現できません。(タスクごとのジェネレータは `task_group` 経由で、あるいは乱数の列を事前生成してそれをパイプラインで処理します。)
- `range` は半開区間 `[lo, hi)` でバイアスがありません。`range(1, 7)` はサイコロです。無意味な引数(`lo >= hi`、`k > len` での `sample`)は、それらしい値を返すのではなく大声で中断します。

## `std.cli`

位置引数が 1、2 個を超えるなら、command にフラグを登録し、唯一の argv 入力である `main(args: array<str>)` を解析します。

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

`flag_bool` の既定値は `false`、`flag_str` と `flag_i64` は既定値を明示します。受理する形式は `--name value`（bool は `--name`）です。未知、重複、不正形式のフラグは `Error.Invalid` を返します。解析成功後の getter は total です。未登録の名前や違う型を要求するのはプログラム上の誤りなので abort します。`p.get_str` は `p` 内部へのビューです。parsed handle より長く残すなら clone してください。

command と parsed result はどちらも Move handle です。メソッドを呼ぶ前にローカルへ束縛してください。一般式の一時値はすでに正しく cleanup されますが、所有する無名 receiver へのチェーン呼び出しは別の v1 surface 制限として残っています。`c.usage()` は生成した usage 文字列を返し、parse の成否後にも呼べます。derive macro も attribute DSL もなく、登録は呼び出し箇所に見える通常コードです。

次の wave —— network、HTTP/TLS、process、圧縮、暗号 —— も実装済みです。第 [18](18-std-services.md) 章で紹介します。
