# Map same-length form output with its checked count

Form encoding already counts bytes requiring three-byte percent escapes before
one exact allocation. When that checked count equals the input length, every
input byte is unreserved or space. Unlike percent and HTML unchanged-output
branches, this branch must still map spaces to plus. Evaluate a simple safe
byte-to-slot mapping at that authenticated call site, preserving the existing
private writer for expanded output and all of its mismatch checks.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Count authority and grammar | Only form_encoded_len supplies the no-expansion proof. Equal lengths admit unreserved bytes and spaces; each space still becomes plus. Plus, percent, slash, NUL and arbitrary non-ASCII bytes retain uppercase percent escapes. Existing all256 count/output oracles and mixed ordinary/space/escape controls close both branches. |
| Complete initialization | The exact owned_str_exact slice has the same length as input on the new branch. Safe zip iteration writes every destination slot once; no raw pointer, uninitialized read, extra buffer or allocation. Existing public exact-output owner checks independent storage and null/empty shape. Keep private destination mismatch refusal unchanged. |
| Allocation and publication | Existing byte-view admission and checked count precede the same final allocation. Only publish after callback completion; output Drop frees once and input survives. The isolated positive-counter-witness owner retains zero allocations for empty input and exactly one payload allocation/free otherwise. No new failure or cleanup path. |
| Compiler and control | No type, IR, ABI, cache, generic, borrowing or source contract changes. Existing m10_encoding source owners cover imports, returned owned output, round trips and percent/form distinctions. K1 and aggregate provenance remain deferred. |
| Measurement | Extend the existing C corpus with space-only and mixed unreserved/space inputs for both arms. Keep every existing encoder/control and size. Independently check bytes, native returned extents, call and output-byte counts. Rebuild both ordinary release producers, run ABBA on idle macOS ARM64 through native_probe.run_phase, retain every sample and short/expanded/control cost. Adopt only useful local gains; no portable/application/RSS claim or timing correctness gate. |
| Artifact lifecycle | Record source/archive/harness identities, emit retained CSV with explicit LF and check the staged/committed range. Keep compact measurements and review evidence; retire task archives/executables after their final consumer. |

The public contract and reviewed allocation/safety strategy are unchanged.
Perform an author-side matrix/extracted-obligation pass and one fresh full-diff
inspection review, followed by final-SHA owner, bounded-gate and Clippy checks.
This is a complete independent form-writing capability: the prior unchanged-byte
copy proof cannot implement its mandatory space transformation.

## Qualification

The new branch is confined to the existing checked-count form entry; its safe
mapping initializes the exact final allocation. The expanded writer and private
mismatch checks are unchanged. The parameterized public owner now combines
spaces and unreserved bytes at short/vector/reduction/tail boundaries, with
plus, percent, slash, NUL and arbitrary-byte expansion controls. Its isolated
allocation mode retains a positive counter witness, exact output allocation/
free counts and independent output ownership. Four native owners and all
eighteen m10_encoding source owners pass on macOS ARM64.

The [measurement record](../../bench/encoding_writes/README.md) retains all
8,640 observations across 240 cases. Same-length form cases at 4/64 KiB improve
78.1–89.5%; long expanded cases range from -1.70% to +3.84%, with a largest
absolute increase of 0.406 microseconds. Short form cost increases by at most
3.65 ns. Other encoders remain explicit controls. Adopt the local gains and
costs without changing the public contract or making a portable performance
claim. The CSV writer uses explicit LF, and staged/committed-range whitespace
checks include the new artifact.
