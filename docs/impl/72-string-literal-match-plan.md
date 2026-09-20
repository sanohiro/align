# String literal patterns in `match`

Status: plan of record for
[issue 1085](https://github.com/sanohiro/align/issues/1085). The language
contract is complete; implementation starts only after one fresh independent
adversarial review of this ledger. Evidence baseline: Align `8863ecc2`, LLVM
22.1.8. The issue's client baseline is align-llm `b72b0b48` on Apple M1 with a
release build.

This capability completes the existing value-pattern family with exact string
literals. It adds no guard, binding, range, pattern object, hash semantics or
second ownership rule. The implementation is one PR because parsing and checked
representation without the dispatch consumer would publish dormant syntax,
while a dispatch record without the syntax would have no stable consumer. The
expected hand-written diff is below the roughly 1,000-line boundary.

## 1. Public-contract ledger

```text
Surface           match text {
                    "literal" => value,
                    "a" | "b" => value,
                    _ => fallback,
                  }

Exact domain      The scrutinee is str or string. Each pattern is one decoded
                  UTF-8 string-literal value. Matching is exact equality of the
                  decoded byte sequence and its byte length, including an empty
                  literal, non-ASCII UTF-8 and embedded NUL. Escape spellings
                  are not observable after lexing. Literal alternatives use the
                  existing `|` grammar and bind nothing.

Defaults          None. There is no implicit case folding, normalization,
                  prefix match, locale, regex, hash seed or ambient setting.
                  Source arms retain their written order for diagnostics and
                  body evaluation. Pattern selection has no side effect.

Errors            Every string match requires a `_` arm because the domain is
                  infinite. A repeated decoded byte sequence is a duplicate,
                  including duplicates within one or-pattern or across arms.
                  A string range (`"a"..="z"`) is rejected: ranges remain an
                  integer/char facility. Integer or char patterns in a text
                  match, and string patterns in an integer/char match, receive
                  the ordinary pattern-domain mismatch. Variant patterns remain
                  invalid for a text scrutinee. A second `_`, or a literal after
                  `_`, keeps the existing duplicate/unreachable diagnostics.

Validation order  Lex and parse validity first; then scrutinee formation and
                  type; then patterns in source/alternative order. For each
                  pattern, domain compatibility precedes range-form rejection,
                  which precedes duplicate comparison. Wildcard reachability is
                  checked at its source position. Exhaustiveness is checked
                  after all arms. Arm bodies retain the existing checking and
                  result-type unification order, so a pattern error does not
                  invent a different body contract.

Ownership         The scrutinee expression evaluates exactly once. Selection
                  shared-borrows its two-word text view and never moves a local
                  string. A local string remains live and owned after the
                  match. A reached temporary string remains live through the
                  dispatch and is dropped exactly once after selection and
                  before its arm body, including an arm that returns, breaks,
                  propagates with `?`, traps or otherwise diverges. No arm binds
                  the text or extends the dispatch borrow.

Lifetime          The borrow lasts only through pattern selection. Existing
                  source lifetime and owner-generation rules validate the
                  reached scrutinee before MIR construction. No view is returned
                  or retained by the pattern operation.

Allocation        Pattern dispatch allocates, clones and copies no string.
                  Literal bytes use the compiler's existing immutable string
                  constants. Evaluating the scrutinee may perform whatever
                  visible work that expression already specifies; dispatch adds
                  none.

Effects           Pattern selection is Pure. Scrutinee and selected-arm effects
                  retain their ordinary source order. Unselected bodies do not
                  execute.

Owner             align_lexer owns escape decoding; align_parser/align_ast own
                  syntax; align_sema owns domain, duplicate, exhaustiveness and
                  one-borrow checking; align_mir owns exact literal-to-target
                  dispatch semantics and temporary cleanup; align_codegen_llvm
                  performs only the deterministic lowering below.

Prerequisite      Existing string literals and exact str/string `==`, existing
                  value-pattern parsing, and runtime ABI row A01
                  `align_rt_str_eq`. No runtime, package or milestone
                  prerequisite is added.

Artifact/cache    `LiteralPat` gains `Str(String)` and `HirValuePattern` gains
                  `Str(String)`. A generic public body continues to travel as
                  source and is re-parsed by the consumer; non-generic public
                  interfaces contain signatures only. No interface-format tag
                  or runtime ABI row changes. The compiler build identity and
                  ordinary source/object keys invalidate artifacts containing
                  the new syntax. Literal identity is structural over the exact
                  decoded bytes; type identity remains unchanged.

Acceptance        Sections 3 and 4. The provider acceptance corpus owns the
                  language and lowering guarantees. align-llm rewrites and
                  measurements remain consumer-owned and do not block the Align
                  implementation merge.

Benchmark         A local vocabulary-scan kernel records N=4, 8 and 29 distinct
                  literals, miss-heavy and hit cases, with equal-length and
                  shared-prefix sets. Record elapsed time, dispatch count and
                  full-comparison count against an `if`/`==` ladder under the
                  same release compiler. This measures the issue's explicit
                  performance claim but is not a correctness gate and makes no
                  absolute latency or CPU-wide promise.

Mirrors           draft.md, docs/language-spec.md, docs/design-notes.md,
                  docs/history.md, docs/open-questions.md, this plan and
                  HANDOFF.md.
```

No public declaration syntax is added. The examples above are call-site
expressions, not declarations. Pattern literals use the settled single-line
string escape set. Source UTF-8 and escapes pass through the existing lexer, so
duplicate checking observes decoded values: for example `"\x41"` is not a
supported alternate spelling, while two supported spellings that decode to the
same contents are one duplicate. Embedded NUL is an ordinary byte because Align
strings are length-counted; it is rejected only by APIs whose own native-boundary
contract forbids it.

## 2. Exact compiler records and lowering

The AST retains the existing `ValuePattern` shape:

```text
LiteralPat = Int(i128) | Char(u32) | Str(String)
ValuePattern = Single(LiteralPat, Span) |
               Range { start: LiteralPat, end: LiteralPat, span: Span }
```

`LiteralPat` is no longer `Copy`. The parser admits `TokKind::Str` at the same
value-pattern gate as integers and characters. It parses a following `..=` or
`..` into the ordinary range form so sema can issue the domain-specific string
range diagnostic rather than a cascading arm-syntax error.

Checked HIR is:

```text
HirValuePattern = Single(i128) | Range(i128, i128) | Str(String)
```

The first two variants and their integer/char meaning remain byte-for-byte
unchanged. A text arm contains only `Str`; a wildcard still has an empty values
vector. Checked-HIR validation independently replays the scrutinee domain,
wildcard requirement, decoded-byte uniqueness, absence of string ranges and
absence of bindings/variant tags. Malformed mixtures reject before MIR.

MIR adds one semantic terminator:

```text
StrMatch {
  scrutinee: Operand<str|string>,
  cases: [(String, target BlockId)],
  otherwise: BlockId,
}
```

Cases are stored once in source/alternative order; several cases may target one
or-pattern arm. Each `String` is the already-decoded literal, so invalid UTF-8
is unrepresentable in this record. The terminator means exact length-and-byte equality against each
unique case and otherwise branches to the required wildcard. It reads but does
not consume the operand. MIR validation requires a live `str`/`string` operand,
at least one case, unique decoded values, valid targets, and no case equal to
another. The ordinary zero-case source form lowers directly
to the wildcard and creates no terminator. CFG successor, liveness, cloning,
printing, optimization and malformed-input sweeps must enumerate the new
terminator; an unknown or invalid record cannot reach LLVM.

LLVM lowering is deterministic and does not rediscover language semantics:

1. Load the scrutinee byte length once and emit a switch whose cases are the
   distinct literal byte lengths. A miss branches to `otherwise`.
2. Within a same-length group, recursively select an unused byte offset with the
   greatest number of distinct candidate byte values; ties choose the lowest
   offset. Load only an offset proved in bounds by the selected length case and
   switch on that byte. A missing byte value branches to `otherwise`.
3. At a one-candidate leaf, call the existing exact `align_rt_str_eq` once and
   branch to that case's target on true or `otherwise` on false. The empty-string
   leaf follows the same rule; no byte load occurs.

For one to three cases, codegen may retain source-ordered exact comparisons.
For four or more distinct cases it must use the length/byte tree above. Thus an
N-case text match with N >= 4 executes at most one full-string comparison on any
path. This is the precise O(1) comparison promise: byte-discriminator depth can
grow with the longest literal and the plan does not claim constant total
instructions or constant compile time. No perfect hash, hash collision rule,
hash seed or new runtime symbol is introduced.

The raw LLVM structural owner pins the length switch and guarded byte loads.
The optimized-IR owner pins that every entry-to-arm/default path reaches at most
one `align_rt_str_eq`, even if LLVM folds a one-case switch into a comparison or
inlines A01 under `--rt-lto`. It also proves all byte loads are dominated by the
matching length edge. Literal targets, not comparison order, determine the
selected body, so the tree may reorder pure pattern tests without reordering
source diagnostics or arm effects.

## 3. Implementation closure matrix

| Cell | Implementation and owner evidence |
| --- | --- |
| Formation and parsing | Parser/AST owners for a single literal, `|`, empty/non-ASCII/NUL escapes, string ranges reaching the intended diagnostic, malformed separators and recovery. Formatter round-trip preserves the existing literal spelling policy. |
| Type and diagnostics | Sema owners for `str` and `string`; required `_`; duplicate decoded values within/across arms; string range; both mixed-domain directions; variant pattern; duplicate/unreachable wildcard; result-type unification. Diagnostics are pinned once without cascades. |
| Construction and evaluation | Driver owner with a side-effecting scrutinee proves one evaluation. Local owned string survives the match. A returned heap string, arena string and early/diverging selected arms prove one reached cleanup and no unselected cleanup. |
| Move-in/out, source nulling, Drop, replacement and return | Dispatch never moves or nulls a local owner. Replacement cannot overlap the selection borrow. Temporary Drop precedes the chosen body and occurs on ordinary result, return, `?`, `map_err`, loop `break` and trap paths. No pattern value has Drop. |
| Control-flow product | Selected bodies cover block/`if`/nested `match`/`else`/`?`/`map_err`, branch and loop joins, early return and loop-local `break`. The new terminator participates in every CFG successor and liveness authority. |
| Lowering shape | N=4, 8 and 29, with unique lengths, one equal-length group, shared prefixes, empty/non-ASCII/NUL literals, hit of every arm and miss. Raw LLVM has the length switch; each optimized path has at most one full comparison and every byte load is length-dominated. |
| Ownership and allocation | Raw/optimized MIR and LLVM assert no `str_clone`, heap/arena allocation, materialized candidate string or hidden scrutinee copy for a borrowed view. A `string` local has one unchanged cleanup owner. |
| Existing domains | Run the #1055 integer/char/range owner unchanged and compare its relevant MIR/LLVM goldens byte-for-byte. Enum, Option and Result matching retain their owners. |
| Generic, interface and cache | Local and imported generic functions reparse the source pattern; whole-program and per-unit results agree. Edit/revert changes and restores ordinary source/interface/object keys without a format-version change. |
| Malformed records | Checked-HIR and MIR negative owners cover wrong scrutinee type, mixed pattern variants, duplicate decoded values, missing wildcard/default, bad targets and stale CFG metadata before LLVM. Invalid UTF-8 is unrepresentable in the owned `String` case record. |
| Runtime and targets | A01's ABI/effects record is unchanged. Default and `--rt-lto` builds preserve semantics; x86-64 and AArch64 owners inspect equivalent length/byte decisions without a target-specific source promise. |
| Performance evidence | Release benchmark described in §1, recording the compiler SHA, target, corpus and command. It supports only the full-comparison claim; client-wide profiling remains external. |

One parameterized owner may close several rows. Reuse an existing owner when it
would fail for the changed defect. The implementation author performs one
matrix-to-diff pass before the fresh full-diff review. Because this change adds
an HIR variant and a MIR terminator, the align-self-review variant sweep is
mandatory before the implementation PR.

## 4. Acceptance and delivery

Provider acceptance is:

1. `match text { "a" => ..., "b" | "c" => ..., _ => ... }` type-checks and
   executes identically for `str` and `string`; all diagnostics and decoded-byte
   cases in §3 are pinned.
2. The scrutinee evaluates once and selection adds no clone, allocation, copy,
   move or source nulling. Owned temporaries clean up exactly once on every
   reached exit.
3. N=4, 8 and 29 owners prove the deterministic length/byte tree, safe loads and
   at most one full equality comparison per path, including equal lengths and
   shared prefixes.
4. Existing integer, char, range, variant, Option and Result behavior is
   unchanged; malformed checked records reject before LLVM.
5. The local release benchmark records its reproducible baseline and result.

Implementation is one capability PR after this plan's independent review. Run
the parser/sema/MIR/driver owners, the unchanged #1055 owner, the benchmark, the
align-self-review skill and the normal code-tier preflight. CI is confirmation,
not the discovery loop.

The external align-llm acceptance remains pending after the provider merge:
rewrite `alignpack$role_id` and `tokenizer_qwen2$eog_text`, verify tokenizer
smoke output and pack digests byte-for-byte, record the whole-image comparison
count and re-profile the tokenizer path. Align reports its merged surface and
limits in the request register but does not modify the consumer implementation.
