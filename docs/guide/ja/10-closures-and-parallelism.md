# クロージャと並列性

> 🌐 [English](../10-closures-and-parallelism.md) · **日本語**

Align は、値キャプチャするクロージャと**純粋性の推論**によって、並列処理での共有状態と副作用を制限します。多数の要素に同じ関数を適用するには `par_map`、異なる処理を並行して実行するには `task_group` を使います。

## ラムダ

ラムダ式は `fn`、パラメータ、ブロックで構成されます。[06](06-pipelines.md) 章のパイプラインでも使った形です。

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

## 純粋性は推論される ― `par_map` はそれを要求する

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

`par_map(f)` は所有権を持つ `array<R>` を作ります。受け付けられる Pure な関数では、値と結果の並びは `map(f).to_array()` と同じです。単独の `map(f)` は未完了のパイプラインなので、これが完全な逐次処理の対応形です。対応済みの範囲カーネルは、実行時に呼び出し元だけで処理するか、永続的なワーカープールを使うかを選びます。ワーカーの完了順で配列の並びは変わりませんが、実体化やスケジューリングにはコストがあります。対応済みの直接の整数 `par_map(f).sum()` は `range-reduce` を選び、結果全体の中間配列を省けます。`explain-opt` で選択された形式を確認してください。範囲カーネルの表示だけでは、実際にワーカーが動いたとは限りません。

`par_map` は、処理全体が等価な逐次版と比較して計測します。入力サイズ、要素ごとの仕事量、メモリ確保、メモリ帯域、利用可能なワーカー数が結果に影響します。短い処理や軽い演算では準備とスケジューリングが支配し、融合された逐次パイプラインがベクトル化されて速くなる場合があります。Copy 値をキャプチャする直接ソースの `par_map`、対応するスカラーマップとフィルター、対応するフィールド・射影形式では範囲カーネルを使います。フィルター付きの形式では、件数の計算と結果の書き込みで Pure な述語を再評価することがあります。未対応の形式は逐次実行へフォールバックし、Move 値のキャプチャは引き続き拒否されます。選ばれた経路は `alignc explain-opt` で確認し、計測の手順は [26 章](26-performance.md)を参照してください。

## `task_group` ― タスク並列

異なる複数の処理を同時に進め、すべて完了してから結果をまとめるには、次のように書きます。

```align
fn main() -> Result<(), Error> {
    base: i64 := 100
    task_group {
        a := spawn(fn { base + 5 })     // becomes eligible for worker execution
        b := spawn(fn { base * 2 })
        wait()                          // join everything spawned in this group
        print(a.get() + b.get())        // 305
    }
    return Ok(())
}
```

`spawn(fn { ... })` はタスクを開始してハンドルを返し、`wait()` はグループの完了を待ちます。`.get()` は join の成功後に結果を読みます。タスクは `task_group` より長生きできず、早期リターンを含めスコープを離れるときには残りのタスクを join します。

タスクが1つだけでも、`spawn` が戻る前にワーカーが実行できる状態になります。ただし、ワーカーが使用中なら開始は遅れます。キャプチャしたビューは元のデータを共有するため、`wait` まで元のデータを変更・解放できません。タスクハンドルを捨てたり再代入したりしても、この保護は続きます。タスクは共有ビューを読み、自分で作ったデータを変更できます。

`par_map` と異なり、spawn したタスクは I/O などの非純粋な処理を実行できます。`wait()` も自分のグループの未実行タスクを処理するため、すべてのワーカーが使用中でも入れ子のグループが進行できます。ただし、プログラムが作った循環した待ち合わせを解消するものではありません。タスクの結果には、外部の記憶域を借用しないプリミティブ値、string、buffer、通常の動的配列、レコード、タプル、sum 型を使えます。`Result<R, Error>` で包むこともできます。所有データを含められますが、ビュー、ポインタ、関数値、ネイティブハンドルや resource は含められません。wait の成功後、Copy 値の `get()` は繰り返し呼べます。所有値の `get()` はハンドルを消費し、結果を一度だけ移します。結果はグループ終了後も使えます。グループ終了時には全タスクを待ち、wait が失敗した場合も未取得の結果を解放します。共有キャプチャは引き続き読み取り専用です。連続した出力を並列生成するには、各タスクで区間ごとの所有値を作り、名前を付けたハンドルを区間順に取得します。

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

`wait()?` はすべてのタスクを join してから、spawn インデックスが最も低い失敗を通常の `Err` として伝播します。逐次コードと同じ伝播演算子を使います。

### グループ終了後も所有結果を使う

タスクの中で出力を作り、`wait` の後で受け取ります。この関数は、タスクが作った文字列を
もう一度 clone せずに返します。

```align
fn make_label(name: str) -> string {
    task_group {
        task := spawn(fn {
            text := builder()
            text.write("hello, ")
            text.write(name)
            text.to_string()
        })
        wait()
        task.get()
    }
}

fn main() -> i32 {
    label := make_label("Ada")
    print(label)    // hello, Ada
    return 0
}
```

キャプチャした `name` は join まで借用されます。builder は独立した文字列を作り、
`task.get()` が呼び出し側に所有権を移します。もう一度 `task.get()` を呼ぶと、
ムーブ済みハンドルの再利用として拒否されます。`name` 自体を返すと結果に借用ビューが
残るため、対応するタスク結果の型にはなりません。

## どちらを、いつ

- 多数の要素に同じ処理を適用する → **`par_map`**。要素ごとの処理が十分に重く、スケジューリングのコストを含めても逐次実行より速くなることを計測で確認して使います。
- 性質の異なる複数の処理を並行して実行する場合 → **`task_group`** を使用します。
- それ以外 → **逐次パイプライン**。ワーカースレッドを増やさずに SIMD を使える場合があります（[12章](12-simd.md)）。

`par_map` と `spawn` は並列処理を明示します。実際のワーカー利用は、操作と実行時のスケジューリングで決まります。
