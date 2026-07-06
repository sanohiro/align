; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

@str = constant [1 x i8] c"\0A"

; Function Attrs: nounwind
define { i8, i32, { i32, i32 } } @align_main({ ptr, i64 } %0) #0 {
bb0:
  %_0 = alloca { ptr, i64 }, align 8
  %_1 = alloca ptr, align 8
  %_2 = alloca ptr, align 8
  %_3 = alloca i64, align 8
  %_4 = alloca ptr, align 8
  %_5 = alloca { i8, ptr, { i32, i32 } }, align 8
  %_6 = alloca ptr, align 8
  %_7 = alloca { i8, ptr, { i32, i32 } }, align 8
  %_8 = alloca { i8, i64, { i32, i32 } }, align 8
  %_9 = alloca { i8, i32, { i32, i32 } }, align 4
  store { ptr, i64 } %0, ptr %_0, align 8
  store ptr null, ptr %_1, align 8
  store ptr null, ptr %_2, align 8
  %load = load { ptr, i64 }, ptr %_0, align 8
  %len = extractvalue { ptr, i64 } %load, 1
  %ge = icmp sge i64 1, %len
  %or = or i1 false, %ge
  br i1 %or, label %bb1, label %bb2

bb1:                                              ; preds = %bb0
  call void @align_rt_bounds_fail(i64 1, i64 %len)
  unreachable

bb2:                                              ; preds = %bb0
  %ptr = extractvalue { ptr, i64 } %load, 0
  %slcidx = getelementptr inbounds { ptr, i64 }, ptr %ptr, i64 1
  %slcload = load { ptr, i64 }, ptr %slcidx, align 8
  store ptr null, ptr %_4, align 8
  %path_p = extractvalue { ptr, i64 } %slcload, 0
  %path_l = extractvalue { ptr, i64 } %slcload, 1
  %open = call i32 @align_rt_io_reader_open(ptr %path_p, i64 %path_l, ptr %_4)
  %eq = icmp eq i32 %open, 0
  br i1 %eq, label %bb3, label %bb4

bb3:                                              ; preds = %bb2
  %load1 = load ptr, ptr %_4, align 8
  %ok = insertvalue { i8, ptr, { i32, i32 } } zeroinitializer, ptr %load1, 1
  store { i8, ptr, { i32, i32 } } %ok, ptr %_5, align 8
  br label %bb5

bb4:                                              ; preds = %bb2
  %sub = sub i32 %open, 1
  %ge2 = icmp sge i32 %sub, 3
  %sel = select i1 %ge2, i32 3, i32 %sub
  %sub3 = sub i32 %open, 4
  %ge4 = icmp sge i32 %sub3, 0
  %sel5 = select i1 %ge4, i32 %sub3, i32 0
  %etag = insertvalue { i32, i32 } zeroinitializer, i32 %sel, 0
  %ecode = insertvalue { i32, i32 } %etag, i32 %sel5, 1
  %err = insertvalue { i8, ptr, { i32, i32 } } { i8 1, ptr null, { i32, i32 } zeroinitializer }, { i32, i32 } %ecode, 2
  store { i8, ptr, { i32, i32 } } %err, ptr %_5, align 8
  br label %bb5

bb5:                                              ; preds = %bb4, %bb3
  %load6 = load { i8, ptr, { i32, i32 } }, ptr %_5, align 8
  %tag = extractvalue { i8, ptr, { i32, i32 } } %load6, 0
  %isok = icmp eq i8 %tag, 0
  br i1 %isok, label %bb6, label %bb7

bb6:                                              ; preds = %bb5
  %ok7 = extractvalue { i8, ptr, { i32, i32 } } %load6, 1
  store ptr %ok7, ptr %_1, align 8
  %load8 = load { ptr, i64 }, ptr %_0, align 8
  %len9 = extractvalue { ptr, i64 } %load8, 1
  %ge10 = icmp sge i64 2, %len9
  %or11 = or i1 false, %ge10
  br i1 %or11, label %bb8, label %bb9

bb7:                                              ; preds = %bb5
  %err12 = extractvalue { i8, ptr, { i32, i32 } } %load6, 2
  %err13 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %err12, 2
  %drop = load { ptr, i64 }, ptr %_0, align 8
  %dropptr = extractvalue { ptr, i64 } %drop, 0
  call void @align_rt_free(ptr %dropptr)
  %droph = load ptr, ptr %_1, align 8
  call void @align_rt_io_reader_free(ptr %droph)
  %droph14 = load ptr, ptr %_2, align 8
  call void @align_rt_io_writer_free(ptr %droph14)
  ret { i8, i32, { i32, i32 } } %err13

