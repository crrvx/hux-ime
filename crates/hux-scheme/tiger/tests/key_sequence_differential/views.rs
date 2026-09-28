// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 逐步比对视图：金样行、本仓期望行与实测行共用的字段集合与逐字段比对。

use crate::golden::Step;

/// 一步的比对视图（金样行 / 本仓期望行 / 实测行共用）。
#[derive(PartialEq, Eq, Debug)]
pub struct RowView {
    pub consumed: bool,
    pub input: String,
    pub caret: usize,
    pub commit: String,
    pub page_no: usize,
    pub highlight: usize,
    pub count: usize,
    pub candidates: Vec<String>,
    pub comments: Vec<String>,
}

impl RowView {
    pub fn of_golden(step: &Step) -> Self {
        Self {
            consumed: step.consumed,
            input: step.input.clone(),
            caret: step.caret,
            commit: step.commit.clone(),
            page_no: step.page_no,
            highlight: step.highlight,
            count: step.count,
            candidates: step.candidates.clone(),
            comments: step.comments.clone(),
        }
    }
}

/// 实测的一步（重放产物）。
pub struct Observed {
    pub repr: String,
    pub row: RowView,
}

/// 逐字段比对（失败消息与既有实现一致，另附期望/实测的紧凑行）。
pub fn compare(label: &str, expected: &RowView, observed: &RowView, failures: &mut Vec<String>) {
    if observed.consumed != expected.consumed {
        failures.push(format!(
            "{label}: consumed 期望 {} 实际 {}",
            expected.consumed, observed.consumed
        ));
    }
    if observed.input != expected.input {
        failures.push(format!(
            "{label}: input 期望 {} 实际 {}",
            expected.input, observed.input
        ));
    }
    if observed.caret != expected.caret {
        failures.push(format!(
            "{label}: caret 期望 {} 实际 {}",
            expected.caret, observed.caret
        ));
    }
    if observed.commit != expected.commit {
        failures.push(format!(
            "{label}: commit 期望 {} 实际 {}",
            expected.commit, observed.commit
        ));
    }
    if observed.page_no != expected.page_no {
        failures.push(format!(
            "{label}: page_no 期望 {} 实际 {}",
            expected.page_no, observed.page_no
        ));
    }
    if observed.highlight != expected.highlight {
        failures.push(format!(
            "{label}: highlight 期望 {} 实际 {}",
            expected.highlight, observed.highlight
        ));
    }
    if observed.count != expected.count {
        failures.push(format!(
            "{label}: candidate count 期望 {} 实际 {}",
            expected.count, observed.count
        ));
    }
    if observed.candidates != expected.candidates {
        failures.push(format!(
            "{label}: candidates 期望 {:?} 实际 {:?}",
            expected.candidates, observed.candidates
        ));
    }
    if observed.comments != expected.comments {
        failures.push(format!(
            "{label}: comments 期望 {:?} 实际 {:?}",
            expected.comments, observed.comments
        ));
    }
}

/// 两个视图是否逐字段相同（用于「确有差异」判定）。
pub fn rows_differ(left: &RowView, right: &RowView) -> bool {
    left != right
}

/// 登记表的紧凑期望行 → [`RowView`]（字段序与 [`step_from_fields`] 的比对字段一致）。
pub fn expected_row(raw: &str) -> RowView {
    let fields: Vec<&str> = raw.split('\t').collect();
    assert_eq!(
        fields.len(),
        10,
        "登记表期望行必须 10 列（repr/consumed/input/caret/commit/page_no/highlight/count/candidates/comments）：{raw:?}"
    );
    let count: usize = fields[7].parse().expect("count");
    let split = |value: &str| -> Vec<String> {
        if count == 0 {
            Vec::new()
        } else {
            value.split(',').map(str::to_string).collect()
        }
    };
    RowView {
        consumed: fields[1] == "1",
        input: fields[2].to_string(),
        caret: fields[3].parse().expect("caret"),
        commit: fields[4].to_string(),
        page_no: fields[5].parse().expect("page_no"),
        highlight: fields[6].parse().expect("highlight"),
        count,
        candidates: split(fields[8]),
        comments: split(fields[9]),
    }
}

/// 紧凑行（失败消息用；与登记表期望行同格式）。
pub fn describe(row: &RowView) -> String {
    let join = |values: &[String]| -> String {
        if values.is_empty() {
            "-".to_string()
        } else {
            values.join(",")
        }
    };
    format!(
        "consumed={} input={} caret={} commit={} highlight={} count={} candidates={} comments={}",
        row.consumed as u8,
        row.input,
        row.caret,
        row.commit,
        row.highlight,
        row.count,
        join(&row.candidates),
        join(&row.comments)
    )
}
