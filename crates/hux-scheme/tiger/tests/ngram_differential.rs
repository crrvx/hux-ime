// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 差分测试：重放 Lua 参照实现生成的金样 transcript，逐位比对。
//!
//! 金样由 `tools/generators/gen_ngram_golden.lua` 生成：
//! * fixture 模式入库（`goldens/ngram_fixture.*`）；
//! * sample 模式对真实模型抽样，仅本地（`goldens/local/`，不入库；缺失即跳过）。

use hux_scheme_tiger::ngram::{Limits, MobileModel};
use hux_test_support::{decode_hex as decode, open_golden, parse_bits, repo_path, try_open_golden};
use std::io::BufRead;
use std::path::PathBuf;

/// 重放一份 transcript，返回记录数；任何一条与本地实现不一致即 panic。
fn run_transcript(model: &mut MobileModel, reader: impl BufRead) -> usize {
    let mut records = 0usize;
    for line in reader.lines() {
        let line = line.expect("read golden line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, rest) = line.split_once('\t').unwrap_or((&line, ""));
        match kind {
            "bytes" => {
                let expected: u64 = rest.parse().expect("bytes value");
                assert_eq!(model.bytes(), expected, "model size mismatch");
            }
            "logp" => {
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
            "obs" => {
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
            "status" => {
                assert_eq!(
                    model.cache_status().canonical(),
                    rest,
                    "cache status mismatch at record {records}"
                );
            }
            "cfg" => {
                let mut parts = rest.split('\t');
                let mut value =
                    || -> usize { parts.next().expect("limit").parse().expect("limit value") };
                let limits = Limits {
                    page_bytes: value(),
                    context_entries: value(),
                    bigram_entries: value(),
                    index_pages: value(),
                };
                model.configure_cache(limits).expect("configure_cache");
            }
            "trim" => model.trim_caches(),
            "close" => model.close(),
            other => panic!("unknown record kind: {other}"),
        }
        records += 1;
    }
    records
}

#[test]
fn ngram_fixture_transcript_is_bit_exact() {
    let path = repo_path("goldens/ngram_fixture.bin");
    let mut model = MobileModel::load(&path, None).expect("load fixture model");
    let reader = open_golden("goldens/ngram_fixture.tsv.gz");
    let records = run_transcript(&mut model, reader);
    assert!(records > 29_000, "transcript too short: {records} records");
    println!("fixture: {records} golden records verified bit-exact");
}

#[test]
fn ngram_sample_transcript_is_bit_exact_when_present() {
    let Some(reader) = try_open_golden("goldens/local/ngram_sample.tsv.gz") else {
        eprintln!("skip: goldens/local/ngram_sample.tsv.gz not present (local-only sample)");
        return;
    };
    let Some(model_path) = std::env::var_os("HUX_NGRAM_MODEL")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                PathBuf::from(home)
                    .join(".local/share/fcitx5/rime/models/sentence-ngram-mobile.bin")
            })
        })
    else {
        eprintln!("skip: no sample model path (set HUX_NGRAM_MODEL)");
        return;
    };
    if !model_path.is_file() {
        eprintln!("skip: sample model not found at {}", model_path.display());
        return;
    }
    let mut model = MobileModel::load(&model_path, None).expect("load sample model");
    let records = run_transcript(&mut model, reader);
    assert!(records > 60_000, "transcript too short: {records} records");
    println!("sample: {records} golden records verified bit-exact");
}
