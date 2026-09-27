// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 可达性与资格判定：显式 rank / 文本前缀约束下，候选能否从当前输入到达
//! （`has_complete_candidate`、`eligible_candidates`、`advance_required_prefix`）。
//!
//! `has_selection_suffix` 与 beam 侧、交互侧共用同一份后缀判定。

use super::beam::{has_letter, normalize, parse_selector};
use super::*;

/// 参照 `advance_required_prefix`（字节比较）。
fn advance_required_prefix(required: &str, matched: usize, candidate: &str) -> Option<usize> {
    if matched >= required.len() {
        return Some(matched);
    }
    let required_bytes = required.as_bytes();
    let candidate_bytes = candidate.as_bytes();
    let compare = candidate_bytes.len().min(required.len() - matched);
    if compare == 0 || required_bytes[matched..matched + compare] != candidate_bytes[..compare] {
        return None;
    }
    Some(required.len().min(matched + candidate_bytes.len()))
}

/// 参照 `has_selection_suffix` / `has_selection_suffix_bytes`：显式选重后缀（分号/引号/数字）。
///
/// beam 侧与交互侧（`interaction::early_commit`）共用这一份判定；同语法的 `parse_selector`
/// 还要返回消费长度，故保持独立、不合并。
pub(crate) fn has_selection_suffix(raw: &[u8]) -> bool {
    raw.iter()
        .any(|byte| *byte == b';' || *byte == b'\'' || byte.is_ascii_digit())
}

/// 参照 `has_complete_candidate(raw_code, required_text_prefix, excluded_text,
/// group_eligible_only, locked)`。
pub fn has_complete_candidate(
    lexicon: &Lexicon,
    raw_code: &str,
    required_text_prefix: &str,
    excluded_text: Option<&str>,
    group_eligible_only: bool,
    allow_duplicate_single: bool,
    lock: Option<&DecodeLock<'_>>,
) -> bool {
    let raw = normalize(raw_code);
    if raw.is_empty() || !has_letter(&raw) {
        return false;
    }
    let required = required_text_prefix;
    if required.is_empty() && excluded_text.is_none() && !group_eligible_only && lock.is_none() {
        let mut reachable = vec![false; raw.len() + 1];
        reachable[0] = true;
        for position in 0..raw.len() {
            if !reachable[position] {
                continue;
            }
            for &code_length in &lexicon.lengths {
                if position + code_length > raw.len() {
                    break;
                }
                let Ok(code) = std::str::from_utf8(&raw[position..position + code_length]) else {
                    continue;
                };
                let Some(candidates) = lexicon.codes.get(code) else {
                    continue;
                };
                let (selected_rank, consumed_end) = parse_selector(&raw, position + code_length);
                let whole_input_edge = position == 0 && consumed_end == raw.len();
                if raw.len() > 1 && consumed_end - position < 2 {
                    continue;
                }
                if !eligible_candidates(
                    candidates,
                    selected_rank,
                    whole_input_edge,
                    allow_duplicate_single,
                )
                .is_empty()
                {
                    reachable[consumed_end] = true;
                }
            }
        }
        return reachable[raw.len()];
    }

    let first_ranks_only = group_eligible_only && !has_selection_suffix(&raw);
    let stride = excluded_text.map(|text| text.len() + 2).unwrap_or(1);
    let mut states: Vec<HashSet<usize>> = (0..=raw.len()).map(|_| HashSet::new()).collect();
    let mut start = 0usize;
    let mut matched = 0usize;
    let mut excluded = 0usize;
    if let Some(lock) = lock {
        // 参照：锁前缀必须同时匹配输入与已确认文本，扫描自锁末端开始。
        let prefix = normalize(lock.raw);
        matched = required.len().min(lock.text.len());
        if !raw.starts_with(&prefix)
            || required.as_bytes().get(..matched) != lock.text.as_bytes().get(..matched)
        {
            return false;
        }
        start = prefix.len();
        if let Some(excluded_text) = excluded_text {
            excluded = if excluded_text.as_bytes().starts_with(lock.text.as_bytes()) {
                lock.text.len()
            } else {
                excluded_text.len() + 1
            };
        }
        if start == raw.len() {
            return matched == required.len()
                && excluded_text
                    .map(|text| excluded != text.len())
                    .unwrap_or(true);
        }
    }
    states[start].insert(matched * stride + excluded);
    for position in start..raw.len() {
        if states[position].is_empty() {
            continue;
        }
        let packed_states: Vec<usize> = states[position].iter().copied().collect();
        for &code_length in &lexicon.lengths {
            let code_end = position + code_length;
            if code_end > raw.len() {
                break;
            }
            let Ok(code) = std::str::from_utf8(&raw[position..code_end]) else {
                continue;
            };
            let Some(candidates) = lexicon.codes.get(code) else {
                continue;
            };
            let (selected_rank, consumed_end) = parse_selector(&raw, code_end);
            let whole_input_edge = position == 0 && consumed_end == raw.len();
            if raw.len() > 1 && consumed_end - position < 2 {
                continue;
            }
            let selected = eligible_candidates(
                candidates,
                selected_rank,
                whole_input_edge,
                allow_duplicate_single,
            );
            for &packed in &packed_states {
                let matched_length = packed / stride;
                for candidate in &selected {
                    let Some(next_matched) =
                        advance_required_prefix(required, matched_length, &candidate.text)
                    else {
                        continue;
                    };
                    if first_ranks_only
                        && candidate.rank != 1
                        && !(allow_duplicate_single && candidate.text.chars().count() == 1)
                    {
                        continue;
                    }
                    let mut next_excluded = packed % stride;
                    if let Some(excluded) = excluded_text
                        && next_excluded <= excluded.len()
                    {
                        let tail = &excluded.as_bytes()[next_excluded..];
                        if tail.starts_with(candidate.text.as_bytes()) {
                            next_excluded += candidate.text.len();
                        } else {
                            next_excluded = excluded.len() + 1;
                        }
                    }
                    if consumed_end == raw.len()
                        && next_matched == required.len()
                        && excluded_text
                            .map(|text| next_excluded != text.len())
                            .unwrap_or(true)
                    {
                        return true;
                    }
                    states[consumed_end].insert(next_matched * stride + next_excluded);
                }
            }
        }
    }
    false
}

/// 参照 `eligible_candidates`。
pub(super) fn eligible_candidates(
    candidates: &[CodeEntry],
    selected_rank: u64,
    whole_input_edge: bool,
    allow_duplicate_single: bool,
) -> Vec<&CodeEntry> {
    let single = |entry: &CodeEntry| entry.text.chars().count() == 1;
    if candidates.len() == 1 {
        let candidate = &candidates[0];
        if selected_rank > 0 {
            if candidate.rank as u64 == selected_rank {
                return vec![candidate];
            }
        } else if whole_input_edge
            || candidate.rank == 1
            || (allow_duplicate_single && single(candidate))
        {
            return vec![candidate];
        }
    }
    if selected_rank == 0 {
        if whole_input_edge {
            return candidates.iter().collect();
        }
        if allow_duplicate_single {
            return candidates
                .iter()
                .filter(|entry| entry.rank == 1 || single(entry))
                .collect();
        }
    }
    let rank = if selected_rank > 0 { selected_rank } else { 1 };
    candidates
        .iter()
        .filter(|entry| entry.rank as u64 == rank)
        .collect()
}
