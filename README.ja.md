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
インストールスクリプトがパッケージリポジトリを設定し、`apt install alignc` でコンパイラとランタイムの依存関係が導入されます。

### ソースからのビルド

ワークスペースのビルドには **Rust 1.96 以上**、**LLVM 22**、ネイティブ C ツールチェーンが必要です。計測付き PGO には、対応する **clang 22** とそのコンパイラランタイムも必要です。ネイティブライブラリには OpenSSL、zstd、SQLite、**libpq 17 以上**を使います。PostgreSQL サーバーは、データベースを使うアプリケーションや統合テストでのみ必要です。

#### Linux (Ubuntu 24.04)

リポジトリのルートから、CI と同じ署名付き LLVM・PostgreSQL パッケージリポジトリを使います。Ubuntu 24.04 標準の libpq 16 は、データベースのストリーム API には古すぎます。
```sh
sudo apt install build-essential curl wget gnupg pkg-config
scripts/ci-pgdg.sh
align_deps_tmp="$(mktemp -d)"
RUNNER_TEMP="$align_deps_tmp" \
ALIGN_APT_PACKAGES="llvm-22-dev clang-22 libclang-rt-22-dev lld-22 libpq-dev libsqlite3-dev libssl-dev zlib1g-dev libzstd-dev" \
  scripts/ci-apt-llvm.sh install --uncached
rmdir "$align_deps_tmp"
scripts/check-libpq-version.sh
```

#### macOS (Apple Silicon)

Homebrew で依存関係をインストールします：
```sh
brew install llvm@22 openssl@3 libpq zstd
```
`scripts/cargo.sh` が LLVM 22 を見つけ、コンパイラのビルド中に Homebrew のライブラリ検索パスを設定します。その後 `alignc` を使うシェルでは、keg-only のツールとネイティブライブラリを参照できるようにします。
```sh
export PATH="$(brew --prefix llvm@22)/bin:$(brew --prefix libpq)/bin:$PATH"
export LIBRARY_PATH="$(brew --prefix libpq)/lib:$(brew --prefix openssl@3)/lib:$(brew --prefix zstd)/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
scripts/check-libpq-version.sh
```

#### ビルド

```sh
scripts/cargo.sh build --release --workspace
# コンパイラ: target/release/alignc、REPL: target/release/align-repl
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

**[Align で高速な処理を書く](docs/guide/ja/26-performance.md)**（[English](docs/guide/26-performance.md)）は、計測、パイプラインの融合、領域の再利用、データ配置、SIMD、並列化を一つの実践手順にまとめています。

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
