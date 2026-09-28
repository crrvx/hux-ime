// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 退格：锁下 `pop_input` 与提交文本不一致时的字符边界截断。

use super::*;

#[test]
fn processor_backspace_pops_locked_input() {
    let mut h = Harness::new();
    // 锁分支：退格在锁下走 pop_input
    h.push_segment(b"ab", &["交"]);
    h.state.locks.push(Lock {
        raw: "a".to_string(),
        text: "交".to_string(),
        boundaries: "1,3;".to_string(),
    });
    h.state.committed_raw = "a".to_string();
    h.state.committed_text = "交".to_string();
    assert_eq!(h.press("BackSpace"), ProcessorResult::Consume);
    assert_eq!(h.context.input(), b"a");
}

#[test]
fn backspace_with_inconsistent_committed_text_does_not_panic() {
    let mut h = Harness::new();
    // 组合存在（缓冲退格分支的前提）且 live input 为空。
    h.push_segment(b"", &[]);
    // 属性可能来自旧版本/外部：committed_text 尾字符与 buffered 尾字符不一致时，
    // 退格只按字符边界截断，不得 panic。
    h.state.buffered_text = "A".to_string();
    h.state.committed_text = "甲".to_string();
    h.state.committed_raw = "a".to_string();
    assert_eq!(h.press("BackSpace"), ProcessorResult::Consume);
}
