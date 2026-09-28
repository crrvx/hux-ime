// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 大写早提交：确认组合并请求 `forward_after_commit`；未知 / 已释放会话不得沿用粘滞标志。

use super::*;

#[test]
fn uppercase_commits_composition_and_requests_forward() {
    let _guard = serial();
    // 用户报告：组合中收到大写字母时，应先上屏当前候选（而非把字母插到预编辑之前）。
    // 核心语义保持「提交 + 不消费」（同 librime）；宿主层据 `forward_after_commit`
    // 消费该键并以 forwardKey 重发，保证「候选 → 字母」送达顺序。
    COMMITS.lock().unwrap().clear();
    UPDATES.lock().unwrap().clear();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(!engine.forward_after_commit, "普通输入不应请求转发");
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(
        !engine.key(0x41, FCITX_SHIFT, false),
        "大写字母应交宿主（不消费）"
    );
    assert!(
        engine.forward_after_commit,
        "提交且未消费 → 宿主应消费并重发该键"
    );
    assert_eq!(COMMITS.lock().unwrap().last().unwrap(), "甲");
    assert!(
        engine.session().context.input().is_empty(),
        "组合已提交并清空"
    );
}

/// `forward_after_commit` 是**粘性输出标志**，未知 / 已释放会话
/// 不得沿用上一次按键的取值。此前 `with_session` 返回 `None` 时直接 `unwrap_or(false)`，
/// 标志保留 ⇒ `hux_engine_key` 只回 `HUX_KEY_FORWARD_AFTER_COMMIT`（无 CONSUMED），
/// 宿主会 `filterAndAccept` + `forwardKey` 一个并不存在的提交。
#[test]
fn unknown_session_does_not_reuse_the_sticky_forward_flag() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    // 先造出「提交 + 未消费」（组合中按大写字母）：此时转发位为真。
    engine.key(u32::from(b'a'), 0, false);
    engine.key(u32::from(b'b'), 0, false);
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    assert!(engine.forward_after_commit);
    // 未知会话按键：必须清位（且不消费）。
    let unknown = engine.session + 1000;
    assert!(!engine.engine.key(unknown, u32::from(b'x'), 0, false));
    assert!(
        !engine.engine.forward_after_commit,
        "未知会话按键不得沿用上一次的转发位"
    );
    // 候选点击路径同理（重新置位后再走未知会话）。
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    assert!(engine.forward_after_commit);
    assert!(!engine.engine.select_candidate(unknown, 0));
    assert!(
        !engine.engine.forward_after_commit,
        "未知会话的候选点击不得沿用上一次的转发位"
    );
    // 已释放会话同理。
    assert!(engine.key(u32::from(b'a'), 0, false));
    assert!(engine.key(u32::from(b'b'), 0, false));
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    let released = engine.session;
    engine.engine.session_free(released);
    assert!(!engine.engine.key(released, u32::from(b'x'), 0, false));
    assert!(
        !engine.engine.forward_after_commit,
        "已释放会话按键不得沿用上一次的转发位"
    );
}

#[test]
fn idle_uppercase_does_not_request_forward() {
    let _guard = serial();
    let mut engine = TestEngine::new(host(), fixture_dirs(), None, None);
    assert!(!engine.key(0x41, FCITX_SHIFT, false));
    assert!(!engine.forward_after_commit);
}
