; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

; Function Attrs: nounwind
define void @scale({ ptr, i64 } %0, { ptr, i64 } %1) #0 {
bb0:
  %_0 = alloca { ptr, i64 }, align 8
  %_1 = alloca { ptr, i64 }, align 8
  %_2 = alloca <4 x i64>, align 32
  store { ptr, i64 } %0, ptr %_0, align 8
  store { ptr, i64 } %1, ptr %_1, align 8
  %load = load { ptr, i64 }, ptr %_0, align 8
  %len = extractvalue { ptr, i64 } %load, 1
  %gt = icmp sgt i64 4, %len
  %or = or i1 false, %gt
  br i1 %or, label %bb1, label %bb2

bb1:                                              ; preds = %bb0
  call void @align_rt_range_fail(i64 0, i64 4, i64 %len)
  unreachable

bb2:                                              ; preds = %bb0
  %vlbuf = extractvalue { ptr, i64 } %load, 0
  %vloadgep = getelementptr inbounds i64, ptr %vlbuf, i64 0
  %vload = load <4 x i64>, ptr %vloadgep, align 8
  store <4 x i64> %vload, ptr %_2, align 32
  %load1 = load { ptr, i64 }, ptr %_1, align 8
  %load2 = load <4 x i64>, ptr %_2, align 32
  %vmul = mul <4 x i64> %load2, <i64 2, i64 2, i64 2, i64 2>
  %len3 = extractvalue { ptr, i64 } %load1, 1
  %gt4 = icmp sgt i64 4, %len3
  %or5 = or i1 false, %gt4
  br i1 %or5, label %bb3, label %bb4

bb3:                                              ; preds = %bb2
  call void @align_rt_range_fail(i64 0, i64 4, i64 %len3)
  unreachable

bb4:                                              ; preds = %bb2
  %vsbuf = extractvalue { ptr, i64 } %load1, 0
  %vstoregep = getelementptr inbounds i64, ptr %vsbuf, i64 0
  store <4 x i64> %vmul, ptr %vstoregep, align 8
  ret void
}

; Function Attrs: nounwind
define i32 @main() #0 {
bb0:
  %_0 = alloca [4 x i64], align 8
  %_1 = alloca [4 x i64], align 8
  %elemptr = getelementptr inbounds [4 x i64], ptr %_0, i64 0, i64 0
  store i64 10, ptr %elemptr, align 8
  %elemptr1 = getelementptr inbounds [4 x i64], ptr %_0, i64 0, i64 1
  store i64 20, ptr %elemptr1, align 8
  %elemptr2 = getelementptr inbounds [4 x i64], ptr %_0, i64 0, i64 2
  store i64 30, ptr %elemptr2, align 8
  %elemptr3 = getelementptr inbounds [4 x i64], ptr %_0, i64 0, i64 3
  store i64 40, ptr %elemptr3, align 8
  %elemptr4 = getelementptr inbounds [4 x i64], ptr %_1, i64 0, i64 0
  store i64 0, ptr %elemptr4, align 8
  %elemptr5 = getelementptr inbounds [4 x i64], ptr %_1, i64 0, i64 1
  store i64 0, ptr %elemptr5, align 8
  %elemptr6 = getelementptr inbounds [4 x i64], ptr %_1, i64 0, i64 2
  store i64 0, ptr %elemptr6, align 8
  %elemptr7 = getelementptr inbounds [4 x i64], ptr %_1, i64 0, i64 3
  store i64 0, ptr %elemptr7, align 8
  %slcbase = getelementptr inbounds [4 x i64], ptr %_0, i64 0, i64 0
  %slcptr = insertvalue { ptr, i64 } poison, ptr %slcbase, 0
  %slclen = insertvalue { ptr, i64 } %slcptr, i64 4, 1
  %slcbase8 = getelementptr inbounds [4 x i64], ptr %_1, i64 0, i64 0
  %slcptr9 = insertvalue { ptr, i64 } poison, ptr %slcbase8, 0
  %slclen10 = insertvalue { ptr, i64 } %slcptr9, i64 4, 1
  call void @scale({ ptr, i64 } %slclen, { ptr, i64 } %slclen10)
  br i1 false, label %bb1, label %bb2

bb1:                                              ; preds = %bb0
  call void @align_rt_bounds_fail(i64 0, i64 4)
  unreachable

bb2:                                              ; preds = %bb0
  %elemptr11 = getelementptr inbounds [4 x i64], ptr %_1, i64 0, i64 0
  %idx = load i64, ptr %elemptr11, align 8
  br i1 false, label %bb3, label %bb4

bb3:                                              ; preds = %bb2
  call void @align_rt_bounds_fail(i64 3, i64 4)
  unreachable

