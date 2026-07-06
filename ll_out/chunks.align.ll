; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

; Function Attrs: nounwind
define { i8, i32, { i32, i32 } } @align_main() #0 {
bb0:
  %_0 = alloca [5 x i64], align 8
  %_1 = alloca { ptr, i64 }, align 8
  %_2 = alloca i64, align 8
  %_3 = alloca i64, align 8
  %_4 = alloca i64, align 8
  %_5 = alloca i64, align 8
  store { ptr, i64 } zeroinitializer, ptr %_1, align 8
  %elemptr = getelementptr inbounds [5 x i64], ptr %_0, i64 0, i64 0
  store i64 1, ptr %elemptr, align 8
  %elemptr1 = getelementptr inbounds [5 x i64], ptr %_0, i64 0, i64 1
  store i64 2, ptr %elemptr1, align 8
  %elemptr2 = getelementptr inbounds [5 x i64], ptr %_0, i64 0, i64 2
  store i64 3, ptr %elemptr2, align 8
  %elemptr3 = getelementptr inbounds [5 x i64], ptr %_0, i64 0, i64 3
  store i64 4, ptr %elemptr3, align 8
  %elemptr4 = getelementptr inbounds [5 x i64], ptr %_0, i64 0, i64 4
  store i64 5, ptr %elemptr4, align 8
  %slcbase = getelementptr inbounds [5 x i64], ptr %_0, i64 0, i64 0
  %slcptr = insertvalue { ptr, i64 } poison, ptr %slcbase, 0
  %slclen = insertvalue { ptr, i64 } %slcptr, i64 5, 1
  %srcptr = extractvalue { ptr, i64 } %slclen, 0
  %srclen = extractvalue { ptr, i64 } %slclen, 1
  %chunks = call { ptr, i64 } @align_rt_chunks(ptr %srcptr, i64 %srclen, i64 2, i64 8)
  store { ptr, i64 } %chunks, ptr %_1, align 8
  %load = load { ptr, i64 }, ptr %_1, align 8
  %len = extractvalue { ptr, i64 } %load, 1
  call void @align_rt_print_i64(i64 %len)
  %load5 = load { ptr, i64 }, ptr %_1, align 8
  %len6 = extractvalue { ptr, i64 } %load5, 1
  %ge = icmp sge i64 0, %len6
  %or = or i1 false, %ge
  br i1 %or, label %bb1, label %bb2

bb1:                                              ; preds = %bb0
  call void @align_rt_bounds_fail(i64 0, i64 %len6)
  unreachable

bb2:                                              ; preds = %bb0
  %ptr = extractvalue { ptr, i64 } %load5, 0
  %slcidx = getelementptr inbounds { ptr, i64 }, ptr %ptr, i64 0
  %slcload = load { ptr, i64 }, ptr %slcidx, align 8
  %len7 = extractvalue { ptr, i64 } %slcload, 1
  store i64 0, ptr %_2, align 8
  store i64 0, ptr %_3, align 8
  br label %bb3

bb3:                                              ; preds = %bb5, %bb2
  %load8 = load i64, ptr %_3, align 8
  %lt = icmp slt i64 %load8, %len7
  br i1 %lt, label %bb4, label %bb6

bb4:                                              ; preds = %bb3
  %load9 = load i64, ptr %_3, align 8
  %ptr10 = extractvalue { ptr, i64 } %slcload, 0
  %slcidx11 = getelementptr inbounds i64, ptr %ptr10, i64 %load9
  %slcload12 = load i64, ptr %slcidx11, align 8
  %load13 = load i64, ptr %_2, align 8
  %add = add i64 %load13, %slcload12
  store i64 %add, ptr %_2, align 8
  br label %bb5

bb5:                                              ; preds = %bb4
  %load14 = load i64, ptr %_3, align 8
  %add15 = add i64 %load14, 1
  store i64 %add15, ptr %_3, align 8
  br label %bb3

bb6:                                              ; preds = %bb3
  %load16 = load i64, ptr %_2, align 8
  call void @align_rt_print_i64(i64 %load16)
  %load17 = load { ptr, i64 }, ptr %_1, align 8
  %len18 = extractvalue { ptr, i64 } %load17, 1
  %ge19 = icmp sge i64 2, %len18
  %or20 = or i1 false, %ge19
  br i1 %or20, label %bb7, label %bb8

bb7:                                              ; preds = %bb6
  call void @align_rt_bounds_fail(i64 2, i64 %len18)
  unreachable

bb8:                                              ; preds = %bb6
  %ptr21 = extractvalue { ptr, i64 } %load17, 0
  %slcidx22 = getelementptr inbounds { ptr, i64 }, ptr %ptr21, i64 2
  %slcload23 = load { ptr, i64 }, ptr %slcidx22, align 8
  %len24 = extractvalue { ptr, i64 } %slcload23, 1
  store i64 0, ptr %_4, align 8
  store i64 0, ptr %_5, align 8
  br label %bb9

bb9:                                              ; preds = %bb11, %bb8
  %load25 = load i64, ptr %_5, align 8
  %lt26 = icmp slt i64 %load25, %len24
  br i1 %lt26, label %bb10, label %bb12

bb10:                                             ; preds = %bb9
  %load27 = load i64, ptr %_5, align 8
  %ptr28 = extractvalue { ptr, i64 } %slcload23, 0
  %slcidx29 = getelementptr inbounds i64, ptr %ptr28, i64 %load27
  %slcload30 = load i64, ptr %slcidx29, align 8
  %load31 = load i64, ptr %_4, align 8
  %add32 = add i64 %load31, %slcload30
  store i64 %add32, ptr %_4, align 8
  br label %bb11

bb11:                                             ; preds = %bb10
  %load33 = load i64, ptr %_5, align 8
  %add34 = add i64 %load33, 1
  store i64 %add34, ptr %_5, align 8
  br label %bb9

bb12:                                             ; preds = %bb9
  %load35 = load i64, ptr %_4, align 8
  call void @align_rt_print_i64(i64 %load35)
  %drop = load { ptr, i64 }, ptr %_1, align 8
  %dropptr = extractvalue { ptr, i64 } %drop, 0
  call void @align_rt_free(ptr %dropptr)
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
