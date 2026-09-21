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
//!   （`--early-commit 1 --required 1 --learning 1`）——遗留②：学习 × 证据抑制的交互
//!   （`learning_affected && truncated` 的拒绝分支、`share`/`base_share` 双权重）。

use hux_core::learning::{Event, LearningIndex};
use hux_scheme_tiger::decode::{DecodeLock, Decoder, has_complete_candidate};
use hux_scheme_tiger::lexicon::{Lexicon, Supplement};
use hux_scheme_tiger::ngram::MobileModel;
use hux_test_support::{decode_hex, field, open_golden, parse_bits, repo_path};
use std::io::BufRead;

/// decode 差分用解码器：`goldens/lexicon` 数据 + `data/` 词先验位图
/// （与金样生成时的参照数据目录一致）。方案专属夹具，留在本包内（不进 `hux-test-support`）。
fn make_decoder(model: Option<MobileModel>) -> Decoder {
    let data_dir = hux_test_support::repo_path("goldens/lexicon");
    let lexical_dir = hux_test_support::repo_path("data");
    let lexicon = Lexicon::load(&[data_dir.clone(), lexical_dir], 1500);
    let supplement = Supplement::load_default(Some(&data_dir));
    Decoder::new(lexicon, supplement, model)
}

fn replay(mut decoder: Decoder, reader: impl BufRead, early: bool) -> usize {
    let mut lines = reader
        .lines()
        .map(|line| line.expect("read golden line"))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let mut records = 0usize;
    while let Some(line) = lines.next() {
        let (kind, rest) = line.split_once('\t').expect("record payload");
        if kind == "learningsetup" {
            let fields: Vec<&str> = rest.split('\t').collect();
            let now: f64 = fields[0].parse().expect("learning now");
            let mode = decode_hex(fields[1]);
            let count: usize = fields[2].parse().expect("levent count");
            let mut events = Vec::with_capacity(count);
            for _ in 0..count {
                let line = lines.next().expect("levent record");
                let (kind, rest) = line.split_once('\t').expect("levent payload");
                assert_eq!(kind, "levent");
                let parts: Vec<&str> = rest.split('\t').collect();
                events.push(Event {
                    time: parts[0].parse().expect("time"),
                    mode: decode_hex(parts[1]),
                    code: decode_hex(parts[2]),
                    text: decode_hex(parts[3]),
                    context: decode_hex(parts[4]),
                });
            }
            decoder.set_learning(LearningIndex::build(&events, now), &mode);
            records += 1;
            continue;
        }
        if kind == "complete" {
            // `has_complete_candidate` 用例（证据金样专用；生成器固定 duplicate=1）。
            let mut parts = rest.split('\t');
            let input = decode_hex(field(parts.next().expect("input"), "input="));
            let required = decode_hex(field(parts.next().expect("required"), "required="));
            let excluded = decode_hex(field(parts.next().expect("excluded"), "excluded="));
            let group: u8 = field(parts.next().expect("group"), "group=")
                .parse()
                .expect("group value");
            let lock_field = field(parts.next().expect("lock"), "lock=");
            let expected: u8 = field(parts.next().expect("result"), "result=")
                .parse()
                .expect("result value");
            let lock_pair = if lock_field == "-" {
                None
            } else {
                let (raw, text) = lock_field.split_once(',').expect("lock pair");
                Some((decode_hex(raw), decode_hex(text)))
            };
            let lock = lock_pair.as_ref().map(|(raw, text)| DecodeLock {
                raw,
                text,
                boundaries: "",
            });
            let value = has_complete_candidate(
                decoder.lexicon(),
                &input,
                &required,
                if excluded.is_empty() {
                    None
                } else {
                    Some(excluded.as_str())
                },
                group != 0,
                true,
                lock.as_ref(),
            );
            assert_eq!(value as u8, expected, "complete mismatch for {input:?}");
            records += 1;
            continue;
        }
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
        let required = decode_hex(field(parts.next().expect("required"), "required="));

        let output = if early {
            decoder
                .decode_with(&input, true, &required)
                .expect("decode with evidence")
        } else {
            decoder.decode(&input).expect("decode")
        };
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
            let early_confidence = parse_bits(fields.next().expect("early confidence"));

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
            assert_eq!(
                expected.early_commit_confidence_score.to_bits(),
                early_confidence,
                "early confidence mismatch for {context}"
            );
            records += 1;
        }

        if early {
            let line = lines.next().expect("evidence record");
            let (kind, rest) = line.split_once('\t').expect("evidence payload");
            assert_eq!(kind, "evidence", "expected evidence record, got {kind}");
            let mut fields = rest.split('\t');
            let proposal = decode_hex(fields.next().expect("proposal"));
            let share = parse_bits(fields.next().expect("share"));
            let mut flags = (false, false, false, false);
            let mut prefix_count = 0usize;
            let mut raw_count = 0usize;
            for part in fields {
                if let Some((name, value)) = part.split_once('=') {
                    match name {
                        "nit" => flags.0 = value == "1",
                        "mit" => flags.1 = value == "1",
                        "nlc" => flags.2 = value == "1",
                        "trunc" => flags.3 = value == "1",
                        "prefixes" => prefix_count = value.parse().expect("prefix count"),
                        "raws" => raw_count = value.parse().expect("raw count"),
                        other => panic!("unknown evidence field: {other}"),
                    }
                }
            }
            let evidence = &output.evidence;
            records += 1;
            assert_eq!(
                evidence.proposal, proposal,
                "proposal mismatch for {input:?}"
            );
            assert_eq!(
                evidence.proposal_share.to_bits(),
                share,
                "proposal share mismatch for {input:?}"
            );
            assert_eq!(evidence.neutral_incomplete_tail, flags.0);
            assert_eq!(evidence.merged_incomplete_tail, flags.1);
            assert_eq!(evidence.neutral_low_confidence, flags.2);
            assert_eq!(evidence.confidence_truncated, flags.3);
            assert_eq!(
                evidence.prefixes.len(),
                prefix_count,
                "prefix count for {input:?}"
            );

            for (position, expected) in evidence.prefixes.iter().enumerate() {
                let line = lines.next().expect("prefix record");
                let (kind, rest) = line.split_once('\t').expect("prefix payload");
                assert_eq!(kind, "prefix", "expected prefix record, got {kind}");
                let mut fields = rest.split('\t');
                let text = decode_hex(fields.next().expect("text"));
                let raw_length: usize = fields.next().expect("raw_length").parse().expect("len");
                let prefix_share = parse_bits(fields.next().expect("share"));
                let base_share = parse_bits(fields.next().expect("base share"));
                let boundary_share = parse_bits(fields.next().expect("boundary share"));
                let closed: u8 = fields.next().expect("closed").parse().expect("closed");
                let chars: usize = fields.next().expect("chars").parse().expect("chars");
                let context = format!("{input:?} prefix #{position}");
                assert_eq!(expected.text, text, "prefix text mismatch for {context}");
                assert_eq!(
                    expected.raw_length, raw_length,
                    "prefix raw mismatch for {context}"
                );
                assert_eq!(
                    expected.share.to_bits(),
                    prefix_share,
                    "prefix share for {context}"
                );
                assert_eq!(
                    expected.base_share.to_bits(),
                    base_share,
                    "prefix base share for {context}"
                );
                assert_eq!(
                    expected.boundary_share.to_bits(),
                    boundary_share,
                    "boundary share for {context}"
                );
                assert_eq!(
                    expected.boundary_closed as u8, closed,
                    "closed for {context}"
                );
                assert_eq!(expected.text_char_count, chars, "chars for {context}");
                records += 1;
            }

            let mut keys: Vec<&String> = evidence.raw_lengths.keys().collect();
            keys.sort();
            assert_eq!(keys.len(), raw_count, "raw count mismatch for {input:?}");
            for key in keys {
                let line = lines.next().expect("rawlen record");
                let (kind, rest) = line.split_once('\t').expect("rawlen payload");
                assert_eq!(kind, "rawlen", "expected rawlen record, got {kind}");
                let mut fields = rest.split('\t');
                let text = decode_hex(fields.next().expect("text"));
                let raw_length: usize = fields.next().expect("raw_length").parse().expect("len");
                assert_eq!(&text, key, "rawlen text mismatch for {input:?}");
                assert_eq!(
                    raw_length, evidence.raw_lengths[key],
                    "rawlen value mismatch for {input:?}"
                );
                records += 1;
            }
        }
        records += 1;
    }
    records
}

