// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 光标导航：镜像 librime `Navigator::ProcessKeyEvent`（组合中生效，含 `FallbackOptions::All` 回退）。

use super::HostResult;
use crate::key::{K_CONTROL_MASK, K_SHIFT_MASK, KeyEvent};
use crate::session::Context;

#[derive(Clone, Copy)]
enum NavigatorAction {
    Rewind,
    Forward,
    LeftByChar,
    RightByChar,
    LeftBySyllable,
    RightBySyllable,
    Home,
    End,
}

impl NavigatorAction {
    /// 参照 `Navigator` 的 Horizontal/Vertical keymap（精确修饰匹配：无修饰 / Ctrl）。
    fn from_key(key_event: &KeyEvent, vertical: bool) -> Option<Self> {
        let code = key_event.keycode;
        match (code, key_event.modifier, vertical) {
            (0xff51, 0, false) | (0xff53, 0, true) => Some(Self::Rewind), // Left / Right（竖排）
            (0xff53, 0, false) | (0xff51, 0, true) => Some(Self::Forward),
            (0xff51, K_CONTROL_MASK, false) | (0xff53, K_CONTROL_MASK, true) => {
                Some(Self::LeftBySyllable)
            }
            (0xff53, K_CONTROL_MASK, false) | (0xff51, K_CONTROL_MASK, true) => {
                Some(Self::RightBySyllable)
            }
            (0xff96, 0, false) => Some(Self::LeftByChar), // KP_Left
            (0xff98, 0, false) => Some(Self::RightByChar), // KP_Right
            (0xff50 | 0xff95, 0, _) => Some(Self::Home),
            (0xff57 | 0xff9c, 0, _) => Some(Self::End),
            (0xff52, 0, true) => Some(Self::Rewind), // Up（竖排）
            (0xff54, 0, true) => Some(Self::Forward), // Down（竖排）
            (0xff52, K_CONTROL_MASK, true) => Some(Self::LeftBySyllable),
            (0xff54, K_CONTROL_MASK, true) => Some(Self::RightBySyllable),
            (0xff97, 0, true) => Some(Self::LeftByChar), // KP_Up（竖排）
            (0xff99, 0, true) => Some(Self::RightByChar), // KP_Down（竖排）
            _ => None,
        }
    }
}

/// 参照 `Navigator::ProcessKeyEvent`：组合中生效，含 `FallbackOptions::All` 回退。
pub(super) fn navigator(key_event: &KeyEvent, context: &mut Context) -> HostResult {
    if !context.is_composing() {
        return HostResult::Forward;
    }
    let vertical = context.get_option("_vertical");
    if let Some(action) = NavigatorAction::from_key(key_event, vertical) {
        return navigator_action(action, context);
    }
    // 回退：Ctrl/Alt 不参与；Shift 依次按「视为 Ctrl」「忽略 Shift」重试。
    if key_event.ctrl() || key_event.alt() {
        return HostResult::Forward;
    }
    if key_event.shift() {
        let shift_as_control = KeyEvent::new(
            key_event.keycode,
            (key_event.modifier & !K_SHIFT_MASK) | K_CONTROL_MASK,
        );
        if let Some(action) = NavigatorAction::from_key(&shift_as_control, vertical) {
            return navigator_action(action, context);
        }
        let ignore_shift = KeyEvent::new(key_event.keycode, key_event.modifier & !K_SHIFT_MASK);
        if let Some(action) = NavigatorAction::from_key(&ignore_shift, vertical) {
            return navigator_action(action, context);
        }
    }
    HostResult::Forward
}

