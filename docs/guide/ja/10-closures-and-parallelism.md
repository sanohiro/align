# クロージャと並列性

> 🌐 [English](../10-closures-and-parallelism.md) · **日本語**

Align は、値キャプチャするクロージャと**純粋性の推論**によって、並列処理での共有状態と副作用を制限します。多数の要素に同じ関数を適用するには `par_map`、異なる処理を並行して実行するには `task_group` を使います。

## ラムダ

ラムダ式は、`fn` キーワード、パラメータ、そしてブロックで構成されます。これは [06](06-pipelines.md) 章のパイプライン処理で継続して使用してきたものです。

```align
[1, 2, 3].map(fn x { x * 2 }).sum()
[1, 2, 3, 4].reduce(0, fn acc, x { acc + x })
```

ラムダは外側の Copy 型の変数を**値でキャプチャ**します。生成時の値をコピーして取り込むので、元の `mut` 変数が後から変わっても、クロージャが保持する値は変わりません。書き換え可能な環境を共有する仕組みではありません。

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

現時点では、変数に代入するラムダ式のパラメータには型注釈が必要で、関数値をそのまま戻り値として返す機能はまだ保留されています。関数値は構造体および同一シグネチャの配列・スライスに格納できます。名前付き関数と非キャプチャ関数は再利用でき、フレームをキャプチャした値が環境より長く生存する場合はエスケープ解析が拒否します。

## 純粋性は推論される ― そして並列性はそれを要求する

コンパイラは、各関数が **Pure（純粋）** かどうかを推論します。I/O、乱数生成、FFI 呼び出し、自分が所有しない外部状態の変更がない関数は Pure です。ただし、明示的な `borrow mut` パラメータで渡された値の更新は、呼び出し元が許可した操作なので Pure として扱います。純粋性をソースに注釈する必要はありません。次の例では、I/O を行う関数を `par_map` に渡したためエラーになります。

```align
fn show(x: i64) -> i64 {
    print(x)        // I/O — show is Impure
    return x
}

ys := [1, 2].par_map(fn x { show(x) })
// error: 'par_map' requires a Pure function, but 'main$lambda0' has an
//        observable side effect (I/O or a caller-view write); use `reduce`
//        for an accumulation
```

逐次の `map` なら `show` を呼べます。表示は入力順に行われます。それを `par_map` に変えると、出力順がワーカーの実行順に依存するため拒否されます。並列に渡す関数には、純粋性とキャプチャの両方の制限があります。Move 値のキャプチャは拒否され、確保先を指定する `region` も、Copy 型ですがワーカーへ渡せません。これらはコンパイラが検査し、`Send` や `Sync` の注釈は書きません。

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

`par_map(f)` は永続的なワーカースレッドプールで実行し、所有権を持つ `array<R>` を返します。受け付けられる Pure な関数なら、値と結果の順序は `map(f).to_array()` と同じです。逐次版では `.to_array()` まで必要で、`map(f)` だけではパイプラインが完成しません。ワーカーの完了順で配列の順序は変わりませんが、タスクの割り当てや結果配列の確保にかかるコストはあります。

ただし、切り替えは慎重に行うべきです。**`par_map` のオーバーヘッドを相殺できるのは、適用する関数 `f` の計算コストが十分に高い場合のみ**です。範囲カーネルにも起動とスケジューリングのオーバーヘッドがあり、一方で逐次の `map` は自動ベクトライザによってベクトル化されたループに融合されます。単純な算術演算であれば、素朴な `map().sum()` の方が多くの場合で**速い**のです。`par_map` を採用する前には必ずベンチマークを計測してください。直接ソースの `par_map` と、その前にあるプリミティブスカラーの長さを保つ `map` ステージは、1つの不変な呼び出しスコープのコンテキストを持つ同じ範囲カーネルで実行されます。`where` を含む形式や未対応の形式は逐次実行のままで、Move 値のキャプチャは拒否されます。

## `task_group` ― タスク並列

異なる複数の処理を同時に進め、すべて完了してから結果をまとめるには、次のように書きます。

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

`spawn(fn { ... })` は新しいタスクを起動してタスクハンドルを返し、`wait()` はグループ内のすべてのタスクの完了を待機（join）します。`.get()` は待機が完了した後に結果を読み取ります。この `task_group` ブロック自体がタスクのライフタイムとなります。構造的に、タスクが属する `task_group` よりも長生きすることはできません。切り離されて制御不能になったスレッド（デタッチト・スレッド）や、`join` の呼び忘れは発生しません。言語のスコープ規則がそれを許さないからです。

失敗する可能性があるタスクは `Result` を返します。グループのエラーを伝播させるには `wait()?` と書きます。

```align
fn fetch(n: i64) -> Result<i64, Error> {
    if n < 0 { return Err(error(2)) }
    return Ok(n * 10)
}

fn main() -> Result<(), Error> {
    task_group {
        a := spawn(fn { fetch(3) })
        b := spawn(fn { fetch(-1) })
        wait()?                         // joins ALL tasks, then propagates the lowest-index error
        print(a.get() + b.get())        // not reached
    }
    return Ok(())
}
```

`wait()?` は、タスクグループにおけるエラーの境界として機能します。まずすべてのタスクの完了を待機し（一部のタスクだけが放置されることはありません）、その上で、spawn インデックスが最も低いタスクのエラーが通常の `Err` として伝播します。並列処理におけるエラーハンドリングも、逐次処理と全く同じ `?` 演算子1つで完結します。

## どちらを、いつ

- 多数の要素に同じ処理を適用する → **`par_map`**。要素ごとの処理が十分に重く、スケジューリングのコストを含めても逐次実行より速くなることを計測で確認して使います。
- 性質の異なる複数の処理を並行して実行する場合 → **`task_group`** を使用します。
- それ以外 → **逐次パイプライン**。ワーカースレッドを増やさずに SIMD を使える場合があります（[12章](12-simd.md)）。

これらの並列処理の挙動は、すべてソースコード上で明白に読み取れます。Align において、「別のスレッドでこれを実行する」ということを意味するキーワードは、`par_map` と `spawn` の2つだけです。
