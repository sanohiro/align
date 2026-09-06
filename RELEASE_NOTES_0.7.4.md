# Align v0.7.4 Release Notes

Align v0.7.4 fixes a MIR producer-certification nontermination exposed by the
align-llm resident Qwen AlignPack loader.

## Producer fixed-point termination

Producer certification now computes present access, reachable absence, and
validation-only invalidity in separate monotone phases. Authenticated guard
projection no longer feeds a transient narrowing back into cyclic slot/value
graphs, and rejecting access states use one conservative semilattice element.

The correction preserves the existing fail-closed ownership contract: borrowed,
mixed, unresolved, and malformed producers remain rejected, and only capable,
unconditional producer results enter the certification cache. Reduced
borrowed-reader regressions cover whole-program and per-unit compilation as well
as shorter-lived-view rejection in both modes.

The unchanged align-llm Request 58 source now leaves MIR resource validation and
reaches native linking in 15.52 seconds on the Align development host, where the
v0.7.3 compiler remained in validation beyond 75 seconds. The align-llm owner
performs the release acceptance run on the requested Apple M1 host after updating
its pinned Align revision.
