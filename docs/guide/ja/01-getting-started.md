# はじめる

> 🌐 [English](../01-getting-started.md) · **日本語**

Align はまだ初期の 0.x プロジェクトであり、後方互換性は保証されていません。タグ付きリリースは macOS (Apple Silicon) および Ubuntu 24.04 (x86_64 / ARM64) 向けにパッケージ化され、以下のコマンドで公開済みの最新版をインストールできます。リポジトリの最新状態が必要な場合は、ソースからビルドしてください。

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

このセットアップスクリプトは、署名付きの Align リポジトリと公式の LLVM 22 リポジトリを追加します。2つ目のコマンドを実行するまでは、`alignc` 自体はインストールされません。対応する GitHub リリースから、アーカイブや `.deb` を直接取得することもできます。

## コンパイラをビルドする

必要なのは **Rust 1.96 以上**と **LLVM 22** です。Debian/Ubuntu なら次のようにします（apt.llvm.org 経由）。

```text
apt install llvm-22 llvm-22-dev clang-22 libclang-rt-22-dev libssl-dev zlib1g-dev libzstd-dev
git clone https://github.com/sanohiro/align
cd align
cargo build
```

これでコンパイラは `./target/debug/alignc` に置かれます。`PATH` が通っていないため、フルパスで実行するかエイリアスを設定してください。（`--release` を付けてビルドすると `./target/release/alignc` が生成されます。コンパイラ自体の実行は速くなりますが、生成されるコードの性能は同じです。）なお、`alignc` は LLVM 22 を動的にリンクし、生成したプログラムのリンクには `cc` を呼び出すため、コンパイラの実行ファイルだけをコピーしてもネイティブツールチェーンへの依存は残ります。

Ubuntu 24.04 標準の OpenSSL 3.0 を使用すれば、TLS、ハッシュ、HMAC、HKDF、AEAD などの機能を利用できます。ただし、`crypto.argon2id` のプロバイダは OpenSSL 3.2 で追加されたため、Argon2id が必要な場合は新しいバージョンの OpenSSL をインストールしてください。未対応の環境では、実行時に engine error が返されます。

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

失敗する可能性のある `main` は、代わりに `Result` を返します。その構文や終了コードの扱いは [04](04-errors.md) 章で解説します。

## サブコマンド

```text
alignc check          file.align          型検査と lint
alignc check-per-unit file.align          各 import unit を interface 経由で検査
alignc emit-interface file.align          public interface と hash を表示
alignc build          file.align          ネイティブ実行ファイルを生成(./file)
alignc run            file.align [args…]  build + run、末尾の引数は main(args) へ渡る
alignc fmt            file.align [--write] 整形(表示、--write でその場で書き換え)
alignc emit-mir       file.align          中間 IR をダンプ
alignc emit-llvm      file.align          最適化前後の LLVM IR をダンプ
alignc emit-obj       file.align [out.o]  object file のみ、link なし
alignc explain-opt    file.align          optimizer の判断を source line 上で説明
alignc size           file.align          build して executable の size を報告
alignc cache clear                        解決した codegen cache を消去
alignc --version                          compiler version を表示
```

日常的な開発サイクルでは、編集中は `check` を、テスト実行時は `run` を使用します。また、`emit-llvm` の存在を早めに知っておくことは有益です。Align は、素直なコードが効率的な機械語にコンパイルされるように設計されています。`emit-llvm` は、その設計通りにコンパイルされているかをご自身の目で確かめるためのコマンドです。

## コンパイルエラーを読む

Align のコンパイラは厳格です。null は存在せず、`match` はすべてのケースを網羅する必要があり、処理されていない `Result` はエラーになります。また、ムーブされた値は再利用できません。診断メッセージは、どの規則に違反したのかを的確に教えてくれます。初めて書いたプログラムがコンパイルエラーになる場合、多くは次の2つのいずれかが原因です。

```align
fn main() -> i32 {
    x := 1
    x = 2          // error: x is not `mut`
    return 0
}
```

変数を変更するには、明示的に宣言する必要があります (`mut x := 1`)。もう一つはこちらです。

```align
import std.fs

fn main() -> i32 {
    fs.write_file("out.txt", "hi")   // error: unhandled Result
    return 0
}
```

失敗する可能性のある関数はすべて `Result` を返します。これを暗黙のうちに無視するとコンパイルエラーとなり、単なる lint 警告では済まされません。`Result` を扱う方法は3通りあり、詳しくは [04](04-errors.md) 章で解説します。

## 次に読むもの

[02](02-language-basics.md) 章では、式指向のコアについて一気に解説します。ドリル形式で学ぶのが好みなら、[The Little Aligner](../../little-aligner/ja/README.md) から始めることもできます。
