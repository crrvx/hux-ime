// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 差分测试：重放 learning 金样（纯计算部分），逐位比对。
//!
//! 金样由 `tools/generators/gen_learning_golden.lua` 生成（`goldens/learning.tsv.gz`）。

use hashbrown::HashMap;
use hux_core::learning::{self, DiffItem, DiffPathNode, Event, LearningIndex, RewardNode};
use hux_test_support::{decode_bytes, decode_hex, open_golden, parse_bits};
use std::io::BufRead;

struct Harness {
    corpora: HashMap<String, Vec<Event>>,
    indexes: HashMap<String, LearningIndex>,
    chains: HashMap<String, Vec<RewardNode>>,
    diffcases: HashMap<String, DiffItem>,
    journal_values: Vec<(String, String)>,
    journal_events: Vec<Event>,
    records: usize,
}

impl Harness {
    fn new() -> Self {
        Self {
            corpora: HashMap::new(),
            indexes: HashMap::new(),
            chains: HashMap::new(),
            diffcases: HashMap::new(),
            journal_values: Vec::new(),
            journal_events: Vec::new(),
            records: 0,
        }
    }

    fn event(&self, parts: &[&str]) -> Event {
        let time: f64 = parts[0].parse().expect("time");
        Event {
            time,
            mode: decode_hex(parts[1]),
            code: decode_hex(parts[2]),
            text: decode_hex(parts[3]),
            context: decode_hex(parts[4]),
        }
    }

    fn events_of(&self, name: &str) -> Vec<Event> {
        self.corpora
            .get(name)
            .unwrap_or_else(|| panic!("unknown corpus {name}"))
            .clone()
    }
}

