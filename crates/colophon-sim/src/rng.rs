//! The only source of randomness in a run (S10): `SplitMix64`, seeded once.
//!
//! `SplitMix64` is tiny, fast, and passes `BigCrush`; its output for a seed is fixed forever, so a
//! seed reproduces on any machine (S11). Not for anything secret.

/// A seeded pseudo-random generator.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator whose whole sequence is fixed by `seed`.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..bound`; `bound` must be at least 1.
    pub fn below(&mut self, bound: usize) -> usize {
        // The modulo bias is below 2^-40 for the small bounds used here.
        let bound = u64::try_from(bound.max(1)).unwrap_or(u64::MAX);
        usize::try_from(self.next_u64() % bound).unwrap_or(0)
    }

    /// True with probability `per_mille` / 1000.
    pub fn chance(&mut self, per_mille: u16) -> bool {
        self.next_u64() % 1000 < u64::from(per_mille)
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn the_sequence_is_fixed_by_the_seed() {
        // `SplitMix64`'s published first output for seed 0.
        assert_eq!(Rng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        assert!((0..100).all(|_| a.next_u64() == b.next_u64()));
        assert_ne!(Rng::new(7).next_u64(), Rng::new(8).next_u64());
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = Rng::new(1);
        assert!((0..1000).all(|_| rng.below(3) < 3));
        assert_eq!(rng.below(1), 0);
    }
}
