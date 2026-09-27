// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 金样装载：`key_sequence.tsv.gz` / `sound_to_char_shape.tsv.gz` 的 `case` 与 `step` 行。

use flate2::read::GzDecoder;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub struct Step {
    pub repr: String,
    pub consumed: bool,
    pub input: String,
    pub caret: usize,
    pub commit: String,
    /// 参照 `RimeMenu::page_no`（0 基；参与比对）。
    pub page_no: usize,
    pub highlight: usize,
    pub count: usize,
    pub candidates: Vec<String>,
    pub comments: Vec<String>,
}

pub struct Case {
    pub name: String,
    pub options: Vec<(String, bool)>,
    pub steps: Vec<Step>,
}

pub fn load_cases(path: &Path) -> Vec<Case> {
    let file = std::fs::File::open(path).expect("open key_sequence golden");
    let mut cases: Vec<Case> = Vec::new();
    for line in BufReader::new(GzDecoder::new(file)).lines() {
        let line = line.expect("golden line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[0] {
            "case" => {
                let options = fields
                    .get(2)
                    .filter(|value| !value.is_empty())
                    .map(|value| {
                        value
                            .split(',')
                            .map(|item| {
                                let (name, value) =
                                    item.split_once('=').expect("option assignment");
                                (name.to_string(), value == "1")
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                cases.push(Case {
                    name: fields[1].to_string(),
                    options,
                    steps: Vec::new(),
                });
            }
            "step" => {
                assert_eq!(fields.len(), 14, "step fields: {line}");
                cases
                    .last_mut()
                    .expect("case record before step")
                    .steps
                    .push(step_from_fields(&fields));
            }
            other => panic!("unknown golden record: {other}"),
        }
    }
    cases
}

/// 金样 `step` 行 → [`Step`]（比对字段 + 计数语义：`count == 0` 时候选/注释为空）。
pub fn step_from_fields(fields: &[&str]) -> Step {
    let count: usize = fields[11].parse().expect("count");
    // `-` 既表示「无候选」也表示「单候选且文本为空」，用计数区分。
    let candidates = if count == 0 {
        Vec::new()
    } else {
        fields[12].split(',').map(str::to_string).collect()
    };
    let comments = if count == 0 {
        Vec::new()
    } else {
        fields[13].split(',').map(str::to_string).collect()
    };
    Step {
        repr: fields[3].to_string(),
        consumed: fields[4] == "1",
        input: fields[5].to_string(),
        caret: fields[6].parse().expect("caret"),
        commit: fields[7].to_string(),
        page_no: fields[9].parse().expect("page_no"),
        highlight: fields[10].parse().expect("highlight"),
        count,
        candidates,
        comments,
    }
}

/// 覆盖下限：金样被截断 / 少解析若干 case 时必须失败，不得静默变少。
pub fn check_coverage(cases: &[Case]) {
    let golden_steps: usize = cases.iter().map(|case| case.steps.len()).sum();
    assert!(
        cases.len() >= 68,
        "key_sequence 金样用例数不足：{} < 68（金样被截断？）",
        cases.len()
    );
    assert!(
        golden_steps >= 285,
        "key_sequence 金样步数不足：{golden_steps} < 285（金样被截断？）"
    );
}
