# Sparse partial-permutation sampling

`rng.sample(xs, k)` currently initializes all n source indices even for a small
sample. Represent the same partial Fisher-Yates permutation sparsely when
k <= n / 512, retaining the dense contiguous representation otherwise. This
is an internal representation choice, with no public API or language change.
Before accepting the threshold, compare actual baseline/candidate release
runtime entry points on macOS and Linux, including both sides of the threshold.

## Implementation closure matrix

| Obligation | Implementation / owner |
| --- | --- |
| Exact draws and RNG advancement | Both representations use the existing bounded draw for suffix [i,n). A sparse map holds only displaced unselected slots; missing slot j means value j. Remove selected j; move the current i value into j when distinct; discard i forever. Test arbitrary scripted j selections against a full permutation, plus seeded complete output and final state against the existing dense oracle. |
| Determinism and effects | Use HashMap with explicit BuildHasherDefault<DefaultHasher>; no OS seed, ambient cache or iteration order affects output. Existing mutable rng/effects remain. |
| Empty/full, self-swap, collisions, moved slots | Exhaust all legal pick sequences on small n, across k=0..n; include self-selects, selecting an already displaced slot, retiring a displaced i, repeated j, and all source elements selected. Test sparse helper directly as well as actual dispatch. |
| Index and byte safety | k admission and checked output byte count precede allocation/RNG use exactly as today. Suffix draws imply i<=j<n, and the partial-permutation invariant implies selected<n. Keep the existing raw copy into the separate checked output allocation; source is borrowed for the call. No new raw storage or unsafe allocation strategy. |
| Ownership and cleanup | Map/Vec are ordinary safe Rust temporaries; output retains align_rt_alloc/free provenance and atomic return. Source never mutates; Copy element bytes, including str descriptors, retain the existing caller-inferred lifetime. Existing native/driver owners close returned arrays/Drop. No new type, IR, ABI, serialization, generic or compiler path. |
| Scratch resource claim | Sparse representation reserves k entries, has at most k insertions, stores no source payload, and allocates no n-sized Vec. Feature-gated thread-local real allocator byte/count observation compares fixed k across n and validates no growth; output allocation remains unchanged. Measure scratch separately from input/output/RSS. |
| Source and compilation | Existing m10_rand owns deterministic pinned sample, invalid k, zero output, Copy rng, effects and Move-element rejection. Add a source whole/per-unit sparse consumer with deterministic selected bytes and subsequent RNG outputs. |
| Latency | Non-test release-runtime C harness validates full output and final RNG state against an independent dense oracle before timing; every timed call checks output length, count, first/last sampled elements, final state, and frees output. Cover empty/small/dense, large n with k=1/8/128 and threshold neighbors. Identical compiler flags; balanced AB/BA and distinct producer hashes. No application/RSS guarantee. |

The change follows the existing checked-output/ordinary Rust scratch strategy.
An author matrix-to-diff pass and one independent full-diff preflight review own
this boundary. No separate public/safety-strategy design review is required.
Expected change is below 1,000 hand-written lines. Only the roadmap's existing
O(n)-scratch status sentence needs updating at the capability boundary; the
settled rand specification remains unchanged. K1 stays deferred.

## Owner closure

`sample_permutation_all_small_draw_sequences_preserve_the_suffix` enumerates
all legal draw sequences through seven elements, asserting the entire remaining
suffix and retirement of selected slots after every step.
`sample_permutation_dispatch_matches_dense_output_and_final_rng` compares
complete results and state across empty/full draws, seeds and dispatch boundaries.
`sample_permutation_native_copy_is_independent_for_every_stride` owns 1/3/8/24-byte
native payload copying, unchanged source and independent returned storage.
`sample_permutation_sparse_scratch_depends_on_k_not_n` observes real successful
Rust allocation calls/bytes, with a separate allocation witness. For n=65,536 and
1,048,576, k=1/8/128 requests 76/280/4,360 bytes respectively in one allocation;
k=0 requests none. These exclude unchanged source/output storage and are not RSS.
The probe is cfg(test)+alloc-count only, so it cannot alter measured release code.

`sparse_sample_imported_execution_preserves_output_and_rng` builds imported
whole/per-unit programs in an exclusively acquired ArtifactStage. It checks eight
pinned samples and the subsequent RNG output; a bounded child guard owns kill/reap
before any fallible post-spawn work. Existing m10_rand and the sampled-view
borrow-liveness owner cover unchanged admission, effects and lifetime rules.
No legacy predictable TempProject or unbounded execution helper is used by the
new owner. Both native and actual-source owners pass on macOS and Linux.

The scratch owner rejects a mutation that restores the pre-change dense path
for every nonempty draw: n=65,536/k=1 requests 524,288 bytes, exceeding its
resource assertion. The restored candidate passes all four native owners.

## Performance acceptance

Balanced AB/BA actual-release-runtime measurements qualify the 512:1 dispatch
threshold on macOS/Linux ARM64. At n=512/k=1, medians fall 82.70→60.67 ns and
128.52→70.53 ns respectively. At n=1,048,576/k=128, they fall 212.61→3.11 µs
and 177.71→3.05 µs. The largest observed short/dense increase is 1.30 ns;
empty/short behavior is otherwise effectively unchanged or improves. Complete
corpus, clocks, counts and reproduction are in
[`bench/rand_sample`](../../bench/rand_sample/README.md). No application/RSS
promise is made. Qualified fast-case repeat counts exceed clock resolution;
earlier under-repeated exploratory samples were discarded. Both producer logs
show actual compilation and distinct binary hashes under one harness hash.
