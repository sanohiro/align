; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

; Function Attrs: nounwind
define { i8, i32, { i32, i32 } } @align_main() #0 {
bb0:
  %_0 = alloca { ptr, i64 }, align 8
  %_1 = alloca { ptr, i64 }, align 8
  %_2 = alloca [4 x i64], align 8
  %_3 = alloca i64, align 8
  %_4 = alloca i64, align 8
  %_5 = alloca i64, align 8
  %_6 = alloca i64, align 8
  %_7 = alloca [5 x i64], align 8
  %_8 = alloca i64, align 8
  %_9 = alloca i64, align 8
  %_10 = alloca i64, align 8
  %_11 = alloca i64, align 8
  %arena = call ptr @align_rt_arena_begin()
  %elemptr = getelementptr inbounds [4 x i64], ptr %_2, i64 0, i64 0
  store i64 10, ptr %elemptr, align 8
  %elemptr1 = getelementptr inbounds [4 x i64], ptr %_2, i64 0, i64 1
  store i64 21, ptr %elemptr1, align 8
  %elemptr2 = getelementptr inbounds [4 x i64], ptr %_2, i64 0, i64 2
  store i64 32, ptr %elemptr2, align 8
  %elemptr3 = getelementptr inbounds [4 x i64], ptr %_2, i64 0, i64 3
  store i64 3, ptr %elemptr3, align 8
  %buf = call ptr @align_rt_arena_alloc(ptr %arena, i64 32, i64 8)
  store i64 0, ptr %_3, align 8
  store i64 0, ptr %_4, align 8
  br label %bb1

bb1:                                              ; preds = %bb3, %bb0
  %load = load i64, ptr %_4, align 8
  %lt = icmp slt i64 %load, 4
  br i1 %lt, label %bb2, label %bb4

bb2:                                              ; preds = %bb1
  %load4 = load i64, ptr %_4, align 8
  %elemptr5 = getelementptr inbounds [4 x i64], ptr %_2, i64 0, i64 %load4
  %idx = load i64, ptr %elemptr5, align 8
  %load6 = load i64, ptr %_3, align 8
  %ptrstore = getelementptr inbounds i64, ptr %buf, i64 %load6
  store i64 %idx, ptr %ptrstore, align 8
  %add = add i64 %load6, 1
  store i64 %add, ptr %_3, align 8
  br label %bb3

bb3:                                              ; preds = %bb2
  %load7 = load i64, ptr %_4, align 8
  %add8 = add i64 %load7, 1
  store i64 %add8, ptr %_4, align 8
  br label %bb1

bb4:                                              ; preds = %bb1
  %load9 = load i64, ptr %_3, align 8
  %arrptr = insertvalue { ptr, i64 } poison, ptr %buf, 0
  %arrlen = insertvalue { ptr, i64 } %arrptr, i64 %load9, 1
  %ptr = extractvalue { ptr, i64 } %arrlen, 0
  %len = extractvalue { ptr, i64 } %arrlen, 1
  store i64 1, ptr %_5, align 8
  br label %bb5

bb5:                                              ; preds = %bb10, %bb4
  %load10 = load i64, ptr %_5, align 8
  %lt11 = icmp slt i64 %load10, %len
  br i1 %lt11, label %bb6, label %bb11

bb6:                                              ; preds = %bb5
  %load12 = load i64, ptr %_5, align 8
  %ptr13 = extractvalue { ptr, i64 } %arrlen, 0
  %slcidx = getelementptr inbounds i64, ptr %ptr13, i64 %load12
  %slcload = load i64, ptr %slcidx, align 8
  %call = call i64 @"main$lambda0"(i64 %slcload)
  %sub = sub i64 %load12, 1
  store i64 %sub, ptr %_6, align 8
  br label %bb7

bb7:                                              ; preds = %bb9, %bb6
  %load14 = load i64, ptr %_6, align 8
  %ge = icmp sge i64 %load14, 0
  br i1 %ge, label %bb8, label %bb10

bb8:                                              ; preds = %bb7
  %ptr15 = extractvalue { ptr, i64 } %arrlen, 0
  %slcidx16 = getelementptr inbounds i64, ptr %ptr15, i64 %load14
  %slcload17 = load i64, ptr %slcidx16, align 8
  %call18 = call i64 @"main$lambda0"(i64 %slcload17)
  %gt = icmp sgt i64 %call18, %call
  br i1 %gt, label %bb9, label %bb10

