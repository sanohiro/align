# Map admitted prefixes in expanded form output

Plan177 maps complete form output when its checked expansion count is zero.
Plan178 copies ordinary prefixes in expanded percent output. Expanded form
output still applies the scalar branch chain to a long prefix of unreserved
bytes and spaces before a final escape. Evaluate the same bounded classifier
with form grammar, then map spaces to plus in those admitted output slots.
The remainder keeps the existing scalar form writer.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Admission and transformation | Only zero escaped_byte_count<false,true> over complete 128-byte chunks admits unreserved bytes or spaces. Every admitted space becomes plus; plus, percent, slash, NUL and non-ASCII remain escapes. A shared same-width prefix helper must preserve both percent specializations with SPACE=false. The all256 count oracle and shared prefix fixtures distinguish all three grammars. |
| Work and extent | Keep the 512-byte minimum and disjoint chunks_exact(128), stopping before the first escaping chunk. A dense input adds at most one classification; no repeated tail scan. Safely split the destination, initialize the admitted prefix exactly once, and pass corresponding source/output suffixes to the unchanged scalar form writer. Its bounds and final assertion retain short/long destination refusal. The two-sentinel private owner exercises the complete wrapper. |
| Complete initialization | Factor the existing same-width space mapping only if both callers prove equal source/output extents; retain a concrete extent assertion in that helper. The public zero-expansion branch and prefix split supply those extents. No raw pointer, unsafe operation, temporary buffer or allocation is added. |
| Allocation and publication | Byte-view admission and full checked sizing precede the same exact owned allocation; publish only after filling. Empty output remains null without allocation, other outputs own one independent payload freed once. The isolated public owner retains positive allocation witnesses, independent bytes, extent, ownership and Drop checks; existing source owners retain return/import/round-trip behavior. |
| Compiler and controls | No syntax, type, IR, ABI, layout, cache, generic, borrow, branch or cleanup change. Percent/component, percent/path and HTML are measured controls. K1 and general aggregate provenance remain deferred. No normative specification or EN/JA mirror promise changes. |
| Measurement | Rebuild identified ordinary-release producers and link the identical C harness in both arms. Extend the existing 240 cases with space-only and mixed-space prefixes ending in percent, at every existing size and for every encoder. Preserve independent full-byte oracles, native extent/work counters, ABBA ordering, all 10,080 observations and producer identities. Run without concurrent local builds/tests through native_probe.run_phase; its existing deadlines remain. |
| Adoption limits | Before measurement, require at least 20% improvement for each 4/64-KiB form late-escape case, including all-ordinary and both added space cases. Other long form cases must cost at most 10% and 1 microsecond; short form at most 10 ns. Unchanged controls must remain within 10% on long cases and 10 ns on short cases. Record linked text/data sizes; admit at most 16 KiB of text growth and no new startup initializer. Retain rejected evidence if these local limits fail; timing never becomes a correctness/CI gate or portable/application/RSS guarantee. |
| Lifecycle and verification | Emit retained CSV with explicit LF; check every raw observation, parsed identity and staged/committed whitespace. Run the four encoding_writes_tests owners with alloc-count and eighteen m10_encoding source owners after rebuilding the workspace. Complete author extracted-obligation/matrix review and one fresh independent full-diff review; final-SHA preflight owns the bounded gate and Clippy. Retain compact evidence and retire temporary producers after their last consumer. |

This refinement preserves the existing public and allocation/safety contracts.
A separate design review is not required. One complete form capability closes
the distinct mandatory space transformation; the shared classifier retains its
existing percent consumers in the same proof and owner coverage.

## Author measurement revision

The first shape passed all correctness owners and improved long late-escape
form cases 60.2–82.0%, but two 64-KiB repeated-quote controls added 2.617 and
1.984 microseconds. Those exceed the preregistered 1-microsecond absolute bound;
reject that shape and retain all 10,080 observations. Inspection shows that the
native form entry's disassembly grew from 393 to 591 lines, with both expanded
prefix preparation and scalar writing inlined. This observation does not prove
the cause of the regression. Evaluate one revision that keeps the complete
expanded wrapper out of line, separating it from the native entry's checked
count and same-width output path. Repeat the identical comparison and apply
the same adoption limits; if it fails, defer this candidate instead of tuning
those limits or iterating over code placement.

## Qualification

The final out-of-line wrapper preserves the admitted-prefix transformation and
unchanged scalar remainder writer. Existing two-sentinel mismatch and isolated
public allocation owners now exercise it directly. Shared fixtures distinguish
ordinary, slash, space and mixed-space inputs, with expansion bytes before,
at and after chunk boundaries and around the 512-byte threshold. Four native
owners with alloc-count and eighteen m10_encoding source owners pass after a
workspace rebuild; the public language/ABI/ownership contract is unchanged.

The [measurement record](../../bench/encoding_writes/README.md) retains both
complete 10,080-sample comparisons and every producer identity. The final six
long late-escape form cases improve 59.1–81.7%. Other long form cases cost at
most 5.33% and 0.867 microseconds, short form at most 2.25 ns, and controls at
most 5.83% long or 2.50 ns short. Linked __TEXT/__DATA segment sizes are equal,
and neither executable has a __mod_init_func section. Every preregistered
adoption limit passes. Adopt this local result with its explicit control costs
and limitations; the failed inlined shape remains rejected. No public contract,
allocation strategy, portable performance or application-size promise changes.