bb8:                                              ; preds = %bb6
  call void @align_rt_bounds_fail(i64 2, i64 %len9)
  unreachable

bb9:                                              ; preds = %bb6
  %ptr15 = extractvalue { ptr, i64 } %load8, 0
  %slcidx16 = getelementptr inbounds { ptr, i64 }, ptr %ptr15, i64 2
  %slcload17 = load { ptr, i64 }, ptr %slcidx16, align 8
  store ptr null, ptr %_6, align 8
  %path_p18 = extractvalue { ptr, i64 } %slcload17, 0
  %path_l19 = extractvalue { ptr, i64 } %slcload17, 1
  %open20 = call i32 @align_rt_io_writer_create(ptr %path_p18, i64 %path_l19, ptr %_6)
  %eq21 = icmp eq i32 %open20, 0
  br i1 %eq21, label %bb10, label %bb11

bb10:                                             ; preds = %bb9
  %load22 = load ptr, ptr %_6, align 8
  %ok23 = insertvalue { i8, ptr, { i32, i32 } } zeroinitializer, ptr %load22, 1
  store { i8, ptr, { i32, i32 } } %ok23, ptr %_7, align 8
  br label %bb12

bb11:                                             ; preds = %bb9
  %sub24 = sub i32 %open20, 1
  %ge25 = icmp sge i32 %sub24, 3
  %sel26 = select i1 %ge25, i32 3, i32 %sub24
  %sub27 = sub i32 %open20, 4
  %ge28 = icmp sge i32 %sub27, 0
  %sel29 = select i1 %ge28, i32 %sub27, i32 0
  %etag30 = insertvalue { i32, i32 } zeroinitializer, i32 %sel26, 0
  %ecode31 = insertvalue { i32, i32 } %etag30, i32 %sel29, 1
  %err32 = insertvalue { i8, ptr, { i32, i32 } } { i8 1, ptr null, { i32, i32 } zeroinitializer }, { i32, i32 } %ecode31, 2
  store { i8, ptr, { i32, i32 } } %err32, ptr %_7, align 8
  br label %bb12

bb12:                                             ; preds = %bb11, %bb10
  %load33 = load { i8, ptr, { i32, i32 } }, ptr %_7, align 8
  %tag34 = extractvalue { i8, ptr, { i32, i32 } } %load33, 0
  %isok35 = icmp eq i8 %tag34, 0
  br i1 %isok35, label %bb13, label %bb14

bb13:                                             ; preds = %bb12
  %ok36 = extractvalue { i8, ptr, { i32, i32 } } %load33, 1
  store ptr %ok36, ptr %_2, align 8
  %load37 = load ptr, ptr %_1, align 8
  %load38 = load ptr, ptr %_2, align 8
  %copy = call i64 @align_rt_io_copy(ptr %load37, ptr %load38)
  %ge39 = icmp sge i64 %copy, 0
  br i1 %ge39, label %bb15, label %bb16

bb14:                                             ; preds = %bb12
  %err40 = extractvalue { i8, ptr, { i32, i32 } } %load33, 2
  %err41 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %err40, 2
  %drop42 = load { ptr, i64 }, ptr %_0, align 8
  %dropptr43 = extractvalue { ptr, i64 } %drop42, 0
  call void @align_rt_free(ptr %dropptr43)
  %droph44 = load ptr, ptr %_1, align 8
  call void @align_rt_io_reader_free(ptr %droph44)
  %droph45 = load ptr, ptr %_2, align 8
  call void @align_rt_io_writer_free(ptr %droph45)
  ret { i8, i32, { i32, i32 } } %err41

bb15:                                             ; preds = %bb13
  %ok46 = insertvalue { i8, i64, { i32, i32 } } zeroinitializer, i64 %copy, 1
  store { i8, i64, { i32, i32 } } %ok46, ptr %_8, align 8
  br label %bb17

bb16:                                             ; preds = %bb13
  %sub47 = sub i64 0, %copy
  %cast = trunc i64 %sub47 to i32
  %sub48 = sub i32 %cast, 1
  %ge49 = icmp sge i32 %sub48, 3
  %sel50 = select i1 %ge49, i32 3, i32 %sub48
  %sub51 = sub i32 %cast, 4
  %ge52 = icmp sge i32 %sub51, 0
  %sel53 = select i1 %ge52, i32 %sub51, i32 0
  %etag54 = insertvalue { i32, i32 } zeroinitializer, i32 %sel50, 0
  %ecode55 = insertvalue { i32, i32 } %etag54, i32 %sel53, 1
  %err56 = insertvalue { i8, i64, { i32, i32 } } { i8 1, i64 0, { i32, i32 } zeroinitializer }, { i32, i32 } %ecode55, 2
  store { i8, i64, { i32, i32 } } %err56, ptr %_8, align 8
  br label %bb17

