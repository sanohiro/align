# Compiler Pipeline

Defines the stages from `source.align` to executable, and the boundaries of the IR flowing between stages. **IR boundary = crate boundary** (`00-overview.md`). Each stage depends only on the previous stage's output and knows nothing of later stages.

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
  │  align_mir  lowering (desugaring) + analysis
  ▼
MIR                         backend-agnostic core. SIMD/fusion/arena fixed
  │  align_mir  optimization passes
  ▼
MIR (optimized)
  │  align_codegen_llvm
  ▼
LLVM IR → object
  │  align_driver  link (+ align_runtime)
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
- Absorbs syntax such as `:=` / `mut` / the `fn ... = expr` short form / struct literals / `?` / `else` / `arena {}` / `template` / `html` / `json` strings here.
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
  - `template` / `html` / `json` strings → `write_static` / `write_value` sequences (§13).
  - Array expression `a = (b+c)*d` → a fused loop that creates no temporary array (§9).
  - `map`/`where`/`sum` chains → fusion into a single loop.
  - `arena {}` → arena allocator allocate / bulk-free calls.
  - struct → SoA/AoS layout decision, field table generation (§14).
- allocation / error path / parallel unit (chunk) are held as **explicit nodes** in MIR (nothing hidden).

### MIR Optimization (`align_mir`)
- loop fusion, branchless lowering of `mask`, elimination of unnecessary clone/heap, const string pooling.
- Many lints (`draft.md` §16) reuse the results of this analysis for diagnostics.

### Codegen (`align_codegen_llvm`)
- MIR → LLVM IR. Maps `vecN<T>`/`maskN<T>` to LLVM's vector type / select and emits vector instructions deterministically.
- Arena allocation becomes runtime calls. Details in `05-backend-llvm.md`.

### Driver (`align_driver`)
- CLI. Calls the stages in order, links the object with `align_runtime`, and produces an executable.
- Subcommands (planned): `alignc build` / `alignc run` / `alignc check` (up to sema) / `alignc emit-mir` / `alignc emit-llvm`.

## Cross-cutting Crates

- `align_span`: file ID + byte offset range. Every IR node carries a span, and diagnostics point back into the original source.
- `align_diag`: types, display, and aggregation of multiple errors/warnings. Each stage continues as far as possible even on failure, accumulating diagnostics.

## The Path Driven First by the Skeleton (walking skeleton)

The minimal path driven in M0 (`07-roadmap.md`). Connects only the "trivial implementation" of each stage.

```align
fn main() -> i32 {
  x := 1
  return x
}
```

If this one program flows through lexer → parser → sema (types only) → MIR → LLVM → executable (exit code 1), the skeleton is complete. Subsequent features are plugged into all stages little by little.
