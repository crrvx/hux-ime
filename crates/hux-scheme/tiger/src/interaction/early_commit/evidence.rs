// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 证据与追踪器判据：公共前缀、竞争切分边界、成熟度保留与 tracker 择优。

use super::super::*;

/// 参照 `reset_early_evidence`。
pub fn reset_early_evidence(state: &mut SentenceState) {
    state.trackers.clear();
    state.last_seen_raw.clear();
}

/// 参照 `common_text_prefix`：逐字符公共前缀。
pub fn common_text_prefix(left: &str, right: &str) -> String {
    let mut out = String::new();
    for (a, b) in left.chars().zip(right.chars()) {
        if a != b {
            break;
        }
        out.push(a);
    }
    out
}

/// 参照 `prefix_extends`：互为字节前缀。
pub(crate) fn prefix_extends(left: &str, right: &str) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

/// 参照 `prefix_contradicted`。
pub(crate) fn prefix_contradicted(tracker: &Tracker, evidence: &Evidence) -> bool {
    if evidence.prefixes.is_empty() {
        return false;
    }
    let own = evidence.find(&tracker.text, tracker.raw_length);
    let self_share = own.map(|prefix| prefix.share).unwrap_or(0.0);
    for prefix in &evidence.prefixes {
        if !prefix.text.is_empty()
            && prefix.text != tracker.text
            && !prefix_extends(&prefix.text, &tracker.text)
        {
            let shared = common_text_prefix(&prefix.text, &tracker.text);
            if !shared.is_empty()
                && shared.len() < tracker.text.len()
                && (own.is_none() || prefix.share > self_share)
            {
                return true;
            }
        }
    }
    false
}

/// 参照 `retain_trackers_without_counting`。
///
/// `reset_maturity`（`d30867a` 新增）为真时把保留下来的 tracker 的
/// `evidence_count`/`strong_count` 清零：低置信度缺口只能保住「身份」，
/// 不能把缺口前的成熟度带到缺口之后。
pub(crate) fn retain_trackers_without_counting(
    trackers: &Map<String, Tracker>,
    evidence: &Evidence,
    reset_maturity: bool,
) -> Map<String, Tracker> {
    let mut next = Map::new();
    for (key, tracker) in trackers.iter() {
        let Some(current) = evidence.find(&tracker.text, tracker.raw_length) else {
            continue;
        };
        if prefix_contradicted(tracker, evidence) {
            continue;
        }
        let mut tracker = tracker.clone();
        tracker.gap_count += 1;
        if tracker.gap_count <= EARLY_COMMIT_MAXIMUM_NEUTRAL_GAP {
            tracker.last_share = current.share;
            if reset_maturity {
                tracker.evidence_count = 0;
                tracker.strong_count = 0;
            }
            next.insert(key.clone(), tracker);
        }
    }
    next
}

/// 参照 `competing_boundary_end`（`d30867a`）：竞争切分的前瞻保护边界。
///
/// 保留量必须按**已输出的文本元素数**对齐比较，而不是按同一个 raw 边界：
/// 在 `nv` 提交 `有` 时，也必须等到 `nvt` 处的一字替代 `郁` 攒够 K 个 raw 键的
/// 后缀证据；而 `nv|tah` 这类已经输出两个字素的第二条边不得拖延一字提交。
///
/// 返回「从 `committed_raw_length` 起，用码表分段恰好凑出 `target_text_elements`
/// 个字素时能到达的最远 raw 边界」与 `proposed_raw_length` 的较大者。
/// 参数非法（含 `target_text_elements < 1`）时原样返回 `proposed_raw_length`。
pub(crate) fn competing_boundary_end(
    raw: &[u8],
    lexicon: &Lexicon,
    committed_raw_length: usize,
    proposed_raw_length: usize,
    target_text_elements: usize,
) -> usize {
    if proposed_raw_length <= committed_raw_length
        || proposed_raw_length > raw.len()
        || target_text_elements < 1
    {
        return proposed_raw_length;
    }
    let Ok(text) = std::str::from_utf8(raw) else {
        return proposed_raw_length;
    };
    // `reachable[start]` = 从 committed 起点分段走到 start 时，已输出字素数的集合。
    let mut reachable: HashMap<usize, HashSet<usize>> = HashMap::new();
    reachable.insert(committed_raw_length, HashSet::from([0]));
    let mut furthest = proposed_raw_length;
    for start in committed_raw_length..text.len() {
        let Some(counts) = reachable.get(&start) else {
            continue;
        };
        let counts: Vec<usize> = counts.iter().copied().collect();
        let maximum = lexicon.max_code_len.min(text.len() - start);
        for length in 1..=maximum {
            let finish = start + length;
            let Some(entries) = text
                .get(start..finish)
                .and_then(|code| lexicon.codes.get(code))
            else {
                continue;
            };
            for count in &counts {
                for entry in entries {
                    let next_count = count + entry.text.chars().count();
                    if next_count == target_text_elements {
                        furthest = furthest.max(finish);
                    } else if next_count < target_text_elements {
                        reachable.entry(finish).or_default().insert(next_count);
                    }
                }
            }
        }
    }
    furthest
}

/// 参照 `tracker_better`：字符数 → `last_share` → 保留长度。
///
/// 参照在该三元组**全等**时对两者都返回 false ⇒ 胜者取决于 `pairs(state.trackers)` 的
/// 哈希迭代序（不确定）。本仓额外按 `text` 字典序兜底：
/// 调用点的 key 已排序，兜底与「首个更优者获胜」的结果一致，但把确定性写进判据本身，
/// 不再依赖调用点的排序。差异仅在「两个 tracker 三元全等」时可见，现有金样未触发。
pub(crate) fn tracker_better(left: &Tracker, right: &Tracker) -> bool {
    if left.text_char_count != right.text_char_count {
        return left.text_char_count > right.text_char_count;
    }
    if left.last_share != right.last_share {
        return left.last_share > right.last_share;
    }
    if left.raw_length != right.raw_length {
        return left.raw_length < right.raw_length;
    }
    left.text < right.text
}
