# Runtime native ABI ledger

## Status and authority

This is the exact native symbol/type/attribute appendix for L2b-a2-am-r and
L2b-a2-am-c1. It records the LLVM 22 declaration surface emitted by the current
backend for every validated runtime target and the additional externally
visible runtime definitions that occupy link identities. The keyed surface is
generated from a trivial valid program; the complete base and `alloc-count`
surfaces are independently compared with the Rust runtime exports.

With bounded canonical JSON, process capture, bounded HTTP response bodies,
owned JSON, and exclusive filesystem publication, there are 296 `RuntimeKey`
variants and a one-to-one native-symbol record. Relative to Am-c1, F-B added
`ArrayBuilderNewIn` and `ArrayBuilderPushBytes`; the four
AEAD symbols that were previously selected from `AeadCipher × AeadDir` become
ordinary typed keys; they may no longer bypass the registry. Thirteen always-built
runtime records have no `RuntimeKey` and instead use the thirteen-variant
`UnkeyedRuntimeKey`: the two main-wrapper callees
`align_rt_report_error` and `align_rt_args_build`, plus the runtime-internal
`align_rt_arena_reset`, `align_rt_realloc`, and
`align_rt_http_serialize`, and eight package-internal PostgreSQL codec helpers:
`align_rt_f32_to_bits`, `align_rt_f32_from_bits`, `align_rt_f64_to_bits`,
`align_rt_f64_from_bits`, `align_rt_f32_text_len`, `align_rt_f64_text_len`,
`align_rt_f32_text_write`, and `align_rt_f64_text_write`. The base native registry
therefore has 309 records. Request 12 adds the keyed bounded-builder stack
initializer and consuming status/out-slot finish; both reuse existing ABI shapes
A51 and A19.
The explicit `alloc-count` runtime feature may expose four
test/benchmark-only counter definitions. `par-map-probe` may expose four more:
`void @align_rt_test_par_map_force_caller(i32)`,
`i64 @align_rt_test_par_map_min_chunk()`,
`i64 @align_rt_test_par_map_min_chunk_for(i64, i64, i64)`, and
`i64 @align_rt_test_par_map_workers()`. `task-group-probe` changes internal
Rust state only and adds no unmangled native export.

The compiler-visible native registry is always exactly the 309 base records.
There is no target option, environment variable, Cargo feature, linked-runtime
inspection, or other ambient input that changes it. The eight optional probe
records extend only the verification-time maximum runtime-export table to 317.
They never gain a `RuntimeKey`, callable/declaration policy, collision
reservation, or compatible-extern reuse. Their spellings remain ordinary
program/extern/export identities in a normal build. Probe-feature runtime
builds are test/benchmark fixtures and never link user artifacts. Thus runtime
feature selection changes neither accepted callable input nor MIR, interface,
artifact, or cache identity. Registry membership is never inferred from symbol
spelling.

Request 11 added six regular `RuntimeKey` rows for bounded process capture. Request 5 subsequently
added the two bounded-HTTP setters, and Request 9 then added `BuilderWriteUint`.
Their runtime definitions and registry entries activated atomically: the current
exact counts are 296 keyed records, 309 base records, and 317 records in the
maximum optional-probe export table. No unkeyed or probe category changed.

## Request 9 owned JSON extension

The implemented Request 9 design in `docs/impl/24-owned-json-plan.md` adds exactly
one keyed base record while reusing an existing LLVM function shape:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `BuilderWriteUint` | `align_rt_builder_write_uint` | A66: `void @SYM(ptr, i64)`; Rust receives `(*mut Builder, u64)`; no curated attributes |

The implementation changed the exact counts from 293 to 294 `RuntimeKey`
variants, 306 to 307 base records, and 314 to 315 maximum optional-probe exports.
The key, registry row, LLVM declaration/selection, unmangled Rust export,
key↔symbol bijection, and base/export parity must land atomically. The wrapper
checks the existing sticky limit and calls the existing internal
`builder_push_u64`; signed widths keep `BuilderWriteInt`, while every unsigned
width is zero-extended and selects `BuilderWriteUint`. The ABI still carries the
same 64 bits as LLVM `i64`; only Rust's decimal interpretation is unsigned.

Request 9 also extends the compiler/runtime interpretation of the existing
`JsonField.tag` kind byte passed through A103: kind `8`, width `16`, null
`sub` is an owned `string`; kind `9`, width `16`, null `sub` is an owned
`array<string>`. A direct `Option<string>` uses kind `8` with its validated
nonnegative record-relative `opt_tag`; required forms use `-1`. Kinds `0..=7`
retain their shipped meanings.

