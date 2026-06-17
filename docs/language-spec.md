# Align 言語仕様 v0.1（要約）

`draft.md`（権威ある詳細仕様）の要約。詳細・最新はつねに `draft.md` を参照する。

## 目的

Align は次を一致(align)させるために設計された AOT コンパイル言語である。

* 人間の意図
* AI が生成するコード
* コンパイラ最適化
* 現代ハードウェア

Align が優先するもの:

```text
Less code
Predictable performance
Compiler-friendly design
Data-oriented programming
```

## 中核原則

* 同じことはひとつの書き方
* 隠れた allocation はない
* 隠れた error はない
* 隠れた parallelism はない
* 既定でデータ指向
* 既定でキャッシュ親和的
* 既定で SIMD 親和的
* 既定で AI 親和的

## 含むもの

### 型

```text
bool

i8 i16 i32 i64
u8 u16 u32 u64

f32 f64

char

str
string
bytes
buffer
builder

Option<T>
Result<T,E>

array<T>
slice<T>

vec<N,T>
mask<T>
bitset
```

### メモリ

```text
value types
arena
explicit heap
unsafe
```

### エラー処理

```text
Result<T,E>
?
```

例外はない。

### データ処理

```text
map
par_map
filter
where
reduce
scan
partition
group_by
sort
chunks
```

### 縮約

```text
sum
min
max
count
any
all
dot
```

### 文字列

```text
str
string
bytes
buffer
builder
```

### JSON

```text
json.scan
json.decode<T>
json.encode<T>
json.validate<T>
```

### テンプレート

```text
template
html
json
raw
```

### 並列

```text
par_map
reduce
chunks
task_group
```

v1 に async/await はない。

### 安全性

通常コード:

```text
safe
```

危険な操作:

```text
unsafe
```

unsafe ブロックの内側でのみ。

## コアライブラリ

```text
core.option
core.result

core.array
core.slice

core.vec
core.mask
core.bitset

core.builder

core.json
core.template

core.hash
core.math

core.arena
```

## 標準ライブラリ

```text
std.io
std.fs
std.path
std.process
std.env
std.time
std.net
std.cli
std.encoding
std.compress
std.rand
std.crypto
std.http
```

## パッケージ

```text
pkg.db.*
pkg.web.*
pkg.rpc.*
pkg.cloud.*
pkg.ai.*
```

言語コアの一部ではない。
