# 3. Keep Some

> 🌐 [English](../03-keep-some.md) · **日本語**

**Q1.** `[1, 2, 3, 4, 5]` のうち、2 より大きいのはどれですか？

**A1.** `3, 4, 5`。いま `where` をしましたね。

---

**Q2.** Align では？

**A2.** `[1, 2, 3, 4, 5].where(fn x { x > 2 }).sum()` — これは `12` です。

---

**Q3.** `true` または `false` だけを返すとき、`fn x { x > 2 }` は何と呼ばれますか？

**A3.** 述語（predicate）です。`where` は、述語が認めた要素だけを残します。

---

**Q4.** `[1, 2, 3, 4, 5].where(fn x { x > 2 }).count()` は何ですか？

**A4.** `3`。生き残ったものを数えます。足すのではありません。

---

**Q5.** `[1, 2, 3].where(fn x { x > 10 }).sum()` は何ですか？

**A5.** `0` です。何も生き残りませんでした。無の和はゼロになります。エラーではありません — 空の結果は正当な答えであって、失敗ではないのです。

---

**Q6.** では、名前のついたデータを:

```align
Item { price: i64, active: bool }

items := [
    Item { price: 100, active: true },
    Item { price: 50,  active: false },
    Item { price: 200, active: true },
]
```

active な価格はどれですか?

**A6.** `100` と `200` です。

---

**Q7.** それを Align で言ってください。

**A7.** `items.where(.active).price.sum()` — これは `300` です。

---

**Q8.** Q7 には新しい仕掛けが2つあります。`where` の中の `.active` は何をしていますか？

**A8.** フィールド省略記法です。`where(.active)` は、`active` フィールドが `true` の行を残します。フィールドがすでに述語そのものなら、それ以上書くことはありません。

---

**Q9.** そして単独の `.price` ステージは何をしていますか？

**A9.** 射影 (projection) です。生き残った各 `Item` から `price` フィールドを取り出します。構造体の流れが数値の流れに変換され、足し合わせる準備が整います。

---

**Q10.** `items.price.where(fn p { p > 60 }).sum()` は何ですか？

**A10.** こちらも `300` です（`100 + 200`）。今度は価格に注目し、`active` の有無は完全に無視しています。先に射影を行ってから、後で絞り込む — このアプローチも正しいのです。ステージはあなたの意図した順番通りに動作します。

---

**Q11.** Q7 は、その3つの構造体のうちいくつをどこかにコピーしましたか？

**A11.** 1つもコピーしません。`where` は条件に合わないものをスキップし、`.price` はその行のフィールドを読み込み、`sum` がそれを足し合わせます。中間のデータ領域を作ることもなく、1回のループ処理で行われます。2章で説明した融合（fuse）と同じです。

---

**Q12.** `where` と `map` は pipeline を共有できますか？

**A12.** はい、そのために設計されています:

```align
items.where(.active).price.map(fn p { p * 108 / 100 }).sum()
```

`324` — active な価格に税をかけ、合計しました。左から右へ読むだけで、コードが何をしているかがそのままわかります。

---

**Q13.** 小さな table です。

```align
Reading { value: i64, valid: bool }

readings := [
    Reading { value: 5,  valid: true },
    Reading { value: 40, valid: false },
    Reading { value: 12, valid: true },
]
```

`readings.where(.valid).value.to_array()` は？

**A13.** `[5, 12]`。まず行を残し、次に残った field を射影します。

---

**Q14.** `readings.value.where(fn x { x > 10 }).to_array()` は？

**A14.** `[40, 12]`。この問いは `valid` を見ていません。field を射影すると、ほかの field は忘れられます。`.value` のあとは数だけが流れます。

---

**Q15.** valid で、かつ10より大きい reading だけを残してください。

**A15.**

```align
readings
    .where(.valid)
    .where(fn r { r.value > 10 })
    .value
    .to_array()
```

答えは `[12]`。まだ射影していないので、2つ目の predicate も `Reading` を受け取ります。

---

**Q16.** 2つ目の `where` より先に `.value` を射影できますか？

**A16.** valid な行を選んだあとなら可能です。

```align
readings.where(.valid).value.where(fn x { x > 10 }).to_array()
```

答えは同じです。今度の predicate は `i64` を受け取ります。考えをいちばん明瞭に言える順序を選びます。

---

**Q17.** 残った値を2倍して合計してください。

**A17.**

```align
readings
    .where(.valid)
    .value
    .where(fn x { x > 10 })
    .map(fn x { x * 2 })
    .sum()
```

`24`。

---

**Q18.** Q17 は中間 array をいくつ作りましたか？

**A18.** 0です。3つの reading が入り、1つだけが multiply に届き、1つの数が accumulator に届きます。想像上の collection ではなく、要素を追ってください。

---

**Q19.** Q17 を syntax なしで言ってください。

**A19.** 「reading を取り、valid なものを残し、その value を取り、10より大きい value を残し、2倍し、足す」。文と chain が一致しなければ、どちらかが間違っています。

---

> **第三の戒律**
>
> *絞るには `where`、指し示すには `.field`、そしてデータをその答えへと流れさせよ。*