Those rows are emitted only from a target-local `OwnedJsonDescV1` whose
`OwnedJsonInterfaceEnvelopeV1` target/ABI identity has already validated before
descriptor offsets are read; whole-program/private/monomorph producers construct
the same envelope locally.
Owned decode calls A103 with the existing final arena argument null; the new
kind itself selects free-standing allocation. The `JsonField` C layout and A103
signature do not change. Owned encode does not pass these kinds through A80:
direct and optional strings use A73's existing JSON-string writer, and
`array<string>` uses A74 with the shipped borrowed-element tag
`(3 << 8) | 16` because encode only reads each `{ptr,len}`. A80 retains its
current descriptor domain. `BuilderWriteUint` and kinds `8`/`9` are shipped;
the exact counts and A66 row above are current.

## Request 14 exclusive filesystem publication (implementation candidate)

Request 14 adds exactly two keyed records and no new ABI shape. The implementation
candidate activates these rows atomically with the HIR/MIR/runtime support, taking
the registry to 296 keyed / 309 base / 317 maximum optional-probe records:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `IoWriterCreateExclusive` | `align_rt_io_writer_create_exclusive` | A08: `i32 @SYM(ptr, i64, ptr)` |
| `FsRenameNoReplace` | `align_rt_fs_rename_no_replace` | A09: `i32 @SYM(ptr, i64, ptr, i64)` |

The constructor row keeps the existing `writer` output-slot convention: the
runtime checks a null `out_writer` first, clears it before later validation, and
never publishes a writer on a recoverable failure. Both path operands are
borrowed views; the native runtime constructs ephemeral NUL-terminated copies,
constructing rename's source before its destination. The native implementation
must use Linux `renameat2(..., RENAME_NOREPLACE)` or macOS
`renameatx_np(..., RENAME_EXCL)` and must not fall back to ordinary replacing
rename, existence checks, `link` plus remove, or a subprocess. The full path,
allocation, platform, pair-cleanup, and owner matrix is in
`docs/impl/27-fs-exclusive-publication-plan.md` and
`docs/impl/std-design/fs.md`.

## Request 13 recursive owned JSON replacement (design accepted)

Request 13 added no runtime key, symbol, LLVM function shape, C signature, or
optional probe. At that capability boundary its implementation preserved the
then-current 294/307/315 counts while atomically replacing V1 descriptor
production with `OwnedJsonGraphDescV2` and retaining A103 decode and A80 encode:

| Existing ABI | V2 use |
|---|---|
| A103 `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, ptr, i64, i64, ptr)` / `align_rt_json_decode` | root `JsonField` table, null arena, recursive kind-4/5/7 edges, and owned leaf kinds 8/9 |
| A80 `void @SYM(ptr, ptr, ptr, i64)` / `align_rt_json_encode_object` | one root builder/base/table call; kind 8 reads owned string as the existing `{ptr,len}` string layout and kind 9 reads `array<string>`; nested tables use the same validated graph |

Kinds 8/9 remain illegal for arbitrary borrowed/AoS/union tables. They enter A80
only from a validated V2 owned graph. Options retain `opt_tag`; nested records
and record arrays retain `JsonField.sub`; no C layout changes. The implementation
must prove A103/A80 table identity and registry counts unchanged, and delete the
flat V1 encode-part path rather than keep two runtime producers. Exact graph
bytes, allocation/drop tags, and owner matrix are in
`docs/impl/25-recursive-owned-json-plan.md`. Until that implementation lands, the
preceding Request 9 rows are the shipped ABI domain.

The key-to-symbol mapping is `key -> "align_rt_" + snake_case(key)` except:

```text
Print       -> align_rt_print_i64
CliCommand  -> align_rt_cli_command_new
HttpRequest -> align_rt_http_request_new
```

The four added keys map regularly:

```text
CryptoAesGcmOpen              -> align_rt_crypto_aes_gcm_open
CryptoAesGcmSeal              -> align_rt_crypto_aes_gcm_seal
CryptoChacha20Poly1305Open    -> align_rt_crypto_chacha20_poly1305_open
CryptoChacha20Poly1305Seal    -> align_rt_crypto_chacha20_poly1305_seal
```

Every symbol occurs exactly once below. `@SYM` is replaced with that row's
symbol. Braces after a function type are exact function attributes; return and
parameter attributes remain inline. An absent brace means no curated
attribute. The four rt-LTO guarded symbols `align_rt_str_eq`,
`align_rt_str_starts_with`, `align_rt_str_ends_with`, and
`align_rt_str_eq_ignore_case` use the shown declaration attributes when
rt-LTO is off. When rt-LTO is on, their curated declaration attributes are
withheld before their visible bodies are linked; LLVM then derives attributes
from those bodies. `align_rt_str_cmp` is not guarded and always keeps A01.

