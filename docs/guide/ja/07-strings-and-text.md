# 文字列とテキスト

> 🌐 [English](../07-strings-and-text.md) · **日本語**

文字列にも [05 章](05-memory.md)のメモリモデルが適用されます。`str` は既存のテキストを借用し、`string` はバッファを所有します。この違いによって、値をいつまで使えるか、どこでメモリ確保が必要かが決まります。

## `str` と `string`

- **`str`** ― 既存のテキストを借用する、変更不可の**ビュー**です。ポインタとバイト長のペアで、文字列リテラルもこの型です。ビューをコピーしてもテキスト自体はコピーされず、参照先のデータの有効範囲（region）を引き継ぎます。
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

`path[i]` は文字列の操作ではありません。UTF-8 の1バイトを取り出すなら `path.bytes()[i]` を使います。`str.split` メソッドはまだありません。決まった区切り文字で分ける場合は `find` / `rfind` と `[a..b]` を組み合わせ、もっと複雑な文法を持つ入力にはパーサーを使います。

> **コスト:** `str` のコピーとスライスは O(1) で、メモリ確保もバイト列のコピーも行いません。`trim`、`trim_start`、`trim_end` も無確保・無コピーのビューを返しますが、空白を走査するため最悪 O(n) の時間がかかります。`.clone()` は O(n) で、結果領域を最大1回確保し、n バイトを所有する `string` へコピーします。検索は最悪 O(n) です。

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

builder は1つの拡張可能なバッファを使い、最後に `string` を返します。バッファを拡張するコストは、複数回の追記に分散されます。`write` は `str` または所有権を持つ `string` を受け取ります。`write_int` は、一時文字列を作らずに整数をバッファへ直接書き込みます。また、コンパイラは連続するリテラルや整数の書き込みを、1回の実行時呼び出しにまとめられます。

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

1行の整形にはテンプレート文字列を、長い文書の組み立てには `builder` を使います。`printf` のような書式指定のための構文はありません。パイプラインのラムダ内でも、arena を使わずにテンプレートを作り、その場で結果を使えます。ただし、動的なテンプレートが返すビューの有効期間は、そのラムダ呼び出しのフレーム内に限られるため、ラムダの戻り値にはできません。後で使うテキストを作りたい場合は、パイプラインの完了後に整形します。

## ひと目で選ぶ

| やりたいこと | 使うもの |
|---|---|
| テキストを渡したり調べたりする | `str`（ビュー。テキスト自体はコピーしない） |
| テキストを供給元のライフタイムより長く保持する | `.clone()` → `string` |
| いくつかの断片を 1 度だけ貼り合わせる | `builder` |
| テキストを逐次的に/大量に組み立てる | `builder` |
| 整形した 1 行 | `template "..."` |
