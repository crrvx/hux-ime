// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 空码候选捕获：未截断池的强置信判定、候选资格判据与待定候选的选取。

use super::super::*;
use super::decode_lock;

/// 参照 `strong_empty_code_candidate`：未截断池中的强置信候选
/// （阈值取排序先验的 `empty_code_strong_share`，比普通强阈值更严）。
pub(crate) fn strong_empty_code_candidate(
    eligible: &[&Evaluated],
    candidate_index: usize,
    visible_top: Option<&str>,
    pool_truncated: bool,
    empty_code_strong_share: f64,
) -> bool {
    if pool_truncated {
        return false;
    }
    let Some(top) = visible_top else {
        return false;
    };
    if eligible[candidate_index].text != top {
        return false;
    }
    let max_score = eligible
        .iter()
        .map(|candidate| candidate.confidence_score)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut total = 0.0;
    let mut candidate_mass = 0.0;
    for (index, candidate) in eligible.iter().enumerate() {
        let mass = (candidate.confidence_score - max_score).exp();
        total += mass;
        if index == candidate_index {
            candidate_mass += mass;
        }
    }
    total > 0.0 && candidate_mass / total >= empty_code_strong_share
}

/// 参照 `capture_empty_code_candidate` 内的 `is_eligible` 闭包。
fn empty_code_eligible(
    candidate: &Evaluated,
    restrict: bool,
    allow_duplicate_single: bool,
) -> bool {
    let previous_nonempty = candidate
        .previous_text
        .as_deref()
        .map(|text| !text.is_empty())
        .unwrap_or(false);
    !restrict
        || candidate.max_rank <= 1
        || (allow_duplicate_single && (previous_nonempty || candidate.text.chars().count() == 1))
}

/// 参照 `capture_empty_code_candidate` 的候选定位：首个合格候选的下标、
/// 置信池中全部合格候选，及其在其中的下标。
fn empty_code_pool(
    decoded: &crate::decode::DecodeOutput,
    restrict: bool,
    allow_duplicate_single: bool,
) -> Option<(usize, Vec<&Evaluated>, usize)> {
    let first_index = decoded
        .items
        .iter()
        .position(|candidate| empty_code_eligible(candidate, restrict, allow_duplicate_single))?;
    let eligible: Vec<&Evaluated> = decoded
        .confidence_candidates
        .iter()
        .filter(|candidate| empty_code_eligible(candidate, restrict, allow_duplicate_single))
        .collect();
    // 置信池中找不到对应项时按参照语义拒绝（该候选质量按 0 计），不静默取首项。
    let candidate_index = eligible.iter().position(|candidate| {
        candidate.path == decoded.items[first_index].path
            && candidate.text == decoded.items[first_index].text
    })?;
    Some((first_index, eligible, candidate_index))
}

/// 参照 `capture_empty_code_candidate`。
pub fn capture_empty_code_candidate(
    decoder: &mut Decoder,
    full_before: &[u8],
    committed_text: &str,
    allow_duplicate_single: bool,
    lock: Option<&Lock>,
) -> anyhow::Result<Option<EmptyCodePending>> {
    let raw = String::from_utf8_lossy(full_before).into_owned();
    decoder.set_allow_duplicate_single(allow_duplicate_single);
    let lock = lock.map(decode_lock);
    let decoded = decoder.decode_with_lock(&raw, false, committed_text, lock)?;
    if decoded.items.is_empty() || decoded.learning_affected {
        return Ok(None);
    }
    let visible_top = decoded
        .items
        .first()
        .map(|candidate| candidate.text.clone());
    let restrict = !has_selection_suffix(full_before);
    let Some((first_index, eligible, candidate_index)) =
        empty_code_pool(&decoded, restrict, allow_duplicate_single)
    else {
        return Ok(None);
    };
    let first = &decoded.items[first_index];
    if first.text.is_empty()
        || !first.text.starts_with(committed_text)
        || first.text.len() <= committed_text.len()
    {
        return Ok(None);
    }
    let pool_truncated = decoded.evidence.confidence_truncated;
    let empty_code_strong_share = decoder.ranking_prior_parameters().empty_code_strong_share;
    if eligible.len() > 1
        && !strong_empty_code_candidate(
            &eligible,
            candidate_index,
            visible_top.as_deref(),
            pool_truncated,
            empty_code_strong_share,
        )
    {
        return Ok(None);
    }
    Ok(Some(EmptyCodePending {
        candidate_text: first.text.clone(),
        requires_uniqueness_check: eligible.len() == 1,
        committed_text: committed_text.to_string(),
        base_raw_length: full_before.len(),
        last_segment_start: first.previous_raw_length,
    }))
}
