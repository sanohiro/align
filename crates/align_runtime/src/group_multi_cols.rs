use super::{AlignStr, WyBuildHasher, WyKey, align_rt_alloc, safe_len};
use std::collections::{HashMap, hash_map::Entry};
use std::hash::{BuildHasher, Hash};
use std::mem::size_of;
use std::ptr;

#[repr(C)]
pub struct GroupColumnSpec {
    pub values: *const i64,
    pub op: i64,
}

fn byte_extent<T>(count: usize) -> Option<i64> {
    let bytes = count.checked_mul(size_of::<T>())?;
    isize::try_from(bytes).ok()?;
    i64::try_from(bytes).ok()
}

fn pointer_ok<T>(pointer: *const T, count: usize) -> bool {
    count == 0 || (!pointer.is_null() && pointer.is_aligned())
}

/// Stage the full reduction before allocating any owned payload or publishing a header.
unsafe fn fold<K: Hash + Eq, R: Copy, S: BuildHasher>(
    n: usize,
    specs: &[GroupColumnSpec],
    headers: *mut AlignStr,
    hasher: S,
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
                let next = start.checked_add(specs.len())?;
                byte_extent::<i64>(next)?;
                accumulators.resize(next, 0);
                entry.insert(start);
                representatives.push(repr);
                (start, true)
            }
        };
        for (spec, acc) in specs
            .iter()
            .zip(&mut accumulators[start..start + specs.len()])
        {
            let value = if spec.op == 3 {
                1
            } else {
                unsafe { *spec.values.add(i) }
            };
            *acc = if fresh {
                value
            } else {
                match spec.op {
                    1 => (*acc).min(value),
                    2 => (*acc).max(value),
                    _ => acc.wrapping_add(value),
                }
            };
        }
    }
    unsafe { publish(&representatives, &accumulators, specs.len(), headers) }
}

/// Both key domains finish staging and all fallible checks before publishing.
unsafe fn publish<R: Copy>(
    representatives: &[R],
    accumulators: &[i64],
    columns: usize,
    headers: *mut AlignStr,
) -> Option<i64> {
    let groups = representatives.len();
    let count = i64::try_from(groups).ok()?;
    let keys_size = byte_extent::<R>(groups)?;
    let values_size = byte_extent::<i64>(groups)?;
    // Every validation and fallible size computation precedes the first owned allocation.
    let keys = align_rt_alloc(keys_size);
    if groups != 0 {
        unsafe { ptr::copy_nonoverlapping(representatives.as_ptr(), keys.cast::<R>(), groups) };
    }
    unsafe {
        headers.write(AlignStr {
            ptr: keys,
            len: count,
        })
    };
    for column in 0..columns {
        let values = align_rt_alloc(values_size).cast::<i64>();
        for (i, group) in accumulators.chunks_exact(columns).enumerate() {
            unsafe { values.add(i).write(group[column]) };
        }
        unsafe {
            headers.add(column + 1).write(AlignStr {
                ptr: values.cast(),
                len: count,
            })
        };
    }
    Some(count)
}

/// Select the reduction once per column and tile. Offsets identify the
/// beginning of an existing group's K cells; column is below K.
unsafe fn reduce_values(
    offsets: &[usize],
    column: usize,
    values: &[i64],
    accumulators: &mut [i64],
    combine: impl Fn(i64, i64) -> i64,
) {
    for (&offset, &value) in offsets.iter().zip(values) {
        // The caller interns every offset as the start of a full row.
        let cell = unsafe { accumulators.get_unchecked_mut(offset + column) };
        *cell = combine(*cell, value);
    }
}

#[derive(PartialEq, Eq)]
struct IntegerKey(i64);

impl Hash for IntegerKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(super::group_hash(self.0));
    }
}

