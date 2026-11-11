//! The ASLE container: the only module that knows the file layout.
//!
//! container magic "ASLE" (32 bits)
//! | Elias gamma(input bit count + 1)
//! | Elias gamma(maximum symbol bit length n)      (omitted for empty input)
//! | size table: n bits, bit i set if some symbol is i+1 bits long
//! | Huffman tree, depth-first: 1 = internal node, 0 = leaf followed by the
//!   leaf's index into the size table (exactly enough bits for the table) and
//!   the symbol's bits
//! | body: the Huffman code of every placed symbol, in order.

use crate::bit_io::{BitReader, BitWriter};
use crate::builder_events::BuilderObserver;
use crate::huffman_tree::HuffmanTree;
use crate::packed_bit_sequence::PackedBitSequence;
use crate::symbol_builder::build_symbol_dictionary;
use crate::symbol_definition::SymbolDefinition;
use crate::symbol_stream::SymbolStream;

const CONTAINER_MAGIC: u64 = 0x4153_4C45; // "ASLE"

/// Exact bits needed to index a table of `table_length` entries.
fn bits_to_index_table(table_length: usize) -> u32 {
    if table_length <= 1 {
        0
    } else {
        usize::BITS - (table_length - 1).leading_zeros()
    }
}

fn elias_gamma_bit_count(value: u64) -> u64 {
    2 * (64 - value.leading_zeros() as u64) - 1
}

/// Which symbol sizes are in use (index = bit length) for the current leaves.
fn find_used_symbol_sizes(stream: &SymbolStream) -> Vec<bool> {
    let mut size_is_used = vec![false; stream.maximum_symbol_bit_length as usize + 1];
    for (symbol_id, definition) in stream.symbol_definitions.iter().enumerate() {
        if stream.symbol_frequencies[symbol_id] > 0 {
            size_is_used[definition.bit_length as usize] = true;
        }
    }
    size_is_used
}

/// Exact header bits the container would need for the current state.
/// Measurement only: never used by any building decision.
pub fn measure_header_bit_count(stream: &SymbolStream) -> u64 {
    let leaf_ids: Vec<usize> =
        (0..stream.symbol_definitions.len()).filter(|&symbol_id| stream.symbol_frequencies[symbol_id] > 0).collect();
    let used_size_count = find_used_symbol_sizes(stream).iter().filter(|&&used| used).count();
    let size_index_bits = bits_to_index_table(used_size_count) as u64;
    let leaf_payload_bits: u64 = leaf_ids
        .iter()
        .map(|&symbol_id| size_index_bits + stream.symbol_definitions[symbol_id].bit_length as u64)
        .sum();
    let tree_bits = if leaf_ids.len() == 1 {
        // Root with the single leaf written on both sides.
        1 + 2 * (1 + leaf_payload_bits)
    } else {
        2 * leaf_ids.len() as u64 - 1 + leaf_payload_bits
    };
    32 + elias_gamma_bit_count(stream.input_bit_count + 1)
        + elias_gamma_bit_count(stream.maximum_symbol_bit_length as u64)
        + stream.maximum_symbol_bit_length as u64
        + tree_bits
}

pub struct DictionaryEntry {
    pub bit_string: String,
    pub bit_length: u32,
    pub frequency: u64,
    pub code_length: u32,
}

pub struct CompressionReport {
    /// Sorted by frequency, most frequent first.
    pub dictionary: Vec<DictionaryEntry>,
    pub header_bit_count: u64,
    pub body_bit_count: u64,
    pub symbols_created: usize,
}

