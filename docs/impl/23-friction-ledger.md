# The expressiveness friction ledger

Align is expressive by restriction. Most of what the language refuses is refused
on purpose, and the refusals are what let the compiler infer contiguity,
no-alias, non-null, and region facts without lifetime syntax. This document
exists so that a restriction can be *re-examined* without the re-examination
being an argument about taste.

## Purpose

Two things get confused when someone says "I could not express this":

- the design permits it and the implementation has not caught up, or
- the design refuses it.

The first is ordinary work. The second is a settled decision, and reopening a
settled decision must be an evidence-driven act. This ledger separates the two
and accumulates the evidence for the second.

**Feel is not evidence. Hypothetical code is not evidence.** A restriction is
re-examined on the strength of workarounds that exist in real programs, counted
where they sit. Nothing else enters the ledger.

## Category A versus Category B

| | Category A — implementation gap | Category B — deliberate restriction |
|---|---|---|
| What it means | The design allows the program; the compiler does not accept it yet | The design refuses the program |
| Where it goes | The owning plan's backlog, `docs/impl/07-roadmap.md`, or `docs/open-questions.md` as DEFERRED | This ledger |
| How it closes | Ordinary implementation work | Only through the reopen protocol below |
| Threshold process | Does not apply | Applies |

Classifying an item as A is not a promise to do it soon; it is a statement that
no decision has to change for it to be done. Classifying an item as B is not a
refusal to ever discuss it; it is a statement that discussing it requires
evidence.

### Known Category A items

These are recorded here only so the ledger's boundary is legible. They are owned
elsewhere and are **not** ledger rows.

| Item | Owner of record | Status |
|---|---|---|
| A value-carrying `if`/`else` expression cannot move an already-bound owned local out of an arm. Every sibling form — a `match` arm, an `else`-unwrap fallback, a block tail, a statement-form `if` + `return` — already does this correctly. | `docs/open-questions.md`, "A value-carrying `if` expression cannot move a bound owned local (DEFERRED, recorded 2026-08-13)"; spec text in `docs/language-spec.md` Memory and `draft.md` §6.3 | DEFERRED. Closing it is a sema/MIR change mirroring the working `match` join, plus a diagnostic reword. |
| A `sort_by_key` key must be a **Copy** `Ord` value. An owned `string` key type-checks and is then rejected at the MIR boundary as an internal error, because the fused sort path has no per-key Drop. | `docs/impl/19-hir-validation-ledger.md`; spec text in `docs/language-spec.md` and `draft.md` | DEFERRED. The restriction is a missing capability in the fused sort path, not a decision about keys. |

Both are A because the design already describes the working behavior; only the
implementation is short. Neither belongs in the reopen protocol.

## Ledger schema

One row per restriction. A row accumulates occurrences over time.

| Field | Meaning |
|---|---|
| Restriction | The refused shape, in one line |
| Settled reference | The exact `docs/open-questions.md` item (or spec line) that settled it |
| Workaround shape | The **mechanical** rewrite a programmer performs instead |
| Occurrences | `file:line` plus the corpus the file belongs to |
| Count | Sites, and how many independent programs they span |

**Only mechanical workarounds are admissible.** A mechanical workaround is one
where the rewrite is determined by the restriction: given the refused program,
any competent author produces substantially the same replacement. If avoiding
the restriction required a judgement call — a different data layout, a different
algorithm, a different module boundary — that is a design problem in the program,
not friction in the language, and it does not enter the ledger.

This rule is what keeps the ledger from becoming a complaints file. It also
means a high count is not by itself a verdict: a count says the shape recurs,
and a human still has to read the sites.

## Reopen protocol

A proposal to re-examine a settled decision is admissible when its ledger row
records **at least five occurrences across at least two independent real
programs**. Two files in one package are not two programs.

Reaching the threshold does not change a decision, and does not create a
presumption that it should change. It makes the proposal *admissible*. The
re-examination itself runs through the normal machinery with nothing relaxed:
the `docs/open-questions.md` procedure for moving an item out of Settled, and
the design gate in `CLAUDE.md` for whatever replaces it.

Below the threshold, the answer to "can we reopen this?" is to keep building and
let the ledger fill, or to accept the restriction.

## Admissible widening shape

This section governs *what a change may look like* if a restriction is ever
widened. It applies before any proposal is worth writing: a request that cannot
take this shape is refused regardless of how much evidence it accumulates.

A widening is admissible only when it is:

- **structural** — determined by the shape of the data, not by a declaration;
- **compiler-derived** — the compiler produces it, the programmer does not write it;
- **non-customizable** — there is no hook, override, or per-type variation;
- **specified exactly** — the specification fixes one behavior, so two
  implementations cannot differ and a reader never has to look anything up.

Two illustrations of the shape, neither of which is a commitment to ship:

- Struct `==` derived as the structural comparison of every field, with no way
  to opt out, opt in, or alter it.
