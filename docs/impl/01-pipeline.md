# Compiler Pipeline

Defines the stages from `source.align` to executable, and the boundaries of the IR flowing between stages. **IR boundary = crate boundary** (`00-overview.md`). Each compiler stage depends only on earlier IR. The current driver runs this pipeline per reachable module, exchanges canonical `align_interface` summaries across unit boundaries, caches content-identified objects, and links them in import-DAG order.

## Overall Diagram

```text
source (.align)
  │  align_lexer
  ▼
Tokens                      positioned token stream
  │  align_parser
  ▼
AST                         syntax tree. Before semantic analysis. With spans
  │  align_sema (1) name resolution / module resolution
  ▼
Resolved AST                references bound to definitions
  │  align_sema (2) type inference / type checking
  ▼
Typed HIR                   high-level IR with types on every expression
  │  align_sema (3) move checking / arena escape checking
  ▼
Checked HIR                 safety-verified. If it passes here it is safe
  │  align_mir  lowering (desugaring) + target-independent transforms
  ▼
MIR                         backend-agnostic core. ownership, fused pipelines,
                            vector operations, tasks, and par-map materialization fixed
  │  align_codegen_llvm
  ▼
LLVM IR → object
  │  align_driver  content-addressed object cache / parallel unit codegen
  ▼
per-unit objects             interfaces checked; deterministic DAG order
  │  align_driver  link (+ capability-selected align_runtime components)
  ▼
executable
```

## Stage Responsibilities

### Lexer (`align_lexer`)
- Input byte stream → token stream.
- The **compile-time meta** of string literals (`draft.md` §12: len / hash / ascii / utf8_valid / whether escaping is needed) is computed once here and attached to the token.
- Blocks are `{}`; indentation is not significant (non-Python). Statement termination is **Go style**: a newline is an implicit terminator, and `;` is an optional separator for cramming onto one line. The lexer decides "if the end-of-line token can end a statement, insert an implicit `;` at the newline; but if the next line starts with `.`/a binary operator, it is a continuation of the previous line."

### Parser (`align_parser`)
- Token stream → AST. With error recovery (reports multiple errors within one file).
- Absorbs syntax such as `:=` / `mut` / the `fn ... = expr` short form / struct literals / `?` / `else` / `loop` / `arena {}` / `task_group {}` / `unsafe {}` / plain `template` strings here. The spec's `html` / `raw` / JSON-template variants remain deferred.
- **No desugaring.** Expansion of `?` and `template` is the MIR stage. The AST is kept as written (the lint uses the AST; the formatter is token-driven with AST *assist* — it re-emits the original token text and recovers comments/newlines from source spans, consulting the AST only to disambiguate `<>`/unary spacing; see `open-questions.md` "Formatter").

### Sema (1) Name Resolution (`align_sema`)
- Resolution of `module` / `import`, symbol table construction, binding references → definitions.
- Visibility (`pub`) checking.

### Sema (2) Type Inference / Type Checking (`align_sema`)
- Local type inference (deciding the type of `x := 10`) and reconciliation with annotations.
- Typing of `Option<T>` / `Result<T,E>` / `array<T>` / `slice<T>` / `vecN<T>` / `maskN<T>`.
- Checking that the `?` operator applies only to `Result`.
- Typing of array-operation chains (`map`/`where`/`sum` ...). Details in `03-types.md`.

### Sema (3) Move Checking / Arena Escape Checking (`align_sema`)
- Make use-after-move of owning types a **compile error** (`draft.md` §6.3).
- Check that a view allocated inside `arena {}` does not leak outside the arena (§6.4, §15).
- Check that a function passed to `par_map` does not mutate external mutable state (§11).
- Check the no-alias constraint on `out` arguments (§7).
- **No lifetime annotations are required.** Lifetime violations are detected by flow analysis (`03-types.md`).

### MIR Generation (`align_mir`)
- This is where **desugaring** first happens. Details in `04-mir.md`.
  - `?` → early return + cold error path branch.
  - plain `template` strings → `write_static` / `write_value` sequences (§13).
  - Array expression `a = (b+c)*d` → a fused loop that creates no temporary array (§9).
  - `map`/`where`/`sum` chains → fusion into a single loop.
  - `arena {}` → arena allocator allocate / bulk-free calls.
  - explicit `to_soa` / SoA and grouped operations → column layout and aggregation nodes (§14).
- allocation/drop, task-group operations, and parallel-map materialization are held as
  **explicit nodes** in MIR (nothing hidden). Control flow, including error paths and loops, uses
  ordinary blocks and branches.

### MIR Optimization model (`align_mir`)
- pipeline fusion, guarded mask/select lowering, explicit SIMD shapes, and constant/string pooling
  are target-independent. Some are performed while constructing MIR rather than by a separately
  scheduled MIR-to-MIR pass; `emit-mir` is the concrete truth.
- Structural performance lints run in sema; LLVM optimization remarks are exposed separately by `alignc explain-opt`.

### Codegen (`align_codegen_llvm`)
- MIR → LLVM IR. Maps `vecN<T>`/`maskN<T>` to LLVM's vector type / select and emits vector instructions deterministically.
- Arena allocation becomes runtime calls. Details in `05-backend-llvm.md`.

### Driver (`align_driver`)
- CLI. Discovers the import DAG, builds/verifies unit interfaces, runs per-unit codegen through the
  default-on object cache (parallel by default), selects runtime capabilities, links, and atomically
  publishes the executable.
- Shipped subcommands: `check`, `check-per-unit`, `emit-interface`, `emit-mir`, `emit-llvm`,
  `emit-obj`, `explain-opt`, `fmt`, `build`, `run`, `size`, and `cache clear`. Build controls include
  profiles/target CPUs, `-j`, cache stats, runtime LTO, ThinLTO, and instrumented PGO.

## Cross-cutting Crates

- `align_span`: file ID + byte offset range. Every IR node carries a span, and diagnostics point back into the original source.
- `align_diag`: types, display, and aggregation of multiple errors/warnings. Each stage continues as far as possible even on failure, accumulating diagnostics.
- `align_ast`: syntax shared without making sema depend on parser internals.
- `align_interface`: canonical public type/function/effect summaries and fail-closed decoding.
- `align_hash`: one deterministic hash implementation for language operations and artifact identity.
- `align_fmt`: formatting over source tokens with AST assistance; it preserves deliberate line layout.

## The Path Driven First by the Skeleton (walking skeleton)

The minimal path driven in M0 (`07-roadmap.md`). Connects only the "trivial implementation" of each stage.

```align
fn main() -> i32 {
  x := 1
  return x
}
```

If this one program flows through lexer → parser → sema (types only) → MIR → LLVM → executable (exit code 1), the skeleton is complete. Subsequent features are plugged into all stages little by little.
