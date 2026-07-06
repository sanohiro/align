; ModuleID = 'align'
source_filename = "align"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

%User = type { i64, i64, i1 }

; Function Attrs: nounwind
define i32 @main() #0 {
bb0:
  %_0 = alloca [3 x %User], align 8
  %_1 = alloca { ptr, i64 }, align 8
  %_2 = alloca i64, align 8
  %_3 = alloca %User, align 8
  %_4 = alloca i64, align 8
  %_5 = alloca i64, align 8
  %_6 = alloca i64, align 8
  %_7 = alloca i64, align 8
  %_8 = alloca i64, align 8
  %_9 = alloca i64, align 8
  %_10 = alloca i64, align 8
  %_11 = alloca i64, align 8
  %_12 = alloca %User, align 8
  %arena = call ptr @align_rt_arena_begin()
  %elemfield = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 0, i32 2
  store i1 true, ptr %elemfield, align 1
  %elemfield1 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 0, i32 0
  store i64 10, ptr %elemfield1, align 8
  %elemfield2 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 0, i32 1
  store i64 30, ptr %elemfield2, align 8
  %elemfield3 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 1, i32 2
  store i1 false, ptr %elemfield3, align 1
  %elemfield4 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 1, i32 0
  store i64 99, ptr %elemfield4, align 8
  %elemfield5 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 1, i32 1
  store i64 40, ptr %elemfield5, align 8
  %elemfield6 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 2, i32 2
  store i1 true, ptr %elemfield6, align 1
  %elemfield7 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 2, i32 0
  store i64 20, ptr %elemfield7, align 8
  %elemfield8 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 2, i32 1
  store i64 25, ptr %elemfield8, align 8
  %soabuf = call ptr @align_rt_arena_alloc(ptr %arena, i64 56, i64 8)
  store i64 0, ptr %_7, align 8
  br label %bb1

bb1:                                              ; preds = %bb2, %bb0
  %load = load i64, ptr %_7, align 8
  %lt = icmp slt i64 %load, 3
  br i1 %lt, label %bb2, label %bb3

bb2:                                              ; preds = %bb1
  %load9 = load i64, ptr %_7, align 8
  %elemfield10 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 %load9, i32 2
  %idxfld = load i1, ptr %elemfield10, align 1
  %colbase = getelementptr inbounds i8, ptr %soabuf, i64 0
  %colelem = getelementptr inbounds i1, ptr %colbase, i64 %load9
  store i1 %idxfld, ptr %colelem, align 1
  %elemfield11 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 %load9, i32 0
  %idxfld12 = load i64, ptr %elemfield11, align 8
  %colbase13 = getelementptr inbounds i8, ptr %soabuf, i64 8
  %colelem14 = getelementptr inbounds i64, ptr %colbase13, i64 %load9
  store i64 %idxfld12, ptr %colelem14, align 8
  %elemfield15 = getelementptr inbounds [3 x %User], ptr %_0, i64 0, i64 %load9, i32 1
  %idxfld16 = load i64, ptr %elemfield15, align 8
  %colbase17 = getelementptr inbounds i8, ptr %soabuf, i64 32
  %colelem18 = getelementptr inbounds i64, ptr %colbase17, i64 %load9
  store i64 %idxfld16, ptr %colelem18, align 8
  %add = add i64 %load9, 1
  store i64 %add, ptr %_7, align 8
  br label %bb1

bb3:                                              ; preds = %bb1
  %arrptr = insertvalue { ptr, i64 } poison, ptr %soabuf, 0
  %arrlen = insertvalue { ptr, i64 } %arrptr, i64 3, 1
  store { ptr, i64 } %arrlen, ptr %_1, align 8
  %load19 = load { ptr, i64 }, ptr %_1, align 8
  %len = extractvalue { ptr, i64 } %load19, 1
  store i64 0, ptr %_8, align 8
  store i64 0, ptr %_9, align 8
  br label %bb4

bb4:                                              ; preds = %bb6, %bb3
  %load20 = load i64, ptr %_9, align 8
  %lt21 = icmp slt i64 %load20, %len
  br i1 %lt21, label %bb5, label %bb7

