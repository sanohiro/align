# The toolchain: alignc, the formatter, the lints

> 🌐 **English** · [Japanese](./ja/16-toolchain.md)

One binary, `alignc`, carries the compiler, runner, formatter, cache controls, and inspection tools. A multi-file program starts at one entry file; imports form the build graph, so there is still no separate build-file dialect.

## The commands you'll actually use

```text
alignc check file.align         # whole-program parse + typecheck + lints
alignc run   file.align [args…] # build + execute; trailing args → main(args)
alignc build file.align         # executable named <stem> in the current directory
alignc fmt   file.align --write # normalize formatting in place
```

The edit loop is `check` and `run`. Multi-file builds compile one module per `.align` file, check imports against explicit interfaces, and link the reachable DAG. `check-per-unit` exposes that interface-based checker; `emit-interface` prints each unit's public surface and interface/implementation hashes.

Codegen uses a default-on content-addressed object cache and parallel workers. It is silent unless asked:

```text
alignc build app.align --cache-stats -j 4
alignc cache clear
```

`-j` overrides `ALIGNC_JOBS`. `ALIGNC_CACHE=off` disables caching; `ALIGNC_CACHE=<path>` relocates it. Cache identity includes source/interface content, compiler and LLVM identity, target, profile, exports, runtime bitcode, and PGO mode. A hit therefore means reusable bytes, not merely a newer timestamp.

## Seeing what the compiler saw

```text
alignc emit-mir  file.align
alignc emit-llvm file.align --stage raw
alignc emit-llvm file.align --stage optimized
alignc emit-obj  file.align
alignc explain-opt file.align --verbose
alignc size file.align --profile tiny
```

`emit-mir` is the semantic lens. Raw LLVM IR shows lowering before optimization; optimized IR shows the code LLVM actually shaped. `explain-opt` translates vectorization and other optimization remarks back to source lines. `size` builds the same artifact as `build` under the selected profile and reports where its bytes went. For standalone objects or IR, repeat `--export name` to keep selected entry-unit functions externally visible.

## Profiles, targets, and whole-program optimization

```text
--profile dev|release|fast|small|tiny   # O0, O2, O3, Os, Oz
--target-cpu baseline|native|<LLVM CPU>
--rt-lto                               # inline selected runtime bitcode
--thin-lto                             # cross-unit ThinLTO
```

The default is portable `baseline` plus `release`. `native` is for the current machine; a named LLVM CPU such as `x86-64-v3` is useful for a known deployment fleet. `--rt-lto` and `--thin-lto` are explicit because they change compile cost and optimization scope. Both require `release` or `fast`; ThinLTO applies to linked `build`/`run`/`size` operations, is parallel and cached, and composes with runtime LTO.

For a representative production workload, instrumented PGO is available:

```text
alignc build app.align --profile fast --pgo-instrument
./app                                      # writes the announced .profraw file
llvm-profdata-22 merge default.profraw -o app.profdata
alignc build app.align --profile fast --pgo-use app.profdata
```

The compiler prints the actual raw-profile destination. Instrument and use modes are mutually exclusive, cached independently, and currently cannot be combined with `--thin-lto`; `--rt-lto` does compose. A missing, unreadable, corrupt, or version-invalid profile is a hard error. A stale or wrong-but-readable profile produces a prominent warning and still builds because profile mismatch affects performance, not program semantics.

## The formatter

`alignc fmt` prints the normalized form; `--write` rewrites the file. It normalizes only meaningless variation — spacing, `;` placement, trailing commas, alignment — and preserves your line breaks. It refuses to format a file that does not parse. Run it routinely so diffs stay semantic.

## The lints

Every check and build runs the lint suite. There is no per-file suppression surface.

**Hard errors** enforce correctness:

```text
unhandled Result        handle it with ?, match, else, or a binding
```

**Warnings** expose deterministic costs without blocking a build:

```text
lossy conversion        an `as` that can discard information
huge struct copy        a by-value copy larger than about two cache lines
unnecessary heap        a narrow allocate-then-immediately-read shape
wasteful default        a large literal array using a wider inferred element than needed
unused import           an imported capability unused by that file
```

These are the performance model speaking at the source line, not style rules. Fix the data shape first; when you intentionally keep a warning, measure the artifact with `explain-opt`, `size`, and a representative benchmark.

## What's deliberately missing

There is no package manager, project manifest, general test runner, or debugger integration yet. The `pkg` layer is intended to remain outside core and std. The current contract is deliberately small: one binary, import-discovered builds, content-identified artifacts, and inspectable optimization.
