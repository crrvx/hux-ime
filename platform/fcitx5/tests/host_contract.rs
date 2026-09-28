// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 宿主壳与 ABI 头文件的**源码文本契约**。
//!
//! C++ 侧（`platform/fcitx5/shell/hux.cpp`）与 `platform/fcitx5/src/abi.rs` 分属两次编译
//! （cmake / cargo）：两侧对不上时两边都编译得过，只有读源码逐项比对才拦得住——配置项
//! 默认值漂移、枚举改名 / 改值、ABI 入口漏声明都属于这一类。
//!
//! 分层：这些用例只依赖仓库内的文本（经 `repo_path` 定位）与 `hux-cfg` 的公开类型，
//! 不触碰 crate 内部项，故放集成测试；引擎行为（含 ABI 入口的判空 / 越界）留在
//! `src/tests.rs` 单测，那里够得着 `Engine`。

use hux_test_support::repo_path;

// 集成测试目标里 `mod x;` 相对 `tests/` 解析，故用显式路径指向本文件同名目录。
#[path = "host_contract/config_page.rs"]
mod config_page;
#[path = "host_contract/fields.rs"]
mod fields;
#[path = "host_contract/model_entry.rs"]
mod model_entry;
#[path = "host_contract/schema.rs"]
mod schema;
#[path = "host_contract/source.rs"]
mod source;
#[path = "host_contract/tray.rs"]
mod tray;
#[path = "host_contract/tri_state.rs"]
mod tri_state;

/// 取 C++ 配置 schema（`shell/hux.cpp`）里 `.path{"<name>"}` 之后的 `.defaultValue` 字面量。
///
/// C++ 侧的默认值不参与 cargo 测试（`hux.cpp` 由 cmake 单独编译），改错了两侧都编译得过；
/// 这里以「解析源码」把它变成可断言的字面量（剥掉行注释；`KeyList` 的默认值跨多行，
/// 按花括号配平补齐）。
fn schema_default(name: &str) -> String {
    let source =
        std::fs::read_to_string(repo_path("platform/fcitx5/shell/hux.cpp")).expect("read hux.cpp");
    let lines: Vec<String> = source
        .lines()
        .map(|line| {
            line.trim()
                .split("//")
                .next()
                .unwrap_or("")
                .trim_end()
                .to_string()
        })
        .collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix(".path{") else {
            continue;
        };
        if rest.trim_end_matches("},").trim_matches('"') != name {
            continue;
        }
        for (offset, next) in lines[index + 1..].iter().enumerate() {
            if next.starts_with(".path{") {
                break;
            }
            let Some(rest) = next.strip_prefix(".defaultValue = ") else {
                continue;
            };
            let mut value = rest.trim_end_matches(',').to_string();
            let mut open = value.matches('{').count() as i32 - value.matches('}').count() as i32;
            let mut cursor = index + offset + 2;
            while open > 0 && cursor < lines.len() {
                let more = lines[cursor].trim_end_matches(',');
                value.push_str(more);
                open += more.matches('{').count() as i32 - more.matches('}').count() as i32;
                cursor += 1;
            }
            return value;
        }
        panic!("schema 项 {name} 没有 defaultValue");
    }
    panic!("schema 缺少项 {name}");
}
