# Unfinished text-builder byte-view assessment

Status: deferred after independent strategy review. No new source, native ABI,
HIR/MIR operation or interface contract is selected or implemented.

LLM Request28 needs exact comparisons against previously accumulated names.
Plan140 supplies initialized lengths and Plan209 supplies independent Copy
indexed reads from typed builders. The remaining text-prefix request would
publish a borrowed `slice<u8>` from an unfinished text builder. All three
consumer registers were reconciled after PR1361; media and voice use text
builders for names and token keys but do not add a separate authority solution.
Consumer adoption remains external.

## Rejected strategy and evidence

The initial proposal assumed existing immutable-text roots and flat lifetime
summaries could preserve a new byte view's read-only permission through helpers,
carriers and imported returns without new interface facts. Independent review
found that premise false. At merged main `f71c934a5858932150538879e18c54176fe1e471`,
the checker rejects writing through a direct `text.bytes()` result but accepts
both of these compile-only negative witnesses:

```align
fn prefix(text: str) -> slice<u8> = text.bytes()
fn main() {
  mut text := "abc".clone()
  mut view := prefix(text)
  view[0] = 65 as u8
}
```

```align
fn prefix(text: str) -> slice<u8> = text.bytes()
fn main() {
  mut text := "abcdefgh".clone()
  view := prefix(text)
  mut typed: slice<u32> := view.view_le() else { return }
  typed[0] = 0 as u32
}
```

These witnesses were checked only, never executed. A shared returned-view read
also checks successfully. The distinction is permission transport, not type
formation or ownership of a copied scalar.

`summary_from_roots` deliberately removes `BorrowRoot::ReadOnly` when producing
`ReturnBorrowSummary`; that public record carries only parameter/capture indices.
`map_summary_roots`, `selected_move_call_headers` and
`mutable_call_result_completion` do not restore returned read-only authority.
Internal projected header facts are not a public transport contract. Existing
mutable-view effects describe eligible direct Copy-view parameter writes and
rebindings; they do not encode returned permission or selected result paths.

This is the already-recorded prerequisite in Plan61 and Plan196, not a new
builder-specific defect. A new `BuilderBytes` producer or a return-only flag
would leave hidden callee writes, mutable destinations and recursive admitted
carriers without a complete proof. The proposed operation cannot safely be
published by reusing those records unchanged.

## Disposition and restart boundary

The complete text-byte-view capability remains pending with the existing
interprocedural access prerequisite. This assessment does not resume either
parked Plan61 implementation or the general aggregate-provenance experiment.
Their current selection and finite-abstraction limits remain in HANDOFF.
No native pointer getter, special caller restriction, implicit copy or second
collection is shipped as a partial substitute.

A future selected strategy must specify a finite permission representation,
producer and projection transfers, call/return and mutable-destination transport,
recursive bounds, cross-unit encoding and independent checked replay together.
It must preserve read-only authority through helpers and carriers and retire
views on every builder write, finish, move, replacement and Drop. Positive
readers and explicit copies must remain usable. Reusing an existing internal
root is insufficient evidence for those public guarantees. The authoritative
ledger and closure matrix must be independently reviewed before implementation.

Request28 remains partially provider-delivered: lengths and independent Copy
element reads are shipped; text views, view-bearing elements and consumer
verification are pending. There is no new performance promise or benchmark.
The review finding changes this assessment only, so specifications, ABI/HIR
inventories and language mirrors retain their shipped contracts.
