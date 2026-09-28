// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 评分换算：事件/键校验、纠错等级分、分区累加与汇总码提取。

use super::model::{Choice, Event, Group, MAX_LEVEL, Summary};
use super::text::{chars, key, static_text, unframe};
use hashbrown::HashMap;

fn mode_valid(mode: &str) -> bool {
    !mode.is_empty() && mode.len() <= 512
}

fn code_valid(code: &str) -> bool {
    !code.is_empty() && code.len() <= 128
}

fn context_valid(context: &str) -> bool {
    if context.is_empty() {
        return true;
    }
    // 只解码一次：非空与长度判定复用同一结果。
    chars(context).is_some_and(|list| !list.is_empty() && list.len() <= 2)
}

/// 参照 `M.build` / `M.runtime_index` 的共用事件过滤。
pub(super) fn event_valid(e: &Event) -> bool {
    mode_valid(&e.mode)
        && code_valid(&e.code)
        && static_text(&e.text)
        && context_valid(&e.context)
        && e.time >= 0.0
}

/// 参照 `correction_level`：等级 = 向下取整的累积权重，钳到 `0..=MAX_LEVEL`。
fn correction_level(weight: f64) -> f64 {
    (weight + 1e-12).floor().clamp(0.0, MAX_LEVEL)
}

/// 参照 `exact_score`：同上下文纠错等级分（L1=9 … L10=27）。
pub(super) fn exact_score(weight: f64) -> f64 {
    let level = correction_level(weight);
    if level > 0.0 { 7.0 + 2.0 * level } else { 0.0 }
}

/// 参照 `general_score`：跨上下文纠错等级分（L1=6 … L10=24）。
pub(super) fn general_of(weight: f64) -> f64 {
    let level = correction_level(weight);
    if level > 0.0 { 4.0 + 2.0 * level } else { 0.0 }
}

pub(super) fn score_of(summary: Option<&Summary>, context: &str) -> f64 {
    match summary {
        None => 0.0,
        Some(summary) => summary
            .general
            .max(summary.exact.get(context).copied().unwrap_or(0.0)),
    }
}

/// 参照 `append_group`：把一条事件并入分区（同组竞争项各 ×0.25，命中项等级 +1）。
pub(super) fn append_group(partition: &mut HashMap<String, Group>, e: &Event) {
    let k = key(&[&e.mode, &e.context]);
    let old = partition.remove(&k);
    let mut group = Group {
        code: e.code.clone(),
        mode: e.mode.clone(),
        context: e.context.clone(),
        choices: HashMap::new(),
    };
    if let Some(old) = old {
        for (text, choice) in old.choices {
            let weight = choice.weight * if text != e.text { 0.25 } else { 1.0 };
            group.choices.insert(text, Choice { weight });
        }
    }
    let choice = group
        .choices
        .entry(e.text.clone())
        .or_insert(Choice { weight: 0.0 });
    choice.weight = MAX_LEVEL.min(choice.weight + 1.0);
    partition.insert(k, group);
}

pub(super) fn summary_code(summary_key: &str) -> String {
    match unframe(summary_key) {
        Some(parts) if !parts.is_empty() => parts[0].clone(),
        _ => String::new(),
    }
}
