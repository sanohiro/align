# Data-oriented design: SoA and grouped aggregation

> 🌐 **English** · [Japanese](./ja/11-data-oriented.md)

This chapter is why Align exists. How you lay data out in memory decides how fast you can process it — often by an order of magnitude — and Align makes the fast layout a declaration instead of a rewrite.

## Array-of-structs vs structure-of-arrays

A million particles, each with position and velocity. The obvious layout is an array of structs (AoS):

```text
[ {x0,y0,vx0,vy0}, {x1,y1,vx1,vy1}, ... ]
```

To update just the `x` of every particle, the values you need are scattered every few words: you drag whole structs through cache to touch one field, and SIMD can't load the lanes contiguously. The structure-of-arrays (SoA) layout stores each field as its own dense column:

```text
x:  [x0, x1, x2, ...]
y:  [y0, y1, y2, ...]
vx: [vx0, vx1, vx2, ...]
```

Now a field-wise pass walks dense arrays in lockstep — perfect cache behavior, 8–16 SIMD lanes per instruction. This is the layout GPUs demand and databases converged on (columnar storage) — and in most languages, adopting it means manually shredding every struct in your program. In Align it is one call.

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

You still *think* in `User`; only the memory is columnar. `s.field` projects a column (an ordinary slice — the entire chapter [06](06-pipelines.md) vocabulary applies), `s[i]` gathers a row, `s.field[a..b]` windows a column. The columns live in an arena (`to_soa` must be called inside one): a batch layout with a batch lifetime, per chapter [05](05-memory.md).

The payoff is not subtle. A columnar scan like `s.where(.active).score.sum()` benchmarks around **8–10× faster** than the same logic over idiomatic AoS in Rust — not because of a smarter loop, but because the layout stopped fetching bytes the loop never used.

Better still, you can skip the transpose entirely: `json.decode` parses **directly into** `soa<T>` (chapter [08](08-json.md)) — columns filled while parsing, string columns borrowing the input.

## `group_by` — the analytics primitive

Partition rows by a key, reduce each group. Over an `i64` key on a soa:

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

`group_by(.key)` must be completed by an aggregate — `.sum(.f)`, `.min(.f)`, `.max(.f)`, `.count()` — and returns a pair of columns: `g.0` the keys, `g.1` the aggregated values. (A bare `group_by` with no aggregate is a compile error: an unmaterialized "grouped thing" would be a hidden cost.)

> **Cost:** For a fixed aggregate list, grouping is expected O(n) and uses O(n) additional storage in the worst case. A compact integer-key range takes a direct-index path; other shapes use hashing. `.agg(...)` scans the rows once, while its per-row work grows with the number of listed accumulators.

Over a `str` key on a decoded array, with **several aggregates in one pass**:

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

`.agg(...)` interns each key once and folds all the accumulators in a single pass — the shape a hand-written analytics loop takes, generated from a declaration. It accepts `str`-key AoS and SoA sources. Numeric-key SoA supports the individual grouped reducers; its multi-aggregate `.agg(...)` form remains deferred.

## `dict_encode` — pay for the key once

String keys cost hashing and comparison. When you aggregate the same key column repeatedly, encode it once into dictionary ids, then reuse:

```align
e := xs.dict_encode(.name)              // intern the str column → dense ids
s := e.group_by(.name).sum(.score)      // these reuse the ids —
c := e.group_by(.name).count()          // no re-hashing per pass
```

This is the classic columnar-database trick (dictionary encoding), surfaced as one call. It benchmarks 1.4–4× faster than re-grouping raw strings per pass.

## The habit

When data is processed in bulk — records walked repeatedly, a field or two at a time — reach for `soa<T>` at the point the data enters the program, and think in whole-column operations from there. The rule of thumb: **if a loop touches one or two fields of many rows, AoS is fighting you.** Keep AoS for data you touch whole and rarely (a config struct, one request), and let `emit-llvm` or a benchmark settle any argument — in Align the layout change is one line, so trying it is cheap.
