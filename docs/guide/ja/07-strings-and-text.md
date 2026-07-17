# 文字列とテキスト

> 🌐 [English](../07-strings-and-text.md) · **日本語**

Align におけるテキスト処理は、[05](05-memory.md) 章で解説したメモリモデルに厳格に従います。ライフタイムが2種類あるため、文字列型も2種類用意されています。また、メモリ確保はすべてコード上で明示的に行われます。ここでこのパターンを一度理解してしまえば、Align における他のあらゆるリソース管理にも同じパターンが適用できることがわかるでしょう。

## `str` と `string`

- **`str`** ― 借用された、変更不可（イミュータブル）の**ビュー**です。内部的にはポインタとバイト長のペアであり、文字列リテラルはこの `str` 型になります。コピーはコストゼロ（無料）であり、指し示すデータの有効範囲（リージョン）を引き継ぎます。
- **`string`** ― **所有権を持つ**ヒープ上のバッファです。Move 型であるため、別の変数への代入は所有権の移動を意味し、所有者のライフタイムが終了するとバッファは自動的に破棄されます。

`string` は `.clone()`（ディープコピー）を呼び出すか、後述する `builder` を使って生成します。また、所有権を持つ `string` は、`str` が期待される場所では**自動的に借用（ビューへの変換）が行われます**。そのため、`str` を受け取る関数に `string` を渡してもパフォーマンス上のコストはかからず、所有権が消費（ムーブ）されることもありません。

```align
fn greet(who: str) -> i64 = who.len()

fn main() -> i32 {
    owned := "align".clone()    // string
    print(greet(owned))         // borrows — owned is still alive
    print(owned.len())          // 5
    return 0
}
```

関数のシグネチャでは、デフォルトで `str` を使用してください。引数としてビューを受け取り、既存のデータの一部を返す場合もビューを返します。関数が本当に新しいテキストデータを生成して返す必要がある場合にのみ、`string` を返すように設計すべきです。

## リテラル、エスケープ、バイト

ダブルクォートの 1 行リテラルで、エスケープは `\n` `\t` `\r` `\0` `\\` `\"` と `\u{...}` —— 未知のエスケープはコンパイルエラーです。`char` リテラルはシングルクォート(`'A'`・`'あ'`)で、Unicode スカラー 1 個を保持します。文字列は UTF-8 で、`.len()` は**バイト**長です。

```align
print("あ".len())    // 3 — UTF-8 bytes, not characters
```

## メソッド

```align
fn main() -> i32 {
    s := "hello, align"
    print(s.contains("align"))      // true
    print(s.starts_with("hello"))   // true
    print(s.ends_with("!"))         // false
    t := "  padded  "
    print(t.trim())                 // "padded" — a zero-copy sub-view
    return 0
}
```

現時点で提供されているメソッドは、`len`、`contains`、`starts_with`、`ends_with`、`find`、`rfind`、`eq_ignore_ascii_case`、`trim`、`trim_start`、`trim_end`、`clone` のみです。これらはすべてバイト単位で動作し、検索処理には SIMD 化可能なスキャン命令が使用されます（実際にベクトル化されるか、何レーンで処理されるかは、ターゲットアーキテクチャやプロファイル、入力の形状に依存します）。`find` と `rfind` は、最初（または最後）に一致した位置のバイトインデックスを `Option<i64>` として返します（見つからなければ `None`）。これは文字列に対する範囲スライス（スライス記法）と組み合わせて使用できます。

```align
fn main() -> i32 {
    path := "align/docs/guide.md"
    j := path.rfind("/") else -1
    print(path[j + 1..path.len()])      // guide.md — ゼロコピーのビュー
    return 0
}
```

(`path[i]` のような単一バイトへのアクセスはありません ― バイトインデックスはスライスのためのものであって、1 バイトずつ辿るためのものではありません。)`split` はまだ存在しません(実装中)。今は `find` / `rfind` に `[a..b]` を組み合わせて手動の分割を組み立てるか、本物のパーサーを書いてください。

> **Cost:** `str` のコピー、slice、`trim*` は O(1) のviewで、確保もbyte copyもありません。`.clone()` は O(n) で、結果領域を最大1回確保し、n byteを所有する `string` へコピーします。検索は最悪 O(n) です。

## 連結 ― builder が唯一の方法

文字列に対する `a + b` のような `+` 演算子による結合は、いかなる場所でもコンパイルエラーになります。文字列の結合には新たなメモリ確保が伴うため、Align ではその「メモリ確保」と「所有権の発生」を、1つの明示的な構築手段によって表現するように設計されています。

```align
fn shout(name: str) -> string {
    b := builder()
    b.write("hey, ")
    b.write(name)
    b.write("!")
    return b.to_string()
}

fn main() -> i32 {
    print(shout("align"))           // hey, align!
    return 0
}
```

これは、テキスト処理における「すべてを明示する（Nothing hidden）」と「1つの目的に対して1つの方法（One way to do things）」の具現化です。`xs.reduce("", fn acc, x { acc + x })` のような書き方は、背後でのメモリ確保を隠蔽し、次第に大きくなる中間文字列の無駄なコピーを繰り返してしまいます。Align では、たとえ arena 内であっても例外を設けず `+` を拒否し、1回の結合であっても、ループによる逐次的な組み立てであっても、一貫して `builder` を使用させます。

## builder

テキストを少しずつ組み立てていく処理（例えば、ループ内で文字列を追記していくようなケース）には `builder` を使用します。

```align
fn label(name: str, score: i64) -> string {
    b := builder()          // or builder(64) with a capacity hint
    b.write(name)
    b.write(": ")
    b.write_int(score)
    return b.to_string()    // finish → owned string
}

fn main() -> i32 {
    print(label("ada", 95))     // ada: 95
    return 0
}
```

拡張可能なバッファが1つだけ確保され、追記のコストは償却（アモルタイズ）され、最後に1つの `string` が生成されます。`write` は `str`（または所有権を持つ `string`）を受け取り、`write_int` は整数を一時的な文字列領域を介さずに、直接バッファへフォーマットして書き込みます。コンパイラは隣接する固定文字列の書き込みを融合（fuse）することすら行います（例えば、`"lit"` + 整数 + `"lit"` の書き込みは単一の実行時呼び出しに最適化されます）。つまり、`builder` は安全であるだけでなく、最も高速な方法でもあります。

## テンプレート文字列

変数の値を埋め込むような1回限りの文字列整形には、`template` キーワードを使用した補間（インターポレーション）を利用します。

```align
fn main() -> i32 {
    name := "align"
    score := 40
    print(template "Hello {name}, score={score + 2}")   // Hello align, score=42
    return 0
}
```

テンプレート文字列は「組み立てた1行を `print` する」ようなケースに適しており、`builder` は「長いドキュメント全体を組み立てる」ケースを担います。なお、`printf` スタイルの複雑な書式指定ミニ言語は存在しません。（また、パイプラインのラムダ式内では、`+` 演算子と同様に「隠れたメモリ確保」を防ぐため、`template` の使用も拒否されます。文字列の整形はパイプライン処理の内部で要素ごとに行うのではなく、パイプラインの完了後に行うように設計してください。）

## ひと目で選ぶ

| やりたいこと | 使うもの |
|---|---|
| テキストを持ち回して調べる | `str`(ビュー、無料) |
| テキストを供給元のライフタイムより長く保持する | `.clone()` → `string` |
| いくつかの断片を 1 度だけ貼り合わせる | `builder` |
| テキストを逐次的に/大量に組み立てる | `builder` |
| 整形した 1 行 | `template "..."` |
