# String and array allocation, copying, and short-input audit

> Status: audit record, 2026-07-13. **Partial corrective implementation shipped in the working
> tree on 2026-07-13:** the UTF-8 range-boundary part of §3.1 is fixed and regression-pinned;
> its zero-cost `s.bytes()` companion and §3.2's `str + str` contract correction shipped
> 2026-07-15. Arena-free template ownership, unbound Move temporaries, and known-null cleanup
> pruning shipped that day; §6.1's borrowed ABI strings shipped 2026-07-16. The other findings
> remain open. No other implementation in this document is
> shipped merely because it is described here. Performance changes remain gated unless an existing
> roadmap item already says otherwise. The complete corrective wave is summarized in
> [`source-correctness-fixes-2026-07-13.md`](source-correctness-fixes-2026-07-13.md).

## 1. Scope and classification

This record answers a narrower question than documents 10–12:

- do text and array operations allocate or copy more than their result ownership requires;
- do paths tuned for large inputs impose disproportionate setup costs on empty and short inputs;
- does generated IR preserve the allocation-free, fused shape that Align intends;
- which implementation changes are new, and which are already planned elsewhere;
- are there language-contract questions worth asking Claude Code to assess without treating them as
  decisions.

The audit read the runtime, sema, MIR, LLVM lowering, tests, current implementation plans, and the
settled/open language records. A local Apple-Silicon release microprobe covered `0..4096` bytes or
elements for UTF-8 validation, equality, substring search, `builder`, and `array_builder`. Its numbers
are directional evidence, not a merge gate: the probe used the runtime dylib boundary and
`black_box`, but not the repository's required balanced AB/BA harness or hardware counters.

Classifications:

- **CONFIRMED P0** — violates a settled invariant or leaks in a repeatable ordinary program;
- **CONFIRMED P1** — unnecessary work is mechanically present and the correct shape is clear;
- **ALREADY PLANNED** — do not count it as a new finding or build a parallel mechanism;
- **MEASURE FIRST** — plausible, but keep the current implementation unless the stated gate wins;
- **SHIPPED / GOOD** — retain it and use it as the oracle or negative control;
- **CLAUDE QUESTION** — a non-binding language-design question, not a proposal adopted here.

## 2. Executive result

The large-input story is not generally crowding out small inputs. Fused numeric pipelines have no
intermediate arrays, LLVM emits scalar fallbacks for mapped materializers, substring search uses a
special path below 64 bytes, fixed arrays are stack values, and `array_builder.build()` freezes its
payload without copying. The two-word `str`/`string`/slice/array view layout should remain simple;
this audit does **not** reopen SSO or a hidden small-vector representation.

The strongest problems are instead ownership and fixed-cost gaps:

| Area | Current shape | Disposition |
|---|---|---|
| `s[a..b]` | O(1) range + UTF-8 scalar-boundary checks | **FIXED 2026-07-13**; `str` validity is preserved |
| `s.bytes()` | descriptor-only `str`/`string` → `slice<u8>` view | **SHIPPED 2026-07-15**; zero allocation/copy |
| `str + str` | rejected in sema; no MIR concatenation path remains | **FIXED 2026-07-15**; `builder` is the one construction path |
| arena-free `template` / `json.encode` | hidden owned `string` backs the surface `str` view | **FIXED 2026-07-15**; frame-bounded and dropped |
| unbound owned temporaries | synthetic path-local owner; scalar early-drop, view retention | **FIXED 2026-07-15**; control flow and loops pinned |
| moved slots | MIR prunes cleanup edges whose drop flag is definitely false | **FIXED 2026-07-15**; no known-null destructor call |
| filesystem/path ABI views | Rust consumers borrow; C consumers construct `CString` directly | **SHIPPED 2026-07-16**; redundant allocation/copy removed |
| UTF-8 validation | x86 uses AVX2 from 32 bytes; native Apple-M1 NEON has no no-regression length crossover | **MEASURED / CLOSED**; aarch64 production remains scalar |
| `builder.to_string()` | allocator-compatible grow buffer transfers into owned result | **SHIPPED 2026-07-16**; no final allocation/copy |
| `array_builder` | header allocation + payload allocation + per-push ABI call | **CONFIRMED P1** for tiny builders; zero-copy freeze is good |
| `chunks(n)` | direct `.len()` / `[i]` are virtual; stored and pipeline values materialize | **SHIPPED 2026-07-16** for direct consumers |
| str-key group/dictionary | single aggregates/dictionary write caller outputs directly | **SHIPPED 2026-07-16**; staging Vecs/copies removed |
| `path.normalize` | one exact-upper-bound final buffer, filled in place | **SHIPPED 2026-07-16**; no staging/final copy |
| large constant local arrays | entry alloca plus O(N) stores remains after O2 | **MEASURE FIRST** global constant/memcpy crossover |
| Base64/hex encode | exact final allocation; ARM Base64 NEON from 48 bytes, ARM hex from 16 bytes | **SHIPPED** in document 12 |
| sorting | stable O(n log n), but allocates unused merge scratch at tiny N and ignores ordered runs | **MEASURED P1** adaptive total-order path in document 12; keep insertion base case |

Correctness/resource work comes first. Several current leaks accidentally keep borrowed views alive;
freeing them without owner/view liveness would convert a leak into a UAF.

## 3. Correctness and ownership prerequisites

### 3.1 FIXED 2026-07-13 — `str` range slicing cannot create invalid UTF-8

