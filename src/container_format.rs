//! The ASLE container: the only module that knows the file layout.
//!
//! container magic "ASLE" (32 bits)
//! | Elias gamma(input bit count + 1)            (the rest is omitted for empty input)
//! | symbol prefix tree (see symbol_prefix_tree): skipped full levels, child
//!   bits, gamma(k + 1), combination index; symbols in depth-first order
//! | code tree shape, depth first (1 = internal, 0 = leaf); its leaves, left
//!   to right, are the symbols in the same depth-first order
//! | body: the code of every placed symbol, in order.
//!
//! The code tree is the optimal tree that keeps the symbols' order (see
//! order_preserving_code_tree). The builder still decides with Huffman code
//! lengths; the container is free to encode however is smallest.

use crate::bit_io::{BitReader, BitWriter};
use crate::builder_events::BuilderObserver;
use crate::code_tree::BinaryCodeTree;
use crate::order_preserving_code_tree::build_order_preserving_optimal_code_tree;
use crate::packed_bit_sequence::PackedBitSequence;
use crate::symbol_builder::build_symbol_dictionary;
use crate::symbol_definition::SymbolDefinition;
use crate::symbol_prefix_tree::{describe_symbol_prefix_tree, read_symbol_prefix_tree, SymbolPrefixTreeDescription};
use crate::symbol_stream::SymbolStream;

const CONTAINER_MAGIC: u64 = 0x4153_4C45; // "ASLE"

fn elias_gamma_bit_count(value: u64) -> u64 {
    2 * (64 - value.leading_zeros() as u64) - 1
}

fn describe_leaves_prefix_tree(stream: &SymbolStream) -> SymbolPrefixTreeDescription {
    let leaves: Vec<(usize, SymbolDefinition)> = (0..stream.symbol_definitions.len())
        .filter(|&symbol_id| stream.symbol_frequencies[symbol_id] > 0)
        .map(|symbol_id| (symbol_id, stream.symbol_definitions[symbol_id]))
        .collect();
    describe_symbol_prefix_tree(&stream.shared_bit_sequence, leaves)
}

/// Bits of a code tree shape with this many leaves (a full binary tree).
fn code_tree_shape_bit_count(leaf_count: usize) -> u64 {
    2 * leaf_count as u64 - 1
}

/// Exact header bits the container would need for the current state.
/// Measurement only: never used by any building decision.
pub fn measure_header_bit_count(stream: &SymbolStream) -> u64 {
    32 + elias_gamma_bit_count(stream.input_bit_count + 1)
        + describe_leaves_prefix_tree(stream).bit_count()
        + code_tree_shape_bit_count(stream.leaf_count())
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
    /// What a Huffman body would take (the measure the builder decides with).
    pub huffman_encoded_symbol_bits: u64,
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
            huffman_encoded_symbol_bits: 0,
            symbols_created: 0,
        };
        return (bit_writer.into_bytes(), report);
    }

    let mut stream = SymbolStream::from_input_bytes(input_bytes, maximum_symbol_bit_length);
    build_symbol_dictionary(&mut stream, observer);

    let prefix_tree = describe_leaves_prefix_tree(&stream);
    prefix_tree.write(&mut bit_writer);
    let leaf_frequencies_depth_first: Vec<u64> =
        prefix_tree.symbol_ids_depth_first.iter().map(|&symbol_id| stream.symbol_frequencies[symbol_id]).collect();
    let code_tree = build_order_preserving_optimal_code_tree(&leaf_frequencies_depth_first);
    code_tree.write_depth_first(&mut bit_writer, &mut |_, _| {});
    let header_bit_count = bit_writer.bits_written;
    assert_eq!(header_bit_count, measure_header_bit_count(&stream), "header measurement and writer disagree");

    let leaf_count = prefix_tree.symbol_ids_depth_first.len();
    let codes_by_leaf_index = code_tree.calculate_codes_by_leaf_index(leaf_count);
    let mut leaf_index_by_symbol_id = vec![usize::MAX; stream.symbol_definitions.len()];
    for (leaf_index, &symbol_id) in prefix_tree.symbol_ids_depth_first.iter().enumerate() {
        leaf_index_by_symbol_id[symbol_id] = leaf_index;
    }
    for &symbol_id in &stream.placed_symbol_ids {
        let (code_bits, code_length) = codes_by_leaf_index[leaf_index_by_symbol_id[symbol_id as usize]];
        bit_writer.write_bits(code_bits, code_length);
    }
    let body_bit_count = bit_writer.bits_written - header_bit_count;
    let expected_body_bit_count: u64 = (0..leaf_count)
        .map(|leaf_index| leaf_frequencies_depth_first[leaf_index] * codes_by_leaf_index[leaf_index].1 as u64)
        .sum();
    assert_eq!(body_bit_count, expected_body_bit_count, "code tree and body disagree");

    let mut dictionary: Vec<DictionaryEntry> = prefix_tree
        .symbol_ids_depth_first
        .iter()
        .enumerate()
        .map(|(leaf_index, &symbol_id)| DictionaryEntry {
            bit_string: stream.symbol_as_bit_string(symbol_id),
            bit_length: stream.symbol_definitions[symbol_id].bit_length,
            frequency: stream.symbol_frequencies[symbol_id],
            code_length: codes_by_leaf_index[leaf_index].1,
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
        huffman_encoded_symbol_bits: stream.encoded_symbol_bits(),
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
    let (decoded_symbol_bits, symbol_definitions_depth_first): (PackedBitSequence, Vec<SymbolDefinition>) =
        read_symbol_prefix_tree(&mut bit_reader);
    let mut next_leaf_index = 0usize;
    let code_tree = BinaryCodeTree::read_depth_first(&mut bit_reader, &mut |_| {
        next_leaf_index += 1;
        next_leaf_index - 1
    });
    assert_eq!(next_leaf_index, symbol_definitions_depth_first.len(), "code tree and prefix tree disagree");
    let mut output_writer = BitWriter::new();
    while output_writer.bits_written < input_bit_count {
        let definition = symbol_definitions_depth_first[code_tree.decode_next_leaf_index(&mut bit_reader)];
        decoded_symbol_bits.for_each_chunk(definition.first_bit_offset, definition.bit_length as u64, |chunk, count| {
            output_writer.write_bits(chunk, count)
        });
    }
    output_writer.into_bytes()
}
