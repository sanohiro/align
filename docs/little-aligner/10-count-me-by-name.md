# 10. Count Me by Name

> 🌐 **English** · [Japanese](./ja/10-count-me-by-name.md)

**Q1.** Sales, one row per sale:

```text
east 3
west 4
east 5
```

Total *per region*?

**A1.** east `8`, west `4`. You grouped, then you summed. Everyone can — the question is saying it without writing a hash map by hand.

---

**Q2.** Say it in Align, over a soa with `k` (region id) and `v`:

**A2.**

```align
g := s.group_by(.k).sum(.v)
```

Group by one field, fold another. That single line is the analytics workhorse.

---

**Q3.** What comes back?

**A3.** Two columns, as a pair: `g.0` the distinct keys, `g.1` each key's total — row `i` of one matches row `i` of the other. Columns in, columns out; chapter 9 never stopped applying.

---

**Q4.** With the sales above, what are `g.0.count()` and `g.1.sum()`?

**A4.** `2` (east, west) and `12` (all sales — grouping only rearranged the same numbers).

---

**Q5.** Besides `sum`, what may follow a `group_by`?

**A5.** `min(.f)`, `max(.f)`, `count()` — biggest sale per region, sales count per region. (`count` takes no field; counting needs none.)

---

**Q6.** May `group_by` stand alone — grouping now, aggregating later?

**A6.** No — same law as chapter 5: a bare `group_by` is an unfinished sentence (a hidden table of pieces). Say what to fold, in the same breath.

---

**Q7.** Keys that are *names*, and three questions at once — sum, max, count. Three passes?

**A7.** One:

```align
g := xs.group_by(.name).agg(sum(.a), max(.b), count())
```

`agg` folds all three per key in a single pass: `g.0` the names, `g.1` the sums, then the maxes, the counts. One trip through memory, every accumulator riding along.

---

**Q8.** Why does one pass matter so much?

**A8.** Because at a million rows, the trip through memory *is* the cost. Three passes read the table thrice. This is the same lesson as fusion in chapter 5, at the analytics scale.

---

**Q9.** We group by `name` — strings — five different times. What is silently expensive?

**A9.** Hashing and comparing the same strings, five times over.

---

**Q10.** The cure?

**A10.** Pay once:

```align
e := xs.dict_encode(.name)          // intern the names → small ids
s := e.group_by(.name).sum(.score)  // these ride the ids —
c := e.group_by(.name).count()      //   no re-hashing
```

Dictionary encoding — the columnar database's oldest trick, as one visible call.

---

**Q11.** Columnar databases keep appearing in our answers. Coincidence?

**A11.** None at all. Sideways layout (9), grouped folds (10), dictionary encoding — analytics engines converged on these because the hardware insists. Align's move is making them *language*, so ordinary code lands where the engines did.

---

**Q12.** Drill, from a JSON string of `{"name":..., "a":..., "b":...}` rows: distinct names, and the largest `b` per name — one pass.

**A12.**

```align
xs: array<Row> := json.decode(data)?
g := xs.group_by(.name).agg(max(.b), count())
print(g.0.count())      // how many names
print(g.1.max())        // the largest of the per-name maxima
```

(Decode, group, fold — three lines from text to answer. The `count()` rode along free.)

---

**Q13.** Does “one pass” mean “no extra memory”?

**A13.** No. Grouping needs a table of accumulators and then its result columns — roughly one entry per distinct key. `agg` shares that table and the trip through the input; it does not make the groups cease to exist.

---

**Q14.** What if every row has a different key?

**A14.** Then the grouped result is nearly as large as the input. The operation is still direct and visible, but not free. Always ask two sizes: *how many rows?* and *how many groups?*

---

**Q15.** Should we `dict_encode` before every string grouping?

**A15.** No. A single string `group_by` already does the necessary interning for that aggregation. `dict_encode` earns its place when the encoded column will be reused for several groupings or comparisons. Pay once only when there really is a twice.

---

**Q16.** Orders by customer:

```text
7  10
9   4
7   3
9   8
7   2
```

What per-customer sums must come back?

**A16.** Customer `7 → 15`; customer `9 → 12`. Do the grouping in your head before touching syntax.

---

**Q17.** Say it over fields `.customer` and `.amount`.

**A17.**

```align
g := orders.group_by(.customer).sum(.amount)
```

`g.0` holds the customers; `g.1` holds the aligned sums.

---

**Q18.** We also need each customer's largest order and order count. Three groupings?

**A18.** One:

```align
g := orders.group_by(.customer).agg(
    sum(.amount),
    max(.amount),
    count(),
)
```

One key table, three accumulators per group.

---

**Q19.** What is the sum of `g.1`, the per-customer sums?

**A19.** `27`, the same as the sum of every input amount. Grouping changes association, not the total contribution.

---

**Q20.** What is the maximum of the per-customer maxima?

**A20.** `10`, the largest order in the whole input. A reduction may follow a grouped reduction; just keep track of which column now flows.

---

**Q21.** Five reports reuse customer names, but each report groups a different value. What is the shape of the optimization?

**A21.**

```align
encoded := orders.dict_encode(.customer)
sales := encoded.group_by(.customer).sum(.amount)
counts := encoded.group_by(.customer).count()
```

Encode the repeated key column once, then ask several grouped questions. The reusable thing is not a half-`group_by`; it is the explicitly encoded data.

---

> **The Tenth Commandment**
>
> *Group and fold in one breath. Ask all your questions in one pass, and pay for a string key once.*
