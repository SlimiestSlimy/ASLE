//! An observer that writes every builder event as one JSON object per line.
//! Every line also carries the state at that moment: leaves, placed symbols,
//! W, and the exact header and total bits (measured, never used to decide).

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
        let huffman_weight = stream.huffman_weight_in_bits();
        let header_bit_count = measure_header_bit_count(stream);
        let _ = writeln!(
            self.writer,
            "{{\"event\":\"{event_name}\",{fields}\"leaves\":{},\"placed_symbols\":{},\"huffman_weight_W\":{huffman_weight},\"header_bits\":{header_bit_count},\"total_bits\":{}}}",
            stream.leaf_count(),
            stream.occupied_position_count(),
            huffman_weight + header_bit_count
        );
    }
}

fn quoted_symbol_names(stream: &SymbolStream, symbol_ids: impl Iterator<Item = usize>) -> String {
    symbol_ids.map(|symbol_id| format!("\"{}\"", stream.symbol_as_bit_string(symbol_id))).collect::<Vec<_>>().join(",")
}

impl BuilderObserver for JsonLinesEncoderLog {
    fn on_build_started(&mut self, stream: &SymbolStream) {
        let fields = format!(
            "\"input_bits\":{},\"maximum_symbol_bit_length\":{},",
            stream.input_bit_count, stream.maximum_symbol_bit_length
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
            "\"window_bit_length\":{},\"pass\":{},\"round\":{},\"options\":{},\"best_options_with_deletion\":{},\"candidates_dropped_for_single_occurrence\":{},\"merges_that_shrink_alone\":{},\"deletion_subsets_tried\":{},\"deletion_subsets_unspellable\":{},\"deletion_subsets_not_shrinking\":{},\"deletion_subsets_shrinking\":{},",
            event.window_bit_length,
            event.pass_number,
            event.round_number,
            event.option_count,
            event.best_options_with_deletion,
            event.candidates_dropped_for_single_occurrence,
            event.merges_that_shrink_alone,
            event.deletion_subsets_tried,
            event.deletion_subsets_unspellable,
            event.deletion_subsets_not_shrinking,
            event.deletion_subsets_shrinking
        );
        self.write_line("round_analyzed", fields, stream);
    }

    fn on_merge_rejected(&mut self, event: &MergeRejected, stream: &SymbolStream) {
        let reason = match event.reason {
            RejectionReason::MergedCodeNotShorter => "merged_code_not_shorter",
        };
        let fields = format!(
            "\"window_bit_length\":{},\"pass\":{},\"round\":{},\"rank_position\":{},\"symbol\":\"{}\",\"frequency\":{},\"reason\":\"{reason}\",\"merged_code_bits_after\":{},\"consumed_code_bits_before\":{},\"symbols_its_deletion_would_remove\":{},",
            event.window_bit_length,
            event.pass_number,
            event.round_number,
            event.rank_position,
            event.merged_definition.as_bit_string(),
            event.frequency,
            event.merged_code_bits_after,
            event.consumed_code_bits_before,
            event.symbols_its_deletion_would_remove
        );
        self.write_line("merge_rejected", fields, stream);
    }

    fn on_change_committed(&mut self, event: &ChangeCommitted, stream_after: &SymbolStream) {
        self.commit_number += 1;
        let kind = match event.kind {
            CommitKind::Merge => "merge",
            CommitKind::MergeThatShrinksAlone => "merge_that_shrinks_alone",
            CommitKind::MergeWithDeletion => "merge_with_deletion",
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
        let respellings = event
            .respellings
            .iter()
            .map(|respelling| {
                format!(
                    "{{\"deleted_symbol\":\"{}\",\"deleted_frequency\":{},\"respelled_as\":[{}]}}",
                    stream_after.symbol_as_bit_string(respelling.deleted_symbol_id),
                    respelling.deleted_frequency,
                    quoted_symbol_names(stream_after, respelling.replacement_symbol_ids.iter().map(|&id| id as usize))
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let fields = format!(
            "\"commit_number\":{},\"window_bit_length\":{},\"pass\":{},\"round\":{},\"rank_position\":{},\"kind\":\"{kind}\",\"symbol\":\"{}\",\"symbol_is_new\":{},\"frequency\":{},\"consumed_parts\":[{consumed_parts}],\"merged_code_bits_after\":{},\"consumed_code_bits_before\":{},\"score_numerator\":{},\"score_denominator\":{},\"W_before\":{},\"W_after_merge\":{},\"W_final\":{},\"leaves_before\":{},\"removed_leaves\":[{}],\"added_leaves\":[{}],\"revived_symbols\":{},\"respellings\":[{respellings}],\"placed_symbols_before\":{},\"consumed_placements_from_respelling\":{},",
            self.commit_number,
            event.window_bit_length,
            event.pass_number,
            event.round_number,
            event.rank_position,
            event.merged_definition.as_bit_string(),
            event.merged_symbol_is_new,
            event.frequency,
            event.merged_code_bits_after,
            event.consumed_code_bits_before,
            event.score_numerator,
            event.score_denominator,
            event.huffman_weight_before,
            event.huffman_weight_after_merge,
            event.huffman_weight_final,
            event.leaves_before,
            quoted_symbol_names(stream_after, event.removed_leaf_ids.iter().copied()),
            quoted_symbol_names(stream_after, event.added_leaf_ids.iter().copied()),
            event.revived_symbol_count,
            event.placed_symbols_before,
            event.consumed_placements_from_respelling
        );
        self.write_line("change_committed", fields, stream_after);
    }

    fn on_window_size_finished(&mut self, event: &WindowSizeFinished, stream: &SymbolStream) {
        let fields = format!(
            "\"window_bit_length\":{},\"passes\":{},\"merges_committed\":{},\"parts_deleted\":{},\"options_evaluated\":{},\"most_parts_in_one_candidate\":{},\"stopped_by_repeated_state\":{},\"elapsed_milliseconds\":{},",
            event.window_bit_length,
            event.passes,
            event.merges_committed,
            event.parts_deleted,
            event.options_evaluated,
            event.most_parts_in_one_candidate,
            event.stopped_by_repeated_state,
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
