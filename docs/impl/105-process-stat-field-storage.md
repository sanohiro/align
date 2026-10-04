# Linux process-stat field storage

Status: bounded implementation selected after the existing process-table assessment.
Baseline: `98c25a4bfa2e73f107fdd64b54f21653acd524dc`.
Plans 49/50 retain the complete process observation contract.

## Consumer and boundary

`process.table` and an unreaped session child's `group_members` share Linux
`observe`/`parse_stat`. The existing file-redirection/table/group consumer in
`m11_process_live` exercises both source operations. The parser currently collects
every whitespace-separated stat field into a heap Vec but reads only fields
1, 2, 11, 12, 17 and 21 of the tail after the final closing parenthesis.
Use 22 optional borrowed text slots in fixed local storage instead. Fill only
that prefix, then retain the current indexed numeric conversion and native queries.

This changes no public signature, output representation, interface, ABI,
ownership, native resource, scan budget or error contract. Complete tail UTF-8
validation still precedes tokenization; ignored later text is not newly admitted.
The input byte Vec, PID/row arrays and published Align allocation remain.
macOS uses its unchanged native observation path. No throughput or RSS promise.

## Implementation closure

| Invariant | Site and owning verification |
| --- | --- |
| Parentheses, positive/exact PID and complete tail UTF-8 validation | Existing `parse_stat` prefix remains; native identity owner plus prefix/tail matrix. Invalid UTF-8 after field 21 must still fail. |
| Required parent/group fields and error/native-query order | Indexed conversion remains in parent, group, page, tick order; prefix/malformed essential fields owner. Missing or invalid essentials stay Invalid. |
| Optional counters, signed/range/arithmetic admission and missing fields | Existing conversion closures remain; prefix matrix and existing optional-counter owner cover RSS, CPU, threads and None. |
| Later fields, whitespace and command bytes | Prefix-only token references; ignored valid tails and command parentheses/newline owner. No interpretation of later tokens. |
| Storage bounds and input lifetime | Exactly 22 `Option<&str>` slots borrow call-local validated text; safe iteration only. Local allocator measurement on actual parser calls compares the old Vec with the candidate; no new native/FFI allocation strategy. |
| Native I/O, errors, PID scan/order, output allocation and cleanup | Export/observe/publish code unchanged; native table/layout/identity owners and existing source table/group consumer. |
| Construction, moves, joins, generics, interfaces, whole/per-unit compilation | Compiler and value ownership unchanged; existing m11_process_live whole/per-unit source checking and executable consumer closes the shared native surface. No new compiler fixture. |

The author matrix pass and one fresh full-diff preflight review suffice: this is
a private safe-Rust token-storage refinement of the existing reviewed contract,
with no new public or native safety strategy. Run native process_table owners
on Linux and macOS and the directly relevant source consumer. Measurements are
separate from correctness gates and must distinguish parser scratch from whole
process-table storage.

## Local allocation evidence

A Linux ARM64 release harness extracts the exact baseline/candidate parser bodies,
native record declarations and OptionalCount conversion from canonical sources,
then counts their Rust System allocator calls. Input construction and observation
I/O are outside the interval. Returned records/errors match in all four cases.

| Parser input | Baseline alloc/realloc calls | Baseline cumulative requested bytes | Candidate calls/bytes |
| --- | ---: | ---: | ---: |
| Required identity fields only | 1 / 0 | 64 | 0 / 0 |
| Ordinary 111-byte stat record | 1 / 4 | 1984 | 0 / 0 |
| 64051-byte valid record with ignored tail | 1 / 13 | 1048512 | 0 / 0 |
| Invalid parent field | 1 / 0 | 64 | 0 / 0 |

These are isolated parser allocations, not peak-live bytes, libc-internal memory,
whole-table storage or process RSS. The fixed local array is 22 borrowed text
slots; the existing native input and returned-row allocations remain. No speed
or whole-process memory improvement follows from these counts.
