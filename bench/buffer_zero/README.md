# Fresh zero-filled buffers

Run `bash bench/buffer_zero/run.sh`. The shared supervisor rebuilds the production
release runtime, links this C consumer and bounds build/link/probe process groups.
Each row runs in a fresh fork, checks initialized bytes and alignment, then records
acquisition, a complete first read, first write, same-region refill and Drop.
Wall/user/system time, minor faults and process peak RSS are reported separately.
The source includes zero/small and nonzero-fill, aligned and fallible controls.
This is a local measurement, not a correctness gate or a latency guarantee.

`linux-before.csv` uses the feca30d0 buffer implementation (with the independent
JSON-status repair already present). `linux-after.csv` uses plan160 zeroed
acquisition. Both use the ordinary, uninstrumented release runtime and the same
probe. Samples were collected sequentially on 2026-10-09, Linux x86-64 WSL2
6.18.40.1, Ryzen 9 5950X, 4096-byte pages. No baseline archive digest was retained.
The measured after archive SHA-256 is
`4b3b22e194c8c2c679a9e7d0ff76854e39ded739ed89506db6afca21f1a445da`;
the probe source SHA-256 is
`dbf2ccea47140d16f65c7f10427db114c927f34edc951f1d474706b3fba8c7cb`.

Three-trial medians for the client's 2,204,631,040-byte size:

| Phase | Before ms | After ms | Before / after minor faults |
| --- | ---: | ---: | ---: |
| Acquire | 1027.26 | 0.03 | 538248 / 7 |
| First complete read | 245.19 | 484.50 | 0 / 538240 |
| First write after that read | 135.12 | 1914.57 | 0 / 538240 |
| Same-region refill | 135.80 | 126.46 | 0 / 0 |

The allocator can defer physical work. This corpus's read-then-write total is
slower after the change; constructor time alone is not an end-to-end improvement.
All rows, including small/nonzero/alignment controls and RSS, remain in the CSVs.
Real launch-to-first-answer, cache behavior and other-host measurements remain
consumer-owned. No page-size, residency or universal allocator promise follows.

The emitted x86-64 ordinary constructor branches on the fill byte: zero calls
`__rust_alloc_zeroed` and publishes length without `memset`; nonzero calls
`__rust_alloc` followed by `memset`. Both retain the single payload owner and
existing header allocation. Native owner tests separately prove allocation
layouts, initialization, failure retirement and Drop.
