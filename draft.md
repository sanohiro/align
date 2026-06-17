# Align Language Specification Draft v0.1

## 1. Vision

Align は、人間・AI・コンパイラ・ハードウェアが同じ方向を向ける AOT コンパイル言語である。

目的は、

> 少なく書く。予測可能に速い。

である。

Align は「高速化テクニックを書く言語」ではなく、「普通に書くとコンパイラとハードウェアが最適化しやすい形になる言語」を目指す。

---

## 2. Target Use Cases

主対象。

```text
CLI tools
batch processing
API server foundation
data processing
JSON / HTTP processing
compiler / parser / tooling
systems-adjacent applications
```

非主対象。

```text
OS kernel
browser frontend
large OOP application
dynamic scripting
heavy GUI framework
```

---

## 3. Core Philosophy

### 3.1 One Way

同じことは基本的に一つの書き方にする。

```text
error      = Result + ?
optional   = Option
memory     = value / arena / explicit heap
parallel   = map / reduce / chunks
string     = str / string / buffer / builder
```

### 3.2 Less Code, Fewer Bugs

書く量が減ればバグは減る。

ただし、以下は隠さない。

```text
allocation
error
side effect
parallelism
unsafe
```

### 3.3 Compiler Friendly

コンパイラが次を推論しやすい設計にする。

```text
contiguous memory
non-null
no-alias
arena lifetime
cold error path
pure-ish function
loop independence
alignment
```

### 3.4 Hardware Friendly

現代 CPU / GPU / Cache / SIMD / Branch Predictor に素直なコードを生成しやすくする。

---

## 4. Basic Syntax

### Variables

```align
x := 10
name := "sano"

mut count := 0
count = count + 1
```

デフォルトは immutable。

### Type Annotation

```align
x: i64 := 10
```

### Function

```align
fn add(a: i32, b: i32) -> i32 {
  return a + b
}
```

単一式の関数は `= expr` 形で書く。

```align
fn add(a: i32, b: i32) -> i32 = a + b
```

### Statement Terminator (Go style)

文は改行で終端する（Go スタイル）。普段 `;` は書かない。インデントは意味を持たず（ブロックは `{}`）、Python のようなレイアウト強制はない。

```align
fn classify(u: User) -> str {
  s := score(u)
  if s > 80 { "high" } else { "low" }
}
```

`;` は**任意のセパレータ**で、1行に複数文を詰めたいときだけ使う。`{}` があるのでどんなブロックも1行に inline できる（ワンライナーの自由）。

```align
fn classify(u: User) -> str { s := score(u); if s > 80 { "high" } else { "low" } }

point := { mut a := x; a = a * 2; a }
```

行頭が `.` や二項演算子なら前の行の継続。チェーンを `;` なしで複数行に書ける。

```align
total := users
  .where(.active)
  .score
  .sum()
```

### Block Value

ブロックの末尾に置いた式（後ろに文が続かないもの）は、そのブロックの値になる。値にしたくない式文はそのまま並べる。

```align
fn abs(x: i32) -> i32 = if x < 0 { -x } else { x }

user := find_user(id) else return Error.NotFound
```

`if` / `else` 取り出し / `match` は式であり、単一式なら自然に一行になる。

### Style and Convergence

公式 formatter（§16）は**意味のないブレだけ**を正規化する。

```text
正規化する     空白 / ; の置き方 / 末尾カンマ / 整列
正規化しない   一行 ↔ 複数行 の選択（作者の自由を残す。Python 的強制をしない）
```

「書き方は単一」とは「許されるレイアウトが1つ」ではなく「**あるレイアウトに対する正しい整形が1つ**」の意味（gofmt / rustfmt と同じ）。単一式の本体は `= expr`、複数文の本体は `{}` ブロック、という形ごとの正規形は保ちつつ、行の詰め方は強制しない。

### Struct

```align
User {
  id: i64,
  name: str,
  active: bool,
  score: i32,
}
```

class / inheritance は持たない。

---

## 5. Types

### Primitive Types

```text
bool

i8 i16 i32 i64
u8 u16 u32 u64

f32 f64

char
```

### Integer Overflow

整数演算は溢れても**未定義動作にしない**。既定は2の補数の wrap（全ビルドで同一・分岐なし）。

```text
既定        wrap (定義済み、ゼロコスト、SIMD を妨げない)
明示 op     checked_*(-> Option) / saturating_* / wrapping_*
開発時      overflow チェック付きビルド + lint でバグ検出 (意味論は変えない)
```

理由は「予測可能に速い」の徹底。全ビルドで挙動が同じで、ホットループのベクトル化を壊さない。安全が要る箇所は明示 op を使う。

ゼロ除算など溢れ以外の算術エラーはこれと別扱いで、silent にせず必ずエラーにする。

### Optional

```align
Option<User>
```

null はない。

```align
user := find_user(id) else {
  return Error.NotFound
}
```

### Result

```align
Result<T, E>
```

```align
data := fs.read_file(path)?
user := json.decode<User>(data)?
```

