# Compiler Pipeline

Defines the stages from `source.align` to executable, and the boundaries of the IR flowing between stages. **IR boundary = crate boundary** (`00-overview.md`). Each compiler stage depends only on earlier IR. The current driver runs this pipeline per reachable module, exchanges canonical `align_interface` summaries across unit boundaries, caches content-identified objects, and links them in import-DAG order.

## Overall Diagram

```text
reachable source (.align) + exact registered static inputs
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
Checked HIR + static contracts
                            safety-verified; Query/command static artifacts fixed
  │  align_mir  lowering (desugaring) + target-independent transforms
  ▼
MIR                         backend-agnostic core. ownership, fused pipelines,
                            vector operations, tasks, and par-map materialization fixed
  │  align_codegen_llvm
  ▼
LLVM IR → object
  │  align_driver  content-addressed object cache / parallel unit codegen
  ▼
per-unit objects + static artifacts
                            interfaces checked; deterministic DAG order
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
- Resolution of compiler-known static constructors. Only a resolved recognized callee with literal
  definition-time input may register a static file; a same-spelled local/user function cannot. A
  Query/command constructor must be the single whole expression body of a named zero-argument
  non-generic descriptor function, making module+item one unique artifact identity.

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
- Borrowed parameter modes, owner-generation invalidation, dependent resources, recursive tagged
  Drop plans, and named `region` substitution follow
  `17-library-boundary-prerequisites.md`; none dispatches on a database/std package name.

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
- Package resources become generic producer-owned Drop-thunk calls. Generated Query bind/decode thunks and immutable
  descriptors lower from MIR/static artifacts without runtime reflection.

### Driver (`align_driver`)
- CLI. Discovers the import DAG, builds/verifies unit interfaces, runs per-unit codegen through the
  default-on object cache (parallel by default), selects runtime capabilities, links, and atomically
  publishes the executable.
- On a cold source/import identity, import/name resolution first proves recognized static-input
  callees; only then does the driver resolve/read/hash exact safe `File` paths. An `Inline(query_id)`
  entry uses decoded literal bytes from the unit and never causes a file read. A later pre-frontend
  lookup may reuse only a versioned static-input manifest bound to that exact resolution digest, so
  shadowed/same-spelled calls never cause a file read.
- Emits versioned Query/command artifacts beside interfaces/objects. SQL bytes and generated thunks
  affect the producer implementation and link; only Params/Row/restriction/static semantics affect
  consumer interfaces.
- Shipped subcommands: `check`, `check-per-unit`, `emit-interface`, `emit-mir`, `emit-llvm`,
  `emit-obj`, `explain-opt`, `fmt`, `build`, `run`, `size`, and `cache clear`. Build controls include
  profiles/target CPUs, `-j`, cache stats, runtime LTO, ThinLTO, instrumented PGO, and the explicit
  foreground `--watch` loop.
- Build-performance item 6 has a settled, implementation-pending refinement for the explicit
  `--thin-lto` path: one support partition for producer-owned resource thunks plus one LLVM module
  per MIR function, followed by the same fresh global thin-link and cached import-sensitive
  backends. Non-root functions use unit-qualified hidden composition symbols, so duplicate
  consumer-side monomorphs remain distinct rather than becoming conflicting prevailing definitions.
  A sealed shared view fingerprints each unit's complete codegen tables once, then every function
  hash combines that fingerprint with its selected body and peer ABI/symbol declarations.
  This does not change the current flag-off, PGO, frontend-cache, `emit-llvm`, or `explain-opt`
  pipeline. The exact boundary is `21-build-perf-plan.md` item 6.
- Watch builds route every consumed source, import, PGO profile, file-backed static input, and
  checked metadata read through `align_watch`. That internal crate retains bounded semantic and
  filesystem-topology evidence for final publication checks and periodic native-event-backed
  audits. `align_driver` owns revision execution, captured linker/strip output, last-good atomic
  publication, signals, and the stderr protocol; external toolchain and runtime inputs remain
  restart-triggered rather than watched.

## Cross-cutting Crates

- `align_span`: file ID + byte offset range. Every IR node carries a span, and diagnostics point back into the original source.
- `align_diag`: types, display, and aggregation of multiple errors/warnings. Each stage continues as far as possible even on failure, accumulating diagnostics.
- `align_ast`: syntax shared without making sema depend on parser internals.
- `align_interface`: canonical public type/function/effect summaries and fail-closed decoding.
- `align_hash`: one deterministic hash implementation for language operations and artifact identity.
- `align_fmt`: formatting over source tokens with AST assistance; it preserves deliberate line layout.
- `align_watch`: bounded compiler-input observation, final semantic/topology revalidation, alias
  repair dependencies, and native registration identities for foreground watch builds.

## The Path Driven First by the Skeleton (walking skeleton)

The minimal path driven in M0 (`07-roadmap.md`). Connects only the "trivial implementation" of each stage.

```align
fn main() -> i32 {
  x := 1
  return x
}
```

If this one program flows through lexer → parser → sema (types only) → MIR → LLVM → executable (exit code 1), the skeleton is complete. Subsequent features are plugged into all stages little by little.
