//! Align's one canonical non-cryptographic hash — **wyhash final v3**.
//!
//! Dependency-free, strong-avalanche, ~40 lines (vs an `ahash`/AES dependency or FxHash's weaker
//! mixing). This is THE non-crypto hash of the whole toolchain, the "one way to hash bytes":
//!
//! - the `hash64`/`hash128` builtins (`align_runtime`) call it,
//! - `group_by` / `dict_encode` string interning (`align_runtime`) keys on it,
//! - and the compile-time JSON perfect-hash table (`align_codegen_llvm`) **and** its runtime probe
//!   (`align_runtime`) both call it — so the codegen↔runtime PHF byte-match is *structural* (one
//!   function, one seed convention; the two ends cannot drift).
//!
//! A given seed → deterministic output within a build. **NOT cryptographic:** not DoS-resistant,
//! not a stable on-disk/wire format, not for security (crypto hashes live in `std.crypto`).
//!
//! Reference (public domain): <https://github.com/wangyi-fudan/wyhash>

#![forbid(unsafe_code)]

/// wyhash's default secret (`_wyp`). Public so the `hash128` second pass can derive its lane seed
/// from `WY_SECRET[2]` while still using the one `wyhash`.
pub const WY_SECRET: [u64; 4] = [
    0xa076_1d64_78bd_642f,
    0xe703_7ed1_a0b4_28db,
    0x8ebc_6af0_9c88_c6e3,
    0x5899_65cc_7537_4cc3,
];

/// The fixed seed for the canonical `hash64`/`hash128` builtins — determinism within a build. (The
/// PHF passes its own search-chosen seed instead; `wyhash` takes the seed as a parameter.)
pub const WY_SEED: u64 = 0;

#[inline]
fn wymum(a: u64, b: u64) -> (u64, u64) {
    let r = (a as u128).wrapping_mul(b as u128);
    (r as u64, (r >> 64) as u64)
}
#[inline]
fn wymix(a: u64, b: u64) -> u64 {
    let (lo, hi) = wymum(a, b);
    lo ^ hi
}
#[inline]
fn wyr8(p: &[u8]) -> u64 {
    u64::from_le_bytes(p[..8].try_into().unwrap())
}
#[inline]
fn wyr4(p: &[u8]) -> u64 {
    u32::from_le_bytes(p[..4].try_into().unwrap()) as u64
}
/// Read 1..=3 trailing bytes into a 64-bit lane (wyhash `_wyr3`).
#[inline]
fn wyr3(p: &[u8], k: usize) -> u64 {
    ((p[0] as u64) << 16) | ((p[k >> 1] as u64) << 8) | (p[k - 1] as u64)
}

/// wyhash final v3 over `key` with `seed`. Faithful port of the reference scalar path.
pub fn wyhash(key: &[u8], seed: u64) -> u64 {
    let len = key.len();
    let mut seed = seed ^ wymix(seed ^ WY_SECRET[0], WY_SECRET[1]);
    let (a, b);
    if len <= 16 {
        if len >= 4 {
            let off = (len >> 3) << 2;
            a = (wyr4(key) << 32) | wyr4(&key[off..]);
            b = (wyr4(&key[len - 4..]) << 32) | wyr4(&key[len - 4 - off..]);
        } else if len > 0 {
            a = wyr3(key, len);
            b = 0;
        } else {
            a = 0;
            b = 0;
        }
    } else {
        let mut i = len;
        let mut p = 0usize;
        if i > 48 {
            let mut see1 = seed;
            let mut see2 = seed;
            while i > 48 {
                seed = wymix(wyr8(&key[p..]) ^ WY_SECRET[1], wyr8(&key[p + 8..]) ^ seed);
                see1 = wymix(wyr8(&key[p + 16..]) ^ WY_SECRET[2], wyr8(&key[p + 24..]) ^ see1);
                see2 = wymix(wyr8(&key[p + 32..]) ^ WY_SECRET[3], wyr8(&key[p + 40..]) ^ see2);
                p += 48;
                i -= 48;
            }
            seed ^= see1 ^ see2;
        }
        while i > 16 {
            seed = wymix(wyr8(&key[p..]) ^ WY_SECRET[1], wyr8(&key[p + 8..]) ^ seed);
            i -= 16;
            p += 16;
        }
        a = wyr8(&key[len - 16..]);
        b = wyr8(&key[len - 8..]);
    }
    let (lo, hi) = wymum(a ^ WY_SECRET[1], b ^ seed);
    wymix(lo ^ WY_SECRET[0] ^ (len as u64), hi ^ WY_SECRET[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// wyhash final v3 reference test vectors (default secret `_wyp`, the per-line seeds from the
    /// reference). Pins the port against the canonical implementation.
    #[test]
    fn wyhash_matches_reference_vectors() {
        assert_eq!(wyhash(b"", 0), 0x0409_638e_e2bd_e459);
        assert_eq!(wyhash(b"a", 1), 0xa841_2d09_1b5f_e0a9);
        assert_eq!(wyhash(b"abc", 2), 0x32dd_92e4_b291_5153);
        assert_eq!(wyhash(b"message digest", 3), 0x8619_1240_89a3_a16b);
        assert_eq!(wyhash(b"abcdefghijklmnopqrstuvwxyz", 4), 0x7a43_afb6_1d7f_5f40);
    }

    /// The value the JSON PHF byte-match is pinned to on both the codegen and runtime sides. If this
    /// changes, `align_codegen_llvm::phf_hash_is_pinned` and `align_runtime::phf_hash_matches_codegen`
    /// must change with it (they assert the same constant) — a canary for an accidental algorithm edit.
    #[test]
    fn phf_pinned_vector() {
        assert_eq!(wyhash(b"score", 0), 0x1300_a50c_fadb_78d9);
    }
}
