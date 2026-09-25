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
    pub learning_on_tab: i32,
    pub high_freq_limit: i32,
    /// 反查（按**读音**入口）触发键（fcitx5 keysym + 状态位，可多项；平台转成 rime 键名）。
    pub reverse_lookup_pronunciation: HuxKeyList,
    /// 反查（按**字符**入口）触发键（fcitx5 keysym + 状态位，可多项）。
    pub reverse_lookup_character: HuxKeyList,
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
    pub min_retained_input_length: i32,
    /// 启用全字集（追加码表）：0 = 关（只装主表），1 = 开（默认）。
    pub full_charset: i32,
    /// 过滤非汉字（追加码表里的部首/笔画/注音/假名等）：0 = 关，1 = 开（默认）。
    pub filter_non_han: i32,
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

    /// `hux_options` 的字段名序列（Rust ↔ `include/hux_abi.h` ↔ C++ 壳三处的唯一对照）。
    ///
    /// 每个名字都在 [`c_layout_matches_header`] 里被 `offset_of!` 逐字段引用 ⇒ 改 Rust 字段名
    /// 即**编译失败**；本表与 [`HUX_OPTIONS_FIELDS`] 的顺序再由 `hux_abi.h` 的解析结果校对。
    const HUX_OPTIONS_FIELDS: &[&str] = &[
        "early_commit",
        "early_commit_to_preedit",
        "allow_duplicate_single",
        "full_shape",
        "ascii_punct",
        "learning_on_tab",
        "high_freq_limit",
        "reverse_lookup_pronunciation",
        "reverse_lookup_character",
        "page_size",
        "page_up",
        "page_down",
        "digit_select",
        "candidate_layout",
        "preedit_mode",
        "page_cycle",
        "min_retained_input_length",
        "full_charset",
        "filter_non_han",
    ];

    /// 头文件 `typedef struct hux_options { … } hux_options;` 的成员名（声明序，去注释）。
    fn header_options_fields() -> Vec<String> {
        let header = include_str!("../include/hux_abi.h");
        let start = header
            .find("typedef struct hux_options {")
            .expect("hux_abi.h 缺少 hux_options 结构体")
            + "typedef struct hux_options {".len();
        let rest = &header[start..];
        let end = rest.find("} hux_options;").expect("hux_options 未闭合");
        let mut fields = Vec::new();
        for line in rest[..end].lines() {
            // 去块注释 / 行注释（头文件里字段上方有说明性注释）。
            let mut text = String::new();
            let mut in_comment = false;
            let bytes: Vec<char> = line.chars().collect();
            let mut index = 0;
            while index < bytes.len() {
                if !in_comment && bytes[index] == '/' && bytes.get(index + 1) == Some(&'*') {
                    in_comment = true;
                    index += 2;
                    continue;
                }
                if in_comment && bytes[index] == '*' && bytes.get(index + 1) == Some(&'/') {
                    in_comment = false;
                    index += 2;
                    continue;
                }
                if !in_comment {
                    text.push(bytes[index]);
                }
                index += 1;
            }
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            // `int32_t name;` / `hux_key_list name;` / `int32_t name[HUX_MAX_KEYS];`
            let decl = text.trim_end_matches(';').trim();
            let name = decl
                .split_whitespace()
                .next_back()
                .expect("字段声明")
                .split('[')
                .next()
                .expect("字段名");
            fields.push(name.to_string());
        }
        fields
    }

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

        // `hux_options`：15 个标量 int32 + 4 个键位列表（顺序见头文件）。
        let scalar = size_of::<i32>();
        let list = size_of::<HuxKeyList>();
        assert_eq!(size_of::<HuxOptions>(), 15 * scalar + 4 * list);
        // **逐字段**（名字 + 偏移，按声明序）：任何改名都让 `offset_of!` 编译失败，
        // 任何同宽换序都让下一条偏移断言失败（此前只有 8 个抽查点）。
        let expected: &[(&str, usize)] = &[
            ("early_commit", 0),
            ("early_commit_to_preedit", scalar),
            ("allow_duplicate_single", 2 * scalar),
            ("full_shape", 3 * scalar),
            ("ascii_punct", 4 * scalar),
            ("learning_on_tab", 5 * scalar),
            ("high_freq_limit", 6 * scalar),
            ("reverse_lookup_pronunciation", 7 * scalar),
            ("reverse_lookup_character", 7 * scalar + list),
            ("page_size", 7 * scalar + 2 * list),
            ("page_up", 8 * scalar + 2 * list),
            ("page_down", 8 * scalar + 3 * list),
            ("digit_select", 8 * scalar + 4 * list),
            ("candidate_layout", 9 * scalar + 4 * list),
            ("preedit_mode", 10 * scalar + 4 * list),
            ("page_cycle", 11 * scalar + 4 * list),
            ("min_retained_input_length", 12 * scalar + 4 * list),
            ("full_charset", 13 * scalar + 4 * list),
            ("filter_non_han", 14 * scalar + 4 * list),
        ];
        let offsets = [
            offset_of!(HuxOptions, early_commit),
            offset_of!(HuxOptions, early_commit_to_preedit),
            offset_of!(HuxOptions, allow_duplicate_single),
            offset_of!(HuxOptions, full_shape),
            offset_of!(HuxOptions, ascii_punct),
            offset_of!(HuxOptions, learning_on_tab),
            offset_of!(HuxOptions, high_freq_limit),
            offset_of!(HuxOptions, reverse_lookup_pronunciation),
            offset_of!(HuxOptions, reverse_lookup_character),
            offset_of!(HuxOptions, page_size),
            offset_of!(HuxOptions, page_up),
            offset_of!(HuxOptions, page_down),
            offset_of!(HuxOptions, digit_select),
            offset_of!(HuxOptions, candidate_layout),
            offset_of!(HuxOptions, preedit_mode),
            offset_of!(HuxOptions, page_cycle),
            offset_of!(HuxOptions, min_retained_input_length),
            offset_of!(HuxOptions, full_charset),
            offset_of!(HuxOptions, filter_non_han),
        ];
        assert_eq!(
            offsets.len(),
            HUX_OPTIONS_FIELDS.len(),
            "本表的字段数与 `HUX_OPTIONS_FIELDS` 不一致"
        );
        for (index, ((name, offset), actual)) in expected.iter().zip(offsets).enumerate() {
            assert_eq!(
                *name, HUX_OPTIONS_FIELDS[index],
                "第 {index} 个字段名与 `HUX_OPTIONS_FIELDS` 不一致"
            );
            assert_eq!(
                *offset, actual,
                "`hux_options.{name}` 的偏移应为 {offset}，实际 {actual}"
            );
        }
    }

    /// `hux_options` 的**字段名与声明序**必须与 Rust 结构体逐项一致。
    ///
    /// 同宽字段换序（如 `early_commit` ↔ `allow_duplicate_single`，都是 `int32_t`）不会改变
    /// 尺寸，只靠 `size_of`/`offset_of` 的自比抓不到；本用例直接解析头文件与
    /// [`HUX_OPTIONS_FIELDS`]（后者每个名字都被 `offset_of!` 引用 ⇒ 与 Rust 字段绑定）比对。
    #[test]
    fn options_field_names_match_header_order() {
        let header = header_options_fields();
        let expected: Vec<String> = HUX_OPTIONS_FIELDS
            .iter()
            .map(|name| name.to_string())
            .collect();
        assert_eq!(
            header, expected,
            "hux_abi.h 的 hux_options 字段（名字 / 顺序）必须与 Rust `HuxOptions` 一致"
        );
    }
}
