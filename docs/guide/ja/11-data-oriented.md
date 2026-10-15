# データ指向設計 ― SoA とグループ集計

> 🌐 [English](../11-data-oriented.md) · **日本語**

メモリ上の配置によって、プロセッサが読み込むデータの量は変わります。この章では `soa<T>` でフィールドを列ごとに格納する方法と、キーごとに集計する方法を紹介します。

## array-of-structs と structure-of-arrays

位置と速度を持つ粒子を考えます。構造体の配列（AoS）では、粒子ごとのフィールドをまとめて並べます。

```text
[ {x0,y0,vx0,vy0}, {x1,y1,vx1,vy1}, ... ]
```

`x` だけを走査すると、他のフィールドを飛び越えて読み進めるため、使わないバイトまで取得することがあります。Structure-of-arrays（SoA）は、フィールドごとに連続した列へ配置します。

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

`soa<User>` はレコード型を保ち、フィールドを別々の列に格納します。`s.field` は列を通常のスライスとして取り出し（[06 章](06-pipelines.md)）、`s[i]` は行を集め、`s.field[a..b]` は列の一部を参照します。`to_soa` は arena 内で呼びます。列のストレージはその arena に属し、寿命も制限されます（[05 章](05-memory.md)）。

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

多数のレコードの一部のフィールドを繰り返し走査するなら、`soa<T>` を検討します。レコード全体を使う処理には AoS が適する場合があります。変換のコストと、その後の走査回数を含めて比較し、代表的な入力を使ったベンチマークで選びます。`emit-llvm` は生成されたアクセス方法を調べる手がかりになります。
