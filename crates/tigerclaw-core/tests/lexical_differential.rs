//! 差分测试：重放 lexical 金样（TCSLEX01 读取 / Bloom / 最大权词覆盖打分）逐位比对。
//!
//! 金样由 `tools/gen_lexical_golden.lua` 生成：参照 main ≥ `35a10b9` 的词先验模块 +
//! 真实位图 `data/tiger_sentence.lexical.bin`；语料取自参照码表（正例）与确定性
//! 采样（负例），全量记录查询与结果，故重放不依赖任何外部词表。

mod common;

use common::{decode_hex, open_golden, parse_bits, repo_path};
use std::io::BufRead;
use tigerclaw_core::lexical;

#[test]
fn lexical_transcript_is_bit_exact() {
    let model = lexical::load(&repo_path("data/tiger_sentence.lexical.bin"))
        .expect("load real lexical model");
    let mut records = 0usize;
    let mut positives = 0usize;
    for line in open_golden("goldens/lexical.tsv.gz").lines() {
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