| ABI | Exact LLVM declaration | Symbols |
|---|---|---|
| A00 | `i32 @SYM(ptr readonly captures(none), i64) {nofree nosync willreturn}` | `align_rt_utf8_valid` |
| A01 | `i32 @SYM(ptr readonly captures(none), i64, ptr readonly captures(none), i64) {nofree nosync willreturn memory(argmem: read)}` | `align_rt_str_eq`, `align_rt_str_starts_with`, `align_rt_str_ends_with`, `align_rt_str_cmp`, `align_rt_str_eq_ignore_case` |
| A02 | `i32 @SYM(ptr readonly captures(none), i64, ptr readonly captures(none), i64) {nofree nosync willreturn}` | `align_rt_str_contains` |
| A03 | `i32 @SYM(ptr)` | `align_rt_io_writer_flush`, `align_rt_http_stream_finish` |
| A04 | `i32 @SYM(ptr, i64)` | `align_rt_json_doc_kind`, `align_rt_fs_exists`, `align_rt_fs_remove`, `align_rt_child_kill` |
| A05 | `i32 @SYM(ptr, i64, i32, ptr)` | `align_rt_json_decode_array`, `align_rt_json_decode_scalar` |
| A06 | `i32 @SYM(ptr, i64, i64, i64, ptr)` | `align_rt_tcp_connect` |
| A07 | `i32 @SYM(ptr, i64, i64, ptr)` | `align_rt_json_doc_key`, `align_rt_tcp_listen`, `align_rt_udp_bind`, `align_rt_compress_gzip_compress`, `align_rt_compress_zstd_compress`, `align_rt_http_serve`, `align_rt_http_serve_shared` |
| A08 | `i32 @SYM(ptr, i64, ptr)` | `align_rt_json_doc_as_str`, `align_rt_json_doc_as_i64`, `align_rt_json_doc_as_f64`, `align_rt_json_doc_as_bool`, `align_rt_fs_read_file`, `align_rt_fs_write_file_builder`, `align_rt_fs_read_dir`, `align_rt_dns_resolve`, `align_rt_io_reader_open`, `align_rt_bytes_as_str`, `align_rt_io_writer_create`, `align_rt_io_file_create`, `align_rt_io_file_open`, `align_rt_base64_decode`, `align_rt_base64url_decode`, `align_rt_hex_decode`, `align_rt_percent_decode`, `align_rt_form_decode`, `align_rt_compress_gzip_decompress`, `align_rt_compress_zstd_decompress`, `align_rt_http_parse`, `align_rt_regex_compile`, `align_rt_regex_captures_group`, `align_rt_env_get` |
| A09 | `i32 @SYM(ptr, i64, ptr, i64)` | `align_rt_fs_write_file`, `align_rt_process_exec`, `align_rt_crypto_ct_equal`, `align_rt_env_set` |
| A10 | `i32 @SYM(ptr, i64, ptr, i64, i64, i64, i64, i64, ptr)` | `align_rt_crypto_argon2id` |
| A12 | `i32 @SYM(ptr, i64, ptr, i64, ptr)` | `align_rt_process_spawn` |
| A13 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, i64, ptr)` | `align_rt_crypto_hkdf_sha256` |
| A15 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, ptr, i64, ptr)` | `align_rt_crypto_aes_gcm_seal`, `align_rt_crypto_aes_gcm_open`, `align_rt_crypto_chacha20_poly1305_seal`, `align_rt_crypto_chacha20_poly1305_open` |
| A16 | `i32 @SYM(ptr, i64, ptr, i64, ptr, ptr, ptr, i64, i64)` | `align_rt_json_decode_soa` |
| A17 | `i32 @SYM(ptr, i64, ptr, ptr)` | `align_rt_json_doc_parse`, `align_rt_fs_read_file_view`, `align_rt_fs_read_bytes_view` |
| A103 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, ptr, i64, i64, ptr)` | `align_rt_json_decode` (final nullable arena) |
| A104 | `i32 @SYM(ptr, i64, ptr, i64, i64, ptr, ptr, i64, i64, ptr)` | `align_rt_json_decode_struct_array` (final nullable arena) |
| A105 | `i32 @SYM(ptr, i64, ptr, ptr, ptr)` | `align_rt_json_decode_union` (final nullable arena) |
| A18 | `i32 @SYM(ptr, i64, ptr, ptr, i64, ptr, i64, ptr, i64, i64)` | `align_rt_json_scan_next` |
| A19 | `i32 @SYM(ptr, ptr)` | `align_rt_builder_finish_bounded_stack`, `align_rt_tcp_accept`, `align_rt_command_run`, `align_rt_io_writer_write_builder`, `align_rt_http_accept`, `align_rt_http_respond`, `align_rt_http_stream_reject` |
| A20 | `i32 @SYM(ptr, ptr, i64)` | `align_rt_io_writer_write`, `align_rt_cli_get_bool`, `align_rt_regex_is_match`, `align_rt_http_stream_send`, `align_rt_http_stream_send_event` |
| A21 | `i32 @SYM(ptr, ptr, i64, i64, ptr)` | `align_rt_http_get_many`, `align_rt_regex_find` |
| A22 | `i32 @SYM(ptr, ptr, i64, ptr)` | `align_rt_cli_parse`, `align_rt_http_resp_header`, `align_rt_http_client_get`, `align_rt_regex_find_all`, `align_rt_regex_split`, `align_rt_regex_captures`, `align_rt_http_ctx_header` |
| A23 | `i32 @SYM(ptr, ptr, i64, ptr, i64, ptr)` | `align_rt_http_client_post` |
| A24 | `i32 @SYM(ptr, ptr, ptr)` | `align_rt_http_client_request`, `align_rt_http_respond_stream` |
| A25 | `i64 @SYM()` | `align_rt_time_now`, `align_rt_time_instant`, `align_rt_process_cpu_count` |
| A26 | `i64 @SYM(ptr readonly captures(none), i64) {nofree nosync willreturn memory(argmem: read)}` | `align_rt_hash64` |
| A27 | `i64 @SYM(ptr readonly captures(none), i64, ptr readonly captures(none), i64) {nofree nosync willreturn}` | `align_rt_str_find`, `align_rt_str_rfind` |
| A28 | `i64 @SYM(ptr readonly captures(none), ptr readonly captures(none), i64) {nofree nosync willreturn}` | `align_rt_str_finder_find` |
| A29 | `i64 @SYM(ptr)` | `align_rt_child_wait`, `align_rt_run_output_code`, `align_rt_io_file_len`, `align_rt_buffer_len`, `align_rt_rng_next`, `align_rt_http_resp_status`, `align_rt_regex_group_count` |
| A30 | `i64 @SYM(ptr, i64)` | `align_rt_json_doc_len` |
| A31 | `i64 @SYM(ptr, i64, i64)` | `align_rt_rng_range` |
| A32 | `i64 @SYM(ptr, i64, i64, i64, i64, ptr, ptr, i64)` | `align_rt_group_sum_str`, `align_rt_group_min_str`, `align_rt_group_max_str`, `align_rt_group_count_str` |
| A33 | `i64 @SYM(ptr, i64, i64, i64, ptr, i64, ptr, i64)` | `align_rt_group_multi_str` |
| A34 | `i64 @SYM(ptr, i64, i64, i64, ptr, ptr, i64)` | `align_rt_dict_encode_str` |
| A35 | `i64 @SYM(ptr, i64, ptr, ptr, i64)` | `align_rt_group_count_i64` |
| A36 | `i64 @SYM(ptr, ptr)` | `align_rt_udp_recv_from`, `align_rt_io_reader_read`, `align_rt_io_reader_read_line`, `align_rt_io_copy` |
| A37 | `i64 @SYM(ptr, ptr, i64)` | `align_rt_io_file_pread`, `align_rt_cli_get_i64`, `align_rt_regex_group_index` |
| A38 | `i64 @SYM(ptr, ptr, i64, i64)` | `align_rt_io_file_pwrite` |
| A39 | `i64 @SYM(ptr, ptr, i64, i64, i64, i64, ptr)` | `align_rt_par_map_reduce` |
| A40 | `i64 @SYM(ptr, ptr, i64, ptr, i64, i64)` | `align_rt_udp_send_to` |
| A41 | `i64 @SYM(ptr, ptr, i64, ptr, ptr, i64)` | `align_rt_group_sum_i64`, `align_rt_group_min_i64`, `align_rt_group_max_i64`, `align_rt_group_sum_str_cols`, `align_rt_group_min_str_cols`, `align_rt_group_max_str_cols`, `align_rt_group_count_str_cols` |
| A42 | `noalias ptr @SYM() {nofree nounwind}` | `align_rt_arena_begin`, `align_rt_tg_begin` |
| A43 | `noalias ptr @SYM(i64) {nofree nounwind}` | `align_rt_alloc`, `align_rt_array_builder_new` |
| A44 | `noalias ptr @SYM(ptr, i64) {nofree nounwind}` | `align_rt_str_finder_new`, `align_rt_builder_new` |
| A45 | `noalias ptr @SYM(ptr, i64, i64) {nounwind}` | `align_rt_arena_alloc`, `align_rt_array_builder_new_in`, `align_rt_tg_alloc` |
| A46 | `noalias ptr @SYM(ptr, ptr, i64, i64, i64, i64, ptr)` | `align_rt_par_map` |
| A47 | `ptr @SYM()` | `align_rt_io_reader_stdin`, `align_rt_http_client_new` |
| A48 | `ptr @SYM(i32, i32)` | `align_rt_io_writer_std` |
| A49 | `ptr @SYM(i64)` | `align_rt_buffer_new`, `align_rt_http_response_new` |
| A50 | `ptr @SYM(ptr)` | `align_rt_tg_wait`, `align_rt_tcp_conn_reader`, `align_rt_tcp_conn_writer`, `align_rt_io_reader_buffered` |
| A51 | `ptr @SYM(ptr, i64)` | `align_rt_array_builder_init_stack`, `align_rt_builder_init_bounded_stack`, `align_rt_cli_command_new` |
| A52 | `ptr @SYM(ptr, i64, ptr, i64)` | `align_rt_command_new`, `align_rt_http_request_new` |
| A53 | `ptr @SYM(ptr, ptr, i64)` | `align_rt_builder_init_stack` |
| A54 | `void @SYM() {noreturn}` | `align_rt_div_fail`, `align_rt_alloc_size_fail`, `align_rt_process_abort` |
| A55 | `void @SYM(double)` | `align_rt_print_f64` |
| A56 | `void @SYM(float)` | `align_rt_print_f32` |
| A57 | `void @SYM(i32)` | `align_rt_print_bool`, `align_rt_print_char` |
| A58 | `void @SYM(i64)` | `align_rt_print_i64`, `align_rt_time_sleep` |
| A59 | `void @SYM(i64) {noreturn}` | `align_rt_process_exit` |
| A60 | `void @SYM(i64, i64) {noreturn}` | `align_rt_bounds_fail`, `align_rt_len_mismatch_fail`, `align_rt_utf8_boundary_fail` |
| A61 | `void @SYM(i64, i64, i64) {noreturn}` | `align_rt_range_fail` |
| A62 | `void @SYM(ptr)` | `align_rt_arena_end`, `align_rt_tg_end`, `align_rt_free`, `align_rt_str_finder_free`, `align_rt_builder_pop_comma`, `align_rt_tcp_conn_free`, `align_rt_tcp_listener_free`, `align_rt_udp_socket_free`, `align_rt_child_free`, `align_rt_command_env_clear`, `align_rt_command_free`, `align_rt_run_output_free`, `align_rt_io_reader_free`, `align_rt_io_writer_free`, `align_rt_io_file_free`, `align_rt_buffer_free`, `align_rt_array_builder_free`, `align_rt_array_builder_free_stack`, `align_rt_array_builder_free_strings`, `align_rt_array_builder_free_strings_stack`, `align_rt_crypto_random`, `align_rt_rng_seed_os`, `align_rt_cli_command_free`, `align_rt_cli_parsed_free`, `align_rt_http_request_free`, `align_rt_http_resp_free`, `align_rt_http_client_free`, `align_rt_http_server_free`, `align_rt_regex_captures_free`, `align_rt_regex_free`, `align_rt_http_ctx_free`, `align_rt_http_response_free`, `align_rt_http_stream_free`, `align_rt_builder_free`, `align_rt_builder_free_stack` |
| A63 | `void @SYM(ptr, double)` | `align_rt_builder_write_f64` |
| A64 | `void @SYM(ptr, float)` | `align_rt_builder_write_f32` |
| A65 | `void @SYM(ptr, i32)` | `align_rt_builder_write_bool`, `align_rt_builder_write_char` |
| A66 | `void @SYM(ptr, i64)` | `align_rt_print_str`, `align_rt_builder_write_int`, `align_rt_tcp_read_timeout`, `align_rt_tcp_write_timeout`, `align_rt_command_timeout`, `align_rt_free_string_array`, `align_rt_array_builder_push`, `align_rt_rng_seed_with`, `align_rt_http_timeout`, `align_rt_http_client_timeout`, `align_rt_free_response_array` |
| A67 | `void @SYM(ptr, i64, i64, i32)` | `align_rt_buffer_put` |
| A68 | `void @SYM(ptr, i64, i64, i64, ptr)` | `align_rt_gather_i64` |
| A69 | `void @SYM(ptr, i64, i64, ptr)` | `align_rt_json_doc_at` |
| A70 | `void @SYM(ptr, i64, ptr, i64, ptr)` | `align_rt_json_doc_get`, `align_rt_dict_lookup` |
| A71 | `void @SYM(ptr, i64, ptr, ptr)` | `align_rt_json_doc_elems` |
| A72 | `void @SYM(ptr, ptr)` | `align_rt_array_builder_push_bytes`, `align_rt_buffer_bytes` |
| A73 | `void @SYM(ptr, ptr, i64)` | `align_rt_builder_write`, `align_rt_builder_write_json_str`, `align_rt_command_cwd`, `align_rt_buffer_append`, `align_rt_array_builder_push_str`, `align_rt_array_builder_append`, `align_rt_cli_flag_bool`, `align_rt_http_body`, `align_rt_http_rb_body` |
| A74 | `void @SYM(ptr, ptr, i64, i32)` | `align_rt_json_encode_scalar_array` |
| A75 | `void @SYM(ptr, ptr, i64, i64)` | `align_rt_rng_shuffle`, `align_rt_cli_flag_i64` |
| A76 | `void @SYM(ptr, ptr, i64, i64, ptr, i64)` | `align_rt_builder_write_str_int_str` |
| A77 | `void @SYM(ptr, ptr, i64, ptr, i64)` | `align_rt_command_env`, `align_rt_cli_flag_str`, `align_rt_http_header`, `align_rt_http_rb_header` |
| A78 | `void @SYM(ptr, ptr, i64, ptr, i64, i64)` | `align_rt_json_encode_struct_array` |
| A79 | `void @SYM(ptr, ptr, ptr)` | `align_rt_json_encode_union` |
| A80 | `void @SYM(ptr, ptr, ptr, i64)` | `align_rt_json_encode_object` |
| A81 | `void @SYM(ptr, ptr, ptr, ptr, ptr, ptr)` | `align_rt_tg_register` |
| A82 | `{ i64, i64 } @SYM(ptr readonly captures(none), i64) {nofree nosync willreturn memory(argmem: read)}` | `align_rt_hash128` |
| A83 | `{ ptr, i64 } @SYM(ptr)` | `align_rt_run_output_stdout`, `align_rt_run_output_stderr`, `align_rt_array_builder_build`, `align_rt_array_builder_build_stack`, `align_rt_builder_finish`, `align_rt_builder_finish_stack`, `align_rt_cli_usage`, `align_rt_http_resp_body`, `align_rt_http_ctx_method`, `align_rt_http_ctx_path`, `align_rt_http_ctx_body`, `align_rt_builder_into_string`, `align_rt_builder_into_string_stack` |
| A84 | `{ ptr, i64 } @SYM(ptr, i64)` | `align_rt_str_clone`, `align_rt_base64_encode`, `align_rt_base64url_encode`, `align_rt_hex_encode`, `align_rt_percent_encode`, `align_rt_form_encode`, `align_rt_html_escape`, `align_rt_crypto_sha256`, `align_rt_crypto_sha512`, `align_rt_str_trim`, `align_rt_str_trim_start`, `align_rt_str_trim_end`, `align_rt_path_base`, `align_rt_path_dir`, `align_rt_path_ext`, `align_rt_path_normalize` |
| A85 | `{ ptr, i64 } @SYM(ptr, i64, i64, i64)` | `align_rt_chunks` |
| A86 | `{ ptr, i64 } @SYM(ptr, i64, ptr, i64)` | `align_rt_crypto_hmac_sha256`, `align_rt_path_join` |
| A87 | `{ ptr, i64 } @SYM(ptr, ptr, i64)` | `align_rt_cli_get_str` |
| A88 | `{ ptr, i64 } @SYM(ptr, ptr, i64, i64, i64)` | `align_rt_rng_sample` |
| A89 | `{ ptr, i64 } @SYM(ptr, ptr, i64, i64, i64, i64, ptr, ptr)` | `align_rt_par_map_filter` |
| A90 | `{ ptr, i64 } @SYM(ptr, ptr, i64, ptr, i64, i32)` | `align_rt_regex_replace` |

Request 11 keyed delta:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `CommandMaxCapture` | `align_rt_command_max_capture` | A66: `void @SYM(ptr, i64)`; no curated attributes |
| `CommandRunBytes` | `align_rt_command_run_bytes` | A19: `i32 @SYM(ptr, ptr)`; no curated attributes |
| `RunBytesCode` | `align_rt_run_bytes_code` | A29: `i64 @SYM(ptr)`; no curated attributes |
| `RunBytesStdout` | `align_rt_run_bytes_stdout` | A83: `{ ptr, i64 } @SYM(ptr)`; no curated attributes |
| `RunBytesStderr` | `align_rt_run_bytes_stderr` | A83: `{ ptr, i64 } @SYM(ptr)`; no curated attributes |
| `RunBytesFree` | `align_rt_run_bytes_free` | A62: `void @SYM(ptr)`; no curated attributes |

All six use the regular `align_rt_` plus snake-case key mapping and occupy collision-reserved native
identities as soon as the capability activates. At that capability boundary,
`runtime_abi_registry_is_complete_and_unique` owned the then-current 294/307 counts,
key/symbol bijection, and reverse lookup; the exact extern-type matrix owns every
parameter/return/attribute cell; the checked-in declaration golden owns spelling and row order; and
the base/feature runtime-export parity owners require all six definitions in every normal runtime
while rejecting any missing, duplicate, near-spelled, or wrong-signature record. The capability must
not add a direct declaration outside this registry.

Request 5 bounded-HTTP shipped delta:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `HttpMaxResponseBodyBytes` | `align_rt_http_max_response_body_bytes` | A66: `void @SYM(ptr, i64)`; no curated attributes |
| `HttpClientMaxResponseBodyBytes` | `align_rt_http_client_max_response_body_bytes` | A66: `void @SYM(ptr, i64)`; no curated attributes |

Both use ordinary keyed-native identity and are mandatory base exports. The implementation updated
registry counts, bijection, declaration golden, and base runtime-export parity in the same change.
The HTTP-private negative result sentinel is not an ABI
symbol: client-response MIR maps it to reserved `Error.Code(-1)` before the common positive status
decoder, so it cannot collide with a saturating encoded errno.

Unkeyed native records:

| Owner | Exact LLVM declaration | Runtime export presence |
|---|---|---|
| main error wrapper | `i32 @align_rt_report_error(i32)` | every Unit/Result main wrapper; no attributes |
| argv wrapper | `{ ptr, i64 } @align_rt_args_build(i32, ptr)` | only argv main; no attributes |
| arena implementation | `void @align_rt_arena_reset(ptr)` | always linked; runtime-internal, no curated declaration attributes |
| allocator implementation | `ptr @align_rt_realloc(ptr, i64)` | always linked; runtime-internal, no curated declaration attributes |
| HTTP implementation | `i32 @align_rt_http_serialize(ptr, ptr)` | always linked; runtime-internal, no curated declaration attributes |
| PostgreSQL codec | `i32 @align_rt_f32_to_bits(float) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `float @align_rt_f32_from_bits(i32) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `i64 @align_rt_f64_to_bits(double) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `double @align_rt_f64_from_bits(i64) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `i64 @align_rt_f32_text_len(float) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `i64 @align_rt_f64_text_len(double) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `i64 @align_rt_f32_text_write(float, ptr, i64) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| PostgreSQL codec | `i64 @align_rt_f64_text_write(double, ptr, i64) {nofree nosync willreturn}` | always linked; package-internal compatible extern |
| allocation probe | `i64 @align_rt_alloc_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| allocation probe | `i64 @align_rt_free_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| finder probe | `i64 @align_rt_str_finder_new_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| finder probe | `i64 @align_rt_str_finder_free_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| parallel probe | `void @align_rt_test_par_map_force_caller(i32)` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_min_chunk()` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_min_chunk_for(i64, i64, i64)` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_workers()` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |

## Machine gates

Am-c1 replaces the current ABI/declaration, dedicated-consumer, AEAD-selection,
and attribute authorities with one typed `RuntimeAbi` row per identity:
`{ key, symbol, return type, ordered parameter types, return attrs, parameter
attrs, function attrs, rt_lto_policy }`. Declaration and call lookup consume
that row. `key` is `RuntimeAbiId`, either `Keyed(RuntimeKey)` or
`Unkeyed(UnkeyedRuntimeKey)`. The thirteen base unkeyed records use the same ABI
shape; only `ReportError` and `ArgsBuild` have a dedicated Align main-wrapper
declaration policy and yield typed wrapper handles when that wrapper requires
them. The other eleven yield no unconditional compiler handle. An exact
compatible source extern may declare or reuse every unkeyed row except `ArgsBuild`
through the ordinary extern path. `ArgsBuild`
returns the native `{ptr, i64}` argv view, which no source-valid extern return
can express: `str`/slice view returns are rejected, `raw` is not a valid
`layout(C)` field, and the closest source-valid `layout(C) { u64, i64 }` return
lowers as `{i64, i64}` rather than `{ptr, i64}`. It is therefore wrapper-only,
the only base unkeyed row without a compatible source-extern form. The legacy mixed string map remains as a handle-only alias seam
for unchanged `Rvalue::Call(String)` resolution through c1: it is populated in
post-c1 class order from stored definitions, non-shadowed imports, externs,
alphabetical `RuntimeKey::ALL` aliases, then existing generated aliases. The
keyed aliases are mutually unique, so their relative normalization preserves
every final binding. The alias seam cannot define a symbol, type, attribute,
membership, or reuse rule. C3 deletes it.

Registry and extern-compatibility preflight create no LLVM value. C1 fixes LLVM
construction order as stored definitions in vector order, non-shadowed imports
in vector order, externs in vector order, keyed native rows in alphabetical
`RuntimeKey::ALL` order, then generated helpers in their existing order. This
intentionally changes only the relative keyed-native declaration order from the
hand-written pre-c1 source; keyed physical symbols are mutually unique and all
program/import/extern claimants still precede them. Textual/raw LLVM order
changes once; object bytes may change or remain equal and have no equality
promise. Compiler build id changes cache identity for one miss then an
unchanged-input hit. Every symbol spelling and final legacy alias binding
remains fixed. A source extern is compatible exactly when its
source-derived LLVM function type equals the fixed row; source externs carry no
curated native attributes. The selected native row supplies every curated
attribute and rt-LTO policy to the one reused handle. A stored/imported program
function with the same physical spelling is not native reuse and retains the
current LLVM uniquification result until c3 encodes program symbols.
Main-wrapper emission reuses a type-compatible `ReportError` extern or adds its
row, and always adds the wrapper-only `ArgsBuild` row when argv marshalling is
required. Both remain in current wrapper order. Attributes are applied through
typed row handles, never by a symbol prefix scan that could select a program claimant. Thus a same-spelled program
claimant keeps its current physical name/uniquification but loses accidental
native attributes, while the actual possibly-suffixed native declaration gains
the row attributes.

The eight probe rows have
`verification_presence = AllocCount | ParMapProbe`: their exact signatures are
checked against the corresponding runtime fixture, but their names do not
participate in compiler collision validation and no compatible-extern reuse
path is synthesized for them. The compiler uses the fixed base table before
LLVM construction and receives no runtime-feature input.

Tests compare:

- all 296 keys, mapped symbols, LLVM declaration types, and default attributes
  against this table through the checked-in
  `crates/align_codegen_llvm/tests/golden/runtime_abi_declarations.txt`;
- the 309 base native symbols against default-feature `align_runtime` exports,
  plus every actual Rust definition's normalized native return and ordered
  parameter types against the declaration golden, failing on either direction's
  difference through `scripts/test-runtime-abi-exports.sh`;
- the 313 `alloc-count` and 313 `par-map-probe` native symbols against
  `align_runtime` built with each feature separately, including the four exact
  probe signatures above;
- the 317 maximum native symbols against `align_runtime` built with
  `alloc-count,par-map-probe,task-group-probe`, while proving
  `task-group-probe` adds no unmangled export;
- rt-LTO off/on attributes for every guarded symbol, with missing,
  declaration-only, wrong-type, internal, private, available-externally, and
  non-C-calling-convention artifact negatives;
- all 309 identities through the one `RuntimeAbiId`-keyed row iterator and all
  309 exact registry function types through the production compatibility
  predicate, one return mutation per row, and one mutation of every parameter
  ordinal; source-valid compatible reuse for a keyed builtin and the twelve
  source-reachable unkeyed rows; exact `ArgsBuild` `str` rejection plus the
  source-valid `layout(C) { u64, i64 }` aggregate mismatch; and
  compatible reuse representatives for each of the five checked-in attribute
  classes (`#0`–`#4`), with the native row supplying its curated attributes;
