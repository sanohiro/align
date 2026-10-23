# 9. Turn It Sideways

> 🌐 **English** · [Japanese](./ja/09-turn-it-sideways.md)

**Q1.** Three users on a table, one per row:

```text
alice   30   true
bob     25   false
carol   41   true
```

Read every **age**. Which way do your eyes move?

**A1.** Down a column — skipping past a name and a flag on every row.

---

**Q2.** And if the table were written *sideways* — one row per field?

```text
name:   alice  bob  carol
age:    30     25   41
active: true   false true
```

**A2.** Straight across one line, touching nothing else. The same two arrangements are possible in memory: **AoS** (array of structs) keeps each record together; **SoA** (struct of arrays) keeps each field's values together.

---

**Q3.** Why does the machine care?

**A3.** The CPU loads memory in cache lines. With 64-byte cache lines and `i64` ages, a line of an age column holds eight ages. An AoS layout also brings in names and flags, even when we only want ages. Contiguous columns also suit SIMD loads (chapter 12 of the guide).

---

**Q4.** If SoA helps with column scans, why keep AoS at all?

**A4.** Because some work uses a whole record at a time. AoS keeps that record's fields together. SoA helps when we repeatedly scan selected fields across many records. Choose the layout for the work; neither layout requires a particular programming paradigm.

---

**Q5.** How do we turn data sideways in Align?

**A5.** One call, inside an arena:

```align
User { name: str, age: i64, active: bool }

arena {
    mut s := rows.to_soa()      // soa<User> — three columns now
    ...
}
```

Still a `soa<User>` — you keep thinking in `User`; only the memory turned.

---

**Q6.** What is `s.age`?

**A6.** The age **column** — a plain slice of `i64`. And a slice means every chapter so far applies: `s.age.sum()`, `s.age.max()`, `s.age.map(...)...`.

---

**Q7.** With the three users above, what is `s.age.sum()`?

**A7.** `96`. One dense line of memory, summed.

---

**Q8.** What is `s.where(.active).age.sum()`?

**A8.** `71` — `30 + 41`; bob is inactive. This pass reads two columns: `active` to filter and `age` to sum. It does not read the names.

---

**Q9.** May we still have alice back — the whole row?

**A9.** `u := s[0]` gathers one `User` from the three columns. Reading the whole row means accessing all three columns, rather than just the age column.

---

**Q10.** And one cell? A write?

**A10.** `s[0].age` reads one cell; with a `mut` soa, `s[1].age = 26` writes one — straight into the column.

---

**Q11.** After that update, how do we read a range from the middle of a column?

**A11.** `s.age[1..3].sum()` — slice the column like any slice. After Q10's write, this is `26 + 41 = 67`.

---

**Q12.** When might AoS suit the work better?

**A12.** When you usually handle a whole record at a time — a config record, one request. AoS keeps its fields together. SoA is useful when the repeated work scans selected columns.

---

**Q13.** The data arrives as JSON. Must we build rows first and turn them after?

**A13.** No — decode *directly* sideways. With `import core.json` at file scope, inside the arena:

```align
s: soa<User> := json.decode(data)?
```

The parser fills columns as it reads, without an intermediate array of rows. Unescaped `str` fields view the input text. Escaped strings are decoded into arena storage, so those fields need an allocation.

---

**Q14.** Why did every soa live inside an `arena`?

**A14.** These forms of `to_soa` and `json.decode` allocate their columns in the arena. The batch remains available for any analyses inside that scope; the columns are released when the scope ends.

---

**Q15.** What should we remember when we need to scan columns?

**A15.** Choose the layout before those scans. Convert with `to_soa()`, or use `json.decode` directly into `soa<T>`, then work on its columns.

---

**Q16.** Is `rows.to_soa()` free?

**A16.** No. It allocates columns and transposes every row into them. Repeated column scans may repay that conversion. For one small whole-row operation, the conversion adds work with little to gain.

---

**Q17.** At what point should we choose the layout?

**A17.** Before the repeated work begins. When reading JSON, decode directly into `soa<T>` to avoid an intermediate array of rows. If an AoS already exists, consider whether the coming column scans will repay the conversion.

---

**Q18.** Choose AoS or SoA: a million particles, every frame updating only `x`, `y`, and `velocity`, while names and metadata stay cold.

**A18.** SoA. The hot loop should stream the few hot columns without hauling cold fields through cache.

---

**Q19.** Choose again: twelve configuration records, usually loaded, validated, and printed one whole record at a time.

**A19.** AoS. Small, whole-row work gets no useful repayment from a transpose.

---

**Q20.** Orders have `price`, `quantity`, and `active`. Total the value of active orders.

**A20.**

```align
orders
    .where(.active)
    .map(fn o { o.price * o.quantity })
    .sum()
```

`map(fn o { … })` receives a whole **row**, so `orders` here is an AoS `array<Order>`. This form of `map` is rejected on `soa<Order>` because it would gather every column to rebuild each row. With SoA, bind `s.price` and `s.quantity`, pair them with `zip` (chapter 13), and reduce. `where(.field)` also works on SoA: it tests one column without rebuilding the row (A8).

---

**Q21.** Sideways now, over a `soa<Order>` named `s`. Why not write `s.price.sum() * s.quantity.sum()`?

**A21.** Because multiplication belongs row by row before the sum. The first order's price pairs with the first order's quantity. Columnar layout changes storage, never relationships.

---

**Q22.** Analyze only rows 100 through 199.

**A22.** Slice the projected columns before reducing. A projected column is an ordinary slice; slicing the complete SoA as `s[100..200]` is not supported:

```align
s.price[100..200].sum()
```

The half-open range contains one hundred prices.

---

**Q23.** You need one customer's entire record after finding its index. Is gathering one row a failure of SoA?

**A23.** No. `s[i]` gathers it. Choose the layout for the work you do most. Occasional row gathers can fit a program that mostly scans columns.

---

**Q24.** Finish Q20 using columns. What does this complete program print?

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
        print(total)
    }
    return 0
}
```

**A24.** `48`: `10 * 2 + 7 * 4`. `zip` pairs values at the same index: `.0` is price, `.1` is quantity, and `.2` is active. It supplies one tuple at a time, without building an array of tuples. The filter keeps or rejects that whole tuple.

---

**Q25.** May we filter or sort each column separately before `zip`?

**A25.** Only if the columns still represent the same rows in the same order. Equal length is necessary but does not prove that relationship. For example, sorting prices alone pairs them with other orders' quantities. `zip` rejects unequal lengths rather than truncating to the shortest input, but preserving the row correspondence is our job. Zip the related columns first, then apply a shared filter as Q24 does.

---

> **The Ninth Commandment**
>
> *Choose SoA for repeated column scans. Convert once, then reuse the columns.*
