# 10. Count Me by Name

> 🌐 [English](../10-count-me-by-name.md) · **日本語**

**Q1.** 売上、1行に1件:

```text
east 3
west 4
east 5
```

*地域ごと* の合計は?

**A1.** east `8`、west `4`。グループ分けして、それから足しました。誰にでもできます — 問題は、手作業でハッシュマップを書かずに、この処理をどう表現するかです。

---

**Q2.** Align で言ってください。`k`(地域 id)と `v` を持つ soa に対して:

**A2.**

```align
g := s.group_by(.k).sum(.v)
```

一つのフィールドでグループ分けし、別のフィールドを畳み込む。この1行がデータ分析の要(かなめ)です。

---

**Q3.** 返ってくるのは何ですか？

**A3.** 2つの列がペアで返ってきます: `g.0` は重複のないキー、`g.1` は各キーの合計 — 一方の行 `i` が他方の行 `i` に対応します。列を入力し、列が出力される。9章の考え方は、ここでもそのまま当てはまります。

---

**Q4.** 上の売上で、`g.0.count()` と `g.1.sum()` は何ですか？

**A4.** `2`（east、west）と `12`（全売上 — グループ分けは同じ値を順序を並べ替えて足し合わせただけです）。

---

**Q5.** `sum` のほかに、`group_by` の後には何が続けられますか？

**A5.** `min(.f)`、`max(.f)`、`count()` — 地域ごとの最大の売上、地域ごとの売上件数。（`count` はフィールドを取りません。数えるのには必要ないからです。）

---

**Q6.** `group_by` は単独で立てますか — いまグループ分けして、あとで集計？

**A6.** いいえ — 5章と同じ掟です。裸の `group_by` は未完成の文(断片化された見えないテーブル)です。何を畳み込むかまで、ひと息に書き切ってください。

---

**Q7.** キーが *名前* で、一度に3つの問い — sum、max、count。3回のパス？

**A7.** 1回:

```align
g := xs.group_by(.name).agg(sum(.a), max(.b), count())
```

`agg` はキーごとに3つすべてを1回のパスで畳み込みます: `g.0` は名前、`g.1` は sum、続いて max、count。メモリを1回走査する間に、すべてのアキュムレータが同時に相乗りして計算されます。

---

**Q8.** なぜ1回のパスがそれほど大事なのですか？

**A8.** 100万行ともなれば、メモリを端から端まで走査すること自体が大きなコストだからです。3回のパスはテーブルを3度読みます。これは5章の「融合」と同じ教えを、データ分析の規模で語ったものです。

---

**Q9.** `name` — string — でグループ分けを5回、別々にします。目に見えないところで、何がコストになりますか？

**A9.** 同じ string をハッシュし比較すること、それを5回繰り返すことになります。

---

**Q10.** 治療法は？

**A10.** コストは一度だけ払います:

```align
e := xs.dict_encode(.name)          // intern the names → small ids
s := e.group_by(.name).sum(.score)  // these ride the ids —
c := e.group_by(.name).count()      //   no re-hashing
```

辞書エンコーディング — 列指向データベースの最も古典的な定石（テクニック）を、明示的な呼び出し1つで実現できます。

---

**Q11.** 列指向データベースが、私たちの答えに何度も現れます。偶然ですか？

**A11.** まったく偶然ではありません。横倒しレイアウト(9章)、グループ畳み込み(10章)、辞書エンコーディング — 分析エンジンがこれらに収束したのは、ハードウェアの特性がそれを求めているからです。Alignのアプローチは、これらを「言語」の機能として組み込むことです。だから普通のコードを書くだけで、分析エンジンが行き着いたのと同じ境地に到達できるのです。

---

**Q12.** ドリル。`{"name":..., "a":..., "b":...}` の行からなる JSON 文字列から、相異なる名前と、名前ごとの最大の `b` を — 1回のパスで。

**A12.**

```align
xs: array<Row> := json.decode(data)?
g := xs.group_by(.name).agg(max(.b), count())
print(g.0.count())      // how many names
print(g.1.max())        // the largest of the per-name maxima
```

(decode、group、fold — テキストから答えまで3行。`count()` はついでにコストゼロで計算されました。)

---

**Q13.** 「1パス」は「追加 memory なし」という意味？

**A13.** いいえ。grouping には accumulator の table と結果 column が必要で、おおよそ distinct key ごとに一 entry です。`agg` は table と入力の旅を共有しますが、group を消しはしません。

---

**Q14.** すべての row が別の key なら？

**A14.** grouped result は入力とほぼ同じ大きさになります。操作は直接的で目に見えますが、無料ではありません。「row はいくつか」と「group はいくつか」の2つを必ず問います。

---

**Q15.** string grouping の前には毎回 `dict_encode` するべき？

**A15.** いいえ。一度の string `group_by` は、その集計に必要な intern をすでに行います。encoded column を複数の grouping や比較で再利用するときに `dict_encode` は価値を持ちます。

---

**Q16.** customer ごとの order です。

```text
7  10
9   4
7   3
9   8
7   2
```

customer ごとの sum は？

**A16.** customer `7 → 15`、customer `9 → 12`。syntax に触る前に頭の中で group します。

---

**Q17.** field `.customer` と `.amount` で書いてください。

**A17.**

```align
g := orders.group_by(.customer).sum(.amount)
```

`g.0` が customer、`g.1` が対応する sum です。

---

**Q18.** customer ごとの最大 order と order count も必要です。grouping は3回？

**A18.** 一度です。

```align
g := orders.group_by(.customer).agg(
    sum(.amount),
    max(.amount),
    count(),
)
```

一つの key table に、group ごと3つの accumulator。

---

**Q19.** customer ごとの sum である `g.1` をさらに sum すると？

**A19.** `27`。入力の全 amount の sum と同じです。grouping は結びつきを変えますが、寄与の総量は変えません。

---

**Q20.** customer ごとの max を、さらに max すると？

**A20.** `10`。入力全体で最大の order です。grouped reduction のあとにも reduction を続けられます。いま何の column が流れているかを追ってください。

---

**Q21.** 5つの report が customer name を再利用し、それぞれ別の値を group 化します。optimization の形は？

**A21.**

```align
encoded := orders.dict_encode(.customer)
sales := encoded.group_by(.customer).sum(.amount)
counts := encoded.group_by(.customer).count()
```

繰り返す key column を一度 encode し、そのあと複数の grouped question を尋ねます。再利用するものは半端な `group_by` ではなく、明示的に encode された data です。

---

> **第十の戒律**
>
> *グループ分けと畳み込みは、ひと息で。問いはすべて1回のパスで尋ね、string のキーには一度だけコストを払え。*
