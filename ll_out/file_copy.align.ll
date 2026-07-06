; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

; Function Attrs: nounwind
define { i8, i32, { i32, i32 } } @copy_all(ptr %0, ptr %1, ptr %2) #0 {
bb0:
  %_0 = alloca ptr, align 8
  %_1 = alloca ptr, align 8
  %_2 = alloca ptr, align 8
  %_3 = alloca i64, align 8
  %_4 = alloca { i8, i64, { i32, i32 } }, align 8
  %_5 = alloca { i8, i32, { i32, i32 } }, align 4
  store ptr %0, ptr %_0, align 8
  store ptr %1, ptr %_1, align 8
  store ptr %2, ptr %_2, align 8
  %load = load ptr, ptr %_0, align 8
  %load1 = load ptr, ptr %_2, align 8
  %read = call i64 @align_rt_io_reader_read(ptr %load, ptr %load1)
  %ge = icmp sge i64 %read, 0
  br i1 %ge, label %bb1, label %bb2

bb1:                                              ; preds = %bb0
  %ok = insertvalue { i8, i64, { i32, i32 } } zeroinitializer, i64 %read, 1
  store { i8, i64, { i32, i32 } } %ok, ptr %_4, align 8
  br label %bb3

bb2:                                              ; preds = %bb0
  %sub = sub i64 0, %read
  %cast = trunc i64 %sub to i32
  %sub2 = sub i32 %cast, 1
  %ge3 = icmp sge i32 %sub2, 3
  %sel = select i1 %ge3, i32 3, i32 %sub2
  %sub4 = sub i32 %cast, 4
  %ge5 = icmp sge i32 %sub4, 0
  %sel6 = select i1 %ge5, i32 %sub4, i32 0
  %etag = insertvalue { i32, i32 } zeroinitializer, i32 %sel, 0
  %ecode = insertvalue { i32, i32 } %etag, i32 %sel6, 1
  %err = insertvalue { i8, i64, { i32, i32 } } { i8 1, i64 0, { i32, i32 } zeroinitializer }, { i32, i32 } %ecode, 2
  store { i8, i64, { i32, i32 } } %err, ptr %_4, align 8
  br label %bb3

bb3:                                              ; preds = %bb2, %bb1
  %load7 = load { i8, i64, { i32, i32 } }, ptr %_4, align 8
  %tag = extractvalue { i8, i64, { i32, i32 } } %load7, 0
  %isok = icmp eq i8 %tag, 0
  br i1 %isok, label %bb4, label %bb5

bb4:                                              ; preds = %bb3
  %ok8 = extractvalue { i8, i64, { i32, i32 } } %load7, 1
  store i64 %ok8, ptr %_3, align 8
  %load9 = load i64, ptr %_3, align 8
  %eq = icmp eq i64 %load9, 0
  br i1 %eq, label %bb6, label %bb7

bb5:                                              ; preds = %bb3
  %err10 = extractvalue { i8, i64, { i32, i32 } } %load7, 2
  %err11 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %err10, 2
  %droph = load ptr, ptr %_0, align 8
  call void @align_rt_io_reader_free(ptr %droph)
  %droph12 = load ptr, ptr %_1, align 8
  call void @align_rt_io_writer_free(ptr %droph12)
  %droph13 = load ptr, ptr %_2, align 8
  call void @align_rt_buffer_free(ptr %droph13)
  ret { i8, i32, { i32, i32 } } %err11

bb6:                                              ; preds = %bb4
  %droph14 = load ptr, ptr %_0, align 8
  call void @align_rt_io_reader_free(ptr %droph14)
  %droph15 = load ptr, ptr %_1, align 8
  call void @align_rt_io_writer_free(ptr %droph15)
  %droph16 = load ptr, ptr %_2, align 8
  call void @align_rt_buffer_free(ptr %droph16)
  ret { i8, i32, { i32, i32 } } zeroinitializer

bb7:                                              ; preds = %bb4
  br label %bb8

bb8:                                              ; preds = %bb7
  %load17 = load ptr, ptr %_1, align 8
  %load18 = load ptr, ptr %_2, align 8
  %bytesslot = alloca { ptr, i64 }, align 8
  call void @align_rt_buffer_bytes(ptr %load18, ptr %bytesslot)
  %bytes = load { ptr, i64 }, ptr %bytesslot, align 8
  %wptr = extractvalue { ptr, i64 } %bytes, 0
  %wlen = extractvalue { ptr, i64 } %bytes, 1
  %wr = call i32 @align_rt_io_writer_write(ptr %load17, ptr %wptr, i64 %wlen)
  %eq19 = icmp eq i32 %wr, 0
  br i1 %eq19, label %bb9, label %bb10

