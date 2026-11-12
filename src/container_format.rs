//! The VaSyLeRE container: the only module that knows the file layout.
//!
//! Elias gamma(placed symbol count + 1)           (the rest is omitted for empty input)
//! | symbol prefix tree (see symbol_prefix_tree): skipped full levels, child
//!   bits, gamma(k + 1), combination index; symbols in depth-first order
//! | code length section (see code_length_section): which Huffman code length
//!   each symbol has: counts per code length within the range the code tree
//!   allows, plus one multiset-permutation index
//! | body (see range_coded_body): every placed symbol's canonical Huffman code,
//!   range coded bit by bit with an order-3 last-choice memory.
//!
//! The code lengths are exactly the ones the builder decides with.

use crate::bit_io::{BitReader, BitWriter};
use crate::builder_events::BuilderObserver;
use crate::canonical_huffman_code::{assign_canonical_codes, CanonicalHuffmanDecoder};
use crate::range_coded_body::{decode_body, encode_body};
use crate::code_length_section::{measure_code_length_section_bit_count, read_code_length_section, write_code_length_section};
use crate::packed_bit_sequence::PackedBitSequence;
use crate::symbol_builder::build_symbol_dictionary;
use crate::symbol_definition::SymbolDefinition;
use crate::symbol_prefix_tree::{describe_symbol_prefix_tree, read_symbol_prefix_tree, SymbolPrefixTreeDescription};
use crate::symbol_stream::SymbolStream;

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

/// The builder's Huffman code lengths, listed in the prefix tree's depth-first symbol order.
fn code_lengths_in_depth_first_order(stream: &SymbolStream, prefix_tree: &SymbolPrefixTreeDescription) -> Vec<u32> {
    let code_lengths_by_symbol_id = stream.code_lengths_by_symbol_id();
    prefix_tree.symbol_ids_depth_first.iter().map(|&symbol_id| code_lengths_by_symbol_id[symbol_id]).collect()
}

/// Exact header bits the container would need for the current state.
/// Measurement only: never used by any building decision.
pub fn measure_header_bit_count(stream: &SymbolStream) -> u64 {
    let prefix_tree = describe_leaves_prefix_tree(stream);
    elias_gamma_bit_count(stream.occupied_position_count() as u64 + 1)
        + prefix_tree.bit_count()
        + measure_code_length_section_bit_count(&code_lengths_in_depth_first_order(stream, &prefix_tree))
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

/// Compresses exactly the first `input_bit_count` bits of `input_bytes`.
/// The returned bytes are the stream, its last byte padded with zeros (a file
/// holds whole bytes; the padding is not part of the format).
pub fn compress_input(
    input_bytes: &[u8],
    input_bit_count: u64,
    minimum_window_bit_length: u32,
    maximum_symbol_bit_length: u32,
    observer: &mut dyn BuilderObserver,
) -> (Vec<u8>, CompressionReport) {
    let mut bit_writer = BitWriter::new();
    if input_bit_count == 0 {
        bit_writer.write_elias_gamma(1);
        let report = CompressionReport {
            dictionary: Vec::new(),
            header_bit_count: bit_writer.bits_written,
            body_bit_count: 0,
            symbols_created: 0,
        };
        return (bit_writer.into_bytes(), report);
    }

    let mut stream = SymbolStream::from_input_bits(input_bytes, input_bit_count, minimum_window_bit_length, maximum_symbol_bit_length);
    build_symbol_dictionary(&mut stream, observer);
    bit_writer.write_elias_gamma(stream.placed_symbol_ids.len() as u64 + 1);

    let prefix_tree = describe_leaves_prefix_tree(&stream);
    prefix_tree.write(&mut bit_writer);
    let code_lengths_in_order = code_lengths_in_depth_first_order(&stream, &prefix_tree);
    write_code_length_section(&mut bit_writer, &code_lengths_in_order);
    let header_bit_count = bit_writer.bits_written;
    assert_eq!(header_bit_count, measure_header_bit_count(&stream), "header measurement and writer disagree");

    let codes_in_order = assign_canonical_codes(&code_lengths_in_order);
    let mut position_by_symbol_id = vec![usize::MAX; stream.symbol_definitions.len()];
    for (position, &symbol_id) in prefix_tree.symbol_ids_depth_first.iter().enumerate() {
        position_by_symbol_id[symbol_id] = position;
    }
    let placed_positions = stream.placed_symbol_ids.iter().map(|&symbol_id| position_by_symbol_id[symbol_id as usize]);
    let body_bit_count = encode_body(&mut bit_writer, placed_positions, &codes_in_order);
    assert_eq!(bit_writer.bits_written, header_bit_count + body_bit_count, "body bits miscounted");

    let mut dictionary: Vec<DictionaryEntry> = prefix_tree
        .symbol_ids_depth_first
        .iter()
        .enumerate()
        .map(|(leaf_index, &symbol_id)| DictionaryEntry {
            bit_string: stream.symbol_as_bit_string(symbol_id),
            bit_length: stream.symbol_definitions[symbol_id].bit_length,
            frequency: stream.symbol_frequencies[symbol_id],
            code_length: code_lengths_in_order[leaf_index],
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

/// The decoded bits and how many there are; the bytes' last one is padded with zeros.
pub fn decompress_container(container_bytes: &[u8]) -> (Vec<u8>, u64) {
    let mut bit_reader = BitReader::new(container_bytes);
    let placed_symbol_count = bit_reader.read_elias_gamma() - 1;
    if placed_symbol_count == 0 {
        return (Vec::new(), 0);
    }
    let (decoded_symbol_bits, symbol_definitions_depth_first): (PackedBitSequence, Vec<SymbolDefinition>) =
        read_symbol_prefix_tree(&mut bit_reader);
    let code_lengths_in_order = read_code_length_section(&mut bit_reader, symbol_definitions_depth_first.len() as u64);
    assert_eq!(code_lengths_in_order.len(), symbol_definitions_depth_first.len(), "code lengths and prefix tree disagree");
    let decoder = CanonicalHuffmanDecoder::from_code_lengths(&code_lengths_in_order);
    let placed_positions = decode_body(&mut bit_reader, placed_symbol_count, &decoder);
    let file_bits = 8 * container_bytes.len() as u64;
    assert!(
        bit_reader.bits_read() <= file_bits && file_bits - bit_reader.bits_read() < 8,
        "the body must end in the file's last byte, only padding after it"
    );
    let mut output_writer = BitWriter::new();
    for position in placed_positions {
        let definition = symbol_definitions_depth_first[position];
        decoded_symbol_bits.for_each_chunk(definition.first_bit_offset, definition.bit_length as u64, |chunk, count| {
            output_writer.write_bits(chunk, count)
        });
    }
    let decoded_bit_count = output_writer.bits_written;
    (output_writer.into_bytes(), decoded_bit_count)
}
