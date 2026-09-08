# std.time wire formats and path encoding

This ledger owns the cloud prerequisite selected on 2026-09-08 after the
post-XML and consumer-boundary investigations. One implementation PR delivers
the complete named-format family and slash-preserving encoding. `pkg.s3`
and SigV4 are the next consumer; this capability performs no network work.

## Public contract

Existing `now`, `instant`, and `sleep` retain their contracts. The following
functions use signed `i64` Unix epoch nanoseconds, UTC, and the proleptic
Gregorian calendar. No locale, timezone database, ambient configuration,
strftime language, new time type, or leap-second timeline is introduced.

| Formatter signature | Canonical output | Resolution in nanoseconds |
|---|---|---:|
| `time.rfc3339(ns: i64) -> Result<string, Error>` | `YYYY-MM-DDTHH:MM:SS[.fraction]Z`; omit zero fraction, otherwise 1–9 digits without trailing zeroes | 1 |
| `time.rfc3339_ms(ns: i64) -> Result<string, Error>` | `YYYY-MM-DDTHH:MM:SS.sssZ`; exactly three fractional digits | 1000000 |
| `time.rfc1123(ns: i64) -> Result<string, Error>` | `Wdy, DD Mon YYYY HH:MM:SS GMT`; fixed English names | 1000000000 |
| `time.basic_iso(ns: i64) -> Result<string, Error>` | `YYYYMMDDTHHMMSSZ` | 1000000000 |
| `time.basic_date(ns: i64) -> Result<string, Error>` | `YYYYMMDD` (midnight UTC) | 86400000000000 |

Every formatter first floors the instant to its resolution using mathematical
floor, including negative instants. If that result is outside `i64`, return
`Error.Invalid` before allocating. Otherwise return one owned Move `string`.
The exact-resolution formatter accepts every `i64`. This single representability
rule ensures that every successful output parses back to the floored instant;
it does not silently clamp or wrap at the lower endpoint. Allocation failure
follows the existing owned-string hard-OOM policy, not a new recoverable error.

Each formatter has a matching `time.parse_NAME(input: str) -> Result<i64, Error>`
where NAME is exactly `rfc3339`, `rfc3339_ms`, `rfc1123`, `basic_iso`, or
`basic_date`. There are no defaults or optional arguments. A `string` argument
uses the usual call-only borrowed `str` conversion. Parsers retain no input,
allocate nothing, and return a Copy integer. The encoder operations allocate
owned output; all new operations are deterministic pure transforms under the
existing encoding purity convention.

Parsers consume the entire input without trimming. Reject embedded NUL,
non-ASCII bytes, invalid Gregorian dates, hour > 23, minute or second > 59,
extra bytes and any normalized nanosecond result outside `i64` as
`Error.Invalid`. Leap seconds are rejected, not folded into the following day.
Years have exactly four digits, and every numeric field has the width shown.
`rfc1123` accepts exactly the displayed IMF-fixdate form, case-sensitive English
names and GMT, with a weekday matching the date; no obsolete HTTP-date grammar.
The basic parsers accept exactly their formatter grammar.

The RFC3339 parsers additionally accept lowercase `t`/`z` and explicit
`+HH:MM`/`-HH:MM` offsets with hours 00–23 and minutes 00–59. Reject `-00:00`
(unknown local offset). Subtract the offset before checking representability.
`parse_rfc3339` accepts an absent fraction or 1–9 digits; `parse_rfc3339_ms`
requires exactly three. All other punctuation is exact. Every malformed-input
class maps to the same `Error.Invalid`; no error-message precedence is exposed.

`encoding.percent_encode_path(data: bytes) -> string` accepts the same `bytes`,
`str`, and borrowed `string` input forms as `percent_encode`. It preserves
ASCII `A-Z a-z 0-9 - . _ ~ /` and encodes every other byte as uppercase `%XX`.
It neither decodes existing escapes nor normalizes slash runs or dot segments.
It borrows the input only during the call and returns one independently owned
string using the existing encoding allocation policy. It is encode-only;
`percent_decode` remains the common inverse byte decoder. URI/query structure,
object-key boundaries, and SigV4 canonicalization remain consumer-owned.