bb5:                                              ; preds = %bb4
  %load22 = load i64, ptr %_9, align 8
  %soaptr = extractvalue { ptr, i64 } %load19, 0
  %soalen = extractvalue { ptr, i64 } %load19, 1
  %colbase23 = getelementptr inbounds i8, ptr %soaptr, i64 0
  %colelem24 = getelementptr inbounds i1, ptr %colbase23, i64 %load22
  %idxcol = load i1, ptr %colelem24, align 1
  %soaptr25 = extractvalue { ptr, i64 } %load19, 0
  %soalen26 = extractvalue { ptr, i64 } %load19, 1
  %coladv = mul i64 %soalen26, 1
  %colend = add i64 0, %coladv
  %colbump = add i64 %colend, 7
  %colalign = and i64 %colbump, -8
  %colbase27 = getelementptr inbounds i8, ptr %soaptr25, i64 %colalign
  %colelem28 = getelementptr inbounds i64, ptr %colbase27, i64 %load22
  %idxcol29 = load i64, ptr %colelem28, align 8
  %load30 = load i64, ptr %_8, align 8
  %sel = select i1 %idxcol, i64 %idxcol29, i64 0
  %add31 = add i64 %load30, %sel
  store i64 %add31, ptr %_8, align 8
  br label %bb6

bb6:                                              ; preds = %bb5
  %load32 = load i64, ptr %_9, align 8
  %add33 = add i64 %load32, 1
  store i64 %add33, ptr %_9, align 8
  br label %bb4

bb7:                                              ; preds = %bb4
  %load34 = load i64, ptr %_8, align 8
  store i64 %load34, ptr %_2, align 8
  %load35 = load { ptr, i64 }, ptr %_1, align 8
  %len36 = extractvalue { ptr, i64 } %load35, 1
  %ge = icmp sge i64 2, %len36
  %or = or i1 false, %ge
  br i1 %or, label %bb8, label %bb9

bb8:                                              ; preds = %bb7
  call void @align_rt_bounds_fail(i64 2, i64 %len36)
  unreachable

bb9:                                              ; preds = %bb7
  %soaptr37 = extractvalue { ptr, i64 } %load35, 0
  %soalen38 = extractvalue { ptr, i64 } %load35, 1
  %gcolbase = getelementptr inbounds i8, ptr %soaptr37, i64 0
  %gcolelem = getelementptr inbounds i1, ptr %gcolbase, i64 2
  %gload = load i1, ptr %gcolelem, align 1
  %ginsert = insertvalue %User poison, i1 %gload, 2
  %coladv39 = mul i64 %soalen38, 1
  %colend40 = add i64 0, %coladv39
  %colbump41 = add i64 %colend40, 7
  %colalign42 = and i64 %colbump41, -8
  %gcolbase43 = getelementptr inbounds i8, ptr %soaptr37, i64 %colalign42
  %gcolelem44 = getelementptr inbounds i64, ptr %gcolbase43, i64 2
  %gload45 = load i64, ptr %gcolelem44, align 8
  %ginsert46 = insertvalue %User %ginsert, i64 %gload45, 0
  %coladv47 = mul i64 %soalen38, 1
  %colend48 = add i64 0, %coladv47
  %colbump49 = add i64 %colend48, 7
  %colalign50 = and i64 %colbump49, -8
  %coladv51 = mul i64 %soalen38, 8
  %colend52 = add i64 %colalign50, %coladv51
  %colbump53 = add i64 %colend52, 7
  %colalign54 = and i64 %colbump53, -8
  %gcolbase55 = getelementptr inbounds i8, ptr %soaptr37, i64 %colalign54
  %gcolelem56 = getelementptr inbounds i64, ptr %gcolbase55, i64 2
  %gload57 = load i64, ptr %gcolelem56, align 8
  %ginsert58 = insertvalue %User %ginsert46, i64 %gload57, 1
  store %User %ginsert58, ptr %_3, align 8
  %load59 = load { ptr, i64 }, ptr %_1, align 8
  %len60 = extractvalue { ptr, i64 } %load59, 1
  store i64 %len60, ptr %_4, align 8
  %load61 = load { ptr, i64 }, ptr %_1, align 8
  %len62 = extractvalue { ptr, i64 } %load61, 1
  %ge63 = icmp sge i64 0, %len62
  %or64 = or i1 false, %ge63
  br i1 %or64, label %bb10, label %bb11

bb10:                                             ; preds = %bb9
  call void @align_rt_bounds_fail(i64 0, i64 %len62)
  unreachable

