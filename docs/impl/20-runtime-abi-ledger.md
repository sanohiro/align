# Runtime native ABI ledger

**R69–R76: all eight native rows implemented:**
[Plan 54 §5](54-r69-r76-prerequisite-batch-plan.md#5-compiler-representation-and-native-abi)
fixes eight exported symbols and their exact shapes, scratch layouts and
ownership. Six native-observation rows and the lossy/SHA-1 rows are implemented.
`align_rt_utf8_decode_lossy` and `align_rt_crypto_sha1` both use shape A84,
returning independently owned pointer/length results from borrowed bytes. Canonical leaf tags are unchanged. R88 adds the ordinary-path regular-reader row (plan 58). Plan 65's
SIMD-surface consistency capability adds `BufferAppendFilled`. Current inventory: 446 keyed,
464 base, 471 alloc-count, 468 par-map-probe and 475 maximum exports.

**R65 planned contract:** [plan 50](50-r65-process-capability-handoff.md) and
[plan 49](49-native-process-contract.md), designed and ready for implementation, own the new
closed operation schemas, writable out provenance, receiver families, Move
carriers, canonical tags, exact native declarations and replacement child-wait/
capture-status ABI. Current rows below remain the shipped baseline; update them
atomically with the implementing capability, not as an alternative contract.

**R63 implementation contract:**
[Plan 47](47-json-numeric-contract.md) supersedes this document's JSON float
exclusions, infallible/arena-view encoder result, bounded-only encoder IR/native
names and V1/V2 JSON transport. Both encoders return owned `Result<string, Error>`;
§§1–5 fix errors, ownership, V3 descriptor/envelope, interface 11 -> 12, the unified
HIR/MIR operation and exact ABI rows. §8 owns closure and §9 performance evidence.
Historical vectors/version transitions and current shipped ABI inventories below
remain baseline records, not alternate implementation contracts. In particular,
plan 47 records the replacement ABI counts and rows.

## Status and authority

This is the exact native symbol/type/attribute appendix for L2b-a2-am-r and
L2b-a2-am-c1. It records the LLVM 22 declaration surface emitted by the current
backend for every validated runtime target and the additional externally
visible runtime definitions that occupy link identities. The keyed surface is
generated from a trivial valid program; the complete base and `alloc-count`
surfaces are independently compared with the Rust runtime exports.

With bounded canonical JSON, process capture, bounded HTTP response bodies,
owned JSON, exclusive filesystem publication, retained-root regular-file access, private temporary-directory
lifecycle, HTTP client
raw/SSE receive streaming, asymmetric signatures, `std.log`, `core.codec`, `pkg.frame`, `pkg.kv`,
`pkg.csv`, `pkg.ws`, `pkg.template`, named time/path wire formats, incremental SHA-256 host observation and ordinary directory operations, there
are 446 `RuntimeKey` variants and a one-to-one native-symbol record. Relative to Am-c1, F-B added
`ArrayBuilderNewIn` and `ArrayBuilderPushBytes`; the four
AEAD symbols that were previously selected from `AeadCipher × AeadDir` become
ordinary typed keys; they may no longer bypass the registry. Eighteen always-built
runtime records have no `RuntimeKey` and instead use the eighteen-variant
`UnkeyedRuntimeKey`: the two main-wrapper callees
`align_rt_report_error` and `align_rt_args_build`, plus the runtime-internal
`align_rt_arena_reset`, `align_rt_realloc`, and
`align_rt_http_serialize`, and eight package-internal PostgreSQL codec helpers:
`align_rt_f32_to_bits`, `align_rt_f32_from_bits`, `align_rt_f64_to_bits`,
`align_rt_f64_from_bits`, `align_rt_f32_text_len`, `align_rt_f64_text_len`,
`align_rt_f32_text_write`, and `align_rt_f64_text_write`, plus the four compiler-private
`core.test` child-control rows recorded below and the package-internal checked TCP timeout row
`align_rt_tcp_conn_set_io_timeout`. The base native registry therefore has 464 records. R63 replaces Request 12's two bounded-only rows with four unified JSON builder rows
(A127–A130), including separate finite-only f32/f64 writers.
The explicit `alloc-count` runtime feature may expose seven
test/benchmark-only definitions: the allocation/free and finder counters plus
`align_rt_requested_live_reset`, `align_rt_requested_live_bytes`, and
`align_rt_requested_live_peak`. `par-map-probe` may expose four more:
`void @align_rt_test_par_map_force_caller(i32)`,
`i64 @align_rt_test_par_map_min_chunk()`,
`i64 @align_rt_test_par_map_min_chunk_for(i64, i64, i64)`, and
`i64 @align_rt_test_par_map_workers()`. `task-group-probe` and
`crypto-asymmetric-probe` change internal Rust state only and add no unmangled native export.

The compiler-visible native registry is always exactly the 464 base records.
There is no target option, environment variable, Cargo feature, linked-runtime
inspection, or other ambient input that changes it. The eleven optional probe
records extend only the verification-time maximum runtime-export table to 475.
They never gain a `RuntimeKey`, callable/declaration policy, collision
reservation, or compatible-extern reuse. Their spellings remain ordinary
program/extern/export identities in a normal build. Probe-feature runtime
builds are test/benchmark fixtures and never link user artifacts. Thus runtime
feature selection changes neither accepted callable input nor MIR, interface,
artifact, or cache identity. Registry membership is never inferred from symbol
spelling.

Request 11 added six regular `RuntimeKey` rows for bounded process capture. Request 5 subsequently
added the two bounded-HTTP setters, and Request 9 then added `BuilderWriteUint`.
The raw HTTP receive-stream capability subsequently added six keyed rows for buffer-capacity
inspection, stream construction/access/read, and Drop. The SSE capability added four keyed rows for
the consuming transition, state getters, and event read. The asymmetric signature suite then added
six keyed rows. The `core.test` child-control extension then added four unkeyed rows, and `std.log`
added six keyed rows, `core.codec` then added eight, `pkg.frame` added two, `pkg.kv` added one
source-reachable unkeyed row, `pkg.csv` added one keyed row, `pkg.ws` added eleven keyed rows, and
`pkg.template` added five keyed rows. Their runtime
definitions and registry entries activated atomically at their respective capability boundaries: the current exact counts
are 446 keyed records, 464 base records, and 475 records in the maximum optional-probe export table.
No new probe category was introduced. The implemented `pkg.kv` row reuses an existing ABI shape. Its two
independently useful prerequisites first hardened the shared TCP timeout substrate and existing
TCP-derived writers without changing a symbol, key, shape, attribute, or count.

## Runtime effect classification

Every one of the 464 base rows has one `RuntimeEffects` record in
`align_codegen_llvm::runtime_abi`. The record is a total match over
`RuntimeAbiId`; there is no unclassified/default arm. Its exact fields are
`class`, `argmem`, per-pointer `params`, `escapes`, `releases`,
`returns_fresh`, and `diverges`. `escapes` is conservative: a pointer absent
from it receives `captures(none)`, so an unaudited withheld row lists every
pointer ordinal rather than making an optimistic capture claim. The checked-in
declaration golden owns the emitted declaration and the class token together.

| Registry row | Physical symbol | `EffectClass` |
|---|---|---|
| `Alloc` | `align_rt_alloc` | `AllocNew` |
| `AllocSizeFail` | `align_rt_alloc_size_fail` | `FailNoReturn` |
| `ArenaAlloc` | `align_rt_arena_alloc` | `IndirectStorage` |
| `ArenaBegin` | `align_rt_arena_begin` | `AllocNew` |
| `ArenaEnd` | `align_rt_arena_end` | `HostState` |
| `ArrayBuilderAppend` | `align_rt_array_builder_append` | `IndirectStorage` |
| `ArrayBuilderBuild` | `align_rt_array_builder_build` | `IndirectStorage` |
| `ArrayBuilderBuildStack` | `align_rt_array_builder_build_stack` | `IndirectStorage` |
| `ArrayBuilderFree` | `align_rt_array_builder_free` | `IndirectStorage` |
| `ArrayBuilderFreeStack` | `align_rt_array_builder_free_stack` | `IndirectStorage` |
| `ArrayBuilderFreeStrings` | `align_rt_array_builder_free_strings` | `IndirectStorage` |
| `ArrayBuilderFreeStringsStack` | `align_rt_array_builder_free_strings_stack` | `IndirectStorage` |
| `ArrayBuilderInitStack` | `align_rt_array_builder_init_stack` | `IndirectStorage` |
| `ArrayBuilderNew` | `align_rt_array_builder_new` | `AllocNew` |
| `ArrayBuilderNewIn` | `align_rt_array_builder_new_in` | `IndirectStorage` |
| `ArrayBuilderPush` | `align_rt_array_builder_push` | `IndirectStorage` |
| `ArrayBuilderPushBytes` | `align_rt_array_builder_push_bytes` | `IndirectStorage` |
| `ArrayBuilderPushStr` | `align_rt_array_builder_push_str` | `IndirectStorage` |
| `Base64Decode` | `align_rt_base64_decode` | `IndirectStorage` |
| `Base64Encode` | `align_rt_base64_encode` | `ArgRead` |
| `Base64urlDecode` | `align_rt_base64url_decode` | `IndirectStorage` |
| `Base64urlEncode` | `align_rt_base64url_encode` | `ArgRead` |
| `BoundsFail` | `align_rt_bounds_fail` | `FailNoReturn` |
| `BufferAppend` | `align_rt_buffer_append` | `IndirectStorage` |
| `BufferAppendFilled` | `align_rt_buffer_append_filled` | `IndirectStorage` |
| `BufferBytes` | `align_rt_buffer_bytes` | `IndirectStorage` |
| `BufferCapacity` | `align_rt_buffer_capacity` | `IndirectStorage` |
| `BufferFilled` | `align_rt_buffer_filled` | `AllocNew` |
| `BufferFree` | `align_rt_buffer_free` | `IndirectStorage` |
| `BufferLen` | `align_rt_buffer_len` | `IndirectStorage` |
| `BufferNew` | `align_rt_buffer_new` | `AllocNew` |
| `BufferPut` | `align_rt_buffer_put` | `IndirectStorage` |
| `BuilderFinish` | `align_rt_builder_finish` | `IndirectStorage` |
| `BuilderFinishStack` | `align_rt_builder_finish_stack` | `IndirectStorage` |
| `BuilderFree` | `align_rt_builder_free` | `IndirectStorage` |
| `BuilderFreeStack` | `align_rt_builder_free_stack` | `IndirectStorage` |
| `BuilderInitStack` | `align_rt_builder_init_stack` | `IndirectStorage` |
| `BuilderIntoString` | `align_rt_builder_into_string` | `IndirectStorage` |
| `BuilderIntoStringStack` | `align_rt_builder_into_string_stack` | `IndirectStorage` |
| `BuilderNew` | `align_rt_builder_new` | `AllocNew` |
| `BuilderPopComma` | `align_rt_builder_pop_comma` | `IndirectStorage` |
| `BuilderWrite` | `align_rt_builder_write` | `IndirectStorage` |
| `BuilderWriteBool` | `align_rt_builder_write_bool` | `IndirectStorage` |
| `BuilderWriteChar` | `align_rt_builder_write_char` | `IndirectStorage` |
| `BuilderWriteF32` | `align_rt_builder_write_f32` | `IndirectStorage` |
| `BuilderWriteF64` | `align_rt_builder_write_f64` | `IndirectStorage` |
| `BuilderWriteInt` | `align_rt_builder_write_int` | `IndirectStorage` |
| `BuilderWriteJsonStr` | `align_rt_builder_write_json_str` | `IndirectStorage` |
| `BuilderWriteStrIntStr` | `align_rt_builder_write_str_int_str` | `IndirectStorage` |
| `BuilderWriteUint` | `align_rt_builder_write_uint` | `IndirectStorage` |
| `BytesAsStr` | `align_rt_bytes_as_str` | `IndirectStorage` |
| `ChildFree` | `align_rt_child_free` | `HostState` |
| `ChildGroupMembers` | `align_rt_child_group_members` | `HostState` |
| `ChildId` | `align_rt_child_id` | `HostState` |
| `ChildKill` | `align_rt_child_kill` | `HostState` |
| `ChildKillGroup` | `align_rt_child_kill_group` | `HostState` |
| `ChildPoll` | `align_rt_child_poll` | `HostState` |
| `ChildReadStderr` | `align_rt_child_read_stderr` | `HostState` |
| `ChildReadStdout` | `align_rt_child_read_stdout` | `HostState` |
| `ChildStatus` | `align_rt_child_status` | `HostState` |
| `ChildTryWait` | `align_rt_child_try_wait` | `HostState` |
| `ChildWait` | `align_rt_child_wait` | `HostState` |
| `Chunks` | `align_rt_chunks` | `ArgRead` |
| `CliCommand` | `align_rt_cli_command_new` | `AllocNew` |
| `CliCommandFree` | `align_rt_cli_command_free` | `IndirectStorage` |
| `CliFlagBool` | `align_rt_cli_flag_bool` | `IndirectStorage` |
| `CliFlagI64` | `align_rt_cli_flag_i64` | `IndirectStorage` |
| `CliFlagStr` | `align_rt_cli_flag_str` | `IndirectStorage` |
| `CliGetBool` | `align_rt_cli_get_bool` | `IndirectStorage` |
| `CliGetI64` | `align_rt_cli_get_i64` | `IndirectStorage` |
| `CliGetStr` | `align_rt_cli_get_str` | `IndirectStorage` |
| `CliParse` | `align_rt_cli_parse` | `IndirectStorage` |
| `CliParsedFree` | `align_rt_cli_parsed_free` | `IndirectStorage` |
| `CliUsage` | `align_rt_cli_usage` | `IndirectStorage` |
| `CodecEncoderFinishV1` | `align_rt_codec_encoder_finish_v1` | `IndirectStorage` |
| `CodecEncoderFreeV1` | `align_rt_codec_encoder_free_v1` | `IndirectStorage` |
| `CodecEncoderNewV1` | `align_rt_codec_encoder_new_v1` | `IndirectStorage` |
| `CodecEncoderPutBoolV1` | `align_rt_codec_encoder_put_bool_v1` | `IndirectStorage` |
| `CodecEncoderPutF64V1` | `align_rt_codec_encoder_put_f64_v1` | `IndirectStorage` |
| `CodecEncoderPutI64V1` | `align_rt_codec_encoder_put_i64_v1` | `IndirectStorage` |
| `CodecEncoderPutStrV1` | `align_rt_codec_encoder_put_str_v1` | `IndirectStorage` |
| `CodecOpenV1` | `align_rt_codec_open_v1` | `IndirectStorage` |
| `CommandCwd` | `align_rt_command_cwd` | `HostState` |
| `CommandEnv` | `align_rt_command_env` | `HostState` |
| `CommandEnvClear` | `align_rt_command_env_clear` | `HostState` |
| `CommandFree` | `align_rt_command_free` | `HostState` |
| `CommandImage` | `align_rt_command_image` | `HostState` |
| `CommandInheritFile` | `align_rt_command_inherit_file` | `HostState` |
| `CommandInheritNamespace` | `align_rt_command_inherit_namespace` | `HostState` |
| `CommandMaxCapture` | `align_rt_command_max_capture` | `HostState` |
| `CommandNew` | `align_rt_command_new` | `IndirectStorage` |
| `CommandNewSession` | `align_rt_command_new_session` | `HostState` |
| `CommandRun` | `align_rt_command_run` | `HostState` |
| `CommandRunBytes` | `align_rt_command_run_bytes` | `HostState` |
| `CommandStart` | `align_rt_command_start` | `HostState` |
| `CommandStartScope` | `align_rt_command_start_scope` | `HostState` |
| `CommandStderrTo` | `align_rt_command_stderr_to` | `HostState` |
| `CommandStdoutTo` | `align_rt_command_stdout_to` | `HostState` |
| `CommandTimeout` | `align_rt_command_timeout` | `HostState` |
| `CompressGzipCompress` | `align_rt_compress_gzip_compress` | `Foreign` |
| `CompressGzipDecompress` | `align_rt_compress_gzip_decompress` | `Foreign` |
| `CompressZstdCompress` | `align_rt_compress_zstd_compress` | `Foreign` |
| `CompressZstdDecompress` | `align_rt_compress_zstd_decompress` | `Foreign` |
| `CryptoAesGcmOpen` | `align_rt_crypto_aes_gcm_open` | `Foreign` |
| `CryptoAesGcmSeal` | `align_rt_crypto_aes_gcm_seal` | `Foreign` |
| `CryptoArgon2id` | `align_rt_crypto_argon2id` | `Foreign` |
| `CryptoChacha20Poly1305Open` | `align_rt_crypto_chacha20_poly1305_open` | `Foreign` |
| `CryptoChacha20Poly1305Seal` | `align_rt_crypto_chacha20_poly1305_seal` | `Foreign` |
| `CryptoCtEqual` | `align_rt_crypto_ct_equal` | `Foreign` |
| `CryptoDigestFinish` | `align_rt_crypto_digest_finish` | `Foreign` |
| `CryptoDigestFree` | `align_rt_crypto_digest_free` | `Foreign` |
| `CryptoDigestNew` | `align_rt_crypto_digest_new` | `Foreign` |
| `CryptoDigestUpdate` | `align_rt_crypto_digest_update` | `Foreign` |
| `CryptoHkdfSha256` | `align_rt_crypto_hkdf_sha256` | `Foreign` |
| `CryptoHmacSha256` | `align_rt_crypto_hmac_sha256` | `Foreign` |
| `CryptoKeyFree` | `align_rt_crypto_key_free` | `Foreign` |
| `CryptoPrivateKeyFromPem` | `align_rt_crypto_private_key_from_pem` | `Foreign` |
| `CryptoPublicKeyFromJwk` | `align_rt_crypto_public_key_from_jwk` | `Foreign` |
| `CryptoPublicKeyFromPem` | `align_rt_crypto_public_key_from_pem` | `Foreign` |
| `CryptoRandom` | `align_rt_crypto_random` | `HostState` |
| `CryptoSha1` | `align_rt_crypto_sha1` | `Foreign` |
| `CryptoSha256` | `align_rt_crypto_sha256` | `Foreign` |
| `CryptoSha512` | `align_rt_crypto_sha512` | `Foreign` |
| `CryptoSign` | `align_rt_crypto_sign` | `Foreign` |
| `CryptoVerify` | `align_rt_crypto_verify` | `Foreign` |
| `CsvDecodeSoaV1` | `align_rt_csv_decode_soa_v1` | `IndirectStorage` |
| `DictEncodeStr` | `align_rt_dict_encode_str` | `IndirectStorage` |
| `DictLookup` | `align_rt_dict_lookup` | `IndirectStorage` |
| `DivFail` | `align_rt_div_fail` | `FailNoReturn` |
| `DnsResolve` | `align_rt_dns_resolve` | `HostState` |
| `EnvGet` | `align_rt_env_get` | `HostState` |
| `EnvSet` | `align_rt_env_set` | `HostState` |
| `FormDecode` | `align_rt_form_decode` | `IndirectStorage` |
| `FormEncode` | `align_rt_form_encode` | `ArgRead` |
| `FrameInnerJoinI64V1` | `align_rt_frame_inner_join_i64_v1` | `IndirectStorage` |
| `FrameInnerJoinStrV1` | `align_rt_frame_inner_join_str_v1` | `IndirectStorage` |
| `Free` | `align_rt_free` | `FreeLocal` |
| `FreeResponseArray` | `align_rt_free_response_array` | `IndirectStorage` |
| `FreeStringArray` | `align_rt_free_string_array` | `IndirectStorage` |
| `FsCreateDir` | `align_rt_fs_create_dir` | `HostState` |
| `FsCreatePrivateTempDir` | `align_rt_fs_create_private_temp_dir` | `HostState` |
| `FsCursorFree` | `align_rt_fs_cursor_free` | `HostState` |
| `FsCursorNext` | `align_rt_fs_cursor_next` | `HostState` |
| `FsDirectoryAccess` | `align_rt_fs_directory_access` | `HostState` |
| `FsDirectoryAccessAt` | `align_rt_fs_directory_access_at` | `HostState` |
| `FsDirectoryCreateDir` | `align_rt_fs_directory_create_dir` | `HostState` |
| `FsDirectoryCreateNew` | `align_rt_fs_directory_create_new` | `HostState` |
| `FsDirectoryCreateSymlink` | `align_rt_fs_directory_create_symlink` | `HostState` |
| `FsDirectoryCursor` | `align_rt_fs_directory_cursor` | `HostState` |
| `FsDirectoryFree` | `align_rt_fs_directory_free` | `HostState` |
| `FsDirectoryMetadata` | `align_rt_fs_directory_metadata` | `HostState` |
| `FsDirectoryMetadataAt` | `align_rt_fs_directory_metadata_at` | `HostState` |
| `FsDirectoryMetadataFollow` | `align_rt_fs_directory_metadata_follow` | `HostState` |
| `FsDirectoryOpen` | `align_rt_fs_directory_open` | `HostState` |
| `FsDirectoryOpenDir` | `align_rt_fs_directory_open_dir` | `HostState` |
| `FsDirectoryOpenRead` | `align_rt_fs_directory_open_read` | `HostState` |
| `FsDirectoryOpenReadSingleLink` | `align_rt_fs_directory_open_read_single_link` | `HostState` |
| `FsDirectoryReadLink` | `align_rt_fs_directory_read_link` | `HostState` |
| `FsDirectoryRemoveDir` | `align_rt_fs_directory_remove_dir` | `HostState` |
| `FsDirectoryRemoveFile` | `align_rt_fs_directory_remove_file` | `HostState` |
| `FsDirectorySetMode` | `align_rt_fs_directory_set_mode` | `HostState` |
| `FsExists` | `align_rt_fs_exists` | `HostState` |
| `FsFileMetadata` | `align_rt_fs_file_metadata` | `HostState` |
| `FsFileSetMode` | `align_rt_fs_file_set_mode` | `HostState` |
| `FsIsDir` | `align_rt_fs_is_dir` | `HostState` |
| `FsMemoryFile` | `align_rt_fs_memory_file` | `HostState` |
| `FsMemoryFree` | `align_rt_fs_memory_free` | `HostState` |
| `FsMemorySeal` | `align_rt_fs_memory_seal` | `HostState` |
| `FsMemoryWrite` | `align_rt_fs_memory_write` | `HostState` |
| `FsReadBytesView` | `align_rt_fs_read_bytes_view` | `HostState` |
| `FsReadDir` | `align_rt_fs_read_dir` | `HostState` |
| `FsReadFile` | `align_rt_fs_read_file` | `HostState` |
| `FsReadFileView` | `align_rt_fs_read_file_view` | `HostState` |
| `FsReaderMetadata` | `align_rt_fs_reader_metadata` | `HostState` |
| `FsReaderSetMode` | `align_rt_fs_reader_set_mode` | `HostState` |
| `FsRemove` | `align_rt_fs_remove` | `HostState` |
| `FsRemoveEmptyDir` | `align_rt_fs_remove_empty_dir` | `HostState` |
| `FsRenameNoReplace` | `align_rt_fs_rename_no_replace` | `HostState` |
| `FsSealedFree` | `align_rt_fs_sealed_free` | `HostState` |
| `FsSealedLen` | `align_rt_fs_sealed_len` | `HostState` |
| `FsSealedReadAt` | `align_rt_fs_sealed_read_at` | `HostState` |
| `FsWriteFile` | `align_rt_fs_write_file` | `HostState` |
| `FsWriteFileBuilder` | `align_rt_fs_write_file_builder` | `HostState` |
| `FsWriterMetadata` | `align_rt_fs_writer_metadata` | `HostState` |
| `FsWriterSetMode` | `align_rt_fs_writer_set_mode` | `HostState` |
| `GatherI64` | `align_rt_gather_i64` | `IndirectStorage` |
| `GroupCountI64` | `align_rt_group_count_i64` | `IndirectStorage` |
| `GroupCountStr` | `align_rt_group_count_str` | `IndirectStorage` |
| `GroupCountStrCols` | `align_rt_group_count_str_cols` | `IndirectStorage` |
| `GroupMaxI64` | `align_rt_group_max_i64` | `IndirectStorage` |
| `GroupMaxStr` | `align_rt_group_max_str` | `IndirectStorage` |
| `GroupMaxStrCols` | `align_rt_group_max_str_cols` | `IndirectStorage` |
| `GroupMinI64` | `align_rt_group_min_i64` | `IndirectStorage` |
| `GroupMinStr` | `align_rt_group_min_str` | `IndirectStorage` |
| `GroupMinStrCols` | `align_rt_group_min_str_cols` | `IndirectStorage` |
| `GroupMultiStr` | `align_rt_group_multi_str` | `IndirectStorage` |
| `GroupSumI64` | `align_rt_group_sum_i64` | `IndirectStorage` |
| `GroupSumStr` | `align_rt_group_sum_str` | `IndirectStorage` |
| `GroupSumStrCols` | `align_rt_group_sum_str_cols` | `IndirectStorage` |
| `Hash128` | `align_rt_hash128` | `PureArgRead` |
| `Hash64` | `align_rt_hash64` | `PureArgRead` |
| `HexDecode` | `align_rt_hex_decode` | `IndirectStorage` |
| `HexEncode` | `align_rt_hex_encode` | `ArgRead` |
| `HtmlEscape` | `align_rt_html_escape` | `ArgRead` |
| `HttpAccept` | `align_rt_http_accept` | `HostState` |
| `HttpBody` | `align_rt_http_body` | `HostState` |
| `HttpClientFree` | `align_rt_http_client_free` | `HostState` |
| `HttpClientGet` | `align_rt_http_client_get` | `HostState` |
| `HttpClientMaxResponseBodyBytes` | `align_rt_http_client_max_response_body_bytes` | `HostState` |
| `HttpClientNew` | `align_rt_http_client_new` | `AllocNew` |
| `HttpClientPost` | `align_rt_http_client_post` | `HostState` |
| `HttpClientRequest` | `align_rt_http_client_request` | `HostState` |
| `HttpClientRequestStream` | `align_rt_http_client_request_stream` | `HostState` |
| `HttpClientTimeout` | `align_rt_http_client_timeout` | `HostState` |
| `HttpCtxBody` | `align_rt_http_ctx_body` | `HostState` |
| `HttpCtxFree` | `align_rt_http_ctx_free` | `HostState` |
| `HttpCtxHeader` | `align_rt_http_ctx_header` | `HostState` |
| `HttpCtxMethod` | `align_rt_http_ctx_method` | `HostState` |
| `HttpCtxPath` | `align_rt_http_ctx_path` | `HostState` |
| `HttpCtxUpgradeReady` | `align_rt_http_ctx_upgrade_ready` | `HostState` |
| `HttpGetMany` | `align_rt_http_get_many` | `HostState` |
| `HttpHeader` | `align_rt_http_header` | `HostState` |
| `HttpHeadersContainsToken` | `align_rt_http_headers_contains_token` | `HostState` |
| `HttpHeadersContainsTokenExact` | `align_rt_http_headers_contains_token_exact` | `HostState` |
| `HttpHeadersCount` | `align_rt_http_headers_count` | `HostState` |
| `HttpHeadersTokensValid` | `align_rt_http_headers_tokens_valid` | `HostState` |
| `HttpMaxResponseBodyBytes` | `align_rt_http_max_response_body_bytes` | `HostState` |
| `HttpParse` | `align_rt_http_parse` | `HostState` |
| `HttpRbBody` | `align_rt_http_rb_body` | `HostState` |
| `HttpRbHeader` | `align_rt_http_rb_header` | `HostState` |
| `HttpReadStreamFree` | `align_rt_http_read_stream_free` | `HostState` |
| `HttpReadStreamHeader` | `align_rt_http_read_stream_header` | `HostState` |
| `HttpReadStreamRead` | `align_rt_http_read_stream_read` | `HostState` |
| `HttpReadStreamSse` | `align_rt_http_read_stream_sse` | `HostState` |
| `HttpReadStreamStatus` | `align_rt_http_read_stream_status` | `HostState` |
| `HttpRequest` | `align_rt_http_request_new` | `AllocNew` |
| `HttpRequestFree` | `align_rt_http_request_free` | `HostState` |
| `HttpRespBody` | `align_rt_http_resp_body` | `HostState` |
| `HttpRespFree` | `align_rt_http_resp_free` | `HostState` |
| `HttpRespHeader` | `align_rt_http_resp_header` | `HostState` |
| `HttpRespStatus` | `align_rt_http_resp_status` | `HostState` |
| `HttpRespond` | `align_rt_http_respond` | `HostState` |
| `HttpRespondStream` | `align_rt_http_respond_stream` | `HostState` |
| `HttpRespondUpgrade` | `align_rt_http_respond_upgrade` | `HostState` |
| `HttpResponseFree` | `align_rt_http_response_free` | `HostState` |
| `HttpResponseNew` | `align_rt_http_response_new` | `AllocNew` |
| `HttpServe` | `align_rt_http_serve` | `Callback` |
| `HttpServeShared` | `align_rt_http_serve_shared` | `Callback` |
| `HttpServerFree` | `align_rt_http_server_free` | `HostState` |
| `HttpSseStreamLastEventId` | `align_rt_http_sse_stream_last_event_id` | `HostState` |
| `HttpSseStreamNext` | `align_rt_http_sse_stream_next` | `HostState` |
| `HttpSseStreamRetryMs` | `align_rt_http_sse_stream_retry_ms` | `HostState` |
| `HttpStreamFinish` | `align_rt_http_stream_finish` | `HostState` |
| `HttpStreamFree` | `align_rt_http_stream_free` | `HostState` |
| `HttpStreamReject` | `align_rt_http_stream_reject` | `HostState` |
| `HttpStreamSend` | `align_rt_http_stream_send` | `HostState` |
| `HttpStreamSendEvent` | `align_rt_http_stream_send_event` | `HostState` |
| `HttpTimeout` | `align_rt_http_timeout` | `HostState` |
| `HttpUpgradeDeadline` | `align_rt_http_upgrade_deadline` | `HostState` |
| `HttpUpgradeFree` | `align_rt_http_upgrade_free` | `HostState` |
| `HttpUpgradeReadExact` | `align_rt_http_upgrade_read_exact` | `HostState` |
| `HttpUpgradeShutdown` | `align_rt_http_upgrade_shutdown` | `HostState` |
| `HttpUpgradeWrite` | `align_rt_http_upgrade_write` | `HostState` |
| `IoCopy` | `align_rt_io_copy` | `HostState` |
| `IoFileCreate` | `align_rt_io_file_create` | `HostState` |
| `IoFileFree` | `align_rt_io_file_free` | `HostState` |
| `IoFileLen` | `align_rt_io_file_len` | `HostState` |
| `IoFileOpen` | `align_rt_io_file_open` | `HostState` |
| `IoFilePread` | `align_rt_io_file_pread` | `HostState` |
| `IoFilePwrite` | `align_rt_io_file_pwrite` | `HostState` |
| `IoReaderBuffered` | `align_rt_io_reader_buffered` | `HostState` |
| `IoReaderFree` | `align_rt_io_reader_free` | `HostState` |
| `IoReaderOpen` | `align_rt_io_reader_open` | `HostState` |
| `IoReaderOpenBeneath` | `align_rt_io_reader_open_beneath` | `HostState` |
| `IoReaderOpenBeneathSingleLink` | `align_rt_io_reader_open_beneath_single_link` | `HostState` |
| `IoReaderOpenRegular` | `align_rt_io_reader_open_regular` | `HostState` |
| `IoReaderRead` | `align_rt_io_reader_read` | `HostState` |
| `IoReaderReadLine` | `align_rt_io_reader_read_line` | `HostState` |
| `IoReaderStdin` | `align_rt_io_reader_stdin` | `HostState` |
| `IoWriterCreate` | `align_rt_io_writer_create` | `HostState` |
| `IoWriterCreateExclusive` | `align_rt_io_writer_create_exclusive` | `HostState` |
| `IoWriterCreateExclusiveBeneath` | `align_rt_io_writer_create_exclusive_beneath` | `HostState` |
| `IoWriterFlush` | `align_rt_io_writer_flush` | `HostState` |
| `IoWriterFree` | `align_rt_io_writer_free` | `HostState` |
| `IoWriterStd` | `align_rt_io_writer_std` | `HostState` |
| `IoWriterWrite` | `align_rt_io_writer_write` | `HostState` |
| `IoWriterWriteBuilder` | `align_rt_io_writer_write_builder` | `HostState` |
| `JsonBuilderFinish` | `align_rt_json_builder_finish` | `IndirectStorage` |
| `JsonBuilderInit` | `align_rt_json_builder_init` | `IndirectStorage` |
| `JsonBuilderWriteF32` | `align_rt_json_builder_write_f32` | `IndirectStorage` |
| `JsonBuilderWriteF64` | `align_rt_json_builder_write_f64` | `IndirectStorage` |
| `JsonDecode` | `align_rt_json_decode` | `IndirectStorage` |
| `JsonDecodeArray` | `align_rt_json_decode_array` | `IndirectStorage` |
| `JsonDecodeScalar` | `align_rt_json_decode_scalar` | `IndirectStorage` |
| `JsonDecodeSoa` | `align_rt_json_decode_soa` | `IndirectStorage` |
| `JsonDecodeStructArray` | `align_rt_json_decode_struct_array` | `IndirectStorage` |
| `JsonDecodeUnion` | `align_rt_json_decode_union` | `IndirectStorage` |
| `JsonDocAsBool` | `align_rt_json_doc_as_bool` | `IndirectStorage` |
| `JsonDocAsF64` | `align_rt_json_doc_as_f64` | `IndirectStorage` |
| `JsonDocAsI64` | `align_rt_json_doc_as_i64` | `IndirectStorage` |
| `JsonDocAsStr` | `align_rt_json_doc_as_str` | `IndirectStorage` |
| `JsonDocAt` | `align_rt_json_doc_at` | `IndirectStorage` |
| `JsonDocElems` | `align_rt_json_doc_elems` | `IndirectStorage` |
| `JsonDocGet` | `align_rt_json_doc_get` | `IndirectStorage` |
| `JsonDocKey` | `align_rt_json_doc_key` | `IndirectStorage` |
| `JsonDocKind` | `align_rt_json_doc_kind` | `IndirectStorage` |
| `JsonDocLen` | `align_rt_json_doc_len` | `IndirectStorage` |
| `JsonDocParse` | `align_rt_json_doc_parse` | `IndirectStorage` |
| `JsonEncodeObject` | `align_rt_json_encode_object` | `IndirectStorage` |
| `JsonEncodeScalarArray` | `align_rt_json_encode_scalar_array` | `IndirectStorage` |
| `JsonEncodeStructArray` | `align_rt_json_encode_struct_array` | `IndirectStorage` |
| `JsonEncodeUnion` | `align_rt_json_encode_union` | `IndirectStorage` |
| `JsonScanNext` | `align_rt_json_scan_next` | `IndirectStorage` |
| `LenMismatchFail` | `align_rt_len_mismatch_fail` | `FailNoReturn` |
| `LogEnabled` | `align_rt_log_enabled` | `HostState` |
| `LogFlush` | `align_rt_log_flush` | `HostState` |
| `LogFree` | `align_rt_log_free` | `HostState` |
| `LogLine` | `align_rt_log_line` | `HostState` |
| `LogLineBuilder` | `align_rt_log_line_builder` | `HostState` |
| `LogNew` | `align_rt_log_new` | `AllocNew` |
| `OsHost` | `align_rt_os_host` | `HostState` |
| `OsIdentity` | `align_rt_os_identity` | `HostState` |
| `ParMap` | `align_rt_par_map` | `Callback` |
| `ParMapFilter` | `align_rt_par_map_filter` | `Callback` |
| `ParMapReduce` | `align_rt_par_map_reduce` | `Callback` |
| `PathBase` | `align_rt_path_base` | `PureArgRead` |
| `PathDir` | `align_rt_path_dir` | `PureArgRead` |
| `PathExt` | `align_rt_path_ext` | `PureArgRead` |
| `PathJoin` | `align_rt_path_join` | `ArgRead` |
| `PathNormalize` | `align_rt_path_normalize` | `ArgRead` |
| `PercentDecode` | `align_rt_percent_decode` | `IndirectStorage` |
| `PercentEncode` | `align_rt_percent_encode` | `ArgRead` |
| `PercentEncodePath` | `align_rt_percent_encode_path` | `ArgRead` |
| `Print` | `align_rt_print_i64` | `HostState` |
| `PrintBool` | `align_rt_print_bool` | `HostState` |
| `PrintChar` | `align_rt_print_char` | `HostState` |
| `PrintF32` | `align_rt_print_f32` | `HostState` |
| `PrintF64` | `align_rt_print_f64` | `HostState` |
| `PrintStr` | `align_rt_print_str` | `HostState` |
| `ProcessAbort` | `align_rt_process_abort` | `FailNoReturn` |
| `ProcessCpuCount` | `align_rt_process_cpu_count` | `HostState` |
| `ProcessCurrentImage` | `align_rt_process_current_image` | `HostState` |
| `ProcessExec` | `align_rt_process_exec` | `HostState` |
| `ProcessExecutable` | `align_rt_process_executable` | `HostState` |
| `ProcessExit` | `align_rt_process_exit` | `ProcessExit` |
| `ProcessImageFree` | `align_rt_process_image_free` | `HostState` |
| `ProcessImageLen` | `align_rt_process_image_len` | `HostState` |
| `ProcessImageReadAt` | `align_rt_process_image_read_at` | `HostState` |
| `ProcessMemberFinished` | `align_rt_process_member_finished` | `HostState` |
| `ProcessMemberFree` | `align_rt_process_member_free` | `HostState` |
| `ProcessMemberKill` | `align_rt_process_member_kill` | `HostState` |
| `ProcessSignalClose` | `align_rt_process_signal_close` | `HostState` |
| `ProcessSignalFree` | `align_rt_process_signal_free` | `HostState` |
| `ProcessSignalNext` | `align_rt_process_signal_next` | `HostState` |
| `ProcessSignalNumber` | `align_rt_process_signal_number` | `HostState` |
| `ProcessSignals` | `align_rt_process_signals` | `HostState` |
| `ProcessSpawn` | `align_rt_process_spawn` | `HostState` |
| `ProcessTable` | `align_rt_process_table` | `HostState` |
| `ProcessUserNamespace` | `align_rt_process_user_namespace` | `HostState` |
| `ProcessUserNamespaceFree` | `align_rt_process_user_namespace_free` | `HostState` |
| `RangeFail` | `align_rt_range_fail` | `FailNoReturn` |
| `RegexCaptures` | `align_rt_regex_captures` | `IndirectStorage` |
| `RegexCapturesFree` | `align_rt_regex_captures_free` | `IndirectStorage` |
| `RegexCapturesGroup` | `align_rt_regex_captures_group` | `IndirectStorage` |
| `RegexCompile` | `align_rt_regex_compile` | `IndirectStorage` |
| `RegexFind` | `align_rt_regex_find` | `IndirectStorage` |
| `RegexFindAll` | `align_rt_regex_find_all` | `IndirectStorage` |
| `RegexFree` | `align_rt_regex_free` | `IndirectStorage` |
| `RegexGroupCount` | `align_rt_regex_group_count` | `IndirectStorage` |
| `RegexGroupIndex` | `align_rt_regex_group_index` | `IndirectStorage` |
| `RegexIsMatch` | `align_rt_regex_is_match` | `IndirectStorage` |
| `RegexReplace` | `align_rt_regex_replace` | `IndirectStorage` |
| `RegexSplit` | `align_rt_regex_split` | `IndirectStorage` |
| `RngNext` | `align_rt_rng_next` | `IndirectStorage` |
| `RngRange` | `align_rt_rng_range` | `IndirectStorage` |
| `RngSample` | `align_rt_rng_sample` | `IndirectStorage` |
| `RngSeedOs` | `align_rt_rng_seed_os` | `HostState` |
| `RngSeedWith` | `align_rt_rng_seed_with` | `IndirectStorage` |
| `RngShuffle` | `align_rt_rng_shuffle` | `IndirectStorage` |
| `RunBytesFree` | `align_rt_run_bytes_free` | `HostState` |
| `RunBytesStatus` | `align_rt_run_bytes_status` | `HostState` |
| `RunBytesStderr` | `align_rt_run_bytes_stderr` | `HostState` |
| `RunBytesStdout` | `align_rt_run_bytes_stdout` | `HostState` |
| `RunOutputFree` | `align_rt_run_output_free` | `HostState` |
| `RunOutputStatus` | `align_rt_run_output_status` | `HostState` |
| `RunOutputStderr` | `align_rt_run_output_stderr` | `HostState` |
| `RunOutputStdout` | `align_rt_run_output_stdout` | `HostState` |
| `ScopeChildren` | `align_rt_scope_children` | `HostState` |
| `ScopeFree` | `align_rt_scope_free` | `HostState` |
| `ScopeOwnerId` | `align_rt_scope_owner_id` | `HostState` |
| `ScopeReap` | `align_rt_scope_reap` | `HostState` |
| `ScopeRelease` | `align_rt_scope_release` | `HostState` |
| `StrClone` | `align_rt_str_clone` | `ArgRead` |
| `StrCmp` | `align_rt_str_cmp` | `PureArgRead` |
| `StrContains` | `align_rt_str_contains` | `DispatchCache` |
| `StrEndsWith` | `align_rt_str_ends_with` | `PureArgRead` |
| `StrEq` | `align_rt_str_eq` | `PureArgRead` |
| `StrEqIgnoreCase` | `align_rt_str_eq_ignore_case` | `PureArgRead` |
| `StrFind` | `align_rt_str_find` | `DispatchCache` |
| `StrFinderFind` | `align_rt_str_finder_find` | `DispatchCache` |
| `StrFinderFree` | `align_rt_str_finder_free` | `IndirectStorage` |
| `StrFinderNew` | `align_rt_str_finder_new` | `AllocNew` |
| `StrRfind` | `align_rt_str_rfind` | `DispatchCache` |
| `StrStartsWith` | `align_rt_str_starts_with` | `PureArgRead` |
| `StrTrim` | `align_rt_str_trim` | `PureArgRead` |
| `StrTrimEnd` | `align_rt_str_trim_end` | `PureArgRead` |
| `StrTrimStart` | `align_rt_str_trim_start` | `PureArgRead` |
| `TcpAccept` | `align_rt_tcp_accept` | `HostState` |
| `TcpConnFree` | `align_rt_tcp_conn_free` | `HostState` |
| `TcpConnReader` | `align_rt_tcp_conn_reader` | `HostState` |
| `TcpConnWriter` | `align_rt_tcp_conn_writer` | `HostState` |
| `TcpConnect` | `align_rt_tcp_connect` | `HostState` |
| `TcpListen` | `align_rt_tcp_listen` | `HostState` |
| `TcpListenerFree` | `align_rt_tcp_listener_free` | `HostState` |
| `TcpReadTimeout` | `align_rt_tcp_read_timeout` | `HostState` |
| `TcpWriteTimeout` | `align_rt_tcp_write_timeout` | `HostState` |
| `TemplateHtmlFree` | `align_rt_template_html_free_v1` | `IndirectStorage` |
| `TemplateHtmlNew` | `align_rt_template_html_new_v1` | `AllocNew` |
| `TemplateHtmlRaw` | `align_rt_template_html_raw_v1` | `IndirectStorage` |
| `TemplateHtmlToString` | `align_rt_template_html_into_string_v1` | `IndirectStorage` |
| `TemplateHtmlWrite` | `align_rt_template_html_write_v1` | `IndirectStorage` |
| `TgAlloc` | `align_rt_tg_alloc` | `IndirectStorage` |
| `TgBegin` | `align_rt_tg_begin` | `AllocNew` |
| `TgEnd` | `align_rt_tg_end` | `HostState` |
| `TgRegister` | `align_rt_tg_register` | `Callback` |
| `TgWait` | `align_rt_tg_wait` | `IndirectStorage` |
| `TimeFormat` | `align_rt_time_format` | `HostState` |
| `TimeInstant` | `align_rt_time_instant` | `HostState` |
| `TimeNow` | `align_rt_time_now` | `HostState` |
| `TimeParse` | `align_rt_time_parse` | `HostState` |
| `TimeSleep` | `align_rt_time_sleep` | `HostState` |
| `UdpBind` | `align_rt_udp_bind` | `HostState` |
| `UdpRecvFrom` | `align_rt_udp_recv_from` | `HostState` |
| `UdpSendTo` | `align_rt_udp_send_to` | `HostState` |
| `UdpSocketFree` | `align_rt_udp_socket_free` | `HostState` |
| `Utf8BoundaryFail` | `align_rt_utf8_boundary_fail` | `FailNoReturn` |
| `Utf8DecodeLossy` | `align_rt_utf8_decode_lossy` | `ArgRead` |
| `Utf8Valid` | `align_rt_utf8_valid` | `DispatchCache` |
| `XmlAttributeCount` | `align_rt_xml_attribute_count` | `IndirectStorage` |
| `XmlAttributeName` | `align_rt_xml_attribute_name` | `IndirectStorage` |
| `XmlAttributeValue` | `align_rt_xml_attribute_value` | `IndirectStorage` |
| `XmlFree` | `align_rt_xml_free` | `IndirectStorage` |
| `XmlName` | `align_rt_xml_name` | `IndirectStorage` |
| `XmlNext` | `align_rt_xml_next` | `IndirectStorage` |
| `XmlParse` | `align_rt_xml_parse` | `IndirectStorage` |
| `XmlText` | `align_rt_xml_text` | `IndirectStorage` |
| `Unkeyed::ReportError` | `align_rt_report_error` | `IndirectStorage` |
| `Unkeyed::ArgsBuild` | `align_rt_args_build` | `IndirectStorage` |
| `Unkeyed::ArenaReset` | `align_rt_arena_reset` | `HostState` |
| `Unkeyed::Realloc` | `align_rt_realloc` | `IndirectStorage` |
| `Unkeyed::HttpSerialize` | `align_rt_http_serialize` | `HostState` |
| `Unkeyed::F32ToBits` | `align_rt_f32_to_bits` | `PureScalar` |
| `Unkeyed::F32FromBits` | `align_rt_f32_from_bits` | `PureScalar` |
| `Unkeyed::F64ToBits` | `align_rt_f64_to_bits` | `PureScalar` |
| `Unkeyed::F64FromBits` | `align_rt_f64_from_bits` | `PureScalar` |
| `Unkeyed::F32TextLen` | `align_rt_f32_text_len` | `PureScalar` |
| `Unkeyed::F64TextLen` | `align_rt_f64_text_len` | `PureScalar` |
| `Unkeyed::F32TextWrite` | `align_rt_f32_text_write` | `IndirectStorage` |
| `Unkeyed::F64TextWrite` | `align_rt_f64_text_write` | `IndirectStorage` |
| `Unkeyed::TestLaunchRecvV1` | `align_rt_test_launch_recv_v1` | `IndirectStorage` |
| `Unkeyed::TestFdCloexecV1` | `align_rt_test_fd_cloexec_v1` | `IndirectStorage` |
| `Unkeyed::TestAckV1` | `align_rt_test_ack_v1` | `IndirectStorage` |
| `Unkeyed::TestReportV1` | `align_rt_test_report_v1` | `IndirectStorage` |
| `Unkeyed::TcpConnSetIoTimeout` | `align_rt_tcp_conn_set_io_timeout` | `HostState` |

The class-to-attribute derivation is closed:

| Class | LLVM memory effect | Function attributes |
|---|---|---|
| `PureScalar` | `memory(none)` | `nounwind nofree nosync willreturn` |
| `PureArgRead` | `memory(argmem: read)` | `nounwind nofree nosync willreturn` |
| `ArgRead` | `memory(argmem: read, inaccessiblemem: readwrite)` | `nounwind` |
| `AllocNew` | `memory(inaccessiblemem: readwrite)` or the `ArgRead` memory set | `nounwind nofree` |
| `FreeLocal` | `memory(argmem: readwrite, inaccessiblemem: readwrite)` | `nounwind` |
| `IndirectStorage` | withheld | `nounwind` |
| `DispatchCache` | withheld | `nounwind nofree nosync willreturn` |
| `HostState` | withheld | `nounwind` |
| `Callback` | withheld | `nounwind` |
| `Foreign` | withheld | `nounwind` |
| `FailNoReturn` | `memory(inaccessiblemem: readwrite)` | `nounwind cold noreturn` |
| `ProcessExit` | `memory(inaccessiblemem: readwrite)` | `nounwind noreturn` |

`returns_fresh` adds return `noalias`; `diverges` adds `noreturn`.
Every admitted `params` entry is `Read` and adds `readonly`; callback, foreign,
free, diverging, and exit rows forbid `params`. Every pointer ordinal absent
from `escapes` gets `captures(none)`. The shape inventory below fixes types only. A single shape
can span several effect classes, so the per-symbol records and declaration
golden, rather than an A-shape, own attributes.

## O0 parallel range-source safety contract (design accepted)

O0 changes no symbol, `RuntimeKey`, declaration shape, attribute, count, or
fingerprint row. It revises only the compiler-private safety interpretation of
A39 `align_rt_par_map_reduce` and A46 `align_rt_par_map`. For those two rows,
`in_buf` may name a compiler-certified immutable source whose physical extent
is not `count * in_stride`; the pointer and every byte the generated kernel can
derive from it remain valid until the synchronous call joins. `in_stride` is a
logical scheduling width; O0 supplies the positive 16-byte slice-header width,
and the runtime validates that `count * in_stride` fits `isize` as a work-span
bound without dereferencing the source. The generated kernel must bounds-check
its source-specific derivation, read only immutable source regions assigned by
the supplied logical range, write only its disjoint output range, and retain
neither source nor context.

Ordinary A39/A46 callers continue to satisfy the stronger physical
`count * in_stride` input-array form. A89 `align_rt_par_map_filter` is outside
O0 and retains that physical-span requirement. The implementation activates
this contract by updating both Rust `# Safety` clauses and the input-product
validation name, with direct ABI owners for opaque A39/A46 sources. Until that
implementation lands, the shipped Rust safety clauses remain authoritative.

## core.test child-control extension

The `core.test` design added four compiler-private unkeyed rows while leaving the then-current keyed
count at 314. The registry keys, LLVM declarations, Rust exports, collision reservation, runtime
ABI fingerprint, and test-mode selectors activated atomically. Only a generated
test harness may select them; they receive no language-callable `RuntimeKey` or compatible user
extern reuse.

The first-capability test-callgraph validator rejects every catalog-reachable
`ExprKind::ProcessCommand` before runtime selection or artifact allocation. It therefore adds no
containment descriptor, supervisor-status codec, or fifth child-control ABI row; ordinary
production `process.command` continues to select its shipped runtime entries unchanged.

| Unkeyed key | Exact symbol and LLVM declaration | Exact Rust ABI |
|---|---|---|
| `TestLaunchRecvV1` | A110: `i32 @align_rt_test_launch_recv_v1(i32, ptr)` | `extern "C" fn(i32, *mut u32) -> i32` |
| `TestFdCloexecV1` | A111: `i32 @align_rt_test_fd_cloexec_v1(i32)` | `extern "C" fn(i32) -> i32` |
| `TestAckV1` | A112: `i32 @align_rt_test_ack_v1(i32, i32)` | `extern "C" fn(i32, u32) -> i32` |
| `TestReportV1` | A113: `i32 @align_rt_test_report_v1(i32, i8, i8, i32, i32)` | `extern "C" fn(i32, u8, u8, i32, u32) -> i32` |

These declarations occupy A110 through A113. The implemented `std.log` design occupies A114 through
A117, `core.codec` occupies A118 through A120, `pkg.frame` occupies A121/A122 below, and `pkg.csv`
occupies A123. Named time formatting/parsing occupies A124/A125; A126 is the next unreserved design shape. All
four declarations use their per-symbol effect records above. `TestLaunchRecvV1` requires a non-null four-byte-aligned output, stores zero
before I/O, performs one blocking datagram receive with a fixed 17-byte capacity and EINTR retry,
requires the exact 16-byte `ALTESTL` v1 envelope with zero reserved bytes, and stores the decoded
little-endian ordinal only on success. The generated harness, not the runtime, validates that ordinal
against its linked catalog. `TestFdCloexecV1` adds `FD_CLOEXEC` to fd 3 without changing any other
descriptor flag.

`TestAckV1` accepts every `u32` ordinal and emits the exact 16-byte `ALTESTA` v1 envelope.
`TestReportV1` accepts only outcome 0 with tag 255/code zero, or outcome 1 with tag 0..=4 and code
zero unless tag 4; it emits the exact 20-byte `ALTEST\0` v1 envelope. Each encoder uses one
stack-resident fixed array and one datagram send, retries EINTR, and maps a short send to `EIO`.
Every row returns zero on success or a positive raw OS code, with `EINVAL` for an invalid ABI
argument and `EPROTO` for malformed launch bytes. They allocate nothing, retain no pointer or
descriptor, never close fd 3, and change no process-global state. The harness owns fd 3 until
successful `process.exec` closes it through close-on-exec or process termination closes it after the
harness returns. The independent driver codecs and the runtime codecs both pin the three semantic
goldens in `core-design/test.md`; malformed-input, EINTR, short-send, export-parity, whole/per-unit,
and reserved-child-exit owners land with the rows.

## `pkg.kv` TCP capability and prerequisites (implemented 2026-09-02)

The two independently useful prerequisites are implemented in the shipped shared timeout and
TCP-writer substrates without changing an ABI identity. The checked package row and its consumer
are also implemented. For every usable address and positive `timeout_ns`,
`align_rt_tcp_connect` records a monotonic start and positive `Duration` budget immediately before
the first `F_GETFL`, then checks `F_GETFL` and `F_SETFL(flags | O_NONBLOCK)` before `connect`.
Either failure records its fixed errno-mapped status, closes that candidate, and continues to the
next address without calling `connect`. After checked installation, exactly one immediate
`connect` is issued: zero succeeds, `EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` enter the wait, and every
other errno is mapped immediately. Either immediate terminal result wins even if the budget is
simultaneously exhausted. The in-progress path continues against the same start/budget pair; it
never forms an absolute `start + budget`, so `Instant::checked_add` overflow cannot turn a huge
positive timeout into an unbounded wait. Each iteration subtracts `start.elapsed()` from the
budget. A positive remainder is rounded up to the next millisecond and saturated at `i32::MAX` for
one `poll`, so the complete positive i64 range remains bounded through repeated chunks; an
exhausted remainder returns `AL_TIMEOUT` before another poll. It does not issue a final
zero-timeout `poll` call.
EINTR recomputes the remainder, any other poll error is mapped immediately, and a zero result
causes another monotonic recheck and re-poll only when time remains or returns `AL_TIMEOUT` without
another poll when the budget is exhausted. A positive readiness/error event wins over a
simultaneously exhausted budget and is resolved through `SO_ERROR`. Every immediate or polled
success then checks `F_GETFL` and
`F_SETFL(flags & !O_NONBLOCK)`. Restoration failure closes that candidate, records the failure, and
continues, so no connection is published before checked blocking-mode restoration. The existing
nonpositive raw-ABI blocking path stays unchanged: public HTTP callers reject negative values before
this ABI, and raw `tcp.connect` supplies zero.

A nonzero `getaddrinfo` result returns before address iteration. `EAI_NONAME`/`EAI_NODATA` maps to
`AL_INVALID`; every other symbolic EAI value maps to
`AL_CODE.saturating_add(eai.saturating_abs())`. The already-cleared connection output remains null,
no socket is attempted, no address-list owner escapes, and transient host/service storage drops
before return. Direct symbolic EAI owners pin both mapping categories, null output, zero socket
calls, and cleanup.

After successful resolution, resolver order is observable. Unsupported families, null addresses,
and zero address lengths are skipped without changing the last failure. The first successful usable
address wins. No usable address returns `AL_INVALID`; if every attempted candidate fails, the
runtime returns the last status from socket creation, nonblocking `F_GETFL`, nonblocking `F_SETFL`,
an immediate connect errno, poll error/timeout, `getsockopt(SO_ERROR)` failure, nonzero `SO_ERROR`,
blocking-restore `F_GETFL`, or blocking-restore `F_SETFL`. Direct mixed-address owners place a
skipped entry and a later success after every failure class; all-failure variants pin the last
attempted status and candidate close count. DNS and the sum across addresses have no end-to-end
deadline.

The same prerequisite makes the shared positive-nanosecond socket-timeout conversion exact for
`std.net`, `std.http`, and the checked package row:
`ceil(timeout_ns / 1000)` microseconds, split into normalized
`timeval { tv_sec, tv_usec: 0..999999 }`; exact microseconds remain exact and zero retains the
existing clear/no-timeout meaning.

The command-capture consumer of the same poll-millisecond conversion also replaces its absolute
`Instant::checked_add` deadline with a monotonic start and positive `Duration` budget. Its complete
positive-i64 range therefore cannot degrade to an unbounded run; every positive remainder rounds up
and saturates exactly as above, while its existing post-syscall timeout-wins checkpoint order stays
unchanged. Direct owners cover exact/next and maximum-positive ns, us, ms, chunk, and deadline
boundaries; `F_GETFL`/`F_SETFL` install and restore failures on immediate and polled success; early
zero-result recheck versus exhausted/no-call poll, `EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` versus other
immediate errno, EINTR remainder recomputation, readiness at the deadline, every resolver skip and
last-status failure class, candidate close/continuation, a blocking-mode probe on every published
connection, HTTP plain/TLS/pool rearm, and command pipe-drain/post-EOF reap.

The direct runtime owners are `socket_timeout_timeval_quantization`,
`tcp_connect_timeout_budget_quantization`, `tcp_connect_transition_and_address_matrix`,
`tcp_connect_resolver_status_and_order_matrix`, `tcp_connect_positive_timeout_publishes_blocking_fd`,
`http_timeout_quantization_plain_tls_pool_rearm`, and `command_timeout_budget_quantization`. No
registry declaration, source-reachable symbol, ABI count, or runtime export changed at this boundary.

The implemented second independently useful prerequisite repairs the already-shipped
`TcpConnWriter` -> `IoWriterWrite` path rather than adding a second write ABI. The private runtime
`Writer` gains a socket sink kind and macOS/BSD readiness bit; only `align_rt_tcp_conn_writer` sets
the kind. A nonempty socket-kind write keeps the existing complete partial-write loop, EINTR retry,
and `EAGAIN`/`EWOULDBLOCK` timeout mapping, but Linux calls `send(MSG_NOSIGNAL)` and macOS/BSD
performs checked `SO_NOSIGPIPE` before the first send on that writer shell, caching only success.
The option failure sends nothing through that call and remains retryable; positive-length zero
progress deterministically returns `AL_CODE` (`core.Error.Code(0)`). File and standard-stream
writers retain the existing generic `write(2)` path. Connection-derived writers remain unbuffered
and non-owning, so `IoWriterFree` performs no write and does not close the socket.
`SO_NOSIGPIPE` is monotone and idempotent per socket: overlapping shells may each attempt it, each
sends only after its own successful result, a failed shell remains retryable, no shell Drop clears
it, and connection close discards it. No process-global signal state changes. Direct owners cover a
failed install with no send followed by retry, both success/failure orders for overlapping shells,
shell Drop without option clear, connection close, and closed-peer subprocess routes through the
direct slice overload, builder overload, `std.log`, and `io.copy`, plus file/standard-stream and
partial/EINTR/timeout/zero-progress parity. In particular, the existing keyed
`IoWriterWriteBuilder` identity keeps A19's
`i32 @align_rt_io_writer_write_builder(ptr, ptr)` declaration and
`unsafe extern "C" fn(*mut Writer, *mut Builder) -> i32` Rust ABI. At that prerequisite boundary it
remained in the then-shipped 330/347/355 keyed/base/maximum counts and delegated its borrowed
builder bytes to the hardened `IoWriterWrite` row. The existing `TcpConnWriter`, `IoWriterWrite`,
`IoWriterWriteBuilder`, and
`IoWriterFree` identities, LLVM declarations, Rust exports, attributes, registry entries,
fingerprints, and counts remain unchanged.

The direct runtime owners are `tcp_writer_complete_send_transition_matrix`,
`tcp_writer_macos_nosigpipe_state_matrix`, `tcp_writer_generic_fd_parity_and_socket_lifecycle`, and
`tcp_writer_closed_peer_routes_do_not_sigpipe`.

The implemented `pkg.kv` capability adds exactly one package-internal,
source-reachable unkeyed row. It closes the checked-configuration failure domain that the existing
public timeout setters cannot: those setters return Unit and discard `setsockopt` failure. The new
row is a general TCP-connection operation rather than a RESP parser or package-specific helper:

| Unkeyed key | Exact symbol | ABI row and exact LLVM declaration | Exact Rust ABI |
|---|---|---|---|
| `TcpConnSetIoTimeout` | `align_rt_tcp_conn_set_io_timeout` | A04: `i32 @SYM(ptr, i64)` | `unsafe extern "C" fn(*mut TcpConn, i64) -> i32` |

`TcpConnSetIoTimeout` first rejects a null connection with `AL_INVALID`, then rejects
`timeout_ns` outside `1..=86400000000000` with `AL_INVALID`; either rejection occurs before reading
the fd or calling `setsockopt`, and a live connection remains usable after the range rejection.
Every non-null call has the unsafe precondition that the pointer names one live, unfreed `TcpConn`
held with exclusive logical access for the complete call, with no live reader/writer shell derived
from that connection and no other value retaining one at entry. A dangling or concurrently
aliased pointer or a live derived shell violates that precondition and is not detectable; no read,
write, other configuration, reader-or-writer construction, free, or Drop may overlap.

A target-connection retainer is classified by runtime provenance rather than numeric-fd equality:
its active recursive Drop graph reaches an initialized direct/buffered reader derived from that
connection, a derived writer, or a logger owning such a writer. The positive value graph uses
direct leaves, acyclic user-struct fields, nested active `Option`/`Result`, active user-sum paths
rooted in a logger/retaining struct/nested sum/tagged carrier, and elements of source-constructed
fixed arrays of retaining structs, including ordinary local, parameter, return, move, and by-value-
call placement. The fixed-array element is the retaining struct, so this composes existing struct-
field and fixed Move-struct-array rules rather than widening direct handle placement. Direct handle
collection/fixed-array/tuple/box elements and direct reader/writer
user-sum payloads reject formation. The admitted dynamic-array/slice shapes for retaining
structs/sums can name a type. A direct `DynStructArray` may additionally occupy a dynamic-array/
slice element, tuple element, or builtin `Option`/`Result` payload. Every admitted shape in this
paragraph except the tuple wrapper may occupy a user-struct field and then recurse through the
ordinary acyclic struct/tagged/sum carrier grammar. Every current
materializer, builder, and decode producer rejects a live handle-retaining
value. Over an in-place fixed array of retaining structs, the Move-element
slice producer forms the borrowed Copy `{ptr,len}` header admitted by
[plan 44](44-move-record-slice-plan.md): it has no Drop plan, retains nothing,
and adds no target leaf, so the viewed fixed array stays the counted owner.
Over retaining sums that array itself never forms, so the producer stays a
formation negative there. Inactive arms,
moved/null leaves, and carriers containing only other-
connection shells have target count zero; a compatible call requires zero even when a carrier can
otherwise reach multiple or mixed-provenance leaves.

For an admitted input the row uses the normalized ceil-to-microsecond `timeval` above, then installs
`SO_RCVTIMEO`. A failure returns its fixed errno-mapped status without attempting `SO_SNDTIMEO`;
otherwise it installs `SO_SNDTIMEO` and returns that status, or zero only after both succeed. Let
`R0` and `S0` be the receive/send option states at entry and `T` the requested state. Receive failure
leaves `{R0,S0}`, send failure leaves `{T,S0}`, and success leaves `{T,T}`. After either option
failure, a compatible caller must retire the still-owned connection, perform no read, write,
configuration, reader-or-writer construction, or retry on it, and invoke its ordinary free/Drop
path exactly once; the zero-derived-shell entry state leaves no shell cleanup to order against that
close. Success preserves usability and may construct derived shells afterward, but a later timeout
call may overwrite both options only after every such shell and retaining value has dropped. The row
itself allocates, retains, rolls back, closes, or consumes nothing. The null x
range product directly owns validation order and the no-fd/no-option side-effect rule.
Parameterized direct-runtime owners pre-arm both option states and pin live/exclusive plus
zero-derived-shell entry preconditions, the exact normalized `timeval`, option order, call counts,
returned status, the
`{R0,S0}`/`{T,S0}`/`{T,T}` post-state product, range-rejection retry versus option-failure retry
prohibition, zero overlapping/post-failure reader/writer-constructor calls, retirement, and later
free/Drop. One source-derived parameterized owner traverses the canonical recursive `DropPlan` and
matches every `DropPlan` node exhaustively, so a future cleanup-node variant requires
classification. Fixed arrays of retaining structs add no `DropPlan` node, so a separate owner pins
their `ty_is_move` and element-plan composition; source formation and no-live-producer negatives
own the admitted and excluded storage edges. Together they cross direct/buffered reader, direct
writer, logger-owned writer, struct/tagged/sum/fixed-struct-array placement,
active/inactive/moved-out state, target/other/mixed provenance, and zero/one/multiple target leaves.
They exclude every nonzero target count without invoking the unsafe row. For each positive carrier
class, the success cycle configures at zero, constructs and moves the leaf into that carrier, moves
it out where supported and drops it or recursively drops the smallest owning carrier, observes
zero, and reconfigures. The package
calls only on a fresh exclusively owned unpublished
connection with both entry options clear and before shell construction; its owner
closes after either option failure and proves that resolution is not reopened, no other address is
attempted, and no partially configured client is published.

The LLVM and Rust definitions use A04's default C calling convention and the row's `HostState`
effect record. The compiler recognizes the fixed physical symbol for exact ABI
compatibility, collision reservation, and source reachability. This adds no language builtin,
checked-HIR or MIR operation, call-spelling selector, or new ABI shape.

At package implementation, the one new key, symbol, definition, collision reservation, typed
registry row, runtime ABI fingerprint input, base/maximum export entry, and source-compatible extern
reuse activated atomically. It increased the unkeyed/base/maximum counts by one and the keyed count
by zero: the exact then-current keyed/base/maximum counts were 330/348/356. It reused an existing
shape, so A123 remained the next unused active shape until the later `pkg.csv` implementation below.
The current unkeyed count is eighteen, thirteen of which
are source-reachable. Both prerequisite hardenings and the new row are active with the `pkg.kv`
consumer.
Exact public consumption, poisoning, and owner matrix: `pkg-design/kv.md`.

## Implemented std.log extension (2026-08-31)

The logging implementation added four new shapes and two existing-shape keyed rows atomically:

| Runtime key | Exact symbol | ABI row and exact declaration | Exact Rust ABI |
|---|---|---|---|
| `LogNew` | `align_rt_log_new` | A114: `ptr @SYM(ptr, i32)` | `unsafe extern "C" fn(*mut Writer, i32) -> *mut Logger` |
| `LogEnabled` | `align_rt_log_enabled` | A115: `i32 @SYM(ptr, i32)` | `unsafe extern "C" fn(*mut Logger, i32) -> i32` |
| `LogLine` | `align_rt_log_line` | A116: `i32 @SYM(ptr, i32, ptr, i64)` | `unsafe extern "C" fn(*mut Logger, i32, *const u8, i64) -> i32` |
| `LogLineBuilder` | `align_rt_log_line_builder` | A117: `i32 @SYM(ptr, i32, ptr)` | `unsafe extern "C" fn(*mut Logger, i32, *mut Builder) -> i32` |
| `LogFlush` | `align_rt_log_flush` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut Logger) -> i32` |
| `LogFree` | `align_rt_log_free` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut Logger)` |

