//! ASLE: Arbitrary Symbol Length Encoding. Command-line interface only.

mod bit_io;
mod builder_events;
mod change_prediction;
mod container_format;
mod fast_hash;
mod huffman_tree;
mod json_lines_log;
mod merge_candidates;
mod option_ranking;
mod predicted_changes;
mod symbol_builder;
mod symbol_definition;
mod symbol_stream;

use builder_events::*;
use container_format::{compress_input, decompress_container, CompressionReport};
use json_lines_log::JsonLinesEncoderLog;
use symbol_stream::SymbolStream;
use std::time::Instant;

const USAGE: &str = "usage:
  asle compress   <input> <output> --max-symbol-bits <n> [--verbose] [--log <file.jsonl>]
  asle decompress <input> <output>
  asle benchmark  --max-symbol-bits <n> [--verbose] <files...>";

fn exit_with_usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2)
}

/// Forwards builder events to an optional JSON Lines log, and prints one line
/// per finished window size when verbose.
struct CommandLineObserver {
    print_window_size_summaries: bool,
    json_lines_log: Option<JsonLinesEncoderLog>,
}

impl BuilderObserver for CommandLineObserver {
    fn on_build_started(&mut self, stream: &SymbolStream) {
        if let Some(log) = self.json_lines_log.as_mut() {
            log.on_build_started(stream);
        }
    }
    fn on_window_size_started(&mut self, window_bit_length: u32, stream: &SymbolStream) {
        if let Some(log) = self.json_lines_log.as_mut() {
            log.on_window_size_started(window_bit_length, stream);
        }
    }
    fn on_pass_started(&mut self, event: &PassStarted, stream: &SymbolStream) {
        if let Some(log) = self.json_lines_log.as_mut() {
            log.on_pass_started(event, stream);
        }
    }
    fn on_round_analyzed(&mut self, event: &RoundAnalyzed, stream: &SymbolStream) {
        if let Some(log) = self.json_lines_log.as_mut() {
            log.on_round_analyzed(event, stream);
        }
    }
    fn on_merge_rejected(&mut self, event: &MergeRejected, stream: &SymbolStream) {
        if let Some(log) = self.json_lines_log.as_mut() {
            log.on_merge_rejected(event, stream);
        }
    }
    fn on_change_committed(&mut self, event: &ChangeCommitted, stream_after: &SymbolStream) {
        if let Some(log) = self.json_lines_log.as_mut() {
            log.on_change_committed(event, stream_after);
        }
    }
    fn on_window_size_finished(&mut self, event: &WindowSizeFinished, stream: &SymbolStream) {
        if self.print_window_size_summaries {
            eprintln!(
                "  window={:3} passes={:3} merges={:5} merges_evaluated={:7} leaves={:5} encoded_symbol_bits={} ms={}",
                event.window_bit_length,
                event.passes,
                event.merges_committed,
                event.merges_evaluated,
                stream.leaf_count(),
                stream.encoded_symbol_bits(),
                event.elapsed_milliseconds
            );
        }
        if let Some(log) = self.json_lines_log.as_mut() {
            log.on_window_size_finished(event, stream);
        }
    }
    fn on_build_finished(&mut self, event: &BuildFinished, stream: &SymbolStream) {
        if let Some(log) = self.json_lines_log.as_mut() {
            log.on_build_finished(event, stream);
        }
    }
}

/// Integer "x.yyy" formatting of numerator / denominator.
fn format_ratio(numerator: u64, denominator: u64) -> String {
    if denominator == 0 {
        return "-".into();
    }
    let thousandths = numerator * 1000 / denominator;
    format!("{}.{:03}", thousandths / 1000, thousandths % 1000)
}