bb11:                                             ; preds = %bb9
  %soaptr65 = extractvalue { ptr, i64 } %load61, 0
  %soalen66 = extractvalue { ptr, i64 } %load61, 1
  %coladv67 = mul i64 %soalen66, 1
  %colend68 = add i64 0, %coladv67
  %colbump69 = add i64 %colend68, 7
  %colalign70 = and i64 %colbump69, -8
  %colbase71 = getelementptr inbounds i8, ptr %soaptr65, i64 %colalign70
  %colelem72 = getelementptr inbounds i64, ptr %colbase71, i64 0
  %idxcol73 = load i64, ptr %colelem72, align 8
  store i64 %idxcol73, ptr %_5, align 8
  %soa = load { ptr, i64 }, ptr %_1, align 8
  %soaptr74 = extractvalue { ptr, i64 } %soa, 0
  %soalen75 = extractvalue { ptr, i64 } %soa, 1
  %coladv76 = mul i64 %soalen75, 1
  %colend77 = add i64 0, %coladv76
  %colbump78 = add i64 %colend77, 7
  %colalign79 = and i64 %colbump78, -8
  %colptr = getelementptr inbounds i8, ptr %soaptr74, i64 %colalign79
  %colptr80 = insertvalue { ptr, i64 } poison, ptr %colptr, 0
  %collen = insertvalue { ptr, i64 } %colptr80, i64 %soalen75, 1
  %len81 = extractvalue { ptr, i64 } %collen, 1
  %gt = icmp sgt i64 3, %len81
  %or82 = or i1 false, %gt
  br i1 %or82, label %bb12, label %bb13

bb12:                                             ; preds = %bb11
  call void @align_rt_range_fail(i64 1, i64 3, i64 %len81)
  unreachable

bb13:                                             ; preds = %bb11
  %subptr = extractvalue { ptr, i64 } %collen, 0
  %subgep = getelementptr inbounds i64, ptr %subptr, i64 1
  %subvptr = insertvalue { ptr, i64 } poison, ptr %subgep, 0
  %subvlen = insertvalue { ptr, i64 } %subvptr, i64 2, 1
  %len83 = extractvalue { ptr, i64 } %subvlen, 1
  store i64 0, ptr %_10, align 8
  store i64 0, ptr %_11, align 8
  br label %bb14

bb14:                                             ; preds = %bb16, %bb13
  %load84 = load i64, ptr %_11, align 8
  %lt85 = icmp slt i64 %load84, %len83
  br i1 %lt85, label %bb15, label %bb17

bb15:                                             ; preds = %bb14
  %load86 = load i64, ptr %_11, align 8
  %ptr = extractvalue { ptr, i64 } %subvlen, 0
  %slcidx = getelementptr inbounds i64, ptr %ptr, i64 %load86
  %slcload = load i64, ptr %slcidx, align 8
  %load87 = load i64, ptr %_10, align 8
  %add88 = add i64 %load87, %slcload
  store i64 %add88, ptr %_10, align 8
  br label %bb16

bb16:                                             ; preds = %bb15
  %load89 = load i64, ptr %_11, align 8
  %add90 = add i64 %load89, 1
  store i64 %add90, ptr %_11, align 8
  br label %bb14

bb17:                                             ; preds = %bb14
  %load91 = load i64, ptr %_10, align 8
  store i64 %load91, ptr %_6, align 8
  %load92 = load { ptr, i64 }, ptr %_1, align 8
  %len93 = extractvalue { ptr, i64 } %load92, 1
  %ge94 = icmp sge i64 1, %len93
  %or95 = or i1 false, %ge94
  br i1 %or95, label %bb18, label %bb19

bb18:                                             ; preds = %bb17
  call void @align_rt_bounds_fail(i64 1, i64 %len93)
  unreachable

bb19:                                             ; preds = %bb17
  %ptr96 = extractvalue { ptr, i64 } %load92, 0
  %coladv97 = mul i64 %len93, 1
  %colend98 = add i64 0, %coladv97
  %colbump99 = add i64 %colend98, 7
  %colalign100 = and i64 %colbump99, -8
  %colbase101 = getelementptr inbounds i8, ptr %ptr96, i64 %colalign100
  %colelem102 = getelementptr inbounds i64, ptr %colbase101, i64 1
  store i64 99, ptr %colelem102, align 8
  %load103 = load { ptr, i64 }, ptr %_1, align 8
  %len104 = extractvalue { ptr, i64 } %load103, 1
  %ge105 = icmp sge i64 2, %len104
  %or106 = or i1 false, %ge105
  br i1 %or106, label %bb20, label %bb21

bb20:                                             ; preds = %bb19
  call void @align_rt_bounds_fail(i64 2, i64 %len104)
  unreachable

