# Runtime and core/std Bootstrap (draft)

Design sketch for the ABI of `align_runtime` and for how to bring up core/std. Here we define the implementation of the `align_rt_*` that `05-backend-llvm.md` calls, closing the source→executable design.

Policy:

```text
keep the runtime thin       no GC. only the "minimum the language requires"—arena / parallel / panic / mutable buffer, etc.
ownership is on the compiler lifetimes & release points are settled in MIR (03/04). the runtime allocates/frees exactly as told
core is close to the language  write it in Align itself as much as possible. drop to the runtime (C ABI) only for the minimal lowest layer
std is the OS boundary       std is a thin wrapper over OS syscalls. built on core
```

This document is a **draft**. Open items are at the end + inline `// OPEN:`.

---

## 1. Runtime structure

`align_runtime` is a thin native library (implemented in Rust, leaning toward `no_std`, exposing a C ABI). The driver links it with the object (`01`/`05`).

```text
align_runtime
  start          entry (_start / main equivalent). calls main and returns the exit code
  arena          bump allocator + bulk reset
  heap           explicit heap (thin wrapper over the malloc family)
  par            data-parallel runtime (work-stealing or chunk splitting)
  task           I/O concurrency (task_group)
  buffer/builder mutable byte buffer
  panic          abort + message
  intrinsics     memcpy/memset etc.; SIMD helpers via RUNTIME CPU-feature dispatch
```

**SIMD in the library is runtime-dispatched** (`open-questions.md` "Build targets & portability"):
because the default build targets a portable per-arch baseline, the wide-SIMD speedups (JSON / UTF-8
/ string scan, bulk copy) come from the *library* detecting the host CPU at run time and falling back
safely — so one binary uses AVX2 / NEON when present across a varied cloud/Docker fleet.

How the dispatch actually works (important detail): runtime adaptation requires **function
multi-versioning**, not a single portable function. A SIMD routine is compiled in several variants
(e.g. `#[target_feature(enable = "avx2")]` and a baseline one) and a thin entry selects among them
via `is_x86_feature_detected!`. **`std::simd` does not by itself produce a runtime-adaptive binary** —
it lets us write each variant's body *portably* (one source compiling to SSE/AVX/NEON per the active
target features), but the per-feature variants + the runtime selector are still explicit. In practice
the cheapest path is to lean on crates that already do this internally (`memchr` etc.); we add our own
multi-versioned routine only where no crate fits. Either way **no hand-written per-architecture
intrinsics**, and x86-64 + aarch64 are covered from one source. (Build lands with the std/runtime
layer; the policy is fixed.)

`// OPEN:` whether to pin static linking, and how far to depend on libc (`05 §10`). Whether to hit the OS directly (syscalls) or go through libc is a decision shared with std.

---

## 2. Value ABI (the contract between compiler and runtime)

Match the MIR/codegen layout (`05 §1`). The runtime receives these shapes by assumption.

```text
slice<T>     { T* ptr, i64 len }
array<T>     { T* ptr, i64 len, i64 cap }
str          { u8* ptr, i64 len }
builder      { u8* ptr, i64 len, i64 cap, Arena* arena? }   // described below
arena handle  Arena*   (opaque pointer. its contents are runtime-private)
```

All small headers passable by value. The actual memory is past the ptr (arena/heap/static).

---

## 3. arena allocator

The center of Align's memory model (`draft.md` §6.4). bump allocator + bulk reset at the block exit.

```text
Arena* align_rt_arena_begin(void)
void*  align_rt_arena_alloc(Arena*, i64 size, i64 align)
void   align_rt_arena_reset(Arena*)        // bulk-release all allocations. no individual free
void   align_rt_arena_end(Arena*)          // return the arena itself
```

Implementation: take a large block from the OS in chunk units and just advance the pointer (O(1) allocation). `reset` returns the pointer to the start (freeing/pooling chunks if needed). `alloc` rounds up to the requested align (SIMD alignment, `draft.md` §3.4).

codegen correspondence (`05 §4`):

```text
arena { .. }  →  a = arena_begin(); ...body (alloc is arena_alloc(a,..))...; arena_reset(a)/end(a)
```

Because typecheck has verified that a view inside the arena does not escape (`03 §7`), the runtime tracks no lifetimes at all.

`// OPEN:` chunk sharing/reuse for nested arenas, and the API when an explicit allocator (`arena a {}`, open-questions) is used.

---

## 4. heap

```text
void* align_rt_heap_alloc(i64 size, i64 align)
void  align_rt_heap_free(void* ptr)
```

Normal code does not manually free (`draft.md` §6.5). The release point comes from MIR's `Drop` (derived from the move check, `04 §1`), and codegen emits `heap_free`. raw alloc is `unsafe` only (`draft.md` §6.5) and goes through a separate thin API.

---

## 5. Data parallelism (par)

The target that MIR's `ParLoop` (`04 §6`) lowers to (`05 §7`).

```text
void align_rt_par_for(
  void* items, i64 len, i64 elem_size,
  i64 chunk,                       // 0 means the runtime default
  void (*body)(void* chunk_ptr, i64 chunk_len, void* ctx),
  void* ctx)
```

