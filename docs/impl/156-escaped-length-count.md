# Count percent/form expansion before checked sizing

Percent/component, percent/path and form encoding previously performed a checked
addition for every input byte while counting exact output length. Count bytes
that expand to three output bytes, then check `input length + 2 * count` once.
The count is bounded by the input slice length. Preserve the existing predicates,
checked final arithmetic, one exact output allocation and safe writer callbacks.
This follows plan123's established destination/publication strategy.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Exact length and bytes | Unreserved bytes cost one; path additionally preserves slash, and form space costs one as plus. Every other byte costs three. The independent encoding_writes_tests reference output supplies exact lengths for all three counters, arbitrary bytes, Unicode, empty and boundary inputs. Existing writer sentinels and mismatched-destination owners retain the initialized-output proof. |
| Overflow, allocation and publication | The number of matching bytes cannot exceed the valid slice length. Checked multiply/add reject unrepresentable output before the unchanged owned_str_exact allocation. No intermediate output, new unsafe code, extra copy or owned descriptor. Counter differences cannot be hidden: the existing final writer count rejects a wrong destination extent before publication. |
| Consumers and unchanged siblings | m10_encoding owns all three source encoders and returned string cleanup. HTML counting/writing is unchanged and remains a benchmark/control-owner sibling. No source/IR/ABI/interface/cache or type/control-flow change. |
| Measurement | Reuse encoding_writes with independent expected bytes and producer-observed call/length/byte counts, extending empty/short and dense controls. Compare ordinary release archives with identical C/flags in ABBA order on macOS/Linux ARM64, without overlapping builds/tests; keep every observation and report regressions. Adopt only with useful long-input improvement and acceptable short controls. No timing gate, application/RSS or portable speed guarantee. |
| Probe lifecycle | Route the existing encoding_writes entry through the shared bounded runner and runtime-producer refresh. Extend the existing parameterized wrapper lifecycle owner to that entry; retain the real stale-archive producer owner. No new process/signal ownership strategy. |

The author matrix-to-diff pass and one fresh inspection-only implementation
review close this representation-preserving refinement. No public contract,
allocation policy, safety strategy or normative language mirror changes.

## Qualification

The exact counter/writer invariant owner and all 18 `m10_encoding` source tests
pass on macOS and Linux ARM64. The existing native-probe owner passes all six
wrappers across build/link/probe success, failure and termination paths, plus
real stale-runtime refresh. The expanded C harness is warning-clean.

[The measurement record](../../bench/encoding_writes/README.md) retains all
2,736 observations per host and the full corpus/producer-count rules. Long
path and form cases improve on both hosts. The largest short-case increases
are 9.6 ns for macOS dense component encoding and 1.4 ns for Linux's unchanged
HTML control. These costs are accepted without a portable speed guarantee.
No source language, allocation/ownership or native ABI contract changes.
