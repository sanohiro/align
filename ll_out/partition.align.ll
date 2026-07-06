; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

; Function Attrs: nounwind
define i1 @is_even(i64 %0) #0 {
bb0:
  %_0 = alloca i64, align 8
  store i64 %0, ptr %_0, align 8
  %load = load i64, ptr %_0, align 8
  %srem = srem i64 %load, 2
  %eq = icmp eq i64 %srem, 0
  ret i1 %eq
}

; Function Attrs: nounwind
define { i8, i32, { i32, i32 } } @align_main() #0 {
bb0:
  %_0 = alloca { ptr, i64 }, align 8
  %_1 = alloca { ptr, i64 }, align 8
  %_2 = alloca [5 x i64], align 8
  %_3 = alloca i64, align 8
  %_4 = alloca i64, align 8
  %_5 = alloca i64, align 8
  %_6 = alloca i64, align 8
  %_7 = alloca i64, align 8
  %_8 = alloca i64, align 8
  %_9 = alloca i64, align 8
  store { ptr, i64 } zeroinitializer, ptr %_0, align 8
  store { ptr, i64 } zeroinitializer, ptr %_1, align 8
  %elemptr = getelementptr inbounds [5 x i64], ptr %_2, i64 0, i64 0
  store i64 1, ptr %elemptr, align 8
  %elemptr1 = getelementptr inbounds [5 x i64], ptr %_2, i64 0, i64 1
  store i64 2, ptr %elemptr1, align 8
  %elemptr2 = getelementptr inbounds [5 x i64], ptr %_2, i64 0, i64 2
  store i64 3, ptr %elemptr2, align 8
  %elemptr3 = getelementptr inbounds [5 x i64], ptr %_2, i64 0, i64 3
  store i64 4, ptr %elemptr3, align 8
  %elemptr4 = getelementptr inbounds [5 x i64], ptr %_2, i64 0, i64 4
  store i64 5, ptr %elemptr4, align 8
  %buf = call ptr @align_rt_alloc(i64 40)
  %buf5 = call ptr @align_rt_alloc(i64 40)
  store i64 0, ptr %_3, align 8
  store i64 0, ptr %_4, align 8
  store i64 0, ptr %_5, align 8
  br label %bb1

bb1:                                              ; preds = %bb3, %bb0
  %load = load i64, ptr %_5, align 8
  %lt = icmp slt i64 %load, 5
  br i1 %lt, label %bb2, label %bb4

bb2:                                              ; preds = %bb1
  %load6 = load i64, ptr %_5, align 8
  %elemptr7 = getelementptr inbounds [5 x i64], ptr %_2, i64 0, i64 %load6
  %idx = load i64, ptr %elemptr7, align 8
  %call = call i1 @is_even(i64 %idx)
  br i1 %call, label %bb5, label %bb6

bb3:                                              ; preds = %bb6, %bb5
  %load8 = load i64, ptr %_5, align 8
  %add = add i64 %load8, 1
  store i64 %add, ptr %_5, align 8
  br label %bb1

bb4:                                              ; preds = %bb1
  %load9 = load i64, ptr %_3, align 8
  %arrptr = insertvalue { ptr, i64 } poison, ptr %buf, 0
  %arrlen = insertvalue { ptr, i64 } %arrptr, i64 %load9, 1
  %load10 = load i64, ptr %_4, align 8
  %arrptr11 = insertvalue { ptr, i64 } poison, ptr %buf5, 0
  %arrlen12 = insertvalue { ptr, i64 } %arrptr11, i64 %load10, 1
  %tup = insertvalue { { ptr, i64 }, { ptr, i64 } } poison, { ptr, i64 } %arrlen, 0
  %tup13 = insertvalue { { ptr, i64 }, { ptr, i64 } } %tup, { ptr, i64 } %arrlen12, 1
  %tupidx = extractvalue { { ptr, i64 }, { ptr, i64 } } %tup13, 0
  store { ptr, i64 } %tupidx, ptr %_0, align 8
  %tupidx14 = extractvalue { { ptr, i64 }, { ptr, i64 } } %tup13, 1
  store { ptr, i64 } %tupidx14, ptr %_1, align 8
  %load15 = load { ptr, i64 }, ptr %_0, align 8
  %len = extractvalue { ptr, i64 } %load15, 1
  store i64 0, ptr %_6, align 8
  store i64 0, ptr %_7, align 8
  br label %bb7

