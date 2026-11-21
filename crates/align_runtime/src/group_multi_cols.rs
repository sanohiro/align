use super::{AlignStr, WyBuildHasher, WyKey, safe_len};
use std::collections::{HashMap, hash_map::Entry};
use std::hash::{BuildHasher, Hash};
use std::mem::size_of;

#[repr(C)]
pub struct GroupColumnSpec {
    pub values: *const i64,
    pub op: i64,
    pub out: *mut i64,
}

fn fits<T>(count: usize) -> bool {
    count.checked_mul(size_of::<T>()).is_some_and(|n| isize::try_from(n).is_ok())
}

fn pointer_ok<T>(pointer: *const T, count: usize) -> bool {
    count == 0 || (!pointer.is_null() && pointer.is_aligned())
}

/// Each row supplies one key and K scalar updates. Publish only after all rows validate.
/// The caller guarantees the validated nonempty input and output ranges are live and disjoint
/// where writable. The map and the accumulator vector never escape the call.
unsafe fn fold<K: Hash + Eq, R: Copy, S: BuildHasher>(
    n: usize, specs: &[GroupColumnSpec], out_keys: *mut R, hasher: S,
    mut key_at: impl FnMut(usize) -> Option<(K, R)>,
) -> Option<i64> {
    let mut ids = HashMap::with_capacity_and_hasher(n.min(1024), hasher);
    let mut representatives = Vec::new();
    let mut accumulators: Vec<i64> = Vec::new();
    for i in 0..n {
        let (key, repr) = key_at(i)?;
        let (start, fresh) = match ids.entry(key) {
            Entry::Occupied(entry) => (*entry.get(), false),
            Entry::Vacant(entry) => {
                let start = accumulators.len();
                let next = start.checked_add(specs.len()).filter(|&n| fits::<i64>(n))?;
                accumulators.resize(next, 0);
                entry.insert(start);
                representatives.push(repr);
                (start, true)
            }
        };
        for (spec, acc) in specs.iter().zip(&mut accumulators[start..start + specs.len()]) {
            let value = if spec.op == 3 { 1 } else { unsafe { *spec.values.add(i) } };
            *acc = if fresh { value } else {
                match spec.op {
                    1 => (*acc).min(value),
                    2 => (*acc).max(value),
                    _ => acc.wrapping_add(value),
                }
            };
        }
    }
    let count = i64::try_from(representatives.len()).ok()?;
    if count > 0 {
        unsafe { std::ptr::copy_nonoverlapping(representatives.as_ptr(), out_keys, representatives.len()) };
        for (index, group) in accumulators.chunks_exact(specs.len()).enumerate() {
            for (spec, &value) in specs.iter().zip(group) {
                unsafe { spec.out.add(index).write(value) };
            }
        }
    }
    Some(count)
}