`?` は Result 専用。

---

## 6. Memory Model

### 6.1 Default

```text
GCなし
値型中心
heapは明示
arena標準
unsafeは隔離
```

### 6.2 Value

```align
p := Point{x: 1, y: 2}
```

小さい struct は値として扱う。

大きい値のコピーは lint 対象。

### 6.3 Move

所有型は基本 move。

```align
data := fs.read_file(path)?
other := data

print(data) // compile error
```

明示 clone。

```align
other := data.clone()
```

### 6.4 Arena

```align
arena {
  data := fs.read_file(path)?
  users := json.decode<array<User>>(data)?
  process(users)?
}
```

arena 内の allocation はブロック終了時にまとめて解放。

arena 内の view は arena 外へ出せない。

### 6.5 Heap

```align
p := heap.new(User{id: 1})
```

通常コードでは manual free しない。

raw allocation は unsafe のみ。

```align
unsafe {
  p := raw.alloc(size)
  raw.free(p)
}
```

---

## 7. Array and Slice

### Array

```align
users: array<User>
```

`array<T>` は所有する連続メモリ。

### Slice

```align
items: slice<User>
```

`slice<T>` は view。

### Out Parameter

```align
fn add(out dst: slice<f32>, a: slice<f32>, b: slice<f32>) {
  dst = a + b
}
```

`out` は no-alias の最適化ヒントであり、安全制約でもある。

---

## 8. Data Processing Core

Align の中核は配列処理である。

### Basic Operations

```align
scores := users.map(calc_score)
active := users.where(.active)
total := active.score.sum()
```

### Core Array Functions

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

### Reductions

```text
sum
min
max
count
any
all
dot
```

### Chunk Processing

```align
users
  .chunks(1024)
  .par_map(process_chunk)
```

並列化の単位は基本的に chunk。

---

## 9. SIMD and Vector

### Fixed Vector Types

```align
vec2<f32>
vec4<f32>
vec8<i32>
vec16<u8>
```

Example.

```align
a: vec4<f32>
b: vec4<f32>
c := a + b
d := dot(a, b)
```

### Array Expressions

```align
a = b + c
a = (b + c) * d - e
```

一時配列を作らず loop fusion する。

### Mask

```align
m := scores > 80
total := scores.sum_where(m)
```

mask は SIMD / branchless / GPU のための第一級概念。

---

## 10. Branch and Hot Path

`if` は持つ。

ただし大量データ処理では branch を中心にしない。

```align
active := users.where(.active)
total := active.score.sum()
```

Result の失敗経路は cold path として扱いやすくする。

```align
data := fs.read_file(path)?
json := json.parse(data)?
```

---

## 11. Parallelism

### Philosophy

thread / mutex を普通にしない。

基本は data parallel。

```align
scores := users.par_map(calc_score)
```

### Side Effect Rule

`par_map` に渡す関数は外部 mutable state を変更できない。

禁止例。

```align
mut total := 0

users.par_map(fn u {
  total = total + u.score
})
```

代わりに reduce。

```align
total := users.reduce(0, fn acc, u {
  acc + u.score
})
```

### Task Group

I/O 並行は task_group。

```align
tasks := task_group()

a := tasks.spawn(fs.read_file("a.txt"))
b := tasks.spawn(fs.read_file("b.txt"))

tasks.wait()?
```

async/await は初期仕様には入れない。

---

## 12. String

### Types

```text
str      // read-only view
string   // owned string
bytes    // read-only byte view
buffer   // mutable byte buffer
builder  // append-oriented writer
```

### No Implicit Concatenation Allocation

```align
msg := a + b // string allocation は禁止または lint
```

推奨。

```align
b := builder()
b.write("hello ")
b.write(name)
msg := b.to_string()
```

### Static String Meta

文字列リテラルはコンパイル時に meta を持つ。

```text
len
hash
ascii
utf8_valid
json_escape_needed
html_escape_needed
```

表面上は `str` として扱う。

### Const String Pool

以下は const string pool に置ける。

```text
literal strings
JSON field names
template static parts
HTTP header names
```

### Scan Once

同じ byte列を何度も scan しない。

標準 parser は scan 結果を再利用する。

---

## 13. Template

テンプレートは実行時 format parse ではなく、コンパイル時解析する。

```align
msg := template "Hello {name}, score={score}"
```

内部的には、

```text
write_static("Hello ")
write_value(name)
write_static(", score=")
write_value(score)
```

へ展開される。

### Escaping Context

```align
html "<p>{name}</p>"
json "{name}"
```

raw 出力は明示。

```align
raw(name)
```

---

## 14. JSON

JSON は Align の core に近い機能とする。

理由は、Align の強みである以下がすべて活きるため。

```text
SIMD scan
scan once
zero-copy
arena
typed decode
field table
builder encode
```

### Typed Decode

```align
user := json.decode<User>(data)?
```

### Zero Copy

escape がなければ入力 buffer への `str` view を返す。

escape がある場合のみ decode buffer を使う。

### Struct as Schema

