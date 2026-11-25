//! FNV-1a 64-bit hashing for world-state and image hashes.
//!
//! Not cryptographic. Chosen because it is tiny, has no dependencies and is
//! defined byte-by-byte, so it is identical on every platform.

#[derive(Clone, Debug)]
pub struct Hasher64(u64);

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

impl Default for Hasher64 {
    fn default() -> Self {
        Hasher64(OFFSET)
    }
}

impl Hasher64 {
    pub fn new() -> Hasher64 {
        Hasher64::default()
    }

    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        for b in bytes {
            self.0 ^= *b as u64;
            self.0 = self.0.wrapping_mul(PRIME);
        }
        self
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.bytes(&[v])
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    pub fn i64(&mut self, v: i64) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(v as u8)
    }

    /// Hashes the exact bit pattern, so -0.0 and 0.0 differ.
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.u32(v.to_bits())
    }

    pub fn f64(&mut self, v: f64) -> &mut Self {
        self.u64(v.to_bits())
    }

    /// Length-prefixed, so ("ab","c") and ("a","bc") differ.
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.u64(s.len() as u64).bytes(s.as_bytes())
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

/// One-shot hash of a byte slice.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    Hasher64::new().bytes(bytes).finish()
}

/// 16 lowercase hex digits. The canonical way hashes appear in output.
pub fn hex(h: u64) -> String {
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn strings_are_length_prefixed() {
        let a = Hasher64::new().str("ab").str("c").finish();
        let b = Hasher64::new().str("a").str("bc").finish();
        assert_ne!(a, b);
    }
}
