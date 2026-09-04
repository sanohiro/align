# Align ガイド

> 🌐 [English](../README.md) · **日本語**

Align を書くための実践的な入門です。[00 章](00-why-align.md)から基礎を順に読み、後半のツールやライブラリの章は必要に応じて参照してください。言語仕様の詳細は [draft.md](../../../draft.md)にまとめています。

**[The Little Aligner](../../little-aligner/ja/README.md)** では、*The Little Schemer* にならった短い問答を通して、パイプライン、データ配置、所有権を練習します。どちらから始めてもかまいません。別の説明や練習がほしくなったら、もう一方も開いてみてください。

## コード例の読み方と試し方

コード例には、プログラム全体を示したものと、前後で紹介した宣言を使う断片があります。型や関数はファイル直下で宣言し、実行する文は `main` の中に置きます。エラーだと明示した例は、コンパイラが何を拒否するかを確かめるためのものです。未実装の機能には、別にその旨を記しています。

`alignc check file.align` で検査し、`alignc run file.align` で実行できます。テストファイルには `alignc test file.align` を使います。詳しくは [16 章](16-toolchain.md)を参照してください。短い式は `align-repl` で試せます。独立した例へ移るときは `:clear` で入力を消してください。入力のたびに、それまでのプログラム全体が副作用も含めて再実行されるためです。インストールと最初の操作は [01 章](01-getting-started.md)で説明します。

## 第 I 部 —— 基礎

- [00 — なぜ Align か](00-why-align.md)
- [01 — はじめる](01-getting-started.md) — インストール、`align-repl`、最初のプログラム
- [02 — 言語の基本](02-language-basics.md)
- [03 — データをモデリングする: 構造体、直和型、match](03-modeling-data.md)
- [04 — エラー: Option、Result、そして `?`](04-errors.md)
- [05 — メモリ: 値、arena、heap](05-memory.md)

## 第 II 部 —— 言語の核心

- [06 — パイプライン: データ処理の中核](06-pipelines.md)
- [07 — 文字列とテキスト](07-strings-and-text.md)
- [08 — JSON](08-json.md)
- [09 — ジェネリクスとモジュール](09-generics-and-modules.md)
- [10 — クロージャと並列処理](10-closures-and-parallelism.md)
- [11 — データ指向設計: SoA とグループ集計](11-data-oriented.md)
- [12 — 明示的な SIMD: vecN、マスク、アライメント](12-simd.md)

## 第 III 部 —— 標準ライブラリと境界

- [13 — std: ファイル、I/O、そして OS 境界](13-std-os.md)
- [14 — std: encoding、regex、rand、cli](14-std-encoding-rand-cli.md)
- [15 — 境界: unsafe と C FFI](15-unsafe-and-ffi.md)
- [16 — ツールチェーン: alignc、テスト、align-repl、フォーマット、lint](16-toolchain.md)
- [17 — Align の流儀](17-the-align-way.md)
- [18 — std services: network、HTTP、process、圧縮、暗号](18-std-services.md)

## 第 IV 部 —— オブジェクトを持たないシステム設計

- [19 — オブジェクト指向のアンラーニング](19-unlearning-objects.md)
- [20 — Arena の先へ: プールとライフタイム](20-beyond-arenas.md)
- [21 — ステートマシン](21-state-machines.md)
- [22 — システムの構築: ECS](22-building-a-system.md)

## 第 V 部 —— パッケージ

- [23 — パッケージ: ソースの配置とライブラリの選び方](23-packages.md)
- [24 — データベース: pkg.db の実践](24-database.md)
- [25 — pkg.db を通じたベクトル検索](25-vector-search.md)