/// A direct-mapped table resolves repeated keys with one probe. Keys sharing a slot
/// use the full hash map. No preliminary key-range scan is necessary.
unsafe fn fold_integers(
    keys: &[i64],
    specs: &[GroupColumnSpec],
    headers: *mut AlignStr,
) -> Option<i64> {
    if keys.is_empty() {
        return unsafe { publish::<i64>(&[], &[], specs.len(), headers) };
    }
    let width = (keys.len() / 4).clamp(64, 65536).next_power_of_two();
    let mask = (width - 1) as u64;
    let mut direct = vec![(0i64, 0usize); width];
    let mut distant = HashMap::with_hasher(WyBuildHasher::default());
    let mut representatives = Vec::new();
    let mut accumulators = Vec::new();
    let mut offsets = [0usize; 256];
    for (tile, chunk) in keys.chunks(offsets.len()).enumerate() {
        let row = tile * offsets.len();
        let offsets = &mut offsets[..chunk.len()];
        for (slot, &key) in offsets.iter_mut().zip(chunk) {
            let bits = key as u64;
            let index = usize::try_from((bits ^ (bits >> 16) ^ (bits >> 48)) & mask).ok()?;
            let (tag, direct_id) = &mut direct[index];
            if *direct_id != 0 && *tag == key {
                *slot = *direct_id - 1;
                continue;
            }
            let id = if *direct_id == 0 {
                *tag = key;
                direct_id
            } else {
                distant.entry(IntegerKey(key)).or_insert(0)
            };
            if *id == 0 {
                let start = accumulators.len();
                let end = start.checked_add(specs.len())?;
                byte_extent::<i64>(end)?;
                accumulators.extend(specs.iter().map(|s| match s.op {
                    1 => i64::MAX,
                    2 => i64::MIN,
                    _ => 0,
                }));
                representatives.push(key);
                *id = start + 1;
            }
            *slot = *id - 1;
        }
        for (column, spec) in specs.iter().enumerate() {
            if spec.op == 3 {
                for &offset in offsets.iter() {
                    // Every interned offset starts a row of specs.len() cells.
                    unsafe { *accumulators.get_unchecked_mut(offset + column) += 1 };
                }
            } else {
                // Validation guarantees a live, aligned value column of keys.len() elements.
                let values =
                    unsafe { std::slice::from_raw_parts(spec.values.add(row), chunk.len()) };
                unsafe {
                    match spec.op {
                        1 => reduce_values(offsets, column, values, &mut accumulators, i64::min),
                        2 => reduce_values(offsets, column, values, &mut accumulators, i64::max),
                        _ => reduce_values(
                            offsets,
                            column,
                            values,
                            &mut accumulators,
                            i64::wrapping_add,
                        ),
                    }
                }
            }
        }
    }
    unsafe { publish(&representatives, &accumulators, specs.len(), headers) }
}

