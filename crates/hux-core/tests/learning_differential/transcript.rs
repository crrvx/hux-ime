// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 金样主循环：按记录种类分派，循环结束后校验日志编码。

use std::io::BufRead;

use crate::harness::Harness;
use crate::{events, index, journal, pure};

/// 逐行重放金样记录；返回已消费的记录数。
pub fn run(mut harness: Harness, reader: impl BufRead) -> usize {
    let mut lines = reader
        .lines()
        .map(|line| line.expect("read golden line"))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    while let Some(line) = lines.next() {
        let (kind, rest) = line.split_once('\t').unwrap_or((&line, ""));
        match kind {
            "hash" => pure::check_hash(rest),
            "corpus" => index::load_corpus(&mut harness, rest, &mut lines),
            "index" => index::build_index(&mut harness, rest),
            "confirmed" => index::build_confirmed(&mut harness, rest),
            "codes" => index::check_codes(&harness, rest),
            "score" => index::check_score(&mut harness, rest),
            "prefix" => index::check_prefix(&mut harness, rest),
            "trim" => index::trim_caches(&mut harness, rest),
            "chain" => index::load_chain(&mut harness, rest, &mut lines),
            "reward" => index::check_reward(&mut harness, rest),
            "maturity" => pure::check_maturity(rest),
            "contribution" => pure::check_contribution(rest),
            "fusionmode" => pure::check_fusion_mode(rest),
            "paircode" => pure::check_pair_code(rest),
            "fusion" => index::check_fusion(&mut harness, rest),
            "fusionnone" => events::check_fusion_empty(rest),
            "fusionevent" => events::check_fusion_event(rest),
            "diffcase" => events::load_diffcase(&mut harness, rest, &mut lines),
            "diff" => events::check_diff(&harness, rest, &mut lines),
            "journalrecords" => events::load_journal_records(&mut harness, rest, &mut lines),
            "journalevents" => events::load_journal_events(&mut harness, rest, &mut lines),
            other => panic!("unknown record kind: {other}"),
        }
        harness.records += 1;
    }

    journal::verify(&harness);
    harness.records
}