bb9:                                              ; preds = %bb8
  store { i8, i32, { i32, i32 } } zeroinitializer, ptr %_5, align 4
  br label %bb11

bb10:                                             ; preds = %bb8
  %sub20 = sub i32 %wr, 1
  %ge21 = icmp sge i32 %sub20, 3
  %sel22 = select i1 %ge21, i32 3, i32 %sub20
  %sub23 = sub i32 %wr, 4
  %ge24 = icmp sge i32 %sub23, 0
  %sel25 = select i1 %ge24, i32 %sub23, i32 0
  %etag26 = insertvalue { i32, i32 } zeroinitializer, i32 %sel22, 0
  %ecode27 = insertvalue { i32, i32 } %etag26, i32 %sel25, 1
  %err28 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %ecode27, 2
  store { i8, i32, { i32, i32 } } %err28, ptr %_5, align 4
  br label %bb11

bb11:                                             ; preds = %bb10, %bb9
  %load29 = load { i8, i32, { i32, i32 } }, ptr %_5, align 4
  %tag30 = extractvalue { i8, i32, { i32, i32 } } %load29, 0
  %isok31 = icmp eq i8 %tag30, 0
  br i1 %isok31, label %bb12, label %bb13

bb12:                                             ; preds = %bb11
  %ok32 = extractvalue { i8, i32, { i32, i32 } } %load29, 1
  %load33 = load ptr, ptr %_0, align 8
  %load34 = load ptr, ptr %_1, align 8
  %load35 = load ptr, ptr %_2, align 8
  store ptr null, ptr %_0, align 8
  store ptr null, ptr %_1, align 8
  store ptr null, ptr %_2, align 8
  %call = call { i8, i32, { i32, i32 } } @copy_all(ptr %load33, ptr %load34, ptr %load35)
  %droph36 = load ptr, ptr %_0, align 8
  call void @align_rt_io_reader_free(ptr %droph36)
  %droph37 = load ptr, ptr %_1, align 8
  call void @align_rt_io_writer_free(ptr %droph37)
  %droph38 = load ptr, ptr %_2, align 8
  call void @align_rt_buffer_free(ptr %droph38)
  ret { i8, i32, { i32, i32 } } %call

bb13:                                             ; preds = %bb11
  %err39 = extractvalue { i8, i32, { i32, i32 } } %load29, 2
  %err40 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %err39, 2
  %droph41 = load ptr, ptr %_0, align 8
  call void @align_rt_io_reader_free(ptr %droph41)
  %droph42 = load ptr, ptr %_1, align 8
  call void @align_rt_io_writer_free(ptr %droph42)
  %droph43 = load ptr, ptr %_2, align 8
  call void @align_rt_buffer_free(ptr %droph43)
  ret { i8, i32, { i32, i32 } } %err40
}

; Function Attrs: nounwind
define { i8, i32, { i32, i32 } } @align_main({ ptr, i64 } %0) #0 {
bb0:
  %_0 = alloca { ptr, i64 }, align 8
  %_1 = alloca ptr, align 8
  %_2 = alloca ptr, align 8
  %_3 = alloca ptr, align 8
  %_4 = alloca ptr, align 8
  %_5 = alloca { i8, ptr, { i32, i32 } }, align 8
  %_6 = alloca ptr, align 8
  %_7 = alloca { i8, ptr, { i32, i32 } }, align 8
  store { ptr, i64 } %0, ptr %_0, align 8
  store ptr null, ptr %_1, align 8
  store ptr null, ptr %_2, align 8
  store ptr null, ptr %_3, align 8
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
  %droph15 = load ptr, ptr %_3, align 8
  call void @align_rt_buffer_free(ptr %droph15)
  ret { i8, i32, { i32, i32 } } %err13

bb8:                                              ; preds = %bb6
  call void @align_rt_bounds_fail(i64 2, i64 %len9)
  unreachable

bb9:                                              ; preds = %bb6
  %ptr16 = extractvalue { ptr, i64 } %load8, 0
  %slcidx17 = getelementptr inbounds { ptr, i64 }, ptr %ptr16, i64 2
  %slcload18 = load { ptr, i64 }, ptr %slcidx17, align 8
  store ptr null, ptr %_6, align 8
  %path_p19 = extractvalue { ptr, i64 } %slcload18, 0
  %path_l20 = extractvalue { ptr, i64 } %slcload18, 1
  %open21 = call i32 @align_rt_io_writer_create(ptr %path_p19, i64 %path_l20, ptr %_6)
  %eq22 = icmp eq i32 %open21, 0
  br i1 %eq22, label %bb10, label %bb11

bb10:                                             ; preds = %bb9
  %load23 = load ptr, ptr %_6, align 8
  %ok24 = insertvalue { i8, ptr, { i32, i32 } } zeroinitializer, ptr %load23, 1
  store { i8, ptr, { i32, i32 } } %ok24, ptr %_7, align 8
  br label %bb12