- Split the input into chunks and hand them to worker threads. The parallel unit is the chunk (`draft.md` §11).
- `body` is the function carved out of the body fused in MIR (`05 §7`). `Effect=Pure` is guaranteed (`03 §8`), so no races occur.

Parallel reduce:

```text
void align_rt_par_reduce(
  ..., void (*body)(.., void* partial),     // the per-chunk partial result into partial
  void (*combine)(void* acc, void* partial),// combine partial results (associative)
  void* acc)
```

Combine partial results tree-wise/serially. Reuse the process-resident `ParPool`; `// OPEN:` ordering
guarantee of the combine (floating-point reproducibility).

---

## 6. I/O concurrency (task_group)

```text
Task*  align_rt_task_spawn(Result (*fn)(void* ctx), void* ctx)
Result align_rt_task_wait_all(TaskGroup*)
```

I/O-wait concurrency (`draft.md` §11). `?` applies to each spawn result, and the first failure propagates at the `wait` join point. There is no async/await (`non-goals.md`), so start from a naive implementation that puts blocking I/O on threads/a pool in the runtime.

---

## 7. buffer / builder

The foundation for string output and template desugaring (`04 §2.5`).

```text
Builder align_rt_builder_new(Arena* a?)        // can be tied to an arena
void    align_rt_builder_write(Builder*, u8* ptr, i64 len)   // static part (memcpy)
void    align_rt_builder_write_int(Builder*, i64)
void    align_rt_builder_write_f64(Builder*, f64)
str     align_rt_builder_finish(Builder*)
```

`template "Hello {name}"` is static part → `builder_write` (length known from string meta, `03`/`05 §6`), value part → per-type `write_*`. If the total static-part length is known, preallocate capacity at `builder_new` time (1 `Alloc`, `04 §2.5`). Keep separate escaping `write`s for `html`/`json`.

---

## 8. panic / traps

```text
noreturn void align_rt_panic(str msg, SrcLoc loc)
```

Called for arithmetic errors other than overflow such as divide-by-zero (`draft.md` §5), and for unrecovered invariant violations. The location comes from Span (`05 §9`). overflow defaults to wrap, so it is not normally called (only on optional checks in dev builds). `panic` prints the message + location to stderr and aborts. `// OPEN:` whether to provide a catch boundary that converts panic into a Result (current: none = immediate abort).

---

## 9. Entry point

```text
i32 align_rt_start(i32 argc, char** argv):
  args = convert argv into an array<str> (arena/static)
  r = user_main(args)               // pub fn main(args) -> Result<(), Error>
  match r:
    Ok      => return 0
    Err(e)  => report(e); return non-zero
```

Maps `main`'s return (`draft.md` §17) to the exit code. The display format of `Error` is settled in the error type design (`03`/M2). Both `fn main() -> i32` and the `Result`/argv entry forms are shipped; the M0→M2 sequence below is retained as bootstrap history.

---

## 10. core / std bootstrap

```text
core  foundation close to the language philosophy (draft.md §18.1)
      option/result, array/slice/chunks, vec/mask/bitset,
      map/reduce/scan/partition/sort, str/string/bytes/buffer/builder,
      arena, json, template, hash, math
std   the OS boundary (draft.md §18.2)
      io/fs/path/process/env/time/net/cli/encoding/compress/rand/crypto/http
```

### Policy on the implementation language
- **Write core in Align itself as much as possible**. To make MIR's fusion (`04 §3`) work for `map`/`where`/`reduce`, the direction is to define them not as specially-handled builtins but as **normal Align generic functions + intrinsic hooks**. The lowest layer (SIMD scan, the core of hash) drops to runtime intrinsics.
- **Write std as a thin wrapper** over the runtime + OS syscalls, in Align. `fs.read_file` etc. call the runtime's I/O primitives.

### Bootstrap order (historical; all listed milestones completed)
```text
M0-M1  minimal runtime (start/arena/panic) + builtin print only. almost no core/std
M2     core.option / core.result. std.fs.read_file (a concrete example of ?)
M3     wire core.arena to the language feature
M4     core.array / slice / reduce family (the verification target for fusion)
M5     core.str/string/builder, core.json, core.template
M6     core.vec / mask
M7     parallel (par_map on the core side / task on std)
M8+    std expansion (path/env/time/net/...). pkg is out of scope (draft.md §18.3)
```

The current builtin/runtime boundary is documented by the core-design inventories and the shipped
M4–M12 records in `07-roadmap.md`; consumer-driven intrinsics remain preferable to speculative
self-hosting work.

---

## 11. Open items (to be settled)

```text
- static linking / scope of libc dependence / whether direct syscalls are allowed (common with 05 §10)
- floating-point reproducibility of parallel reduce
- whether to provide a panic catch boundary (current: immediate abort)
- Error type display and exit-code correspondence (M2 error type design)
- the boundary for writing core in Align (how far to drop to intrinsics)
- API for arena nesting / explicit allocator (linked with 03/04)
```

Once settled, reflect into `draft.md` (the relevant feature) and this document.