- Struct `print` emitting a fixed structural dump, with the format pinned by the
  specification rather than chosen at the call site.

The following are refused permanently, and evidence does not apply to them:
traits or interfaces for operator behavior, macros, operator overloading, and
format specifiers or any other per-call-site formatting control. Each one moves
behavior from the shape of the data to a declaration somewhere else, which is
the property the restrictions exist to prevent.

**A request that cannot be expressed in the admissible shape is refused, and the
refusal is recorded as a row with status `refused`.** Recording it is the point:
it stops the same request from being re-litigated as though it had never been
answered, and it keeps the reason attached to the request.

## Corpus

Evidence is drawn from real Align source that someone wrote for a reason:

- `examples/`
- `apps/` — the `pkg.db` and `pkg.web` applications
- `bench/` — the `.align` kernels
- `../align-llm/docs/align-requests.md`, the request register for the external
  align-llm client
- REPL sessions users report, when reported

**Nothing is collected automatically and no telemetry exists.** Rows are added by
someone reading code and writing the row. This is slower than instrumentation
and it is the only option consistent with nothing hidden: a language that
refuses to hide allocation does not get to quietly measure its users.

The align-llm implementation is expected to be the main future source, because
it is the largest Align program being written against the language by someone
who did not design it.

## Ledger

Seeded 2026-08-16 by scanning the 134 `.align` files then present in
`examples/`, `apps/`, and `bench/`. Counts are measured, not estimated. A row
with no occurrences stays in the table with a count of zero — the absence is
evidence too.

### B1 — `==` on aggregates

| | |
|---|---|
| Restriction | `==` supports scalars and strings only; two struct values cannot be compared |
| Settled reference | `CLAUDE.md` locked decisions; `docs/language-spec.md` ("Aggregates have no order, exactly as they have no `==`") |
| Workaround shape | `a.f == b.f && a.g == b.g && …` over every field of two instances of one struct |
| Occurrences | none |
| Count | **0 sites, 0 programs** |

The scan matched the two-instance shape `X.f == Y.f` specifically. The corpus
does contain many `==`/`&&` chains, but they validate independent scalars —
header fields, protocol constants — and are not struct comparisons. They are not
occurrences and are not counted.

### B2 — `print` of an aggregate

| | |
|---|---|
| Restriction | Printing supports primitives only; a struct cannot be printed |
| Settled reference | `CLAUDE.md` locked decisions; `docs/language-spec.md` |
| Workaround shape | A run of consecutive `print(x.field)` calls covering every field of one receiver |
| Occurrences | `examples/point.align:9-10` (`Point`, 2/2 fields) · `bench/pkg_db_sqlite_callbacks/kernel.align:78-80` (`Timed`, 3/3) · `bench/pkg_db_postgres_binary/kernel.align:109-112` (`Probe`, 4/4) · `bench/pkg_db_dynamic/kernel.align:212-216` (`Probe`, 5/5) |
| Count | **4 sites, 4 files** |

Every site prints *all* of its struct's fields in declaration order, which is
what makes it mechanical rather than selective. Three of the four are `emit`-style
functions in benchmark kernels whose only job is to dump a result record.

Below the reopen threshold on sites (4 of 5). Whether the three benchmark
kernels count as three independent programs is a judgement for whoever writes a
proposal; they share an author and a purpose.

### B3 — one `loop`, no `for`/`while`

| | |
|---|---|
| Restriction | Sequential control is one `loop` expression with value-carrying `break`; there is no `for`, `while`, `continue`, or labels |
| Settled reference | `CLAUDE.md` locked decisions; `docs/open-questions.md` |
| Workaround shape | `mut i := 0` · `loop { if i >= n { break } … i = i + 1 }` |
| Occurrences | 20 files, concentrated in `apps/db/pkg/db*` and `apps/web/pkg/web*` |
| Count | **150 sites, 20 files** |

Recorded because the count is real and the shape is mechanical. It is also the
clearest demonstration of why this ledger does not treat a count as a verdict.

The counted `loop` is the *intended* form for a control circle; array and slice
work is meant to go through pipelines (`map`, `where`, `sum`), which carry no
such boilerplate. Most of these 150 sites are byte-level protocol and parser code
in `pkg.db` and `pkg.web` where no pipeline applies, so the count largely
measures how much low-level code those two packages contain, not how often the
language got in someone's way.

A proposal built on this row would have to show that the sites are ones a
pipeline should have covered. The count alone does not show that, and this row
should not be cited as though it did.

## Maintaining this file

- Add a row the first time a restriction produces a mechanical workaround in real
  code. Record zero-count rows for restrictions that were looked for and not
  found; that is a measurement.
- Extend an existing row with new `file:line` occurrences as they appear. Do not
  restate the restriction in a second row.
- Record a refusal as a row with status `refused` when a request cannot take the
  admissible widening shape, with the reason attached.
- Do not adjust a count without rerunning the scan that produced it, and say
  when it was last run.