The language contract says every `str` is valid UTF-8 and a byte-range slice aborts if either bound
splits a scalar ([draft §12](../../draft.md#12-string),
[language summary](../language-spec.md#strings)). At the audit baseline MIR performed only
`0 <= start <= end <= len`, then created a byte-stride `SubSlice`; there was no
continuation-byte test in MIR, codegen, or the runtime.

For example, the audit-baseline optimizer accepted and constant-folded the length of an invalid view:

```align
s := "é"          // c3 a9
bad := s[1..2]     // a9, a UTF-8 continuation byte
print(bad.len())   // 1
```

The fix is O(1): `0` and `len` are valid; otherwise the byte at the boundary must not match
`10xxxxxx`. MIR now checks both endpoints after the ordinary range guard and calls the noreturn
`align_rt_utf8_boundary_fail` cold path on a split scalar. Integration tests cover accepted
1/2/3/4-byte boundaries, omitted endpoints, and both a split start and split end. This preserves the
two loads/masks shape instead of rescanning the sub-string.

The documented arbitrary-byte escape hatch `s.bytes() -> slice<u8>` shipped 2026-07-15 as the
existing contract requires. Sema records a borrow-producing `StrBytes` HIR node (auto-borrowing an
owned `string` first), region and owner provenance follow the source, and MIR returns the same
`{ptr,len}` operand without an rvalue or runtime call. Regression tests cover UTF-8 continuation-byte
access, static/caller-safe returns, owned/arena escape rejection, owner invalidation, and the
descriptor-only MIR shape.

### 3.2 FIXED 2026-07-15 — the settled `str + str` hard error is enforced

`draft.md`, `language-spec.md`, and the settled ledger all say concatenation through `+` is a hard
error and `builder` is the one construction path
([settlement ledger](../open-questions.md)).
The audit baseline checker explicitly accepted it, and MIR lowered it to a fresh two-piece
`Template`. The checker now rejects both `str` and owned `string` arithmetic, with `+` naming
`builder`, `.write()`, and `.to_string()` as the single construction path. The obsolete MIR
two-piece-template branch is removed, and stale region/Drop tests use `template` when they need an
arena-backed `str`, so they continue to exercise their original invariant instead of passing on the
new diagnostic.

This is not a new language proposal. Enforce the settled error, change the stale tests, and use the
same diagnostic the spec gives. Besides restoring One way / Nothing hidden, this prevents a chain
such as `a + b + c + d` from building and recopying a growing intermediate at every syntax node.
Inside an arena that is O(k²) copied bytes plus retained intermediates; outside an arena it joins the
process-lifetime leak below.

### 3.3 FIXED 2026-07-15 — arena-free `template` and `json.encode` have scoped owners

The surface result remains `str`. Inside an explicit arena, templates keep the existing arena-backed
finish. Outside an arena, MIR now creates a hidden synthetic `string` owner, codegen finishes through
`align_rt_builder_into_string`, and the returned view carries that owner through existing borrow-owner
propagation. Function/loop cleanup drops stored values, while scalar consumers such as `print`, hash,
or `.len()` can release the owner immediately after the view is consumed. The owner is registered
before lowering holes, so an early `?` also cleans it safely.

Escape analysis classifies the arena-free dynamic view as `Frame`: it can be stored and consumed
locally, including inside a lifted pipeline lambda, but cannot be returned beyond its hidden owner.
Static-only templates fold in HIR to pooled string literals and emit no builder or allocation.
Dedicated regressions pin the static fold, a 20,000-iteration cleanup loop, `json.encode`, local
pipeline-lambda consumption, cleanup for `?` in a hole, and the rejected escape. Direct
builder-to-sink fusion remains a
separate P2 optimization; correctness no longer depends on it, and the existing synthetic-owner
early-drop path avoids retaining completed strings longer than their scalar consumer.

### 3.4 FIXED 2026-07-15 — unbound owned temporaries have view-aware synthetic owners

MIR now gives a fresh unbound Move expression a zero-initialized hidden slot plus a path-local drop
flag. The flag is separate from ordinary ownership: at `(if c { make() } else { bound }).len()`, the
fresh arm sets the temporary bit and the bound arm leaves it clear, so the existing local is borrowed
rather than transferred or double-freed. `if`, `match`, `else`-unwrap, and `?` carry that selected bit
through the same value-result joins as ordinary ownership provenance.

The five confirmed optimized-IR leaks now each retain exactly one `align_rt_free`:

```align
"x".clone().len()
[n, n + 1].to_array().len()
[n, n + 1].to_array()[0]
[1, 2, 3].chunks(2).len()
b.build().len()
```

Scalar consumers (`len`, scalar index, predicates, borrow-only sinks) end the hidden owner's liveness
immediately after producing the scalar. Borrow-producing operations propagate the owner alongside the
SSA value: subslices, `trim`, `str.bytes`, `path.base`/`dir`/`ext`, chunks, indexed views, and calls
returning a borrow therefore retain it through later use. Escape analysis caps a view of fresh owned
storage at `Frame`, rejecting a returned string/slice/chunk element before cleanup can dangle it.
Synthetic owners discovered inside a loop join the loop's final per-iteration cleanup set, including
`break`; entry-block initialization makes sibling and early-exit paths safe before a producer runs.

`crates/align_driver/tests/owned_temporaries.rs` pins clone, builder freeze, path/encoding, `to_array`,
sort, chunks, and `par_map`; scalar/index/view/call consumption; mixed `if` and `match`; `?`; rejected
returns; and repeated loop cleanup. Its optimized-IR parity gate requires the five original allocation
shapes to have exactly five frees, while runtime cases cover both sides of mixed control flow and
view use (so neither a leak-masked UAF nor a double-free passes). Partition's two owned outputs remain
covered through its settled destructuring consumer; temporary tuple field extraction is still a
separate surface restriction, not an ownership exception.

### 3.5 FIXED 2026-07-15 — definite-null destructor calls eliminated in MIR

Move lowering still nulls source storage for recursively safe inspection, but a forward MIR
drop-flag pass now propagates constant ownership bits through CFG joins, replaces constant cleanup
branches with their live edge, and removes the newly unreachable destructor blocks. Consequently,
optimized IR no longer retains calls such as:

```llvm
tail call void @align_rt_free(ptr null)
call void @align_rt_array_builder_free(ptr null)
```

Storage nulling remains the safety fallback for runtime-selected ownership; the optimization is
limited to flags with one compile-time value on every reachable incoming path. A live or
path-dependent flag therefore retains its conditional destructor and exactly-once behavior.

The regression gate covers returned strings, a frozen `array_builder`, conditional moves, early
returns, `?`, and live allocations. It requires zero literal `align_rt_free(null)` and
`align_rt_array_builder_free(null)` calls in optimized IR while retaining real destructor calls and
checking the runtime result. MIR unit tests separately pin a moved local at zero reachable drops and
an unmoved local at exactly one.

## 4. Short-input measurements

### 4.1 Directional probe

Host: Apple Silicon arm64, macOS 26.3.1, rustc/cargo 1.96.1, release builds. Ratios below are the
range from two consecutive runs; absolute operations are often only a few nanoseconds, so permanent
acceptance benches must use balanced order and batches.

| Operation | Short sizes | Observed current/reference ratio | Interpretation |
|---|---:|---:|---|
| `utf8_valid` runtime / `std::str::from_utf8` | 0 | 5.1–6.0x | empty SIMD dispatch is unnecessary |
| same | 1 | 3.2–3.5x | strong short crossover signal |
| same | 4 | 2.1–2.6x | strong short crossover signal |
| same | 8–64 | mostly 1.1–1.6x | threshold must be measured per target |
| same | 4096 | 1.07–1.08x | current SIMD remains the large-input oracle |
| `str_eq` runtime / inline slice equality | 1–8 | 1.27–1.79x | opaque call dominates tiny compares |
| same | 16–31 | about 2.0–2.23x | exact M14 runtime-bitcode target |
| same | 4096 | 1.04x | byte scan dominates, wrapper is negligible |
| `str_find` runtime / direct `memmem::find` | 1–16 | 1.06–1.32x | small extra ABI cost only |
| same | 31–63 | about parity | memchr's `<64` one-shot path is already appropriate |
| same | 4096 | 1.02x | do not replace the search algorithm |
| one-write `builder.to_string` / Rust `String` | 0 | 24–25x | header allocation alone dominates |
| same | 1–16 | 3.3–3.6x | header + final allocation/copy dominate |
| same | 64–256 | 2.8–3.1x | still allocation/copy-bound in this probe |
| `array_builder<i64>` / Rust Vec grow-freeze | 0 | about 35x | header Box with no payload |
| same | 1–4 | 2.0–2.46x | header + first backing allocation + ABI calls |
| same | 8–64 | 1.8–2.36x | per-push calls remain visible |

These ratios do not authorize an optimization by themselves. They establish the missing regression
matrix and refute the assumption that only large text/arrays matter.

### 4.2 Permanent benchmark matrix

Every relevant slice must include:

```text
length/count: 0 1 2 3 4 7 8 15 16 31 32 63 64 65 256 4K 1M
element bytes: 1 4 8 16 and one wide Copy struct
text: ASCII, mixed valid UTF-8, invalid-at-head/middle/tail, escape-dense, escape-free
ownership: literal/view, bound owned, unbound owned temporary, arena result, returned result
state: cold first dispatch/allocation and warm steady state
target: arm64 baseline, x86-64 baseline and v3/native where available
```

Record allocator calls, allocated bytes, initialized/copied bytes, wall time, and optimized IR.
Require the `0..64` geometric mean to regress no more than 3%, even when a large case passes. Use a
15% positive gate for new infrastructure; smaller mechanical cleanups may use an allocation-count
gate plus statistically stable wall-time improvement.

## 5. String paths already shaped well

- `str`/`string` remain a stable two-word `{ptr,len}` view/owner representation; auto-borrowing an
  owned string to `str` does not move or copy it.
- `str.clone()` uses exactly one final allocation and one copy, and empty clone allocates nothing
  ([runtime](../../crates/align_runtime/src/lib.rs#L137)).
- equality rejects length mismatch, same-pointer, and empty cases before reading memory
  ([runtime](../../crates/align_runtime/src/lib.rs#L6145)).
- contains/find/rfind handle empty and too-long needles before search. `memchr 2.8.2` explicitly uses
  Rabin–Karp for haystacks below 64 bytes and a prepared/vector portfolio above it; retain it.
- trim and `path.base`/`dir`/`ext` return borrowed subviews, not owned copies.
- JSON decoded string fields borrow validated input; no per-field allocation. The long JSON structural
  scans retain scalar prefixes/tails and SIMD oracles.
- integer builder formatting uses a stack buffer, including a special `-999..=999` path, and MIR
  already fuses the common `str + int + str` write triplet into one runtime call.
- direct regular-file read allocates the final string once; `bytes.as_str()` validates then returns a
  view rather than copying.

SSO remains rejected for good reasons: it would branch every pointer access, complicate FFI pointer
stability and Move/drop, and penalize all strings to help a subset. Remove excess allocation count
around the existing representation first.

## 6. String allocation and copy opportunities

### 6.1 SHIPPED 2026-07-16 — ABI path strings are borrowed or copied once at the C boundary

The audit baseline's `path_from_view` validated bytes, then called `to_string()`. It was used by file
existence/removal, directory reads, mmap views, reader/writer/file open/create, multiple network host
paths, and process launch. Thus even a five-byte path paid an owned allocation/copy before a
filesystem API that only needed `&str`; C consumers then copied it again into `CString`.

The runtime now splits conversion by the real boundary need:

- filesystem/path consumers: `abi_str_view -> Option<&str>` with the same defensive UTF-8 check,
  then pass the borrow directly to `std::fs`;
- C-string consumers (`getaddrinfo`, exec): validate the view and construct one `CString` directly
  from bytes, rather than `String` then `CString`;
- language-only trusted callers may later avoid redundant UTF-8 validation only if the public C ABI
  and its safety contract are split explicitly. Do not silently make malformed external calls UB.

The helper gate covers 0/1/8/32/256-byte inputs, proves that every nonempty Rust view retains the
caller's exact pointer, and pins UTF-8/interior-NUL rejection plus direct C-string contents. The full
runtime file, DNS, TCP, UDP, and process controls remain green, so syscall or DNS latency is outside
the structural no-copy proof.

### 6.2 MEASURED 2026-07-16 — x86-64 crossover shipped; Apple-M1 NEON rejected

At the audit baseline, `validate_utf8` entered AVX2 whenever available and NEON unconditionally on
aarch64. A tail shorter than the vector width was copied into a zeroed 32/16-byte stack block after
lookup vectors were loaded. This differed from the JSON quote scan's 16-byte scalar prefix and
memmem's explicit `<64` strategy.

The durable ignored release probe now compares `std::str::from_utf8`, the direct SIMD path, and the
shipped dispatch using balanced order and median-of-seven samples over valid ASCII, valid multibyte,
early-invalid, and late-invalid inputs. The existing 30,000 raw-byte plus 5,000 valid/corrupted-case
differential gate remains the correctness oracle across the crossover.

**Measured x86-64 (2026-07-16, Ryzen 9 5950X / Zen 3, WSL2, release):** direct AVX2 was 1.2–3.7x
slower than scalar for the nonempty 1–16-byte matrix. At 24 and 31 bytes only the multibyte case
favored AVX2 (0.83–0.88x); ASCII and both invalid placements still favored scalar. At exactly 32
bytes AVX2 reached 0.86x on ASCII and 0.25–0.41x on multibyte/late-invalid input. An early invalid
byte continues to favor scalar because `from_utf8` returns immediately, but that failure-only shape
does not justify scanning every valid input twice. The 64-MiB mixed-text control remained at 18.6
GB/s versus 4.0 GB/s scalar and 18.4 GB/s memcpy.

The shipped x86-64 dispatch therefore returns empty immediately, uses scalar below 32 bytes, and
keeps AVX2 from 32 onward. A narrow 33–63-byte all-ASCII proof avoids the otherwise-visible padded
tail setup without imposing a second scan on long invalid/multibyte inputs; AVX2 and NEON tails now
share their full-block ASCII fast path. This changes no language surface. Do **not** infer an aarch64
threshold from the 32-byte AVX2 result.

**Measured aarch64 (2026-07-16, native Apple M1, macOS 26.3.1, rustc 1.96.1 / LLVM 22.1.8):**
the checked-in balanced median-of-seven probe ran repeatedly with one test thread. NEON lost on
valid four-byte inputs but won from eight bytes for the short ASCII, multibyte, and late-invalid
cases. Across two 1..=64 selected-point runs, candidate speedup versus scalar geometric mean was
1.26-1.27x over all four distributions and 1.67-1.68x when the early-invalid failure-only
distribution was excluded. That short result did not survive the required large negative controls:

| 1 MiB case | NEON speedup versus scalar | NEON throughput |
|---|---:|---:|
| ASCII | 0.83x | 23.9 GiB/s |
| multibyte | 6.40x | 9.5 GiB/s |
| invalid first byte | effectively 0x | 23.8 GiB/s |
| invalid last byte | 0.83x | 23.9 GiB/s |

At 1 KiB, the corresponding speedups were approximately 0.96x, 6.4x, 0.08x, and 1.05x. The 64 MiB
realistic mixed-text candidate reached 15.1 GB/s versus 3.1 GB/s scalar, but a length-only threshold
cannot retain that gain without the repeatable ASCII, late-invalid, and early-invalid regressions.
The aarch64 production dispatcher therefore uses `std::str::from_utf8`; the baseline-NEON backend,
scalar oracle, full-block/tail logic, and differential suite remain for a materially different
content-adaptive candidate. This native portability item is closed as a negative result rather than
shipping a benchmark-specific threshold.

### 6.3 SHIPPED 2026-07-16 — owned builder freeze transfers allocator-compatible storage

At the audit baseline, a surface builder owned a Rust `Vec<u8>` in a boxed header. `to_string()` allocated again
through `align_rt_alloc`, copies the complete output, then drops the Vec
([freeze](../../crates/align_runtime/src/lib.rs#L4427)). For a one-write tiny string that means a
header allocation, a Vec allocation, a final allocation, and one full copy.

The existing `array_builder` proves the compatible shape: C `malloc/realloc` storage can be handed to
an Align owned value and freed by the existing size-less `align_rt_free`. `BuilderBuf` now uses that
same C allocator family directly, so owned freeze transfers its pointer without copying. This is
allocator-compatible on glibc, musl, macOS, and other supported C runtimes without assuming Rust's
global allocator happens to be the system allocator. Builder, exported allocation ABI, and
array-builder storage route through one internal alloc/free/realloc family; exported positive
`i64` sizes pass `safe_len` before conversion, including on 32-bit targets. The implementation
preserves:

- UTF-8-by-construction writes;
- geometric growth and checked capacity arithmetic (the initial capacity hint remains best-effort);
- builder Move/drop and null-on-move behavior;
- arena template behavior as a distinct case — an arena result still needs arena-owned storage;
- direct builder-to-I/O paths, which should never freeze a string merely to write it.

M14's already-planned runtime-bitcode ceiling probe addresses per-write ABI calls, not this final
allocation/copy. Sequence the two measurements independently. A nonescaping stack header is another
measure-first layer; do not combine both mechanisms in the first benchmark so the attribution stays
clear.

**Measured ceiling (2026-07-14, Ryzen 9 5950X, release/native):** a one-write builder was opened with
`capacity == payload length`, filled, and consumed through the current owned-copy freeze or through
the existing non-arena `finish` transfer as a current-host proxy for a malloc/realloc-compatible
owned freeze. The proxy result was freed after every iteration; on this Linux host Rust's system
allocator and `align_rt_free` are the same glibc malloc/free family. Median of nine samples:

| payload | current copy freeze | transfer proxy | current/proxy |
|---:|---:|---:|---:|
| 1 KiB | 48.8 ns | 43.6 ns | 1.12x |
| 4 KiB | 103.4 ns | 67.3 ns | 1.54x |
| 16 KiB | 472.1 ns | 163.2 ns | 2.89x |
| 64 KiB | 1.98 us | 0.94 us | 2.10x |
| 256 KiB | 157.0 us | 4.21 us | 37.3x |
| 1 MiB | 742.0 us | 18.5 us | 40.1x |

The jump above the allocator's large-allocation crossover includes the real cost of creating,
touching, and freeing the second large allocation, not just memcpy throughput. At 8-64 bytes the
result was small/noisy, so this does not justify a short-string representation change. It does
confirm the raw-buffer transfer as **P1 for medium/large owned freezes**; the implementation gate
must additionally cover unknown/geometrically grown capacity and non-glibc targets rather than
depending on this proxy's allocator compatibility.

**Shipped adoption gate (2026-07-16, same Ryzen 9 5950X, release/native, balanced median of nine):**
the checked-in ignored probe gives both paths the same C-allocator grow buffer and one bulk input
write. The control recreates only the removed final malloc+copy; the candidate transfers the grow
pointer. Exact pre-sizing and capacity-zero geometric growth both win across every measured size:

| growth | bytes | copy freeze | transferred freeze | copy/transfer |
|---|---:|---:|---:|---:|
| exact | 64 | 14.83 ns | 8.75 ns | 1.70x |
| exact | 1,024 | 27.46 ns | 14.97 ns | 1.83x |
| exact | 4,096 | 91.29 ns | 48.48 ns | 1.88x |
| exact | 16,384 | 406.81 ns | 145.03 ns | 2.80x |
| exact | 65,536 | 1.816 us | 0.895 us | 2.03x |
| exact | 262,144 | 9.150 us | 4.410 us | 2.07x |
| exact | 1,048,576 | 36.967 us | 18.429 us | 2.01x |
| grow | 64 | 16.69 ns | 10.50 ns | 1.59x |
| grow | 1,024 | 29.75 ns | 15.50 ns | 1.92x |
| grow | 4,096 | 93.26 ns | 49.21 ns | 1.90x |
| grow | 16,384 | 410.68 ns | 146.38 ns | 2.81x |
| grow | 65,536 | 1.804 us | 0.901 us | 2.00x |
| grow | 262,144 | 9.432 us | 4.501 us | 2.10x |
| grow | 1,048,576 | 39.388 us | 19.207 us | 2.05x |

Pointer-identity tests pin transfer for boxed and stack headers after exact or geometric growth;
arena finish remains a distinct copy into arena-owned storage, empty freeze stays canonical null/0,
and unfinished Drop plus direct builder-to-file/writer paths retain their existing ownership shape.

### 6.4 SHIPPED 2026-07-16 — direct payloads in `read_dir` and DNS results

`fs.read_dir` and DNS resolution now allocate each final UTF-8 name/numeric-IP payload once while
enumerating and retain only `AlignStr` headers in a shared RAII list. A later iteration or size error
drops that list and frees every payload accumulated so far while the ABI output remains canonical
`{null,0}`. Success publishes one final header buffer and clears the temporary owner, transferring
the payloads to generic `array<string>` deep-drop. DNS duplicate detection reads those same final
payloads, so ordering and deduplication are unchanged. This removes one staging allocation and one
full payload copy per returned entry.

Do not change `array<string>` representation or introduce a shared hidden slab here. Per-element
ownership is observable through Move/drop and the generic deep-free path; a slab requires a distinct
owner representation.

### 6.5 SHIPPED 2026-07-16 — normalize directly into the final buffer

`path.normalize` now allocates the Align-owned result once at the proven `max(input_len, 1)` upper
bound. The initialized prefix is the component stack: ordinary components append directly and `..`
rewinds to the preceding separator. Bytes removed by a pop are not scanned again, so the operation
remains linear without either the old component `Vec<&[u8]>`, output `Vec<u8>`, or final full copy.

A staged-oracle differential gate covers 1,000 generated shapes, short/deep paths, repeated `..`,
root clamping, repeated separators, and long UTF-8 components. The checked-in ignored release probe
(balanced median of nine, allocation-inclusive) measured 2.30x for an already-normal 16-byte path,
1.86x for a mixed 10-byte path, and 1.43x for 1,169 bytes/256 normal components. The disclosed
negative shape — 256 appended components followed by 192 pops — was 0.77x because the direct form
writes components that a staging stack later discards; this does not affect the short-path adoption
gate but remains the explicit worst-case tradeoff of the one-allocation representation.

### 6.6 MEASURED — repeated-needle plan hoisting and JSON escape scan

For one search, keep `memchr::memmem`. For a pipeline applying the same loop-invariant needle to many
strings, the one-shot API rebuilds a Finder/FinderRev each call. Implement preparation as MIR/runtime
plan hoisting first; do not add a public Pattern type unless a real dynamic consumer cannot use
compiler hoisting.

`builder_write_json_str` still scans escape-free long content byte by byte. A scalar prefix followed
by a block classifier for quote, backslash, or `<0x20` helps long strings, but per-write ABI and
allocation dominate short records. A short scalar path is mandatory, and the existing scalar
encoder remains the differential oracle.

**Measured kernels (2026-07-14, Ryzen 9 5950X, release/native, median of nine):** reusing one
`memchr::memmem::Finder` for a no-match repeated search was 2.5-6.1x faster than reconstructing the
one-shot search at 32-128-byte haystacks (4/16-byte needles), 1.5-2.2x at 1 KiB, and only 1.0-1.1x at
16 KiB where scanning dominates. This confirms plan hoisting for the compiler-visible
same-needle/many-strings shape, not a replacement for one-shot `find` and not a new public Pattern
type.

For JSON encoding, a single-pass AVX2 32-byte classifier was compared with the current scalar scan
while reusing the same sufficiently-sized output Vec (allocation excluded). It produced identical
bytes for clean, sparse-escape (one per 97 bytes), and dense-escape (one per 8 bytes) inputs. At 32
bytes and above it was 3.1-17.1x faster on clean input, 3.6-5.7x at 256 bytes through 16 KiB on sparse
input, and 1.3-1.5x on dense input. At 8 bytes SIMD setup was 22-29% slower. Promote the long-string
classifier to **P1 with a scalar `<32` crossover**; the adoption gate remains an end-to-end builder
benchmark on x86 baseline/v3 and arm64 plus differential tails and every control-byte class.

### 6.7 ALREADY PLANNED — do not duplicate

- exact-final Base64/hex allocation and the independent Base64/hex SIMD gates: document 12 §6.3/8.2;
- JSON Vec-to-final allocator measurement: roadmap wave 3;
- runtime bitcode for short `str_eq`/`str_cmp`/hash wrappers: M14 ceiling probe;
- direct template/encode-to-writer Sink vocabulary: consumer-gated open design;
- no general SSO, string interning, or automatic global allocator replacement.

## 7. Array paths already shaped well

- fixed arrays are Copy stack aggregates; dynamic arrays are Move `{ptr,len}` owners and slices are
  Copy views. A subslice is pointer arithmetic plus a new header, with no allocation/copy.
- reductions are one fused counted loop with no intermediate array.
- `to_array`/`scan` allocate the source upper bound once and fill it in one loop; there is no growth
  or final right-size copy. Lazy pages make low selectivity less costly than the virtual capacity
  suggests.
- `map_into` performs no allocation and carries scoped source/destination alias facts.
- optimized mapped materializers have a scalar fallback (`min.iters.check`) rather than forcing SIMD
  setup on short arrays. Identity materialization can become `llvm.memcpy`.
- `array_builder` grows from capacity four by checked doubling and hands its realloc-compatible
  payload to `array<T>` without copying.
- two full-capacity `partition` outputs avoid a second predicate pass and preserve independent
  ownership. Do not add a count pass or a shared allocation without a measured win and new owner
  model.

## 8. Array allocation, copying, and IR opportunities

### 8.1 CONFIRMED P1 — make `array_builder` cheap when it is tiny

The boxed `array_builder_new` ABI still allocates a header for escaping values; proven local values
now use aligned caller storage instead ([runtime](../../crates/align_runtime/src/lib.rs#L7652)). The
first push separately reallocates backing storage in either case. A dynamic push loop also retains
one opaque `align_rt_array_builder_push` call per element, so LLVM cannot turn it into bulk/vector
stores. Eligible empty locals now allocate zero times and 1–4 elements once; escaping values retain
the old one/two-allocation shape, while larger loops remain ABI-bound.

Separate refinements:

1. ~~Mark `align_rt_array_builder_new` with the same audited allocator attributes as
   `builder_new`.~~ **DONE 2026-07-16.** Its declaration now goes through `mark_alloc_like`; the IR
   gate pins `noalias`, `nofree`, and `nounwind` while forbidding the unsound `willreturn`. This is
   hygiene, not the main speedup.
2. ~~Probe an entry-alloca/by-value header for a proven nonescaping local, retaining boxed headers
   for escaping values.~~ **DONE 2026-07-16.** A conservative whole-MIR proof selects only directly
   bound locals whose loads feed audited non-retaining operations; aliases, user calls, returns,
   closure captures, and unknown uses retain the boxed ABI. Compiler-internal dynamic template /
   `json.encode` builders use the same entry storage because their header is never exposed as an
   operand. Each selected expression/local gets one 64-byte, 16-aligned entry buffer (reused across
   loop iterations or safe reassignment), with compile-time runtime layout-envelope assertions.
   Payload representation, growth, pointer stability, push/write calls, and zero-copy array freeze
   are unchanged. Element-aware unfinished Drop and both consuming paths have runtime + raw-IR
   gates; call/return escape controls pin the boxed fallback.
3. Prefer existing bulk `append(slice)` whenever a source slice exists.
4. Feed future internal `Exact/AtMost` cardinality into one reserve/direct-fill plan. Do not add a
   user capacity parameter merely because the compiler lacks that summary today.

**Measured (2026-07-14, Ryzen 9 5950X, release/native, median of nine):** an exported
`array_builder<u64>` push loop was compared with one existing bulk `append` and an exact
malloc/copy/free direct-fill ceiling. `build()` was included in every builder path and remained
zero-copy.

| elements | per-element push | one append | direct fill | push/append |
|---:|---:|---:|---:|---:|
| 1 | 22.2 ns | 21.3 ns | 9.1 ns | 1.04x |
| 4 | 35.6 ns | 21.5 ns | 8.4 ns | 1.65x |
| 16 | 119.7 ns | 21.9 ns | 8.7 ns | 5.46x |
| 1,024 | 4.90 us | 87.8 ns | 72.1 ns | 55.8x |
| 100,000 | 471.4 us | 14.0 us | 14.4 us | 33.7x |

**Nonescaping-header adoption gate (2026-07-16, same host, release, allocation-inclusive median of
nine):** the checked-in ignored probe compared only header placement, retaining the exact payload
and write/push paths. The 0/1/4-element decision cases all exceeded the required 15% improvement;
the 1K/1M controls stayed within 0.3%.

| kind | elements | boxed header | stack header | boxed/stack |
|---|---:|---:|---:|---:|
| builder | 0 | 10.21 ns | 3.79 ns | 2.69x |
| builder | 1 | 27.66 ns | 19.50 ns | 1.42x |
| builder | 4 | 31.08 ns | 23.78 ns | 1.31x |
| builder | 1,024 | 1.127 us | 1.130 us | 1.00x |
| builder | 1,000,000 | 945.5 us | 947.8 us | 1.00x |
| array_builder | 0 | 9.95 ns | 2.95 ns | 3.37x |
| array_builder | 1 | 18.64 ns | 11.74 ns | 1.59x |
| array_builder | 4 | 28.11 ns | 21.21 ns | 1.33x |
| array_builder | 1,024 | 3.604 us | 3.600 us | 1.00x |
| array_builder | 1,000,000 | 3.435 ms | 3.438 ms | 1.00x |

At 100K, bulk append and exact direct fill are at parity: payload copying is already the right
shape, while the opaque call per pushed element is the large gap. Removing the header helps tiny
builders (append/direct was 2.3-2.6x at 1-16 elements) but saves only about 13 ns in absolute terms.
The shipped header cleanup addresses that fixed tiny-builder cost; next prioritize compiler-selected
bulk/direct fill from cardinality for the much larger loop gap.

The shipped gate required 0–4 elements to lose one allocation and improve by at least 15%, with no
more than 3% regression in the 1K/1M append and push controls. Pin optimized IR for no per-element
call only in the future direct-fill/bulk case — header placement alone cannot remove it.

### 8.2 SHIPPED 2026-07-16 — virtual `chunks` for direct `.len()` and index

`align_rt_chunks` allocates `ceil(len/n) * 16` bytes and fills every `{ptr,len}` header
([runtime](../../crates/align_runtime/src/lib.rs#L1348)). The stored-value and pipeline paths still
use that representation. Before this slice, even an immediate `.len()` or index materialized it, so
one short chunk paid a heap allocation and `chunks(1)` wrote an entire metadata array that its next
consumer immediately reread.

Represent a nonescaping chunk source as `(base, source_len, chunk_len, chunk_count)` and compute each
view in SSA:

```text
start = i * chunk_len
ptr   = base + start * element_size
len   = min(chunk_len, source_len - start)
```

MIR now folds `.len()` to `chunk_count` and direct index to one view. Both guard `n <= 0` before any
division and preserve the runtime's canonical empty result; index then uses the ordinary bounds-fail
path. Fresh owned sources retain their synthetic owner through the returned slice, while scalar
`.len()` drops it after consumption. Raw-IR gates require no materializer/allocation and no header
fill for both direct shapes, and retain `align_rt_chunks` for `cs := xs.chunks(n)`. Runtime gates pin
exact and partial chunks, `n = 0`, bounds failure, and an owned temporary source.

A bound/escaping value and pipeline or `par_map` consumer deliberately continue to materialize.
Virtualizing those iteration sources needs its own end-to-end work/effect-order gate; it is not
required for the direct-consumer result shipped here. The language question in section 10 remains
open, and document 11's later explicit-parallel result elision is related but does not itself remove
these producer headers.

**Measured (2026-07-14, same host/profile):** for the direct consumer `chunks(k).len()`, then-current
materialization versus the virtual count formula took 613.6 ns versus 1.5 ns for 1,024 headers
(`k=1`, 396x), and 37.9 us versus 1.5 ns for 65,536 headers (about 25,000x). Even one header was
9.4 ns versus 1.6 ns. With `k=64`, 1,024 source elements/16 headers still measured 17.2 ns versus
1.5 ns, and 65,536 elements/1,024 headers measured 606.2 ns versus 1.5 ns. This strongly confirms
the `.len()` fold and direct-index virtualization; pipeline consumers still need their own
end-to-end gate because they do real work after producing each virtual view.

### 8.3 SHIPPED 2026-07-16 — write single str-group and dictionary results directly

Generated str-key group-by already allocates `out_keys` and `out_vals` at the row-count upper bound.
The single-aggregate runtime now writes a representative and seed directly at `out_[id]` on a
vacant hash entry, then updates `out_vals[id]` on hits. This removes `reprs: Vec<AlignStr>` and
`acc: Vec<i64>` plus both final copies without changing the hash table, first-occurrence dense ids,
or output ownership. The AoS and two-column SoA entry points share this core.

`dict_encode` likewise writes each vacant representative directly to its already allocated
dictionary buffer, eliminating its staging Vec and final copy. Both paths convert the signed caller
capacity to `usize` before pointer arithmetic, stop before the first out-of-capacity write, and state
the generated input/output non-overlap contract explicitly. A sentinel guard test pins both output
boundaries; the existing AoS/SoA/dictionary and reuse suites pin values and first-occurrence order.

**Measured (2026-07-16, native, 1M rows, consecutive min-of-20 runs):** at 632,390 distinct keys,
four single group-bys (`a1`) fell from 690.0 to 630.4 ms (1.09x) and dictionary reuse (`a2`) from
200.9 to 194.7 ms (1.03x). At 100 and 10,000 groups both stayed within 3%. This is supporting
directional evidence rather than a balanced adoption duel; the deterministic adoption fact is the
removal of three internal Vec allocations and three final copies across the two runtime shapes.

Multi-aggregate group-by deliberately keeps a row-major accumulator for update locality, so do not
scatter it into K output streams without measurement. It can still avoid allocating `ops`/`val_offs`
Vecs from the call's already-present specs.

Add a small-N strategy probe: for `n <= 8/16`, a linear scan over the caller output may beat a heap
HashMap. This remains measure-first and must include duplicate-heavy and all-distinct inputs.

### 8.4 MEASURE FIRST — pool large constant array literals

A 256-element immutable local i64 literal read through a runtime index still produced a
`[256 x i64]` entry alloca and 128 vector stores after O2. The literal lowering stores each element
individually ([MIR](../../crates/align_mir/src/lib.rs#L3758)).

Prefer the already-recorded top-level aggregate-constant feature first. Its backend mechanism can
emit `private unnamed_addr constant` storage. Then measure an extension for local all-constant
literals:

- immutable/non-address-mutated local: read the pooled constant directly;
- mutable large local: one memcpy from a constant template;
- short local: retain inline stores when they are smaller/faster;
- runtime-valued, Move-element, explicitly aligned, or address-sensitive cases remain on the
  existing path.

Sweep 1..4096 elements around L1/code-size/frame thresholds. Require at least 15% on the positive
large case, no O(N) store sequence, and <=3% regression below the chosen cutoff.

### 8.5 SHIPPED / ALREADY PLANNED — keep one owner

- stable O(n log n) sort/sort_by_key with a tiny insertion base and once-decorated keys is shipped;
  document 12 adds the measured ordered-run/tiny-scratch refinement;
- donate a uniquely owned unbound temporary buffer to compatible map/where/scan materialization:
  document 10 §8.1; extend to sort only through the same ownership proof;
- SIMD stable compaction for selective materializers: document 12, measure first;
- redundant `.to_array().sum()` lint/legality-aware elision: document 12;
- whole-range `par_map` and explicit-parallel terminal elision: document 11;
- exact/AtMost cardinality and `range(n)` are already consumer-gated design work.

## 9. Allocation/copy matrix

| Operation | Payload allocations today | Full payload copies after production | Target shape |
|---|---:|---:|---|
| empty `str.clone()` | 0 | 0 | keep |
| nonempty `str.clone()` | 1 | 1 input→final | keep; ownership requires it |
| `builder.to_string()` | grow buffer (Box only on conservative fallback) | 0 | keep allocator-compatible zero-copy freeze |
| template in arena | grow buffer + arena | 1 grow→arena | direct arena/sink fill after ownership settlement |
| template outside arena | grow buffer | 0 | keep zero-copy owned freeze + scoped free |
| `path.join` | 1 exact final | two input runs→final | keep; add checked total length with document-12 hardening |
| `path.normalize` | 1 exact-upper-bound final | 0 | shipped direct fill |
| `fs.read_dir` / DNS, N names | N final + header + header Vec | 0 | shipped direct payloads + RAII unwind |
| Base64/hex encode | 1 exact final | 0 | shipped document-12 direct destination |
| JSON decoded string field | 0 per field | 0 | keep zero-copy view |
| JSON decoded array | parser Vec + final | Vec→final | retain one-pass staging; exact-count scalar probe regressed large inputs |
| `to_array` | 1 output | source→output fill | keep; donation only for proven unique temporary |
| `partition` | 2 outputs | one store per source element | keep baseline |
| `array_builder.build()` | grow payload (Box only when escaping) | 0 | keep nonescaping header elision + freeze |
| direct `chunks(...).len()` / `[i]` | 0 | 0 / one direct source view | shipped virtual lowering |
| stored/pipeline `chunks` | 1 header array | header fill then consumer read | retain; measure fused iteration separately |
| str-key single group | 2 result + HashMap | 0 | shipped direct result accumulation |
| `dict_encode` | ids + dictionary + HashMap | 0 | shipped direct dictionary representatives |

## 10. Questions for Claude Code — not decisions

The user explicitly asked that language-spec changes remain optional. No new syntax was adopted by
this audit. The first question below was settled on 2026-07-15; the others remain open for comparison
against One way, Nothing hidden, predictable performance, and inferred regions:

1. **SETTLED 2026-07-15 — where does a template result live?** The surface remains `str`; outside an
   arena a hidden scoped `string` owner backs dynamic results, while static-only templates are pooled
   literals. Returning the dynamic view is rejected. Contextual sink fusion remains optional and
   does not define correctness.
2. **What does `.to_array()` promise?** Is it a guaranteed fresh physical allocation, or a visible
   owned result whose backing may be donated from a provably unique, unobservable temporary? The
   latter would legitimize document 10's planned donation without new syntax.
3. **Is `chunks` a virtual pipeline stage or an eagerly owned array?** Prefer compiler elision for
   direct consumers first. Only consider requiring `.to_array()` for storage if that makes the
   source model materially clearer and the compatibility cost is acceptable.
4. **Should `split` start virtual?** The already-specified but unimplemented ideal result is borrowed
   `array<str>` views, never copied owned substrings. Could `split(...).where/map/reduce` be a virtual
   source and materialize headers only at `.to_array()`?
5. **Does `array_builder` need a capacity surface?** First evaluate internal Exact/AtMost inference,
   bulk append, and nonescaping headers. Add `array_builder(capacity)` only if real consumers still
   show realloc-bound behavior that inference cannot express.
6. **Does repeated dynamic search need a visible compiled Pattern?** First try loop-invariant
   memmem Finder hoisting with no language change. A new type is justified only if profiling shows a
   common case the compiler cannot safely recognize.
7. **Resolve existing clone text drift.** Some prose suggests arena-local clone allocation, while
   implementation and escape examples require `str.clone()` to be heap-owned and returnable. Decide
   and make the documents agree; this audit does not recommend changing the current heap behavior.

The default answer should be “implementation-only” wherever ownership and result identity are not
observable. Do not add SSO, hidden small-vector storage, a second concatenation operator, automatic
AoS/SoA conversion, or a second substring-search algorithm.

## 11. Implementation sequence

### C0 — invariants and resource ownership

1. ~~Enforce UTF-8 range boundaries~~ **DONE 2026-07-13**; ~~ship the specified zero-cost
   `s.bytes()` view~~ **DONE 2026-07-15**.
2. ~~Enforce the settled `str + str` hard error and correct stale tests/docs.~~ **DONE 2026-07-15.**
3. ~~Add owned expression temporaries/synthetic owners with view-aware liveness; close string,
   array, chunks, and builder direct-consumer leaks.~~ **DONE 2026-07-15.**
4. ~~Remove definite-null destructor calls after the ownership dataflow is trustworthy.~~ **DONE
   2026-07-15.**
5. ~~Settle arena-free template/json.encode ownership and fold static-only templates.~~ **DONE
   2026-07-15.** Direct sink fusion remains a separate P2 copy/call optimization.
6. ~~Complete document 12's checked dynamic allocation-size arithmetic.~~ **DONE 2026-07-13.**

### P1 — short fixed costs

1. ~~Borrow filesystem/path ABI strings and construct C strings directly.~~ **DONE 2026-07-16.**
2. ~~Add the audited `array_builder_new` allocator attributes.~~ **DONE 2026-07-16.**
3. ~~Run M14's runtime-bitcode ceiling probe on short string equality/order/hash.~~ **DONE
   2026-07-14.** `str_eq` cleared the gate, `str_cmp` regressed and remains excluded, while
   `hash64` benefited from native tuning rather than LTO visibility; M14 Slice 2 shipped the guarded
   memcmp-class set behind `--rt-lto`.
4. Establish the UTF-8 scalar/SIMD crossover per target. **DONE 2026-07-16:** x86-64 uses scalar
   below 32; native Apple-M1 NEON had no no-regression length-only crossover, so aarch64 production
   remains scalar and the negative result is closed.
5. ~~Prototype nonescaping builder/array-builder headers separately from payload changes.~~ **DONE
   2026-07-16.** Proven local and compiler-internal template headers use aligned entry storage;
   escaping/call-crossing values remain boxed. Payload changes remain a separate P2 item.

### P2 — remove proven staging

1. ~~Make owned builder freeze allocator-compatible and zero-copy.~~ **DONE 2026-07-16.**
2. ~~Virtualize direct-consumer `chunks`.~~ **DONE 2026-07-16** for immediate `.len()` and index;
   stored/escaping and pipeline/`par_map` values retain the owned materialized representation.
3. ~~Write single str-group and dictionary outputs directly.~~ **DONE 2026-07-16.**
4. ~~Direct-fill `path.normalize`, `read_dir`, and DNS final payloads.~~ **DONE 2026-07-16.**
5. ~~Execute document 12's codec exact-destination slice and the roadmap JSON-copy probe.~~ **DONE
   2026-07-16.** Base64/Base64url/hex now fill one checked exact final allocation; the balanced probe
   improved every short case and 64 MiB by 1.70-1.86x. An `array<i64>` count-then-direct JSON probe
   regressed 1K-1M elements to 0.71-0.73x, so the one-pass parser Vec remains. Native Apple-M1
   Base64/Base64url subsequently cleared their independent 48-byte NEON crossover, and native ARM
   hex separately cleared its 16-byte NEON crossover.

### P3 — larger portfolios after measurement

1. Add document 12's measured total-order ordered-run sort path while retaining the tiny insertion
   base case; remove only scratch that no merge pass can read.
2. Pool large constant array literals after the top-level aggregate-constant surface exists.
3. Run unique-buffer donation, repeated-needle plan, JSON escape scan, and short-N group strategy
   gates independently.

## 12. Regression and IR gates

Correctness/resource mutations must fail when any of the following is removed:

- UTF-8 start/end continuation-byte check (**shipped and regression-pinned 2026-07-13**) and
  descriptor-only `s.bytes()` (**shipped and regression-pinned 2026-07-15**);
- hard error for `str + str` (**shipped and regression-pinned 2026-07-15**);
- synthetic owner/drop for an unbound Move temporary;
- owner lifetime extension for a borrowed result view;
- arena-free template ownership, local lambda consumption, and rejected frame-view escape
  (**shipped and regression-pinned 2026-07-15**).

IR gates:

- no allocation for static-only template (**shipped and regression-pinned 2026-07-15**),
  `str.bytes()`, string subview, slice, trim, or direct chunk `.len()`;
- no `*_free(null)` for definitely moved slots (**shipped and regression-pinned 2026-07-15**);
- direct-consumer chunks contain no header allocation or `N*16` header-store loop (**shipped and
  regression-pinned 2026-07-16**);
- bulk/direct array builder contains no per-element push call;
- str-group single aggregate and `dict_encode` contain no accumulator/representative staging copies
  (**shipped and regression-pinned 2026-07-16**);
- mapped materializers retain `min.iters.check` and a scalar short path;
- large constant-array positive case uses a private constant and direct read/one memcpy, while the
  short control retains the winning inline shape.

Benchmark adoption requires the matrix in §4.2, balanced AB/BA, allocation/copy counters, optimized
IR, and a negative workload. Large-input throughput never excuses a short-input regression, and a
short microbenchmark never justifies code-size or memory-traffic loss at scale.

## 13. Relationship to existing records

- document 10 owns artifact caching, unique temporary donation, AoS→SoA blocking, and cache-sensitive
  benchmark methodology;
- document 11 owns parallel correctness, range kernels, scheduler contention, and explicit-parallel
  terminal elision;
- document 12 owns allocation arithmetic, arena initialized-before-read classes, exact codecs,
  stream compaction, I/O syscall paths, and SIMD portfolio rules;
- the roadmap owns M14 runtime bitcode, hybrid sort replacement, tiny par-map startup, and the JSON
  double-allocation probe;
- `open-questions.md` remains the sole decision ledger. Section 10 above must be copied there only
  after a real decision, not because this audit asked the question.

The new contribution of document 13 is the short-input evidence and the confirmed ownership/copy
gaps: the now-fixed UTF-8 slicing defect, settled concat enforcement, the now-fixed arena-free
template lifetime and unbound temporary drop, known-null drops, borrowed ABI paths,
builder/array-builder headers and freeze, virtual chunks,
direct str-group outputs, direct path normalization, staged name/IP payloads, and large constant-local
initialization.
