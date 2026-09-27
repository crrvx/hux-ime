// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 空码（整句唯一候选）自动上屏：续接权限、前置校验与提交落地。

use super::super::*;
use super::decode_lock;

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

/// 参照 `try_empty_code_commit` 的前置一致性检查：池内已有完整候选，或待定候选
/// 与当前状态不再匹配时拒绝（两条路径都清空待定）。
fn empty_code_pending_rejected(
    learning: &mut LearningCommit<'_>,
    state: &mut SentenceState,
    full_raw: &[u8],
    pending: &EmptyCodePending,
    params: EarlyCommitParams,
) -> bool {
    let raw = String::from_utf8_lossy(full_raw).into_owned();
    if crate::decode::has_complete_candidate(
        learning.decoder.lexicon(),
        &raw,
        &state.committed_text,
        None,
        false,
        params.allow_duplicate_single,
        state.active_lock().map(decode_lock).as_ref(),
    ) {
        state.empty_code_pending = None;
        return true;
    }
    if pending.committed_text != state.committed_text
        || pending.base_raw_length >= full_raw.len()
        || pending.last_segment_start >= full_raw.len()
    {
        state.empty_code_pending = None;
        return true;
    }
    false
}

/// 参照 `try_empty_code_commit` 的保留量检查：末段仍是合法前缀、保留量不足，
/// 或唯一性复核失败时阻止提交（仅最后一条清空待定）。
fn empty_code_commit_blocked(
    learning: &mut LearningCommit<'_>,
    state: &mut SentenceState,
    full_raw: &[u8],
    pending: &EmptyCodePending,
    params: EarlyCommitParams,
) -> bool {
    let extended_last_segment =
        String::from_utf8_lossy(&full_raw[pending.last_segment_start..]).into_owned();
    if learning
        .decoder
        .lexicon()
        .proper_code_prefixes
        .contains(&extended_last_segment)
    {
        return true;
    }
    // 参照 `d30867a`：保留量边界改按竞争切分的前瞻保护边界计算
    // （`pending.candidate_text:sub(#committed_text + 1)` 的字符数即目标字素数）。
    let pending_commit = pending
        .candidate_text
        .get(pending.committed_text.len()..)
        .unwrap_or_default();
    let protected_boundary = competing_boundary_end(
        full_raw,
        learning.decoder.lexicon(),
        state.committed_raw.len(),
        pending.base_raw_length,
        pending_commit.chars().count(),
    );
    if params.min_retained > 0 && full_raw.len() - protected_boundary < params.min_retained {
        return true;
    }
    if pending.requires_uniqueness_check
        && crate::decode::has_complete_candidate(
            learning.decoder.lexicon(),
            &String::from_utf8_lossy(&full_raw[..pending.base_raw_length]),
            &pending.committed_text,
            Some(&pending.candidate_text),
            true,
            params.allow_duplicate_single,
            state.active_lock().map(decode_lock).as_ref(),
        )
    {
        state.empty_code_pending = None;
        return true;
    }
    false
}

/// 参照 `try_empty_code_commit` 的提交落地段。
fn finish_empty_code_commit(
    learning: &mut LearningCommit<'_>,
    context: &mut Context,
    state: &mut SentenceState,
    full_raw: &[u8],
    pending: &EmptyCodePending,
    dot_armed: &mut bool,
) {
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
    if empty_code_pending_rejected(learning, state, &full_raw, &pending, params) {
        return Ok(false);
    }
    if empty_code_commit_blocked(learning, state, &full_raw, &pending, params) {
        return Ok(false);
    }
    finish_empty_code_commit(learning, context, state, &full_raw, &pending, dot_armed);
    Ok(true)
}