bb5:                                              ; preds = %bb2
  %load16 = load i64, ptr %_3, align 8
  %ptrstore = getelementptr inbounds i64, ptr %buf, i64 %load16
  store i64 %idx, ptr %ptrstore, align 8
  %add17 = add i64 %load16, 1
  store i64 %add17, ptr %_3, align 8
  br label %bb3

bb6:                                              ; preds = %bb2
  %load18 = load i64, ptr %_4, align 8
  %ptrstore19 = getelementptr inbounds i64, ptr %buf5, i64 %load18
  store i64 %idx, ptr %ptrstore19, align 8
  %add20 = add i64 %load18, 1
  store i64 %add20, ptr %_4, align 8
  br label %bb3

bb7:                                              ; preds = %bb9, %bb4
  %load21 = load i64, ptr %_7, align 8
  %lt22 = icmp slt i64 %load21, %len
  br i1 %lt22, label %bb8, label %bb10

bb8:                                              ; preds = %bb7
  %load23 = load i64, ptr %_7, align 8
  %ptr = extractvalue { ptr, i64 } %load15, 0
  %slcidx = getelementptr inbounds i64, ptr %ptr, i64 %load23
  %slcload = load i64, ptr %slcidx, align 8
  %load24 = load i64, ptr %_6, align 8
  %add25 = add i64 %load24, %slcload
  store i64 %add25, ptr %_6, align 8
  br label %bb9

bb9:                                              ; preds = %bb8
  %load26 = load i64, ptr %_7, align 8
  %add27 = add i64 %load26, 1
  store i64 %add27, ptr %_7, align 8
  br label %bb7

bb10:                                             ; preds = %bb7
  %load28 = load i64, ptr %_6, align 8
  call void @align_rt_print_i64(i64 %load28)
  %load29 = load { ptr, i64 }, ptr %_1, align 8
  %len30 = extractvalue { ptr, i64 } %load29, 1
  store i64 0, ptr %_8, align 8
  store i64 0, ptr %_9, align 8
  br label %bb11

bb11:                                             ; preds = %bb13, %bb10
  %load31 = load i64, ptr %_9, align 8
  %lt32 = icmp slt i64 %load31, %len30
  br i1 %lt32, label %bb12, label %bb14

bb12:                                             ; preds = %bb11
  %load33 = load i64, ptr %_9, align 8
  %ptr34 = extractvalue { ptr, i64 } %load29, 0
  %slcidx35 = getelementptr inbounds i64, ptr %ptr34, i64 %load33
  %slcload36 = load i64, ptr %slcidx35, align 8
  %load37 = load i64, ptr %_8, align 8
  %add38 = add i64 %load37, %slcload36
  store i64 %add38, ptr %_8, align 8
  br label %bb13

bb13:                                             ; preds = %bb12
  %load39 = load i64, ptr %_9, align 8
  %add40 = add i64 %load39, 1
  store i64 %add40, ptr %_9, align 8
  br label %bb11

bb14:                                             ; preds = %bb11
  %load41 = load i64, ptr %_8, align 8
  call void @align_rt_print_i64(i64 %load41)
  %drop = load { ptr, i64 }, ptr %_0, align 8
  %dropptr = extractvalue { ptr, i64 } %drop, 0
  call void @align_rt_free(ptr %dropptr)
  %drop42 = load { ptr, i64 }, ptr %_1, align 8
  %dropptr43 = extractvalue { ptr, i64 } %drop42, 0
  call void @align_rt_free(ptr %dropptr43)
  ret { i8, i32, { i32, i32 } } zeroinitializer
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

declare i32 @align_rt_report_error(i32)

; Function Attrs: nounwind
define i32 @main() #0 {
entry:
  %r = call { i8, i32, { i32, i32 } } @align_main()
  %tag = extractvalue { i8, i32, { i32, i32 } } %r, 0
  %iserr = icmp ne i8 %tag, 0
  br i1 %iserr, label %err, label %ok

err:                                              ; preds = %entry
  %err1 = extractvalue { i8, i32, { i32, i32 } } %r, 2
  %etag = extractvalue { i32, i32 } %err1, 0
  %ecode = extractvalue { i32, i32 } %err1, 1
  %iscode = icmp eq i32 %etag, 3
  %catcode = add i32 %etag, 1
  %exitcode = select i1 %iscode, i32 %ecode, i32 %catcode
  %exit = call i32 @align_rt_report_error(i32 %exitcode)
  ret i32 %exit

ok:                                               ; preds = %entry
  ret i32 0
}

attributes #0 = { nounwind }
attributes #1 = { nofree nounwind }
