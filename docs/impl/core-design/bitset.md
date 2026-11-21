# Borrowed packed bits

[日本語](ja/bitset.md)

`import core.bitset` provides `bitset.view(storage: slice<u8>, count: i64) -> bitset`. The view borrows existing bytes and allocates nothing. Bit zero is the low bit of the first byte. A negative count or a count exceeding the available bits aborts. Extra bytes and high padding bits are ignored.

```align
import core.bitset

fn example() -> i64 {
  mut bytes := [0 as u8, 0 as u8]
  mut flags := bitset.view(bytes[..], 13)
  flags[8] = true
  return flags[3..].count()
}
```

| Expression | Result |
|---|---|
| `bits[i]` | The selected `bool`. |
| `bits[i] = flag` | Changes one bit in writable backing. |
| `bits[lo..hi]` | A borrowed window, with either bound optional. |
| `bits.len()`, `bits.count()` | Logical length and number of set bits, as `i64`. |
| `bits.any()`, `bits.all()` | Whether any or all logical bits are set. An empty view gives `false` and `true` respectively. |

Indices and bounds are `i64`. An out-of-range index or reversed window aborts before accessing storage. Windows may begin at any bit offset. Indexing and slicing take constant time. Counting scans the touched bytes in constant temporary space.

The Copy descriptor retains the byte slice's lifetime. A shared input or ordinarily returned view permits reads. Mutation requires proved writable backing. To update a view stored in a record, bind the field to a mutable local. Updating a bit reads and writes its containing byte, including for alias and parallel-access checks. Numeric word storage can be viewed through its existing `as_bytes()` operation. The bit numbering of the bytes is independent of native word order.

`core_bitset` exercises borrowed word arrays, unaligned windows, padding, guard pages, native code, and whole and per-unit compilation. [Compiler and layout contract](../271-borrowed-bitsets.md)
