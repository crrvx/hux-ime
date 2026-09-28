// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 数字直选与候选点选（按页 / 按全局索引选中并提交）。

use super::super::*;

/// 数字直选位置（0-based）：`1`–`9` → `0`–`8`，`0` → `9`（第 10 个）。
pub(crate) fn digit_page_position(ch: char) -> Option<usize> {
    match ch {
        '1'..='9' => Some(ch as usize - '1' as usize),
        '0' => Some(9),
        _ => None,
    }
}

/// 数字直选（`DigitSelect`，addon 扩展）：选择当前页第 `position`（0-based）个候选，
/// 走与 `space` 相同的确认/学习链并直接上屏；候选不在页内时不消费（交回普通数字处理）。
pub(crate) fn select_page_candidate(
    decoder: &mut Decoder,
    context: &mut Context,
    state: &mut SentenceState,
    live: &mut LiveLearning,
    now: f64,
    page_size: usize,
    position: usize,
) -> anyhow::Result<bool> {
    let page_size = page_size.max(1);
    if position >= page_size {
        return Ok(false);
    }
    let Some(segment) = context.composition.back() else {
        return Ok(false);
    };
    let page_start = (segment.selected_index / page_size) * page_size;
    select_candidate_at(decoder, context, state, live, now, page_start + position)
}

/// 候选点击 / 数字直选共用：按**全局索引**选中候选，走与 `space` 相同的确认链
/// 并直接上屏（对齐参照 `ConcreteEngine::OnSelect` + `RimeState::selectCandidate`：
/// 点选后提交整个组合）。索引越界（候选未生成）或无可选段时返回 `false`。
///
/// 学习：候选点击在参照里**只经提交通知器**一次「暂存 + 提交」
/// （`lua/tiger_sentence.lua` 的 `commit_notifier` 回调；参照的候选点击不经过方案处理器），
/// 故这里**不得**再显式 `learning_stage` 一次——否则同一次点击会在 `pending` 里留下
/// 两条完全相同的成对偏好事件（`learning_submit` 全部接受 ⇒ 权重记两次）。
/// 对照：参照 `space`/标点分支确有「处理器先暂存 + 通知器再暂存」的两段（本仓照搬），
/// 反查段数字直选则同样只走通知器一次。
pub fn select_candidate_at(
    decoder: &mut Decoder,
    context: &mut Context,
    state: &mut SentenceState,
    live: &mut LiveLearning,
    now: f64,
    index: usize,
) -> anyhow::Result<bool> {
    {
        let Some(segment) = context.composition.back() else {
            return Ok(false);
        };
        if index >= segment.prepare(index + 1) {
            return Ok(false);
        }
    }
    context.highlight(index);
    confirm_selection(
        Some(&mut LearningCommit {
            decoder: &mut *decoder,
            live: &mut *live,
            now,
        }),
        context,
        state,
    );
    live.pending.clear();
    live.baseline = None;
    state.reset(context, false);
    Ok(true)
}
