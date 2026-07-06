; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

; Function Attrs: nounwind
define i32 @double(i32 %0) #0 {
bb0:
  %_0 = alloca i32, align 4
  store i32 %0, ptr %_0, align 4
  %load = load i32, ptr %_0, align 4
  %mul = mul i32 %load, 2
  ret i32 %mul
}

; Function Attrs: nounwind
define i1 @big(i32 %0) #0 {
bb0:
  %_0 = alloca i32, align 4
  store i32 %0, ptr %_0, align 4
  %load = load i32, ptr %_0, align 4
  %gt = icmp sgt i32 %load, 4
  ret i1 %gt
}

; Function Attrs: nounwind
define i32 @main() #0 {
bb0:
  %_0 = alloca { ptr, i64 }, align 8
  %_1 = alloca [5 x i32], align 4
  %_2 = alloca i64, align 8
  %_3 = alloca i64, align 8
  %_4 = alloca i32, align 4
  %_5 = alloca i64, align 8
  %arena = call ptr @align_rt_arena_begin()
  %elemptr = getelementptr inbounds [5 x i32], ptr %_1, i64 0, i64 0
  store i32 1, ptr %elemptr, align 4
  %elemptr1 = getelementptr inbounds [5 x i32], ptr %_1, i64 0, i64 1
  store i32 2, ptr %elemptr1, align 4
  %elemptr2 = getelementptr inbounds [5 x i32], ptr %_1, i64 0, i64 2
  store i32 3, ptr %elemptr2, align 4
  %elemptr3 = getelementptr inbounds [5 x i32], ptr %_1, i64 0, i64 3
  store i32 4, ptr %elemptr3, align 4
  %elemptr4 = getelementptr inbounds [5 x i32], ptr %_1, i64 0, i64 4
  store i32 5, ptr %elemptr4, align 4
  %buf = call ptr @align_rt_arena_alloc(ptr %arena, i64 20, i64 4)
  store i64 0, ptr %_2, align 8
  store i64 0, ptr %_3, align 8
  br label %bb1

bb1:                                              ; preds = %bb3, %bb0
  %load = load i64, ptr %_3, align 8
  %lt = icmp slt i64 %load, 5
  br i1 %lt, label %bb2, label %bb4

bb2:                                              ; preds = %bb1
  %load5 = load i64, ptr %_3, align 8
  %elemptr6 = getelementptr inbounds [5 x i32], ptr %_1, i64 0, i64 %load5
  %idx = load i32, ptr %elemptr6, align 4
  %call = call i32 @double(i32 %idx)
  %call7 = call i1 @big(i32 %call)
  br i1 %call7, label %bb5, label %bb3

bb3:                                              ; preds = %bb5, %bb2
  %load8 = load i64, ptr %_3, align 8
  %add = add i64 %load8, 1
  store i64 %add, ptr %_3, align 8
  br label %bb1

bb4:                                              ; preds = %bb1
  %load9 = load i64, ptr %_2, align 8
  %arrptr = insertvalue { ptr, i64 } poison, ptr %buf, 0
  %arrlen = insertvalue { ptr, i64 } %arrptr, i64 %load9, 1
  store { ptr, i64 } %arrlen, ptr %_0, align 8
  %load10 = load { ptr, i64 }, ptr %_0, align 8
  %len = extractvalue { ptr, i64 } %load10, 1
  %eq = icmp eq i64 %len, 3
  br i1 %eq, label %bb6, label %bb7

bb5:                                              ; preds = %bb2
  %load11 = load i64, ptr %_2, align 8
  %ptrstore = getelementptr inbounds i32, ptr %buf, i64 %load11
  store i32 %call, ptr %ptrstore, align 4
  %add12 = add i64 %load11, 1
  store i64 %add12, ptr %_2, align 8
  br label %bb3

bb6:                                              ; preds = %bb4
  %load13 = load { ptr, i64 }, ptr %_0, align 8
  %len14 = extractvalue { ptr, i64 } %load13, 1
  store i32 0, ptr %_4, align 4
  store i64 0, ptr %_5, align 8
  br label %bb9

bb7:                                              ; preds = %bb4
  br label %bb8

bb8:                                              ; preds = %bb7
  call void @align_rt_arena_end(ptr %arena)
  ret i32 0

bb9:                                              ; preds = %bb11, %bb6
  %load15 = load i64, ptr %_5, align 8
  %lt16 = icmp slt i64 %load15, %len14
  br i1 %lt16, label %bb10, label %bb12

bb10:                                             ; preds = %bb9
  %load17 = load i64, ptr %_5, align 8
  %ptr = extractvalue { ptr, i64 } %load13, 0
  %slcidx = getelementptr inbounds i32, ptr %ptr, i64 %load17
  %slcload = load i32, ptr %slcidx, align 4
  %load18 = load i32, ptr %_4, align 4
  %add19 = add i32 %load18, %slcload
  store i32 %add19, ptr %_4, align 4
  br label %bb11

bb11:                                             ; preds = %bb10
  %load20 = load i64, ptr %_5, align 8
  %add21 = add i64 %load20, 1
  store i64 %add21, ptr %_5, align 8
  br label %bb9

bb12:                                             ; preds = %bb9
  %load22 = load i32, ptr %_4, align 4
  call void @align_rt_arena_end(ptr %arena)
  ret i32 %load22
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
