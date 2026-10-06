# Write CLI usage directly into its owned result

The existing `examples/http_fetch.align`, `http_sse_watch.align`, `file_sha256.align`
and `tree_summary.align` render `command.usage()` for help. The native renderer
first grows a Rust String, allocates one temporary decimal string per integer
default, and then copies the complete text into the owned Align result. An
actual allocator probe of the exported entry records 3/10/74/1038 Rust
allocation events for 0/4/64/1024 integer flags on both macOS and Linux ARM64. The returned C-owned payload
is outside that counter.

Render the existing text through one private formatting routine and an
allocation-free callback sink. A checked counting pass admits the complete
length, then `owned_str_exact` supplies the sole final output payload to the
same renderer. The write pass checks every destination extent and verifies
complete initialization before publishing. Decimal spelling continues to use
Rust Display; names/default text, registration order, duplicate flags, newlines
and embedded NUL remain verbatim. Null-command usage remains the empty null
string. No new public contract, allocator family, native ABI, effect, compiler
record, ownership model or safety strategy is introduced.

| Closure axis | Implementation and owner |
| --- | --- |
| Input and byte semantics | Existing constructor/registration normalization is unchanged. A native owner compares complete usage bytes for empty/nonempty names and flag tables, all three flag kinds, duplicate names, Unicode, embedded NUL/newlines and lossy native registration input. |
| Counting and complete writes | One private rendering routine serves count and write passes. Checked addition and isize/i64 conversion precede allocation. The write sink rejects excess fragments, counts initialized bytes and refuses publication if the final count differs. A synthetic length-limit owner tests arithmetic refusal without huge memory; decimal boundary values and long strings exercise exact bytes. |
| Independent returned ownership | Existing owned_str_exact/align_rt_free ownership remains. Native owners read multiple returned strings after dropping or modifying the command. Existing m10_cli owners cover parse success/failure, usage after error, moves and returned string destruction. |
| Allocation discrimination | An exact-filter subprocess isolates the actual Rust allocator counter from other tests that enable/reset the process-global requested-live probe; an independent live allocating witness validates the counter; the complete exported entry must allocate no Rust temporary storage for 0/4/64/1024 integer flags and long text. Before/after standalone probes count the same entry. The final Align result allocation remains required; no latency, throughput, RSS or C allocator-internal claim is made. |
| Compiler and consumers | No type/IR, generic, interface/cache or generated Drop changes. Existing m10_cli and actual help consumers retain their behavior; a driver owner asserts complete usage bytes rather than partial containment. |

The author matrix-to-diff pass and one fresh independent preflight inspection
close this existing allocation strategy. No separate public-design review or
language mirror update is needed.

The allocation owner runs in an exact-filter subprocess with a direct kill/reap
guard, one 15-second deadline and five seconds reserved for cleanup. It spawns
no descendants and uses no captured pipes. It initializes the feature-only requested-live probe with an
unrelated one-byte allocation before measuring, without warming CLI rendering;
its mutex can allocate lazily on macOS. A separate ordinary-library probe
includes the cold first CLI call and confirms that this initialization belongs
to instrumentation rather than the production renderer. The candidate records
zero temporary Rust allocations for all four cases on macOS and Linux ARM64.
