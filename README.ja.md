# Align

> 🌐 [English](./README.md) · **日本語**

Align は AOT コンパイル方式のデータ指向プログラミング言語です。コードを書く**人間**、コードを生成する **AI**、コードを最適化する**コンパイラ**、コードを実行する**ハードウェア** —— この四者の足並みを、一度にそろえます。

> Less code. Predictable performance. Nothing hidden.
> (コードは少なく。性能は予測どおりに。隠し事はしない。)

まだ初期段階のプロジェクトです。設計の正典は `draft.md` + `docs/` にあり、コンパイラ(`alignc`)は `crates/` 以下で Rust による実装が進んでいます。

## Align を選ぶ理由

- **データ指向のコア。** 配列とスライスが言語の中心です。`prices.map(with_tax).where(in_stock).sum()` と書くだけで、コンパイラはこれを中間配列のない単一ループへ融合します。素直に書いたコードがそのままきれいに最適化されるので、キャッシュにも SIMD にも優しく仕上がります。
- **何も隠さない。** アロケーション、エラー、副作用、並列処理は、常にソースコード上に見える形で現れます。裏で起きるコピーも、例外も、勝手に立ち上がるスレッドもありません。
- **やり方はひとつだけ。** エラーモデルはひとつ(`Result` + `?`)、オプショナルもひとつ(`Option`、null なし)、所有権モデルもひとつ(値 / `arena` / heap)、並列モデルもひとつ(`map` / `reduce` / `task_group`)。
- **手動のメモリ管理なし、GC なし。** 所有権は型の性質であり、ライフタイムは region として推論されます —— 書く必要はありません。データを*どこに*置くか(値・`arena`・heap)を選ぶだけで、解放処理はコンパイラが挿入します。

## 一例

```align
Item { price: f64, active: bool }

fn with_tax(p: f64) -> f64 = p * 1.08

fn main() -> i32 {
    items := [
        Item { price: 100.0, active: true },
        Item { price: 50.0,  active: false },
        Item { price: 200.0, active: true },
    ]
    total := items.where(.active).price.map(with_tax).sum()  // one fused loop, no temporaries
    print(total)                                             // 324.0
    return 0
}
```

## Align を学ぶ

言語に初めて触れる方は、まずガイドから始めてください。Align で考え、書くための実践的な入門です。

**[チュートリアル(日本語)](docs/guide/ja/README.md)** · **[Tutorial (English)](docs/guide/README.md)**

問題を解きながら学ぶ方には **[The Little Aligner(日本語)](docs/little-aligner/ja/README.md)**([English](docs/little-aligner/README.md))がおすすめです。*The Little Schemer* のスタイルで、同じイディオムを Q&A 形式のワークブックとして学べます。

## ビルドと実行

```sh
cargo build
cargo test
cargo run --bin alignc -- run examples/arena.align     # arena + heap box; exits 42
cargo run --bin alignc -- run examples/pipeline.align  # fused map/where/sum; exits 24
```

`alignc` のサブコマンド: `check`, `emit-mir`, `emit-llvm`, `build`, `run`。

**必要環境:** Rust(stable)、LLVM 19(`llvm-config` が `PATH` 上にあること)、リンク用の C コンパイラ(`cc`)。

## ステータス

まだ初期段階ですが、パイプラインは端から端まで動きます(`lexer → parser → sema → MIR → LLVM → native`)。関数と制御フロー、構造体、プリミティブ型のフルセット、`?` を伴う `Option`/`Result`、move・エスケープチェック付きの `arena`/`box`、融合された配列パイプライン、文字列と `json`、SIMD(`vecN`/`soa`/`group_by`)、実スレッド上の `par_map`/`task_group`、`unsafe`/FFI、そして拡充中の標準ライブラリ(`io`/`fs`/`path`/`env`/`time`/`encoding`/`rand`/`cli`/`net`/`process`/`compress`/`crypto`、`http` は実装中)まで揃っています。マイルストーンの詳細は `docs/impl/07-roadmap.md` を参照してください。

## パフォーマンスと移植性

デフォルトのビルドは、**安全で移植性の高いアーキテクチャ別ベースライン**(amd64 では `x86-64-v2`、arm64 では `armv8-a`/NEON)を使います。そのため、混在したクラウドフリート上でも1つのバイナリで動きます。より攻めたターゲットは**オプトインであり、決してデフォルトにはなりません** —— ホスト固有のビルドなら `--target-cpu native`、移植可能な AVX2/FMA 帯なら `x86-64-v3` を指定します。多様なフリート全体で広い SIMD を活かすのは、ベースラインを引き上げることではなく、ライブラリ側の実行時 CPU 機能ディスパッチによって実現する方針です。詳しくは `draft.md` §3.4 と `docs/open-questions.md`(「Build targets & portability」)を参照してください。

## レイアウト

- `draft.md` —— 言語仕様の正典
- `docs/guide/` —— 実践的なチュートリアル、全18章(英語 + 日本語)
- `docs/little-aligner/` —— *The Little Schemer* スタイルの Q&A ドリル・ワークブック(英語 + 日本語)
- `docs/` —— 設計の根拠、経緯、非目標、未解決の論点
- `docs/impl/` —— コンパイラ実装計画 + 標準ライブラリのモジュール設計仕様
- `editors/` —— Vim / Emacs / VS Code 対応(シンタックスハイライト、スニペット)
- `crates/` —— `alignc` コンパイラのワークスペース

## ライセンス

MIT
