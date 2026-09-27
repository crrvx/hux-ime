// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 差分测试：重放 lexicon 金样 transcript，逐条比对。
//!
//! 金样由 `tools/generators/gen_lexicon_golden.lua` 生成：
//! * present：`goldens/lexicon/` 数据 + `goldens/lexicon.tsv.gz`；
//! * missing：数据缺失路径 + `goldens/lexicon_missing.tsv.gz`。

// 集成测试目标里 `mod x;` 相对 `tests/` 解析，故用显式路径指向本文件同名目录。
#[path = "lexicon_differential/transcript.rs"]
mod transcript;

use hux_scheme_tiger::lexicon::{Lexicon, Supplement};
use hux_test_support::{open_golden, repo_path};

fn replay(data_relative: &str, golden_relative: &str) -> usize {
    let data_dir = repo_path(data_relative);
    let mut lexicon = Lexicon::load(std::slice::from_ref(&data_dir), 1500);
    let supplement = Supplement::load_default(Some(&data_dir));
    transcript::run(&mut lexicon, &supplement, open_golden(golden_relative))
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
