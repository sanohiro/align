if exists("b:current_syntax")
  finish
endif

syn keyword alignKeyword fn return mut pub module import if else arena task_group match loop break template unsafe extern as out
syn match alignAttribute "\<\(align\|layout\|link\)\ze\s*("
syn keyword alignType i8 u8 i16 u16 i32 u32 i64 u64 f32 f64 bool char str string array slice soa box raw builder writer reader buffer array_builder file rng regex regex_match tcp_conn tcp_listener udp_socket child Option Result Error Num Ord Eq vec2 vec4 vec8 vec16 mask2 mask4 mask8 mask16
syn keyword alignBuiltin map par_map where reduce scan partition group_by agg sort sort_by_key chunks zip to_array map_into to_soa dict_encode sum min max count any all dot sum_where select fma abs sqrt floor ceil round trunc pow Some None Ok Err error map_err spawn wait print
syn keyword alignBoolean true false

syn match alignComment "//.*$"
syn region alignString start=+"+ skip=+\\\\\|\\"+ end=+"+ oneline
syn region alignChar start=+'+ skip=+\\'+ end=+'+ oneline
syn match alignNumber "\<[0-9][0-9_]*\>"
syn match alignNumber "\<0[xX][0-9A-Fa-f_]\+\>"
syn match alignNumber "\<0[oO][0-7_]\+\>"
syn match alignNumber "\<0[bB][01_]\+\>"
syn match alignFloat "\<[0-9][0-9_]*\.[0-9][0-9_]*\([eE][+-]\?[0-9][0-9_]*\)\?\>"
syn match alignFloat "\<[0-9][0-9_]*[eE][+-]\?[0-9][0-9_]*\>"
syn match alignOperator ":=\|->\|=>\|==\|!=\|<=\|>=\|&&\|<<\|>>\|\.\."
syn match alignOperator "||"
syn match alignOperator "[=+*/%<>!&|^~?-]"

hi def link alignKeyword Keyword
hi def link alignAttribute PreProc
hi def link alignBuiltin Function
hi def link alignType Type
hi def link alignBoolean Boolean
hi def link alignComment Comment
hi def link alignString String
hi def link alignChar Character
hi def link alignNumber Number
hi def link alignFloat Float
hi def link alignOperator Operator

let b:current_syntax = "align"
