//! Unsigned integers of any size, with just the operations the combination
//! index needs: add, subtract, compare, multiply and divide by a small number,
//! binomial coefficients, and exact-width bit reading and writing.

use crate::bit_io::{BitReader, BitWriter};
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigUnsignedInteger {
    /// Base 2^64 digits, least significant first, no trailing zero digits.
    digits_least_significant_first: Vec<u64>,
}

impl BigUnsignedInteger {
    pub fn zero() -> Self {
        BigUnsignedInteger { digits_least_significant_first: Vec::new() }
    }

    pub fn from_u64(value: u64) -> Self {
        let mut number = BigUnsignedInteger { digits_least_significant_first: vec![value] };
        number.remove_leading_zero_digits();
        number
    }

    fn remove_leading_zero_digits(&mut self) {
        while self.digits_least_significant_first.last() == Some(&0) {
            self.digits_least_significant_first.pop();
        }
    }

    pub fn add_assign(&mut self, other: &BigUnsignedInteger) {
        let digit_count = self.digits_least_significant_first.len().max(other.digits_least_significant_first.len());
        self.digits_least_significant_first.resize(digit_count, 0);
        let mut carry = 0u128;
        for index in 0..digit_count {
            let other_digit = *other.digits_least_significant_first.get(index).unwrap_or(&0) as u128;
            let sum = self.digits_least_significant_first[index] as u128 + other_digit + carry;
            self.digits_least_significant_first[index] = sum as u64;
            carry = sum >> 64;
        }
        if carry > 0 {
            self.digits_least_significant_first.push(carry as u64);
        }
    }

    /// Requires self >= other.
    pub fn subtract_assign(&mut self, other: &BigUnsignedInteger) {
        assert!(self.compare(other) != Ordering::Less, "big integer subtraction would go below zero");
        let mut borrow = 0i128;
        for index in 0..self.digits_least_significant_first.len() {
            let other_digit = *other.digits_least_significant_first.get(index).unwrap_or(&0) as i128;
            let mut difference = self.digits_least_significant_first[index] as i128 - other_digit - borrow;
            borrow = if difference < 0 {
                difference += 1i128 << 64;
                1
            } else {
                0
            };
            self.digits_least_significant_first[index] = difference as u64;
        }
        self.remove_leading_zero_digits();
    }

    pub fn compare(&self, other: &BigUnsignedInteger) -> Ordering {
        let (mine, theirs) = (&self.digits_least_significant_first, &other.digits_least_significant_first);
        mine.len().cmp(&theirs.len()).then_with(|| mine.iter().rev().cmp(theirs.iter().rev()))
    }

    pub fn multiply_by_small(&mut self, factor: u64) {
        let mut carry = 0u128;
        for digit in self.digits_least_significant_first.iter_mut() {
            let product = *digit as u128 * factor as u128 + carry;
            *digit = product as u64;
            carry = product >> 64;
        }
        if carry > 0 {
            self.digits_least_significant_first.push(carry as u64);
        }
        self.remove_leading_zero_digits();
    }

    /// Divides in place; returns the remainder.
    pub fn divide_by_small(&mut self, divisor: u64) -> u64 {
        assert!(divisor > 0);
        let mut remainder = 0u128;
        for digit in self.digits_least_significant_first.iter_mut().rev() {
            let current = (remainder << 64) | *digit as u128;
            *digit = (current / divisor as u128) as u64;
            remainder = current % divisor as u128;
        }
        self.remove_leading_zero_digits();
        remainder as u64
    }

    pub fn bit_length(&self) -> u64 {
        match self.digits_least_significant_first.last() {
            None => 0,
            Some(&top) => (self.digits_least_significant_first.len() as u64 - 1) * 64 + (64 - top.leading_zeros() as u64),
        }
    }

    fn bit(&self, bit_index: u64) -> u64 {
        let digit = *self.digits_least_significant_first.get((bit_index / 64) as usize).unwrap_or(&0);
        (digit >> (bit_index % 64)) & 1
    }

    /// Writes exactly `bit_count` bits, most significant first (value must fit).
    pub fn write_bits(&self, bit_writer: &mut BitWriter, bit_count: u64) {
        assert!(self.bit_length() <= bit_count, "big integer does not fit in its field");
        for bit_index in (0..bit_count).rev() {
            bit_writer.write_bit(self.bit(bit_index));
        }
    }

    pub fn read_bits(bit_reader: &mut BitReader, bit_count: u64) -> Self {
        let digit_count = ((bit_count + 63) / 64) as usize;
        let mut number = BigUnsignedInteger { digits_least_significant_first: vec![0; digit_count] };
        for bit_index in (0..bit_count).rev() {
            if bit_reader.read_bit() == 1 {
                number.digits_least_significant_first[(bit_index / 64) as usize] |= 1u64 << (bit_index % 64);
            }
        }
        number.remove_leading_zero_digits();
        number
    }
}

/// C(total, chosen); zero when chosen > total.
pub fn binomial_coefficient(total: u64, chosen: u64) -> BigUnsignedInteger {
    if chosen > total {
        return BigUnsignedInteger::zero();
    }
    let smaller_side = chosen.min(total - chosen);
    let mut result = BigUnsignedInteger::from_u64(1);
    // After step i, result = C(total - smaller_side + i, i): every division is exact.
    for step in 1..=smaller_side {
        result.multiply_by_small(total - smaller_side + step);
        let remainder = result.divide_by_small(step);
        debug_assert_eq!(remainder, 0);
    }
    result
}
