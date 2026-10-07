# Text byte producer provenance

Request 124's caller-rooted Unicode trim checks in HIR but fails MIR producer
certification. The smaller `value[bytes[0] as i64..value.len()]`, with
`bytes := value.bytes()`, fails without a loop. A byte element read reaches
`Rvalue::Use`'s text-to-byte-view retype with an `Element` projection. Replaying
that projection unchanged against the source `str` cannot select a typed leaf.

The existing string contract owns this behavior: `.bytes()` is a zero-copy,
read-only byte view, indexing it copies a `u8`, and a string range inherits the
text owner's region. Plans 52, 53 and 55 own the unchanged read-only, founded
initialization and MIR publication strategy. Correct the typed projection at
the retype; do not change those proofs, source lifetime inference, K1, any
runtime operation, or any persisted summary/ABI.

## Implementation closure matrix

| Cell | Implementation and exact owner |
| --- | --- |
| Formation and type admission | `Rvalue::Use` still requires its existing exact value-flow or view-retype relation. A byte element maps to the source text at the same enclosing path only for the admitted `str`/`string` to `slice<u8>` retype. Option prefixes retain exact projection/tag semantics; other retypes keep ordinary projection rules. MIR `text_byte_retype_producers_preserve_grounding_and_authority` covers text source classes, wrappers, wrong element widths and malformed operand/type records. |
| Construction and read authority | Ground the selected byte in a read dependency on the selected source text, never an unconditional seed. Preserve every readability/initialization obligation. Existing scalar-copy normalization grants owned bits to the indexed `u8`; the separate writable-backing graph is unchanged. The MIR owner crosses initialized/uninitialized and unreadable sources; the driver rejects source writes through text bytes and admits an explicit writable copy. |
| Move-in/out, source nulling, Drop, replacement and return | No ownership or cleanup lowering changes. New driver `text_byte_provenance` returns a caller-owned range after byte-derived bound calculation and exercises borrowed owned text, replacement of scalar bounds and independent owned copies. Callee-owned text escape and byte-view write negatives retain rejection. Existing producer owned-return and cleanup mutation owners remain required. |
| Control and completion | The original Unicode witness spans both loops, nested loop joins, conditional helper calls and early breaks. Driver cases cover empty, ASCII, multibyte whitespace/content and ASCII-only mode, with exact returned bytes. Existing branch/match/else/?/map_err and malformed-control producer owners cover unchanged joins; no global fixed-point or reachability rule changes. |
| Generic/imported/interface | Driver local/imported/generic readers run whole-program and per-unit with interface serialization replay. The compiler's existing build identity separates artifacts; no interface byte or cache schema change. |
| Whole/per-unit/native and malformed publication | MIR mutation owner checks full and partition certification; driver executes both compilation modes. Existing backend producer owners keep emission parity. No failing body is marked certified merely from its declared return type. |
| Runtime provenance, allocation and performance | Text and byte views retain the original owner/region and allocate no backing storage. No runtime, MIR shape, LLVM lowering or ABI changes. No performance/resource improvement is promised, so no benchmark is required. |
| Test artifact/process lifecycle | New driver fixtures exclusively acquire ArtifactStage, retain it through all compiler APIs and execution, and arm bounded child cleanup immediately. No legacy TempProject or unbounded Command::output call chain. |

One capability joins the typed producer correction with its positive and
adversarial owners. It follows the reviewed existing proof strategy; perform
the author matrix-to-diff pass and one fresh preflight review. Focused owners
are the named MIR test and the `text_byte_provenance` driver target, followed
by the normal final-SHA code gate.

General forged `StoreIndex` write-authority certification belongs to deferred
K1, not this producer correction. Source text-byte writes remain refused by
the existing HIR analysis. Do not add a claimed general MIR write proof to
this boundary or resume K1 to supply it.
