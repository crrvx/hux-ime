// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! hux-ime（虎虚）C ABI 的 **C 布局类型与常量**（与 `include/hux_abi.h` 一一对应）。
//!
//! 导出函数（`#[no_mangle]`）在平台适配层（`platform/fcitx5`），因为其需要平台引擎与存储；
//! 本 crate 只定义跨边界的数据契约，供平台与宿主（C++/JNI 等）共用。

use std::ffi::{c_char, c_void};

/// 键位列表上限（与 `include/hux_abi.h` 的 `HUX_MAX_KEYS` 一致）。
pub const HUX_MAX_KEYS: usize = 8;

/// 键位列表（fcitx5 `KeyList` → C ABI；`sym == 0` 的项忽略）。
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct HuxKeyList {
    pub count: i32,
    pub sym: [i32; HUX_MAX_KEYS],
    pub states: [i32; HUX_MAX_KEYS],
}

/// 外部配置（C ABI 布局；与 `include/hux_abi.h` 一致）。
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct HuxOptions {
    pub early_commit: i32,
    pub early_commit_to_preedit: i32,
    pub allow_duplicate_single: i32,
    pub full_shape: i32,
    pub ascii_punct: i32,
    pub tab_learning: i32,
    pub high_freq_limit: i32,
    /// 音反查触发键（fcitx5 keysym + 状态位，可多项；平台转成 rime 键名）。
    pub sound_to_char_shape: HuxKeyList,
    /// 字反查触发键（fcitx5 keysym + 状态位，可多项）。
    pub char_to_sound_shape: HuxKeyList,
    /// 每页候选个数（1..=10）。
    pub page_size: i32,
    /// 上/下翻页键（fcitx5 keysym + 状态位，可多项）。
    pub page_up: HuxKeyList,
    pub page_down: HuxKeyList,
    /// 数字直选（1–9；0=10）。
    pub digit_select: i32,
    /// 候选排列：0 = 跟随全局（默认），1 = 横排，2 = 竖排。
    pub candidate_layout: i32,
    /// 预编辑内容：0 = 候选分码（默认），1 = 原始输入，2 = 不显示。
    pub preedit_mode: i32,
    /// 翻页循环：1 = 开（默认 0 = 关）。
    pub page_cycle: i32,
    /// 提前上屏最短保留码数（0..=20；0 = 不额外限制）。
    pub min_retained_raw_length: i32,
}

/// 宿主回调表（由 C++ 薄壳提供；函数指针可为空，便于测试）。
#[derive(Clone, Copy)]
#[repr(C)]
pub struct HostCallback {
    pub user: *mut c_void,
    pub commit: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    #[allow(clippy::type_complexity)]
    pub update: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            i32,
            *const *const c_char,
            *const *const c_char,
            i32,
            i32,
            *const c_char,
            *const c_char,
        ),
    >,
}

/// `hux_engine_key` 返回值位掩码：已消费（宿主不应再处理该键）。
pub const HUX_KEY_CONSUMED: i32 = 0x1;
/// `hux_engine_key` 返回值位掩码：已提交且未消费——宿主应消费该键并以 `forwardKey`
/// 重发（保证客户端先收到提交、后收到按键；对齐 fcitx5 核心 `KeyEventOrderFix` 修法）。
pub const HUX_KEY_FORWARD_AFTER_COMMIT: i32 = 0x2;

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    /// C 布局守卫：数值与 `include/hux_abi.h` 的字段表一一对应（漂移即失败）。
    ///
    /// 重要：C++ 薄壳（`platform/fcitx5/shell/hux.cpp`）逐字段填充本结构、`abi.rs` 逐字段读取，
    /// 字段顺序/宽度漂移在两侧都能编译通过，故用尺寸 + 偏移钉住。
    /// 只改 `hux_abi.h` 而忘了这里 → 本测试失败；反之亦然。
    #[test]
    fn c_layout_matches_header() {
        // `hux_key_list`：int32 count + 2 × HUX_MAX_KEYS × int32。
        assert_eq!(size_of::<HuxKeyList>(), 4 + 2 * HUX_MAX_KEYS * 4);
        assert_eq!(offset_of!(HuxKeyList, count), 0);
        assert_eq!(offset_of!(HuxKeyList, sym), 4);
        assert_eq!(offset_of!(HuxKeyList, states), 4 + HUX_MAX_KEYS * 4);

        // `hux_options`：11 个标量 int32 + 4 个键位列表（顺序见头文件）。
        let scalar = size_of::<i32>();
        let list = size_of::<HuxKeyList>();
        assert_eq!(size_of::<HuxOptions>(), 13 * scalar + 4 * list);
        assert_eq!(offset_of!(HuxOptions, early_commit), 0);
        assert_eq!(offset_of!(HuxOptions, high_freq_limit), 6 * scalar);
        assert_eq!(offset_of!(HuxOptions, sound_to_char_shape), 7 * scalar);
        assert_eq!(
            offset_of!(HuxOptions, char_to_sound_shape),
            7 * scalar + list
        );
        assert_eq!(offset_of!(HuxOptions, page_size), 7 * scalar + 2 * list);
        assert_eq!(offset_of!(HuxOptions, page_up), 8 * scalar + 2 * list);
        assert_eq!(offset_of!(HuxOptions, digit_select), 8 * scalar + 4 * list);
        assert_eq!(
            offset_of!(HuxOptions, min_retained_raw_length),
            12 * scalar + 4 * list
        );
    }
}
