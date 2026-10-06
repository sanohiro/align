# Environment text admission

Before this repair, `env.get` copied arbitrary native environment bytes into `string`.
An inherited value containing `ff c0` reproduced invalid UTF-8 in a successful
owned result on both macOS and Linux. Returning `None` for those bytes would
hide a decoding failure as absence. Preserve the settled UTF-8 invariant and
the single error model by changing this pre-release API outright.

## Public contract ledger

| Field | Contract |
| --- | --- |
| Surface | `env.get(name: str) -> Result<Option<string>, Error>`, imported from `std.env`; Impure. No defaults or ambient configuration beyond the queried process environment. |
| Name | Borrowed for the call; nonempty UTF-8 with neither NUL nor `=`. Invalid names return `Error.Invalid` before environment lookup or output allocation. No normalization or restriction to ASCII identifier spelling. |
| Value | A missing name returns `Ok(None)`. A present empty value returns `Ok(Some(""))`. A present valid UTF-8 value is copied byte-for-byte into `Ok(Some(value))`. Invalid UTF-8 returns `Err(Error.Invalid)`; no replacement decoding, byte API or absence coercion. |
| Ordering | Validate the native output slot, then the complete name extent/encoding/grammar, then query the environment, then validate the complete present value, then allocate/copy. Every admission failure is Invalid; no lookup or output allocation precedes name admission. |
| Ownership | Name borrowed only during the call. Successful nonempty text owns one independently allocated payload; empty success owns canonical `{null,0}`. Later sequential `env.set` cannot change an already returned value. Ordinary nested Result/Option move, replacement and Drop apply. No allocation for absent/invalid output. |
| Native scratch | One transient NUL-terminated name allocation after admission; borrowed native value is validated and copied before returning. Existing terminal allocator policy remains. No performance promise or benchmark requirement. |
| Concurrency | Existing environment-mutation exclusion remains: callers must not overlap environment mutation with environment access. No hidden lock, process-global state change, retry or new synchronization API. |
| Owner and prerequisites | Existing runtime `align_rt_env_get`, sema `EnvGet`, checked HIR and MIR Result/Option composition; no new language carrier, package, milestone or deferred K1 prerequisite. LLVM stays pure lowering. |
| ABI | Keep keyed `EnvGet` / A08 `i32(ptr,i64,ptr)` and HostState effects. Return 1 for present, 0 for absent, negative fixed error status (`-AL_INVALID`, -2) for rejection. Writable output is zero on absent/error. MIR maps negative status through the existing fixed Error conversion and constructs the public nested result. Non-null native pointers retain ordinary readable/writable, aligned, disjoint range obligations; null output is rejected before access. |
| Artifact identity | Existing compiler build identity invalidates compiled units after compiler changes; runtime source fingerprint authenticates the linked archive. No interface encoding or ABI shape change; existing tagged-type serialization carries the new signature. No persisted/wire format, runtime inspection table or new cache option. |
| Sources of truth | `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, Settled I/O rules in `docs/open-questions.md`, M9 surface in `07-roadmap.md`, checked-HIR `19-hir-validation-ledger.md`, native `20-runtime-abi-ledger.md`, and English/Japanese chapter 13 guide agree. Update in-repository compiled consumers directly; external align-llm adoption is not in scope. |

`env.set` retains its existing signature and sequential behavior. This capability
does not redesign global environment concurrency or argument admission.

## Implementation closure matrix

| Axis | Implementation and acceptance owner |
| --- | --- |
| Type formation and validation | Sema interns existing `Option<string>` tagged payload in builtin `Result<...,Error>`. Checked HIR independently verifies exact nested shape, name type and builtin Error identity. Extend native-record mutation owner for wrong result/payload/name and stale Option-only shape. Existing generic substitution, purity and variant sweeps remain exhaustive. |
| Native construction / malformed input | `align_rt_env_get` admits output/name before libc and present bytes before clone. Native owner supplies child-scoped inherited UTF-8/empty/unset and invalid leading/continuation/overlong/surrogate/truncated/out-of-range encodings; tests null output, negative/empty/null-positive/invalid UTF-8/NUL/equals names, canonical empty/error outputs and raw status identity. Tests never mutate the parallel runner's process environment. |
| Move-in / move-out / source nulling / Drop / replacement / return | Existing nested tagged ownership lowers native fresh string into Some then Ok exactly once, with no load on absent/error. Compiled owner retains values across sequential env.set, returns them through imported/generic helpers and exercises replacement, discarded output and arena escape of independent owned values. Name stays borrowed and reusable. |
| Control flow | Compiled owner covers present/absent/Invalid across `if`, `match`, `else`, `?`, `map_err`, loop/branch joins, early exit in the name expression, and function return. Existing nested tagged cleanup owns each branch; no new cleanup mechanism. |
| Whole-program / per-unit / interface / generic | The same imported typed and generic helpers run through whole-program and per-unit compilation. Round-trip the ordinary serialized interface; existing compiler-build cache identity remains unchanged and receives no special-case bypass. |
| Provenance / allocation parity | Native output owns cloned allocation, never libc storage. Child-isolated native allocation owner checks successful payload allocation/free and no absent/invalid output allocation; shared Rust name scratch is separate. Existing MIR producer validates Str input / String out slot and remains the ownership authority. |
| Existing consumers | Update m9 path/env/time owner and guide examples; adjust pkg_db_q2/q4b inherited-environment fixtures to handle Result explicitly. Run those selected DB owners via required `scripts/db-verify-local.sh`; do not use CI for discovery. |
| ABI and effects | Existing typed native declarations and registry owners prove same A08 shape/key/effects; native status and compiled matching prove new semantics agree. No new IR variant or public extern. |

One capability contains runtime admission, sema/checked-HIR/MIR result assembly,
consumer migration and owner coverage. Splitting them would leave either unsafe
strings or mismatched compiler/runtime status handling. The expected diff is
below roughly 1,000 handwritten lines. Perform author ledger-to-prose and
matrix-to-diff passes, one independent strategy review before implementation,
then the normal fresh implementation review, owner checks, bounded gate and
separate-target Clippy on the final candidate.

## Acceptance bindings

- `align_runtime --features alloc-count --lib env_tests`: inherited-value
  encoding/status/allocation matrix and malformed name/output admission. The old
  native test that mutated the parallel test process is replaced by isolated
  native admission and compiled sequential `env.set`/`env.get` coverage.
- `align_driver --test env_get`: all public result states, imported/generic
  whole/per-unit ownership and control paths, interface serialization, old-signature
  rejection, bad arity/type/import and Impure parallel rejection.
- `align_driver --test m9_path_env_time`: existing path/environment/time
  consumer, including sequential set/get and inherited values.
- `align_mir --lib hir_body_validator_native`: exact nested owned result,
  stale Option-only output, wrong payload and name mutations, plus the existing
  body/control-flow native matrix.
- `align_codegen_llvm --lib producer_imported_owned_option_result_survives_borrowed_match`:
  independently owned output through a borrowed optional name, with unreadable,
  missing-slot and wrong-projection producer mutations retained.
- `scripts/db-verify-local.sh`: migrated q2/q4b environment consumers and
  unchanged native DB service boundary.

The ordinary bounded gate also owns tagged serialization/Drop/type-predicate
sweeps and the typed ABI registry/declaration inventory. No new timing gate.
The independent strategy review found no contract or boundary defects.