#[test]
fn decode_transcript_is_bit_exact_without_model() {
    let records = replay(
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
    let records = replay(
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
    let records = replay(
        decoder,
        open_golden("goldens/decode_rank_first.tsv.gz"),
        false,
    );
    assert!(records > 300, "transcript too short: {records}");
    println!("decode (model, no duplicate): {records} golden records verified");
}

#[test]
fn decode_evidence_transcript_is_bit_exact_without_model() {
    let records = replay(
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
    let records = replay(
        make_decoder(Some(model)),
        open_golden("goldens/decode_evidence_model.tsv.gz"),
        true,
    );
    assert!(records > 830, "transcript too short: {records}");
    println!("decode evidence (fixture model): {records} golden records verified");
}

#[test]
fn decode_learning_transcript_is_bit_exact_without_model() {
    let records = replay(
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
    let records = replay(
        make_decoder(Some(model)),
        open_golden("goldens/decode_learning_model.tsv.gz"),
        false,
    );
    assert!(records > 300, "transcript too short: {records}");
    println!("decode learning (fixture model): {records} golden records verified");
}

/// 遗留②：`--learning 1 --early-commit 1` 的组合金样（学习 × 证据抑制的交互）。
/// 生成器两侧开关本就可并用，缺的是**组合覆盖**——校准记录与 learning 索引
/// （`learningsetup` / `levent`）同批重放，`learning=1 && truncated=1` 的记录
/// 正是 `try_early_commit` 的拒绝分支与双权重交互的比对面。
#[test]
fn decode_learning_evidence_transcript_is_bit_exact_without_model() {
    let records = replay(
        make_decoder(None),
        open_golden("goldens/decode_learning_evidence.tsv.gz"),
        true,
    );
    assert!(records > 12_000, "transcript too short: {records}");
    println!("decode learning + evidence (no model): {records} golden records verified");
}
