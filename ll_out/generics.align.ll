; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

; Function Attrs: nounwind
define i32 @main() #0 {
bb0:
  %_0 = alloca i32, align 4
  %_1 = alloca i32, align 4
  %_2 = alloca i32, align 4
  %_3 = alloca i32, align 4
  %_4 = alloca i32, align 4
  %_5 = alloca { i8, i32 }, align 4
  %_6 = alloca i32, align 4
  %call = call i32 @"id$i32"(i32 5)
  store i32 %call, ptr %_0, align 4
  %call1 = call i32 @"pick$i32"(i32 10, i32 20)
  store i32 %call1, ptr %_1, align 4
  %call2 = call i32 @"wrap$i32"(i32 7)
  store i32 %call2, ptr %_2, align 4
  %call3 = call i32 @"add$i32"(i32 1, i32 2)
  store i32 %call3, ptr %_3, align 4
  %call4 = call i32 @"max$i32"(i32 4, i32 1)
  store i32 %call4, ptr %_4, align 4
  store { i8, i32 } { i8 1, i32 2 }, ptr %_5, align 4
  %load = load { i8, i32 }, ptr %_5, align 4
  %call5 = call i32 @"unwrap_or$i32"({ i8, i32 } %load, i32 0)
  store i32 %call5, ptr %_6, align 4
  %load6 = load i32, ptr %_0, align 4
  %load7 = load i32, ptr %_1, align 4
  %add = add i32 %load6, %load7
  %load8 = load i32, ptr %_2, align 4
  %add9 = add i32 %add, %load8
  %load10 = load i32, ptr %_3, align 4
  %add11 = add i32 %add9, %load10
  %load12 = load i32, ptr %_4, align 4
  %add13 = add i32 %add11, %load12
  %load14 = load i32, ptr %_6, align 4
  %add15 = add i32 %add13, %load14
  ret i32 %add15
}

; Function Attrs: nounwind
define i32 @"id$i32"(i32 %0) #0 {
bb0:
  %_0 = alloca i32, align 4
  store i32 %0, ptr %_0, align 4
  %load = load i32, ptr %_0, align 4
  ret i32 %load
}

; Function Attrs: nounwind
define i32 @"pick$i32"(i32 %0, i32 %1) #0 {
bb0:
  %_0 = alloca i32, align 4
  %_1 = alloca i32, align 4
  store i32 %0, ptr %_0, align 4
  store i32 %1, ptr %_1, align 4
  %load = load i32, ptr %_0, align 4
  ret i32 %load
}

; Function Attrs: nounwind
define i32 @"wrap$i32"(i32 %0) #0 {
bb0:
  %_0 = alloca i32, align 4
  store i32 %0, ptr %_0, align 4
  %load = load i32, ptr %_0, align 4
  %call = call i32 @"id$i32"(i32 %load)
  ret i32 %call
}

; Function Attrs: nounwind
define i32 @"add$i32"(i32 %0, i32 %1) #0 {
bb0:
  %_0 = alloca i32, align 4
  %_1 = alloca i32, align 4
  store i32 %0, ptr %_0, align 4
  store i32 %1, ptr %_1, align 4
  %load = load i32, ptr %_0, align 4
  %load1 = load i32, ptr %_1, align 4
  %add = add i32 %load, %load1
  ret i32 %add
}

; Function Attrs: nounwind
define i32 @"max$i32"(i32 %0, i32 %1) #0 {
bb0:
  %_0 = alloca i32, align 4
  %_1 = alloca i32, align 4
  %_2 = alloca i32, align 4
  store i32 %0, ptr %_0, align 4
  store i32 %1, ptr %_1, align 4
  %load = load i32, ptr %_0, align 4
  %load1 = load i32, ptr %_1, align 4
  %gt = icmp sgt i32 %load, %load1
  br i1 %gt, label %bb1, label %bb2

bb1:                                              ; preds = %bb0
  %load2 = load i32, ptr %_0, align 4
  store i32 %load2, ptr %_2, align 4
  br label %bb3

bb2:                                              ; preds = %bb0
  %load3 = load i32, ptr %_1, align 4
  store i32 %load3, ptr %_2, align 4
  br label %bb3

bb3:                                              ; preds = %bb2, %bb1
  %load4 = load i32, ptr %_2, align 4
  ret i32 %load4
}

; Function Attrs: nounwind
define i32 @"unwrap_or$i32"({ i8, i32 } %0, i32 %1) #0 {
bb0:
  %_0 = alloca { i8, i32 }, align 4
  %_1 = alloca i32, align 4
  %_2 = alloca i32, align 4
  store { i8, i32 } %0, ptr %_0, align 4
  store i32 %1, ptr %_1, align 4
  %load = load { i8, i32 }, ptr %_0, align 4
  %tag = extractvalue { i8, i32 } %load, 0
  %issome = icmp eq i8 %tag, 1
  br i1 %issome, label %bb1, label %bb2

bb1:                                              ; preds = %bb0
  %some = extractvalue { i8, i32 } %load, 1
  store i32 %some, ptr %_2, align 4
  br label %bb3

bb2:                                              ; preds = %bb0
  %load1 = load i32, ptr %_1, align 4
  store i32 %load1, ptr %_2, align 4
  br label %bb3

bb3:                                              ; preds = %bb2, %bb1
  %load2 = load i32, ptr %_2, align 4
  ret i32 %load2
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
