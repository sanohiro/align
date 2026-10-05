# Launch PATH-search storage

## Consumer and boundary

Baseline: `aa5202232e3e4bdca6f0957098322e3d287ebea1`.
Ordinary process commands, including the direct executable paths in the
multimodal worker composition and process-output example, share `Prepared::new`.
It currently copies the final environment's PATH into a Vec before discarding
that Vec for direct paths and retained executable images.

Choose the existing target discriminator before constructing that scratch.
Direct paths and images use an empty Vec; only a searched executable copies
PATH or queries `_CS_PATH`. The final environment is still formed, validated
and passed to the native child in the same order. No PATH caching is introduced.
The direct-path candidate CString, environment/argv ownership, retained-image
and inherited descriptors, Linux bootstrap and Darwin spawn remain unchanged.

This is private safe-Rust allocation refinement of plans 49/50. There is no
public signature, ABI, new lifetime, native safety strategy or compiler change.
The author matrix pass folds boundary inspection into one fresh full-diff
preflight review rather than requesting another design review.

## Implementation closure matrix

| Invariant | Implementation and owner |
| --- | --- |
| Final inherited/cleared/overridden environment | Existing variables construction remains; `native_launch_error_and_final_environment` owns native PATH override/search and direct launch errors. New preparation owner pins exact final PATH bytes and argv terminators without spawning. |
| Direct target and retained image | Target guard precedes only discarded search scratch. Preparation owner checks both kinds (image on Linux), direct singleton and image-empty candidates; existing verified-image and source process owners retain execution/descriptor identity. |
| Searched target and absent/empty PATH | Original copied search bytes, `confstr`, candidate ordering and empty-prefix semantics remain; parameterized preparation owner plus existing native search/errno owner. |
| UTF-8/NUL and failure order | Override conversion remains before search; final environment validation remains before candidate construction and descriptor acquisition. Invalid override input owners check rejection even when search is unused. |
| Construction, moves, Drop and partial acquisition | All owning fields/descriptors and return paths unchanged. Existing partial-launch acquisition owner retains cleanup; no new shared/process-global state. |
| Whole/per-unit and synchronous/live consumers | Compiler/interface/ABI unchanged; existing process source owners, including the actual process-output example, retain coverage. |
| Actual allocation evidence | `alloc-count` preparation probe uses the runtime's existing thread-local whole-Rust allocator on actual `Prepared::new` calls; a positive allocation witness is required. Compare exact baseline/candidate on fixed explicit environments and both target kinds. No whole-launch latency or RSS claim. |

Run native process owners on macOS and Linux, the source live-process owner,
and the separate allocation probe. An old eager-search-copy mutation must
exceed the candidate's measured allocation budget on nonempty PATH.

## Local allocation evidence

The actual preparation probe uses a cleared environment with one explicit
4096-byte PATH, three fixed arguments and no inherited descriptors. Configuration
and image-source fd construction are outside the interval. The image probe only
exercises preparation/duplication; existing verified-image owners supply executable
admission and native execution coverage.

| Target/host | Baseline successful Rust alloc/realloc calls | Candidate |
| --- | ---: | ---: |
| Direct `/bin/sh`, macOS | 14 | 13 |
| Direct `/bin/sh`, Linux ARM64 | 14 | 13 |
| Retained image, Linux ARM64 | 12 | 11 |

These are actual complete `Prepared::new` call counts, not peak-live bytes,
libc-internal allocation, whole-launch RSS or speed. Exactly the unused PATH
search Vec disappears; the final environment and all native ownership remain.

A normal macOS release build without test/counter instrumentation also retains
the baseline search allocation and memcpy before the image/direct-path guard.
In candidate release IR the guard bypasses the PATH lookup/allocation blocks;
the searched-name path retains its allocation. This checks that the removed
work was not already eliminated in the ordinary optimized runtime. It is not
a dist/LTO, throughput or peak-memory result.
