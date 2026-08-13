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

Two content-addressed caches sit behind every build, both default-on and both silent unless asked. Codegen also runs parallel workers:

```text
alignc build app.align --cache-stats -j 4
alignc cache clear
```

`--cache-stats` reports the two layers in pipeline order — the frontend first, then codegen:

```text
alignc: cache: main frontend hit
alignc: cache: 1 frontend: 1 hit, 0 miss
alignc: cache: main hit
alignc: cache: 1 unit(s): 1 hit, 0 miss
```

The **frontend cache** stores each unit's checked interface summary, its diagnostics, and its link libraries, so a later process does not re-check that unit. Its identity is frontend-only: the unit's exact source bytes, the transitive interface closure it was checked against, the compiler and interface-format identity, and the target triple. Profile, `--target-cpu`, runtime LTO, and PGO mode are deliberately absent, because none of them changes what the frontend produces — one entry serves every build configuration.

The **codegen cache** stores object bytes, so its identity does include those backend knobs: profile, target CPU, exports, runtime bitcode, LLVM identity, and PGO mode. Rebuilding the same source under a different `--profile` or `--target-cpu` therefore hits the frontend and misses codegen, and `--cache-stats` names the reason:

```text
alignc: cache: main frontend hit
alignc: cache: main miss (profile)
```

A hit in either layer means reusable bytes, not merely a newer timestamp. `-j` overrides `ALIGNC_JOBS`. `ALIGNC_CACHE=off` disables both layers and `ALIGNC_CACHE=<path>` relocates them; the two live in disjoint subtrees of one cache root, and `alignc cache clear` empties that root.

Projects that use `pkg.db` (chapter [23](23-packages.md)) get five more subcommands; they matter nowhere else:

```text
alignc db prepare file.align            regenerate the checked SQLite/PostgreSQL metadata
alignc db migrate --entry file.align    apply an explicit migration catalog
alignc db status  --entry file.align    report the migration state
alignc db check   --entry file.align    require the exact expected migration state
alignc db repair  --entry file.align    checksum-bound repair of one dirty migration
```

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
--rt-lto / --no-rt-lto                 # force runtime-bitcode LTO on/off (default: on at release/fast)
--thin-lto                             # cross-unit ThinLTO
```

The default is portable `baseline` plus `release`. `native` is for the current machine; a named LLVM CPU such as `x86-64-v3` is useful for a known deployment fleet. Runtime LTO is **on by default** under the optimizing `release`/`fast` profiles (measured 2-3× on string-predicate pipelines, non-regressing elsewhere, +1-2ms compile) and off under `dev`/`small`/`tiny`; `--no-rt-lto` / `--rt-lto` force either direction. `--thin-lto` stays explicit because it changes compile cost and optimization scope; it requires `release` or `fast`, applies to linked `build`/`run`/`size` operations, is parallel and cached, and composes with runtime LTO.

For a representative production workload, instrumented PGO is available:

```text
alignc build app.align --profile fast --pgo-instrument
./app                                      # writes the announced .profraw file
llvm-profdata-22 merge default.profraw -o app.profdata
alignc build app.align --profile fast --pgo-use app.profdata
```

The compiler prints the actual raw-profile destination. Instrument and use modes are mutually exclusive, cached independently, and currently cannot be combined with `--thin-lto`; `--rt-lto` does compose. A missing, unreadable, corrupt, or version-invalid profile is a hard error. A stale or wrong-but-readable profile produces a prominent warning and still builds because profile mismatch affects performance, not program semantics.

## The linker

`alignc` links through the system C driver. On ELF targets it additionally asks that driver to run LLVM's `ld.lld`, which ships inside the LLVM toolchain `alignc` already requires, so nothing new needs installing. `ALIGNC_LINKER` pins the choice:

```text
ALIGNC_LINKER=lld       ELF: use ld.lld, or fail loudly if none is found
                        Mach-O: a hard error, not a silent no-op
ALIGNC_LINKER=system    always use the system linker
unset (default)         ELF: ld.lld when the toolchain ships one, else the system linker
                        Mach-O: always the system linker
```

Any other value is a hard error. Only link speed changes: the objects, the hygiene flags, the per-profile strip, and the optimization applied are identical either way, so this is not a `--profile` in disguise. macOS is unaffected — Apple's linker is already fast, and Mach-O never selects lld. A failed link names the linker that ran, and a failed lld link also names `ALIGNC_LINKER=system` as the escape hatch.

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

There is no Align package registry/resolver, fetch command, project manifest, general test runner, or debugger integration yet. Source packages work today by vendoring their source under `pkg/`; imports and the filesystem remain the dependency graph, with no manifest or lockfile. Homebrew and apt distribute the compiler and runtime, not those source packages. Chapter [23](23-packages.md) covers the current package model. The toolchain contract remains deliberately small: one binary, import-discovered builds, content-identified artifacts, and inspectable optimization.
