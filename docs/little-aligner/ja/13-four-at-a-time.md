# 13. Four at a Time

> 🌐 [English](../13-four-at-a-time.md) · **日本語**

**Q1.** 2つの配列 `[1, 2, 3, 4]` と `[10, 20, 30, 40]` があります。同じ位置の要素同士を足してください。

**A1.** `zip(a, b).map(fn v { v.0 + v.1 }).to_array()`。`zip` はパイプラインのソースで、2つの配列を同じ歩調で進みながら、各ステージにペアを1つずつ手渡します。

---

**Q2.** CPUは何回の足し算を実行しますか？

**A2.** 4回です。各ペアに対して1回ずつ計算します。

---

**Q3.** それを1回で終わらせることはできますか？

**A3.** はい。CPUが幅の広いレジスタを持っていれば可能です。4つの `i32` を1つの `vec4<i32>` に詰め込み、たった1回の `+` で足し合わせることができます。

---

**Q4.** どう書けばいいですか？

**A4.** 
```align
v1: vec4<i32> := [1, 2, 3, 4]
v2: vec4<i32> := [10, 0, 30, 40]
v3 := v1 + v2
```
型注釈がリテラルをベクトルにします — 専用のコンストラクタは存在しません。これで `v3` は `[11, 2, 33, 44]` を保持します。1つの命令で、4つの結果が出ます。（`v2` にはわざとゼロを1つ混ぜてあります — このあと使うので。）

---

**Q5.** では `v2` で割り算したいとします。ただし、分母がゼロでない場合だけ。

**A5.** 通常のコードなら `if` を使います。しかし、ベクトルは分岐しません — 常にすべてを同時に計算します。ここでは **マスク（mask）** を使います。

---

**Q6.** マスクとは何ですか？

**A6.** レーンごとの真偽値のベクトルです。`m := v2 != 0` と書くと、ブロードキャストされたスカラー `0` と全レーンが一度に比較され、`v2` が `0` の2番目のレーンだけ false、それ以外は true の `mask4<i32>` が得られます。マスクは比較からしか生まれません — 手で書くことはないのです。

---

**Q7.** ゼロ除算を避けるために、それをどう使いますか？

**A7.** `select` を使います。
```align
ones: vec4<i32> := [1, 1, 1, 1]
safe_v2 := select(m, v2, ones)
ans := v1 / safe_v2
```
マスクが true のレーンは `v2` から、false のレーンは `ones` から選ばれます — つまり `safe_v2` は `[10, 1, 30, 40]`、`ans` は `[0, 2, 0, 0]` になります。2番目のレーンは `v2` の `0` でトラップする代わりに、置き換えられた `1` で割られました。これで割り算がトラップすることはなく、CPUが分岐処理で立ち止まることもありません。

---

**Q8.** 大きな配列に対して、いちいち手作業でこれを書くのですか？

**A8.** いいえ。だからこそ Align はパイプラインを自動ベクトル化（auto-vectorize）するのです。`.map()` を使えば、コンパイラが裏で `vec` と `mask` を構築してくれます。

---

**Q9.** では、なぜわざわざ `vec` と `mask` を学ぶのですか？

**A9.** 自動ベクトル化はヒューリスティクス（推論）であって、絶対の保証ではないからです。暗号アルゴリズム、独自のハッシュ関数、あるいは新しい圧縮方式を書くとき、あなたはシリコンと直接対話しなければなりません。Align はハードウェアの姿を隠蔽しません。

---

**Q10.** `v1` が vector のとき、`v1 * 2` は？

**A10.** scalar `2` が全 lane に broadcast されます。`[2, 2, 2, 2]` と書かなくても、一つの vector 演算が4つを multiply します。

---

**Q11.** `a.max(b)` と `a.max()` の違いは？

**A11.** argument ありなら lane-wise で vector を返します。argument なしなら lane 全体を reduce し、一つの scalar を返します。punctuation は小さくても、答えの形は違います。

---

**Q12.** literal ではなく、本物の array から4つを register へ入れるには？

**A12.** slice を使います。

```align
v: vec4<i32> := xs.load(i)
dst.store(i, v * 2)
```

`load` が連続4要素を読み、`store` が4要素を書きます。どちらも bounds-checked。register は値ですが、memory との境界は明示的です。

---

**Q13.** slice は10要素です。`i == 8` で `vec4` を load できますか？

**A13.** いいえ。要素8から11を求め、bounds check で abort します。4つの group を2回処理し、最後の2要素は scalar tail にします。SIMD でも data の外を読む権利はありません。

---

**Q14.** mask だけで危険な演算は安全になりますか？

```align
select(v2 != 0, v1 / v2, v1 * 0)
```

**A14.** いいえ。division は `select` の argument なので、blend より先に zero lane も計算されます。Q7 のように先に安全な分母を select し、それから divide します。mask は結果を選びます。すでに頼んだ仕事は消しません。

---

**Q15.** 自分で `vec4` を選ぶ価格は？

**A15.** fixed width が kernel の一部になります。別 target の最適幅は違うかもしれず、tail も自分の責任です。explicit SIMD は小さな slice-shaped function の中に保ち、普通の caller と loop は width-agnostic な pipeline のままにします。

---

**Q16.** lane ごとに計算してください。

```align
v: vec4<i32> := [1, 2, 3, 4]
w := v * 3 + 1
```

**A16.** `[4, 7, 10, 13]`。2つの scalar が broadcast され、各 lane で multiply が addition より先です。

---

**Q17.** `m := w > 8`。true なのはどの lane で、`select(m, w, 0)` は？

**A17.** 後ろ2 lane が true。結果は `[0, 0, 10, 13]` です。false 側では scalar `0` が broadcast されます。

---

**Q18.** 選ばれた lane だけを reduce すると？

**A18.** `w.sum_where(m)` は `23`。masked reduction は、小さな vector を store せずに lane を collapse します。

---

**Q19.** 10要素の slice を `vec4` で重ならずに歩きます。安全な full-width 開始 index は？

**A19.** `0` と `4`。index `8` は scalar tail のものです。load を書く前に index を言ってください。off-by-one error も SIMD なら速くなるだけです。

---

**Q20.** pipeline か explicit vector か選んでください。

- 普通の整数1000万個に1を足す
- hash design が指定する固定4 lane の mixing round
- 一つの SoA column を sum する

**A20.** pipeline、explicit vector、pipeline。width-agnostic から始め、algorithm 自体が lane で語る場所だけに fixed-width syntax を使います。

---

**Q21.** program の残りは hand-vectorized kernel をどう呼ぶべき？

**A21.** 普通の slice と値を通して呼びます。`vec4` は小さな関数の中に閉じ、caller は一つの machine の register plan ではなく data を記述します。escape hatch は入口が狭いほど健全です。

---

> **第十三の戒律**
>
> *大量のデータはパイプラインの自動ベクトル化に任せよ。だがシリコンの限界を引き出すときは、ベクトルとマスクの言葉で語れ。決して分岐させるな。*
