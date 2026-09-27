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
        (0x20, 0) | (0x20, K_SHIFT_MASK) => {
            // Confirm：`confirm_current_selection() || commit()`。Shift 变体走
            // `FallbackOptions::All` 回退（Shift+space → `{XK_space, 0}`），与本模式同义。
            if !context.confirm_current_selection() {
                let commit_text = context.get_commit_text();
                commit_notifier(observer, context, &commit_text);
                context.commit();
            }
            true
        }
        (0xff08, 0) | (0xff08, K_SHIFT_MASK) | (0xff08, K_CONTROL_MASK) => {
            // `{XK_BackSpace, 0}` = RevertLastEdit；Shift 变体走 `FallbackOptions::All` 回退。
            //
            // **Ctrl 变体（有意偏离上游）**：参照是 `{XK_BackSpace, kControlMask}` =
            // `Editor::BackToPreviousSyllable`（按音节回退），本仓按要求**不做**该交互——
            // `Ctrl+BackSpace` 与普通 `BackSpace` 同义。
            revert_last_edit(context);
            true
        }
        (0xff0d, 0) => {
            // 参照 `Editor::CommitRawInput` = `ClearNonConfirmedComposition(); Commit();`：
            // 先丢弃**未确认**的末段（本实现的 `selected_candidate()` 不看 `selected` 标志，
            // 故必须显式清段），保证 Return 提交的是原始输入码而不是高亮候选。
            context.refresh_non_confirmed_composition();
            let commit_text = context.get_commit_text();
            commit_notifier(observer, context, &commit_text);
            context.commit();
            true
        }
        (0xff0d, K_CONTROL_MASK) => {
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
        // 注意：模式里的 `|` 是**或模式**而非按位或，故组合修饰键必须用 match guard。
        (0xff0d, modifier) if modifier == K_CONTROL_MASK | K_SHIFT_MASK => {
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
        (0xffff, 0) | (0xffff, K_SHIFT_MASK) | (0xffff, K_CONTROL_MASK) => {
            // `{XK_Delete, 0}` = DeleteChar；Shift 变体走回退。
            //
            // **Ctrl 变体（有意偏离上游）**：参照是 `{XK_Delete, kControlMask}` =
            // `Editor::DeleteCandidate`，本仓按要求**不做**该交互——`Ctrl+Delete` 与普通 `Delete`
            // 同义。
            context.delete_input(1);
            true
        }
        (0xff1b, 0) => {
            cancel_composition(context);
            true
        }
        _ => false,
    };
    if consumed {
        return HostResult::Consumed;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::test_support::*;
    use crate::host::{HostOptions, process_key};
    use crate::session::{Candidate, Segment};

    #[test]
    fn editor_commits_composition_on_uppercase_then_passes() {
        // 组合中收到大写字母：先提交组合（保证上屏顺序），按键交宿主。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "A"), HostResult::Forward);
        assert_eq!(context.last_commit_text(), "甲");
        assert!(context.input().is_empty());
    }

    #[test]
    fn editor_passes_uppercase_when_idle() {
        let mut context = Context::new();
        assert_eq!(press(&mut context, "A"), HostResult::Forward);
        assert_eq!(context.last_commit_text(), "");
    }

    #[test]
    fn editor_backspace_removes_char_before_caret() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "BackSpace"), HostResult::Consumed);
        assert_eq!(context.input(), b"a");
        assert_eq!(context.caret(), 1);
    }

    #[test]
    fn editor_delete_removes_char_at_caret() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        // 光标在末尾：Delete 不删除（librime `DeleteInput` 越界返回 false）
        assert_eq!(press(&mut context, "Delete"), HostResult::Consumed);
        assert_eq!(context.input(), b"ab");
        assert_eq!(press(&mut context, "Home"), HostResult::Consumed);
        assert_eq!(press(&mut context, "Delete"), HostResult::Consumed);
        assert_eq!(context.input(), b"b");
    }

    #[test]
    fn editor_passes_idle_editing_keys() {
        let mut context = Context::new();
        assert_eq!(press(&mut context, "BackSpace"), HostResult::Forward);
        assert_eq!(press(&mut context, "Delete"), HostResult::Forward);
        assert_eq!(press(&mut context, "Left"), HostResult::Forward);
    }

    /// 直接构造键事件（绑定测试不依赖 repr 解析）。
    fn press_raw(context: &mut Context, keycode: i32, modifier: i32) -> HostResult {
        process_key(
            &KeyEvent::new(keycode, modifier),
            context,
            None,
            &HostOptions::default(),
            None,
        )
    }

    #[test]
    fn editor_confirm_cancel_and_bindings() {
        // 参照 `ExpressEditor`：`{XK_space,0}`=Confirm、`{XK_Escape,0}`=CancelComposition。
        // 这些键在真机路径上会先被方案 `processor` 消费，故金样覆盖不到宿主链，须在此钉住。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "space"), HostResult::Consumed);
        assert!(
            context.composition.segments[0].selected,
            "空格应确认高亮段（ConfirmCurrentSelection）"
        );

        // 参照 `CancelComposition` = `ClearPreviousSegment() || Clear()`：
        // 有段时截到末段起点（可能仍有输入），无段时整体清空。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "Escape"), HostResult::Consumed);
        assert_eq!(context.last_commit_text(), "", "取消不上屏");

        let mut raw_only = Context::new();
        raw_only.push_input(b"ab");
        assert!(raw_only.is_composing());
        assert_eq!(press(&mut raw_only, "Escape"), HostResult::Consumed);
        assert!(!raw_only.is_composing(), "无段时取消应整体清空");
    }

    /// **有意偏离上游**：参照把
    /// `Ctrl+BackSpace` 绑到 `BackToPreviousSyllable`（按音节回退）、`Ctrl+Delete` 绑到
    /// `DeleteCandidate`；本仓按要求**取消这两个交互**，让它们与不带修饰的
    /// `BackSpace`/`Delete` **同义**。
    ///
    /// 守护方式：对同一初始状态分别按「带 Ctrl」与「不带 Ctrl」，断言**结果状态逐字段相等**
    /// ——这样即使将来 `BackSpace`/`Delete` 的语义变了，等价关系仍被钉住。
    #[test]
    fn ctrl_backspace_and_ctrl_delete_match_their_plain_variants() {
        // 造一个「有组合输入 + 有菜单 + 有已选段」的状态：三种路径（pop_input /
        // reopen_previous_selection / delete_input）都可能被走到，故等价断言必须比状态。
        let build = || {
            let mut context = context_with_menu(&["甲乙", "甲"], 0);
            context.push_input(b"ab");
            context
        };

        // `Context` 没有 `Debug`，故比对**可观测状态指纹**：输入 / 光标 / 组合段数与
        // 选中态 / 菜单候选与高亮 / 已上屏文本。
        let fingerprint = |context: &Context| -> String {
            let segments = context
                .composition
                .segments
                .iter()
                .map(|segment| {
                    format!(
                        "{}..{}/sel={}/tr={}/idx={}/{:?}",
                        segment.start,
                        segment.end,
                        segment.selected,
                        segment.translated,
                        segment.selected_index,
                        segment.selected_candidate().map(|c| c.text.clone())
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "input={:?} caret={} composing={} menu={} segs=[{}] commit={:?}",
                String::from_utf8_lossy(context.input()),
                context.caret(),
                context.is_composing(),
                context.has_menu(),
                segments,
                context.last_commit_text(),
            )
        };

        let mut plain_backspace = build();
        let mut ctrl_backspace = build();
        assert_eq!(
            press_raw(&mut plain_backspace, 0xff08, 0),
            HostResult::Consumed
        );
        assert_eq!(
            press_raw(&mut ctrl_backspace, 0xff08, K_CONTROL_MASK),
            HostResult::Consumed
        );
        assert_eq!(
            fingerprint(&plain_backspace),
            fingerprint(&ctrl_backspace),
            "Ctrl+BackSpace 必须与普通 BackSpace 完全同义（有意偏离）"
        );

        let mut plain_delete = build();
        let mut ctrl_delete = build();
        assert_eq!(
            press_raw(&mut plain_delete, 0xffff, 0),
            HostResult::Consumed
        );
        assert_eq!(
            press_raw(&mut ctrl_delete, 0xffff, K_CONTROL_MASK),
            HostResult::Consumed
        );
        assert_eq!(
            fingerprint(&plain_delete),
            fingerprint(&ctrl_delete),
            "Ctrl+Delete 必须与普通 Delete 完全同义（有意偏离）"
        );
    }

    #[test]
    fn editor_ctrl_return_commits_script_text() {
        // 参照 `{XK_Return, kControlMask}` = `CommitScriptText`：提交**脚本文本**
        // （`Composition::GetScriptText`：preedit 优先，否则原始输入切片），
        // 与 `{XK_Return, 0}`（`CommitRawInput`）区分开；**不发提交通知**（不写学习）。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(
            press_raw(&mut context, 0xff0d, K_CONTROL_MASK),
            HostResult::Consumed
        );
        assert_eq!(
            context.last_commit_text(),
            "ab",
            "候选无 preedit ⇒ 原始输入"
        );

        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press_raw(&mut context, 0xff0d, 0), HostResult::Consumed);
        assert_eq!(
            context.last_commit_text(),
            "ab",
            "原始输入 = 未确认段的原文"
        );
    }

    /// `CommitScriptText` 不经 `Commit()`：不产生提交通知（学习事件）。
    #[test]
    fn editor_ctrl_return_does_not_notify_commit() {
        #[derive(Default)]
        struct Recorder {
            calls: usize,
        }
        impl CommitObserver for Recorder {
            fn on_commit(&mut self, _context: &Context, _commit_text: &str) {
                self.calls += 1;
            }
        }
        let mut recorder = Recorder::default();
        let mut context = context_with_menu(&["甲", "乙"], 0);
        let key = KeyEvent::new(0xff0d, K_CONTROL_MASK);
        assert_eq!(
            process_key(
                &key,
                &mut context,
                None,
                &HostOptions::default(),
                Some(&mut recorder)
            ),
            HostResult::Consumed
        );
        assert_eq!(recorder.calls, 0, "`Clear()` 不触发提交通知");
        // 对照：普通 `Return`（`CommitRawInput`）走 `Commit()`，通知照发。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(
            process_key(
                &KeyEvent::new(0xff0d, 0),
                &mut context,
                None,
                &HostOptions::default(),
                Some(&mut recorder)
            ),
            HostResult::Consumed
        );
        assert_eq!(recorder.calls, 1);
    }

    /// `CommitScriptText` 的 preedit 优先与去首个 `\t`（生产候选带 preedit）。
    #[test]
    fn editor_ctrl_return_prefers_candidate_preedit() {
        let mut context = Context::new();
        context.set_input(b"ab");
        let mut segment = Segment {
            start: 0,
            end: 2,
            tags: vec!["abc".to_string()],
            translated: true,
            ..Segment::default()
        };
        let mut candidate = Candidate::new("sentence", 0, 2, "先", "");
        candidate.preedit = "xi\tan".to_string();
        segment.candidates.push(candidate);
        context.composition.segments.push(segment);
        context.drain_events();
        assert_eq!(
            press_raw(&mut context, 0xff0d, K_CONTROL_MASK),
            HostResult::Consumed
        );
        assert_eq!(context.last_commit_text(), "xian");
    }

    #[test]
    fn editor_ctrl_shift_return_keeps_composition_without_comment() {
        // 参照 `Editor::CommitComment`：注释为空 ⇒ 只吞键（不清组合、不提交）。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(
            press_raw(&mut context, 0xff0d, K_CONTROL_MASK | K_SHIFT_MASK),
            HostResult::Consumed
        );
        assert_eq!(context.last_commit_text(), "", "空注释不提交空串");
        assert_eq!(context.input(), b"ab", "空注释不清组合");
        assert!(context.is_composing());
        // 无候选段同理：不 clear、不提交。
        let mut empty = Context::new();
        empty.set_input(b"xx");
        empty.composition.segments.push(Segment {
            start: 0,
            end: 2,
            tags: vec!["abc".to_string()],
            translated: true,
            ..Segment::default()
        });
        empty.drain_events();
        assert_eq!(
            press_raw(&mut empty, 0xff0d, K_CONTROL_MASK | K_SHIFT_MASK),
            HostResult::Consumed
        );
        assert_eq!(empty.input(), b"xx");
        assert_eq!(empty.last_commit_text(), "");
    }

    #[test]
    fn editor_ctrl_shift_return_commits_candidate_comment() {
        // 参照 `{XK_Return, kControlMask | kShiftMask}` = `CommitComment`：提交高亮候选的注释。
        let mut context = Context::new();
        context.set_input(b"ab");
        let mut segment = Segment {
            start: 0,
            end: 2,
            tags: vec!["abc".to_string()],
            translated: true,
            selected_index: 0,
            ..Segment::default()
        };
        segment
            .candidates
            .push(Candidate::new("sentence", 0, 2, "中", "d/dg/dgs"));
        context.composition.segments.push(segment);
        context.drain_events();
        assert_eq!(
            press_raw(&mut context, 0xff0d, K_CONTROL_MASK | K_SHIFT_MASK),
            HostResult::Consumed
        );
        let events = context.drain_events();
        assert!(
            events.iter().any(
                |event| matches!(event, crate::session::Event::Commit(text) if text == "d/dg/dgs")
            ),
            "应提交候选注释：{events:?}"
        );
    }

    #[test]
    fn editor_fallbacks_match_reference_keymap() {
        // 参照 `ExpressEditor` 键表 + `FallbackOptions::All`：Shift 变体回退到无修饰绑定。
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(
            press_raw(&mut context, 0x20, K_SHIFT_MASK),
            HostResult::Consumed
        );

        let mut context = context_with_menu(&["甲", "乙"], 0);
        context.set_input(b"abc");
        assert_eq!(
            press_raw(&mut context, 0xff08, K_SHIFT_MASK),
            HostResult::Consumed
        );

        let mut context = context_with_menu(&["甲", "乙"], 0);
        context.set_input(b"abc");
        assert_eq!(
            press_raw(&mut context, 0xffff, K_SHIFT_MASK),
            HostResult::Consumed
        );
    }
}
