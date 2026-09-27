// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `shell/hux.cpp` 的源码文本视图：折叠空白后按「函数体 / 聚合块」切窗口。
//!
//! 三态折算、托盘结构、模型入口三个用例都按源码文本断言，窗口口径必须一致，故收在一处：
//! 折叠文本取 `flat()`，函数体取 `function()`，聚合块取 `block()`。

use hux_test_support::repo_path;

/// `platform/fcitx5/shell/hux.cpp` 折成单行后的源码视图。
pub(super) struct Source {
    flat: String,
}

impl Source {
    /// 读取并折叠 `shell/hux.cpp`。
    pub(super) fn read() -> Self {
        let source = std::fs::read_to_string(repo_path("platform/fcitx5/shell/hux.cpp"))
            .expect("read hux.cpp");
        // 折成单行：断行与缩进不影响断言（涉及的字符串字面量里没有空白，故折叠不改变它们）。
        let flat = source.split_whitespace().collect::<Vec<_>>().join(" ");
        Self { flat }
    }

    /// 折叠后的全文。
    pub(super) fn flat(&self) -> &str {
        &self.flat
    }

    // 取某个函数「签名 + 函数体」（按花括号配平，避免窗口切进下一个函数）。
    pub(super) fn function(&self, signature: &str) -> String {
        let flat = self.flat.as_str();
        let start = flat
            .find(signature)
            .unwrap_or_else(|| panic!("hux.cpp 缺少 {signature}"));
        let open = start
            + flat[start..]
                .find('{')
                .unwrap_or_else(|| panic!("{signature} 没有函数体"));
        let mut depth = 0i32;
        for (offset, ch) in flat[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return flat[start..open + offset + 1].to_string();
                    }
                }
                _ => {}
            }
        }
        panic!("{signature} 的花括号不配平");
    }

    // 取某段源码（从签名到其后的第一个 `};`，用于表 / 文案表这种聚合块）。
    pub(super) fn block(&self, signature: &str) -> &str {
        let flat = self.flat.as_str();
        let start = flat
            .find(signature)
            .unwrap_or_else(|| panic!("hux.cpp 缺少 {signature}"));
        let end = start
            + flat[start..]
                .find("};")
                .unwrap_or_else(|| panic!("{signature} 未闭合"));
        &flat[start..end]
    }
}
