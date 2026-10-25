# Exact producer query result reuse

## Measured defect and boundary

R37's authenticated whole-project helper control exceeds a 240 s supervised
MIR-emission deadline near 600 MiB RSS. Samples place active CPU in native
producer access-graph construction and solving. This is separate from
[Plan247](247-builder-validation-live-state.md)'s builder-reservation memory.

Reuse the final `XmlProducerState` of every completely solved node within one
immutable `ValidatedProducerGraph`. The graph owns the cache, so its function,
program, definition tables, slot stores and local-contract component cannot
change while cached results remain available. Operand, slot and buffer queries
validate their type/path/descriptor before looking up an exact node. A result
answers only that root query. The existing internal-equation shortcut remains
restricted to valid Present Owned/Shared/Exclusive/Unreadable seeds; conditional,
absent, mixed and invalid results never become unconditional internal seeds.

Equations depend on graph and node, not the selected query root. Complete the
pending dependency closure and solver before publication. Final invalidity
always overrides the raw value, and an unresolved node records Invalid. Preserve
all initialization, discriminator, copied-bit, read-dependency and invalidity
rules. No language, solver, IR, ABI, emitted-code or persistent-cache change.

## Implementation closure matrix

| Axis | Invariant and owner |
| --- | --- |
| Identity and lifetime | The key retains node kind, value/slot and complete tagged path. The graph owns a fresh function/component-local cache. `producer_query_cache_preserves_complete_results` compares fresh and warm results in both query orders, including distinct Value/Capture/Buffer kinds. |
| Formation and malformed input | Operand source, exact slot type/path and buffer constructors still authenticate before build. The same owner warms a valid node before malformed type/path queries and checks missing definitions and uninitialized cycles. Existing producer structural owners retain their rejection. |
| Complete result publication | Every built equation gets a result only after the complete solve; invalid and unresolved override raw states. The differential owner includes absent/conditional roots and guarded extraction. Existing `producer_copied_bits_preserves_grounding_and_readability` and `producer_fixed_point_preserves_guarded_absence_in_seeded_cycles` retain lattice, check-edge and invalidity coverage. |
| Internal reuse | Only the four previously admitted valid Present authorities seed a different graph. Exact-root reuse must not weaken initialization or erase a discriminator. The differential owner and existing producer authority/call owners cover these boundaries. |
| Position-sensitive calls | Keep call mode, descriptor, backing authority and call-position initialization checks outside the cache. Never memoize whole call-validity or initialization predicates by node. `producer_query_cache_preserves_call_position` warms a later initialized call then rejects an earlier uninitialized call, and reverses the order. |
| Ownership and control | Formation, construction, move/nulling, Drop, replacement, return, branch/loop joins and fallible/early exits retain existing equations and transfers. `producer::borrow_call_tests` covers arena/fixed/dynamic/tagged owners, malformed producers and direct/indirect modes; `arena_decoder_buffer_and_payload_are_distinct` now compares cold/warm buffer queries after slot queries. |
| Consumer and runtime | No generic/interface/cache schema, allocation or backend change. `call_boundary_views` retains imported/whole/per-unit, publication, native and cold/cache-hit owners. Existing MIR producer and codegen gate owners close the remaining unchanged transfer surface. |
| Work and resource | A test-only count measures actual solved equation nodes; repeated/reversed queries must add no work after their exact roots are cached. No wall-time assertion. Authenticated helper and inline projects run under the same 3 GiB/240 s supervisor; source/compiler/output hashes bind the observations. |

Independent strategy review accepted exact result reuse before implementation and
accepted the same graph-owned cache across slot/buffer entry points. An initial
operand-only replay still reached 240 s; a fresh sample identified uncached
place-call slot queries. The completed boundary includes those queries while
retaining their position-sensitive prechecks.

## Validation and measured result

On this macOS arm64 host with an opt-level-1 compiler, whole-project helper MIR
emission completes in 44.080 s at sampled peak 603,968 KiB. The original inline
project completes in 67.150 s at 1,519,088 KiB. Both exit successfully under the
same 3 GiB/240 s supervisor. Earlier helper runs reached 240 s without completion;
these observations do not establish a general asymptotic or cross-machine bound.

Private manifests remain under `.git/handoff/r37-query-reuse-complete`.
MIR output is hashed and counted while streaming, not retained as a large dump.
Private consumer source remains outside public Git. No consumer code, pin or
adoption state changes as part of this capability.
