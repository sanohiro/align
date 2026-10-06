# Fixed-field rendering for named UTC formats

This is the single follow-up capability selected by plan 129. Replace generic
numeric formatting inside the private `time_formats::format` stack writer with
checked fixed-width decimal output. Preserve the already-shipped calendar,
rounding, grammar, parser, native admission, errors and final owned allocation.
The source of truth remains `std-design/time.md`; no public or mirrored contract
changes, new IR/type/effect/ABI/interface shape or allocation strategy are needed.

## Closure matrix

| Obligation | Implementation and owner |
| --- | --- |
| Decimal fields | A checked stack writer emits exactly the requested field width and rejects negative, overflowing-width or out-of-storage inputs. Date fields reachable from i64 nanoseconds have four-digit positive years; time/fraction fields fit their settled 2/3/9-digit widths. An independent generic-format oracle and explicit width/storage refusal owners cover the writer and all five outputs. |
| Wire identity | Keep exact punctuation, English month/weekday names, three-digit milliseconds, zero-fraction omission and trailing-zero removal. Existing native wire goldens anchor expected bytes; new independent rendering parity covers reachable dates, seeded instants and every fraction width. Parsing remains an independent inverse, not the only byte oracle. |
| Calendar/resolution/errors | Leave civil_date, civil_days, resolution and i128 floor/representability arithmetic unchanged. Reuse every-representable-day calendar owner and native precision/endpoint/kind rejection owners. No panic or partial owned string on a rendering refusal. |
| Storage/publication | Keep a 32-byte initialized stack Text and unchanged owned_str_exact publication. Safe bounded slice writes only; no new unsafe code, pointers, allocation or cleanup. A pure private-format allocation owner can observe no Rust heap requests without calling the C probe. Existing native and source owners cover zero-on-error, independent strings, replacement and Drop. |
| Native/control/interface | Exact existing TimeFormat ABI and discriminator, parsing and output/input metadata rules remain unchanged. Reuse runtime ffi_preflight_and_zero_on_failure and the whole/per-unit/imported/generic/control/purity driver time_formats owners; pkg_s3 covers the real request/presign consumer. No new driver fixture helper. |
| Performance acceptance | One identical C oracle fixture exercises actual baseline/candidate non-test release entrypoints, including final allocation/free. Fixed independent golden vectors cover all five kinds, negative/zero/modern/endpoints and each fractional length; timed calls observe status, output length and byte/call counts. Warm each case, retain all per-case ABBA samples on sequential macOS/Linux runs, pin Linux guest vCPU, and record actual producer recompilation and binary hashes. No concurrent builds. Adopt only a useful measured improvement without material regression; no timing CI gate or application/network throughput promise. |

The implementation follows the existing safe stack writer and owned-string
publication strategy. Perform the author matrix/extraction pass and one fresh
full-diff review; a separate strategy review is unnecessary. One capability
contains all five format branches and their shared writer/owners; the expected
handwritten implementation/test/measurement diff is below 1,000 lines. If timing
does not qualify it, record that result and close the selected batch without
inventing a replacement optimization.

## Consumer verification repair

The initial macOS `pkg_s3` owner run exposed the existing accept helper's
inherited nonblocking sockets: `explicit_body_wire`, `presign_round_trip` and
`operation_round_trip` all failed at `read_request` with WouldBlock. The same
initial Linux target passed all 11 owners. Set the accepted stream to blocking
before its existing five-second read/write timeouts; retain the existing bounded
accept and child lifecycle. All three failures use this one helper, so this
closes the observed class without changing product transport behavior or adding
a fixture family. Re-run the complete consumer target on both platforms.

## Implemented closure

`Text::push` and `Text::digits` use checked ranges and safe initialized byte
writes. Numeric conversion is checked before writing; excess decimal width
leaves the committed Text length unchanged. Formatting failure never reaches
owned-string publication. All reachable years are 1677 through 2262, established
by the existing exhaustive calendar owner; each width is therefore exact for
this input domain. No parser, calendar, native entrypoint or allocator changed.

The four new `time_formats::rendering_tests` owners compare generic-format bytes
on every reachable date, all fraction widths, endpoints and 4096 seeded instants;
exhaust the small decimal widths and reject width/storage overflow; and observe
zero private Rust heap requests. The existing independent calendar/wire/native
owners remain active. Both macOS and Linux pass 10 native owners (the existing
manual timing probe stays ignored), five driver time owners and all 11 S3 owners.
`bench/time_fields/README.md` records the independent golden C fixture, actual
producer identities and accepted all-sample latency results. All five format
corpus medians improve on both hosts; no timing threshold enters CI.
