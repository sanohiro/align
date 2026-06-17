# 実装方針（概要）

`draft.md` を実装するための最上位ドキュメント。以降の `docs/impl/*.md` はすべてこの方針に従う。

## 決定事項

```text
実装言語     Rust
バックエンド  LLVM (唯一の本命) ただし MIR を必ず挟む
進め方       設計は全体を先に固める / 実装は縦に貫く骨格を先に通す
```

## なぜこの3つなのか

### 実装言語: Rust

コンパイラ実装の定番。lexer/parser の生成器、LLVM バインディング (`inkwell`)、強い型と所有権による自己検証が揃う。将来 Align で Align を書く(セルフホスト)際の足場にもなる。

### バックエンド: LLVM 直行、ただし MIR を挟む

「C バックエンド先行 → 後で LLVM」という段階戦略は**採らない**。理由:

- Align の核心は `vec<N,T>` / `mask` / loop fusion を**決定論的にベクトル命令へ落とす**こと。C 経由はホスト C コンパイラの自動ベクトル化頼みになり、「予測可能に速い」というアイデンティティが崩れる。
- 後から LLVM に移行すると大改修になる。

代わりに **バックエンド非依存の中間表現 MIR を必ず挟む**。Align の意味論(arena / move / fusion / SIMD 化判断)はすべて MIR 側に置き、`MIR → LLVM` は最終段の純粋な lowering に限定する。これにより:

- 将来 C バックエンドやデバッグ用テキスト出力を足したくなっても「lowering を1個追加」で済み、書き直しにならない。
- バックエンド都合の判断が型チェッカーまで漏れない。

詳細は `04-mir.md` / `05-backend-llvm.md`。

### 進め方: 全体設計 → 縦切り骨格 → 肉付け

コンパイラ開発で最も危険なのは「一つの段を、パイプライン全体が繋がる前に作り込む」こと。完全な型システムを codegen 前に作り込むと、codegen 段階で型情報の形が合わず型チェッカーを書き直す——これが大改修の正体。

対策は2軸を分けること。

```text
軸A 機能カバレッジ   少ない機能 → 機能を足す
軸B パイプライン貫通 source → 実行ファイル が端から端まで繋がっているか
```

危険は軸Bにある。よって:

> 設計(本 impl docs)は全体を先に固める。
> 実装は最小の縦切り骨格(walking skeleton)を端から端まで先に通し、そこへ機能を差し込む。

`x := 1` レベルのプログラムが lexer → parser → typecheck → MIR → LLVM → 実行ファイル まで通る骨格を最初に完成させる。骨格が通れば `map` / `where` / arena / JSON は同じパイプラインへ**差し込む**だけになり、各段の書き直しが起きない。

## クレート構成 (案)

Rust workspace。段ごとにクレートを分け、IR の境界をクレート境界に一致させる。

```text
alignc/                  workspace root
  crates/
    align_span/          ソース位置・ファイル管理 (全段が依存)
    align_diag/          診断(エラー/警告)の共通基盤
    align_lexer/         source → tokens
    align_parser/        tokens → AST
    align_ast/           AST 定義
    align_sema/          名前解決 + 型推論/検査 + move/arena 検査 → typed HIR
    align_mir/           HIR → MIR 変換 + MIR 最適化(fusion 等)
    align_codegen_llvm/  MIR → LLVM IR → object
    align_runtime/       最小ランタイム(arena allocator 等)。出力にリンク
    align_driver/        CLI: alignc build / run。各段を繋ぐ
  tests/                 端から端まで(.align → 実行 → 出力比較)
```

段の責務とIR境界の詳細は `01-pipeline.md`。

## ドキュメント一覧

```text
00-overview.md        本書。全体方針
01-pipeline.md        パイプライン各段とIR境界
02-frontend.md        lexer / parser / AST
03-types.md           型システム / 推論 / move・arena 検査
04-mir.md             MIR 設計(バックエンド非依存の核)
05-backend-llvm.md    MIR → LLVM lowering / SIMD / arena codegen
06-runtime-std.md     最小ランタイムと core/std のブートストラップ
07-roadmap.md         マイルストーン M0..Mn
```

## 不変条件 (実装でも守る)

`draft.md` / `docs/design-notes.md` の設計不変条件は実装段階でも拘束力を持つ。特に:

- allocation / error / 副作用 / 並列 / unsafe は**生成コードでも追跡可能**にする(隠さない)。
- 制約はコンパイラ推論のための情報源。lifetime を表に出さずに no-alias / non-null / arena 寿命 / cold error path を推論する(`03-types.md`)。
- `map` / `reduce` / `scan` / `filter` / `mask` が自然にベクトル化される lowering を MIR で実現する(`04-mir.md`)。
