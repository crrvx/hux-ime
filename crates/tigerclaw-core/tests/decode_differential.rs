//! 差分测试：重放 decode 金样（冷路径、无学习），逐位比对。
//!
//! 金样由 `tools/gen_decode_golden.lua` 生成：
//! * `goldens/decode.tsv.gz`：无模型；
//! * `goldens/decode_model.tsv.gz`：fixture 模型。

use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use tigerclaw_core::decode::Decoder;
use tigerclaw_core::lexicon::{Lexicon, Supplement};
use tigerclaw_core::ngram::MobileModel;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn decode_hex(text: &str) -> String {
    if text == "-" {
        return String::new();
    }
    let bytes: Vec<u8> = (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit"))
        .collect();
    String::from_utf8(bytes).expect("valid UTF-8")
}

fn parse_bits(text: &str) -> u64 {
    let digits = text.strip_prefix("0x").expect("0x prefix");
    let hi = u64::from_str_radix(&digits[..8], 16).expect("hex digit");
    let lo = u64::from_str_radix(&digits[8..], 16).expect("hex digit");
    hi << 32 | lo
}

fn field<'a>(part: &'a str, name: &str) -> &'a str {
    part.strip_prefix(name)
        .unwrap_or_else(|| panic!("expected {name}=..., got {part}"))
}

fn open_golden(relative: &str) -> BufReader<GzDecoder<File>> {
    let file =
        File::open(repo_path(relative)).unwrap_or_else(|error| panic!("open {relative}: {error}"));
    BufReader::new(GzDecoder::new(file))
}

fn make_decoder(model: Option<MobileModel>) -> Decoder {
    let data_dir = repo_path("goldens/lexicon");
    let lexicon = Lexicon::load(std::slice::from_ref(&data_dir), 1500);
    let supplement = Supplement::load_default(Some(&data_dir));
    Decoder::new(lexicon, supplement, model)
}

fn replay(mut decoder: Decoder, reader: impl BufRead) -> usize {
    let mut lines = reader
        .lines()
        .map(|line| line.expect("read golden line"))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let mut records = 0usize;
    while let Some(line) = lines.next() {
        let (kind, rest) = line.split_once('\t').expect("record payload");
        assert_eq!(kind, "decode", "expected decode record, got {kind}");
        let mut parts = rest.split('\t');
        let input = decode_hex(parts.next().expect("input"));
        let count: usize = field(parts.next().expect("count"), "count=")
            .parse()
            .expect("count value");
        let learning: u8 = field(parts.next().expect("learning"), "learning=")
            .parse()
            .expect("learning value");
        let truncated: u8 = field(parts.next().expect("truncated"), "truncated=")
            .parse()
            .expect("truncated value");

        let output = decoder.decode(&input).expect("decode");
        assert_eq!(output.items.len(), count, "count mismatch for {input:?}");
        assert_eq!(
            output.learning_affected as u8, learning,
            "learning flag mismatch for {input:?}"
        );
        assert_eq!(
            output.completed_truncated as u8, truncated,
            "truncated flag mismatch for {input:?}"
        );
        for (position, expected) in output.items.iter().enumerate() {
            let line = lines.next().expect("result record");
            let (kind, rest) = line.split_once('\t').expect("result payload");
            assert_eq!(kind, "result", "expected result record, got {kind}");
            let mut fields = rest.split('\t');
            let text = decode_hex(fields.next().expect("text"));
            let segmented = decode_hex(fields.next().expect("segmented"));
            let score = parse_bits(fields.next().expect("score"));
            let confidence = parse_bits(fields.next().expect("confidence"));
            let max_rank: usize = fields.next().expect("max_rank").parse().expect("rank");
            let edge_count: usize = fields.next().expect("edge_count").parse().expect("edges");
            let supplement = parse_bits(fields.next().expect("supplement"));
            let learning_score = parse_bits(fields.next().expect("learning"));

            let context = format!("{input:?} #{position}");
            assert_eq!(expected.text, text, "text mismatch for {context}");
            assert_eq!(
                expected.segmented, segmented,
                "segmented mismatch for {context}"
            );
            assert_eq!(
                expected.score.to_bits(),
                score,
                "score mismatch for {context}: got 0x{:016x} want 0x{score:016x}",
                expected.score.to_bits()
            );
            assert_eq!(
                expected.confidence_score.to_bits(),
                confidence,
                "confidence mismatch for {context}"
            );
            assert_eq!(
                expected.max_rank, max_rank,
                "max_rank mismatch for {context}"
            );
            assert_eq!(
                expected.edge_count, edge_count,
                "edge_count mismatch for {context}"
            );
            assert_eq!(
                expected.supplement_score.to_bits(),
                supplement,
                "supplement mismatch for {context}"
            );
            assert_eq!(
                expected.learning_score.to_bits(),
                learning_score,
                "learning mismatch for {context}"
            );
            records += 1;
        }
        records += 1;
    }
    records
}

#[test]
fn decode_transcript_is_bit_exact_without_model() {
    let records = replay(make_decoder(None), open_golden("goldens/decode.tsv.gz"));
    assert!(records > 1_900, "transcript too short: {records}");
    println!("decode (no model): {records} golden records verified");
}

#[test]
fn decode_transcript_is_bit_exact_with_fixture_model() {
    let model = MobileModel::load(repo_path("goldens/ngram_fixture.bin"), None)
        .expect("load fixture model");
    let records = replay(
        make_decoder(Some(model)),
        open_golden("goldens/decode_model.tsv.gz"),
    );
    assert!(records > 300, "transcript too short: {records}");
    println!("decode (fixture model): {records} golden records verified");
}

#[test]
fn decode_rank_first_transcript_is_bit_exact() {
    let model = MobileModel::load(repo_path("goldens/ngram_fixture.bin"), None)
        .expect("load fixture model");
    let mut decoder = make_decoder(Some(model));
    decoder.set_allow_duplicate_single(false);
    let records = replay(decoder, open_golden("goldens/decode_rank_first.tsv.gz"));
    assert!(records > 300, "transcript too short: {records}");
    println!("decode (model, no duplicate): {records} golden records verified");
}
