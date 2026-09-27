// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 索引查询：`score`/`fusion_score`/`prefix_score` 与窗口、前缀查找。

use super::super::hash::{fusion_mode, fusion_pair_code};
use super::super::model::{CODE_WINDOW_SLOTS, LearningIndex};
use super::super::score::score_of;
use super::super::text::key;

impl LearningIndex {
    /// 参照 `M.score`。
    pub fn score(&mut self, mode: &str, code: &str, text: &str, context: &str) -> f64 {
        let k = key(&[code, mode, text]);
        if let Some(exact) = &self.exact {
            return score_of(exact.get(&k), context);
        }
        match self.materialized(code) {
            None => 0.0,
            Some(materialized) => score_of(materialized.exact.get(&k), context),
        }
    }

    /// 参照 `M.fusion_score`：同一 `(raw, direct, composed)` 三元组内 `D` 与 `C` 的得分差。
    ///
    /// 入参 `mode` 是**实时模式**（内部自行加 `fusion-v1|` 前缀）；空模式一律 0
    /// （参照的「索引为 nil」在 Rust 由调用侧用 `Option` 表达，空索引本身也返回 0）。
    pub fn fusion_score(&mut self, mode: &str, raw: &[u8], direct: &str, composed: &str) -> f64 {
        if mode.is_empty() {
            return 0.0;
        }
        let fusion = fusion_mode(mode);
        let code = fusion_pair_code(raw, direct, composed);
        self.score(&fusion, &code, "D", "") - self.score(&fusion, &code, "C", "")
    }

    /// `code_window`：返回 `codes` 中 [first, last] 闭区间（0 基；last 可为 -1）。
    fn code_window(&mut self, code: &str) -> (usize, isize) {
        if let Some(&bounds) = self.code_windows.get(&code.to_string()) {
            return bounds;
        }
        let first = self
            .codes
            .partition_point(|existing| existing.as_str() < code);
        let mut last = first as isize - 1;
        let end = (first + CODE_WINDOW_SLOTS).min(self.codes.len());
        for candidate in self.codes.iter().take(end).skip(first) {
            if !candidate.starts_with(code) {
                break;
            }
            last += 1;
        }
        let bounds = (first, last);
        self.code_windows.put(code.to_string(), bounds);
        bounds
    }

    fn lookup_prefix(&mut self, code: &str, mode: &str, text: &str, context: &str) -> f64 {
        let pk = key(&[code, mode, text]);
        let entry = if let Some(prefixes) = &self.prefixes {
            prefixes.get(&pk).cloned()
        } else {
            self.materialized(code)
                .and_then(|materialized| materialized.prefixes.get(&pk).cloned())
        };
        entry
            .map(|entry| {
                entry
                    .general
                    .max(entry.exact.get(context).copied().unwrap_or(0.0))
            })
            .unwrap_or(0.0)
    }

    /// 参照 `M.prefix_score`。
    pub fn prefix_score(&mut self, mode: &str, code: &str, text: &str, context: &str) -> f64 {
        if code.is_empty() || text.is_empty() {
            return 0.0;
        }
        let (first, last) = self.code_window(code);
        if last < 0 || first as isize > last {
            return 0.0;
        }
        let query = key(&[mode, code, text, context]);
        if let Some(cached) = self.prefix_queries.get(&query) {
            return *cached;
        }
        let candidates: Vec<String> = self.codes[first..=last as usize].to_vec();
        let mut best = 0.0f64;
        for candidate in candidates {
            if candidate.len() > code.len() {
                best = best.max(self.lookup_prefix(&candidate, mode, text, context));
            }
        }
        self.prefix_queries.put(query, best);
        best
    }
}
