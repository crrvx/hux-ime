// SPDX-FileCopyrightText: 2026 明雅流风 <crrvx@outlook.com>
// SPDX-License-Identifier: GPL-3.0-or-later

//! 设置单测：缺省语义、钳制与角色接线（存储层缺省只含已声明角色）。

use hux_core::host::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};
use hux_core::key::KeyEvent;

use super::*;

/// 缺省值检查表：逐项钉住开箱行为（引擎开关 → 宿主与菜单项 → 运行时开关与派生值）。
const DEFAULT_CHECKS: &[fn(&Settings)] = &[
    engine_switch_defaults,
    host_option_defaults,
    derived_defaults,
];

/// 缺省值是发布语义的一部分，改这里等于改用户的开箱行为，故逐项钉住。
#[test]
fn defaults_match_builtin_semantics() {
    let settings = Settings::default();
    for &check in DEFAULT_CHECKS {
        check(&settings);
    }
}

/// 引擎开关缺省：早提交、单字重码与标点模式。
fn engine_switch_defaults(settings: &Settings) {
    assert!(settings.early_commit, "缺省应开启早提交");
    assert!(
        !settings.early_commit_to_preedit,
        "缺省不得开启「上屏前先进预编辑」"
    );
    assert!(settings.allow_duplicate_single, "缺省应允许单字重码");
    assert!(!settings.full_shape, "缺省不得是中文标点模式");
    assert!(!settings.ascii_punct, "缺省不得是英文标点模式");
    assert!(settings.learning_on_tab, "缺省应开启 Tab 学习");
}

/// 宿主与菜单项缺省：高频上限、页大小、翻页键与反查键。
fn host_option_defaults(settings: &Settings) {
    assert_eq!(
        settings.high_freq_limit, DEFAULT_HIGH_FREQ_LIMIT,
        "高频字上限缺省应为 DEFAULT_HIGH_FREQ_LIMIT"
    );
    assert_eq!(
        settings.page_size, DEFAULT_PAGE_SIZE,
        "页大小缺省应为 DEFAULT_PAGE_SIZE"
    );
    assert_eq!(
        settings.page_up_keys,
        vec!["minus".to_string(), "bracketleft".to_string()],
        "翻页上键缺省应为 minus 与 bracketleft"
    );
    assert_eq!(
        settings.page_down_keys,
        vec!["equal".to_string(), "bracketright".to_string()],
        "翻页下键缺省应为 equal 与 bracketright"
    );
    assert_eq!(
        settings.reverse_lookup_pronunciation_keys,
        vec!["grave".to_string()],
        "音反查键缺省应为 grave"
    );
    assert_eq!(
        settings.reverse_lookup_character_keys,
        vec!["asciitilde".to_string()],
        "字反查键缺省应为 asciitilde"
    );
}

/// 运行时开关与派生值缺省：数字选字、全字集、非汉字过滤、布局、预编辑、翻页与留存长度。
fn derived_defaults(settings: &Settings) {
    assert!(settings.digit_select, "缺省应开启数字选字");
    assert!(settings.full_charset, "缺省应开启全字集");
    assert!(settings.filter_non_han, "缺省应过滤非汉字");
    assert_eq!(
        settings.candidate_layout,
        CandidateLayout::FollowGlobal,
        "候选布局缺省跟随全局"
    );
    assert_eq!(
        settings.preedit_mode,
        PreeditMode::CandidateCode,
        "预编辑缺省显示候选编码"
    );
    assert!(!settings.page_cycle, "缺省不得开启翻页循环");
    assert_eq!(
        settings.min_retained_input_length, 0,
        "留存输入长度缺省为 0"
    );
    assert_eq!(
        settings.min_retained(),
        0,
        "缺省设置下 min_retained() 应给 0"
    );
}

/// 超限配置必须夹到上限而不是原样透传，避免宿主拿到无法兑现的留存长度。
#[test]
fn min_retained_clamps_upper_bound() {
    let settings = Settings {
        min_retained_input_length: 999,
        ..Default::default()
    };
    assert_eq!(
        settings.min_retained(),
        MAX_MIN_RETAINED_INPUT_LENGTH,
        "超上限的留存长度必须夹到 MAX_MIN_RETAINED_INPUT_LENGTH"
    );
}

/// 页大小的 0 与超大值都必须在交给宿主前落进合法区间。
#[test]
fn host_options_clamp_page_size() {
    let low = Settings {
        page_size: 0,
        ..Default::default()
    }
    .host_options();
    assert_eq!(low.page_size, 1, "页大小下限为 1");
    let high = Settings {
        page_size: 999,
        ..Default::default()
    }
    .host_options();
    assert_eq!(high.page_size, MAX_PAGE_SIZE, "页大小上限为 10");
}

