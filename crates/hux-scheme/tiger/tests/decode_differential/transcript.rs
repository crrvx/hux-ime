// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! decode transcript 的逐条重放：`learningsetup` / `complete` / `decode`（含结果与证据记录）
//! 按记录种类分派到各自的比对。

use hux_core::learning::{Event, LearningIndex};
use hux_scheme_tiger::decode::{
    DecodeLock, DecodeOutput, Decoder, Evaluated, Evidence, has_complete_candidate,
};
use hux_test_support::{decode_hex, field, parse_bits};
use std::io::BufRead;

/// 重放一份 transcript，返回记录数；任何一条与本地实现不一致即 panic。
pub fn replay(mut decoder: Decoder, reader: impl BufRead, early: bool) -> usize {
    let mut lines = reader
        .lines()
        .map(|line| line.expect("read golden line"))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let mut records = 0usize;
    while let Some(line) = lines.next() {
        let (kind, rest) = line.split_once('\t').expect("record payload");
        if kind == "learningsetup" {
            records += learningsetup(&mut decoder, rest, &mut lines);
            continue;
        }
        if kind == "complete" {
            records += complete(&decoder, rest);
            continue;
        }
        assert_eq!(kind, "decode", "expected decode record, got {kind}");
        records += decode(&mut decoder, rest, early, &mut lines);
    }
    records
}

/// 学习接入：`learningsetup` 头后紧跟 `count` 条 `levent`。
fn learningsetup(
    decoder: &mut Decoder,
    rest: &str,
    lines: &mut impl Iterator<Item = String>,
) -> usize {
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
    1
}

/// `has_complete_candidate` 用例（证据金样专用；生成器固定 duplicate=1）。
fn complete(decoder: &Decoder, rest: &str) -> usize {
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
    1
}

/// 一条 `decode`：比对结果条数与两个标志、逐条比对 `result`，
/// 证据金样再比对 `evidence` / `prefix` / `rawlen`。返回消耗的记录数。
fn decode(
    decoder: &mut Decoder,
    rest: &str,
    early: bool,
    lines: &mut impl Iterator<Item = String>,
) -> usize {
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
    let mut records = 0usize;
    for (position, expected) in output.items.iter().enumerate() {
        let line = lines.next().expect("result record");
        let (kind, rest) = line.split_once('\t').expect("result payload");
        assert_eq!(kind, "result", "expected result record, got {kind}");
        records += check_result(expected, rest, &input, position);
    }

    if early {
        records += check_evidence(&output, &input, lines);
    }
    records + 1
}

/// 比对一条 `result` 记录；返回 1。
fn check_result(expected: &Evaluated, rest: &str, input: &str, position: usize) -> usize {
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
    1
}

/// 比对 `evidence` 记录、逐条 `prefix` 与其 `rawlen`（仅证据金样）；返回消耗的记录数。
fn check_evidence(
    output: &DecodeOutput,
    input: &str,
    lines: &mut impl Iterator<Item = String>,
) -> usize {
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

    1 + check_prefixes(evidence, input, lines)
        + check_raw_lengths(evidence, input, raw_count, lines)
}

/// 比对逐条 `prefix` 记录；返回消耗的记录数。
fn check_prefixes(
    evidence: &Evidence,
    input: &str,
    lines: &mut impl Iterator<Item = String>,
) -> usize {
    let mut records = 0usize;
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

    records
}

/// 比对 `raw_lengths` 记录；返回消耗的记录数。
fn check_raw_lengths(
    evidence: &Evidence,
    input: &str,
    raw_count: usize,
    lines: &mut impl Iterator<Item = String>,
) -> usize {
    let mut records = 0usize;
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
    records
}
