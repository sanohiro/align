//! Borrowed packed bits share the physical bytes and authority of the source.
mod common;
use common::*;

const HELPERS: &str = r#"module masks
import core.bitset
pub fn mark(borrow mut words: [i64; 4], index: i64, flag: bool) {
  mut flags := bitset.view(words[..].as_bytes(), 256)
  flags[index] = flag
}
pub fn present(borrow words: [i64; 4], index: i64) -> bool =
  bitset.view(words[..].as_bytes(), 256)[index]
pub fn window<T>(ignored: T, bits: bitset, lo: i64, hi: i64) -> bitset = bits[lo..hi]
pub fn set(borrow mut bits: bitset, k:i64) { bits[k] = true }
pub fn card(bits: bitset) -> i64 = bits.count()
"#;

#[test]
fn packed_bytes_and_borrowed_word_arrays_in_whole_and_unit_builds() {
    if !backend_available() {
        return;
    }
    let main = r#"import masks
import core.bitset
Bag { prefix: i64, flags: bitset, suffix: i64 }
fn apply(f: fn(bitset)->i64, bits: bitset)->i64 = f(bits)
fn edit(borrow mut bits:bitset, k:i64) { bits[k] = true }
fn optional(flag: bool, bits: bitset)->Option<bitset> = if flag { Some(bits) } else { None }
fn tagged(bits: bitset)->Result<bitset,Error> = Ok(bits)
extern "C" fn align_rt_alloc_count()->i64
fn main()->i32 {
  allocated := unsafe {align_rt_alloc_count()}
  mut words := [0 as i64, 0 as i64, 0 as i64, 0 as i64]
  masks.mark(words, 63, true)
  masks.mark(words, 64, true)
  masks.mark(words, 255, true)
  if !masks.present(words, 63) || !masks.present(words, 64) || !masks.present(words, 255) { return 1 }
  masks.mark(words, 64, false)
  if words[0] != -9223372036854775808 || words[1] != 0 || words[3] != -9223372036854775808 { return 2 }
  mut flags := bitset.view(words[..].as_bytes(), 256)
  edit(flags, 63)
  masks.set(flags, 11)
  flags[11] = false
  selected := masks.window(0, flags, 1, 256)
  bag := Bag { prefix: 11, flags: selected, suffix: 97 }
  saved := optional(true, bag.flags) else { return 3 }
  result := tagged(saved) else { return 4 }
  if apply(masks.card, result) != 2 || !result[62] || !result[254] || bag.prefix + bag.suffix != 108 { return 5 }
  data := [0xb6 as u8, 0x79 as u8, 0xfe as u8, 0x00 as u8, 0xff as u8, 0x0a as u8, 0x31 as u8, 0xff as u8, 0xfc as u8]
  bits := bitset.view(data[..], 69)
  mut lo := 0
  loop {
    if lo > bits.len() { break }
    mut hi := lo
    mut expected := 0
    loop {
      cut := bits[lo..hi]
      if cut.count() != expected || cut.any() != (expected > 0) || cut.all() != (expected == hi - lo) { return 6 }
      if hi == bits.len() { break }
      bit := (data[hi / 8] & ((1 as u8) << ((hi % 8) as u8))) != 0
      if bit != bits[hi] { return 7 }
      if bit { expected = expected + 1 }
      hi = hi + 1
    }
    lo = lo + 1
  }
  if unsafe {align_rt_alloc_count()} != allocated { return 8 }
  return 0
}
"#;
    let files = [("masks.align", HELPERS), ("main.align", main)];
    let ir = emit_llvm_multi("packed-bits-ir", &files, "main.align");
    assert!(ir.contains("llvm.ctpop.i8"), "missing packed popcount");
    for output in [
        build_and_run_multi("packed-bits-whole", &files, "main.align"),
        build_per_unit_multi("packed-bits-units", &files, "main.align").link_and_run(),
    ] {
        assert!(
            output.status.success(),
            "{:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn packed_storage_authority_and_escape() {
    for (name, body, reject) in [
        (
            "shared",
            "fn f(data: slice<u8>) { mut bits := bitset.view(data, 1); bits[0] = true }",
            true,
        ),
        (
            "value",
            "fn f(bits: bitset) { mut alias := bits; alias[0] = true }",
            true,
        ),
        (
            "direct_out",
            "fn f(out bits: bitset) { bits[0] = true }",
            true,
        ),
        (
            "writable_bytes",
            "fn f(out bytes: slice<u8>) { mut bits := bitset.view(bytes, 1); bits[0] = true }",
            false,
        ),
        (
            "returns",
            "fn get(data: slice<u8>)->bitset = bitset.view(data, 1)\nfn f() { mut data := [0 as u8]; mut bits := get(data[..]); bits[0] = true }",
            true,
        ),
        (
            "stack",
            "fn f()->bitset { data := [0 as u8]; return bitset.view(data[..], 1) }",
            true,
        ),
        (
            "owner",
            "fn f() { mut data := [0 as u8].to_array(); bits := bitset.view(data, 1); data = [1 as u8].to_array(); print(bits[0]) }",
            true,
        ),
        (
            "readonly",
            "fn f() { mut bits := bitset.view(\"a\".bytes(), 3); bits[0] = false }",
            true,
        ),
        (
            "validated",
            "fn f() { data := [65 as u8].to_array(); bytes: slice<u8> := data; text := bytes.as_str() else { return }; mut bits := bitset.view(bytes, 8); bits[7] = true; print(text) }",
            true,
        ),
        (
            "captured",
            "fn f() { mut data := [0 as u8]; bits := bitset.view(data[..], 1); task_group { spawn(fn {bits[0]}); data[0] = 7; wait() } }",
            true,
        ),
        (
            "distinct",
            "fn f() { mut a := [0 as u8]; mut b := [0 as u8]; bits := bitset.view(a[..], 1); task_group { spawn(fn {bits[0]}); b[0] = 7; wait() } }",
            false,
        ),
        ("arity", "fn f(bits: bitset<i64>) {}", true),
        (
            "constructor_type",
            "fn f(data:slice<u8>, width:i32) { bits:=bitset.view(data,width) }",
            true,
        ),
        (
            "index_type",
            "fn f(bits:bitset, k:i32)->bool = bits[k]",
            true,
        ),
        (
            "uninitialized_source",
            "fn f() { mut data:=[0 as u8].to_array(); bits:=bitset.view(data,{ data=[1 as u8].to_array(); 1 }); print(bits[0]) }",
            true,
        ),
        (
            "downgrade",
            "fn f(ro:bitset) { mut data:=[0 as u8]; mut bits:=bitset.view(data[..],1); bits[0]={bits=ro; true} }",
            true,
        ),
        (
            "share_byte",
            "fn f() { mut data:=[0 as u8]; bits:=bitset.view(data[..],8); mut left:=bits[..1]; right:=bits[1..2]; task_group {spawn(fn {right[0]}); left[0]=true; wait()} }",
            true,
        ),
        (
            "bare_function_return",
            "fn forward(bits:bitset)->bitset = bits\nfn f() { chosen:fn(bitset)->bitset := forward }",
            true,
        ),
        ("literal", "fn f(bits:bitset) { values:=[bits] }", true),
        (
            "packed_soa",
            "Flags { bits:bitset }\nfn f(flags:soa<Flags>) {}",
            true,
        ),
        (
            "shared_mut_call",
            "fn edit(borrow mut v:bitset) {v[0]=true}\nfn f(bytes:slice<u8>) {mut bits:=bitset.view(bytes,1); edit(bits)}",
            true,
        ),
        (
            "shared_task_mutate",
            "fn f() {mut bytes:=[0 as u8]; bits:=bitset.view(bytes[..],8); task_group {spawn(fn {mut view:=bits; view[2]=true; view[2]}); wait()} }",
            true,
        ),
        ("collection", "fn f(bits: array<bitset>) {}", true),
        ("box", "fn f(bits: box<bitset>) {}", true),
        ("c_record", "layout(C) CBits { bits: bitset }", true),
        ("extern", "extern \"C\" fn probe(bits: bitset)", true),
    ] {
        let source = format!("import core.bitset\n{body}\nfn main() {{}}\n");
        let files = [("main.align", source.as_str())];
        let result = diff_check_multi(name, &files, "main.align");
        assert_eq!(
            result.whole_errors, reject,
            "{name}: {body}\n{}",
            result.whole_diags
        );
        assert_eq!(
            result.per_unit_errors, reject,
            "{name}: {body}\n{}",
            result.per_unit_diags
        );
    }
}

#[cfg(unix)]
#[test]
fn guard_pages_bound_every_access() {
    if !backend_available() || !cc_available() {
        return;
    }
    let owner = r#"module pkg.bitmap
import pkg.bitmap.internal.drop
import core.bitset
pub resource mapping = pkg.bitmap.internal.drop.release
extern "C" fn protected_mapping()->raw
extern "C" fn window_bytes(p:raw, length:i64, end:i32)->raw
pub fn probe(length:i64, end:bool)->i64 {
  unsafe {
    p := protected_mapping()
    mut memory: mapping := resource.from_raw(p)
    ptr := window_bytes(p, length, if end { 1 } else { 0 })
    bytes: slice<u8> := resource.view_mut_from_raw(memory, ptr, length) else { return -1 }
    mut bits := bitset.view(bytes, length * 8)
    mut head := 0
    loop {
      if head > 7 || head > bits.len() {break}
      cut := bits[head..]
      if cut.count() != cut.len() || !cut.all() || cut.any() != (cut.len() > 0) {return -2}
      if cut.len() > 0 && (!cut[0] || !cut[cut.len()-1]) {return -3}
      head = head + 1
    }
    if bits.len() > 0 {
      last := bits.len()-1
      bits[last] = false
      if bits[last] || bits.count() != last {return -4}
      bits[last] = true
      if !bits.all() {return -5}
    }
    return 0
  }
}
"#;
    let drop = r#"module pkg.bitmap.internal.drop
extern "C" fn release_mapping(p: raw)
pub fn release(p:raw) { unsafe { release_mapping(p) } }
"#;
    let main = r#"import pkg.bitmap
fn main()->i32 {
  mut length:=0
  loop {
    if length==34 { break }
    if pkg.bitmap.probe(length,true)!=0 || pkg.bitmap.probe(length,false)!=0 { return 1 }
    length=length+1
  }
  return 0
}
"#;
    // The zero-length window can start at the first inaccessible byte.
    let native = r#"
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
static size_t page(void) { return (size_t)sysconf(_SC_PAGESIZE); }
void *protected_mapping(void) {
    size_t size = page();
    unsigned char *p = mmap(NULL, 3 * size, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED || mprotect(p + size, size, PROT_READ | PROT_WRITE)) abort();
    memset(p + size, 255, size);
    return p;
}
void *window_bytes(unsigned char *p, int64_t n, int32_t end) {
    return end ? p + 2 * page() - n : p + page();
}
void release_mapping(void *p) { if (munmap(p, 3 * page())) abort(); }
"#;
    let output = build_and_run_multi_with_c(
        "packed-bits-guard",
        &[
            ("pkg/bitmap.align", owner),
            ("pkg/bitmap/internal/drop.align", drop),
            ("main.align", main),
        ],
        "main.align",
        native,
    );
    assert!(
        output.status.success(),
        "{:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn invalid_bounds_abort_and_assignment_reloads_the_destination() {
    if !backend_available() {
        return;
    }
    let source = r#"import core.bitset
fn terminal()->i64 {
  mut bytes := [0 as u8]
  mut bits := bitset.view(bytes[..], 0)
  bits[9] = {return 17; true}
  return 0
}
fn main()->i32 {
  mut a := [0 as u8]
  mut b := [255 as u8]
  mut first := bitset.view(a[..], 8)
  second := bitset.view(b[..], 8)
  first[1] = {first = second; false}
  if a[0] != 0 || b[0] != 253 || terminal() != 17 { return 41 }
  mut upgraded := bitset.view("immutable".bytes(), 1)
  upgraded = bitset.view(a[..], 8)
  upgraded[0] = true
  if a[0] != 1 { return 43 }
  mut owner := [0 as u8].to_array()
  mut owned_bits := bitset.view(owner,8)
  owned_bits[7] = true
  if owner[0] != 128 {return 44}
  mut buffer_owner := buffer.filled(2, 0 as u8)
  mut buffer_bits := bitset.view(buffer_owner.bytes(),16)
  buffer_bits[8] = true
  if buffer_owner.bytes()[1] != 1 {return 45}
  tail := first[1..7]
  if tail[1..4].count() != 3 || first[8..].any() {return 42}
  return 0
}
"#;
    let output = build_and_run("packed-bits-rebind", source);
    assert!(output.status.success(), "{output:?}");
    for operation in [
        "print(bitset.view(data[..], -1).len())",
        "print(bitset.view(data[..], 9223372036854775807).len())",
        "print(bits[-1])",
        "print(bits[8])",
        "bits[8] = true",
        "print(bits[-1..1].len())",
        "print(bits[1..0].len())",
        "print(bits[..9].len())",
    ] {
        let source = format!(
            "import core.bitset\nfn main() {{ mut data:=[0 as u8]; mut bits:=bitset.view(data[..],8); {operation} }}"
        );
        let output = build_and_run("packed-bits-invalid", &source);
        assert!(
            !output.status.success(),
            "{operation} accepted an invalid bound"
        );
    }
}
