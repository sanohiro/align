# Conditional owned-record construction qualification

## Selected boundary

The owner selected R131's remaining evidence-dependent investigation on
2026-10-28. The current contract already admits owned record construction with
conditional scalar fields. This work qualifies that contract; it does not
relax producer certification or change ownership, source order, ABI or allocation.

The historical failure reported an uncertified String at
`[StructField(0), StructField(3)]` in an imported session constructor. The
retained complete source is the successful scalar-hoisting workaround, not
the lost failing snapshot. Replay the authenticated retained source and an
explicitly reconstructed two-hoist reversal separately. A reconstructed
failure is evidence for that source, not proof of the lost source's identity.
Private consumer sources and raw logs remain outside public Git.

## Qualification matrix

| Boundary | Owner and evidence |
| --- | --- |
| Formation and imported construction | `conditional_record_construction` constructs a nested owned String at the reported field path beside buffers and optional owned records. Both scalar branch outcomes and source/declaration-order permutations must certify in whole and per-unit compilation. |
| Evaluation and publication | Scalar conditions and fallible later fields run once in written order. A completed record returns all selected scalar and owned values intact. No source-order hoist is required. |
| Partial construction and return | A later failed `?` must release fresh siblings and retained inputs exactly once. Positive allocation/free counts and zero remaining requested bytes distinguish cleanup from a vacuous successful exit. |
| Replacement, moves and Drop | Repeated construction, complete-owner replacement and return preserve the nested text, buffer contents and active optional payload. Existing `deferred_aggregate_transfers` and `owned_field_replacement` own duplicate-move and invalidated-source refusal. |
| Interface and replay | The same public imported constructor runs through whole-program and per-unit lowering. Existing interface/cache owners retain serialization and cache identity; this qualification changes neither. |
| Real consumer | Authenticate the retained source files against their receipts, restore the omitted internal resource module from the fixed consumer commit, and record the exact compiler and source variants for checks and native object/link verification. Current acceptance and historical causal attribution are separate conclusions. |
| Limits | No performance promise, GPU/model quality claim, language widening or consumer adoption follows from compiler and counted native construction owners. |

The independently authored owner contains no private consumer implementation.
No compiler implementation change is selected unless a valid current witness
fails. Existing safety analyses and rejection owners remain authoritative.

## Qualification result (2026-10-28)

Linux x86_64 (WSL2, Ryzen 5950X), compiler `b98654bf`, LLVM 22:

| Evidence | Result |
| --- | --- |
| Retained source identity | 381 of the receipt's 382 top-level sources match unchanged. The later model-smoke source matches the separate selected receipt. The omitted internal resource module is restored from consumer `59a7deed0320c07ac38e3fb3129931ccea079ac6`. |
| Historical compiler `b20429be50d6ab889496a0589143320683b29aeb` | Both retained hoisted and reconstructed inline variants pass `check` and `check-per-unit`. Neither reproduces the original failure, even on its original compiler. |
| Current model root | Both variants pass `check`, `check-per-unit`, and `emit-obj --no-rt-lto` (24 objects each), with cache disabled. |
| Complete native roots | The reconstructed inline `native_cuda_model_smoke` and `native_cuda_main` both build and link against the retained CUDA loader with `--no-rt-lto` and cache disabled. These executables are not run. |
| Native constructor | Both variants link and execute the actual imported `create_split` using the retained Qwen3.6 model, pack and IR metadata. Schemas 3/4/5 preserve owned source identity, conditional limits, buffers, graph identity and optional-host segment storage. Active requested-live accounting observes positive live storage during construction and returns to baseline after every session; frees increase. |
| Public regression owner | `conditional_record_construction` passes whole-program and per-unit native execution across both conditional outcomes, both field orders, success/failure and repeated replacement. Positive allocation/free deltas agree and requested live bytes return to zero. Existing `deferred_aggregate_transfers` and `owned_field_replacement` also pass. |

The native constructor caller supplies inert Copy tensor descriptors in owned
containers. It creates no device and submits no GPU work; it qualifies CPU
session construction and destruction, not loaded-device behavior or model
quality. The public owner additionally covers fallible partial construction.
Private receipts, exact binary/source hashes, commands and logs are retained in
the provider's `r131-20261028` evidence archive. The generation source digest is
`7ac1265af6586d292824b3fcd172fafe5553d7e042f0f48981c57157230a4832`;
the reconstructed variant changes only the two documented scalar hoists.

Provider disposition: the current constructor boundary is qualified, with a
durable regression owner and no reproduced defect requiring a compiler change.
The unavailable failed snapshot prevents historical causal attribution; archive
that limitation rather than leave a speculative repair selected. Consumer
adoption and full device-session acceptance remain consumer-owned.
