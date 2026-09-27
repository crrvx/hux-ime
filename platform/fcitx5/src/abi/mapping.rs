// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! ABI 取值与换算：fcitx5 按键状态位、`hux_abi.h` 的枚举取值、修饰符掩码映射。
//!
//! 这些名字经 `abi` 重导出，crate 内的可见性与拆分前一致。

use hux_core::key::{
    K_ALT_MASK, K_CONTROL_MASK, K_LOCK_MASK, K_RELEASE_MASK, K_SHIFT_MASK, K_SUPER_MASK,
};

// fcitx5 `KeyState` 位（`fcitx-utils/keysym.h`）。
pub(crate) const FCITX_SHIFT: u32 = 1 << 0;
pub(crate) const FCITX_CAPS_LOCK: u32 = 1 << 1;
pub(crate) const FCITX_CTRL: u32 = 1 << 2;
pub(crate) const FCITX_ALT: u32 = 1 << 3;
pub(crate) const FCITX_SUPER: u32 = 1 << 6;

// `hux_abi.h` 的 `HUX_CANDIDATE_LAYOUT_*` / `HUX_PREEDIT_MODE_*` 取值（ABI）：只给非默认档起名，
// 默认档（跟随全局 / 候选分码）与越界值都是 `hux_engine_apply_settings` 里 `_` 分支的兜底。
// 壳侧同名宏有 `static_assert` 守卫（`shell/hux.cpp`），本侧由用例
// `option_value_enums_match_the_abi_header` 逐项比对头文件里的名字与取值。
pub(crate) const CANDIDATE_LAYOUT_HORIZONTAL: i32 = 1;
pub(crate) const CANDIDATE_LAYOUT_VERTICAL: i32 = 2;
pub(crate) const PREEDIT_MODE_RAW_INPUT: i32 = 1;
pub(crate) const PREEDIT_MODE_HIDDEN: i32 = 2;

/// fcitx5 `KeyState` → core（Rime）掩码。
pub(crate) fn core_modifiers(states: u32, release: bool) -> i32 {
    let mut modifiers = 0;
    if states & FCITX_SHIFT != 0 {
        modifiers |= K_SHIFT_MASK;
    }
    if states & FCITX_CAPS_LOCK != 0 {
        modifiers |= K_LOCK_MASK;
    }
    if states & FCITX_CTRL != 0 {
        modifiers |= K_CONTROL_MASK;
    }
    if states & FCITX_ALT != 0 {
        modifiers |= K_ALT_MASK;
    }
    if states & FCITX_SUPER != 0 {
        modifiers |= K_SUPER_MASK;
    }
    if release {
        modifiers |= K_RELEASE_MASK;
    }
    modifiers
}
