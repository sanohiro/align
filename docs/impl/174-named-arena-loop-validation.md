# Named arena bindings in loop validation

The checked-HIR ledger requires a loop's `body_locals` range to equal every
lexically declared local in its body, excluding lifted-function locals. The
producer records that exact range. The independent body validator counts Let,
LetTuple and match bindings, but omits the handle declared by NamedArena. Even
`loop { arena region {}; break }` passes checking and fails the MIR boundary.
Anonymous arenas and a named arena enclosing the loop pass.

Count the existing NamedArena binding during the same iterative lexical walk.
Keep exact range equality, one declaration per id, handle type and mutability,
lexical scope, region escape and break-depth validation. This implements the
existing ledger without changing its safety strategy, ownership, IR or ABI.

## Implementation closure matrix

| Boundary | Implementation and owner |
| --- | --- |
| Declaration inventory | `loop_body_locals_valid` counts Let, LetTuple, match payload and NamedArena handles. Audit all `record_binding` callers. A parameterized HIR owner includes each kind, nested loops and lifted closures. |
| Exact malformed-input refusal | Keep range order/bounds/equality and duplicate counts. The HIR owner mutates omitted/excess ranges, duplicate/out-of-range handles and handle type/mutability; both lowering modes reject before producing MIR. Existing named-region owners retain ordinary-local, return and capture restrictions. |
| Control paths | Named arenas in statements, eager operands, branches, matches and nested loops count even in retained unreachable syntax. Native zero/one/repeated loops and early return/`?` retain ordinary behavior. Region-crossing break remains rejected. |
| Ownership and cleanup | No construction, move, source nulling, Drop, replacement or return lowering changes. Existing `fb_region` and `loop_expr` owners retain scope cleanup and per-iteration ownership; native named-loop execution checks the previously rejected path. |
| Generic and compilation modes | The same validator handles source and concrete imported/generic bodies. Native whole-program/per-unit and cache replay exercise a helper containing a named arena inside a loop. No interface identity or serialization change. |
| Runtime and performance | Arena allocation and cleanup retain existing producers and runtime ABI. This is a validation correction, with no allocation or performance promise requiring a benchmark. K1 and aggregate provenance remain deferred. |

The author performs the matrix-to-diff and normative-line extraction passes
before the one independent preflight code review. No separate strategy review
is needed: the exact lexical-local contract is already authoritative in plan19.

## Author closure

`named_arena_loop_binding_inventory_matches_all_lexical_declarations` covers
all four binding producers, nested scopes/loops, lifted-local isolation and
retained unreachable syntax. Its statement row fails on the pre-fix validator.
`named_arena_loop_inventory_rejects_inexact_or_forged_bindings` mutates ten
range/handle cells and requires every lowering entrypoint to publish no MIR.
`named_arena_loops` owns imported/generic native execution and cold/hit
materialization, with source scope/escape/break negatives. Existing `fb_region`
and `loop_expr` owners pass. The only production change counts the already
validated handle; all source, lifetime, control and lowering rules remain intact.