fn print_report(
    input_name: &str,
    input_bytes: &[u8],
    container_bytes: &[u8],
    report: &CompressionReport,
    build_seconds: f64,
    decode_seconds: Option<f64>,
    print_dictionary: bool,
) {
    println!(
        "{input_name}: {} -> {} bytes ({} bits/byte)  header={} body={} bits  leaves={}  build={:.2}s{}",
        input_bytes.len(),
        container_bytes.len(),
        format_ratio(container_bytes.len() as u64 * 8, input_bytes.len() as u64),
        report.header_bit_count,
        report.body_bit_count,
        report.dictionary.len(),
        build_seconds,
        decode_seconds.map(|seconds| format!(" decode={seconds:.2}s")).unwrap_or_default()
    );
    let mut leaves_by_bit_length = std::collections::BTreeMap::new();
    let mut leaves_by_frequency_band = std::collections::BTreeMap::new();
    for entry in &report.dictionary {
        *leaves_by_bit_length.entry(entry.definition.bit_length).or_insert(0u32) += 1;
        *leaves_by_frequency_band.entry(64 - entry.frequency.leading_zeros()).or_insert(0u32) += 1;
    }
    let lengths: Vec<String> = leaves_by_bit_length.iter().map(|(bits, count)| format!("{bits}b:{count}")).collect();
    let bands: Vec<String> =
        leaves_by_frequency_band.iter().map(|(band, count)| format!("<{}:{count}", 1u64 << band)).collect();
    println!("  leaves by symbol length: {}", lengths.join(" "));
    println!(
        "  symbols created={} alive={} absorbed={}  leaves by frequency: {}",
        report.symbols_created,
        report.dictionary.len(),
        report.symbols_created - report.dictionary.len(),
        bands.join(" ")
    );
    if print_dictionary {
        for entry in report.dictionary.iter().take(16) {
            println!(
                "    {:>20}  frequency={:<8} code_length={}",
                entry.definition.as_bit_string(),
                entry.frequency,
                entry.code_length
            );
        }
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() < 2 {
        exit_with_usage();
    }
    let mut maximum_symbol_bit_length: Option<u32> = None;
    let mut verbose = false;
    let mut log_path: Option<String> = None;
    let mut positional_arguments: Vec<String> = Vec::new();
    let mut argument_index = 2;
    while argument_index < arguments.len() {
        match arguments[argument_index].as_str() {
            "--max-symbol-bits" => {
                argument_index += 1;
                maximum_symbol_bit_length =
                    Some(arguments.get(argument_index).and_then(|text| text.parse().ok()).unwrap_or_else(|| exit_with_usage()));
            }
            "--verbose" => verbose = true,
            "--log" => {
                argument_index += 1;
                log_path = Some(arguments.get(argument_index).cloned().unwrap_or_else(|| exit_with_usage()));
            }
            other => positional_arguments.push(other.to_string()),
        }
        argument_index += 1;
    }
    match arguments[1].as_str() {
        "compress" => {
            let (Some(maximum_symbol_bit_length), [input_path, output_path]) =
                (maximum_symbol_bit_length, positional_arguments.as_slice())
            else {
                exit_with_usage()
            };
            let input_bytes = std::fs::read(input_path).expect("could not read input");
            let json_lines_log =
                log_path.map(|path| JsonLinesEncoderLog::create(&path).expect("could not create log file"));
            let mut observer = CommandLineObserver { print_window_size_summaries: verbose, json_lines_log };
            let build_start = Instant::now();
            let (container_bytes, report) = compress_input(&input_bytes, maximum_symbol_bit_length, &mut observer);
            let build_seconds = build_start.elapsed().as_secs_f64();
            std::fs::write(output_path, &container_bytes).expect("could not write output");
            print_report(input_path, &input_bytes, &container_bytes, &report, build_seconds, None, verbose);
        }
        "decompress" => {
            let [input_path, output_path] = positional_arguments.as_slice() else { exit_with_usage() };
            let container_bytes = std::fs::read(input_path).expect("could not read input");
            std::fs::write(output_path, decompress_container(&container_bytes)).expect("could not write output");
        }
        "benchmark" => {
            let Some(maximum_symbol_bit_length) = maximum_symbol_bit_length else { exit_with_usage() };
            for input_path in &positional_arguments {
                let input_bytes = std::fs::read(input_path).expect("could not read input");
                let mut observer = CommandLineObserver { print_window_size_summaries: verbose, json_lines_log: None };
                let build_start = Instant::now();
                let (container_bytes, report) = compress_input(&input_bytes, maximum_symbol_bit_length, &mut observer);
                let build_seconds = build_start.elapsed().as_secs_f64();
                let decode_start = Instant::now();
                let decoded_bytes = decompress_container(&container_bytes);
                let decode_seconds = decode_start.elapsed().as_secs_f64();
                assert!(decoded_bytes == input_bytes, "ROUND TRIP FAILED for {input_path}");
                print_report(input_path, &input_bytes, &container_bytes, &report, build_seconds, Some(decode_seconds), verbose);
            }
        }
        _ => exit_with_usage(),
    }
}
