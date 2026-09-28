// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 兜底编辑器单测：keymap 子集、char_handler 直提交与提交通知链。

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

/// 造一个「有组合输入 + 有菜单 + 有已选段」的状态：三种路径（pop_input /
/// reopen_previous_selection / delete_input）都可能被走到，故等价断言必须比状态。
fn ctrl_equivalence_context() -> Context {
    let mut context = context_with_menu(&["甲乙", "甲"], 0);
    context.push_input(b"ab");
    context
}

/// `Context` 没有 `Debug`，故比对**可观测状态指纹**：输入 / 光标 / 组合段数与
/// 选中态 / 菜单候选与高亮 / 已上屏文本。
fn state_fingerprint(context: &Context) -> String {
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
    let mut plain_backspace = ctrl_equivalence_context();
    let mut ctrl_backspace = ctrl_equivalence_context();
    assert_eq!(
        press_raw(&mut plain_backspace, 0xff08, 0),
        HostResult::Consumed
    );
    assert_eq!(
        press_raw(&mut ctrl_backspace, 0xff08, K_CONTROL_MASK),
        HostResult::Consumed
    );
    assert_eq!(
        state_fingerprint(&plain_backspace),
        state_fingerprint(&ctrl_backspace),
        "Ctrl+BackSpace 必须与普通 BackSpace 完全同义（有意偏离）"
    );

    let mut plain_delete = ctrl_equivalence_context();
    let mut ctrl_delete = ctrl_equivalence_context();
    assert_eq!(
        press_raw(&mut plain_delete, 0xffff, 0),
        HostResult::Consumed
    );
    assert_eq!(
        press_raw(&mut ctrl_delete, 0xffff, K_CONTROL_MASK),
        HostResult::Consumed
    );
    assert_eq!(
        state_fingerprint(&plain_delete),
        state_fingerprint(&ctrl_delete),
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