/// 配置里的键名是字符串，交给宿主前必须解析成键码；解析失败的项只丢自己，不牵连其余绑定。
#[test]
fn host_options_parse_keys_ignores_invalid() {
    let options = Settings {
        page_up_keys: vec!["comma".to_string(), "not-a-key".to_string()],
        page_down_keys: Vec::new(),
        ..Default::default()
    }
    .host_options();
    assert_eq!(
        options.page_up_keys,
        vec![KeyEvent::from_repr("comma").unwrap()],
        "非法键名必须丢弃，只留可解析的 comma"
    );
    // 契约：**显式给出即以此为准**（空列表 = 不绑定）。生产路径经配置袋把
    // 原始字符串交给方案（`hux-scheme/tiger` 的 `host_options_from`），
    // 两侧语义必须一致 —— 方案侧由 `empty_page_key_lists_unbind_the_keys` 钉住。
    assert!(options.page_down_keys.is_empty(), "空列表 = 不绑定翻页键");
}

/// 缺省下发顺序是方案与宿主约定的写入序，顺序变化会让宿主状态栏与配置页错位。
#[test]
fn session_option_defaults_follow_settings() {
    let settings = Settings {
        full_shape: true,
        ..Default::default()
    };
    let keys = crate::options::test_option_keys();
    let defaults = settings.session_option_defaults(&keys);
    assert!(
        defaults.contains(&("full_shape", true)),
        "full_shape 开启应作为会话选项缺省下发"
    );
    assert!(
        defaults.contains(&(keys.key(ROLE_EARLY_COMMIT).unwrap(), true)),
        "方案角色键经 OptionKeys 解析后同样要下发 true"
    );
    // 顺序保持既有写入顺序（方案开关 → 宿主标准项 → 运行时开关按角色序）。
    assert_eq!(
        defaults.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        vec![
            "tiger_sentence_early_commit",
            "tiger_sentence_early_commit_to_preedit",
            "tiger_sentence_allow_duplicate_single",
            "full_shape",
            "ascii_punct",
            "tiger_sentence_digit_select",
            "tiger_sentence_full_charset",
            "tiger_sentence_filter_non_han",
        ],
        "下发顺序固定为方案开关、宿主标准项、运行时开关按角色序"
    );
}

/// options.yaml 的缺省集合必须覆盖全部核心开关，漏写会让配置页缺项。
#[test]
fn store_defaults_cover_core_switches() {
    let keys = crate::options::test_option_keys();
    let store_defaults = Settings::default().store_defaults(&keys);
    let key = |role| keys.key(role).expect("测试表应完整");
    assert_eq!(
        store_defaults.get(key(ROLE_EARLY_COMMIT_TO_PREEDIT)),
        Some(&false),
        "早提交到预编辑缺省 false 必须落进 store 缺省"
    );
    assert_eq!(
        store_defaults.get("full_shape"),
        Some(&false),
        "宿主标准键 full_shape 的缺省必须落进 store 缺省"
    );
    assert_eq!(
        store_defaults.get(key(ROLE_DIGIT_SELECT)),
        Some(&true),
        "数字选字缺省 true 必须落进 store 缺省"
    );
    // 字集开关同样经 `apply_settings` 写回 `options.yaml`（缺省开）。
    assert_eq!(
        store_defaults.get(key(ROLE_FULL_CHARSET)),
        Some(&true),
        "全字集缺省 true 必须落进 store 缺省"
    );
    assert_eq!(
        store_defaults.get(key(ROLE_FILTER_NON_HAN)),
        Some(&true),
        "过滤非汉字缺省 true 必须落进 store 缺省"
    );
    assert_eq!(store_defaults.len(), 7, "store 缺省应恰好覆盖 7 个开关");
}

/// 方案没声明的角色不接线：不得回落成角色名字面量，否则会与真实方案键混淆。
#[test]
fn option_keys_absent_roles_are_skipped_not_faked() {
    // 方案未声明的角色**不接线**（不回落成角色名字面量，以免与方案键混淆）。
    let empty = OptionKeys::default();
    let defaults = Settings::default().session_option_defaults(&empty);
    assert_eq!(
        defaults.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        vec!["full_shape", "ascii_punct"],
        "仅宿主标准项保留"
    );
    assert_eq!(
        Settings::default().store_defaults(&empty).len(),
        1,
        "无方案声明时 store 缺省只剩宿主标准项 1 条"
    );
}
