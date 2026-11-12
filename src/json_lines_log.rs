//! An observer that writes every builder event as one JSON object per line.
//! Every line also carries the state at that moment: leaves, placed symbols,
//! encoded symbol bits (Huffman, as the builder decides), and the exact
//! container header bits (measured, never used to decide).

use crate::builder_events::*;
use crate::container_format::measure_header_bit_count;
use crate::symbol_stream::SymbolStream;
use std::fs::File;
use std::io::{BufWriter, Write};

pub struct JsonLinesEncoderLog {
    writer: BufWriter<File>,
    commit_number: u64,
}

impl JsonLinesEncoderLog {
    pub fn create(path: &str) -> std::io::Result<Self> {
        Ok(JsonLinesEncoderLog { writer: BufWriter::new(File::create(path)?), commit_number: 0 })
    }

    fn write_line(&mut self, event_name: &str, fields: String, stream: &SymbolStream) {
        let encoded_symbol_bits = stream.encoded_symbol_bits();
        let header_bit_count = measure_header_bit_count(stream);
        let _ = writeln!(
            self.writer,
            "{{\"event\":\"{event_name}\",{fields}\"leaves\":{},\"placed_symbols\":{},\"encoded_symbol_bits\":{encoded_symbol_bits},\"header_bits\":{header_bit_count},\"encoded_symbol_bits_plus_header_bits\":{}}}",
            stream.leaf_count(),
            stream.occupied_position_count(),
            encoded_symbol_bits + header_bit_count
        );
    }
}

fn quoted_symbol_names(stream: &SymbolStream, symbol_ids: impl Iterator<Item = usize>) -> String {
    symbol_ids.map(|symbol_id| format!("\"{}\"", stream.symbol_as_bit_string(symbol_id))).collect::<Vec<_>>().join(",")
}

impl BuilderObserver for JsonLinesEncoderLog {
    fn on_build_started(&mut self, stream: &SymbolStream) {
        let fields = format!(
            "\"input_bits\":{},\"minimum_window_bit_length\":{},\"maximum_symbol_bit_length\":{},",
            stream.input_bit_count, stream.minimum_window_bit_length, stream.maximum_symbol_bit_length
        );
        self.write_line("build_started", fields, stream);
    }

    fn on_window_size_started(&mut self, window_bit_length: u32, stream: &SymbolStream) {
        self.write_line("window_size_started", format!("\"window_bit_length\":{window_bit_length},"), stream);
    }

    fn on_pass_started(&mut self, event: &PassStarted, stream: &SymbolStream) {
        let fields = format!(
            "\"window_bit_length\":{},\"pass\":{},\"candidates\":{},\"occurrences\":{},",
            event.window_bit_length, event.pass_number, event.candidate_count, event.occurrence_count
        );
        self.write_line("pass_started", fields, stream);
    }

    fn on_round_analyzed(&mut self, event: &RoundAnalyzed, stream: &SymbolStream) {
        let fields = format!(
            "\"window_bit_length\":{},\"pass\":{},\"round\":{},\"merges_ranked\":{},\"non_saving_leaves\":{},\"non_saving_leaves_unspellable\":{},\"candidates_dropped_for_single_occurrence\":{},\"merges_that_lower_the_leaf_count\":{},",
            event.window_bit_length,
            event.pass_number,
            event.round_number,
            event.merges_ranked,
            event.non_saving_leaves,
            event.non_saving_leaves_unspellable,
            event.candidates_dropped_for_single_occurrence,
            event.merges_that_lower_the_leaf_count
        );
        self.write_line("round_analyzed", fields, stream);
    }

    fn on_merge_rejected(&mut self, event: &MergeRejected, stream: &SymbolStream) {
        let reason = match event.reason {
            RejectionReason::EncodedSymbolBitsDoNotDrop => "encoded_symbol_bits_do_not_drop",
        };
        let fields = format!(
            "\"window_bit_length\":{},\"pass\":{},\"round\":{},\"rank_position\":{},\"symbol\":\"{}\",\"frequency\":{},\"reason\":\"{reason}\",\"merged_code_bits_after\":{},\"consumed_code_bits_before\":{},",
            event.window_bit_length,
            event.pass_number,
            event.round_number,
            event.rank_position,
            stream.definition_as_bit_string(event.merged_definition),
            event.frequency,
            event.merged_code_bits_after,
            event.consumed_code_bits_before
        );
        self.write_line("merge_rejected", fields, stream);
    }

