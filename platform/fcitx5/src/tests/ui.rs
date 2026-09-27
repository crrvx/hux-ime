// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! UI 文本工具：内嵌 NUL 的文本送 C 侧时剔除，而不是整条丢空。
//!
//! 本用例直调 `crate::ui::cstring_lossy`，不用共享夹具与 `serial()` 串行锁；
//! 父模块的约定见 `tests.rs`。

/// 文本含 NUL 时剔除后送出，而不是整条丢空。
#[test]
fn nul_in_text_is_stripped_not_dropped() {
    assert_eq!(
        crate::ui::cstring_lossy("中\0文").to_str().expect("utf8"),
        "中文"
    );
}
