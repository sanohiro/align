# CLI finite floats and occurrence observations

Media AM-R04 and AM-R05 need finite scale flags and admission of absent,
explicitly empty and repeated options before model I/O. Extend the existing
std.cli builder and parsed owner. No second argv parser, automatic duplicate
rejection, positional grammar or application range policy is introduced.
All three consumer registers were reconciled: these two requests are media's;
voice and LLM have no conflicting CLI requirement. Their existing argument
handling and pins remain consumer-owned.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Registration | `c.flag_f64(name: str, default: f64) -> ()` registers a binary64 default with the existing bound command receiver, eager source-order evaluation and owned name copy. The default must have type f64, including inferred floating literals; f32 and integers reject. Registration can store any f64; parse validates finiteness as described below. Existing first-registration lookup for repeated names is unchanged. |
| Parsing | `c.parse(args: array<str>) -> Result<parsed, Error>` retains its signature, ownership and Pure effect. Before inspecting argv or allocating parsed entries, it rejects any registered nonfinite f64 default, even one overridden by argv or hidden by an earlier same-name registration. Then it scans argv left-to-right, skipping argv[0]. `--name value` and `--name=value` both accept f64. A separate value token is consumed verbatim even if it starts with `--`. Bool flags remain valueless. Unknown/missing/malformed input returns Error.Invalid and publishes no parsed owner; accumulated entries are dropped. No process, model or filesystem work occurs. |
| Number grammar | A complete ASCII JSON-number token: `-?(0\|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`. No surrounding whitespace, leading plus, leading zero, separators, hex, NaN, infinity, embedded NUL or trailing bytes. Reuse the existing JSON scanner and correctly rounded binary64 conversion. Overflow to a nonfinite value returns Invalid; underflow/subnormals and signed zero are accepted with round-to-nearest ties-to-even, independent of host rounding mode. Grammar applies to argv only, not the already-typed default. |
| Float lookup | `p.get_f64(name: str) -> f64` returns the last supplied value or default. It borrows p and name synchronously, allocates nothing, retains no input and returns an independent Copy scalar. An unregistered name or wrong kind aborts with the existing cli programmer-error policy. The receiver remains a bound parsed local. |
| Occurrences | `p.occurrences(name: str) -> i64` returns the number of explicit occurrences of that registered name in this successful parse, across both value spellings and for every flag kind. Defaults contribute zero; explicit empty strings contribute one; repeated bools each contribute one; value tokens are never counted as flags. Last-occurrence-wins values remain unchanged. Counts are nonnegative, use checked increments and return Invalid on impossible overflow before publication. Unknown names abort; there is no kind restriction. It has the same bound shared receiver, Pure effect, no allocation/retention and independent Copy result as get_i64. Each parse starts fresh. |
| Encoding and lifetimes | Names/default strings retain the existing length-delimited UTF-8 contract; embedded NUL in string names/values is not a terminator. Float tokens containing NUL reject. Parsed owns copied strings and count/value entries and outlives its get_str views; command and argv need not outlive parsed. New scalar getters create no borrow escaping p. Move, replacement, source nulling and deep Drop remain with existing opaque command/parsed owners. |
| Usage | Existing registration-order rendering adds `  --NAME  (f64, default: VALUE)\n`, using the runtime's existing Rust Display rendering strategy used by the usage sink. Nonfinite defaults can be rendered before parse rejects them. This adds no allocation beyond existing owned usage output and name/table storage. No human-text roundtrip or stable float display spelling is promised. |
| Native ABI | Add CliFlagF64 / `align_rt_cli_flag_f64`: `void(ptr command, ptr name, i64 len, double default)`; CliGetF64 / `align_rt_cli_get_f64`: `double(ptr parsed, ptr name, i64 len)`; CliOccurrences / `align_rt_cli_occurrences`: `i64(ptr parsed, ptr name, i64 len)`. Existing opaque handles keep their ABI; internal entries acquire counts. New operations use conservative IndirectStorage effects like sibling CLI keys, with no LLVM readonly promise on aborting getters. Native pointer/span preconditions and null receiver behavior follow corresponding flag_i64/get_i64 siblings. |
| Artifacts and owner | Sema validates exact operands; HIR replay/visitors and checked-HIR records include both scalar observations and F64 registration; MIR owns eager lowering, validation and keys; LLVM purely lowers exact signatures; runtime owns parsing, counts and storage. Existing compiler-bound cache identity and interface builtin types suffice: no public wire format, nominal type, interface tag, runtime reflection or ambient configuration is introduced. Whole/per-unit/imported/generic/cache paths require parity. |
| Scope and prerequisites | Existing CLI, binary64, JSON scanner and opaque-resource machinery only; no later milestone, K1 or aggregate-provenance extension. This one useful CLI admission capability closes both requests without repeating parser/ABI proof in separate producer/consumer PRs. No performance/resource improvement claim or benchmark is required. Consumer duplicate/range policy and adoption remain external. |

## Implementation closure matrix