/// Return the number of groups or -1 without changing the caller's headers.
/// Successful nonempty payloads transfer to the caller for individual align_rt_free calls.
///
/// # Safety
/// All nonempty input ranges must be live and readable. The spec table contains k entries.
/// The caller supplies k+1 aligned writable header cells disjoint from every input, including
/// represented string bytes. Borrowed string key bytes outlive their use from the returned array.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn align_rt_group_multi_cols(
    keys: *const u8,
    specs: *const GroupColumnSpec,
    n: i64,
    headers: *mut AlignStr,
    k: i64,
    key_kind: i64,
) -> i64 {
    let (Ok(n), Ok(k)) = (safe_len(n), safe_len(k)) else {
        return -1;
    };
    let Some(columns) = k.checked_add(1) else {
        return -1;
    };
    if !matches!(key_kind, 0 | 1)
        || k == 0
        || byte_extent::<GroupColumnSpec>(k).is_none()
        || byte_extent::<AlignStr>(columns).is_none()
        || byte_extent::<i64>(n).is_none()
        || (key_kind == 1 && byte_extent::<AlignStr>(n).is_none())
        || !pointer_ok(specs, k)
        || !pointer_ok(keys.cast::<i64>(), n)
        || !pointer_ok(headers, columns)
    {
        return -1;
    }
    let specs = unsafe { std::slice::from_raw_parts(specs, k) };
    for spec in specs {
        if !matches!(spec.op, 0..=3)
            || (spec.op == 3 && !spec.values.is_null())
            || (spec.op != 3 && !pointer_ok(spec.values, n))
        {
            return -1;
        }
    }
    let count = if key_kind == 0 {
        let keys = if n == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(keys.cast::<i64>(), n) }
        };
        unsafe { fold_integers(keys, specs, headers) }
    } else {
        unsafe {
            fold(n, specs, headers, WyBuildHasher::default(), |i| {
                let key = *keys.cast::<AlignStr>().add(i);
                let len = safe_len(key.len).ok()?;
                let bytes = if len == 0 {
                    &[][..]
                } else {
                    if key.ptr.is_null() {
                        return None;
                    }
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
    use super::super::align_rt_free;
    use super::*;

    #[test]
    fn typed_tables_validate_before_publication_and_allocate_exact_columns() {
        assert_eq!(size_of::<GroupColumnSpec>(), 16);
        let keys = [i64::MIN, i64::MAX, i64::MIN];
        let values = [i64::MAX, 7, 1];
        let mut spec = GroupColumnSpec {
            values: values.as_ptr(),
            op: 0,
        };
        let sentinel = AlignStr {
            ptr: ptr::null(),
            len: 123,
        };
        let mut headers = [sentinel; 3];
        let output = headers.as_mut_ptr();
        let input = keys.as_ptr().cast();
        let run = |s, table, n, out, k, tag| unsafe {
            align_rt_group_multi_cols(s, table, n, out, k, tag)
        };
        for (n, k, tag) in [
            (-1, 1, 0),
            (1, 0, 0),
            (1, -1, 0),
            (1, 1, 2),
            (i64::MAX, 1, 0),
            (1, i64::MAX, 0),
        ] {
            assert_eq!(run(input, &spec, n, output, k, tag), -1);
        }
        assert_eq!(run(input, ptr::null(), 1, output, 1, 0), -1);
        assert_eq!(run(ptr::null(), &spec, 1, output, 1, 0), -1);
        assert_eq!(run(ptr::null(), &spec, 0, ptr::null_mut(), 1, 0), -1);
        spec.op = 4;
        assert_eq!(run(input, &spec, 0, output, 1, 0), -1);
        spec.op = 0;
        let invalid = [
            GroupColumnSpec {
                values: values.as_ptr(),
                op: 2,
            },
            GroupColumnSpec {
                values: ptr::null(),
                op: 0,
            },
        ];
        assert_eq!(run(input, invalid.as_ptr(), 3, output, 2, 0), -1);
        assert!(headers.iter().all(|h| h.ptr.is_null() && h.len == 123));
        assert_eq!(run(input, &spec, 3, output, 1, 0), 2);
        let saved = [headers[0], headers[1]];
        assert_eq!(run(ptr::null(), &spec, 0, output, 1, 0), 0);
        assert!(headers[..2].iter().all(|h| h.ptr.is_null() && h.len == 0));
        assert_eq!(headers[2].len, 123);
        for (header, expected) in saved.into_iter().zip([[i64::MIN, i64::MAX], [i64::MIN, 7]]) {
            assert_eq!(header.len, 2);
            assert_eq!(
                unsafe { std::slice::from_raw_parts(header.ptr.cast::<i64>(), 2) },
                expected
            );
            unsafe { align_rt_free(header.ptr.cast_mut()) };
        }
    }

    #[test]
    fn integer_groups_keep_all_columns_aligned_across_tiles_and_collisions() {
        let domain = [0, 65_537, 131_074, i64::MIN, i64::MAX, -1, 1];
        let keys: Vec<_> = (0..777).map(|i| domain[i % domain.len()]).collect();
        let values: Vec<_> = (0..777i64)
            .map(|i| match i % 7 {
                0 => i64::MAX,
                1 => i64::MIN,
                _ if i % 3 == 0 => i64::MAX,
                _ => -i,
            })
            .collect();
        let ops = [
            GroupColumnSpec {
                op: 3,
                values: ptr::null(),
            },
            GroupColumnSpec {
                op: 2,
                values: values.as_ptr(),
            },
            GroupColumnSpec {
                op: 0,
                values: keys.as_ptr(),
            },
            GroupColumnSpec {
                op: 1,
                values: values.as_ptr(),
            },
            GroupColumnSpec {
                op: 0,
                values: values.as_ptr(),
            },
            GroupColumnSpec {
                op: 1,
                values: values.as_ptr(),
            },
        ];
        let mut expected = std::collections::BTreeMap::new();
        for (&key, &value) in keys.iter().zip(&values) {
            let entry = expected
                .entry(key)
                .or_insert([0i64, value, 0, value, 0, value]);
            entry[0] += 1;
            entry[1] = entry[1].max(value);
            entry[2] = entry[2].wrapping_add(key);
            entry[3] = entry[3].min(value);
            entry[4] = entry[4].wrapping_add(value);
            entry[5] = entry[5].min(value);
        }
        let mut headers = [AlignStr {
            ptr: ptr::null(),
            len: 0,
        }; 7];
        let count = unsafe {
            align_rt_group_multi_cols(
                keys.as_ptr().cast(),
                ops.as_ptr(),
                777,
                headers.as_mut_ptr(),
                6,
                0,
            )
        };
        assert_eq!(count, 7);
        let columns = headers.map(|h| {
            assert_eq!(h.len, count);
            unsafe { std::slice::from_raw_parts(h.ptr.cast::<i64>(), 7) }
        });
        for (row, key) in columns[0].iter().enumerate() {
            let cells = expected.remove(key).unwrap();
            for (column, value) in columns[1..].iter().zip(cells) {
                assert_eq!(column[row], value);
            }
        }
        assert!(expected.is_empty());
        for h in headers {
            unsafe { align_rt_free(h.ptr.cast_mut()) };
        }
    }

    #[test]
    fn borrowed_keys_compare_bytes_and_reject_malformed_headers_atomically() {
        let a = [b'x', 0];
        let b = [b'x', 0];
        let mut keys = [
            AlignStr {
                ptr: a.as_ptr(),
                len: 2,
            },
            AlignStr {
                ptr: ptr::null(),
                len: 0,
            },
            AlignStr {
                ptr: b.as_ptr(),
                len: 2,
            },
        ];
        let sentinel = AlignStr {
            ptr: b.as_ptr(),
            len: 7,
        };
        let mut headers = [sentinel; 2];
        let spec = GroupColumnSpec {
            values: ptr::null(),
            op: 3,
        };
        let call = |keys: &[AlignStr], out: &mut [AlignStr]| unsafe {
            align_rt_group_multi_cols(keys.as_ptr().cast(), &spec, 3, out.as_mut_ptr(), 1, 1)
        };
        let copy = keys[2];
        for invalid in [
            AlignStr {
                ptr: ptr::null(),
                len: 1,
            },
            AlignStr {
                ptr: a.as_ptr(),
                len: -1,
            },
        ] {
            keys[2] = invalid;
            assert_eq!(call(&keys, &mut headers), -1);
            assert!(
                headers
                    .iter()
                    .all(|h| h.ptr == sentinel.ptr && h.len == sentinel.len)
            );
        }
        keys[2] = copy;
        assert_eq!(call(&keys, &mut headers), 2);
        let reps = unsafe { std::slice::from_raw_parts(headers[0].ptr.cast::<AlignStr>(), 2) };
        assert_eq!(reps[0].ptr, a.as_ptr());
        assert_eq!(reps[0].len, 2);
        assert_eq!(reps[1].len, 0);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(headers[1].ptr.cast::<i64>(), 2) },
            [2, 1]
        );
        for h in headers {
            unsafe { align_rt_free(h.ptr.cast_mut()) };
        }
    }
}