## Compiler and runtime boundary

Sema owns exact arity, operand type, kind and result type. HIR uses explicit
`TimeFormat { kind, ns }` and `TimeParse { kind, input }` nodes with a closed
five-case `TimeFormatKind`. MIR preserves that discriminator and lowers through
the existing status-to-Result and owned-string cleanup machinery. LLVM only
lowers certified MIR. `EncodingKind::PercentPath` is encode-only and checked
HIR must reject it in a decoder, like the existing HTML case.

The following are compiler-private RuntimeKey entries, selected explicitly:

| Key | Exact native ABI | Result |
|---|---|---|
| `TimeFormat` | `i32 align_rt_time_format(ptr out, i64 ns, i32 kind)` | out is `AlignStr { ptr, i64 len }` |
| `TimeParse` | `i32 align_rt_time_parse(ptr out, ptr input, i64 len, i32 kind)` | out is `i64` |
| `PercentEncodePath` | `{ptr, i64} align_rt_percent_encode_path(ptr input, i64 len)` | existing owned-string ABI |

Time kinds are 0 RFC3339, 1 RFC3339-ms, 2 RFC1123, 3 basic-ISO, 4 basic-date.
Time calls return zero on success or `EINVAL` on failure, using the existing
errno-to-Error mapping. Reject every other kind. Callers provide valid writable
properly aligned output storage. Invalid null/alignment/length/address-span
metadata returns `EINVAL` without dereferencing an invalid span. Once output
metadata is valid, initialize output to canonical zero even on rejection;
check input/output overlap using raw addresses before forming input references.
An overlapping input is rejected after clearing the valid output, without
reading input. A null input is allowed only for length zero; it then fails the
nonempty timestamp grammar. Valid nonempty input must name initialized readable
bytes for the call. There is no retained pointer, callback, process state,
runtime shell, or new ownership mode. Parse checks its bounded grammar length
before forming a byte slice; maximum accepted input is 35 bytes.

The runtime builds formatting bytes in fixed stack storage before publishing
one owned-string allocation. Civil conversion and normalization use widened
integer arithmetic so the `i64` endpoints never overflow intermediate seconds
or days. No new native dependency is needed.

HIR serialization and monomorphization preserve kinds and children. Generic interfaces persist
source templates, not HIR enum ordinals; the existing interface schema remains unchanged.
The cache rehydration owner verifies the replayed MIR including both time kinds and path encoding. The runtime ABI inventory/fingerprint incorporates all three keys;
whole-program and per-unit artifact identities consume the usual compiler,
interface and runtime identities. No alternative format or cache path exists.

## Implementation closure matrix

This is one useful consumer prerequisite with shared calendar, Result, ABI,
serialization and ownership proof. It may exceed 1,000 handwritten changed
lines: splitting individual formats or dormant compiler/runtime producers
would repeat those proofs and expose an incomplete family without reducing
integration risk. There is no explicit throughput claim, so no benchmark gate.

