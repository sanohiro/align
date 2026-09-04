# 3. Keep Some

> 🌐 [English](../03-keep-some.md) · **日本語**

**Q1.** `[1, 2, 3, 4, 5]` のうち、2 より大きいのはどれですか？

**A1.** `3, 4, 5`。いま `where` をしましたね。

---

**Q2.** Align では？

**A2.** `[1, 2, 3, 4, 5].where(fn x { x > 2 }).sum()` — これは `12` です。

---

**Q3.** `true` または `false` だけを返すとき、`fn x { x > 2 }` は何と呼ばれますか？

**A3.** 述語（predicate）です。`where` は、この関数が `true` を返す要素だけを残します。

---

**Q4.** `[1, 2, 3, 4, 5].where(fn x { x > 2 }).count()` は何ですか？

**A4.** `3`。条件に合う要素の個数です。

---

**Q5.** `[1, 2, 3].where(fn x { x > 10 }).sum()` は何ですか？

**A5.** `0` です。条件に合う要素がなくても、合計は求められます。

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

`active` が `true` の商品の価格はどれですか？

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

**A10.** こちらも `300`（`100 + 200`）です。ただし、今度は価格だけで絞り込み、`active` は見ていません。このデータでは同じ答えになりますが、条件は異なります。

---

**Q11.** Q7 は、中間結果として `Item` をコピーしますか？

**A11.** しません。`where` は条件に合わない行を飛ばし、`.price` は各行の価格を読み、`sum` が足します。中間配列を作らずに1つのループで処理します。2章で見た融合と同じです。

---

**Q12.** `where` と `map` はパイプラインを共有できますか？

**A12.** はい、そのために設計されています:

```align
items.where(.active).price.map(fn p { p * 108 / 100 }).sum()
```

`324`。`active` が `true` の商品の価格に税をかけ、合計しました。

---

**Q13.** 小さなテーブルです。

```align
Reading { value: i64, valid: bool }

readings := [
    Reading { value: 5,  valid: true },
    Reading { value: 40, valid: false },
    Reading { value: 12, valid: true },
]
```

`readings.where(.valid).value.to_array()` は？

**A13.** `[5, 12]`。`valid` が `true` の行を残し、その `value` を取り出します。

---

**Q14.** `readings.value.where(fn x { x > 10 }).to_array()` は？

**A14.** `[40, 12]`。この式は `valid` を見ていません。`.value` の後に流れるのは数値だけなので、ほかのフィールドは参照できません。

---

**Q15.** `valid` が `true` で、`value` が10より大きい測定値だけを残してください。

**A15.**

```align
readings
    .where(.valid)
    .where(fn r { r.value > 10 })
    .value
    .to_array()
```

答えは `[12]`。まだ射影していないので、2つ目の述語も `Reading` を受け取ります。

---

**Q16.** 2つ目の `where` より先に `.value` を射影できますか？

**A16.** `valid` が `true` の行を選んだあとなら可能です。

```align
readings.where(.valid).value.where(fn x { x > 10 }).to_array()
```

答えは同じです。今度の述語は `i64` を受け取ります。条件をどのデータに適用するのかが伝わる順序を選んでください。

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

**Q18.** Q17 は中間配列をいくつ作りましたか？

**A18.** 0です。3つの測定値のうち、両方の条件を満たす `12` だけが2倍され、合計に加わります。

---

**Q19.** Q17 の処理を、日本語で説明してください。

**A19.** 「有効な測定値から値を取り出し、10より大きいものを2倍して足す」。式と見比べて、各操作がどの部分に対応するか確かめてください。

---

> **第三の戒律**
>
> *条件に合う要素を残すには `where`、フィールドを取り出すには `.field` を使え。*
