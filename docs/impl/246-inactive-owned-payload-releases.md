# Inactive owned-payload release alternatives

## Evidence and proposed boundary

Plan230's loop-free R80 reduction calls a producer returning
`Result<array<i64>, Error>`, moves its Ok payload into an outer
`Option<array<i64>>`, and returns that Option. The immutable and else-unwrap
controls pass. This is independent of Plan245's nested-loop advance correction.

A bounded author trace of the existing MoveCheck shows the exact cause. On the
Ok arm the generation's release is the destination Local/OptionSome. On the Err
arm the same potentially allocated Ok generation still has its producer's
Staging/ResultOk release, although that payload cannot exist on the Err edge.
The join retains both alternatives and later temporary cleanup ends the retained
generation. Ignoring Staging whenever a Local alternative exists would be unsound.

Preserve the language, runtime and existing finite generation model. Proposed
correction: before walking each owning-match arm, remove only release associations
belonging to provably inactive payload paths of that exact scrutinee. Do not erase
a generation, its contents, historical roots, ended state, caller provenance or
an unrelated owner/release. This is path refinement by an already-checked tag,
not an extension to relational whole-program analysis.

Refine only an initial Staging or current-depth IterTemp association on a known owned completed-scrutinee
header whose descriptor matches its directory entry. Its generation must be
Current(Producer); the release action and complete original result path must
match that producer. Root inactivity is determined from the completed header's
path and the arm's validated complete variant set. If another active or unknown
header supports the same generation/association, retain it. Missing/unknown
metadata remains conservative. Do not traverse subexpressions or backing storage.

The sole IterTemp constructor substitutes it for initial Staging inside a loop;
its nonzero depth must also equal the current loop depth. Fresh narrow strategy
review accepted this equivalent initial-owner case before implementation.
Preserve Prior/parameter generations, reparented staging, Local, unrelated IterTemp,
LoopResult, ArenaScope and Returned releases. Preserve all pre-existing ending
evidence, contents, historical roots and unrelated release alternatives. Named
locals and loop-result controls already pass; this capability does not widen
their ownership handling. Both tags and all user-sum arms are symmetric.

Fresh independent strategy review accepted this narrowed boundary with the above
corrections. Cleanup ordering and the joined-value reachability union remain
unchanged: early cleanup would manufacture false ending evidence retained by a
peer's returned owning value. No language restriction or solver extension is needed.

## Implementation closure matrix

| Axis | Required implementation proof and acceptance owner |
| --- | --- |
| Formation and tag validation | Reuse checked HIR types, typed header paths and arm variants. Do not infer inactivity from missing/unknown metadata. `storage_generation_move_action_directory_matrix` preserves state for malformed/unknown headers and out-of-range variant sets. A wildcard uses the complement of preceding explicit arms; an initial wildcard retains every variant. |
| Construction and source shape | `owned_match_releases::source_and_type_matrix` crosses direct/block/if calls, named-local and loop controls and supported carrier sources. Only the exact Current Producer initial Staging/current-depth IterTemp association is refined; all other source/release kinds survive. |
| Move-in/out and nulling | Existing match binding and destination transfer remain responsible for moving the active payload exactly once and clearing its old source. No source generation is renewed, no lifetime added, and no MIR transfer changes. |
| Drop, replacement and return | Existing owners still end every actually live release. Direct branch assignment must survive match-expression cleanup; an actual source/payload/destination replacement, scope exit, hidden-temporary Drop or escaped borrowed member must still reject. Pre-existing ended generations never recover. |
| Control composition | `owned_match_releases::control_and_expiration` crosses if/match/else/?/map_err, absent/Ok/Err/all sum arms, multiple patterns, branch/loop joins, zero/one/multiple iterations, return/break and early error. Preserve real may-alias invalidation on any reachable edge. |
| Aliases and repeated sources | A generation may have other release alternatives or multiple headers. Removing an inactive scrutinee association must not remove a live sibling/source, erase prior ending evidence or grant write authority. Test shared aliases, borrowed elements, prior/current recency and true staging-plus-local negatives. |
| Type siblings | Scalar array, array of owning records, nested owned record/Option/Result and user sum; owned string/scalar controls. Shared borrowed matches and inactive nested variants keep their existing type/ownership domains. |
| Frontends and serialization | Source and independently replayed checked HIR must agree. Local/imported/generic whole-program and per-unit, cold/cache-hit paths retain the existing interfaces and ABI. No trusted activity flag or new serialized metadata. |
| Runtime and allocation | `owned_match_releases::native_lifecycle_and_cache` covers native retained values and counted allocation/Drop for success/error/absence, repeated replacement and real source expiry. No new copy, allocation, codegen or cleanup path. No performance promise. |
| Authentic consumer | Replay FILE_SET failure and immutable control from authenticated private source under existing bounds, both check modes. Consumer runtime/source/pin/adoption remain external. |

## Scope

One independently useful precision repair; no general mutable sum-payload access,
R132/R142 widening or solver replacement. `MoveCheck::refine_inactive_match_temporary`
implements the refinement and the matrix binds its owners. A safety-review finding changes
the matrix first. R37 remains subsequent phase-isolated performance work.
