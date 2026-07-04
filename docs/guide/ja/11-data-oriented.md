# データ指向設計 ― SoA とグループ集計

> 🌐 [English](../11-data-oriented.md) · **日本語**

この章こそ、Align が存在する理由です。データをメモリ上にどう並べるかが、それをどれだけ速く処理できるかを決めます ― しばしば桁違いに。そして Align は、速いレイアウトを書き直しではなく宣言にします。

## array-of-structs と structure-of-arrays

100 万個の粒子があり、それぞれが位置と速度を持つとします。素直なレイアウトは構造体の配列(AoS)です。

```text
[ {x0,y0,vx0,vy0}, {x1,y1,vx1,vy1}, ... ]
```

すべての粒子の `x` だけを更新したいとき、必要な値は数ワードごとに散らばっています。1 つのフィールドに触れるために構造体まるごとをキャッシュに引きずり込むことになり、SIMD はレーンを連続してロードできません。structure-of-arrays(SoA)のレイアウトは、各フィールドをそれぞれ密な列として格納します。

```text
x:  [x0, x1, x2, ...]
y:  [y0, y1, y2, ...]
vx: [vx0, vx1, vx2, ...]
```

こうすると、フィールド単位のパスは密な配列を歩調をそろえて走査します ― 完璧なキャッシュ挙動、1 命令あたり 8〜16 の SIMD レーンです。これは GPU が要求し、データベースが収束した(列指向ストレージ)レイアウトです。そして多くの言語では、これを採用するにはプログラム中のあらゆる構造体を手作業でばらばらに刻む羽目になります。Align では呼び出し 1 つです。

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

考えるときは相変わらず `User` で考えます。列指向になっているのはメモリだけです。`s.field` は列(ふつうのスライスです ― [06](06-pipelines.md) 章の語彙がまるごと使えます)を射影し、`s[i]` は行を集め、`s.field[a..b]` は列を窓で切り出します。列は arena に置かれます(`to_soa` は arena の内側で呼ぶ必要があります)。[05](05-memory.md) 章のとおり、バッチのレイアウトにはバッチのライフタイムを、というわけです。

見返りは微妙などころではありません。`s.where(.active).score.sum()` のような列指向スキャンは、Rust の慣用的な AoS で同じロジックを書いた場合と比べて**8〜10 倍ほど速い**というベンチマークが出ます。賢いループのおかげではなく、ループが決して使わないバイトをレイアウトが取ってこなくなったからです。

さらに良いことに、転置そのものを丸ごと省けます。`json.decode` は `soa<T>` へ**直接**パースします([08](08-json.md) 章)。パースしながら列が埋まり、文字列の列は入力を借用します。

## `group_by` ― 分析の基本操作

行をキーで分割し、各グループをリダクションします。soa 上の `i64` キーに対して。

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

`group_by(.key)` は集約 ― `.sum(.f)`・`.min(.f)`・`.max(.f)`・`.count()` ― で完結させなければならず、列のペアを返します。`g.0` がキー、`g.1` が集約された値です。(集約のない裸の `group_by` はコンパイルエラーです。マテリアライズされていない「グループ化されたもの」は隠れたコストになるからです。)

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

`.agg(...)` は各キーを 1 度だけインターン化し、すべてのアキュムレータを 1 パスで畳み込みます。手書きの分析ループが取る形を、宣言から生成します。(第一弾は `str` キーの AoS ソースです。soa ソースの `.agg` は実装中です。)

## `dict_encode` ― キーの代金は 1 度だけ

文字列キーはハッシュと比較のコストがかかります。同じキー列を繰り返し集約するなら、1 度だけ辞書 id にエンコードしてから使い回します。

```align
e := xs.dict_encode(.name)              // intern the str column → dense ids
s := e.group_by(.name).sum(.score)      // these reuse the ids —
c := e.group_by(.name).count()          // no re-hashing per pass
```

これは列指向データベースの古典的な技(辞書エンコーディング)を、呼び出し 1 つとして表に出したものです。パスごとに生の文字列を再グループ化する場合と比べて 1.4〜4 倍速いというベンチマークが出ます。

## 身につけたい習慣

データを大量に処理するとき ― レコードを繰り返し、一度に 1〜2 フィールドずつ歩くとき ― データがプログラムに入ってくるその地点で `soa<T>` に手を伸ばし、そこからは列全体に対する操作で考えてください。目安はこうです。**あるループが多数の行の 1〜2 フィールドに触れているなら、AoS はあなたに逆らっています。** まるごと、めったに触れないデータ(設定用の構造体、1 件のリクエスト)には AoS のままにしておき、議論はすべて `emit-llvm` かベンチマークに決めさせましょう。Align ではレイアウトの変更が 1 行なので、試すのは安上がりです。
