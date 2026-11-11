//! A small, fast, deterministic hasher for integer keys (no external crates).

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default)]
pub struct FastIntegerHasher {
    state: u64,
}

impl Hasher for FastIntegerHasher {
    fn finish(&self) -> u64 {
        self.state
    }
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(byte as u64);
        }
    }
    fn write_u64(&mut self, value: u64) {
        self.state = (self.state.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_u32(&mut self, value: u32) {
        self.write_u64(value as u64)
    }
}

pub type FastHashMap<Key, Value> = HashMap<Key, Value, BuildHasherDefault<FastIntegerHasher>>;
