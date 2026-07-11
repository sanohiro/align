# Clang-IR comparison harness (M13 Slice V (c))

Compiles semantically-equal Align and C kernels through the **same LLVM 19** and diffs the
load-bearing shape of the *optimized* IR. The question it answers: **does Align's declarative,
data-oriented pipeline lower to the same optimized vector code as idiomatic C, given the same
backend?** Divergences are *findings* (future optimization leads), not failures.

```sh
bench/clang_ir_compare/run.sh                 # build alignc, compare all kernels at x86-64-v3
ALIGNC=target/release/alignc run.sh           # reuse a prebuilt alignc (skip cargo build)
CPU=x86-64-v2 bench/clang_ir_compare/run.sh   # widths track the tier (2 lanes instead of 4)
```

Needs `clang-19` (same major version as the compiler's LLVM). Absent → the harness prints a skip
line and exits 0.

## How it works

- Each kernel is a pair `kernels/kNN_name.{align,c}` — the C is the hand-written equivalent of the
  Align pipeline. Both are compiled at the **same `--target-cpu` / `-march`** (default
  `x86-64-v3`, AVX2, 256-bit): Align via `alignc emit-llvm --stage optimized` (the `-O2` middle
  end), C via `clang-19 -O2`.
- The harness extracts four toolchain-neutral facts from each optimized IR and tabulates them
  side by side with a MATCH/DIVERGE verdict on the load-bearing three (vec / width / reduce):
  - **vec** — did the pipeline *loop* vectorize? = a horizontal `llvm.vector.reduce.*` intrinsic,
    or a vectorized store with an **SSA** operand (`store <N x ..> %..`). The SSA test deliberately
    excludes the constant literal-array init that Align's `main` vectorizes
    (`store <N x ..> <i64 ...>`) — that is not a loop fact.
  - **width** — widest integer vector lane count (4 at v3, 2 at v2).
  - **reduce** — the set of horizontal-reduction intrinsics (identical spelling in both toolchains).
  - **memcheck** — count of the `vector.memcheck` runtime overlap guard. *Caveat below.*

## Recorded baseline (probed 2026-07-11, LLVM 19, x86-64-v3)

| kernel        | Align (vec / width / reduce / memcheck) | clang (vec / width / reduce / memcheck) | shape |
|---------------|-----------------------------------------|-----------------------------------------|-------|
| k1_map_sum    | yes / 4 / add / 0                       | yes / 4 / add / 0                       | MATCH |
| k2_where_sum  | yes / 4 / add / 0                       | yes / 4 / add / 0                       | MATCH |
| k3_map_into   | yes / 4 / -   / 0                        | yes / 4 / -   / 0                        | MATCH |
| k4_hash_fold  | no  / - / -   / 0                        | no  / - / -   / 0                        | MATCH |
| k5_scan       | no  / - / -   / 0                        | no  / - / -   / 0                        | MATCH |

**Headline:** on all five kernels, Align's optimized vector *shape* matches idiomatic C compiled
with the same LLVM. The two vectorizing reductions (k1, k2) produce the same 256-bit
`llvm.vector.reduce.add`; the masked `where` (k2) if-converts to a `<4 x i1>` mask on both sides;
the two-slice materialize (k3) vectorizes the store loop on both sides with **no** runtime overlap
guard; and both loop-carried kernels (k4 hash recurrence, k5 prefix scan) stay scalar on both
sides. Widths track the tier together (drop to 2 lanes at `x86-64-v2`).

## Divergences recorded (findings — not fixed here)

These do **not** change the MATCH verdict (same vectorization, width, reduction), but are the leads
this harness exists to surface:

1. **Loop interleave factor.** At equal width and equal reduction, clang emits more vector
   arithmetic per loop body than Align — it unrolls/interleaves the vector loop more aggressively
   (k1: ~11 vs ~4 vector-arith ops; k2: ~7 vs ~3). Same correctness and same width; a *throughput*
   difference (interleaving raises ILP on wide out-of-order cores). A future optimization lead:
   Align relies on the stock `default<O2>` interleaver — revisit if a bench shows interleaving
   would win on a hot reduction. Not a vectorization-quality gap.
2. **IR block naming (extraction hazard, not a codegen difference).** Align retains LLVM's *named*
   vectorizer blocks (`vector.body`, `vector.memcheck`); clang `-O2` emits *numbered* blocks. So a
   clang runtime overlap guard can be present without the literal `vector.memcheck` string. **Trust
   the `memcheck` column for Align; read it as a lower bound for clang.** Consequence: the
   string-based assertions in `vectorize_shapes.rs` are valid *within Align's own IR* but do **not**
   transfer to clang — the cross-toolchain signals must be semantic (`reduce`, `<N x>` widths),
   which is what this harness keys on.
3. **No-alias for free on k3.** The C twin uses `restrict` to be *semantically equal* to Align's
   `out dst` (a distinct region by the type system) — and then both omit the memcheck. Drop
   `restrict` from `k3_map_into.c` and clang re-introduces a runtime overlap guard; Align never
   needs one, because `out` proves distinctness at the type level rather than at run time. A point
   in Align's favor, recorded as the reason the k3 memchecks match.

## Note on the literal-init store

Every Align kernel builds its input from a 16-element `i64` array literal in `main`, which LLVM
vectorizes into constant `store <4 x i64> <i64 ...>` groups. That is a one-time init, not a loop —
the harness filters it out by requiring an **SSA** store operand for the `vec` verdict, so k4/k5
correctly read `no` despite those constant stores. The C twins take a pointer parameter and have no
such init.

One residual caveat on the `width` column: it reports the widest `<N x i64>` seen anywhere in the
function, including the constant literal-init store groups. For the current kernels the loop width
equals the init width at both tiers, so it reads correctly; a future kernel whose init is wider
than its loop body would misreport — key on the reduction intrinsic's width in that case.
