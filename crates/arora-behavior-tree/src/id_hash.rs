//! Hash maps keyed by ids that are already uniformly random, such as the
//! UUIDs naming a tree's nodes and parameters: their hasher folds the id's
//! words, as FxHash does, rather than running SipHash's keyed rounds.
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Folds every word it is given.
#[derive(Default, Clone, Copy)]
pub(crate) struct IdHasher(u64);

impl IdHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl Hasher for IdHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }

    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// A map keyed by ids.
pub(crate) type IdMap<K, V> = HashMap<K, V, BuildHasherDefault<IdHasher>>;

/// A set of ids.
pub(crate) type IdSet<K> = std::collections::HashSet<K, BuildHasherDefault<IdHasher>>;