All six keys, symbols, declarations, and definitions are active. At that capability boundary they
changed the inventories from 314/331/339 to 320/337/345 keyed/base/maximum-optional-probe records
and extended the implemented shape range through A117. No curated attribute, optional feature, or
target-dependent row was added. The public/runtime ownership and validation contract remains in
`std-design/log.md`.

## Implemented `core.codec` extension (2026-09-01)

The codec implementation activates eight keyed rows atomically with their checked-HIR records and
owner tests. They extend the inventories from 320/337/345 to 328/345/353
keyed/base/maximum-optional-probe records and the implemented shape range through A120.

| Runtime key | Exact symbol | ABI row and exact LLVM declaration | Exact Rust ABI |
|---|---|---|---|
| `CodecOpenV1` | `align_rt_codec_open_v1` | A118: `i32 @SYM(ptr, i64)` | `unsafe extern "C" fn(*const u8, i64) -> i32` |
| `CodecEncoderNewV1` | `align_rt_codec_encoder_new_v1` | A119: `i32 @SYM(i64, ptr)` | `unsafe extern "C" fn(i64, *mut *mut CodecEncoder) -> i32` |
| `CodecEncoderPutI64V1` | `align_rt_codec_encoder_put_i64_v1` | A120: `i32 @SYM(ptr, ptr, i64, ptr, i64)` | `unsafe extern "C" fn(*mut CodecEncoder, *const u8, i64, *const i64, i64) -> i32` |
| `CodecEncoderPutF64V1` | `align_rt_codec_encoder_put_f64_v1` | A120 | `unsafe extern "C" fn(*mut CodecEncoder, *const u8, i64, *const f64, i64) -> i32` |
| `CodecEncoderPutBoolV1` | `align_rt_codec_encoder_put_bool_v1` | A120 | `unsafe extern "C" fn(*mut CodecEncoder, *const u8, i64, *const u8, i64) -> i32` |
| `CodecEncoderPutStrV1` | `align_rt_codec_encoder_put_str_v1` | A120 | `unsafe extern "C" fn(*mut CodecEncoder, *const u8, i64, *const AlignStr, i64) -> i32` |
| `CodecEncoderFinishV1` | `align_rt_codec_encoder_finish_v1` | existing A50: `ptr @SYM(ptr)` | `unsafe extern "C" fn(*mut CodecEncoder) -> *mut Buffer` |
| `CodecEncoderFreeV1` | `align_rt_codec_encoder_free_v1` | existing A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut CodecEncoder)` |

`CodecOpenV1` receives one compiler-formed valid byte view at any base alignment, performs the exact
owned/heap-allocation-free six-stage `ALNCOL01` validation in `core-design/codec.md`, rejects column
count 1025 before descriptor access, and returns zero or `AL_INVALID`; name uniqueness uses exactly
two `[u16; 1024]` stack arrays, ten stable merge passes, and at most 9,217 lexicographic comparisons.
It retains nothing and has no output slot. MIR forms the `{ input_ptr, input_len }` batch scalar from
the still-live input only after zero. Row and column counts lower to alignment-1 little-endian loads
from header bytes 16 and 24, with target-required swaps and u32-to-i64 zero extension for columns;
they are not hidden scalar fields. Remaining metadata and four typed projections reread descriptor
bytes with byte or alignment-1 little-endian operations and target-required swaps; element access
uses the same alignment-1 rule. `find` and all of these paths lower from the validated input without
another runtime row; no typed descriptor pointer or per-element opaque call is introduced.

`CodecEncoderNewV1` stores null to its nonnull aligned output before validating rows or allocating,
and publishes one allocator-provenanced shell only on zero. Each put requires a nonnull shell,
nonnegative signed lengths, null only for a zero-length name/value range, and the compiler-private
valid element/header range. It rejects a 1025th successful column before allocation or mutation and
completes name/row/kind/duplicate/final-size validation and all
fallible staging allocation before committing one column; `AL_INVALID` leaves the shell byte-for-
byte equivalent for future output. The str entry reads exact `{ ptr, i64 }` headers and copies no
cell before the complete prospective call is admitted. Successful columns are also kept in a
sorted name index, so duplicate admission binary-searches byte-exact names; fixed-index movement is
bounded by the 1024-column cap. OOM uses the hard-abort allocator path and
never returns a status.

Finish receives one live complete shell, allocates and fills the exact canonical final buffer,
consumes all staging, and returns the existing nonnull Buffer pointer; a null/private-invalid shell
hard-aborts because no source-valid call can form it. Free is null-safe and releases unfinished
staging exactly once. Every row is C calling convention and derives its attributes from the
per-symbol effect record above.

## Implemented `pkg.frame` extension (2026-09-01)

The implementation activates two new shapes after A120 in one atomic boundary. Both symbols, keys,
shapes, collision reservations, declarations, definitions, exports, fingerprint inputs, and totals
are active.

| Runtime key | Symbol | ABI row and exact LLVM declaration | Exact Rust ABI |
|---|---|---|---|
| `FrameInnerJoinI64V1` | `align_rt_frame_inner_join_i64_v1` | A121: `i32 @SYM(ptr, i64, ptr, i64, i64, ptr)` | `unsafe extern "C" fn(*const u8, i64, *const u8, i64, i64, *mut AlignStr) -> i32` |
| `FrameInnerJoinStrV1` | `align_rt_frame_inner_join_str_v1` | A122: `i32 @SYM(ptr, ptr, i64, ptr, ptr, i64, i64, ptr)` | `unsafe extern "C" fn(*const u8, *const u8, i64, *const u8, *const u8, i64, i64, *mut AlignStr) -> i32` |

A121 receives two unaligned little-endian i64 value ranges and their element counts. A122 receives
two validated string-column offset/data pairs and their row counts; each offset base addresses an
alignment-1 `(rows + 1) * 4` little-endian i32 range, and each data pointer addresses the bytes
through the validated final offset. The last pointer in both rows is a nonnull, correctly aligned,
writable `AlignStr` header used privately as `{ ptr, element_count }` for 16-byte `RowPair`
elements. It is not an Align source `str`. Empty output is `{ null, 0 }`; nonempty output comes from
the ordinary runtime allocator with the target ABI alignment of `{ i64, i64 }` and must be freed by
the existing dynamic-array owner.

Both rows are C calling convention and `nounwind`. They carry no other curated memory, return, or
parameter attribute. Runtime first
requires and zeroes the output header, then validates negative limit before either input, left view
before right view, right-table load-factor/capacity/byte arithmetic before allocation, and every
output bound before output allocation. For positive right length `R`, the exact logical index uses
`Q = R + ceil(R / 3)`, the smallest power-of-two `C >= max(8,Q)`, two `C`-entry i64 head/tail
tables, and one `R`-entry i64 next-link table. `Q`, `C`, and `16*C + 8*R` must fit i64 and the target
allocation-size domain; `R == 0` needs no index. A representability failure returns private `-2`
even when the semantic join result would be empty. Zero returns success; private `-1` means only
`JoinError.InvalidLimit`; private `-2` means only `JoinError.LimitExceeded`. A positive
`AL_INVALID` identifies a malformed compiler-
private ABI and hard-aborts in compiler-produced lowering. Every return frees transient scratch and
an error publishes no output.

The right input is always indexed in ascending ordinal order. The runtime counts stable matches
before one exact output allocation, probes again to fill left-major/right-ascending pairs, confirms
equality after every hash match, and retains no input pointer. Activation moved the
keyed/base/maximum-optional-probe totals from 328/345/353 to 330/347/355 and made A123 the next
unreserved shape at that time. The later `pkg.csv` implementation occupies A123, so A124 is now the
next unreserved design shape. Exact public semantics and owner matrix:
`pkg-design/frame.md`.

## `pkg.csv` extension (implemented 2026-09-03)

The implementation adds one keyed shape, `RuntimeKey`, declaration, definition, export, collision
identity, fingerprint input, and count atomically:

| Runtime key | Symbol | ABI row and exact LLVM declaration | Exact Rust ABI |
|---|---|---|---|
| `CsvDecodeSoaV1` | `align_rt_csv_decode_soa_v1` | A123: `i32 @SYM(ptr, i64, ptr, i64, ptr, i32, i32, i64, ptr)` | `unsafe extern "C" fn(*const u8, i64, *const CsvField, i64, *mut Arena, i32, i32, i64, *mut AlignStr) -> i32` |

Arguments are input pointer/byte length, descriptor pointer/count, destination arena, checked
header tag, checked line-ending tag, inclusive row bound, and a nonnull writable output header.
The arena pointer has Rust `align_of::<Arena>()`; the descriptor and output pointers have their
target-native `repr(C)` alignments.
The row is C calling convention and `nounwind`, with no other curated function, return, or parameter
attribute. The runtime zeroes output before public validation. Status 0 is success, 1 is
only `pkg.csv.Error.Invalid`, 2 is only `LimitExceeded`, and `-1` is only malformed private ABI.
The runtime returns no other status; the canonical wrapper aborts on `-1` or any forged other i32.
OOM and an impossible second-pass mismatch also
abort. Recoverable error publishes no output and does not advance the arena.

Input length zero accepts either input pointer and performs no dereference or slice formation;
positive length requires a nonnull readable range of exactly that many bytes. The arena is nonnull,
aligned to `align_of::<Arena>()`, live, and exclusive. The output is nonnull, aligned to
`align_of::<AlignStr>()`, writable, and exclusive. The descriptor count is positive, converts
exactly to `usize`, and has no CSV-specific upper bound; its nonnull pointer is aligned to
`align_of::<CsvField>()` and denotes that many immutable records. Every
positive name length has a nonnull readable byte range. Counts, lengths, byte products, and address
additions fit the declared integer and target pointer-offset domains before any Rust reference,
slice, or typed load is formed.

Input bytes, descriptor records, and descriptor-name bytes remain immutable for the complete call.
The live arena control object and output header are disjoint from each other and those immutable
ranges. Input may occupy a prior live allocation of the same arena; the next arena allocation
cannot overlap it. The runtime returns `-1` for a mechanically detectable
negative/null/misaligned/overflowing representation before typed access. Once those guards pass,
exact-compatible unsafe source callers establish dereferenceability, lifetime, provenance,
immutability, and overlap; checked compiler-produced calls derive them before lowering. The guards
cannot prove an arbitrary nonnull address's backing range and do not make an otherwise invalid
unsafe call defined.

`CsvField` is target-native `#[repr(C)]` / non-packed LLVM
`{ name_ptr: ptr, name_len: i64, name_hash: u64, tag: i32, reserved: i32 }`. The global uses at least the exact
`repr(C)` alignment, size, and five field offsets. `name_hash` is the canonical
`align_hash::wyhash(name_bytes, WY_SEED)` with `WY_SEED = 0`; codegen and runtime call the same
shared implementation. `align_hash` owns the algorithm/seed convention; A123 owns descriptor
validation order and the authenticated stored hash. Runtime name validation recomputes and authenticates it before later header
projection reads the stored hash without rehashing the descriptor. Names are nonempty static source identifiers in
declaration order: first byte ASCII `_`/letter, remaining bytes ASCII `_`/letter/digit, and not one
of exact reserved tokens `fn`, `return`, `mut`, `pub`, `module`, `import`, `if`, `else`, `true`,
`false`, `arena`, `task_group`, `match`, `loop`, `break`, `template`, `unsafe`, `extern`, or `as`.
NUL, non-ASCII, invalid UTF-8, punctuation, and every other spelling return private `-1` during the
descriptor phase. `reserved` is zero. `tag` is `(signed << 16) | (kind << 8) | width`:
integer kind 0 with width 1/2/4/8 and sign bit 16; bool kind 1 width 1; float kind 2 width 4/8; str
kind 3 width 16; char kind 4 width 4. All other bits are zero. Before input access or arena effects,
the runtime validates output and arena, every count/range/pointer product, every descriptor field,
and each name's grammar while hashing its bytes once. It does not compare descriptor names with one
another. Record validation and checked descriptor emission prove declaration-order uniqueness for
compiler-produced calls; an exact-compatible unsafe caller must provide the same pairwise-unique
precondition. A violation is outside the unsafe contract, is not authenticated by the runtime, and
is not promised `-1`.

