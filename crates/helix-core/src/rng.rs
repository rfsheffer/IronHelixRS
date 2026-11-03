//! A small seeded random source, so a game can be replayed exactly.

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }

    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Lingo's `random(n)`: 1 to n.
    pub fn random(&mut self, n: u32) -> u32 {
        (self.next() % n.max(1) as u64) as u32 + 1
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn range_and_repeatability() {
        let mut a = super::Rng::new(7);
        let mut b = super::Rng::new(7);
        for _ in 0..1000 {
            let x = a.random(5);
            assert!((1..=5).contains(&x));
            assert_eq!(x, b.random(5));
        }
    }
}
