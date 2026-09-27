// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 证据候选池：从已发射候选与不完整尾码合并出 `EvidenceCandidate` 池。

use super::super::beam::{beam_limit_at, logsumexp};
use super::*;

impl Decoder {
    /// 参照 `add_early_commit_pool_candidate`。
    fn add_pool_candidate(
        &self,
        pool: &mut Vec<EvidenceCandidate>,
        pool_index: &mut HashMap<usize, HashMap<String, usize>>,
        text: String,
        confidence_score: f64,
        early_commit_confidence_score: f64,
        path: usize,
    ) {
        if text.is_empty() {
            return;
        }
        let raw_length = self.arena[path].raw_length;
        let boundary = pool_index.entry(raw_length).or_default();
        match boundary.get(&text).copied() {
            None => {
                pool.push(EvidenceCandidate {
                    text: text.clone(),
                    confidence_score,
                    early_commit_confidence_score,
                    path,
                });
                boundary.insert(text, pool.len() - 1);
            }
            Some(position) => {
                let previous = &pool[position];
                let combined = logsumexp(previous.confidence_score, confidence_score);
                let combined_early =
                    logsumexp(previous.early_confidence(), early_commit_confidence_score);
                // 参照：`best` 按早提交置信度（而非基础置信度）择优。
                let best_path = if early_commit_confidence_score > previous.early_confidence() {
                    path
                } else {
                    previous.path
                };
                pool[position] = EvidenceCandidate {
                    text,
                    confidence_score: combined,
                    early_commit_confidence_score: combined_early,
                    path: best_path,
                };
            }
        }
    }

    /// 收集满足必配前缀的已发射候选，并按边界汇总进池；返回参与证据的候选。
    pub(super) fn collect_pool<'a>(
        &self,
        pool: &mut Vec<EvidenceCandidate>,
        pool_index: &mut HashMap<usize, HashMap<String, usize>>,
        completed: &'a [Evaluated],
        required_text_prefix: &str,
    ) -> Vec<&'a Evaluated> {
        let mut visible: Vec<&Evaluated> = Vec::new();
        for candidate in completed {
            if candidate.text.is_empty() {
                continue;
            }
            if !required_text_prefix.is_empty() && !candidate.text.starts_with(required_text_prefix)
            {
                continue;
            }
            visible.push(candidate);
            self.add_pool_candidate(
                pool,
                pool_index,
                candidate.text.clone(),
                candidate.confidence_score,
                candidate.early_commit_confidence_score,
                candidate.path,
            );
        }
        visible
    }

    /// 参照 `incomplete_code_tail`。
    fn incomplete_code_tail(&self, tail: &[u8]) -> bool {
        if tail.is_empty() || !tail.iter().all(|byte| byte.is_ascii_alphabetic()) {
            return false;
        }
        let Ok(text) = std::str::from_utf8(tail) else {
            return false;
        };
        if !self.lexicon.proper_code_prefixes.contains(text) {
            return false;
        }
        tail.len() < 2 || !self.lexicon.codes.contains_key(text)
    }

    /// 合并不完整尾码：尾码截断处的前缀仍要参与证据。
    /// 返回（是否截断，是否合并过尾码）。
    pub(super) fn merge_incomplete_tails(
        &mut self,
        raw: &[u8],
        states: &mut [Bucket],
        pool: &mut Vec<EvidenceCandidate>,
        pool_index: &mut HashMap<usize, HashMap<String, usize>>,
        required_text_prefix: &str,
        completed_truncated: bool,
    ) -> Result<(bool, bool)> {
        let mut truncated = completed_truncated;
        let mut merged_incomplete_tail = false;
        let maximum_tail_length = self
            .lexicon
            .max_code_len
            .saturating_sub(1)
            .min(raw.len().saturating_sub(1));
        for tail_length in 1..=maximum_tail_length {
            let consumed_length = raw.len() - tail_length;
            let tail = &raw[consumed_length..];
            if !self.incomplete_code_tail(tail) || consumed_length >= states.len() {
                continue;
            }
            let partial = self.dedup_limit(
                std::mem::take(&mut states[consumed_length]),
                beam_limit_at(consumed_length),
            );
            states[consumed_length] = partial;
            let partial_items: Vec<usize> = states[consumed_length].items.clone();
            let partial_truncated = states[consumed_length].truncated;
            let mut added = false;
            for index in partial_items {
                let candidate = self.evaluate_state(index)?;
                if candidate.text.is_empty() {
                    continue;
                }
                if !required_text_prefix.is_empty()
                    && !candidate.text.starts_with(required_text_prefix)
                {
                    continue;
                }
                self.add_pool_candidate(
                    pool,
                    pool_index,
                    candidate.text.clone(),
                    candidate.confidence_score,
                    candidate.early_commit_confidence_score,
                    candidate.path,
                );
                added = true;
            }
            if added {
                merged_incomplete_tail = true;
                truncated = truncated || partial_truncated;
            }
        }
        Ok((truncated, merged_incomplete_tail))
    }
}

/// 参照 `has_low_confidence_completed_generation`。
pub(super) fn has_low_confidence_completed_generation(candidates: &[&Evaluated]) -> bool {
    if candidates.is_empty() {
        return false;
    }
    let max_score = candidates
        .iter()
        .map(|candidate| candidate.confidence_score)
        .fold(f64::NEG_INFINITY, f64::max);
    let total: f64 = candidates
        .iter()
        .map(|candidate| (candidate.confidence_score - max_score).exp())
        .sum();
    total > 0.0 && 1.0 / total < EARLY_COMMIT_MINIMUM_SHARE
}
