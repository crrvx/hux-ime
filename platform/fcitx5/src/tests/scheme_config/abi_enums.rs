// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `hux_abi.h` 对账：角色序与取值枚举（名字与取值）从**头文件源码**逐项比对。

use super::*;

#[test]
fn option_role_order_matches_the_abi_header() {
    let members = abi_enum_members("HUX_OPTION");

    let expected: Vec<String> = hux_cfg::roles::RUNTIME_OPTION_ROLES
        .iter()
        .map(|role| format!("HUX_OPTION_{}", role.to_ascii_uppercase()))
        .collect();
    assert_eq!(
        members.len(),
        expected.len() + 1,
        "枚举 = 角色序 + 计数哨兵（实际：{members:?}）"
    );
    assert_eq!(
        members[..expected.len()]
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        expected.iter().map(String::as_str).collect::<Vec<_>>(),
        "hux_abi.h 的角色序（顺序 / 个数 / 名字）必须等于 RUNTIME_OPTION_ROLES"
    );
    // 下标连续 0..=n：重复 / 跳号会让宿主按角色取到错位的键。
    assert_eq!(
        members.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
        (0..=expected.len() as i32).collect::<Vec<_>>()
    );
    let (sentinel, count) = &members[expected.len()];
    assert_eq!(sentinel, "HUX_OPTION_COUNT");
    assert_eq!(*count as usize, hux_cfg::roles::RUNTIME_OPTION_ROLES.len());
}

/// `hux_abi.h` 的 `HUX_CANDIDATE_LAYOUT_*` / `HUX_PREEDIT_MODE_*` ↔ `crate::abi` 常量（名字与取值）。
///
/// 两组取值是 **ABI**：壳侧 `shell/hux.cpp` 用同名宏填充（`static_assert` 守 C++ 枚举 ↔ 宏），
/// Rust 侧 `abi.rs` 用具名常量读取（`hux_engine_apply_settings`）。头文件 ↔ Rust 常量的名字与
/// 取值由本用例从**头文件源码**逐项比对——改名 / 改值都在此失败。
#[test]
fn option_value_enums_match_the_abi_header() {
    let layout = abi_enum_members("HUX_CANDIDATE_LAYOUT");
    assert_eq!(
        layout,
        vec![
            // 默认档在 Rust 侧不具名：它是 `hux_engine_apply_settings` 里 `_` 分支的兜底。
            ("HUX_CANDIDATE_LAYOUT_FOLLOW_GLOBAL".to_string(), 0),
            (
                "HUX_CANDIDATE_LAYOUT_HORIZONTAL".to_string(),
                crate::abi::CANDIDATE_LAYOUT_HORIZONTAL
            ),
            (
                "HUX_CANDIDATE_LAYOUT_VERTICAL".to_string(),
                crate::abi::CANDIDATE_LAYOUT_VERTICAL
            ),
            ("HUX_CANDIDATE_LAYOUT_COUNT".to_string(), 3),
        ]
    );
    let preedit = abi_enum_members("HUX_PREEDIT_MODE");
    assert_eq!(
        preedit,
        vec![
            ("HUX_PREEDIT_MODE_CANDIDATE_CODE".to_string(), 0),
            (
                "HUX_PREEDIT_MODE_RAW_INPUT".to_string(),
                crate::abi::PREEDIT_MODE_RAW_INPUT
            ),
            (
                "HUX_PREEDIT_MODE_HIDDEN".to_string(),
                crate::abi::PREEDIT_MODE_HIDDEN
            ),
            ("HUX_PREEDIT_MODE_COUNT".to_string(), 3),
        ]
    );
    // 哨兵 = 取值个数（壳侧 static_assert 守同一条不变式）。
    assert_eq!(layout.last().unwrap().1 as usize, layout.len() - 1);
    assert_eq!(preedit.last().unwrap().1 as usize, preedit.len() - 1);
}
