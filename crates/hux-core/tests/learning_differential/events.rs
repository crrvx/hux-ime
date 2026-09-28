// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 事件类记录：融合事件、差分事件与日志记录。

use hux_core::learning::{self, DiffItem, DiffPathNode};
use hux_test_support::{decode_bytes, decode_hex};

use crate::harness::Harness;

/// `fusionnone`：空模式必须不产出融合事件。
pub fn check_fusion_empty(rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let mode = decode_hex(fields[0]);
    let raw = decode_bytes(fields[1]);
    let direct = decode_hex(fields[2]);
    let composed = decode_hex(fields[3]);
    let direct_wins = fields[4] == "1";
    let raw_end: usize = fields[5].parse().expect("raw_end");
    assert!(
        learning::fusion_event(&mode, &raw, &direct, &composed, direct_wins, raw_end, 0.0)
            .is_none(),
        "空模式应不产出融合事件"
    );
}

/// `fusionevent`：融合事件字段逐项比对。
pub fn check_fusion_event(rest: &str) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let mode = decode_hex(fields[0]);
    let raw = decode_bytes(fields[1]);
    let direct = decode_hex(fields[2]);
    let composed = decode_hex(fields[3]);
    let direct_wins = fields[4] == "1";
    let raw_end: usize = fields[5].parse().expect("raw_end");
    let time: f64 = fields[6].parse().expect("time");
    let event = learning::fusion_event(&mode, &raw, &direct, &composed, direct_wins, raw_end, time)
        .unwrap_or_else(|| panic!("fusion_event 不应为空 ({mode:?})"));
    let context = format!("{mode:?}/{direct:?}/{composed:?}");
    assert_eq!(event.mode, decode_hex(fields[7]), "mode {context}");
    assert_eq!(event.code, decode_hex(fields[8]), "code {context}");
    assert_eq!(event.text, decode_hex(fields[9]), "text {context}");
    assert_eq!(event.context, decode_hex(fields[10]), "ctx {context}");
    assert_eq!(
        event.raw_start,
        fields[11].parse::<usize>().unwrap(),
        "raw_start {context}"
    );
    assert_eq!(
        event.text_start,
        fields[12].parse::<usize>().unwrap(),
        "text_start {context}"
    );
    assert_eq!(
        event.text_end,
        fields[13].parse::<usize>().unwrap(),
        "text_end {context}"
    );
    assert_eq!(event.time, time, "time {context}");
    assert_eq!(event.raw_end, raw_end, "raw_end {context}");
}

/// `diffcase`：差分用例（后续 `diffpath` 子行）。
pub fn load_diffcase(harness: &mut Harness, rest: &str, lines: &mut impl Iterator<Item = String>) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let name = fields[0].to_string();
    let text = decode_hex(fields[1]);
    let count: usize = fields[2].parse().expect("path count");
    let mut path = Vec::with_capacity(count);
    for _ in 0..count {
        let line = lines.next().expect("path record");
        let (kind, rest) = line.split_once('\t').expect("path payload");
        assert_eq!(kind, "diffpath");
        let parts: Vec<&str> = rest.split('\t').collect();
        path.push(DiffPathNode {
            raw_length: parts[0].parse().expect("raw_length"),
            text_length: parts[1].parse().expect("text_length"),
        });
    }
    harness.diffcases.insert(name, DiffItem { text, path });
}

/// `diff`：差分事件逐项比对（后续 `diffevent` 子行）。
pub fn check_diff(harness: &Harness, rest: &str, lines: &mut impl Iterator<Item = String>) {
    let fields: Vec<&str> = rest.split('\t').collect();
    let raw = decode_bytes(fields[0]);
    let floor: usize = fields[1].parse().expect("floor");
    let mode = decode_hex(fields[2]);
    let before = harness.diffcases.get(fields[3]).cloned();
    let selected = harness.diffcases.get(fields[4]).cloned();
    let count: usize = fields[5].parse().expect("diff count");
    let events = learning::diff(&raw, before.as_ref(), selected.as_ref(), floor, &mode, 0.0);
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

/// `journalrecords`：日志键值对（后续 `journalrecord` 子行）。
pub fn load_journal_records(
    harness: &mut Harness,
    rest: &str,
    lines: &mut impl Iterator<Item = String>,
) {
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

/// `journalevents`：日志事件（后续 `journalevent` 子行）。
pub fn load_journal_events(
    harness: &mut Harness,
    rest: &str,
    lines: &mut impl Iterator<Item = String>,
) {
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