pub fn compress_input(
    input_bytes: &[u8],
    maximum_symbol_bit_length: u32,
    observer: &mut dyn BuilderObserver,
) -> (Vec<u8>, CompressionReport) {
    let mut bit_writer = BitWriter::new();
    bit_writer.write_bits(CONTAINER_MAGIC, 32);
    bit_writer.write_elias_gamma(input_bytes.len() as u64 * 8 + 1);
    if input_bytes.is_empty() {
        let report = CompressionReport {
            dictionary: Vec::new(),
            header_bit_count: bit_writer.bits_written,
            body_bit_count: 0,
            symbols_created: 0,
        };
        return (bit_writer.into_bytes(), report);
    }
    bit_writer.write_elias_gamma(maximum_symbol_bit_length as u64);

    let mut stream = SymbolStream::from_input_bytes(input_bytes, maximum_symbol_bit_length);
    build_symbol_dictionary(&mut stream, observer);

    let size_is_used = find_used_symbol_sizes(&stream);
    let mut size_table_index_by_bit_length = vec![0u64; maximum_symbol_bit_length as usize + 1];
    let mut used_size_count = 0usize;
    for bit_length in 1..=maximum_symbol_bit_length as usize {
        bit_writer.write_bit(size_is_used[bit_length] as u64);
        if size_is_used[bit_length] {
            size_table_index_by_bit_length[bit_length] = used_size_count as u64;
            used_size_count += 1;
        }
    }
    let size_index_bits = bits_to_index_table(used_size_count);
    let huffman_tree = HuffmanTree::build_from_frequencies(&stream.symbol_frequencies);
    let codes_by_symbol_id = huffman_tree.calculate_codes_by_symbol_id(stream.symbol_definitions.len());
    let symbol_definitions = &stream.symbol_definitions;
    let shared_bit_sequence = &stream.shared_bit_sequence;
    huffman_tree.write_depth_first(&mut bit_writer, &mut |bit_writer: &mut BitWriter, symbol_id: usize| {
        let definition = symbol_definitions[symbol_id];
        bit_writer.write_bits(size_table_index_by_bit_length[definition.bit_length as usize], size_index_bits);
        shared_bit_sequence.for_each_chunk(definition.first_bit_offset, definition.bit_length as u64, |chunk, count| {
            bit_writer.write_bits(chunk, count)
        });
    });
    let header_bit_count = bit_writer.bits_written;
    assert_eq!(header_bit_count, measure_header_bit_count(&stream), "header measurement and writer disagree");
    for &symbol_id in &stream.placed_symbol_ids {
        let (code_bits, code_length) = codes_by_symbol_id[symbol_id as usize];
        bit_writer.write_bits(code_bits, code_length);
    }
    let body_bit_count = bit_writer.bits_written - header_bit_count;
    assert_eq!(body_bit_count, stream.encoded_symbol_bits(), "Huffman tree and encoded symbol bits disagree");

    let mut dictionary: Vec<DictionaryEntry> = (0..stream.symbol_definitions.len())
        .filter(|&symbol_id| stream.symbol_frequencies[symbol_id] > 0)
        .map(|symbol_id| DictionaryEntry {
            bit_string: stream.symbol_as_bit_string(symbol_id),
            bit_length: stream.symbol_definitions[symbol_id].bit_length,
            frequency: stream.symbol_frequencies[symbol_id],
            code_length: codes_by_symbol_id[symbol_id].1,
        })
        .collect();
    dictionary.sort_by(|first, second| {
        (std::cmp::Reverse(first.frequency), first.bit_length, &first.bit_string)
            .cmp(&(std::cmp::Reverse(second.frequency), second.bit_length, &second.bit_string))
    });
    let report = CompressionReport {
        dictionary,
        header_bit_count,
        body_bit_count,
        symbols_created: stream.symbol_definitions.len(),
    };
    (bit_writer.into_bytes(), report)
}

pub fn decompress_container(container_bytes: &[u8]) -> Vec<u8> {
    let mut bit_reader = BitReader::new(container_bytes);
    assert_eq!(bit_reader.read_bits(32), CONTAINER_MAGIC, "not an ASLE container");
    let input_bit_count = bit_reader.read_elias_gamma() - 1;
    if input_bit_count == 0 {
        return Vec::new();
    }
    let maximum_symbol_bit_length = bit_reader.read_elias_gamma() as u32;
    let size_table: Vec<u32> = (1..=maximum_symbol_bit_length).filter(|_| bit_reader.read_bit() == 1).collect();
    let size_index_bits = bits_to_index_table(size_table.len());
    // The decoder's shared bit sequence: every symbol's bits, appended as read.
    let mut decoded_symbol_bits = PackedBitSequence::new();
    let mut symbol_definitions: Vec<SymbolDefinition> = Vec::new();
    let huffman_tree = HuffmanTree::read_depth_first(&mut bit_reader, &mut |bit_reader: &mut BitReader| {
        let bit_length = size_table[bit_reader.read_bits(size_index_bits) as usize];
        let first_bit_offset = decoded_symbol_bits.bit_count();
        let mut remaining = bit_length;
        while remaining > 0 {
            let chunk_bit_count = remaining.min(64);
            decoded_symbol_bits.push_bits(bit_reader.read_bits(chunk_bit_count), chunk_bit_count);
            remaining -= chunk_bit_count;
        }
        symbol_definitions.push(SymbolDefinition { first_bit_offset, bit_length });
        symbol_definitions.len() - 1
    });
    let mut output_writer = BitWriter::new();
    while output_writer.bits_written < input_bit_count {
        let definition = symbol_definitions[huffman_tree.decode_next_symbol_id(&mut bit_reader)];
        decoded_symbol_bits.for_each_chunk(definition.first_bit_offset, definition.bit_length as u64, |chunk, count| {
            output_writer.write_bits(chunk, count)
        });
    }
    output_writer.into_bytes()
}
