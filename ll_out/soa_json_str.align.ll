; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

@str = constant [120 x i8] c"[{\22name\22:\22alice\22,\22age\22:30,\22active\22:true},{\22name\22:\22bob\22,\22age\22:25,\22active\22:false},{\22name\22:\22carol\22,\22age\22:41,\22active\22:true}]"
@str.1 = constant [4 x i8] c"name"
@str.2 = constant [3 x i8] c"age"
@str.3 = constant [6 x i8] c"active"
@jfields = constant [3 x { ptr, i64, i32, i64 }] [{ ptr, i64, i32, i64 } { ptr @str.1, i64 4, i32 784, i64 0 }, { ptr, i64, i32, i64 } { ptr @str.2, i64 3, i32 65544, i64 16 }, { ptr, i64, i32, i64 } { ptr @str.3, i64 6, i32 257, i64 24 }]
@jphf = constant [4 x i32] [i32 1, i32 -1, i32 2, i32 0]

; Function Attrs: nounwind
define { i8, i32, { i32, i32 } } @align_main() #0 {
bb0:
  %_0 = alloca { ptr, i64 }, align 8
  %_1 = alloca { ptr, i64 }, align 8
  %_2 = alloca { ptr, i64 }, align 8
  %_3 = alloca { i8, { ptr, i64 }, { i32, i32 } }, align 8
  %_4 = alloca i64, align 8
  %_5 = alloca i64, align 8
  %_6 = alloca i64, align 8
  %_7 = alloca i64, align 8
  store { ptr, i64 } { ptr @str, i64 120 }, ptr %_0, align 8
  %arena = call ptr @align_rt_arena_begin()
  %load = load { ptr, i64 }, ptr %_0, align 8
  store { ptr, i64 } zeroinitializer, ptr %_2, align 8
  %jin_p = extractvalue { ptr, i64 } %load, 0
  %jin_l = extractvalue { ptr, i64 } %load, 1
  %jdecsoa = call i32 @align_rt_json_decode_soa(ptr %jin_p, i64 %jin_l, ptr @jfields, i64 3, ptr %arena, ptr %_2, ptr @jphf, i64 4, i64 1)
  %eq = icmp eq i32 %jdecsoa, 0
  br i1 %eq, label %bb1, label %bb2

bb1:                                              ; preds = %bb0
  %load1 = load { ptr, i64 }, ptr %_2, align 8
  %ok = insertvalue { i8, { ptr, i64 }, { i32, i32 } } zeroinitializer, { ptr, i64 } %load1, 1
  store { i8, { ptr, i64 }, { i32, i32 } } %ok, ptr %_3, align 8
  br label %bb3

bb2:                                              ; preds = %bb0
  %pl = insertvalue { i32, i32 } { i32 3, i32 0 }, i32 %jdecsoa, 1
  %err = insertvalue { i8, { ptr, i64 }, { i32, i32 } } { i8 1, { ptr, i64 } zeroinitializer, { i32, i32 } zeroinitializer }, { i32, i32 } %pl, 2
  store { i8, { ptr, i64 }, { i32, i32 } } %err, ptr %_3, align 8
  br label %bb3

bb3:                                              ; preds = %bb2, %bb1
  %load2 = load { i8, { ptr, i64 }, { i32, i32 } }, ptr %_3, align 8
  %tag = extractvalue { i8, { ptr, i64 }, { i32, i32 } } %load2, 0
  %isok = icmp eq i8 %tag, 0
  br i1 %isok, label %bb4, label %bb5

bb4:                                              ; preds = %bb3
  %ok3 = extractvalue { i8, { ptr, i64 }, { i32, i32 } } %load2, 1
  store { ptr, i64 } %ok3, ptr %_1, align 8
  %load4 = load { ptr, i64 }, ptr %_1, align 8
  %len = extractvalue { ptr, i64 } %load4, 1
  call void @align_rt_print_i64(i64 %len)
  %load5 = load { ptr, i64 }, ptr %_1, align 8
  %len6 = extractvalue { ptr, i64 } %load5, 1
  store i64 0, ptr %_4, align 8
  store i64 0, ptr %_5, align 8
  br label %bb6

bb5:                                              ; preds = %bb3
  %err7 = extractvalue { i8, { ptr, i64 }, { i32, i32 } } %load2, 2
  %err8 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %err7, 2
  call void @align_rt_arena_end(ptr %arena)
  ret { i8, i32, { i32, i32 } } %err8

bb6:                                              ; preds = %bb8, %bb4
  %load9 = load i64, ptr %_5, align 8
  %lt = icmp slt i64 %load9, %len6
  br i1 %lt, label %bb7, label %bb9

