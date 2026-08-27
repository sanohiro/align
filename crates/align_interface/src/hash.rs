//! The artifact-identity hash for interface / implementation fingerprints.
//!
//! v1 uses a 128-bit non-cryptographic hash built from two independently-seeded [`align_hash::wyhash`]
//! passes over the same bytes. 128 bits is ample collision resistance for a *local* build cache keyed
//! on non-adversarial compiler-produced bytes. It is deliberately NOT presented as tamper-resistant.
//!
//! Recorded upgrade note (docs/impl/10-cache-first-optimization.md §6): when the content-addressed
//! store (CAS) lands in M15 S3, promote these to a strong hash (a 256-bit BLAKE3/SHA-256-class digest)
//! at the CAS boundary. Keeping the identity behind [`Hash128`] means that swap touches one type.

/// A 128-bit content hash. Stable, deterministic, and byte-order-independent of the host (each half
/// is a `u64` serialized little-endian by the codec).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hash128 {
    pub lo: u64,
    pub hi: u64,
}

/// Two independent wyhash seeds (two arbitrary odd 64-bit constants). Using distinct seeds over the
/// same input yields two ~independent 64-bit lanes → ~128-bit combined collision resistance.
const SEED_LO: u64 = 0x9E37_79B9_7F4A_7C15;
const SEED_HI: u64 = 0xC2B2_AE3D_27D4_EB4F;

impl Hash128 {
    /// Hash a byte slice into a 128-bit digest.
    pub fn of(bytes: &[u8]) -> Hash128 {
        Hash128 { lo: align_hash::wyhash(bytes, SEED_LO), hi: align_hash::wyhash(bytes, SEED_HI) }
    }

    /// Lowercase hex rendering (32 chars, `lo` then `hi`), for logs / test assertions / cache keys.
    pub fn to_hex(self) -> String {
        format!("{:016x}{:016x}", self.lo, self.hi)
    }
}

/// Incremental form of [`Hash128::of`] for a stream whose exact length is known before reading.
/// The state is fixed-size and rejects excess or incomplete input rather than changing the digest
/// convention used by persisted cache entries.
pub struct Hash128Stream {
    lo: align_hash::WyHashStream,
    hi: align_hash::WyHashStream,
}

impl Hash128Stream {
    pub fn for_len(len: usize) -> Self {
        Self {
            lo: align_hash::WyHashStream::for_len(SEED_LO, len),
            hi: align_hash::WyHashStream::for_len(SEED_HI, len),
        }
    }

    /// Consume one chunk. Returns `false` without consuming it when it would exceed the declared
    /// length.
    pub fn update(&mut self, bytes: &[u8]) -> bool {
        let lo = self.lo.update(bytes);
        let hi = self.hi.update(bytes);
        debug_assert_eq!(lo, hi);
        lo
    }

    /// Return the digest only after exactly the declared length was consumed.
    pub fn finish(self) -> Option<Hash128> {
        Some(Hash128 { lo: self.lo.finish()?, hi: self.hi.finish()? })
    }
}

impl core::fmt::Debug for Hash128 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Hash128({})", self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash128_stream_matches_the_persisted_one_shot_identity() {
        let bytes: Vec<u8> = (0..2049)
            .map(|index| u8::try_from((index * 19 + 5) % 256).unwrap())
            .collect();
        for len in 0..=bytes.len() {
            for chunk in [1, 15, 16, 17, 47, 48, 49, 127] {
                let mut stream = Hash128Stream::for_len(len);
                for part in bytes[..len].chunks(chunk) {
                    assert!(stream.update(part));
                }
                assert_eq!(stream.finish(), Some(Hash128::of(&bytes[..len])));
            }
        }
    }
}
