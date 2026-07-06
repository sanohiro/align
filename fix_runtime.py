import re

def main():
    path = "crates/align_runtime/src/lib.rs"
    with open(path, "r") as f:
        content = f.read()

    safe_helpers = """
// Helper to safely construct a slice from an FFI pointer and i64 length.
// Returns an empty slice if len <= 0, ptr is null, or len exceeds isize::MAX.
#[inline(always)]
unsafe fn safe_slice<'a, T>(ptr: *const T, len: i64) -> &'a [T] {
    let Ok(n) = isize::try_from(len) else { return &[] };
    if n <= 0 || ptr.is_null() { return &[] }
    unsafe { std::slice::from_raw_parts(ptr, n as usize) }
}

#[inline(always)]
unsafe fn safe_slice_mut<'a, T>(ptr: *mut T, len: i64) -> &'a mut [T] {
    let Ok(n) = isize::try_from(len) else { return &mut [] };
    if n <= 0 || ptr.is_null() { return &mut [] }
    unsafe { std::slice::from_raw_parts_mut(ptr, n as usize) }
}
"""
    if "safe_slice" not in content:
        # inject just before `pub unsafe extern "C" fn align_rt_str_clone`
        idx = content.find("pub unsafe extern \"C\" fn align_rt_str_clone")
        if idx != -1:
            idx = content.rfind("#[unsafe(no_mangle)]", 0, idx)
            if idx != -1:
                content = content[:idx] + safe_helpers + "\n" + content[idx:]

    # Now we replace the following block patterns:
    # let src: &[u8] = if input_len <= 0 || input.is_null() {
    #     &[]
    # } else {
    #     unsafe { std::slice::from_raw_parts(input, input_len as usize) }
    # };
    # With:
    # let src: &[u8] = unsafe { safe_slice(input, input_len) };
    
    pattern = re.compile(
        r'let\s+([a-zA-Z_0-9]+)\s*:\s*&\[([^\]]+)\]\s*=\s*if\s+([a-zA-Z_0-9]+)\s*<=\s*0\s*\|\|\s*([a-zA-Z_0-9]+)\.is_null\(\)\s*\{\s*&\[\]\s*\}\s*else\s*\{\s*unsafe\s*\{\s*std::slice::from_raw_parts\(\s*\4\s*,\s*\3\s+as\s+usize\)\s*\}\s*\};'
    )
    content = pattern.sub(r'let \1: &[\2] = unsafe { safe_slice(\4, \3) };', content)

    # What about fields: &[JsonField] = if n_fields <= 0 || fields.is_null() { ...
    pattern2 = re.compile(
        r'let\s+([a-zA-Z_0-9]+)\s*:\s*&\[([^\]]+)\]\s*=\s*if\s+([a-zA-Z_0-9]+)\s*<=\s*0\s*\|\|\s*([a-zA-Z_0-9]+)\.is_null\(\)\s*\{\s*&\[\]\s*\}\s*else\s*\{\s*unsafe\s*\{\s*std::slice::from_raw_parts\(\s*\4\s*,\s*\3\s+as\s+usize\)\s*\}\s*\};'
    )
    content = pattern2.sub(r'let \1: &[\2] = unsafe { safe_slice(\4, \3) };', content)

    with open(path, "w") as f:
        f.write(content)

if __name__ == "__main__":
    main()
