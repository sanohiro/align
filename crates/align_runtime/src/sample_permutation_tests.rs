//! Partial-permutation equivalence and actual scratch-allocation owners.
use super::*;

fn enumerate(slots: SampleSlots, dense: Vec<usize>, i: usize) {
    if i == dense.len() {
        return;
    }
    for j in i..dense.len() {
        let mut next_slots = slots.clone();
        let mut next_dense = dense.clone();
        next_dense.swap(i, j);
        assert_eq!(sample_sparse_select(&mut next_slots, i, j), next_dense[i]);
        assert!(next_slots.keys().all(|&key| key > i));
        assert!(next_slots.len() <= i + 1);
        for (position, expected) in next_dense.iter().enumerate().skip(i + 1) {
            assert_eq!(next_slots.get(&position).unwrap_or(&position), expected);
        }
        enumerate(next_slots, next_dense, i + 1);
    }
}

#[test]
fn sample_permutation_all_small_draw_sequences_preserve_the_suffix() {
    // Every full sequence also visits every possible shorter prefix. This covers self-swaps,
    // repeated destinations, displaced source/destination slots, and every stopping point k.
    for n in 0..=7 {
        enumerate(SampleSlots::default(), (0..n).collect(), 0);
    }
}

fn dense_oracle(s: &mut [u64; 4], n: usize, k: usize) -> Result<Vec<usize>, String> {
    let mut indices: Vec<usize> = (0..n).collect();
    for i in 0..k {
        let width = u64::try_from(n - i).map_err(|e| e.to_string())?;
        let j = i + usize::try_from(bounded(s, width)).map_err(|e| e.to_string())?;
        indices.swap(i, j);
    }
    indices.truncate(k);
    Ok(indices)
}

#[test]
fn sample_permutation_dispatch_matches_dense_output_and_final_rng() -> Result<(), String> {
    for n in [0usize, 1, 2, 7, 511, 512, 513, 1024, 65_536] {
        let boundary = n / 512;
        for k in [
            0,
            1.min(n),
            boundary.saturating_sub(1),
            boundary,
            (boundary + 1).min(n),
            n / 2,
            n,
        ] {
            for seed in [0, 1, 7, 42, u64::MAX, 0x8000_0000_0000_0000] {
                let mut actual_state = splitmix64_state(seed);
                let mut expected_state = actual_state;
                let expected = dense_oracle(&mut expected_state, n, k)?;
                let mut actual = Vec::with_capacity(k);
                sample_indices(&mut actual_state, n, k, |position, index| {
                    assert_eq!(position, actual.len());
                    actual.push(index);
                });
                assert_eq!(actual, expected, "n={n} k={k} seed={seed}");
                assert_eq!(actual_state, expected_state, "n={n} k={k} seed={seed}");
            }
        }
    }
    Ok(())
}

struct ArrayOwner(AlignStr);
impl Drop for ArrayOwner {
    fn drop(&mut self) {
        unsafe { align_rt_free(self.0.ptr.cast_mut()) };
    }
}

#[test]
fn sample_permutation_native_copy_is_independent_for_every_stride() -> Result<(), String> {
    for (n, k) in [
        (0usize, 0usize),
        (64, 0),
        (64, 8),
        (64, 64),
        (4096, 8),
        (4096, 9),
    ] {
        for stride in [1usize, 3, 8, 24] {
            let mut source: Vec<u8> = (0..=u8::MAX).cycle().take(n * stride).collect();
            let original = source.clone();
            let mut state = splitmix64_state(123);
            let mut expected_state = state;
            let indices = dense_oracle(&mut expected_state, n, k)?;
            let expected: Vec<u8> = indices
                .iter()
                .flat_map(|&index| {
                    original[index * stride..(index + 1) * stride]
                        .iter()
                        .copied()
                })
                .collect();
            let output = ArrayOwner(unsafe {
                align_rt_rng_sample(
                    state.as_mut_ptr(),
                    source.as_ptr(),
                    i64::try_from(n).map_err(|e| e.to_string())?,
                    i64::try_from(k).map_err(|e| e.to_string())?,
                    i64::try_from(stride).map_err(|e| e.to_string())?,
                )
            });
            assert_eq!(source, original, "sample mutated the source");
            source.fill(0xff);
            assert_eq!(output.0.len, i64::try_from(k).map_err(|e| e.to_string())?);
            assert_eq!(state, expected_state);
            if k == 0 {
                assert!(output.0.ptr.is_null());
            } else {
                assert!(!output.0.ptr.is_null());
                assert_eq!(
                    unsafe { core::slice::from_raw_parts(output.0.ptr, k * stride) },
                    expected
                );
            }
        }
    }
    Ok(())
}

#[cfg(feature = "alloc-count")]
#[test]
fn sample_permutation_sparse_scratch_depends_on_k_not_n() {
    let before = global_alloc_bytes();
    let witness = core::hint::black_box(vec![0u8; 4096]);
    assert_eq!(
        global_alloc_bytes() - before,
        4096,
        "counter must observe requested bytes"
    );
    drop(witness);
    for k in [0usize, 1, 8, 128] {
        let mut previous = None;
        for n in [65_536, 1_048_576] {
            let mut state = splitmix64_state(42);
            let before_count = global_alloc_count();
            let before_bytes = global_alloc_bytes();
            let mut emitted = 0;
            sample_indices(
                &mut state,
                core::hint::black_box(n),
                k,
                |position, index| {
                    assert_eq!(position, emitted);
                    assert!(core::hint::black_box(index) < n);
                    emitted += 1;
                },
            );
            let observed = (
                global_alloc_count() - before_count,
                global_alloc_bytes() - before_bytes,
            );
            assert_eq!(emitted, k);
            assert_eq!(observed.0, u64::from(k != 0), "scratch must not grow");
            if k == 0 {
                assert_eq!(observed.1, 0);
            } else {
                assert!(
                    observed.1 <= 128 + 64 * k,
                    "unexpected scratch request: {observed:?}"
                );
            }
            if let Some(previous) = previous {
                assert_eq!(observed, previous, "fixed k scratch must not depend on n");
            }
            previous = Some(observed);
            eprintln!(
                "n={n} k={k} scratch_calls={} scratch_requested_bytes={}",
                observed.0, observed.1
            );
        }
    }
}
