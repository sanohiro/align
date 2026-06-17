# Align 設計ノート

## なぜ Align が存在するのか

Align は新しい構文を発明する試みではない。

Align が存在するのは、現代のソフトウェア開発が変わったからである。

旧来のモデル:

```text
Human -> Code -> Compiler
```

新しいモデル:

```text
Human -> AI -> Code -> Compiler
```

言語設計はこの現実を反映しなければならない。

---

## 4つの一致

Align は次の4者を一致(align)させようとする。

```text
Human
AI
Compiler
Hardware
```

ほとんどの言語は人間だけに最適化している。

Align はこの4者すべてを第一級の市民として扱う。

---

## 中心的な観察

現代の CPU は極めて速い。

現代のコンパイラは極めて高度である。

現代の AI はコードを書ける。

それでも開発者は依然として次を手で最適化している。

```text
allocation
cache locality
SIMD
branch prediction
parallelism
```

Align は、最適な道をそのまま既定の道にしようとする。

---

## より少ないコード

設立の信念のひとつ。

> コードが少なければバグも少ない。

言語は可能な限りボイラープレートを取り除くべきである。

ただし、次は隠さない。

```text
allocation
errors
parallelism
unsafe operations
```

これらは見えたままにする。

---

## 表現力より収束性

多くの現代言語は表現力を最大化する。

Align は収束性を最大化する。

目標:

```text
異なる開発者
異なる AI モデル
異なるコードベース
```

が、自然と似た解にたどり着くこと。

---

## ひとつの道

Align は次を強く好む。

```text
ひとつのエラーモデル
ひとつの optional モデル
ひとつの所有モデル
ひとつの並列モデル
```

複数の競合する方式よりも、ひとつに収束させる。

---

## まずコンパイラ親和

Align は意図的に制限的である。

制限は弱点ではない。

制限はコンパイラへの情報になる。

コンパイラは次を推論できるべきである。

```text
contiguous memory
no alias
cold error path
arena lifetime
non-null values
```

複雑な注釈を要求せずに、である。

---

## まずハードウェア親和

性能はまずキャッシュから始まる。

SIMD より前に。

GPU より前に。

並列化より前に。

重要な概念:

```text
contiguous memory
SoA
hot/cold split
arena
chunk processing
```

---

## SIMD の考え方

Align は開発者に SIMD を書かせようとはしない。

Align は普通のコードが自然と SIMD 向きになるようにする。

例:

```text
map
reduce
scan
filter
mask
```

これらが自然にベクトル化コードへ落ちるべきである。

---

## GPU の考え方

Align は GPU 言語ではない。

Align は将来の GPU 実行を可能なまま保とうとするだけである。

データ指向の操作を好むのは、それらが次へ自然に対応づくからである。

```text
CPU
SIMD
GPU
```

---

## 文字列の考え方

文字列は魔法のオブジェクトではない。

文字列はデータである。

目標:

```text
scan once
zero copy
builder based output
string pools
```

繰り返しの scan は避けるべきである。

---

## JSON の考え方

JSON は現代 API における事実上のアセンブリ言語である。

Align は JSON を第一級の関心事として扱う。

目標:

```text
SIMD scanning
typed decode
zero-copy strings
field tables
arena allocation
```

---

## 安全性の立ち位置

Align は意図的に次の中間に位置する。

```text
Rust
Zig
```

立ち位置:

```text
safer than Zig
simpler than Rust
```

通常コードは安全であるべきである。

危険なコードは隔離されるべきである。

---

## AI の考え方

AI フレンドリーさは機能ではない。

それは設計上の制約である。

避けるもの:

```text
complex lifetime systems
macro systems
multiple paradigms
excessive abstraction
```

好むもの:

```text
predictability
clarity
consistency
```

---

## ひとことで

Align は、人間の意図・AI の生成・コンパイラ最適化・現代ハードウェアを一致させる、データ指向の言語である。
