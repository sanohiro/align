# The toolchain: alignc, the formatter, the lints

> 🌐 **English** · [Japanese](./ja/16-toolchain.md)

One binary, `alignc`, carries the whole toolchain — compiler, runner, formatter, and the IR dumps that let you audit the machine code you're getting. No build-file dialect to learn yet: the unit of building is a file and its imports.

## The commands you'll actually use

```text
alignc check file.align         # fast: parse + typecheck + lints, no codegen
alignc run   file.align [args…] # build + execute; trailing args → main(args)
alignc build file.align         # emit a native executable next to you
alignc fmt   file.align --write # normalize formatting in place
```

The edit loop is `check` (subsecond, prints every diagnostic) and `run`. `build` gives you the deployable artifact — a plain native executable, no runtime to ship. Multi-file programs build from the entry file; imports are found relative to it (chapter [09](09-generics-and-modules.md)).

## Seeing what the compiler saw

```text
alignc emit-mir  file.align     # the mid-level IR: what your code means
alignc emit-llvm file.align     # LLVM IR: what your code became
alignc emit-obj  file.align     # object file only (link it yourself)
```

`emit-llvm` deserves a habit. This book has claimed repeatedly that pipelines fuse and vectorize — don't take its word: dump a pipeline's IR and look for one loop and `<4 x i64>`-style vector types. When a perf question comes up, the answer is one command away, which is the practical meaning of "four-way alignment": the hardware's view of your program is inspectable, not folklore.

## The formatter

`alignc fmt` prints the normalized form; `--write` rewrites the file. Its philosophy is deliberately narrower than most: it normalizes **only meaningless variation** — spacing, `;` placement, trailing commas, alignment. It does **not** reflow your line breaks or force one-line versus multi-line: whether a pipeline reads better as one line or five is information *you* chose, and the formatter preserves it. (It also refuses to format a file that doesn't parse — it never "fixes" code it doesn't understand.) Run it always; diffs stay semantic.

## The lints

`check` (and every build) runs the lint suite. There is no configuration and no `#[allow]` — the suite is small and deliberate, and it splits by severity in an unusual way:

**Hard errors** — correctness rules wearing lint clothing:

```text
unhandled Result        a discarded Result<_, _> — handle it with ? / match / a binding
```

**Warnings** — performance honesty; they never block a build:

```text
lossy conversion        an `as` that truncates (defined behavior, but flagged)
huge struct copy        by-value copy past ~2 cache lines — take a view or restructure
unnecessary heap        a box that never escapes — use a plain value
wasteful default        a large literal array defaulting to a wider element than it needs
unused import           an import no code in the file uses
```

You have met most of these in earlier chapters, because they fire on real beginner code: the box in chapter [05](05-memory.md) that didn't need the heap, the `i64 as i8` in chapter [02](02-language-basics.md). That is the intended experience — the lints are the language's performance model talking to you at the exact line where you left it, not a style cop. When a warning fires, the fix is almost always the idiom this book teaches; when you disagree with one, you can ship anyway (warnings don't fail builds) — but measure first, because each of these flags a cost the language otherwise has no way to make visible.

## What's deliberately missing (for now)

No package manager, no build system, no test runner, no debugger integration — pre-release, the single-binary toolchain is the point: everything above works today and has no configuration to bitrot. The `pkg` layer (frameworks, ecosystem) is designed to live *outside* core and std, so the language never grows a mandatory build ritual. Expect the tool surface to widen; expect the philosophy — one binary, zero config, IR on demand — to stay.