bb21:                                             ; preds = %bb19
  %soaptr107 = extractvalue { ptr, i64 } %load103, 0
  %soalen108 = extractvalue { ptr, i64 } %load103, 1
  %gcolbase109 = getelementptr inbounds i8, ptr %soaptr107, i64 0
  %gcolelem110 = getelementptr inbounds i1, ptr %gcolbase109, i64 2
  %gload111 = load i1, ptr %gcolelem110, align 1
  %ginsert112 = insertvalue %User poison, i1 %gload111, 2
  %coladv113 = mul i64 %soalen108, 1
  %colend114 = add i64 0, %coladv113
  %colbump115 = add i64 %colend114, 7
  %colalign116 = and i64 %colbump115, -8
  %gcolbase117 = getelementptr inbounds i8, ptr %soaptr107, i64 %colalign116
  %gcolelem118 = getelementptr inbounds i64, ptr %gcolbase117, i64 2
  %gload119 = load i64, ptr %gcolelem118, align 8
  %ginsert120 = insertvalue %User %ginsert112, i64 %gload119, 0
  %coladv121 = mul i64 %soalen108, 1
  %colend122 = add i64 0, %coladv121
  %colbump123 = add i64 %colend122, 7
  %colalign124 = and i64 %colbump123, -8
  %coladv125 = mul i64 %soalen108, 8
  %colend126 = add i64 %colalign124, %coladv125
  %colbump127 = add i64 %colend126, 7
  %colalign128 = and i64 %colbump127, -8
  %gcolbase129 = getelementptr inbounds i8, ptr %soaptr107, i64 %colalign128
  %gcolelem130 = getelementptr inbounds i64, ptr %gcolbase129, i64 2
  %gload131 = load i64, ptr %gcolelem130, align 8
  %ginsert132 = insertvalue %User %ginsert120, i64 %gload131, 1
  %load133 = load { ptr, i64 }, ptr %_1, align 8
  %len134 = extractvalue { ptr, i64 } %load133, 1
  %ge135 = icmp sge i64 0, %len134
  %or136 = or i1 false, %ge135
  br i1 %or136, label %bb22, label %bb23

bb22:                                             ; preds = %bb21
  call void @align_rt_bounds_fail(i64 0, i64 %len134)
  unreachable

bb23:                                             ; preds = %bb21
  %ptr137 = extractvalue { ptr, i64 } %load133, 0
  store %User %ginsert132, ptr %_12, align 8
  %fldptr = getelementptr inbounds %User, ptr %_12, i32 0, i32 2
  %fld = load i1, ptr %fldptr, align 1
  %colbase138 = getelementptr inbounds i8, ptr %ptr137, i64 0
  %colelem139 = getelementptr inbounds i1, ptr %colbase138, i64 0
  store i1 %fld, ptr %colelem139, align 1
  %fldptr140 = getelementptr inbounds %User, ptr %_12, i32 0, i32 0
  %fld141 = load i64, ptr %fldptr140, align 8
  %coladv142 = mul i64 %len134, 1
  %colend143 = add i64 0, %coladv142
  %colbump144 = add i64 %colend143, 7
  %colalign145 = and i64 %colbump144, -8
  %colbase146 = getelementptr inbounds i8, ptr %ptr137, i64 %colalign145
  %colelem147 = getelementptr inbounds i64, ptr %colbase146, i64 0
  store i64 %fld141, ptr %colelem147, align 8
  %fldptr148 = getelementptr inbounds %User, ptr %_12, i32 0, i32 1
  %fld149 = load i64, ptr %fldptr148, align 8
  %coladv150 = mul i64 %len134, 1
  %colend151 = add i64 0, %coladv150
  %colbump152 = add i64 %colend151, 7
  %colalign153 = and i64 %colbump152, -8
  %coladv154 = mul i64 %len134, 8
  %colend155 = add i64 %colalign153, %coladv154
  %colbump156 = add i64 %colend155, 7
  %colalign157 = and i64 %colbump156, -8
  %colbase158 = getelementptr inbounds i8, ptr %ptr137, i64 %colalign157
  %colelem159 = getelementptr inbounds i64, ptr %colbase158, i64 0
  store i64 %fld149, ptr %colelem159, align 8
  %load160 = load i64, ptr %_2, align 8
  %fldptr161 = getelementptr inbounds %User, ptr %_3, i32 0, i32 1
  %fld162 = load i64, ptr %fldptr161, align 8
  %add163 = add i64 %load160, %fld162
  %load164 = load i64, ptr %_4, align 8
  %sub = sub i64 %add163, %load164
  %load165 = load i64, ptr %_5, align 8
  %add166 = add i64 %sub, %load165
  %load167 = load i64, ptr %_6, align 8
  %add168 = add i64 %add166, %load167
  %sub169 = sub i64 %add168, 119
  %cast = trunc i64 %sub169 to i32
  call void @align_rt_arena_end(ptr %arena)
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