Validation order is output, arena, output zeroing; header tag, line-ending tag, row bound;
positive representable descriptor count, table size/alignment/non-null guards, then each declaration-
order record's positive name length, name range/source-identifier bytes and one hash, matching
`name_hash`, tag, and zero reserved field; input representation; complete UTF-8 validation;
CSV/header/data/layout; and finally a
nonempty allocation/fill. Thus negative `max_rows` returns 1 before malformed descriptor inspection
whenever output and arena are valid. Invalid UTF-8 returns `-1` before BOM/CSV parsing and arena
allocation. Malformed private input returns `-1`; descriptor and input are never inspected before
their preceding phase.

For `F` descriptor records containing `B` total name bytes, the pre-input phase visits exactly `F`
records and hashes exactly `B` bytes. Absent-header decode performs no later name lookup. Present-
header decode has `H <= 1024` physical names and performs one bounded fixed-table lookup per
descriptor; confirmed-collision candidate comparisons are at most `F * H`, with at most `B * H`
descriptor-name bytes compared. Test counters pin these bounds for wide and common-prefix schemas,
so the uncapped descriptor domain has no quadratic descriptor-to-descriptor path.

Activation is one atomic package/HIR/MIR/runtime capability: it adds the key, exact declaration
golden, definition/export, collision reservation, capability collection, fingerprints, and all
owners together. It changes current keyed/base/either-four-row-probe/maximum totals from
330/348/352/356 to 331/349/353/357 and makes A124 the next unreserved active shape. A source extern
cannot activate the row or select checked `CsvDecode`; exact compatible source-
extern reuse follows the ordinary registry rule. No partial producer may land. Exact semantics,
validation order, allocation contract, and closure matrix: `pkg-design/csv.md`.

