//! Seeded PCG32 random numbers, bit-identical on every platform.
//!
//! The simulation owns exactly one [`Rng`], seeded from the project, the
//! playtest or `--seed`. Systems draw from it in a fixed order (entity id
//! order), so the same seed always gives the same game.

/// PCG-XSH-RR 64/32 (O'Neill, 2014).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
    inc: u64,
}

const MULT: u64 = 6364136223846793005;
const DEFAULT_STREAM: u64 = 0xda3e_39cb_94b9_5bdb;

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut rng = Rng { state: 0, inc: (DEFAULT_STREAM << 1) | 1 };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULT).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `0..n` without modulo bias. `n` must be > 0.
    pub fn below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "Rng::below(0)");
        let threshold = n.wrapping_neg() % n;
        loop {
            let r = self.next_u32();
            if r >= threshold {
                return r % n;
            }
        }
    }

    /// Uniform in `lo..=hi`.
    pub fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        assert!(lo <= hi);
        let span = (hi as i64 - lo as i64 + 1) as u32;
        lo + self.below(span) as i32
    }

    /// Internal state, for hashing and snapshots.
    pub fn state(&self) -> (u64, u64) {
        (self.state, self.inc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn known_values_are_stable_across_platforms() {
        // If this changes, every recorded playtest changes. Never update it
        // casually: see docs/decisions/0005-determinism-model.md.
        let mut r = Rng::new(1);
        let v: Vec<u32> = (0..4).map(|_| r.next_u32()).collect();
        assert_eq!(v, KNOWN_SEED1);
    }

    const KNOWN_SEED1: [u32; 4] = [0xf063_dc4b, 0xb124_1f91, 0xf842_f2c2, 0xa65b_6d95];

    #[test]
    fn below_stays_in_range() {
        let mut r = Rng::new(7);
        for n in 1..50 {
            for _ in 0..50 {
                assert!(r.below(n) < n);
            }
        }
    }
}
