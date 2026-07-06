; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

%CacheLine = type { i64, i64, i64, i64, [32 x i8] }

; Function Attrs: nounwind
define i32 @main() #0 {
bb0:
  %_0 = alloca %CacheLine, align 64
  %_1 = alloca [2 x %CacheLine], align 64
  %_2 = alloca i64, align 8
  %_3 = alloca i64, align 8
  %_4 = alloca i64, align 8
  %fldptr = getelementptr inbounds %CacheLine, ptr %_0, i32 0, i32 0
  store i64 10, ptr %fldptr, align 8
  %fldptr1 = getelementptr inbounds %CacheLine, ptr %_0, i32 0, i32 1
  store i64 20, ptr %fldptr1, align 8
  %fldptr2 = getelementptr inbounds %CacheLine, ptr %_0, i32 0, i32 2
  store i64 30, ptr %fldptr2, align 8
  %fldptr3 = getelementptr inbounds %CacheLine, ptr %_0, i32 0, i32 3
  store i64 40, ptr %fldptr3, align 8
  %elemfield = getelementptr inbounds [2 x %CacheLine], ptr %_1, i64 0, i64 0, i32 0
  store i64 1, ptr %elemfield, align 8
  %elemfield4 = getelementptr inbounds [2 x %CacheLine], ptr %_1, i64 0, i64 0, i32 1
  store i64 2, ptr %elemfield4, align 8
  %elemfield5 = getelementptr inbounds [2 x %CacheLine], ptr %_1, i64 0, i64 0, i32 2
  store i64 3, ptr %elemfield5, align 8
  %elemfield6 = getelementptr inbounds [2 x %CacheLine], ptr %_1, i64 0, i64 0, i32 3
  store i64 4, ptr %elemfield6, align 8
  %elemfield7 = getelementptr inbounds [2 x %CacheLine], ptr %_1, i64 0, i64 1, i32 0
  store i64 5, ptr %elemfield7, align 8
  %elemfield8 = getelementptr inbounds [2 x %CacheLine], ptr %_1, i64 0, i64 1, i32 1
  store i64 6, ptr %elemfield8, align 8
  %elemfield9 = getelementptr inbounds [2 x %CacheLine], ptr %_1, i64 0, i64 1, i32 2
  store i64 7, ptr %elemfield9, align 8
  %elemfield10 = getelementptr inbounds [2 x %CacheLine], ptr %_1, i64 0, i64 1, i32 3
  store i64 8, ptr %elemfield10, align 8
  store i64 0, ptr %_3, align 8
  store i64 0, ptr %_4, align 8
  br label %bb1

bb1:                                              ; preds = %bb3, %bb0
  %load = load i64, ptr %_4, align 8
  %lt = icmp slt i64 %load, 2
  br i1 %lt, label %bb2, label %bb4

bb2:                                              ; preds = %bb1
  %load11 = load i64, ptr %_4, align 8
  %elemfield12 = getelementptr inbounds [2 x %CacheLine], ptr %_1, i64 0, i64 %load11, i32 0
  %idxfld = load i64, ptr %elemfield12, align 8
  %load13 = load i64, ptr %_3, align 8
  %add = add i64 %load13, %idxfld
  store i64 %add, ptr %_3, align 8
  br label %bb3

bb3:                                              ; preds = %bb2
  %load14 = load i64, ptr %_4, align 8
  %add15 = add i64 %load14, 1
  store i64 %add15, ptr %_4, align 8
  br label %bb1

bb4:                                              ; preds = %bb1
  %load16 = load i64, ptr %_3, align 8
  store i64 %load16, ptr %_2, align 8
  %fldptr17 = getelementptr inbounds %CacheLine, ptr %_0, i32 0, i32 0
  %fld = load i64, ptr %fldptr17, align 8
  %fldptr18 = getelementptr inbounds %CacheLine, ptr %_0, i32 0, i32 1
  %fld19 = load i64, ptr %fldptr18, align 8
  %add20 = add i64 %fld, %fld19
  %fldptr21 = getelementptr inbounds %CacheLine, ptr %_0, i32 0, i32 2
  %fld22 = load i64, ptr %fldptr21, align 8
  %add23 = add i64 %add20, %fld22
  %fldptr24 = getelementptr inbounds %CacheLine, ptr %_0, i32 0, i32 3
  %fld25 = load i64, ptr %fldptr24, align 8
  %add26 = add i64 %add23, %fld25
  %load27 = load i64, ptr %_2, align 8
  %add28 = add i64 %add26, %load27
  %cast = trunc i64 %add28 to i32
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