/// 参照 `Navigator` 各动作（除 `Rewind`/`Forward` 的 spans 跳转外均按单段语义）。
fn navigator_action(action: NavigatorAction, context: &mut Context) -> HostResult {
    match action {
        NavigatorAction::Rewind => {
            // 单段等价：MoveLeft（多段跳转见模块注释）。
            move_left(context);
            HostResult::Consumed
        }
        NavigatorAction::Forward => {
            move_right(context);
            HostResult::Consumed
        }
        NavigatorAction::LeftByChar => {
            if !move_left(context) {
                go_to_end(context);
            }
            HostResult::Consumed
        }
        NavigatorAction::RightByChar => {
            if !move_right(context) {
                go_home(context);
            }
            HostResult::Consumed
        }
        NavigatorAction::LeftBySyllable => {
            // `JumpLeft(confirmed_pos, loop)`：单段 → 跳到段首。
            context.set_caret(0);
            HostResult::Consumed
        }
        NavigatorAction::RightBySyllable => {
            // `JumpRight(confirmed_pos, loop)`：单段 → 跳到输入末尾。
            go_to_end(context);
            HostResult::Consumed
        }
        NavigatorAction::Home => {
            go_home(context);
            HostResult::Consumed
        }
        NavigatorAction::End => {
            go_to_end(context);
            HostResult::Consumed
        }
    }
}

/// 参照 `Navigator::MoveLeft`。
fn move_left(context: &mut Context) -> bool {
    let caret = context.caret();
    if caret == 0 {
        return false;
    }
    context.set_caret(caret - 1);
    true
}

/// 参照 `Navigator::MoveRight`。
fn move_right(context: &mut Context) -> bool {
    let caret = context.caret();
    if caret >= context.input().len() {
        return false;
    }
    context.set_caret(caret + 1);
    true
}

/// 参照 `Navigator::GoHome`：跳到首个未确认段的起点，否则回到 0。
fn go_home(context: &mut Context) {
    let caret = context.caret();
    if !context.composition.segments.is_empty() {
        let mut confirmed_pos = caret;
        for segment in context.composition.segments.iter().rev() {
            if segment.selected {
                break;
            }
            confirmed_pos = segment.start;
        }
        if confirmed_pos < caret {
            context.set_caret(confirmed_pos);
            return;
        }
    }
    if caret != 0 {
        context.set_caret(0);
    }
}

/// 参照 `Navigator::GoToEnd`。
fn go_to_end(context: &mut Context) {
    let end = context.input().len();
    if context.caret() != end {
        context.set_caret(end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::test_support::*;
    use crate::session::Segment;

    #[test]
    fn navigator_moves_caret_by_char() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "Left"), HostResult::Consumed);
        assert_eq!(context.caret(), 1);
        assert_eq!(press(&mut context, "Right"), HostResult::Consumed);
        assert_eq!(context.caret(), 2);
    }

    #[test]
    fn navigator_home_end_move_to_edges() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        assert_eq!(press(&mut context, "Home"), HostResult::Consumed);
        assert_eq!(context.caret(), 0);
        assert_eq!(press(&mut context, "End"), HostResult::Consumed);
        assert_eq!(context.caret(), 2);
    }

    #[test]
    fn navigator_ctrl_shift_arrows_jump_edges() {
        let mut context = context_with_menu(&["甲", "乙"], 0);
        // Ctrl/Shift+Left|Right：按单段跳到首/尾
        assert_eq!(press(&mut context, "Control+Left"), HostResult::Consumed);
        assert_eq!(context.caret(), 0);
        assert_eq!(press(&mut context, "Shift+Right"), HostResult::Consumed);
        assert_eq!(context.caret(), 2);
    }

    #[test]
    fn navigator_consumes_without_menu() {
        let mut context = Context::new();
        context.set_input(b"xx");
        context.composition.segments.push(Segment {
            start: 0,
            end: 2,
            tags: vec!["abc".to_string()],
            translated: true,
            ..Segment::default()
        });
        assert_eq!(press(&mut context, "Left"), HostResult::Consumed);
        assert_eq!(context.caret(), 1);
        assert_eq!(press(&mut context, "Home"), HostResult::Consumed);
        assert_eq!(context.caret(), 0);
    }
}
