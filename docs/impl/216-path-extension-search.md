# One reverse search for path extensions

`path.ext` currently locates the last component and then scans that component
again for its final dot. Its result is determined by the last dot-or-slash
before trailing slashes: a slash means no extension; a dot starts an extension
unless it is the first byte of the component. Qualify one reverse two-byte
search through the existing `memchr` dependency at that private seam.

The public view contract stays in `draft.md`'s `std.path` section and roadmap
M9 Slice4. Empty/no-extension results, dotfiles, repeated/trailing separators,
all-slash roots and byte-exact UTF-8 subviews keep their existing behavior.
No allocation, copy, filesystem access, ABI, region, ownership or cleanup change.

| Obligation | Implementation and acceptance owner |
| --- | --- |
| Exact selected view | After unchanged empty/trailing-slash admission, search only the admitted prefix. A final slash or no delimiter yields the original empty view. A final dot at zero or immediately after slash is a dotfile; otherwise return the same suffix pointer and length. |
| Input and lifetime | Retain existing extent/empty-view handling and safe slice bounds. The result remains a subview of the live input. No new raw range, allocation, source mutation, retained address or cleanup path. |
| Semantic owners | Existing native `path_ext_cases`, plus a parameterized independent scalar reference across all delimiter placements, Unicode and long components, check exact pointer/length and bytes. Existing driver `m9_path_env_time` owns source edge semantics and input-bound region refusal. |
| Allocation | Existing native PureArgRead allocation inventory includes PathExt; no new allocation mechanism. |
| Measurement | Qualify actual non-test release producers with the same direct C fixture, fixed byte-counted path/offset/length goldens and checked call/checksum totals. Include short ordinary paths, long names without extensions, long extensions, deep prefixes with short tails and root/dot/Unicode/trailing-slash controls. Base/dir are unchanged operation controls. Two full per-cell ABBA passes, seven samples per process, no discarded samples or concurrent builds/tests. Require at least 20% improvement for a long-tail extension group, with no cell regression above both 5% and 0.5 ns. Preserve producer identities and every sample; no portable/application claim. |
| Probe ownership | Qualification and reproduction reuse the existing bounded native build/link/run supervisor and process-group cleanup. An adopted wrapper joins its parameterized lifecycle owner. |

This matrix precedes implementation. The change is one private search below
1,000 handwritten lines, following the settled view/byte contract. One fresh
independent preflight review is sufficient if the measured candidate qualifies.

The native exact-subview owner matches its component-based reference for 29,524
short delimiter arrangements and 2,496 long/misaligned cases; the existing
native edge and no-allocation owners pass. The [measurement record](../../bench/path_ext/README.md)
retains 840 samples from two full comparisons. All five extension groups improve
12.4–95.6%; the largest positive base/dir control change is +0.083 ns (0.35%).
Every cell satisfies the predeclared bounds, so the candidate is adopted.
All 18 `m9_path_env_time` source owners pass after a workspace build. The shared
runner's five owners pass, including 225 phase/signal lifecycle cells across
15 registered probes and the new wrapper's strict compiler flags and selectors.
