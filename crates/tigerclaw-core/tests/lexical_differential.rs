//! 差分测试：重放 lexical 金样（TCSLEX01 读取 / Bloom / 最大权词覆盖打分）逐位比对。
//!
//! 金样由 `tools/gen_lexical_golden.lua` 生成：参照 main ≥ `35a10b9` 的词先验模块 +
//! 真实位图 `data/tiger_sentence.lexical.bin`；语料取自参照码表（正例）与确定性
//! 采样（负例），全量记录查询与结果，故重放不依赖任何外部词表。

use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use tigerclaw_core::lexical;

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

#[test]
fn lexical_transcript_is_bit_exact() {
    let model = lexical::load(&repo_path("data/tiger_sentence.lexical.bin"))
        .expect("load real lexical model");
    let file = File::open(repo_path("goldens/lexical.tsv.gz")).expect("open lexical golden");
    let mut records = 0usize;
    let mut positives = 0usize;
    for line in BufReader::new(GzDecoder::new(file)).lines() {
        let line = line.expect("read golden line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, rest) = line.split_once('\t').expect("record payload");
        match kind {
            "header" => {
                let fields: Vec<&str> = rest.split('\t').collect();
                let value = |name: &str| -> usize {
                    fields
                        .iter()
                        .find_map(|part| part.strip_prefix(name))
                        .unwrap_or_else(|| panic!("header field {name}"))
                        .parse()
                        .expect("header number")
                };
                assert_eq!(value("bytes="), model.bytes, "bytes mismatch");
                assert_eq!(value("entries="), model.entry_count, "entries mismatch");
                assert_eq!(value("bits="), model.bit_count, "bits mismatch");
                assert_eq!(value("hashes="), model.hash_count, "hashes mismatch");
                assert_eq!(value("min="), model.minimum_length, "min mismatch");
                assert_eq!(value("max="), model.maximum_length, "max mismatch");
                records += 1;
            }
            "hashes" => {
                let fields: Vec<&str> = rest.split('\t').collect();
                let text = decode_hex(fields[0]);
                let (first, second) = lexical::hashes(&text);
                assert_eq!(first.to_string(), fields[1], "first hash for {text:?}");
                assert_eq!(second.to_string(), fields[2], "second hash for {text:?}");
                records += 1;
            }
            "contains" => {
                let fields: Vec<&str> = rest.split('\t').collect();
                let text = decode_hex(fields[0]);
                let expected = fields[1] == "1";
                assert_eq!(model.contains(&text), expected, "contains for {text:?}");
                if expected {
                    positives += 1;
                }
                records += 1;
            }
            "score" => {
                let fields: Vec<&str> = rest.split('\t').collect();
                let text = decode_hex(fields[0]);
                let expected = parse_bits(fields[1]);
                assert_eq!(
                    model.score(&text).to_bits(),
                    expected,
                    "score bits for {text:?}"
                );
                records += 1;
            }
            other => panic!("unknown lexical record: {other}"),
        }
    }
    assert!(records > 700, "transcript too short: {records}");
    assert!(positives > 50, "too few positive samples: {positives}");
    println!("lexical: {records} golden records verified ({positives} positives)");
}