## `pkg.ws` extension (implemented 2026-09-04)

The implemented `pkg.ws` capability activates eleven keyed identities, all on existing ABI shapes:

| Runtime key | Exact symbol | Existing ABI row and exact declaration | Exact Rust ABI |
|---|---|---|---|
| `HttpRespondUpgrade` | `align_rt_http_respond_upgrade` | A24: `i32 @SYM(ptr, ptr, ptr)` | `unsafe extern "C" fn(*mut HttpRequestCtx, *mut ResponseBuilder, *mut *mut HttpUpgrade) -> i32` |
| `HttpUpgradeReadExact` | `align_rt_http_upgrade_read_exact` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut HttpUpgrade, *mut Buffer, i64) -> i32` |
| `HttpUpgradeWrite` | `align_rt_http_upgrade_write` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut HttpUpgrade, *const u8, i64) -> i32` |
| `HttpUpgradeDeadline` | `align_rt_http_upgrade_deadline` | A04: `i32 @SYM(ptr, i64)` | `unsafe extern "C" fn(*mut HttpUpgrade, i64) -> i32` |
| `HttpUpgradeShutdown` | `align_rt_http_upgrade_shutdown` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut HttpUpgrade) -> i32` |
| `HttpUpgradeFree` | `align_rt_http_upgrade_free` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut HttpUpgrade)` |
| `HttpHeadersCount` | `align_rt_http_headers_count` | A37: `i64 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64) -> i64` |
| `HttpHeadersTokensValid` | `align_rt_http_headers_tokens_valid` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64) -> i32` |
| `HttpHeadersContainsToken` | `align_rt_http_headers_contains_token` | A120: `i32 @SYM(ptr, ptr, i64, ptr, i64)` | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64, *const u8, i64) -> i32` |
| `HttpHeadersContainsTokenExact` | `align_rt_http_headers_contains_token_exact` | A120: `i32 @SYM(ptr, ptr, i64, ptr, i64)` | `unsafe extern "C" fn(*mut HttpRequestCtx, *const u8, i64, *const u8, i64) -> i32` |
| `HttpCtxUpgradeReady` | `align_rt_http_ctx_upgrade_ready` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut HttpRequestCtx) -> i32` |

These rows activate together in `RuntimeKey`, declarations, definitions, exports, collision
identity, fingerprints, and count assertions. At that capability boundary the inventory became 342
keyed records, 360 base records, 367 with the seven-row `alloc-count` probe, 364 with the four-row
`par-map-probe`, and 371 with both; A124 remained the next
unreserved active shape.

The same capability hardens the existing HTTP accepted-socket setup without adding a key or shape.
On macOS/iOS, after the existing best-effort `TCP_NODELAY` and `SO_KEEPALIVE`, `SO_NOSIGPIPE`
installation is checked before reading a request or publishing a request context. Failure captures
that errno, closes the accepted fd once without retry while ignoring the close result, and makes
`srv.accept` return the original mapped OS error; no context or writable connection escapes. Linux
retains `send(MSG_NOSIGNAL)`. A socket-option failpoint owns the failure ordering and close count.

`HttpRespondUpgrade` first validates and zeroes its writable aligned output. Invalid output returns
`AL_INVALID` without inspecting or consuming inputs. It then requires and takes a nonnull aligned
builder before ctx validation, so every later result consumes it. After the exact semantic checks,
checked addition computes `H = len("HTTP/1.1 101 Switching Protocols\r\n") +
sum(len(name) + len(": ") + len(value) + len("\r\n")) + len("\r\n")`; an unrepresentable total
returns `AL_INVALID` with ctx unspent. The runtime allocates and fills exactly one `H`-byte head
without growth or a second serialized copy, then allocates the fixed handle shell, all before fd
transfer or wire I/O. With entry builder heap `B` and `U = size_of::<HttpUpgrade>()`, the exact
producer-requested operation high-water excluding allocator-private metadata is `B + H + U`;
compile-time layout assertions and an allocation probe own it. OOM hard-aborts
before transfer. It publishes the handle only after the validated HTTP/1.1, residual-free 101 head
with complete RFC header syntax writes completely and the fd moves from the request context. The
readiness getter returns true only for HTTP/1.1 with no parser residual. Read validates
arguments/state before clearing a live buffer and publishes length only after exact success;
write borrows bytes and is SIGPIPE-safe write-all. Deadline retains one monotonic start-plus-budget
in the opaque handle; every later read/write recomputes the same remaining budget before each
syscall, rounds positive waits up, rechecks an early native timeout wakeup, and makes no call after
exhaustion. Shutdown invokes native `SHUT_RDWR` once, treats ENOTCONN as success, then performs one
no-retry cleanup close; other shutdown errors are returned after close. Free performs close only.
Each operation closes at most once. Caller-invalid precedes handle state; spent read/write/deadline
return `AL_INVALID` without mutation/clock/I/O, shutdown is idempotent, and poisoned operations replay
the stored status without I/O.

The feature-gated requested-live probe resets one explicit measurement window and tracks only the
allocation families attributed to it. `buffer` charges its fixed 64-byte shell budget plus reserved
payload, `array_builder` charges its 64-byte shell budget and C-owned growth (including old+new
overlap at realloc), and a frozen builder transfers that shell charge to its payload through Text
conversion. Ordinary string clone allocation then records the simultaneous staging/result peak.
The owner exercises Binary and Text at a bounded concrete size and separately pins the exact
64-bit maximum equation `128 + 32768 + 2 * 536870912 = 1073774720`; production builds compile every
probe hook and export out.

Header query pointers borrow the live request context for the call and retain nothing. Count and
token validation check context, then name; both membership operations check context, complete name
view/token, then complete searched-token view/token. Header names are case-insensitive;
`HttpHeadersContainsToken` compares members ASCII-case-insensitively and
`HttpHeadersContainsTokenExact` compares them byte-exactly. A null or misaligned context hard-aborts before Rust reference
formation; negative or address-space-unrepresentable length and null positive-length range reject
before slice formation; invalid token bytes hard-abort after safe view formation but before table
scanning. `HttpCtxUpgradeReady` applies the same context rule.
Dangling nonnull pointers remain outside the detectable ABI contract. No malformed query maps to
an ordinary zero/false result. All other pointer/length/count/capacity/address products are rejected
before Rust reference or slice formation as specified by their status-returning rows.

All eleven exports use the Rust C calling convention and must not unwind across it. Their generated
LLVM declarations derive attributes from their per-symbol effect records; the reused
A03/A04/A20/A24/A37/A62/A120 shapes remain type-only. Exact public semantics, status mapping, validation
order, ownership, allocation, cache identity, and closure matrix: `pkg-design/ws.md`.

## `pkg.template` extension (implemented 2026-09-04)

The `pkg.template` capability activates five keyed identities on existing ABI shapes:

| Runtime key | Exact symbol | Existing ABI row and exact declaration | Exact Rust ABI |
|---|---|---|---|
| `TemplateHtmlNew` | `align_rt_template_html_new_v1` | A47: `ptr @SYM()` | `extern "C" fn() -> *mut TemplateHtmlBuilder` |
| `TemplateHtmlWrite` | `align_rt_template_html_write_v1` | A73: `void @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut TemplateHtmlBuilder, *const u8, i64)` |
| `TemplateHtmlRaw` | `align_rt_template_html_raw_v1` | A73: `void @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*mut TemplateHtmlBuilder, *const u8, i64)` |
| `TemplateHtmlToString` | `align_rt_template_html_into_string_v1` | A83: `{ ptr, i64 } @SYM(ptr)` | `unsafe extern "C" fn(*mut TemplateHtmlBuilder) -> AlignStr` |
| `TemplateHtmlFree` | `align_rt_template_html_free_v1` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut TemplateHtmlBuilder)` |

