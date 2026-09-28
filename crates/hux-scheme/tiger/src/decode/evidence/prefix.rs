// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 前缀证据：按（前缀文本，raw 边界）汇总基础与早提交两套份额。

use super::*;

/// 证据池的两套归一化权重与总量。
struct EvidenceWeights {
    /// 基础权重的逐候选值。
    base: Vec<f64>,
    /// 早提交权重的逐候选值。
    early: Vec<f64>,
    /// 基础权重总量。
    base_total: f64,
    /// 早提交权重总量。
    early_total: f64,
}

/// 计算证据池的归一化权重；池为空或总量非正时返回 `None`。
fn evidence_weights(pool: &[EvidenceCandidate]) -> Option<EvidenceWeights> {
    if pool.is_empty() {
        return None;
    }
    let base_max = pool
        .iter()
        .map(|candidate| candidate.confidence_score)
        .fold(f64::NEG_INFINITY, f64::max);
    let early_max = pool
        .iter()
        .map(EvidenceCandidate::early_confidence)
        .fold(f64::NEG_INFINITY, f64::max);
    let base: Vec<f64> = pool
        .iter()
        .map(|candidate| (candidate.confidence_score - base_max).exp())
        .collect();
    let early: Vec<f64> = pool
        .iter()
        .map(|candidate| (candidate.early_confidence() - early_max).exp())
        .collect();
    let base_total: f64 = base.iter().sum();
    let early_total: f64 = early.iter().sum();
    if base_total <= 0.0 || early_total <= 0.0 {
        return None;
    }
    Some(EvidenceWeights {
        base,
        early,
        base_total,
        early_total,
    })
}

/// 前缀证据累加器：沿 arena 链按（raw 边界，前缀文本）汇总两套权重。
struct EvidenceAccumulator {
    /// 已累计的前缀条目。
    entries: Vec<PrefixEvidence>,
    /// 每条目的基础权重。
    entry_base_weights: Vec<f64>,
    /// 每条目的早提交权重。
    entry_early_weights: Vec<f64>,
    /// （raw 边界，前缀文本）到条目下标的索引。
    entry_index: HashMap<(usize, String), usize>,
    /// 每个 raw 边界的基础总权重。
    base_boundary_mass: HashMap<usize, f64>,
}

impl EvidenceAccumulator {
    /// 空累加器。
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            entry_base_weights: Vec::new(),
            entry_early_weights: Vec::new(),
            entry_index: HashMap::new(),
            base_boundary_mass: HashMap::new(),
        }
    }

    /// 沿一条候选的 arena 链累加权重。
    fn add_chain(
        &mut self,
        item: &EvidenceCandidate,
        base_weight: f64,
        early_weight: f64,
        arena: &[State],
    ) {
        let mut current = Some(item.path);
        while let Some(index) = current {
            let prefix_text = arena[index].text.clone();
            if !prefix_text.is_empty() && prefix_text.len() <= item.text.len() {
                let raw_length = arena[index].raw_length;
                let key = (raw_length, prefix_text.clone());
                let slot = match self.entry_index.get(&key) {
                    Some(&slot) => slot,
                    None => {
                        let slot = self.entries.len();
                        self.entries.push(PrefixEvidence {
                            text: prefix_text,
                            raw_length,
                            share: 0.0,
                            base_share: 0.0,
                            boundary_share: 0.0,
                            boundary_closed: false,
                            text_char_count: 0,
                        });
                        self.entry_base_weights.push(0.0);
                        self.entry_early_weights.push(0.0);
                        self.entry_index.insert(key, slot);
                        slot
                    }
                };
                self.entry_base_weights[slot] += base_weight;
                self.entry_early_weights[slot] += early_weight;
                *self.base_boundary_mass.entry(raw_length).or_insert(0.0) += base_weight;
            }
            current = arena[index].previous;
        }
    }

    /// 依权重总量结算份额与边界闭合标记。
    fn finish(self, weights: &EvidenceWeights) -> Vec<PrefixEvidence> {
        let mut entries = self.entries;
        for (position, entry) in entries.iter_mut().enumerate() {
            let boundary = self
                .base_boundary_mass
                .get(&entry.raw_length)
                .copied()
                .unwrap_or(0.0);
            let boundary_share = boundary / weights.base_total;
            entry.share = self.entry_early_weights[position] / weights.early_total;
            entry.base_share = self.entry_base_weights[position] / weights.base_total;
            entry.boundary_share = boundary_share;
            entry.boundary_closed = boundary_share >= EARLY_COMMIT_CLOSED_BOUNDARY_SHARE;
            entry.text_char_count = entry.text.chars().count();
        }
        entries
    }
}

/// 参照 `build_prefix_evidence`：按 (前缀文本, raw 边界) 汇总证据。
/// 基础权重（模型置信度）与早提交权重（个性化置信度）各算一套份额。
pub(super) fn build_prefix_evidence(
    pool: &[EvidenceCandidate],
    arena: &[State],
) -> Vec<PrefixEvidence> {
    let Some(weights) = evidence_weights(pool) else {
        return Vec::new();
    };
    let mut accumulator = EvidenceAccumulator::new();
    for (position, item) in pool.iter().enumerate() {
        accumulator.add_chain(item, weights.base[position], weights.early[position], arena);
    }
    accumulator.finish(&weights)
}

pub(super) fn prefix_lookup(prefixes: &[PrefixEvidence]) -> Map<usize, Map<String, usize>> {
    let mut lookup: Map<usize, Map<String, usize>> = Map::new();
    for (index, prefix) in prefixes.iter().enumerate() {
        lookup
            .entry_or_default(prefix.raw_length)
            .insert(prefix.text.clone(), index);
    }
    lookup
}