fn run(mut harness: Harness, reader: impl BufRead) -> usize {
    let mut lines = reader
        .lines()
        .map(|line| line.expect("read golden line"))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    while let Some(line) = lines.next() {
        let (kind, rest) = line.split_once('\t').unwrap_or((&line, ""));
        match kind {
            "hash" => {
                let fields: Vec<&str> = rest.split('\t').collect();
                let text = decode_hex(fields[0]);
                assert_eq!(
                    learning::hash(&text),
                    fields[1],
                    "hash mismatch for {text:?}"
                );
            }
            "corpus" => {
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
            "index" => {
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
            "confirmed" => {
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
            "codes" => {
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
            "score" => {
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
            "prefix" => {
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
            "trim" => {
                let name = rest;
                harness
                    .indexes
                    .get_mut(name)
                    .unwrap_or_else(|| panic!("unknown index {name}"))
                    .trim_caches();
            }
            "chain" => {
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
                    });
                }
                harness.chains.insert(name, nodes);
            }
            "reward" => {
                let fields: Vec<&str> = rest.split('\t').collect();
                let index_name = fields[0];
                let chain_name = fields[1];
                let mode = decode_hex(fields[2]);
                let raw = decode_bytes(fields[3]);
                let text = decode_hex(fields[4]);
                let finish: usize = fields[5].parse().expect("finish");
                let best = parse_bits(fields[6]);
                let potential = parse_bits(fields[7]);
                let chain = harness.chains.get(chain_name).expect("chain").clone();
                let index = harness
                    .indexes
                    .get_mut(index_name)
                    .unwrap_or_else(|| panic!("unknown index {index_name}"));
                let (got_best, got_potential) =
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
            }
            "diffcase" => {
                let fields: Vec<&str> = rest.split('\t').collect();
                let name = fields[0].to_string();
                let text = decode_hex(fields[1]);
                let count: usize = fields[2].parse().expect("diffpath count");
                let mut path = Vec::with_capacity(count);
                for _ in 0..count {
                    let line = lines.next().expect("diffpath record");
                    let (kind, rest) = line.split_once('\t').expect("diffpath payload");
                    assert_eq!(kind, "diffpath");
                    let parts: Vec<&str> = rest.split('\t').collect();
                    path.push(DiffPathNode {
                        raw_length: parts[0].parse().expect("raw_length"),
                        text_length: parts[1].parse().expect("text_length"),
                    });
                }
                harness.diffcases.insert(name, DiffItem { text, path });
            }
            "diff" => {
                let fields: Vec<&str> = rest.split('\t').collect();
                let raw = decode_bytes(fields[0]);
                let floor: usize = fields[1].parse().expect("floor");
                let mode = decode_hex(fields[2]);
                let before = harness.diffcases.get(fields[3]).cloned();
                let selected = harness.diffcases.get(fields[4]).cloned();
                let count: usize = fields[5].parse().expect("diff count");
                let events =
                    learning::diff(&raw, before.as_ref(), selected.as_ref(), floor, &mode, 0.0);
                assert_eq!(
                    events.len(),
                    count,
                    "diff count for {}->{}",
                    fields[3],
                    fields[4]
                );
                for (position, expected) in events.iter().enumerate() {
                    let line = lines.next().expect("diffevent record");
                    let (kind, rest) = line.split_once('\t').expect("diffevent payload");
                    assert_eq!(kind, "diffevent");
                    let parts: Vec<&str> = rest.split('\t').collect();
                    let context = format!("{}->{} #{position}", fields[3], fields[4]);
                    assert_eq!(expected.text, decode_hex(parts[0]), "diff text {context}");
                    assert_eq!(expected.code, decode_hex(parts[1]), "diff code {context}");
                    assert_eq!(expected.context, decode_hex(parts[2]), "diff ctx {context}");
                    assert_eq!(
                        expected.raw_start,
                        parts[3].parse().unwrap(),
                        "raw_start {context}"
                    );
                    assert_eq!(
                        expected.raw_end,
                        parts[4].parse().unwrap(),
                        "raw_end {context}"
                    );
                    assert_eq!(
                        expected.text_start,
                        parts[5].parse().unwrap(),
                        "text_start {context}"
                    );
                    assert_eq!(
                        expected.text_end,
                        parts[6].parse().unwrap(),
                        "text_end {context}"
                    );
                }
            }
            "journalrecords" => {
                let count: usize = rest.parse().expect("journal count");
                harness.journal_values.clear();
                for _ in 0..count {
                    let line = lines.next().expect("journalrecord");
                    let (kind, rest) = line.split_once('\t').expect("journal payload");
                    assert_eq!(kind, "journalrecord");
                    let fields: Vec<&str> = rest.split('\t').collect();
                    harness
                        .journal_values
                        .push((decode_hex(fields[0]), decode_hex(fields[1])));
                }
            }
            "journalevents" => {
                let count: usize = rest.parse().expect("journalevent count");
                harness.journal_events.clear();
                for _ in 0..count {
                    let line = lines.next().expect("journalevent");
                    let (kind, rest) = line.split_once('\t').expect("journalevent payload");
                    assert_eq!(kind, "journalevent");
                    let parts: Vec<&str> = rest.split('\t').collect();
                    harness.journal_events.push(harness.event(&parts));
                }
            }
            other => panic!("unknown record kind: {other}"),
        }
        harness.records += 1;
    }

    // 日志编码：键为 e/%010d，值为 frame(time, mode, code, text, context)。
    for (position, (key, value)) in harness.journal_values.iter().enumerate() {
        let expected_key = format!("e/{:010}", position + 1);
        assert_eq!(*key, expected_key, "journal key");
        let parts = learning::unframe(value).expect("journal value decodes");
        assert_eq!(parts.len(), 5, "journal value parts");
        assert_eq!(learning::frame(&parts), *value, "frame round trip");
        let got_time: f64 = parts[0].parse().expect("time part");
        let event = &harness.journal_events[position];
        assert_eq!(got_time, event.time, "journal time");
        assert_eq!(parts[1], event.mode, "journal mode");
        assert_eq!(parts[2], event.code, "journal code");
        assert_eq!(parts[3], event.text, "journal text");
        assert_eq!(parts[4], event.context, "journal context");
    }
    harness.records
}

#[test]
fn learning_transcript_is_bit_exact() {
    let harness = Harness::new();
    let records = run(harness, open_golden("goldens/learning.tsv.gz"));
    assert!(records > 10_000, "transcript too short: {records} records");
    println!("learning: {records} golden records verified");
}