All five rows activate together in `RuntimeKey`, declaration and export inventories, compatible-
extern collision identity, fingerprints, and count assertions. The inventory is therefore 347
keyed, 365 base, 372 with `alloc-count`, 369 with `par-map-probe`, and 376 with both optional probe
sets. No probe category or ABI shape is added, and A124 remains the next unreserved shape.

The exact 32-byte shell, validation order, escaping table reuse, zero-copy finish, ownership,
allocation, cache identity, and closure matrix are owned by `pkg-design/template.md`.

## `std.xml` extension (implemented 2026-09-05)

The XML capability adds eight keyed identities, all on existing ABI shapes:

| Runtime key | Exact symbol | Existing ABI row and exact declaration | Exact Rust ABI |
|---|---|---|---|
| `XmlParse` | `align_rt_xml_parse` | A08: `i32 @SYM(ptr, i64, ptr)` | `unsafe extern "C" fn(*mut u8, i64, *mut *mut XmlReader) -> i32` |
| `XmlNext` | `align_rt_xml_next` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut XmlReader) -> i32` |
| `XmlName` | `align_rt_xml_name` | A19: `i32 @SYM(ptr, ptr)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr) -> i32` |
| `XmlAttributeCount` | `align_rt_xml_attribute_count` | A29: `i64 @SYM(ptr)` | `unsafe extern "C" fn(*const XmlReader) -> i64` |
| `XmlAttributeName` | `align_rt_xml_attribute_name` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr, i64) -> i32` |
| `XmlAttributeValue` | `align_rt_xml_attribute_value` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr, i64) -> i32` |
| `XmlText` | `align_rt_xml_text` | A19: `i32 @SYM(ptr, ptr)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr) -> i32` |
| `XmlFree` | `align_rt_xml_free` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut XmlReader)` |

