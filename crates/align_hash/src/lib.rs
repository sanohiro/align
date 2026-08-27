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

/// Incremental wyhash for a byte stream whose exact length is known before the first byte.
///
/// Wyhash's final lanes overlap the last processed block, so an ordinary state that knows only the
/// bytes seen so far cannot preserve the one-shot result. The declared length fixes the 48-byte and
/// 16-byte block boundaries up front; this state retains only the current block and the final 16
/// bytes. [`finish`](Self::finish) returns `None` unless exactly the declared byte count arrived.
pub struct WyHashStream {
    expected: usize,
    received: usize,
    triple_bytes: usize,
    process_bytes: usize,
    processed: usize,
    seed: u64,
    see1: u64,
    see2: u64,
    block: [u8; 48],
    block_len: usize,
    tail: [u8; 16],
    tail_len: usize,
}

impl WyHashStream {
    pub fn for_len(seed: u64, len: usize) -> Self {
        let triple_bytes = if len > 48 { ((len - 1) / 48) * 48 } else { 0 };
        let remaining = len - triple_bytes;
        let single_bytes = if len > 16 { ((remaining - 1) / 16) * 16 } else { 0 };
        let seed = seed ^ wymix(seed ^ WY_SECRET[0], WY_SECRET[1]);
        Self {
            expected: len,
            received: 0,
            triple_bytes,
            process_bytes: triple_bytes + single_bytes,
            processed: 0,
            seed,
            see1: seed,
            see2: seed,
            block: [0; 48],
            block_len: 0,
            tail: [0; 16],
            tail_len: 0,
        }
    }

    /// Consume one chunk. An update that would exceed the declared length returns `false` without
    /// changing the state.
    pub fn update(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() > self.expected - self.received {
            return false;
        }
        self.update_tail(bytes);
        self.received += bytes.len();

        let mut cursor = 0;
        while cursor < bytes.len() && self.processed + self.block_len < self.process_bytes {
            let target = if self.processed < self.triple_bytes { 48 } else { 16 };
            let remaining_prefix = self.process_bytes - self.processed - self.block_len;
            let take = (target - self.block_len)
                .min(remaining_prefix)
                .min(bytes.len() - cursor);
            self.block[self.block_len..self.block_len + take]
                .copy_from_slice(&bytes[cursor..cursor + take]);
            self.block_len += take;
            cursor += take;
            if self.block_len == target {
                self.process_block(target);
            }
        }
        true
    }

    /// Finish only after the exact declared length has been consumed.
    pub fn finish(self) -> Option<u64> {
        if self.received != self.expected
            || self.processed != self.process_bytes
            || self.block_len != 0
        {
            return None;
        }
        let (a, b) = if self.expected <= 16 {
            if self.expected >= 4 {
                let off = (self.expected >> 3) << 2;
                (
                    (wyr4(&self.tail) << 32) | wyr4(&self.tail[off..]),
                    (wyr4(&self.tail[self.expected - 4..]) << 32)
                        | wyr4(&self.tail[self.expected - 4 - off..]),
                )
            } else if self.expected > 0 {
                (wyr3(&self.tail, self.expected), 0)
            } else {
                (0, 0)
            }
        } else {
            (wyr8(&self.tail), wyr8(&self.tail[8..]))
        };
        let length = u64::try_from(self.expected).ok()?;
        let (lo, hi) = wymum(a ^ WY_SECRET[1], b ^ self.seed);
        Some(wymix(lo ^ WY_SECRET[0] ^ length, hi ^ WY_SECRET[1]))
    }

    fn update_tail(&mut self, bytes: &[u8]) {
        let tail_capacity = self.tail.len();
        if bytes.len() >= tail_capacity {
            self.tail.copy_from_slice(&bytes[bytes.len() - tail_capacity..]);
            self.tail_len = tail_capacity;
            return;
        }
        let keep = self.tail_len.min(tail_capacity - bytes.len());
        if keep > 0 {
            self.tail.copy_within(self.tail_len - keep..self.tail_len, 0);
        }
        self.tail[keep..keep + bytes.len()].copy_from_slice(bytes);
        self.tail_len = keep + bytes.len();
    }

    fn process_block(&mut self, size: usize) {
        if size == 48 {
            self.seed = wymix(wyr8(&self.block) ^ WY_SECRET[1], wyr8(&self.block[8..]) ^ self.seed);
            self.see1 = wymix(
                wyr8(&self.block[16..]) ^ WY_SECRET[2],
                wyr8(&self.block[24..]) ^ self.see1,
            );
            self.see2 = wymix(
                wyr8(&self.block[32..]) ^ WY_SECRET[3],
                wyr8(&self.block[40..]) ^ self.see2,
            );
        } else {
            self.seed = wymix(wyr8(&self.block) ^ WY_SECRET[1], wyr8(&self.block[8..]) ^ self.seed);
        }
        self.processed += size;
        self.block_len = 0;
        if self.triple_bytes != 0 && self.processed == self.triple_bytes {
            self.seed ^= self.see1 ^ self.see2;
        }
    }
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

    #[test]
    fn streamed_hash_matches_one_shot_at_every_block_boundary() {
        let bytes: Vec<u8> = (0..4097)
            .map(|i| u8::try_from((i * 37 + 11) % 256).unwrap())
            .collect();
        for len in 0..=bytes.len() {
            for chunk in [1, 2, 3, 7, 15, 16, 17, 47, 48, 49, 64, 257] {
                let mut stream = WyHashStream::for_len(23, len);
                for part in bytes[..len].chunks(chunk) {
                    assert!(stream.update(part));
                }
                assert_eq!(stream.finish(), Some(wyhash(&bytes[..len], 23)), "len={len} chunk={chunk}");
            }
        }
    }

    #[test]
    fn streamed_hash_requires_the_declared_length_without_consuming_excess() {
        let mut short = WyHashStream::for_len(0, 4);
        assert!(short.update(b"abc"));
        assert_eq!(short.finish(), None);

        let mut excess = WyHashStream::for_len(0, 3);
        assert!(!excess.update(b"abcd"));
        assert!(excess.update(b"abc"));
        assert_eq!(excess.finish(), Some(wyhash(b"abc", 0)));
    }
}
