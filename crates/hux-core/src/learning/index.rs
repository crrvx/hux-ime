// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 索引本体：全量重放（`build`）、运行时快照（`runtime`/`update`）与物化缓存。

use super::model::{
    CODE_WINDOW_LIMIT, Choice, Event, Group, LearningIndex, MATERIALIZED_CODE_LIMIT, MAX_LEVEL,
    Materialized, PREFIX_QUERY_LIMIT, PrefixEntry, Summary,
};
use super::score::{append_group, event_valid, exact_score, general_of, summary_code};
use super::text::{chars, key};
use crate::cache::Fifo;
use hashbrown::{HashMap, HashSet};
use std::rc::Rc;

mod query;

impl LearningIndex {
    fn empty() -> Self {
        Self {
            codes: Vec::new(),
            now: 0.0,
            future: 0.0,
            partitions: None,
            exact: None,
            prefixes: None,
            cache: Fifo::new(MATERIALIZED_CODE_LIMIT),
            prefix_queries: Fifo::new(PREFIX_QUERY_LIMIT),
            code_windows: Fifo::new(CODE_WINDOW_LIMIT),
        }
    }

    /// 参照 `M.build`：独立的全量重放 oracle。
    ///
    /// `now` 只写入 `index.now`，不参与分数计算（时间语义见模块文档）。
    pub fn build(events: &[Event], now: f64) -> Self {
        let groups = replay_groups(events);
        let summaries = summarize_choices(&groups);
        let (mut codes, exact) = collect_codes_and_exact(summaries);
        let prefixes = build_prefixes(&exact);
        codes.sort();
        Self {
            codes,
            now,
            future: 0.0,
            partitions: None,
            exact: Some(exact),
            prefixes: Some(prefixes),
            cache: Fifo::new(MATERIALIZED_CODE_LIMIT),
            prefix_queries: Fifo::new(PREFIX_QUERY_LIMIT),
            code_windows: Fifo::new(CODE_WINDOW_LIMIT),
        }
    }

    /// 参照 `M.runtime_index`。
    ///
    /// `now` 与 `future` 按参照写入（`future` 记录最大事件时间），二者只作元数据
    /// （时间语义见模块文档）。
    pub fn runtime(events: &[Event], now: f64) -> Self {
        let mut index = Self::empty();
        index.now = now;
        let mut partitions: HashMap<String, HashMap<String, Group>> = HashMap::new();
        let mut codes: Vec<String> = Vec::new();
        for e in events {
            if !event_valid(e) {
                continue;
            }
            let partition = partitions.entry(e.code.clone()).or_default();
            if !codes.contains(&e.code) {
                codes.push(e.code.clone());
            }
            append_group(partition, e);
            index.future = index.future.max(e.time);
        }
        codes.sort();
        index.codes = codes;
        index.partitions = Some(partitions);
        index
    }

    /// 参照 `update_index`：重建受影响的 code 分区。
    ///
    /// 本仓无「时钟回退 / 未来事件 ⇒ 全量重放」的判据，接受的事件也不参与 `future` 更新
    /// （`future` 只作元数据，原样带过；时间语义见模块文档）。
    /// 注意：`partitions.clone()` 为整体深拷贝（参照的 `copy` 只复制外层表），
    /// 单次确认代价 O(历史规模)。
    pub fn update(&self, accepted: &[Event], all_events: &[Event], now: f64) -> Self {
        let Some(partitions) = &self.partitions else {
            return Self::runtime(all_events, now);
        };
        let mut next_partitions = partitions.clone();
        let mut changed: HashSet<String> = HashSet::new();
        let mut new_codes: Vec<String> = Vec::new();
        for e in accepted {
            if !event_valid(e) {
                continue;
            }
            if !changed.contains(&e.code) {
                let existing = next_partitions.get(&e.code).cloned();
                if existing.is_none() {
                    new_codes.push(e.code.clone());
                }
                next_partitions.insert(e.code.clone(), existing.unwrap_or_default());
                changed.insert(e.code.clone());
            }
            append_group(next_partitions.get_mut(&e.code).expect("inserted"), e);
        }
        let mut codes = self.codes.clone();
        for code in &new_codes {
            let position = codes.partition_point(|existing| existing < code);
            codes.insert(position, code.clone());
        }
        Self {
            codes,
            now,
            future: self.future,
            partitions: Some(next_partitions),
            exact: None,
            prefixes: None,
            cache: Fifo::new(MATERIALIZED_CODE_LIMIT),
            prefix_queries: Fifo::new(PREFIX_QUERY_LIMIT),
            code_windows: Fifo::new(CODE_WINDOW_LIMIT),
        }
    }

    /// 参照 `M.trim_caches`。
    pub fn trim_caches(&mut self) {
        if self.partitions.is_some() {
            self.cache.clear();
        }
        self.prefix_queries.clear();
        self.code_windows.clear();
    }