    fn on_change_committed(&mut self, event: &ChangeCommitted, stream_after: &SymbolStream) {
        self.commit_number += 1;
        let kind = match event.kind {
            CommitKind::Merge => "merge",
            CommitKind::MergeThatLowersTheLeafCount => "merge_that_lowers_the_leaf_count",
        };
        let consumed_parts = event
            .consumed_parts
            .iter()
            .map(|part| {
                format!(
                    "{{\"symbol\":\"{}\",\"consumed\":{},\"frequency_after_merge\":{}}}",
                    stream_after.symbol_as_bit_string(part.symbol_id),
                    part.consumed_count,
                    part.frequency_after_merge
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let fields = format!(
            "\"commit_number\":{},\"window_bit_length\":{},\"pass\":{},\"round\":{},\"rank_position\":{},\"kind\":\"{kind}\",\"symbol\":\"{}\",\"symbol_is_new\":{},\"lowered_the_total\":{},\"frequency\":{},\"consumed_parts\":[{consumed_parts}],\"merged_code_bits_after\":{},\"consumed_code_bits_before\":{},\"score_numerator\":{},\"encoded_symbol_bits_before\":{},\"encoded_symbol_bits_after\":{},\"leaves_before\":{},\"removed_leaves\":[{}],\"added_leaves\":[{}],\"placed_symbols_before\":{},",
            self.commit_number,
            event.window_bit_length,
            event.pass_number,
            event.round_number,
            event.rank_position,
            stream_after.definition_as_bit_string(event.merged_definition),
            event.merged_symbol_is_new,
            event.lowered_the_total,
            event.frequency,
            event.merged_code_bits_after,
            event.consumed_code_bits_before,
            event.score_numerator,
            event.encoded_symbol_bits_before,
            event.encoded_symbol_bits_after,
            event.leaves_before,
            quoted_symbol_names(stream_after, event.removed_leaf_ids.iter().copied()),
            quoted_symbol_names(stream_after, event.added_leaf_ids.iter().copied()),
            event.placed_symbols_before
        );
        self.write_line("change_committed", fields, stream_after);
    }

    fn on_leaf_removed(&mut self, event: &LeafRemoved, stream_after: &SymbolStream) {
        self.commit_number += 1;
        let fields = format!(
            "\"commit_number\":{},\"window_bit_length\":{},\"pass\":{},\"round\":{},\"removed_symbol\":\"{}\",\"removed_frequency\":{},\"removed_bit_length\":{},\"removed_code_length_before\":{},\"removed_code_bits_before\":{},\"respelled_as\":[{}],\"replacement_code_bits_after\":{},\"encoded_symbol_bits_before\":{},\"encoded_symbol_bits_after\":{},\"leaves_before\":{},\"non_saving_leaves\":{},\"lowered_the_total\":{},\"removed_because_placed_once\":{},",
            self.commit_number,
            event.window_bit_length,
            event.pass_number,
            event.round_number,
            stream_after.symbol_as_bit_string(event.removed_symbol_id),
            event.removed_frequency,
            event.removed_bit_length,
            event.removed_code_length_before,
            event.removed_code_bits_before,
            quoted_symbol_names(stream_after, event.replacement_symbol_ids.iter().map(|&id| id as usize)),
            event.replacement_code_bits_after,
            event.encoded_symbol_bits_before,
            event.encoded_symbol_bits_after,
            event.leaves_before,
            event.non_saving_leaves,
            event.lowered_the_total,
            event.removed_because_placed_once
        );
        self.write_line("leaf_removed", fields, stream_after);
    }

    fn on_window_size_finished(&mut self, event: &WindowSizeFinished, stream: &SymbolStream) {
        let fields = format!(
            "\"window_bit_length\":{},\"passes\":{},\"merges_committed\":{},\"merge_committed_without_lowering_the_total\":{},\"leaf_removed_without_lowering_the_total\":{},\"leaves_removed\":{},\"single_placement_leaves_removed\":{},\"stopped_by_repeated_state\":{},\"merges_evaluated\":{},\"elapsed_milliseconds\":{},",
            event.window_bit_length,
            event.passes,
            event.merges_committed,
            event.merge_committed_without_lowering_the_total,
            event.leaf_removed_without_lowering_the_total,
            event.leaves_removed,
            event.single_placement_leaves_removed,
            event.stopped_by_repeated_state,
            event.merges_evaluated,
            event.elapsed_milliseconds
        );
        self.write_line("window_size_finished", fields, stream);
    }

    fn on_build_finished(&mut self, event: &BuildFinished, stream: &SymbolStream) {
        let fields = format!(
            "\"symbols_created\":{},\"elapsed_milliseconds\":{},",
            event.symbols_created, event.elapsed_milliseconds
        );
        self.write_line("build_finished", fields, stream);
        let _ = self.writer.flush();
    }
}
