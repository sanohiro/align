# Function-level incremental compilation benchmark

This local, non-CI item-6 harness measures the fixed `apps/db` corpus through
the unchanged unit-ThinLTO API and the function-partition API in the same
optimized compiler:

```text
bench/function_incremental/run.sh
```

The edit case swaps two pure guards in the private
`pkg.db.internal.resource.recovery_deadline` leaf without changing behavior.
Each of seven counterbalanced samples uses
fresh independent caches, primes the original corpus outside the timed region,
then times only codegen plus fresh thin-link for the edited MIR. It requires
exactly one prelink miss in both granularities and a function/unit median ratio
at most 0.75. `ALIGN_FUNCTION_SAMPLES` may select a larger odd count of at
least three.

Separate fresh processes measure cold end-to-end wall time and peak RSS; each
function/unit ratio must be at most 1.25. The parity control links and runs both
granularities, requires the final size ratio to remain within 5%, and proves
cache-off, cold-cache, and hot-cache function executables are byte-identical.
The existing ThinLTO runtime corpus remains the broad semantic/runtime owner;
this harness does not duplicate it.
