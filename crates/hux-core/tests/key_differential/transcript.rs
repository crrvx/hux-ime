// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `key` transcript 的逐条重放：按记录种类分派到各自的比对。

use hux_core::key::{self, KeyEvent};
use hux_test_support::open_golden;
use std::io::BufRead;

/// 重放键金样，返回记录数；任何一条与本地实现不一致即 panic。
pub fn run() -> usize {
    let reader = open_golden("goldens/key.tsv.gz");
    let mut records = 0usize;
    for line in reader.lines() {
        let line = line.expect("read golden line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[0] {
            "name" => check_name(&fields),
            "repr" => check_repr(&fields),
            "parse" => check_parse(&fields),
            "modifier" => check_modifier(&fields),
            other => panic!("unknown record kind: {other}"),
        }
        records += 1;
    }
    records
}

fn check_name(fields: &[&str]) {
    let keyval: i32 = fields[1].parse().expect("keyval");
    let expected = if fields[2] == "-" {
        None
    } else {
        Some(fields[2])
    };
    assert_eq!(key::key_name(keyval), expected, "name for {keyval:#x}");
}

fn check_repr(fields: &[&str]) {
    let keyval: i32 = fields[1].parse().expect("keyval");
    let modifier: i32 = fields[2].parse().expect("modifier");
    let expected = fields[3];
    assert_eq!(
        KeyEvent::new(keyval, modifier).repr(),
        expected,
        "repr for {keyval:#x}/{modifier:#x}"
    );
}

fn check_parse(fields: &[&str]) {
    let repr = fields[1];
    let expected_ok = fields[2] == "ok";
    let parsed = KeyEvent::from_repr(repr);
    assert_eq!(parsed.is_some(), expected_ok, "parse ok flag for {repr:?}");
    if let Some(event) = parsed {
        let keycode: i32 = fields[3].parse().expect("keycode");
        let modifier: i32 = fields[4].parse().expect("modifier");
        assert_eq!(event.keycode, keycode, "parse keycode for {repr:?}");
        assert_eq!(event.modifier, modifier, "parse modifier for {repr:?}");
        assert_eq!(event.repr(), fields[5], "re-repr for {repr:?}");
    }
}

fn check_modifier(fields: &[&str]) {
    let index: u32 = fields[1].parse().expect("index");
    let expected = if fields[2] == "-" {
        None
    } else {
        Some(fields[2])
    };
    assert_eq!(
        key::modifier_name(1i32 << index),
        expected,
        "modifier {index}"
    );
}
