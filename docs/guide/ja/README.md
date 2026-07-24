> 🌐 [English](../README.md) · **日本語**

Align を書くための実践的な入門です。仕様書(それは draft.md の役割です)ではなく、Align でどう考え、どう書くかを扱います。各章は順番に読むことを前提にしているので、まずは 00 から始めてください。掲載しているコード例は、*implementation in progress* と印を付けたもの以外、すべて現在の `alignc` でコンパイルできます。

手を動かしながら学ぶほうが好みですか。**[The Little Aligner](../../little-aligner/ja/README.md)** なら、*The Little Schemer* のスタイルで、同じ範囲を一問一答のドリルとして解きながら身につけられます。

## 第 I 部 —— 基礎

- [00 — なぜ Align か](00-why-align.md)
- [01 — はじめる](01-getting-started.md)
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
- [16 — ツールチェーン: alignc、フォーマッタ、lint](16-toolchain.md)
- [17 — Align の流儀](17-the-align-way.md)
- [18 — std services: network、HTTP、process、圧縮、暗号](18-std-services.md)

## 第 IV 部 —— オブジェクトを持たないシステム設計

- [19 — オブジェクト指向のアンラーニング](19-unlearning-objects.md)
- [20 — Arena の先へ: プールとライフタイム](20-beyond-arenas.md)
- [21 — ステートマシン](21-state-machines.md)
- [22 — システムの構築: ECS](22-building-a-system.md)
