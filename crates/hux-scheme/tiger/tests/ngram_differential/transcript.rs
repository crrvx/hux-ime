// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ngram` transcript 的逐条重放：按记录种类分派到各自的比对。

use hux_scheme_tiger::ngram::{Limits, MobileModel};
use hux_test_support::{decode_hex as decode, parse_bits};
use std::io::BufRead;

/// 重放一份 transcript，返回记录数；任何一条与本地实现不一致即 panic。
pub fn run(model: &mut MobileModel, reader: impl BufRead) -> usize {
    let mut records = 0usize;
    for line in reader.lines() {
        let line = line.expect("read golden line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, rest) = line.split_once('\t').unwrap_or((&line, ""));
        match kind {
            "bytes" => check_bytes(model, rest),
            "logp" => check_logp(model, rest),
            "obs" => check_observed(model, rest),
            "status" => check_status(model, rest, records),
            "cfg" => check_configure(model, rest),
            "trim" => model.trim_caches(),
            "close" => model.close(),
            other => panic!("unknown record kind: {other}"),
        }
        records += 1;
    }
    records
}

fn check_bytes(model: &mut MobileModel, rest: &str) {
    let expected: u64 = rest.parse().expect("bytes value");
    assert_eq!(model.bytes(), expected, "model size mismatch");
}

fn check_logp(model: &mut MobileModel, rest: &str) {
    let mut parts = rest.split('\t');
    let prev2 = decode(parts.next().expect("arg"));
    let prev1 = decode(parts.next().expect("arg"));
    let target = decode(parts.next().expect("arg"));
    let expected = parse_bits(parts.next().expect("bits"));
    let got = model.logp(&prev2, &prev1, &target).expect("logp query");
    assert_eq!(
        got.to_bits(),
        expected,
        "logp mismatch for ({prev2:?}, {prev1:?}, {target:?}): got 0x{:016x} want 0x{expected:016x}",
        got.to_bits()
    );
}

fn check_observed(model: &mut MobileModel, rest: &str) {
    let mut parts = rest.split('\t');
    let prev = decode(parts.next().expect("arg"));
    let target = decode(parts.next().expect("arg"));
    let expected = parts.next().expect("flag") == "1";
    let got = model
        .has_observed_bigram(&prev, &target)
        .expect("observed query");
    assert_eq!(
        got, expected,
        "observed mismatch for ({prev:?}, {target:?})"
    );
}

fn check_status(model: &mut MobileModel, rest: &str, records: usize) {
    assert_eq!(
        model.cache_status().canonical(),
        rest,
        "cache status mismatch at record {records}"
    );
}

fn check_configure(model: &mut MobileModel, rest: &str) {
    let mut parts = rest.split('\t');
    let mut value = || -> usize { parts.next().expect("limit").parse().expect("limit value") };
    let limits = Limits {
        page_bytes: value(),
        context_entries: value(),
        bigram_entries: value(),
        index_pages: value(),
    };
    model.configure_cache(limits).expect("configure_cache");
}
