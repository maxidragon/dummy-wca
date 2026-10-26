/// SplitMix64. Hand-rolled because the generated data is a contract with
/// FKMTime's tests: `rand`'s `StdRng` may change its output between versions.
///
/// Only integer and basic float arithmetic is used (no `ln`, `exp`, `cos`),
/// so the output is the same on every platform.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    #[cfg(test)]
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    /// An independent stream for one part of the generator, so changing the
    /// events doesn't reshuffle the names, and so on.
    pub fn derive(seed: u64, label: &str) -> Self {
        // FNV-1a over the label, mixed with the seed.
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in label.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let mut rng = Rng(seed ^ hash);
        rng.next_u64();
        rng
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n`. `n` must be positive.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn chance(&mut self, probability: f64) -> bool {
        self.unit() < probability
    }

    /// Approximately standard normal (Irwin-Hall with 12 uniforms), in `[-6, 6]`.
    pub fn normal(&mut self) -> f64 {
        (0..12).map(|_| self.unit()).sum::<f64>() - 6.0
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }

    pub fn weighted<'a, T>(&mut self, items: &'a [(T, u32)]) -> &'a T {
        let total: u64 = items.iter().map(|(_, weight)| *weight as u64).sum();
        let mut roll = self.below(total);
        for (item, weight) in items {
            if roll < *weight as u64 {
                return item;
            }
            roll -= *weight as u64;
        }
        unreachable!("roll is below the total weight")
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_gives_same_stream() {
        let a: Vec<u64> = (0..5)
            .scan(Rng::new(7), |r, _| Some(r.next_u64()))
            .collect();
        let b: Vec<u64> = (0..5)
            .scan(Rng::new(7), |r, _| Some(r.next_u64()))
            .collect();
        assert_eq!(a, b);
    }

    #[test]
    fn derived_streams_differ_by_label() {
        assert_ne!(
            Rng::derive(1, "persons").next_u64(),
            Rng::derive(1, "schedule").next_u64()
        );
    }

    #[test]
    fn known_output_stays_stable() {
        // SplitMix64 reference value for seed 0; guards against accidental changes.
        assert_eq!(Rng::new(0).next_u64(), 0xe220a8397b1dcdaf);
    }
}
