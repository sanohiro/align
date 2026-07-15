# パイプライン ― データ処理の中核

> 🌐 [English](../06-pipelines.md) · **日本語**

ここが Align の心臓部です。ループを書く代わりに、コレクションに対する変換をステージごとに記述すれば、ループはコンパイラが生成します。しかも単一パスに融合され、分岐は最小、ベクトル化も可能な形で。この章では、その語彙をひととおり紹介します。

## 全体の形

```align
total := prices.map(with_tax).where(in_stock).sum()
```

左から右へ読んでください。`prices` を取り、それぞれを変換し、一部だけ残し、最後に 1 つの値へまとめる、という流れです。ここで重要なのは、**中間配列がいっさい生成されない**ことです。`map`・`where`・`sum` は 1 本のカウント付きループに融合され、中間値はレジスタ上で完結します。これはあなたではなくコンパイラが行うストリーム融合であり、だからこそ「素直に書いたコード」がそのまま「速いコード」になります。

パイプラインは必ず**終端しなければなりません**。終端とはリダクション(`sum`・`count`・`reduce` など)か、マテリアライズ(`to_array`・`map_into`)のことです。終端のない `xs.map(f)` を宙ぶらりんのまま残すとコンパイルエラーになります。持ち回せる遅延値は、隠れたコストになってしまうからです。

## 変換ステージ

```align
xs.map(f)          // transform each element
xs.where(p)        // keep elements where p holds
xs.where(.active)  // field shorthand: keep rows whose bool field is true
xs.price           // field projection: array<Item> → the price of each
xs.scan(0, add)    // running accumulation — a stage, not a terminal
```

ステージに渡すのは名前付き関数か、インラインのラムダです。ラムダは `fn x { x * 2 }` のように、パラメータを波括弧の前に書きます。(ラムダは周囲の値もキャプチャできます。詳しくは [10](10-closures-and-parallelism.md) 章で扱います。)

## `zip` で複数 source を読む

同じ index の複数配列/スライスから一つの結果を作る場合は `zip` を使います。

```align
fn combine(a: slice<f32>, b: slice<f32>, c: slice<f32>, out dst: slice<f32>) {
    zip(a, b, c).map(fn v { v.0 + v.1 * v.2 }).map_into(dst)
}
```

`zip` は tuple 配列ではなく遅延 pipeline head です。全 source の長さを反復前に検査し、`v` は
各 index だけに存在する SSA tuple です。v1 は2個以上の Copy primitive-scalar source を受けます。
source 同士は alias してもよい一方、`map_into` の destination は全 source と非重複でなければなりません。

## リダクション終端

```align
xs.sum()                              // add everything
xs.count()                            // how many survived the stages
xs.min()   /  xs.max()                // extrema
xs.any(p)  /  xs.all(p)               // bool: does any / do all satisfy p
xs.reduce(init, f)                    // the general fold — init FIRST, then fn acc, x
```

```align
fn main() -> i32 {
    xs := [1, 2, 3, 4]
    print(xs.reduce(1, fn acc, x { acc * x }))       // 24 — product
    print(xs.scan(0, fn acc, x { acc + x }).max())   // 10 — max prefix sum
    print(xs.map(fn x { x * x }).sum())              // 30
    return 0
}
```

## 並べ替えと分割

```align
fn main() -> i32 {
    xs := [10, 21, 32, 3]
    sorted := xs.sort_by_key(fn x { -x })            // descending: negate the key
    print(sorted[0])                                 // 32

    (evens, odds) := [1, 2, 3, 4, 5].partition(fn x { x % 2 == 0 })
    print(evens.count())                             // 2
    print(odds.sum())                                // 9
    return 0
}
```

`sort()` は昇順にソートし、`sort_by_key(f)` は計算したキーでソートします。`partition(p)` は 1 パスで 2 つの所有配列に分割します。条件を満たす要素、続いてそれ以外です。

## チャンク分割

`chunks(n)` は連続する窓をスライスとして順に取り出します(最後だけ短くなることがあります)。バッチ処理の典型的な形です。

```align
fn per_chunk(xs: slice<i64>) -> i64 = xs.sum()

fn main() -> i32 {
    xs := [1, 2, 3, 4, 5]
    sums := xs.chunks(2).map(per_chunk).to_array()   // [3, 7, 5]
    print(sums.sum())                                // 15
    return 0
}
```

## マテリアライズ ― `to_array` と `map_into`

ほとんどのパイプラインはリダクションで終わり、いっさいメモリ確保をしません。それでも変換後のコレクションそのものが欲しいときは、その意図を明示します。

```align
big := xs.map(fn x { x * 10 }).where(fn x { x > 20 }).to_array()   // owned array<i64>
```

書き込み先がすでに存在する場合は、そこへ直接書き込みます。メモリ確保はゼロで、しかもコンパイラが「入力元と書き込み先がエイリアスしない」ことを証明します。

```align
fn dbl(x: i64) -> i64 = x * 2

fn scale(src: slice<i64>, out dst: slice<i64>) {
    src.map(dbl).map_into(dst)      // lengths must match; checked
}

fn main() -> i32 {
    xs := [1, 2, 3, 4]
    mut ys := [0, 0, 0, 0]
    mut d: slice<i64> := ys
    scale(xs, d)
    print(ys.sum())                 // 20
    return 0
}
```

パラメータに付いた `out` マーカーに注目してください。スライス越しに書き込む関数は、そのことをシグネチャで宣言します。ミューテーションも含め、何ひとつ隠しません。

## 実例で見る

構造体の配列に対して、在庫のある商品の税込価格を合計してみましょう。

```align
Item { price: f64, active: bool }

fn with_tax(p: f64) -> f64 = p * 1.08

fn main() -> i32 {
    items := [
        Item { price: 100.0, active: true },
        Item { price: 50.0,  active: false },
        Item { price: 200.0, active: true },
    ]
    total := items.where(.active).price.map(with_tax).sum()
    print(total)                    // 324.0
    return 0
}
```

ループは 1 本です。`where(.active)` は条件を満たさない要素で次の反復へスキップする分岐に、`.price` はフィールドのロードに、`with_tax` はインライン化に、`sum` はレジスタ上での累積になります。自分で確かめてみてください。このプログラムに `alignc emit-llvm` をかけると、融合された単一のループが見えます。`-O2` ではベクトル化されたループになります。

## なぜ速いのか(そして、これを出し抜く抜け道がなぜ無いのか)

スカラー言語で手書きした版は、ステップごとにメモリを 1 度ずつ走査し、その合間に一時領域を確保します。融合されたパイプラインは 1 度だけ走査し、いっさい確保せず、エイリアスのないクリーンなカウント付きループと、cold な(まれにしか通らない)エラーパスを LLVM に渡します。これはまさに自動ベクトライザが対象とするために作られた形です。ここでは明快さと速さを引き換えにしていません。「明快な版こそが速い版である」というのがこの設計の主張であり、`emit-llvm` を使えばその主張はいつでも監査できます。

## 語彙で本当に表現できないとき

反復回数が実行そのものによって決まる逐次的な制御 ― ストリームを EOF までポンプする、バックオフしながらリトライする、ステートマシンを駆動する ― は `loop` 式の出番です([02](02-language-basics.md) 章)。大量データのグループ集計は `group_by` です([11](11-data-oriented.md) 章)。この 2 つで、`for` に手が伸びそうになる場面のほぼすべてをカバーできます。`loop` の中でインデックスを走査しようとしている自分に気づいたら、いったん立ち止まり、「この変換は本当は何なのか」を問い直してください。答えはたいてい、この章でまだ見ていないパイプラインです。
