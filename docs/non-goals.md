# Align の非目標

以下は意図的に目標としない。

## C++ の代替ではない

Align はあらゆるプログラミングスタイルを支えようとはしない。

---

## 最大の表現力を目指さない

Align は次に最適化しない。

```text
metaprogramming
DSL creation
advanced type wizardry
```

---

## OOP ファーストではない

Align はオブジェクト指向ではない。

次を支える目標はない。

```text
class hierarchies
inheritance
deep object graphs
```

---

## ランタイムの魔法を持たない

避けるもの:

```text
hidden allocation
hidden exceptions
hidden thread creation
hidden copying
```

---

## どこでも async にしない

Align は言語を async/await 中心に組み立てない。

主たるモデル:

```text
map
reduce
chunks
task_group
```

---

## トレイトの複雑さを持たない

Rust 風の複雑さを避ける。

---

## テンプレートの複雑さを持たない

C++ 風のテンプレート複雑性を避ける。

---

## GC ファーストではない

Align はガベージコレクション中心の設計ではない。

---

## フレームワーク駆動ではない

Web フレームワーク、ORM、クラウド SDK、AI SDK はパッケージに属する。

core ではない。

---

## GPU 専用ではない

Align は GPU 互換である。

GPU 中心ではない。

---

## 学術的純粋さを追わない

理論的な美しさよりも、実用的な性能を優先する。
