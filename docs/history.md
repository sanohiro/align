# Align の歴史

## 最初の発想

プロジェクトは単純な観察から始まった。

> 同じことが何通りもの書き方を持つべきではない。

これが次へとつながった。

```text
one error model
one ownership model
one optional model
```

---

## 性能の議論

焦点は次へ移っていった。

```text
cache locality
allocation cost
memory layout
```

生の命令性能よりも、こちらへ。

観察:

キャッシュはしばしば SIMD より重要である。

---

## データ指向への方向

議論は OOP から離れていった。

向かった先:

```text
array processing
SoA
hot/cold split
chunk processing
```

---

## AI 時代の議論

大きな気づき:

プログラミングは今やこうである。

```text
Human -> AI -> Compiler
```

これが優先順位を変えた。

言語が最適化すべきは次である。

```text
convergence
predictability
consistency
```

最大の自由ではなく、こちらを。

---

## エラー処理

例外ベースの方式は却下された。

Go 風の明示的エラー処理は冗長すぎると判断された。

選んだ方向:

```text
Result<T,E>
?
```

---

## メモリモデル

GC ファーストの方式は却下された。

Rust 風の可視ライフタイムは重すぎると判断された。

選んだ方向:

```text
value types
arena
explicit heap
unsafe isolation
```

---

## SIMD の方向

目標:

SIMD を書かせることではない。

自然と SIMD になるコードを書かせること。

これが次を導いた。

```text
map
reduce
scan
mask
vec
```

これらがコア概念になった。

---

## 文字列と JSON の方向

繰り返しの scan が主要なコストとして特定された。

選んだ方向:

```text
scan once
reuse metadata
builder output
zero copy
field tables
```

---

## コンパイラ親和の方向

制限は意図的に加えられた。

目標:

コンパイラの推論を可能にすること。

プログラマの注釈を要求するのではなく。

---

## ライブラリ構成

最終的な方向:

```text
core
std
pkg
```

core はデータ処理のプリミティブを含む。

std は OS 統合を含む。

pkg はフレームワークとエコシステムを含む。

---

## 命名

いくつかの名前が検討された。

例:

```text
Opt
Air
Bound
Fuse
Grain
```

最終的な本命:

```text
Align
```

理由は、次の一致(alignment)を表すから。

```text
Human
AI
Compiler
Hardware
```

同時に次も指している。

```text
memory alignment
cache alignment
SIMD alignment
```
