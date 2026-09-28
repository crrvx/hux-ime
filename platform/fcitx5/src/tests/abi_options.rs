// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 平台层设置 ↔ C ABI 选项结构（[`hux_ffi::HuxOptions`]）的映射守卫。
//!
//! `hux_engine_apply_settings` 是「配置页 → 方案」的唯一搬运点：逐字段手写映射，
//! 漏一项 / 接错项都只是静默失效（界面里改了没反应），故用一张按
//! [`HUX_OPTIONS_FIELDS`] 声明序写成的期望表逐字段核对往返结果。

use super::*;
use crate::abi::{CANDIDATE_LAYOUT_VERTICAL, PREEDIT_MODE_HIDDEN, hux_engine_apply_settings};
use hux_ffi::HUX_OPTIONS_FIELDS;

/// 键名经内核 `repr()` 规范化后的对照值（`Page_Down` 的规范名是 `Next`）。
fn canonical(reprs: &[String]) -> Vec<String> {
    reprs
        .iter()
        .map(|repr| KeyEvent::from_repr(repr).expect("测试键名须可解析").repr())
        .collect()
}

/// 键名 → 键位表（内核 `repr()` 的逆）：平台只搬运 rime 键名，往返须回到同一组键位。
fn keys_of(reprs: &[&str]) -> hux_ffi::HuxKeyList {
    let keys: Vec<(i32, i32)> = reprs
        .iter()
        .map(|repr| {
            let key = KeyEvent::from_repr(repr).expect("测试键名须可解析");
            let mut states = 0;
            let modifiers = key.modifier;
            if modifiers & hux_core::key::K_SHIFT_MASK != 0 {
                states |= FCITX_SHIFT;
            }
            if modifiers & hux_core::key::K_LOCK_MASK != 0 {
                states |= FCITX_CAPS_LOCK;
            }
            if modifiers & hux_core::key::K_CONTROL_MASK != 0 {
                states |= FCITX_CTRL;
            }
            if modifiers & hux_core::key::K_ALT_MASK != 0 {
                states |= FCITX_ALT;
            }
            if modifiers & hux_core::key::K_SUPER_MASK != 0 {
                states |= FCITX_SUPER;
            }
            (key.keycode, states as i32)
        })
        .collect();
    key_list(&keys)
}

/// 桌面端设置：每项都取**非缺省**值（缺省值相同会让「漏搬运」也看起来正确）。
fn sentinel_settings() -> Settings {
    Settings {
        early_commit: false,
        early_commit_to_preedit: false,
        allow_duplicate_single: false,
        full_shape: true,
        ascii_punct: true,
        learning_on_tab: true,
        high_freq_limit: 800,
        reverse_lookup_pronunciation_keys: vec!["semicolon".into(), "apostrophe".into()],
        reverse_lookup_character_keys: vec!["grave".into()],
        page_size: 7,
        page_up_keys: vec!["Page_Up".into()],
        page_down_keys: vec!["Page_Down".into(), "bracketright".into()],
        digit_select: false,
        candidate_layout: CandidateLayout::Vertical,
        preedit_mode: PreeditMode::Hidden,
        page_cycle: true,
        min_retained_input_length: 6,
        full_charset: false,
        filter_non_han: false,
    }
}

/// 同一个设置值的 C ABI 期望表：字段名按 [`HUX_OPTIONS_FIELDS`] 的**声明序**书写
/// （逐项对照，顺序本身即断言的一部分）。
fn expected_options() -> hux_ffi::HuxOptions {
    hux_ffi::HuxOptions {
        early_commit: 0,
        early_commit_to_preedit: 0,
        allow_duplicate_single: 0,
        full_shape: 1,
        ascii_punct: 1,
        learning_on_tab: 1,
        high_freq_limit: 800,
        reverse_lookup_pronunciation: keys_of(&["semicolon", "apostrophe"]),
        reverse_lookup_character: keys_of(&["grave"]),
        page_size: 7,
        page_up: keys_of(&["Page_Up"]),
        page_down: keys_of(&["Page_Down", "bracketright"]),
        digit_select: 0,
        candidate_layout: CANDIDATE_LAYOUT_VERTICAL,
        preedit_mode: PREEDIT_MODE_HIDDEN,
        page_cycle: 1,
        min_retained_input_length: 6,
        full_charset: 0,
        filter_non_han: 0,
    }
}

