// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 兜底编辑器：镜像 librime `ExpressEditor`（`_auto_commit = true` 变体）的 keymap 子集与 `char_handler`。

use super::commit_notifier::commit_notifier;
use super::{CommitObserver, HostResult};
use crate::key::{K_CONTROL_MASK, K_SHIFT_MASK, KeyEvent};
use crate::session::Context;

/// 参照 `ExpressEditor`（`_auto_commit = true` 变体）的 keymap 子集 + `char_handler`。
///
/// Return/space/Escape 在组合中已被方案侧 `processor` 消费，此处为完整的兜底实现；
/// 可打印字符按 `char_handler`（ExpressEditor = `DirectCommit`）处理：
/// **先提交当前组合**（保证上屏顺序），按键交宿主。
/// 学习链：提交点经 [`commit_notifier`] 记录（对应参照 librime 的提交通知器）。
pub(super) fn editor(
    key_event: &KeyEvent,
    context: &mut Context,
    observer: &mut Option<&mut dyn CommitObserver>,
) -> HostResult {
    if !context.is_composing() {
        return HostResult::Forward;
    }
    let consumed = match (key_event.keycode, key_event.modifier) {
        (0x20, 0) | (0x20, K_SHIFT_MASK) => confirm_selection(context, observer),
        (0xff08, 0) | (0xff08, K_SHIFT_MASK) | (0xff08, K_CONTROL_MASK) => revert_edit(context),
        (0xff0d, 0) => commit_raw_input(context, observer),
        (0xff0d, K_CONTROL_MASK) => commit_script_text(context),
        // 注意：模式里的 `|` 是**或模式**而非按位或，故组合修饰键必须用 match guard。
        (0xff0d, modifier) if modifier == K_CONTROL_MASK | K_SHIFT_MASK => {
            commit_candidate_comment(context)
        }
        (0xffff, 0) | (0xffff, K_SHIFT_MASK) | (0xffff, K_CONTROL_MASK) => delete_char(context),
        (0xff1b, 0) => escape(context),
        _ => false,
    };
    if consumed {
        return HostResult::Consumed;
    }
    direct_commit_printable(key_event, context, observer);
    HostResult::Forward
}

/// 参照 `Editor::RevertLastEdit`：`ReopenPreviousSelection() || (PopInput() && ReopenPreviousSegment())`。
fn revert_last_edit(context: &mut Context) {
    if reopen_previous_selection(context) {
        return;
    }
    if context.pop_input(1) {
        reopen_previous_segment(context);
    }
}

/// 参照 `Context::ReopenPreviousSelection`：末尾已选段回退为未选。
///
/// 与参照的结构差异（有意）：参照另有两道护栏——`seg->status > kSelected` 与
/// `seg->tags.count("selected_before_editing")`，本模型下**不可达**：
/// 已确认段会被移出组合（进 committed/locks 状态），故不存在 `kConfirmed` 段；
/// 本 crate 也无 `BeginEditing` 等价物（无该标签的写入方）。
/// 若将来引入「编辑态」（`BeginEditing`）或组合内的确认段，须同步补这两道判据。
fn reopen_previous_selection(context: &mut Context) -> bool {
    let mut index = context.composition.segments.len();
    while index > 0 {
        index -= 1;
        if !context.composition.segments[index].selected {
            continue;
        }
        let caret = context.caret();
        context.composition.segments.truncate(index + 1);
        let segment = &mut context.composition.segments[index];
        reopen_segment(segment, caret);
        return true;
    }
    false
}

/// 参照 `Context::ReopenPreviousSegment`：`composition.Trim()` 后回退末尾已选段。
fn reopen_previous_segment(context: &mut Context) -> bool {
    if !context.composition.trim() {
        return false;
    }
    let caret = context.caret();
    if let Some(segment) = context.composition.back_mut()
        && segment.selected
    {
        reopen_segment(segment, caret);
    }
    true
}

/// 参照 `Segment::Reopen`：清掉选中状态（同位置保留候选与高亮）。
fn reopen_segment(segment: &mut crate::session::Segment, caret: usize) {
    segment.selected = false;
    if segment.end != caret {
        segment.translated = false;
        segment.candidates.clear();
        segment.selected_index = 0;
    }
}

/// 参照 `Editor::CancelComposition`：`ClearPreviousSegment() || Clear()`。
fn cancel_composition(context: &mut Context) {
    if !clear_previous_segment(context) {
        context.clear();
    }
}

/// 参照 `Context::ClearPreviousSegment`：输入截到末段起点。
fn clear_previous_segment(context: &mut Context) -> bool {
    let Some(segment) = context.composition.back() else {
        return false;
    };
    let where_ = segment.start;
    if where_ >= context.input().len() {
        return false;
    }
    let head = context.input()[..where_].to_vec();
    context.set_input(&head);
    true
}

