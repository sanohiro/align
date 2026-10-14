# JSON object key traversal

Voice V001 enumerates a tokenizer vocabulary of roughly 151,000 members.
Repeated `doc.key(i)` walks each prefix again. `doc.keys()` materializes one
ordered slice in an explicit arena and then indexes it in constant time.

Run from the repository root after building matching compiler/runtime artifacts:

```text
python3 bench/json_doc_keys/measure.py target/debug/alignc samples.json --source-id COMMIT
```

The runner records compiler/runtime binary hashes, source/input hashes, host,
every sample and medians. It generates decimal keys and equal integer values,
with 4,000/8,000/16,000 members for both paths and 151,000 for keys(). Five fresh
processes run per case. Generated code uses release O2, baseline CPU, one compile
worker, disabled runtime LTO and disabled codegen cache. The compiler/runtime
artifacts use the repository's local dev build; their hashes fix that identity.

Parsing is outside the timed region. The original consumer key(i) loop is
unchanged; the keys() loop includes materialization and the same key-byte sum.
Both include the byte-count print before observing elapsed time, so fixed
process/output noise dominates small keys() cases. The new program then checks
every key byte against an independently formatted index and every corresponding
elems() value against that index, outside timing. The driver/native owners cover
escaped keys, duplicates and lifetimes; this corpus is a scaling measurement,
not Unicode or ownership coverage. Real tokenizer parity remains consumer work.

Temporary sources, input, objects, binaries and file-backed process output live
under one exclusively acquired directory. Each subprocess has a 55-second work
deadline and owned process-group cleanup. The 151,000-member quadratic control
is deliberately omitted; the three smaller sizes already establish its scaling.
No fixed speedup, cross-host timing or application startup claim follows.

## Local observation (2026-10-14)

[All samples and identities](macos-samples.json) use the committed implementation
`96db7e9effe8621eb08e101c25821bae7d61c6fc`. Each reported time is the median of five
fresh processes on this Mac. Fixed output overhead obscures fine scaling in the
small keys() cases; no minimum speedup is asserted.

| Members | Repeated key(i) | One keys() materialization |
| --- | ---: | ---: |
| 4,000 | 30.270 ms | 0.258 ms |
| 8,000 | 120.572 ms | 0.163 ms |
| 16,000 | 483.975 ms | 0.209 ms |
| 151,000 | not measured | 1.201 ms |
