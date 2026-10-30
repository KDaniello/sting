use rand_chacha::{
    ChaCha8Rng,
    rand_core::{RngCore, SeedableRng},
};

/// The seeded random stream of one run.
///
/// `ChaCha`, because its output is defined by specification and therefore identical on every
/// platform — a run reproduced on another machine draws the same bytes. Each `(seed, stream)`
/// pair is an independent stream: sweeps derive per-run streams from one scenario seed, so runs
/// are uncorrelated but individually reproducible.
#[derive(Debug, Clone)]
pub struct RunRng(ChaCha8Rng);

impl RunRng {
    /// The stream numbered `stream` of the scenario's `seed`. Different streams of one seed are
    /// independent; the same pair anywhere reproduces the same bytes.
    #[must_use]
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        rng.set_stream(stream);
        RunRng(rng)
    }

    /// The next word of the stream, advancing it.
    #[must_use]
    pub fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// A draw in `[0, bound)` without modulo bias, by rejection sampling.
    ///
    /// # Panics
    ///
    /// Panics when `bound` is zero: an empty range is a caller bug, not a drawable value.
    #[must_use]
    pub fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0, "empty range");
        let zone = u64::MAX - (u64::MAX % bound);
        loop {
            let draw = self.next_u64();
            if draw < zone {
                return draw % bound;
            }
        }
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream_same_bytes() {
        let mut a = RunRng::new(42, 7);
        let mut b = RunRng::new(42, 7);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_streams_diverge() {
        let mut a = RunRng::new(42, 0);
        let mut b = RunRng::new(42, 1);
        let same = (0..100).filter(|_| a.next_u64() == b.next_u64()).count();
        assert!(same < 100);
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = RunRng::new(1, 0);
        let mut b = RunRng::new(2, 0);
        let same = (0..100).filter(|_| a.next_u64() == b.next_u64()).count();
        assert!(same < 100);
    }

    #[test]
    fn below_stays_inside_the_range() {
        let mut rng = RunRng::new(7, 0);
        for bound in [1, 2, 3, 1_000, u64::MAX] {
            for _ in 0..64 {
                assert!(rng.below(bound) < bound, "a draw escaped the bound {bound}");
            }
        }
    }

    /// Rejection sampling is there to remove modulo bias; a bound that does not divide the
    /// word range is where a naive modulo would show it.
    #[test]
    fn below_covers_the_range_it_is_given() {
        let mut rng = RunRng::new(11, 0);
        let mut seen = [false; 3];
        for _ in 0..200 {
            let drawn = usize::try_from(rng.below(3)).expect("below(3) answers 0, 1 or 2");
            seen[drawn] = true;
        }
        assert_eq!(seen, [true; 3]);
    }

    #[test]
    #[should_panic(expected = "empty range")]
    fn below_zero_is_a_caller_bug() {
        let _ = RunRng::new(0, 0).below(0);
    }
}
