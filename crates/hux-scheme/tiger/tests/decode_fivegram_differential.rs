// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 差分测试：重放五阶（TCSKNM03）decode 金样，逐位比对。
//!
//! 金样由 `tools/generators/gen_decode_golden.lua` 生成
//! （`--model goldens/fivegram_fixture.bin --lexical data/tiger_sentence.lexical.bin
//! --every 7 --lock 1`），字段口径与 `decode_differential.rs` 一致，另多一类 `locked`
//! 记录（作用于紧随其后的 `decode`：该遍走 `decode_with_lock`）。
//!
//! 覆盖面（相对无模型 / 三阶金样的增量）：
//! * 模型按文件头 magic 派发到五阶读取器（[`SentenceModel::load`]）；
//! * 根状态取 `LmHistory::begin(bos_id)`，逐字 `step` 的槽位平移与 `count` 增长；
//! * `evaluate_state` 的 EOS 打分（金样的 `confidence_score` 直接含它）；
//! * 锁定重放：整段锁（只发种子）与前缀锁（种子之后继续扩展，覆盖槽位从种子传到扩展）。

use hux_scheme_tiger::decode::{DecodeLock, Decoder, SentenceModel};
use hux_scheme_tiger::lexicon::{Lexicon, Supplement};
use hux_test_support::{decode_hex, field, open_golden, parse_bits, repo_path};
use std::io::BufRead;

/// 五阶夹具模型：经 [`SentenceModel::load`] 装载，顺带校验 magic 派发落到五阶变体。
fn fixture_model() -> SentenceModel {
    let model =
        SentenceModel::load(repo_path("goldens/fivegram_fixture.bin")).expect("装载五阶夹具");
    assert!(
        matches!(model, SentenceModel::Fivegram(_)),
        "magic 派发必须落到五阶变体"
    );
    model
}

/// decode 差分用解码器：`goldens/lexicon` 数据 + `data/` 词先验位图
/// （与金样生成时的参照数据目录一致）。
fn make_decoder() -> Decoder {
    let data_dir = repo_path("goldens/lexicon");
    let lexical_dir = repo_path("data");
    let lexicon = Lexicon::load(&[data_dir.clone(), lexical_dir], 1500);
    let supplement = Supplement::load_default(Some(&data_dir));
    Decoder::with_model(lexicon, supplement, Some(fixture_model()))
}

/// 重放 transcript，返回比对的记录数。
fn replay(mut decoder: Decoder, reader: impl BufRead) -> usize {
    let mut lines = reader
        .lines()
        .map(|line| line.expect("read golden line"))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let mut records = 0usize;
    // 待用锁：`locked` 记录先于它作用的 `decode` 记录出现。
    let mut pending_lock: Option<(String, String, String)> = None;
    while let Some(line) = lines.next() {
        let (kind, rest) = line.split_once('\t').expect("record payload");
        if kind == "locked" {
            let mut parts = rest.split('\t');
            pending_lock = Some((
                decode_hex(parts.next().expect("lock raw")),
                decode_hex(parts.next().expect("lock text")),
                parts.next().expect("lock boundaries").to_string(),
            ));
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
        assert!(
            required.is_empty(),
            "五阶金样不带必需前缀（输入 {input:?}）"
        );

        let lock = pending_lock.take();
        let output = match &lock {
            Some((raw, text, boundaries)) => decoder
                .decode_with_lock(
                    &input,
                    false,
                    "",
                    Some(DecodeLock {
                        raw,
                        text,
                        boundaries,
                    }),
                )
                .expect("decode with lock"),
            None => decoder.decode(&input).expect("decode"),
        };
        let context = match &lock {
            Some((raw, text, boundaries)) => {
                format!("{input:?} locked({raw:?},{text:?},{boundaries})")
            }
            None => format!("{input:?}"),
        };
        assert_eq!(output.items.len(), count, "count mismatch for {context}");
        assert_eq!(
            output.learning_affected as u8, learning,
            "learning flag mismatch for {context}"
        );
        assert_eq!(
            output.completed_truncated as u8, truncated,
            "truncated flag mismatch for {context}"
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

            let context = format!("{context} #{position}");
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
        records += 1;
    }
    assert!(
        pending_lock.is_none(),
        "transcript 末尾有未配对的 locked 记录"
    );
    records
}

#[test]
fn decode_fivegram_transcript_is_bit_exact() {
    let records = replay(
        make_decoder(),
        open_golden("goldens/decode_fivegram.tsv.gz"),
    );
    assert!(records > 700, "transcript too short: {records}");
    println!("decode (fivegram fixture, locked passes): {records} golden records verified");
}