/// 参照 `{XK_space, 0}` = `Confirm`：`confirm_current_selection() || commit()`。
fn confirm_selection(
    context: &mut Context,
    observer: &mut Option<&mut dyn CommitObserver>,
) -> bool {
    // Confirm：`confirm_current_selection() || commit()`。Shift 变体走
    // `FallbackOptions::All` 回退（Shift+space → `{XK_space, 0}`），与本模式同义。
    if !context.confirm_current_selection() {
        let commit_text = context.get_commit_text();
        commit_notifier(observer, context, &commit_text);
        context.commit();
    }
    true
}

/// 参照 `{XK_BackSpace, 0}` = `RevertLastEdit`（Shift 变体走回退、Ctrl 变体同义）。
fn revert_edit(context: &mut Context) -> bool {
    // `{XK_BackSpace, 0}` = RevertLastEdit；Shift 变体走 `FallbackOptions::All` 回退。
    //
    // **Ctrl 变体（有意偏离上游）**：参照是 `{XK_BackSpace, kControlMask}` =
    // `Editor::BackToPreviousSyllable`（按音节回退），本仓按要求**不做**该交互——
    // `Ctrl+BackSpace` 与普通 `BackSpace` 同义。
    revert_last_edit(context);
    true
}

/// 参照 `{XK_Return, 0}` = `CommitRawInput`：清掉未确认末段后提交原始输入码。
fn commit_raw_input(context: &mut Context, observer: &mut Option<&mut dyn CommitObserver>) -> bool {
    // 参照 `Editor::CommitRawInput` = `ClearNonConfirmedComposition(); Commit();`：
    // 先丢弃**未确认**的末段（本实现的 `selected_candidate()` 不看 `selected` 标志，
    // 故必须显式清段），保证 Return 提交的是原始输入码而不是高亮候选。
    context.refresh_non_confirmed_composition();
    let commit_text = context.get_commit_text();
    commit_notifier(observer, context, &commit_text);
    context.commit();
    true
}

/// 参照 `{XK_Return, kControlMask}` = `CommitScriptText`：提交脚本文本且不发通知。
fn commit_script_text(context: &mut Context) -> bool {
    // 参照 `{XK_Return, kControlMask}` = `Editor::CommitScriptText`（`gear/editor.cc`）：
    // `engine_->sink()(ctx->GetScriptText()); ctx->Clear();`
    // ——提交**脚本文本**（每段 preedit 优先且去首个 `\t`，否则原始输入切片；
    // 已确认段在 `keep_selection = true`（`composition.h` 默认实参）下取候选文字），
    // 且**不经 `Commit()`**：不发提交通知 ⇒ 不产生学习事件。
    let text = context.get_script_text();
    context.direct_commit(&text);
    context.clear();
    true
}

/// 参照 `{XK_Return, kControlMask | kShiftMask}` = `CommitComment`：仅注释非空时上屏。
fn commit_candidate_comment(context: &mut Context) -> bool {
    // 参照 `{XK_Return, kControlMask | kShiftMask}` = `Editor::CommitComment`
    // （`gear/editor.cc`）：**仅当**高亮候选存在且注释非空时 `sink(comment) + Clear()`；
    // 注释为空则只吞键——不清组合、不提交空串。
    let comment = context
        .composition
        .back()
        .and_then(|segment| segment.selected_candidate())
        .map(|candidate| candidate.comment.clone())
        .filter(|comment| !comment.is_empty());
    if let Some(comment) = comment {
        context.direct_commit(&comment);
        context.clear();
    }
    true
}

/// 参照 `{XK_Delete, 0}` = `DeleteChar`（Shift 变体走回退、Ctrl 变体同义）。
fn delete_char(context: &mut Context) -> bool {
    // `{XK_Delete, 0}` = DeleteChar；Shift 变体走回退。
    //
    // **Ctrl 变体（有意偏离上游）**：参照是 `{XK_Delete, kControlMask}` =
    // `Editor::DeleteCandidate`，本仓按要求**不做**该交互——`Ctrl+Delete` 与普通 `Delete`
    // 同义。
    context.delete_input(1);
    true
}

/// 参照 `{XK_Escape, 0}` = `CancelComposition`：`ClearPreviousSegment() || Clear()`。
fn escape(context: &mut Context) -> bool {
    cancel_composition(context);
    true
}

fn direct_commit_printable(
    key_event: &KeyEvent,
    context: &mut Context,
    observer: &mut Option<&mut dyn CommitObserver>,
) {
    // 参照 `Editor::ProcessKeyEvent` 的 char_handler（ExpressEditor = `DirectCommit`）：
    // 可打印字符（>0x20 且 <0x7f，无 Ctrl/Alt/Super）先提交组合，再交宿主。
    // 宿主层（fcitx5 addon）会据此消费该键并以 `forwardKey` 重发，保证「提交 → 按键」送达顺序。
    if !key_event.ctrl()
        && !key_event.alt()
        && !key_event.super_modifier()
        && key_event.keycode > 0x20
        && key_event.keycode < 0x7f
    {
        let commit_text = context.get_commit_text();
        commit_notifier(observer, context, &commit_text);
        context.commit();
    }
}

#[cfg(test)]
mod tests;
