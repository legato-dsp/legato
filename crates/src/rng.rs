/// A fast, non-cryptographic xorshift PRNG for audio-rate use.
#[derive(Clone)]
pub struct XorShift {
    state: u32,
}

impl Default for XorShift {
    fn default() -> Self {
        Self::seeded()
    }
}

impl XorShift {
    /// A generator seeded from OS entropy, so each instance — and each run —
    /// produces a different stream. Seeding happens at build time, never on the
    /// audio thread. Use [`XorShift::with_seed`] when you need reproducibility.
    pub fn seeded() -> Self {
        Self::with_seed(rand::random::<u32>())
    }

    pub fn with_seed(seed: u32) -> Self {
        Self { state: seed | 1 }
    }

    #[inline(always)]
    pub fn next_u32(&mut self) -> u32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state
    }

    /// A sample in `[-1, 1]`.
    #[inline(always)]
    pub fn bipolar(&mut self) -> f32 {
        (self.next_u32() as i32 as f32) * (1.0 / i32::MAX as f32)
    }

    /// A sample in `[0, 1)`, using the high 24 bits for an even spread.
    #[inline(always)]
    pub fn unipolar(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / (1u32 << 24) as f32)
    }
}
