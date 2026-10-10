# Count HTML entity expansion before checked sizing

The private HTML output-length counter currently performs a checked addition
for every byte. Accumulate entity expansion in a narrow counter for each
32-byte chunk, then checked-add that expansion to the input length. The largest
chunk expansion is 32 * 5 = 160, which fits in u8. This follows plan123's
unchanged exact destination/publication strategy and plan156's count-before-size
refinement. The five entities and the writer remain authoritative and unchanged.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Exact length and bytes | Less-than/greater-than expand by three, ampersand/apostrophe by four and double quote by five; all other bytes retain their length. The independent encoding_writes_tests oracle owns exact count/output parity for empty, ASCII, Unicode, each entity at vector/tail boundaries and mixed inputs. Existing writer sentinels and short/long destinations retain the initialized-output proof. |
| Overflow and allocation | Each chunk has at most 32 bytes and each byte contributes at most five, so its narrow sum cannot overflow. Every addition to the complete extent is checked before the unchanged owned_str_exact allocation or template builder reserve. No intermediate output, extra copy, new unsafe block or allocation. Admission and pre-side-effect validation stay in the existing native entry owners. |
| Ownership and control | The shared private counter returns Option<usize>; overflow retains the existing terminal path before allocation/reserve. Existing html_text/template native and source owners cover returned/moved/replaced String payloads, builder transfer/Drop and error cleanup. There is no source, IR, ABI, interface/cache, provenance or representation change; generic/control-flow/return ownership is unchanged. |
| Consumers | encoding.html_escape and pkg.template HTML write share the counter and unchanged writer. Run encoding_writes_tests, native template_html owners and m10_encoding, html_text, pkg_template and template_ownership source owners. Percent/component, percent/path and form stay benchmark controls. |
| Measurement | Extend encoding_writes with a dense five-entity seed, keeping its independent oracle and exact producer call/length/byte counts. Compare baseline and candidate ordinary release archives with identical C source/flags in ABBA order on this Linux x86_64 host. No build/test overlaps timing. Keep every observation and report short/control regressions; adopt only with useful long-input improvement and acceptable short costs. No ARM/macOS, application-throughput, RSS or portable speed claim, and no timing gate. |
| Probe lifecycle | Reuse the shared native_probe run_phase owner for build/link/probe deadlines and process-group retirement. The existing shell entry uses the same runner; this capability changes only corpus and counter, not signals, budgets or child ownership. Remove temporary archives/executables after retaining compact reproducible evidence. |

The author matrix-to-diff pass and one fresh independent implementation review
close this representation-preserving refinement. No new safety strategy or public
contract requires a separate design review or normative language mirror change.
K1 and the general aggregate-provenance experiment remain deferred.

The initial three-wide-counter experiment was rejected after ABBA measurement:
64-KiB plain/unreserved/mixed HTML calls regressed 34.6%, 46.3% and 23.5% on
this host, despite a 4.4% improvement on dense entities. Retain its complete
observations with the measurement record. The bounded narrow reduction avoids
three usize reduction chains; it must independently satisfy the same acceptance.

## Qualification

The bounded narrow reduction improves every approximately 4-KiB and 64-KiB
HTML case on the measured host (22.5–34.6%). The largest short HTML increase
is 0.95 ns; empty input adds 0.75 ns. Unchanged controls include up to 6.0%
long and 9.2 ns short increases, retained and accepted with the reported scope.
[The measurement record](../../bench/encoding_writes/README.md) preserves both
ABBA experiments, exact corpus/producer-count checks and all 6,912 observations.
These measurements do not establish performance on other architectures.