The Rust definitions use C calling convention and may not unwind across it. Generated declarations
retain the reused A03/A08/A19/A20/A29/A62 shapes' empty curated function, return, memory, and
parameter attribute sets. These are `unsafe` boundaries. Before dereference, the runtime may inspect
pointer integers and lengths to reject null where forbidden, misalignment, negative length,
noncanonical empty, address-range overflow, and supplied-range alias. A caller may rely on those
exact shape rejections. Every nonnull pointer that passes its shape checks and could then be accessed
must have provenance, lifetime, dereferenceability, accessibility, and the declared shared or
exclusive access for its exact range against access not represented by the call. Parse input is
canonical `{null,0}` with no allocation or one positive-length allocator-compatible owned range.
Getters/count share a shape-valid live shell allocation, while next and nonnull free hold it
exclusively. When an operation follows the shell's stored input pointer, its live allocation and exact
readable range are also caller preconditions unless the shell was published unchanged by this
runtime. Violating a post-shape-check pointer/access precondition is outside the ABI contract and is
not promised a safe abort.

At parse, the runtime checks representable raw ranges and every detectable input/output alias before
mutation, Rust reference creation, or slice creation. A mechanically rejected parse returns positive
`AL_INVALID`, leaves output untouched, and accepts no input
ownership. In particular, nonnull zero-length input is rejected and never freed. After that
preflight it stores null and accepts no allocation for canonical empty or the positive-length owned
input: status zero publishes the sole shell, while `-1` releases accepted input responsibility (a
no-op for canonical empty) and means public `Error.Invalid`. An
output-bearing getter leaves output untouched when any complete mechanical preflight step fails:
address shape, output/fixed-shell disjointness, shell fields, stored shell/input internal disjointness,
or output/input disjointness. It zeros `{ptr,i64}` only after that preflight; a later state/index
failure leaves canonical zero for borrowed and owned results alike. Success fills a borrowed name
view or publishes one completed
owned value/text allocation. `XmlNext` returns only `0=None`, `1=Start`, `2=End`, or `3=Text`.
Count is only `0..=256`; every impossible getter result aborts. Free is null-safe; a nonnull argument
must be a genuine exclusively held shell, and detectable malformed fields abort before following an
invalid stored input pointer. The runtime does not authenticate an arbitrary dangling address.

No new shape is used: A124 remains the next unreserved shape. All eight keys, symbols,
declarations, definitions, exports, collision identities, fingerprint rows, count assertions, and
their type/Drop consumers activate atomically. The exact active totals are 356 keyed, 374 base, 381
with `alloc-count`, 378 with `par-map-probe`, 374 with `task-probe`, and 385 with every probe. The
grammar, status mapping, ownership, validation order, allocation contract, and closure matrix are
authoritative in `std-design/xml.md`.

## HTTP client raw receive-stream substrate (implemented)

The first HTTP receive-stream capability adds exactly six keyed records and no new ABI shape:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `BufferCapacity` | `align_rt_buffer_capacity` | A29: `i64 @SYM(ptr)` |
| `HttpClientRequestStream` | `align_rt_http_client_request_stream` | A24: `i32 @SYM(ptr, ptr, ptr)` |
| `HttpReadStreamFree` | `align_rt_http_read_stream_free` | A62: `void @SYM(ptr)` |
| `HttpReadStreamHeader` | `align_rt_http_read_stream_header` | A22: `i32 @SYM(ptr, ptr, i64, ptr)` |
| `HttpReadStreamRead` | `align_rt_http_read_stream_read` | A24: `i32 @SYM(ptr, ptr, ptr)` |
| `HttpReadStreamStatus` | `align_rt_http_read_stream_status` | A29: `i64 @SYM(ptr)` |

`HttpClientRequestStream` consumes the request and publishes a dependent stream handle only on
success. Header lookup publishes a borrowed view into the retained head block. Read writes an exact
byte count to its out slot and mutates only the caller buffer; a source-visible zero-capacity buffer
is rejected before this ABI is called. `HttpReadStreamFree` is null-safe and closes rather than
draining an incomplete response.

## HTTP client SSE receive extension (implemented)

The second HTTP receive-stream capability adds exactly four keyed records and no new ABI shape:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `HttpReadStreamSse` | `align_rt_http_read_stream_sse` | A50: `ptr @SYM(ptr)` |
| `HttpSseStreamLastEventId` | `align_rt_http_sse_stream_last_event_id` | A83: `{ ptr, i64 } @SYM(ptr)` |
| `HttpSseStreamNext` | `align_rt_http_sse_stream_next` | A24: `i32 @SYM(ptr, ptr, ptr)` |
| `HttpSseStreamRetryMs` | `align_rt_http_sse_stream_retry_ms` | A29: `i64 @SYM(ptr)` |

The transition preserves the one runtime pointer while MIR nulls the consumed raw source. `Next`
writes the fixed 64-byte native envelope only through its output pointer and returns the existing
signed HTTP status discriminator. The ID getter returns a stream-bound view, retry uses `-1` as the
`None` sentinel, and the raw/SSE types share the one null-safe free row.

## Asymmetric signature delta (implemented 2026-08-30)

The post-pkg.db implementation adds six keyed records atomically. They are included in the exact
current counts and the shipped A00–A109 table below.

| Runtime key | Exact symbol | ABI row and exact declaration |
|---|---|---|
| `CryptoPrivateKeyFromPem` | `align_rt_crypto_private_key_from_pem` | A106: `i32 @SYM(i32, ptr, i64, ptr)` |
| `CryptoPublicKeyFromPem` | `align_rt_crypto_public_key_from_pem` | A106: `i32 @SYM(i32, ptr, i64, ptr)` |
| `CryptoPublicKeyFromJwk` | `align_rt_crypto_public_key_from_jwk` | A107: `i32 @SYM(i32, ptr, i64, ptr, i64, ptr)` |
| `CryptoSign` | `align_rt_crypto_sign` | A108: `i32 @SYM(i32, ptr, ptr, i64, ptr)` |
| `CryptoVerify` | `align_rt_crypto_verify` | A109: `i32 @SYM(i32, ptr, ptr, i64, ptr, i64, ptr)` |
| `CryptoKeyFree` | `align_rt_crypto_key_free` | A62: `void @SYM(ptr)` |

`algorithm` is the closed `i32` ABI form of `0=RS256`, `1=ES256`, `2=Ed25519`; the runtime validates
it before narrowing. The JWK row passes Ed25519's absent second component as null/zero. Constructor
and sign output slots must be non-null/aligned and are null-initialized; verify's final `i32` slot must
be non-null/aligned and is zero-initialized. An invalid slot returns `AL_INVALID` without writing.
Every input length is nonnegative and `usize`-representable before slice formation; zero length may
carry null and uses an internal empty sentinel, while positive length requires non-null. Ed25519's
absent second JWK pair is exactly null/zero. Every handle repeats the closed key-kind byte and each
operation checks algorithm, public/private class, and kind before EVP. Its private shell fields own
one ordinary library context, its explicitly loaded built-in default provider, and the PKEY.
Private PEM is canonical PKCS#8 v1 `PrivateKeyInfo` version zero decoded only through
`d2i_PKCS8_PRIV_KEY_INFO` and `EVP_PKCS82PKEY_ex`; one exact `SensitiveDer` and every private
re-encoding scratch cleanse before free on all paths. Each fallible OpenSSL call clears and drains
its thread-local error queue. Provider checks and verify exhaust a disjoint native-return ×
`Empty`/`InputOnly`/`CodeBearing` queue table: documented zero plus Empty/InputOnly is
`AL_INVALID`/false, CodeBearing dominates a zero, and negative/unsupported/unexpected returns are
`AL_CODE`. Decoder/import empty/unknown/resource/internal/fetch entries are `AL_CODE`. Every
decode/import/signature/digest fetch uses exact `provider=default`; the key and operation provider
pointers must equal the owned provider before publication/action, and Ed25519 construction performs
wrapper-owned canonical/on-curve/non-small-order point validation. Final free order is PKEY,
`OPENSSL_thread_stop_ex`, provider unload, library-context free, then shell free. Status and cleanup
follow the exact crypto design ledger: zero success; `AL_INVALID` for direct or closed-queue
constructor/key rejection and malformed internal ABI; `AL_CODE` for opaque provider/allocation/
empty-or-unknown queue failure; post-view signature mismatch publishes
false; and free is null-safe and one-time. These rows are part of the main keyed inventory; they
increase the keyed/base/max counts by six and extend the shipped ABI range to A00–A109.

## Request 9 owned JSON extension

The implemented Request 9 design in `docs/impl/24-owned-json-plan.md` adds exactly
one keyed base record while reusing an existing LLVM function shape:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `BuilderWriteUint` | `align_rt_builder_write_uint` | A66: `void @SYM(ptr, i64)`; Rust receives `(*mut Builder, u64)`; effects record above |

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

## Request 14 exclusive filesystem publication (implemented)

Request 14 adds exactly two keyed records and no new ABI shape. The implementation
activated these rows atomically with the HIR/MIR/runtime support, taking
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

## Request 18 retained-root regular-file access (implemented)

Request 18 adds exactly two keyed records and no new ABI shape. The implementation activated the
rows atomically with HIR/MIR lowering, checked-HIR validation, native exports, and retained-directory
traversal, taking the registry to 298 keyed / 311 base / 319 maximum optional-probe records:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `IoReaderOpenBeneath` | `align_rt_io_reader_open_beneath` | A12: `i32 @SYM(ptr, i64, ptr, i64, ptr)` |
| `IoWriterCreateExclusiveBeneath` | `align_rt_io_writer_create_exclusive_beneath` | A12: `i32 @SYM(ptr, i64, ptr, i64, ptr)` |

Both constructors keep the existing reader/writer output-slot convention: validate the slot first,
clear it, validate/copy/parse the complete root before inspecting relative, then
validate/copy/parse the complete relative before a filesystem call and traverse from retained
directory descriptors. The runtime publishes a handle only after the final regular-file open or
exclusive create succeeds. The exact declaration golden, key/symbol bijection, exports,
whole/per-unit declarations, and rt-LTO inventory update in the same capability. The public
contract and closure matrix are in
`docs/impl/29-fs-retained-root-plan.md` and `docs/impl/std-design/fs.md`.

## Request 55 retained-root single-link open (implemented)

Request 55 activates one keyed record using existing ABI shape A12:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `IoReaderOpenBeneathSingleLink` | `align_rt_io_reader_open_beneath_single_link` | A12: `i32 @SYM(ptr, i64, ptr, i64, ptr)` |

The row keeps the existing reader output-slot and two-path validation order. It reuses retained-root
traversal and regular-file descriptor revalidation, then consumes that opened descriptor's existing
stat record and publishes the existing reader only when `st_nlink == 1`. Implementation activated
the key atomically with the exact declaration golden, key/symbol bijection, native export,
whole/per-unit declaration, and rt-LTO inventory. It is part of the 356 keyed / 374 base inventory
recorded above. The
authoritative contract is `docs/impl/34-fs-single-link-plan.md`.

## Request 56 private temporary-directory lifecycle (implemented)

Request 56 activates two keyed records on existing ABI shapes with the complete HIR/MIR/runtime
implementation and does not change a shape or optional probe:

| Runtime key | Exact symbol | Existing ABI row and exact declaration |
|---|---|---|
| `FsCreatePrivateTempDir` | `align_rt_fs_create_private_temp_dir` | A08: `i32 @SYM(ptr, i64, ptr)` |
| `FsRemoveEmptyDir` | `align_rt_fs_remove_empty_dir` | A04: `i32 @SYM(ptr, i64)` |

The constructor keeps the existing owned-string output-slot convention: validate and zero the slot,
allocate the exact absolute-path result before filesystem mutation, and publish it only after one
successful `mkdirat`. Removal borrows one absolute strict path and returns only status. Activation
moves the prior 356 keyed / 374 base / 385 maximum-probe inventory to 358 / 376 / 387. Exact
prefix/root/randomness semantics, removal race boundary, and closure matrix are authoritative in
`docs/impl/36-fs-private-temp-plan.md`.

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
symbol. This table fixes the LLVM function type only; the per-symbol effect
record above fixes return, parameter, memory, and function attributes. The four
rt-LTO guarded symbols `align_rt_str_eq`, `align_rt_str_starts_with`,
`align_rt_str_ends_with`, and `align_rt_str_eq_ignore_case` use their record's
declaration attributes when rt-LTO is off. When rt-LTO is on, those attributes
are withheld before their visible bodies are linked and LLVM derives attributes
from the bodies. A merged body also carries no producer string attribute.
`align_rt_str_cmp` is not guarded and always uses its `PureArgRead` record.

