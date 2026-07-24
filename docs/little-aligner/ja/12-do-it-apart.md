# 12. Do It Apart

> 🌐 [English](../12-do-it-apart.md) · **日本語**

**Q1.** 多数の値に一つの処理を行う方法は知っています。

```align
ys := xs.map(expensive).to_array()
```

同時に処理してよい値なら？

**A1.**

```align
ys := xs.par_map(expensive)
```

`par_map` は並列性を声に出して言います。

---

**Q2.** `par_map` に `.to_array()` は必要？

**A2.** いいえ。自分で owned result array を materialize します。worker は答えを置く場所を必要とし、目に見える並列境界が materialization の境界でもあります。

---

**Q3.** Pure な関数なら、`map(f).to_array()` と `par_map(f)` の答えは違ってよい？

**A3.** いいえ。同じ入力位置、同じ出力位置、同じ値です。parallelism が変えるのは work の schedule であり、program の意味ではありません。

---

**Q4.** `[3, 1, 2].par_map(fn x { x * x })` は？

**A4.** `[9, 1, 4]`。worker は別の順序で終わっても、結果は入力順へ戻ります。completion order は data order ではありません。

---

**Q5.** callable は各値を print してよい？

**A5.** いいえ。どの worker が先に表示するのでしょう？ 観測できる答えが scheduling に依存してしまいます。`par_map` は Pure callable を要求します。I/O も外部状態の mutation も不可です。

---

**Q6.** `pure fn` と書く必要がありますか？

**A6.** 一度もありません。compiler が関数の行動から purity を推論します。I/O、rng、FFI、`unsafe`、外部 mutation へ届く call があれば、何が Impure にしたかを示せます。

---

**Q7.** parallel lambda は外側の `factor` を使えますか？

```align
factor := 3
ys := xs.par_map(fn x { x * factor })
```

**A7.** はい。closure は `factor` を値で capture します。各 task が受け取るのは事実であり、別 task が変えられる共有 box ではありません。

---

**Q8.** `factor` が `mut` binding で、あとから変わるなら？

**A8.** closure は作られたときの値を持ち続けます。第2章の小さな規則が並列 safety の規則になりました。共有された mutable environment はありません。

---

**Q9.** 100万個の整数で `f(x) = x + 1`。`par_map`？

**A9.** たいてい違います。逐次 `map` は fuse・vectorize され、worker dispatch と結果 materialization を避けます。要素が多いだけでは、仕事は高価になりません。

---

**Q10.** 独立した1000枚の image に、それぞれ高価な transform。`par_map`？

**A10.** 計測も同意するなら、よい候補です。parallelism には入場料があり、高価な Pure work はそれを返す時間を持ちます。

---

**Q11.** 6つの数を、2つずつの独立した3 hand で sum します。

```align
fn chunk_sum(c: slice<i64>) -> i64 = c.sum()
```

式を完成してください。

**A11.**

```align
total := [1, 2, 3, 4, 5, 6]
    .chunks(2)
    .par_map(chunk_sum)
    .sum()
```

partial answer は `3`、`7`、`11`。最後は `21`。

---

**Q12.** 一つの数ごとに task を作らず、chunk にするのはなぜ？

**A12.** task は scheduling の費用を返せるだけの仕事を必要とします。chunk は近い data もまとめます。grain size は algorithm の多数の要素と machine の少数の worker を結ぶ橋です。

---

**Q13.** chunk が3番目、1番目、2番目の順に終わると、最後の sum は変わりますか？

**A13.** この整数 sum では変わりません。`par_map` は結果順を戻し、最後の reduction は逐次です。一般にも、隠れた completion order を入力にしてはいけません。順序が重要なら data に持たせます。

---

**Q14.** job が異なります。profile の fetch、model の load、configuration の read。`par_map`？

**A14.** 共通の element function を map していません。少数の heterogeneous task なので `task_group` です。

---

**Q15.** 何を表示しますか？

```align
base := 100
task_group {
    a := spawn(fn { base + 5 })
    b := spawn(fn { base * 2 })
    wait()
    print(a.get() + b.get())
}
```

**A15.** `305`。task はどちらの順でも動けます。`wait()` が両方を join し、`.get()` が完了した値を読みます。

---

**Q16.** task は `task_group` のあとも走り続けられますか？

**A16.** いいえ。block が lifetime です。detached task も join 忘れも source の形から逃げられません。

---

**Q17.** 一つの task が `Err` を返します。`wait()?` は残りを放置する？

**A17.** いいえ。すべてを join してから、最初の failure を普通の `?` の扉へ渡します。structured concurrency では cleanup と failure が同じ境界で一致します。

---

**Q18.** 選んでください。

- slice 上の安い arithmetic
- 多数の独立 item に同じ高価な Pure function
- 3つの異なる独立 operation

**A18.** 逐次 pipeline、`par_map`、`task_group`。SIMD lane、data-parallel worker、task-parallel worker。3つの scale に、3つの見える選択があります。

---

**Q19.** Align で「別 thread がこれを実行する」を意味する言葉は？

**A19.** `par_map` と `spawn`。それだけです。そのどちらも指させないなら、program は密かに parallel work を作っていません。

---

> **第十二の戒律**
>
> *高価で Pure な仕事は `par_map` へ、異なる独立した仕事は `task_group` へ渡せ。すべての task に scope を、すべての failure に join を与えよ。*
