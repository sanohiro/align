# Runtime native ABI ledger

## Status and authority

This is the exact native symbol/type/attribute appendix for L2b-a2-am-r and
L2b-a2-am-c. It records the LLVM 22 declaration surface emitted by the current
backend for every validated runtime target and the additional externally
visible runtime definitions that occupy link identities. The keyed surface is
generated from a trivial valid program; the complete base and `alloc-count`
surfaces are independently compared with the Rust runtime exports.

Am-c has 281 `RuntimeKey` variants and a one-to-one native-symbol record. Four
AEAD symbols that were previously selected from `AeadCipher × AeadDir` become
ordinary typed keys; they may no longer bypass the registry. Five always-built
runtime records have no `RuntimeKey`: the two main-wrapper callees
`align_rt_report_error` and `align_rt_args_build`, plus the runtime-internal
`align_rt_arena_reset`, `align_rt_realloc`, and
`align_rt_http_serialize`. The base native registry therefore has 286 records.
The explicit `alloc-count` runtime feature adds four test/benchmark-only
counter exports. `par-map-probe` adds four more:
`void @align_rt_test_par_map_force_caller(i32)`,
`i64 @align_rt_test_par_map_min_chunk()`,
`i64 @align_rt_test_par_map_min_chunk_for(i64, i64, i64)`, and
`i64 @align_rt_test_par_map_workers()`. `task-group-probe` changes internal
Rust state only and adds no unmangled native export. The maximum registry,
with all three features enabled, therefore has 294 records. Registry
membership is never inferred from symbol spelling.

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
| A11 | `i32 @SYM(ptr, i64, ptr, i64, i64, ptr, ptr, i64, i64)` | `align_rt_json_decode_struct_array` |
| A12 | `i32 @SYM(ptr, i64, ptr, i64, ptr)` | `align_rt_process_spawn` |
| A13 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, i64, ptr)` | `align_rt_crypto_hkdf_sha256` |
| A14 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, ptr, i64, i64)` | `align_rt_json_decode` |
| A15 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, ptr, i64, ptr)` | `align_rt_crypto_aes_gcm_seal`, `align_rt_crypto_aes_gcm_open`, `align_rt_crypto_chacha20_poly1305_seal`, `align_rt_crypto_chacha20_poly1305_open` |
| A16 | `i32 @SYM(ptr, i64, ptr, i64, ptr, ptr, ptr, i64, i64)` | `align_rt_json_decode_soa` |
| A17 | `i32 @SYM(ptr, i64, ptr, ptr)` | `align_rt_json_decode_union`, `align_rt_json_doc_parse`, `align_rt_fs_read_file_view`, `align_rt_fs_read_bytes_view` |
| A18 | `i32 @SYM(ptr, i64, ptr, ptr, i64, ptr, i64, ptr, i64, i64)` | `align_rt_json_scan_next` |
| A19 | `i32 @SYM(ptr, ptr)` | `align_rt_tcp_accept`, `align_rt_command_run`, `align_rt_io_writer_write_builder`, `align_rt_http_accept`, `align_rt_http_respond`, `align_rt_http_stream_reject` |
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
| A45 | `noalias ptr @SYM(ptr, i64, i64) {nounwind}` | `align_rt_arena_alloc`, `align_rt_tg_alloc` |
| A46 | `noalias ptr @SYM(ptr, ptr, i64, i64, i64, i64, ptr)` | `align_rt_par_map` |
| A47 | `ptr @SYM()` | `align_rt_io_reader_stdin`, `align_rt_http_client_new` |
| A48 | `ptr @SYM(i32, i32)` | `align_rt_io_writer_std` |
| A49 | `ptr @SYM(i64)` | `align_rt_buffer_new`, `align_rt_http_response_new` |
| A50 | `ptr @SYM(ptr)` | `align_rt_tg_wait`, `align_rt_tcp_conn_reader`, `align_rt_tcp_conn_writer`, `align_rt_io_reader_buffered` |
| A51 | `ptr @SYM(ptr, i64)` | `align_rt_array_builder_init_stack`, `align_rt_cli_command_new` |
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
| A72 | `void @SYM(ptr, ptr)` | `align_rt_buffer_bytes` |
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

Unkeyed native records:

| Owner | Exact LLVM declaration | Presence |
|---|---|---|
| main error wrapper | `i32 @align_rt_report_error(i32)` | every Unit/Result main wrapper; no attributes |
| argv wrapper | `{ ptr, i64 } @align_rt_args_build(i32, ptr)` | only argv main; no attributes |
| arena implementation | `void @align_rt_arena_reset(ptr)` | always linked; runtime-internal, no curated declaration attributes |
| allocator implementation | `ptr @align_rt_realloc(ptr, i64)` | always linked; runtime-internal, no curated declaration attributes |
| HTTP implementation | `i32 @align_rt_http_serialize(ptr, ptr)` | always linked; runtime-internal, no curated declaration attributes |
| allocation probe | `i64 @align_rt_alloc_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| allocation probe | `i64 @align_rt_free_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| finder probe | `i64 @align_rt_str_finder_new_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| finder probe | `i64 @align_rt_str_finder_free_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| parallel probe | `void @align_rt_test_par_map_force_caller(i32)` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_min_chunk()` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_min_chunk_for(i64, i64, i64)` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_workers()` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |

## Machine gates

Am-c replaces the current declaration statements, runtime string map, AEAD
selection match, and attribute lookup with one typed `RuntimeAbi` row per key:
`{ key, symbol, return type, ordered parameter types, return attrs, parameter
attrs, function attrs, rt_lto_policy }`. Declaration and call lookup consume
that row. Unkeyed records use the same ABI shape without a key and add
`presence = Always | AllocCount | ParMapProbe`; only the two wrapper records
have an Align module declaration policy. All thirteen unkeyed identities still
participate in external-collision and compatible-extern validation before LLVM
construction.

Tests compare:

- all 281 keys, mapped symbols, LLVM declaration types, and default attributes
  against this table;
- the 286 base native symbols against default-feature `align_runtime` exports
  and fail on either direction's difference;
- the 290 `alloc-count` and 290 `par-map-probe` native symbols against
  `align_runtime` built with each feature separately, including the four exact
  probe signatures above;
- the 294 maximum native symbols against `align_runtime` built with
  `alloc-count,par-map-probe,task-group-probe`, while proving
  `task-group-probe` adds no unmangled export;
- rt-LTO off/on attributes for every guarded symbol;
- compatible extern reuse against the active registry's complete LLVM type and
  curated declaration attributes;
- one mutation of return type, every parameter ordinal, each attribute class,
  symbol, and key; and
- trivial whole/per-unit emitted IR against a checked-in declaration golden
  whose body is exactly the expanded rows above.