| ABI | Exact LLVM function type | Symbols |
|---|---|---|
| A00 | `i32 @SYM(ptr, i64)` | `align_rt_utf8_valid` |
| A01 | `i32 @SYM(ptr, i64, ptr, i64)` | `align_rt_str_eq`, `align_rt_str_starts_with`, `align_rt_str_ends_with`, `align_rt_str_cmp`, `align_rt_str_eq_ignore_case` |
| A02 | `i32 @SYM(ptr, i64, ptr, i64)` | `align_rt_str_contains` |
| A03 | `i32 @SYM(ptr)` | `align_rt_io_writer_flush`, `align_rt_http_stream_finish`, `align_rt_os_host` |
| A04 | `i32 @SYM(ptr, i64)` | `align_rt_json_doc_kind`, `align_rt_fs_exists`, `align_rt_fs_create_dir`, `align_rt_fs_remove`, `align_rt_fs_remove_empty_dir`, `align_rt_child_kill`, `align_rt_tcp_conn_set_io_timeout` |
| A05 | `i32 @SYM(ptr, i64, i32, ptr)` | `align_rt_json_decode_array`, `align_rt_json_decode_scalar` |
| A06 | `i32 @SYM(ptr, i64, i64, i64, ptr)` | `align_rt_tcp_connect` |
| A07 | `i32 @SYM(ptr, i64, i64, ptr)` | `align_rt_json_doc_key`, `align_rt_tcp_listen`, `align_rt_udp_bind`, `align_rt_compress_gzip_compress`, `align_rt_compress_zstd_compress`, `align_rt_http_serve`, `align_rt_http_serve_shared` |
| A08 | `i32 @SYM(ptr, i64, ptr)` | `align_rt_json_doc_as_str`, `align_rt_json_doc_as_i64`, `align_rt_json_doc_as_f64`, `align_rt_json_doc_as_bool`, `align_rt_fs_read_file`, `align_rt_fs_is_dir`, `align_rt_fs_write_file_builder`, `align_rt_fs_read_dir`, `align_rt_fs_create_private_temp_dir`, `align_rt_dns_resolve`, `align_rt_io_reader_open`, `align_rt_bytes_as_str`, `align_rt_io_writer_create`, `align_rt_io_file_create`, `align_rt_io_file_open`, `align_rt_base64_decode`, `align_rt_base64url_decode`, `align_rt_hex_decode`, `align_rt_percent_decode`, `align_rt_form_decode`, `align_rt_compress_gzip_decompress`, `align_rt_compress_zstd_decompress`, `align_rt_http_parse`, `align_rt_regex_compile`, `align_rt_regex_captures_group`, `align_rt_env_get` |
| A09 | `i32 @SYM(ptr, i64, ptr, i64)` | `align_rt_fs_write_file`, `align_rt_process_exec`, `align_rt_crypto_ct_equal`, `align_rt_env_set` |
| A10 | `i32 @SYM(ptr, i64, ptr, i64, i64, i64, i64, i64, ptr)` | `align_rt_crypto_argon2id` |
| A12 | `i32 @SYM(ptr, i64, ptr, i64, ptr)` | `align_rt_process_spawn`, `align_rt_io_reader_open_beneath`, `align_rt_io_reader_open_beneath_single_link`, `align_rt_io_writer_create_exclusive_beneath` |
| A13 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, i64, ptr)` | `align_rt_crypto_hkdf_sha256` |
| A15 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, ptr, i64, ptr)` | `align_rt_crypto_aes_gcm_seal`, `align_rt_crypto_aes_gcm_open`, `align_rt_crypto_chacha20_poly1305_seal`, `align_rt_crypto_chacha20_poly1305_open` |
| A16 | `i32 @SYM(ptr, i64, ptr, i64, ptr, ptr, ptr, i64, i64)` | `align_rt_json_decode_soa` |
| A17 | `i32 @SYM(ptr, i64, ptr, ptr)` | `align_rt_json_doc_parse`, `align_rt_fs_read_file_view`, `align_rt_fs_read_bytes_view` |
| A103 | `i32 @SYM(ptr, i64, ptr, i64, ptr, i64, ptr, i64, i64, ptr)` | `align_rt_json_decode` (final nullable arena) |
| A104 | `i32 @SYM(ptr, i64, ptr, i64, i64, ptr, ptr, i64, i64, ptr)` | `align_rt_json_decode_struct_array` (final nullable arena) |
| A105 | `i32 @SYM(ptr, i64, ptr, ptr, ptr)` | `align_rt_json_decode_union` (final nullable arena) |
| A106 | `i32 @SYM(i32, ptr, i64, ptr)` | `align_rt_crypto_private_key_from_pem`, `align_rt_crypto_public_key_from_pem` |
| A107 | `i32 @SYM(i32, ptr, i64, ptr, i64, ptr)` | `align_rt_crypto_public_key_from_jwk` |
| A108 | `i32 @SYM(i32, ptr, ptr, i64, ptr)` | `align_rt_crypto_sign` |
| A109 | `i32 @SYM(i32, ptr, ptr, i64, ptr, i64, ptr)` | `align_rt_crypto_verify` |
| A18 | `i32 @SYM(ptr, i64, ptr, ptr, i64, ptr, i64, ptr, i64, i64)` | `align_rt_json_scan_next` |
| A19 | `i32 @SYM(ptr, ptr)` | `align_rt_tcp_accept`, `align_rt_command_run`, `align_rt_io_writer_write_builder`, `align_rt_http_accept`, `align_rt_http_respond`, `align_rt_http_stream_reject` |
| A127 | `ptr @SYM(ptr, i32, i64)` | `align_rt_json_builder_init` |
| A128 | `i32 @SYM(ptr, ptr)` | `align_rt_json_builder_finish` |
| A129 | `void @SYM(ptr, float)` | `align_rt_json_builder_write_f32` |
| A130 | `void @SYM(ptr, double)` | `align_rt_json_builder_write_f64` |
| A131 | `void @SYM(ptr, i8)` | `align_rt_command_new_session` |
| A132 | `i32 @SYM(ptr, i32, i64, ptr)` | `align_rt_child_poll` |
| A133 | `i64 @SYM(i32)` | `align_rt_process_signal_number` |
| A134 | `i32 @SYM(i32, ptr)` | `align_rt_process_signals` |
| A135 | `i32 @SYM(i32, i64, ptr)` | `align_rt_fs_memory_file` |
| A20 | `i32 @SYM(ptr, ptr, i64)` | `align_rt_io_writer_write`, `align_rt_cli_get_bool`, `align_rt_regex_is_match`, `align_rt_http_stream_send`, `align_rt_http_stream_send_event` |
| A21 | `i32 @SYM(ptr, ptr, i64, i64, ptr)` | `align_rt_http_get_many`, `align_rt_regex_find` |
| A22 | `i32 @SYM(ptr, ptr, i64, ptr)` | `align_rt_cli_parse`, `align_rt_http_resp_header`, `align_rt_http_client_get`, `align_rt_regex_find_all`, `align_rt_regex_split`, `align_rt_regex_captures`, `align_rt_http_ctx_header`, `align_rt_http_read_stream_header` |
| A23 | `i32 @SYM(ptr, ptr, i64, ptr, i64, ptr)` | `align_rt_http_client_post` |
| A24 | `i32 @SYM(ptr, ptr, ptr)` | `align_rt_http_client_request`, `align_rt_http_client_request_stream`, `align_rt_http_read_stream_read`, `align_rt_http_respond_stream` |
| A25 | `i64 @SYM()` | `align_rt_time_now`, `align_rt_time_instant`, `align_rt_process_cpu_count` |
| A26 | `i64 @SYM(ptr, i64)` | `align_rt_hash64` |
| A27 | `i64 @SYM(ptr, i64, ptr, i64)` | `align_rt_str_find`, `align_rt_str_rfind` |
| A28 | `i64 @SYM(ptr, ptr, i64)` | `align_rt_str_finder_find` |
| A29 | `i64 @SYM(ptr)` | `align_rt_io_file_len`, `align_rt_buffer_len`, `align_rt_buffer_capacity`, `align_rt_rng_next`, `align_rt_http_resp_status`, `align_rt_http_read_stream_status`, `align_rt_regex_group_count` |
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
| A42 | `ptr @SYM()` | `align_rt_arena_begin`, `align_rt_tg_begin` |
| A43 | `ptr @SYM(i64)` | `align_rt_alloc` |
| A44 | `ptr @SYM(ptr, i64)` | `align_rt_str_finder_new`, `align_rt_builder_new` |
| A45 | `ptr @SYM(ptr, i64, i64)` | `align_rt_arena_alloc`, `align_rt_tg_alloc` |
| A46 | `ptr @SYM(ptr, ptr, i64, i64, i64, i64, ptr)` | `align_rt_par_map` |
| A47 | `ptr @SYM()` | `align_rt_io_reader_stdin`, `align_rt_http_client_new`, `align_rt_crypto_digest_new` |
| A48 | `ptr @SYM(i32, i32)` | `align_rt_io_writer_std` |
| A49 | `ptr @SYM(i64)` | `align_rt_buffer_new`, `align_rt_http_response_new` |
| A50 | `ptr @SYM(ptr)` | `align_rt_tg_wait`, `align_rt_tcp_conn_reader`, `align_rt_tcp_conn_writer`, `align_rt_io_reader_buffered` |
| A51 | `ptr @SYM(ptr, i64)` | `align_rt_builder_init_bounded_stack`, `align_rt_cli_command_new` |
| A52 | `ptr @SYM(ptr, i64, ptr, i64)` | `align_rt_command_new`, `align_rt_http_request_new` |
| A53 | `ptr @SYM(ptr, ptr, i64)` | `align_rt_builder_init_stack` |
| A54 | `void @SYM()` | `align_rt_div_fail`, `align_rt_alloc_size_fail`, `align_rt_process_abort` |
| A55 | `void @SYM(double)` | `align_rt_print_f64` |
| A56 | `void @SYM(float)` | `align_rt_print_f32` |
| A57 | `void @SYM(i32)` | `align_rt_print_bool`, `align_rt_print_char` |
| A58 | `void @SYM(i64)` | `align_rt_print_i64`, `align_rt_time_sleep` |
| A59 | `void @SYM(i64)` | `align_rt_process_exit` |
| A60 | `void @SYM(i64, i64)` | `align_rt_bounds_fail`, `align_rt_len_mismatch_fail`, `align_rt_utf8_boundary_fail` |
| A61 | `void @SYM(i64, i64, i64)` | `align_rt_range_fail` |
| A62 | `void @SYM(ptr)` | `align_rt_arena_end`, `align_rt_tg_end`, `align_rt_free`, `align_rt_str_finder_free`, `align_rt_builder_pop_comma`, `align_rt_tcp_conn_free`, `align_rt_tcp_listener_free`, `align_rt_udp_socket_free`, `align_rt_child_free`, `align_rt_command_env_clear`, `align_rt_command_free`, `align_rt_run_output_free`, `align_rt_io_reader_free`, `align_rt_io_writer_free`, `align_rt_io_file_free`, `align_rt_buffer_free`, `align_rt_array_builder_free`, `align_rt_array_builder_free_stack`, `align_rt_array_builder_free_strings`, `align_rt_array_builder_free_strings_stack`, `align_rt_crypto_random`, `align_rt_crypto_key_free`, `align_rt_rng_seed_os`, `align_rt_cli_command_free`, `align_rt_cli_parsed_free`, `align_rt_http_request_free`, `align_rt_http_read_stream_free`, `align_rt_http_resp_free`, `align_rt_http_client_free`, `align_rt_http_server_free`, `align_rt_regex_captures_free`, `align_rt_regex_free`, `align_rt_http_ctx_free`, `align_rt_http_response_free`, `align_rt_http_stream_free`, `align_rt_builder_free`, `align_rt_builder_free_stack`, `align_rt_crypto_digest_free` |
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
| A73 | `void @SYM(ptr, ptr, i64)` | `align_rt_builder_write`, `align_rt_builder_write_json_str`, `align_rt_command_cwd`, `align_rt_buffer_append`, `align_rt_array_builder_push_str`, `align_rt_array_builder_append`, `align_rt_cli_flag_bool`, `align_rt_http_body`, `align_rt_http_rb_body`, `align_rt_crypto_digest_update` |
| A74 | `void @SYM(ptr, ptr, i64, i32)` | `align_rt_json_encode_scalar_array` |
| A75 | `void @SYM(ptr, ptr, i64, i64)` | `align_rt_rng_shuffle`, `align_rt_cli_flag_i64` |
| A76 | `void @SYM(ptr, ptr, i64, i64, ptr, i64)` | `align_rt_builder_write_str_int_str` |
| A77 | `void @SYM(ptr, ptr, i64, ptr, i64)` | `align_rt_command_env`, `align_rt_cli_flag_str`, `align_rt_http_header`, `align_rt_http_rb_header` |
| A78 | `void @SYM(ptr, ptr, i64, ptr, i64, i64)` | `align_rt_json_encode_struct_array` |
| A79 | `void @SYM(ptr, ptr, ptr)` | `align_rt_json_encode_union` |
| A80 | `void @SYM(ptr, ptr, ptr, i64)` | `align_rt_json_encode_object` |
| A81 | `void @SYM(ptr, ptr, ptr, ptr, ptr, ptr)` | `align_rt_tg_register` |
| A82 | `{ i64, i64 } @SYM(ptr, i64)` | `align_rt_hash128` |
| A83 | `{ ptr, i64 } @SYM(ptr)` | `align_rt_run_output_stdout`, `align_rt_run_output_stderr`, `align_rt_array_builder_build`, `align_rt_array_builder_build_stack`, `align_rt_builder_finish`, `align_rt_builder_finish_stack`, `align_rt_cli_usage`, `align_rt_http_resp_body`, `align_rt_http_ctx_method`, `align_rt_http_ctx_path`, `align_rt_http_ctx_body`, `align_rt_builder_into_string`, `align_rt_builder_into_string_stack`, `align_rt_crypto_digest_finish` |
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
| `CommandMaxCapture` | `align_rt_command_max_capture` | A66: `void @SYM(ptr, i64)`; effects record above |
| `CommandRunBytes` | `align_rt_command_run_bytes` | A19: `i32 @SYM(ptr, ptr)`; effects record above |
| `RunBytesStatus` | `align_rt_run_bytes_status` | A72: `void @SYM(ptr, ptr)`; effects record above |
| `RunBytesStdout` | `align_rt_run_bytes_stdout` | A83: `{ ptr, i64 } @SYM(ptr)`; effects record above |
| `RunBytesStderr` | `align_rt_run_bytes_stderr` | A83: `{ ptr, i64 } @SYM(ptr)`; effects record above |
| `RunBytesFree` | `align_rt_run_bytes_free` | A62: `void @SYM(ptr)`; effects record above |

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
| `HttpMaxResponseBodyBytes` | `align_rt_http_max_response_body_bytes` | A66: `void @SYM(ptr, i64)`; effects record above |
| `HttpClientMaxResponseBodyBytes` | `align_rt_http_client_max_response_body_bytes` | A66: `void @SYM(ptr, i64)`; effects record above |

Both use ordinary keyed-native identity and are mandatory base exports. The implementation updated
registry counts, bijection, declaration golden, and base runtime-export parity in the same change.
The HTTP-private negative result sentinel is not an ABI
symbol: client-response MIR maps it to reserved `Error.Code(-1)` before the common positive status
decoder, so it cannot collide with a saturating encoded errno.

Unkeyed native records:

| Owner | Exact LLVM function type | Runtime export presence |
|---|---|---|
| main error wrapper | `i32 @align_rt_report_error(i32)` | every Unit/Result main wrapper; effects record above |
| argv wrapper | `{ ptr, i64 } @align_rt_args_build(i32, ptr)` | only argv main; effects record above |
| arena implementation | `void @align_rt_arena_reset(ptr)` | always linked; runtime-internal; effects record above |
| allocator implementation | `ptr @align_rt_realloc(ptr, i64)` | always linked; runtime-internal; effects record above |
| HTTP implementation | `i32 @align_rt_http_serialize(ptr, ptr)` | always linked; runtime-internal; effects record above |
| PostgreSQL codec | `i32 @align_rt_f32_to_bits(float)` | always linked; package-internal compatible extern; effects record above |
| PostgreSQL codec | `float @align_rt_f32_from_bits(i32)` | always linked; package-internal compatible extern; effects record above |
| PostgreSQL codec | `i64 @align_rt_f64_to_bits(double)` | always linked; package-internal compatible extern; effects record above |
| PostgreSQL codec | `double @align_rt_f64_from_bits(i64)` | always linked; package-internal compatible extern; effects record above |
| PostgreSQL codec | `i64 @align_rt_f32_text_len(float)` | always linked; package-internal compatible extern; effects record above |
| PostgreSQL codec | `i64 @align_rt_f64_text_len(double)` | always linked; package-internal compatible extern; effects record above |
| PostgreSQL codec | `i64 @align_rt_f32_text_write(float, ptr, i64)` | always linked; package-internal compatible extern; effects record above |
| PostgreSQL codec | `i64 @align_rt_f64_text_write(double, ptr, i64)` | always linked; package-internal compatible extern; effects record above |
| pkg.kv TCP configuration | `i32 @align_rt_tcp_conn_set_io_timeout(ptr, i64)` | always linked; package-internal compatible extern; effects record above |
| allocation probe | `i64 @align_rt_alloc_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| allocation probe | `i64 @align_rt_free_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| requested-live probe | `void @align_rt_requested_live_reset()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| requested-live probe | `i64 @align_rt_requested_live_bytes()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| requested-live probe | `i64 @align_rt_requested_live_peak()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| finder probe | `i64 @align_rt_str_finder_new_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| finder probe | `i64 @align_rt_str_finder_free_count()` | only with the explicit `align_runtime/alloc-count` feature; no curated declaration attributes |
| parallel probe | `void @align_rt_test_par_map_force_caller(i32)` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_min_chunk()` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_min_chunk_for(i64, i64, i64)` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |
| parallel probe | `i64 @align_rt_test_par_map_workers()` | only with the explicit `align_runtime/par-map-probe` feature; no curated declaration attributes |

## Machine gates

Am-c1 replaces the current ABI/declaration, dedicated-consumer, AEAD-selection,
and attribute authorities with one typed `RuntimeAbi` row per identity.
`RuntimeAbi` owns `{ key, symbol, return type, ordered parameter types }` and the
separate total `RuntimeEffects` match owns class, argument memory, per-pointer
modes and escape, release, fresh-return, divergence, and rt-LTO admission facts.
Declaration and call lookup consume both records. `key` is `RuntimeAbiId`, either `Keyed(RuntimeKey)` or
`Unkeyed(UnkeyedRuntimeKey)`. The eighteen base unkeyed records use the same
typed-row machinery; only `ReportError` and `ArgsBuild` have a dedicated Align main-wrapper
declaration policy and yield typed wrapper handles when that wrapper requires
them. The other sixteen yield no unconditional compiler handle. An exact
compatible source extern may declare or reuse every non-test-control unkeyed row except
`ArgsBuild` through the ordinary extern path. The four compiler-private
`core.test` rows reject source reuse by policy. The remaining thirteen unkeyed rows
are source-reachable. `ArgsBuild`
returns the native `{ptr, i64}` argv view, which no source-valid extern return
can express: `str`/slice view returns are rejected, `raw` is not a valid
`layout(C)` field, and the closest source-valid `layout(C) { u64, i64 }` return
lowers as `{i64, i64}` rather than `{ptr, i64}`. It is therefore wrapper-only,
the only non-test-control base unkeyed row whose ABI has no compatible
source-extern form. The legacy mixed string map remains as a handle-only alias seam
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

