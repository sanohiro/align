# Align

> 🌐 [English](./README.md) · **日本語**

Align は AOT コンパイル方式のデータ指向プログラミング言語です。コードを書く**人間**、コードを生成する **AI**、コードを最適化する**コンパイラ**、コードを実行する**ハードウェア**の四者を考慮して設計しています。エラー、所有権の移動、メモリ確保、並列処理はソースコードに明示します。配列やスライスへの変換はパイプラインで表し、コンパイラがループに融合できます。また、列指向の配置により、よく使うフィールドをメモリ上にまとめられます。

## プラットフォーム

現在サポートしているプラットフォームは以下の通りです：
- **Linux x86-64 / ARM64**
- **macOS Apple Silicon (aarch64)**
- *※ Windows には対応していません。*

## インストール

### Homebrew (macOS Apple Silicon)

```sh
brew tap sanohiro/align
brew install align
```

### apt (Ubuntu 24.04)

```sh
curl -fsSL https://sanohiro.github.io/align/install.sh | sudo sh
sudo apt install alignc
```
インストールスクリプトが LLVM 22 と Align の apt リポジトリを設定し、`apt install alignc` でコンパイラとランタイムが導入されます。

### ソースからのビルド

コンパイラのビルドには **Rust 1.96 以上** と **LLVM 22**（Cコンパイラ/リンカーとして対応する **clang**）が必要です。

#### Linux (Ubuntu 24.04)

公式リポジトリ (`apt.llvm.org`) から LLVM ツールチェーンをインストールします。`llvm-config-22` が `PATH` 上にある必要があります：
```sh
sudo apt install llvm-22 llvm-22-dev clang-22
```

#### macOS (Apple Silicon)

Homebrew で依存関係をインストールします：
```sh
brew install llvm openssl@3 zstd
```
現在 `llvm` formula は LLVM 22 を提供します。Homebrew の `llvm` が 22 より先に進んでいる場合は、バージョン固定の `llvm@22` formula を代わりにインストールしてください。Homebrew の LLVM は keg-only（`llvm-config` が `PATH` に載りません）なので、ビルドがそれを参照できるようにし、ランタイムのネイティブライブラリ（`zstd`、`openssl@3`）のリンカ検索パスを追加します。以下をシェルのプロファイルに追加するか、`cargo` / `alignc` の各コマンドの先頭に付けてください（`alignc` でビルドしたプログラムがこれらのライブラリをリンクして実行する際にも、同じ `LIBRARY_PATH` が必要です）：
```sh
export LLVM_SYS_221_PREFIX="$(brew --prefix llvm)"
export LIBRARY_PATH="$(brew --prefix)/lib:$(brew --prefix openssl@3)/lib"
```

#### ビルド

```sh
cargo build --release
# コンパイラは target/release/alignc に生成されます
```

## Hello World

`hello.align` というファイルを作成します：

```align
fn main() -> i32 {
    print("hello, align")
    return 0
}
```

以下のコマンドで実行します：
```sh
alignc run hello.align
```

ソースからビルドした場合は、リポジトリのルートで `alignc` を
`./target/release/alignc`、`align-repl` を `./target/release/align-repl` に
読み替えて実行してください。

編集中はコンパイラを起動したままにして、読み込んだソースファイルやその他の入力が
変更されるたびに再ビルドできます：

```sh
alignc build hello.align --watch
```

最後に成功した実行ファイルはそのまま保持されます。ツールチェーンやライブラリの置換は、
次に監視対象のソース／入力が変更されたとき、またはコマンドを再起動した直後に反映されます。

`align-repl` では、式を入力して試すこともできます。入力のたびにセッションのプログラムへ
追記し、`alignc` がネイティブの実行ファイルにコンパイルして実行します。

```sh
align-repl
```

```text
align> 1 + 2
3
```

## Align を学ぶ

ガイドでは、具体例を使って構文、ツール、ライブラリを順に説明します。

**[チュートリアル(日本語)](docs/guide/ja/README.md)** · **[Tutorial (English)](docs/guide/README.md)**

**[The Little Aligner（日本語）](docs/little-aligner/ja/README.md)**（[English](docs/little-aligner/README.md)）は、*The Little Schemer* にならった短い問答で学ぶ本です。一つずつ自分で考えたい方は、こちらから始めてください。結果を予想し、データの流れを追い、所有権や処理のコストを考えます。どちらからでも読み始められますし、併せて読むこともできます。

## レイアウト

- `draft.md` —— 言語仕様の原本
- `docs/guide/` —— 実践的なチュートリアル(英語 + 日本語)
- `docs/little-aligner/` —— *The Little Schemer* スタイルの Q&A ドリル・ワークブック(英語 + 日本語)
- `docs/` —— 設計の根拠、経緯、非目標、未解決の論点
- `docs/impl/` —— コンパイラ実装計画 + 標準ライブラリのモジュール設計仕様
- `apps/` —— `pkg.web`、`pkg.auth`、`pkg.kv` など、Align が提供するパッケージのワークスペース
- `editors/` —— Vim / Emacs / VS Code 対応(シンタックスハイライト、スニペット)
- `crates/` —— `alignc` コンパイラのワークスペース

## ライセンス

本プロジェクトは以下のいずれかのライセンスを選択できるデュアルライセンスです：
- MIT License ([LICENSE-MIT](LICENSE-MIT) または http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) または http://www.apache.org/licenses/LICENSE-2.0)
