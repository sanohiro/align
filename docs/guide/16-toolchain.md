# The toolchain: alignc, align-repl, the formatter, the lints

> 🌐 **English** · [Japanese](./ja/16-toolchain.md)

One binary, `alignc`, carries the compiler, runner, formatter, cache controls, and inspection tools; a second, `align-repl`, drives the same compiler as an interactive session. A multi-file program starts at one entry file; imports form the build graph, so there is still no separate build-file dialect.

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

## align-repl

A second binary, `align-repl`, is an AOT REPL. It ships in the same release archive, `.deb`, and
Homebrew formula as `alignc`, so a packaged install already has it. It takes no arguments:

```text
$ align-repl
align> 1 + 2
3
```

There is no interpreter and no JIT. The session is **one growing Align program**: each entry is
spliced in, the whole program is recompiled through the same driver calls `alignc build` uses, and
the resulting native binary is executed. Behavior is therefore identical to a production compile —
the same profile, the same `rt-lto` default, the same object.

Re-execution is the model, not an implementation detail. Because the whole program runs again every
entry, output you have already seen is elided and only the new lines are printed:

```text
align> x := 5
align> print(x * 2)
10
```

Rebinding a name **edits the earlier line in place** — Align forbids shadowing, so there is nothing
else it could mean — and the later lines re-run against the new value. That changes earlier output,
so the REPL prints the whole run behind a banner rather than hiding the difference:

```text
align> x := 21
align-repl: re-execution differs from the previous run (a replaced binding, nondeterminism, or an external side effect) — full output follows
42
align-repl: replaced entry 1
```

`:list` shows the program that is actually being compiled, with source line numbers on the left and
entry ordinals beside them. Ordinals are never reused, so a removed entry leaves a visible gap:

```text
align> :list
   1             | // generated by align-repl; every line below is real Align
   2             | // `main` is fixed at `-> Result<(), Error>` so `?` works in every entry
   3             | // every statement re-runs on each entry; external side effects are repeated
   4             | fn main() -> Result<(), Error> {
   5    1   main |   x := 5
   6    2   main |   print(x * 2)
   7             |   return Ok(())
   8             | }
```

An entry whose value cannot be printed echoes its type instead, and nothing is bound or consumed:

```text
align> xs := [1, 2, 3]
align> xs
<array<i64>[3]>
```

### The commands

```text
:help              this text                :undo            remove the last entry
:quit              exit (also Ctrl-D)       :drop N          remove entry N
:list              show the program         :clear           drop every entry
:type EXPR         the type of EXPR         :out             reprint the last output
:const NAME := E   a top-level constant     :time [N]        time the built binary
:save PATH         write a .align file      :save! PATH      … overwriting
```

`:save` is the exit ramp: it writes the exact program that was compiled, so compiling that file
with `alignc` produces a byte-identical object.

`:const` exists because a `fn` you define cannot see `main`'s bindings — the session's `x := …`
entries are locals inside `main`. A value a function must reach is a top-level constant:

```text
align> :const WIDTH := 6
align> fn area(h: i64) -> i64 = WIDTH * h
align> print(area(7))
42
```

`:time [N]` runs the already-built binary N times and reports min/median/max. It measures **your
program**, not the compiler: compilation is not included, every sample includes process spawn, and
the reported spawn floor is there to be subtracted.

```text
align> :time 3
3 runs: min 1.6 ms, median 1.8 ms, max 3.7 ms
```

Ctrl-D and `:quit` both leave. Ctrl-C ends the session — and, while your program is running, ends
the program with it, because the REPL installs no signal handler.

There is no built-in line editing yet, so arrow-key history does not work. `rlwrap align-repl` adds
it from outside if you want it.

Two limits are worth knowing before you hit them. A region-scoped value cannot span entries:
`arena` and `heap.new` are block-scoped by the language and each entry is one statement, so a box
allocated in one entry is already dropped by the next. Type the whole block as one entry — the
prompt keeps reading while brackets are open:

```text
align> arena {
...     ys := [1, 2, 3, 4, 5].map(fn v: i64 { v * 2 }).where(fn v: i64 { v > 4 }).to_array()
...     print(ys.sum())
...   }
24
```

And a method chain split across lines is continued only inside brackets; at top level, write it on
one line or wrap it in parentheses.

`ALIGNC_CACHE`, `ALIGNC_JOBS`, and `ALIGNC_LINKER` are read by the same driver code `alignc` uses
and are not reinterpreted, so a session works unchanged with the cache disabled:

```text
$ ALIGNC_CACHE=off align-repl
align> 6 * 7
42
```

## What's deliberately missing

There is no Align package registry/resolver, fetch command, project manifest, general test runner, or debugger integration yet. Source packages work today by vendoring their source under `pkg/`; imports and the filesystem remain the dependency graph, with no manifest or lockfile. Homebrew and apt distribute the compiler and runtime, not those source packages. Chapter [23](23-packages.md) covers the current package model. The toolchain contract remains deliberately small: one binary, import-discovered builds, content-identified artifacts, and inspectable optimization.
