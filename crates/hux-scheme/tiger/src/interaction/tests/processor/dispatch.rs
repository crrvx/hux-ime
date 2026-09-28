// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 按键分派：释放 / 空闲标点转交、空闲数字与小数点待发、Return 提交、Escape 清空，
//! 以及缓冲态菜单导航的拦截与无菜单时的转交。

use super::*;

#[test]
fn processor_forwards_release_and_idle_punct() {
    let mut h = Harness::new();
    // 释放事件交宿主
    let release = KeyEvent::new(
        hux_core::key::keycode_by_name("a").expect("a"),
        hux_core::key::K_RELEASE_MASK,
    );
    assert_eq!(h.press_event(&release), ProcessorResult::Forward);
    // 空闲分号/引号交标点处理器
    assert_eq!(h.press("semicolon"), ProcessorResult::Forward);
    assert_eq!(h.press("apostrophe"), ProcessorResult::Forward);
}

#[test]
fn processor_commits_idle_digit_and_arms_dot() {
    let mut h = Harness::new();
    // 空闲数字直接上屏并置待发
    assert_eq!(h.press("5"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "5");
    assert!(h.dot_armed);
    // 紧随的句点按 ASCII 小数点上屏
    assert_eq!(h.press("period"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), ".");
    assert!(!h.dot_armed);
    // 无待发状态时句点交宿主
    assert_eq!(h.press("period"), ProcessorResult::Forward);
}

#[test]
fn processor_return_commits_buffer_and_input() {
    let mut h = Harness::new();
    assert_eq!(h.press("a"), ProcessorResult::Consume);
    assert_eq!(h.press("b"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b"ab");
    // 真实会话中组合由 translator 建立；这里手工合成后再走提交/清空分支。
    h.push_segment(b"ab", &["交"]);
    // Return：提交「缓冲 + 实时输入」并清空
    assert_eq!(h.press("Return"), ProcessorResult::Consume);
    assert_eq!(h.context.last_commit_text(), "ab");
    assert!(h.context.input().is_empty());
}

#[test]
fn processor_escape_clears_composition() {
    let mut h = Harness::new();
    h.push_segment(b"a", &["甲"]);
    // Escape：直接清空
    assert_eq!(h.press("Escape"), ProcessorResult::Consume);
    assert!(h.context.input().is_empty());
    assert!(h.state.committed_raw.is_empty());
}

#[test]
fn processor_guards_menu_navigation_while_buffered() {
    let mut h = Harness::new();
    // 缓冲空闲：菜单导航键拦给宿主
    h.state.buffered_text = "交".to_string();
    assert_eq!(h.press("Tab"), ProcessorResult::Consume);
    assert_eq!(h.press("Up"), ProcessorResult::Consume);
}

#[test]
fn processor_forwards_navigation_without_menu() {
    let mut h = Harness::new();
    // 无缓冲：Up 交宿主；Tab 无菜单可用时同样交宿主
    assert_eq!(h.press("Up"), ProcessorResult::Forward);
    assert_eq!(h.press("Tab"), ProcessorResult::Forward);
}

#[test]
fn processor_space_confirms_candidate() {
    let mut h = Harness::new();
    h.push_segment(b"ab", &["交"]);
    assert_eq!(h.press("space"), ProcessorResult::Consume);
    assert!(h.state.committed_raw.is_empty());
}
