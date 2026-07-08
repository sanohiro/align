if exists("b:current_syntax")
  finish
endif

syn keyword alignKeyword fn return mut pub module import if else arena box task_group match template unsafe extern as out
syn keyword alignType i8 u8 i16 u16 i32 u32 i64 u64 f32 f64 bool char str string array slice raw writer reader buffer rng tcp_conn tcp_listener udp_socket child soa Option Result vec2 vec4 vec8 vec16 mask2 mask4 mask8 mask16
syn keyword alignBuiltin map par_map where reduce scan partition group_by sort sort_by_key chunks sum min max count any all dot
syn keyword alignBoolean true false

syn match alignComment "//.*$"
syn region alignString start=+"+ skip=+\\\\\|\\"+ end=+"+
syn region alignChar start=+'+ skip=+\\'+ end=+'+
syn match alignNumber "\<[0-9_]\+\(\.[0-9_]\+\)\?\([eE][+-]\?[0-9_]\+\)\?\>"

hi def link alignKeyword Keyword
hi def link alignBuiltin Function
hi def link alignType Type
hi def link alignBoolean Boolean
hi def link alignComment Comment
hi def link alignString String
hi def link alignChar Character
hi def link alignNumber Number

let b:current_syntax = "align"
