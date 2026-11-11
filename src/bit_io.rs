//! Bit-level reading and writing, most significant bit first.

pub struct BitWriter {
    finished_bytes: Vec<u8>,
    pending_byte: u8,
    pending_bit_count: u8,
    pub bits_written: u64,
}

impl BitWriter {
    pub fn new() -> Self {
        BitWriter { finished_bytes: Vec::new(), pending_byte: 0, pending_bit_count: 0, bits_written: 0 }
    }

    #[inline]
    pub fn write_bit(&mut self, bit: u64) {
        self.pending_byte = (self.pending_byte << 1) | (bit as u8 & 1);
        self.pending_bit_count += 1;
        self.bits_written += 1;
        if self.pending_bit_count == 8 {
            self.finished_bytes.push(self.pending_byte);
            self.pending_byte = 0;
            self.pending_bit_count = 0;
        }
    }

    /// Writes the low `bit_count` bits of `value`, most significant first.
    #[inline]
    pub fn write_bits(&mut self, value: u64, bit_count: u32) {
        for bit_position in (0..bit_count).rev() {
            self.write_bit((value >> bit_position) & 1);
        }
    }

    /// Elias gamma code of `value` (value >= 1).
    pub fn write_elias_gamma(&mut self, value: u64) {
        assert!(value >= 1);
        let significant_bits = 64 - value.leading_zeros();
        self.write_bits(0, significant_bits - 1);
        self.write_bits(value, significant_bits);
    }

    /// Pads the last byte with zero bits and returns all bytes.
    pub fn into_bytes(mut self) -> Vec<u8> {
        if self.pending_bit_count > 0 {
            self.pending_byte <<= 8 - self.pending_bit_count;
            self.finished_bytes.push(self.pending_byte);
        }
        self.finished_bytes
    }
}

pub struct BitReader<'a> {
    bytes: &'a [u8],
    bit_position: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        BitReader { bytes, bit_position: 0 }
    }

    #[inline]
    pub fn read_bit(&mut self) -> u64 {
        let byte = self.bytes[self.bit_position >> 3];
        let bit = (byte >> (7 - (self.bit_position & 7))) & 1;
        self.bit_position += 1;
        bit as u64
    }

    pub fn read_bits(&mut self, bit_count: u32) -> u64 {
        let mut value = 0;
        for _ in 0..bit_count {
            value = (value << 1) | self.read_bit();
        }
        value
    }

    pub fn read_elias_gamma(&mut self) -> u64 {
        let mut leading_zeros = 0;
        while self.read_bit() == 0 {
            leading_zeros += 1;
        }
        let mut value = 1;
        for _ in 0..leading_zeros {
            value = (value << 1) | self.read_bit();
        }
        value
    }
}
