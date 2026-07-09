# 文字列とテキスト

> 🌐 [English](../07-strings-and-text.md) · **日本語**

Align のテキスト処理は、[05](05-memory.md) 章のメモリモデルにきっちり従います。ライフタイムが 2 種類あるから文字列型も 2 種類あり、あらゆるメモリ確保は目に見えます。ここでパターンを一度つかんでしまえば、言語のあらゆるリソースについて同じパターンを見たことになります。

## `str` と `string`

- **`str`** ― 借用された、変更不可の**ビュー**です。ポインタとバイト長の組で、文字列リテラルは `str` です。コピーは無料で、指し示すデータのリージョンを引き継ぎます。
- **`string`** ― **所有**するヒープバッファです。Move 型なので、代入は所有権の移動になり、所有者が寿命を終えるとバッファは破棄されます。

`string` は `.clone()`(ディープコピー)か `builder`(後述)から得られます。そして所有された `string` は、`str` が期待される場所ではどこでも**自動的に借用されます**。`str` パラメータに渡してもコストはかからず、消費もされません。

```align
fn greet(who: str) -> i64 = who.len()

fn main() -> i32 {
    owned := "align".clone()    // string
    print(greet(owned))         // borrows — owned is still alive
    print(owned.len())          // 5
    return 0
}
```

シグネチャのデフォルトは `str` です。ビューを受け取り、データがすでに存在するならビューを返し、関数が本当に新しいテキストを生成するときにだけ `string` を返してください。

## リテラル、エスケープ、バイト

ダブルクォートの 1 行リテラルで、エスケープは `\n` `\t` `\r` `\0` `\\` `\"` と `\u{...}` —— 未知のエスケープはコンパイルエラーです(`\r` `\0` `\u{...}` とそのエラーは**実装進行中**。`\n` `\t` `\"` `\\` は今日から使えます)。`char` リテラルはシングルクォート(`'A'`・`'あ'`)で、Unicode スカラー 1 個を保持します。文字列は UTF-8 で、`.len()` は**バイト**長です。

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

`len` / `contains` / `starts_with` / `ends_with` / `find` / `rfind` / `eq_ignore_ascii_case` / `trim` / `trim_start` / `trim_end` / `clone` ― 現時点のメソッドはこれで全部です。いずれもバイト単位で動作し、検索系は内部で SIMD を使っています(`contains` はナイーブなループではなくベクトル化されたスキャンです)。`find` / `rfind` は `Option<i64>` ― 最初/最後に一致した位置のバイトインデックス、なければ `None` ― を返し、文字列にも使える範囲スライスと組み合わせられます。

```align
fn main() -> i32 {
    path := "align/docs/guide.md"
    j := path.rfind("/") else -1
    print(path[j + 1..path.len()])      // guide.md ― ゼロコピーのビュー
    return 0
}
```

(`path[i]` のような単一バイトへのアクセスはありません ― バイトインデックスはスライスのためのものであって、1 バイトずつ辿るためのものではありません。)`split` はまだ存在しません(実装中)。今は `find` / `rfind` に `[a..b]` を組み合わせて手動の分割を組み立てるか、本物のパーサーを書いてください。

## 連結 ― 確保先が定まっているところでのみ許される

文字列に対する `a + b` はメモリを確保します。だからこそ Align は、その確保に目に見えるライフタイムがあることを要求します。`arena` の内側では、連結が一時的なテキストを組み立てる自然な手段になります。

```align
fn shout(name: str) -> string {
    arena {
        s := "hey, " + name + "!"   // arena-backed temporaries
        return s.clone()            // copy the survivor out
    }
}

fn main() -> i32 {
    print(shout("align"))           // hey, align!
    return 0
}
```

ところが**パイプラインのラムダ**の内側での `+` はコンパイルエラーになります。`xs.reduce("", fn acc, x { acc + x })` は要素ごとに所有者のないメモリ確保を行い、たった 1 行の無邪気なコードに隠れた二乗オーダーのリークを生みます。コンパイラはこれを拒否し、直し方は次節の builder です。これはテキストに適用された「何も隠さない」です。あらゆる文字列のメモリ確保は、arena か、所有者か、builder のいずれかに属します。融合ループの真ん中には決して属しません。

## builder

テキストを少しずつ組み立てる ― ループ内で追記していく形 ― には `builder` を使います。

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

伸長可能なバッファが 1 つ、追記は償却され、最後に `string` が 1 つできます。`write` は `str`(または所有された `string`)を受け取り、`write_int` は整数を一時領域なしにそのままバッファへ整形します。コンパイラは隣接する書き込みを融合さえします(`"lit"` + int + `"lit"` は単一のランタイム呼び出しになります)。つまり builder は安全な方法であるだけでなく、速い方法でもあります。

## テンプレート文字列

1 回きりの整形には、`template` が式全体を補間します。

```align
fn main() -> i32 {
    name := "align"
    score := 40
    print(template "Hello {name}, score={score + 2}")   // Hello align, score=42
    return 0
}
```

テンプレートは「組み立てた 1 行を `print` する」ケースを、builder は「ドキュメントを組み立てる」ケースをそれぞれ担います。printf スタイルの書式文字列ミニ言語はありません。(パイプラインのラムダ内では、`+` と同じ隠れた確保の理由から `template` も拒否されます。整形はパイプラインの後で行い、内部で要素ごとに行わないでください。)

## ひと目で選ぶ

| やりたいこと | 使うもの |
|---|---|
| テキストを持ち回して調べる | `str`(ビュー、無料) |
| テキストを供給元のライフタイムより長く保持する | `.clone()` → `string` |
| いくつかの断片をスコープ内で 1 度だけ貼り合わせる | `arena` 内の `+` |
| テキストを逐次的に/大量に組み立てる | `builder` |
| 整形した 1 行 | `template "..."` |
