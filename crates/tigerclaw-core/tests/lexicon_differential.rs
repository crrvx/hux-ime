//! 差分测试：重放 lexicon 金样 transcript，逐条比对。
//!
//! 金样由 `tools/gen_lexicon_golden.lua` 生成：
//! * present：`goldens/lexicon/` 数据 + `goldens/lexicon.tsv.gz`；
//! * missing：数据缺失路径 + `goldens/lexicon_missing.tsv.gz`。

use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use tigerclaw_core::lexicon::{Lexicon, Supplement};

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// transcript 字符串参数：`-` 表示空串，其余为 UTF-8 字节的小写十六进制。
fn decode(text: &str) -> String {
    if text == "-" {
        return String::new();
    }
    let bytes: Vec<u8> = (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit"))
        .collect();
    String::from_utf8(bytes).expect("golden argument is valid UTF-8")
}

fn encode(text: &str) -> String {
    if text.is_empty() {
        return "-".to_string();
    }
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.as_bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn open_golden(relative: &str) -> BufReader<GzDecoder<File>> {
    let file =
        File::open(repo_path(relative)).unwrap_or_else(|error| panic!("open {relative}: {error}"));
    BufReader::new(GzDecoder::new(file))
}

fn run_transcript(lexicon: &mut Lexicon, supplement: &Supplement, reader: impl BufRead) -> usize {
    let mut records = 0usize;
    for line in reader.lines() {
        let line = line.expect("read golden line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, rest) = line.split_once('\t').unwrap_or((&line, ""));
        match kind {
            "status" => {
                assert_eq!(
                    lexicon.data_status().canonical(),
                    rest,
                    "data status mismatch at record {records}"
                );
            }
            "lengths" => {
                let got = lexicon
                    .lengths()
                    .iter()
                    .map(|length| length.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                assert_eq!(got, rest, "lengths mismatch at record {records}");
            }
            "probe" => {
                let (code_hex, items) = rest.split_once('\t').expect("probe payload");
                let code = decode(code_hex);
                let got = match lexicon.probe(&code) {
                    None => "-".to_string(),
                    Some(entries) => entries
                        .iter()
                        .map(|entry| {
                            format!(
                                "{}:{}:{}",
                                encode(&entry.text),
                                entry.rank,
                                entry.optimal_single as u8
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(","),
                };
                assert_eq!(got, items, "probe mismatch for code {code:?}");
            }
            "limit" => {
                let limit: usize = rest.parse().expect("limit value");
                lexicon.apply_high_freq_limit(limit);
            }
            "supp" => {
                assert_eq!(
                    supplement.status().canonical(),
                    rest,
                    "supplement status mismatch at record {records}"
                );
            }
            other => panic!("unknown record kind: {other}"),
        }
        records += 1;
    }
    records
}

fn replay(data_relative: &str, golden_relative: &str) -> usize {
    let data_dir = repo_path(data_relative);
    let mut lexicon = Lexicon::load(std::slice::from_ref(&data_dir), 1500);
    let supplement = Supplement::load_default(Some(&data_dir));
    run_transcript(&mut lexicon, &supplement, open_golden(golden_relative))
}

#[test]
fn lexicon_present_transcript_is_exact() {
    let records = replay("goldens/lexicon", "goldens/lexicon.tsv.gz");
    assert!(records > 18_000, "transcript too short: {records} records");
    println!("lexicon present: {records} golden records verified");
}

#[test]
fn lexicon_variants_transcript_is_exact() {
    let records = replay(
        "goldens/lexicon_variants",
        "goldens/lexicon_variants.tsv.gz",
    );
    assert!(records > 20, "transcript too short: {records} records");
    println!("lexicon variants: {records} golden records verified");
}

#[test]
fn lexicon_codes_only_transcript_is_exact() {
    let records = replay(
        "goldens/lexicon_codes_only",
        "goldens/lexicon_codes_only.tsv.gz",
    );
    assert!(records > 10, "transcript too short: {records} records");
    println!("lexicon codes-only: {records} golden records verified");
}

#[test]
fn lexicon_missing_transcript_is_exact() {
    let records = replay("goldens/no-such-data-dir", "goldens/lexicon_missing.tsv.gz");
    assert_eq!(records, 5, "unexpected record count: {records}");
    println!("lexicon missing: {records} golden records verified");
}