bb11:                                             ; preds = %bb9
  %sub25 = sub i32 %open21, 1
  %ge26 = icmp sge i32 %sub25, 3
  %sel27 = select i1 %ge26, i32 3, i32 %sub25
  %sub28 = sub i32 %open21, 4
  %ge29 = icmp sge i32 %sub28, 0
  %sel30 = select i1 %ge29, i32 %sub28, i32 0
  %etag31 = insertvalue { i32, i32 } zeroinitializer, i32 %sel27, 0
  %ecode32 = insertvalue { i32, i32 } %etag31, i32 %sel30, 1
  %err33 = insertvalue { i8, ptr, { i32, i32 } } { i8 1, ptr null, { i32, i32 } zeroinitializer }, { i32, i32 } %ecode32, 2
  store { i8, ptr, { i32, i32 } } %err33, ptr %_7, align 8
  br label %bb12

bb12:                                             ; preds = %bb11, %bb10
  %load34 = load { i8, ptr, { i32, i32 } }, ptr %_7, align 8
  %tag35 = extractvalue { i8, ptr, { i32, i32 } } %load34, 0
  %isok36 = icmp eq i8 %tag35, 0
  br i1 %isok36, label %bb13, label %bb14

bb13:                                             ; preds = %bb12
  %ok37 = extractvalue { i8, ptr, { i32, i32 } } %load34, 1
  store ptr %ok37, ptr %_2, align 8
  %buf = call ptr @align_rt_buffer_new(i64 65536)
  store ptr %buf, ptr %_3, align 8
  %load38 = load ptr, ptr %_1, align 8
  %load39 = load ptr, ptr %_2, align 8
  %load40 = load ptr, ptr %_3, align 8
  store ptr null, ptr %_1, align 8
  store ptr null, ptr %_2, align 8
  store ptr null, ptr %_3, align 8
  %call = call { i8, i32, { i32, i32 } } @copy_all(ptr %load38, ptr %load39, ptr %load40)
  %tag41 = extractvalue { i8, i32, { i32, i32 } } %call, 0
  %isok42 = icmp eq i8 %tag41, 0
  br i1 %isok42, label %bb15, label %bb16

bb14:                                             ; preds = %bb12
  %err43 = extractvalue { i8, ptr, { i32, i32 } } %load34, 2
  %err44 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %err43, 2
  %drop45 = load { ptr, i64 }, ptr %_0, align 8
  %dropptr46 = extractvalue { ptr, i64 } %drop45, 0
  call void @align_rt_free(ptr %dropptr46)
  %droph47 = load ptr, ptr %_1, align 8
  call void @align_rt_io_reader_free(ptr %droph47)
  %droph48 = load ptr, ptr %_2, align 8
  call void @align_rt_io_writer_free(ptr %droph48)
  %droph49 = load ptr, ptr %_3, align 8
  call void @align_rt_buffer_free(ptr %droph49)
  ret { i8, i32, { i32, i32 } } %err44

bb15:                                             ; preds = %bb13
  %ok50 = extractvalue { i8, i32, { i32, i32 } } %call, 1
  %drop51 = load { ptr, i64 }, ptr %_0, align 8
  %dropptr52 = extractvalue { ptr, i64 } %drop51, 0
  call void @align_rt_free(ptr %dropptr52)
  %droph53 = load ptr, ptr %_1, align 8
  call void @align_rt_io_reader_free(ptr %droph53)
  %droph54 = load ptr, ptr %_2, align 8
  call void @align_rt_io_writer_free(ptr %droph54)
  %droph55 = load ptr, ptr %_3, align 8
  call void @align_rt_buffer_free(ptr %droph55)
  ret { i8, i32, { i32, i32 } } zeroinitializer

bb16:                                             ; preds = %bb13
  %err56 = extractvalue { i8, i32, { i32, i32 } } %call, 2
  %err57 = insertvalue { i8, i32, { i32, i32 } } { i8 1, i32 0, { i32, i32 } zeroinitializer }, { i32, i32 } %err56, 2
  %drop58 = load { ptr, i64 }, ptr %_0, align 8
  %dropptr59 = extractvalue { ptr, i64 } %drop58, 0
  call void @align_rt_free(ptr %dropptr59)
  %droph60 = load ptr, ptr %_1, align 8
  call void @align_rt_io_reader_free(ptr %droph60)
  %droph61 = load ptr, ptr %_2, align 8
  call void @align_rt_io_writer_free(ptr %droph61)
  %droph62 = load ptr, ptr %_3, align 8
  call void @align_rt_buffer_free(ptr %droph62)
  ret { i8, i32, { i32, i32 } } %err57
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
