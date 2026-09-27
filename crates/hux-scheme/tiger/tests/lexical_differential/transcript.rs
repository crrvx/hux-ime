// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `lexical` transcript 的逐条重放：按记录种类分派到各自的比对。

use hux_scheme_tiger::lexical::{self, LexicalModel};
use hux_test_support::{decode_hex, parse_bits};
use std::io::BufRead;

/// 重放 lexical 金样，返回（记录数，contains 正例数）。
pub fn run(model: &LexicalModel, reader: impl BufRead) -> (usize, usize) {
    let mut records = 0usize;
    let mut positives = 0usize;
    for line in reader.lines() {
        let line = line.expect("read golden line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, rest) = line.split_once('\t').expect("record payload");
        match kind {
            "header" => check_header(model, rest),
            "hashes" => check_hashes(rest),
            "contains" => {
                if check_contains(model, rest) {
                    positives += 1;
                }
            }
            "score" => check_score(model, rest),
            other => panic!("unknown lexical record: {other}"),
        }
        records += 1;
    }
    (records, positives)
}

fn check_header(model: &LexicalModel, rest: &str) {
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
}

fn check_hashes(rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let text = decode_hex(fields[0]);
    let (first, second) = lexical::hashes(&text);
    assert_eq!(first.to_string(), fields[1], "first hash for {text:?}");
    assert_eq!(second.to_string(), fields[2], "second hash for {text:?}");
}

/// 比对一条 contains 记录，返回参照实现是否判为包含。
fn check_contains(model: &LexicalModel, rest: &str) -> bool {
    let fields: Vec<&str> = rest.split('\t').collect();
    let text = decode_hex(fields[0]);
    let expected = fields[1] == "1";
    assert_eq!(model.contains(&text), expected, "contains for {text:?}");
    expected
}

fn check_score(model: &LexicalModel, rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let text = decode_hex(fields[0]);
    let expected = parse_bits(fields[1]);
    assert_eq!(
        model.score(&text).to_bits(),
        expected,
        "score bits for {text:?}"
    );
}
