# クロージャと並列性

> 🌐 [English](../10-closures-and-parallelism.md) · **日本語**

並列処理は、隠れたものに命を取られる場所です。隠れた共有状態、隠れた副作用、隠れたスレッド。だからこそ Align の並列処理は、目に見える 2 つの部品 ― 値キャプチャを持つクロージャと、**推論される純粋性** ― の上に、2 つの構文 ― データ並列の `par_map` とタスク並列の `task_group` ― として組み立てられています。ほかにスレッドを生成するものはありません。

## ラムダ

ラムダは `fn` + パラメータ + ブロックです。[06](06-pipelines.md) 章からずっとパイプラインで使ってきたものです。

```align
[1, 2, 3].map(fn x { x * 2 }).sum()
[1, 2, 3, 4].reduce(0, fn acc, x { acc + x })
```

ラムダは**値でキャプチャ**します。内側で使う外側の束縛は、生成時にコピーされて取り込まれます。共有された変更可能な環境は存在せず、これこそが下記の並列構文を安全にしている当のものです。

```align
factor := 3
print([1, 2, 3].map(fn x { x * factor }).sum())     // 18
```

## 値としての関数

`fn(T) -> R` 型のパラメータ(や束縛)は、名前付き関数・ラムダ・キャプチャするクロージャを受け取ります。

```align
fn apply(f: fn(i64) -> i64, x: i64) -> i64 = f(x)

fn double(x: i64) -> i64 = x * 2

fn main() -> i32 {
    print(apply(double, 21))            // 42 — named function
    print(apply(fn n: i64 { n + 1 }, 41))   // 42 — lambda
    k: i64 := 100
    print(apply(fn n: i64 { n + k }, 5))    // 105 — capturing closure
    twice := fn x: i64 { x * 2 }        // a lambda as a value (params must be typed)
    print(twice(6))                     // 12
    return 0
}
```

現時点の制限(実装中)を挙げておきます。値に束縛したラムダにはパラメータの型注釈が必要で、関数値を**返す** ― あるいは構造体や配列に格納する ― ことは、リージョンに裏打ちされたクロージャ環境が入るまで先送りです。関数を*下へ*渡すことはパイプラインと 2 つの並列構文の両方をカバーしており、これが荷重を支えるケースです。

## 純粋性は推論される ― そして並列性はそれを要求する

コンパイラはすべての関数について、それが**Pure** かどうか(I/O なし、rng なし、FFI なし、外部の何ものへのミューテーションもなし)を推論します。注釈を付けることは決してなく、間違えようがありません。それが自分を守ってくれるときにだけ、その存在に気づきます。

```align
fn show(x: i64) -> i64 {
    print(x)        // I/O — show is Impure
    return x
}

ys := [1, 2].par_map(fn x { show(x) })
// error: 'par_map' requires a Pure function, but the lambda has a side
//        effect (it reads/writes I/O)
```

データ競合には、共有された変更可能な状態か、順序のない副作用が必要です。入力を値でキャプチャする Pure な関数には、そのどちらもありません。だから Align は競合を検出するのではなく、並列構文の中で競合を**表現不可能**にします。コンパイル時に、しかも `Send` / `Sync` という語彙を覚える必要なしに。

## `par_map` ― データ並列

```align
Emp { base: i64, bonus: i64 }

fn net(e: Emp) -> i64 = e.base + e.bonus

fn main() -> Result<(), Error> {
    pay := [
        Emp { base: 30, bonus: 12 },
        Emp { base: 18, bonus: 4 },
    ].par_map(net)          // fan out across a persistent worker pool
    print(pay.sum())        // 64
    return Ok(())
}
```

`par_map(f)` は永続的なワーカースレッドプールをまたいだ `map` で、所有された `array<R>` をマテリアライズします。意味的には `map` と同一です ― 純粋性がそれを保証します ― ので、データサイズの変化に応じて両者を自由に切り替えられます。

そして、そうすべきです。**`par_map` が元を取るのは `f` が高価なときだけ**です。要素ごとに間接呼び出しをまたぐのに対し、逐次の `map` はベクトル化されたループに融合します。安価な算術なら、素朴な `map().sum()` のほうがたいてい*速い*のです。手を伸ばす前に計測してください。(キャプチャするクロージャは現時点では逐次実行にフォールバックします ― 実装中です。)

## `task_group` ― タスク並列

異種の仕事 ― 「この 3 つを同時にやってから合わせる」 ― には次のように書きます。

```align
fn main() -> Result<(), Error> {
    base: i64 := 100
    task_group {
        a := spawn(fn { base + 5 })     // runs on a real thread
        b := spawn(fn { base * 2 })
        wait()                          // join everything spawned in this group
        print(a.get() + b.get())        // 305
    }
    return Ok(())
}
```

`spawn(fn { ... })` はタスクを起動してハンドルを返し、`wait()` はそのすべてを join し、`.get()` は join のあとで結果を読みます。ブロックがライフタイムです。タスクは自分の `task_group` より長生きできません ― 構造的に。切り離されたスレッドも、忘れられた join もありません。スコープがそれを書かせてくれないからです。

失敗しうるタスクは `Result` を返し、join 点が `?` をまといます。

```align
fn fetch(n: i64) -> Result<i64, Error> {
    if n < 0 { return Err(error(2)) }
    return Ok(n * 10)
}

fn main() -> Result<(), Error> {
    task_group {
        a := spawn(fn { fetch(3) })
        b := spawn(fn { fetch(-1) })
        wait()?                         // joins ALL tasks, then propagates the first error
        print(a.get() + b.get())        // not reached
    }
    return Ok(())
}
```

`wait()?` はグループのエラー境界です。すべてのタスクが完了し(中途半端に join された状態は生じません)、そのうえで最初の失敗が通常の `Err` として伝播します。並列のエラー処理も、他のすべてと同じたった 1 つの演算子で行います。

## どちらを、いつ

- 多数の要素に同じ関数を → `par_map`。ただしその関数が、ベクトル化された逐次ループを上回るほど高価なときに限ります。
- いくつかの異なる仕事を同時に → `task_group`。
- それ以外すべて → 逐次パイプライン。これはすでに SIMD レーンを並列に使っています([12](12-simd.md) 章)。

そのすべてがソースに見えています。`par_map` と `spawn` は、言語の中で「別のスレッドがこれを走らせる」を意味する 2 語だけです。
