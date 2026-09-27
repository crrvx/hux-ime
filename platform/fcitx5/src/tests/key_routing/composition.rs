// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 组合内导航：左右移光标 / 切换候选、上下移高亮、退格删输入并清组合。

use super::*;

#[test]
fn composing_left_right_move_caret_and_toggle_candidates() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    // ←：光标左移；组合按 caret 前缀重建（候选清空）
    assert!(engine.key(0xff51, 0, false), "组合中 Left 应被消费");
    let (preedit, cursor, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "ab");
    assert_eq!(cursor, 1);
    assert!(candidates.is_empty(), "光标在输入中间时无候选");
    // →：回到末尾，候选恢复
    assert!(engine.key(0xff53, 0, false));
    let (_, cursor, candidates, _, _, _) = last_update();
    assert_eq!(cursor, 2);
    assert_eq!(candidates, vec!["甲".to_string(), "乙".to_string()]);
}

#[test]
fn composing_up_down_move_highlight() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    // ↓：高亮下移；↑ 到首项
    assert!(engine.key(0xff54, 0, false));
    let (_, _, _, selected, _, _) = last_update();
    assert_eq!(selected, 1);
    assert!(engine.key(0xff52, 0, false));
    let (_, _, _, selected, _, _) = last_update();
    assert_eq!(selected, 0);
}

#[test]
fn composing_backspace_deletes_input_and_clears_composition() {
    let _guard = serial();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    // 退格：删除输入
    assert!(engine.key(0xff08, 0, false));
    let (preedit, cursor, candidates, _, _, _) = last_update();
    assert_eq!(preedit, "a");
    assert_eq!(cursor, 1);
    assert!(candidates.is_empty());
    // 再退格清空组合
    assert!(engine.key(0xff08, 0, false));
    let (preedit, _, candidates, _, _, _) = last_update();
    assert!(preedit.is_empty());
    assert!(candidates.is_empty());
}