| Axis | Implementation obligation | Owner target |
|---|---|---|
| Formation and invalid input | Exact five-by-two API/kind matrix; narrow integer inference, call-only text conversion, malformed HIR operand/result/kind refusal | D2, H1 |
| Calendar and precision | All formats, epoch ±1, leap/century rules, independent known dates, both i64 endpoints and first valid floored point, offset crossings | R1, R2, R3 |
| Parser grammar | Parameterized widths, every delimiter, fraction/offset cardinality, invalid date/time/weekday/NUL/non-ASCII/trailing bytes; valid controls | R1, R2, R3, D6 |
| Native ABI | Null, alignment, negative/excessive lengths, overflow/overlap, invalid kind, zero-on-error and exact signatures | R4, A1, A2 |
| MIR producer/access certification | Require readable exact i64 formatter input or str parser input, exact i32 status and String/i64 output slot, and the closed kind. Validate the primary equation and every dependency before atomically publishing fresh owned formatter output; never seed an invalid or absent producer. Reject forged result, operand access/type, kind and output slot. | P1, D1, D3 |
| Construction/return/move/drop/replacement | Fresh owned result, bind, unwrap, returned result, nested existing containers, move-in/out and source nulling; parse result retains no backing storage | D3, D4 |
| Control paths | `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, early return and terminating operand; exactly-once input evaluation and no post-termination operation | D3; existing `lower_required_binding!` termination guard |
| Call paths and effects | Direct/helper/imported, generic replay, noncapturing indirect wrapper, closure; pure transforms accepted where existing encoders are pure | D3 |
| Encoding sibling | All 256 bytes compared to independent unreserved/slash oracle; empty, text, owned input, literal percent, duplicate slash and dot segments; forbid decoder kind | R5, D5, H1 |
| Persistence and compilation | Whole/unit equivalence; generic interface round-trip; cold/warm/change/restore compilation and artifact invalidation | D1, D3, C1 |
| Explicit boundary | No direct builtin function-value syntax widening, returned/capturing closure widening, arbitrary calendar/TZ/locale formats, obsolete HTTP-date, leap seconds, S3 networking or consumer-repository adoption | Deferred by this ledger |

The owner inventory is exact (Rust package/test-target names are shown):

| Owner | Exact test or command |
|---|---|
| R1 | `align_runtime --lib time_formats::tests::wire_golden_vectors_and_precision_endpoints` |
| R2 | `align_runtime --lib time_formats::tests::parser_grammar_and_calendar_matrix` |
| R3 | `align_runtime --lib time_formats::tests::grammar_widths_and_fraction_lengths_are_closed` |
| R4 | `align_runtime --lib time_formats::tests::ffi_preflight_and_zero_on_failure` |
| R5 | `align_runtime --lib time_formats::tests::percent_path_all_bytes_match_independent_oracle` |
| D1 | `align_driver --test time_formats named_formats_round_trip_negative_epoch_whole_and_unit` |
| D2 | `align_driver --test time_formats named_formats_reject_wrong_types_and_arities` |
| D3 | `align_driver --test time_formats format_family_imports_generic_replay_control_flow_and_purity` |
| D4 | `align_driver --test time_formats moved_formatter_results_are_rejected_whole_and_unit` |
| D5 | `align_driver --test m10_encoding percent_path_preserves_structure_and_owns_output` |
| D6 | `align_driver --test time_formats malformed_timestamps_map_to_the_shared_invalid_error` |
| H1 | `align_mir --lib hir_body_validator_native` (family operands/results and encode-only decoder mutation) |
| P1 | `align_codegen_llvm --lib xml_out_slot_seed_is_atomic_with_its_exact_producer` (all five time kinds, both outputs, status/type/access/slot mutations; whole/unit refusal) |
| C1 | `align_driver --test unit_cache named_time_generic_templates_survive_cache_change_and_restore` |
| A1 | `align_codegen_llvm --lib runtime_abi`; `align_runtime --lib runtime_export_source_inventory_matches_registry` |
| A2 | `scripts/test-runtime-abi-exports.sh` (all native types and base/probe symbol sets) |

The existing HIR/storage variant tripwires now count 327 variants. Native time tags have
invalid-domain owners in R4; Rust's closed HIR/MIR enum cannot safely represent an unknown tag.
The source baseline fails D1 because the family is absent. During author verification D1 also
caught the missing native-envelope admission, while P1 caught scalar parser outputs bypassing
the protected-leaf walk; both are closed before requesting code review. All matrix cells above
point to implemented owners; only the explicit boundary row is deferred. No broader compiler
restriction was widened to make these tests pass.

## Sources and consistency

The deliberately bounded RFC3339 grammar derives from
[RFC 3339 §5.6](https://www.rfc-editor.org/rfc/rfc3339.html#section-5.6).
The HTTP wire form is
[RFC 9110 §5.6.7](https://www.rfc-editor.org/rfc/rfc9110.html#section-5.6.7).
Basic date/time and path preservation serve the
[AWS S3 SigV4 canonical request](https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-header-based-auth.html).

Required synchronized records: `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md`, Settled in `docs/open-questions.md`,
`docs/impl/07-roadmap.md`, `19-hir-validation-ledger.md`,
`20-runtime-abi-ledger.md`, this file and `ja/time.md`. HANDOFF records the
capability boundary once. No source example may claim unrestricted RFC or HTTP
date parsing; the exact subset above is authoritative.
