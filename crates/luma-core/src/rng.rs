//! Seeded seven-bag randomizer. The generator is a 64-bit SplitMix, chosen because its whole
//! state is one integer (trivially saved with a game) and its output is identical on every
//! platform, which deterministic replay depends on.

use crate::piece::{Kind, ALL_KINDS};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitMix64 {
    pub state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform integer in `0..n` (rejection sampling, no modulo bias).
    pub fn below(&mut self, n: u64) -> u64 {
        let zone = u64::MAX - (u64::MAX % n);
        loop {
            let v = self.next_u64();
            if v < zone {
                return v % n;
            }
        }
    }
}

/// Each run of seven pieces is a shuffled permutation of all seven kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bag {
    rng: SplitMix64,
    pending: Vec<Kind>,
}

impl Bag {
    pub fn new(seed: u64) -> Self {
        Bag {
            rng: SplitMix64::new(seed),
            pending: Vec::new(),
        }
    }
    #[allow(clippy::should_implement_trait)] // an infinite generator, not an Iterator
    pub fn next(&mut self) -> Kind {
        if self.pending.is_empty() {
            let mut b = ALL_KINDS.to_vec();
            // Fisher-Yates.
            for i in (1..b.len()).rev() {
                let j = self.rng.below(i as u64 + 1) as usize;
                b.swap(i, j);
            }
            b.reverse(); // pop() then yields in shuffled order
            self.pending = b;
        }
        self.pending.pop().expect("bag refilled")
    }
    /// Pieces still owed by the current bag (used by save validation).
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
    pub fn is_valid(&self) -> bool {
        let mut seen = [false; 7];
        for k in &self.pending {
            let i = *k as usize;
            if seen[i] {
                return false;
            }
            seen[i] = true;
        }
        self.pending.len() <= 7
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_aligned_window_of_seven_is_a_permutation() {
        let mut b = Bag::new(12345);
        for _ in 0..200 {
            let mut w: Vec<Kind> = (0..7).map(|_| b.next()).collect();
            w.sort();
            assert_eq!(w, ALL_KINDS.to_vec().tap_sort());
        }
    }

    #[test]
    fn same_seed_same_sequence_and_different_seed_differs() {
        let a: Vec<Kind> = {
            let mut b = Bag::new(7);
            (0..70).map(|_| b.next()).collect()
        };
        let a2: Vec<Kind> = {
            let mut b = Bag::new(7);
            (0..70).map(|_| b.next()).collect()
        };
        let c: Vec<Kind> = {
            let mut b = Bag::new(8);
            (0..70).map(|_| b.next()).collect()
        };
        assert_eq!(a, a2);
        assert_ne!(a, c);
    }

    #[test]
    fn longest_drought_is_bounded_by_bag() {
        let mut b = Bag::new(99);
        let seq: Vec<Kind> = (0..7000).map(|_| b.next()).collect();
        for k in ALL_KINDS {
            let mut gap = 0;
            for s in &seq {
                if *s == k {
                    gap = 0;
                } else {
                    gap += 1;
                    assert!(gap <= 12, "{k:?} drought {gap}");
                }
            }
        }
    }

    trait TapSort {
        fn tap_sort(self) -> Self;
    }
    impl TapSort for Vec<Kind> {
        fn tap_sort(mut self) -> Self {
            self.sort();
            self
        }
    }
}