| Axis | Implementation and discriminating owner |
| --- | --- |
| Formation and exact ABI | CliFlagKind::F64 and dedicated CliGetF64/CliOccurrences HIR/MIR variants; Gate-1 sweep, malformed-HIR CLI schema cases, runtime ABI declaration/export goldens and source type/arity/import/bound-receiver negatives in `cli_admission`. |
| Grammar, values and precedence | Runtime `cli_` parameterized owners cross separate/equals tokens, valid decimal/exponent/extreme/subnormal/signed-zero tokens, invalid syntax/UTF-8/NUL/nonfinite/overflow, finite/nonfinite defaults including duplicate registration, and partial failure followed by successful command reuse. Existing JSON numeric owners supply rounding proof; CLI pins exact f64 bits. |
| Counts and last value | Runtime and source owners cross bool/str/i64/f64, absent/present/repeated, explicit empty, mixed spellings, flag-like separate values, interleaved names, duplicate registrations and repeated parse independence. Parameterized count helper owner forces checked overflow without impossible argv allocation. |
| Usage and programmer errors | Extend `cli_usage_tests` complete-byte, extent and allocation owners with f64 signed zero, finite extremes and nonfinite defaults; retain source usage after parse error. Parameterized native source execution must abort for get_f64 unknown/wrong-kind names and occurrences unknown names. |
| Ownership and cleanup | Existing m10_cli get_str escape/clone and deep Drop owners remain. New source owners return Copy getters from a scope that drops parsed, transfer/replace opaque owners, retain independent parsed values after command/argv expiry and exercise partially parsed string cleanup on errors. Existing resource owner machinery owns source nulling and deep Drop; no new Move kind. |
| Control flow | Eager argument blocks that return/? before registration or getter action must terminate without malformed MIR or runtime action. Existing CLI parse match/? and opaque owner if/match/else/loop/return cleanup owners are reused; add focused f64/count branches and repeated parses. map_err carries existing parse Result with no new payload representation. |
| Receiver lifetime through eager operands | Author inspection found that existing CLI operations load their opaque receiver before later arguments, but do not reserve that owner: `command.flag_f64("scale", { command = cli.command("replacement"); 1.0 })` and `parsed.occurrences({ parsed = command.parse(args)?; "count" })` pass checking while releasing the loaded handle. Extend the existing exact native-action receiver reservation/retirement mechanism (used by file and timeout actions) to every CLI operation with later operands: all flag kinds, parse, all getters and occurrences. This is one shared CLI selector, with source and handcrafted-HIR move/replacement negatives across name/default/argv positions and branch/loop/early-exit controls. Ordinary replacement after a completed scalar observation stays valid; get_str still retains its original parsed lifetime. No pointer reload or evaluation-order change is permitted. |
| Generic/interface/cache | Imported generic forwarding used inside flag/getter arguments plus whole-program and per-unit native execution, cold and hit frontend cache replay in the CLI owner. HIR replay clones every new child; fresh/malformed HIR variants authenticate receiver, name/default/result types and access. |
| Documentation | Update draft.md, docs/language-spec.md, concise rationale in docs/design-notes.md, Settled in docs/open-questions.md, English/ja std-design/cli.md, plan19 checked-HIR and plan20 ABI records. One capability status in HANDOFF. Compile the existing CLI recipe through `m10_cli` and the new calls through `cli_admission`; audit new native signatures. |

The author ledger-to-matrix pass maps every new signature, grammar, ownership,
error, ABI and cache obligation above to these owners. No process-global native
state, persisted canonical format, runtime inspection table or new allocation
strategy is involved. The fresh independent adversarial plan review qualified this boundary before
implementation; one candidate code review and final-SHA preflight govern
publication. At merge, answer AM-R04/05 in the uncommitted consumer register and
run exactly `cargo build --release --workspace` with the local native environment.

## Plan qualification

The fresh independent plan review found two owner gaps: exact f64 usage rendering
and the new getter abort paths. The matrix now names both discriminating owners;
no strategy change was required. The complete visitor/ABI sweep, bilingual
contract and parameterized native/cache owners may exceed 1,000 changed
hand-written lines (estimated 1,200–1,500). Keeping both CLI admission operations
together avoids duplicate parser-storage, deep-Drop, malformed-HIR and ABI proof;
each request composes with the other's duplicate/finite admission at the real
consumer. There is no dormant producer seam to split.

The author-discovered receiver class was checked without executing the unsafe
program. A narrow independent review qualified reuse of MoveCheck's existing
reservation and retirement ordering. Tests additionally cross nested same-owner
CLI calls and same-expression replacement after scalar versus borrowed-string
observations; checked-HIR mutation must reject through production lowering.

## Author closure

The ledger-to-diff extraction maps exact signatures and receiver formation to
sema dispatch, checked-HIR native schema mutations, and `cli_exact_types_and_bound_receivers`;
every grammar/finite/default/count obligation maps to runtime `cli_admission_tests`.
`cli_usage_tests` owns complete bytes, extent and allocation; the driver abort
owner covers unknown/wrong-kind lookups. `cli_admission_executes_and_replays`
covers eager termination, nested noninvalidating calls, independent scalar
results, whole/per-unit/generic imports and cold/hit replay.
`cli_eager_operands_cannot_replace_or_move_receivers` discriminates the unsafe
pre-fix receiver behavior; `cli_receiver_lifetime_checked_hir_replay` changes a
same-typed receiver and exercises checked plus all four infallible lowerers.
The Gate-1 sibling sweep covers every existing CliGetI64 visitor; the ABI
registry/golden and native export audit close every new return/parameter ordinal.
Existing `m10_cli` owners retain borrowed get_str escape/clone and opaque Drop
coverage. No applicable matrix cell is deferred.
