# Align v0.13.0 Release Notes

Align v0.13.0 adds nameable HTTP listener ownership and Copy request views, reduces generated predicate and bounds-check overhead, and repairs borrowed-view lifetime checks used by database APIs.

## HTTP

- `http_server` names the existing Move listener type returned by `http.serve` and `http.serve_shared`. Listeners can pass through supported owning parameters, returns and record fields, with the existing single-owner cleanup rules. Shared helpers can borrow a listener for repeated accepts.
- `ctx.view() -> http_request_view` creates a Copy, non-owning request view without allocation. It supports method, path, body, headers, upgrade-readiness and peer-EOF observations, including ordinary callback captures. The view retains the context's lifetime; response consumption and task protection remain enforced. Cancellation and application/GPU cleanup policy remain application-owned.

## Generated code

- Constant string prefix/suffix chains specialize more effectively through runtime LTO. The measured consumer classifier no longer retains its six suffix calls. Dynamic predicates still follow ordinary LLVM inlining decisions; universal inlining is not promised.
- Conditional Option results, guarded codec accessors and checked codec column projections share direct join lowering. Tagged storage remains defined, and range/type checks still precede dependent reads.
- Fixed-width byte reads and writes use one fused unsigned bounds guard. Invalid offsets retain their signed diagnostics and evaluation order. Both range-proof consumers recognize the same predicate. The measured consumer comparison helper has fewer comparisons and trap sites; matched native timing was neutral.
- Matched tokenizer measurements qualify the existing scalar-builder path, and imported conditional record construction retains its cleanup and per-unit behavior. Consumer-wide acceptance remains separate from these provider measurements.

## Correctness and qualification

- Borrowed indexed elements and headerless resource references retain their exact caller lifetime when returned through view-producing helpers. Local, arena and by-value owning sources remain unable to escape. Missing, ended and unknown provenance still fails closed; the repair does not grant writable return authority.
- Database batch and row-view APIs are covered by whole-program, per-unit and real PostgreSQL/SQLite integration owners.
- Nightly owners now assert the shipped call-boundary rules and exact diagnostics. Full-suite builds omit Rust debug and incremental artifacts in a separate cache namespace, retaining all test binaries, shard coverage and existing timeout budgets.

## Compatibility and known limits

- Ordinary value parameters, captures and returned non-owning views remain read-only. Partial exclusive owned-field restrictions remain unchanged. No general writable-return or direct fixed-array Result capability is added.
- Validation summaries remain conservative across a single aggregate parameter: text may refer to a numeric sibling's byte view. Keep independent mutable domains separate when repeated calls cannot prove that distinction.
- Install the compiler, REPL and matching runtime archive together and rebuild generated programs. Provider delivery does not change external consumers' compiler pins or complete their adoption work.

Artifacts target Linux x86_64, Linux ARM64 and macOS Apple Silicon and include the matching runtime and first-party package cache. Versioned builds use the existing thin-LTO and two-phase compiler-PGO distribution workflow.

Implementation and verification: [changes since v0.12.0](https://github.com/sanohiro/align/compare/v0.12.0...v0.13.0).
