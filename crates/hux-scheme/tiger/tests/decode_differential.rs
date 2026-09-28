// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 差分测试：重放 decode 金样（冷路径、无学习），逐位比对。
//!
//! 金样由 `tools/generators/gen_decode_golden.lua` 生成：
//! * `goldens/decode.tsv.gz`：无模型；
//! * `goldens/decode_model.tsv.gz`：fixture 模型；
//! * `goldens/decode_rank_first.tsv.gz`：fixture 模型 + 关闭单字重码；
//! * `goldens/decode_evidence*.tsv.gz`：早提交证据（`--early-commit 1`）；
//! * `goldens/decode_learning_evidence.tsv.gz`：早提交证据 **+ 学习接入**
//!   （`--early-commit 1 --required 1 --learning 1`）——学习 × 证据抑制的交互
//!   （`learning_affected && truncated` 的拒绝分支、`share`/`base_share` 双权重）。

// 集成测试目标里 `mod x;` 相对 `tests/` 解析，故用显式路径指向本文件同名目录。
#[path = "decode_differential/transcript.rs"]
mod transcript;

use hux_scheme_tiger::decode::Decoder;
use hux_scheme_tiger::lexicon::{Lexicon, Supplement};
use hux_scheme_tiger::ngram::MobileModel;
use hux_test_support::{open_golden, repo_path};

/// decode 差分用解码器：`goldens/lexicon` 数据 + `data/` 词先验位图
/// （与金样生成时的参照数据目录一致）。方案专属夹具，留在本包内（不进 `hux-test-support`）。
fn make_decoder(model: Option<MobileModel>) -> Decoder {
    let data_dir = hux_test_support::repo_path("goldens/lexicon");
    let lexical_dir = hux_test_support::repo_path("data");
    let lexicon = Lexicon::load(&[data_dir.clone(), lexical_dir], 1500);
    let supplement = Supplement::load_default(Some(&data_dir));
    Decoder::new(lexicon, supplement, model)
}

#[test]
fn decode_transcript_is_bit_exact_without_model() {
    let records = transcript::replay(
        make_decoder(None),
        open_golden("goldens/decode.tsv.gz"),
        false,
    );
    assert!(records > 1_900, "transcript too short: {records}");
    println!("decode (no model): {records} golden records verified");
}

#[test]
fn decode_transcript_is_bit_exact_with_fixture_model() {
    let model = MobileModel::load(repo_path("goldens/ngram_fixture.bin"), None)
        .expect("load fixture model");
    let records = transcript::replay(
        make_decoder(Some(model)),
        open_golden("goldens/decode_model.tsv.gz"),
        false,
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
    let records = transcript::replay(
        decoder,
        open_golden("goldens/decode_rank_first.tsv.gz"),
        false,
    );
    assert!(records > 300, "transcript too short: {records}");
    println!("decode (model, no duplicate): {records} golden records verified");
}

#[test]
fn decode_evidence_transcript_is_bit_exact_without_model() {
    let records = transcript::replay(
        make_decoder(None),
        open_golden("goldens/decode_evidence.tsv.gz"),
        true,
    );
    assert!(records > 4_990, "transcript too short: {records}");
    println!("decode evidence (no model): {records} golden records verified");
}

#[test]
fn decode_evidence_transcript_is_bit_exact_with_fixture_model() {
    let model = MobileModel::load(repo_path("goldens/ngram_fixture.bin"), None)
        .expect("load fixture model");
    let records = transcript::replay(
        make_decoder(Some(model)),
        open_golden("goldens/decode_evidence_model.tsv.gz"),
        true,
    );
    assert!(records > 830, "transcript too short: {records}");
    println!("decode evidence (fixture model): {records} golden records verified");
}

#[test]
fn decode_learning_transcript_is_bit_exact_without_model() {
    let records = transcript::replay(
        make_decoder(None),
        open_golden("goldens/decode_learning.tsv.gz"),
        false,
    );
    assert!(records > 1_900, "transcript too short: {records}");
    println!("decode learning (no model): {records} golden records verified");
}

#[test]
fn decode_learning_transcript_is_bit_exact_with_fixture_model() {
    let model = MobileModel::load(repo_path("goldens/ngram_fixture.bin"), None)
        .expect("load fixture model");
    let records = transcript::replay(
        make_decoder(Some(model)),
        open_golden("goldens/decode_learning_model.tsv.gz"),
        false,
    );
    assert!(records > 300, "transcript too short: {records}");
    println!("decode learning (fixture model): {records} golden records verified");
}

/// 学习 × 证据抑制的交互金样（`--learning 1 --early-commit 1`）。
/// 生成器两侧开关本就可并用，缺的是**组合覆盖**——校准记录与 learning 索引
/// （`learningsetup` / `levent`）同批重放，`learning=1 && truncated=1` 的记录
/// 正是 `try_early_commit` 的拒绝分支与双权重交互的比对面。
#[test]
fn decode_learning_evidence_transcript_is_bit_exact_without_model() {
    let records = transcript::replay(
        make_decoder(None),
        open_golden("goldens/decode_learning_evidence.tsv.gz"),
        true,
    );
    assert!(records > 12_000, "transcript too short: {records}");
    println!("decode learning + evidence (no model): {records} golden records verified");
}