    fn materialized(&mut self, code: &str) -> Option<Rc<Materialized>> {
        let partitions = self.partitions.as_ref()?;
        if let Some(cached) = self.cache.get(&code.to_string()) {
            return Some(cached.clone());
        }
        let partition = partitions.get(code)?;
        let mut result = Materialized::default();
        // 同 `build`：累加顺序需确定（哈希序跨进程不定）。
        let mut group_keys: Vec<&String> = partition.keys().collect();
        group_keys.sort();
        for group_key in group_keys {
            let group = &partition[group_key];
            for (text, choice) in &group.choices {
                let k = key(&[code, &group.mode, text]);
                let summary = result.exact.entry(k).or_insert_with(|| Summary {
                    mode: group.mode.clone(),
                    text: text.clone(),
                    exact: HashMap::new(),
                    weight: 0.0,
                    general: 0.0,
                });
                summary
                    .exact
                    .insert(group.context.clone(), exact_score(choice.weight));
                summary.weight += choice.weight;
            }
        }
        for summary in result.exact.values_mut() {
            summary.general = general_of(summary.weight);
        }
        for summary in result.exact.values() {
            let letters = chars(&summary.text).unwrap_or_default();
            let mut prefix = String::new();
            for letter in letters.iter().take(letters.len().saturating_sub(1)) {
                prefix.push(*letter);
                let pk = key(&[code, &summary.mode, &prefix]);
                let entry = result.prefixes.entry(pk).or_insert_with(|| PrefixEntry {
                    general: 0.0,
                    exact: HashMap::new(),
                });
                entry.general = entry.general.max(summary.general);
                for (ctx, value) in &summary.exact {
                    let slot = entry.exact.entry(ctx.clone()).or_insert(0.0);
                    *slot = slot.max(*value);
                }
            }
        }
        let shared = Rc::new(result);
        self.cache.put(code.to_string(), shared.clone());
        Some(shared)
    }
}

/// 全量重放事件为分组（同一 `code/mode/context` 下的候选权重表）。
fn replay_groups(events: &[Event]) -> HashMap<String, Group> {
    let mut groups: HashMap<String, Group> = HashMap::new();
    for e in events {
        if !event_valid(e) {
            continue;
        }
        let k = key(&[&e.code, &e.mode, &e.context]);
        let group = groups.entry(k).or_insert_with(|| Group {
            code: e.code.clone(),
            mode: e.mode.clone(),
            context: e.context.clone(),
            choices: HashMap::new(),
        });
        for (text, choice) in group.choices.iter_mut() {
            if *text != e.text {
                choice.weight *= 0.25;
            }
        }
        let choice = group
            .choices
            .entry(e.text.clone())
            .or_insert(Choice { weight: 0.0 });
        choice.weight = MAX_LEVEL.min(choice.weight + 1.0);
    }
    groups
}

/// 分组 → 汇总（`text` 维度的精确分与总权重）。
fn summarize_choices(groups: &HashMap<String, Group>) -> HashMap<String, Summary> {
    let mut summaries: HashMap<String, Summary> = HashMap::new();
    // 汇总为浮点累加，顺序需确定（哈希序跨进程不定）：group 键排序后遍历。
    let mut group_keys: Vec<&String> = groups.keys().collect();
    group_keys.sort();
    for group_key in group_keys {
        let group = &groups[group_key];
        for (text, choice) in &group.choices {
            let k = key(&[&group.code, &group.mode, text]);
            let summary = summaries.entry(k).or_insert_with(|| Summary {
                mode: group.mode.clone(),
                text: text.clone(),
                exact: HashMap::new(),
                weight: 0.0,
                general: 0.0,
            });
            summary
                .exact
                .insert(group.context.clone(), exact_score(choice.weight));
            summary.weight += choice.weight;
        }
    }
    summaries
}

/// 汇总 → 码表（去重后待排序）与精确分表（`general` 在此定稿）。
fn collect_codes_and_exact(
    summaries: HashMap<String, Summary>,
) -> (Vec<String>, HashMap<String, Summary>) {
    let mut exact: HashMap<String, Summary> = HashMap::new();
    let mut codes: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (k, mut summary) in summaries {
        summary.general = general_of(summary.weight);
        let code = summary_code(&k);
        if seen.insert(code.clone()) {
            codes.push(code);
        }
        exact.insert(k, summary);
    }
    (codes, exact)
}

/// 精确分表 → 前缀表（每个 `text` 的全前缀，逐上下文取最大）。
fn build_prefixes(exact: &HashMap<String, Summary>) -> HashMap<String, PrefixEntry> {
    let mut prefixes: HashMap<String, PrefixEntry> = HashMap::new();
    for (k, summary) in exact {
        let code = summary_code(k);
        let letters = chars(&summary.text).unwrap_or_default();
        let mut prefix = String::new();
        for letter in letters.iter().take(letters.len().saturating_sub(1)) {
            prefix.push(*letter);
            let pk = key(&[&code, &summary.mode, &prefix]);
            let entry = prefixes.entry(pk).or_insert_with(|| PrefixEntry {
                general: 0.0,
                exact: HashMap::new(),
            });
            entry.general = entry.general.max(summary.general);
            for (ctx, value) in &summary.exact {
                let slot = entry.exact.entry(ctx.clone()).or_insert(0.0);
                *slot = slot.max(*value);
            }
        }
    }
    prefixes
}