bb9:                                              ; preds = %bb8
  %add19 = add i64 %load14, 1
  %ptrstore20 = getelementptr inbounds i64, ptr %ptr, i64 %add19
  store i64 %slcload17, ptr %ptrstore20, align 8
  %sub21 = sub i64 %load14, 1
  store i64 %sub21, ptr %_6, align 8
  br label %bb7

bb10:                                             ; preds = %bb8, %bb7
  %add22 = add i64 %load14, 1
  %ptrstore23 = getelementptr inbounds i64, ptr %ptr, i64 %add22
  store i64 %slcload, ptr %ptrstore23, align 8
  %add24 = add i64 %load12, 1
  store i64 %add24, ptr %_5, align 8
  br label %bb5

bb11:                                             ; preds = %bb5
  store { ptr, i64 } %arrlen, ptr %_0, align 8
  %load25 = load { ptr, i64 }, ptr %_0, align 8
  %len26 = extractvalue { ptr, i64 } %load25, 1
  %ge27 = icmp sge i64 0, %len26
  %or = or i1 false, %ge27
  br i1 %or, label %bb12, label %bb13

bb12:                                             ; preds = %bb11
  call void @align_rt_bounds_fail(i64 0, i64 %len26)
  unreachable

bb13:                                             ; preds = %bb11
  %ptr28 = extractvalue { ptr, i64 } %load25, 0
  %slcidx29 = getelementptr inbounds i64, ptr %ptr28, i64 0
  %slcload30 = load i64, ptr %slcidx29, align 8
  call void @align_rt_print_i64(i64 %slcload30)
  %load31 = load { ptr, i64 }, ptr %_0, align 8
  %len32 = extractvalue { ptr, i64 } %load31, 1
  %ge33 = icmp sge i64 3, %len32
  %or34 = or i1 false, %ge33
  br i1 %or34, label %bb14, label %bb15

bb14:                                             ; preds = %bb13
  call void @align_rt_bounds_fail(i64 3, i64 %len32)
  unreachable

bb15:                                             ; preds = %bb13
  %ptr35 = extractvalue { ptr, i64 } %load31, 0
  %slcidx36 = getelementptr inbounds i64, ptr %ptr35, i64 3
  %slcload37 = load i64, ptr %slcidx36, align 8
  call void @align_rt_print_i64(i64 %slcload37)
  %elemptr38 = getelementptr inbounds [5 x i64], ptr %_7, i64 0, i64 0
  store i64 3, ptr %elemptr38, align 8
  %elemptr39 = getelementptr inbounds [5 x i64], ptr %_7, i64 0, i64 1
  store i64 1, ptr %elemptr39, align 8
  %elemptr40 = getelementptr inbounds [5 x i64], ptr %_7, i64 0, i64 2
  store i64 4, ptr %elemptr40, align 8
  %elemptr41 = getelementptr inbounds [5 x i64], ptr %_7, i64 0, i64 3
  store i64 1, ptr %elemptr41, align 8
  %elemptr42 = getelementptr inbounds [5 x i64], ptr %_7, i64 0, i64 4
  store i64 5, ptr %elemptr42, align 8
  %buf43 = call ptr @align_rt_arena_alloc(ptr %arena, i64 40, i64 8)
  store i64 0, ptr %_8, align 8
  store i64 0, ptr %_9, align 8
  br label %bb16

bb16:                                             ; preds = %bb18, %bb15
  %load44 = load i64, ptr %_9, align 8
  %lt45 = icmp slt i64 %load44, 5
  br i1 %lt45, label %bb17, label %bb19

bb17:                                             ; preds = %bb16
  %load46 = load i64, ptr %_9, align 8
  %elemptr47 = getelementptr inbounds [5 x i64], ptr %_7, i64 0, i64 %load46
  %idx48 = load i64, ptr %elemptr47, align 8
  %load49 = load i64, ptr %_8, align 8
  %ptrstore50 = getelementptr inbounds i64, ptr %buf43, i64 %load49
  store i64 %idx48, ptr %ptrstore50, align 8
  %add51 = add i64 %load49, 1
  store i64 %add51, ptr %_8, align 8
  br label %bb18

bb18:                                             ; preds = %bb17
  %load52 = load i64, ptr %_9, align 8
  %add53 = add i64 %load52, 1
  store i64 %add53, ptr %_9, align 8
  br label %bb16

