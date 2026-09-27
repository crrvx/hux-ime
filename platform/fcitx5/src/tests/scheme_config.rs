// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 方案配置装配：角色声明全覆盖、角色序与取值枚举同 `hux_abi.h` 对账、选项键来源。
//!
//! 头文件解析助手（`abi_enum_members`）随本主题就近放置；`serial()` 约定见父模块 `tests.rs`。

use super::*;

// 主题分组：角色表（`roles`）、头文件对账（`abi_enums`）、选项键来源（`option_keys`）；
// 头文件解析助手 `abi_enum_members` 留在本文件，子模块经 `use super::*;` 取用。
mod abi_enums;
mod option_keys;
mod roles;

/// `hux_abi.h` 的 `HUX_OPTION_*` 枚举序 ↔ `hux_cfg::roles::RUNTIME_OPTION_ROLES`（顺序 / 个数 / 名字）。
///
/// 角色序在三处手工同步（角色表、头文件枚举、C++ 文案表 `kLabels[role]`）：
/// **调序**会让菜单文案与开关静默错位、`HUX_OPTION_DIGIT_SELECT` 取到别的选项键。
/// C++ 侧只能守长度（`static_assert(std::size(kLabels) == HUX_OPTION_COUNT)`，见 `shell/hux.cpp`），
/// 顺序由本用例从**头文件源码**解析后逐项比对——改名 / 加角色 / 调序都在此失败。
/// 从 `hux_abi.h` 源码解析某个具名枚举（`NAME = n, …`，含末尾计数哨兵）。
///
/// 头文件里有多个 `enum { … };`（取值枚举、角色枚举），故按**成员前缀**挑出目标枚举。
fn abi_enum_members(prefix: &str) -> Vec<(String, i32)> {
    let header = std::fs::read_to_string(hux_test_support::repo_path(
        "crates/hux-ffi/include/hux_abi.h",
    ))
    .expect("read hux_abi.h");
    for body in header.split("enum {").skip(1) {
        let body = body.split_once("};").expect("枚举结束").0;
        let members: Vec<(String, i32)> = body
            .lines()
            .filter_map(|line| {
                let (name, value) = line.trim().split_once('=')?;
                let name = name.trim();
                if !name.starts_with(&format!("{prefix}_")) {
                    return None;
                }
                Some((
                    name.to_string(),
                    value
                        .trim()
                        .trim_end_matches(',')
                        .parse::<i32>()
                        .expect("枚举下标应为整数"),
                ))
            })
            .collect();
        if !members.is_empty() {
            return members;
        }
    }
    panic!("hux_abi.h 里没有 {prefix}_* 枚举");
}
