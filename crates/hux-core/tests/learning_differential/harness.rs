// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 重放状态：金样构建出的语料、索引、奖励链、差分用例与日志记录。

use hux_core::collections::Map;
use hux_core::learning::{DiffItem, Event, LearningIndex, RewardNode};
use hux_test_support::decode_hex;

pub struct Harness {
    pub corpora: Map<String, Vec<Event>>,
    pub indexes: Map<String, LearningIndex>,
    pub chains: Map<String, Vec<RewardNode>>,
    pub diffcases: Map<String, DiffItem>,
    pub journal_values: Vec<(String, String)>,
    pub journal_events: Vec<Event>,
    pub records: usize,
}

impl Harness {
    pub fn new() -> Self {
        Self {
            corpora: Map::new(),
            indexes: Map::new(),
            chains: Map::new(),
            diffcases: Map::new(),
            journal_values: Vec::new(),
            journal_events: Vec::new(),
            records: 0,
        }
    }

    pub fn event(&self, parts: &[&str]) -> Event {
        let time: f64 = parts[0].parse().expect("time");
        Event {
            time,
            mode: decode_hex(parts[1]),
            code: decode_hex(parts[2]),
            text: decode_hex(parts[3]),
            context: decode_hex(parts[4]),
        }
    }

    pub fn events_of(&self, name: &str) -> Vec<Event> {
        self.corpora
            .get(name)
            .unwrap_or_else(|| panic!("unknown corpus {name}"))
            .clone()
    }
}