The eleven probe rows have
`verification_presence = AllocCount | ParMapProbe`: their exact signatures are
checked against the corresponding runtime fixture, but their names do not
participate in compiler collision validation and no compatible-extern reuse
path is synthesized for them. The compiler uses the fixed base table before
LLVM construction and receives no runtime-feature input.

Tests compare:

- all 446 keys, mapped symbols, LLVM declaration types, and derived effects
  against this table through the checked-in
  `crates/align_codegen_llvm/tests/golden/runtime_abi_declarations.txt`;
- the 464 base native symbols against default-feature `align_runtime` exports,
  plus every actual Rust definition's normalized native return and ordered
  parameter types against the declaration golden, failing on either direction's
  difference through `scripts/test-runtime-abi-exports.sh`;
- the 471 `alloc-count` and 468 `par-map-probe` native symbols against
  `align_runtime` built with each feature separately, including the eleven exact
  probe signatures above;
- the 475 maximum native symbols against `align_runtime` built with
  `alloc-count,par-map-probe,task-group-probe`, while proving
  `task-group-probe` adds no unmangled export;
- rt-LTO off/on attributes for every guarded symbol, with missing,
  declaration-only, wrong-type, internal, private, available-externally, and
  non-C-calling-convention artifact negatives;
- all 446 identities through the one `RuntimeAbiId`-keyed row iterator and all
  446 exact registry function types through the production compatibility
  predicate, one return mutation per row, and one mutation of every parameter
  ordinal; source-valid compatible reuse for a keyed builtin and the thirteen
  source-reachable unkeyed rows; exact `ArgsBuild` `str` rejection plus the
  source-valid `layout(C) { u64, i64 }` aggregate mismatch; and
  compatible reuse representatives for the source-expressible emitted
  attribute forms, with the native row supplying its derived attributes; the
  checked-in golden covers all twelve effect classes, including rows whose
  native result has no source-valid extern spelling;
- one class-token mutation proving the declaration golden is class-sensitive,
  plus symbol and key coverage through the checked-in golden and uniqueness
  owners;
- ordinary extern and program-definition positives for all eleven probe
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
never a silent static-runtime fallback after partial mutation. That post-link
removal covers the row's curated enum attributes.

Before linking, codegen additionally sheds every string attribute from every
definition the artifact carries — not only the guarded rows, because linking
merges whatever that artifact defines — at the function, return, and every
parameter location, and then re-derives the result from that same module. A
definition still carrying one is one more baked-artifact defect: it falls back
loudly to the runtime staticlib without merging, on the same terms as a missing
symbol or a wrong type, so a new guarded row or a producing-compiler upgrade
can neither silently reintroduce a target-bound merged body nor fail a user's
build. The guarantee is function-scoped; module-level flags remain whatever
linking reconciles.

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

## Named time wire-format extension

`std-design/time.md` owns the exact five-by-two public contract and the following three
additional keyed rows. Compiler selection, native implementations, interface replay and cache
fingerprints activate together. Counts increase from 358/376/387 to 361 keyed, 379 base,
386 alloc-count, 383 par-map-probe and 390 maximum probe records. No probe category is added.

| Key | Shape | Exact declaration | Attributes |
|---|---|---|---|
| TimeFormat | A124 | `i32 @align_rt_time_format(ptr, i64, i32)` | nounwind; no parameter/return attributes |
| TimeParse | A125 | `i32 @align_rt_time_parse(ptr, ptr, i64, i32)` | nounwind; no parameter/return attributes |
| PercentEncodePath | A84 | `{ptr, i64} @align_rt_percent_encode_path(ptr, i64)` | existing A84 defaults |

The runtime uses the existing C convention and errno-to-Error translation. Output pointer/span
preflight precedes references; valid output is canonically zero even on failure. Closed native
time-kind tags 0–4 select the reviewed algorithms; every other tag returns EINVAL. One exact
owned-string allocation follows successful stack formatting. Parsing allocates nothing and
retains no input. The public ledger supplies the complete length, overlap, ownership and error
contract. A126 is the next unreserved shape. All earlier next-shape mentions describe their
historical capability boundary, not the current allocation point.

## HTTP client Drop capability closure

An HTTP client owns pooled connections; its existing destructor can call
`SSL_shutdown` and `SSL_free` even when a compilation unit contains no request
operation. Capability collection must therefore retain TLS libraries for client
construction and for a slot whose reachable value type contains a client.
This repairs the existing link contract without adding a runtime symbol, ABI
shape, allocation, ownership rule or serialized interface field.

| Closure axis | Implementation and exact owner |
|---|---|
| Construction, return-only producer, unbound expression | MIR `HttpClient` rvalue requires TLS; `http_client_drop_capability_matrix` |
| Move-in/out, Drop, replacement and joins | Existing runtime and MIR cleanup remain unchanged; slot-type capability discovery uses the cycle-safe structural type graph already used for signature keys; `http_client_drop_capability_matrix` covers direct, record, tuple, Option/Result, and enum carriers; owned record arrays containing clients remain rejected by the existing heap-record rule |
| Generic/imported and whole/per-unit compilation | Existing `function_capabilities` union feeds both MIR and interface summaries; `http_client_drop_capability_matrix` checks imported producer/consumer units and both native profiles |
| Borrow/control/early-exit and malformed input | No checking or lowering rule changes. The shared traversal retains its checked-index and visited-definition guards; `capability_linking` retains the signature-key and pure-program negative owners |
| ABI, allocation and runtime provenance | Existing constructor/destructor and cleanup-bit ABI are unchanged. Native construct/drop runs verify link completion without network access; no benchmark or resource claim is added |

The author-side matrix follows the shipped capability-linking strategy. Its
boundary check belongs to the single preflight code review. The focused owner is
`capability_linking`; no broad design or library-surface update is needed.

## Incremental SHA-256 (R29)

Plan 41 adds four keyed rows, taking the current registry from 361/379 to
365 keyed / 383 base records (394 with all optional probes). Each context is
uniquely owned; Update borrows input only for the call, Finish consumes the
context and returns the normal owned 32-byte array, and Free accepts null.

| Key | Symbol | Shape |
| --- | --- | --- |
| CryptoDigestNew | `align_rt_crypto_digest_new` | A47: `ptr ()`, no extra attributes |
| CryptoDigestUpdate | `align_rt_crypto_digest_update` | A73: `void (ptr, ptr, i64)` |
| CryptoDigestFinish | `align_rt_crypto_digest_finish` | A83: `{ptr,i64} (ptr)` |
| CryptoDigestFree | `align_rt_crypto_digest_free` | A62: `void (ptr)` |

The latter three rows also have no extra attributes. Constructor error cleanup
can free native allocations, so New deliberately does not claim `nofree`.
The exact ownership, input checks and failure order are in plan 41.

## Host observation ABI

`RuntimeKey::OsHost` maps to `align_rt_os_host`, A03 `i32(ptr)` with no extra
attributes. The exact 88-byte, alignment-8 out-record and validation order are
owned by [plan 42](42-host-observation-plan.md). Zero means success; nonzero is
existing errno status with empty scratch and no escaped owner. The native and
LLVM layout owners independently pin every field offset. Counts are now 366
keyed / 384 base / 391 alloc-count / 388 par-map-probe / 395 maximum exports.
No new optional probe category or scalar encoding tag is introduced.

## Ordinary directory operation ABI

`FsCreateDir` selects `align_rt_fs_create_dir`, A04 i32(ptr,i64).
`FsIsDir` selects `align_rt_fs_is_dir`, A08 i32(ptr,i64,ptr), with a one-byte
bool out scratch. Both return existing errno status with no extra attributes.
[Plan 43](43-ordinary-directory-plan.md) fixes address/overlap/content validation
order and untouched-versus-false error output. Counts are 368 keyed / 386 base /
393 alloc-count / 390 par-map-probe / 397 maximum exports. No new probe category.

## Retained byte-tree native boundary (R64)

[Plan 45](45-retained-byte-tree-plan.md) is the exact 21-symbol appendix: nineteen
operations and directory/cursor Drop. Both owners are pointers. Metadata is the
64-byte, alignment-8 ordinary record with the specified physical permutation.
CursorNext writes a 16-byte owned byte header plus a separate one-byte presence
flag; MIR reconstructs Result/Option. A closed `FsTreeOutput` discriminator and
exact schema, input and fresh-scratch producer certification precede native writes.
Numeric extent/alignment/overlap errors leave output untouched; subsequent
lexical/mode/native errors leave zero output. No new LLVM pointer attributes or
probe category was added. At that boundary: 391 keyed, 409 base, 416 alloc-count,
413 par-map-probe, 420 maximum exports. Declaration and native signature/export
owners independently verify every symbol and argument ordinal.

## Common live process boundary (R65)

Plan 49 owns the exact common operations and scratch layouts. Sixteen new keyed
rows replace the two numeric capture-code accessors (net fourteen); ChildWait
now writes a 32-byte typed wait result. A131 is void(ptr,i8), A132 is
i32(ptr,i32,i64,ptr), and A133 is i64(i32). Every other row reuses an existing
shape. Current inventory: 409 keyed, 427 base, 434 alloc-count, 431 par-map-probe,
and 438 maximum exports. No probe category is added. The declaration golden and
native export owner cover every ordinal; live output buffers additionally require
exclusive backing provenance in HIR and MIR. Signal subscriptions and the stronger
file/scope capabilities activate their separate rows in the subsequent capability
boundaries selected by plan 50.

### R65 explicit signal subscription

`ProcessSignals` adds A134 `i32 @SYM(i32, ptr)` with no function or parameter
attributes: the four low input bits select HUP/INT/QUIT/TERM, and the pointer
receives a null-initialized owned subscription. `ProcessSignalNext` uses A19;
its disjoint output is eight bytes (`u8` option tag, three zero padding bytes,
`i32` signal discriminant). `ProcessSignalClose` uses A03 and
`ProcessSignalFree` uses A62. The latter remains reachable through recursive
Drop even when the importing module performs no signal operation. All four
operations are impure, with no allocation/read-only/termination attributes.

The opaque non-Send owner uses canonical Ty tag 76 / Scalar tag 54, native
pointer layout, and existing interface-12 named transport. Signal handlers
reference only permanent atomic state; no native owner pointer escapes to them.

### R65 verified launch authority

Seventeen keyed records brought that capability inventory to 426 keyed, 444 base,
451 alloc-count, 448 par-map-probe and 455 maximum exports. Plan 50 owns exact
signatures and ownership. FsMemoryFile uses A135. FsMemoryWrite and both
CommandInherit rows use A20; FsMemorySeal and ProcessExecutable use A19;
FsSealedReadAt and ProcessImageReadAt use A12; both cached Len rows use A29;
CommandImage uses A22; ProcessCurrentImage uses A03; ProcessUserNamespace uses
A08; the four owner frees use A62. None adds native attributes or optional exports.

### R65 exclusive child scope

Nine keyed rows brought the plan-50 inventory to 435 keyed, 453 base, 460
alloc-count, 457 par-map-probe and 464 maximum exports. Plan 50 owns the
exact signatures and record order. CommandStartScope uses A19; ScopeOwnerId
uses A29; ScopeChildren and ScopeReap use A08; ProcessMemberKill uses A04;
ProcessMemberFinished and ScopeRelease use A19; both owner frees use A62.
The shared Child operations accept the validated NativeChild prefix at offset
zero only through scope-specific checked operations; ScopeFree retains the
exclusive lease until cleanup and restoration complete. MemberInfo is a natural
16-byte owned record; Reaped is a 40-byte Copy record with status at offset 8.
The two canonical leaves are Ty 81/82 and Scalar 59/60. No attributes or optional
exports are added.


## Retained observations and identity (plan 54 capability A)

These six keys are inserted alphabetically in RuntimeKey::ALL and in the
independent declaration golden. All have the ordinary nounwind attribute and
no curated parameter attributes. No new probe export or canonical type tag is added.

| RuntimeKey | Symbol | Shape |
| --- | --- | --- |
| FsDirectoryAccess | align_rt_fs_directory_access | A136: `i32(ptr, i8, i8, i8, ptr)` |
| FsDirectoryAccessAt | align_rt_fs_directory_access_at | A137: `i32(ptr, ptr, i64, i8, i8, i8, ptr)` |
| FsDirectoryCreateSymlink | align_rt_fs_directory_create_symlink | A120: `i32(ptr, ptr, i64, ptr, i64)` |
| FsDirectoryMetadataFollow | align_rt_fs_directory_metadata_follow | A22: `i32(ptr, ptr, i64, ptr)` |
| FsDirectoryReadLink | align_rt_fs_directory_read_link | A21: `i32(ptr, ptr, i64, i64, ptr)` |
| OsIdentity | align_rt_os_identity | A03: `i32(ptr)` |

Plan 54 fixes input ordering, numeric/disjointness validation, zero-on-content-error
scratch, exact output layouts, ownership, native error partition and platform
requirements. The three access bytes are normalized read/write/execute values,
not native masks. Identity output has i64 fields at offsets 0 and 8, size 16,
alignment 8. Read-link output is an independently owned byte-array header.

## Ordinary-path regular reader (R88)

| Key | Native symbol | Exact declaration |
| --- | --- | --- |
| `IoReaderOpenRegular` | `align_rt_io_reader_open_regular` | A08: `i32 @SYM(ptr, i64, ptr)` |

The input is an immutable UTF-8 path view; the output is a reader-pointer slot.
Numerical output alignment/extent, input length/extent and disjointness validation
precede reads and writes. Malformed numerical inputs leave output untouched;
after preflight it is cleared and every semantic/native failure leaves null.
UTF-8/NUL validation precedes open. Zero-length null input represents an empty
path; positive null, negative length, scratch-length overflow and overlap reject.
Linux/macOS use ordinary path resolution, read-only nonblocking close-on-exec open,
fd stat regular-kind admission, and blocking restoration on that same descriptor.
Each open/stat/fcntl interruption retries. Primary errors retain their status while
RAII performs existing single-close cleanup; close errors do not replace them.
Success publishes the existing owned reader layout and Drop, retaining no input.
Transient NUL-terminated path scratch and the reader handle are the allocations.
No new ABI attributes, wire format, runtime layout or probe category is added.

## Issue batch constructor ABI

Plan 65 adds two keyed/base exports and changes three constructor signatures.
The Buffer and ArrayBuilder physical layouts are unchanged. Compiler and runtime
migrate together; no compatibility exports are retained.

| Shape | Exact declaration | Semantics |
| --- | --- | --- |
| BufferFilled | `ptr @align_rt_buffer_filled(i64 length, i8 value)` | Validate length against target isize before allocation; OOM aborts. Return one owned handle with exactly initialized length. Zero has no payload; nonzero has one payload acquisition. |
| BufferAppendFilled | `void @align_rt_buffer_append_filled(ptr buffer, i64 length, i8 value)` | Row B's append member. A null handle is a no-op. Validate length against target isize, then the published length plus it, before touching the payload; an invalid or overflowing request aborts with the window unchanged, and OOM aborts. Truncate to the logical length, then one reserve plus one resize — never a growth sequence. Zero length publishes nothing. |
| ArrayBuilderCapacity | `noalias ptr @align_rt_array_builder_new(i64 stride, i64 capacity) {nofree nounwind}` | Fresh heap header; validated count × stride before any acquisition; initialized length zero; nonzero requested storage reserved. |
| ArrayBuilderRegionCapacity | `noalias ptr @align_rt_array_builder_new_in(ptr arena, i64 stride, i64 align, i64 capacity) {nounwind}` | Existing arena/stride/alignment validation precedes count/layout validation and allocation. Header and initial chunk belong to the arena; length zero. |
| ArrayBuilderStackCapacity | `ptr @align_rt_array_builder_init_stack(ptr out, i64 stride, i64 capacity)` | Existing writable 64-byte/16-aligned header precondition; same count/layout validation; heap payload with stack header. |

Capacity zero is the explicit generated argument for omitted source capacity.
Only initialized prefixes participate in Drop or freeze; the heap payload still
transfers and the region output still compacts. `explicit_constructor_capacity_preserves_payload_and_initialized_prefix`
owns header-mode, no-growth, initialization and heap transfer parity.
Float inspection adds no native export.

Plan 70 PR 3 adds no export and does not change these declarations. Its compiler-side scalar-push
fast path consumes an exact `ArrayBuilder` layout contract: `data = 0`, `len = 8`, `cap = 16`,
`elem_size = 24`, `arena = 32`, total size 64, and alignment at most 16. Const assertions in
`align_runtime` pin the native definition; `align_codegen_llvm::ARRAY_BUILDER_LAYOUT` carries the
same table, and the driver owner compares them. Heap mode with matching stride and spare capacity
uses one typed store and increments `len`; `bool` is zero-extended from LLVM `i1` to the runtime's
canonical `i8` byte before that store. Every other state calls the existing
`align_rt_array_builder_push` exactly once.