- one mutation of each registry attribute class, symbol, and key through the
  checked-in golden and uniqueness owners;
- ordinary extern and program-definition positives for all eight probe
  spellings while the normal runtime export set excludes them; and
- trivial whole-program and per-unit-shaped emitted IR with identical
  alphabetical runtime declarations, whose exact rows are owned by the
  checked-in declaration golden above.

For rt-LTO, codegen first requires all four guarded logical symbols in this
deterministic order: present, exact registry function type, body present,
external linkage, and C calling convention. Any failed check loudly falls back
without merging and reapplies the curated attributes to every guarded
declaration. The complete baked definitions then begin with their logical
symbols. Before linking, codegen renames each one to
the captured physical LLVM name of its typed declaration. This is normally the
same spelling; when a preceding program/import claimant forced LLVM
uniquification, it is the suffixed native name. The merge therefore fills only
the typed native handle and never the same-spelled program claimant. After
linking, every captured typed handle must still be an external C-convention
definition with a body before attributes are removed and linkage becomes
internal; a missing body or changed linkage/convention is a compiler error,
never a silent static-runtime fallback after partial mutation.

## D14 generated SQLite scalar-callback ABI

D14's callback entrypoints are program-generated helpers, not runtime-native declarations. They do
not add a `RuntimeKey`, `UnkeyedRuntimeKey`, runtime export, compatible-extern reuse row, or registry
count. The generated-family identity includes callback kind/version, target identity, exact Align
signature, effect, return provenance/cleanup, and the canonical C signature. Each semantic callback
has one private/internal Align target reference, one program-lifetime 32-byte v1 descriptor, and one
C-callable generated definition whose physical name is reserved by the ordinary generated-symbol
collision preflight.

The exact external callback ABIs are:

| Kind | Exact LLVM function type | Required attributes and body boundary |
|---|---|---|
| ScalarFunction v1 | `void @GEN(ptr, i32, ptr)` | C calling convention, `nounwind`; hard-abort null context/database handle, validate argc/argv and all 0..127 SQLite values in type/byte-count/final-pointer order with immediate errcode after null Text, normalize empty views to a stable non-null sentinel, build fixed stack scratch, call the exact Align target once, consume its dynamic cleanup result, call one result/error family, return void |

The function is not `readnone`, `readonly`, `willreturn`, or `nofree`: SQLite value/result routines
may allocate or mutate native state, scalar application code may be Impure, and the Align target
may hard-terminate. `nounwind` states only that no language or Rust unwind crosses C. Descriptor
validation and generated-body preflight occur before its address is installed in
SQLite. Whole-program, per-unit, and ThinLTO emission must agree on the descriptor bytes, target
symbol, physical generated name, C type, and attributes; a malformed checked program emits neither
descriptor nor callback definition.
