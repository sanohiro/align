# はじめる

> 🌐 [English](../01-getting-started.md) · **日本語**

Align はまだリリース前です。リリース自動化は、タグ付きビルドを macOS Apple Silicon と Ubuntu 24.04 (x86_64 / ARM64)向けにパッケージ化します。インストール先は、最初の配布リリースとリポジトリ設定が完了した後に利用できます。それまではコンパイラをソースからビルドしてください。

## パッケージ版をインストールする

macOS Apple Silicon では次のようにします。

```text
brew tap sanohiro/align
brew install align
```

Ubuntu 24.04 では次のようにします。

```text
curl -fsSL https://sanohiro.github.io/align/install.sh | sudo sh
sudo apt install alignc
```

セットアップスクリプトが追加するのは、署名付きの Align リポジトリと公式 LLVM 22 リポジトリです。2つ目のコマンドを実行するまで `alignc` 自体はインストールしません。対応する GitHub リリースから、アーカイブや `.deb` を直接取得することもできます。

## コンパイラをビルドする

必要なのは **Rust 1.96 以上**と **LLVM 22** です。Debian/Ubuntu なら次のようにします（apt.llvm.org 経由）。

```text
apt install llvm-22 llvm-22-dev clang-22 libclang-rt-22-dev libssl-dev zlib1g-dev libzstd-dev
git clone https://github.com/sanohiro/align
cd align
cargo build
```

これでコンパイラは `./target/debug/alignc` に置かれます。`PATH` は通っていないので、パスを指定して呼ぶか、エイリアスを張ってください。(`--release` ビルドなら `./target/release/alignc` ができます。コンパイラ自体の実行は速くなりますが、生成されるコードは同じです。) `alignc` は LLVM 22 を動的に利用し、生成するプログラムのリンクに `cc` を呼び出すため、実行ファイルだけを置いてもネイティブツールチェーンへの依存はなくなりません。

## Hello, Align

次の内容を `hello.align` として保存します。

```align
fn main() -> i32 {
    print("hello, align")
    return 0
}
```

実行します。

```text
$ alignc run hello.align
hello, align
```

`alignc run` は、ネイティブ実行ファイルへのコンパイルと実行を一度に行います。`main` が `i32` を返すと、その戻り値がプロセスの終了コードになります。`print` はプリミティブな値 —— 整数、浮動小数点数、`bool`、`char`、文字列 —— を、末尾に改行を付けて書き出します。

失敗しうる `main` は、代わりに `Result` を返します。その形(および終了コードがどうなるか)は [04](04-errors.md) 章で扱います。

## サブコマンド

```text
alignc check     file.align          型検査のみ、診断を表示
alignc build     file.align          ネイティブ実行ファイルを生成(./file)
alignc run       file.align [args…]  ビルド + 実行、末尾の引数は main(args) へ渡る
alignc fmt       file.align [--write] 整形(標準出力へ表示、--write でその場で書き換え)
alignc emit-mir  file.align          中間 IR をダンプ(興味のある人向け)
alignc emit-llvm file.align          LLVM IR をダンプ(自分のコードが何になったか正確に確認)
alignc emit-obj  file.align [out.o]  オブジェクトファイルのみ、リンクなし
```

日々のループは、編集中は `check`、試すときは `run` です。`emit-llvm` は早めに知っておく価値があります。Align の設計は、素直なコードが引き締まった機械語へ落ちることを約束しています。その約束を自分の目で確かめる手段が `emit-llvm` です。

## コンパイルエラーを読む

Align のコンパイラは厳格です。null なし、`match` は網羅必須、扱われていない `Result` はエラー、move された値は再利用できません。診断メッセージは、どの規則がどこで発動したかを教えてくれます。最初のプログラムがつまずくのは、たいていこの2つのどちらかです。

```align
fn main() -> i32 {
    x := 1
    x = 2          // error: x is not `mut`
    return 0
}
```

変更は宣言しなければなりません(`mut x := 1`)。そして、こちら。

```align
import std.fs

fn main() -> i32 {
    fs.write_file("out.txt", "hi")   // error: unhandled Result
    return 0
}
```

失敗しうるものはすべて `Result` を返します。それを黙って捨てるのはコンパイルエラーであって、無視できる lint ではありません。扱う方法は 3 通りあり、[04](04-errors.md) 章で紹介します。

## 次に読むもの

[02](02-language-basics.md) 章では、式指向のコアをひと息に扱います。散文よりドリルが好みなら、[The Little Aligner](../../little-aligner/ja/README.md) もゼロから始められます。
