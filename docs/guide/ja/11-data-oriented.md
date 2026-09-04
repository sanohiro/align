# データ指向設計 ― SoA とグループ集計

> 🌐 [English](../11-data-oriented.md) · **日本語**

メモリ上の配置によって、プロセッサが読み込むデータの量は変わります。この章では `soa<T>` でフィールドを列ごとに格納する方法と、キーごとに集計する方法を紹介します。

## array-of-structs と structure-of-arrays

100 万個の粒子があり、それぞれが位置と速度を持つとします。素直なレイアウトは構造体の配列(AoS)です。

```text
[ {x0,y0,vx0,vy0}, {x1,y1,vx1,vy1}, ... ]
```

もしすべての粒子の `x` 座標だけを更新したい場合、必要なデータはメモリ上で数ワードごとに散らばって存在することになります。たった1つのフィールドにアクセスするためだけに、不要なフィールドを含んだ構造体全体を CPU キャッシュに引きずり込むことになり、SIMD 命令も連続したメモリからデータをロード（ベクタロード）することができません。一方、Structure of Arrays (SoA) レイアウトでは、各フィールドをそれぞれ独立した密な配列（列データ）として格納します。

```text
x:  [x0, x1, x2, ...]
y:  [y0, y1, y2, ...]
vx: [vx0, vx1, vx2, ...]
```

この配置では、必要なフィールドの要素だけを連続して読み込めます。キャッシュを効率よく使いやすくなり、SIMD の連続ロードにも適した形になります。列指向データベースも同じ考え方を使っています。Align では `to_soa()` で構造体の配列をこの配置に変換できます。

## `soa<T>`

```align
User { active: bool, score: i64, age: i64 }

fn main() -> i32 {
    arena {
        rows := [
            User { active: true,  score: 10, age: 30 },
            User { active: false, score: 20, age: 25 },
            User { active: true,  score: 30, age: 41 },
        ]
        mut s := rows.to_soa()      // transpose into columns, in this arena

        print(s.where(.active).score.sum())    // 40 — streams 2 columns, ignores `age`
        print(s.age.max())                      // 41 — one dense column scan
        u := s[2]                               // gather a whole row back when needed
        print(u.score)                          // 30
        s[1].score = 99                         // in-place write to one column slot
        print(s.score.sum())                    // 139
    }
    return 0
}
```

コードを書く際のメンタルモデルは、依然として `User` という構造体のままです。列指向になっているのはメモリ上の物理的な配置だけです。`s.field` は特定の列を射影し（これは通常のスライスとして扱えるため、[06](06-pipelines.md) 章のパイプライン操作がすべて適用可能です）、`s[i]` は指定したインデックスのデータを集めて1つの「行（構造体）」として復元します。`s.field[a..b]` のように特定の列の一部をスライスとして切り出すことも可能です。これらの列データは `arena` 内に配置されます（そのため `to_soa` は `arena` ブロックの中で呼び出す必要があります）。[05](05-memory.md) 章で触れたように、「バッチ処理のレイアウトには、バッチと同じライフタイムを紐づける」という設計思想に基づくものです。

`s.where(.active).score.sum()` のような列の走査では、処理に使わないフィールドを読み込まずに済みます。配置を選ぶ際は、自分のデータでも計測してください。

転置そのものを省くこともできます。`json.decode` は `soa<T>` へ**直接**パースします（[08 章](08-json.md)）。読み込みながら列を構築し、エスケープのない文字列フィールドは入力を借用します。

## 列の対応を保って組み合わせる

複数の列を使う計算には `zip` を使います。同じインデックスの要素をタプルとして渡し、その組に対して同じ条件で絞り込めば、値の対応を保てます。

```align
Order { price: i64, quantity: i64, active: bool }

fn main() -> i32 {
    arena {
        rows := [
            Order { price: 10, quantity: 2, active: true },
            Order { price: 100, quantity: 3, active: false },
            Order { price: 7, quantity: 4, active: true },
        ]
        s := rows.to_soa()
        prices := s.price
        quantities := s.quantity
        active := s.active
        total := zip(prices, quantities, active)
            .where(fn row { row.2 })
            .map(fn row { row.0 * row.1 })
            .sum()
        print(total)    // 48
    }
    return 0
}
```

タプルの配列は作りません。`zip` は要素の処理を始める前に、列の長さが等しいことを確認します。最も短い列に合わせて打ち切ることはありません。ただし、長さが等しいだけでは対応関係は保証されません。ある列だけを別にソートしたり絞り込んだりすると、長さが同じでも別の行の値と組み合わさる場合があります。同じ行順を保ち、組にした値を一緒に絞り込みます。

## `group_by` ― 分析の基本操作