bb7:                                              ; preds = %bb6
  %load10 = load i64, ptr %_5, align 8
  %soaptr = extractvalue { ptr, i64 } %load5, 0
  %soalen = extractvalue { ptr, i64 } %load5, 1
  %coladv = mul i64 %soalen, 16
  %colend = add i64 0, %coladv
  %colbump = add i64 %colend, 7
  %colalign = and i64 %colbump, -8
  %colbase = getelementptr inbounds i8, ptr %soaptr, i64 %colalign
  %colelem = getelementptr inbounds i64, ptr %colbase, i64 %load10
  %idxcol = load i64, ptr %colelem, align 8
  %load11 = load i64, ptr %_4, align 8
  %add = add i64 %load11, %idxcol
  store i64 %add, ptr %_4, align 8
  br label %bb8

bb8:                                              ; preds = %bb7
  %load12 = load i64, ptr %_5, align 8
  %add13 = add i64 %load12, 1
  store i64 %add13, ptr %_5, align 8
  br label %bb6

bb9:                                              ; preds = %bb6
  %load14 = load i64, ptr %_4, align 8
  call void @align_rt_print_i64(i64 %load14)
  %load15 = load { ptr, i64 }, ptr %_1, align 8
  %len16 = extractvalue { ptr, i64 } %load15, 1
  %ge = icmp sge i64 0, %len16
  %or = or i1 false, %ge
  br i1 %or, label %bb10, label %bb11

bb10:                                             ; preds = %bb9
  call void @align_rt_bounds_fail(i64 0, i64 %len16)
  unreachable

bb11:                                             ; preds = %bb9
  %soaptr17 = extractvalue { ptr, i64 } %load15, 0
  %soalen18 = extractvalue { ptr, i64 } %load15, 1
  %colbase19 = getelementptr inbounds i8, ptr %soaptr17, i64 0
  %colelem20 = getelementptr inbounds { ptr, i64 }, ptr %colbase19, i64 0
  %idxcol21 = load { ptr, i64 }, ptr %colelem20, align 8
  %len22 = extractvalue { ptr, i64 } %idxcol21, 1
  call void @align_rt_print_i64(i64 %len22)
  %load23 = load { ptr, i64 }, ptr %_1, align 8
  %len24 = extractvalue { ptr, i64 } %load23, 1
  store i64 0, ptr %_6, align 8
  store i64 0, ptr %_7, align 8
  br label %bb12

bb12:                                             ; preds = %bb14, %bb11
  %load25 = load i64, ptr %_7, align 8
  %lt26 = icmp slt i64 %load25, %len24
  br i1 %lt26, label %bb13, label %bb15

bb13:                                             ; preds = %bb12
  %load27 = load i64, ptr %_7, align 8
  %soaptr28 = extractvalue { ptr, i64 } %load23, 0
  %soalen29 = extractvalue { ptr, i64 } %load23, 1
  %coladv30 = mul i64 %soalen29, 16
  %colend31 = add i64 0, %coladv30
  %colbump32 = add i64 %colend31, 7
  %colalign33 = and i64 %colbump32, -8
  %coladv34 = mul i64 %soalen29, 8
  %colend35 = add i64 %colalign33, %coladv34
  %colbase36 = getelementptr inbounds i8, ptr %soaptr28, i64 %colend35
  %colelem37 = getelementptr inbounds i1, ptr %colbase36, i64 %load27
  %idxcol38 = load i1, ptr %colelem37, align 1
  %soaptr39 = extractvalue { ptr, i64 } %load23, 0
  %soalen40 = extractvalue { ptr, i64 } %load23, 1
  %coladv41 = mul i64 %soalen40, 16
  %colend42 = add i64 0, %coladv41
  %colbump43 = add i64 %colend42, 7
  %colalign44 = and i64 %colbump43, -8
  %colbase45 = getelementptr inbounds i8, ptr %soaptr39, i64 %colalign44
  %colelem46 = getelementptr inbounds i64, ptr %colbase45, i64 %load27
  %idxcol47 = load i64, ptr %colelem46, align 8
  %load48 = load i64, ptr %_6, align 8
  %sel = select i1 %idxcol38, i64 %idxcol47, i64 0
  %add49 = add i64 %load48, %sel
  store i64 %add49, ptr %_6, align 8
  br label %bb14

bb14:                                             ; preds = %bb13
  %load50 = load i64, ptr %_7, align 8
  %add51 = add i64 %load50, 1
  store i64 %add51, ptr %_7, align 8
  br label %bb12

bb15:                                             ; preds = %bb12
  %load52 = load i64, ptr %_6, align 8
  call void @align_rt_print_i64(i64 %load52)
  call void @align_rt_arena_end(ptr %arena)
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