bb4:                                              ; preds = %bb2
  %elemptr12 = getelementptr inbounds [4 x i64], ptr %_1, i64 0, i64 3
  %idx13 = load i64, ptr %elemptr12, align 8
  %add = add i64 %idx, %idx13
  %cast = trunc i64 %add to i32
  ret i32 %cast
}

declare void @align_rt_print_i64(i64)

declare void @align_rt_bounds_fail(i64, i64)

declare void @align_rt_len_mismatch_fail(i64, i64)

declare void @align_rt_range_fail(i64, i64, i64)

declare void @align_rt_div_fail()

; Function Attrs: nofree nounwind
declare noalias ptr @align_rt_arena_begin() #1

; Function Attrs: nounwind
declare noalias ptr @align_rt_arena_alloc(ptr, i64, i64) #0

declare void @align_rt_arena_end(ptr)

; Function Attrs: nofree nounwind
declare noalias ptr @align_rt_tg_begin() #1

; Function Attrs: nounwind
declare noalias ptr @align_rt_tg_alloc(ptr, i64, i64) #0

declare void @align_rt_tg_register(ptr, ptr, ptr, ptr, ptr, ptr)

declare ptr @align_rt_tg_wait(ptr)

declare void @align_rt_tg_end(ptr)

; Function Attrs: nofree nounwind
declare noalias ptr @align_rt_alloc(i64) #1

declare void @align_rt_free(ptr)

declare { ptr, i64 } @align_rt_chunks(ptr, i64, i64, i64)

declare noalias ptr @align_rt_par_map(ptr, i64, i64, i64, ptr)

declare void @align_rt_print_str(ptr, i64)

declare void @align_rt_print_bool(i32)

declare void @align_rt_print_char(i32)

declare void @align_rt_print_f32(float)

declare void @align_rt_print_f64(double)

declare i32 @align_rt_str_eq(ptr, i64, ptr, i64)

declare i32 @align_rt_str_contains(ptr, i64, ptr, i64)

declare i32 @align_rt_str_starts_with(ptr, i64, ptr, i64)

declare i32 @align_rt_str_ends_with(ptr, i64, ptr, i64)

declare i64 @align_rt_str_find(ptr, i64, ptr, i64)

declare i64 @align_rt_str_rfind(ptr, i64, ptr, i64)

declare i32 @align_rt_str_eq_ignore_case(ptr, i64, ptr, i64)

; Function Attrs: nofree nounwind
declare noalias ptr @align_rt_builder_new(ptr, i64) #1

declare void @align_rt_builder_write(ptr, ptr, i64)

declare void @align_rt_builder_write_int(ptr, i64)

declare void @align_rt_builder_write_str_int_str(ptr, ptr, i64, i64, ptr, i64)

declare void @align_rt_builder_write_bool(ptr, i32)

declare void @align_rt_builder_write_char(ptr, i32)

declare void @align_rt_builder_write_f32(ptr, float)

declare void @align_rt_builder_write_f64(ptr, double)

declare void @align_rt_builder_write_json_str(ptr, ptr, i64)

declare i32 @align_rt_json_decode(ptr, i64, ptr, i64, ptr, i64, ptr, i64, i64)

declare i32 @align_rt_json_decode_array(ptr, i64, i32, ptr)

declare i32 @align_rt_fs_read_file(ptr, i64, ptr)

declare i32 @align_rt_fs_write_file(ptr, i64, ptr, i64)

declare i32 @align_rt_fs_write_file_builder(ptr, i64, ptr)

declare i32 @align_rt_fs_exists(ptr, i64)

declare i32 @align_rt_fs_remove(ptr, i64)

declare i32 @align_rt_fs_read_dir(ptr, i64, ptr)

declare i32 @align_rt_fs_read_file_view(ptr, i64, ptr, ptr)

declare void @align_rt_free_string_array(ptr, i64)

declare i32 @align_rt_io_reader_open(ptr, i64, ptr)

declare ptr @align_rt_io_reader_stdin()

declare i64 @align_rt_io_reader_read(ptr, ptr)

declare void @align_rt_io_reader_free(ptr)

declare i32 @align_rt_io_writer_create(ptr, i64, ptr)

declare ptr @align_rt_io_writer_std(i32, i32)

declare i32 @align_rt_io_writer_write(ptr, ptr, i64)

declare i32 @align_rt_io_writer_write_builder(ptr, ptr)

declare i32 @align_rt_io_writer_flush(ptr)

declare void @align_rt_io_writer_free(ptr)

declare i64 @align_rt_io_copy(ptr, ptr)

declare ptr @align_rt_buffer_new(i64)

declare void @align_rt_buffer_bytes(ptr, ptr)

declare i64 @align_rt_buffer_len(ptr)

declare void @align_rt_buffer_free(ptr)

declare i32 @align_rt_json_decode_struct_array(ptr, i64, ptr, i64, i64, ptr, ptr, i64, i64)

declare i32 @align_rt_json_decode_soa(ptr, i64, ptr, i64, ptr, ptr, ptr, i64, i64)

