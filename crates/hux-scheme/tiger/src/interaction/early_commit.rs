// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// 强证据（`strong_count`）看**纯模型**份额 `base_share`；个性化不再制造强证据。
pub(crate) const EARLY_COMMIT_STRONG_SHARE: f64 = 0.999;
pub(crate) const EARLY_COMMIT_REQUIRED_EVIDENCE: usize = 3;
pub(crate) const EARLY_COMMIT_REQUIRED_STRONG: usize = 2;
pub(crate) const EARLY_COMMIT_MAXIMUM_NEUTRAL_GAP: usize = 3;
pub(crate) const EARLY_COMMIT_RETAINED_RAW_LENGTH: usize = 3;

/// 参照 `reset_early_evidence`。
pub fn reset_early_evidence(state: &mut SentenceState) {
    state.trackers.clear();
    state.last_seen_raw.clear();
}

/// 参照 `has_selection_suffix`：显式选重后缀（分号/引号/数字）。
pub fn has_selection_suffix(raw: &[u8]) -> bool {
    raw.iter()
        .any(|byte| *byte == b';' || *byte == b'\'' || byte.is_ascii_digit())
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

/// 参照 `implicit_rank_allowed`：空码提交后的续接只放宽到合法隐式路径。
pub fn implicit_rank_allowed(
    candidate: &Evaluated,
    raw: &[u8],
    continuation_after_auto_commit: bool,
    allow_duplicate_single: bool,
) -> bool {
    if !continuation_after_auto_commit {
        return true;
    }
    let previous_nonempty = candidate
        .previous_text
        .as_deref()
        .map(|text| !text.is_empty())
        .unwrap_or(false);
    has_selection_suffix(raw)
        || candidate.max_rank <= 1
        || (allow_duplicate_single && previous_nonempty)
}

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
    let lock = lock.map(|lock| DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    });
    let decoded = decoder.decode_with_lock(&raw, false, committed_text, lock)?;
    if decoded.items.is_empty() || decoded.learning_affected {
        return Ok(None);
    }
    let visible_top = decoded
        .items
        .first()
        .map(|candidate| candidate.text.clone());
    let restrict = !has_selection_suffix(full_before);
    let is_eligible = |candidate: &Evaluated| {
        let previous_nonempty = candidate
            .previous_text
            .as_deref()
            .map(|text| !text.is_empty())
            .unwrap_or(false);
        !restrict
            || candidate.max_rank <= 1
            || (allow_duplicate_single
                && (previous_nonempty || candidate.text.chars().count() == 1))
    };
    let Some(first_index) = decoded.items.iter().position(is_eligible) else {
        return Ok(None);
    };
    let eligible: Vec<&Evaluated> = decoded
        .confidence_candidates
        .iter()
        .filter(|candidate| is_eligible(candidate))
        .collect();
    // 置信池中找不到对应项时按参照语义拒绝（该候选质量按 0 计），不静默取首项。
    let Some(candidate_index) = eligible.iter().position(|candidate| {
        candidate.path == decoded.items[first_index].path
            && candidate.text == decoded.items[first_index].text
    }) else {
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

/// 参照 `submit_early`：缓冲分支写回缓冲与单锁；否则返回待上屏文本。
pub fn submit_early(
    context: &mut Context,
    state: &mut SentenceState,
    commit: &str,
) -> Option<String> {
    if context.get_option(OPTION_EARLY_COMMIT_TO_PREEDIT) || !state.buffered_text.is_empty() {
        state.buffered_text.push_str(commit);
        state.locks = vec![Lock {
            raw: state.committed_raw.clone(),
            text: state.committed_text.clone(),
            boundaries: format!(
                "{},{};",
                state.committed_raw.len(),
                state.committed_text.len()
            ),
        }];
        state.save(context);
        None
    } else {
        Some(commit.to_string())
    }
}

/// 参照 `auto_commit_matches_visible_top`：置信度（不含末尾排序先验，如词先验/学习重排）
/// 只允许提交与**显示的首选候选**一致的前缀；无显示候选（`None`）时不做该限制
/// （保留不完整尾段合并证据的既有策略）。
pub fn auto_commit_matches_visible_top(visible_top: Option<&str>, text: &str) -> bool {
    visible_top.is_none_or(|top| top.starts_with(text))
}

/// 参照 `try_commit_mature_prefix`：证据成熟则提交选中前缀。
pub fn try_commit_mature_prefix(
    learning: &mut LearningCommit<'_>,
    context: &mut Context,
    state: &mut SentenceState,
    evidence_raw: &[u8],
    min_retained: usize,
    visible_top: Option<&str>,
    dot_armed: &mut bool,
) -> bool {
    let retain = if min_retained > 0 {
        EARLY_COMMIT_RETAINED_RAW_LENGTH.max(min_retained)
    } else {
        EARLY_COMMIT_RETAINED_RAW_LENGTH
    };
    // 哈希表迭代序不确定：按 key 排序后再比较，保证平局时的确定性。
    let mut keys: Vec<&String> = state.trackers.keys().collect();
    keys.sort();
    let committed_text_elements = state.committed_text.chars().count();
    let committed_raw_length = state.committed_raw.len();
    let mut selected: Option<&Tracker> = None;
    for key in keys {
        let tracker = &state.trackers[key];
        if (tracker.evidence_count >= EARLY_COMMIT_REQUIRED_EVIDENCE
            || tracker.strong_count >= EARLY_COMMIT_REQUIRED_STRONG)
            && tracker.raw_length > committed_raw_length
            && tracker.raw_length <= evidence_raw.len()
            && evidence_raw.len()
                - competing_boundary_end(
                    evidence_raw,
                    learning.decoder.lexicon(),
                    committed_raw_length,
                    tracker.raw_length,
                    tracker
                        .text_char_count
                        .saturating_sub(committed_text_elements),
                )
                >= retain
            && tracker.text.len() > state.committed_text.len()
            && tracker.text.starts_with(&state.committed_text)
            && auto_commit_matches_visible_top(visible_top, &tracker.text)
            && selected
                .map(|current| tracker_better(tracker, current))
                .unwrap_or(true)
        {
            selected = Some(tracker);
        }
    }
    let Some(selected) = selected else {
        return false;
    };
    if evidence_raw.len() - state.last_auto_commit_raw_length < EARLY_COMMIT_RETAINED_RAW_LENGTH {
        return false;
    }
    let commit = selected.text[state.committed_text.len()..].to_string();
    if commit.is_empty() {
        return false;
    }
    let selected_text = selected.text.clone();
    let selected_raw_length = selected.raw_length;
    state.committed_text = selected_text.clone();
    state.committed_raw =
        String::from_utf8_lossy(&evidence_raw[..selected_raw_length]).into_owned();
    state.last_auto_commit_raw_length = selected_raw_length;
    state.continuation_after_auto_commit = false;
    reset_early_evidence(state);
    state.save(context);
    if let Some(commit_text) = submit_early(context, state, &commit) {
        learning.commit_with_learning(
            context,
            state,
            &commit_text,
            &selected_text,
            selected_raw_length,
        );
    }
    // 参照：自动上屏文本以数字结尾时重新武装待发小数点（缓冲分支亦然）。
    if ends_with_digit(&commit) {
        *dot_armed = true;
    }
    restore_composition_input(context, &evidence_raw[selected_raw_length..]);
    true
}

/// 提交文本到上下文（组合外直接提交；供 `submit_early` 的非缓冲分支使用）。
pub(crate) fn context_commit(context: &mut Context, text: &str) {
    context.direct_commit(text);
}

/// 提前上屏的共用参数（避免 `too_many_arguments`）。
#[derive(Clone, Copy, Debug)]
pub struct EarlyCommitParams {
    pub allow_duplicate_single: bool,
    pub generation: u64,
    pub min_retained: usize,
}

/// 参照 `try_early_commit`：证据驱动的前缀提前上屏。
pub fn try_early_commit(
    learning: &mut LearningCommit<'_>,
    context: &mut Context,
    state: &mut SentenceState,
    params: EarlyCommitParams,
    dot_armed: &mut bool,
) -> anyhow::Result<bool> {
    let live_raw = live_input(context);
    if input_caret(context) != live_raw.len()
        || !context.get_option(OPTION_EARLY_COMMIT)
        || state.suspended
    {
        reset_early_evidence(state);
        return Ok(false);
    }
    let mut full_raw = state.committed_raw.as_bytes().to_vec();
    full_raw.extend_from_slice(&live_raw);
    if full_raw.len() <= 4 {
        reset_early_evidence(state);
        return Ok(false);
    }
    let raw = String::from_utf8_lossy(&full_raw).into_owned();
    learning
        .decoder
        .set_allow_duplicate_single(params.allow_duplicate_single);
    let lock = state.active_lock().map(|lock| DecodeLock {
        raw: &lock.raw,
        text: &lock.text,
        boundaries: &lock.boundaries,
    });
    let decoded = learning
        .decoder
        .decode_with_lock(&raw, true, &state.committed_text, lock)?;
    // 防御：调用方代次与当前状态不一致时重同步（processor 构造时取同值，通常为假）。
    if params.generation != state.model_generation {
        state.synchronize_model_state(params.generation);
        return Ok(false);
    }
    // 学习会改变哪些路径留在 beam 里；这种池再截断时连纯模型 BaseShare 也
    // 可能被条件性抬高，故一并拒绝。
    let truncated = decoded.evidence.confidence_truncated;
    if decoded.learning_affected && truncated {
        reset_early_evidence(state);
        return Ok(false);
    }
    let evidence_raw = full_raw;
    // 置信度有意不含末尾排序先验；它只能授权「与末尾排名后的显示首选一致」的前缀。
    // nil（无显示候选）保留不完整尾段合并证据的既有策略。
    let visible_top = decoded
        .items
        .first()
        .map(|candidate| candidate.text.clone());

    if state.last_seen_raw == raw {
        return Ok(try_commit_mature_prefix(
            learning,
            context,
            state,
            &evidence_raw,
            params.min_retained,
            visible_top.as_deref(),
            dot_armed,
        ));
    }

    let extends_previous_generation = state.last_seen_raw.is_empty()
        || (evidence_raw.len() == state.last_seen_raw.len() + 1
            && raw.starts_with(&state.last_seen_raw));
    if !extends_previous_generation {
        state.trackers.clear();
    }
    state.last_seen_raw = raw;

    let merged_incomplete_tail = decoded.evidence.merged_incomplete_tail;
    let mut qualifying: HashMap<String, &crate::decode::PrefixEvidence> = HashMap::new();
    for prefix in &decoded.evidence.prefixes {
        // 截断池下只接受纯模型份额已达强阈值的证据（个性化不得制造强证据）。
        let base_share = prefix.base_share;
        if !prefix.text.is_empty()
            && prefix.boundary_closed
            && prefix.share >= crate::decode::EARLY_COMMIT_MINIMUM_SHARE
            && (!truncated || base_share >= EARLY_COMMIT_STRONG_SHARE)
            && prefix.raw_length > state.committed_raw.len()
            && prefix.text.len() > state.committed_text.len()
            && prefix.text.starts_with(&state.committed_text)
            && auto_commit_matches_visible_top(visible_top.as_deref(), &prefix.text)
            && (merged_incomplete_tail
                || decoded
                    .visible_prefixes
                    .contains(&(prefix.raw_length, prefix.text.clone())))
        {
            qualifying.insert(
                format!("{}{}{}", prefix.text, STATE_SEPARATOR, prefix.raw_length),
                prefix,
            );
        }
    }

    let retain_without_counting = qualifying.is_empty()
        && (decoded.evidence.neutral_low_confidence || merged_incomplete_tail);
    if retain_without_counting {
        // 参照 `d30867a`：比较型缺口按 evidence 的低置信度决定是否清零成熟度。
        state.trackers = retain_trackers_without_counting(
            &state.trackers,
            &decoded.evidence,
            decoded.evidence.neutral_low_confidence,
        );
        return Ok(try_commit_mature_prefix(
            learning,
            context,
            state,
            &evidence_raw,
            params.min_retained,
            visible_top.as_deref(),
            dot_armed,
        ));
    }

    let mut next_trackers: Map<String, Tracker> = Map::new();
    for (key, prefix) in qualifying {
        let mut tracker = state.trackers.get(&key).cloned().unwrap_or(Tracker {
            text: prefix.text.clone(),
            text_char_count: prefix.text_char_count,
            raw_length: prefix.raw_length,
            evidence_count: 0,
            strong_count: 0,
            gap_count: 0,
            last_share: 0.0,
        });
        tracker.evidence_count = EARLY_COMMIT_REQUIRED_EVIDENCE.min(tracker.evidence_count + 1);
        // 强证据判定同样只看纯模型份额。
        tracker.strong_count = if prefix.base_share >= EARLY_COMMIT_STRONG_SHARE {
            EARLY_COMMIT_REQUIRED_STRONG.min(tracker.strong_count + 1)
        } else {
            0
        };
        tracker.gap_count = 0;
        tracker.last_share = prefix.share;
        next_trackers.insert(key, tracker);
    }
    state.trackers = next_trackers;
    Ok(try_commit_mature_prefix(
        learning,
        context,
        state,
        &evidence_raw,
        params.min_retained,
        visible_top.as_deref(),
        dot_armed,
    ))
}

/// 参照 `try_empty_code_commit`：空码（整句唯一候选）自动上屏。
pub fn try_empty_code_commit(
    learning: &mut LearningCommit<'_>,
    context: &mut Context,
    state: &mut SentenceState,
    full_before: &[u8],
    appended_letter: &[u8],
    params: EarlyCommitParams,
    dot_armed: &mut bool,
) -> anyhow::Result<bool> {
    if !context.get_option(OPTION_EARLY_COMMIT) || state.suspended {
        state.empty_code_pending = None;
        return Ok(false);
    }
    // 防御：模型代次变化时重置瞬态（同上，通常为假）。
    if state.synchronize_model_state(params.generation) {
        return Ok(false);
    }
    let pending = match state.empty_code_pending.clone() {
        Some(pending) => Some(pending),
        None => capture_empty_code_candidate(
            learning.decoder,
            full_before,
            &state.committed_text,
            params.allow_duplicate_single,
            state.active_lock(),
        )?,
    };
    let mut full_raw = state.committed_raw.as_bytes().to_vec();
    full_raw.extend_from_slice(&live_input(context));
    let mut expected = full_before.to_vec();
    expected.extend_from_slice(appended_letter);
    if full_raw != expected || input_caret(context) != live_input(context).len() {
        state.empty_code_pending = None;
        return Ok(false);
    }
    state.empty_code_pending = pending.clone();

    let Some(pending) = pending else {
        return Ok(false);
    };
    let raw = String::from_utf8_lossy(&full_raw).into_owned();
    if crate::decode::has_complete_candidate(
        learning.decoder.lexicon(),
        &raw,
        &state.committed_text,
        None,
        false,
        params.allow_duplicate_single,
        state
            .active_lock()
            .map(|lock| DecodeLock {
                raw: &lock.raw,
                text: &lock.text,
                boundaries: &lock.boundaries,
            })
            .as_ref(),
    ) {
        state.empty_code_pending = None;
        return Ok(false);
    }
    if pending.committed_text != state.committed_text
        || pending.base_raw_length >= full_raw.len()
        || pending.last_segment_start >= full_raw.len()
    {
        state.empty_code_pending = None;
        return Ok(false);
    }
    let extended_last_segment =
        String::from_utf8_lossy(&full_raw[pending.last_segment_start..]).into_owned();
    if learning
        .decoder
        .lexicon()
        .proper_code_prefixes
        .contains(&extended_last_segment)
    {
        return Ok(false);
    }
    // 参照 `d30867a`：保留量边界改按竞争切分的前瞻保护边界计算
    // （`pending.candidate_text:sub(#committed_text + 1)` 的字符数即目标字素数）。
    let pending_commit = pending
        .candidate_text
        .get(pending.committed_text.len()..)
        .unwrap_or_default();
    let protected_boundary = competing_boundary_end(
        &full_raw,
        learning.decoder.lexicon(),
        state.committed_raw.len(),
        pending.base_raw_length,
        pending_commit.chars().count(),
    );
    if params.min_retained > 0 && full_raw.len() - protected_boundary < params.min_retained {
        return Ok(false);
    }
    if pending.requires_uniqueness_check
        && crate::decode::has_complete_candidate(
            learning.decoder.lexicon(),
            &String::from_utf8_lossy(&full_raw[..pending.base_raw_length]),
            &pending.committed_text,
            Some(&pending.candidate_text),
            true,
            params.allow_duplicate_single,
            state
                .active_lock()
                .map(|lock| DecodeLock {
                    raw: &lock.raw,
                    text: &lock.text,
                    boundaries: &lock.boundaries,
                })
                .as_ref(),
        )
    {
        state.empty_code_pending = None;
        return Ok(false);
    }
    let commit = pending.candidate_text[pending.committed_text.len()..].to_string();
    let retained_raw = full_raw[pending.base_raw_length..].to_vec();
    state.committed_text = pending.candidate_text.clone();
    state.committed_raw =
        String::from_utf8_lossy(&full_raw[..pending.base_raw_length]).into_owned();
    state.last_auto_commit_raw_length = pending.base_raw_length;
    state.trackers.clear();
    state.last_seen_raw.clear();
    state.suspended = false;
    state.empty_code_pending = None;
    state.continuation_after_auto_commit = true;
    // 参照顺序：submit_early → 数字结尾时重武装小数点 → save_sentence_state → restore
    // （缓冲分支在 submit_early 内部已保存一次，幂等）。
    if let Some(commit_text) = submit_early(context, state, &commit) {
        learning.commit_with_learning(
            context,
            state,
            &commit_text,
            &pending.candidate_text,
            pending.base_raw_length,
        );
    }
    if ends_with_digit(&commit) {
        *dot_armed = true;
    }
    state.save(context);
    restore_composition_input(context, &retained_raw);
    Ok(true)
}

/// 参照 `get_min_retained_raw_length`：由配置提供的下限（缺失/非法为 0）。
pub fn min_retained_raw_length(value: Option<i64>) -> usize {
    match value {
        Some(number) if number >= 0 => number as usize,
        _ => 0,
    }
}
