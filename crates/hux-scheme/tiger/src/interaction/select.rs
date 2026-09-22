// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// 参照 commit 通知器（优先级 -100）：缓冲提交前把缓冲前缀并入选中候选文本。
/// 对应 `prepare_learning` 里注册的 `buffer_commit_connection`（宿主在提交前调用）。
pub fn apply_buffered_commit(context: &mut Context) {
    let prefix = buffered_text(context);
    if prefix.is_empty() {
        return;
    }
    let Some(segment) = context.composition.back_mut() else {
        return;
    };
    let Some(candidate) = segment.candidates.get_mut(segment.selected_index) else {
        return;
    };
    if candidate.kind == "sentence_buffered" {
        candidate.text = format!("{prefix}{}", candidate.text);
        candidate.kind = "sentence_buffered_commit".to_string();
    }
}

/// 参照 librime 引擎对 `ConfirmCurrentSelection` 的**同步**反应：
/// 末段覆盖整段输入且 `_auto_commit` 开启时，先合并缓冲前缀，再在清空前触发
/// 提交通知器（学习选择/暂存/提交，`learning` 为 `None` 时跳过学习），随后立即提交
/// （librime：确认 → 选择通知 → 引擎 `OnSelect` → 自动提交 → 提交通知器 → `Clear`）。
/// 宿主需在会话初始化时置 `_auto_commit`（对应 librime `express_editor`
/// 的默认 true），否则确认段会保持未提交。
pub fn confirm_selection(
    learning: Option<&mut LearningCommit<'_>>,
    context: &mut Context,
    state: &mut SentenceState,
) {
    if !context.confirm_current_selection() {
        return;
    }
    let covered = context
        .composition
        .back()
        .map(|segment| segment.end == context.input().len())
        .unwrap_or(false);
    if !covered || !context.get_option("_auto_commit") {
        return;
    }
    apply_buffered_commit(context);
    if let Some(learning) = learning {
        let LearningCommit { decoder, live, now } = learning;
        let commit_text = context.get_commit_text();
        learning_commit(decoder, context, state, live, *now, &commit_text);
    }
    context.commit();
}
