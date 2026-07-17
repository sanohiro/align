# パイプライン ― データ処理の中核

> 🌐 [English](../06-pipelines.md) · **日本語**

ここが Align の心臓部です。手動でループを書く代わりに、コレクションに対する変換処理をステージごとに記述することで、コンパイラが自動的にループを生成します。生成されるループは単一のパスに融合（fuse）され、分岐を最小限に抑えた、ベクトル化可能な形式になります。この章では、パイプライン処理で使用できる操作をひと通り紹介します。

## 全体の形

```align
total := prices.map(with_tax).where(in_stock).sum()
```

この処理は左から右へ読み下せます。`prices` を受け取り、それぞれを変換し、条件に合うものだけを残し、最後に1つの値へと集計する、という流れです。ここで重要なのは、**中間配列が一切生成されない**ことです。`map`、`where`、`sum` の各処理は、1つのループとして融合され、中間値のやり取りは CPU レジスタ上で完結します。これはコンパイラが自動的に行うストリーム融合（Stream Fusion）であり、だからこそ「素直に書いたコード」がそのまま「高速に動作するコード」になるのです。

パイプラインは必ず**終端しなければなりません**。終端とはリダクション（`sum`、`count`、`reduce` など）か、実体化（`to_array`、`map_into`）のことです。終端を持たないパイプライン（例：`xs.map(f)` のまま放置すること）はコンパイルエラーになります。遅延評価される状態をそのまま持ち回すと、どこで計算コストが発生するかが隠れてしまうためです。

> **Cost:** 融合されたリダクションは O(n) で、中間コレクションを確保しません。`to_array` は結果領域を最大1回確保し(`where` がある場合はsource長を上限にします)、`map_into` は呼び出し側の領域へ書くので確保しません。

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
    zip(a, b, c)
        .map(fn v { v.0 + v.1 * v.2 })
        .map_into(dst)
}
```

`zip` はタプルの配列を生成するのではなく、遅延評価されるパイプラインの起点（head）として機能します。反復処理を開始する前にすべての入力元の長さが検査され、各インデックスにおける `v` は SSA（静的単一代入）タプルとして扱われます。入力元同士がエイリアス（同じメモリ領域を指すこと）を持つのは構いませんが、`map_into` の出力先はすべての入力元と重複しない（オーバーラップしない）領域でなければなりません。

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

> **Cost:** どちらのsortもstableで、最悪計算量は O(n log n)、所有する結果をmaterializeします。追加の作業領域は最悪 O(n) です。`sort_by_key` はkey関数を入力順に各要素ちょうど1回評価します。これらの保証を保つ限り、内部のmerge方式は変更されることがあります。

## チャンク分割（一定サイズの切り出し）

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

## 実体化（マテリアライズ） ― `to_array` と `map_into`

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

従来のスカラー言語で手書きしたコードは、処理のステップごとにメモリを何度も走査し、その間に一時的な領域を確保しがちです。一方、融合されたパイプラインはメモリを1度しか走査せず、動的な確保も一切行いません。コンパイラは、エイリアスのないクリーンなループと、実行頻度の低い（cold な）エラーパスを LLVM に渡します。これはまさに自動ベクトライザが好む形式です。Align では「分かりやすさ」と「速さ」がトレードオフの関係になりません。「明快に書かれたコードこそが最も速い」というのが Align の設計思想であり、その結果は `emit-llvm` を使っていつでも検証できます。

## 語彙で本当に表現できないとき

反復回数が実行中の状態によって決まるような逐次的な制御（例えば、ストリームを EOF まで読み込む、バックオフしながらリトライする、ステートマシンを駆動するなど）には `loop` 式の出番です（[02](02-language-basics.md) 章）。また、大量のデータをグループ化して集計する場合は `group_by` を使用します（[11](11-data-oriented.md) 章）。この2つとパイプラインを組み合わせることで、`for` ループを使いたくなる場面のほぼすべてをカバーできます。もし `loop` の中でインデックスを使って配列を走査しようとしている自分に気づいたら、いったん立ち止まり、「自分が行いたい変換の本質は何か」を問い直してください。多くの場合、その答えはこの章で紹介したパイプラインのいずれかになります。
