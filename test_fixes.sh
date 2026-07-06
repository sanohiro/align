#!/bin/bash
grep -n "isize::try_from(i).unwrap_or(0) as usize" crates/align_runtime/src/lib.rs
grep -n -A 5 "pub unsafe extern \"C\" fn align_rt_group_multi_str" crates/align_runtime/src/lib.rs
grep -n -A 5 "pub unsafe extern \"C\" fn align_rt_dict_encode_str" crates/align_runtime/src/lib.rs
grep -n -A 10 "pub unsafe extern \"C\" fn align_rt_builder_write(" crates/align_runtime/src/lib.rs
grep -n -A 20 "pub unsafe extern \"C\" fn align_rt_builder_write_str_int_str" crates/align_runtime/src/lib.rs
grep -n -A 10 "pub unsafe extern \"C\" fn align_rt_dict_lookup" crates/align_runtime/src/lib.rs
