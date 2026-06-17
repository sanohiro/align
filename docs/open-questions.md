# 未解決の論点

設計上の論点を「決着済み」「未解決」「v1 範囲外」に分けて管理する。決着済みは決定内容と記録先を残す（再燃を防ぐため）。

---

## 決着済み

### コンパイラバックエンド
**決定: LLVM。ただし backend 非依存の MIR を必ず挟む。**
「C backend 先行 → 後で LLVM」は採らない（後回しの罠 + SIMD 制御を失う）。意味論は MIR に置き `MIR → LLVM` は純粋 lowering。将来の別 backend は lowering 追加で対応。
記録: `impl/00-overview.md`, `impl/04-mir.md`, `impl/05-backend-llvm.md`

### 構文: 文終端とレイアウト
**決定: Go スタイル。** 改行が文を終端、`;` は1行に詰めるとき用の任意セパレータ。ブロックは `{}` でインデントは無意味（非 Python）。行頭 `.`/二項演算子は前行の継続。
理由: 「綺麗さ（`;`なし）」「自由（ワンライナー可）」「非 Python（レイアウト非強制）」を同時に満たす。
記録: `draft.md` §4, `impl/01-pipeline.md`, `impl/02-frontend.md`

### 整数オーバーフロー
**決定: 既定は2の補数 wrap（UB にしない・ゼロコスト・SIMD を妨げない）。** 明示 op（`checked_*`/`saturating_*`/`wrapping_*`）を提供。開発時のみ任意のチェック付きビルド。ゼロ除算等は別扱いで必ずエラー。
記録: `draft.md` §5, `impl/03-types.md`

### 型宣言の構文
**決定: キーワードなし。** `ident: Type` を含めば struct、`ident`/`ident(...)` なら sum type と中身で判別。フィールド/variant は `,` 区切り。
記録: `draft.md` §4, `impl/02-frontend.md`

### 純粋性モデル
**決定: コンパイラ推論（明示マークなし）。** 効果（Pure/Impure）を本体から推論し、`par_map` 等のクロージャに Pure を要求。
記録: `impl/03-types.md` §8

### 所有権の構文
**決定: 所有はキーワードでなく型の性質。** `array<T>`/`string`/`buffer`/heap は Move、プリミティブ/小 struct/`slice`(view) は Copy。`owned` 修飾子は導入しない。寿命は推論し lifetime 構文を表に出さない。
記録: `impl/03-types.md` §6–§7

### SIMD の露出（基本方針）
**決定: `vec<N,T>` + 自動ベクトル化を基本線。** mask を第一級に。
（明示 SIMD intrinsics を std に置くかは未解決、下記参照）
記録: `draft.md` §9, `impl/04-mir.md` §4, `impl/05-backend-llvm.md` §5

### リフレクション
**決定: なし。** 限定的なコンパイル時 reflection の可否のみ将来検討。

### データベースのエコシステム
**決定: パッケージに委ねる。** core/std に SQL 抽象は入れない。土台部品（bytes/buffer/json/reader-writer 等）は core/std に置く。
記録: `draft.md` §18.3

---

## 未解決（要決着）

各項目に決着目標マイルストーン（`impl/07-roadmap.md`）を付す。

### ジェネリクス（最小システム）— M4 前
構造的制約推論 vs 明示境界（trait 風）。単相化の実装単位。`vec<N,T>` の値ジェネリクス。core を Align 自身で書くために必要（`impl/03-types.md` §9, `impl/06-runtime-std.md` §10）。

### エラー型の設計 — M2
single `Error` / typed errors / error categories。`?` の `E → E'` 変換規則と終了コード対応も含む（`impl/03-types.md` §5, `impl/06-runtime-std.md` §9）。

### 明示 allocator 付き arena — M3
`arena a {}` のような形を入れるか。ネスト arena の region 順序・chunk 共有（`impl/03-types.md` §7, `impl/06-runtime-std.md` §3）。

### SIMD intrinsics の std への公開
自動ベクトル化に加え、明示 intrinsics を std に置くか（`impl/04-mir.md` §9）。

### SoA 変換のトリガ
`array<T>` を SoA に置く判定を自動化するか注釈か。array ABI への影響（`impl/05-backend-llvm.md` §2）。

### ビルドシステム / パッケージ配置
可視性（`pub`）・import・module は決定済み（`impl/02-frontend.md`）。残るのはビルドシステムとパッケージ配置・依存解決の設計。

### FFI（外部関数インターフェース）— M8 後
C / Rust / Zig 相互運用の詳細設計。

### 細目（実装中に決着）
```text
- 数値リテラルの接尾辞集合と既定型の lint
- match 網羅性判定アルゴリズム / ガード / | 複数パターン
- Copy/Move を分ける struct サイズ閾値
- ベクトル幅 W の決定（vec<N> 固定 vs ターゲット ISA）
- LLVM 最適化パイプラインの採用範囲
- 文字列補間に {expr} を許すか（{ident} のみか）
- スレッドプール寿命 / par reduce の浮動小数再現性
- panic 捕捉境界を設けるか（現状: 即アボート）
```
詳細は各 `impl/*.md` の「未決事項」に対応。

---

## 将来（v1 範囲外）

```text
GPU backend
distributed execution
incremental compilation
self-host
```
MIR を backend 非依存に保つことで将来追加を阻害しない（`impl/00-overview.md`）。
