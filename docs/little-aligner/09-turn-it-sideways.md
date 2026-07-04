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

**A2.** Straight across one line, touching nothing else. Memory has the same preference as your eyes. Row-major is **AoS** (array of structs); sideways is **SoA** (struct of arrays).

---

**Q3.** Why does the machine care?

**A3.** Cache lines. Memory arrives in 64-byte gulps: in AoS, each gulp of ages brings names and flags you didn't ask for; in SoA, a gulp of ages is *sixteen ages*. And SIMD (chapter 12 of the guide) eats only contiguous lanes — the column *is* the lane feed.

---

**Q4.** How do we turn data sideways in Align?

**A4.** One call, inside an arena:

```align
User { active: bool, score: i64, age: i64 }

arena {
    mut s := rows.to_soa()      // soa<User> — three columns now
    ...
}
```

Still a `soa<User>` — you keep thinking in `User`; only the memory turned.

---

**Q5.** What is `s.age`?

**A5.** The age **column** — a plain slice of `i64`. And a slice means every chapter so far applies: `s.age.sum()`, `s.age.max()`, `s.age.map(...)...`.

---

**Q6.** With the three users above, what is `s.age.sum()`?

**A6.** `96`. One dense line of memory, summed.

---

**Q7.** What is `s.where(.active).age.sum()`?

**A7.** `71` — `30 + 41`; bob is inactive. Two columns touched (`active` to filter, `age` to sum); the names never left RAM. That is the entire trick, and it is a big one.

---

**Q8.** May we still have alice back — the whole row?

**A8.** `u := s[0]` gathers one `User` from the three columns. Costlier than a column read (three fetches instead of one) — the sideways layout charges for *rows* what it saved on *columns*.

---

**Q9.** And one cell? A write?

**A9.** `s[0].age` reads one cell; with a `mut` soa, `s[1].score = 99` writes one — straight into the column.

---

**Q10.** A window? The middle of a column?

**A10.** `s.age[1..3].sum()` — slice the column like any slice: `25 + 41 = 66`.

---

**Q11.** When does the sideways layout *lose*?

**A11.** When you touch whole rows, rarely and singly — a config record, one request. Gathering every field re-scatters what SoA gathered. Rows you handle whole: AoS. Columns you scan in bulk: SoA.

---

**Q12.** The data arrives as JSON. Must we build rows first and turn them after?

**A12.** No — decode *directly* sideways:

```align
s: soa<User> := json.decode(data)?
```

The parser fills columns as it reads; no row-shaped middleman, and string columns are views into the JSON text itself.

---

**Q13.** Why did every soa live inside an `arena`?

**A13.** Chapter 8's answer: columns are a *batch* with one lifetime — born from one decode, dead after one analysis. The arena states it; the compiler holds you to it.

---

**Q14.** Say the habit.

**A14.** *When I scan fields in bulk, I turn the data sideways at the door* — `to_soa()`, or `json.decode` straight into `soa<T>` — *and speak in columns from then on.*

---

> **The Ninth Commandment**
>
> *Scan columns, not rows. Turn data sideways once, at the door, and the machine will thank you all day.*