/// Fused SoA group-by. Return the number of groups, or -1 before any output write.
///
/// # Safety
/// The spec table and all nonempty typed column ranges must be live and valid for their stated
/// lengths. String key byte ranges must be readable through consumption of returned key views.
/// Outputs must be disjoint from each other, the table, input columns and all represented bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_multi_cols(
    keys: *const u8, specs: *const GroupColumnSpec, n: i64, out_keys: *mut u8, k: i64, key_kind: i64,
) -> i64 {
    let (Ok(n), Ok(k)) = (safe_len(n), safe_len(k)) else { return -1 };
    if !matches!(key_kind, 0 | 1) || k == 0 || !fits::<GroupColumnSpec>(k)
        || !fits::<i64>(n) || (key_kind == 1 && !fits::<AlignStr>(n))
        || !pointer_ok(specs, k) || !pointer_ok(keys.cast::<i64>(), n)
        || !pointer_ok(out_keys.cast::<i64>(), n) {
        return -1;
    }
    let specs = unsafe { std::slice::from_raw_parts(specs, k) };
    for spec in specs {
        if !matches!(spec.op, 0..=3) || !pointer_ok(spec.out, n)
            || (spec.op == 3 && !spec.values.is_null())
            || (spec.op != 3 && !pointer_ok(spec.values, n)) {
            return -1;
        }
    }
    if n == 0 { return 0; }
    let count = if key_kind == 0 {
        unsafe {
            fold(n, specs, out_keys.cast::<i64>(), std::hash::BuildHasherDefault::<std::hash::DefaultHasher>::default(), |i| {
                let key = *keys.cast::<i64>().add(i);
                Some((key, key))
            })
        }
    } else {
        unsafe {
            fold(n, specs, out_keys.cast::<AlignStr>(), WyBuildHasher::default(), |i| {
                let key = *keys.cast::<AlignStr>().add(i);
                let len = safe_len(key.len).ok()?;
                let bytes = if len == 0 { &[][..] } else {
                    if key.ptr.is_null() { return None; }
                    std::slice::from_raw_parts(key.ptr, len)
                };
                Some((WyKey::new(bytes), key))
            })
        }
    };
    count.unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::{null, null_mut};

    #[test]
    fn typed_columns_validate_before_publication() {
        assert_eq!(size_of::<GroupColumnSpec>(), 24);
        let keys = [i64::MIN, i64::MAX, i64::MIN];
        let values = [i64::MAX, 7, 1];
        let mut output_keys = [9_i64; 3];
        let mut results = [9_i64; 3];
        let mut spec = GroupColumnSpec { values: values.as_ptr(), op: 0, out: results.as_mut_ptr() };
        let key_ptr = keys.as_ptr().cast();
        let output_ptr = output_keys.as_mut_ptr().cast();
        let run = |source, specs, n, output, k, tag| unsafe {
            align_rt_group_multi_cols(source, specs, n, output, k, tag)
        };
        assert_eq!(run(null(), &spec, 0, null_mut(), 1, 0), 0);
        for (n, k, tag) in [(-1,1,0),(1,0,0),(1,-1,0),(1,1,2),(i64::MAX,1,0),(1,i64::MAX,0)] {
            assert_eq!(run(key_ptr, &spec, n, output_ptr, k, tag), -1);
        }
        assert_eq!(run(key_ptr, null(), 1, output_ptr, 1, 0), -1);
        assert_eq!(run(null(), &spec, 1, output_ptr, 1, 0), -1);
        assert_eq!(run(key_ptr, &spec, 1, null_mut(), 1, 0), -1);
        spec.op = 4;
        assert_eq!(run(key_ptr, &spec, 0, output_ptr, 1, 0), -1);
        spec.op = 0;
        let mut other_output = [7_i64; 3];
        let invalid = [
            GroupColumnSpec { values: values.as_ptr(), op: 2, out: results.as_mut_ptr() },
            GroupColumnSpec { values: null(), op: 0, out: other_output.as_mut_ptr() },
        ];
        assert_eq!(run(key_ptr, invalid.as_ptr(), 3, output_ptr, 2, 0), -1);
        assert_eq!(output_keys, [9; 3]);
        assert_eq!(results, [9; 3]);
        assert_eq!(run(key_ptr, &spec, 3, output_ptr, 1, 0), 2);
        assert_eq!(&output_keys[..2], &[i64::MIN, i64::MAX]);
        assert_eq!(&results[..2], &[i64::MIN, 7]);
    }

    #[test]
    fn borrowed_keys_compare_bytes_and_reject_malformed_headers_atomically() {
        let a = [b'x', 0];
        let b = [b'x', 0];
        let mut keys = [
            AlignStr { ptr: a.as_ptr(), len: 2 },
            AlignStr { ptr: null(), len: 0 },
            AlignStr { ptr: b.as_ptr(), len: 2 },
        ];
        let sentinel = AlignStr { ptr: b.as_ptr(), len: 7 };
        let mut output = [sentinel; 3];
        let mut counts = [7_i64; 3];
        let spec = GroupColumnSpec { values: null(), op: 3, out: counts.as_mut_ptr() };
        let call = |keys: &[AlignStr], out: &mut [AlignStr]| unsafe {
            align_rt_group_multi_cols(keys.as_ptr().cast(), &spec, 3, out.as_mut_ptr().cast(), 1, 1)
        };
        let copy = keys[2];
        for invalid in [AlignStr { ptr: null(), len: 1 }, AlignStr { ptr: a.as_ptr(), len: -1 }] {
            keys[2] = invalid;
            assert_eq!(call(&keys, &mut output), -1);
            assert!(output.iter().all(|key| key.ptr == sentinel.ptr && key.len == sentinel.len));
            assert_eq!(counts, [7; 3]);
        }
        keys[2] = copy;
        assert_eq!(call(&keys, &mut output), 2);
        assert_eq!(output[0].ptr, a.as_ptr());
        assert_eq!(output[0].len, 2);
        assert_eq!(output[1].len, 0);
        assert_eq!(&counts[..2], &[2, 1]);
    }
}