行データを特定のキーでグループ化し、各グループに対してリダクション（集計）を行います。以下は `soa` 上の `i64` キーに対する例です。

```align
P { k: i64, v: i64 }

arena {
    s := [
        P { k: 1, v: 10 },
        P { k: 2, v: 5 },
        P { k: 1, v: 7 },
    ].to_soa()
    g := s.group_by(.k).sum(.v)     // → (keys, sums)
    print(g.0.count())              // 2 groups
    print(g.1.sum())                // 22
}
```

`group_by(.key)` メソッドは、必ず集約操作（`.sum(.f)`、`.min(.f)`、`.max(.f)`、`.count()` のいずれか）をチェーンさせて終端させる必要があり、結果として「キーの列」と「集約された値の列」のペア（タプル）を返します（`g.0` がキー、`g.1` が集約値）。集約を伴わない単独の `group_by` はコンパイルエラーになります。これは、実体化（マテリアライズ）されていない「グループ化の中間状態」を保持することが、パフォーマンス上の隠れたコストになるのを防ぐためです。

> **コスト:** 集計項目の数が一定なら、グループ化の期待計算量は O(n) で、追加領域は最悪 O(n) です。整数キーの範囲が狭ければキーを添字として使い、それ以外はハッシュを使います。`.agg(...)` は行を1回だけ走査し、1行あたりの処理量は指定した集計項目の数に応じて増えます。

デコードされた配列上の `str` キーに対して、**1 パスで複数の集約**を行うには次のようにします。

```align
import core.json

Row { name: str, a: i64, b: i64 }

fn main() -> Result<(), Error> {
    data := "[{\"name\":\"east\",\"a\":3,\"b\":9},{\"name\":\"west\",\"a\":4,\"b\":2},{\"name\":\"east\",\"a\":5,\"b\":7}]"
    xs: array<Row> := json.decode(data)?
    g := xs.group_by(.name).agg(sum(.a), max(.b), count())
    print(g.0.count())      // 2 — east, west
    print(g.1.sum())        // 12 — the sum(.a) column: (3+5) + 4
    return Ok(())
}
```

`.agg(...)` は各キーを 1 度だけインターン化し、すべてのアキュムレータを 1 パスで畳み込みます。手書きの分析ループが取る形を、宣言から生成します。ただし、この融合形は個別の集約メソッドより受け付ける範囲が狭く、現時点では `str` キーの AoS `array<Struct>` だけを受け取ります。soa を渡すとキーの型を問わず拒否され（`fused group_by(.key).agg(...) first cut needs an AoS array<Struct> with a str key, got soa<…>`）、AoS 側でも `group_by` のキー自体が `str` である必要があります。SoA では上で示した個別の集約メソッド（`.sum(.f)`、`.min(.f)`、`.max(.f)`、`.count()`）が数値キー・`str` キーのどちらでも使えます。未実装なのは、SoA に対する複数集約の `.agg(...)` 形式だけです。

## `dict_encode` ― 変換したキーを再利用する

文字列キーを用いたグループ化は、ハッシュ計算や文字列比較のコストがかかります。もし同じ文字列キーの列に対して繰り返し集約処理を行うのであれば、一度だけ整数ベースの「辞書 ID」にエンコード（インターン化）し、それを再利用するべきです。

```align
e := xs.dict_encode(.name)              // intern the str column → dense ids
s := e.group_by(.name).sum(.a)          // these reuse the ids —
c := e.group_by(.name).count()          // no re-hashing per pass
```

この手法を辞書エンコーディングと呼びます。次からのグループ化では整数の ID を使えるため、元の文字列のハッシュ計算や比較を繰り返さずに済みます。

この断片は、直前の `Row` の例の `main` 内に続けて書きます。集計する列は `.a`、文字列のキーは `.name` です。

## 身につけたい習慣

大量のデータを処理する場合（例えば、何万ものレコードをループで回し、そのうちの1〜2個のフィールドにしかアクセスしないような処理）、データがプログラムに入ってくる最初の地点で `soa<T>` を選択し、そこから先は「列全体に対する一括操作」としてロジックを組み立ててください。判断の目安はこうです。「**あるループが、非常に多くの行データのうち、わずか1〜2個のフィールドにしか触れていないのであれば、AoS レイアウトはパフォーマンスの足を引っ張っています。**」

一方で、個別のレコード全体として扱うことが多く、大量のループ処理を行わないデータ（設定ファイルを表す構造体や、単一の Web リクエストなど）は AoS のままにしておくのが適切です。レイアウトの選択に迷ったときは、`emit-llvm` で生成されるコードを確認するか、実際のベンチマーク結果に委ねましょう。Align ではデータレイアウトの変更がたった1行のコード修正で済むため、様々なアプローチを非常に低コストで試すことができます。
