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
        return raw_is_reachable(&raw, lexicon, allow_duplicate_single);
    }
    constrained_complete(
        &raw,
        lexicon,
        required,
        excluded_text,
        group_eligible_only,
        allow_duplicate_single,
        lock,
    )
}

/// 无附加约束时的快速判定：布尔前沿能否覆盖整串输入。
fn raw_is_reachable(raw: &[u8], lexicon: &Lexicon, allow_duplicate_single: bool) -> bool {
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
            let Some((consumed_end, selected)) =
                edge_candidates(raw, position, code_length, lexicon, allow_duplicate_single)
            else {
                continue;
            };
            if !selected.is_empty() {
                reachable[consumed_end] = true;
            }
        }
    }
    reachable[raw.len()]
}

/// 约束扫描的起点：输入位移、已匹配前缀长度与已排除文本长度。
struct ScanStart {
    /// 扫描从此位移开始。
    position: usize,
    /// 已匹配的必配前缀长度。
    matched: usize,
    /// 已排除文本的进度（等于排除文本长度表示已失败）。
    excluded: usize,
}

impl ScanStart {
    /// 起点已在输入末端时的判定：前缀必须配齐且排除文本未命中。
    fn complete(&self, required: &str, excluded_text: Option<&str>) -> bool {
        self.matched == required.len()
            && excluded_text
                .map(|text| self.excluded != text.len())
                .unwrap_or(true)
    }
}

/// 约束扫描的只读上下文：输入、必配前缀、排除文本与资格开关。
struct ScanStream<'a> {
    /// 归一化输入。
    raw: &'a [u8],
    /// 必配文本前缀。
    required: &'a str,
    /// 排除文本（命中即失败）。
    excluded_text: Option<&'a str>,
    /// 打包步长（`excluded_text.len() + 2`）。
    stride: usize,
    /// 是否只接受首位候选。
    first_ranks_only: bool,
    /// 是否允许重复单字。
    allow_duplicate_single: bool,
}

/// 校验锁前缀（须同时匹配输入与已确认文本）并求扫描起点；不匹配时返回 `None`。
fn scan_start(
    raw: &[u8],
    required: &str,
    excluded_text: Option<&str>,
    lock: Option<&DecodeLock<'_>>,
) -> Option<ScanStart> {
    let mut start = ScanStart {
        position: 0,
        matched: 0,
        excluded: 0,
    };
    if let Some(lock) = lock {
        // 参照：锁前缀必须同时匹配输入与已确认文本，扫描自锁末端开始。
        let prefix = normalize(lock.raw);
        start.matched = required.len().min(lock.text.len());
        if !raw.starts_with(&prefix)
            || required.as_bytes().get(..start.matched) != lock.text.as_bytes().get(..start.matched)
        {
            return None;
        }
        start.position = prefix.len();
        if let Some(excluded_text) = excluded_text {
            start.excluded = if excluded_text.as_bytes().starts_with(lock.text.as_bytes()) {
                lock.text.len()
            } else {
                excluded_text.len() + 1
            };
        }
    }
    Some(start)
}

/// 由位置 `position` 上长度为 `code_length` 的一条码边取可展开候选；
/// 返回（消费末端，候选集），无可用边时返回 `None`。
fn edge_candidates<'a>(
    raw: &[u8],
    position: usize,
    code_length: usize,
    lexicon: &'a Lexicon,
    allow_duplicate_single: bool,
) -> Option<(usize, Vec<&'a CodeEntry>)> {
    let code_end = position + code_length;
    let code = std::str::from_utf8(&raw[position..code_end]).ok()?;
    let candidates = lexicon.codes.get(code)?;
    let (selected_rank, consumed_end) = parse_selector(raw, code_end);
    let whole_input_edge = position == 0 && consumed_end == raw.len();
    if raw.len() > 1 && consumed_end - position < 2 {
        return None;
    }
    Some((
        consumed_end,
        eligible_candidates(
            candidates,
            selected_rank,
            whole_input_edge,
            allow_duplicate_single,
        ),
    ))
}

/// 由一条边推进所有打包状态；命中完整候选（配齐前缀且排除文本未命中）时返回 `true`。
fn advance_scan(
    states: &mut [HashSet<usize>],
    packed_states: &[usize],
    selected: &[&CodeEntry],
    consumed_end: usize,
    stream: &ScanStream<'_>,
) -> bool {
    for &packed in packed_states {
        let matched_length = packed / stream.stride;
        for candidate in selected {
            let Some(next_matched) =
                advance_required_prefix(stream.required, matched_length, &candidate.text)
            else {
                continue;
            };
            if stream.first_ranks_only
                && candidate.rank != 1
                && !(stream.allow_duplicate_single && candidate.text.chars().count() == 1)
            {
                continue;
            }
            let mut next_excluded = packed % stream.stride;
            if let Some(excluded) = stream.excluded_text
                && next_excluded <= excluded.len()
            {
                let tail = &excluded.as_bytes()[next_excluded..];
                if tail.starts_with(candidate.text.as_bytes()) {
                    next_excluded += candidate.text.len();
                } else {
                    next_excluded = excluded.len() + 1;
                }
            }
            if consumed_end == stream.raw.len()
                && next_matched == stream.required.len()
                && stream
                    .excluded_text
                    .map(|text| next_excluded != text.len())
                    .unwrap_or(true)
            {
                return true;
            }
            states[consumed_end].insert(next_matched * stream.stride + next_excluded);
        }
    }
    false
}

/// 有附加约束时的完整判定：锁前缀、文本前缀、排除文本与资格过滤共同约束。
fn constrained_complete(
    raw: &[u8],
    lexicon: &Lexicon,
    required: &str,
    excluded_text: Option<&str>,
    group_eligible_only: bool,
    allow_duplicate_single: bool,
    lock: Option<&DecodeLock<'_>>,
) -> bool {
    let first_ranks_only = group_eligible_only && !has_selection_suffix(raw);
    let stride = excluded_text.map(|text| text.len() + 2).unwrap_or(1);
    let mut states: Vec<HashSet<usize>> = (0..=raw.len()).map(|_| HashSet::new()).collect();
    let start = match scan_start(raw, required, excluded_text, lock) {
        Some(start) => start,
        None => return false,
    };
    if start.position == raw.len() {
        return start.complete(required, excluded_text);
    }
    states[start.position].insert(start.matched * stride + start.excluded);
    let stream = ScanStream {
        raw,
        required,
        excluded_text,
        stride,
        first_ranks_only,
        allow_duplicate_single,
    };
    for position in start.position..raw.len() {
        if states[position].is_empty() {
            continue;
        }
        let packed_states: Vec<usize> = states[position].iter().copied().collect();
        for &code_length in &lexicon.lengths {
            if position + code_length > raw.len() {
                break;
            }
            let Some((consumed_end, selected)) =
                edge_candidates(raw, position, code_length, lexicon, allow_duplicate_single)
            else {
                continue;
            };
            if advance_scan(
                &mut states,
                &packed_states,
                &selected,
                consumed_end,
                &stream,
            ) {
                return true;
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