```align
User {
  id: i64,
  name: str,
  active: bool,
}
```

struct 定義から以下を生成可能にする。

```text
decode
encode
validate
field table
```

### Field Table

コンパイル時に field 情報を持つ。

```text
name
len
hash
first byte
offset
escape info
```

### SIMD Scan

JSON scanner は structural chars を SIMD で探す。

```text
{ } [ ] : , " \ whitespace
```

---

## 15. Safety

通常コードでは以下を禁止または制限する。

```text
use-after-free
uninitialized read
data race
manual free
raw pointer
unchecked cast
```

危険な処理は `unsafe` ブロックのみ。

```align
unsafe {
  p := raw.ptr_cast<T>(x)
}
```

Rust の lifetime を表には出さない。
ただし arena 外への view 流出など、明らかな寿命違反はコンパイルエラー。

---

## 16. AI Friendly Rules

### Formatter

公式 formatter を必須とする。空白 / `;` の置き方 / 末尾カンマ / 整列など**意味のないブレ**だけを正規化し、一行 ↔ 複数行の選択は強制しない（§4 Style and Convergence）。

### Lint

標準 lint で検出する。

```text
loop内allocation
巨大struct copy
不要clone
不要heap
未処理Result
hot loop内branch
string再scan
暗黙copy
```

### Convergence Over Expression

表現力より収束性を重視する。

AI が迷う自由度は減らす。

---

## 17. Modules

```align
module main

import core.json
import std.fs
```

公開は明示。

```align
pub fn main(args: array<str>) -> Result<(), Error> {
}
```

---

# 18. Library Layout

## 18.1 core

`core` は言語思想そのものに近い基盤。

```text
core.option
core.result

core.array
core.slice
core.chunks

core.vec
core.mask
core.bitset

core.map
core.reduce
core.scan
core.partition
core.sort

core.str
core.string
core.bytes
core.buffer
core.builder

core.arena

core.json
core.template

core.hash
core.math
```

### core.array / core.slice

```text
array<T>
slice<T>
chunks
map
filter
where
reduce
scan
partition
sort
group_by
```

### core.vec / core.mask

```text
vec<N,T>
mask<T>
bitset
dot
sum_where
select
```

### core.string

```text
str
string
bytes
buffer
builder
find
find_any
split
trim
contains
starts_with
ends_with
```

SIMD fast path を標準実装に持つ。

### core.json

```text
json.scan
json.decode<T>
json.encode<T>
json.validate<T>
json.token
json.field_table<T>
```

### core.template

```text
template
html
json template
raw
```

### core.hash

非暗号用途の hash。

```text
hash64
hash128
```

暗号 hash は std.crypto。

---

## 18.2 std

`std` は OS との境界。

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

### std.io

```text
reader
writer
stream
stdin
stdout
stderr
```

### std.fs

```text
read_file
write_file
open
create
remove
exists
read_dir
```

### std.path

```text
join
base
dir
ext
normalize
```

### std.process

```text
spawn
exec
exit
```

### std.env

```text
args
get
set
```

### std.time

```text
now
instant
duration
sleep
```

### std.net

低レベル中心。

```text
tcp
udp
dns
socket
```

### std.cli

```text
args
flags
command
usage
```

### std.encoding

```text
base64
base64url
hex
utf8
```

### std.compress

```text
gzip
zstd
```

### std.rand

非暗号用途。

```text
seed
range
shuffle
sample
```

### std.crypto

暗号用途。

```text
crypto.random
sha256
sha512
blake3
hmac
hkdf
argon2id
aes_gcm
chacha20_poly1305
constant_time_equal
```

### std.http

フレームワークではなく primitive。

```text
request
response
header
method
status
client
server primitive
```

---

## 18.3 pkg

`pkg` は外部パッケージ領域。

```text
pkg.web
pkg.router
pkg.db.postgres
pkg.db.mysql
pkg.db.sqlite
pkg.orm
pkg.rpc
pkg.aws
pkg.openai
```

DB driver や Web framework は core/std に入れない。

ただし、それらを作りやすくする部品は core/std に置く。

```text
bytes
buffer
builder
arena
json
reader/writer
http primitive
crypto
encoding
```

---

# 19. Example

```align
module main

import core.json
import std.fs
import std.io

User {
  id: i64,
  name: str,
  active: bool,
  score: i32,
}

pub fn main(args: array<str>) -> Result<(), Error> {
  arena {
    data := fs.read_file(args[1])?
    users := json.decode<array<User>>(data)?

    total := users
      .where(.active)
      .score
      .sum()

    out := builder()
    out.write("active score: ")
    out.write_int(total)
    out.write("\n")

    io.stdout.write(out)?
  }

  return ok
}
```

---

# 20. Positioning

```text
Goより allocation と error が見える
Zigより普通の道が安全
Rustより lifetime を書かない
Cより alias と寿命が明確
Pythonより速く、AIが書いても性能が崩れにくい
```

---

# 21. One Sentence

Align is a data-oriented AOT language designed to align human intent, AI generation, compiler optimization, and modern hardware.
