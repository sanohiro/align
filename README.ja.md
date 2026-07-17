# Align

> 🌐 [English](./README.md) · **日本語**

Align は AOT コンパイル方式のデータ指向プログラミング言語です。コードを書く**人間**、コードを生成する **AI**、コードを最適化する**コンパイラ**、コードを実行する**ハードウェア** —— これら四者の要求を同時に満たします。背後の挙動を隠蔽しない厳格なポリシーと、エラーや所有権の統一されたモデル、データ指向の配列・スライスを中心とした設計により、予測可能なパフォーマンスとキャッシュに優しい融合ループを通常のコードから実現します。

## プラットフォーム

現在サポートしているプラットフォームは以下の通りです：
- **Linux x86-64**
- **macOS Apple Silicon (aarch64)**
- *※ Windows には対応していません。*

## インストール

現状はソースからのビルドのみ提供しています。コンパイラをビルドするには以下が必要です：

- **Rust 1.96 以上**
- **LLVM 22** (`llvm-config-22` が `PATH` 上にある必要があります)
- **clang-22** (Cコンパイラ/リンカーとして使用します)

Ubuntu 24.04 の場合は、公式リポジトリ (`apt.llvm.org`) から LLVM 依存関係をインストールできます：
```sh
sudo apt install llvm-22 llvm-22-dev clang-22
```

ビルド手順：
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
./target/release/alignc run hello.align
```

## Align を学ぶ

言語に初めて触れる方は、まずガイドから始めてください。Align で考え、書くための実践的な入門です。

**[チュートリアル(日本語)](docs/guide/ja/README.md)** · **[Tutorial (English)](docs/guide/README.md)**

問題を解きながら学ぶ方には **[The Little Aligner(日本語)](docs/little-aligner/ja/README.md)**([English](docs/little-aligner/README.md))がおすすめです。*The Little Schemer* のスタイルで、同じイディオムを Q&A 形式のワークブックとして学べます。

## レイアウト

- `draft.md` —— 言語仕様の正典
- `docs/guide/` —— 実践的なチュートリアル、全19章(`00`〜`18`、英語 + 日本語)
- `docs/little-aligner/` —— *The Little Schemer* スタイルの Q&A ドリル・ワークブック(英語 + 日本語)
- `docs/` —— 設計の根拠、経緯、非目標、未解決の論点
- `docs/impl/` —— コンパイラ実装計画 + 標準ライブラリのモジュール設計仕様
- `editors/` —— Vim / Emacs / VS Code 対応(シンタックスハイライト、スニペット)
- `crates/` —— `alignc` コンパイラのワークスペース

## ライセンス

本プロジェクトは以下のいずれかのライセンスを選択できるデュアルライセンスです：
- MIT License ([LICENSE-MIT](LICENSE-MIT) または http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) または http://www.apache.org/licenses/LICENSE-2.0)