declare { ptr, i64 } @align_rt_builder_finish(ptr)

declare i64 @align_rt_group_sum_i64(ptr, ptr, i64, ptr, ptr, i64)

declare i64 @align_rt_group_min_i64(ptr, ptr, i64, ptr, ptr, i64)

declare i64 @align_rt_group_max_i64(ptr, ptr, i64, ptr, ptr, i64)

declare i64 @align_rt_group_count_i64(ptr, i64, ptr, ptr, i64)

declare i64 @align_rt_group_sum_str(ptr, i64, i64, i64, i64, ptr, ptr, i64)

declare i64 @align_rt_group_min_str(ptr, i64, i64, i64, i64, ptr, ptr, i64)

declare i64 @align_rt_group_max_str(ptr, i64, i64, i64, i64, ptr, ptr, i64)

declare i64 @align_rt_group_count_str(ptr, i64, i64, i64, i64, ptr, ptr, i64)

declare i64 @align_rt_group_sum_str_cols(ptr, ptr, i64, ptr, ptr, i64)

declare i64 @align_rt_group_min_str_cols(ptr, ptr, i64, ptr, ptr, i64)

declare i64 @align_rt_group_max_str_cols(ptr, ptr, i64, ptr, ptr, i64)

declare i64 @align_rt_group_count_str_cols(ptr, ptr, i64, ptr, ptr, i64)

declare i64 @align_rt_group_multi_str(ptr, i64, i64, i64, ptr, i64, ptr, i64)

declare i64 @align_rt_dict_encode_str(ptr, i64, i64, i64, ptr, ptr, i64)

declare void @align_rt_gather_i64(ptr, i64, i64, i64, ptr)

declare void @align_rt_dict_lookup(ptr, i64, ptr, i64, ptr)

declare { ptr, i64 } @align_rt_str_clone(ptr, i64)

declare i64 @align_rt_hash64(ptr, i64)

declare { i64, i64 } @align_rt_hash128(ptr, i64)

declare { ptr, i64 } @align_rt_base64_encode(ptr, i64)

declare { ptr, i64 } @align_rt_base64url_encode(ptr, i64)

declare { ptr, i64 } @align_rt_hex_encode(ptr, i64)

declare i32 @align_rt_base64_decode(ptr, i64, ptr)

declare i32 @align_rt_base64url_decode(ptr, i64, ptr)

declare i32 @align_rt_hex_decode(ptr, i64, ptr)

declare i32 @align_rt_utf8_valid(ptr, i64)

declare void @align_rt_rng_seed_with(ptr, i64)

declare void @align_rt_rng_seed_os(ptr)

declare i64 @align_rt_rng_next(ptr)

declare i64 @align_rt_rng_range(ptr, i64, i64)

declare void @align_rt_rng_shuffle(ptr, ptr, i64, i64)

declare { ptr, i64 } @align_rt_rng_sample(ptr, ptr, i64, i64, i64)

declare ptr @align_rt_cli_command_new(ptr, i64)

declare void @align_rt_cli_flag_bool(ptr, ptr, i64)

declare void @align_rt_cli_flag_str(ptr, ptr, i64, ptr, i64)

declare void @align_rt_cli_flag_i64(ptr, ptr, i64, i64)

declare i32 @align_rt_cli_parse(ptr, ptr, i64, ptr)

declare i32 @align_rt_cli_get_bool(ptr, ptr, i64)

declare i64 @align_rt_cli_get_i64(ptr, ptr, i64)

declare { ptr, i64 } @align_rt_cli_get_str(ptr, ptr, i64)

declare { ptr, i64 } @align_rt_cli_usage(ptr)

declare void @align_rt_cli_command_free(ptr)

declare void @align_rt_cli_parsed_free(ptr)

declare { ptr, i64 } @align_rt_str_trim(ptr, i64)

declare { ptr, i64 } @align_rt_str_trim_start(ptr, i64)

declare { ptr, i64 } @align_rt_str_trim_end(ptr, i64)

declare { ptr, i64 } @align_rt_path_base(ptr, i64)

declare { ptr, i64 } @align_rt_path_dir(ptr, i64)

declare { ptr, i64 } @align_rt_path_ext(ptr, i64)

declare { ptr, i64 } @align_rt_path_normalize(ptr, i64)

declare { ptr, i64 } @align_rt_path_join(ptr, i64, ptr, i64)

declare i32 @align_rt_env_get(ptr, i64, ptr)

declare i32 @align_rt_env_set(ptr, i64, ptr, i64)

declare i64 @align_rt_time_now()

declare i64 @align_rt_time_instant()

declare void @align_rt_time_sleep(i64)

declare { ptr, i64 } @align_rt_builder_into_string(ptr)

declare void @align_rt_builder_free(ptr)

attributes #0 = { nounwind }
attributes #1 = { nofree nounwind }