/// 逐字段核对：`HuxOptions` 的每个字段（按 [`HUX_OPTIONS_FIELDS`] 的顺序）都能往返。
#[test]
fn settings_options_field_by_field() {
    let _guard = serial();
    let dir = temp_user_dir("k2-options");
    let engine = ffi_engine(dir.clone());
    let input = sentinel_settings();
    let options = expected_options();
    let applied = unsafe { hux_engine_apply_settings(engine, &options) };
    assert_eq!(applied, 1, "应用非缺省设置应报告已应用");
    let back = unsafe { &(*engine).settings };

    every_settings_field_round_trips(back, &input);
    options_field_names_match_the_abi_table();
    unsafe { crate::abi::hux_engine_free(engine) };
    std::fs::remove_dir_all(&dir).ok();
}

/// 往返后每个设置字段都回到输入值（顺序 = `HUX_OPTIONS_FIELDS` 的声明序）。
fn every_settings_field_round_trips(back: &Settings, input: &Settings) {
    // 逐字段核对（顺序 = `HUX_OPTIONS_FIELDS` 的声明序）：往返后必须回到输入值。
    // 标量字段逐字相等；四个键表比到「内核 `repr()` 规范形」——键名的别名
    // （如 `Page_Down` → `Next`）在这里归一，不算搬运失败。
    assert_eq!(back.early_commit, input.early_commit, "early_commit");
    assert_eq!(
        back.early_commit_to_preedit, input.early_commit_to_preedit,
        "early_commit_to_preedit"
    );
    assert_eq!(
        back.allow_duplicate_single, input.allow_duplicate_single,
        "allow_duplicate_single"
    );
    assert_eq!(back.full_shape, input.full_shape, "full_shape");
    assert_eq!(back.ascii_punct, input.ascii_punct, "ascii_punct");
    assert_eq!(
        back.learning_on_tab, input.learning_on_tab,
        "learning_on_tab"
    );
    assert_eq!(
        back.high_freq_limit, input.high_freq_limit,
        "high_freq_limit"
    );
    assert_eq!(
        back.reverse_lookup_pronunciation_keys,
        canonical(&input.reverse_lookup_pronunciation_keys),
        "reverse_lookup_pronunciation"
    );
    assert_eq!(
        back.reverse_lookup_character_keys,
        canonical(&input.reverse_lookup_character_keys),
        "reverse_lookup_character"
    );
    assert_eq!(back.page_size, input.page_size, "page_size");
    assert_eq!(back.page_up_keys, canonical(&input.page_up_keys), "page_up");
    assert_eq!(
        back.page_down_keys,
        canonical(&input.page_down_keys),
        "page_down"
    );
    assert_eq!(back.digit_select, input.digit_select, "digit_select");
    assert_eq!(
        back.candidate_layout, input.candidate_layout,
        "candidate_layout"
    );
    assert_eq!(back.preedit_mode, input.preedit_mode, "preedit_mode");
    assert_eq!(back.page_cycle, input.page_cycle, "page_cycle");
    assert_eq!(
        back.min_retained_input_length, input.min_retained_input_length,
        "min_retained_input_length"
    );
    assert_eq!(back.full_charset, input.full_charset, "full_charset");
    assert_eq!(back.filter_non_han, input.filter_non_han, "filter_non_han");
}

/// C ABI 字段名序列与上面逐字段断言的顺序逐项一致。
fn options_field_names_match_the_abi_table() {
    // 表与结构体同源：字段名序列必须与 C ABI 的字段表逐项一致（本用例逐字段断言的顺序即它）。
    let names: Vec<String> = HUX_OPTIONS_FIELDS
        .iter()
        .map(|name| name.to_string())
        .collect();
    assert_eq!(
        names,
        vec![
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
        ],
        "`HUX_OPTIONS_FIELDS` 的顺序即本用例的断言顺序"
    );
}
