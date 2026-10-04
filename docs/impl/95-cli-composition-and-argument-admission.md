# CLI composition and compiler argument admission

## Existing-module disposition

Plan51's CLI assessment is complete. The plan93 tree summary uses existing
std.cli registration, typed defaults, parsing, getters and usage. The multimodal
reference has three fixed positional roles with exact arity and canonical numeric
inputs; it does not need a second flag parser or a subcommand framework. Existing
std.cli flags suffice for the demonstrated consumer. Slice-based subcommand
parsing has no selected consumer and remains unproposed; no language widening,
new package or real-engine adapter is selected.

Compiler CLI inspection found a separate concrete failure: check accepts an
unrecognized trailing option, and build ignores an unsupported -o plus output
name while producing its normal executable. Refuse leftover compiler arguments
before source, cache, artifact or explicit C-driver work. This changes invalid
tool invocations only; it adds no compiler option or language/library API.

## Exact admission record

Inputs are the UTF-8 compiler prefix already captured by main, after existing
global option syntax validation and stripping. The program suffix after -- is
never inspected. The gate borrows those strings and returns the validated LLVM
stage bool; other verbs return false. Successful admission allocates no new
argument storage. Errors format one owned diagnostic with verbatim token
interpolation, write it to stderr with a terminating newline, produce no stdout
or artifact and return failure (Unix status 1). Embedded token newlines remain
newlines, as in existing stage diagnostics; no single-line promise is made.
Global lexical errors retain their existing precedence, then this residual gate,
then existing verb/option compatibility, environment and source processing.
No new ambient input, cache identity, persisted artifact, runtime ABI, ownership
rule or prerequisite milestone is introduced. The compiler binary owns the gate.

| Verb | Residual compiler arguments after verb and required path/subcommand | Defaults / rejection |
| --- | --- | --- |
| check, check-per-unit, emit-interface, emit-mir, build, size | None | Reject the first remaining argument in source order. build --help still succeeds alone. |
| test | None | Preserve test accepts exactly one entry path on excess or missing entry. |
| fmt | --write or -w, including repeats | Existing read-only default; reject the first other argument. |
| explain-opt | --verbose or -v, including repeats | Existing terse default; reject the first other argument. |
| emit-obj | Zero or one output path, not beginning with - | Existing stem.o default; a leading-dash option or second output is rejected. Use ./ for a leading-dash filename. |
| emit-llvm | --stage raw/optimized or --stage=raw/optimized, including repeats | Existing raw default and last stage wins. Validate left-to-right; malformed/missing stage retains its stage diagnostic, another token is an unexpected argument. |
| cache clear | None | Reject the first remaining argument before resolving/removing cache entries. Unknown cache subcommands retain their existing diagnostic. |
| run | Existing implicit trailing program arguments and explicit -- suffix | Forward unchanged; global options in the prefix retain existing stripping. |
| db, version, unknown/missing verbs or paths | Existing owner | db and standalone version dispatch before this gate. Missing/unknown invocation diagnostics remain with their current owner. |

The diagnostic for a disallowed residual token is exactly
alignc: unexpected argument '<token>' for '<verb>'. Stage-value errors retain
alignc: unknown --stage '<value>' (expected `raw` or `optimized`). No source
contents or filesystem state are needed to reject a residual token.

## Closure matrix and acceptance

| Cell | Implementation / owner |
| --- | --- |
| One gate before native C-driver, target, source/cache/artifact work | main immediately after global stripping; real CLI no-effect owner with valid and absent source and invalid explicit CC |
| Every non-running verb and supported local spelling | Parameterized residual grammar and real CLI owner; existing profile/export/target/CC/watch owners |
| Left-to-right errors, stage default/repetition/value states, missing/excess inputs | Same grammar owner and real multi-invalid diagnostics, including a newline-bearing residual token |
| No partial format, executable/object output or cache clear on rejection | Exact source bytes, artifact absence and private cache marker assertions |
| Compiler prefix versus program suffix | Real run argv output for implicit args and -- suffix containing compiler-option spellings; existing delimiter owners |
| Normal flags, output path, stage and help remain useful | Valid local-option CLI controls and existing bounded gate |
| Test harness acquisition, child failure and cleanup | Exclusive private fixture with immediate RAII cleanup; existing c_driver Process deadline and process-group kill/reap guard |
| Public specification and mirrors | Only getting-started English/ja guidance changes. Language semantics, std.cli, runtime ABI and library mirrors are unchanged. |

The existing c_driver target owns both parameterized residual_arguments tests;
its Process and ArtifactStage owners supply child and fixture cleanup. Linux and
macOS pass the new owners, and removing the admission gate makes the no-effect
owner fail on a silently accepted check suffix. The existing profile, target,
export and watch owners cover their unchanged routes. A pre-existing macOS
control-socket peer-close failure in the C-driver test route is independently
reproduced with the baseline main and is a separate capability boundary.

The author ledger-to-diff pass closes the cells above. Complete one fresh
independent full-diff review.
No benchmark is required: this is argument correctness with no performance promise.