bb17:                                             ; preds = %bb16, %bb15
  %load57 = load { i8, i64, { i32, i32 } }, ptr %_8, align 8
  %tag58 = extractvalue { i8, i64, { i32, i32 } } %load57, 0
  %isok59 = icmp eq i8 %tag58, 0
  br i1 %isok59, label %bb18, label %bb19

bb18:                                             ; preds = %bb17
  %ok60 = extractvalue { i8, i64, { i32, i32 } } %load57, 1
  store i64 %ok60, ptr %_3, align 8
  %load61 = load ptr, ptr %_2, align 8
  %wr = call i32 @align_rt_io_writer_write(ptr %load61, ptr @str, i64 1)
  %eq62 = icmp eq i32 %wr, 0
  br i1 %eq62, label %bb20, label %bb21

bb19:                                             ; preds = %bb17
  %err63 = extractvalue { i8, i64, { i32, i32 } } %load57, 2
  %err64 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %err63, 2
  %drop65 = load { ptr, i64 }, ptr %_0, align 8
  %dropptr66 = extractvalue { ptr, i64 } %drop65, 0
  call void @align_rt_free(ptr %dropptr66)
  %droph67 = load ptr, ptr %_1, align 8
  call void @align_rt_io_reader_free(ptr %droph67)
  %droph68 = load ptr, ptr %_2, align 8
  call void @align_rt_io_writer_free(ptr %droph68)
  ret { i8, i32, { i32, i32 } } %err64

bb20:                                             ; preds = %bb18
  store { i8, i32, { i32, i32 } } zeroinitializer, ptr %_9, align 4
  br label %bb22

bb21:                                             ; preds = %bb18
  %sub69 = sub i32 %wr, 1
  %ge70 = icmp sge i32 %sub69, 3
  %sel71 = select i1 %ge70, i32 3, i32 %sub69
  %sub72 = sub i32 %wr, 4
  %ge73 = icmp sge i32 %sub72, 0
  %sel74 = select i1 %ge73, i32 %sub72, i32 0
  %etag75 = insertvalue { i32, i32 } zeroinitializer, i32 %sel71, 0
  %ecode76 = insertvalue { i32, i32 } %etag75, i32 %sel74, 1
  %err77 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %ecode76, 2
  store { i8, i32, { i32, i32 } } %err77, ptr %_9, align 4
  br label %bb22

bb22:                                             ; preds = %bb21, %bb20
  %load78 = load { i8, i32, { i32, i32 } }, ptr %_9, align 4
  %tag79 = extractvalue { i8, i32, { i32, i32 } } %load78, 0
  %isok80 = icmp eq i8 %tag79, 0
  br i1 %isok80, label %bb23, label %bb24

bb23:                                             ; preds = %bb22
  %ok81 = extractvalue { i8, i32, { i32, i32 } } %load78, 1
  %load82 = load i64, ptr %_3, align 8
  call void @align_rt_print_i64(i64 %load82)
  %drop83 = load { ptr, i64 }, ptr %_0, align 8
  %dropptr84 = extractvalue { ptr, i64 } %drop83, 0
  call void @align_rt_free(ptr %dropptr84)
  %droph85 = load ptr, ptr %_1, align 8
  call void @align_rt_io_reader_free(ptr %droph85)
  %droph86 = load ptr, ptr %_2, align 8
  call void @align_rt_io_writer_free(ptr %droph86)
  ret { i8, i32, { i32, i32 } } zeroinitializer

bb24:                                             ; preds = %bb22
  %err87 = extractvalue { i8, i32, { i32, i32 } } %load78, 2
  %err88 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %err87, 2
  %drop89 = load { ptr, i64 }, ptr %_0, align 8
  %dropptr90 = extractvalue { ptr, i64 } %drop89, 0
  call void @align_rt_free(ptr %dropptr90)
  %droph91 = load ptr, ptr %_1, align 8
  call void @align_rt_io_reader_free(ptr %droph91)
  %droph92 = load ptr, ptr %_2, align 8
  call void @align_rt_io_writer_free(ptr %droph92)
  ret { i8, i32, { i32, i32 } } %err88
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
define i32 @main(i32 %0, ptr %1) #0 {
entry:
  %args = call { ptr, i64 } @align_rt_args_build(i32 %0, ptr %1)
  %r = call { i8, i32, { i32, i32 } } @align_main({ ptr, i64 } %args)
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

declare { ptr, i64 } @align_rt_args_build(i32, ptr)

attributes #0 = { nounwind }
attributes #1 = { nofree nounwind }
