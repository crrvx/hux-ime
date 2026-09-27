// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 证据驱动的提前上屏主入口：池级防御、合格前缀收集与 tracker 推进。

use super::super::*;
use super::decode_lock;

/// 参照 `try_early_commit` 的解码段：带锁解码 + 代次重同步防御 + 学习截断防御。
///
/// 返回 `None` 表示已按防御路径处理（必要时已重置证据），调用方直接返回 `Ok(false)`。
fn decode_early_pool(
    learning: &mut LearningCommit<'_>,
    state: &mut SentenceState,
    params: EarlyCommitParams,
    raw: &str,
) -> anyhow::Result<Option<crate::decode::DecodeOutput>> {
    learning
        .decoder
        .set_allow_duplicate_single(params.allow_duplicate_single);
    let lock = state.active_lock().map(decode_lock);
    let decoded = learning
        .decoder
        .decode_with_lock(raw, true, &state.committed_text, lock)?;
    // 防御：调用方代次与当前状态不一致时重同步（processor 构造时取同值，通常为假）。
    if params.generation != state.model_generation {
        state.synchronize_model_state(params.generation);
        return Ok(None);
    }
    // 学习会改变哪些路径留在 beam 里；这种池再截断时连纯模型 BaseShare 也
    // 可能被条件性抬高，故一并拒绝。
    let truncated = decoded.evidence.confidence_truncated;
    if decoded.learning_affected && truncated {
        reset_early_evidence(state);
        return Ok(None);
    }
    Ok(Some(decoded))
}

/// 参照 `try_early_commit` 的合格前缀收集循环。
fn qualifying_prefixes<'a>(
    decoded: &'a crate::decode::DecodeOutput,
    committed_text: &str,
    committed_raw_length: usize,
    truncated: bool,
    merged_incomplete_tail: bool,
    visible_top: Option<&str>,
) -> HashMap<String, &'a crate::decode::PrefixEvidence> {
    let mut qualifying: HashMap<String, &crate::decode::PrefixEvidence> = HashMap::new();
    for prefix in &decoded.evidence.prefixes {
        // 截断池下只接受纯模型份额已达强阈值的证据（个性化不得制造强证据）。
        let base_share = prefix.base_share;
        if !prefix.text.is_empty()
            && prefix.boundary_closed
            && prefix.share >= crate::decode::EARLY_COMMIT_MINIMUM_SHARE
            && (!truncated || base_share >= EARLY_COMMIT_STRONG_SHARE)
            && prefix.raw_length > committed_raw_length
            && prefix.text.len() > committed_text.len()
            && prefix.text.starts_with(committed_text)
            && auto_commit_matches_visible_top(visible_top, &prefix.text)
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
    qualifying
}

/// 参照 `try_early_commit` 的 tracker 推进循环：合格前缀各计一次证据。
fn advance_trackers(
    trackers: &Map<String, Tracker>,
    qualifying: HashMap<String, &crate::decode::PrefixEvidence>,
) -> Map<String, Tracker> {
    let mut next_trackers: Map<String, Tracker> = Map::new();
    for (key, prefix) in qualifying {
        let mut tracker = trackers.get(&key).cloned().unwrap_or(Tracker {
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
    next_trackers
}

/// 参照 `try_early_commit` 的证据推进段：世代延续、合格前缀、缺口保留或 tracker 推进。
fn update_trackers_for_evidence(
    state: &mut SentenceState,
    evidence_raw: &[u8],
    raw: String,
    decoded: &crate::decode::DecodeOutput,
    visible_top: Option<&str>,
) {
    let extends_previous_generation = state.last_seen_raw.is_empty()
        || (evidence_raw.len() == state.last_seen_raw.len() + 1
            && raw.starts_with(&state.last_seen_raw));
    if !extends_previous_generation {
        state.trackers.clear();
    }
    state.last_seen_raw = raw;

    let merged_incomplete_tail = decoded.evidence.merged_incomplete_tail;
    let qualifying = qualifying_prefixes(
        decoded,
        &state.committed_text,
        state.committed_raw.len(),
        decoded.evidence.confidence_truncated,
        merged_incomplete_tail,
        visible_top,
    );

    let retain_without_counting = qualifying.is_empty()
        && (decoded.evidence.neutral_low_confidence || merged_incomplete_tail);
    if retain_without_counting {
        // 参照 `d30867a`：比较型缺口按 evidence 的低置信度决定是否清零成熟度。
        state.trackers = retain_trackers_without_counting(
            &state.trackers,
            &decoded.evidence,
            decoded.evidence.neutral_low_confidence,
        );
        return;
    }

    state.trackers = advance_trackers(&state.trackers, qualifying);
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
    let Some(decoded) = decode_early_pool(learning, state, params, &raw)? else {
        return Ok(false);
    };
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

    update_trackers_for_evidence(state, &evidence_raw, raw, &decoded, visible_top.as_deref());
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
