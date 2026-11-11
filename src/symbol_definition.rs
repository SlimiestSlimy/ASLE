//! What a symbol is: a specific bit string of a specific length.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SymbolDefinition {
    /// The symbol's bits, right-aligned (the first bit is the most significant).
    pub bits: u64,
    pub bit_length: u32,
}

impl SymbolDefinition {
    pub fn as_bit_string(&self) -> String {
        (0..self.bit_length).rev().map(|position| if (self.bits >> position) & 1 == 1 { '1' } else { '0' }).collect()
    }
}
