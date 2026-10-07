# Owned resource value provenance

Request 125 reports a valid owning `Result` match whose extracted resource is
moved into a record and then mutably borrowed. MoveCheck retains the arm binding
as a borrowed owner of the transferred value and rejects the receiving record.
The same synthetic dependency can pass through another local or a control join.

This corrects the existing L3 contract in
`17-library-boundary-prerequisites.md`: moving an owner transfers ownership;
only an actual retained reference or dependent child borrows another owner.
The storage root used by `resource.borrow` remains distinct from the value's
contained dependencies. Preserve stored parent roots and parameter summaries;
do not weaken live-dependent rejection, borrowed-match restrictions, or the
deferred K1 authority boundary. No syntax, type, ABI, allocation, interface
format, or runtime cleanup strategy changes.

## Implementation closure matrix

| Cell | Implementation and acceptance owner |
| --- | --- |
| Type formation and construction | Existing nominal resource/ref validation and `from_raw`/`from_raw_borrowed` remain. `resource_ownership` declaration, privilege, null and generic owners retain malformed-input coverage. |
| Value versus storage provenance | `MoveCheck::local_borrow_fact` must retain explicit dependencies without inventing a dependency on a transferred resource-only owner's own local. `local_storage_roots` still supplies that local for a real borrow. New resource transfer owner covers direct resources and nested aggregate/sum payloads. |
| Move-in/out, source nulling, replacement, return and Drop | New transfer owner performs match extraction, wrapping, replacement and mutable observation, with Drop-hook output proving exactly one cleanup per constructed resource. Existing `resource_drop_is_exactly_once_across_moves_returns_and_into_raw` owns cleanup-bit/nulling and all completion forms. |
| Control paths | New parameterized owner crosses Result/Option/enum matches, direct/block/if/loop transfer and early error return; existing drop owner covers `else`, `?`, `map_err`, branch/loop joins and early exit. All continuing paths retain dependencies; diverging alternatives supply no value. |
| Genuine dependencies and exclusivity | New negative cases carry child-parent and retained resource-reference dependencies through owning matches/records and reject parent mutation/move and stale reference use. Existing raw-view, all-peer alias, dependent-child Drop-order and borrowed-match owners remain authoritative. No exclusion based merely on a root being moved. |
| Imports, generics and whole/per-unit compilation | New transfer owner uses imported constructors and generic identity, with whole-program and per-unit diagnostics/executable parity. Existing imported/indirect generation owners cover summary transport. No serialized representation changes. |
| Runtime provenance and allocation parity | Runtime, MIR and LLVM are unchanged; existing allocation/cleanup paths remain. Drop output and child-before-parent owners close observable ownership. No performance/resource improvement is claimed, so no benchmark is required. |

One capability changes value-fact formation and its owner tests. It implements
the reviewed L3 safety strategy, so the author matrix pass precedes code and
the fresh preflight review checks the final boundary. The existing resource
owner target is the focused gate; no new broad design review is needed.
