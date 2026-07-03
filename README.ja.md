# Align

> 🌐 [English](./README.md) · **日本語**

Align は AOT コンパイル方式の、データ指向プログラミング言語です。コードを書く**人間**、コードを生成する**AI**、コードを最適化する**コンパイラ**、コードを実行する**ハードウェア**——この4者を同時に一直線に揃えます。

> Less code. Predictable performance. Nothing hidden.
> (コードは少なく。性能は予測可能に。隠し事はしない。)

これは初期段階のプロジェクトです。権威ある設計は `draft.md` + `docs/` にあり、コンパイラ(`alignc`)は `crates/` 以下で Rust により実装が進められています。

## Align を選ぶ理由

- **データ指向のコア。** 配列とスライスが言語の中心です。`prices.map(with_tax).where(in_stock).sum()` と書くだけで、コンパイラはこれを中間配列なしの1つのループに融合します——キャッシュにも SIMD にも優しい、なぜなら普通に書いたコードがそのまま良く落ちるからです。
- **隠し事をしない。** アロケーション、エラー、副作用、並列処理は常にソースコード上で可視です。隠れたコピーも、例外も、裏で勝手に生成されるスレッドもありません。
- **やり方は一つだけ。** エラーモデルは一つ(`Result` + `?`)、オプショナルモデルも一つ(`Option`、null なし)、所有権モデルも一つ(値 / `arena` / heap)、並列モデルも一つ(`map` / `reduce` / `task_group`)。
- **手動メモリ管理なし、GC なし。** 所有権は型の性質であり、ライフタイムはリージョンとして推論されます——書く必要はありません。データが*どこに*存在するか(値、`arena`、あるいは heap)を選ぶだけで、あとはコンパイラが解放処理を挿入します。

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

言語に初めて触れる方は、まずガイドから始めてください——Align で考え、書くための実践的な入門です。

**[チュートリアル(日本語)](docs/guide/ja/README.md)** · **[Tutorial (English)](docs/guide/README.md)**

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

まだ初期段階ですが、パイプラインは端から端まで動作します(`lexer → parser → sema → MIR → LLVM → native`):関数と制御フロー、構造体、フルセットのプリミティブ型、`?` を伴う `Option`/`Result`、move・エスケープチェック付きの `arena`/`box`、融合された配列パイプライン、文字列と `json`、SIMD(`vecN`/`soa`/`group_by`)、実スレッド上の `par_map`/`task_group`、`unsafe`/FFI、そして拡充中の標準ライブラリ(`io`/`fs`/`path`/`env`/`time`/`encoding`/`rand`)。マイルストーンの詳細は `docs/impl/07-roadmap.md` を参照してください。

## パフォーマンスと移植性

デフォルトのビルドは**安全で移植性の高いアーキテクチャ別ベースライン**(amd64 では `x86-64-v2`、arm64 では `armv8-a`/NEON)を使用します。これにより、1つのバイナリが混在クラウドフリート上で動作します。より積極的なターゲットは**オプトインであり、決してデフォルトにはなりません**——ホスト固有のビルドには `--target-cpu native`、移植可能な AVX2/FMA ティアには `x86-64-v3` を指定します。多様なフリート全体での広い SIMD 活用は、ベースラインを引き上げることではなく、ライブラリ内の実行時 CPU 機能ディスパッチから得られるべきものです。詳細は `draft.md` §3.4 と `docs/open-questions.md`(「Build targets & portability」)を参照してください。

## レイアウト

- `draft.md` — 権威ある言語仕様
- `docs/guide/` — 実践的なチュートリアル(英語 + 日本語)
- `docs/` — 設計の根拠、経緯、非目標、未解決の論点
- `docs/impl/` — コンパイラ実装計画 + 標準ライブラリのモジュール設計仕様
- `crates/` — `alignc` コンパイラのワークスペース

## ライセンス

MIT
