//! What a symbol is: a slice of the shared bit sequence. Two definitions with
//! equal bits (compared through the sequence) are the same symbol.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SymbolDefinition {
    /// Where one copy of the symbol's bits starts in the shared bit sequence.
    pub first_bit_offset: u64,
    pub bit_length: u32,
}
