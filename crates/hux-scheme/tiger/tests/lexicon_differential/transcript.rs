// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `lexicon` transcript 的逐条重放：按记录种类分派到各自的比对。

use hux_scheme_tiger::lexicon::{Lexicon, Supplement};
use hux_test_support::{decode_hex as decode, hex as encode};
use std::io::BufRead;

/// 重放 transcript，返回记录数；任何一条与本地实现不一致即 panic。
pub fn run(lexicon: &mut Lexicon, supplement: &Supplement, reader: impl BufRead) -> usize {
    let mut records = 0usize;
    for line in reader.lines() {
        let line = line.expect("read golden line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, rest) = line.split_once('\t').unwrap_or((&line, ""));
        match kind {
            "status" => check_status(lexicon, rest, records),
            "lengths" => check_lengths(lexicon, rest, records),
            "probe" => check_probe(lexicon, rest),
            "limit" => {
                let limit: usize = rest.parse().expect("limit value");
                lexicon.apply_high_freq_limit(limit);
            }
            "supp" => check_supplement(supplement, rest, records),
            other => panic!("unknown record kind: {other}"),
        }
        records += 1;
    }
    records
}

fn check_status(lexicon: &Lexicon, rest: &str, records: usize) {
    assert_eq!(
        lexicon.data_status().canonical(),
        rest,
        "data status mismatch at record {records}"
    );
}

fn check_lengths(lexicon: &Lexicon, rest: &str, records: usize) {
    let got = lexicon
        .lengths()
        .iter()
        .map(|length| length.to_string())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(got, rest, "lengths mismatch at record {records}");
}

fn check_probe(lexicon: &Lexicon, rest: &str) {
    let (code_hex, items) = rest.split_once('\t').expect("probe payload");
    let code = decode(code_hex);
    let got = match lexicon.probe(&code) {
        None => "-".to_string(),
        Some(entries) => entries
            .iter()
            .map(|entry| {
                format!(
                    "{}:{}:{}",
                    encode(entry.text.as_bytes()),
                    entry.rank,
                    entry.optimal_single as u8
                )
            })
            .collect::<Vec<_>>()
            .join(","),
    };
    assert_eq!(got, items, "probe mismatch for code {code:?}");
}

fn check_supplement(supplement: &Supplement, rest: &str, records: usize) {
    assert_eq!(
        supplement.status().canonical(),
        rest,
        "supplement status mismatch at record {records}"
    );
}