bb19:                                             ; preds = %bb16
  %load54 = load i64, ptr %_8, align 8
  %arrptr55 = insertvalue { ptr, i64 } poison, ptr %buf43, 0
  %arrlen56 = insertvalue { ptr, i64 } %arrptr55, i64 %load54, 1
  %ptr57 = extractvalue { ptr, i64 } %arrlen56, 0
  %len58 = extractvalue { ptr, i64 } %arrlen56, 1
  store i64 1, ptr %_10, align 8
  br label %bb20

bb20:                                             ; preds = %bb25, %bb19
  %load59 = load i64, ptr %_10, align 8
  %lt60 = icmp slt i64 %load59, %len58
  br i1 %lt60, label %bb21, label %bb26

bb21:                                             ; preds = %bb20
  %load61 = load i64, ptr %_10, align 8
  %ptr62 = extractvalue { ptr, i64 } %arrlen56, 0
  %slcidx63 = getelementptr inbounds i64, ptr %ptr62, i64 %load61
  %slcload64 = load i64, ptr %slcidx63, align 8
  %call65 = call i64 @"main$lambda1"(i64 %slcload64)
  %sub66 = sub i64 %load61, 1
  store i64 %sub66, ptr %_11, align 8
  br label %bb22

bb22:                                             ; preds = %bb24, %bb21
  %load67 = load i64, ptr %_11, align 8
  %ge68 = icmp sge i64 %load67, 0
  br i1 %ge68, label %bb23, label %bb25

bb23:                                             ; preds = %bb22
  %ptr69 = extractvalue { ptr, i64 } %arrlen56, 0
  %slcidx70 = getelementptr inbounds i64, ptr %ptr69, i64 %load67
  %slcload71 = load i64, ptr %slcidx70, align 8
  %call72 = call i64 @"main$lambda1"(i64 %slcload71)
  %gt73 = icmp sgt i64 %call72, %call65
  br i1 %gt73, label %bb24, label %bb25

bb24:                                             ; preds = %bb23
  %add74 = add i64 %load67, 1
  %ptrstore75 = getelementptr inbounds i64, ptr %ptr57, i64 %add74
  store i64 %slcload71, ptr %ptrstore75, align 8
  %sub76 = sub i64 %load67, 1
  store i64 %sub76, ptr %_11, align 8
  br label %bb22

bb25:                                             ; preds = %bb23, %bb22
  %add77 = add i64 %load67, 1
  %ptrstore78 = getelementptr inbounds i64, ptr %ptr57, i64 %add77
  store i64 %slcload64, ptr %ptrstore78, align 8
  %add79 = add i64 %load61, 1
  store i64 %add79, ptr %_10, align 8
  br label %bb20

bb26:                                             ; preds = %bb20
  store { ptr, i64 } %arrlen56, ptr %_1, align 8
  %load80 = load { ptr, i64 }, ptr %_1, align 8
  %len81 = extractvalue { ptr, i64 } %load80, 1
  %ge82 = icmp sge i64 0, %len81
  %or83 = or i1 false, %ge82
  br i1 %or83, label %bb27, label %bb28

bb27:                                             ; preds = %bb26
  call void @align_rt_bounds_fail(i64 0, i64 %len81)
  unreachable

bb28:                                             ; preds = %bb26
  %ptr84 = extractvalue { ptr, i64 } %load80, 0
  %slcidx85 = getelementptr inbounds i64, ptr %ptr84, i64 0
  %slcload86 = load i64, ptr %slcidx85, align 8
  call void @align_rt_print_i64(i64 %slcload86)
  call void @align_rt_arena_end(ptr %arena)
  ret { i8, i32, { i32, i32 } } zeroinitializer
}

; Function Attrs: nounwind
define i64 @"main$lambda0"(i64 %0) #0 {
bb0:
  %_0 = alloca i64, align 8
  store i64 %0, ptr %_0, align 8
  %load = load i64, ptr %_0, align 8
  %srem = srem i64 %load, 10
  ret i64 %srem
}

; Function Attrs: nounwind
define i64 @"main$lambda1"(i64 %0) #0 {
bb0:
  %_0 = alloca i64, align 8
  store i64 %0, ptr %_0, align 8
  %load = load i64, ptr %_0, align 8
  %neg = sub i64 0, %load
  ret i64 %neg
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
