// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 索引类记录：语料、索引构建与索引上的评分/奖励查询。

use hux_core::learning::{self, LearningIndex, RewardNode};
use hux_test_support::{decode_bytes, decode_hex, parse_bits};

use crate::harness::Harness;

/// `corpus`：具名事件序列（后续 `event` 子行）。
pub fn load_corpus(harness: &mut Harness, rest: &str, lines: &mut impl Iterator<Item = String>) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let name = fields[0].to_string();
    let count: usize = fields[1].parse().expect("corpus count");
    let mut events = Vec::with_capacity(count);
    for _ in 0..count {
        let line = lines.next().expect("event record");
        let (kind, rest) = line.split_once('\t').expect("event payload");
        assert_eq!(kind, "event");
        let parts: Vec<&str> = rest.split('\t').collect();
        events.push(harness.event(&parts));
    }
    harness.corpora.insert(name, events);
}

/// `index`：按 `full`/`runtime` 构建具名索引。
pub fn build_index(harness: &mut Harness, rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let name = fields[0].to_string();
    let kind = fields[1];
    let now: f64 = fields[2].parse().expect("now");
    let base = fields[3];
    let events = harness.events_of(base);
    let index = if kind == "full" {
        LearningIndex::build(&events, now)
    } else {
        LearningIndex::runtime(&events, now)
    };
    harness.indexes.insert(name, index);
}

/// `confirmed`：两次 `update`（先基础、后追加确认）构建索引。
pub fn build_confirmed(harness: &mut Harness, rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let name = fields[0].to_string();
    let base = harness.events_of(fields[1]);
    let accepted = harness.events_of(fields[2]);
    let first_now: f64 = fields[3].parse().expect("first now");
    let second_now: f64 = fields[4].parse().expect("second now");
    let mut all = base.clone();
    all.extend(accepted.iter().cloned());
    let index = LearningIndex::runtime(&[], first_now)
        .update(&base, &base, first_now)
        .update(&accepted, &all, second_now);
    harness.indexes.insert(name, index);
}

/// `codes`：索引码表逐项比对。
pub fn check_codes(harness: &Harness, rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let name = fields[0];
    let count: usize = fields[1].parse().expect("codes count");
    let index = harness
        .indexes
        .get(name)
        .unwrap_or_else(|| panic!("unknown index {name}"));
    assert_eq!(index.codes.len(), count, "codes length for {name}");
    for (position, code) in index.codes.iter().enumerate() {
        assert_eq!(
            *code,
            decode_hex(fields[2 + position]),
            "codes[{position}] for {name}"
        );
    }
}

/// `score`：评分逐位比对。
pub fn check_score(harness: &mut Harness, rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let name = fields[0];
    let mode = decode_hex(fields[1]);
    let code = decode_hex(fields[2]);
    let text = decode_hex(fields[3]);
    let ctx = decode_hex(fields[4]);
    let expected = parse_bits(fields[5]);
    let index = harness
        .indexes
        .get_mut(name)
        .unwrap_or_else(|| panic!("unknown index {name}"));
    let got = index.score(&mode, &code, &text, &ctx);
    assert_eq!(
        got.to_bits(),
        expected,
        "score mismatch for {name} ({mode:?},{code:?},{text:?},{ctx:?})"
    );
}

/// `prefix`：前缀评分逐位比对。
pub fn check_prefix(harness: &mut Harness, rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let name = fields[0];
    let mode = decode_hex(fields[1]);
    let code = decode_hex(fields[2]);
    let text = decode_hex(fields[3]);
    let ctx = decode_hex(fields[4]);
    let expected = parse_bits(fields[5]);
    let index = harness
        .indexes
        .get_mut(name)
        .unwrap_or_else(|| panic!("unknown index {name}"));
    let got = index.prefix_score(&mode, &code, &text, &ctx);
    assert_eq!(
        got.to_bits(),
        expected,
        "prefix mismatch for {name} ({mode:?},{code:?},{text:?},{ctx:?})"
    );
}

/// `trim`：裁剪缓存。
pub fn trim_caches(harness: &mut Harness, rest: &str) {
    let name = rest;
    harness
        .indexes
        .get_mut(name)
        .unwrap_or_else(|| panic!("unknown index {name}"))
        .trim_caches();
}

/// `chain`：奖励链（后续 `node` 子行）。
pub fn load_chain(harness: &mut Harness, rest: &str, lines: &mut impl Iterator<Item = String>) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let name = fields[0].to_string();
    let count: usize = fields[1].parse().expect("chain count");
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let line = lines.next().expect("node record");
        let (kind, rest) = line.split_once('\t').expect("node payload");
        assert_eq!(kind, "node");
        let parts: Vec<&str> = rest.split('\t').collect();
        nodes.push(RewardNode {
            text_length: parts[0].parse().expect("text_length"),
            raw_length: parts[1].parse().expect("raw_length"),
            learning_score: f64::from_bits(parse_bits(parts[2])),
            learning_early_commit_bonus: f64::from_bits(parse_bits(parts[3])),
        });
    }
    harness.chains.insert(name, nodes);
}

/// `reward`：奖励三元组逐位比对。
pub fn check_reward(harness: &mut Harness, rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let index_name = fields[0];
    let chain_name = fields[1];
    let mode = decode_hex(fields[2]);
    let raw = decode_bytes(fields[3]);
    let text = decode_hex(fields[4]);
    let finish: usize = fields[5].parse().expect("finish");
    let best = parse_bits(fields[6]);
    let potential = parse_bits(fields[7]);
    let early_bonus = parse_bits(fields[8]);
    let chain = harness.chains.get(chain_name).expect("chain").clone();
    let index = harness
        .indexes
        .get_mut(index_name)
        .unwrap_or_else(|| panic!("unknown index {index_name}"));
    let (got_best, got_potential, got_early_bonus) =
        learning::reward(index, &mode, &raw, &text, finish, &chain);
    assert_eq!(
        got_best.to_bits(),
        best,
        "reward best mismatch for {index_name}/{chain_name}"
    );
    assert_eq!(
        got_potential.to_bits(),
        potential,
        "reward potential mismatch for {index_name}/{chain_name}"
    );
    assert_eq!(
        got_early_bonus.to_bits(),
        early_bonus,
        "reward early bonus mismatch for {index_name}/{chain_name}"
    );
}

/// `fusion`：索引融合评分逐位比对。
pub fn check_fusion(harness: &mut Harness, rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let name = fields[0];
    let mode = decode_hex(fields[1]);
    let raw = decode_bytes(fields[2]);
    let direct = decode_hex(fields[3]);
    let composed = decode_hex(fields[4]);
    let expected = parse_bits(fields[5]);
    let index = harness
        .indexes
        .get_mut(name)
        .unwrap_or_else(|| panic!("unknown index {name}"));
    let got = index.fusion_score(&mode, &raw, &direct, &composed);
    assert_eq!(
        got.to_bits(),
        expected,
        "fusion_score mismatch for {name} ({mode:?},{raw:?},{direct:?},{composed:?})"
    );
}
