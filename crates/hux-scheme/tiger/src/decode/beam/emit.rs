// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 候选发射：最终排序、来源融合、上限裁剪与路径文本化。
//! `emit` 依次调用求值、排序、展示回写、词先验重排、来源融合与证据构建。

use super::super::fusion::apply_fusion_ordering;
use super::*;

impl Decoder {
    pub(super) fn emit(
        &mut self,
        raw: &[u8],
        states: &mut [Bucket],
        length: usize,
        include_early_commit: bool,
        required_text_prefix: &str,
    ) -> Result<DecodeOutput> {
        let completed =
            self.dedup_limit(std::mem::take(&mut states[length]), beam_limit_at(length));
        states[length] = completed;
        let candidates: Vec<usize> = states[length].items.clone();
        let completed_truncated = states[length].truncated;
        let mut all = Vec::with_capacity(candidates.len());
        for index in candidates {
            all.push(self.evaluate_state(index)?);
        }
        let comparator = self.emit_comparator(&all);
        let order = self.emit_order(&all, comparator);
        let mut items = self.emit_display_items(raw, &mut all, &order);
        self.apply_lexical_prior(&mut items, &mut all, &order, comparator);
        self.fuse_sources(raw, &mut items);
        let mut evidence = Evidence::default_for(completed_truncated);
        if include_early_commit {
            evidence = self.build_early_commit_evidence(
                raw,
                states,
                &all,
                completed_truncated,
                required_text_prefix,
            )?;
        }
        let visible_prefixes = self.visible_prefixes(&items);
        Ok(DecodeOutput {
            items,
            confidence_candidates: all,
            evidence,
            visible_prefixes,
            learning_affected: self.learning_affected,
            completed_truncated,
        })
    }

    /// 最终排序比较器：无模型 → NoModel，有模型按 `prefer_score_over_lexicon_rank`
    /// 取 ScoreFirst/RankFirst；任一候选带学习分则强制 ScoreFirst。
    fn emit_comparator(&self, all: &[Evaluated]) -> Comparator {
        // 参照 `emit`：无模型 → NoModel；有模型 → prefer_score 时 ScoreFirst，
        // 否则 RankFirst（注意与 `current_state_comparator` 的 allow_dup 分支不同）。
        let mut comparator = if self.model.is_none() {
            Comparator::NoModel
        } else if self.prefer_score_over_lexicon_rank(all) {
            Comparator::ScoreFirst
        } else {
            Comparator::RankFirst
        };
        if all.iter().any(|item| item.learning_score > 0.0) {
            comparator = Comparator::ScoreFirst;
        }
        comparator
    }

    /// 排序下标：按 `comparator` 排序后截到候选上限。
    fn emit_order(&self, all: &[Evaluated], comparator: Comparator) -> Vec<usize> {
        let mut order: Vec<usize> = (0..all.len()).collect();
        order.sort_by(|&left, &right| {
            if Decoder::state_better(comparator, &all[left], &all[right]) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        });
        order.truncate(CANDIDATE_LIMIT);
        order
    }

    /// 展示 Top-K：按 `order` 取候选，并把 `segmented` 回写到置信度视图。
    fn emit_display_items(
        &mut self,
        raw: &[u8],
        all: &mut [Evaluated],
        order: &[usize],
    ) -> Vec<Evaluated> {
        let mut items: Vec<Evaluated> = order.iter().map(|&index| all[index].clone()).collect();
        // 参照中 Top-K 与 `_confidence_candidates` 共享同一批表：展示字段（segmented）
        // 需同步回写，保持两个视图一致。
        for (position, item) in items.iter_mut().enumerate() {
            item.segmented = segmented_from_path(raw, &self.arena, item.path);
            all[order[position]].segmented = item.segmented.clone();
        }
        items
    }

    /// 词先验：只重排展示 Top-N（不改 mass/置信度，也不改变候选集合）。
    fn apply_lexical_prior(
        &mut self,
        items: &mut [Evaluated],
        all: &mut [Evaluated],
        order: &[usize],
        comparator: Comparator,
    ) {
        if items.len() > 1
            && let Some(model) = &self.lexical
            && self.ranking_prior.lexical_prior_weight > 0.0
            && self.model.is_some()
        {
            let mut cache = Map::new();
            let limit = self.ranking_prior.lexical_candidate_limit.min(items.len());
            for (position, item) in items.iter_mut().take(limit).enumerate() {
                let lexical_score = model.score_with_cache(&item.text, &mut cache)
                    * self.ranking_prior.lexical_prior_weight;
                item.score += lexical_score;
                all[order[position]].score = item.score;
            }
            items.sort_by(|left, right| {
                if Decoder::state_better(comparator, left, right) {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            });
        }
    }

    /// 跨来源偏好融合（参照 `learning.apply_fusion_ordering(raw, result)`）：
    /// 在词先验重排之后、证据构建之前重排展示 Top-K。
    fn fuse_sources(&mut self, raw: &[u8], items: &mut [Evaluated]) {
        let (index, mode) = match &mut self.learning {
            Some(wiring) => (Some(&mut wiring.index), wiring.mode.as_str()),
            None => (None, ""),
        };
        apply_fusion_ordering(index, mode, raw, items);
    }

    /// 可见顶层候选路径上的全部前缀（参照 `prefix_belongs_to_visible`）。
    fn visible_prefixes(&self, items: &[Evaluated]) -> Set<(usize, String)> {
        let mut visible_prefixes: Set<(usize, String)> = Set::new();
        for item in items {
            let mut current = Some(item.path);
            while let Some(index) = current {
                let text = self.arena[index].text.clone();
                if item.text.starts_with(&text) {
                    visible_prefixes.insert((self.arena[index].raw_length, text));
                }
                current = self.arena[index].previous;
            }
        }
        visible_prefixes
    }

    fn prefer_score_over_lexicon_rank(&self, values: &[Evaluated]) -> bool {
        if !self.allow_duplicate_single {
            return false;
        }
        values.iter().any(|item| {
            self.arena[item.path]
                .previous
                .map(|previous| self.arena[previous].raw_length > 0)
                .unwrap_or(false)
        })
    }
}

/// 参照 `segmented_from_path`：按路径边界切分原始输入，以空格连接。
pub(super) fn segmented_from_path(raw: &[u8], arena: &[State], path: usize) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let mut ends = Vec::new();
    let mut current = Some(path);
    while let Some(index) = current {
        if arena[index].raw_length == 0 {
            break;
        }
        ends.push(arena[index].raw_length);
        current = arena[index].previous;
    }
    let mut pieces = Vec::new();
    let mut start = 0usize;
    for &finish in ends.iter().rev() {
        if finish <= start || finish > raw.len() {
            break;
        }
        pieces.push(String::from_utf8_lossy(&raw[start..finish]).into_owned());
        start = finish;
    }
    pieces.join(" ")
}
