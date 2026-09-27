// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 早提交证据：`Decoder` 的第二个 impl——从扩展池收集证据候选并汇总前缀证据。
//!
//! 与主 impl（`beam`）的分工：主 impl 推进解码状态并发射候选；此处只读候选池与
//! arena 纯计算证据，不推进状态。

use super::beam::{beam_limit_at, logsumexp};
use super::*;

// ---------------------------------------------------------------- 早提交证据

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

    /// 参照 `build_early_commit_evidence`。
    pub(super) fn build_early_commit_evidence(
        &mut self,
        raw: &[u8],
        states: &mut [Bucket],
        completed: &[Evaluated],
        completed_truncated: bool,
        required_text_prefix: &str,
    ) -> Result<Evidence> {
        // 截断池不再早退：保留已算出的质量用于「强证据」策略（强证据只认 BaseShare）。
        let mut pool: Vec<EvidenceCandidate> = Vec::new();
        let mut pool_index: HashMap<usize, HashMap<String, usize>> = HashMap::new();
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
                &mut pool,
                &mut pool_index,
                candidate.text.clone(),
                candidate.confidence_score,
                candidate.early_commit_confidence_score,
                candidate.path,
            );
        }

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
                    &mut pool,
                    &mut pool_index,
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

        let prefixes = build_prefix_evidence(&pool, &self.arena);

        let mut proposal = String::new();
        let mut proposal_share = 0.0;
        let mut proposal_raw_length = 0usize;
        let mut proposal_chars = 0usize;
        let mut raw_lengths: Map<String, usize> = Map::new();
        let mut raw_share: HashMap<String, f64> = HashMap::new();
        for prefix in &prefixes {
            if !prefix.boundary_closed {
                continue;
            }
            let replace_raw = match raw_lengths.get(&prefix.text) {
                None => true,
                Some(current_length) => {
                    let current_share = raw_share.get(&prefix.text).copied().unwrap_or(0.0);
                    prefix.share > current_share
                        || (prefix.share == current_share && prefix.raw_length < *current_length)
                }
            };
            if replace_raw {
                raw_lengths.insert(prefix.text.clone(), prefix.raw_length);
                raw_share.insert(prefix.text.clone(), prefix.share);
            }
            if prefix.share >= EARLY_COMMIT_MINIMUM_SHARE {
                let replace = if proposal.is_empty() {
                    true
                } else {
                    let prefix_chars = prefix.text_char_count;
                    if prefix_chars != proposal_chars {
                        prefix_chars > proposal_chars
                    } else if prefix.share != proposal_share {
                        prefix.share > proposal_share
                    } else {
                        prefix.raw_length < proposal_raw_length
                    }
                };
                if replace {
                    proposal = prefix.text.clone();
                    proposal_share = prefix.share;
                    proposal_raw_length = prefix.raw_length;
                    proposal_chars = prefix.text_char_count;
                }
            }
        }
        let by_boundary = prefix_lookup(&prefixes);
        Ok(Evidence {
            prefixes,
            by_boundary,
            proposal,
            proposal_share,
            raw_lengths,
            neutral_incomplete_tail: visible.is_empty() && merged_incomplete_tail,
            merged_incomplete_tail,
            neutral_low_confidence: has_low_confidence_completed_generation(&visible),
            confidence_truncated: truncated,
        })
    }
}

/// 参照 `build_prefix_evidence`：按 (前缀文本, raw 边界) 汇总证据。
/// 基础权重（模型置信度）与早提交权重（个性化置信度）各算一套份额。
fn build_prefix_evidence(pool: &[EvidenceCandidate], arena: &[State]) -> Vec<PrefixEvidence> {
    if pool.is_empty() {
        return Vec::new();
    }
    let base_max = pool
        .iter()
        .map(|candidate| candidate.confidence_score)
        .fold(f64::NEG_INFINITY, f64::max);
    let early_max = pool
        .iter()
        .map(EvidenceCandidate::early_confidence)
        .fold(f64::NEG_INFINITY, f64::max);
    let base_weights: Vec<f64> = pool
        .iter()
        .map(|candidate| (candidate.confidence_score - base_max).exp())
        .collect();
    let early_weights: Vec<f64> = pool
        .iter()
        .map(|candidate| (candidate.early_confidence() - early_max).exp())
        .collect();
    let base_total: f64 = base_weights.iter().sum();
    let early_total: f64 = early_weights.iter().sum();
    if base_total <= 0.0 || early_total <= 0.0 {
        return Vec::new();
    }
    let mut entries: Vec<PrefixEvidence> = Vec::new();
    let mut entry_base_weights: Vec<f64> = Vec::new();
    let mut entry_early_weights: Vec<f64> = Vec::new();
    let mut entry_index: HashMap<(usize, String), usize> = HashMap::new();
    let mut base_boundary_mass: HashMap<usize, f64> = HashMap::new();
    for (position, item) in pool.iter().enumerate() {
        let base_weight = base_weights[position];
        let early_weight = early_weights[position];
        let mut current = Some(item.path);
        while let Some(index) = current {
            let prefix_text = arena[index].text.clone();
            if !prefix_text.is_empty() && prefix_text.len() <= item.text.len() {
                let raw_length = arena[index].raw_length;
                let key = (raw_length, prefix_text.clone());
                let slot = match entry_index.get(&key) {
                    Some(&slot) => slot,
                    None => {
                        let slot = entries.len();
                        entries.push(PrefixEvidence {
                            text: prefix_text,
                            raw_length,
                            share: 0.0,
                            base_share: 0.0,
                            boundary_share: 0.0,
                            boundary_closed: false,
                            text_char_count: 0,
                        });
                        entry_base_weights.push(0.0);
                        entry_early_weights.push(0.0);
                        entry_index.insert(key, slot);
                        slot
                    }
                };
                entry_base_weights[slot] += base_weight;
                entry_early_weights[slot] += early_weight;
                *base_boundary_mass.entry(raw_length).or_insert(0.0) += base_weight;
            }
            current = arena[index].previous;
        }
    }
    for (position, entry) in entries.iter_mut().enumerate() {
        let boundary = base_boundary_mass
            .get(&entry.raw_length)
            .copied()
            .unwrap_or(0.0);
        let boundary_share = boundary / base_total;
        entry.share = entry_early_weights[position] / early_total;
        entry.base_share = entry_base_weights[position] / base_total;
        entry.boundary_share = boundary_share;
        entry.boundary_closed = boundary_share >= EARLY_COMMIT_CLOSED_BOUNDARY_SHARE;
        entry.text_char_count = entry.text.chars().count();
    }
    entries
}

fn prefix_lookup(prefixes: &[PrefixEvidence]) -> Map<usize, Map<String, usize>> {
    let mut lookup: Map<usize, Map<String, usize>> = Map::new();
    for (index, prefix) in prefixes.iter().enumerate() {
        lookup
            .entry_or_default(prefix.raw_length)
            .insert(prefix.text.clone(), index);
    }
    lookup
}

/// 参照 `has_low_confidence_completed_generation`。
fn has_low_confidence_completed_generation(candidates: &[&Evaluated]) -> bool {
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
